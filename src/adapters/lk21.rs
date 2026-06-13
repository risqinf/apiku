//! Adapter for LayarKaca21 (lk21) — movie / film streaming.
//!
//! Movie detail pages live at a root-level slug ending in a release year
//! (e.g. `/mortal-kombat-ii-2026`). The page carries:
//!   - `<h1>` "Nonton <title> Sub Indo di Lk21"
//!   - `.info-tag` (rating / quality / resolution / duration)
//!   - `.tag-list .tag a` linking to `/genre/<slug>` and `/country/<slug>`
//!   - `.synopsis` text and a `.detail` block (Sutradara / Bintang Film /
//!     Negara / Release)
//!   - `<iframe id="main-player">` default embed
//!   - a download portal link (`dl.lk21.party/<slug>/`)
//!
//! Browse feeds are static HTML grids of `.poster` cards; keyword search uses
//! a separate JSON API (handled in `web::search`).

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ContentModel, MovieDetail, MovieRelated, MovieServer};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::Selector;
use std::collections::HashMap;

/// Canonical lk21 mirror used to build browse / detail URLs.
pub const LK21_BASE: &str = "https://tv11.lk21official.cc";

static YEAR_SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"-\d{4}(?:-\d+)?/?$").unwrap());
static RESOLUTION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d+p$").unwrap());

pub struct Lk21Adapter;

impl Lk21Adapter {
    pub fn new() -> Self {
        Self
    }

    fn is_lk21_host(url: &str) -> bool {
        let u = url.to_lowercase();
        u.contains("lk21") || u.contains("layarkaca")
    }

    /// A movie/series detail page: a single-segment slug ending in a year.
    pub fn is_detail_url(url: &str) -> bool {
        if !Self::is_lk21_host(url) {
            return false;
        }
        let after = match url.split_once("://") {
            Some((_, rest)) => rest,
            None => url,
        };
        let path = after.split_once('/').map(|(_, p)| p).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or("");
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() || trimmed.contains('/') {
            return false;
        }
        const NON_DETAIL: &[&str] = &[
            "search",
            "populer",
            "latest",
            "rating",
            "release",
            "most-commented",
            "nontondrama",
            "latest-series",
            "top-series-today",
            "rekomendasi-film-pintar",
        ];
        if NON_DETAIL.contains(&trimmed) {
            return false;
        }
        YEAR_SLUG_RE.is_match(url)
    }

