//! RESTful API v1.
//!
//! Every endpoint returns the same JSON envelope:
//!
//! ```json
//! {
//!   "status": 200,
//!   "ok": true,
//!   "data": { ... },
//!   "meta": { "took_ms": 123, "cached": false, "request_id": "1f8b2c4d-..." }
//! }
//! ```
//!
//! Errors share the shape with `ok: false` and an `error` object:
//!
//! ```json
//! { "status": 404, "ok": false, "error": { "code": "not_found", "message": "..." }, "meta": { ... } }
//! ```
//!
//! ## Endpoint families
//!
//! - `health` / `info`                          - liveness + server metadata
//! - `search`                                   - cross-provider search
//! - `browse`                                   - per-provider home / popular / latest feeds
//! - `manga` / `donghua` / `novel` / `nhentai`  - series detail (with paged chapter list)
//! - `manga/chapter` / `donghua/episode`
//!   `novel/chapter` / `nhentai/chapter`        - leaf content (pages, video servers, text body)
//! - `cosplay`                                  - photoset / gallery post
//! - `img`                                      - HMAC-signed image proxy
//!
//! ## Resource IDs
//!
//! All IDs are opaque, HMAC-SHA256-signed tokens. See `opaque.rs` for the wire format.
//! Image URLs in responses are rewritten to `/img?p=...&s=...` (see `img_proxy`).

use crate::engine::ScraperEngine;
use crate::fingerprint::BrowserFingerprint;
use crate::models::{
    AnimeEpisode, AnimeSeries, ChapterInfo, ContentModel, CosplayPost, DonghuaEpisode,
    DonghuaSeries, EpisodeInfo, MangaChapter, MangaSeries, MovieDetail, NovelChapter,
    NovelChapterRef, NovelSeries, PageImage, ScrapeResult,
};
use crate::opaque::{Kind, OpaqueCodec, OpaqueError, Source};
use crate::web::search::{
    build_search_url, mangaball_search_endpoint, parse_mangaball_search, parse_nhentai_search,
    parse_search_html, SearchResultItem, SearchSource,
};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use moka::future::Cache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// Application state shared across all axum handlers.
#[derive(Clone)]
pub struct ApiState {
    pub engine: Arc<ScraperEngine>,
    pub codec: Arc<OpaqueCodec>,
    pub cache: Cache<String, Arc<ScrapeResult>>,
    pub search_cache: Cache<String, Arc<SearchEnvelopeData>>,
    /// Cache of proxied image bytes (content-type, body) keyed by upstream URL,
    /// so re-viewing a feed/detail serves images instantly instead of
    /// re-fetching each one from the source CDN.
    pub img_cache: Cache<String, Arc<(String, Vec<u8>)>>,
    pub started_at: Instant,
    pub sysspec: crate::sysspec::SysSpec,
    /// Consumer web app branding / customization.
    pub web: Arc<crate::config::WebConfig>,
}

impl ApiState {
    pub fn new(
        engine: ScraperEngine,
        codec: OpaqueCodec,
        sysspec: crate::sysspec::SysSpec,
        web: crate::config::WebConfig,
    ) -> Self {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(600))
            .max_capacity(sysspec.scrape_cache_capacity())
            .build();
        let search_cache = Cache::builder()
            .time_to_live(Duration::from_secs(300))
            .max_capacity(sysspec.search_cache_capacity())
            .build();
        // Image bytes cache: many entries, capped, with a longer TTL since
        // upstream art rarely changes. Capacity scales with the scrape cache.
        let img_cache = Cache::builder()
            .time_to_live(Duration::from_secs(86_400))
            .max_capacity(sysspec.scrape_cache_capacity().saturating_mul(2).max(2_000))
            .build();
        Self {
            engine: Arc::new(engine),
            codec: Arc::new(codec),
            cache,
            search_cache,
            img_cache,
            started_at: Instant::now(),
            sysspec,
            web: Arc::new(web),
        }
    }
}

// ---------------------------------------------------------------------------
// Response envelopes
// ---------------------------------------------------------------------------

/// Successful response wrapper.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub status: u16,
    pub ok: bool,
    pub data: T,
    pub meta: ResponseMeta,
}

/// Error response wrapper.
#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub status: u16,
    pub ok: bool,
    pub error: ApiError,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ResponseMeta {
    pub took_ms: u64,
    pub cached: bool,
    pub request_id: String,
}

/// Build a successful JSON response with the given HTTP status code.
fn ok<T: Serialize>(
    status: StatusCode,
    data: T,
    started: Instant,
    cached: bool,
    req_id: &str,
) -> Response {
    let body = Envelope {
        status: status.as_u16(),
        ok: true,
        data,
        meta: ResponseMeta {
            took_ms: started.elapsed().as_millis() as u64,
            cached,
            request_id: req_id.to_string(),
        },
    };
    (status, Json(body)).into_response()
}

/// Build an error JSON response.
fn err(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    started: Instant,
    req_id: &str,
) -> Response {
    let body = ErrorEnvelope {
        status: status.as_u16(),
        ok: false,
        error: ApiError {
            code: code.to_string(),
            message: message.into(),
        },
        meta: ResponseMeta {
            took_ms: started.elapsed().as_millis() as u64,
            cached: false,
            request_id: req_id.to_string(),
        },
    };
    (status, Json(body)).into_response()
}

/// Extract the request id from the X-Request-Id header (set by middleware).
fn req_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

// ---------------------------------------------------------------------------
// Public DTOs (what consumers see)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MangaSeriesDto {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub genres: Vec<String>,
    pub cover: Option<String>,
    pub chapter_count: usize,
    pub chapter_page: u32,
    pub chapter_page_size: u32,
    pub chapter_total_pages: u32,
    pub chapters: Vec<MangaChapterRef>,
}

#[derive(Debug, Serialize)]
pub struct MangaChapterRef {
    pub id: String,
    pub number: f64,
    pub title: Option<String>,
    pub translations: Vec<MangaTranslationRef>,
}

#[derive(Debug, Serialize)]
pub struct MangaTranslationRef {
    pub id: String,
    pub language: Option<String>,
    pub group: Option<String>,
    pub date: Option<String>,
    pub pages: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct MangaChapterDto {
    pub id: String,
    pub series_title: Option<String>,
    pub chapter_number: f64,
    pub page_count: usize,
    pub pages: Vec<MangaPageDto>,
}

#[derive(Debug, Serialize)]
pub struct MangaPageDto {
    pub index: usize,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct DonghuaSeriesDto {
    pub id: String,
    pub title: String,
    pub synopsis: Option<String>,
    pub status: Option<String>,
    pub genres: Vec<String>,
    pub cover: Option<String>,
    pub episode_count: usize,
    pub episode_page: u32,
    pub episode_page_size: u32,
    pub episode_total_pages: u32,
    pub episodes: Vec<DonghuaEpisodeRef>,
}

#[derive(Debug, Serialize)]
pub struct DonghuaEpisodeRef {
    pub id: String,
    pub number: u32,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DonghuaEpisodeDto {
    pub id: String,
    pub series_title: Option<String>,
    pub series_id: Option<String>,
    pub episode_number: u32,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    pub servers: Vec<DonghuaServer>,
    pub downloads: Vec<DownloadGroupDto>,
}

#[derive(Debug, Serialize)]
pub struct DonghuaServer {
    pub label: String,
    pub embed_url: String,
    pub format: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DownloadGroupDto {
    pub quality: String,
    pub mirrors: Vec<DownloadMirrorDto>,
}

#[derive(Debug, Serialize)]
pub struct DownloadMirrorDto {
    pub name: String,
    pub url: String,
}

// ---- Donghua schedule DTOs ----

#[derive(Debug, Serialize)]
pub struct ScheduleDto {
    pub days: Vec<ScheduleDayDto>,
}

#[derive(Debug, Serialize)]
pub struct ScheduleDayDto {
    pub day: String,
    pub items: Vec<ScheduleItemDto>,
}

#[derive(Debug, Serialize)]
pub struct ScheduleItemDto {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_at: Option<i64>,
}

// ---- Anime (otakudesu) DTOs ----

#[derive(Debug, Serialize)]
pub struct AnimeSeriesDto {
    pub id: String,
    pub title: String,
    pub japanese_title: Option<String>,
    pub synopsis: Option<String>,
    pub cover: Option<String>,
    pub score: Option<String>,
    pub producer: Option<String>,
    pub anime_type: Option<String>,
    pub status: Option<String>,
    pub total_episodes: Option<String>,
    pub duration: Option<String>,
    pub release_date: Option<String>,
    pub studio: Option<String>,
    pub genres: Vec<String>,
    pub episode_count: usize,
    pub episodes: Vec<AnimeEpisodeRefDto>,
    pub batch: Vec<AnimeEpisodeRefDto>,
}

#[derive(Debug, Serialize)]
pub struct AnimeEpisodeRefDto {
    pub id: String,
    pub number: Option<f64>,
    pub title: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnimeEpisodeDto {
    pub id: String,
    pub series_title: Option<String>,
    pub series_id: Option<String>,
    pub episode_number: Option<f64>,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    /// Default embed URL ready to play (already present in the page).
    pub default_embed: Option<String>,
    /// Streaming mirrors; resolve a `stream_id` via `/api/v1/anime/stream`.
    pub mirrors: Vec<AnimeMirrorDto>,
    pub downloads: Vec<AnimeDownloadGroupDto>,
}

#[derive(Debug, Serialize)]
pub struct AnimeMirrorDto {
    pub name: String,
    pub quality: String,
    /// Signed token resolved to an embed URL by `/api/v1/anime/stream`.
    pub stream_id: String,
    pub default: bool,
}

#[derive(Debug, Serialize)]
pub struct AnimeDownloadGroupDto {
    pub quality: String,
    pub size: Option<String>,
    pub mirrors: Vec<DownloadMirrorDto>,
}

#[derive(Debug, Serialize)]
pub struct CosplayPostDto {
    pub id: String,
    pub title: String,
    pub cosplayer: Option<String>,
    pub character: Option<String>,
    pub series: Option<String>,
    pub photo_count: Option<u32>,
    pub video_count: Option<u32>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub author: Option<String>,
    pub published_at: Option<String>,
    pub cover: Option<String>,
    pub images: Vec<String>,
    pub videos: Vec<String>,
    pub downloads: Vec<DownloadMirrorDto>,
    pub unzip_password: Option<String>,
    /// "Suggestions for you" — related cosplay posts, ready to render as cards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<SearchItemDto>,
}

#[derive(Debug, Serialize)]
pub struct NovelSeriesDto {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub status: Option<String>,
    pub genres: Vec<String>,
    pub synopsis: Option<String>,
    pub cover: Option<String>,
    pub rating: Option<String>,
    pub chapter_count: usize,
    pub chapter_page: u32,
    pub chapter_page_size: u32,
    pub chapter_total_pages: u32,
    pub chapters: Vec<NovelChapterRefDto>,
}

#[derive(Debug, Serialize)]
pub struct NovelChapterRefDto {
    pub id: String,
    pub number: u32,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NovelChapterDto {
    pub id: String,
    pub series_title: Option<String>,
    pub series_id: Option<String>,
    pub chapter_number: u32,
    pub chapter_title: Option<String>,
    pub body: String,
    pub body_html: Option<String>,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    pub word_count: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchEnvelopeData {
    pub query: String,
    pub source: String,
    pub page: u32,
    /// Nominal items-per-page used for pagination math.
    pub per_page: u32,
    /// Number of items on *this* page (kept for back-compat with chip counts).
    pub total: usize,
    /// Total number of pages upstream when known; `0` means unknown.
    pub total_pages: u32,
    /// Whether another page is expected after this one.
    pub has_next: bool,
    pub items: Vec<SearchItemDto>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchItemDto {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub snippet: Option<String>,
    pub tags: Vec<String>,
    /// Cosplayer name (present only for cosplay results that split cleanly)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosplayer: Option<String>,
    /// Character name (present only for cosplay results that split cleanly)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// Series/franchise name (present only for cosplay results when known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
}

// ---- Suggest (live search suggestions) DTOs ----

#[derive(Debug, Serialize, Clone)]
pub struct SuggestDto {
    pub query: String,
    pub suggestions: Vec<SuggestionItemDto>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SuggestionItemDto {
    #[serde(rename = "type")]
    pub r#type: String,
    pub label: String,
    pub value: String,
    pub source: String,
    pub kind: String,
    /// Opaque id — set only for `title` suggestions that map to a concrete
    /// result so the client can deep-link. Omitted for `tag` suggestions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InfoDto {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub uptime_s: u64,
    pub system: SystemInfoDto,
    pub providers: Vec<ProviderDto>,
    pub endpoints: Vec<EndpointDoc>,
}

#[derive(Debug, Serialize)]
pub struct SystemInfoDto {
    pub cpu_cores: usize,
    pub total_mem_mib: u64,
    pub avail_mem_mib: u64,
    pub profile: &'static str,
    pub tokio_threads: usize,
    pub http_concurrency: usize,
    pub scrape_cache_capacity: u64,
    pub search_cache_capacity: u64,
}

#[derive(Debug, Serialize)]
pub struct ProviderDto {
    pub source: &'static str,
    pub kind: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Serialize)]
pub struct EndpointDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_opaque(
    state: &ApiState,
    opaque: &str,
) -> Result<crate::opaque::DecodedOpaque, OpaqueError> {
    state.codec.decode(opaque)
}

fn proxy_url(state: &ApiState, raw: &str) -> String {
    if raw.is_empty() || raw.starts_with("data:") {
        return String::new();
    }
    let payload = URL_SAFE_NO_PAD.encode(raw.as_bytes());
    let sig = state.codec.sign_image(&payload);
    format!("/img?p={}&s={}", payload, sig)
}

fn proxy_opt(state: &ApiState, raw: Option<&str>) -> Option<String> {
    raw.map(|u| proxy_url(state, u)).filter(|u| !u.is_empty())
}

/// Get a scrape result via the engine + cache, with single-flight coalescing.
async fn cached_scrape(state: &ApiState, url: &str) -> Result<(Arc<ScrapeResult>, bool), String> {
    let key = format!("scrape:{}", url);
    let already_cached = state.cache.get(&key).await.is_some();

    let url_owned = url.to_string();
    let engine = state.engine.clone();
    let arc = state
        .cache
        .try_get_with(key, async move {
            let results = engine
                .scrape_all(&[url_owned])
                .await
                .map_err(|e| e.to_string())?;
            let r = results
                .into_iter()
                .next()
                .ok_or_else(|| "no result".to_string())?;
            if !r.success {
                return Err(r
                    .error
                    .clone()
                    .unwrap_or_else(|| "scrape failed".to_string()));
            }
            Ok(Arc::new(r))
        })
        .await
        .map_err(|e: Arc<String>| (*e).clone())?;
    Ok((arc, already_cached))
}

// ---------------------------------------------------------------------------
// Endpoint handlers
// ---------------------------------------------------------------------------

pub async fn health(req: Request) -> Response {
    let started = Instant::now();
    let id = req_id(req.headers());
    ok(
        StatusCode::OK,
        serde_json::json!({"status": "healthy"}),
        started,
        false,
        &id,
    )
}

pub async fn info(State(state): State<ApiState>, req: Request) -> Response {
    let started = Instant::now();
    let id = req_id(req.headers());
    let s = state.sysspec;

    let info = InfoDto {
        name: "apiku",
        version: env!("CARGO_PKG_VERSION"),
        description: env!("CARGO_PKG_DESCRIPTION"),
        uptime_s: state.started_at.elapsed().as_secs(),
        system: SystemInfoDto {
            cpu_cores: s.cpu_cores,
            total_mem_mib: s.total_mem_mib,
            avail_mem_mib: s.avail_mem_mib,
            profile: s.profile(),
            tokio_threads: s.worker_threads(),
            http_concurrency: s.http_concurrency(),
            scrape_cache_capacity: s.scrape_cache_capacity(),
            search_cache_capacity: s.search_cache_capacity(),
        },
        providers: vec![
            ProviderDto {
                source: "mangaball",
                kind: "manga",
                label: "Mangaball - manga, manhwa, manhua (global database)",
            },
            ProviderDto {
                source: "anichin",
                kind: "donghua",
                label: "Anichin - donghua streaming with Indonesian subs",
            },
            ProviderDto {
                source: "otakudesu",
                kind: "anime",
                label: "Otakudesu - anime streaming with Indonesian subs",
            },
            ProviderDto {
                source: "cosplaytele",
                kind: "cosplay",
                label: "Cosplaytele - cosplay photoset archive",
            },
            ProviderDto {
                source: "nhentai",
                kind: "doujin",
                label: "nhentai - doujinshi catalogue (multi-mirror)",
            },
            ProviderDto {
                source: "novelid",
                kind: "novel",
                label: "NovelID - Indonesian novel translations",
            },
        ],
        endpoints: api_endpoint_docs(),
    };

    ok(StatusCode::OK, info, started, false, &id)
}

pub fn api_endpoint_docs() -> Vec<EndpointDoc> {
    vec![
        EndpointDoc {
            method: "GET",
            path: "/api/v1/health",
            description: "Liveness probe",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/info",
            description: "Server info, system tuning, providers, endpoints",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/search?q={query}&source={all|manga|donghua|cosplay|nhentai|novel}&page={n}",
            description: "Cross-provider search (paginated: returns page/per_page/total_pages/has_next). nhentai accepts `[tag]` syntax in q.",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/suggest?q={query}&source={all|provider}",
            description: "Live search suggestions (type-ahead): catalog-derived tag + title suggestions. Best-effort; empty/failed lookups return an empty list.",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/browse/{provider}?feed={feed}&page={n}&size={N}",
            description: "Provider home / popular / latest feed. Providers: mangaball | anichin | cosplaytele | nhentai | novelid",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/manga/{id}?page={n}&size={N}",
            description: "Manga series detail (Mangaball). Chapter list paginated (default 60/page, max 300).",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/manga/chapter/{id}",
            description: "Manga chapter pages",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/donghua/{id}?page={n}&size={N}",
            description: "Donghua series detail (Anichin). Episode list paginated (default 50/page, max 200).",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/donghua/episode/{id}",
            description: "Donghua episode + servers + download mirrors",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/donghua/schedule",
            description: "Donghua weekly release schedule (Anichin), grouped by day (Monday→Sunday)",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/anime/{id}",
            description: "Anime series detail (Otakudesu) — full metadata + episode list",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/anime/episode/{id}",
            description: "Anime episode: streaming mirrors (by quality) + downloads + nav",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/anime-stream?id={signed}",
            description: "Resolve an anime mirror token into a playable embed URL",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/cosplay/{id}",
            description: "Cosplay post (gallery + downloads + resolved video)",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/cosplay-video?p={payload}&s={signature}",
            description: "Resolve a Cosplaytele embed into a playable HLS stream URL",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/novel/{id}?page={n}&size={N}",
            description: "Novel series detail (NovelID). Handles upstream-paginated chapter lists for novels with thousands of chapters.",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/novel/chapter/{id}",
            description: "Novel chapter (text body, plus prev/next IDs)",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/nhentai/{id}",
            description: "nhentai gallery by opaque ID (browser fingerprint spoofed)",
        },
        EndpointDoc {
            method: "GET",
            path: "/api/v1/nhentai/chapter/{id}",
            description: "nhentai gallery as a chapter (proxied page URLs)",
        },
        EndpointDoc {
            method: "GET",
            path: "/img?p={payload}&s={signature}",
            description: "HMAC-signed image proxy that hides upstream CDNs",
        },
        EndpointDoc {
            method: "GET",
            path: "/hls?p={payload}&s={signature}",
            description: "HLS playlist proxy (segments stream direct from CDN to client)",
        },
    ]
}

// ---- Search ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_page")]
    pub page: u32,
}
fn default_source() -> String {
    "all".to_string()
}
fn default_page() -> u32 {
    1
}

// ---- Suggest (live search suggestions) -----------------------------------

#[derive(Debug, Deserialize)]
pub struct SuggestQuery {
    pub q: String,
    #[serde(default = "default_source")]
    pub source: String,
}

// ---- Browse (home / popular / latest) ------------------------------------

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    /// Feed name. Common values:
    ///   "home" / ""          - default landing feed
    ///   "popular"            - popular all-time
    ///   "popular-today"      - popular today (nhentai)
    ///   "popular-week"       - popular this week (nhentai)
    ///   "latest"             - latest updates
    ///   "recommend"          - recommended (mangaball)
    /// Any other value is passed through to the provider for genre/category browsing.
    #[serde(default = "default_feed")]
    pub feed: String,
    #[serde(default = "default_page")]
    pub page: u32,
    /// Optional page size (capped per provider). Ignored by HTML-paginated providers.
    #[serde(default)]
    pub size: Option<u32>,
    /// When set ("1"/"true"), multi-source providers (e.g. anime) collapse
    /// duplicate titles, keeping the preferred source. Used by the home page so
    /// it doesn't show the same series twice; full browse/search leave both in.
    #[serde(default)]
    pub dedupe: Option<String>,
}
fn default_feed() -> String {
    "home".to_string()
}

pub async fn search(
    State(state): State<ApiState>,
    Query(q): Query<SearchQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let id = req_id(req.headers());

    let qstr = q.q.trim();
    if qstr.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "missing_query",
            "Query parameter 'q' is required and must not be empty",
            started,
            &id,
        );
    }
    if qstr.len() > 200 {
        return err(
            StatusCode::BAD_REQUEST,
            "query_too_long",
            "Query parameter 'q' exceeds 200 characters",
            started,
            &id,
        );
    }

