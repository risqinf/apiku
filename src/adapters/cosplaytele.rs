//! Adapter for cosplaytele.com (WordPress + Flatsome theme).
//!
//! Cosplaytele posts have a fairly consistent structure:
//!   - h1.entry-title contains the post title
//!   - The entry-content blockquote lists "Cosplayer", "Character", "Appear In"
//!   - Tags via <a rel="tag">
//!   - Categories via <a href="/category/<slug>/">
//!   - Gallery images: all <img> with `wp-content/uploads/.../*_result.webp`
//!   - Download links: Mediafire, Telegram, Gofile, Mega, Drive
//!   - Photo/video counts often baked into the title

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ContentModel, CosplayPost, DownloadMirror};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

static PHOTO_COUNT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(\d+)\s*(?:photos?|images?|pics?|pictures?)").unwrap());
static VIDEO_COUNT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(\d+)\s*(?:videos?|clips?|movies?)").unwrap());

pub struct CosplayteleAdapter;

impl CosplayteleAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Build a browse URL for Cosplaytele.
    /// `feed` values:
    ///   "home" / "latest" / ""  -> homepage (latest posts)
    ///   "popular"               -> /category/popular-cosplay/ widget feed
    ///   any other category slug -> /category/<slug>/
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        match feed {
            "" | "home" | "latest" | "recent" => {
                if p == 1 {
                    "https://cosplaytele.com/".to_string()
                } else {
                    format!("https://cosplaytele.com/page/{}/", p)
                }
            }
            // For now route every other feed (popular, hot, etc.) to the
            // hot tag — Cosplaytele uses the same template.
            "popular" | "hot" | "trending" => {
                if p == 1 {
                    "https://cosplaytele.com/tag/hot/".to_string()
                } else {
                    format!("https://cosplaytele.com/tag/hot/page/{}/", p)
                }
            }
            slug => {
                if p == 1 {
                    format!("https://cosplaytele.com/category/{}/", slug)
                } else {
                    format!("https://cosplaytele.com/category/{}/page/{}/", slug, p)
                }
            }
        }
    }

    /// Cosplaytele posts have URL of the form https://cosplaytele.com/<slug>/
    /// (excluding category/tag/author etc. archive pages).
    fn is_post_url(url: &str) -> bool {
        if !url.contains("cosplaytele.com") {
            return false;
        }
        // Exclude archive / index pages
        let archive_paths = [
            "/category/",
            "/tag/",
            "/author/",
            "/page/",
            "/wp-admin",
            "/wp-content",
            "/wp-json",
            "/feed",
            "/explore-categories",
            "/best-cosplayer",
            "/24-hours",
            "/3-day",
            "/7-day",
        ];
        for p in &archive_paths {
            if url.contains(p) {
                return false;
            }
        }
        // Must have a slug
        let after = url.split("cosplaytele.com").nth(1).unwrap_or("");
        let slug = after.trim_start_matches('/').trim_end_matches('/');
        !slug.is_empty()
    }

    fn extract_post(url: &str, html: &str) -> CosplayPost {
        let parser = HtmlParser::parse(html);

        let raw_title = parser
            .text("h1.entry-title")
            .or_else(|| parser.attr("meta[property='og:title']", "content"))
            .unwrap_or_default();
        let title = clean_title(&raw_title);

        // Photo / video counts from title (e.g. "23 photos and 1 video")
        let photo_count = PHOTO_COUNT_RE
            .captures(&raw_title)
            .and_then(|c| c[1].parse::<u32>().ok());
        let video_count = VIDEO_COUNT_RE
            .captures(&raw_title)
            .and_then(|c| c[1].parse::<u32>().ok());

        // Cosplayer / Character / Series from the blockquote
        let blockquote_text = parser.text(".entry-content blockquote").unwrap_or_default();

        let cosplayer = extract_field(&blockquote_text, &["cosplayer:", "cosplayer "]);
        let character = extract_field(&blockquote_text, &["character:", "character "]);
        let series = extract_field(
            &blockquote_text,
            &["appear in:", "appear in ", "from:", "series:"],
        );

        // Categories from <a href="/category/...">
        let categories = collect_taxonomy(&parser, "category");
        // Tags via rel="tag" links
        let tags = parser
            .select_all("a[rel='tag']")
            .iter()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let tags = dedup_preserving(&tags);

        // Author byline
        let author = parser
            .text(".meta-author a")
            .or_else(|| parser.text(".byline .meta-author"));

        // Date
        let published_at = parser
            .attr("time.entry-date", "datetime")
            .or_else(|| parser.attr("time.published", "datetime"))
            .or_else(|| parser.attr("meta[property='article:published_time']", "content"));

        // Gallery images: pick from the actual post body, but skip
        // related-post thumbnails which live in `.box.box-blog-post` containers.
        // Walk the entry-content tree and reject any <img> whose ancestors include
        // these related-post markers.
        let mut images: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for el in parser.select_all(".entry-content img, .single-page img") {
            // Skip if the image is inside a related-post box
            if has_ancestor_class(
                &el,
                &[
                    "box-blog-post",
                    "post-item",
                    "related-posts",
                    "comments-area",
                ],
            ) {
                continue;
            }

            let v = el.value();
            let src = v
                .attr("data-src")
                .or_else(|| v.attr("data-lazy-src"))
                .or_else(|| v.attr("src"));
            let raw = match src {
                Some(s) if !s.is_empty() && !s.starts_with("data:") => s,
                _ => continue,
            };
            let resolved = resolve_url(url, raw);
            // Filter: only wp-content/uploads images
            if !resolved.contains("wp-content/uploads") {
                continue;
            }
            // Skip non-gallery images
            if resolved.contains("/avatar")
                || resolved.contains("gravatar")
                || resolved.contains("logo")
                || resolved.contains("/icons/")
                || resolved.contains("blank.gif")
            {
                continue;
            }
            if seen.insert(resolved.clone()) {
                images.push(resolved);
            }
        }

        // Video URLs
        let mut videos: Vec<String> = Vec::new();
        for el in parser
            .select_all(".entry-content video source, .entry-content video, .entry-content iframe")
        {
            if let Some(src) = el.value().attr("src") {
                let resolved = resolve_url(url, src);
                if !videos.contains(&resolved) {
                    videos.push(resolved);
                }
            }
        }

        // Cover image: prefer the first gallery image (most accurate),
        // fall back to og:image / featured-image
        let cover_image = images
            .first()
            .cloned()
            .or_else(|| parser.attr(".wp-post-image", "src"))
            .or_else(|| parser.attr(".post-thumbnail img", "src"))
            .or_else(|| parser.attr("meta[property='og:image']", "content"));

        // Download links
        let download_links = extract_download_links(&parser);

        // Unzip password (if shown in post)
        let unzip_password = extract_unzip_password(html);

        CosplayPost {
            title: if title.is_empty() { None } else { Some(title) },
            cosplayer,
            character,
            series,
            photo_count,
            video_count,
            images,
            videos,
            categories,
            tags,
            author,
            published_at,
            cover_image,
            download_links,
            unzip_password,
            url: url.to_string(),
        }
    }
}

