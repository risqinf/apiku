use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ContentModel, DonghuaEpisode, DonghuaSeries, EpisodeInfo, VideoSource};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static QUALITY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{3,4}p)").unwrap());
static EP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:episode|ep|eps)[\s._-]*(\d+)").unwrap());

static JS_VIDEO_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(
            r#"(?:src|file|url|source)\s*[:=]\s*['"](https?://[^'"]+\.(?:mp4|m3u8|mpd))['"]"#,
        )
        .unwrap(),
        Regex::new(r#"(?:video_url|stream_url|play_url)\s*[:=]\s*['"](https?://[^'"]+)['"]"#)
            .unwrap(),
        Regex::new(r#"sources\s*:\s*\[\s*\{\s*(?:file|src)\s*:\s*['"](https?://[^'"]+)['"]"#)
            .unwrap(),
    ]
});

/// Donghua/Anime site adapter
pub struct DonghuaAdapter {
    /// Known donghua/anime site domains
    known_domains: Vec<&'static str>,
}

impl DonghuaAdapter {
    pub fn new() -> Self {
        Self {
            known_domains: vec![
                "donghua",
                "anime",
                "anoboy",
                "otakudesu",
                "samehadaku",
                "neonime",
                "kusonime",
                "oploverz",
                "animeindo",
                "nanime",
                "gomunime",
                "animasu",
                "kuramanime",
                "anitoki",
                "donghuastream",
                "animexin",
                "luciferdonghua",
            ],
        }
    }

    /// Detect if the page is an episode page or a series page
    fn is_episode_page(&self, html: &str) -> bool {
        let parser = HtmlParser::parse(html);
        // Episode pages typically have video players or download links
        let has_video = parser
            .select_one("video, iframe[src*='player'], .player-embed, #pembed")
            .is_some();
        let has_download = parser
            .select_one(".download-eps, .download-link, .smokeurl")
            .is_some();
        has_video || has_download
    }