    let cache_key = format!("search:{}|{}|{}", qstr, q.source, q.page);
    let already_cached = state.search_cache.get(&cache_key).await.is_some();

    let state_clone = state.clone();
    let q_clone = qstr.to_string();
    let source_clone = q.source.clone();
    let cache_key_for_invalidate = cache_key.clone();
    let arc = state
        .search_cache
        .try_get_with(cache_key, async move {
            run_search(&state_clone, &q_clone, &source_clone, q.page)
                .await
                .map(Arc::new)
        })
        .await;

    // Don't retain an empty result so a transient upstream failure doesn't
    // poison the cache for the whole TTL.
    if let Ok(ref data) = arc {
        if data.total == 0 {
            state
                .search_cache
                .invalidate(&cache_key_for_invalidate)
                .await;
        }
    }

    match arc {
        Ok(data) => ok(
            StatusCode::OK,
            (*data).clone(),
            started,
            already_cached,
            &id,
        ),
        Err(e) => err(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            (*e).clone(),
            started,
            &id,
        ),
    }
}

/// `GET /api/v1/suggest?q={query}&source={all|provider}` — live search
/// suggestions for type-ahead dropdowns.
///
/// Best-effort and keystroke-driven: an empty query or a hard upstream
/// failure returns an `ok` envelope with an empty suggestion list rather than
/// an error, so it can never break typing in the client.
///
/// Suggestions come in two flavours, ordered tag-first (most specific):
///   - `tag`   — derived from nhentai's typed tags (parody/character/tag/
///     artist/group). The `value` uses the `[Tag Name]` exact-tag syntax so
///     selecting it searches that tag.
///   - `title` — derived from real search results; `value` is the title and
///     `id` is the opaque id for deep-linking.
pub async fn suggest(
    State(state): State<ApiState>,
    Query(q): Query<SuggestQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    // Trim + cap (truncate, don't error) so the client can call freely.
    let mut qstr = q.q.trim().to_string();
    if qstr.chars().count() > 100 {
        qstr = qstr.chars().take(100).collect();
    }

    if qstr.is_empty() {
        return ok(
            StatusCode::OK,
            SuggestDto {
                query: String::new(),
                suggestions: Vec::new(),
            },
            started,
            false,
            &rid,
        );
    }

    let mut suggestions: Vec<SuggestionItemDto> = Vec::new();
    let q_lower = qstr.to_lowercase();

    // --- TAG suggestions (nhentai typed tags) ---
    // Only when the requested source could surface nhentai results.
    let want_nhentai = matches!(q.source.as_str(), "all" | "nhentai" | "doujin");
    if want_nhentai {
        let url = crate::adapters::nhentai::NhentaiAdapter::api_url_for_search(&qstr, 1);
        if let Ok(body) = call_nhentai_search_api(&state, &url).await {
            // The search endpoint only returns numeric `tag_ids`, not tag
            // names/types. To build real facets we fetch the top few matching
            // galleries' detail JSON (which carries full `tags`) in parallel
            // and tally them — so "Genshin Impact" surfaces `parody: Genshin
            // Impact`, `tag: full color`, etc.
            use std::collections::HashMap;
            let ids: Vec<u64> = body
                .get("result")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.get("id").and_then(|i| i.as_u64()))
                        .take(8)
                        .collect()
                })
                .unwrap_or_default();
            let futs = ids.into_iter().map(|id| {
                let st = state.clone();
                async move {
                    let gurl = crate::adapters::nhentai::NhentaiAdapter::api_url_for_gallery(id);
                    call_nhentai_api(&st, &gurl).await.ok()
                }
            });
            let galleries = futures::future::join_all(futs).await;
            let mut counts: HashMap<(String, String), (u32, String)> = HashMap::new();
            for g in galleries.into_iter().flatten() {
                let tags = match g.get("tags").and_then(|v| v.as_array()) {
                    Some(t) => t,
                    None => continue,
                };
                for t in tags {
                    let ttype = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty()
                        || !matches!(ttype, "parody" | "character" | "tag" | "artist" | "group")
                    {
                        continue;
                    }
                    let key = (ttype.to_string(), name.to_lowercase());
                    let e = counts.entry(key).or_insert((0, name.to_string()));
                    e.0 += 1;
                }
            }
            // Rank: facets matching the typed text first (parody/character ahead
            // of generic tags), then the most frequent refinement tags.
            let type_rank = |t: &str| match t {
                "parody" => 0,
                "character" => 1,
                "group" => 2,
                "artist" => 3,
                _ => 4,
            };
            let mut entries: Vec<(String, String, u32)> = counts
                .into_iter()
                .map(|((ttype, _lc), (count, disp))| (ttype, disp, count))
                .collect();
            entries.sort_by(|a, b| {
                let am = a.1.to_lowercase().contains(&q_lower);
                let bm = b.1.to_lowercase().contains(&q_lower);
                bm.cmp(&am)
                    .then(type_rank(&a.0).cmp(&type_rank(&b.0)))
                    .then(b.2.cmp(&a.2))
            });
            // Identify a base parody/series that matches what the user typed
            // (e.g. "genshin impact" -> Parody: Genshin Impact). When present,
            // we offer cumulative refinements like `[Genshin Impact] [full
            // color]` so each pick narrows the search, nhentai-style.
            let base_parody: Option<String> = entries
                .iter()
                .find(|(t, name, _)| {
                    t == "parody" && {
                        let n = name.to_lowercase();
                        n.contains(&q_lower) || q_lower.contains(&n)
                    }
                })
                .map(|(_, name, _)| name.clone());

            use std::collections::HashMap as Hm;

            if let Some(base) = base_parody.as_ref() {
                // Standalone parody first.
                suggestions.push(SuggestionItemDto {
                    r#type: "tag".to_string(),
                    label: format!("Parody: {}", base),
                    value: format!("[{}]", base),
                    source: "nhentai".to_string(),
                    kind: "doujin".to_string(),
                    id: None,
                });
                // Cumulative refinements: base parody + a popular facet.
                let base_lc = base.to_lowercase();
                let mut refine_per_type: Hm<String, u32> = Hm::new();
                let mut refined = 0usize;
                for (ttype, name, _count) in entries.iter() {
                    if ttype == "parody" || name.to_lowercase() == base_lc {
                        continue;
                    }
                    let cap = match ttype.as_str() {
                        "tag" => 5,
                        "character" => 4,
                        _ => 2,
                    };
                    let c = refine_per_type.entry(ttype.clone()).or_insert(0);
                    if *c >= cap {
                        continue;
                    }
                    *c += 1;
                    suggestions.push(SuggestionItemDto {
                        r#type: "tag".to_string(),
                        label: format!("{} · {}: {}", base, titlecase(ttype), name),
                        value: format!("[{}] [{}]", base, name),
                        source: "nhentai".to_string(),
                        kind: "doujin".to_string(),
                        id: None,
                    });
                    refined += 1;
                    if refined >= 10 {
                        break;
                    }
                }
            } else {
                // No clear base parody: a varied flat facet list, capping each
                // type so `character` can't crowd out parody/tag/group/artist.
                let type_cap = |t: &str| match t {
                    "parody" => 3,
                    "character" => 5,
                    "tag" => 6,
                    "group" => 2,
                    "artist" => 3,
                    _ => 2,
                };
                let mut per_type: Hm<String, u32> = Hm::new();
                let mut picked = 0usize;
                for (ttype, name, _count) in entries.iter() {
                    let c = per_type.entry(ttype.clone()).or_insert(0);
                    if *c >= type_cap(ttype) {
                        continue;
                    }
                    *c += 1;
                    suggestions.push(SuggestionItemDto {
                        r#type: "tag".to_string(),
                        label: format!("{}: {}", titlecase(ttype), name),
                        value: format!("[{}]", name),
                        source: "nhentai".to_string(),
                        kind: "doujin".to_string(),
                        id: None,
                    });
                    picked += 1;
                    if picked >= 14 {
                        break;
                    }
                }
            }
        }
    }

