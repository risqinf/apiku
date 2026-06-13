//! Thin wrapper around `scraper` for selector-based HTML extraction.
//!
//! Adapters use `HtmlParser` to query the DOM with CSS selectors without
//! having to handle missing-element edge cases throughout. Methods here
//! are infallible and return `None` / empty `Vec` when nothing matches.

use scraper::{ElementRef, Html, Selector};

/// HTML parser wrapper providing CSS selector-based extraction
pub struct HtmlParser {
    document: Html,
}

impl HtmlParser {
    /// Parse an HTML string into a traversable document
    pub fn parse(html: &str) -> Self {
        Self {
            document: Html::parse_document(html),
        }
    }

    /// Borrow the underlying parsed document for advanced nested traversal.
    pub fn document(&self) -> &Html {
        &self.document
    }

    /// Select all elements matching a CSS selector
    pub fn select_all(&self, selector: &str) -> Vec<ElementRef<'_>> {
        match Selector::parse(selector) {
            Ok(sel) => self.document.select(&sel).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Select the first element matching a CSS selector
    pub fn select_one(&self, selector: &str) -> Option<ElementRef<'_>> {
        match Selector::parse(selector) {
            Ok(sel) => self.document.select(&sel).next(),
            Err(_) => None,
        }
    }

    /// Extract text content from the first element matching a selector
    pub fn text(&self, selector: &str) -> Option<String> {
        self.select_one(selector)
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
    }

    /// Extract an attribute value from the first element matching a selector
    pub fn attr(&self, selector: &str, attr_name: &str) -> Option<String> {
        self.select_one(selector)
            .and_then(|el| el.value().attr(attr_name).map(|s| s.to_string()))
    }

    /// Extract inner HTML from the first element matching a selector
    pub fn inner_html(&self, selector: &str) -> Option<String> {
        self.select_one(selector).map(|el| el.inner_html())
    }

    /// Extract all text content from elements matching a selector
    pub fn texts(&self, selector: &str) -> Vec<String> {
        self.select_all(selector)
            .iter()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Extract attribute values from all elements matching a selector
    pub fn attrs(&self, selector: &str, attr_name: &str) -> Vec<String> {
        self.select_all(selector)
            .iter()
            .filter_map(|el| el.value().attr(attr_name).map(|s| s.to_string()))
            .collect()
    }

    /// Extract image URLs handling lazy-load attributes
    pub fn image_urls(&self, selector: &str) -> Vec<String> {
        self.select_all(selector)
            .iter()
            .filter_map(|el| {
                let attrs = el.value();
                // Check lazy-load attributes first
                attrs
                    .attr("data-src")
                    .or_else(|| attrs.attr("data-lazy-src"))
                    .or_else(|| attrs.attr("data-original"))
                    .or_else(|| {
                        // Use src only if it doesn't look like a placeholder
                        let src = attrs.attr("src")?;
                        if is_placeholder(src) {
                            None
                        } else {
                            Some(src)
                        }
                    })
                    .map(|s| s.to_string())
            })
            .filter(|url| !url.is_empty())
            .collect()
    }
}

/// Check if a URL looks like a placeholder/loading image
fn is_placeholder(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("placeholder")
        || lower.contains("loading")
        || lower.contains("data:image")
        || lower.contains("blank.gif")
        || lower.contains("1x1")
        || lower.contains("pixel")
}

/// Resolve a potentially relative URL against a base URL
pub fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }

    if relative.starts_with("//") {
        return format!("https:{}", relative);
    }

    match url::Url::parse(base) {
        Ok(base_url) => match base_url.join(relative) {
            Ok(resolved) => resolved.to_string(),
            Err(_) => relative.to_string(),
        },
        Err(_) => relative.to_string(),
    }
}
