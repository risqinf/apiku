//! Consumer-facing streaming / reading web app.
//!
//! Unlike `tester.rs` (a developer API console), this module serves the
//! actual end-user platform: a single-page app that browses, searches,
//! watches donghua, reads manga/novels, and views cosplay galleries — all
//! by talking to the same `/api/v1/*` JSON endpoints.
//!
//! The shell is a minimal HTML document; all rendering happens client-side
//! in `app.js` (a dependency-free hash-router SPA). Styling lives in
//! `app.css`. Both are compiled into the binary via `include_str!`.
//!
//! Branding (site name, logo, tagline, footer), SEO/ad verification
//! snippets, and ad slots are injected from `[web]` config at runtime, so
//! operators can rebrand and monetize without recompiling.

use crate::web::api::ApiState;
use axum::extract::State;
use axum::response::Html;

const APP_CSS: &str = include_str!("../../assets/webapp/app.css");
const APP_JS: &str = include_str!("../../assets/webapp/app.js");

/// HTML-escape a string for safe interpolation into attributes / text.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// JSON-escape a string for embedding inside a `<script>` JS string literal.
fn js_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"), // avoid </script> breakouts
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

/// Serve the SPA shell. The client router takes over from `#/`.
pub async fn index(State(state): State<ApiState>) -> Html<String> {
    let web = &state.web;
    let site_name = if web.site_name.trim().is_empty() {
        "apiku"
    } else {
        web.site_name.trim()
    };
    let tagline = web.tagline.trim();

    // Build the brand config object the SPA reads (window.__BRAND).
    let ads_json = serde_json::to_string(&web.ads).unwrap_or_else(|_| "{}".to_string());
    let brand_script = format!(
        "<script>window.__BRAND={{name:\"{name}\",tagline:\"{tag}\",logo:\"{logo}\",footer:\"{footer}\",ads:{ads}}};</script>",
        name = js_str(site_name),
        tag = js_str(tagline),
        logo = js_str(web.logo_url.trim()),
        footer = js_str(web.footer_html.trim()),
        ads = ads_json,
    );

    let icon = if web.logo_url.trim().is_empty() {
        "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>&#128250;</text></svg>".to_string()
    } else {
        esc(web.logo_url.trim())
    };
    let desc = if tagline.is_empty() {
        "Streaming donghua, baca komik & novel, galeri cosplay - semua dalam satu platform."
            .to_string()
    } else {
        esc(tagline)
    };

    // NOTE: we deliberately avoid `format!` for the CSS/JS blocks because the
    // embedded code is full of `{` `}` which the format machinery would choke
    // on. Plain concatenation keeps the braces literal.
    let head = format!(
        r##"<!DOCTYPE html>
<html lang="id" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{title}</title>
<meta name="description" content="{desc}">
<meta name="theme-color" content="#0b0e14">
<link rel="icon" href="{icon}">
{head_html}
<style>"##,
        title = esc(site_name),
        desc = desc,
        icon = icon,
        head_html = web.head_html, // raw, operator-controlled
    );

    let mid = format!(
        r##"</style>
</head>
<body>
<div id="app"><div class="boot">Loading...</div></div>
{brand}
<script>"##,
        brand = brand_script,
    );

    let tail = format!(
        r##"</script>
{body_html}
</body>
</html>"##,
        body_html = web.body_html, // raw, operator-controlled
    );

    let mut html = String::with_capacity(
        head.len() + APP_CSS.len() + mid.len() + APP_JS.len() + tail.len() + 256,
    );
    html.push_str(&head);
    html.push_str(APP_CSS);
    html.push_str(&mid);
    html.push_str(APP_JS);
    html.push_str(&tail);
    Html(html)
}