    // --- TITLE suggestions (real search results) ---
    // Reuse the existing search path so titles + ids match the search UX.
    if let Ok(data) = run_search(&state, &qstr, &q.source, 1).await {
        let mut seen_titles: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in data.items.into_iter() {
            let key = item.title.to_lowercase();
            if key.is_empty() || !seen_titles.insert(key) {
                continue;
            }
            suggestions.push(SuggestionItemDto {
                r#type: "title".to_string(),
                label: item.title.clone(),
                value: item.title,
                source: item.source,
                kind: item.kind,
                id: Some(item.id),
            });
            // Tag suggestions occupy the head; cap the title block so the
            // combined list stays around a dozen entries.
            let title_count = suggestions.iter().filter(|s| s.r#type == "title").count();
            if title_count >= 8 {
                break;
            }
        }
    }

    // Cap the total to keep the dropdown tidy (tags already lead the list).
    suggestions.truncate(18);

    ok(
        StatusCode::OK,
        SuggestDto {
            query: qstr,
            suggestions,
        },
        started,
        false,
        &rid,
    )
}

/// Title-case a lowercase tag-type token (e.g. "parody" -> "Parody").
fn titlecase(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

async fn run_search(
    state: &ApiState,
    query: &str,
    source: &str,
    page: u32,
) -> Result<SearchEnvelopeData, String> {
    let sources: Vec<SearchSource> = match source {
        "manga" | "mangaball" => vec![SearchSource::Mangaball],
        "donghua" | "anichin" => vec![SearchSource::Anichin],
        "cosplay" | "cosplaytele" => vec![SearchSource::Cosplaytele],
        "nhentai" | "doujin" => vec![SearchSource::Nhentai],
        "novel" | "novelid" => vec![SearchSource::Novelid],
        "anime" | "otakudesu" => vec![SearchSource::Otakudesufit, SearchSource::Otakudesu],
        "lmanime" | "lm" => vec![SearchSource::Lmanime],
        "movie" | "lk21" | "film" => vec![SearchSource::Lk21],
        "nekopoi" | "hentai" => vec![SearchSource::Nekopoi],
        "all" => vec![
            SearchSource::Mangaball,
            SearchSource::Anichin,
            SearchSource::Cosplaytele,
            SearchSource::Nhentai,
            SearchSource::Novelid,
            SearchSource::Otakudesufit,
            SearchSource::Otakudesu,
            SearchSource::Lmanime,
            SearchSource::Lk21,
        ],
        other => return Err(format!("Unknown source '{}'", other)),
    };

    let futures = sources.into_iter().map(|src| {
        let q = query.to_string();
        let state = state.clone();
        async move {
            let res = run_single_search(&state, src, &q, page).await;
            (src, res)
        }
    });
    let results = futures::future::join_all(futures).await;

    let mut items = Vec::new();
    // Aggregate pagination across providers: the overall result set has a
    // "next page" if *any* provider does, and the total page count is the max
    // any single provider reports (so the pager can reach the deepest source).
    let mut agg_total_pages: Option<u32> = None;
    let mut agg_has_next = false;
    for (_, r) in results {
        if let Ok(single) = r {
            if let Some(tp) = single.total_pages {
                agg_total_pages = Some(agg_total_pages.map_or(tp, |cur| cur.max(tp)));
            }
            agg_has_next = agg_has_next || single.has_next;
            for raw in single.items {
                items.push(raw_search_to_dto(state, raw));
            }
        }
    }

    // Sort by relevance to the query so the closest title matches come first
    // (upstream order is often "recommended"/recency, not match quality).
    // Higher score = better; ties keep the original (per-provider) order.
    let q_norm = normalize_for_match(query);
    let q_terms: Vec<String> = q_norm.split_whitespace().map(|s| s.to_string()).collect();
    let mut scored: Vec<(i64, usize, SearchItemDto)> = items
        .into_iter()
        .enumerate()
        .map(|(idx, it)| (relevance_score(&it.title, &q_norm, &q_terms), idx, it))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let items: Vec<SearchItemDto> = scored.into_iter().map(|(_, _, it)| it).collect();

    // If a provider gave us a full page but no explicit page count, assume at
    // least one more page exists so the UI shows a "Next" affordance.
    let has_next = agg_total_pages.map(|tp| page < tp).unwrap_or(agg_has_next);

    Ok(SearchEnvelopeData {
        query: query.to_string(),
        source: source.to_string(),
        page,
        per_page: SEARCH_PER_PAGE,
        total: items.len(),
        total_pages: agg_total_pages.unwrap_or(0),
        has_next,
        items,
    })
}

/// Nominal page size we advertise for search results. Providers paginate
/// independently upstream; this is only used by the client to render a
/// numeric pager when an exact page count is unknown.
const SEARCH_PER_PAGE: u32 = 25;

/// Lowercase + collapse non-alphanumeric runs to single spaces.
fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

/// Extract a season number from a normalized anime title, handling both
/// "season 4" and "4th season" forms (ignores other stray numbers like
/// "2 nensei").
fn anime_season(norm: &str) -> Option<u32> {
    static SEASON_RE: Lazy<regex::Regex> = Lazy::new(|| {
        regex::Regex::new(r"(?:season\s+(\d+))|(?:(\d+)\s*(?:st|nd|rd|th)\s+season)").unwrap()
    });
    SEASON_RE.captures(norm).and_then(|c| {
        c.get(1)
            .or_else(|| c.get(2))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    })
}

/// A dedup key for an anime title. Collapses the same series listed under
/// different romanizations across otakudesu.fit / .blog:
///   - `tokens`: significant tokens (particles / fansub markers / season
///     markers removed) for fuzzy overlap matching (handles truncated
///     romanizations, e.g. blog dropping "reijou"/"na"/"no").
///   - `squash`: those same tokens concatenated in order, which is agnostic to
///     word-splitting differences (e.g. "Himekishi" vs "Hime Kishi").
///   - `season`: detected season number so different seasons stay distinct.
struct AnimeKey {
    tokens: std::collections::HashSet<String>,
    squash: String,
    season: Option<u32>,
}

fn anime_series_key(title: &str) -> AnimeKey {
    // Low-signal tokens that vary between romanizations / sources and would
    // otherwise dilute the overlap ratio: Japanese grammatical particles, the
    // "sub indo" / fansub markers blog appends, and English articles.
    const STOPWORDS: &[&str] = &[
        "no",
        "na",
        "ni",
        "wa",
        "wo",
        "ga",
        "e",
        "to",
        "ne",
        "da",
        "de",
        "mo",
        "ya",
        "yo",
        "desu",
        "the",
        "a",
        "an",
        "of",
        "sub",
        "indo",
        "subtitle",
        "indonesia",
        "season",
    ];
    let norm = normalize_for_match(title);
    let season = anime_season(&norm);
    let mut skip_next_season_word = false;
    let mut tokens: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    let parts: Vec<&str> = norm.split_whitespace().collect();
    for (i, tok) in parts.iter().enumerate() {
        // Drop the literal "season" token and the adjacent number/ordinal.
        if *tok == "season" {
            skip_next_season_word = true;
            continue;
        }
        if skip_next_season_word {
            skip_next_season_word = false;
            if tok.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
        }
        // Drop an ordinal-season token like "4th" when followed by "season".
        if parts.get(i + 1).copied() == Some("season")
            && tok
                .trim_end_matches(|c: char| c.is_alphabetic())
                .parse::<u32>()
                .is_ok()
        {
            continue;
        }
        // Drop grammatical particles / fansub markers.
        if STOPWORDS.contains(tok) {
            continue;
        }
        tokens.insert((*tok).to_string());
        ordered.push((*tok).to_string());
    }
    let squash = ordered.join("");
    AnimeKey {
        tokens,
        squash,
        season,
    }
}

/// Whether two anime titles denote the same series.
fn same_anime_series(a: &AnimeKey, b: &AnimeKey) -> bool {
    // Different explicit seasons => different entries.
    if let (Some(sa), Some(sb)) = (a.season, b.season) {
        if sa != sb {
            return false;
        }
    }
    // Word-split-agnostic exact match: "Himekishi wa Barbaroi no Yome" vs
    // "Hime Kishi wa Barbaroi no Yome" squash to the same string.
    if !a.squash.is_empty() && a.squash == b.squash {
        return true;
    }
    let min_len = a.tokens.len().min(b.tokens.len());
    if min_len < 3 {
        // Too short to fuzzy-match safely; require exact token-set equality.
        return a.tokens == b.tokens;
    }
    let inter = a.tokens.intersection(&b.tokens).count();
    (inter as f64) / (min_len as f64) >= 0.8
}

/// Heuristic relevance score of a result title against a normalized query.
fn relevance_score(title: &str, q_norm: &str, q_terms: &[String]) -> i64 {
    if q_norm.is_empty() {
        return 0;
    }
    let t = normalize_for_match(title);
    let mut score = 0i64;
    if t == q_norm {
        score += 1000; // exact title match
    }
    if t.starts_with(q_norm) {
        score += 400; // title begins with the query
    }
    if t.contains(q_norm) {
        score += 200; // full query appears as a phrase
    }
    // Per-term coverage.
    let mut matched = 0;
    for term in q_terms {
        if term.is_empty() {
            continue;
        }
        if t.split_whitespace().any(|w| w == term) {
            score += 40; // whole-word match
            matched += 1;
        } else if t.contains(term.as_str()) {
            score += 15; // substring match
            matched += 1;
        }
    }
    if !q_terms.is_empty() && matched == q_terms.len() {
        score += 100; // all terms present
    }
    // Prefer shorter titles when scores are otherwise close (closer match).
    score -= (t.len() as i64) / 40;
    score
}

/// One provider's search result page plus what we could learn about its
/// pagination. `total_pages` is `None` when the provider doesn't expose it;
/// `has_next` is a best-effort hint (e.g. a full page of results) used as a
/// fallback when the page count is unknown.
struct SingleSearch {
    items: Vec<SearchResultItem>,
    total_pages: Option<u32>,
    has_next: bool,
}

async fn run_single_search(
    state: &ApiState,
    source: SearchSource,
    query: &str,
    page: u32,
) -> Result<SingleSearch, String> {
    if matches!(source, SearchSource::Mangaball) {
        // Mangaball search returns the full match set in one response (no
        // upstream paging), so everything is page 1.
        let items = search_mangaball(state, query).await?;
        return Ok(SingleSearch {
            items,
            total_pages: Some(1),
            has_next: false,
        });
    }
    if matches!(source, SearchSource::Nhentai) {
        let (items, total_pages) = search_nhentai_sorted(state, query, page, "").await?;
        let has_next = total_pages.map(|tp| page < tp).unwrap_or(!items.is_empty());
        return Ok(SingleSearch {
            items,
            total_pages,
            has_next,
        });
    }
    if matches!(source, SearchSource::Lk21) {
        // lk21 keyword search hits a JSON API rather than scraping HTML.
        let url = crate::web::search::lk21_search_api_url(query, page);
        let body = call_lk21_search_api(state, &url).await?;
        let total_pages = body
            .get("totalPages")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        let items = crate::web::search::parse_lk21_search_json(&body);
        let has_next = total_pages.map(|tp| page < tp).unwrap_or(!items.is_empty());
        return Ok(SingleSearch {
            items,
            total_pages,
            has_next,
        });
    }
    let url = match build_search_url(source, query, page) {
        Some(u) => u,
        None => return Err(format!("no search URL for source {:?}", source)),
    };
    let (items, total_pages) = fetch_and_parse_html_listing(state, source, &url).await?;
    // Cosplaytele's WordPress search is loose and the page also embeds
    // recommendation carousels. We already strip the carousels during parsing;
    // here we additionally keep only results that actually match the query
    // (every query word must appear in the title or the cat-label snippet,
    // which carries the cosplayer name). This gives high-precision results
    // like "xiaoyaoyaoyao" -> only xiaoyaoyaoyao posts.
    let items = if matches!(source, SearchSource::Cosplaytele) {
        filter_relevant(items, query)
    } else {
        items
    };
    let has_next = total_pages.map(|tp| page < tp).unwrap_or(!items.is_empty());
    Ok(SingleSearch {
        items,
        total_pages,
        has_next,
    })
}

/// Keep only items whose title/snippet contain *all* whitespace-delimited
/// query terms (case-insensitive). Falls back to the unfiltered list if the
/// filter would remove everything (so a slightly-off match still shows
/// something rather than an empty page).
fn filter_relevant(items: Vec<SearchResultItem>, query: &str) -> Vec<SearchResultItem> {
    let terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();
    if terms.is_empty() {
        return items;
    }
    let filtered: Vec<SearchResultItem> = items
        .iter()
        .filter(|it| {
            let hay = format!(
                "{} {}",
                it.title.to_lowercase(),
                it.snippet.as_deref().unwrap_or("").to_lowercase()
            );
            terms.iter().all(|t| hay.contains(t))
        })
        .cloned()
        .collect();
    if filtered.is_empty() {
        items
    } else {
        filtered
    }
}

/// Fetch an HTML listing page and parse it. If the parse yields zero items
/// but the body looks like an anti-bot interstitial (Cloudflare "checking
/// your browser", short bodies), retry once after a short delay. This makes
/// HTML providers (Anichin, Cosplaytele, NovelID) resilient to transient
/// "200 OK but empty" responses.
///
/// Returns the parsed items plus the total page count parsed from the
/// upstream pager (`None` when there is no pager / single page).
async fn fetch_and_parse_html_listing(
    state: &ApiState,
    source: SearchSource,
    url: &str,
) -> Result<(Vec<SearchResultItem>, Option<u32>), String> {
    let html = state
        .engine
        .fetch_html(url)
        .await
        .map_err(|e| e.to_string())?;
    let items = parse_search_html(source, url, &html);
    if !items.is_empty() {
        let pages = crate::web::search::parse_html_total_pages(&html);
        return Ok((items, pages));
    }
    // Zero items: distinguish a genuinely-empty listing from an anti-bot
    // interstitial / truncated body. Retry once when the body looks suspect.
    if looks_like_interstitial(&html) {
        tracing::warn!(url = %url, bytes = html.len(), "empty HTML listing looked like an interstitial; retrying once");
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let html2 = state
            .engine
            .fetch_html(url)
            .await
            .map_err(|e| e.to_string())?;
        let items2 = parse_search_html(source, url, &html2);
        let pages = crate::web::search::parse_html_total_pages(&html2);
        return Ok((items2, pages));
    }
    Ok((items, None))
}

/// Heuristic: does this HTML look like an anti-bot challenge or a broken /
/// truncated page rather than a real (possibly empty) listing?
fn looks_like_interstitial(html: &str) -> bool {
    if html.len() < 2000 {
        return true;
    }
    let lower = html.to_lowercase();
    lower.contains("just a moment")
        || lower.contains("checking your browser")
        || lower.contains("cf-browser-verification")
        || lower.contains("attention required")
        || lower.contains("enable javascript and cookies")
}

/// Hit the nhentai JSON search API directly. We construct browser-like
/// headers via the fingerprint module so the request looks like a real
/// nhentai.net visit.
#[allow(dead_code)]
async fn search_nhentai(
    state: &ApiState,
    query: &str,
    page: u32,
) -> Result<(Vec<SearchResultItem>, Option<u32>), String> {
    search_nhentai_sorted(state, query, page, "").await
}

async fn search_nhentai_sorted(
    state: &ApiState,
    query: &str,
    page: u32,
    sort: &str,
) -> Result<(Vec<SearchResultItem>, Option<u32>), String> {
    let url = crate::adapters::nhentai::NhentaiAdapter::api_url_for_search_sorted(
        query,
        page.max(1),
        sort,
    );
    let body = call_nhentai_search_api(state, &url).await?;
    let (total_pages, _per_page) = crate::web::search::parse_nhentai_pagination(&body);
    Ok((parse_nhentai_search(&body), total_pages))
}

async fn call_nhentai_search_api(state: &ApiState, url: &str) -> Result<serde_json::Value, String> {
    let fp = BrowserFingerprint::for_url(url);
    let mut adapter_headers = fp.as_header_map();
    adapter_headers.insert("Referer".to_string(), "https://nhentai.net/".to_string());
    adapter_headers.insert(
        "Accept".to_string(),
        "application/json, text/plain, */*".to_string(),
    );
    adapter_headers.insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    adapter_headers.insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    adapter_headers.insert("Sec-Fetch-Site".to_string(), "same-origin".to_string());
    adapter_headers.remove("Upgrade-Insecure-Requests");

    let pipeline = state.engine.pipeline();
    let headers = pipeline
        .build_headers(url, None, Some(&adapter_headers))
        .map_err(|e| e.to_string())?;

    let resp = state
        .engine
        .client()
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        tracing::warn!(url = %url, status = status.as_u16(), "nhentai upstream non-2xx");
        return Ok(serde_json::Value::Null);
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let body: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(err = %e, snippet = %&text.chars().take(200).collect::<String>(), "nhentai body not JSON");
            return Ok(serde_json::Value::Null);
        }
    };
    Ok(body)
}

async fn search_mangaball(state: &ApiState, query: &str) -> Result<Vec<SearchResultItem>, String> {
    let home_url = "https://mangaball.net/";
    let home_html = state
        .engine
        .fetch_html(home_url)
        .await
        .map_err(|e| e.to_string())?;
    let csrf = match regex::Regex::new(r#"<meta\s+name="csrf-token"\s+content="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(&home_html).map(|c| c[1].to_string()))
    {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    let mut adapter_headers = std::collections::HashMap::new();
    adapter_headers.insert("X-CSRF-TOKEN".to_string(), csrf);
    adapter_headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
    adapter_headers.insert(
        "Accept".to_string(),
        "application/json, text/javascript, */*; q=0.01".to_string(),
    );
    adapter_headers.insert("Referer".to_string(), home_url.to_string());

    let pipeline = state.engine.pipeline();
    let api_url = mangaball_search_endpoint();
    let headers = pipeline
        .build_headers(api_url, None, Some(&adapter_headers))
        .map_err(|e| e.to_string())?;

    let form = [("search_input", query)];
    let resp = state
        .engine
        .client()
        .post(api_url)
        .headers(headers)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_mangaball_search(&body))
}

fn raw_search_to_dto(state: &ApiState, raw: SearchResultItem) -> SearchItemDto {
    let (source, kind_for_ux) = match raw.source.as_str() {
        "mangaball" => (Source::Mangaball, "manga"),
        "anichin" => (Source::Anichin, "donghua"),
        "cosplaytele" => (Source::Cosplaytele, "cosplay"),
        "nhentai" => (Source::Nhentai, "doujin"),
        "novelid" => (Source::Novelid, "novel"),
        "otakudesu" => (Source::Otakudesu, "anime"),
        "otakudesufit" => (Source::Otakudesufit, "anime"),
        "lmanime" => (Source::Lmanime, "lmanime"),
        "lk21" => (Source::Lk21, "movie"),
        "nekopoi" => (Source::Nekopoi, "nekopoi"),
        _ => (Source::Mangaball, "unknown"),
    };
    let opaque_kind = match source {
        Source::Mangaball
        | Source::Anichin
        | Source::Nhentai
        | Source::Novelid
        | Source::Otakudesu
        | Source::Otakudesufit
        | Source::Lmanime => Kind::Series,
        Source::Cosplaytele | Source::Lk21 | Source::Nekopoi => Kind::Post,
    };
    let id = state.codec.encode(source, opaque_kind, &raw.url);
    SearchItemDto {
        id,
        source: raw.source,
        kind: kind_for_ux.to_string(),
        title: raw.title,
        thumbnail: proxy_opt(state, raw.thumbnail.as_deref()),
        snippet: raw.snippet,
        tags: raw.tags,
        cosplayer: raw.cosplayer,
        character: raw.character,
        series: raw.series,
    }
}

// ---- Donghua schedule -----------------------------------------------------

/// `GET /api/v1/donghua/schedule` — Anichin weekly release schedule.
///
/// Fetches the schedule page fresh each call (the engine maintains its own
/// HTML cache), parses the day blocks, and maps each entry to a
/// `ScheduleItemDto` whose `id` is the opaque-encoded series URL (so it opens
/// the existing donghua detail flow) and whose `thumbnail` is routed through
/// the image proxy.
pub async fn donghua_schedule(State(state): State<ApiState>, req: Request) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    let url = crate::adapters::anichin::AnichinAdapter::schedule_url();
    let html = match state.engine.fetch_html(url).await {
        Ok(h) => h,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                e.to_string(),
                started,
                &rid,
            );
        }
    };

    let parsed = crate::adapters::anichin::AnichinAdapter::parse_schedule(url, &html);
    let days: Vec<ScheduleDayDto> = parsed
        .into_iter()
        .map(|d| ScheduleDayDto {
            day: d.day,
            items: d
                .items
                .into_iter()
                .map(|it| ScheduleItemDto {
                    id: state.codec.encode(Source::Anichin, Kind::Series, &it.url),
                    source: "anichin".to_string(),
                    kind: "donghua".to_string(),
                    title: it.title,
                    thumbnail: proxy_opt(&state, it.thumbnail.as_deref()),
                    episode: it.episode,
                    time_label: it.time_label,
                    release_at: it.release_at,
                })
                .collect(),
        })
        .collect();

    ok(StatusCode::OK, ScheduleDto { days }, started, false, &rid)
}