/// Strip site suffix " - Cosplaytele" from titles
fn clean_title(s: &str) -> String {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_suffix(" - Cosplaytele")
        .or_else(|| trimmed.strip_suffix(" – Cosplaytele"))
        .unwrap_or(trimmed);
    stripped.trim().to_string()
}

/// Walk up the DOM ancestor chain and check if any ancestor has any of the given classes.
fn has_ancestor_class(el: &scraper::ElementRef<'_>, classes: &[&str]) -> bool {
    let mut current = el.parent();
    while let Some(node) = current {
        if let Some(elref) = scraper::ElementRef::wrap(node) {
            if let Some(class_attr) = elref.value().attr("class") {
                for c in classes {
                    if class_attr.split_whitespace().any(|cls| cls == *c) {
                        return true;
                    }
                }
            }
            current = elref.parent();
        } else {
            break;
        }
    }
    false
}

/// Collect a taxonomy (e.g. "category" or "tag") from anchor URLs
fn collect_taxonomy(parser: &HtmlParser, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for el in parser.select_all(&format!("a[href*='/{}/']", name)) {
        let text = el.text().collect::<Vec<_>>().join("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        if seen.insert(text.clone()) {
            out.push(text);
        }
    }
    out
}

/// Extract a field like "Cosplayer: <value>" from a free-form text block
fn extract_field(text: &str, prefixes: &[&str]) -> Option<String> {
    let lower = text.to_lowercase();
    for prefix in prefixes {
        if let Some(start) = lower.find(prefix) {
            // Slice the original (preserving casing) starting after the prefix
            let after = &text[start + prefix.len()..];
            // Field ends at line break or " — " separator or another known label
            let end = after
                .find('\n')
                .or_else(|| after.find("Character:"))
                .or_else(|| after.find("Appear In"))
                .or_else(|| after.find("From:"))
                .or_else(|| after.find("Photos:"))
                .or_else(|| after.find("File Size:"))
                .or_else(|| after.find("Unzip"))
                .unwrap_or(after.len());
            let value = after[..end].trim().to_string();
            if !value.is_empty() && value.len() < 200 {
                return Some(value);
            }
        }
    }
    None
}

