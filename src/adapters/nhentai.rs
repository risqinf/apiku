//! Adapter for nhentai.net.
//!
//! nhentai exposes a clean JSON API (`/api/v2/...`) so we use it directly
//! instead of scraping HTML. The adapter recognises three URL kinds:
//!
//!   * gallery detail   - `/g/<id>/`           -> `/api/v2/galleries/<id>`
//!   * search           - `/search/?q=<query>` -> `/api/v2/search?query=...`
//!   * homepage / list  - `/`                  -> `/api/v2/galleries`
//!
//! Multiple alternate domains are supported (`nhentai.net`, `nhentai.xxx`,
//! `nhentai.to`) and normalised to `nhentai.net` when building API URLs.
//!
//! Image URLs use a sharded CDN — `iN.nhentai.net` for full-size pages,
//! `tN.nhentai.net` for thumbnails (where N is 1..=4). The image proxy
//! whitelists all of these.

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ChapterInfo, ContentModel, MangaChapter, MangaSeries, PageImage};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static GALLERY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/g/(\d+)").unwrap());

pub struct NhentaiAdapter;

impl NhentaiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Recognise any nhentai mirror domain.
    pub fn matches_url(url: &str) -> bool {
        let lower = url.to_lowercase();
        lower.contains("nhentai.net")
            || lower.contains("nhentai.xxx")
            || lower.contains("nhentai.to")
    }

    /// Extract the numeric gallery ID from a URL like `/g/123456/`.
    pub fn gallery_id_from_url(url: &str) -> Option<u64> {
        GALLERY_RE
            .captures(url)
            .and_then(|c| c[1].parse::<u64>().ok())
    }

    /// Build the canonical API URL for a gallery ID.
    pub fn api_url_for_gallery(id: u64) -> String {
        format!("https://nhentai.net/api/v2/galleries/{}", id)
    }

    /// Build the canonical API URL for a search query (JSON v2 endpoint).
    /// Note: the JSON endpoint does NOT support `[tag]` syntax in the query.
    /// `sort` may be: `""` / `"popular"` / `"popular-week"` / `"popular-today"` / `"date"`.
    pub fn api_url_for_search(query: &str, page: u32) -> String {
        Self::api_url_for_search_sorted(query, page, "")
    }

    /// Build the search URL with explicit sort.
    pub fn api_url_for_search_sorted(query: &str, page: u32, sort: &str) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        let mut url = format!(
            "https://nhentai.net/api/v2/search?query={}&page={}&per_page=25",
            q,
            page.max(1)
        );
        if !sort.is_empty() {
            url.push_str(&format!("&sort={}", sort));
        }
        url
    }

    /// Build the popular / homepage feed URL.
    /// `sort` may be `""` (recent), `"popular"`, `"popular-week"`, `"popular-today"`.
    pub fn api_url_for_popular(page: u32, sort: &str) -> String {
        let mut url = format!(
            "https://nhentai.net/api/v2/galleries?page={}&per_page=25",
            page.max(1)
        );
        if !sort.is_empty() {
            url.push_str(&format!("&sort={}", sort));
        }
        url
    }

    /// Build the URL for the HTML search page. Unlike the JSON API, this
    /// endpoint supports the user-facing `[tag]` syntax (e.g.
    /// `Genshin Impact [full color]` matches galleries tagged with both).
    /// Sort parameter: "" (recent) or "popular" / "popular-week" / "popular-today".
    #[allow(dead_code)]
    pub fn html_url_for_search(query: &str, page: u32, sort: Option<&str>) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        let mut url = format!("https://nhentai.net/search/?q={}&page={}", q, page.max(1));
        if let Some(s) = sort.filter(|s| !s.is_empty()) {
            url.push_str(&format!("&sort={}", s));
        }
        url
    }

    /// Build the URL for the homepage / popular feed.
    /// Sort parameter: "popular", "popular-week", "popular-today", or "" for recent.
    #[allow(dead_code)]
    pub fn html_url_for_home(page: u32, sort: Option<&str>) -> String {
        let p = page.max(1);
        let mut url = if p > 1 {
            format!("https://nhentai.net/?page={}", p)
        } else {
            "https://nhentai.net/".to_string()
        };
        if let Some(s) = sort.filter(|s| !s.is_empty()) {
            let sep = if url.contains('?') { "&" } else { "?" };
            url.push_str(&format!("{}sort={}", sep, s));
        }
        url
    }

    /// Map a generic feed name to the nhentai-specific sort string.
    /// `home` → recent (default empty), `popular` → all-time popular,
    /// `popular-today` / `popular-week` are passed through.
    pub fn feed_to_sort(feed: &str) -> &'static str {
        match feed {
            "popular" | "popular-all" | "popular-time" => "popular",
            "popular-week" => "popular-week",
            "popular-today" => "popular-today",
            "latest" | "home" | "" | "recent" => "",
            _ => "",
        }
    }

    /// Convert a single-gallery API JSON into our `MangaSeries` model.
    /// Each "page" of the gallery becomes a single chapter (nhentai galleries
    /// are typically short doujinshi without a chapter concept) — we
    /// represent it as a series with one chapter that has all pages.
    pub fn parse_gallery_json(url: &str, json: &serde_json::Value) -> Option<MangaSeries> {
        let id = json.get("id")?.as_u64()?;
        let media_id = json.get("media_id").and_then(|v| v.as_str())?;
        let title = json
            .get("title")
            .and_then(|t| {
                t.get("english")
                    .or_else(|| t.get("pretty"))
                    .or_else(|| t.get("japanese"))
                    .and_then(|s| s.as_str())
            })
            .map(|s| s.to_string());

        let cover = json
            .get("cover")
            .and_then(|c| c.get("path"))
            .and_then(|s| s.as_str())
            .map(|p| build_cdn_url("i", media_id, p));
        // Collect tags into category buckets
        let tags = json.get("tags").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let mut authors = Vec::new();
        let mut groups = Vec::new();
        let mut categories = Vec::new();
        let mut genres = Vec::new();
        let mut languages = Vec::new();
        let mut parodies = Vec::new();

        for t in &tags {
            let kind = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            match kind {
                "artist" => authors.push(name.to_string()),
                "group" => groups.push(name.to_string()),
                "category" => categories.push(name.to_string()),
                "tag" => genres.push(name.to_string()),
                "language" => languages.push(name.to_string()),
                "parody" => parodies.push(name.to_string()),
                _ => {}
            }
        }

        // Build pages from the gallery's `pages` array
        let pages_raw = json.get("pages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let pages: Vec<PageImage> = pages_raw
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let path = p.get("path").and_then(|v| v.as_str())?;
                Some(PageImage {
                    index: i + 1,
                    url: build_cdn_url("i", media_id, path),
                })
            })
            .collect();

        // The gallery is rendered as a single-chapter series
        let chapter_url = format!("https://nhentai.net/g/{}/", id);

        let synopsis = if !categories.is_empty() || !parodies.is_empty() {
            let mut bits = Vec::new();
            if !parodies.is_empty() {
                bits.push(format!("Parodies: {}", parodies.join(", ")));
            }
            if !categories.is_empty() {
                bits.push(format!("Category: {}", categories.join(", ")));
            }
            if !languages.is_empty() {
                bits.push(format!("Language: {}", languages.join(", ")));
            }
            if let Some(n) = json.get("num_pages").and_then(|v| v.as_u64()) {
                bits.push(format!("{} pages", n));
            }
            if let Some(n) = json.get("num_favorites").and_then(|v| v.as_u64()) {
                bits.push(format!("{} favorites", n));
            }
            Some(bits.join(" - "))
        } else {
            None
        };

        let chapters = vec![ChapterInfo {
            number: 1.0,
            title: Some(format!("Pages 1-{}", pages.len())),
            url: chapter_url,
            translations: Vec::new(),
        }];

        Some(MangaSeries {
            title,
            author: if authors.is_empty() {
                None
            } else {
                Some(authors.join(", "))
            },
            artist: if groups.is_empty() {
                None
            } else {
                Some(groups.join(", "))
            },
            genres,
            synopsis,
            cover_image: cover,
            chapters,
            url: url.to_string(),
        })
    }

    /// Parse a gallery JSON as a chapter (URL-direct read flow).
    pub fn parse_gallery_as_chapter(url: &str, json: &serde_json::Value) -> Option<MangaChapter> {
        let id = json.get("id")?.as_u64()?;
        let media_id = json.get("media_id").and_then(|v| v.as_str())?;
        let title = json
            .get("title")
            .and_then(|t| {
                t.get("english")
                    .or_else(|| t.get("pretty"))
                    .or_else(|| t.get("japanese"))
                    .and_then(|s| s.as_str())
            })
            .map(|s| s.to_string());

        let pages_raw = json.get("pages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let pages: Vec<PageImage> = pages_raw
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let path = p.get("path").and_then(|v| v.as_str())?;
                Some(PageImage {
                    index: i + 1,
                    url: build_cdn_url("i", media_id, path),
                })
            })
            .collect();

        Some(MangaChapter {
            series_title: title,
            chapter_number: id as f64,
            pages,
            url: url.to_string(),
        })
    }
}

