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
    DonghuaSeries, EpisodeInfo, MangaChapter, MangaSeries, NovelChapter, NovelChapterRef,
    NovelSeries, PageImage, ScrapeResult,
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
        Self {
            engine: Arc::new(engine),
            codec: Arc::new(codec),
            cache,
            search_cache,
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
        "anime" | "otakudesu" => vec![SearchSource::Otakudesu],
        "all" => vec![
            SearchSource::Mangaball,
            SearchSource::Anichin,
            SearchSource::Cosplaytele,
            SearchSource::Nhentai,
            SearchSource::Novelid,
            SearchSource::Otakudesu,
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
    let has_next = agg_total_pages
        .map(|tp| page < tp)
        .unwrap_or(agg_has_next);

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
    let has_next = total_pages
        .map(|tp| page < tp)
        .unwrap_or(!items.is_empty());
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
        _ => (Source::Mangaball, "unknown"),
    };
    let opaque_kind = match source {
        Source::Mangaball
        | Source::Anichin
        | Source::Nhentai
        | Source::Novelid
        | Source::Otakudesu => Kind::Series,
        Source::Cosplaytele => Kind::Post,
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
    }
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
    let cache_key = format!(
        "browse:{}|{}|{}|{}",
        provider,
        q.feed,
        p,
        q.size.unwrap_or(0)
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
            let br = run_browse(&state_clone, &provider_lc, &feed, p, size).await?;
            let has_next = br
                .total_pages
                .map(|tp| p < tp)
                .unwrap_or(br.has_next);
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
) -> Result<BrowseResult, String> {
    match provider {
        "mangaball" | "manga" => browse_mangaball(state, feed, page, size).await,
        "anichin" | "donghua" => browse_anichin(state, feed, page).await,
        "cosplaytele" | "cosplay" => browse_cosplaytele(state, feed, page).await,
        "nhentai" | "doujin" => browse_nhentai(state, feed, page).await,
        "novelid" | "novel" => browse_novelid(state, feed, page).await,
        "otakudesu" | "anime" => browse_otakudesu(state, feed, page).await,
        _ => Err(format!("unknown provider '{}'", provider)),
    }
}

async fn browse_otakudesu(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
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

async fn browse_anichin(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
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

async fn browse_nhentai(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
    let sort = crate::adapters::nhentai::NhentaiAdapter::feed_to_sort(feed);
    let url = crate::adapters::nhentai::NhentaiAdapter::api_url_for_popular(page, sort);
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

async fn browse_novelid(
    state: &ApiState,
    feed: &str,
    page: u32,
) -> Result<BrowseResult, String> {
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
    if dec.source != Source::Otakudesu {
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
    let dto = anime_series_to_dto(&state, series, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn anime_episode_ref_to_dto(
    state: &ApiState,
    e: &crate::models::AnimeEpisodeRef,
) -> AnimeEpisodeRefDto {
    AnimeEpisodeRefDto {
        id: state.codec.encode(Source::Otakudesu, Kind::Item, &e.url),
        number: e.number,
        title: e.title.clone(),
        date: e.date.clone(),
    }
}

fn anime_series_to_dto(state: &ApiState, s: &AnimeSeries, id: &str) -> AnimeSeriesDto {
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
            .map(|e| anime_episode_ref_to_dto(state, e))
            .collect(),
        batch: s
            .batch
            .iter()
            .map(|e| anime_episode_ref_to_dto(state, e))
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
    if dec.source != Source::Otakudesu {
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
    let dto = anime_episode_to_dto(&state, ep, &id);
    ok(StatusCode::OK, dto, started, cached, &rid)
}

fn anime_episode_to_dto(state: &ApiState, e: &AnimeEpisode, id: &str) -> AnimeEpisodeDto {
    AnimeEpisodeDto {
        id: id.to_string(),
        series_title: e.series_title.clone(),
        series_id: e
            .series_url
            .as_deref()
            .map(|u| state.codec.encode(Source::Otakudesu, Kind::Series, u)),
        episode_number: e.episode_number,
        prev_id: e
            .prev_episode
            .as_deref()
            .map(|u| state.codec.encode(Source::Otakudesu, Kind::Item, u)),
        next_id: e
            .next_episode
            .as_deref()
            .map(|u| state.codec.encode(Source::Otakudesu, Kind::Item, u)),
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

    match resolve_otakudesu_stream(&state, episode_url, token).await {
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
    if !is_allowed_hls_host(&url) {
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
    hdrs.insert("Referer".to_string(), "https://cossora.stream/".to_string());
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
        let text = String::from_utf8_lossy(&bytes);
        let rewritten = rewrite_m3u8(&state, &text, &url);
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
fn rewrite_m3u8(state: &ApiState, text: &str, base: &str) -> String {
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
    // Playlists -> proxied + signed; everything else -> direct absolute URL.
    let resolve = |raw: &str| -> Option<String> {
        let abs = absolutize(raw)?;
        if is_playlist_ref(&abs) {
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

/// Allowlist of hosts the HLS proxy will fetch from.
fn is_allowed_hls_host(url: &str) -> bool {
    let host = match url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
    {
        Some(h) => h,
        None => return false,
    };
    host == "cossora.stream" || host.ends_with(".cossora.stream")
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
    (StatusCode::OK, headers, bytes).into_response()
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
    if h.contains("otakudesu.") {
        return "https://otakudesu.blog/".to_string();
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

    false
}
