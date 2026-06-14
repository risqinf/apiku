//! Resolver for the streaming embeds used by NekoPoi posts.
//!
//! NekoPoi pages embed third-party file-host players in `<iframe>`s. Some of
//! those hosts refuse to be framed (they send `X-Frame-Options` /
//! `frame-ancestors`, so the browser shows a "this page can't be embedded"
//! error), and even the ones that *do* frame deliver IP-locked streams. To make
//! playback reliable on both mobile and desktop we resolve the real media URL
//! server-side and play it inline (proxied) instead of iframing the player.
//!
//! Two host families cover essentially all NekoPoi servers:
//!
//!   * **DoodStream clones** (`playmogo.com`, `dood*`, …): the embed page runs
//!     `$.get('/pass_md5/<path>')` then appends a 10-char nonce plus
//!     `?token=<tok>&expiry=<ms>`. The resulting URL is a direct `.mp4`.
//!   * **StreamWish / Filemoon clones** (`streampoi.com`, `streamruby`, …): the
//!     embed page packs a JWPlayer config with Dean-Edwards `eval(p,a,c,k,e,d)`;
//!     unpacking it reveals an `.m3u8` master playlist.
//!
//! This module holds the *pure* parsing/derivation logic (regex extraction +
//! packer unpacking + URL assembly) so it is unit-testable; the HTTP fetches
//! live in `api::nekopoi_stream`.

use once_cell::sync::Lazy;
use regex::Regex;

/// Which embed family an iframe URL belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// DoodStream clone -> resolves to a direct `.mp4`.
    Dood,
    /// StreamWish / Filemoon clone -> resolves to an `.m3u8` playlist.
    StreamWish,
    /// Anything we don't crack -> hand the iframe back to the client as-is.
    Unknown,
}

static PASS_MD5_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"/pass_md5/[^'"]+"#).unwrap());
/// The packed `eval(function(p,a,c,k,e,d){…}('payload',radix,count,'k|e|y'.split('|')…`.
static PACKED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)\}\('(.+)',\s*(\d+),\s*(\d+),\s*'(.*?)'\.split\('\|'\)").unwrap()
});
static M3U8_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s"'\\]+\.m3u8[^\s"'\\]*"#).unwrap());

/// Classify an embed URL by host so we know which extraction to run.
pub fn detect_provider(url: &str) -> Provider {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .unwrap_or_default();
    // DoodStream clones.
    if host.contains("playmogo")
        || host.contains("dood")
        || host.contains("doods")
        || host.contains("vidply")
        || host.contains("d-s.io")
        || host.contains("ds2play")
        || host.contains("d000d")
    {
        return Provider::Dood;
    }
    // StreamWish / Filemoon clones.
    if host.contains("streampoi")
        || host.contains("streamruby")
        || host.contains("streamwish")
        || host.contains("filemoon")
        || host.contains("vidhide")
        || host.contains("filelions")
        || host.contains("dhtpre")
        || host.contains("swdyu")
    {
        return Provider::StreamWish;
    }
    Provider::Unknown
}

// ---- DoodStream ------------------------------------------------------------

/// Pull the `/pass_md5/<a>/<token>` path out of a DoodStream embed page.
pub fn dood_pass_md5_path(html: &str) -> Option<String> {
    PASS_MD5_RE.find(html).map(|m| m.as_str().to_string())
}

/// The DoodStream token is the final path segment of the `pass_md5` path.
pub fn dood_token(pass_md5_path: &str) -> Option<String> {
    pass_md5_path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Assemble the final direct-MP4 URL exactly like the player's `makePlay()`:
/// `<base><nonce>?token=<token>&expiry=<ms>`.
pub fn dood_build_final(base: &str, nonce: &str, token: &str, expiry_ms: u128) -> String {
    format!("{}{}?token={}&expiry={}", base, nonce, token, expiry_ms)
}

// ---- StreamWish / packed JWPlayer ------------------------------------------

/// Encode `n` into the Dean-Edwards packer's base-`radix` alphabet.
fn packer_encode(mut n: usize, radix: usize) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        let r = n % radix;
        let ch = if r < 10 {
            (b'0' + r as u8) as char
        } else if r < 36 {
            (b'a' + (r - 10) as u8) as char
        } else {
            // c%a > 35 -> String.fromCharCode(c%a + 29)
            char::from_u32((r + 29) as u32).unwrap_or('?')
        };
        out.push(ch);
        n /= radix;
    }
    out.iter().rev().collect()
}

