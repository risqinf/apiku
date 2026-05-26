//! HTTP server module: builds the axum `Router`, wires middleware
//! (request-id, CORS, compression, tracing) and starts listening.
//!
//! Routes:
//!
//!   GET /                            -> tester website
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

use crate::api::{self, ApiState};
use crate::tester;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use std::net::SocketAddr;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Build the axum router with all routes + middleware applied.
pub fn build_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // HTML tester
        .route("/", get(tester::home))
        // REST API
        .route("/api/v1/health", get(api::health))
        .route("/api/v1/info", get(api::info))
        .route("/api/v1/search", get(api::search))
        .route("/api/v1/manga/{id}", get(api::manga_series))
        .route("/api/v1/manga/chapter/{id}", get(api::manga_chapter))
        .route("/api/v1/donghua/{id}", get(api::donghua_series))
        .route("/api/v1/donghua/episode/{id}", get(api::donghua_episode))
        .route("/api/v1/cosplay/{id}", get(api::cosplay_post))
        .route("/api/v1/novel/{id}", get(api::novel_series))
        .route("/api/v1/novel/chapter/{id}", get(api::novel_chapter))
        .route("/api/v1/browse/{provider}", get(api::browse))
        .route("/api/v1/nhentai/{id}", get(api::nhentai_gallery))
        .route("/api/v1/nhentai/chapter/{id}", get(api::nhentai_chapter))
        // Image proxy
        .route("/img", get(api::img_proxy))
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
pub async fn run(state: ApiState, addr: SocketAddr) -> anyhow::Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(addr = %addr, "server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
