//! Adapter for novelid.org.
//!
//! novelid.org is a server-rendered Indonesian-language novel reader. It
//! exposes three URL kinds we care about:
//!
//!   * search       - `/?s=<query>`                  -> HTML result cards
//!   * novel detail - `/novel/<slug>`                -> metadata + first ~30 chapters
//!     - `/novel/<slug>?page=<n>`                    -> chapter list page N (~30 chapters each)
//!   * chapter      - `/novel/<slug>/bab/<n>/`       -> text body + prev/next nav
//!
//! ## Upstream-paginated chapter lists
//!
//! novelid only returns ~30 chapters per fetch of the detail page. For long
//! novels (thousands of chapters) the API server fetches as many upstream
//! pages as the requested API window covers, in parallel, then slices by
//! chapter number. The pagination metadata is parsed from the
//! `<div class="pagination">` block — see `parse_detail` and
//! `detect_pagination`.
//!
//! `detail_url_for_page(canonical, n)` gives the right upstream URL for
//! page N. `NovelSeries::chapters_paginated_upstream` reflects whether the
//! site is currently using a multi-page chapter list for this novel.

use crate::adapters::{FetchContext, SiteAdapter};
use crate::error::Result;
use crate::models::{ContentModel, NovelChapter, NovelChapterRef, NovelSeries};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

/// Slug pattern: `/novel/<slug>` (no trailing /bab/)
static NOVEL_DETAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/novel/[^/]+/?$").unwrap());

/// Chapter pattern: `/novel/<slug>/bab/<n>/?`
static CHAPTER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/novel/([^/]+)/bab/(\d+)/?").unwrap());

pub struct NovelidAdapter;

