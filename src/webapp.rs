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

use axum::response::Html;

const APP_CSS: &str = include_str!("app.css");
const APP_JS: &str = include_str!("app.js");

/// Serve the SPA shell. The client router takes over from `#/`.
pub async fn index() -> Html<String> {
    // NOTE: we deliberately avoid `format!` here because the embedded CSS/JS
    // are full of `{` `}` which the format machinery would choke on. Plain
    // concatenation keeps the braces literal.
    let head = r##"<!DOCTYPE html>
<html lang="id" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>apiku - nonton & baca</title>
<meta name="description" content="Streaming donghua, baca manga, novel, cosplay - satu tempat. Ditenagai apiku.">
<meta name="theme-color" content="#0b0e14">
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>&#128250;</text></svg>">
<style>"##;

    let mid = r##"</style>
</head>
<body>
<div id="app"><div class="boot">Loading...</div></div>
<script>"##;

    let tail = r##"</script>
</body>
</html>"##;

    let mut html =
        String::with_capacity(head.len() + APP_CSS.len() + mid.len() + APP_JS.len() + tail.len());
    html.push_str(head);
    html.push_str(APP_CSS);
    html.push_str(mid);
    html.push_str(APP_JS);
    html.push_str(tail);
    Html(html)
}
