//! Opaque ID encoding / decoding using HMAC-SHA256.
//!
//! When the API exposes content to consumers, we never reveal the raw upstream
//! URL. Instead we issue an opaque, signed token that:
//!
//!   1. Is fully URL-safe (uses base64url with no padding)
//!   2. Is signed with HMAC-SHA256 to detect tampering
//!   3. Encodes the source ("mb" / "ac" / "ct") + content kind + raw URL
//!   4. May optionally carry an expiration timestamp
//!
//! Wire format
//! -----------
//! `<6-byte header>.<base64url-payload>.<base64url-mac-16-bytes>`
//!
//! Header (6 chars):  `<source 2 chars><kind 1 char><nonce 3 chars>`
//!   - source:  `mb` (mangaball) | `ac` (anichin) | `ct` (cosplaytele)
//!   - kind:    `s` (series) | `i` (item) | `p` (post)
//!   - nonce:   3 random base32 chars to prevent identical MACs for same URL
//!
//! Payload: base64url(raw_url_bytes [|| u64-le expires_at])
//! MAC:     base64url(HMAC-SHA256(secret, header || "." || payload)[..16])
//!
//! Security notes
//! --------------
//!   - 128-bit MAC truncated from HMAC-SHA256 (overkill for this threat model).
//!   - Constant-time MAC comparison avoids timing-side-channel signature checks.
//!   - Server secret is rotated on every restart unless `APIKU_SECRET` is set.
//!   - Even with the secret, opaque IDs do not reveal the upstream domain at
//!     a glance (payload is base64url; consumer would have to decode it).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Source identifier for an opaque ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Mangaball,
    Anichin,
    Cosplaytele,
    Nhentai,
    Novelid,
    Otakudesu,
    Lmanime,
    Lk21,
    Otakudesufit,
    Nekopoi,
}

impl Source {
    pub fn short_code(&self) -> &'static str {
        match self {
            Source::Mangaball => "mb",
            Source::Anichin => "ac",
            Source::Cosplaytele => "ct",
            Source::Nhentai => "nh",
            Source::Novelid => "nv",
            Source::Otakudesu => "od",
            Source::Lmanime => "lm",
            Source::Lk21 => "lk",
            Source::Otakudesufit => "of",
            Source::Nekopoi => "np",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "mb" => Some(Self::Mangaball),
            "ac" => Some(Self::Anichin),
            "ct" => Some(Self::Cosplaytele),
            "nh" => Some(Self::Nhentai),
            "nv" => Some(Self::Novelid),
            "od" => Some(Self::Otakudesu),
            "lm" => Some(Self::Lmanime),
            "lk" => Some(Self::Lk21),
            "of" => Some(Self::Otakudesufit),
            "np" => Some(Self::Nekopoi),
            _ => None,
        }
    }

    /// Detect source from a raw URL (used by the search adapters)
    #[allow(dead_code)]
    pub fn detect(url: &str) -> Option<Self> {
        let u = url.to_lowercase();
        if u.contains("mangaball.net") {
            Some(Self::Mangaball)
        } else if u.contains("anichin.") {
            Some(Self::Anichin)
        } else if u.contains("cosplaytele.com") {
            Some(Self::Cosplaytele)
        } else if u.contains("nhentai.") {
            Some(Self::Nhentai)
        } else if u.contains("novelid.org") {
            Some(Self::Novelid)
        } else if u.contains("otakudesu.fit") {
            Some(Self::Otakudesufit)
        } else if u.contains("otakudesu.") {
            Some(Self::Otakudesu)
        } else if u.contains("lmanime.com") {
            Some(Self::Lmanime)
        } else if u.contains("lk21") || u.contains("layarkaca") {
            Some(Self::Lk21)
        } else if u.contains("nekopoi") {
            Some(Self::Nekopoi)
        } else {
            None
        }
    }
}

/// Content kind hint encoded inside an opaque ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Series, // manga series, donghua series
    Item,   // manga chapter, donghua episode
    Post,   // cosplay post
}

impl Kind {
    pub fn short_code(&self) -> char {
        match self {
            Kind::Series => 's',
            Kind::Item => 'i',
            Kind::Post => 'p',
        }
    }

    pub fn from_short(c: char) -> Option<Self> {
        match c {
            's' => Some(Self::Series),
            'i' => Some(Self::Item),
            'p' => Some(Self::Post),
            _ => None,
        }
    }
}

/// Opaque-ID encoder/decoder backed by an HMAC-SHA256 secret.
#[derive(Debug, Clone)]
pub struct OpaqueCodec {
    secret: Vec<u8>,
}