impl NovelidAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Recognise novelid.org URLs.
    pub fn matches_url(url: &str) -> bool {
        url.to_lowercase().contains("novelid.org")
    }

    /// Build a URL for fetching upstream chapter-list page N for a given
    /// canonical novel detail URL. `?page=N` is the upstream pagination
    /// parameter and chapters_per_page is fixed at ~30.
    pub fn detail_url_for_page(canonical_url: &str, upstream_page: u32) -> String {
        // Strip any existing `?page=...` from the URL
        let base = canonical_url.split('?').next().unwrap_or(canonical_url);
        let p = upstream_page.max(1);
        if p == 1 {
            base.to_string()
        } else {
            format!("{}?page={}", base, p)
        }
    }

    /// Classify a URL: chapter? novel detail? other?
    fn url_kind(url: &str) -> NovelidUrl {
        if let Some(c) = CHAPTER_RE.captures(url) {
            let slug = c[1].to_string();
            let n: u32 = c[2].parse().unwrap_or(0);
            return NovelidUrl::Chapter { slug, number: n };
        }
        if let Ok(parsed) = url::Url::parse(url) {
            if NOVEL_DETAIL_RE.is_match(parsed.path()) {
                return NovelidUrl::NovelDetail;
            }
        }
        NovelidUrl::Other
    }

    /// Build the search URL for the given query and page.
    /// novelid.org search supports the `?s=<query>` parameter; pagination is
    /// `?s=<query>&page=<n>` (page>=2 only, 1 has no `page` arg).
    pub fn search_url(query: &str, page: u32) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        if page <= 1 {
            format!("https://novelid.org/?s={}", q)
        } else {
            format!("https://novelid.org/?s={}&page={}", q, page)
        }
    }

    /// Build the URL for a browse feed.
    /// `feed` values:
    ///   "home" / "latest" / ""    -> default genre listing (all novels)
    ///   "popular"                 -> /genre/tamat/ (completed novels, the
    ///                                site's closest equivalent to "popular")
    ///   any genre slug like "romantis" / "religi" / "fantasi"
    ///                             -> /genre/<slug>/
    pub fn browse_url(feed: &str, page: u32) -> String {
        let p = page.max(1);
        let slug = match feed {
            "" | "home" | "latest" | "all" | "terbaru" => "",
            "popular" | "complete" | "completed" => "tamat",
            other => other,
        };
        let base = if slug.is_empty() {
            "https://novelid.org/genre".to_string()
        } else {
            format!("https://novelid.org/genre/{}", slug)
        };
        if p == 1 {
            format!("{}/", base)
        } else {
            format!("{}/page/{}/", base, p)
        }
    }

    /// Parse the search-results HTML into a list of novel detail URLs.
    /// Each card is `<a href='/novel/<slug>' class='genre-item-box'>` with
    /// `.genre-item-title`, `.genre-item-image img`, `.genre-item-label`.
    pub fn parse_search_results(base_url: &str, html: &str) -> Vec<NovelidSearchItem> {
        let parser = HtmlParser::parse(html);
        let mut out = Vec::new();
        for el in parser.select_all("a.genre-item-box") {
            let href = match el.value().attr("href").filter(|h| !h.is_empty()) {
                Some(h) => h,
                None => continue,
            };
            // Only accept /novel/... links
            if !href.contains("/novel/") {
                continue;
            }
            let url = resolve_url(base_url, href);

            let title_sel = scraper::Selector::parse(".genre-item-title").unwrap();
            let title = el
                .select(&title_sel)
                .next()
                .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }

            let img_sel = scraper::Selector::parse(".genre-item-image img").unwrap();
            let thumbnail = el.select(&img_sel).next().and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });

            let label_sel = scraper::Selector::parse(".genre-item-label").unwrap();
            let mut tags: Vec<String> = el
                .select(&label_sel)
                .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            // Status often appears with " | " separator in label, e.g.
            // "Novel Translate | Tamat" — split
            let mut split_tags: Vec<String> = Vec::new();
            for t in tags.drain(..) {
                for part in t.split('|') {
                    let p = part.trim();
                    if !p.is_empty() && !split_tags.iter().any(|x| x == p) {
                        split_tags.push(p.to_string());
                    }
                }
            }
            let tags = split_tags;

            out.push(NovelidSearchItem {
                title,
                url,
                thumbnail,
                tags,
            });
        }
        out
    }

    /// Parse the novel detail page into a `NovelSeries`.
    pub fn parse_detail(base_url: &str, html: &str) -> Option<NovelSeries> {
        let parser = HtmlParser::parse(html);

        let title = parser.text(".detail-title").filter(|s| !s.is_empty());
        // The cover author line is ".detail-author.web-author" with text
        // "Nama Author : Fight007" — strip the prefix.
        let author = parser
            .text(".detail-author.web-author")
            .map(|s| {
                s.replace("Nama Author", "")
                    .trim_start_matches([':', '：', ' '])
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty());

        let synopsis = parser
            .inner_html(".detail-desc-info")
            .map(|raw| html_to_text(&raw))
            .filter(|s| !s.is_empty());

        // Cover: prefer the right-side <img>, fall back to the background-image
        // on .detail-top.
        let cover_image = parser
            .attr(".detail-top-right img", "src")
            .map(|s| resolve_url(base_url, &s))
            .or_else(|| {
                parser
                    .attr(".detail-top", "style")
                    .and_then(|style| extract_bg_url(&style).map(|u| resolve_url(base_url, &u)))
            });

        let rating = parser
            .text(".detail-score > span")
            .filter(|s| !s.is_empty());

        // Status / genre tags from ".detail-tag-item span". The site puts
        // both genre and status in this list, e.g. "Romantis", "Tamat".
        let mut genres: Vec<String> = parser
            .texts(".detail-tag-item span")
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        let status = genres
            .iter()
            .find(|g| {
                let l = g.to_lowercase();
                l == "tamat" || l == "ongoing" || l == "completed" || l == "complete"
            })
            .cloned();
        // Remove status from genres list to keep it clean
        if let Some(ref s) = status {
            genres.retain(|g| g != s);
        }

        // Chapters: <a class="episodes-info-a-item" href=".../bab/N/">
        let num_sel = scraper::Selector::parse(".episode-item-num").unwrap();
        let title_sel = scraper::Selector::parse(".episode-item-title").unwrap();

        let mut chapters: Vec<NovelChapterRef> = Vec::new();
        for a in parser.select_all("a.episodes-info-a-item") {
            let href = match a.value().attr("href").filter(|h| !h.is_empty()) {
                Some(h) => h,
                None => continue,
            };
            let url = resolve_url(base_url, href);
            let n: u32 = a
                .select(&num_sel)
                .next()
                .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string())
                .and_then(|s| s.parse::<u32>().ok())
                .or_else(|| {
                    CHAPTER_RE
                        .captures(&url)
                        .and_then(|c| c[2].parse::<u32>().ok())
                })
                .unwrap_or(0);
            let title = a
                .select(&title_sel)
                .next()
                .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty());
            chapters.push(NovelChapterRef {
                number: n,
                title,
                url,
            });
        }
        // Order by chapter number ascending
        chapters.sort_by_key(|c| c.number);

        // Detect upstream chapter-list pagination from `.pagination`.
        let (paginated, upstream_total_pages) = detect_pagination(&parser);
        // The site returns up to 30 chapters per page. We use the actual
        // count we observed as a hint, capped to 30 (a single page can
        // legitimately have fewer if it's the last page).
        let upstream_per_page = if paginated {
            Some(if chapters.len() > 30 {
                30
            } else {
                chapters.len().max(1) as u32
            })
        } else {
            None
        };

        let title_set = title.is_some();
        if !title_set && chapters.is_empty() {
            return None;
        }

        Some(NovelSeries {
            title,
            author,
            status,
            genres,
            synopsis,
            cover_image,
            rating,
            chapters,
            chapters_paginated_upstream: paginated,
            upstream_chapters_per_page: upstream_per_page,
            upstream_total_pages,
            url: base_url.to_string(),
        })
    }

    /// Parse a chapter reader page into a `NovelChapter`.
    pub fn parse_chapter(base_url: &str, html: &str) -> Option<NovelChapter> {
        let parser = HtmlParser::parse(html);

        let series_title = parser.text(".watch-main-title").filter(|s| !s.is_empty());
        let chapter_title = parser
            .text(".watch-chapter-title")
            .filter(|s| !s.is_empty());

        // Body: inner HTML of .watch-chapter-detail, sanitised
        let body_html = parser
            .inner_html(".watch-chapter-detail")
            .map(|h| sanitise_chapter_html(&h));
        let body = body_html.as_deref().map(html_to_text).unwrap_or_default();
        if body.is_empty() {
            return None;
        }

        // Prev / next nav
        let mut prev_url = parser
            .attr(".watch-pre a", "href")
            .filter(|s| !s.is_empty() && s != "#")
            .map(|s| resolve_url(base_url, &s));
        let next_url = parser
            .attr(".watch-next a", "href")
            .filter(|s| !s.is_empty() && s != "#")
            .map(|s| resolve_url(base_url, &s));

        // Series URL (strip /bab/N/ off the chapter URL)
        let series_url = CHAPTER_RE
            .captures(base_url)
            .map(|c| format!("https://novelid.org/novel/{}", &c[1]));

        let chapter_number = CHAPTER_RE
            .captures(base_url)
            .and_then(|c| c[2].parse::<u32>().ok())
            .unwrap_or(0);

        // Fallback: NovelID sometimes omits or leaves the previous link as '#'
        // If we are on chapter > 1, we can safely synthesise the previous chapter URL.
        if prev_url.is_none() && chapter_number > 1 {
            if let Some(ref s_url) = series_url {
                prev_url = Some(format!("{}/bab/{}/", s_url, chapter_number - 1));
            }
        }

        Some(NovelChapter {
            series_title,
            chapter_number,
            chapter_title,
            body,
            body_html,
            prev_url,
            next_url,
            series_url,
            url: base_url.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct NovelidSearchItem {
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NovelidUrl {
    NovelDetail,
    Chapter { slug: String, number: u32 },
    Other,
}

/// Convert a (small) snippet of HTML into clean plain text. Replaces `<br>`
/// with newlines, strips other tags, decodes common entities, collapses
/// whitespace per paragraph but preserves paragraph breaks.
fn html_to_text(html: &str) -> String {
    // Replace block-ish boundaries with newlines
    let with_breaks = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n\n")
        .replace("</P>", "\n\n");

    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    let no_tags = TAG_RE.replace_all(&with_breaks, "");

    // Decode common HTML entities. We use scraper's entity decoder for
    // correctness, but a tiny manual map covers most cases here.
    let decoded = decode_entities(&no_tags);

    // Collapse runs of spaces/tabs but keep newlines
    let mut out = String::with_capacity(decoded.len());
    let mut last_space = false;
    let mut newline_run = 0;
    for ch in decoded.chars() {
        match ch {
            ' ' | '\t' | '\u{a0}' => {
                if !last_space && newline_run == 0 {
                    out.push(' ');
                    last_space = true;
                }
            }
            '\n' => {
                if newline_run < 2 {
                    out.push('\n');
                }
                newline_run += 1;
                last_space = false;
            }
            '\r' => {}
            _ => {
                out.push(ch);
                last_space = false;
                newline_run = 0;
            }
        }
    }
    out.trim().to_string()
}

/// Decode a small set of common HTML entities. The chapter HTML uses
/// `&ldquo;`, `&rdquo;`, `&hellip;` etc. for stylised quotes.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Find the closing ;
            if let Some(end) = s[i..].find(';') {
                let entity = &s[i..i + end + 1];
                let replacement = match entity {
                    "&amp;" => Some("&"),
                    "&lt;" => Some("<"),
                    "&gt;" => Some(">"),
                    "&quot;" => Some("\""),
                    "&apos;" => Some("'"),
                    "&nbsp;" => Some(" "),
                    "&ldquo;" => Some("\u{201C}"),
                    "&rdquo;" => Some("\u{201D}"),
                    "&lsquo;" => Some("\u{2018}"),
                    "&rsquo;" => Some("\u{2019}"),
                    "&hellip;" => Some("\u{2026}"),
                    "&mdash;" => Some("\u{2014}"),
                    "&ndash;" => Some("\u{2013}"),
                    _ => None,
                };
                if let Some(r) = replacement {
                    out.push_str(r);
                    i += entity.len();
                    continue;
                }
                // Numeric entity? &#1234; or &#x1A;
                if entity.starts_with("&#") {
                    let inner = &entity[2..entity.len() - 1];
                    let cp = if let Some(hex) = inner.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        inner.parse::<u32>().ok()
                    };
                    if let Some(c) = cp.and_then(char::from_u32) {
                        out.push(c);
                        i += entity.len();
                        continue;
                    }
                }
            }
        }
        // Push the byte's char (multi-byte safe via char_indices walk)
        let ch_len = bytes[i].leading_ones() as usize;
        let char_bytes = if ch_len == 0 { 1 } else { ch_len };
        let end = (i + char_bytes).min(bytes.len());
        if let Ok(slice) = std::str::from_utf8(&bytes[i..end]) {
            out.push_str(slice);
        }
        i = end;
    }
    out
}

