use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ContentModel, MediaItem, WordPressPost};
use crate::parser::HtmlParser;
use async_trait::async_trait;

/// WordPress site adapter for extracting posts and media
pub struct WordPressAdapter {
    /// Patterns that indicate a WordPress site
    wp_indicators: Vec<&'static str>,
}

impl WordPressAdapter {
    pub fn new() -> Self {
        Self {
            wp_indicators: vec!["wp-content", "wp-includes", "wordpress", "wp-json", "/wp/"],
        }
    }

    fn extract_post(&self, url: &str, html: &str) -> WordPressPost {
        let parser = HtmlParser::parse(html);

        // Extract title - try multiple common WordPress selectors
        let title = parser
            .text("h1.entry-title")
            .or_else(|| parser.text("h1.post-title"))
            .or_else(|| parser.text(".entry-header h1"))
            .or_else(|| parser.text("article h1"))
            .or_else(|| parser.text("h1"));

        // Extract content body
        let content = parser
            .inner_html(".entry-content")
            .or_else(|| parser.inner_html(".post-content"))
            .or_else(|| parser.inner_html("article .content"))
            .or_else(|| parser.inner_html(".article-content"));

        // Extract author
        let author = parser
            .text(".author-name")
            .or_else(|| parser.text(".entry-author"))
            .or_else(|| parser.text(".post-author a"))
            .or_else(|| parser.text("a[rel='author']"));

        // Extract date (try to find ISO 8601 format in time element)
        let date = parser
            .attr("time.entry-date", "datetime")
            .or_else(|| parser.attr("time.published", "datetime"))
            .or_else(|| parser.attr("time", "datetime"))
            .or_else(|| parser.text(".entry-date"))
            .or_else(|| parser.text(".post-date"));

        // Extract categories
        let categories = parser.texts("a[rel='category tag']");
        let categories = if categories.is_empty() {
            parser.texts(".cat-links a")
        } else {
            categories
        };

        // Extract featured image
        let featured_image = parser
            .attr(".post-thumbnail img", "src")
            .or_else(|| parser.attr(".wp-post-image", "src"))
            .or_else(|| parser.attr("article img.featured", "src"));

        // Extract embedded media
        let media = self.extract_media(&parser);

        WordPressPost {
            title,
            content,
            author,
            date,
            categories,
            featured_image,
            media,
            url: url.to_string(),
        }
    }

    fn extract_media(&self, parser: &HtmlParser) -> Vec<MediaItem> {
        let mut media = Vec::new();

        // Extract images from content
        for url in parser.attrs(".entry-content img", "src") {
            media.push(MediaItem {
                url,
                mime_type: Some("image/jpeg".to_string()),
            });
        }

        // Extract video sources
        for url in parser.attrs("video source", "src") {
            let mime = parser
                .attr(&format!("source[src='{}']", url), "type")
                .unwrap_or_else(|| "video/mp4".to_string());
            media.push(MediaItem {
                url,
                mime_type: Some(mime),
            });
        }

        // Extract audio sources
        for url in parser.attrs("audio source", "src") {
            let mime = parser
                .attr(&format!("source[src='{}']", url), "type")
                .unwrap_or_else(|| "audio/mpeg".to_string());
            media.push(MediaItem {
                url,
                mime_type: Some(mime),
            });
        }

        // Extract iframe embeds (YouTube, etc.)
        for url in parser.attrs(".entry-content iframe", "src") {
            media.push(MediaItem {
                url,
                mime_type: Some("text/html".to_string()),
            });
        }

        media
    }
}

#[async_trait]
impl SiteAdapter for WordPressAdapter {
    fn name(&self) -> &str {
        "wordpress"
    }

    fn matches(&self, url: &str) -> bool {
        // WordPress sites are detected by URL patterns
        let lower = url.to_lowercase();
        self.wp_indicators.iter().any(|ind| lower.contains(ind))
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        let post = self.extract_post(url, html);
        Ok(vec![ContentModel::WordPressPost(post)])
    }
}
