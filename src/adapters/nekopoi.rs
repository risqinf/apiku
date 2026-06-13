//! Adapter for NekoPoi (nekopoi.care) — adult anime (hentai) releases.
//!
//! Each post is a standalone release page carrying:
//!   - `.nk-post-header h1` title + `.nk-featured-img img` cover
//!   - several `.nk-player-frame > iframe` streaming servers (playmogo /
//!     streampoi / ...), all directly embeddable
//!   - `.nk-download-row` groups: `.nk-download-name` (quality) +
//!     `.nk-download-links a` (named mirrors)
//!   - `.nk-related-*` related-post suggestions
//!
//! Browse listings use `.nk-post-card` cards (thumbnail in a CSS
//! `background-image`); keyword search redirects `?s=` -> `/search/<q>` and
//! renders `.nk-search-item` rows.
//!
//! This is adult content and is gated behind the client's 18+ toggle, like the
//! cosplay / doujin providers.

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{
    ContentModel, DownloadGroup, DownloadMirror, MovieRelated, MovieServer, NekopoiPost,
};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::Selector;
use std::collections::HashMap;

pub const NEKOPOI_BASE: &str = "https://nekopoi.care";

/// `background-image: url('...')` extractor for the thumbnail crops. (No
/// backreference — the `regex` crate doesn't support them; we just stop at the
/// first quote or closing paren.)
static BG_IMAGE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"background-image:\s*url\(['"]?([^'")]+)"#).unwrap());

/// Path segments that are site sections, never a post slug.
const RESERVED_SEGMENTS: &[&str] = &[
    "category",
    "tag",
    "genre",
    "genre-list",
    "hentai-list",
    "page",
    "search",
    "tracking",
    "author",
    "wp-content",
    "wp-admin",
    "wp-json",
    "feed",
    "privacy",
    "dmca",
    "disclaimer",
    "about",
    "contact",
    "request",
];

pub struct NekopoiAdapter;

impl NekopoiAdapter {
    pub fn new() -> Self {
        Self
    }

    fn is_nekopoi_host(url: &str) -> bool {
        url.to_lowercase().contains("nekopoi")
    }

    /// A post/detail page: either a single root-level slug (`/<slug>/`) or a
    /// `/hentai/<slug>/` path. Section pages (`/category/...`, `/page/N`, ...)
    /// are excluded.
    pub fn is_detail_url(url: &str) -> bool {
        if !Self::is_nekopoi_host(url) {
            return false;
        }
        let after = match url.split_once("://") {
            Some((_, rest)) => rest,
            None => url,
        };
        let path = after.split_once('/').map(|(_, p)| p).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or("");
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match segs.as_slice() {
            [slug] => !RESERVED_SEGMENTS.contains(slug),
            ["hentai", _slug] => true,
            _ => false,
        }
    }

    /// Build a browse URL.
    ///   "" / "home" / "latest"     -> newest releases (homepage grid)
    ///   "hentai"                   -> /category/hentai
    ///   "3d"                       -> /category/3d-hentai
    ///   "2d"                       -> /category/2d-animation
    ///   "jav"                      -> /category/jav
    ///   "jav-cosplay"              -> /category/jav-cosplay
    ///   "category:<slug>"          -> /category/<slug>
    ///   "genre:<slug>"             -> /genre/<slug>
    ///   any other slug             -> /category/<slug>
    /// Pagination uses the `/page/N` segment.
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let path: String = match feed {
            "" | "home" | "latest" | "populer" | "popular" => String::new(),
            "hentai" => "category/hentai".into(),
            "3d" | "3d-hentai" => "category/3d-hentai".into(),
            "2d" | "2d-animation" => "category/2d-animation".into(),
            "jav" => "category/jav".into(),
            "jav-cosplay" => "category/jav-cosplay".into(),
            other => {
                if let Some(slug) = other.strip_prefix("category:") {
                    format!("category/{}", slug)
                } else if let Some(slug) = other.strip_prefix("genre:") {
                    format!("genre/{}", slug)
                } else {
                    format!("category/{}", other)
                }
            }
        };
        match (path.is_empty(), p) {
            (true, 1) => format!("{}/", NEKOPOI_BASE),
            (true, n) => format!("{}/page/{}/", NEKOPOI_BASE, n),
            (false, 1) => format!("{}/{}/", NEKOPOI_BASE, path),
            (false, n) => format!("{}/{}/page/{}/", NEKOPOI_BASE, path, n),
        }
    }

    /// Keyword search URL. The site redirects `?s=<q>` to `/search/<q>`.
    pub fn search_url(query: &str, _page: u32) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query.trim(), NON_ALPHANUMERIC).to_string();
        format!("{}/search/{}", NEKOPOI_BASE, q)
    }

    fn bg_image(style: &str, base: &str) -> Option<String> {
        BG_IMAGE_RE
            .captures(style)
            .and_then(|c| c.get(1))
            .map(|m| resolve_url(base, m.as_str().trim()))
    }

    /// Parse a browse listing into unified cards. The homepage uses
    /// `.nk-post-card`; category/tag pages reuse the `.nk-search-item` layout —
    /// we accept both and merge (deduped by URL).
    pub fn parse_listing(base: &str, html: &str) -> Vec<NekopoiCard> {
        let parser = HtmlParser::parse(html);
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let a_sel = Selector::parse("a[href]").ok();
        let thumb_sel = Selector::parse(".nk-thumb-crop, .nk-post-thumb").ok();
        let title_sel = Selector::parse(".nk-post-meta h2 a, h2 a").ok();
        for card in parser.select_all(".nk-post-card") {
            let anchor = title_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .or_else(|| a_sel.as_ref().and_then(|s| card.select(s).next()));
            let anchor = match anchor {
                Some(a) => a,
                None => continue,
            };
            let href = anchor.value().attr("href").unwrap_or("");
            if href.is_empty() {
                continue;
            }
            let url = resolve_url(base, href);
            if !Self::is_detail_url(&url) || !seen.insert(url.clone()) {
                continue;
            }
            let title = anchor.text().collect::<String>().trim().to_string();
            if title.is_empty() {
                continue;
            }
            let thumbnail = thumb_sel.as_ref().and_then(|s| {
                card.select(s).find_map(|t| {
                    t.value()
                        .attr("style")
                        .and_then(|st| Self::bg_image(st, base))
                })
            });
            out.push(NekopoiCard {
                title: clean_title(&title),
                url,
                thumbnail,
            });
        }
        // Category / tag pages render with the search-item layout.
        for c in Self::parse_search(base, html) {
            if seen.insert(c.url.clone()) {
                out.push(c);
            }
        }
        out
    }

    /// Parse `/search/<q>` results (`.nk-search-item`) into unified items.
    pub fn parse_search(base: &str, html: &str) -> Vec<NekopoiCard> {
        let parser = HtmlParser::parse(html);
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let thumb_sel = Selector::parse(".nk-search-thumb").ok();
        let title_sel = Selector::parse(".nk-search-info h2, h2").ok();
        for a in parser.select_all("a.nk-search-item") {
            let href = a.value().attr("href").unwrap_or("");
            if href.is_empty() {
                continue;
            }
            let url = resolve_url(base, href);
            if !Self::is_detail_url(&url) || !seen.insert(url.clone()) {
                continue;
            }
            let title = title_sel
                .as_ref()
                .and_then(|s| a.select(s).next())
                .map(|n| n.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let thumbnail = thumb_sel.as_ref().and_then(|s| {
                a.select(s)
                    .next()
                    .and_then(|t| t.value().attr("style"))
                    .and_then(|st| Self::bg_image(st, base))
            });
            out.push(NekopoiCard {
                title,
                url,
                thumbnail,
            });
        }
        out
    }

    fn parse_post(url: &str, html: &str) -> NekopoiPost {
        let parser = HtmlParser::parse(html);

        // og:title is the most reliable across the heterogeneous page variants
        // (video post / series / jav); fall back to a content <h1>, never a
        // section header like "Informasi Anime" / "Daftar Episode".
        let title = parser
            .attr("meta[property='og:title']", "content")
            .map(|t| clean_title(&t))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                parser
                    .text(".nk-post-header h1, .nk-article h1")
                    .map(|t| clean_title(&t))
                    .filter(|s| !s.is_empty())
            });

        // Cover: featured img (video post) or series poster (CSS background) or
        // og:image — whichever the page variant provides.
        let cover = parser
            .attr(".nk-featured-img img", "src")
            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
            .map(|s| resolve_url(url, &s))
            .or_else(|| {
                parser
                    .attr(".nk-series-poster", "style")
                    .and_then(|st| Self::bg_image(&st, url))
            })
            .or_else(|| {
                parser
                    .attr("meta[property='og:image']", "content")
                    .filter(|s| !s.is_empty() && !s.starts_with("data:"))
                    .map(|s| resolve_url(url, &s))
            });

        let synopsis = parser
            .text(".nk-series-synopsis p, .nk-synopsis, .nk-post-desc")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() > 8)
            .or_else(|| {
                parser
                    .attr("meta[property='og:description']", "content")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s.len() > 8)
            });

        let date = parser
            .select_all(".nk-post-header-meta span, .nk-series-meta-list span")
            .iter()
            .map(|s| s.text().collect::<String>().trim().to_string())
            .find(|t| t.contains(',') || t.chars().any(|c| c.is_ascii_digit()));

        // Genres / tags from the inline series-link / meta list.
        let mut genres = Vec::new();
        if let Ok(sel) =
            Selector::parse(".nk-series-link, .nk-series-meta-list a, .nk-genres a, .nk-tags a")
        {
            for a in parser.document().select(&sel) {
                let href = a.value().attr("href").unwrap_or("");
                // Only genre/category taxonomy links, not episode/download links.
                if !(href.contains("/genre") || href.contains("/category") || href.is_empty()) {
                    continue;
                }
                let t = a.text().collect::<String>().trim().to_string();
                if !t.is_empty() && t.len() < 40 && !genres.contains(&t) {
                    genres.push(t);
                }
            }
        }

        // Episode list (series page variant): `.nk-episode-card` links.
        let episodes = parse_episodes(&parser, url);

        // Streaming servers: every `.nk-player-frame > iframe[src]`.
        let mut servers = Vec::new();
        if let Ok(sel) = Selector::parse(".nk-player-frame iframe[src], .nk-stream iframe[src]") {
            for (i, f) in parser.document().select(&sel).enumerate() {
                let src = f.value().attr("src").unwrap_or("").trim();
                if src.is_empty() {
                    continue;
                }
                let embed = normalize_embed(&resolve_url(url, src));
                let host = url::Url::parse(&embed)
                    .ok()
                    .and_then(|u| {
                        u.host_str()
                            .map(|h| h.trim_start_matches("www.").to_string())
                    })
                    .unwrap_or_default();
                let label = server_label(&host, i);
                servers.push(MovieServer {
                    name: format!("s{}", i + 1),
                    label,
                    embed_url: embed,
                });
            }
        }

        // Download groups: `.nk-download-row` -> quality + named mirrors.
        let mut downloads = Vec::new();
        if let (Ok(row_sel), Ok(name_sel), Ok(link_sel)) = (
            Selector::parse(".nk-download-row"),
            Selector::parse(".nk-download-name"),
            Selector::parse(".nk-download-links a[href]"),
        ) {
            for row in parser.document().select(&row_sel) {
                let quality = row
                    .select(&name_sel)
                    .next()
                    .map(|n| clean_title(&n.text().collect::<String>()))
                    .unwrap_or_default();
                let mirrors: Vec<DownloadMirror> = row
                    .select(&link_sel)
                    .filter_map(|a| {
                        let href = a.value().attr("href").unwrap_or("").trim();
                        let name = a.text().collect::<String>().trim().to_string();
                        if href.is_empty() || name.is_empty() {
                            None
                        } else {
                            Some(DownloadMirror {
                                name,
                                url: href.to_string(),
                            })
                        }
                    })
                    .collect();
                if !mirrors.is_empty() {
                    downloads.push(DownloadGroup {
                        quality: if quality.is_empty() {
                            "Download".to_string()
                        } else {
                            quality
                        },
                        mirrors,
                    });
                }
            }
        }

        // Related posts.
        let related = parse_related(&parser, url);

        NekopoiPost {
            title,
            synopsis,
            cover,
            date,
            genres,
            servers,
            episodes,
            downloads,
            related,
            url: url.to_string(),
        }
    }
}

