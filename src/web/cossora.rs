//! Resolver for `cossora.stream` embed players used by Cosplaytele videos.
//!
//! Cosplaytele embeds its videos through `cossora.stream/embed/<uuid>`. That
//! page:
//!   * only serves real content when the `Referer` is `cosplaytele.com`
//!     (otherwise it returns a 404 / "Unknown Error xD") — so a browser
//!     iframe pointing straight at it always fails;
//!   * ships the real HLS URL AES-encrypted in an inline `videoURL` constant,
//!     decrypted client-side by an obfuscated `decryptLink(videoURL, key)`.
//!
//! We replicate that decryption server-side:
//!   * `key`  = the 32-char hex *string* found in the page, used as raw UTF-8
//!     bytes (32 bytes -> AES-256);
//!   * the base64 `videoURL` decodes to `[iv(16 bytes)][ciphertext]`;
//!   * AES-256-CBC / PKCS7 yields the master `…/index.m3u8?token=…` URL.
//!
//! The token in the resulting playlist is locked to the IP that requested the
//! embed (our server), so the playlists must be proxied through us; the `.ts`
//! segments are open. See `api::cosplay_video` / `api::hls_proxy`.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use once_cell::sync::Lazy;
use regex::Regex;

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// `const videoURL = '<base64>';`
static VIDEO_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"videoURL\s*=\s*'([^']+)'"#).unwrap());
/// `decryptLink(videoURL, '<32 hex chars>')`
static KEY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"decryptLink\(\s*videoURL\s*,\s*'([0-9a-fA-F]{32})'\s*\)"#).unwrap());
/// `cossora.stream/embed/<uuid>` recogniser.
static EMBED_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)cossora\.stream/embed/([0-9a-f\-]+)"#).unwrap());

/// Does this URL look like a cossora embed we can resolve?
pub fn is_cossora_embed(url: &str) -> bool {
    EMBED_RE.is_match(url)
}

/// Extract the encrypted `videoURL` and its key from an embed page's HTML.
fn extract_video_url_and_key(html: &str) -> Option<(String, String)> {
    let v = VIDEO_URL_RE.captures(html)?.get(1)?.as_str().to_string();
    let k = KEY_RE.captures(html)?.get(1)?.as_str().to_string();
    Some((v, k))
}

/// Decrypt the `videoURL` payload using the page key.
///
/// `payload` is base64 of `iv(16) || ciphertext`; `key_str` is the 32-char
/// string used directly as UTF-8 (32 bytes) for AES-256-CBC + PKCS7.
fn decrypt_link(payload_b64: &str, key_str: &str) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let raw = STANDARD.decode(payload_b64.trim()).ok()?;
    if raw.len() <= 16 {
        return None;
    }
    let key = key_str.as_bytes();
    if key.len() != 32 {
        return None;
    }
    let (iv, ct) = raw.split_at(16);

    let mut buf = ct.to_vec();
    let pt = Aes256CbcDec::new_from_slices(key, iv)
        .ok()?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    let s = String::from_utf8_lossy(pt).to_string();
    if s.starts_with("http") {
        Some(s)
    } else {
        None
    }
}

/// Given the raw embed-page HTML, return the decrypted master playlist URL.
pub fn resolve_master_from_html(html: &str) -> Option<String> {
    let (video_url, key) = extract_video_url_and_key(html)?;
    decrypt_link(&video_url, &key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

    #[test]
    fn recognises_embed_urls() {
        assert!(is_cossora_embed(
            "https://cossora.stream/embed/071d1d07-5652-4db2-9ff7-7fd58ea83876"
        ));
        assert!(!is_cossora_embed("https://example.com/video"));
    }

    #[test]
    fn decrypts_aes256_cbc_with_prefixed_iv() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        // Round-trip: encrypt a known URL exactly like cossora does, then make
        // sure our resolver recovers it.
        let key = "ad82ca641e034e520030e94902f49979"; // 32 chars -> 32 bytes
        let iv = [
            0x82u8, 0xe5, 0x4e, 0x53, 0x41, 0xa4, 0xe4, 0x9a, 0xd7, 0x4b, 0x5b, 0xde, 0xe7, 0xcf,
            0xab, 0x1a,
        ];
        let plaintext = b"https://cossora.stream/api-embed/abc/index.m3u8?token=xyz";
        let mut buf = vec![0u8; plaintext.len() + 16];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct = Aes256CbcEnc::new_from_slices(key.as_bytes(), &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap();

        let mut payload = iv.to_vec();
        payload.extend_from_slice(ct);
        let b64 = STANDARD.encode(&payload);

        let html = format!(
            "<script>const videoURL = '{}'; const x = decryptLink(videoURL, '{}');</script>",
            b64, key
        );
        let got = resolve_master_from_html(&html).expect("resolve");
        assert_eq!(
            got,
            "https://cossora.stream/api-embed/abc/index.m3u8?token=xyz"
        );
    }

    #[test]
    fn returns_none_without_markers() {
        assert!(resolve_master_from_html("<html>no player here</html>").is_none());
    }
}