// ---- Browse (home / popular / latest) ------------------------------------

/// Generic browse handler: `GET /api/v1/{provider}/browse?feed=<feed>&page=<n>`.
/// Each provider maps `feed` to a different upstream URL.
pub async fn browse(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    Query(q): Query<BrowseQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    let p = q.page.max(1);
    let dedupe = matches!(q.dedupe.as_deref(), Some("1") | Some("true") | Some("yes"));
    let cache_key = format!(
        "browse:{}|{}|{}|{}|{}",
        provider,
        q.feed,
        p,
        q.size.unwrap_or(0),
        dedupe as u8
    );
    let cached = state.search_cache.get(&cache_key).await.is_some();

    let provider_lc = provider.to_lowercase();
    let feed = q.feed.clone();
    let size = q.size;
    let state_clone = state.clone();

    let cache_key_for_invalidate = cache_key.clone();
    let arc = state
        .search_cache
        .try_get_with(cache_key, async move {
            let br = run_browse(&state_clone, &provider_lc, &feed, p, size, dedupe).await?;
            let has_next = br.total_pages.map(|tp| p < tp).unwrap_or(br.has_next);
            Ok::<Arc<SearchEnvelopeData>, String>(Arc::new(SearchEnvelopeData {
                query: String::new(),
                source: provider_lc,
                page: p,
                per_page: br.per_page,
                total: br.items.len(),
                total_pages: br.total_pages.unwrap_or(0),
                has_next,
                items: br.items,
            }))
        })
        .await;

    // Never retain an empty result: a transient upstream hiccup (Cloudflare
    // challenge, rate-limit) can parse to 0 items, and we don't want to
    // poison the cache for the whole TTL. Genuinely-empty feeds are cheap to
    // re-fetch.
    if let Ok(ref data) = arc {
        if data.total == 0 {
            state
                .search_cache
                .invalidate(&cache_key_for_invalidate)
                .await;
        }
    }

    match arc {
        Ok(data) => ok(StatusCode::OK, (*data).clone(), started, cached, &rid),
        Err(e) => err(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            (*e).clone(),
            started,
            &rid,
        ),
    }
}

/// A browse feed page plus pagination info parsed from the provider.
struct BrowseResult {
    items: Vec<SearchItemDto>,
    /// Nominal items-per-page used for client pager math.
    per_page: u32,
    /// Total pages upstream when known (`None` otherwise).
    total_pages: Option<u32>,
    /// Best-effort "is there a next page?" hint when the count is unknown.
    has_next: bool,
}

async fn run_browse(
    state: &ApiState,
    provider: &str,
    feed: &str,
    page: u32,
    size: Option<u32>,
    dedupe: bool,
) -> Result<BrowseResult, String> {
    match provider {
        "mangaball" | "manga" => browse_mangaball(state, feed, page, size).await,
        "anichin" | "donghua" => browse_anichin(state, feed, page).await,
        "cosplaytele" | "cosplay" => browse_cosplaytele(state, feed, page).await,
        "nhentai" | "doujin" => browse_nhentai(state, feed, page).await,
        "novelid" | "novel" => browse_novelid(state, feed, page).await,
        "otakudesu" | "anime" => browse_anime_merged(state, feed, page, dedupe).await,
        "otakudesufit" => browse_otakudesufit(state, feed, page).await,
        "lmanime" => browse_lmanime(state, feed, page).await,
        "lk21" | "movie" | "film" => browse_lk21(state, feed, page).await,
        "nekopoi" | "hentai" => browse_nekopoi(state, feed, page).await,
        _ => Err(format!("unknown provider '{}'", provider)),
    }
}

