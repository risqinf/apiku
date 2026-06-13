//! Adapter for anichin.cafe / anichin.care (donghua streaming, WordPress + ts theme).
//!
//! Series pages live at `/seri/<slug>/` and contain:
//!   - h1.entry-title and `.thumbook .thumb img` cover
//!   - `.spe span` for status, network, studio, released, duration, season, country, type, episodes
//!   - `.genxed a` for genre tags
//!   - `.entry-content[itemprop=description]` for synopsis
//!   - `.eplister li` for episode list (with epl-num, epl-title, epl-date)
//!
//! Episode pages have `<select class="mirror">` with base64-encoded `<iframe>`
//! HTML for each player option, and `.soraurlx` blocks per resolution with
//! mirror download links.

use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{
    ContentModel, DonghuaEpisode, DonghuaSeries, DownloadGroup, DownloadMirror, EpisodeInfo,
    VideoSource,
};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use base64::Engine;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static EP_NUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:episode|ep|eps)[\s._-]*(\d+)").unwrap());
static IFRAME_SRC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"src="([^"]+)""#).unwrap());

pub struct AnichinAdapter;

/// A single donghua entry within a schedule day.
#[derive(Debug, Clone)]
pub struct ScheduleItem {
    /// Series title.
    pub title: String,
    /// Absolute `/seri/...` series URL.
    pub url: String,
    /// Cover/thumbnail URL (absolute) when present.
    pub thumbnail: Option<String>,
    /// Upcoming/last episode label from the `.sb` badge (e.g. "7" or "??").
    pub episode: Option<String>,
    /// Release-time label from the `.bt .epx` element, e.g. "at 09:47" or
    /// "released". `None` when the element is missing or empty.
    pub time_label: Option<String>,
    /// Unix release timestamp in SECONDS parsed from `data-rlsdt`. `None` when
    /// the attribute is empty (already released) or non-numeric.
    pub release_at: Option<i64>,
}

/// One weekday block of the release schedule.
#[derive(Debug, Clone)]
pub struct ScheduleDay {
    /// Day label as shown on the page (e.g. "Monday").
    pub day: String,
    /// Series releasing on this day, in source order.
    pub items: Vec<ScheduleItem>,
}

impl AnichinAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Build a browse URL for Anichin's series catalogue.
    ///
    /// Uses the themesia series archive at `/seri/`, which honours an `order`
    /// query parameter (the homepage's empty `?s=` search ignores it, which is
    /// why every feed used to return the same list). Pagination is via a
    /// `?page=N` query param (NOT the `/page/N/` path), matching the theme's
    /// own "Next" pager.
    ///
    /// `feed` values:
    ///   "home" / "latest" / "update" -> sorted by recent updates
    ///   "popular"                    -> all-time popular (most viewed)
    ///   "rating"                     -> by rating
    ///   "title" / "az"               -> A-Z
    ///   "latest-added"               -> newest additions
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let order = match feed {
            "popular" => "popular",
            "rating" => "rating",
            "title" | "az" => "title",
            "latest-added" => "latest",
            "" | "home" | "latest" | "update" => "update",
            other => other,
        };
        if p == 1 {
            format!("https://anichin.cafe/seri/?order={}", order)
        } else {
            format!("https://anichin.cafe/seri/?page={}&order={}", p, order)
        }
    }

    /// URL of the Anichin weekly release schedule page.
    pub fn schedule_url() -> &'static str {
        "https://anichin.cafe/schedule/"
    }

    /// Parse the weekly schedule page into ordered day blocks.
    ///
    /// The page renders one `.bixbox.schedulepage` block per weekday, each
    /// carrying a `sch_<day>` class and a `.releases h3 span` label. Within a
    /// block, `.listupd .bs .bsx a` anchors point at `/seri/<slug>/` series
    /// pages. Days are returned in the order they appear on the page so the
    /// canonical Monday→Sunday ordering is preserved. Empty days are skipped.
    ///
    /// Total and panic-free: missing DOM nodes are skipped rather than
    /// unwrapped, so a layout change degrades gracefully to fewer items.
    pub fn parse_schedule(base_url: &str, html: &str) -> Vec<ScheduleDay> {
        let parser = HtmlParser::parse(html);
        let mut days: Vec<ScheduleDay> = Vec::new();

        let anchor_sel = match scraper::Selector::parse(".listupd .bs .bsx a[href]") {
            Ok(s) => s,
            Err(_) => return days,
        };
        let day_label_sel = match scraper::Selector::parse(".releases h3 span") {
            Ok(s) => s,
            Err(_) => return days,
        };

        for block in parser.select_all(".bixbox.schedulepage") {
            let day = block
                .select(&day_label_sel)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty());

            let day = match day {
                Some(d) => d,
                None => continue,
            };

            let mut items: Vec<ScheduleItem> = Vec::new();
            for anchor in block.select(&anchor_sel) {
                let href = match anchor.value().attr("href") {
                    Some(h) if !h.is_empty() => h,
                    _ => continue,
                };
                let url = resolve_url(base_url, href);
                if !url.contains("/seri/") {
                    continue;
                }

                // Title: prefer the anchor's `title` attribute, fallback to `.tt` text.
                let title = anchor
                    .value()
                    .attr("title")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        scraper::Selector::parse(".tt").ok().and_then(|sel| {
                            anchor
                                .select(&sel)
                                .next()
                                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                        })
                    })
                    .unwrap_or_default();
                if title.is_empty() {
                    continue;
                }

                let thumbnail = scraper::Selector::parse("img").ok().and_then(|sel| {
                    anchor.select(&sel).next().and_then(|img| {
                        img.value()
                            .attr("data-src")
                            .or_else(|| img.value().attr("src"))
                            .map(|s| resolve_url(base_url, s))
                    })
                });

                let episode = scraper::Selector::parse(".bt .sb").ok().and_then(|sel| {
                    anchor
                        .select(&sel)
                        .next()
                        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                        .filter(|s| !s.is_empty())
                });

                // Release time: the `.bt .epx` element carries a human label
                // (e.g. "at 09:47" or "released") plus a `data-rlsdt` unix
                // seconds timestamp (empty for already-released items).
                let epx = scraper::Selector::parse(".bt .epx")
                    .ok()
                    .and_then(|sel| anchor.select(&sel).next());
                let time_label = epx
                    .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                    .filter(|s| !s.is_empty());
                let release_at = epx
                    .and_then(|el| el.value().attr("data-rlsdt"))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<i64>().ok());

                items.push(ScheduleItem {
                    title,
                    url,
                    thumbnail,
                    episode,
                    time_label,
                    release_at,
                });
            }

            if !items.is_empty() {
                days.push(ScheduleDay { day, items });
            }
        }

        days
    }

    fn is_series_url(url: &str) -> bool {
        url.contains("/seri/")
    }

    fn is_episode_url(url: &str) -> bool {
        // Anichin episode URLs follow pattern <slug>-episode-<n>-subtitle-indonesia/
        EP_NUM_RE.is_match(url) && !url.contains("/seri/")
    }

    fn extract_series(url: &str, html: &str) -> DonghuaSeries {
        let parser = HtmlParser::parse(html);

        let title = parser
            .text("h1.entry-title")
            .or_else(|| parser.attr("meta[property='og:title']", "content"))
            .map(|t| clean_title(&t));

        // Cover image
        let cover_image = parser
            .attr(".thumbook .thumb img", "src")
            .or_else(|| parser.attr(".bigcontent .thumb img", "src"))
            .or_else(|| parser.attr("img.wp-post-image", "src"))
            .or_else(|| parser.attr("meta[property='og:image']", "content"))
            .map(|u| resolve_url(url, &u));

        // Synopsis
        let synopsis = parser
            .text(".entry-content[itemprop='description']")
            .or_else(|| parser.text(".entry-content"))
            .or_else(|| parser.attr("meta[name='description']", "content"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // Genres from .genxed a
        let genres = parser
            .select_all(".genxed a")
            .iter()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        // Status (and other metadata) from .spe span
        let mut info: HashMap<String, String> = HashMap::new();
        for span in parser.select_all(".info-content .spe span") {
            let raw = span.text().collect::<Vec<_>>().join("");
            // span format: "<b>Key:</b> Value"
            if let Some((key, value)) = raw.split_once(':') {
                let k = key.trim().to_lowercase();
                let v = value.trim().to_string();
                if !k.is_empty() && !v.is_empty() {
                    info.insert(k, v);
                }
            }
        }

        let status = info.get("status").cloned().map(|s| {
            let lower = s.to_lowercase();
            if lower.contains("ongoing") || lower.contains("airing") {
                "airing".to_string()
            } else if lower.contains("completed") || lower.contains("tamat") {
                "completed".to_string()
            } else if lower.contains("upcoming") {
                "upcoming".to_string()
            } else {
                "unknown".to_string()
            }
        });

        // Episodes
        let episodes = Self::extract_episode_list(&parser, url);

        DonghuaSeries {
            title,
            synopsis,
            genres,
            status,
            thumbnail: cover_image,
            episodes,
            url: url.to_string(),
        }
    }

    fn extract_episode_list(parser: &HtmlParser, base_url: &str) -> Vec<EpisodeInfo> {
        let mut episodes: Vec<EpisodeInfo> = Vec::new();

        for li in parser.select_all(".eplister li") {
            // Extract anchor + sub-elements
            let anchor = match li.select(&scraper::Selector::parse("a").unwrap()).next() {
                Some(a) => a,
                None => continue,
            };

            let href = match anchor.value().attr("href") {
                Some(h) if !h.is_empty() => h,
                _ => continue,
            };

            let num_text = li
                .select(&scraper::Selector::parse(".epl-num").unwrap())
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                .unwrap_or_default();

            let title_text = li
                .select(&scraper::Selector::parse(".epl-title").unwrap())
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string());

            // Parse episode number from .epl-num (which may say "440 END")
            let number_only = num_text
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u32>();

            let number = match number_only {
                Ok(n) => n,
                Err(_) => {
                    // Fallback: extract from URL
                    EP_NUM_RE
                        .captures(href)
                        .and_then(|c| c[1].parse::<u32>().ok())
                        .unwrap_or(0)
                }
            };

            episodes.push(EpisodeInfo {
                number,
                title: title_text,
                url: resolve_url(base_url, href),
            });
        }

        // Sort ascending by number
        episodes.sort_by_key(|e| e.number);
        episodes
    }

    fn extract_episode(url: &str, html: &str) -> DonghuaEpisode {
        let parser = HtmlParser::parse(html);

        // Series title: prefer the link to the series page, fallback to stripping h1
        let series_link_text = parser
            .select_all("a[href*='/seri/']")
            .iter()
            .filter_map(|el| {
                let text = el.text().collect::<Vec<_>>().join("").trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
            .next();
        let series_title = series_link_text.or_else(|| {
            parser
                .text("h1.entry-title")
                .or_else(|| parser.text("h1"))
                .map(|s| strip_episode_suffix(&s))
        });

        // Series URL
        let series_url = parser
            .select_all("a[href*='/seri/']")
            .iter()
            .filter_map(|el| el.value().attr("href").map(|s| s.to_string()))
            .next();

        // Episode number from URL
        let episode_number = EP_NUM_RE
            .captures(url)
            .and_then(|c| c[1].parse::<u32>().ok())
            .unwrap_or(0);

        // Video sources from <select class="mirror"> options (base64-encoded iframe HTML)
        let mut sources: Vec<VideoSource> = Vec::new();
        for option in parser.select_all("select.mirror option, select.mirror2 option") {
            let value = option.value().attr("value").unwrap_or("");
            let label = option
                .text()
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string();
            if value.is_empty() {
                continue;
            }
            // Decode base64 iframe HTML
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(value)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok());

            let src = decoded
                .as_deref()
                .and_then(|s| IFRAME_SRC_RE.captures(s).map(|c| c[1].to_string()));

            if let Some(src) = src {
                let format = if src.contains(".m3u8") {
                    Some("hls".to_string())
                } else if src.contains(".mpd") {
                    Some("dash".to_string())
                } else {
                    Some("embed".to_string())
                };
                sources.push(VideoSource {
                    url: src,
                    quality: if label.is_empty() { None } else { Some(label) },
                    format,
                });
            }
        }

        // Also get the currently rendered iframe (#pembed iframe) as a default source
        if let Some(default_src) = parser.attr("#pembed iframe", "src") {
            if !sources.iter().any(|s| s.url == default_src) {
                sources.insert(
                    0,
                    VideoSource {
                        url: default_src,
                        quality: Some("Default".to_string()),
                        format: Some("embed".to_string()),
                    },
                );
            }
        }

        // Download groups from .soraurlx
        let downloads = Self::extract_download_groups(&parser);

        // Prev / Next episode navigation. Anichin uses:
        //   <a rel="prev" href="...">  for previous
        //   <a rel="next" href="...">  for next
        //   <span class="nolink">      when there is no next/prev (e.g. last episode)
        let prev_episode = parser
            .attr("a[rel='prev']", "href")
            .map(|s| s.trim().to_string());
        let next_episode = parser
            .attr("a[rel='next']", "href")
            .map(|s| s.trim().to_string());

        DonghuaEpisode {
            series_title,
            episode_number,
            sources,
            downloads,
            prev_episode,
            next_episode,
            series_url,
            url: url.to_string(),
        }
    }

    fn extract_download_groups(parser: &HtmlParser) -> Vec<DownloadGroup> {
        let mut groups: Vec<DownloadGroup> = Vec::new();

        for block in parser.select_all(".soraurlx") {
            // Quality is in <strong>
            let quality = block
                .select(&scraper::Selector::parse("strong").unwrap())
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                .unwrap_or_default();

            if quality.is_empty() {
                continue;
            }

            // Mirror links
            let mirrors = block
                .select(&scraper::Selector::parse("a").unwrap())
                .filter_map(|a| {
                    let href = a.value().attr("href")?.to_string();
                    let name = a.text().collect::<Vec<_>>().join("").trim().to_string();
                    if href.is_empty() || name.is_empty() {
                        None
                    } else {
                        Some(DownloadMirror { name, url: href })
                    }
                })
                .collect::<Vec<_>>();

            if !mirrors.is_empty() {
                groups.push(DownloadGroup { quality, mirrors });
            }
        }

        groups
    }
}

/// Clean a series title by stripping the site suffix.
fn clean_title(s: &str) -> String {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_suffix(" - Anichin")
        .or_else(|| trimmed.strip_suffix(" – Anichin"))
        .unwrap_or(trimmed);
    stripped.trim().to_string()
}

/// Strip "Episode N..." trailing text from a title to get the bare series name.
fn strip_episode_suffix(s: &str) -> String {
    let cleaned = clean_title(s);
    static SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\s+Episode\s+\d.*$").unwrap());
    SUFFIX_RE.replace(&cleaned, "").trim().to_string()
}

#[async_trait]
impl SiteAdapter for AnichinAdapter {
    fn name(&self) -> &str {
        "anichin"
    }

    fn matches(&self, url: &str) -> bool {
        // Match all anichin variants (anichin.cafe, anichin.care, anichin.cloud, etc.)
        let lower = url.to_lowercase();
        lower.contains("anichin.")
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
        if Self::is_series_url(url) {
            Ok(vec![ContentModel::DonghuaSeries(Self::extract_series(
                url, html,
            ))])
        } else if Self::is_episode_url(url) {
            Ok(vec![ContentModel::DonghuaEpisode(Self::extract_episode(
                url, html,
            ))])
        } else {
            // Homepage or other archive — let deep extractor handle it
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_schedule_groups_items_by_day_in_order() {
        let html = r##"
            <div class="bixbox schedulepage sch_monday">
              <div class="releases"><h3><span>Monday</span></h3></div>
              <div class="listupd">
                <div class="bs">
                  <div class="bsx">
                    <a href="https://anichin.cafe/seri/peerless/" title="Peerless Series">
                      <div class="bt">
                        <span class='epx cndwn' data-cndwn='47658' data-rlsdt='1780972490'>at 02:34</span>
                        <span class="sb Sub">7</span>
                      </div>
                      <div class="limit"><img src="https://cdn.example/peerless.jpg" width="200" height="300" /></div>
                      <div class="tt">Peerless Fallback</div>
                    </a>
                  </div>
                </div>
              </div>
            </div>
            <div class="bixbox schedulepage sch_tuesday">
              <div class="releases"><h3><span>Tuesday</span></h3></div>
              <div class="listupd">
                <div class="bs">
                  <div class="bsx">
                    <a href="https://anichin.cafe/seri/another/" title="Another Series">
                      <div class="bt">
                        <span class='epx cndwn' data-cndwn='-1200' data-rlsdt=''>released</span>
                        <span class="sb Sub">??</span>
                      </div>
                      <div class="limit"><img data-src="https://cdn.example/another.jpg" /></div>
                    </a>
                  </div>
                </div>
                <div class="bs">
                  <div class="bsx">
                    <a href="https://anichin.cafe/not-a-series/" title="Skip Me">
                      <div class="bt"><span class="sb">1</span></div>
                    </a>
                  </div>
                </div>
              </div>
            </div>
        "##;

        let days = AnichinAdapter::parse_schedule("https://anichin.cafe/schedule/", html);
        assert_eq!(days.len(), 2);

        assert_eq!(days[0].day, "Monday");
        assert_eq!(days[0].items.len(), 1);
        let monday = &days[0].items[0];
        assert_eq!(monday.title, "Peerless Series");
        assert_eq!(monday.url, "https://anichin.cafe/seri/peerless/");
        assert_eq!(
            monday.thumbnail.as_deref(),
            Some("https://cdn.example/peerless.jpg")
        );
        assert_eq!(monday.episode.as_deref(), Some("7"));
        assert_eq!(monday.time_label.as_deref(), Some("at 02:34"));
        assert_eq!(monday.release_at, Some(1780972490));

        // Tuesday keeps source order and skips the non-/seri/ link.
        assert_eq!(days[1].day, "Tuesday");
        assert_eq!(days[1].items.len(), 1);
        let tuesday = &days[1].items[0];
        assert_eq!(tuesday.title, "Another Series");
        assert_eq!(tuesday.episode.as_deref(), Some("??"));
        // Already-released items expose the label but an empty `data-rlsdt`
        // parses to `None`.
        assert_eq!(tuesday.time_label.as_deref(), Some("released"));
        assert_eq!(tuesday.release_at, None);
        assert_eq!(
            tuesday.thumbnail.as_deref(),
            Some("https://cdn.example/another.jpg")
        );
    }

    #[test]
    fn parse_schedule_title_falls_back_to_tt() {
        let html = r##"
            <div class="bixbox schedulepage sch_friday">
              <div class="releases"><h3><span>Friday</span></h3></div>
              <div class="listupd">
                <div class="bs">
                  <div class="bsx">
                    <a href="https://anichin.cafe/seri/no-title-attr/">
                      <div class="tt">Title From TT</div>
                    </a>
                  </div>
                </div>
              </div>
            </div>
        "##;

        let days = AnichinAdapter::parse_schedule("https://anichin.cafe/schedule/", html);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].items[0].title, "Title From TT");
        assert!(days[0].items[0].episode.is_none());
        assert!(days[0].items[0].time_label.is_none());
        assert!(days[0].items[0].release_at.is_none());
    }

    #[test]
    fn parse_schedule_empty_for_garbage() {
        assert!(
            AnichinAdapter::parse_schedule("https://anichin.cafe/schedule/", "<html></html>")
                .is_empty()
        );
    }
}