/// Deduplicate a Vec<String> preserving insertion order
fn dedup_preserving(items: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for s in items {
        if seen.insert(s.clone()) {
            out.push(s.clone());
        }
    }
    out
}

/// Patterns for known external download host identification
fn host_for_url(u: &str) -> Option<&'static str> {
    let lower = u.to_lowercase();
    if lower.contains("mediafire.com") {
        Some("Mediafire")
    } else if lower.contains("mega.nz") || lower.contains("mega.co.nz") {
        Some("Mega")
    } else if lower.contains("gofile.io") {
        Some("Gofile")
    } else if lower.contains("drive.google.com") {
        Some("Google Drive")
    } else if lower.contains("t.me")
        || lower.contains("telegram.me")
        || lower.contains("telegram.org")
    {
        Some("Telegram")
    } else if lower.contains("krakenfiles") {
        Some("Krakenfiles")
    } else if lower.contains("terabox.com") || lower.contains("1024terabox") {
        Some("Terabox")
    } else if lower.contains("mirrored.to") {
        Some("Mirrored")
    } else if lower.ends_with(".zip") || lower.ends_with(".rar") || lower.ends_with(".7z") {
        Some("Direct")
    } else {
        None
    }
}

/// Extract download links from the post body
fn extract_download_links(parser: &HtmlParser) -> Vec<DownloadMirror> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    // Look at all anchor tags; pick those that point to known download hosts
    for el in parser.select_all(".entry-content a, article a") {
        let v = el.value();
        let href = match v.attr("href") {
            Some(h) => h.to_string(),
            None => continue,
        };

        if let Some(host) = host_for_url(&href) {
            // Use button text as primary mirror name if available, else fallback to host
            let label = el.text().collect::<Vec<_>>().join("").trim().to_string();
            let name = if !label.is_empty() && label.len() < 60 {
                label
            } else {
                host.to_string()
            };

            if seen.insert(href.clone()) {
                links.push(DownloadMirror { name, url: href });
            }
        }
    }
    links
}

/// Try to extract an unzip password if it's shown in the post
fn extract_unzip_password(html: &str) -> Option<String> {
    // Common patterns:
    //   <input ... value="cosplaytele" />
    //   "Unzip Password: cosplaytele"
    static PWD_INPUT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)Unzip\s*Password[\s\S]{0,200}?value="([^"]+)""#).unwrap());
    static PWD_TEXT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)Unzip\s*Password\s*[:：]\s*([^\s<>]{1,40})"#).unwrap());

    if let Some(c) = PWD_INPUT_RE.captures(html) {
        return Some(c[1].to_string());
    }
    if let Some(c) = PWD_TEXT_RE.captures(html) {
        return Some(c[1].to_string());
    }
    None
}

#[async_trait]
impl SiteAdapter for CosplayteleAdapter {
    fn name(&self) -> &str {
        "cosplaytele"
    }

    fn matches(&self, url: &str) -> bool {
        Self::is_post_url(url)
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        let mut h = HashMap::new();
        h.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                .to_string(),
        );
        Some(h)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        Ok(vec![ContentModel::CosplayPost(Self::extract_post(
            url, html,
        ))])
    }
}