    /// Build a browse URL.
    /// `feed` values:
    ///   "" / "home" / "populer"    -> most popular
    ///   "latest"                   -> newest uploads
    ///   "rating"                   -> highest rated
    ///   "release"                  -> by year
    ///   "series" / "nontondrama"   -> series list
    ///   "latest-series"            -> updated series
    ///   "genre:<slug>"             -> a genre
    ///   "country:<slug>"           -> a country
    ///   "year:<n>"                 -> a year
    ///   any other slug             -> treated as a genre
    /// Pagination uses the `/page/N` segment.
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let path: String = match feed {
            "" | "home" | "populer" | "popular" => "populer".into(),
            "latest" => "latest".into(),
            "rating" => "rating".into(),
            "release" => "release".into(),
            "series" | "nontondrama" => "nontondrama".into(),
            "latest-series" => "latest-series".into(),
            "most-commented" => "most-commented".into(),
            other => {
                if let Some(slug) = other.strip_prefix("genre:") {
                    format!("genre/{}", slug)
                } else if let Some(slug) = other.strip_prefix("country:") {
                    format!("country/{}", slug)
                } else if let Some(slug) = other.strip_prefix("year:") {
                    format!("year/{}", slug)
                } else {
                    format!("genre/{}", other)
                }
            }
        };
        if p == 1 {
            format!("{}/{}", LK21_BASE, path)
        } else {
            format!("{}/{}/page/{}", LK21_BASE, path, p)
        }
    }

    fn parse_movie(url: &str, html: &str) -> MovieDetail {
        let parser = HtmlParser::parse(html);

        let title = parser.text("h1").map(|t| clean_movie_title(&t));

        let poster = parser
            .attr("meta[property='og:image']", "content")
            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
            .map(|s| resolve_url(url, &s));

        // .info-tag: rating (in <strong>), then quality / resolution / duration.
        let mut rating = None;
        let mut quality = None;
        let mut resolution = None;
        let mut duration = None;
        if let Some(strong) = parser.text(".info-tag strong") {
            let r = strong.trim().trim_start_matches('★').trim().to_string();
            if !r.is_empty() {
                rating = Some(r);
            }
        }
        for span in parser.texts(".info-tag span") {
            let s = span.trim().to_string();
            if s.is_empty() || s.starts_with('★') || s == rating.clone().unwrap_or_default() {
                continue;
            }
            if RESOLUTION_RE.is_match(&s) {
                resolution.get_or_insert(s);
            } else if s.contains('h') || s.contains('m') || s.contains(':') {
                duration.get_or_insert(s);
            } else if quality.is_none() {
                quality = Some(s);
            }
        }
        // The duration value (e.g. "1h 56m") combined with resolution gives a
        // friendly quality label; keep resolution appended to quality.
        if let (Some(q), Some(res)) = (quality.clone(), resolution.clone()) {
            quality = Some(format!("{} {}", q, res));
        } else if quality.is_none() {
            quality = resolution.clone();
        }

        // Genres / countries from the inline tag list.
        let mut genres = Vec::new();
        let mut countries = Vec::new();
        if let Ok(sel) = Selector::parse(".tag-list .tag a, .tag-list a") {
            for a in parser.document().select(&sel) {
                let href = a.value().attr("href").unwrap_or("");
                let text = a.text().collect::<String>().trim().to_string();
                if text.is_empty() {
                    continue;
                }
                if href.contains("/genre/") && !genres.contains(&text) {
                    genres.push(text);
                } else if href.contains("/country/") && !countries.contains(&text) {
                    countries.push(text);
                }
            }
        }

        let synopsis = parser
            .text(".synopsis")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // .detail block: labelled <p> rows.
        let mut directors = Vec::new();
        let mut cast = Vec::new();
        let mut release_date = None;
        if let Ok(p_sel) = Selector::parse(".detail p") {
            let a_sel = Selector::parse("a").ok();
            for p in parser.document().select(&p_sel) {
                let label = p
                    .select(&Selector::parse("span").unwrap())
                    .next()
                    .map(|n| n.text().collect::<String>().to_lowercase())
                    .unwrap_or_default();
                let links: Vec<String> = a_sel
                    .as_ref()
                    .map(|s| {
                        p.select(s)
                            .map(|a| a.text().collect::<String>().trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                if label.contains("sutradara") {
                    directors = links;
                } else if label.contains("bintang") {
                    cast = links;
                } else if label.contains("negara") {
                    for c in links {
                        if !countries.contains(&c) {
                            countries.push(c);
                        }
                    }
                } else if label.contains("release") {
                    let full = p.text().collect::<String>();
                    let val = full
                        .split_once(':')
                        .map(|(_, v)| v)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !val.is_empty() {
                        release_date = Some(val);
                    }
                }
            }
        }

        let year = YEAR_SLUG_RE
            .find(url)
            .map(|m| m.as_str().trim_matches(['-', '/']).to_string())
            .and_then(|s| s.split('-').next().map(|y| y.to_string()))
            .filter(|y| y.len() == 4);

        let embed_url = parser
            .attr("#main-player", "src")
            .or_else(|| parser.attr(".player-wrapper iframe", "src"))
            .or_else(|| parser.attr("iframe", "src"))
            .map(|s| normalize_embed(&s));

        // "GANTI PLAYER" switchable servers: #player-list li a[data-server][data-url].
        let servers = Self::parse_servers(&parser, url);

        // "MOVIE TERKAIT" related suggestions.
        let related = Self::parse_related(&parser, url);

        let download_url = parser
            .select_all(".movie-action a, a")
            .iter()
            .filter_map(|a| a.value().attr("href"))
            .find(|h| h.contains("dl.lk21") || h.contains("/download"))
            .map(|h| h.to_string());

        MovieDetail {
            title,
            synopsis,
            poster,
            year,
            rating,
            quality,
            duration,
            genres,
            countries,
            directors,
            cast,
            release_date,
            embed_url,
            servers,
            related,
            download_url,
            url: url.to_string(),
        }
    }

    /// Parse the "GANTI PLAYER" server list. Each `<li><a>` carries
    /// `data-server` (machine name), `data-url` / `href` (wrapper embed) and a
    /// text label. Falls back to the `<select id="player-select">` options.
    fn parse_servers(parser: &HtmlParser, base: &str) -> Vec<MovieServer> {
        fn push(
            servers: &mut Vec<MovieServer>,
            seen: &mut std::collections::HashSet<String>,
            base: &str,
            name: String,
            label: String,
            raw: &str,
        ) {
            let embed = normalize_embed(&resolve_url(base, raw));
            if embed.is_empty() || !seen.insert(name.clone()) {
                return;
            }
            servers.push(MovieServer {
                name,
                label,
                embed_url: embed,
            });
        }
        let mut servers: Vec<MovieServer> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Ok(sel) = Selector::parse("#player-list li a, .player-options a[data-url]") {
            for a in parser.document().select(&sel) {
                let v = a.value();
                let raw = v.attr("data-url").or_else(|| v.attr("href")).unwrap_or("");
                if raw.is_empty() || raw == "#" {
                    continue;
                }
                let label = a.text().collect::<String>().trim().to_string();
                let name = v
                    .attr("data-server")
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| label.to_lowercase());
                if name.is_empty() {
                    continue;
                }
                let label = if label.is_empty() {
                    name.to_uppercase()
                } else {
                    label
                };
                push(&mut servers, &mut seen, base, name, label, raw);
            }
        }
        if servers.is_empty() {
            if let Ok(sel) = Selector::parse("#player-select option[value]") {
                for o in parser.document().select(&sel) {
                    let raw = o.value().attr("value").unwrap_or("");
                    if raw.is_empty() {
                        continue;
                    }
                    let name = o
                        .value()
                        .attr("data-server")
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();
                    let label = o
                        .text()
                        .collect::<String>()
                        .trim()
                        .trim_start_matches("GANTI PLAYER")
                        .trim()
                        .to_string();
                    let name = if name.is_empty() {
                        label.to_lowercase()
                    } else {
                        name
                    };
                    if name.is_empty() {
                        continue;
                    }
                    let label = if label.is_empty() {
                        name.to_uppercase()
                    } else {
                        label
                    };
                    push(&mut servers, &mut seen, base, name, label, raw);
                }
            }
        }
        servers
    }

    /// Parse the "MOVIE TERKAIT" related list: `.related-content .video-list li a`.
    fn parse_related(parser: &HtmlParser, base: &str) -> Vec<MovieRelated> {
        let mut out = Vec::new();
        let sel = match Selector::parse(".related-content .video-list li a, .related-content li a")
        {
            Ok(s) => s,
            Err(_) => return out,
        };
        let title_sel = Selector::parse(".video-title").ok();
        let year_sel = Selector::parse(".video-year").ok();
        let img_sel = Selector::parse("img").ok();
        for a in parser.document().select(&sel) {
            let href = a.value().attr("href").unwrap_or("");
            if href.is_empty() || href == "#" {
                continue;
            }
            let url = resolve_url(base, href);
            if !Self::is_detail_url(&url) {
                continue;
            }
            let title = title_sel
                .as_ref()
                .and_then(|s| a.select(s).next())
                .map(|n| n.text().collect::<String>().trim().to_string())
                .or_else(|| {
                    img_sel
                        .as_ref()
                        .and_then(|s| a.select(s).next())
                        .and_then(|n| n.value().attr("title").map(|t| t.trim().to_string()))
                })
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let year = year_sel
                .as_ref()
                .and_then(|s| a.select(s).next())
                .map(|n| n.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());
            let poster = img_sel
                .as_ref()
                .and_then(|s| a.select(s).next())
                .and_then(|n| {
                    let v = n.value();
                    v.attr("data-src")
                        .or_else(|| v.attr("data-original"))
                        .or_else(|| v.attr("src"))
                        .filter(|s| !s.is_empty() && !s.starts_with("data:"))
                        .map(|s| resolve_url(base, s))
                });
            if out.iter().any(|r: &MovieRelated| r.url == url) {
                continue;
            }
            out.push(MovieRelated {
                title,
                url,
                poster,
                year,
            });
        }
        out
    }
}

fn clean_movie_title(s: &str) -> String {
    let mut t = s.trim().to_string();
    for prefix in ["Nonton series ", "Nonton Series ", "Nonton "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.to_string();
            break;
        }
    }
    // Cut common suffixes.
    for marker in [
        " Sub Indo",
        " Subtitle Indonesia",
        " di Lk21",
        " streaming",
        " Streaming",
    ] {
        if let Some(idx) = t.find(marker) {
            t.truncate(idx);
        }
    }
    t.trim().to_string()
}

fn normalize_embed(src: &str) -> String {
    if let Some(rest) = src.strip_prefix("//") {
        format!("https://{}", rest)
    } else {
        src.to_string()
    }
}

#[async_trait]
impl SiteAdapter for Lk21Adapter {
    fn name(&self) -> &str {
        "lk21"
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
        Ok(vec![ContentModel::Movie(Self::parse_movie(url, html))])
    }
}
