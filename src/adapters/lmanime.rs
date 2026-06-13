//! Adapter for lmanime.com — Chinese anime / donghua with English &
//! multi-language subs (the "animestream" WordPress theme).
//!
//! The markup is part of the themesia family used by anichin/otakudesu, so the
//! shapes mirror those adapters:
//!   - `<h1>` title, `og:image` cover
//!   - `.spe` metadata block (Status / Studio / Released / Duration / Type /
//!     Episodes / Director / Producers)
//!   - genre links `a[href*="/genres/"][rel="tag"]`
//!   - `.eplister ul li a` episode list (`.epl-num`, `.epl-title`, `.epl-date`)
//!   - a server `<select>` whose option values are `/v/N/` resolver pages
//!   - `.soraddlx` download blocks
//!
//! Series and episodes both live at root-level slugs; episodes are
//! distinguished by the `-episode-<n>/` suffix.

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{
    AnimeDownloadGroup, AnimeEpisode, AnimeEpisodeRef, AnimeSeries, AnimeStreamMirror,
    ContentModel, DownloadMirror,
};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::Selector;
use std::collections::HashMap;

static EP_NUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)episode\s*0*(\d+(?:\.\d+)?)").unwrap());

pub struct LmanimeAdapter;

impl LmanimeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Build a browse URL.
    /// `feed` values:
    ///   "home" / "ongoing"        -> ongoing series
    ///   "completed"               -> completed series
    ///   "all" / "az" / "title"    -> full A-Z catalogue (`/anime-list/`)
    ///   any other slug            -> `/genres/<slug>/`
    /// Pagination is the standard WordPress `/page/N/` segment.
    ///
    /// Note: lmanime's `/anime-list/` is a static A-Z catalogue (it ignores
    /// any `?order=` param), and the homepage's "latest" cards link to
    /// episodes rather than series, so those aren't usable as series feeds.
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let path: String = match feed {
            "" | "home" | "ongoing" | "latest" | "update" => "ongoing".into(),
            "completed" | "complete" | "all" | "az" | "title" | "list" | "popular" => {
                "anime-list".into()
            }
            other => format!("genres/{}", other),
        };
        if p == 1 {
            format!("https://lmanime.com/{}/", path)
        } else {
            format!("https://lmanime.com/{}/page/{}/", path, p)
        }
    }

    /// Search URL for a keyword query.
    pub fn search_url(query: &str, page: u32) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        if page <= 1 {
            format!("https://lmanime.com/?s={}", q)
        } else {
            format!("https://lmanime.com/page/{}/?s={}", page, q)
        }
    }

    /// Episode pages carry a `-episode-<n>/` suffix.
    pub fn is_episode_url(url: &str) -> bool {
        url.contains("lmanime.com") && url.contains("-episode-")
    }

    /// A series page is a lmanime root-level slug that isn't an episode or one
    /// of the site/taxonomy pages.
    pub fn is_series_url(url: &str) -> bool {
        if !url.contains("lmanime.com") || Self::is_episode_url(url) {
            return false;
        }
        let after = url.split("lmanime.com").nth(1).unwrap_or("");
        let path = after.split(['?', '#']).next().unwrap_or("");
        let slug = path.trim_matches('/');
        if slug.is_empty() || slug.contains('/') {
            // Empty (homepage) or nested (taxonomy / system) paths are not series.
            return false;
        }
        const NON_SERIES: &[&str] = &[
            "anime-list",
            "ongoing",
            "completed",
            "schedule",
            "upcoming",
            "bookmarks",
            "support-us",
            "genres",
            "studio",
            "director",
            "producer",
            "page",
            "wp-admin",
            "wp-json",
            "wp-content",
            "feed",
            "search",
            "season",
            "type",
        ];
        !NON_SERIES.contains(&slug)
    }

    fn parse_series(url: &str, html: &str) -> AnimeSeries {
        let parser = HtmlParser::parse(html);

        let title = parser
            .text("h1.entry-title")
            .or_else(|| parser.text("h1"))
            .map(|s| clean_series_title(&s));

        let thumbnail = parser
            .attr("meta[property='og:image']", "content")
            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
            .or_else(|| pick_real_image(&parser, ".thumbook img, .thumb img, .bigcover img"))
            .map(|s| resolve_url(url, &s));

        let synopsis = parser
            .text(".bixbox.synp .entry-content")
            .or_else(|| parser.text(".entry-content.entry-content-single"))
            .or_else(|| parser.text(".entry-content"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // `.spe` metadata: one <span> per field, with a leading <b>Label:</b>.
        let mut meta: HashMap<String, String> = HashMap::new();
        if let Ok(span_sel) = Selector::parse(".spe span") {
            let doc = parser.document();
            for span in doc.select(&span_sel) {
                let full = span.text().collect::<String>();
                if let Some((label, value)) = full.split_once(':') {
                    let key = label.trim().to_lowercase();
                    let value = value.trim().to_string();
                    if !key.is_empty() && !value.is_empty() {
                        meta.entry(key).or_insert(value);
                    }
                }
            }
        }

        let genres = collect_genres(&parser);

        let episodes = parse_episode_list(url, &parser);

        AnimeSeries {
            title,
            japanese_title: meta.get("japanese").cloned(),
            synopsis,
            thumbnail,
            score: meta.get("score").or_else(|| meta.get("rating")).cloned(),
            producer: meta
                .get("producers")
                .or_else(|| meta.get("producer"))
                .cloned(),
            anime_type: meta.get("type").cloned(),
            status: meta.get("status").cloned(),
            total_episodes: meta.get("episodes").cloned(),
            duration: meta.get("duration").cloned(),
            release_date: meta.get("released").cloned(),
            studio: meta.get("studio").cloned(),
            genres,
            episodes,
            batch: Vec::new(),
            url: url.to_string(),
        }
    }

    fn parse_episode(url: &str, html: &str) -> AnimeEpisode {
        let parser = HtmlParser::parse(html);

        let raw_title = parser.text("h1.entry-title").or_else(|| parser.text("h1"));
        let series_title = raw_title.as_ref().map(|t| {
            let cut = EP_NUM_RE.find(t).map(|m| m.start()).unwrap_or(t.len());
            clean_series_title(t[..cut].trim_end_matches(['-', '–', ':', ' ']).trim())
        });
        let episode_number = raw_title
            .as_ref()
            .and_then(|t| EP_NUM_RE.captures(t))
            .and_then(|c| c[1].parse::<f64>().ok());

        // Default embed: the iframe already present in the player container.
        let default_embed = parser
            .attr("#pembed iframe", "src")
            .or_else(|| parser.attr(".player-embed iframe", "src"))
            .or_else(|| parser.attr(".mctnx iframe", "src"))
            .map(|s| normalize_embed(&s));

        // Server mirrors: each <option> value is a `/v/N/` resolver page.
        let mut mirrors: Vec<AnimeStreamMirror> = Vec::new();
        if let Ok(opt_sel) = Selector::parse(".mirror select option, select.mirror option") {
            let doc = parser.document();
            for (idx, opt) in doc.select(&opt_sel).enumerate() {
                let value = opt.value().attr("value").unwrap_or("").trim().to_string();
                if value.is_empty() {
                    continue;
                }
                let name = opt.text().collect::<String>().trim().to_string();
                let name = if name.is_empty() {
                    format!("Server {}", mirrors.len() + 1)
                } else {
                    name
                };
                mirrors.push(AnimeStreamMirror {
                    name,
                    quality: String::new(),
                    token: resolve_url(url, &value),
                    default: idx == 0 || mirrors.is_empty(),
                });
            }
        }
        // Mark the first mirror as default if none flagged.
        if !mirrors.iter().any(|m| m.default) {
            if let Some(first) = mirrors.first_mut() {
                first.default = true;
            }
        }

        let downloads = parse_downloads(&parser);

        let (prev_episode, next_episode) = parse_prev_next(url, &parser);
        let series_url = parse_series_link(url, &parser);

        AnimeEpisode {
            series_title,
            episode_number,
            default_embed,
            mirrors,
            downloads,
            prev_episode,
            next_episode,
            series_url,
            url: url.to_string(),
        }
    }
}

/// Strip "Watch ", trailing " Sub Indo"/" Subtitle Indonesia" noise from titles.
fn clean_series_title(s: &str) -> String {
    let mut t = s.trim().to_string();
    for prefix in ["Watch ", "Nonton "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.to_string();
        }
    }
    t.trim().to_string()
}

