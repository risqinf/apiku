//! Domain models used across the scraping engine and the public API.
//!
//! `ContentModel` is the tagged enum each adapter returns; downstream code
//! either renders it directly (CLI mode) or maps it to the public DTOs in
//! `api.rs`. Auxiliary structs (`MangaSeries`, `DonghuaEpisode`, `CosplayPost`,
//! ...) are the per-kind data shapes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unified content model that all site adapters produce
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)] // DeepPage is intentionally feature-rich
pub enum ContentModel {
    #[serde(rename = "wordpress_post")]
    WordPressPost(WordPressPost),

    #[serde(rename = "manga_series")]
    MangaSeries(MangaSeries),

    #[serde(rename = "manga_chapter")]
    MangaChapter(MangaChapter),

    #[serde(rename = "donghua_series")]
    DonghuaSeries(DonghuaSeries),

    #[serde(rename = "donghua_episode")]
    DonghuaEpisode(DonghuaEpisode),

    /// Anime series (e.g. otakudesu.blog)
    #[serde(rename = "anime_series")]
    AnimeSeries(AnimeSeries),

    /// Anime episode (streaming mirrors + downloads)
    #[serde(rename = "anime_episode")]
    AnimeEpisode(AnimeEpisode),

    #[serde(rename = "cosplay_post")]
    CosplayPost(CosplayPost),

    /// Light novel / web novel series (e.g. novelid.org)
    #[serde(rename = "novel_series")]
    NovelSeries(NovelSeries),

    /// Light novel / web novel chapter (text body)
    #[serde(rename = "novel_chapter")]
    NovelChapter(NovelChapter),

    #[serde(rename = "generic")]
    Generic(GenericContent),

    #[serde(rename = "deep_page")]
    DeepPage(DeepPage),

