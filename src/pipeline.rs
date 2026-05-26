//! Outbound request pipeline: composes default + per-site + per-adapter
//! headers, applies referer-spoofing, and hardens against banned headers.
//!
//! Used both by the engine for content scraping and by the API server for
//! the image proxy and search calls.

use crate::config::{AppConfig, SiteConfig};
use crate::error::{Result, ScraperError};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, REFERER, USER_AGENT};
use std::collections::HashMap;
use url::Url;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Request pipeline that handles header injection, referer bypass, and request modification
pub struct RequestPipeline {
    default_headers: HashMap<String, String>,
    default_user_agent: String,
}

impl RequestPipeline {
    pub fn new(config: &AppConfig) -> Self {
        let default_user_agent = config
            .headers
            .get("user-agent")
            .or_else(|| config.headers.get("User-Agent"))
            .cloned()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        Self {
            default_headers: config.headers.clone(),
            default_user_agent,
        }
    }

    /// Build headers for a request to the given URL, merging global defaults with site-specific overrides
    pub fn build_headers(
        &self,
        target_url: &str,
        site_config: Option<&SiteConfig>,
        adapter_headers: Option<&HashMap<String, String>>,
    ) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();

        // Parse target URL for auto-referer
        let parsed_url = Url::parse(target_url).map_err(|_| ScraperError::ParseError {
            message: format!("Invalid URL: {}", target_url),
        })?;

        // Set User-Agent
        let user_agent = site_config
            .and_then(|s| s.user_agent.as_ref())
            .unwrap_or(&self.default_user_agent);

        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent).map_err(|_| ScraperError::InvalidHeaderName {
                name: "User-Agent value".to_string(),
            })?,
        );

        // Set Referer - use site-specific, or auto-generate from origin
        let referer = site_config
            .and_then(|s| s.referer.as_ref())
            .cloned()
            .or_else(|| {
                self.default_headers
                    .get("referer")
                    .or_else(|| self.default_headers.get("Referer"))
                    .cloned()
            })
            .unwrap_or_else(|| {
                // Auto-generate referer from target site's origin
                format!("{}://{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""))
            });

        headers.insert(
            REFERER,
            HeaderValue::from_str(&referer).map_err(|_| ScraperError::InvalidHeaderName {
                name: "Referer value".to_string(),
            })?,
        );

        // Apply global default headers
        let mut total_custom = 0;
        for (name, value) in &self.default_headers {
            let lower = name.to_lowercase();
            if lower == "user-agent" || lower == "referer" {
                continue; // Already handled
            }
            self.insert_header(&mut headers, name, value)?;
            total_custom += 1;
        }

        // Apply site-specific headers (override globals)
        if let Some(site_cfg) = site_config {
            for (name, value) in &site_cfg.headers {
                self.insert_header(&mut headers, name, value)?;
                total_custom += 1;
            }
        }

        // Apply adapter-specific headers (highest priority)
        if let Some(adapter_hdrs) = adapter_headers {
            for (name, value) in adapter_hdrs {
                self.insert_header(&mut headers, name, value)?;
                total_custom += 1;
            }
        }

        if total_custom > 50 {
            return Err(ScraperError::ConfigError {
                message: "Exceeded maximum of 50 custom headers per request".to_string(),
            });
        }

        Ok(headers)
    }

    fn insert_header(&self, headers: &mut HeaderMap, name: &str, value: &str) -> Result<()> {
        if name.is_empty() {
            return Err(ScraperError::InvalidHeaderName {
                name: "(empty)".to_string(),
            });
        }

        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            ScraperError::InvalidHeaderName {
                name: name.to_string(),
            }
        })?;

        let header_value =
            HeaderValue::from_str(value).map_err(|_| ScraperError::InvalidHeaderName {
                name: format!("{} (invalid value)", name),
            })?;

        headers.insert(header_name, header_value);
        Ok(())
    }
}
