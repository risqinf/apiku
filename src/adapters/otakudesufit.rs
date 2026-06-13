//! Adapter for otakudesu.fit — Indonesian-subtitled anime on the themesia
//! WordPress theme (the same family as anichin/lmanime), distinct from the
//! older otakudesu.blog markup.
//!
//!   - series live at `/series/<slug>/`
//!   - episodes at `/<slug>-episode-<n>-subtitle-indonesia/`
//!   - `.spe` metadata, `.eplister` episode list, `.genxed` genres
//!   - a `select.mirror` whose option values are base64-encoded HTML
//!     fragments containing the player `<iframe>` (decoded server-side, no
//!     extra request needed)
//!   - `.soraddlx` download blocks

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{
    AnimeDownloadGroup, AnimeEpisode, AnimeEpisodeRef, AnimeSeries, AnimeStreamMirror,
    ContentModel, DownloadMirror,
};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::Selector;
use std::collections::HashMap;

pub const OTAKUDESUFIT_BASE: &str = "https://otakudesu.fit";

static EP_NUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)episode\s*0*(\d+(?:\.\d+)?)").unwrap());

pub struct OtakudesufitAdapter;

impl OtakudesufitAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Build a browse URL. otakudesu.fit's series directory lives at
    /// `/series/?status=&type=&order=` (the `/ongoing-anime/` style paths
    /// redirect to the homepage). Status feeds map to `status`/`order` query
    /// params; any other value is a genre slug under `/genres/<slug>/`.
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        if Self::is_status_feed(feed) {
            let (status, order) = match feed {
                "ongoing" => ("Ongoing", "update"),
                "completed" | "complete" => ("Completed", "update"),
                "popular" => ("", "popular"),
                "title" | "az" => ("", "title"),
                _ => ("", "update"),
            };
            return format!(
                "{}/series/?status={}&type=&order={}&page={}",
                OTAKUDESUFIT_BASE, status, order, p
            );
        }
        if p == 1 {
            format!("{}/genres/{}/", OTAKUDESUFIT_BASE, feed)
        } else {
            format!("{}/genres/{}/page/{}/", OTAKUDESUFIT_BASE, feed, p)
        }
    }

    /// Whether a feed is a status/landing feed (served by the `/series/`
    /// archive) rather than a genre slug.
    pub fn is_status_feed(feed: &str) -> bool {
        matches!(
            feed,
            "" | "home"
                | "ongoing"
                | "completed"
                | "complete"
                | "all"
                | "az"
                | "title"
                | "list"
                | "latest"
                | "update"
                | "popular"
        )
    }

    pub fn search_url(query: &str, page: u32) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        if page <= 1 {
            format!("{}/?s={}", OTAKUDESUFIT_BASE, q)
        } else {
            format!("{}/page/{}/?s={}", OTAKUDESUFIT_BASE, page, q)
        }
    }

    pub fn is_episode_url(url: &str) -> bool {
        url.contains("otakudesu.fit") && url.contains("-episode-")
    }

    pub fn is_series_url(url: &str) -> bool {
        if !url.contains("otakudesu.fit") {
            return false;
        }
        let after = url.split("otakudesu.fit").nth(1).unwrap_or("");
        let path = after.split(['?', '#']).next().unwrap_or("");
        let trimmed = path.trim_matches('/');
        // Series pages are `/series/<slug>` (exactly two segments).
        let mut parts = trimmed.split('/');
        matches!(parts.next(), Some("series")) && parts.next().is_some_and(|s| !s.is_empty())
    }

    /// Decode a base64-encoded player fragment into the iframe `src` URL.
    pub fn embed_from_token(token: &str) -> Option<String> {
        let decoded = STANDARD
            .decode(token.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())?;
        // The fragment is an HTML snippet like `<p><iframe src="...">`.
        let re = Regex::new(r#"(?is)<iframe[^>]*\ssrc=["']([^"']+)["']"#).ok()?;
        re.captures(&decoded).map(|c| normalize_embed(c[1].trim()))
    }

    fn parse_series(url: &str, html: &str) -> AnimeSeries {
        let parser = HtmlParser::parse(html);
        let title = parser
            .text("h1.entry-title")
            .or_else(|| parser.text("h1"))
            .map(|s| clean_title(&s));
        let thumbnail = parser
            .attr("meta[property='og:image']", "content")
            .filter(|s| !s.is_empty() && !s.starts_with("data:"))
            .or_else(|| pick_real_image(&parser, ".thumbook img, .thumb img, .bigcover img"))
            .map(|s| resolve_url(url, &s));
        let synopsis = parser
            .text(".bixbox.synp .entry-content")
            .or_else(|| parser.text(".entry-content"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut meta: HashMap<String, String> = HashMap::new();
        if let Ok(span_sel) = Selector::parse(".spe span") {
            for span in parser.document().select(&span_sel) {
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
            total_episodes: meta
                .get("episodes")
                .or_else(|| meta.get("total episode"))
                .cloned(),
            duration: meta.get("duration").cloned(),
            release_date: meta.get("released").cloned(),
            studio: meta.get("studio").cloned(),
            genres: collect_genres(&parser),
            episodes: parse_episode_list(url, &parser),
            batch: Vec::new(),
            url: url.to_string(),
        }
    }

    fn parse_episode(url: &str, html: &str) -> AnimeEpisode {
        let parser = HtmlParser::parse(html);
        let raw_title = parser.text("h1.entry-title").or_else(|| parser.text("h1"));
        let series_title = raw_title.as_ref().map(|t| {
            let cut = EP_NUM_RE.find(t).map(|m| m.start()).unwrap_or(t.len());
            clean_title(t[..cut].trim_end_matches(['-', '–', ':', ' ']).trim())
        });
        let episode_number = raw_title
            .as_ref()
            .and_then(|t| EP_NUM_RE.captures(t))
            .and_then(|c| c[1].parse::<f64>().ok());

        // Default embed: iframe already present, else decode the first option.
        let mut mirrors: Vec<AnimeStreamMirror> = Vec::new();
        if let Ok(opt_sel) = Selector::parse("select.mirror option, .mirror select option") {
            for opt in parser.document().select(&opt_sel) {
                let value = opt.value().attr("value").unwrap_or("").trim().to_string();
                if value.len() < 8 {
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
                    token: value,
                    default: mirrors.is_empty(),
                });
            }
        }
        let default_embed = parser
            .attr(".player-embed iframe", "src")
            .or_else(|| parser.attr("#pembed iframe", "src"))
            .map(|s| normalize_embed(&s))
            .or_else(|| {
                mirrors
                    .first()
                    .and_then(|m| Self::embed_from_token(&m.token))
            });

        AnimeEpisode {
            series_title,
            episode_number,
            default_embed,
            mirrors,
            downloads: parse_downloads(&parser),
            prev_episode: None,
            next_episode: None,
            series_url: parse_series_link(url, &parser),
            url: url.to_string(),
        }
    }
}

fn clean_title(s: &str) -> String {
    let mut t = s.trim().to_string();
    for prefix in ["Nonton ", "Watch "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.to_string();
        }
    }
    for marker in [" Sub Indo", " Subtitle Indonesia"] {
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

fn pick_real_image(parser: &HtmlParser, selector: &str) -> Option<String> {
    for el in parser.select_all(selector) {
        let v = el.value();
        let candidate = v
            .attr("data-src")
            .or_else(|| v.attr("data-lazy-src"))
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
        for a in parser.document().select(&sel) {
            let href = a.value().attr("href").unwrap_or("");
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
    for a in parser.document().select(&li_sel) {
        let href = match a.value().attr("href").filter(|h| !h.is_empty()) {
            Some(h) => resolve_url(base_url, h),
            None => continue,
        };
        if !href.contains("-episode-") {
            continue;
        }
        let number = num_sel
            .as_ref()
            .and_then(|s| a.select(s).next())
            .map(|n| n.text().collect::<String>())
            .map(|t| {
                t.chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect::<String>()
            })
            .and_then(|t| t.parse::<f64>().ok());
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
    for block in parser.document().select(&block_sel) {
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

fn parse_series_link(base_url: &str, parser: &HtmlParser) -> Option<String> {
    if let Ok(sel) = Selector::parse(".nvs a, .nvsc a, .breadcrumb a, span[itemprop='name'] a") {
        for a in parser.document().select(&sel) {
            if let Some(href) = a.value().attr("href") {
                let resolved = resolve_url(base_url, href);
                if OtakudesufitAdapter::is_series_url(&resolved) {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

#[async_trait]
impl SiteAdapter for OtakudesufitAdapter {
    fn name(&self) -> &str {
        "otakudesufit"
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
