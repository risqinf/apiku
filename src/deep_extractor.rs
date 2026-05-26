//! Deep page extractor — works on ANY HTML page without requiring a site-specific adapter.
//!
//! Extracts all the structured information possible from a generic HTML page:
//! titles, headings, links, images, media, OpenGraph metadata, JSON-LD,
//! API endpoints, inline JSON, scripts, stylesheets, and forms.

use crate::models::{
    DeepPage, FormRef, Heading, ImageRef, LinkRef, MediaRef,
};
use crate::parser::{resolve_url, HtmlParser};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use url::Url;

static API_URL_RE: Lazy<Regex> = Lazy::new(|| {
    // Match URLs that look like API endpoints in JSON/JS strings
    Regex::new(
        r#"["'`](/(?:api|ajax|graphql|rpc|wp-json|wp-admin/admin-ajax\.php)[^"'`\s]*)["'`]"#,
    )
    .unwrap()
});

static FETCH_CALL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:fetch|axios\.[a-z]+|\$\.(?:get|post|ajax))\s*\(\s*['"`]([^'"`]+)['"`]"#).unwrap()
});

static ABS_API_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"["'`](https?://[^"'`\s]*?/(?:api|ajax|graphql|rpc|wp-json)[^"'`\s]*)["'`]"#)
        .unwrap()
});