/// Merged anime browse: otakudesu.fit (primary, more complete) + otakudesu.blog
/// (secondary). When `dedupe` is set (home page), drop blog entries whose title
/// matches a fit entry so the same series isn't shown twice; otherwise keep
/// both so duplicates are visible on the full browse/search.
async fn browse_anime_merged(
    state: &ApiState,
    feed: &str,
    page: u32,
    dedupe: bool,
) -> Result<BrowseResult, String> {
    let (fit_res, blog_res) = futures::join!(
        browse_otakudesufit(state, feed, page),
        browse_otakudesu(state, feed, page),
    );
    let fit = fit_res.unwrap_or_else(|_| empty_browse());
    let blog = blog_res.unwrap_or_else(|_| empty_browse());

    // Always collapse the same series listed under different romanizations
    // across the two sources (e.g. fit "Jishou Akuyaku Reijou na Konyakusha
    // no Kansatsu Kiroku" vs blog's shortened, "-sub-indo" variant). We keep
    // the otakudesu.fit entry (more complete metadata) and drop the blog twin
    // regardless of feed — the `dedupe` flag now only documents intent; doubles
    // are filtered everywhere so the same title never appears twice.
    //
    // fit is processed first (priority), then blog; an item is admitted only if
    // it doesn't match any already-admitted series key, which also folds away
    // any intra-source duplicates.
    let _ = dedupe;
    let mut items: Vec<SearchItemDto> = Vec::with_capacity(fit.items.len() + blog.items.len());
    let mut keys: Vec<AnimeKey> = Vec::new();
    for it in fit.items.into_iter().chain(blog.items) {
        let k = anime_series_key(&it.title);
        if keys.iter().any(|ek| same_anime_series(ek, &k)) {
            continue;
        }
        keys.push(k);
        items.push(it);
    }

    let total_pages = match (fit.total_pages, blog.total_pages) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let has_next = fit.has_next || blog.has_next;
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_otakudesufit(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
    let url = crate::adapters::otakudesufit::OtakudesufitAdapter::browse_url(feed, page);
    let (raw, total_pages) =
        fetch_and_parse_html_listing(state, SearchSource::Otakudesufit, &url).await?;
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_lk21(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::lk21::Lk21Adapter::browse_url(feed, page);
    let html = state
        .engine
        .fetch_html(&url)
        .await
        .map_err(|e| e.to_string())?;
    let raw = crate::web::search::parse_lk21_listing(&url, &html);
    let total_pages = crate::web::search::parse_html_total_pages(&html);
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_nekopoi(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::nekopoi::NekopoiAdapter::browse_url(feed, page);
    let html = state
        .engine
        .fetch_html(&url)
        .await
        .map_err(|e| e.to_string())?;
    let raw = crate::web::search::parse_nekopoi_listing(&url, &html);
    let total_pages = crate::web::search::parse_html_total_pages(&html);
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

/// GET the lk21 JSON search API with browser-like headers + Referer.
async fn call_lk21_search_api(state: &ApiState, url: &str) -> Result<serde_json::Value, String> {
    let fp = BrowserFingerprint::for_url(url);
    let mut adapter_headers = fp.as_header_map();
    adapter_headers.insert(
        "Referer".to_string(),
        format!("{}/", crate::adapters::lk21::LK21_BASE),
    );
    adapter_headers.insert(
        "Accept".to_string(),
        "application/json, text/plain, */*".to_string(),
    );
    adapter_headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
    adapter_headers.insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    adapter_headers.insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    adapter_headers.insert("Sec-Fetch-Site".to_string(), "cross-site".to_string());
    adapter_headers.remove("Upgrade-Insecure-Requests");

    let headers = state
        .engine
        .pipeline()
        .build_headers(url, None, Some(&adapter_headers))
        .map_err(|e| e.to_string())?;
    let resp = state
        .engine
        .client()
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("upstream returned {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

async fn browse_lmanime(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::lmanime::LmanimeAdapter::browse_url(feed, page);
    let (raw, total_pages) =
        fetch_and_parse_html_listing(state, SearchSource::Lmanime, &url).await?;
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_otakudesu(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::otakudesu::OtakudesuAdapter::browse_url(feed, page);
    let html = state
        .engine
        .fetch_html(&url)
        .await
        .map_err(|e| e.to_string())?;
    let hits = crate::adapters::otakudesu::OtakudesuAdapter::parse_browse(&url, &html);
    let total_pages = crate::web::search::parse_html_total_pages(&html);
    let items: Vec<SearchItemDto> = hits
        .into_iter()
        .map(|hit| {
            let raw = SearchResultItem {
                source: "otakudesu".to_string(),
                title: hit.title,
                url: hit.url,
                thumbnail: hit.thumbnail,
                kind: Some("anime_series".to_string()),
                snippet: None,
                tags: hit.genres,
                cosplayer: None,
                character: None,
                series: None,
            };
            raw_search_to_dto(state, raw)
        })
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_anichin(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::anichin::AnichinAdapter::browse_url(feed, page);
    let (raw, total_pages) =
        fetch_and_parse_html_listing(state, SearchSource::Anichin, &url).await?;
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_cosplaytele(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
    let url = crate::adapters::cosplaytele::CosplayteleAdapter::browse_url(feed, page);
    let (raw, total_pages) =
        fetch_and_parse_html_listing(state, SearchSource::Cosplaytele, &url).await?;
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

async fn browse_nhentai(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let sort = crate::adapters::nhentai::NhentaiAdapter::feed_to_sort(feed);
    // The plain galleries endpoint ignores `sort` (always recent), so popular
    // feeds go through the search API with an all-matching query; only the
    // "recent" feed uses the galleries listing.
    let url = if sort.is_empty() {
        crate::adapters::nhentai::NhentaiAdapter::api_url_for_popular(page, "")
    } else {
        crate::adapters::nhentai::NhentaiAdapter::api_url_for_search_sorted("pages:>0", page, sort)
    };
    let body = call_nhentai_search_api(state, &url).await?;
    let (total_pages, per_page) = crate::web::search::parse_nhentai_pagination(&body);
    let raw = parse_nhentai_search(&body);
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = total_pages.map(|tp| page < tp).unwrap_or(!items.is_empty());
    Ok(BrowseResult {
        per_page: per_page.unwrap_or_else(|| items.len().max(1) as u32),
        total_pages,
        has_next,
        items,
    })
}

async fn browse_novelid(state: &ApiState, feed: &str, page: u32) -> Result<BrowseResult, String> {
    let url = crate::adapters::novelid::NovelidAdapter::browse_url(feed, page);
    // NovelID uses the same card markup as its search results.
    let (raw, total_pages) = fetch_and_parse_novelid_listing(state, &url).await?;
    let items: Vec<SearchItemDto> = raw
        .into_iter()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let has_next = !items.is_empty();
    Ok(BrowseResult {
        per_page: items.len().max(1) as u32,
        total_pages,
        has_next,
        items,
    })
}

/// Like `fetch_and_parse_html_listing` but for NovelID's card markup.
/// Returns the parsed items plus the upstream total page count when present.
async fn fetch_and_parse_novelid_listing(
    state: &ApiState,
    url: &str,
) -> Result<(Vec<SearchResultItem>, Option<u32>), String> {
    let html = state
        .engine
        .fetch_html(url)
        .await
        .map_err(|e| e.to_string())?;
    let items = crate::web::search::parse_novelid_search(url, &html);
    if !items.is_empty() {
        let pages = crate::web::search::parse_html_total_pages(&html);
        return Ok((items, pages));
    }
    if looks_like_interstitial(&html) {
        tracing::warn!(url = %url, bytes = html.len(), "empty NovelID listing looked like an interstitial; retrying once");
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let html2 = state
            .engine
            .fetch_html(url)
            .await
            .map_err(|e| e.to_string())?;
        let items2 = crate::web::search::parse_novelid_search(url, &html2);
        let pages = crate::web::search::parse_html_total_pages(&html2);
        return Ok((items2, pages));
    }
    Ok((items, None))
}

/// Mangaball browse: POSTs to `/api/v1/title/search/` with a `search_type`
/// derived from the feed name. The response JSON shape mirrors smart-search.
///
/// Mangaball returns the whole feed in one response (no upstream paging), so
/// we request a generous cap, then slice the requested window locally and
/// derive an exact page count from the full result set.
async fn browse_mangaball(
    state: &ApiState,
    feed: &str,
    page: u32,
    size: Option<u32>,
) -> Result<BrowseResult, String> {
    let stype = crate::adapters::mangaball::MangaballAdapter::browse_search_type(feed);
    let s = size.unwrap_or(30).clamp(5, 60) as usize;
    // Mangaball returns the whole feed in one response (no upstream paging).
    // Fetch a fixed generous cap so the page count is stable across pages and
    // we can paginate locally with an accurate total.
    let limit = 300usize;

    let home_url = "https://mangaball.net/";
    let home_html = state
        .engine
        .fetch_html(home_url)
        .await
        .map_err(|e| e.to_string())?;
    let csrf = match regex::Regex::new(r#"<meta\s+name="csrf-token"\s+content="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(&home_html).map(|c| c[1].to_string()))
    {
        Some(c) => c,
        None => return Ok(empty_browse()),
    };
    let mut adapter_headers = std::collections::HashMap::new();
    adapter_headers.insert("X-CSRF-TOKEN".to_string(), csrf);
    adapter_headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
    adapter_headers.insert(
        "Accept".to_string(),
        "application/json, text/javascript, */*; q=0.01".to_string(),
    );
    adapter_headers.insert("Referer".to_string(), home_url.to_string());

    let pipeline = state.engine.pipeline();
    let api_url = crate::adapters::mangaball::MangaballAdapter::browse_endpoint();
    let headers = pipeline
        .build_headers(api_url, None, Some(&adapter_headers))
        .map_err(|e| e.to_string())?;

    let limit_str = limit.to_string();
    let form = [("search_type", stype), ("search_limit", &limit_str)];
    let resp = state
        .engine
        .client()
        .post(api_url)
        .headers(headers)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(empty_browse());
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // Mangaball's response is `{ code: 200, data: { manga: [...] } }`. We
    // also see shapes where `data` is a flat array. Reuse the existing
    // search parser to handle both, then slice the page window.
    let raw = crate::web::search::parse_mangaball_search(&body);
    let total_items = raw.len();
    // Page window [(page-1)*size .. page*size].
    let start = ((page.max(1) - 1) as usize) * s;
    let end = (start + s).min(total_items);
    let slice = if start >= total_items {
        &[]
    } else {
        &raw[start..end]
    };
    let items: Vec<SearchItemDto> = slice
        .iter()
        .cloned()
        .map(|r| raw_search_to_dto(state, r))
        .collect();
    let tp = if total_items == 0 {
        1
    } else {
        total_items.div_ceil(s)
    };
    let total_pages = Some(tp as u32);
    let has_next = (page as usize) < tp;

    Ok(BrowseResult {
        per_page: s as u32,
        total_pages,
        has_next,
        items,
    })
}

/// An empty browse page (used when an upstream prerequisite is missing).
fn empty_browse() -> BrowseResult {
    BrowseResult {
        items: Vec::new(),
        per_page: 1,
        total_pages: Some(1),
        has_next: false,
    }
}

// ---- Manga ----------------------------------------------------------------

pub async fn manga_series(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(pq): Query<ChapterPageQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Mangaball {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not mangaball",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series = match &result.content {
        Some(ContentModel::MangaSeries(s)) => s,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a manga series",
                started,
                &rid,
            )
        }
    };
    let dto = manga_series_to_dto(&state, series, &id, pq.page, pq.size);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn manga_series_to_dto(
    state: &ApiState,
    s: &MangaSeries,
    id: &str,
    page: u32,
    size: Option<u32>,
) -> MangaSeriesDto {
    let total = s.chapters.len();
    let size = size.unwrap_or(60).clamp(1, 300) as usize;
    let p = page.max(1) as usize;
    let start = (p - 1) * size;
    let end = (start + size).min(total);
    let window: Vec<&ChapterInfo> = if start >= total {
        Vec::new()
    } else {
        s.chapters[start..end].iter().collect()
    };
    let chapters = window
        .iter()
        .map(|c| chapter_ref_to_dto(state, c))
        .collect::<Vec<_>>();
    let total_pages = if total == 0 { 1 } else { total.div_ceil(size) };
    MangaSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        description: s.synopsis.clone(),
        author: s.author.clone(),
        artist: s.artist.clone(),
        genres: s.genres.clone(),
        cover: proxy_opt(state, s.cover_image.as_deref()),
        chapter_count: total,
        chapter_page: p as u32,
        chapter_page_size: size as u32,
        chapter_total_pages: total_pages as u32,
        chapters,
    }
}

fn chapter_ref_to_dto(state: &ApiState, c: &ChapterInfo) -> MangaChapterRef {
    MangaChapterRef {
        id: state.codec.encode(Source::Mangaball, Kind::Item, &c.url),
        number: c.number,
        title: c.title.clone(),
        translations: c
            .translations
            .iter()
            .map(|t| MangaTranslationRef {
                id: state.codec.encode(Source::Mangaball, Kind::Item, &t.url),
                language: t.language.clone(),
                group: t.group.clone(),
                date: t.date.clone(),
                pages: t.pages,
            })
            .collect(),
    }
}

pub async fn manga_chapter(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Mangaball {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not mangaball",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let chap = match &result.content {
        Some(ContentModel::MangaChapter(c)) => c,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a manga chapter",
                started,
                &rid,
            )
        }
    };
    let dto = manga_chapter_to_dto(&state, chap, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn manga_chapter_to_dto(state: &ApiState, c: &MangaChapter, id: &str) -> MangaChapterDto {
    MangaChapterDto {
        id: id.to_string(),
        series_title: c.series_title.clone(),
        chapter_number: c.chapter_number,
        page_count: c.pages.len(),
        pages: c.pages.iter().map(|p| page_to_dto(state, p)).collect(),
    }
}

fn page_to_dto(state: &ApiState, p: &PageImage) -> MangaPageDto {
    MangaPageDto {
        index: p.index,
        url: proxy_url(state, &p.url),
    }
}

// ---- Donghua --------------------------------------------------------------

pub async fn donghua_series(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(pq): Query<ChapterPageQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Anichin {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not anichin",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series = match &result.content {
        Some(ContentModel::DonghuaSeries(s)) => s,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a donghua series",
                started,
                &rid,
            )
        }
    };
    let dto = donghua_series_to_dto(&state, series, &id, pq.page, pq.size);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn donghua_series_to_dto(
    state: &ApiState,
    s: &DonghuaSeries,
    id: &str,
    page: u32,
    size: Option<u32>,
) -> DonghuaSeriesDto {
    let total = s.episodes.len();
    let size = size.unwrap_or(50).clamp(1, 5000) as usize;
    let p = page.max(1) as usize;
    let start = (p - 1) * size;
    let end = (start + size).min(total);
    let window: Vec<&EpisodeInfo> = if start >= total {
        Vec::new()
    } else {
        s.episodes[start..end].iter().collect()
    };
    let total_pages = if total == 0 { 1 } else { total.div_ceil(size) };
    DonghuaSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        synopsis: s.synopsis.clone(),
        status: s.status.clone(),
        genres: s.genres.clone(),
        cover: proxy_opt(state, s.thumbnail.as_deref()),
        episode_count: total,
        episode_page: p as u32,
        episode_page_size: size as u32,
        episode_total_pages: total_pages as u32,
        episodes: window
            .iter()
            .map(|e| episode_ref_to_dto(state, e))
            .collect(),
    }
}

fn episode_ref_to_dto(state: &ApiState, e: &EpisodeInfo) -> DonghuaEpisodeRef {
    DonghuaEpisodeRef {
        id: state.codec.encode(Source::Anichin, Kind::Item, &e.url),
        number: e.number,
        title: e.title.clone(),
    }
}

// ---- Anime (otakudesu) ----------------------------------------------------

pub async fn anime_series(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if !matches!(dec.source, Source::Otakudesu | Source::Otakudesufit) {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not otakudesu",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series = match &result.content {
        Some(ContentModel::AnimeSeries(s)) => s,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield an anime series",
                started,
                &rid,
            )
        }
    };
    let dto = anime_series_to_dto_src(&state, series, &id, dec.source);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn anime_episode_ref_to_dto(
    state: &ApiState,
    e: &crate::models::AnimeEpisodeRef,
    source: Source,
) -> AnimeEpisodeRefDto {
    AnimeEpisodeRefDto {
        id: state.codec.encode(source, Kind::Item, &e.url),
        number: e.number,
        title: e.title.clone(),
        date: e.date.clone(),
    }
}

fn anime_series_to_dto_src(
    state: &ApiState,
    s: &AnimeSeries,
    id: &str,
    source: Source,
) -> AnimeSeriesDto {
    AnimeSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        japanese_title: s.japanese_title.clone(),
        synopsis: s.synopsis.clone(),
        cover: proxy_opt(state, s.thumbnail.as_deref()),
        score: s.score.clone(),
        producer: s.producer.clone(),
        anime_type: s.anime_type.clone(),
        status: s.status.clone(),
        total_episodes: s.total_episodes.clone(),
        duration: s.duration.clone(),
        release_date: s.release_date.clone(),
        studio: s.studio.clone(),
        genres: s.genres.clone(),
        episode_count: s.episodes.len(),
        episodes: s
            .episodes
            .iter()
            .map(|e| anime_episode_ref_to_dto(state, e, source))
            .collect(),
        batch: s
            .batch
            .iter()
            .map(|e| anime_episode_ref_to_dto(state, e, source))
            .collect(),
    }
}

pub async fn anime_episode(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if !matches!(dec.source, Source::Otakudesu | Source::Otakudesufit) {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not otakudesu",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let ep = match &result.content {
        Some(ContentModel::AnimeEpisode(e)) => e,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield an anime episode",
                started,
                &rid,
            )
        }
    };
    let dto = anime_episode_to_dto_src(&state, ep, &id, dec.source);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn anime_episode_to_dto_src(
    state: &ApiState,
    e: &AnimeEpisode,
    id: &str,
    source: Source,
) -> AnimeEpisodeDto {
    AnimeEpisodeDto {
        id: id.to_string(),
        series_title: e.series_title.clone(),
        series_id: e
            .series_url
            .as_deref()
            .map(|u| state.codec.encode(source, Kind::Series, u)),
        episode_number: e.episode_number,
        prev_id: e
            .prev_episode
            .as_deref()
            .map(|u| state.codec.encode(source, Kind::Item, u)),
        next_id: e
            .next_episode
            .as_deref()
            .map(|u| state.codec.encode(source, Kind::Item, u)),
        default_embed: e.default_embed.clone(),
        mirrors: e
            .mirrors
            .iter()
            .map(|m| AnimeMirrorDto {
                name: m.name.clone(),
                quality: m.quality.clone(),
                // Sign the mirror token so the resolver can't be used as an
                // open relay. Payload = base64url("<episode_url>|<token>").
                stream_id: sign_anime_stream(state, &e.url, &m.token),
                default: m.default,
            })
            .collect(),
        downloads: e
            .downloads
            .iter()
            .map(|g| AnimeDownloadGroupDto {
                quality: g.quality.clone(),
                size: g.size.clone(),
                mirrors: g
                    .mirrors
                    .iter()
                    .map(|m| DownloadMirrorDto {
                        name: m.name.clone(),
                        url: m.url.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Sign a `<episode_url>|<data-content token>` pair into a `p`/`s` payload the
/// stream resolver verifies, reusing the image HMAC signer.
fn sign_anime_stream(state: &ApiState, episode_url: &str, token: &str) -> String {
    let raw = format!("{}|{}", episode_url, token);
    let payload = URL_SAFE_NO_PAD.encode(raw.as_bytes());
    let sig = state.codec.sign_image(&payload);
    format!("{}.{}", payload, sig)
}

#[derive(Debug, Deserialize)]
pub struct AnimeStreamQuery {
    /// `<payload>.<signature>` produced by `sign_anime_stream`.
    pub id: String,
}

/// Resolve a signed anime stream id into a playable embed URL.
///
/// Performs otakudesu's two-step `admin-ajax.php` handshake (fetch nonce, then
/// fetch the base64 embed HTML for the `{id,i,q}` token) and returns the
/// iframe `src`.
pub async fn anime_stream(
    State(state): State<ApiState>,
    Query(q): Query<AnimeStreamQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    let (payload, sig) = match q.id.split_once('.') {
        Some(p) => p,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Malformed stream id",
                started,
                &rid,
            )
        }
    };
    if !state.codec.verify_image(payload, sig) {
        return err(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "Stream id signature is invalid",
            started,
            &rid,
        );
    }
    let raw = match URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(s) => s,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Stream payload is not valid",
                started,
                &rid,
            )
        }
    };
    let (episode_url, token) = match raw.split_once('|') {
        Some(p) => p,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Stream payload missing token",
                started,
                &rid,
            )
        }
    };

    match resolve_anime_stream(&state, episode_url, token).await {
        Ok(embed) => ok(
            StatusCode::OK,
            serde_json::json!({ "type": "embed", "url": embed }),
            started,
            false,
            &rid,
        ),
        Err(e) => err(StatusCode::BAD_GATEWAY, "resolve_failed", e, started, &rid),
    }
}

/// Dispatch stream resolution by upstream host: otakudesu.fit encodes the
/// player as a base64 HTML fragment (decoded locally); otakudesu.blog uses a
/// two-step admin-ajax handshake.
async fn resolve_anime_stream(
    state: &ApiState,
    episode_url: &str,
    token: &str,
) -> Result<String, String> {
    if episode_url.contains("otakudesu.fit") {
        crate::adapters::otakudesufit::OtakudesufitAdapter::embed_from_token(token)
            .ok_or_else(|| "could not decode embed".to_string())
    } else {
        resolve_otakudesu_stream(state, episode_url, token).await
    }
}

/// Two-step otakudesu AJAX resolution: token -> embed iframe URL.
async fn resolve_otakudesu_stream(
    state: &ApiState,
    episode_url: &str,
    token: &str,
) -> Result<String, String> {
    use crate::adapters::otakudesu::{AJAX_NONCE_ACTION, AJAX_PATH, AJAX_STREAM_ACTION};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    // The token is base64 of `{"id":..,"i":..,"q":".."}`.
    let decoded = STANDARD
        .decode(token)
        .map_err(|_| "bad token".to_string())?;
    let obj: serde_json::Value =
        serde_json::from_slice(&decoded).map_err(|_| "bad token json".to_string())?;
    let id = obj.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let i = obj.get("i").and_then(|v| v.as_i64()).unwrap_or(0);
    let qlt = obj
        .get("q")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let ajax_url = "https://otakudesu.blog".to_string() + AJAX_PATH;
    let client = state.engine.client();
    let build_headers = |body_kind: &str| {
        let fp = BrowserFingerprint::for_url(episode_url);
        let mut h = fp.as_header_map();
        h.insert("Referer".to_string(), episode_url.to_string());
        h.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
        h.insert("Origin".to_string(), "https://otakudesu.blog".to_string());
        h.remove("Accept-Encoding");
        let _ = body_kind;
        h
    };

    // 1) nonce
    let nonce_hdrs = state
        .engine
        .pipeline()
        .build_headers(&ajax_url, None, Some(&build_headers("nonce")))
        .map_err(|e| e.to_string())?;
    let nonce_resp = client
        .post(&ajax_url)
        .headers(nonce_hdrs)
        .form(&[("action", AJAX_NONCE_ACTION)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let nonce_json: serde_json::Value = nonce_resp.json().await.map_err(|e| e.to_string())?;
    let nonce = nonce_json
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no nonce".to_string())?
        .to_string();

    // 2) resolve embed
    let stream_hdrs = state
        .engine
        .pipeline()
        .build_headers(&ajax_url, None, Some(&build_headers("stream")))
        .map_err(|e| e.to_string())?;
    let id_s = id.to_string();
    let i_s = i.to_string();
    let resp = client
        .post(&ajax_url)
        .headers(stream_hdrs)
        .form(&[
            ("id", id_s.as_str()),
            ("i", i_s.as_str()),
            ("q", qlt.as_str()),
            ("nonce", nonce.as_str()),
            ("action", AJAX_STREAM_ACTION),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let data = json
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no stream data".to_string())?;
    let fragment = STANDARD
        .decode(data)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| "bad stream fragment".to_string())?;
    crate::adapters::otakudesu::OtakudesuAdapter::embed_src_from_fragment(&fragment)
        .ok_or_else(|| "no embed url in fragment".to_string())
}

// ---- lmanime (Chinese anime / donghua, English & multi-sub) ---------------

/// `GET /api/v1/lmanime/{id}` — lmanime.com series detail (episode list).
pub async fn lmanime_series(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Lmanime {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not lmanime",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series = match &result.content {
        Some(ContentModel::AnimeSeries(s)) => s,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield an anime series",
                started,
                &rid,
            )
        }
    };
    let dto = anime_series_to_dto_src(&state, series, &id, Source::Lmanime);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// `GET /api/v1/lmanime/episode/{id}` — lmanime episode (servers + downloads).
pub async fn lmanime_episode(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Lmanime {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not lmanime",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let ep = match &result.content {
        Some(ContentModel::AnimeEpisode(e)) => e,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield an anime episode",
                started,
                &rid,
            )
        }
    };
    let dto = anime_episode_to_dto_src(&state, ep, &id, Source::Lmanime);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// `GET /api/v1/lmanime-stream?id=...` — resolve a signed lmanime server token
/// (a `/v/N/` page) into a playable embed URL by fetching it and pulling the
/// iframe `src`.
pub async fn lmanime_stream(
    State(state): State<ApiState>,
    Query(q): Query<AnimeStreamQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    let (payload, sig) = match q.id.split_once('.') {
        Some(p) => p,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Malformed stream id",
                started,
                &rid,
            )
        }
    };
    if !state.codec.verify_image(payload, sig) {
        return err(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "Stream id signature is invalid",
            started,
            &rid,
        );
    }
    let raw = match URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(s) => s,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Stream payload is not valid",
                started,
                &rid,
            )
        }
    };
    // Payload is "<episode_url>|<v-page url>"; only the v-page is needed.
    let v_url = raw.split_once('|').map(|(_, t)| t).unwrap_or(&raw);
    if !v_url.contains("lmanime.com") {
        return err(
            StatusCode::BAD_REQUEST,
            "bad_payload",
            "Stream target not allowed",
            started,
            &rid,
        );
    }

    match state.engine.fetch_html(v_url).await {
        Ok(html) => {
            let parser = crate::parser::HtmlParser::parse(&html);
            let embed = parser
                .attr("#pembed iframe", "src")
                .or_else(|| parser.attr(".player-embed iframe", "src"))
                .or_else(|| parser.attr("iframe", "src"));
            match embed {
                Some(src) => {
                    let url = if let Some(rest) = src.strip_prefix("//") {
                        format!("https://{}", rest)
                    } else {
                        src
                    };
                    ok(
                        StatusCode::OK,
                        serde_json::json!({ "type": "embed", "url": url }),
                        started,
                        false,
                        &rid,
                    )
                }
                None => err(
                    StatusCode::BAD_GATEWAY,
                    "resolve_failed",
                    "no embed url on server page".to_string(),
                    started,
                    &rid,
                ),
            }
        }
        Err(e) => err(
            StatusCode::BAD_GATEWAY,
            "resolve_failed",
            e.to_string(),
            started,
            &rid,
        ),
    }
}

// ---- lk21 (movies) --------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MovieDto {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    pub genres: Vec<String>,
    pub countries: Vec<String>,
    pub directors: Vec<String>,
    pub cast: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_url: Option<String>,
    /// Switchable player servers. The client picks one and calls
    /// `/api/v1/movie-stream/{id}?server={name}` to resolve it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<MovieServerDto>,
    /// "MOVIE TERKAIT" related suggestions (opaque movie IDs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<MovieRelatedDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    /// Original source page — the upstream player blocks third-party embedding
    /// (CSP frame-ancestors) and can't be proxied, so the UI opens this in a
    /// new tab to actually watch.
    pub watch_url: String,
}

