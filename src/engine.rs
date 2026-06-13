//! Core scraping engine.
//!
//! The engine owns the HTTP client, request pipeline, rate limiter, retry
//! handler and adapter registry. It exposes `scrape_all(urls)` (used by both
//! the CLI and the REST API) which fans out URL fetches concurrently
//! according to `AppConfig.concurrency`. After fetch, each adapter takes
//! over to extract typed data; the deep extractor provides a fallback
//! capture of every link / image / OG tag / API endpoint when no adapter
//! matches.

use crate::adapters::{AdapterRegistry, FetchContext};
use crate::config::AppConfig;
use crate::deep_extractor;
use crate::error::{Result, ScraperError};
use crate::models::{ContentModel, DeepPage, JsonApiResponse, ScrapeResult};
use crate::pipeline::RequestPipeline;
use crate::rate_limiter::RateLimiter;
use crate::retry::RetryHandler;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use url::Url;
/// Options that affect how scraping is performed at the engine level
#[derive(Debug, Clone, Default)]
pub struct ScrapeOptions {
    pub follow_api: bool,
    pub max_followed_apis: usize,
    pub deep_only: bool,
    pub no_deep: bool,
    /// Output cleaning preset:
    ///   "none"  = full output (deep + content)
    ///   "clean" = strip deep extraction noise, return content-only with key fields
    ///   "minimal" = like clean but drop empty/null fields
    pub clean: CleanLevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CleanLevel {
    #[default]
    None,
    Clean,
    Minimal,
}

/// Core scraper engine that orchestrates fetching, parsing, and extraction
pub struct ScraperEngine {
    client: Client,
    config: AppConfig,
    pipeline: RequestPipeline,
    rate_limiter: Arc<RateLimiter>,
    retry_handler: RetryHandler,
    adapters: AdapterRegistry,
    options: ScrapeOptions,
}

impl ScraperEngine {
    pub fn with_options(config: AppConfig, options: ScrapeOptions) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .pool_max_idle_per_host(config.concurrency)
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| ScraperError::ConfigError {
                message: format!("Failed to build HTTP client: {}", e),
            })?;

        let pipeline = RequestPipeline::new(&config);
        let rate_limiter = Arc::new(RateLimiter::new(
            config.rate_limit_ms,
            config.rate_limits.clone(),
            300,
        ));
        let retry_handler = RetryHandler::new(config.max_retries, config.retry_base_delay_ms);
        let adapters = AdapterRegistry::with_defaults();