/// Build a full nhentai CDN URL from a path like `galleries/3957087/1.webp`.
///
///   prefix = "i" -> i1..i4.nhentai.net (full-size pages)
///   prefix = "t" -> t1..t4.nhentai.net (thumbnails)
///
/// The shard (1..4) is derived deterministically from the media ID so the
/// same image always resolves to the same subdomain.
fn build_cdn_url(prefix: &str, media_id: &str, path: &str) -> String {
    // Strip duplicate extensions like ".webp.webp" that occasionally appear
    // in API responses.
    let cleaned_path = strip_duplicate_extension(path);
    let shard = (media_id
        .chars()
        .last()
        .and_then(|c| c.to_digit(10))
        .unwrap_or(1)
        % 4)
        + 1;
    format!("https://{}{}.nhentai.net/{}", prefix, shard, cleaned_path)
}

fn strip_duplicate_extension(p: &str) -> String {
    // ".webp.webp" -> ".webp", ".jpg.webp" stays as-is (those are intentional)
    let lower = p.to_lowercase();
    for ext in [".webp", ".jpg", ".png", ".gif"] {
        let dup = format!("{}{}", ext, ext);
        if lower.ends_with(&dup) {
            return p[..p.len() - ext.len()].to_string();
        }
    }
    p.to_string()
}

