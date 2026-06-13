//! Engine-level error types.
//!
//! `ScraperError` is the unified error returned by the scraping engine —
//! HTTP failures, parsing errors, configuration mistakes, rate-limit issues.
//! Variants are tagged for easy inspection but several are part of the public
//! surface even when not currently triggered, so dead-code is allowed here.

use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)] // Some variants are part of the public API but not yet triggered in all flows
pub enum ScraperError {
    #[error("HTTP request failed for {url}: {source}")]
    HttpError { url: String, source: reqwest::Error },

    #[error("Request timeout for {url} after {timeout_secs}s")]
    Timeout { url: String, timeout_secs: u64 },

    #[error("Response body exceeds maximum size of {max_bytes} bytes for {url}")]
    ResponseTooLarge { url: String, max_bytes: usize },

    #[error("No adapter available for URL: {url}")]
    NoAdapterFound { url: String },

    #[error("Parse error: {message}")]
    ParseError { message: String },

    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    #[error("Configuration file not found: {path}")]
    ConfigNotFound { path: String },

    #[error("Invalid configuration value for '{field}': {reason}")]
    ConfigValueInvalid { field: String, reason: String },

    #[error("Rate limit queue full for domain: {domain}")]
    RateLimitQueueFull { domain: String },

    #[error("Rate limit queue timeout for domain: {domain}")]
    RateLimitTimeout { domain: String },

    #[error("Invalid rate limit delay {value_ms}ms: must be between 100ms and 60000ms")]
    InvalidRateLimitDelay { value_ms: u64 },

    #[error("Invalid header name: {name}")]
    InvalidHeaderName { name: String },

    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Serialization error for field '{field}': {reason}")]
    SerializationError { field: String, reason: String },

    #[error("HTTP {status} for {url}")]
    HttpStatus { url: String, status: u16 },

    #[error("All retry attempts exhausted for {url}: {reason}")]
    RetriesExhausted { url: String, reason: String },

    #[error("No video sources found for episode: {url}")]
    NoVideoSources { url: String },
}

pub type Result<T> = std::result::Result<T, ScraperError>;
