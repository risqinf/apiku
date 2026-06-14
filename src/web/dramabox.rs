//! DramaBox / DramaWave (drachin — Chinese vertical short dramas) client.
//!
//! Backed by the MyDramaWave app API (`api.mydramawave.com`), which is a JSON
//! service (not an HTML site) with **AES-128-CBC encrypted request/response
//! bodies** and a guest (anonymous) auth flow. This module replicates that
//! protocol end to end:
//!
//!   - crypto: AES-128-CBC, key `2r36789f45q01ae5`, random 16-byte IV prepended,
//!     Pkcs7, base64(IV || ciphertext) — used for both request bodies and
//!     responses.
//!   - auth: `POST /h5-api/anonymous/login {device_id}` -> `auth_key` +
//!     `auth_secret`; the per-request `authorization` header is
//!     `oauth_signature=md5(SECRET&auth_secret),oauth_token=auth_key,ts=<ms>`.
//!   - browse: `GET /h5-api/homepage/v2/tab/list` (tabs) ->
//!     `GET /h5-api/homepage/v2/tab/index` (modules) ->
//!     `POST /h5-api/homepage/v2/tab/feed {module_key,next}` (paged dramas).
//!   - detail: `GET /dm-api/drama/share/series_info?series_id=<id>` (series +
//!     the free episodes, each carrying an HLS `.m3u8` URL).

use crate::web::api::ApiState;
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use once_cell::sync::Lazy;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const API_BASE: &str = "https://api.mydramawave.com";
const AES_KEY: &[u8] = b"2r36789f45q01ae5"; // 16 bytes -> AES-128
const SIGN_SECRET: &str = "8IAcbWyCsVhYv82S2eofRqK1DF3nNDAv";
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Clone)]
struct Session {
    auth_key: String,
    auth_secret: String,
    device_id: String,
    fetched: Instant,
}

static SESSION: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));

// ---- public DTO-ish types (field names kept stable for the web layer) ----