#[async_trait]
impl SiteAdapter for NhentaiAdapter {
    fn name(&self) -> &str {
        "nhentai"
    }

    fn matches(&self, url: &str) -> bool {
        Self::matches_url(url)
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        // nhentai.net doesn't strictly require any special header for the
        // public API, but a real Accept-Language hint never hurts.
        let mut h = HashMap::new();
        h.insert(
            "Accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        );
        Some(h)
    }

    async fn extract(&self, url: &str, body: &str) -> Result<Vec<ContentModel>> {
        // The nhentai adapter is API-driven: by the time `extract` is
        // called, the `body` is already JSON from `/api/v2/galleries/<id>`.
        let json: serde_json::Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };

        if let Some(series) = Self::parse_gallery_json(url, &json) {
            // Two output forms: when the URL was `/g/<id>/` we expose it as
            // a series; the API server's read-chapter endpoint also accepts
            // the gallery ID and returns it as a chapter.
            return Ok(vec![ContentModel::MangaSeries(series)]);
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_recognition() {
        assert!(NhentaiAdapter::matches_url("https://nhentai.net/g/123/"));
        assert!(NhentaiAdapter::matches_url("https://nhentai.xxx/g/123/"));
        assert!(NhentaiAdapter::matches_url("https://nhentai.to/g/456/"));
        assert!(!NhentaiAdapter::matches_url("https://example.com/"));
    }

    #[test]
    fn extract_gallery_id() {
        assert_eq!(NhentaiAdapter::gallery_id_from_url("https://nhentai.net/g/123/"), Some(123));
        assert_eq!(NhentaiAdapter::gallery_id_from_url("/g/45678"), Some(45678));
        assert_eq!(NhentaiAdapter::gallery_id_from_url("/no-id-here/"), None);
    }

    #[test]
    fn cdn_url_construction() {
        let u = build_cdn_url("i", "3957087", "galleries/3957087/1.webp");
        assert!(u.starts_with("https://i"));
        assert!(u.ends_with("nhentai.net/galleries/3957087/1.webp"));
        // shard derived from last digit (7 % 4 + 1 = 4)
        assert!(u.contains("i4.nhentai.net"));
    }

    #[test]
    fn duplicate_extensions_stripped() {
        assert_eq!(
            strip_duplicate_extension("galleries/1/cover.webp.webp"),
            "galleries/1/cover.webp"
        );
        assert_eq!(
            strip_duplicate_extension("galleries/1/2t.webp.webp"),
            "galleries/1/2t.webp"
        );
        assert_eq!(
            strip_duplicate_extension("galleries/1/cover.jpg"),
            "galleries/1/cover.jpg"
        );
    }
}