impl OpaqueCodec {
    /// Create a codec from the given raw secret bytes (32+ bytes recommended).
    #[allow(dead_code)]
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// Generate a fresh server-lifetime secret using `getrandom` if available,
    /// falling back to a hash of process info.
    pub fn from_random() -> Self {
        let mut buf = [0u8; 32];
        // `getrandom` is pulled in transitively via `rand`/`uuid`. Use uuid to
        // produce 32 bytes of high-entropy data without an extra crate.
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        buf[..16].copy_from_slice(a.as_bytes());
        buf[16..].copy_from_slice(b.as_bytes());
        Self {
            secret: buf.to_vec(),
        }
    }

    /// Resolve the signing secret, preferring stability so issued opaque IDs and
    /// image-proxy URLs survive restarts (otherwise saved favorites / history
    /// and their thumbnails break after every restart).
    ///
    /// Order of preference:
    ///   1. `APIKU_SECRET` env var (operator-provided; best for multi-instance).
    ///   2. A persisted random secret on disk (auto-generated on first run, then
    ///      reused). Location: `$APIKU_DATA_DIR/secret`, else `$HOME/.apiku/secret`,
    ///      else `./.apiku-secret`.
    ///   3. A fresh random secret (only if persistence fails) — logged loudly so
    ///      operators know IDs won't be stable.
    pub fn from_env_or_persisted() -> Self {
        if let Ok(s) = std::env::var("APIKU_SECRET") {
            if !s.is_empty() {
                use sha2::Digest;
                let digest = Sha256::digest(s.as_bytes());
                return Self {
                    secret: digest.to_vec(),
                };
            }
        }
        match load_or_create_persisted_secret() {
            Some((bytes, path)) => {
                tracing::info!(path = %path, "using persisted opaque secret (stable IDs across restarts)");
                use sha2::Digest;
                let digest = Sha256::digest(&bytes);
                Self {
                    secret: digest.to_vec(),
                }
            }
            None => {
                tracing::warn!(
                    "could not persist an opaque secret; using a random per-process secret. \
                     Saved favorites/history and image URLs will break on restart. \
                     Set APIKU_SECRET=<long-random> to fix."
                );
                Self::from_random()
            }
        }
    }

    /// Encode a URL into an opaque ID.
    pub fn encode(&self, source: Source, kind: Kind, url: &str) -> String {
        self.encode_with_nonce(source, kind, url, &random_nonce_3())
    }

    /// Encode with a fixed nonce (used in tests for determinism).
    pub fn encode_with_nonce(&self, source: Source, kind: Kind, url: &str, nonce: &str) -> String {
        let header = format!("{}{}{}", source.short_code(), kind.short_code(), nonce);
        let payload = URL_SAFE_NO_PAD.encode(url.as_bytes());
        let mac = self.compute_mac(&header, &payload);
        format!("{}.{}.{}", header, payload, mac)
    }

    /// Decode an opaque ID back into (source, kind, url). Returns Err on tampering.
    pub fn decode(&self, opaque: &str) -> Result<DecodedOpaque, OpaqueError> {
        let parts: Vec<&str> = opaque.splitn(3, '.').collect();
        if parts.len() != 3 {
            return Err(OpaqueError::Malformed("expected 3 dot-separated parts"));
        }
        let header = parts[0];
        let payload = parts[1];
        let mac = parts[2];

        if header.len() != 6 {
            return Err(OpaqueError::Malformed("header must be 6 chars"));
        }

        // Recompute and compare MAC in constant time
        let expected = self.compute_mac(header, payload);
        if !constant_time_eq(mac.as_bytes(), expected.as_bytes()) {
            return Err(OpaqueError::SignatureMismatch);
        }

        let source = Source::from_short(&header[..2])
            .ok_or(OpaqueError::Malformed("unknown source code"))?;
        let kind = Kind::from_short(header.chars().nth(2).unwrap_or('?'))
            .ok_or(OpaqueError::Malformed("unknown kind code"))?;
        let url_bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| OpaqueError::Malformed("payload base64 invalid"))?;
        let url = String::from_utf8(url_bytes)
            .map_err(|_| OpaqueError::Malformed("payload utf-8 invalid"))?;

        Ok(DecodedOpaque { source, kind, url })
    }

    /// Compute HMAC-SHA256(secret, "header.payload")[..16] -> base64url
    fn compute_mac(&self, header: &str, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac key");
        mac.update(header.as_bytes());
        mac.update(b".");
        mac.update(payload.as_bytes());
        let tag = mac.finalize().into_bytes();
        // 16 bytes = 128 bits — strong enough, gives a 22-char base64url MAC
        URL_SAFE_NO_PAD.encode(&tag[..16])
    }

    /// Sign an arbitrary payload (used for the image proxy)
    pub fn sign_image(&self, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac key");
        mac.update(b"img|");
        mac.update(payload.as_bytes());
        let tag = mac.finalize().into_bytes();
        URL_SAFE_NO_PAD.encode(&tag[..12]) // 96-bit MAC for the image proxy
    }

    /// Verify an image-proxy signature (constant-time)
    pub fn verify_image(&self, payload: &str, sig: &str) -> bool {
        let expected = self.sign_image(payload);
        constant_time_eq(expected.as_bytes(), sig.as_bytes())
    }
}