#[derive(Debug, Serialize)]
pub struct MovieServerDto {
    pub name: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct MovieRelatedDto {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
}

/// `GET /api/v1/movie/{id}` — LayarKaca21 movie detail.
pub async fn lk21_movie(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Lk21 {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not lk21",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let movie = match &result.content {
        Some(ContentModel::Movie(m)) => m,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a movie",
                started,
                &rid,
            )
        }
    };
    let dto = movie_to_dto(&state, movie, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn movie_to_dto(state: &ApiState, m: &MovieDetail, id: &str) -> MovieDto {
    MovieDto {
        id: id.to_string(),
        title: m.title.clone().unwrap_or_default(),
        synopsis: m.synopsis.clone(),
        poster: proxy_opt(state, m.poster.as_deref()),
        year: m.year.clone(),
        rating: m.rating.clone(),
        quality: m.quality.clone(),
        duration: m.duration.clone(),
        genres: m.genres.clone(),
        countries: m.countries.clone(),
        directors: m.directors.clone(),
        cast: m.cast.clone(),
        release_date: m.release_date.clone(),
        embed_url: m.embed_url.clone(),
        servers: m
            .servers
            .iter()
            .map(|s| MovieServerDto {
                name: s.name.clone(),
                label: s.label.clone(),
            })
            .collect(),
        related: m
            .related
            .iter()
            .map(|r| MovieRelatedDto {
                id: state.codec.encode(Source::Lk21, Kind::Post, &r.url),
                title: r.title.clone(),
                poster: proxy_opt(state, r.poster.as_deref()),
                year: r.year.clone(),
            })
            .collect(),
        download_url: m.download_url.clone(),
        watch_url: m.url.clone(),
    }
}

#[derive(Debug, Serialize)]
pub struct NekopoiPostDto {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub genres: Vec<String>,
    /// Embeddable streaming servers; the client iframes the chosen one.
    pub servers: Vec<NekopoiServerDto>,
    /// Episode list when this is a multi-episode series (each links to a post).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<MovieRelatedDto>,
    /// Download links grouped by quality.
    pub downloads: Vec<DownloadGroupDto>,
    /// Related post suggestions (opaque nekopoi IDs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<MovieRelatedDto>,
    /// Original source page (fallback "open externally").
    pub source_url: String,
}

#[derive(Debug, Serialize)]
pub struct NekopoiServerDto {
    pub name: String,
    pub label: String,
    /// Directly embeddable player URL (these hosts allow iframing).
    pub embed_url: String,
}

/// `GET /api/v1/nekopoi/{id}` — NekoPoi adult-anime post detail (18+).
pub async fn nekopoi_post(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Nekopoi {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not nekopoi",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let post = match &result.content {
        Some(ContentModel::NekopoiPost(p)) => p,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a nekopoi post",
                started,
                &rid,
            )
        }
    };
    let dto = NekopoiPostDto {
        id: id.clone(),
        title: post.title.clone().unwrap_or_default(),
        synopsis: post.synopsis.clone(),
        cover: proxy_opt(&state, post.cover.as_deref()),
        date: post.date.clone(),
        genres: post.genres.clone(),
        servers: post
            .servers
            .iter()
            .map(|s| NekopoiServerDto {
                name: s.name.clone(),
                label: s.label.clone(),
                embed_url: s.embed_url.clone(),
            })
            .collect(),
        episodes: post
            .episodes
            .iter()
            .map(|e| MovieRelatedDto {
                id: state.codec.encode(Source::Nekopoi, Kind::Post, &e.url),
                title: e.title.clone(),
                poster: proxy_opt(&state, e.poster.as_deref()),
                year: e.year.clone(),
            })
            .collect(),
        downloads: post
            .downloads
            .iter()
            .map(|g| DownloadGroupDto {
                quality: g.quality.clone(),
                mirrors: g
                    .mirrors
                    .iter()
                    .map(|m| DownloadMirrorDto {
                        name: m.name.clone(),
                        url: m.url.clone(),
                    })
                    .collect(),
            })
            .collect(),
        related: post
            .related
            .iter()
            .map(|r| MovieRelatedDto {
                id: state.codec.encode(Source::Nekopoi, Kind::Post, &r.url),
                title: r.title.clone(),
                poster: proxy_opt(&state, r.poster.as_deref()),
                year: r.year.clone(),
            })
            .collect(),
        source_url: post.url.clone(),
    };
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// Query for the opaque-ID rehydration endpoint.
#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    /// base64url(raw provider URL) — the (secret-independent) payload segment
    /// of an opaque ID.
    pub u: String,
    /// Source short code ("mb", "ac", "lk", ...).
    pub source: String,
    /// Kind short code ('s' series / 'i' item / 'p' post). Defaults to series.
    #[serde(default)]
    pub kind: Option<String>,
}

/// `GET /api/v1/resolve?source=&kind=&u=` — re-sign a known provider URL into a
/// fresh opaque ID.
///
/// This lets the web app self-heal a saved favorite / history entry whose
/// opaque ID became invalid (e.g. the signing secret changed across a
/// redeploy). It is **not** an open URL signer: the decoded URL must resolve to
/// the claimed source's own host (`Source::detect`), so it can never be used to
/// mint IDs pointing at arbitrary or internal addresses.
pub async fn resolve(
    State(state): State<ApiState>,
    Query(q): Query<ResolveQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let source = match Source::from_short(&q.source) {
        Some(s) => s,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_source",
                "unknown source code",
                started,
                &rid,
            )
        }
    };
    let url = match URL_SAFE_NO_PAD
        .decode(q.u.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(u) => u,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "u is not valid base64url",
                started,
                &rid,
            )
        }
    };
    if Source::detect(&url) != Some(source) {
        return err(
            StatusCode::BAD_REQUEST,
            "host_mismatch",
            "URL host does not belong to the claimed source",
            started,
            &rid,
        );
    }
    let kind = q
        .kind
        .as_deref()
        .and_then(|s| s.chars().next())
        .and_then(Kind::from_short)
        .unwrap_or(Kind::Series);
    let id = state.codec.encode(source, kind, &url);
    ok(
        StatusCode::OK,
        serde_json::json!({ "id": id, "source": source.short_code(), "kind": kind.short_code().to_string() }),
        started,
        false,
        &rid,
    )
}