/// Unpack a `p,a,c,k,e,d` payload by substituting each token with its keyword.
fn unpack(payload: &str, radix: usize, count: usize, keywords: &[&str]) -> String {
    // Build the substitution table: token(i) -> keyword[i] (or itself if blank).
    let mut table = std::collections::HashMap::with_capacity(count);
    for i in 0..count {
        let key = packer_encode(i, radix);
        let val = keywords.get(i).copied().filter(|s| !s.is_empty());
        table.insert(key.clone(), val.map(str::to_string).unwrap_or(key));
    }
    static WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\w+\b").unwrap());
    WORD_RE
        .replace_all(payload, |c: &regex::Captures| {
            let tok = c.get(0).unwrap().as_str();
            table.get(tok).cloned().unwrap_or_else(|| tok.to_string())
        })
        .into_owned()
}

/// Unescape the JS single-quoted string literal that holds the packed payload.
fn js_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Unpack the first `eval(p,a,c,k,e,d)` block found in `html`, if any.
pub fn unpack_packed(html: &str) -> Option<String> {
    let caps = PACKED_RE.captures(html)?;
    let payload = js_unescape(caps.get(1)?.as_str());
    let radix: usize = caps.get(2)?.as_str().parse().ok()?;
    let count: usize = caps.get(3)?.as_str().parse().ok()?;
    let keywords_raw = js_unescape(caps.get(4)?.as_str());
    let keywords: Vec<&str> = keywords_raw.split('|').collect();
    Some(unpack(&payload, radix, count, &keywords))
}

/// Find the first `.m3u8` URL in a (possibly unpacked) blob of JS/HTML.
pub fn extract_m3u8(text: &str) -> Option<String> {
    M3U8_RE.find(text).map(|m| m.as_str().to_string())
}

/// Resolve a StreamWish-family embed page to its `.m3u8`: try the unpacked
/// JWPlayer config first, then fall back to a raw scan of the page.
pub fn streamwish_m3u8(html: &str) -> Option<String> {
    if let Some(unpacked) = unpack_packed(html) {
        if let Some(u) = extract_m3u8(&unpacked) {
            return Some(u);
        }
    }
    extract_m3u8(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_hosts() {
        assert_eq!(
            detect_provider("https://playmogo.com/e/l8kifapv4n7r"),
            Provider::Dood
        );
        assert_eq!(
            detect_provider("https://streampoi.com/embed-fxqo6sxprh88.html"),
            Provider::StreamWish
        );
        assert_eq!(
            detect_provider("https://vidnest.live/embed/abc"),
            Provider::Unknown
        );
    }

    #[test]
    fn dood_path_and_token() {
        let html = r#"<script>$.get('/pass_md5/266467567-160-191-1781395145-2e40e206/z6lx4riu30xl6fz3c4c9doln', function(data){})</script>"#;
        let path = dood_pass_md5_path(html).expect("path");
        assert_eq!(
            path,
            "/pass_md5/266467567-160-191-1781395145-2e40e206/z6lx4riu30xl6fz3c4c9doln"
        );
        assert_eq!(dood_token(&path).unwrap(), "z6lx4riu30xl6fz3c4c9doln");
    }

    #[test]
    fn dood_final_url_shape() {
        let url = dood_build_final("https://cdn.example.com/path/", "AbCdEf1234", "tok", 1700);
        assert_eq!(
            url,
            "https://cdn.example.com/path/AbCdEf1234?token=tok&expiry=1700"
        );
    }

    #[test]
    fn packer_encode_matches_reference() {
        // base-36: 0->"0", 10->"a", 35->"z"; base-62: 36->"A".
        assert_eq!(packer_encode(0, 36), "0");
        assert_eq!(packer_encode(9, 36), "9");
        assert_eq!(packer_encode(10, 36), "a");
        assert_eq!(packer_encode(35, 36), "z");
        assert_eq!(packer_encode(36, 62), "A");
        assert_eq!(packer_encode(37, 36), "11");
    }

    #[test]
    fn unpacks_and_extracts_m3u8() {
        // A minimal packed block: tokens 0,1,2 map to file/https/m3u8 pieces.
        // payload uses base-10 indices "0".."2".
        let payload = "var x={2:\\'1://h.test/v.0\\'}";
        let html = format!(
            "eval(function(p,a,c,k,e,d){{}}('{}',10,3,'m3u8|https|file'.split('|'),0,{{}}))",
            payload
        );
        let unpacked = unpack_packed(&html).expect("unpack");
        assert!(unpacked.contains("file://h.test/v.m3u8") || unpacked.contains("https://h.test"));
    }

    #[test]
    fn extract_m3u8_finds_url() {
        let blob = r#"sources:[{file:"https://cdn.test/hls/master.m3u8?t=abc&i=1.2"}]"#;
        assert_eq!(
            extract_m3u8(blob).unwrap(),
            "https://cdn.test/hls/master.m3u8?t=abc&i=1.2"
        );
    }
}