/// Strip ad/script/style elements that occasionally get inlined into the
/// chapter body.
fn sanitise_chapter_html(html: &str) -> String {
    static SCRIPT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    static STYLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    static IFRAME_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<iframe[^>]*>.*?</iframe>").unwrap());
    let s = SCRIPT_RE.replace_all(html, "");
    let s = STYLE_RE.replace_all(&s, "");
    let s = IFRAME_RE.replace_all(&s, "");
    s.into_owned()
}

/// Extract a URL from a `background-image: url(...)` style attribute.
fn extract_bg_url(style: &str) -> Option<String> {
    let needle = "url(";
    let start = style.find(needle)? + needle.len();
    let rest = &style[start..];
    let end = rest.find(')')?;
    let raw = rest[..end].trim().trim_matches(|c| c == '\'' || c == '"');
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

/// Detect whether the chapter list is paginated upstream and, if so, what
/// the highest page number is. novelid.org renders the pager as
/// `<div class="pagination"><a href="?page=44" class="pagination-number">44</a> ...`.
fn detect_pagination(parser: &HtmlParser) -> (bool, Option<u32>) {
    let nodes = parser.select_all(".pagination .pagination-number");
    if nodes.is_empty() {
        return (false, None);
    }
    let mut max_page: u32 = 1;
    for n in &nodes {
        let txt = n.text().collect::<Vec<_>>().join("").trim().to_string();
        if let Ok(p) = txt.parse::<u32>() {
            if p > max_page {
                max_page = p;
            }
        }
        // Also check the href in case the visible text is "..." or a glyph
        if let Some(href) = n.value().attr("href") {
            if let Some(cap) = PAGE_QS_RE.captures(href) {
                if let Ok(p) = cap[1].parse::<u32>() {
                    if p > max_page {
                        max_page = p;
                    }
                }
            }
        }
    }
    (true, Some(max_page))
}

static PAGE_QS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[?&]page=(\d+)").unwrap());

#[async_trait]
impl SiteAdapter for NovelidAdapter {
    fn name(&self) -> &str {
        "novelid"
    }

    fn matches(&self, url: &str) -> bool {
        Self::matches_url(url)
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        let mut h = HashMap::new();
        h.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string(),
        );
        h.insert(
            "Accept-Language".to_string(),
            "id,en-US;q=0.9,en;q=0.8".to_string(),
        );
        Some(h)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        match Self::url_kind(url) {
            NovelidUrl::Chapter { .. } => {
                if let Some(c) = Self::parse_chapter(url, html) {
                    return Ok(vec![ContentModel::NovelChapter(c)]);
                }
            }
            NovelidUrl::NovelDetail => {
                if let Some(s) = Self::parse_detail(url, html) {
                    return Ok(vec![ContentModel::NovelSeries(s)]);
                }
            }
            NovelidUrl::Other => {}
        }
        Ok(vec![])
    }

    async fn extract_with_context(
        &self,
        url: &str,
        html: &str,
        _ctx: &FetchContext<'_>,
    ) -> Result<Vec<ContentModel>> {
        self.extract(url, html).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_recognition() {
        assert!(NovelidAdapter::matches_url("https://novelid.org/"));
        assert!(NovelidAdapter::matches_url("https://novelid.org/novel/foo"));
        assert!(NovelidAdapter::matches_url(
            "https://novelid.org/novel/foo/bab/3/"
        ));
        assert!(!NovelidAdapter::matches_url("https://example.com/"));
    }

    #[test]
    fn url_kinds() {
        match NovelidAdapter::url_kind("https://novelid.org/novel/martial-universe-wu-dong/bab/5/")
        {
            NovelidUrl::Chapter { ref slug, number } => {
                assert_eq!(slug, "martial-universe-wu-dong");
                assert_eq!(number, 5);
            }
            other => panic!("expected Chapter, got {:?}", other),
        }
        assert_eq!(
            NovelidAdapter::url_kind("https://novelid.org/novel/foo"),
            NovelidUrl::NovelDetail
        );
        assert_eq!(
            NovelidAdapter::url_kind("https://novelid.org/novel/foo/"),
            NovelidUrl::NovelDetail
        );
        assert_eq!(
            NovelidAdapter::url_kind("https://novelid.org/?s=foo"),
            NovelidUrl::Other
        );
    }

    #[test]
    fn entity_decoding() {
        assert_eq!(
            decode_entities("&ldquo;Wuu.&rdquo;"),
            "\u{201C}Wuu.\u{201D}"
        );
        assert_eq!(decode_entities("a &amp; b"), "a & b");
        assert_eq!(decode_entities("&hellip;"), "\u{2026}");
        assert_eq!(decode_entities("&#65;"), "A");
    }

    #[test]
    fn html_to_text_collapses_paragraphs() {
        let html = "<p>Hello <b>world</b>.</p><p>Second line</p>";
        let txt = html_to_text(html);
        assert!(txt.contains("Hello world."));
        assert!(txt.contains("Second line"));
        // Two paragraphs separated by a blank line
        assert!(txt.contains("\n\n"));
    }

    #[test]
    fn search_url_construction() {
        assert_eq!(
            NovelidAdapter::search_url("Martial Universe", 1),
            "https://novelid.org/?s=Martial%20Universe"
        );
        assert_eq!(
            NovelidAdapter::search_url("Martial Universe", 2),
            "https://novelid.org/?s=Martial%20Universe&page=2"
        );
    }

    #[test]
    fn extract_bg_url_works() {
        let style = "background-image: url(/uploads/foo/bar.webp); height: 100%;";
        assert_eq!(
            extract_bg_url(style).as_deref(),
            Some("/uploads/foo/bar.webp")
        );
        let style2 = "background-image:url('https://example.com/x.jpg')";
        assert_eq!(
            extract_bg_url(style2).as_deref(),
            Some("https://example.com/x.jpg")
        );
    }

    #[test]
    fn search_parses_card() {
        let html = r#"
            <a href='/novel/martial-universe-wu-dong-qian-kun-terjemah-indo' class='genre-item-box'>
                <div class='genre-content-item'>
                    <div class='genre-item-image'>
                        <img src='https://i2.wp.com/novelid.org/uploads/foo.webp'>
                    </div>
                    <div class='genre-item-info'>
                        <p class='genre-item-title'>Martial Universe (Wu Dong Qian Kun Terjemah Indo)</p>
                        <div class='genre-label'>
                            <span class='genre-item-label'> Novel Translate | Tamat </span>
                        </div>
                    </div>
                </div>
            </a>
        "#;
        let items = NovelidAdapter::parse_search_results("https://novelid.org/", html);
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(
            it.title,
            "Martial Universe (Wu Dong Qian Kun Terjemah Indo)"
        );
        assert!(it
            .url
            .contains("/novel/martial-universe-wu-dong-qian-kun-terjemah-indo"));
        assert!(it.thumbnail.is_some());
        assert!(it.tags.iter().any(|t| t == "Novel Translate"));
        assert!(it.tags.iter().any(|t| t == "Tamat"));
    }

    #[test]
    fn detail_parses_chapters() {
        let html = r#"
            <div class="detail-top-wrapper">
                <div class="detail-top-left">
                    <p class="detail-author web-author">Nama Author ：Fight007</p>
                    <div class="detail-title">Martial Universe</div>
                    <div class="detail-score-box"><div class="detail-score"><span>8.00</span></div></div>
                    <div class="detail-desc-info">A great <b>story</b> &hellip;</div>
                </div>
                <div class="detail-top-right"><img src="/uploads/cover.webp"></div>
            </div>
            <div class="detail-tag-content">
                <div class="detail-tag-item"><span>Romantis</span></div>
                <div class="detail-tag-item"><span>Tamat</span></div>
            </div>
            <div class="episodes-info">
                <a class="episodes-info-a-item" href="/novel/foo/bab/1/">
                    <div class="episodes-item">
                        <span class="episode-item-num">1</span>
                        <div class="episode-item-detail">
                            <span class="episode-item-title">Lin Dong - Bagian 1</span>
                        </div>
                    </div>
                </a>
                <a class="episodes-info-a-item" href="/novel/foo/bab/2/">
                    <div class="episodes-item">
                        <span class="episode-item-num">2</span>
                        <div class="episode-item-detail">
                            <span class="episode-item-title">Tinju Penetrasi - 2</span>
                        </div>
                    </div>
                </a>
            </div>
        "#;
        let s = NovelidAdapter::parse_detail("https://novelid.org/novel/foo", html).unwrap();
        assert_eq!(s.title.as_deref(), Some("Martial Universe"));
        assert_eq!(s.author.as_deref(), Some("Fight007"));
        assert_eq!(s.rating.as_deref(), Some("8.00"));
        assert_eq!(s.status.as_deref(), Some("Tamat"));
        assert_eq!(s.genres, vec!["Romantis"]);
        assert!(s.synopsis.as_deref().unwrap().contains("A great story"));
        assert_eq!(s.chapters.len(), 2);
        assert_eq!(s.chapters[0].number, 1);
        assert_eq!(s.chapters[0].title.as_deref(), Some("Lin Dong - Bagian 1"));
        assert!(s.chapters[0].url.contains("/novel/foo/bab/1/"));
    }

    #[test]
    fn detail_detects_upstream_pagination() {
        let html = r#"
            <div class="detail-title">Big Novel</div>
            <div class="episodes-info">
                <a class="episodes-info-a-item" href="/novel/foo/bab/1/">
                    <div class="episodes-item">
                        <span class="episode-item-num">1</span>
                        <span class="episode-item-title">Bab 1</span>
                    </div>
                </a>
            </div>
            <div class="pagination">
                <a href="?page=2" class="pagination-number">1</a>
                <a href="?page=2" class="pagination-number active">2</a>
                <a href="?page=3" class="pagination-number">3</a>
                <span class="pagination-ellipsis">...</span>
                <a href="?page=44" class="pagination-number">44</a>
            </div>
        "#;
        let s = NovelidAdapter::parse_detail("https://novelid.org/novel/foo", html).unwrap();
        assert!(s.chapters_paginated_upstream);
        assert_eq!(s.upstream_total_pages, Some(44));
        assert_eq!(s.upstream_chapters_per_page, Some(1));
    }

    #[test]
    fn detail_url_for_page_strips_existing_query() {
        let u1 = NovelidAdapter::detail_url_for_page("https://novelid.org/novel/foo", 1);
        assert_eq!(u1, "https://novelid.org/novel/foo");

        let u3 = NovelidAdapter::detail_url_for_page("https://novelid.org/novel/foo", 3);
        assert_eq!(u3, "https://novelid.org/novel/foo?page=3");

        // Existing ?page=N is stripped before adding the new one
        let u5 = NovelidAdapter::detail_url_for_page("https://novelid.org/novel/foo?page=2", 5);
        assert_eq!(u5, "https://novelid.org/novel/foo?page=5");
    }

    #[test]
    fn chapter_parses_body_and_nav() {
        let html = r##"
            <div class="watch-main-top">
                <p class="watch-main-title">Martial Universe</p>
            </div>
            <div class="watch-chapter-content">
                <p class="watch-chapter-title">Lin Dong - Bagian 1</p>
                <div class="watch-chapter-detail">
                    <p>&ldquo;Wuu.&rdquo;</p>
                    <p>Ketika Lin Dong mengumpulkan kekuatan&hellip;</p>
                </div>
            </div>
            <div class="watch-change">
                <div class="watch-pre"><a href="#"><span>Episode sebelumnya</span></a></div>
                <div class="watch-next"><a href="https://novelid.org/novel/foo/bab/2/"><span>Episode berikutnya</span></a></div>
            </div>
        "##;
        let c =
            NovelidAdapter::parse_chapter("https://novelid.org/novel/foo/bab/1/", html).unwrap();
        assert_eq!(c.series_title.as_deref(), Some("Martial Universe"));
        assert_eq!(c.chapter_number, 1);
        assert_eq!(c.chapter_title.as_deref(), Some("Lin Dong - Bagian 1"));
        assert!(c.body.contains("\u{201C}Wuu.\u{201D}"));
        assert!(c.body.contains("Lin Dong"));
        assert!(c.next_url.as_deref().unwrap().ends_with("/bab/2/"));
        assert!(c.prev_url.is_none(), "prev was {:?}", c.prev_url);
        assert_eq!(
            c.series_url.as_deref(),
            Some("https://novelid.org/novel/foo")
        );
    }
}