/// Extract the player token id from a playeriframe embed URL
/// (`/iframe/p2p/<ID>` path segment, or a `?id=<ID>` query param).
fn extract_player_id(embed: &str) -> Option<String> {
    let u = url::Url::parse(embed).ok()?;
    if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "id") {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    u.path_segments()?
        .rfind(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Brutal-sniff a playable HLS URL for an lk21 movie embed.
///
/// The lk21 player chain is: `playeriframe.sbs/iframe/p2p/<ID>` →
/// `cloud.hownetwork.xyz/video.php?id=<ID>` (jwplayer) whose `init.min.js`
/// POSTs `api2.php?id=<ID>` with `{r: referrer, d: hostname}` and gets back
/// `{ "file": "<master .m3u8>", "type": "hls" }`. We replicate that POST
/// server-side (the CDN is Referer-locked) and return the master playlist.
async fn resolve_lk21_stream(state: &ApiState, embed_url: &str) -> Result<String, String> {
    let id = extract_player_id(embed_url).ok_or_else(|| "no player id in embed".to_string())?;
    let api = format!("https://cloud.hownetwork.xyz/api2.php?id={}", id);

    let fp = BrowserFingerprint::for_url(&api);
    let mut hdrs = fp.as_header_map();
    hdrs.insert(
        "Referer".to_string(),
        "https://playeriframe.sbs/".to_string(),
    );
    hdrs.insert(
        "Origin".to_string(),
        "https://cloud.hownetwork.xyz".to_string(),
    );
    hdrs.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
    hdrs.insert(
        "Accept".to_string(),
        "application/json, text/plain, */*".to_string(),
    );
    hdrs.remove("Accept-Encoding");
    hdrs.remove("Upgrade-Insecure-Requests");
    let headers = state
        .engine
        .pipeline()
        .build_headers(&api, None, Some(&hdrs))
        .map_err(|e| e.to_string())?;

    let resp = state
        .engine
        .client()
        .post(&api)
        .headers(headers)
        .form(&[
            ("r", "https://playeriframe.sbs/"),
            ("d", "playeriframe.sbs"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("api2 returned {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let file = json
        .get("file")
        .and_then(|v| v.as_str())
        .filter(|s| s.contains(".m3u8"))
        .ok_or_else(|| "no playable file in api2 response".to_string())?;
    Ok(file.to_string())
}

/// `GET /api/v1/movie-stream/{id}?server={name}` — resolve an lk21 movie into
/// a playable source. P2P (the hownetwork chain) is sniffed to a proxied HLS
/// master playlist and played inline; the other servers (turbovip / cast /
/// hydrax) are their own embeddable players, so we unwrap the ad-laden
/// `playeriframe.sbs` shell to the real inner iframe and hand that to the UI.
#[derive(Debug, Deserialize)]
pub struct MovieStreamQuery {
    #[serde(default)]
    pub server: Option<String>,
}

/// Unwrap a `playeriframe.sbs/iframe/<server>/<id>` shell to its real inner
/// player iframe (e.g. `emturbovid.com/t/<id>`, `abyssplayer.com/<id>`).
async fn resolve_inner_embed(state: &ApiState, wrapper: &str) -> Result<String, String> {
    static INNER_IFRAME_RE: Lazy<regex::Regex> = Lazy::new(|| {
        regex::Regex::new(r#"(?is)embed-container.*?<iframe[^>]*\bsrc=["']([^"']+)["']"#).unwrap()
    });
    static ANY_IFRAME_RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r#"(?is)<iframe[^>]*\bsrc=["']([^"']+)["']"#).unwrap());

    let fp = BrowserFingerprint::for_url(wrapper);
    let mut hdrs = fp.as_header_map();
    hdrs.insert(
        "Referer".to_string(),
        "https://tv11.lk21official.cc/".to_string(),
    );
    hdrs.remove("Accept-Encoding");
    let headers = state
        .engine
        .pipeline()
        .build_headers(wrapper, None, Some(&hdrs))
        .map_err(|e| e.to_string())?;
    let html = state
        .engine
        .client()
        .get(wrapper)
        .headers(headers)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let inner = INNER_IFRAME_RE
        .captures(&html)
        .or_else(|| ANY_IFRAME_RE.captures(&html))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| "no inner player iframe found".to_string())?;
    let inner = if let Some(rest) = inner.strip_prefix("//") {
        format!("https://{}", rest)
    } else {
        inner
    };
    Ok(inner)
}

pub async fn movie_stream(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(q): Query<MovieStreamQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Lk21 {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not lk21",
            started,
            &rid,
        );
    }
    let (result, _cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let movie = match &result.content {
        Some(ContentModel::Movie(m)) => m,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a movie",
                started,
                &rid,
            )
        }
    };

    // Pick the requested server, else the first available, else the default
    // embed. P2P is identified by name/URL and sniffed to HLS; the rest are
    // unwrapped to their inner embeddable player.
    let want = q.server.as_deref().map(|s| s.to_lowercase());
    let chosen = want
        .as_deref()
        .and_then(|n| movie.servers.iter().find(|s| s.name == n))
        .or_else(|| movie.servers.iter().find(|s| s.name == "p2p"))
        .or_else(|| movie.servers.first());

    let (embed, is_p2p) = match chosen {
        Some(s) => (
            s.embed_url.clone(),
            s.name == "p2p" || s.embed_url.contains("/p2p/"),
        ),
        None => match &movie.embed_url {
            Some(e) => (e.clone(), e.contains("/p2p/") || e.contains("hownetwork")),
            None => {
                return err(
                    StatusCode::BAD_GATEWAY,
                    "no_embed",
                    "Movie has no player embed",
                    started,
                    &rid,
                )
            }
        },
    };

    if is_p2p {
        if let Ok(master) = resolve_lk21_stream(&state, &embed).await {
            let hls = signed_hls_url(&state, &master);
            return ok(
                StatusCode::OK,
                serde_json::json!({ "type": "hls", "url": hls }),
                started,
                false,
                &rid,
            );
        }
        // Fall through to an iframe unwrap if the HLS sniff fails.
    }

    match resolve_inner_embed(&state, &embed).await {
        Ok(inner) => ok(
            StatusCode::OK,
            serde_json::json!({ "type": "iframe", "url": inner }),
            started,
            false,
            &rid,
        ),
        Err(e) => err(StatusCode::BAD_GATEWAY, "resolve_failed", e, started, &rid),
    }
}

pub async fn donghua_episode(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Anichin {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not anichin",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let ep = match &result.content {
        Some(ContentModel::DonghuaEpisode(e)) => e,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a donghua episode",
                started,
                &rid,
            )
        }
    };
    let dto = donghua_episode_to_dto(&state, ep, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn donghua_episode_to_dto(state: &ApiState, e: &DonghuaEpisode, id: &str) -> DonghuaEpisodeDto {
    DonghuaEpisodeDto {
        id: id.to_string(),
        series_title: e.series_title.clone(),
        series_id: e
            .series_url
            .as_deref()
            .map(|u| state.codec.encode(Source::Anichin, Kind::Series, u)),
        episode_number: e.episode_number,
        prev_id: e
            .prev_episode
            .as_deref()
            .map(|u| state.codec.encode(Source::Anichin, Kind::Item, u)),
        next_id: e
            .next_episode
            .as_deref()
            .map(|u| state.codec.encode(Source::Anichin, Kind::Item, u)),
        servers: e
            .sources
            .iter()
            .map(|s| DonghuaServer {
                label: s.quality.clone().unwrap_or_else(|| "Server".to_string()),
                embed_url: s.url.clone(),
                format: s.format.clone(),
            })
            .collect(),
        downloads: e
            .downloads
            .iter()
            .map(|g| DownloadGroupDto {
                quality: g.quality.clone(),
                mirrors: g
                    .mirrors
                    .iter()
                    .map(|m| DownloadMirrorDto {
                        name: m.name.clone(),
                        url: m.url.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

// ---- Cosplay --------------------------------------------------------------

pub async fn cosplay_post(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Cosplaytele {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not cosplaytele",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let cp = match &result.content {
        Some(ContentModel::CosplayPost(c)) => c,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a cosplay post",
                started,
                &rid,
            )
        }
    };
    let dto = cosplay_to_dto(&state, cp, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn cosplay_to_dto(state: &ApiState, c: &CosplayPost, id: &str) -> CosplayPostDto {
    CosplayPostDto {
        id: id.to_string(),
        title: c.title.clone().unwrap_or_default(),
        cosplayer: c.cosplayer.clone(),
        character: c.character.clone(),
        series: c.series.clone(),
        photo_count: c.photo_count,
        video_count: c.video_count,
        categories: c.categories.clone(),
        tags: c.tags.clone(),
        author: c.author.clone(),
        published_at: c.published_at.clone(),
        cover: proxy_opt(state, c.cover_image.as_deref()),
        images: c.images.iter().map(|u| proxy_url(state, u)).collect(),
        // Videos: cossora.stream embeds are turned into a signed resolver URL
        // (`/api/v1/cosplay/video?...`) the frontend resolves to an HLS
        // stream and plays with hls.js. Direct files / other embeds pass
        // through raw.
        videos: c
            .videos
            .iter()
            .map(|u| {
                if crate::web::cossora::is_cossora_embed(u) {
                    signed_cosplay_video_url(state, u)
                } else {
                    u.clone()
                }
            })
            .collect(),
        downloads: c
            .download_links
            .iter()
            .map(|m| DownloadMirrorDto {
                name: m.name.clone(),
                url: m.url.clone(),
            })
            .collect(),
        unzip_password: c.unzip_password.clone(),
        recommendations: c
            .recommendations
            .iter()
            .map(|r| {
                raw_search_to_dto(
                    state,
                    SearchResultItem {
                        source: "cosplaytele".to_string(),
                        title: r.title.clone(),
                        url: r.url.clone(),
                        thumbnail: r.thumbnail.clone(),
                        kind: Some("cosplay_post".to_string()),
                        snippet: None,
                        tags: Vec::new(),
                        cosplayer: None,
                        character: None,
                        series: None,
                    },
                )
            })
            .collect(),
    }
}

// ---- Cosplay video (cossora.stream embed) ---------------------------------

#[derive(Debug, Deserialize)]
pub struct CosplayVideoQuery {
    /// Signed payload (base64 of the embed URL).
    pub p: String,
    /// HMAC signature of `p`.
    pub s: String,
}

/// Resolve a Cosplaytele video embed into a playable HLS stream.
///
/// Cosplaytele videos come from `cossora.stream/embed/<id>`, which (a) only
/// serves content when the Referer is `cosplaytele.com`, so a plain browser
/// iframe gets "Unknown Error xD", and (b) AES-encrypts the real `.m3u8` URL
/// in the page. We fetch the embed with the right Referer, decrypt the URL
/// server-side, then hand back a `/hls` proxy URL the browser can play with
/// hls.js. The playlist token is locked to our IP, so it must be proxied.
pub async fn cosplay_video(
    State(state): State<ApiState>,
    Query(q): Query<CosplayVideoQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    if !state.codec.verify_image(&q.p, &q.s) {
        return err(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "Video resolver signature is invalid",
            started,
            &rid,
        );
    }
    let embed_url = match decode_signed_url(&q.p) {
        Some(u) => u,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Embed payload is not valid",
                started,
                &rid,
            )
        }
    };
    if !crate::web::cossora::is_cossora_embed(&embed_url) {
        return err(
            StatusCode::BAD_REQUEST,
            "unsupported_embed",
            "Embed host is not supported",
            started,
            &rid,
        );
    }

    // Fetch the embed page (cached) with a cosplaytele Referer.
    let html = match fetch_text_with_referer(&state, &embed_url, "https://cosplaytele.com/").await {
        Ok(h) => h,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "embed_fetch_failed",
                e,
                started,
                &rid,
            )
        }
    };
    let master = match crate::web::cossora::resolve_master_from_html(&html) {
        Some(m) => m,
        None => {
            return err(
                StatusCode::BAD_GATEWAY,
                "resolve_failed",
                "Could not resolve a playable stream from the embed",
                started,
                &rid,
            )
        }
    };

    // Hand back a proxied HLS URL (playlist is IP-locked to us).
    let hls = signed_hls_url(&state, &master);
    ok(
        StatusCode::OK,
        serde_json::json!({ "type": "hls", "url": hls }),
        started,
        false,
        &rid,
    )
}

/// Proxy an HLS playlist or segment for the cossora stream.
///
/// `.m3u8` playlists are rewritten so every nested playlist / segment URL is
/// re-signed and routed back through this proxy (keeping the IP that fetches
/// the token-locked playlists equal to ours). Binary segments are streamed
/// through unchanged.
pub async fn hls_proxy(
    State(state): State<ApiState>,
    Query(q): Query<CosplayVideoQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    if !state.codec.verify_image(&q.p, &q.s) {
        return err(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "HLS proxy signature is invalid",
            started,
            &rid,
        );
    }
    let url = match decode_signed_url(&q.p) {
        Some(u) => u,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "HLS payload is not valid",
                started,
                &rid,
            )
        }
    };
    if is_blocked_hls_target(&url) {
        return err(
            StatusCode::FORBIDDEN,
            "host_not_allowed",
            "HLS proxy will not fetch this host",
            started,
            &rid,
        );
    }

    let fp = BrowserFingerprint::for_url(&url);
    let mut hdrs = fp.as_header_map();
    // Referer is host-specific. cossora playlists are locked to cossora.stream;
    // everything else through this proxy is the lk21 chain (hownetwork playlists
    // + rotating segment CDNs), all of which expect the hownetwork referer.
    let host = url::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .unwrap_or_default();
    let referer = if host.contains("cossora") {
        "https://cossora.stream/".to_string()
    } else {
        "https://cloud.hownetwork.xyz/".to_string()
    };
    hdrs.insert("Referer".to_string(), referer);
    // Let reqwest manage compression so playlists are auto-decompressed.
    hdrs.remove("Accept-Encoding");
    let headers = match state
        .engine
        .pipeline()
        .build_headers(&url, None, Some(&hdrs))
    {
        Ok(h) => h,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "header_build",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    let resp = match state
        .engine
        .client()
        .get(&url)
        .headers(headers)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if !resp.status().is_success() {
        return err(
            StatusCode::BAD_GATEWAY,
            "upstream_status",
            format!("Upstream returned {}", resp.status()),
            started,
            &rid,
        );
    }
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let is_playlist = url.contains(".m3u8")
        || content_type.contains("mpegurl")
        || content_type.contains("vnd.apple");

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "upstream_body",
                e.to_string(),
                started,
                &rid,
            )
        }
    };

    if is_playlist {
        // Rewrite URLs in the playlist to route back through this proxy.
        // cossora serves segments CORS-open (fetched direct by the browser);
        // the lk21 chain (everything else here) locks segments to the
        // hownetwork referer + rotates their host, so segments must be proxied.
        let proxy_segments = !host.contains("cossora");
        let text = String::from_utf8_lossy(&bytes);
        let rewritten = rewrite_m3u8(&state, &text, &url, proxy_segments);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.apple.mpegurl"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        return (StatusCode::OK, headers, rewritten).into_response();
    }

    // Binary segment / key: stream through with a long cache.
    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    (StatusCode::OK, headers, bytes).into_response()
}

/// Rewrite URLs in an m3u8 playlist.
///
/// Bandwidth-saving policy: only **nested playlists** (`.m3u8`) are routed
/// back through our `/hls` proxy — they are tiny, token-locked to our IP, and
/// served without CORS, so the browser can't fetch them directly. Heavy media
/// **segments** (`.ts`/`.m4s`/`.aac`) and encryption keys are rewritten to
/// their **absolute** CDN URLs and fetched **directly by the client** (the
/// cossora CDN serves them with `Access-Control-Allow-Origin: *` and no
/// token). This keeps all the large traffic client<->CDN, off our server.
fn rewrite_m3u8(state: &ApiState, text: &str, base: &str, proxy_segments: bool) -> String {
    let base_url = url::Url::parse(base).ok();
    let absolutize = |raw: &str| -> Option<String> {
        if raw.starts_with("http://") || raw.starts_with("https://") {
            Some(raw.to_string())
        } else {
            Some(base_url.as_ref()?.join(raw).ok()?.to_string())
        }
    };
    // A resource is a (sub)playlist if it points at an .m3u8.
    let is_playlist_ref = |abs: &str| -> bool {
        let path = abs.split(['?', '#']).next().unwrap_or(abs);
        path.to_lowercase().ends_with(".m3u8")
    };
    // Playlists -> always proxied + signed. Segments -> proxied when
    // `proxy_segments` (origin-locked CDN), otherwise direct absolute URL.
    let resolve = |raw: &str| -> Option<String> {
        let abs = absolutize(raw)?;
        if is_playlist_ref(&abs) || proxy_segments {
            Some(signed_hls_url(state, &abs))
        } else {
            Some(abs)
        }
    };

    let mut out = String::with_capacity(text.len() + 256);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        if trimmed.starts_with('#') {
            // Rewrite URI="..." attributes (keys, maps, media renditions).
            // Keys/maps resolve to direct CDN URLs; nested media playlists are
            // proxied. `resolve` handles that distinction.
            if let Some(rewritten) = rewrite_uri_attr(line, &resolve) {
                out.push_str(&rewritten);
            } else {
                out.push_str(line);
            }
            out.push('\n');
            continue;
        }
        // Plain resource line (nested playlist or segment).
        match resolve(trimmed) {
            Some(u) => out.push_str(&u),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Rewrite a `URI="..."` attribute inside an m3u8 tag line, if present.
fn rewrite_uri_attr<F>(line: &str, resolve: &F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    let key = "URI=\"";
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    let raw = &rest[..end];
    let new = resolve(raw)?;
    Some(format!("{}{}{}", &line[..start], new, &rest[end..]))
}

/// SSRF guard for the HLS proxy.
///
/// Every `/hls` URL reaching this point has already been HMAC-verified — we
/// only ever sign URLs we ourselves produced while resolving a stream or
/// rewriting a playlist fetched from a trusted upstream (cossora / hownetwork).
/// The remaining risk is a malicious upstream playlist pointing us at an
/// internal address, so rather than a static host allowlist — unworkable
/// against the lk21 segment CDN, which rotates every segment across throwaway
/// domains (`qornexia.xyz`, `blaytoro.xyz`, `zenvokar.xyz`, ...) — we allow any
/// *public* host and refuse only loopback / private / link-local / ULA / CGNAT
/// targets and obvious internal names.
fn is_blocked_hls_target(url: &str) -> bool {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return true,
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return true;
    }
    match parsed.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || d.ends_with(".lan")
                || d.ends_with(".home")
        }
        Some(url::Host::Ipv4(ip)) => {
            let o = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_documentation()
                // CGNAT 100.64.0.0/10
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
        }
        Some(url::Host::Ipv6(ip)) => {
            let s = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                // unique-local fc00::/7
                || (s[0] & 0xfe00) == 0xfc00
                // link-local fe80::/10
                || (s[0] & 0xffc0) == 0xfe80
        }
        None => true,
    }
}

/// Build a signed `/hls?p=&s=` proxy URL for an absolute media URL.
fn signed_hls_url(state: &ApiState, raw: &str) -> String {
    let payload = URL_SAFE_NO_PAD.encode(raw.as_bytes());
    let sig = state.codec.sign_image(&payload);
    format!("/hls?p={}&s={}", payload, sig)
}

/// Build a signed `/api/v1/cosplay-video?p=&s=` resolver URL for an embed.
pub fn signed_cosplay_video_url(state: &ApiState, embed_url: &str) -> String {
    let payload = URL_SAFE_NO_PAD.encode(embed_url.as_bytes());
    let sig = state.codec.sign_image(&payload);
    format!("/api/v1/cosplay-video?p={}&s={}", payload, sig)
}

/// Decode a base64url payload (as produced by the signers) back to a string.
fn decode_signed_url(payload: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    String::from_utf8(bytes).ok()
}

/// Fetch a URL as text using a browser fingerprint and a specific Referer,
/// with single-flight caching keyed on the URL.
async fn fetch_text_with_referer(
    state: &ApiState,
    url: &str,
    referer: &str,
) -> Result<String, String> {
    let key = format!("text:{}", url);
    if let Some(arc) = state.cache.get(&key).await {
        if let Some(ContentModel::JsonApi(j)) = &arc.content {
            if let Some(s) = j.data.as_str() {
                return Ok(s.to_string());
            }
        }
    }

    let fp = BrowserFingerprint::for_url(url);
    let mut hdrs = fp.as_header_map();
    hdrs.insert("Referer".to_string(), referer.to_string());
    // Drop the manual Accept-Encoding so reqwest manages compression and
    // auto-decompresses the body. If we send our own Accept-Encoding, reqwest
    // hands back the raw (gzip/br/zstd) bytes and `text()` yields garbage —
    // which made some cossora embeds fail to parse ("resolve_failed").
    hdrs.remove("Accept-Encoding");
    let headers = state
        .engine
        .pipeline()
        .build_headers(url, None, Some(&hdrs))
        .map_err(|e| e.to_string())?;
    let resp = state
        .engine
        .client()
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("upstream returned {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;

    // Cache the HTML body (short TTL via the scrape cache) wrapped as JsonApi.
    let arc = Arc::new(ScrapeResult {
        url: url.to_string(),
        success: true,
        adapter_used: Some("cossora".to_string()),
        content: Some(ContentModel::JsonApi(crate::models::JsonApiResponse {
            url: url.to_string(),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            data: serde_json::Value::String(text.clone()),
        })),
        deep: None,
        error: None,
        elapsed_ms: 0,
    });
    state.cache.insert(key, arc).await;
    Ok(text)
}

// ---- Novel (novelid.org) -------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChapterPageQuery {
    /// Chapter page (1-indexed). Each page returns `size` chapters.
    #[serde(default = "default_page")]
    pub page: u32,
    /// Chapters per page (default 50, max 200).
    #[serde(default)]
    pub size: Option<u32>,
}

pub async fn novel_series(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(pq): Query<ChapterPageQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Novelid {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not novelid",
            started,
            &rid,
        );
    }

    // Always scrape page 1 to get the metadata + first 30 chapters.
    let (result, mut cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series_p1 = match &result.content {
        Some(ContentModel::NovelSeries(s)) => s.clone(),
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a novel series",
                started,
                &rid,
            )
        }
    };

    // Default per-API-page size
    let api_size = pq.size.unwrap_or(30).clamp(1, 200);
    let api_page = pq.page.max(1);

    // If upstream is not paginated, just slice the in-memory list.
    if !series_p1.chapters_paginated_upstream {
        let dto = novel_series_to_dto(&state, &series_p1, &id, api_page, Some(api_size));
        return ok(StatusCode::OK, dto, started, cached, &rid);
    }

    // Upstream IS paginated. Compute which upstream pages we need to fetch.
    let upstream_per_page = series_p1.upstream_chapters_per_page.unwrap_or(30).max(1);
    let upstream_total = series_p1.upstream_total_pages.unwrap_or(1).max(1);

    // Window in absolute chapter indices (0-based)
    let start_idx: u32 = (api_page - 1) * api_size;
    let end_idx: u32 = start_idx + api_size; // exclusive

    // Map to upstream page numbers (1-based)
    let first_upstream = (start_idx / upstream_per_page) + 1;
    let last_upstream_inclusive = ((end_idx - 1) / upstream_per_page) + 1;
    let last_upstream = last_upstream_inclusive.min(upstream_total);

    // Edge case: requested window is past the end
    if first_upstream > upstream_total {
        // Compute an estimate of total chapter count using upstream_total
        // and the current page1 count (if upstream_total == 1, total = chapters.len()).
        let est_total = estimated_total(&series_p1);
        let dto = build_novel_dto_with_pagination(
            &state,
            &series_p1,
            &id,
            api_page,
            api_size,
            Vec::new(),
            est_total,
        );
        return ok(StatusCode::OK, dto, started, cached, &rid);
    }

    // Fetch the additional upstream pages we don't already have
    // (page 1's chapters live in `series_p1.chapters`).
    let mut upstream_pages_to_fetch: Vec<u32> = (first_upstream..=last_upstream)
        .filter(|p| *p != 1)
        .collect();

    // Always fetch the last upstream page when we need an accurate total
    // (cheap: it's cached after the first time anyone hits the novel).
    let want_accurate_total =
        upstream_total > 1 && !upstream_pages_to_fetch.contains(&upstream_total);
    if want_accurate_total {
        upstream_pages_to_fetch.push(upstream_total);
    }

    let canonical = dec.url.clone();
    let fetched = fetch_upstream_chapter_pages(&state, &canonical, &upstream_pages_to_fetch).await;

    // Did everything come from cache?
    if cached && fetched.iter().any(|(_, _, was_cached)| !*was_cached) {
        cached = false;
    }

    // Build a flat sorted chapter list combining page 1's chapters with the
    // fetched upstream pages.
    let mut all_chapters: Vec<NovelChapterRef> = series_p1.chapters.clone();
    let mut last_page_count: Option<u32> = None;
    for (p, s, _) in fetched {
        if let Some(s) = s {
            // If this is the last upstream page, remember its chapter count
            // so we can compute the accurate total.
            if p == upstream_total {
                last_page_count = Some(s.chapters.len() as u32);
            }
            all_chapters.extend(s.chapters);
        }
    }
    // Dedup by number, sort
    all_chapters.sort_by_key(|c| c.number);
    all_chapters.dedup_by_key(|c| c.number);

    // Compute true total when available
    let est_total: usize = if let Some(last_count) = last_page_count {
        ((upstream_total.saturating_sub(1)) * upstream_per_page + last_count) as usize
    } else if upstream_total <= 1 {
        all_chapters.len()
    } else {
        // No accurate count yet — estimate as upstream_total * per_page
        // minus an over-estimate (we trim the page-1 set to accuracy when
        // we've actually fetched it).
        (upstream_total * upstream_per_page) as usize
    };

    // Slice the requested window by *chapter number* (rather than vec
    // index). novelid numbers chapters contiguously 1..total, but we may
    // have fetched the last page in addition to the requested page so the
    // accumulated list has gaps. We pick chapters whose `number` falls in
    // the inclusive range [start_idx + 1 .. end_idx].
    let start_num = start_idx + 1;
    let end_num = end_idx; // inclusive, end_idx = start_idx + api_size
    let window: Vec<NovelChapterRef> = all_chapters
        .into_iter()
        .filter(|c| c.number >= start_num && c.number <= end_num)
        .collect();

    let dto = build_novel_dto_with_pagination(
        &state, &series_p1, &id, api_page, api_size, window, est_total,
    );
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// Estimate total chapter count when only page-1 data is known.
fn estimated_total(s: &NovelSeries) -> usize {
    if !s.chapters_paginated_upstream {
        return s.chapters.len();
    }
    let per_page = s.upstream_chapters_per_page.unwrap_or(30) as usize;
    let total_pages = s.upstream_total_pages.unwrap_or(1) as usize;
    (per_page * total_pages).max(s.chapters.len())
}

/// Fetch a list of upstream chapter-list pages for a given canonical novel URL.
/// Returns a Vec of `(upstream_page_number, parsed page, was_cached)` entries.
async fn fetch_upstream_chapter_pages(
    state: &ApiState,
    canonical: &str,
    pages: &[u32],
) -> Vec<(u32, Option<NovelSeries>, bool)> {
    use futures::future::join_all;

    let futures = pages.iter().copied().map(|p| {
        let url = crate::adapters::novelid::NovelidAdapter::detail_url_for_page(canonical, p);
        let state = state.clone();
        async move {
            match cached_scrape(&state, &url).await {
                Ok((arc, cached)) => match &arc.content {
                    Some(ContentModel::NovelSeries(s)) => (p, Some(s.clone()), cached),
                    _ => (p, None, cached),
                },
                Err(_) => (p, None, false),
            }
        }
    });
    join_all(futures).await
}

fn build_novel_dto_with_pagination(
    state: &ApiState,
    s: &NovelSeries,
    id: &str,
    api_page: u32,
    api_size: u32,
    window: Vec<NovelChapterRef>,
    total: usize,
) -> NovelSeriesDto {
    let chapters = window
        .iter()
        .map(|c| novel_chapter_ref_to_dto(state, c))
        .collect::<Vec<_>>();
    let size = (api_size.max(1)) as usize;
    let total_pages = if total == 0 { 1 } else { total.div_ceil(size) };
    NovelSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        author: s.author.clone(),
        status: s.status.clone(),
        genres: s.genres.clone(),
        synopsis: s.synopsis.clone(),
        cover: proxy_opt(state, s.cover_image.as_deref()),
        rating: s.rating.clone(),
        chapter_count: total,
        chapter_page: api_page,
        chapter_page_size: api_size,
        chapter_total_pages: total_pages as u32,
        chapters,
    }
}