    fn extract_series(&self, url: &str, html: &str) -> DonghuaSeries {
        let parser = HtmlParser::parse(html);

        // Title
        let title = parser
            .text(".entry-title")
            .or_else(|| parser.text("h1.title"))
            .or_else(|| parser.text(".anime-title"))
            .or_else(|| parser.text("h1"))
            .map(|t| truncate(&t, 500));

        // Synopsis
        let synopsis = parser
            .text(".entry-content p")
            .or_else(|| parser.text(".synp p"))
            .or_else(|| parser.text(".synopsis"))
            .or_else(|| parser.text(".anime-synopsis"))
            .or_else(|| parser.text(".desc"))
            .map(|s| truncate(&s, 5000));

        // Genres
        let genres = parser.texts(".genre-info a, .genxed a, .mgen a, .genrez a");
        let genres: Vec<String> = genres.into_iter().take(20).collect();

        // Status
        let status_text = parser
            .text(".status")
            .or_else(|| parser.text(".anime-status"))
            .or_else(|| parser.text("span:contains('Status') + span"));

        let status = status_text.map(|s| {
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

        // Thumbnail
        let thumbnail = parser
            .attr(".thumb img", "src")
            .or_else(|| parser.attr(".anime-thumb img", "src"))
            .or_else(|| parser.attr(".poster img", "src"))
            .or_else(|| parser.attr("img.wp-post-image", "src"))
            .map(|u| resolve_url(url, &u));

        // Episodes
        let episodes = self.extract_episode_list(&parser, url);

        DonghuaSeries {
            title,
            synopsis,
            genres,
            status,
            thumbnail,
            episodes,
            url: url.to_string(),
        }
    }

    fn extract_episode_list(&self, parser: &HtmlParser, base_url: &str) -> Vec<EpisodeInfo> {
        let mut episodes = Vec::new();

        // Try common episode list selectors
        let ep_elements = parser
            .select_all(".eplister a, .episodelist a, .eps-list a, .listeps a, ul.episodios a");

        for (idx, el) in ep_elements.iter().enumerate() {
            let href = el.value().attr("href").unwrap_or("").to_string();
            let text = el.text().collect::<Vec<_>>().join("").trim().to_string();

            let episode_url = resolve_url(base_url, &href);

            // Extract episode number
            let number = if let Some(caps) = EP_RE.captures(&text) {
                caps[1].parse::<u32>().unwrap_or(idx as u32 + 1)
            } else if let Some(caps) = EP_RE.captures(&href) {
                caps[1].parse::<u32>().unwrap_or(idx as u32 + 1)
            } else {
                idx as u32 + 1
            };

            let title = if text.is_empty() {
                None
            } else {
                Some(truncate(&text, 300))
            };

            episodes.push(EpisodeInfo {
                number,
                title,
                url: episode_url,
            });
        }

        // Sort by episode number ascending
        episodes.sort_by_key(|e| e.number);
        episodes
    }

    fn extract_episode(&self, url: &str, html: &str) -> DonghuaEpisode {
        let parser = HtmlParser::parse(html);

        // Series title from breadcrumb
        let series_title = parser
            .text(".breadcrumb li:nth-child(2) a")
            .or_else(|| parser.text("ol.breadcrumb a:nth-child(2)"))
            .or_else(|| parser.text(".anime-title"));

        // Episode number
        let episode_number = EP_RE
            .captures(url)
            .and_then(|c| c[1].parse::<u32>().ok())
            .or_else(|| {
                let title = parser.text("h1").unwrap_or_default();
                EP_RE
                    .captures(&title)
                    .and_then(|c| c[1].parse::<u32>().ok())
            })
            .unwrap_or(1);
        // Extract video sources
        let sources = self.extract_video_sources(&parser, html, url);

        DonghuaEpisode {
            series_title,
            episode_number,
            sources,
            downloads: Vec::new(),
            prev_episode: None,
            next_episode: None,
            series_url: None,
            url: url.to_string(),
        }
    }

    fn extract_video_sources(
        &self,
        parser: &HtmlParser,
        html: &str,
        base_url: &str,
    ) -> Vec<VideoSource> {
        let mut sources = Vec::new();

        // 1. Direct video source elements
        for src in parser.attrs("video source", "src") {
            let quality = parser
                .attr(&format!("source[src='{}']", src), "label")
                .or_else(|| {
                    parser
                        .attr(&format!("source[src='{}']", src), "size")
                        .map(|s| format!("{}p", s))
                });
            sources.push(VideoSource {
                url: resolve_url(base_url, &src),
                quality,
                format: Some("mp4".to_string()),
            });
        }

        // 2. Iframe embeds (player URLs)
        for src in parser.attrs("iframe", "src") {
            if src.contains("player") || src.contains("embed") || src.contains("video") {
                sources.push(VideoSource {
                    url: resolve_url(base_url, &src),
                    quality: None,
                    format: Some("embed".to_string()),
                });
            }
        }

        // 3. Extract from JavaScript variables using regex
        for re in JS_VIDEO_PATTERNS.iter() {
            for caps in re.captures_iter(html) {
                if let Some(url_match) = caps.get(1) {
                    let video_url = url_match.as_str().to_string();
                    let format = if video_url.contains(".m3u8") {
                        Some("hls".to_string())
                    } else if video_url.contains(".mpd") {
                        Some("dash".to_string())
                    } else {
                        Some("mp4".to_string())
                    };

                    // Try to extract quality from nearby context
                    let quality = extract_quality_from_context(html, url_match.start());

                    sources.push(VideoSource {
                        url: video_url,
                        quality,
                        format,
                    });
                }
            }
        }

        // 4. Download links (common in Indonesian anime sites)
        let download_links = parser.select_all(".smokeurl a, .download-eps a, .download-link a");
        for el in download_links {
            if let Some(href) = el.value().attr("href") {
                let text = el.text().collect::<Vec<_>>().join("");
                let quality = QUALITY_RE.captures(&text).map(|c| c[1].to_string());

                if href.contains("mp4") || href.contains("mkv") || quality.is_some() {
                    sources.push(VideoSource {
                        url: resolve_url(base_url, href),
                        quality,
                        format: Some("mp4".to_string()),
                    });
                }
            }
        }

        sources
    }
}

/// Extract quality label from surrounding JavaScript context
fn extract_quality_from_context(html: &str, pos: usize) -> Option<String> {
    let start = pos.saturating_sub(200);
    let context = &html[start..pos];
    QUALITY_RE.captures(context).map(|c| c[1].to_string())
}

/// Truncate a string to max_len characters
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        s.chars().take(max_len).collect()
    }
}

#[async_trait]
impl SiteAdapter for DonghuaAdapter {
    fn name(&self) -> &str {
        "donghua"
    }

    fn matches(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        self.known_domains.iter().any(|d| lower.contains(d))
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        let mut headers = HashMap::new();
        headers.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string(),
        );
        Some(headers)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        if self.is_episode_page(html) {
            let episode = self.extract_episode(url, html);
            Ok(vec![ContentModel::DonghuaEpisode(episode)])
        } else {
            let series = self.extract_series(url, html);
            Ok(vec![ContentModel::DonghuaSeries(series)])
        }
    }
}
