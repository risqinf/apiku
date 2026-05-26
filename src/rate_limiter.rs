//! Per-domain HTTP rate limiter.
//!
//! Enforces a minimum delay between consecutive requests to the same host.
//! Per-domain overrides come from `AppConfig`. The limiter also exposes
//! `pause_domain` so the retry handler can honour 429 `Retry-After` directives.

use crate::error::{Result, ScraperError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

/// Per-domain rate limiter that enforces minimum delays between requests
pub struct RateLimiter {
    /// Per-domain last request timestamps
    domains: Arc<Mutex<HashMap<String, DomainState>>>,
    /// Default delay between requests to the same domain
    default_delay: Duration,
    /// Per-domain delay overrides
    domain_delays: HashMap<String, Duration>,
    /// Maximum queue wait time
    max_wait: Duration,
}

struct DomainState {
    last_request: Instant,
    pending_count: usize,
}

impl RateLimiter {
    pub fn new(
        default_delay_ms: u64,
        domain_delays: HashMap<String, u64>,
        max_wait_secs: u64,
    ) -> Self {
        let domain_delays = domain_delays
            .into_iter()
            .map(|(k, v)| (k, Duration::from_millis(v)))
            .collect();

        Self {
            domains: Arc::new(Mutex::new(HashMap::new())),
            default_delay: Duration::from_millis(default_delay_ms),
            domain_delays,
            max_wait: Duration::from_secs(max_wait_secs),
        }
    }

    /// Wait until it's safe to make a request to the given domain
    pub async fn acquire(&self, domain: &str) -> Result<()> {
        let delay = self
            .domain_delays
            .get(domain)
            .copied()
            .unwrap_or(self.default_delay);

        let start = Instant::now();

        loop {
            let wait_duration = {
                let mut domains = self.domains.lock().await;
                let state = domains.entry(domain.to_string()).or_insert(DomainState {
                    last_request: Instant::now() - delay, // Allow immediate first request
                    pending_count: 0,
                });

                if state.pending_count >= 1000 {
                    return Err(ScraperError::RateLimitQueueFull {
                        domain: domain.to_string(),
                    });
                }

                let elapsed = state.last_request.elapsed();
                if elapsed >= delay {
                    // We can proceed
                    state.last_request = Instant::now();
                    return Ok(());
                }

                state.pending_count += 1;
                delay - elapsed
            };

            // Check if we've exceeded max wait time
            if start.elapsed() + wait_duration > self.max_wait {
                // Decrement pending count
                let mut domains = self.domains.lock().await;
                if let Some(state) = domains.get_mut(domain) {
                    state.pending_count = state.pending_count.saturating_sub(1);
                }
                return Err(ScraperError::RateLimitTimeout {
                    domain: domain.to_string(),
                });
            }

            tokio::time::sleep(wait_duration).await;

            // Try to acquire again
            let mut domains = self.domains.lock().await;
            if let Some(state) = domains.get_mut(domain) {
                state.pending_count = state.pending_count.saturating_sub(1);
                let elapsed = state.last_request.elapsed();
                if elapsed >= delay {
                    state.last_request = Instant::now();
                    return Ok(());
                }
            }
        }
    }

    /// Pause requests to a domain for a specified duration (e.g., from Retry-After header)
    pub async fn pause_domain(&self, domain: &str, duration: Duration) {
        let mut domains = self.domains.lock().await;
        let state = domains.entry(domain.to_string()).or_insert(DomainState {
            last_request: Instant::now(),
            pending_count: 0,
        });
        // Set last_request to now + duration - default_delay so that the next acquire will wait
        state.last_request = Instant::now() + duration - self.default_delay;
    }
}