fn novel_series_to_dto(
    state: &ApiState,
    s: &NovelSeries,
    id: &str,
    page: u32,
    size: Option<u32>,
) -> NovelSeriesDto {
    let total = s.chapters.len();
    let size = size.unwrap_or(50).clamp(1, 200) as usize;
    let p = page.max(1) as usize;
    let start = (p - 1) * size;
    let end = (start + size).min(total);
    let window: Vec<&NovelChapterRef> = if start >= total {
        Vec::new()
    } else {
        s.chapters[start..end].iter().collect()
    };
    let chapters = window
        .iter()
        .map(|c| novel_chapter_ref_to_dto(state, c))
        .collect::<Vec<_>>();
    let total_pages = if total == 0 { 1 } else { total.div_ceil(size) };
    NovelSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        author: s.author.clone(),
        status: s.status.clone(),
        genres: s.genres.clone(),
        synopsis: s.synopsis.clone(),
        cover: proxy_opt(state, s.cover_image.as_deref()),
        rating: s.rating.clone(),
        chapter_count: total,
        chapter_page: p as u32,
        chapter_page_size: size as u32,
        chapter_total_pages: total_pages as u32,
        chapters,
    }
}

fn novel_chapter_ref_to_dto(state: &ApiState, c: &NovelChapterRef) -> NovelChapterRefDto {
    NovelChapterRefDto {
        id: state.codec.encode(Source::Novelid, Kind::Item, &c.url),
        number: c.number,
        title: c.title.clone(),
    }
}

pub async fn novel_chapter(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Novelid {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not novelid",
            started,
            &rid,
        );
    }
    let (result, cached) = match cached_scrape(&state, &dec.url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let chap = match &result.content {
        Some(ContentModel::NovelChapter(c)) => c,
        _ => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield a novel chapter",
                started,
                &rid,
            )
        }
    };
    let dto = novel_chapter_to_dto(&state, chap, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn novel_chapter_to_dto(state: &ApiState, c: &NovelChapter, id: &str) -> NovelChapterDto {
    let word_count = c.body.split_whitespace().count();
    NovelChapterDto {
        id: id.to_string(),
        series_title: c.series_title.clone(),
        series_id: c
            .series_url
            .as_deref()
            .map(|u| state.codec.encode(Source::Novelid, Kind::Series, u)),
        chapter_number: c.chapter_number,
        chapter_title: c.chapter_title.clone(),
        body: c.body.clone(),
        body_html: c.body_html.clone(),
        prev_id: c
            .prev_url
            .as_deref()
            .map(|u| state.codec.encode(Source::Novelid, Kind::Item, u)),
        next_id: c
            .next_url
            .as_deref()
            .map(|u| state.codec.encode(Source::Novelid, Kind::Item, u)),
        word_count,
    }
}

// ---- nhentai --------------------------------------------------------------

/// nhentai gallery as a series (cover + chapter list with one entry).
pub async fn nhentai_gallery(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Nhentai {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not nhentai",
            started,
            &rid,
        );
    }
    // Always go through the JSON API URL — even if the opaque carries the
    // browser-facing /g/<id>/ URL, normalise to the API endpoint.
    let api_url = match crate::adapters::nhentai::NhentaiAdapter::gallery_id_from_url(&dec.url) {
        Some(gid) => crate::adapters::nhentai::NhentaiAdapter::api_url_for_gallery(gid),
        None => dec.url.clone(),
    };

    let (json, cached) = match fetch_nhentai_json(&state, &api_url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let series = match crate::adapters::nhentai::NhentaiAdapter::parse_gallery_json(&dec.url, &json)
    {
        Some(s) => s,
        None => {
            return err(
                StatusCode::BAD_GATEWAY,
                "wrong_kind",
                "URL did not yield an nhentai gallery",
                started,
                &rid,
            )
        }
    };
    let dto = manga_series_to_dto_for_source(&state, &series, &id, Source::Nhentai);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// nhentai gallery as a chapter (direct page list).
pub async fn nhentai_chapter(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());
    let dec = match resolve_opaque(&state, &id) {
        Ok(d) => d,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if dec.source != Source::Nhentai {
        return err(
            StatusCode::BAD_REQUEST,
            "wrong_source",
            "ID source is not nhentai",
            started,
            &rid,
        );
    }
    let api_url = match crate::adapters::nhentai::NhentaiAdapter::gallery_id_from_url(&dec.url) {
        Some(gid) => crate::adapters::nhentai::NhentaiAdapter::api_url_for_gallery(gid),
        None => dec.url.clone(),
    };
    let (json, cached) = match fetch_nhentai_json(&state, &api_url).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, "scrape_failed", e, started, &rid),
    };
    let chap =
        match crate::adapters::nhentai::NhentaiAdapter::parse_gallery_as_chapter(&dec.url, &json) {
            Some(c) => c,
            None => {
                return err(
                    StatusCode::BAD_GATEWAY,
                    "wrong_kind",
                    "URL did not yield nhentai pages",
                    started,
                    &rid,
                )
            }
        };
    let dto = manga_chapter_to_dto(&state, &chap, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

/// Fetch the nhentai JSON API with browser-fingerprint headers and a small
/// in-memory single-flight cache.
async fn fetch_nhentai_json(
    state: &ApiState,
    api_url: &str,
) -> Result<(serde_json::Value, bool), String> {
    let key = format!("nhentai_json:{}", api_url);
    let cached = state.cache.get(&key).await.is_some();

    let url_owned = api_url.to_string();
    let state_clone = state.clone();
    let result = state
        .cache
        .try_get_with(key, async move {
            let json = call_nhentai_api(&state_clone, &url_owned).await?;
            // Wrap the JSON in a fake ScrapeResult so we can reuse the
            // existing scrape_cache type. We stash the JSON inside a
            // ContentModel::JsonApi.
            Ok::<Arc<ScrapeResult>, String>(Arc::new(ScrapeResult {
                url: url_owned.clone(),
                success: true,
                adapter_used: Some("nhentai".to_string()),
                content: Some(ContentModel::JsonApi(crate::models::JsonApiResponse {
                    url: url_owned.clone(),
                    status_code: 200,
                    content_type: Some("application/json".to_string()),
                    data: json,
                })),
                deep: None,
                error: None,
                elapsed_ms: 0,
            }))
        })
        .await
        .map_err(|e: Arc<String>| (*e).clone())?;

    let json = match &result.content {
        Some(ContentModel::JsonApi(j)) => j.data.clone(),
        _ => return Err("unexpected cached content".to_string()),
    };
    Ok((json, cached))
}

async fn call_nhentai_api(state: &ApiState, api_url: &str) -> Result<serde_json::Value, String> {
    let fp = BrowserFingerprint::for_url(api_url);
    let mut adapter_headers = fp.as_header_map();
    adapter_headers.insert("Referer".to_string(), "https://nhentai.net/".to_string());
    adapter_headers.insert(
        "Accept".to_string(),
        "application/json, text/plain, */*".to_string(),
    );
    adapter_headers.insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    adapter_headers.insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    adapter_headers.insert("Sec-Fetch-Site".to_string(), "same-origin".to_string());
    adapter_headers.remove("Upgrade-Insecure-Requests");

    let pipeline = state.engine.pipeline();
    let headers = pipeline
        .build_headers(api_url, None, Some(&adapter_headers))
        .map_err(|e| e.to_string())?;

    let resp = state
        .engine
        .client()
        .get(api_url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("upstream returned {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body)
}

/// Like `manga_series_to_dto`, but emits opaque IDs for the given source so
/// that nhentai chapter IDs encode `Source::Nhentai`.
fn manga_series_to_dto_for_source(
    state: &ApiState,
    s: &MangaSeries,
    id: &str,
    source: Source,
) -> MangaSeriesDto {
    let chapters = s
        .chapters
        .iter()
        .map(|c| MangaChapterRef {
            id: state.codec.encode(source, Kind::Item, &c.url),
            number: c.number,
            title: c.title.clone(),
            translations: c
                .translations
                .iter()
                .map(|t| MangaTranslationRef {
                    id: state.codec.encode(source, Kind::Item, &t.url),
                    language: t.language.clone(),
                    group: t.group.clone(),
                    date: t.date.clone(),
                    pages: t.pages,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let total = chapters.len();
    MangaSeriesDto {
        id: id.to_string(),
        title: s.title.clone().unwrap_or_default(),
        description: s.synopsis.clone(),
        author: s.author.clone(),
        artist: s.artist.clone(),
        genres: s.genres.clone(),
        cover: proxy_opt(state, s.cover_image.as_deref()),
        chapter_count: total,
        chapter_page: 1,
        chapter_page_size: total as u32,
        chapter_total_pages: 1,
        chapters,
    }
}

// ---- Image proxy ----------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ImgQuery {
    pub p: String,
    pub s: String,
}

pub async fn img_proxy(
    State(state): State<ApiState>,
    Query(q): Query<ImgQuery>,
    req: Request,
) -> Response {
    let started = Instant::now();
    let rid = req_id(req.headers());

    if !state.codec.verify_image(&q.p, &q.s) {
        return err(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "Image proxy signature is invalid",
            started,
            &rid,
        );
    }

    let url_bytes = match URL_SAFE_NO_PAD.decode(q.p.as_bytes()) {
        Ok(b) => b,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Image proxy payload is not valid base64url",
                started,
                &rid,
            )
        }
    };
    let url = match String::from_utf8(url_bytes) {
        Ok(u) => u,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_payload",
                "Image proxy payload is not valid utf-8",
                started,
                &rid,
            )
        }
    };

    // Allowlist: only fetch from known upstream hosts.
    if !is_allowed_image_host(&url) {
        return err(
            StatusCode::FORBIDDEN,
            "host_not_allowed",
            "Image proxy will not fetch this host",
            started,
            &rid,
        );
    }

    // Serve from the in-memory image cache when warm (instant re-views).
    if let Some(cached) = state.img_cache.get(&url).await {
        return image_response(&cached.0, cached.1.clone(), &rid);
    }

    let domain = match url::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
    {
        Some(d) => d,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad_url",
                "Could not parse image URL",
                started,
                &rid,
            )
        }
    };
    let pipeline = state.engine.pipeline();

    // Apply a coherent browser fingerprint and tailor it for an image
    // request. The Referer is set to the source domain so origin checks
    // pass (nhentai/cosplaytele block hotlinking otherwise).
    let fp = BrowserFingerprint::for_url(&url);
    let mut fp_headers = fp.as_image_headers();
    let referer = referer_for_host(&domain);
    fp_headers.insert("Referer".to_string(), referer.clone());

    let mut headers = match pipeline.build_headers(&url, None, Some(&fp_headers)) {
        Ok(h) => h,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                "header_build",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if let Ok(v) = HeaderValue::from_str(&referer) {
        headers.insert(header::REFERER, v);
    }

    let resp = match state
        .engine
        .client()
        .get(&url)
        .headers(headers)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                e.to_string(),
                started,
                &rid,
            )
        }
    };
    if !resp.status().is_success() {
        return err(
            StatusCode::BAD_GATEWAY,
            "upstream_status",
            format!("Upstream returned {}", resp.status()),
            started,
            &rid,
        );
    }
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "upstream_body",
                e.to_string(),
                started,
                &rid,
            )
        }
    };

    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    if let Ok(v) = HeaderValue::from_str(&rid) {
        headers.insert("x-request-id", v);
    }

    // Populate the image cache (skip oversized bodies to bound memory).
    if bytes.len() <= 8 * 1024 * 1024 {
        state
            .img_cache
            .insert(
                url.clone(),
                Arc::new((content_type.clone(), bytes.to_vec())),
            )
            .await;
    }

    (StatusCode::OK, headers, bytes).into_response()
}

/// Build an image proxy HTTP response from a content-type + body, with the
/// long-lived immutable cache headers used for all proxied art.
fn image_response(content_type: &str, body: Vec<u8>, rid: &str) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    if let Ok(v) = HeaderValue::from_str(rid) {
        headers.insert("x-request-id", v);
    }
    (StatusCode::OK, headers, body).into_response()
}

/// Pick the right Referer for a given upstream image host. Hotlink-protected
/// CDNs always check Referer against the parent site's origin, so we send the
/// user-facing domain rather than the bare CDN host.
fn referer_for_host(host: &str) -> String {
    let h = host.to_lowercase();
    if h.contains("nhentai.net")
        || h == "i1.nhentai.net"
        || h == "i2.nhentai.net"
        || h == "i3.nhentai.net"
        || h == "i4.nhentai.net"
        || h == "t1.nhentai.net"
        || h == "t2.nhentai.net"
        || h == "t3.nhentai.net"
        || h == "t4.nhentai.net"
    {
        return "https://nhentai.net/".to_string();
    }
    if h.contains("anichin.") || h.contains("wp.com") {
        return "https://anichin.cafe/".to_string();
    }
    if h.contains("cosplaytele.com") {
        return "https://cosplaytele.com/".to_string();
    }
    if h.contains("novelid.org") {
        return "https://novelid.org/".to_string();
    }
    if h.contains("otakudesu.fit") {
        return "https://otakudesu.fit/".to_string();
    }
    if h.contains("otakudesu.") {
        return "https://otakudesu.blog/".to_string();
    }
    if h.contains("lmanime.com") {
        return "https://lmanime.com/".to_string();
    }
    if h.contains("showcdnx.com") || h.contains("lk21") || h.contains("layarkaca") {
        return format!("{}/", crate::adapters::lk21::LK21_BASE);
    }
    if h.contains("nekopoi.") {
        return "https://nekopoi.care/".to_string();
    }
    if h.contains("mangaball.net")
        || h.contains("poke-black-and-white.net")
        || h.contains("red-and-blue.net")
        || h.contains("pokemon-gold-silver.net")
        || h.contains("pokemon-ruby-sapphire.net")
    {
        return "https://mangaball.net/".to_string();
    }
    format!("https://{}/", host)
}

/// Allowlist of upstream hosts the image proxy will fetch from.
/// This is the second line of defence after the HMAC signature: even if a
/// signature is somehow forged the proxy will only ever fetch from these hosts.
fn is_allowed_image_host(url: &str) -> bool {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let host = match parsed.host_str() {
        Some(h) => h.to_lowercase(),
        None => return false,
    };

    // Mangaball CDNs (pokemon-themed subdomains)
    if host.ends_with(".poke-black-and-white.net")
        || host.ends_with(".red-and-blue.net")
        || host.ends_with(".pokemon-gold-silver.net")
        || host.ends_with(".pokemon-ruby-sapphire.net")
        || host == "mangaball.net"
    {
        return true;
    }
    // Anichin
    if host.ends_with("anichin.cafe")
        || host.ends_with("anichin.care")
        || host.ends_with("anichin.cloud")
    {
        return true;
    }
    // Anichin uses i*.wp.com Jetpack CDN for some images
    if host == "i0.wp.com" || host == "i1.wp.com" || host == "i2.wp.com" || host == "i3.wp.com" {
        return true;
    }
    // Cosplaytele
    if host == "cosplaytele.com" || host.ends_with(".cosplaytele.com") {
        return true;
    }
    // nhentai (main domain + sharded image CDN: i1..i4, t1..t4)
    if host == "nhentai.net" || host == "nhentai.xxx" || host == "nhentai.to" {
        return true;
    }
    if host.ends_with(".nhentai.net") {
        return true;
    }
    // NovelID (covers are hosted on the main domain, sometimes via wp.com)
    if host == "novelid.org" || host.ends_with(".novelid.org") {
        return true;
    }
    // Otakudesu (anime covers on the main domain + any wp subdomain)
    if host == "otakudesu.blog" || host.ends_with(".otakudesu.blog") {
        return true;
    }
    if host == "otakudesu.bid" || host == "otakudesu.cloud" || host.contains("otakudesu.") {
        return true;
    }
    // lmanime (covers on the main domain; some via i*.wp.com handled above)
    if host == "lmanime.com" || host.ends_with(".lmanime.com") {
        return true;
    }
    // lk21 movie posters (CDN)
    if host == "poster.showcdnx.com" || host.ends_with(".showcdnx.com") {
        return true;
    }
    // NekoPoi (covers on the main domain /wp-content + any wp/cdn subdomain)
    if host == "nekopoi.care" || host.ends_with(".nekopoi.care") || host.contains("nekopoi.") {
        return true;
    }

    false
}