        Ok(Self {
            client,
            config,
            pipeline,
            rate_limiter,
            retry_handler,
            adapters,
            options,
        })
    }

    /// Public access to the underlying HTTP client for ad-hoc requests (e.g. API search)
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Public access to the request pipeline so server code can reuse header building
    pub fn pipeline(&self) -> &RequestPipeline {
        &self.pipeline
    }

    /// Reset the visited URL dedup set (no-op now — kept for API compat)
    #[allow(dead_code)]
    pub async fn clear_visited(&self) {
        // Per-call dedup is now scoped to each `scrape_all` invocation
    }

    /// Fetch a URL (with full pipeline: referer, retry, rate limiting) and return
    /// the raw response body as a string. Used by the search API to fetch HTML
    /// search-result pages.
    pub async fn fetch_html(&self, url: &str) -> Result<String> {
        let domain = extract_domain(url)?;
        let site_config = self.config.sites.get(&domain);
        let headers = self.pipeline.build_headers(url, site_config, None)?;

        let client = self.client.clone();
        let url_owned = url.to_string();

        let response = self
            .retry_handler
            .execute_with_retry(url, &domain, &self.rate_limiter, || {
                let client = client.clone();
                let url = url_owned.clone();
                let headers = headers.clone();
                async move { client.get(&url).headers(headers).send().await }
            })
            .await?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ScraperError::HttpError {
                url: url.to_string(),
                source: e,
            })?;
        if bytes.len() > self.config.max_body_size {
            return Err(ScraperError::ResponseTooLarge {
                url: url.to_string(),
                max_bytes: self.config.max_body_size,
            });
        }
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Scrape all provided URLs concurrently
    pub async fn scrape_all(&self, urls: &[String]) -> Result<Vec<ScrapeResult>> {
        let total = urls.len();
        info!(
            "Starting scrape of {} URL(s) with concurrency {}",
            total, self.config.concurrency
        );

        // Per-call dedup set so concurrent server requests don't interfere
        let visited: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        // Use owned values in the stream so the resulting future is 'static
        // (required by axum / tower handlers).
        let urls_owned: Vec<(usize, String)> = urls.iter().cloned().enumerate().collect();

        let primary: Vec<ScrapeResult> = stream::iter(urls_owned)
            .map(|(idx, url)| {
                let visited = visited.clone();
                async move {
                    let result = self.scrape_one(&url, &visited).await;
                    let completed = idx + 1;
                    let remaining = total - completed;
                    info!(
                        "[{}/{}] Completed: {} | Remaining: {}",
                        completed, total, url, remaining
                    );
                    result
                }
            })
            .buffer_unordered(self.config.concurrency)
            .collect()
            .await;

        if !self.options.follow_api {
            return Ok(self.finalize(primary));
        }

        // Follow-API: collect API endpoints from primary results
        let mut endpoints: Vec<String> = Vec::new();
        let max_follow = if self.options.max_followed_apis == 0 {
            usize::MAX
        } else {
            self.options.max_followed_apis
        };

        for r in &primary {
            if let Some(deep) = &r.deep {
                for ep in deep.api_endpoints.iter().take(max_follow) {
                    endpoints.push(ep.clone());
                }
            }
        }

        let mut seen = HashSet::new();
        endpoints.retain(|e| seen.insert(e.clone()));

        if endpoints.is_empty() {
            return Ok(self.finalize(primary));
        }

        info!(
            "Follow-API: discovered {} unique endpoints, fetching them",
            endpoints.len()
        );

        let secondary: Vec<ScrapeResult> = stream::iter(endpoints)
            .map(|url| {
                let visited = visited.clone();
                async move {
                    let r = self.scrape_one(&url, &visited).await;
                    debug!("Follow-API completed: {}", url);
                    r
                }
            })
            .buffer_unordered(self.config.concurrency)
            .collect()
            .await;

        let mut all = primary;
        all.extend(secondary);
        Ok(self.finalize(all))
    }

    /// Apply post-processing based on clean level
    fn finalize(&self, results: Vec<ScrapeResult>) -> Vec<ScrapeResult> {
        match self.options.clean {
            CleanLevel::None => results,
            CleanLevel::Clean | CleanLevel::Minimal => {
                results
                    .into_iter()
                    .map(|mut r| {
                        // Strip deep page extraction in clean modes (keep content only)
                        r.deep = None;
                        r
                    })
                    .collect()
            }
        }
    }

    async fn scrape_one(&self, url: &str, visited: &Arc<Mutex<HashSet<String>>>) -> ScrapeResult {
        // Dedup per-call
        {
            let mut visited = visited.lock().await;
            if !visited.insert(url.to_string()) {
                debug!("Already visited: {}", url);
                return ScrapeResult {
                    url: url.to_string(),
                    success: false,
                    adapter_used: None,
                    content: None,
                    deep: None,
                    error: Some("Already visited (deduped)".to_string()),
                    elapsed_ms: 0,
                };
            }
        }

        let start = Instant::now();
        match self.fetch_and_extract(url).await {
            Ok(EngineResult {
                adapter_name,
                content,
                deep,
            }) => ScrapeResult {
                url: url.to_string(),
                success: true,
                adapter_used: adapter_name,
                content,
                deep,
                error: None,
                elapsed_ms: start.elapsed().as_millis() as u64,
            },
            Err(e) => {
                warn!("Failed to scrape {}: {}", url, e);
                ScrapeResult {
                    url: url.to_string(),
                    success: false,
                    adapter_used: None,
                    content: None,
                    deep: None,
                    error: Some(e.to_string()),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                }
            }
        }
    }

    async fn fetch_and_extract(&self, url: &str) -> Result<EngineResult> {
        let domain = extract_domain(url)?;
        let site_config = self.config.sites.get(&domain);

        let adapter = if self.options.deep_only {
            None
        } else {
            self.adapters.find_adapter(url)
        };
        let adapter_name = adapter.map(|a| a.name().to_string());
        if let Some(a) = adapter {
            debug!("Using adapter '{}' for {}", a.name(), url);
        } else {
            debug!("No adapter matched for {}; deep extraction only", url);
        }

        let adapter_headers = adapter.and_then(|a| a.headers());
        let headers = self
            .pipeline
            .build_headers(url, site_config, adapter_headers.as_ref())?;

        let client = self.client.clone();
        let url_owned = url.to_string();
        let max_body_size = self.config.max_body_size;

        let response = self
            .retry_handler
            .execute_with_retry(url, &domain, &self.rate_limiter, || {
                let client = client.clone();
                let url = url_owned.clone();
                let headers = headers.clone();
                async move { client.get(&url).headers(headers).send().await }
            })
            .await?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Capture cookies from response for adapters that need them
        let cookies = capture_cookies(&response);

        info!(
            "HTTP {} for {} ({})",
            status.as_u16(),
            url,
            content_type.as_deref().unwrap_or("?")
        );

        if let Some(len) = response.content_length() {
            if len as usize > max_body_size {
                return Err(ScraperError::ResponseTooLarge {
                    url: url.to_string(),
                    max_bytes: max_body_size,
                });
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ScraperError::HttpError {
                url: url.to_string(),
                source: e,
            })?;

        if bytes.len() > max_body_size {
            return Err(ScraperError::ResponseTooLarge {
                url: url.to_string(),
                max_bytes: max_body_size,
            });
        }

        let is_json = content_type
            .as_deref()
            .map(|ct| ct.contains("application/json") || ct.contains("application/ld+json"))
            .unwrap_or(false);

        let body = String::from_utf8_lossy(&bytes).to_string();

        if is_json {
            let data = serde_json::from_str(&body)
                .unwrap_or_else(|_| serde_json::Value::String(body.clone()));
            return Ok(EngineResult {
                adapter_name: Some("json_api".to_string()),
                content: Some(ContentModel::JsonApi(JsonApiResponse {
                    url: url.to_string(),
                    status_code: status.as_u16(),
                    content_type,
                    data,
                })),
                deep: None,
            });
        }

        let deep: Option<DeepPage> = if self.options.no_deep {
            None
        } else {
            Some(deep_extractor::extract_deep(url, &body, status.as_u16()))
        };

        let adapter_content = if let Some(adapter) = adapter {
            // Build a context with cookies + pipeline so adapters can call APIs
            let ctx = FetchContext {
                client: &self.client,
                pipeline: &self.pipeline,
                site_config,
                cookies,
            };
            adapter
                .extract_with_context(url, &body, &ctx)
                .await
                .ok()
                .and_then(|v| v.into_iter().next())
        } else {
            None
        };

        Ok(EngineResult {
            adapter_name,
            content: adapter_content,
            deep,
        })
    }
}

struct EngineResult {
    adapter_name: Option<String>,
    content: Option<ContentModel>,
    deep: Option<DeepPage>,
}

/// Capture cookies from a response's Set-Cookie headers into a name->value map
fn capture_cookies(response: &reqwest::Response) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for value in response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
    {
        if let Ok(s) = value.to_str() {
            // Just take the name=value part before the first ';'
            if let Some(pair) = s.split(';').next() {
                if let Some((name, value)) = pair.split_once('=') {
                    cookies.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }
    cookies
}

fn extract_domain(url: &str) -> Result<String> {
    Url::parse(url)
        .map_err(|_| ScraperError::ParseError {
            message: format!("Invalid URL: {}", url),
        })
        .map(|u| u.host_str().unwrap_or("unknown").to_string())
}