static INLINE_JSON_OBJECT_RE: Lazy<Regex> = Lazy::new(|| {
    // Match window.__INITIAL_STATE__ = {...}; and similar patterns
    Regex::new(r#"(?s)window\.__\w+__\s*=\s*(\{.+?\});"#).unwrap()
});

/// Run a comprehensive deep extraction on the given HTML and return a `DeepPage`.
pub fn extract_deep(url: &str, html: &str, status_code: u16) -> DeepPage {
    let parser = HtmlParser::parse(html);

    // --- Basic page metadata ------------------------------------------------
    let title = parser
        .text("title")
        .or_else(|| parser.attr("meta[property='og:title']", "content"))
        .or_else(|| parser.text("h1"));

    let description = parser
        .attr("meta[name='description']", "content")
        .or_else(|| parser.attr("meta[property='og:description']", "content"));

    let canonical = parser.attr("link[rel='canonical']", "href");

    let language = parser
        .attr("html", "lang")
        .or_else(|| parser.attr("meta[http-equiv='content-language']", "content"));

    // --- Meta tags ----------------------------------------------------------
    let mut meta: HashMap<String, String> = HashMap::new();
    let mut og: HashMap<String, String> = HashMap::new();

    for el in parser.select_all("meta") {
        let v = el.value();
        let content = match v.attr("content") {
            Some(c) => c.to_string(),
            None => continue,
        };

        if let Some(name) = v.attr("name") {
            meta.insert(name.to_lowercase(), content.clone());
        }
        if let Some(property) = v.attr("property") {
            let p = property.to_lowercase();
            if p.starts_with("og:") || p.starts_with("twitter:") || p.starts_with("fb:") {
                og.insert(p, content.clone());
            } else {
                meta.insert(p, content.clone());
            }
        }
    }

    // --- JSON-LD structured data --------------------------------------------
    let mut json_ld: Vec<serde_json::Value> = Vec::new();
    for el in parser.select_all("script[type='application/ld+json']") {
        let raw = el.text().collect::<Vec<_>>().join("");
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
            json_ld.push(v);
        }
    }

    // --- Inline JSON in script tags -----------------------------------------
    let mut inline_json: Vec<serde_json::Value> = Vec::new();
    // application/json
    for el in parser.select_all("script[type='application/json']") {
        let raw = el.text().collect::<Vec<_>>().join("");
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
            inline_json.push(v);
        }
    }
    // window.__STATE__ = {...} patterns
    for caps in INLINE_JSON_OBJECT_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(m.as_str()) {
                inline_json.push(v);
            }
        }
    }

    // --- Headings (h1-h3) ---------------------------------------------------
    let mut headings: Vec<Heading> = Vec::new();
    for level in 1u8..=3 {
        for el in parser.select_all(&format!("h{}", level)) {
            let text = el.text().collect::<Vec<_>>().join("").trim().to_string();
            if !text.is_empty() {
                headings.push(Heading {
                    level,
                    text: truncate_str(&text, 500),
                });
            }
        }
    }

    // --- Links --------------------------------------------------------------
    let base_host = Url::parse(url).ok().and_then(|u| u.host_str().map(|s| s.to_string()));
    let mut seen_links: HashSet<String> = HashSet::new();
    let mut links: Vec<LinkRef> = Vec::new();

    for el in parser.select_all("a[href]") {
        let v = el.value();
        let href = match v.attr("href") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };

        // Skip JS/anchor/mailto links
        if href.starts_with("javascript:")
            || href == "#"
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            continue;
        }

        let resolved = resolve_url(url, href);

        if seen_links.contains(&resolved) {
            continue;
        }
        seen_links.insert(resolved.clone());

        let text = el.text().collect::<Vec<_>>().join("").trim().to_string();
        let text = if text.is_empty() {
            None
        } else {
            Some(truncate_str(&text, 300))
        };

        let rel = v.attr("rel").map(|s| s.to_string());

        let is_external = if let Some(host) = &base_host {
            Url::parse(&resolved)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()))
                .map(|h| h != *host)
                .unwrap_or(false)
        } else {
            false
        };

        links.push(LinkRef {
            url: resolved,
            text,
            rel,
            is_external,
        });

        if links.len() >= 5000 {
            break;
        }
    }

    // --- Images -------------------------------------------------------------
    let mut seen_images: HashSet<String> = HashSet::new();
    let mut images: Vec<ImageRef> = Vec::new();

    for el in parser.select_all("img") {
        let v = el.value();

        // Try multiple src attributes (lazy-load aware)
        let src = v
            .attr("data-src")
            .or_else(|| v.attr("data-lazy-src"))
            .or_else(|| v.attr("data-original"))
            .or_else(|| v.attr("data-lazyload"))
            .or_else(|| {
                v.attr("src").filter(|s| !is_placeholder_image(s))
            })
            .or_else(|| v.attr("src"));

        let src = match src {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        let resolved = resolve_url(url, src);
        if seen_images.contains(&resolved) {
            continue;
        }
        seen_images.insert(resolved.clone());

        let alt = v.attr("alt").map(|s| truncate_str(s, 300));
        let width = v.attr("width").map(|s| s.to_string());
        let height = v.attr("height").map(|s| s.to_string());

        // Parse srcset
        let srcset: Vec<String> = v
            .attr("srcset")
            .or_else(|| v.attr("data-srcset"))
            .map(|s| {
                s.split(',')
                    .filter_map(|part| {
                        part.split_whitespace()
                            .next()
                            .map(|u| resolve_url(url, u))
                    })
                    .collect()
            })
            .unwrap_or_default();

        images.push(ImageRef {
            url: resolved,
            alt,
            width,
            height,
            srcset,
        });

        if images.len() >= 2000 {
            break;
        }
    }

    // --- Media (video / audio / iframe) -------------------------------------
    let mut media: Vec<MediaRef> = Vec::new();
    let mut seen_media: HashSet<String> = HashSet::new();

    for el in parser.select_all("video") {
        let v = el.value();
        let poster = v.attr("poster").map(|s| resolve_url(url, s));

        if let Some(src) = v.attr("src") {
            let resolved = resolve_url(url, src);
            if seen_media.insert(resolved.clone()) {
                media.push(MediaRef {
                    url: resolved,
                    kind: "video".to_string(),
                    mime_type: v.attr("type").map(|s| s.to_string()),
                    poster: poster.clone(),
                });
            }
        }

        // Also extract <source> children
        for child in el.children() {
            if let Some(elref) = scraper::ElementRef::wrap(child) {
                if elref.value().name() == "source" {
                    if let Some(src) = elref.value().attr("src") {
                        let resolved = resolve_url(url, src);
                        if seen_media.insert(resolved.clone()) {
                            media.push(MediaRef {
                                url: resolved,
                                kind: "video".to_string(),
                                mime_type: elref.value().attr("type").map(|s| s.to_string()),
                                poster: poster.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    for el in parser.select_all("audio") {
        let v = el.value();
        if let Some(src) = v.attr("src") {
            let resolved = resolve_url(url, src);
            if seen_media.insert(resolved.clone()) {
                media.push(MediaRef {
                    url: resolved,
                    kind: "audio".to_string(),
                    mime_type: v.attr("type").map(|s| s.to_string()),
                    poster: None,
                });
            }
        }
        for child in el.children() {
            if let Some(elref) = scraper::ElementRef::wrap(child) {
                if elref.value().name() == "source" {
                    if let Some(src) = elref.value().attr("src") {
                        let resolved = resolve_url(url, src);
                        if seen_media.insert(resolved.clone()) {
                            media.push(MediaRef {
                                url: resolved,
                                kind: "audio".to_string(),
                                mime_type: elref.value().attr("type").map(|s| s.to_string()),
                                poster: None,
                            });
                        }
                    }
                }
            }
        }
    }

    for el in parser.select_all("iframe[src]") {
        if let Some(src) = el.value().attr("src") {
            if !src.is_empty() && !src.starts_with("javascript:") {
                let resolved = resolve_url(url, src);
                if seen_media.insert(resolved.clone()) {
                    media.push(MediaRef {
                        url: resolved,
                        kind: "iframe".to_string(),
                        mime_type: None,
                        poster: None,
                    });
                }
            }
        }
    }

    for el in parser.select_all("embed[src]") {
        if let Some(src) = el.value().attr("src") {
            let resolved = resolve_url(url, src);
            if seen_media.insert(resolved.clone()) {
                media.push(MediaRef {
                    url: resolved,
                    kind: "embed".to_string(),
                    mime_type: el.value().attr("type").map(|s| s.to_string()),
                    poster: None,
                });
            }
        }
    }

    // --- Scripts ------------------------------------------------------------
    let mut scripts: Vec<String> = Vec::new();
    let mut seen_scripts: HashSet<String> = HashSet::new();
    for el in parser.select_all("script[src]") {
        if let Some(src) = el.value().attr("src") {
            let resolved = resolve_url(url, src);
            if seen_scripts.insert(resolved.clone()) {
                scripts.push(resolved);
            }
        }
    }

    // --- Stylesheets --------------------------------------------------------
    let mut stylesheets: Vec<String> = Vec::new();
    let mut seen_css: HashSet<String> = HashSet::new();
    for el in parser.select_all("link[rel='stylesheet'], link[rel='preload'][as='style']") {
        if let Some(href) = el.value().attr("href") {
            let resolved = resolve_url(url, href);
            if seen_css.insert(resolved.clone()) {
                stylesheets.push(resolved);
            }
        }
    }

    // --- API endpoints (network tracking) -----------------------------------
    let mut api_endpoints: Vec<String> = Vec::new();
    let mut seen_apis: HashSet<String> = HashSet::new();

    // Relative API patterns
    for caps in API_URL_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let endpoint = m.as_str();
            let resolved = resolve_url(url, endpoint);
            if seen_apis.insert(resolved.clone()) {
                api_endpoints.push(resolved);
            }
        }
    }

    // Absolute API patterns
    for caps in ABS_API_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let endpoint = m.as_str().to_string();
            if seen_apis.insert(endpoint.clone()) {
                api_endpoints.push(endpoint);
            }
        }
    }

    // fetch() / axios / $.ajax calls
    for caps in FETCH_CALL_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let endpoint = m.as_str();
            // Only collect if it looks like a URL or path
            if endpoint.starts_with('/') || endpoint.starts_with("http") {
                let resolved = resolve_url(url, endpoint);
                if seen_apis.insert(resolved.clone()) {
                    api_endpoints.push(resolved);
                }
            }
        }
    }

    api_endpoints.sort();

    // --- Forms --------------------------------------------------------------
    let mut forms: Vec<FormRef> = Vec::new();
    for form_el in parser.select_all("form") {
        let v = form_el.value();
        let action = v.attr("action").map(|s| resolve_url(url, s));
        let method = v
            .attr("method")
            .map(|s| s.to_uppercase())
            .unwrap_or_else(|| "GET".to_string());

        // Collect input names
        let mut fields: Vec<String> = Vec::new();
        for input in form_el.select(&scraper::Selector::parse("input,textarea,select").unwrap()) {
            if let Some(name) = input.value().attr("name") {
                fields.push(name.to_string());
            }
        }

        forms.push(FormRef {
            action,
            method,
            fields,
        });
    }

    // --- Plain text excerpt -------------------------------------------------
    let body_text = parser
        .text("body")
        .or_else(|| parser.text("main"))
        .or_else(|| parser.text("article"));

    let text_excerpt = body_text.map(|s| {
        let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
        truncate_str(&collapsed, 2000)
    });

    // --- SPA detection ------------------------------------------------------
    // A page is likely a SPA when:
    //  - It has very little visible text content
    //  - It has many empty containers with IDs
    //  - It has lots of scripts and discovered API endpoints
    let visible_text_len = text_excerpt.as_ref().map(|s| s.len()).unwrap_or(0);
    let empty_containers = parser
        .select_all("div[id]")
        .iter()
        .filter(|el| el.text().collect::<String>().trim().is_empty())
        .count();
    let is_spa = visible_text_len < 500
        || (empty_containers >= 3 && !api_endpoints.is_empty() && !scripts.is_empty());

    DeepPage {
        url: url.to_string(),
        title,
        description,
        canonical,
        language,
        is_spa,
        status_code,
        og,
        meta,
        json_ld,
        headings,
        links,
        images,
        media,
        scripts,
        stylesheets,
        api_endpoints,
        inline_json,
        forms,
        text_excerpt,
    }
}

fn is_placeholder_image(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("placeholder")
        || lower.contains("loading.gif")
        || lower.starts_with("data:image")
        || lower.contains("blank.gif")
        || lower.contains("/1x1.")
        || lower.contains("pixel.gif")
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "..."
    }
}