/// Result of decoding an opaque ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedOpaque {
    pub source: Source,
    pub kind: Kind,
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum OpaqueError {
    #[error("Malformed opaque ID: {0}")]
    Malformed(&'static str),
    #[error("Signature does not match — possibly tampered")]
    SignatureMismatch,
}

/// Constant-time byte equality.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Generate a 3-character base32-ish nonce
fn random_nonce_3() -> String {
    let id = uuid::Uuid::new_v4();
    let bytes = id.as_bytes();
    let chars = b"abcdefghijklmnopqrstuvwxyz234567";
    let n0 = chars[(bytes[0] as usize) % chars.len()] as char;
    let n1 = chars[(bytes[1] as usize) % chars.len()] as char;
    let n2 = chars[(bytes[2] as usize) % chars.len()] as char;
    [n0, n1, n2].iter().collect()
}

/// Pick the path where the auto-generated opaque secret is persisted.
fn persisted_secret_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(dir) = std::env::var("APIKU_DATA_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("secret"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".apiku").join("secret"));
        }
    }
    // Windows fallback.
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return Some(PathBuf::from(appdata).join("apiku").join("secret"));
        }
    }
    Some(PathBuf::from(".apiku-secret"))
}

/// Load a persisted secret, generating + storing one on first run. Returns the
/// raw secret bytes and the path used, or `None` if persistence is impossible.
fn load_or_create_persisted_secret() -> Option<(Vec<u8>, String)> {
    let path = persisted_secret_path()?;
    // Reuse an existing secret if present and non-trivial.
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.len() >= 32 {
            return Some((trimmed.as_bytes().to_vec(), path.display().to_string()));
        }
    }
    // Generate a fresh 64-hex-char (256-bit) secret and persist it.
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut raw = [0u8; 32];
    raw[..16].copy_from_slice(a.as_bytes());
    raw[16..].copy_from_slice(b.as_bytes());
    let hexed = hex::encode(raw);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, &hexed).ok()?;
    // Best-effort tighten permissions to owner-only on unix (it's a secret).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Some((hexed.into_bytes(), path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_mangaball_series() {
        let codec = OpaqueCodec::new(b"test-secret-32-bytes-of-padding!".to_vec());
        let url = "https://mangaball.net/title-detail/dark-mortal-68515c91702284f83417989a/";
        let opaque = codec.encode(Source::Mangaball, Kind::Series, url);
        assert!(opaque.starts_with("mbs"));
        let decoded = codec.decode(&opaque).expect("decode");
        assert_eq!(decoded.source, Source::Mangaball);
        assert_eq!(decoded.kind, Kind::Series);
        assert_eq!(decoded.url, url);
    }

    #[test]
    fn deterministic_with_nonce() {
        let codec = OpaqueCodec::new(b"deterministic-test-32-bytes-key!".to_vec());
        let url = "https://anichin.cafe/seri/peerless/";
        let a = codec.encode_with_nonce(Source::Anichin, Kind::Series, url, "abc");
        let b = codec.encode_with_nonce(Source::Anichin, Kind::Series, url, "abc");
        assert_eq!(a, b);
    }

    #[test]
    fn tamper_detected() {
        let codec = OpaqueCodec::new(b"secret-key-padded-to-32-bytes!!!".to_vec());
        let url = "https://anichin.cafe/seri/peerless-martial-spirit/";
        let opaque = codec.encode(Source::Anichin, Kind::Series, url);
        // Flip last char of MAC
        let tampered = format!("{}x", &opaque[..opaque.len() - 1]);
        assert!(matches!(
            codec.decode(&tampered),
            Err(OpaqueError::SignatureMismatch)
        ));
    }

    #[test]
    fn different_secret_rejected() {
        let codec_a = OpaqueCodec::new(b"key-a-padded-to-32-bytes!!!!!!!!".to_vec());
        let codec_b = OpaqueCodec::new(b"key-b-padded-to-32-bytes!!!!!!!!".to_vec());
        let url = "https://cosplaytele.com/raiden-shogun/";
        let opaque = codec_a.encode(Source::Cosplaytele, Kind::Post, url);
        assert!(matches!(
            codec_b.decode(&opaque),
            Err(OpaqueError::SignatureMismatch)
        ));
    }

    #[test]
    fn image_proxy_signature() {
        let codec = OpaqueCodec::new(b"img-test-secret-padded-to-32-byt".to_vec());
        let payload = "aHR0cHM6Ly9leGFtcGxlLmNvbS9pbWFnZS5qcGc";
        let sig = codec.sign_image(payload);
        assert!(codec.verify_image(payload, &sig));
        assert!(!codec.verify_image(payload, "deadbeef"));
        assert!(!codec.verify_image("different-payload", &sig));
    }
}
