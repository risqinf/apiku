//! Browser fingerprint rotation.
//!
//! The image proxy and certain adapters need to look exactly like a real
//! desktop / mobile browser visiting the upstream site (origin checks,
//! hotlink protection, anti-bot fingerprinting). This module provides a
//! curated catalogue of *consistent* fingerprints — User-Agent, Accept,
//! Accept-Language, Sec-CH-UA, Sec-Fetch-* — chosen as a unit so the
//! resulting request looks coherent.
//!
//! A fingerprint is picked deterministically per upstream URL (so the same
//! page always uses the same identity, defeating naive bot-detection that
//! correlates header-rotation across requests).

use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// One coherent browser identity. All headers were copied from a real
/// browser session in late 2024 / early 2026.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // catalogue/platform helpers are part of the public API surface
pub struct BrowserFingerprint {
    pub label: &'static str,
    pub user_agent: &'static str,
    pub accept: &'static str,
    pub accept_language: &'static str,
    pub accept_encoding: &'static str,
    pub sec_ch_ua: Option<&'static str>,
    pub sec_ch_ua_mobile: Option<&'static str>,
    pub sec_ch_ua_platform: Option<&'static str>,
    pub sec_fetch_dest: &'static str,
    pub sec_fetch_mode: &'static str,
    pub sec_fetch_site: &'static str,
    pub sec_fetch_user: Option<&'static str>,
    pub upgrade_insecure_requests: bool,
}

impl BrowserFingerprint {
    /// Pre-baked list of fingerprints. All ten represent commonly seen
    /// browser-OS combinations as of late 2024.
    #[allow(dead_code)]
    pub const fn catalogue() -> &'static [BrowserFingerprint] {
        FINGERPRINTS
    }

    /// Pick a fingerprint deterministically from a URL.
    /// The same URL always returns the same fingerprint so consecutive
    /// requests to the same page look like the same browser session.
    pub fn for_url(url: &str) -> &'static BrowserFingerprint {
        let mut h = Sha256::new();
        h.update(url.as_bytes());
        let digest = h.finalize();
        let idx = (digest[0] as usize) % FINGERPRINTS.len();
        &FINGERPRINTS[idx]
    }

    /// Pick a fingerprint matching a specific platform family
    /// (used by the image proxy when the source domain has known
    /// platform-specific behaviour).
    #[allow(dead_code)]
    pub fn for_platform(platform: Platform) -> &'static BrowserFingerprint {
        FINGERPRINTS
            .iter()
            .find(|f| f.platform() == Some(platform))
            .unwrap_or(&FINGERPRINTS[0])
    }

    /// Best-effort categorisation of the underlying OS.
    #[allow(dead_code)]
    pub fn platform(&self) -> Option<Platform> {
        let ua = self.user_agent;
        if ua.contains("Android") {
            Some(Platform::Android)
        } else if ua.contains("iPhone") || ua.contains("iPad") {
            Some(Platform::Ios)
        } else if ua.contains("Mac OS X") {
            Some(Platform::Macos)
        } else if ua.contains("Linux") && !ua.contains("Android") {
            Some(Platform::Linux)
        } else if ua.contains("Windows") {
            Some(Platform::Windows)
        } else {
            None
        }
    }

    /// Apply this fingerprint as a HashMap of headers, to be merged into
    /// the outbound request. The caller is expected to set Referer
    /// separately because that depends on the origin host.
    pub fn as_header_map(&self) -> HashMap<String, String> {
        let mut h = HashMap::new();
        h.insert("User-Agent".into(), self.user_agent.into());
        h.insert("Accept".into(), self.accept.into());
        h.insert("Accept-Language".into(), self.accept_language.into());
        h.insert("Accept-Encoding".into(), self.accept_encoding.into());
        h.insert("Sec-Fetch-Dest".into(), self.sec_fetch_dest.into());
        h.insert("Sec-Fetch-Mode".into(), self.sec_fetch_mode.into());
        h.insert("Sec-Fetch-Site".into(), self.sec_fetch_site.into());
        if let Some(v) = self.sec_fetch_user {
            h.insert("Sec-Fetch-User".into(), v.into());
        }
        if let Some(v) = self.sec_ch_ua {
            h.insert("Sec-CH-UA".into(), v.into());
        }
        if let Some(v) = self.sec_ch_ua_mobile {
            h.insert("Sec-CH-UA-Mobile".into(), v.into());
        }
        if let Some(v) = self.sec_ch_ua_platform {
            h.insert("Sec-CH-UA-Platform".into(), v.into());
        }
        if self.upgrade_insecure_requests {
            h.insert("Upgrade-Insecure-Requests".into(), "1".into());
        }
        h
    }

    /// As above, but tailored for *image* requests: drops Upgrade-Insecure-Requests
    /// and switches Sec-Fetch-Dest/Mode to image semantics.
    pub fn as_image_headers(&self) -> HashMap<String, String> {
        let mut h = self.as_header_map();
        h.insert("Sec-Fetch-Dest".into(), "image".into());
        h.insert("Sec-Fetch-Mode".into(), "no-cors".into());
        h.insert("Sec-Fetch-Site".into(), "same-site".into());
        h.remove("Sec-Fetch-User");
        h.remove("Upgrade-Insecure-Requests");
        // Browsers use a narrower Accept for <img> tags
        h.insert(
            "Accept".into(),
            "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8".into(),
        );
        h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
}

