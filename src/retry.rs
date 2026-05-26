//! Retry policy with exponential backoff and 429-aware pausing.
//!
//! Wraps the engine's HTTP request closures: catches transient failures,
//! sleeps with capped exponential backoff, and propagates upstream
//! `Retry-After` headers to the rate-limiter so all callers cooperate.

use crate::error::{Result, ScraperError};
use crate::rate_limiter::RateLimiter;
use reqwest::StatusCode;
use std::time::Duration;
use tracing::{debug, error, warn};

/// Retry handler with exponential backoff
pub struct RetryHandler {
    max_attempts: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
    default_429_pause_secs: u64,
}

impl RetryHandler {
    pub fn new(max_attempts: u32, base_delay_ms: u64) -> Self {
        Self {
            max_attempts,
            base_delay_ms,
            max_delay_ms: 60_000,
            default_429_pause_secs: 60,
        }
    }

    /// Determine if a request should be retried based on the error/status
    pub fn should_retry(&self, status: Option<StatusCode>, attempt: u32) -> RetryDecision {
        if attempt >= self.max_attempts {
            return RetryDecision::GiveUp;
        }

        match status {
            Some(status) if status == StatusCode::TOO_MANY_REQUESTS => RetryDecision::PauseDomain,
            Some(status) if status.is_server_error() => {
                RetryDecision::RetryAfter(self.calculate_backoff(attempt))
            }
            Some(status) if status.is_client_error() => {
                // 4xx other than 429 - don't retry
                RetryDecision::GiveUp
            }
            None => {
                // Network error - retry
                RetryDecision::RetryAfter(self.calculate_backoff(attempt))
            }
            _ => RetryDecision::GiveUp,
        }
    }

    /// Calculate exponential backoff delay for a given attempt
    fn calculate_backoff(&self, attempt: u32) -> Duration {
        let delay_ms = self.base_delay_ms * 2u64.pow(attempt.saturating_sub(1));
        let capped = delay_ms.min(self.max_delay_ms);
        Duration::from_millis(capped)
    }

    /// Get the pause duration for a 429 response
    pub fn get_429_pause(&self, retry_after_header: Option<&str>) -> Duration {
        if let Some(value) = retry_after_header {
            // Try to parse as seconds
            if let Ok(secs) = value.parse::<u64>() {
                return Duration::from_secs(secs);
            }
        }
        Duration::from_secs(self.default_429_pause_secs)
    }

    /// Execute a request with retry logic
    pub async fn execute_with_retry<F, Fut>(
        &self,
        url: &str,
        domain: &str,
        rate_limiter: &RateLimiter,
        mut request_fn: F,
    ) -> Result<reqwest::Response>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<reqwest::Response, reqwest::Error>>,
    {
        let mut attempt = 0;

        loop {
            attempt += 1;
            debug!(
                "Request attempt {}/{} for {}",
                attempt, self.max_attempts, url
            );

            // Wait for rate limiter
            rate_limiter.acquire(domain).await?;

            match (request_fn)().await {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() || status.is_redirection() {
                        return Ok(response);
                    }

                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok());
                        let pause = self.get_429_pause(retry_after);

                        warn!(
                            "Rate limited (429) for {}, pausing domain '{}' for {}s",
                            url,
                            domain,
                            pause.as_secs()
                        );

                        rate_limiter.pause_domain(domain, pause).await;

                        match self.should_retry(Some(status), attempt) {
                            RetryDecision::GiveUp => {
                                error!("All retry attempts exhausted for {}", url);
                                return Err(ScraperError::RetriesExhausted {
                                    url: url.to_string(),
                                    reason: "429 Too Many Requests".to_string(),
                                });
                            }
                            _ => continue,
                        }
                    }

                    if status.is_server_error() {
                        match self.should_retry(Some(status), attempt) {
                            RetryDecision::RetryAfter(delay) => {
                                warn!(
                                    "Server error {} for {}, retrying in {}ms (attempt {}/{})",
                                    status.as_u16(),
                                    url,
                                    delay.as_millis(),
                                    attempt,
                                    self.max_attempts
                                );
                                tokio::time::sleep(delay).await;
                                continue;
                            }
                            RetryDecision::GiveUp => {
                                error!(
                                    "All retry attempts exhausted for {}: HTTP {}",
                                    url,
                                    status.as_u16()
                                );
                                return Err(ScraperError::RetriesExhausted {
                                    url: url.to_string(),
                                    reason: format!("HTTP {}", status.as_u16()),
                                });
                            }
                            _ => continue,
                        }
                    }

                    // 4xx client error (not 429) - don't retry
                    warn!(
                        "Client error {} for {} - not retrying",
                        status.as_u16(),
                        url
                    );
                    return Err(ScraperError::HttpStatus {
                        url: url.to_string(),
                        status: status.as_u16(),
                    });
                }
                Err(e) => match self.should_retry(None, attempt) {
                    RetryDecision::RetryAfter(delay) => {
                        warn!(
                            "Network error for {}: {}, retrying in {}ms (attempt {}/{})",
                            url,
                            e,
                            delay.as_millis(),
                            attempt,
                            self.max_attempts
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    RetryDecision::GiveUp => {
                        error!("All retry attempts exhausted for {}: {}", url, e);
                        return Err(ScraperError::RetriesExhausted {
                            url: url.to_string(),
                            reason: e.to_string(),
                        });
                    }
                    _ => continue,
                },
            }
        }
    }
}

#[derive(Debug)]
pub enum RetryDecision {
    RetryAfter(Duration),
    PauseDomain,
    GiveUp,
}