#[derive(Debug, Clone)]
pub struct DramaCard {
    pub book_id: String,
    pub title: String,
    pub cover: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DramaEpisode {
    pub index: u32,
    pub title: String,
    pub video_url: Option<String>,
    pub cover: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DramaDetail {
    pub title: String,
    pub cover: Option<String>,
    pub description: Option<String>,
    pub episodes: Vec<DramaEpisode>,
}

/// Synthetic URL stored in the opaque ID so `Source::detect` recognises it and
/// we can recover the drama key.
pub fn book_url(book_id: &str) -> String {
    format!("https://api.mydramawave.com/drama/{}", book_id)
}
pub fn book_id_from_url(url: &str) -> Option<String> {
    url.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

// ---- crypto ----------------------------------------------------------------

fn rand_iv() -> [u8; 16] {
    let a = uuid::Uuid::new_v4();
    *a.as_bytes()
}

fn encrypt(plaintext: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let iv = rand_iv();
    let pt = plaintext.as_bytes();
    let mut buf = vec![0u8; pt.len() + 16];
    buf[..pt.len()].copy_from_slice(pt);
    let ct = Aes128CbcEnc::new_from_slices(AES_KEY, &iv)
        .expect("aes key/iv")
        .encrypt_padded_mut::<Pkcs7>(&mut buf, pt.len())
        .expect("encrypt");
    let mut payload = iv.to_vec();
    payload.extend_from_slice(ct);
    STANDARD.encode(&payload)
}

fn decrypt(b64: &str) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let raw = STANDARD.decode(b64.trim()).ok()?;
    if raw.len() <= 16 {
        return None;
    }
    let (iv, ct) = raw.split_at(16);
    let mut buf = ct.to_vec();
    let pt = Aes128CbcDec::new_from_slices(AES_KEY, iv)
        .ok()?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    String::from_utf8(pt.to_vec()).ok()
}

// ---- HTTP ------------------------------------------------------------------

fn base_headers(device_id: &str) -> Vec<(String, String)> {
    vec![
        ("User-Agent".into(), "Mozilla/5.0 Chrome/120".into()),
        ("app-name".into(), "com.dramawave.h5".into()),
        ("app-version".into(), "1.2.20".into()),
        ("device-hash".into(), device_id.into()),
        ("device-id".into(), device_id.into()),
        ("device".into(), "h5".into()),
        ("Origin".into(), "https://mydramawave.com".into()),
        ("Referer".into(), "https://mydramawave.com/".into()),
    ]
}

/// Low-level request. `enc_body` (Some) is JSON encrypted into the body;
/// response is always treated as an encrypted base64 blob (falls back to plain
/// when it isn't, e.g. `dm-api` share responses).
async fn request(
    state: &ApiState,
    method: reqwest::Method,
    path: &str,
    enc_body: Option<&str>,
    auth: Option<&str>,
    device_id: &str,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", API_BASE, path);
    let client = state.engine.client();
    let mut rb = client.request(method, &url);
    for (k, v) in base_headers(device_id) {
        rb = rb.header(k, v);
    }
    if let Some(a) = auth {
        rb = rb.header("authorization", a);
    }
    if let Some(body) = enc_body {
        rb = rb
            .header("Content-Type", "application/json")
            .body(encrypt(body));
    }
    let resp = rb.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("dramawave API returned {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let plain = decrypt(&text).unwrap_or(text);
    serde_json::from_str(&plain).map_err(|e| format!("bad json: {e}"))
}

/// Ensure a valid guest session (anonymous login), cached.
async fn ensure_session(state: &ApiState) -> Result<Session, String> {
    {
        let g = SESSION.lock().await;
        if let Some(s) = g.as_ref() {
            if s.fetched.elapsed() < SESSION_TTL {
                return Ok(s.clone());
            }
        }
    }
    let device_id = uuid::Uuid::new_v4().simple().to_string();
    let body = serde_json::json!({ "device_id": device_id });
    let json = request(
        state,
        reqwest::Method::POST,
        "/h5-api/anonymous/login",
        Some(&body.to_string()),
        None,
        &device_id,
    )
    .await?;
    let data = json.get("data").ok_or("login: no data")?;
    let auth_key = data
        .get("auth_key")
        .and_then(|v| v.as_str())
        .ok_or("login: no auth_key")?
        .to_string();
    let auth_secret = data
        .get("auth_secret")
        .and_then(|v| v.as_str())
        .ok_or("login: no auth_secret")?
        .to_string();
    let s = Session {
        auth_key,
        auth_secret,
        device_id,
        fetched: Instant::now(),
    };
    *SESSION.lock().await = Some(s.clone());
    Ok(s)
}

fn auth_header(s: &Session) -> String {
    let sig = format!(
        "{:x}",
        md5::compute(format!("{}&{}", SIGN_SECRET, s.auth_secret))
    );
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(
        "oauth_signature={},oauth_token={},ts={}",
        sig, s.auth_key, ts
    )
}

// ---- card / detail parsing -------------------------------------------------

fn str_field(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
    }
    None
}

/// First HLS/mp4 URL inside an `episode_info`-shaped object.
fn episode_video(v: &serde_json::Value) -> Option<String> {
    for k in [
        "external_audio_h264_m3u8",
        "m3u8_url",
        "video_url",
        "external_audio_h265_m3u8",
    ] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            if s.contains(".m3u8") || s.contains(".mp4") {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn card_from(v: &serde_json::Value) -> Option<DramaCard> {
    let book_id = str_field(v, &["key", "series_id", "id"])?;
    let title = str_field(v, &["title", "series_name", "name"])?;
    let cover = str_field(v, &["cover", "cover_url"]);
    let mut tags = Vec::new();
    if let Some(arr) = v.get("content_tags").and_then(|x| x.as_array()) {
        for t in arr {
            if let Some(s) = t.as_str() {
                if !s.is_empty() && tags.len() < 4 {
                    tags.push(s.to_string());
                }
            }
        }
    }
    if let Some(n) = v.get("episode_count").and_then(|x| x.as_u64()) {
        if n > 0 {
            tags.push(format!("{} eps", n));
        }
    }
    Some(DramaCard {
        book_id,
        title,
        cover,
        tags,
    })
}

/// Browse: paged drama list via the homepage feed.
pub async fn theater(state: &ApiState, page: u32) -> Result<Vec<DramaCard>, String> {
    let s = ensure_session(state).await?;
    let auth = auth_header(&s);

    // Discover the first tab + its recommend module.
    let tabs = request(
        state,
        reqwest::Method::GET,
        "/h5-api/homepage/v2/tab/list",
        None,
        Some(&auth),
        &s.device_id,
    )
    .await?;
    let tab = tabs.pointer("/data/list/0").ok_or("no tabs")?.clone();
    let tab_key = str_field(&tab, &["tab_key"]).unwrap_or_default();
    let pidx = tab
        .get("position_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let idx = request(
        state,
        reqwest::Method::GET,
        &format!(
            "/h5-api/homepage/v2/tab/index?tab_key={}&position_index={}&first=",
            tab_key, pidx
        ),
        None,
        Some(&auth),
        &s.device_id,
    )
    .await?;
    let modules = idx
        .pointer("/data/items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // Pick the module that paginates dramas (has episode_info items / a key).
    let module_key = modules
        .iter()
        .find(|m| {
            m.get("module_key").and_then(|v| v.as_str()).is_some()
                && m.get("type").and_then(|v| v.as_str()) != Some("banner")
        })
        .and_then(|m| str_field(m, &["module_key"]))
        .or_else(|| modules.iter().find_map(|m| str_field(m, &["module_key"])))
        .unwrap_or_default();

    // Page 1 from the index modules; deeper pages from the feed cursor.
    if page <= 1 {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in &modules {
            if let Some(items) = m.get("items").and_then(|v| v.as_array()) {
                for it in items {
                    if it
                        .get("episode_info")
                        .map(|e| !e.is_null())
                        .unwrap_or(false)
                    {
                        if let Some(c) = card_from(it) {
                            if seen.insert(c.book_id.clone()) {
                                out.push(c);
                            }
                        }
                    }
                }
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let next = format!("offset={}", (page.saturating_sub(1)) * 20);
    let feed_body = serde_json::json!({ "module_key": module_key, "next": next });
    let feed = request(
        state,
        reqwest::Method::POST,
        "/h5-api/homepage/v2/tab/feed",
        Some(&feed_body.to_string()),
        Some(&auth),
        &s.device_id,
    )
    .await?;
    let items = feed
        .pointer("/data/items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for it in &items {
        if it
            .get("episode_info")
            .map(|e| !e.is_null())
            .unwrap_or(false)
        {
            if let Some(c) = card_from(it) {
                if seen.insert(c.book_id.clone()) {
                    out.push(c);
                }
            }
        }
    }
    Ok(out)
}

/// Keyword search. The public API doesn't expose a stable H5 search endpoint
/// we could verify, so this is a best-effort no-op for now (browse is the
/// primary discovery path).
pub async fn search(_state: &ApiState, _keyword: &str) -> Result<Vec<DramaCard>, String> {
    Ok(Vec::new())
}

/// Drama detail + (free) episodes via the share API.
pub async fn detail(state: &ApiState, book_id: &str) -> Result<DramaDetail, String> {
    let s = ensure_session(state).await?;
    let auth = auth_header(&s);
    let json = request(
        state,
        reqwest::Method::GET,
        &format!("/dm-api/drama/share/series_info?series_id={}", book_id),
        None,
        Some(&auth),
        &s.device_id,
    )
    .await?;
    let si = json.pointer("/data/series_info").ok_or("no series_info")?;
    let title = str_field(si, &["name", "title"]).unwrap_or_default();
    let cover = str_field(si, &["cover"]);
    let description = str_field(si, &["desc", "description"]);
    let mut episodes = Vec::new();
    if let Some(list) = si.get("episode_list").and_then(|v| v.as_array()) {
        for (i, e) in list.iter().enumerate() {
            let pos = (i as u32) + 1;
            let index = e
                .get("index")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .filter(|&n| n > 0)
                .unwrap_or(pos);
            // The API "name" is often a raw filename like "0.mp4"; fall back to
            // a clean "Episode N" label in that case.
            let raw_name = str_field(e, &["name", "title"]);
            let title = match raw_name {
                Some(n) if !n.ends_with(".mp4") && !n.chars().all(|c| c.is_ascii_digit()) => n,
                _ => format!("Episode {index}"),
            };
            episodes.push(DramaEpisode {
                index,
                title,
                video_url: episode_video(e),
                cover: str_field(e, &["cover"]),
            });
        }
    }
    Ok(DramaDetail {
        title,
        cover,
        description,
        episodes,
    })
}
