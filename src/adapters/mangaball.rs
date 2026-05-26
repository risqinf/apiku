//! Adapter for mangaball.net.
//!
//! mangaball.net is a SPA where the title detail page renders an empty
//! shell and the chapter list is loaded asynchronously via the API
//! `POST /api/v1/chapter/chapter-listing-by-title-id/` (with CSRF + cookies).
//! Chapter detail pages embed the page image URLs inline as a JS array
//! (`const chapterImages = JSON.parse('[...]')`).
//!
//! This adapter transparently handles both cases:
//!   - For `/title-detail/<slug>-<id>/` URLs, it parses HTML metadata
//!     (OG tags, JSON-LD), extracts the CSRF token, then calls the
//!     chapter listing API.
//!   - For `/chapter-detail/<id>/` URLs, it parses inline JS variables
//!     to extract the image list.

use crate::adapters::{FetchContext, SiteAdapter};
use crate::error::{Result, ScraperError};
use crate::models::{
    ChapterInfo, ChapterTranslation, ContentModel, MangaChapter, MangaSeries, PageImage,
};
use crate::parser::HtmlParser;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static TITLE_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/title-detail/(?:[^/]+-)?([0-9a-f]{20,32})/?").unwrap());
static CSRF_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"<meta\s+name="csrf-token"\s+content="([^"]+)""#).unwrap());

// Inline JS variables embedded in chapter-detail pages
static JS_CHAPTER_NUMBER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"const\s+chapterNumber\s*=\s*[`'"]([^`'"]+)[`'"]"#).unwrap());
static JS_CHAPTER_IMAGES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"const\s+chapterImages\s*=\s*JSON\.parse\s*\(\s*[`'"]([\s\S]+?)[`'"]\s*\)"#)
        .unwrap()
});

pub struct MangaballAdapter;

impl MangaballAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Map a generic feed name to the Mangaball search_type used by their
    /// `/api/v1/title/search/` endpoint.
    ///
    ///   home / featured -> getFeatured
    ///   popular         -> getPopular
    ///   latest          -> getLatestTable
    ///   recommend       -> getRecommend
    pub fn browse_search_type(feed: &str) -> &'static str {
        match feed {
            "" | "home" | "featured" => "getFeatured",
            "popular" => "getPopular",
            "latest" | "update" | "updates" => "getLatestTable",
            "recommend" | "recommended" => "getRecommend",
            _ => "getFeatured",
        }
    }

    pub fn browse_endpoint() -> &'static str {
        "https://mangaball.net/api/v1/title/search/"
    }

    fn is_title_page(url: &str) -> bool {
        url.contains("/title-detail/")
    }

    fn is_chapter_page(url: &str) -> bool {
        url.contains("/chapter-detail/")
    }

    /// Extract title ID from a title-detail URL
    fn extract_title_id(url: &str) -> Option<String> {
        TITLE_ID_RE.captures(url).map(|c| c[1].to_string())
    }

    /// Build a MangaSeries from the title-detail HTML page (without chapters yet)
    fn extract_series_metadata(url: &str, html: &str) -> MangaSeries {
        let parser = HtmlParser::parse(html);

        // Title from OG / JSON-LD breadcrumb / page title
        let raw_title = parser
            .attr("meta[property='og:title']", "content")
            .or_else(|| parser.text("title"))
            .unwrap_or_default();

        let title = clean_title(&raw_title);

        // Description / synopsis — only use if it's distinct from the title
        let raw_desc = parser
            .attr("meta[property='og:description']", "content")
            .or_else(|| parser.attr("meta[name='description']", "content"))
            .map(|s| clean_title(&s));
        let synopsis = match (&raw_desc, &title.as_str()) {
            (Some(d), t) if !d.is_empty() && d != t => Some(d.clone()),
            _ => None,
        };

        // Cover image from OG
        let cover_image = parser.attr("meta[property='og:image']", "content");

        // Genres / tags rendered server-side. Mangaball loads tags via API,
        // so server-side rendering is unreliable. We try multiple patterns
        // and skip clearly-bogus values (sidebar nav, status, etc.).
        let candidate_tags = parser.texts(
            ".tag-item, .genre-item, .chip, .pill.tag, .badge.genre, [data-tag-name], a[href*='/genre/'], a[href*='/tag/']",
        );
        let genres: Vec<String> = candidate_tags
            .into_iter()
            .filter(|t| is_real_genre(t))
            .collect();

        MangaSeries {
            title: if title.is_empty() { None } else { Some(title) },
            author: None,
            artist: None,
            genres,
            synopsis,
            cover_image,
            chapters: Vec::new(),
            url: url.to_string(),
        }
    }

    /// Call the chapter listing API and return parsed chapters
    async fn fetch_chapters_via_api(
        ctx: &FetchContext<'_>,
        page_url: &str,
        title_id: &str,
        csrf: &str,
    ) -> Result<Vec<ChapterInfo>> {
        let api_url = "https://mangaball.net/api/v1/chapter/chapter-listing-by-title-id/";

        // Build headers including CSRF, cookies, X-Requested-With, Referer
        let mut adapter_headers: HashMap<String, String> = HashMap::new();
        adapter_headers.insert("X-CSRF-TOKEN".to_string(), csrf.to_string());
        adapter_headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
        adapter_headers.insert(
            "Accept".to_string(),
            "application/json, text/javascript, */*; q=0.01".to_string(),
        );
        adapter_headers.insert("Referer".to_string(), page_url.to_string());
        if let Some(cookie) = ctx.cookie_header() {
            adapter_headers.insert("Cookie".to_string(), cookie);
        }

        let headers =
            ctx.pipeline
                .build_headers(api_url, ctx.site_config, Some(&adapter_headers))?;

        let form = [("title_id", title_id), ("userSettingsEnabled", "true")];

        let response = ctx
            .client
            .post(api_url)
            .headers(headers)
            .form(&form)
            .send()
            .await
            .map_err(|e| ScraperError::HttpError {
                url: api_url.to_string(),
                source: e,
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(ScraperError::HttpStatus {
                url: api_url.to_string(),
                status: status.as_u16(),
            });
        }

        let body: serde_json::Value =
            response.json().await.map_err(|e| ScraperError::HttpError {
                url: api_url.to_string(),
                source: e,
            })?;

        let chapters_arr = body
            .get("ALL_CHAPTERS")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Deduplicate by chapter number — pick the first translation as primary URL
        // and add the rest as `translations` entries.
        use std::collections::BTreeMap;
        let mut by_number: BTreeMap<u64, ChapterInfo> = BTreeMap::new();

        for ch in chapters_arr {
            let number = ch
                .get("number_float")
                .and_then(|v| v.as_f64())
                .unwrap_or_else(|| {
                    ch.get("number")
                        .and_then(|v| v.as_str())
                        .and_then(|s| {
                            s.trim_start_matches("Ch.")
                                .trim_start_matches("Chapter ")
                                .trim()
                                .parse::<f64>()
                                .ok()
                        })
                        .unwrap_or(0.0)
                });

            // Use a quantized integer key (number * 1000) for stable ordering of fractional chapters
            let key = (number * 1000.0).round() as u64;

            let title = ch
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let translations_arr = ch.get("translations").and_then(|v| v.as_array());
            if let Some(trans) = translations_arr {
                for t in trans {
                    let t_url = t
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.replace("http://", "https://"));
                    let url = match t_url {
                        Some(u) => u,
                        None => continue,
                    };

                    let language = t
                        .get("languageName")
                        .or_else(|| t.get("language"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let group = t
                        .get("group")
                        .and_then(|g| g.get("name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let date = t
                        .get("date")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let pages = t.get("pages").and_then(|v| v.as_u64()).map(|n| n as u32);

                    let translation = ChapterTranslation {
                        url: url.clone(),
                        language,
                        group,
                        date,
                        pages,
                    };

                    by_number
                        .entry(key)
                        .and_modify(|existing| existing.translations.push(translation.clone()))
                        .or_insert_with(|| ChapterInfo {
                            number,
                            title: title.clone(),
                            url: url.clone(),
                            translations: vec![translation.clone()],
                        });
                }
            }
        }

        let chapters: Vec<ChapterInfo> = by_number.into_values().collect();
        Ok(chapters)
    }

    /// Parse a chapter-detail page using inline JS variables
    fn extract_chapter_pages(url: &str, html: &str) -> MangaChapter {
        let chapter_number = JS_CHAPTER_NUMBER_RE
            .captures(html)
            .and_then(|c| c[1].parse::<f64>().ok())
            .unwrap_or(1.0);

        let pages = if let Some(caps) = JS_CHAPTER_IMAGES_RE.captures(html) {
            // The match contains a JSON array as a JS template literal
            let raw = caps[1].to_string();
            match serde_json::from_str::<Vec<String>>(&raw) {
                Ok(urls) => urls
                    .into_iter()
                    .enumerate()
                    .map(|(i, u)| PageImage {
                        index: i + 1,
                        url: u,
                    })
                    .collect(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        // Series title from OG, cleaned of "Ch. N - <chapter title>" portion
        let parser = HtmlParser::parse(html);
        let series_title = parser
            .attr("meta[property='og:title']", "content")
            .map(|t| clean_chapter_series_title(&t));

        MangaChapter {
            series_title,
            chapter_number,
            pages,
            url: url.to_string(),
        }
    }
}

/// Strip site suffix from titles like
///   "Foo Online Free - Foo / Foo Multiple Languages"
///   "Foo - Manga Ball"
fn clean_title(s: &str) -> String {
    let trimmed = s.trim();
    // Strip " - Manga Ball" suffix
    let without_site = trimmed.strip_suffix(" - Manga Ball").unwrap_or(trimmed);
    // Strip " | MangaBall" suffix
    let without_site = without_site
        .strip_suffix(" | MangaBall")
        .unwrap_or(without_site);
    // Take the part before " Online Free" or " - " if present
    let primary = without_site
        .split(" Online Free")
        .next()
        .unwrap_or(without_site)
        .trim();
    primary.to_string()
}

/// Strip chapter portion from chapter page titles like
///   "[H] Onnanoko Ni Nareru Game 3 Ch. 1  - Onnanoko Ni Nareru Game 3"
/// Returns just the series name.
fn clean_chapter_series_title(s: &str) -> String {
    let stripped = clean_title(s);
    // Try to split on "Ch. " or "Chapter " — keep only the part before
    static CH_SPLIT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\s+(?:ch\.|chapter)\s+\d").unwrap());
    if let Some(m) = CH_SPLIT_RE.find(&stripped) {
        return stripped[..m.start()].trim().to_string();
    }
    stripped
}

/// Heuristic to filter out non-genre noise that often appears in tag lists
/// (status indicators, ads, navigation, dates, "+5" overflow markers).
fn is_real_genre(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 60 {
        return false;
    }
    // Reject pure numbers and "+N" overflow
    if t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if t.starts_with('+') && t[1..].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Reject common non-genre tokens
    let lower = t.to_lowercase();
    let blacklist = [
        "off",
        "on",
        "all",
        "beta",
        "new",
        "published",
        "ongoing",
        "completed",
        "hiatus",
        "cancelled",
        "0 chapters",
        "1 chapters",
        "online free",
        "read comics online",
        "best comics",
        "latest comics",
        "free comics",
        "read comics",
    ];
    for bad in &blacklist {
        if lower == *bad || lower.starts_with(&format!("{}:", bad)) {
            return false;
        }
    }
    if lower.starts_with("published:") || lower.starts_with("status:") {
        return false;
    }
    // Reject if it looks like a year (4 digits) or a date
    if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

#[async_trait]
impl SiteAdapter for MangaballAdapter {
    fn name(&self) -> &str {
        "mangaball"
    }

    fn matches(&self, url: &str) -> bool {
        url.contains("mangaball.net")
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        let mut h = HashMap::new();
        h.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string(),
        );
        Some(h)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        // Without context (no follow-up API), best effort: title metadata only
        if Self::is_chapter_page(url) {
            Ok(vec![ContentModel::MangaChapter(
                Self::extract_chapter_pages(url, html),
            )])
        } else if Self::is_title_page(url) {
            Ok(vec![ContentModel::MangaSeries(
                Self::extract_series_metadata(url, html),
            )])
        } else {
            // Homepage or other — fall through to nothing (deep extraction will cover it)
            Ok(vec![])
        }
    }

    async fn extract_with_context(
        &self,
        url: &str,
        html: &str,
        ctx: &FetchContext<'_>,
    ) -> Result<Vec<ContentModel>> {
        if Self::is_chapter_page(url) {
            return Ok(vec![ContentModel::MangaChapter(
                Self::extract_chapter_pages(url, html),
            )]);
        }

        if Self::is_title_page(url) {
            let mut series = Self::extract_series_metadata(url, html);

            // Fetch chapters via API if we can extract a title id and CSRF token
            if let (Some(title_id), Some(csrf)) = (
                Self::extract_title_id(url),
                CSRF_RE.captures(html).map(|c| c[1].to_string()),
            ) {
                match Self::fetch_chapters_via_api(ctx, url, &title_id, &csrf).await {
                    Ok(chapters) => {
                        series.chapters = chapters;
                    }
                    Err(e) => {
                        tracing::warn!("mangaball: failed to load chapters via API: {}", e);
                    }
                }
            }
            return Ok(vec![ContentModel::MangaSeries(series)]);
        }

        // Other URLs: nothing
        Ok(vec![])
    }
}
