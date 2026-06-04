//! Application configuration loaded from a TOML file.
//!
//! `AppConfig` carries everything the engine needs at startup: target URLs
//! (used by the CLI batch mode), output destination, HTTP timeouts, retry
//! policy, default headers, per-site overrides, and per-domain rate-limits.
//!
//! All fields have sensible defaults so the config file is optional. Values
//! are validated via `validate()` and rejected with a descriptive error if
//! out of range.

use crate::error::{Result, ScraperError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Target URLs to scrape
    #[serde(default)]
    pub targets: Vec<String>,

    /// Output file path
    #[serde(default = "default_output_path")]
    pub output_path: String,

    /// Maximum concurrent requests
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Maximum response body size in bytes
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    /// Default rate limit delay in milliseconds
    #[serde(default = "default_rate_limit_ms")]
    pub rate_limit_ms: u64,

    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Retry base delay in milliseconds
    #[serde(default = "default_retry_base_delay_ms")]
    pub retry_base_delay_ms: u64,

    /// JSON indentation spaces
    #[serde(default = "default_indent")]
    pub indent: u8,

    /// Default headers for all requests
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Per-site configuration overrides
    #[serde(default)]
    pub sites: HashMap<String, SiteConfig>,

    /// Per-domain rate limit overrides (domain -> delay_ms)
    #[serde(default)]
    pub rate_limits: HashMap<String, u64>,

    /// Maximum pagination pages to follow
    #[serde(default = "default_max_pages")]
    pub max_pages: usize,

    /// Consumer web app branding / customization (logo, name, ads, etc.)
    #[serde(default)]
    pub web: WebConfig,
}

/// Branding and customization for the consumer web app served at `/`.
///
/// Everything here is read at server start and injected into the SPA shell,
/// so operators can rebrand the site, add search-engine / ad-network
/// verification snippets, and place ads without recompiling the binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Site name shown in the header, drawer, and `<title>`.
    #[serde(default = "default_site_name")]
    pub site_name: String,

    /// Short tagline shown on the home hero banner.
    #[serde(default = "default_tagline")]
    pub tagline: String,

    /// Optional custom logo image URL. When empty a built-in gradient mark
    /// is used. Can be an absolute URL or a path served from `static_dir`
    /// (e.g. `/logo.svg`).
    #[serde(default)]
    pub logo_url: String,

    /// Footer HTML. When empty the footer shows a minimal default. Set to a
    /// single space or your own markup to override; raw HTML is allowed.
    #[serde(default)]
    pub footer_html: String,

    /// Raw HTML injected into `<head>` (meta verification tags, analytics,
    /// ad-network loader scripts, etc.).
    #[serde(default)]
    pub head_html: String,

    /// Raw HTML injected just before `</body>` (deferred ad/analytics
    /// scripts).
    #[serde(default)]
    pub body_html: String,

    /// Directory served at the site root for verification files, `ads.txt`,
    /// `sitemap.xml`, favicons, custom logos, etc. Relative to the working
    /// directory. Missing directory is fine (those paths just 404).
    #[serde(default = "default_static_dir")]
    pub static_dir: String,

    /// Named ad slots (slot key -> raw HTML). The web app renders known slots
    /// at fixed positions: `home`, `browse`, `detail`, `reader`.
    #[serde(default)]
    pub ads: HashMap<String, String>,
}

fn default_site_name() -> String {
    "apiku".to_string()
}

fn default_tagline() -> String {
    "Stream donghua, read comics & novels, browse cosplay galleries - all in one platform.".to_string()
}

fn default_static_dir() -> String {
    "public".to_string()
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            site_name: default_site_name(),
            tagline: default_tagline(),
            logo_url: String::new(),
            footer_html: String::new(),
            head_html: String::new(),
            body_html: String::new(),
            static_dir: default_static_dir(),
            ads: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    /// Custom headers for this site
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Custom referer for this site
    pub referer: Option<String>,

    /// Custom user-agent for this site
    pub user_agent: Option<String>,

    /// Rate limit delay in milliseconds for this site
    pub rate_limit_ms: Option<u64>,
}

fn default_output_path() -> String {
    "output.json".to_string()
}

fn default_concurrency() -> usize {
    5
}

fn default_timeout() -> u64 {
    30
}

fn default_max_body_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

fn default_rate_limit_ms() -> u64 {
    1000
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_base_delay_ms() -> u64 {
    1000
}

fn default_indent() -> u8 {
    2
}

fn default_max_pages() -> usize {
    50
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            output_path: default_output_path(),
            concurrency: default_concurrency(),
            timeout_secs: default_timeout(),
            max_body_size: default_max_body_size(),
            rate_limit_ms: default_rate_limit_ms(),
            max_retries: default_max_retries(),
            retry_base_delay_ms: default_retry_base_delay_ms(),
            indent: default_indent(),
            headers: HashMap::new(),
            sites: HashMap::new(),
            rate_limits: HashMap::new(),
            max_pages: default_max_pages(),
            web: WebConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(ScraperError::ConfigNotFound {
                path: path.display().to_string(),
            });
        }

        let content = std::fs::read_to_string(path).map_err(|e| ScraperError::ConfigError {
            message: format!("Failed to read config file '{}': {}", path.display(), e),
        })?;

        let config: Self = toml::from_str(&content).map_err(|e| ScraperError::ConfigError {
            message: format!("Invalid TOML in '{}': {}", path.display(), e),
        })?;

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.concurrency < 1 || self.concurrency > 100 {
            return Err(ScraperError::ConfigValueInvalid {
                field: "concurrency".to_string(),
                reason: "must be between 1 and 100".to_string(),
            });
        }

        if self.timeout_secs < 1 || self.timeout_secs > 300 {
            return Err(ScraperError::ConfigValueInvalid {
                field: "timeout_secs".to_string(),
                reason: "must be between 1 and 300 seconds".to_string(),
            });
        }

        if self.rate_limit_ms < 100 || self.rate_limit_ms > 60_000 {
            return Err(ScraperError::InvalidRateLimitDelay {
                value_ms: self.rate_limit_ms,
            });
        }

        if self.indent > 8 {
            return Err(ScraperError::ConfigValueInvalid {
                field: "indent".to_string(),
                reason: "must be between 0 and 8".to_string(),
            });
        }

        // Validate per-domain rate limits
        for (domain, delay) in &self.rate_limits {
            if *delay < 100 || *delay > 60_000 {
                return Err(ScraperError::ConfigValueInvalid {
                    field: format!("rate_limits.{}", domain),
                    reason: "delay must be between 100ms and 60000ms".to_string(),
                });
            }
        }

        Ok(())
    }
}
