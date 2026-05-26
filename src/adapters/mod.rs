//! Site adapter framework.
//!
//! Every supported provider implements the `SiteAdapter` trait. The engine
//! finds an adapter via `find_adapter(url)` and lets it pull typed content
//! out of the raw HTML. Adapters that need follow-up requests (e.g.
//! Mangaball's chapter-listing API) implement `extract_with_context` and
//! reuse the engine's HTTP client + cookies.

use crate::error::Result;
use crate::models::ContentModel;
use crate::pipeline::RequestPipeline;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;

/// Context passed to site adapters that need to make follow-up HTTP requests
/// (e.g., calling JSON APIs after fetching the initial HTML page).
pub struct FetchContext<'a> {
    pub client: &'a Client,
    pub pipeline: &'a RequestPipeline,
    pub site_config: Option<&'a crate::config::SiteConfig>,
    /// Cookies / state captured from the initial HTML page response
    pub cookies: HashMap<String, String>,
}

impl FetchContext<'_> {
    /// Build a Cookie header value from captured cookies
    pub fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

/// Trait that all site adapters must implement
#[async_trait]
pub trait SiteAdapter: Send + Sync {
    /// Returns the adapter name for logging
    fn name(&self) -> &str;

    /// Check if this adapter can handle the given URL
    fn matches(&self, url: &str) -> bool;

    /// Get adapter-specific headers to merge with the request pipeline
    fn headers(&self) -> Option<HashMap<String, String>> {
        None
    }

    /// Extract content from the given HTML page.
    /// Default implementation does not need network access.
    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>>;

    /// Extract content with the ability to make follow-up HTTP requests.
    /// Adapters that need to call APIs after fetching the HTML override this method.
    /// Default impl just delegates to `extract`.
    async fn extract_with_context(
        &self,
        url: &str,
        html: &str,
        _ctx: &FetchContext<'_>,
    ) -> Result<Vec<ContentModel>> {
        self.extract(url, html).await
    }
}

pub mod anichin;
pub mod cosplaytele;
pub mod donghua;
pub mod manga;
pub mod mangaball;
pub mod nhentai;
pub mod novelid;
pub mod wordpress;

/// Registry of site adapters
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn SiteAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            adapters: Vec::new(),
        }
    }

    /// Create a registry with all built-in adapters
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Order matters: more specific adapters should come first
        registry.register(Box::new(mangaball::MangaballAdapter::new()));
        registry.register(Box::new(nhentai::NhentaiAdapter::new()));
        registry.register(Box::new(novelid::NovelidAdapter::new()));
        registry.register(Box::new(cosplaytele::CosplayteleAdapter::new()));
        registry.register(Box::new(anichin::AnichinAdapter::new()));
        registry.register(Box::new(wordpress::WordPressAdapter::new()));
        registry.register(Box::new(manga::MangaAdapter::new()));
        registry.register(Box::new(donghua::DonghuaAdapter::new()));
        registry
    }

    /// Register a new adapter
    pub fn register(&mut self, adapter: Box<dyn SiteAdapter>) {
        self.adapters.push(adapter);
    }

    /// Find the appropriate adapter for a URL
    pub fn find_adapter(&self, url: &str) -> Option<&dyn SiteAdapter> {
        self.adapters
            .iter()
            .find(|a| a.matches(url))
            .map(|a| a.as_ref())
    }
}