    /// Raw JSON response from an API endpoint
    #[serde(rename = "json_api")]
    JsonApi(JsonApiResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonApiResponse {
    pub url: String,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WordPressPost {
    pub title: Option<String>,
    pub content: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub categories: Vec<String>,
    pub featured_image: Option<String>,
    pub media: Vec<MediaItem>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaItem {
    pub url: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MangaSeries {
    pub title: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub genres: Vec<String>,
    pub synopsis: Option<String>,
    pub cover_image: Option<String>,
    pub chapters: Vec<ChapterInfo>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChapterInfo {
    pub number: f64,
    pub title: Option<String>,
    pub url: String,
    /// Optional list of additional language translations (some sites have multiple)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<ChapterTranslation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChapterTranslation {
    pub url: String,
    pub language: Option<String>,
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MangaChapter {
    pub series_title: Option<String>,
    pub chapter_number: f64,
    pub pages: Vec<PageImage>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageImage {
    pub index: usize,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DonghuaSeries {
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub thumbnail: Option<String>,
    pub episodes: Vec<EpisodeInfo>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeInfo {
    pub number: u32,
    pub title: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DonghuaEpisode {
    pub series_title: Option<String>,
    pub episode_number: u32,
    pub sources: Vec<VideoSource>,
    /// Download links grouped by quality (e.g., "720p" -> [Mirrored, Terabox, ...])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downloads: Vec<DownloadGroup>,
    /// Previous episode URL (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_episode: Option<String>,
    /// Next episode URL (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_episode: Option<String>,
    /// URL of the parent series page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_url: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadGroup {
    /// Quality label e.g. "360p", "720p", "1080p"
    pub quality: String,
    pub mirrors: Vec<DownloadMirror>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadMirror {
    /// Mirror name (e.g., "Mirrored", "Terabox", "Mediafire")
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoSource {
    pub url: String,
    pub quality: Option<String>,
    pub format: Option<String>,
}

/// Anime series detail (e.g. otakudesu.blog).
///
/// Carries the rich metadata block otakudesu renders (judul, japanese title,
/// score, producer, type, status, total episodes, duration, release date,
/// studio) plus the episode list and any "batch" download entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimeSeries {
    pub title: Option<String>,
    pub japanese_title: Option<String>,
    pub synopsis: Option<String>,
    pub thumbnail: Option<String>,
    pub score: Option<String>,
    pub producer: Option<String>,
    pub anime_type: Option<String>,
    pub status: Option<String>,
    pub total_episodes: Option<String>,
    pub duration: Option<String>,
    pub release_date: Option<String>,
    pub studio: Option<String>,
    pub genres: Vec<String>,
    pub episodes: Vec<AnimeEpisodeRef>,
    /// "Batch" download entries (whole-season archives), if present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batch: Vec<AnimeEpisodeRef>,
    pub url: String,
}

/// One episode entry within a series listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimeEpisodeRef {
    /// Parsed episode number when detectable (None for batch/specials).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<f64>,
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub url: String,
}

/// A streaming mirror for an anime episode (one quality + host combo).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimeStreamMirror {
    /// Host/provider label (e.g. "vidhide", "ondesu3", "mega").
    pub name: String,
    /// Quality bucket label (e.g. "360p", "480p", "720p").
    pub quality: String,
    /// Opaque token (base64 of `{id,i,q}`) the API resolves to an embed URL.
    pub token: String,
    /// True when this is the page's default (pre-loaded) mirror.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// Anime episode detail (e.g. otakudesu.blog episode page).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimeEpisode {
    pub series_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<f64>,
    /// The default embed URL already present in the page (ready to play).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_embed: Option<String>,
    /// All streaming mirrors (resolved on demand via the API).
    pub mirrors: Vec<AnimeStreamMirror>,
    /// Download links grouped by quality.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downloads: Vec<AnimeDownloadGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_episode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_episode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_url: Option<String>,
    pub url: String,
}

/// Download group for an anime episode: a source label + per-quality mirrors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimeDownloadGroup {
    /// Quality label (e.g. "Mp4 360p", "Mkv 720p").
    pub quality: String,
    /// Optional file size string (e.g. "77.1 MB").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    pub mirrors: Vec<DownloadMirror>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenericContent {
    pub title: Option<String>,
    pub body: Option<String>,
    pub url: String,
    pub metadata: HashMap<String, String>,
}

/// Cosplay/photoset post (e.g. cosplaytele.com)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CosplayPost {
    pub title: Option<String>,
    /// Cosplayer name(s)
    pub cosplayer: Option<String>,
    /// Character being cosplayed
    pub character: Option<String>,
    /// Source series / game / franchise
    pub series: Option<String>,
    /// Photo count parsed from title (e.g. "23 photos")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo_count: Option<u32>,
    /// Video count parsed from title (e.g. "1 video")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_count: Option<u32>,
    /// All gallery image URLs in display order
    pub images: Vec<String>,
    /// Direct video URLs found in the post
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub videos: Vec<String>,
    /// Categories assigned to the post
    pub categories: Vec<String>,
    /// Tags assigned to the post
    pub tags: Vec<String>,
    /// Author / poster name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// ISO 8601 publication date
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Featured / cover image (usually first photo)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_image: Option<String>,
    /// External download links (e.g. Mediafire, Telegram, Gofile)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub download_links: Vec<DownloadMirror>,
    /// Unzip password if shown on the post
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unzip_password: Option<String>,
    pub url: String,
}

/// Light novel / web novel series metadata (novelid.org and similar).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NovelSeries {
    pub title: Option<String>,
    pub author: Option<String>,
    pub status: Option<String>,
    pub genres: Vec<String>,
    pub synopsis: Option<String>,
    pub cover_image: Option<String>,
    /// Optional rating like "8.00"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    pub chapters: Vec<NovelChapterRef>,
    /// True when the upstream paginates the chapter list (each fetch returns
    /// only a window of chapters — so a single scrape does not give the full
    /// list).
    #[serde(default)]
    pub chapters_paginated_upstream: bool,
    /// Number of chapters returned per upstream page (typically 30).
    /// Only meaningful when `chapters_paginated_upstream` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_chapters_per_page: Option<u32>,
    /// Total number of upstream pages of the chapter list.
    /// Only meaningful when `chapters_paginated_upstream` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_total_pages: Option<u32>,
    pub url: String,
}

/// One chapter entry in a novel's chapter list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NovelChapterRef {
    pub number: u32,
    pub title: Option<String>,
    pub url: String,
}

/// Single novel chapter with full text body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NovelChapter {
    pub series_title: Option<String>,
    pub chapter_number: u32,
    pub chapter_title: Option<String>,
    /// Plain text content (paragraphs joined with double newlines)
    pub body: String,
    /// HTML content as scraped, sanitised (stripped of scripts/ads)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_url: Option<String>,
    pub url: String,
}

/// Deep page extraction — comprehensive data captured from any HTML page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeepPage {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub canonical: Option<String>,
    pub language: Option<String>,
    pub is_spa: bool,
    pub status_code: u16,
    pub og: HashMap<String, String>,
    pub meta: HashMap<String, String>,
    pub json_ld: Vec<serde_json::Value>,
    pub headings: Vec<Heading>,
    pub links: Vec<LinkRef>,
    pub images: Vec<ImageRef>,
    pub media: Vec<MediaRef>,
    pub scripts: Vec<String>,
    pub stylesheets: Vec<String>,
    pub api_endpoints: Vec<String>,
    pub inline_json: Vec<serde_json::Value>,
    pub forms: Vec<FormRef>,
    pub text_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkRef {
    pub url: String,
    pub text: Option<String>,
    pub rel: Option<String>,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageRef {
    pub url: String,
    pub alt: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub srcset: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaRef {
    pub url: String,
    pub kind: String, // "video", "audio", "iframe", "embed"
    pub mime_type: Option<String>,
    pub poster: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormRef {
    pub action: Option<String>,
    pub method: String,
    pub fields: Vec<String>,
}

/// Result of a scraping operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResult {
    pub url: String,
    pub success: bool,
    pub adapter_used: Option<String>,
    /// Specialized adapter content (manga/donghua/wordpress/json_api)
    pub content: Option<ContentModel>,
    /// Deep extraction of EVERYTHING in the page (HTML responses only)
    pub deep: Option<DeepPage>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}