/// Some servers emit protocol-relative iframe srcs (`//host/..`).
fn normalize_embed(src: &str) -> String {
    if let Some(rest) = src.strip_prefix("//") {
        format!("https://{}", rest)
    } else {
        src.to_string()
    }
}

/// Pick the first real image URL from a selector, preferring lazy-load
/// attributes and skipping data: placeholders and theme loading GIFs.
fn pick_real_image(parser: &HtmlParser, selector: &str) -> Option<String> {
    for el in parser.select_all(selector) {
        let v = el.value();
        let candidate = v
            .attr("data-src")
            .or_else(|| v.attr("data-lazy-src"))
            .or_else(|| v.attr("data-lazysrc"))
            .or_else(|| v.attr("src"));
        if let Some(s) = candidate {
            let s = s.trim();
            if !s.is_empty() && !s.starts_with("data:") && !s.contains("loading.gif") {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn collect_genres(parser: &HtmlParser) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(sel) = Selector::parse(".genxed a, a[href*='/genres/'][rel='tag']") {
        let doc = parser.document();
        for a in doc.select(&sel) {
            let href = a.value().attr("href").unwrap_or("");
            // Require a non-empty slug after `/genres/` so the bare index link
            // (label "Genres") is skipped.
            let slug = href
                .split("/genres/")
                .nth(1)
                .map(|s| s.trim_matches('/'))
                .unwrap_or("");
            if slug.is_empty() || slug.contains('/') {
                continue;
            }
            let text = a.text().collect::<String>().trim().to_string();
            if !text.is_empty() && !text.eq_ignore_ascii_case("genres") && !out.contains(&text) {
                out.push(text);
            }
        }
    }
    out
}

fn parse_episode_list(base_url: &str, parser: &HtmlParser) -> Vec<AnimeEpisodeRef> {
    let mut out = Vec::new();
    let li_sel = match Selector::parse(".eplister ul li a") {
        Ok(s) => s,
        Err(_) => return out,
    };
    let num_sel = Selector::parse(".epl-num").ok();
    let title_sel = Selector::parse(".epl-title").ok();
    let date_sel = Selector::parse(".epl-date").ok();

    let doc = parser.document();
    for a in doc.select(&li_sel) {
        let href = match a.value().attr("href").filter(|h| !h.is_empty()) {
            Some(h) => resolve_url(base_url, h),
            None => continue,
        };
        if !href.contains("-episode-") {
            continue;
        }
        let num_text = num_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .map(|n| n.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let number = num_text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .parse::<f64>()
            .ok();
        let title = title_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());
        let date = date_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        out.push(AnimeEpisodeRef {
            number,
            title,
            date,
            url: href,
        });
    }
    out
}

fn parse_downloads(parser: &HtmlParser) -> Vec<AnimeDownloadGroup> {
    let mut groups = Vec::new();
    let block_sel = match Selector::parse(".soraddlx") {
        Ok(s) => s,
        Err(_) => return groups,
    };
    let title_sel = Selector::parse(".sorattlx h3, .sorattlx").ok();
    let row_sel = Selector::parse(".soraurlx").ok();
    let a_sel = Selector::parse("a").ok();

    let doc = parser.document();
    for block in doc.select(&block_sel) {
        let quality = title_sel
            .as_ref()
            .and_then(|s| block.select(s).next())
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Download".to_string());

        let mut mirrors = Vec::new();
        if let (Some(rs), Some(asel)) = (row_sel.as_ref(), a_sel.as_ref()) {
            for row in block.select(rs) {
                for a in row.select(asel) {
                    let href = match a.value().attr("href").filter(|h| !h.is_empty()) {
                        Some(h) => h.to_string(),
                        None => continue,
                    };
                    let name = a.text().collect::<String>().trim().to_string();
                    if name.is_empty() {
                        continue;
                    }
                    mirrors.push(DownloadMirror { name, url: href });
                }
            }
        }
        if !mirrors.is_empty() {
            groups.push(AnimeDownloadGroup {
                quality,
                size: None,
                mirrors,
            });
        }
    }
    groups
}

fn parse_prev_next(base_url: &str, parser: &HtmlParser) -> (Option<String>, Option<String>) {
    let mut prev = None;
    let mut next = None;
    if let Ok(sel) = Selector::parse(".naveps .nvs a, .nvs a") {
        let doc = parser.document();
        for a in doc.select(&sel) {
            let href = match a
                .value()
                .attr("href")
                .filter(|h| !h.is_empty() && *h != "#")
            {
                Some(h) => resolve_url(base_url, h),
                None => continue,
            };
            if !href.contains("-episode-") {
                continue;
            }
            let class = a.value().attr("class").unwrap_or("");
            let rel = a.value().attr("rel").unwrap_or("");
            if class.contains("prev") || rel == "prev" {
                prev.get_or_insert(href);
            } else if class.contains("next") || rel == "next" {
                next.get_or_insert(href);
            } else if prev.is_none() {
                prev = Some(href);
            } else if next.is_none() {
                next = Some(href);
            }
        }
    }
    (prev, next)
}

fn parse_series_link(base_url: &str, parser: &HtmlParser) -> Option<String> {
    // The breadcrumb / "all episodes" anchor points at the series root slug.
    if let Ok(sel) = Selector::parse(".naveps .nvsc a, .breadcrumb a, span[itemprop='name'] a") {
        let doc = parser.document();
        for a in doc.select(&sel) {
            if let Some(href) = a.value().attr("href") {
                let resolved = resolve_url(base_url, href);
                if LmanimeAdapter::is_series_url(&resolved) {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

#[async_trait]
impl SiteAdapter for LmanimeAdapter {
    fn name(&self) -> &str {
        "lmanime"
    }

    fn matches(&self, url: &str) -> bool {
        Self::is_episode_url(url) || Self::is_series_url(url)
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
        if Self::is_episode_url(url) {
            Ok(vec![ContentModel::AnimeEpisode(Self::parse_episode(
                url, html,
            ))])
        } else {
            Ok(vec![ContentModel::AnimeSeries(Self::parse_series(
                url, html,
            ))])
        }
    }
}