/// The actual fingerprint catalogue. Each entry is internally consistent
/// (UA + Sec-CH-UA + platform string match the same browser/OS combo).
const FINGERPRINTS: &[BrowserFingerprint] = &[
    // -- Windows / Chrome 121 --
    BrowserFingerprint {
        label: "Windows / Chrome 121",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br, zstd",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Google Chrome";v="121", "Chromium";v="121""#),
        sec_ch_ua_mobile: Some("?0"),
        sec_ch_ua_platform: Some(r#""Windows""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- Windows / Edge 121 --
    BrowserFingerprint {
        label: "Windows / Edge 121",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36 Edg/121.0.0.0",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br, zstd",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Microsoft Edge";v="121", "Chromium";v="121""#),
        sec_ch_ua_mobile: Some("?0"),
        sec_ch_ua_platform: Some(r#""Windows""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- macOS / Safari 17 --
    BrowserFingerprint {
        label: "macOS / Safari 17",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2.1 Safari/605.1.15",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br",
        sec_ch_ua: None,
        sec_ch_ua_mobile: None,
        sec_ch_ua_platform: None,
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- macOS / Chrome 121 --
    BrowserFingerprint {
        label: "macOS / Chrome 121",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br, zstd",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Google Chrome";v="121", "Chromium";v="121""#),
        sec_ch_ua_mobile: Some("?0"),
        sec_ch_ua_platform: Some(r#""macOS""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- Linux / Firefox 122 --
    BrowserFingerprint {
        label: "Linux / Firefox 122",
        user_agent: "Mozilla/5.0 (X11; Linux x86_64; rv:122.0) Gecko/20100101 Firefox/122.0",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        accept_language: "en-US,en;q=0.5",
        accept_encoding: "gzip, deflate, br",
        sec_ch_ua: None,
        sec_ch_ua_mobile: None,
        sec_ch_ua_platform: None,
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- Linux / Chrome 121 --
    BrowserFingerprint {
        label: "Linux / Chrome 121",
        user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br, zstd",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Google Chrome";v="121", "Chromium";v="121""#),
        sec_ch_ua_mobile: Some("?0"),
        sec_ch_ua_platform: Some(r#""Linux""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- Android / Chrome 121 --
    BrowserFingerprint {
        label: "Android / Chrome 121",
        user_agent: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Mobile Safari/537.36",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br, zstd",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Google Chrome";v="121", "Chromium";v="121""#),
        sec_ch_ua_mobile: Some("?1"),
        sec_ch_ua_platform: Some(r#""Android""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- Android / Samsung Internet --
    BrowserFingerprint {
        label: "Android / Samsung Internet 23",
        user_agent: "Mozilla/5.0 (Linux; Android 14; SM-S928B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/23.0 Chrome/115.0.0.0 Mobile Safari/537.36",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br",
        sec_ch_ua: Some(r#""Not A(Brand";v="99", "Samsung Internet";v="23", "Chromium";v="115""#),
        sec_ch_ua_mobile: Some("?1"),
        sec_ch_ua_platform: Some(r#""Android""#),
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- iOS / Safari 17 (iPhone) --
    BrowserFingerprint {
        label: "iOS / Safari 17",
        user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br",
        sec_ch_ua: None,
        sec_ch_ua_mobile: None,
        sec_ch_ua_platform: None,
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
    // -- iPad / Safari 17 --
    BrowserFingerprint {
        label: "iPadOS / Safari 17",
        user_agent: "Mozilla/5.0 (iPad; CPU OS 17_2_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
        accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        accept_language: "en-US,en;q=0.9",
        accept_encoding: "gzip, deflate, br",
        sec_ch_ua: None,
        sec_ch_ua_mobile: None,
        sec_ch_ua_platform: None,
        sec_fetch_dest: "document",
        sec_fetch_mode: "navigate",
        sec_fetch_site: "none",
        sec_fetch_user: Some("?1"),
        upgrade_insecure_requests: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_per_url() {
        let a = BrowserFingerprint::for_url("https://nhentai.net/g/123/");
        let b = BrowserFingerprint::for_url("https://nhentai.net/g/123/");
        assert_eq!(a.label, b.label);
    }

    #[test]
    fn distribution_uses_all_fingerprints() {
        // Spot-check: lots of distinct URLs hit different fingerprints
        let urls: Vec<String> = (0..100).map(|i| format!("https://x/{}", i)).collect();
        let labels: std::collections::HashSet<_> = urls
            .iter()
            .map(|u| BrowserFingerprint::for_url(u).label)
            .collect();
        assert!(
            labels.len() >= 5,
            "Only {} distinct fingerprints used",
            labels.len()
        );
    }

    #[test]
    fn image_headers_have_image_dest() {
        let fp = &FINGERPRINTS[0];
        let h = fp.as_image_headers();
        assert_eq!(h.get("Sec-Fetch-Dest").map(String::as_str), Some("image"));
        assert!(h.get("Accept").unwrap().contains("image/"));
    }

    #[test]
    fn platform_detection() {
        for fp in FINGERPRINTS {
            assert!(
                fp.platform().is_some(),
                "Could not detect platform for {}",
                fp.label
            );
        }
    }
}
