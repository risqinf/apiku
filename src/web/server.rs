//! HTTP server module: builds the axum `Router`, wires middleware
//! (request-id, CORS, compression, tracing) and starts listening.
//!
//! Routes:
//!
//!   GET /                            -> consumer web app (streaming/reading SPA)
//!   GET /tester                      -> developer API console
//!   GET /api/v1/health               -> liveness probe
//!   GET /api/v1/info                 -> version + system tuning + provider list
//!   GET /api/v1/search               -> cross-provider search
//!   GET /api/v1/browse/{provider}    -> home / popular / latest feed for a provider
//!   GET /api/v1/manga/{id}           -> manga series (Mangaball) — paged chapter list
//!   GET /api/v1/manga/chapter/{id}   -> manga chapter pages
//!   GET /api/v1/donghua/{id}         -> donghua series (Anichin) — paged episode list
//!   GET /api/v1/donghua/episode/{id} -> donghua episode + servers + downloads
//!   GET /api/v1/cosplay/{id}         -> cosplay post (Cosplaytele)
//!   GET /api/v1/novel/{id}           -> novel series (NovelID) — handles
//!                                       upstream-paginated chapter lists
//!   GET /api/v1/novel/chapter/{id}   -> novel chapter (text body)
//!   GET /api/v1/nhentai/{id}         -> nhentai gallery (browser-spoofed)
//!   GET /api/v1/nhentai/chapter/{id} -> nhentai gallery as a chapter
//!   GET /img                         -> HMAC-signed image proxy

use crate::web::api::{self, ApiState};
use crate::web::tester;
use crate::web::webapp;
use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use std::net::SocketAddr;
use std::path::{Component, PathBuf};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Build the axum router with all routes + middleware applied.
///
/// `static_dir` is the directory served at the site root for verification
/// files (`google1234.html`), `ads.txt`, `sitemap.xml`, `robots.txt`,
/// favicons, and custom logos.
pub fn build_router(state: ApiState, static_dir: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let static_root = PathBuf::from(static_dir);

    Router::new()
        // Consumer web app (streaming / reading platform)
        .route("/", get(webapp::index))
        // Developer API console
        .route("/tester", get(tester::home))
        // REST API
        .route("/api/v1/health", get(api::health))
        .route("/api/v1/info", get(api::info))
        .route("/api/v1/search", get(api::search))
        .route("/api/v1/manga/{id}", get(api::manga_series))
        .route("/api/v1/manga/chapter/{id}", get(api::manga_chapter))
        .route("/api/v1/donghua/{id}", get(api::donghua_series))
        .route("/api/v1/donghua/episode/{id}", get(api::donghua_episode))
        .route("/api/v1/anime/{id}", get(api::anime_series))
        .route("/api/v1/anime/episode/{id}", get(api::anime_episode))
        .route("/api/v1/anime-stream", get(api::anime_stream))
        .route("/api/v1/cosplay/{id}", get(api::cosplay_post))
        .route("/api/v1/cosplay-video", get(api::cosplay_video))
        .route("/api/v1/novel/{id}", get(api::novel_series))
        .route("/api/v1/novel/chapter/{id}", get(api::novel_chapter))
        .route("/api/v1/browse/{provider}", get(api::browse))
        .route("/api/v1/nhentai/{id}", get(api::nhentai_gallery))
        .route("/api/v1/nhentai/chapter/{id}", get(api::nhentai_chapter))
        // Image proxy
        .route("/img", get(api::img_proxy))
        // HLS playlist/segment proxy (cosplay video streams)
        .route("/hls", get(api::hls_proxy))
        // Static assets / verification files served from `static_dir`.
        // Single-segment only (e.g. /google1234.html, /ads.txt, /logo.svg) so
        // it never shadows the API or SPA routes.
        .route(
            "/{file}",
            get({
                let root = static_root.clone();
                move |path| serve_static(root.clone(), path)
            }),
        )
        // 404 fallback returning a proper envelope
        .fallback(not_found)
        // Layers (innermost first when applied)
        .layer(SetRequestIdLayer::new(X_REQUEST_ID, MakeRequestUuid))
        .layer(PropagateRequestIdLayer::new(X_REQUEST_ID))
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Serve a single file from the configured static directory.
///
/// Used for SEO / ad-network verification files (`googleXXXX.html`,
/// `ads.txt`, `app-ads.txt`, `sitemap.xml`, `robots.txt`), favicons, and
/// custom branding assets. The filename is a single path segment; any
/// traversal attempt (`.`/`..`/separators) is rejected.
async fn serve_static(root: PathBuf, AxumPath(file): AxumPath<String>) -> Response {
    // Reject path traversal and nested separators outright.
    if file.is_empty()
        || file.contains('/')
        || file.contains('\\')
        || PathBuf::from(&file)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = root.join(&file);
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(content_type_for(&file)) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    // Verification files must not be cached aggressively; assets can be.
    let cache = if file.ends_with(".html") || file.ends_with(".txt") || file.ends_with(".xml") {
        "public, max-age=300"
    } else {
        "public, max-age=86400"
    };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    (StatusCode::OK, headers, bytes).into_response()
}

/// Minimal extension -> MIME map for static files.
fn content_type_for(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

/// Default 404 handler returning a JSON envelope rather than a blank page.
async fn not_found(req: axum::extract::Request) -> impl IntoResponse {
    let req_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let body = serde_json::json!({
        "status": 404,
        "ok": false,
        "error": {
            "code": "not_found",
            "message": format!("Route not found: {}", req.uri().path())
        },
        "meta": {
            "took_ms": 0,
            "cached": false,
            "request_id": req_id,
        }
    });

    let mut headers = axum::http::HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(body["meta"]["request_id"].as_str().unwrap_or("")) {
        headers.insert("x-request-id", v);
    }
    (StatusCode::NOT_FOUND, headers, Json(body))
}

/// Bind to `addr` and run the server until shutdown.
pub async fn run(state: ApiState, addr: SocketAddr, static_dir: &str) -> anyhow::Result<()> {
    let app = build_router(state, static_dir);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(addr = %addr, "server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