/// A unified listing/search card (mapped to `SearchResultItem` in the web layer).
#[derive(Debug, Clone)]
pub struct NekopoiCard {
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
}

/// Parse the episode list on a series page (`.nk-episode-card` links).
fn parse_episodes(parser: &HtmlParser, base: &str) -> Vec<MovieRelated> {
    let mut out = Vec::new();
    let card_sel = match Selector::parse(
        ".nk-episode-grid a.nk-episode-card, .nk-episode-grid li a[href], #animelist a[href]",
    ) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let title_sel = Selector::parse(".nk-episode-card-title, .nk-episode-card-info").ok();
    let thumb_sel = Selector::parse(".nk-episode-card-thumb, .ltd").ok();
    let img_sel = Selector::parse("img").ok();
    let mut seen = std::collections::HashSet::new();
    for a in parser.document().select(&card_sel) {
        let href = a.value().attr("href").unwrap_or("");
        if href.is_empty() {
            continue;
        }
        let url = resolve_url(base, href);
        if !NekopoiAdapter::is_detail_url(&url) || !seen.insert(url.clone()) {
            continue;
        }
        let title = title_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .map(|n| clean_title(&n.text().collect::<String>()))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let t = clean_title(&a.text().collect::<String>());
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let poster = thumb_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .and_then(|t| t.value().attr("style"))
            .and_then(|st| NekopoiAdapter::bg_image(st, base))
            .or_else(|| {
                img_sel
                    .as_ref()
                    .and_then(|s| a.select(s).next())
                    .and_then(|n| {
                        let v = n.value();
                        v.attr("data-src")
                            .or_else(|| v.attr("src"))
                            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
                            .map(|s| resolve_url(base, s))
                    })
            });
        out.push(MovieRelated {
            title,
            url,
            poster,
            year: None,
        });
    }
    out
}

