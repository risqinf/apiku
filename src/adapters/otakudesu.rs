//! Adapter for otakudesu.blog (anime streaming, WordPress + "Animestream" theme).
//!
//! URL kinds:
//!   * search  - `/?s=<query>&post_type=anime` -> `<ul class="chivsrc"><li>` cards
//!   * anime   - `/anime/<slug>/`              -> `.infozingle` metadata + `.episodelist`
//!   * episode - `/episode/<slug>/`            -> default `<iframe>` + `.mirrorstream`
//!     quality/host mirrors + `.download` block
//!
//! Streaming mirrors are not plain links: each `<li><a data-content="<b64>">`
//! carries a base64 `{id,i,q}` token. The page resolves it via a two-step
//! `admin-ajax.php` POST (fetch nonce, then fetch the embed HTML). We expose
//! the token in the episode payload and resolve it on demand in the API layer
//! (`/api/v1/anime/stream`), so consumers get a ready embed URL.

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
    Lazy::new(|| Regex::new(r"(?i)episode\s*([0-9]+(?:\.[0-9]+)?)").unwrap());
static IFRAME_SRC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"src="([^"]+)""#).unwrap());

/// `admin-ajax.php` action that returns a fresh nonce.
pub const AJAX_NONCE_ACTION: &str = "aa1208d27f29ca340c92c66d1926f13f";
/// `admin-ajax.php` action that returns the base64 embed HTML for a mirror.
pub const AJAX_STREAM_ACTION: &str = "2a3505c93b0035d3f455df82bf976b84";
/// The AJAX endpoint (relative to the otakudesu origin).
pub const AJAX_PATH: &str = "/wp-admin/admin-ajax.php";

pub struct OtakudesuAdapter;

impl OtakudesuAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn matches_url(url: &str) -> bool {
        url.to_lowercase().contains("otakudesu.")
    }

    /// Build the search URL (anime post type).
    pub fn search_url(query: &str, _page: u32) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        format!("https://otakudesu.blog/?s={}&post_type=anime", q)
    }

    /// Build a browse / feed URL.
    /// `home`/`ongoing` -> ongoing list, `complete` -> completed list,
    /// `genre/<slug>` passthrough, otherwise the homepage.
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let paged = |base: &str| {
            if p > 1 {
                format!(
                    "https://otakudesu.blog/{}/page/{}/",
                    base.trim_matches('/'),
                    p
                )
            } else {
                format!("https://otakudesu.blog/{}/", base.trim_matches('/'))
            }
        };
        match feed {
            "" | "home" | "ongoing" | "ongoing-anime" => paged("ongoing-anime"),
            "complete" | "completed" | "complete-anime" => paged("complete-anime"),
            other if other.starts_with("genre") => paged(other),
            other if other.starts_with("genres/") => paged(other),
            other => paged(&format!("genres/{}", other)),
        }
    }

    fn is_anime_detail(url: &str) -> bool {
        url.contains("/anime/")
    }

    fn is_episode(url: &str) -> bool {
        url.contains("/episode/")
    }

    // ---- search -----------------------------------------------------------

    /// Parse `/?s=...&post_type=anime` results into unified search items as a
    /// `(title, url, thumbnail, genres, status, rating)` tuple list.
    pub fn parse_search(base_url: &str, html: &str) -> Vec<SearchHit> {
        let parser = HtmlParser::parse(html);
        let mut out = Vec::new();
        let a_sel = Selector::parse("h2 a").unwrap();
        let img_sel = Selector::parse("img").unwrap();
        let set_sel = Selector::parse("div.set").unwrap();
        let genre_a = Selector::parse("a").unwrap();
        let b_sel = Selector::parse("b").unwrap();

        for li in parser.select_all("ul.chivsrc > li") {
            let a = match li.select(&a_sel).next() {
                Some(a) => a,
                None => continue,
            };
            let title = a.text().collect::<String>().trim().to_string();
            let url = match a.value().attr("href") {
                Some(h) => resolve_url(base_url, h),
                None => continue,
            };
            // Only keep anime detail links.
            if !url.contains("/anime/") || title.is_empty() {
                continue;
            }
            let thumbnail = li.select(&img_sel).next().and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });
            let mut genres = Vec::new();
            let mut status = None;
            let mut rating = None;
            for set in li.select(&set_sel) {
                let label = set
                    .select(&b_sel)
                    .next()
                    .map(|b| b.text().collect::<String>().to_lowercase())
                    .unwrap_or_default();
                if label.contains("genre") {
                    genres = set
                        .select(&genre_a)
                        .map(|g| g.text().collect::<String>().trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                } else if label.contains("status") {
                    status = Some(text_after_colon(&set.text().collect::<String>()));
                } else if label.contains("rating") || label.contains("skor") {
                    rating = Some(text_after_colon(&set.text().collect::<String>()));
                }
            }
            out.push(SearchHit {
                title,
                url,
                thumbnail,
                genres,
                status: status.filter(|s| !s.is_empty()),
                rating: rating.filter(|s| !s.is_empty()),
            });
        }
        out
    }

    // ---- anime detail -----------------------------------------------------

    pub fn parse_detail(url: &str, html: &str) -> Option<AnimeSeries> {
        let parser = HtmlParser::parse(html);

        // Cover image.
        let thumbnail = parser
            .attr(".fotoanime img", "src")
            .or_else(|| parser.attr(".fotoanime img", "data-src"))
            .map(|s| resolve_url(url, &s));

        // Metadata block: `.infozingle p > span > b: value`.
        let mut meta: HashMap<String, String> = HashMap::new();
        let mut genres: Vec<String> = Vec::new();
        {
            let b_sel = Selector::parse("b").ok();
            let a_sel = Selector::parse("a").ok();
            for p in parser.select_all(".infozingle p") {
                let full = p.text().collect::<String>();
                let label = b_sel
                    .as_ref()
                    .and_then(|s| p.select(s).next())
                    .map(|b| b.text().collect::<String>())
                    .unwrap_or_default();
                let key = label.trim().trim_end_matches(':').trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                if key.contains("genre") {
                    if let Some(a) = a_sel.as_ref() {
                        genres = p
                            .select(a)
                            .map(|g| g.text().collect::<String>().trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    continue;
                }
                let value = text_after_colon(&full);
                if !value.is_empty() {
                    meta.insert(key, value);
                }
            }
        }

        let title = meta
            .get("judul")
            .cloned()
            .or_else(|| parser.text(".jdlrx h1"))
            .or_else(|| parser.text("h1.posttl"))
            .filter(|s| !s.is_empty());

        // Synopsis: `.sinopc` paragraphs.
        let synopsis = {
            let s = parser
                .texts(".sinopc p")
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if s.trim().is_empty() {
                parser.text(".sinopc").filter(|s| !s.trim().is_empty())
            } else {
                Some(s)
            }
        };

        // Episode + batch lists. otakudesu renders multiple `.episodelist`
        // blocks; the "Batch" one (monktit contains "Batch") holds whole-season
        // archives, the others are the real episode list.
        let mut episodes: Vec<AnimeEpisodeRef> = Vec::new();
        let mut batch: Vec<AnimeEpisodeRef> = Vec::new();
        {
            let title_sel = Selector::parse(".monktit").ok();
            let li_sel = Selector::parse("ul li").ok();
            let a_sel = Selector::parse("a").ok();
            let date_sel = Selector::parse(".zeebr").ok();
            for block in parser.select_all(".episodelist") {
                let heading = title_sel
                    .as_ref()
                    .and_then(|s| block.select(s).next())
                    .map(|t| t.text().collect::<String>().to_lowercase())
                    .unwrap_or_default();
                let is_batch = heading.contains("batch");
                let (Some(li_sel), Some(a_sel)) = (li_sel.as_ref(), a_sel.as_ref()) else {
                    continue;
                };
                for li in block.select(li_sel) {
                    let a = match li.select(a_sel).next() {
                        Some(a) => a,
                        None => continue,
                    };
                    let ep_title = a.text().collect::<String>().trim().to_string();
                    let ep_url = match a.value().attr("href") {
                        Some(h) => resolve_url(url, h),
                        None => continue,
                    };
                    if ep_title.is_empty() {
                        continue;
                    }
                    let date = date_sel
                        .as_ref()
                        .and_then(|s| li.select(s).next())
                        .map(|d| d.text().collect::<String>().trim().to_string())
                        .filter(|s| !s.is_empty());
                    let number = EP_NUM_RE
                        .captures(&ep_title)
                        .and_then(|c| c[1].parse::<f64>().ok());
                    let entry = AnimeEpisodeRef {
                        number,
                        title: Some(ep_title),
                        date,
                        url: ep_url,
                    };
                    if is_batch {
                        batch.push(entry);
                    } else {
                        episodes.push(entry);
                    }
                }
            }
        }

        // Newest-first on the site; expose ascending by number when known.
        episodes.sort_by(|a, b| {
            a.number
                .unwrap_or(0.0)
                .partial_cmp(&b.number.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if title.is_none() && episodes.is_empty() {
            return None;
        }

        Some(AnimeSeries {
            title,
            japanese_title: meta.get("japanese").cloned(),
            synopsis,
            thumbnail,
            score: meta.get("skor").cloned(),
            producer: meta.get("produser").cloned(),
            anime_type: meta.get("tipe").cloned(),
            status: meta.get("status").cloned(),
            total_episodes: meta.get("total episode").cloned(),
            duration: meta.get("durasi").cloned(),
            release_date: meta.get("tanggal rilis").cloned(),
            studio: meta.get("studio").cloned(),
            genres,
            episodes,
            batch,
            url: url.to_string(),
        })
    }

    // ---- episode ----------------------------------------------------------

    pub fn parse_episode(url: &str, html: &str) -> Option<AnimeEpisode> {
        let parser = HtmlParser::parse(html);

        let series_title = parser
            .text("h1.posttl")
            .map(|t| {
                // Strip trailing "Episode N ... Subtitle Indonesia".
                EP_NUM_RE
                    .split(&t)
                    .next()
                    .unwrap_or(&t)
                    .trim()
                    .trim_end_matches("Episode")
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty());

        let episode_number = parser.text("h1.posttl").and_then(|t| {
            EP_NUM_RE
                .captures(&t)
                .and_then(|c| c[1].parse::<f64>().ok())
        });

        // Default embed already in the page.
        let default_embed = parser
            .attr("#pembed iframe", "src")
            .or_else(|| parser.attr(".responsive-embed-stream iframe", "src"))
            .filter(|s| !s.is_empty());

        // Streaming mirrors: `.mirrorstream ul.mXXXp li a[data-content]`.
        let mut mirrors: Vec<AnimeStreamMirror> = Vec::new();
        {
            let a_sel = Selector::parse("li a[data-content]").ok();
            for ul in parser.select_all(".mirrorstream ul") {
                // Quality from the ul class (m360p -> 360p) or its text.
                let quality = ul
                    .value()
                    .attr("class")
                    .and_then(parse_quality_from_class)
                    .or_else(|| {
                        let t = ul.text().collect::<String>();
                        QUALITY_RE.captures(&t).map(|c| c[1].to_string())
                    })
                    .unwrap_or_else(|| "default".to_string());
                if let Some(a_sel) = a_sel.as_ref() {
                    for a in ul.select(a_sel) {
                        let token = match a.value().attr("data-content") {
                            Some(t) if !t.is_empty() => t.to_string(),
                            _ => continue,
                        };
                        let name = a.text().collect::<String>().trim().to_string();
                        let is_default = a.value().attr("data-default") == Some("true");
                        mirrors.push(AnimeStreamMirror {
                            name: if name.is_empty() {
                                "server".to_string()
                            } else {
                                name
                            },
                            quality: quality.clone(),
                            token,
                            default: is_default,
                        });
                    }
                }
            }
        }

        // Downloads: `.download ul li` with `<strong>quality</strong>`,
        // anchors per host, and a trailing `<i>size</i>`.
        let mut downloads: Vec<AnimeDownloadGroup> = Vec::new();
        {
            let strong_sel = Selector::parse("strong").ok();
            let a_sel = Selector::parse("a").ok();
            let i_sel = Selector::parse("i").ok();
            for li in parser.select_all(".download ul li") {
                let quality = strong_sel
                    .as_ref()
                    .and_then(|s| li.select(s).next())
                    .map(|n| n.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();
                if quality.is_empty() {
                    continue;
                }
                let size = i_sel
                    .as_ref()
                    .and_then(|s| li.select(s).next())
                    .map(|n| n.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty());
                let mirrors_dl: Vec<DownloadMirror> = a_sel
                    .as_ref()
                    .map(|s| {
                        li.select(s)
                            .filter_map(|a| {
                                let href = a.value().attr("href")?;
                                let name = a.text().collect::<String>().trim().to_string();
                                if name.is_empty() {
                                    return None;
                                }
                                Some(DownloadMirror {
                                    name,
                                    url: resolve_url(url, href),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if mirrors_dl.is_empty() {
                    continue;
                }
                downloads.push(AnimeDownloadGroup {
                    quality,
                    size,
                    mirrors: mirrors_dl,
                });
            }
        }

        // Prev/next + series link from the episode selector + nav.
        let mut prev_episode = None;
        let mut next_episode = None;
        for a in parser.select_all(".flir a") {
            let label = a.text().collect::<String>().to_lowercase();
            if let Some(href) = a.value().attr("href") {
                let abs = resolve_url(url, href);
                if label.contains("sebelum") || label.contains("prev") {
                    prev_episode = Some(abs);
                } else if label.contains("selanjut") || label.contains("next") {
                    next_episode = Some(abs);
                }
            }
        }
        let series_url = parser
            .select_all(".flir a")
            .iter()
            .find_map(|a| {
                let label = a.text().collect::<String>().to_lowercase();
                if label.contains("list") || label.contains("semua") {
                    a.value().attr("href").map(|h| resolve_url(url, h))
                } else {
                    None
                }
            })
            .or_else(|| {
                // Fall back to any `/anime/...` link on the page.
                parser.select_all("a").iter().find_map(|a| {
                    a.value()
                        .attr("href")
                        .filter(|h| h.contains("/anime/"))
                        .map(|h| resolve_url(url, h))
                })
            });

        Some(AnimeEpisode {
            series_title,
            episode_number,
            default_embed,
            mirrors,
            downloads,
            prev_episode,
            next_episode,
            series_url,
            url: url.to_string(),
        })
    }

    /// Extract the embed iframe `src` from the AJAX-decoded HTML fragment.
    pub fn embed_src_from_fragment(html: &str) -> Option<String> {
        IFRAME_SRC_RE
            .captures(html)
            .map(|c| c[1].to_string())
            .filter(|s| s.starts_with("http"))
    }

    /// Parse a browse / listing page (`/ongoing-anime/`, `/complete-anime/`,
    /// genre pages) into search hits. Two markups are supported:
    ///   * `.venz ul li .detpost` (ongoing/complete grids)
    ///   * `.col-anime` cards (genre pages)
    pub fn parse_browse(base_url: &str, html: &str) -> Vec<SearchHit> {
        let parser = HtmlParser::parse(html);
        let mut out = Vec::new();
        let a_sel = Selector::parse(".thumb a").unwrap();
        let title_sel = Selector::parse("h2.jdlflm").unwrap();
        let img_sel = Selector::parse("img").unwrap();
        let epz_sel = Selector::parse(".epz").unwrap();

        for li in parser.select_all(".venz ul li") {
            let a = match li.select(&a_sel).next() {
                Some(a) => a,
                None => continue,
            };
            let url = match a.value().attr("href") {
                Some(h) => resolve_url(base_url, h),
                None => continue,
            };
            if !url.contains("/anime/") {
                continue;
            }
            let title = li
                .select(&title_sel)
                .next()
                .map(|t| t.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let thumbnail = li.select(&img_sel).next().and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });
            let ep = li
                .select(&epz_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());
            let mut tags = Vec::new();
            if let Some(ep) = ep {
                tags.push(ep);
            }
            out.push(SearchHit {
                title,
                url,
                thumbnail,
                genres: tags,
                status: None,
                rating: None,
            });
        }

        // Genre-page markup: `.col-anime` cards (no thumbnail in the list).
        if out.is_empty() {
            let ca_title = Selector::parse(".col-anime-title a").unwrap();
            let ca_studio = Selector::parse(".col-anime-studio").unwrap();
            let ca_eps = Selector::parse(".col-anime-eps").unwrap();
            let ca_rating = Selector::parse(".col-anime-rating").unwrap();
            let ca_img = Selector::parse(".col-anime-cover img").unwrap();
            for card in parser.select_all(".col-anime") {
                let a = match card.select(&ca_title).next() {
                    Some(a) => a,
                    None => continue,
                };
                let url = match a.value().attr("href") {
                    Some(h) => resolve_url(base_url, h),
                    None => continue,
                };
                if !url.contains("/anime/") {
                    continue;
                }
                let title = a.text().collect::<String>().trim().to_string();
                if title.is_empty() {
                    continue;
                }
                let thumbnail = card.select(&ca_img).next().and_then(|img| {
                    img.value()
                        .attr("data-src")
                        .or_else(|| img.value().attr("src"))
                        .map(|s| resolve_url(base_url, s))
                });
                let mut tags = Vec::new();
                if let Some(eps) = card
                    .select(&ca_eps)
                    .next()
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty())
                {
                    tags.push(eps);
                }
                if let Some(studio) = card
                    .select(&ca_studio)
                    .next()
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty())
                {
                    tags.push(studio);
                }
                let rating = card
                    .select(&ca_rating)
                    .next()
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty());
                out.push(SearchHit {
                    title,
                    url,
                    thumbnail,
                    genres: tags,
                    status: None,
                    rating,
                });
            }
        }

        out
    }
}

/// A lightweight search hit (mapped to the unified search item by the API).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub rating: Option<String>,
}

static QUALITY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{3,4}p)").unwrap());

fn parse_quality_from_class(class: &str) -> Option<String> {
    // "m360p" / "m480p" / "m720p" -> "360p"
    for token in class.split_whitespace() {
        if let Some(rest) = token.strip_prefix('m') {
            if QUALITY_RE.is_match(rest) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Return the text after the first ':' in a label line, trimmed.
fn text_after_colon(s: &str) -> String {
    match s.split_once(':') {
        Some((_, v)) => v.trim().to_string(),
        None => s.trim().to_string(),
    }
}

#[async_trait]
impl SiteAdapter for OtakudesuAdapter {
    fn name(&self) -> &str {
        "otakudesu"
    }

    fn matches(&self, url: &str) -> bool {
        Self::matches_url(url)
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        let mut h = HashMap::new();
        h.insert(
            "Accept-Language".to_string(),
            "id-ID,id;q=0.9,en;q=0.8".to_string(),
        );
        Some(h)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        if Self::is_episode(url) {
            if let Some(ep) = Self::parse_episode(url, html) {
                return Ok(vec![ContentModel::AnimeEpisode(ep)]);
            }
        } else if Self::is_anime_detail(url) {
            if let Some(series) = Self::parse_detail(url, html) {
                return Ok(vec![ContentModel::AnimeSeries(series)]);
            }
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_recognition() {
        assert!(OtakudesuAdapter::matches_url(
            "https://otakudesu.blog/anime/oshi-ko-s3-sub-indo/"
        ));
        assert!(!OtakudesuAdapter::matches_url("https://example.com/"));
    }

    #[test]
    fn search_url_uses_anime_post_type() {
        let u = OtakudesuAdapter::search_url("Oshi no Ko", 1);
        assert!(u.contains("post_type=anime"));
        assert!(u.contains("s=Oshi"));
    }

    #[test]
    fn parse_search_extracts_cards() {
        let html = r##"
            <ul class="chivsrc">
              <li>
                <img src="https://otakudesu.blog/x.jpg" />
                <h2><a href="https://otakudesu.blog/anime/oshi-ko-s3-sub-indo/">Oshi no Ko Season 3 Subtitle Indonesia</a></h2>
                <div class="set"><b>Genres</b> : <a href="#">Drama</a>, <a href="#">Seinen</a></div>
                <div class="set"><b>Status</b> : Ongoing</div>
                <div class="set"><b>Rating</b> : 8.25</div>
              </li>
            </ul>"##;
        let hits = OtakudesuAdapter::parse_search("https://otakudesu.blog/", html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Oshi no Ko Season 3 Subtitle Indonesia");
        assert_eq!(hits[0].genres, vec!["Drama", "Seinen"]);
        assert_eq!(hits[0].status.as_deref(), Some("Ongoing"));
        assert_eq!(hits[0].rating.as_deref(), Some("8.25"));
    }

    #[test]
    fn parse_detail_reads_metadata_and_episodes() {
        let html = r##"
            <div class="fotoanime"><img src="/cover.jpg"></div>
            <div class="infozingle">
              <p><span><b>Judul</b>: Oshi no Ko Season 3</span></p>
              <p><span><b>Japanese</b>: 推しの子</span></p>
              <p><span><b>Skor</b>: 8.25</span></p>
              <p><span><b>Tipe</b>: TV</span></p>
              <p><span><b>Status</b>: Ongoing</span></p>
              <p><span><b>Total Episode</b>: 11</span></p>
              <p><span><b>Studio</b>: Doga Kobo</span></p>
              <p><span><b>Genre</b>: <a href="#">Drama</a>, <a href="#">Seinen</a></span></p>
            </div>
            <div class="sinopc"><p>Sebuah kisah.</p></div>
            <div class="episodelist"><div class="smokelister"><span class="monktit">Batch</span></div><ul></ul></div>
            <div class="episodelist"><div class="smokelister"><span class="monktit">Episode List</span></div>
              <ul>
                <li><span><a href="https://otakudesu.blog/episode/onk-s3-episode-2-sub-indo/">Oshi no Ko Season 3 Episode 2 Subtitle Indonesia</a></span><span class="zeebr">18 Maret,2026</span></li>
                <li><span><a href="https://otakudesu.blog/episode/onk-s3-episode-1-sub-indo/">Oshi no Ko Season 3 Episode 1 Subtitle Indonesia</a></span><span class="zeebr">11 Maret,2026</span></li>
              </ul>
            </div>"##;
        let s = OtakudesuAdapter::parse_detail("https://otakudesu.blog/anime/onk/", html).unwrap();
        assert_eq!(s.title.as_deref(), Some("Oshi no Ko Season 3"));
        assert_eq!(s.score.as_deref(), Some("8.25"));
        assert_eq!(s.anime_type.as_deref(), Some("TV"));
        assert_eq!(s.total_episodes.as_deref(), Some("11"));
        assert_eq!(s.studio.as_deref(), Some("Doga Kobo"));
        assert_eq!(s.genres, vec!["Drama", "Seinen"]);
        assert_eq!(s.synopsis.as_deref(), Some("Sebuah kisah."));
        assert_eq!(s.episodes.len(), 2);
        // ascending by number
        assert_eq!(s.episodes[0].number, Some(1.0));
        assert_eq!(s.episodes[1].number, Some(2.0));
    }

    #[test]
    fn parse_episode_reads_mirrors_and_downloads() {
        let html = r##"
            <h1 class="posttl">Oshi no Ko Season 3 Episode 11 (End) Subtitle Indonesia</h1>
            <div id="pembed"><div class="responsive-embed-stream"><iframe src="https://desustream.info/x"></iframe></div></div>
            <div class="mirrorstream">
              <ul class="m360p"><span>Mirror 360p</span>
                <li><a href="#" data-content="eyJpZCI6MX0=">vidhide</a></li>
              </ul>
              <ul class="m480p"><span>Mirror 480p</span>
                <li><a href="#" data-content="eyJpZCI6Mn0=" data-default="true">ondesu3</a></li>
              </ul>
            </div>
            <div class="download">
              <h4>Episode 11 [Oploverz]</h4>
              <ul>
                <li><strong>Mp4 360p</strong> <a href="https://link.desustream.com/?id=a">ODFiles</a> <a href="https://link.desustream.com/?id=b">Pdrain</a> <i>77.1 MB</i></li>
              </ul>
            </div>"##;
        let e = OtakudesuAdapter::parse_episode(
            "https://otakudesu.blog/episode/onk-s3-episode-11-sub-indo/",
            html,
        )
        .unwrap();
        assert_eq!(e.episode_number, Some(11.0));
        assert_eq!(
            e.default_embed.as_deref(),
            Some("https://desustream.info/x")
        );
        assert_eq!(e.mirrors.len(), 2);
        assert_eq!(e.mirrors[0].quality, "360p");
        assert_eq!(e.mirrors[0].name, "vidhide");
        assert!(e.mirrors[1].default);
        assert_eq!(e.downloads.len(), 1);
        assert_eq!(e.downloads[0].quality, "Mp4 360p");
        assert_eq!(e.downloads[0].size.as_deref(), Some("77.1 MB"));
        assert_eq!(e.downloads[0].mirrors.len(), 2);
    }
}