fn parse_related(parser: &HtmlParser, base: &str) -> Vec<MovieRelated> {
    let mut out = Vec::new();
    let li_sel = match Selector::parse(
        ".nk-related-list--info li, .nk-related-list li, .nk-related li, .nk-related-thumb-frame",
    ) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let a_sel = Selector::parse(".nf h2 a[href], h2 a[href], a[href]").ok();
    let bg_sel = Selector::parse(".ltd, .nk-related-thumb-crop, .img > div").ok();
    let img_sel = Selector::parse("img").ok();
    for li in parser.document().select(&li_sel) {
        let anchor = match a_sel.as_ref().and_then(|s| li.select(s).next()) {
            Some(a) => a,
            None => continue,
        };
        let href = anchor.value().attr("href").unwrap_or("");
        if href.is_empty() {
            continue;
        }
        let url = resolve_url(base, href);
        if !NekopoiAdapter::is_detail_url(&url) || out.iter().any(|r: &MovieRelated| r.url == url) {
            continue;
        }
        let title = {
            let t = anchor.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                clean_title(&t)
            } else {
                anchor
                    .value()
                    .attr("title")
                    .map(clean_title)
                    .unwrap_or_default()
            }
        };
        if title.is_empty() {
            continue;
        }
        let poster = bg_sel
            .as_ref()
            .and_then(|s| li.select(s).next())
            .and_then(|t| t.value().attr("style"))
            .and_then(|st| NekopoiAdapter::bg_image(st, base))
            .or_else(|| {
                img_sel
                    .as_ref()
                    .and_then(|s| li.select(s).next())
                    .and_then(|n| {
                        let v = n.value();
                        v.attr("data-src")
                            .or_else(|| v.attr("src"))
                            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
                            .map(|s| resolve_url(base, s))
                    })
            });
        out.push(MovieRelated {
            title,
            url,
            poster,
            year: None,
        });
        if out.len() >= 12 {
            break;
        }
    }
    out
}

fn server_label(host: &str, i: usize) -> String {
    let base = host.split('.').next().unwrap_or(host);
    if base.is_empty() {
        format!("Server {}", i + 1)
    } else {
        let mut c = base.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => base.to_string(),
        }
    }
}

fn clean_title(s: &str) -> String {
    let t = s.replace('\u{00a0}', " ");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_end_matches(" - NekoPoi")
        .trim_end_matches(" \u{2013} NekoPoi")
        .trim()
        .to_string()
}

fn normalize_embed(src: &str) -> String {
    if let Some(rest) = src.strip_prefix("//") {
        format!("https://{}", rest)
    } else {
        src.to_string()
    }
}

#[async_trait]
impl SiteAdapter for NekopoiAdapter {
    fn name(&self) -> &str {
        "nekopoi"
    }

    fn matches(&self, url: &str) -> bool {
        Self::is_detail_url(url)
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
        Ok(vec![ContentModel::NekopoiPost(Self::parse_post(url, html))])
    }
}
