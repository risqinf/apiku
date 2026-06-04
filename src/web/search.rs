//! Cross-site search abstraction.
//!
//! Each supported site has its own search URL pattern. Search results are
//! parsed into a unified `SearchResult` model so they can be merged across
//! sites and consumed by the API/website client uniformly.

use crate::models::ContentModel;
use crate::parser::{resolve_url, HtmlParser};
use serde::{Deserialize, Serialize};

/// Source identifier for a search result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchSource {
    Cosplaytele,
    Anichin,
    /// Mangaball uses a JSON API for search (separate code path)
    Mangaball,
    /// nhentai uses its own JSON API
    Nhentai,
    /// novelid.org HTML-based search
    Novelid,
    /// otakudesu.blog HTML-based anime search
    Otakudesu,
}

impl SearchSource {
    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            SearchSource::Cosplaytele => "cosplaytele",
            SearchSource::Anichin => "anichin",
            SearchSource::Mangaball => "mangaball",
            SearchSource::Nhentai => "nhentai",
            SearchSource::Novelid => "novelid",
            SearchSource::Otakudesu => "otakudesu",
        }
    }

    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cosplaytele" | "cosplay" => Some(Self::Cosplaytele),
            "anichin" | "donghua" => Some(Self::Anichin),
            "mangaball" | "manga" | "mb" => Some(Self::Mangaball),
            "nhentai" | "nh" => Some(Self::Nhentai),
            "novelid" | "novel" | "nv" => Some(Self::Novelid),
            "otakudesu" | "anime" | "od" => Some(Self::Otakudesu),
            _ => None,
        }
    }
}

/// Build the search URL for a given site (HTML-search, GET-based)
pub fn build_search_url(source: SearchSource, query: &str, page: u32) -> Option<String> {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    let q = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();

    match source {
        SearchSource::Cosplaytele => {
            if page <= 1 {
                Some(format!("https://cosplaytele.com/?s={}", q))
            } else {
                Some(format!("https://cosplaytele.com/page/{}/?s={}", page, q))
            }
        }
        SearchSource::Anichin => {
            if page <= 1 {
                Some(format!("https://anichin.cafe/?s={}", q))
            } else {
                Some(format!("https://anichin.cafe/page/{}/?s={}", page, q))
            }
        }
        SearchSource::Mangaball => None, // Use JSON API instead
        SearchSource::Nhentai => {
            Some(crate::adapters::nhentai::NhentaiAdapter::api_url_for_search(query, page.max(1)))
        }
        SearchSource::Novelid => Some(crate::adapters::novelid::NovelidAdapter::search_url(
            query,
            page.max(1),
        )),
        SearchSource::Otakudesu => Some(crate::adapters::otakudesu::OtakudesuAdapter::search_url(
            query,
            page.max(1),
        )),
    }
}

/// Build a Mangaball search URL endpoint (POSTed elsewhere)
pub fn mangaball_search_endpoint() -> &'static str {
    "https://mangaball.net/api/v1/smart-search/search/"
}

/// Unified search result item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub source: String,
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub kind: Option<String>, // e.g. "manga_series", "donghua_series", "cosplay_post"
    /// Free-form snippet / synopsis if available
    pub snippet: Option<String>,
    /// Genre/category labels for filtering
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SearchResults {
    pub query: String,
    pub source: String,
    pub page: u32,
    pub total_results: usize,
    pub items: Vec<SearchResultItem>,
}

/// Parse a search results page from any of the supported sites.
pub fn parse_search_html(
    source: SearchSource,
    base_url: &str,
    html: &str,
) -> Vec<SearchResultItem> {
    match source {
        SearchSource::Cosplaytele => parse_cosplaytele_search(base_url, html),
        SearchSource::Anichin => parse_anichin_search(base_url, html),
        SearchSource::Mangaball => Vec::new(),
        SearchSource::Nhentai => {
            // The "html" body is actually JSON when called for nhentai
            match serde_json::from_str::<serde_json::Value>(html) {
                Ok(v) => parse_nhentai_search(&v),
                Err(_) => Vec::new(),
            }
        }
        SearchSource::Novelid => parse_novelid_search(base_url, html),
        SearchSource::Otakudesu => parse_otakudesu_search(base_url, html),
    }
}

/// Parse otakudesu.blog anime search HTML into unified search items.
pub fn parse_otakudesu_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    crate::adapters::otakudesu::OtakudesuAdapter::parse_search(base_url, html)
        .into_iter()
        .map(|hit| {
            let mut tags = hit.genres;
            if let Some(s) = hit.status {
                tags.push(s);
            }
            if let Some(r) = hit.rating {
                tags.push(format!("\u{2605} {}", r));
            }
            SearchResultItem {
                source: "otakudesu".to_string(),
                title: hit.title,
                url: hit.url,
                thumbnail: hit.thumbnail,
                kind: Some("anime_series".to_string()),
                snippet: None,
                tags,
            }
        })
        .collect()
}

/// Parse novelid.org search HTML into unified search items.
pub fn parse_novelid_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    crate::adapters::novelid::NovelidAdapter::parse_search_results(base_url, html)
        .into_iter()
        .map(|it| SearchResultItem {
            source: "novelid".to_string(),
            title: it.title,
            url: it.url,
            thumbnail: it.thumbnail,
            kind: Some("novel_series".to_string()),
            snippet: None,
            tags: it.tags,
        })
        .collect()
}

/// Extract the highest page number from a WordPress-style paginated listing.
///
/// Most of the HTML providers we scrape (Anichin/ts theme, Cosplaytele/Flatsome,
/// NovelID) render a numeric pager. We look for the common markers and return
/// the largest page number found.
///
/// We only return `Some(n)` when the page is a *numbered* pager (we saw at
/// least two distinct page numbers — e.g. `1 2 3 … 40`). A "next-only" pager
/// (just a `→ next` link pointing at `?page=2`) tells us nothing about the
/// real total, so we return `None` and let the caller fall back to a
/// "has next page?" heuristic. This avoids a misleading "Page 1 of 2".
pub fn parse_html_total_pages(html: &str) -> Option<u32> {
    let parser = HtmlParser::parse(html);
    let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();

    // WordPress numeric pagers + the common theme variants we hit. We target
    // the individual page *links* (anchors / current-page spans), never the
    // wrapping container — reading a container's combined text would splice
    // "1","2","3" into a bogus "123".
    let selectors = [
        ".pagination a",
        ".pagination span.current",
        ".page-numbers a",
        ".page-numbers span",
        "a.page-numbers",
        "span.page-numbers.current",
        "a.page-number",
        "span.page-number.current",
        ".page-nav a",
        ".nav-links a",
        ".hpage a",
        ".pagenavix a",
        ".pagenavix span",
        "nav.pagination a",
        ".wp-pagenavi a",
        ".wp-pagenavi span",
    ];
    for sel in selectors {
        for el in parser.select_all(sel) {
            // Skip non-leaf elements: if this node wraps other element nodes
            // (e.g. a <ul> pager or an <a> that only contains an <i> icon),
            // its combined text is not a single page number. Reading it would
            // concatenate digits ("1"+"2"+"3" -> "123") or yield icon noise.
            let has_child_element = el.children().any(|c| c.value().is_element());
            if !has_child_element {
                // 1) Numeric link text — only when the *entire* trimmed text
                //    is a single number (e.g. "12").
                let txt = el.text().collect::<Vec<_>>().join("");
                let trimmed = txt.trim();
                if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(n) = trimmed.parse::<u32>() {
                        if n > 0 && n < 1_000_000 {
                            seen.insert(n);
                        }
                    }
                }
            }
            // 2) `?page=N` / `/page/N/` embedded in the href (covers the
            //    "Last »" link that often has no numeric text).
            if let Some(href) = el.value().attr("href") {
                if let Some(n) = page_num_from_url(href) {
                    seen.insert(n);
                }
            }
        }
    }

    // Need at least two distinct page references to trust this as a real
    // numbered pager (one value usually means a lone "next" link).
    if seen.len() >= 2 {
        seen.iter().last().copied()
    } else {
        None
    }
}

/// Pull a page number out of a URL using the two common conventions:
/// `?page=N` / `&page=N` and `/page/N/`.
fn page_num_from_url(url: &str) -> Option<u32> {
    use once_cell::sync::Lazy;
    static QS_RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"[?&]page=(\d+)").unwrap());
    static PATH_RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"/page/(\d+)").unwrap());
    QS_RE
        .captures(url)
        .or_else(|| PATH_RE.captures(url))
        .and_then(|c| c[1].parse::<u32>().ok())
}

/// nhentai's JSON search/listing responses carry `num_pages` and `per_page`
/// at the top level. Returns `(num_pages, per_page)` when present.
pub fn parse_nhentai_pagination(json: &serde_json::Value) -> (Option<u32>, Option<u32>) {
    let num_pages = json
        .get("num_pages")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .filter(|n| *n > 0);
    let per_page = json
        .get("per_page")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .filter(|n| *n > 0);
    (num_pages, per_page)
}

/// Parse a JSON response from `/api/v2/search?query=...` into search items.
/// nhentai's response shape is:
/// `{ result: [ {id, media_id, english_title, japanese_title, thumbnail, num_pages, num_favorites, tag_ids, ...} ], num_pages, per_page, total }`
pub fn parse_nhentai_search(json: &serde_json::Value) -> Vec<SearchResultItem> {
    let mut items = Vec::new();
    let arr = match json.get("result").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return items,
    };
    for entry in arr {
        let id = match entry.get("id").and_then(|v| v.as_u64()) {
            Some(i) => i,
            None => continue,
        };
        let media_id = entry
            .get("media_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        // Flat title fields. Some entries have empty japanese_title.
        let title = entry
            .get("english_title")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                entry
                    .get("japanese_title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            // Fallback for the gallery-detail shape where the title is nested
            .or_else(|| {
                entry.get("title").and_then(|t| {
                    t.get("english")
                        .or_else(|| t.get("pretty"))
                        .or_else(|| t.get("japanese"))
                        .and_then(|s| s.as_str())
                })
            })
            .unwrap_or_default()
            .to_string();
        if title.is_empty() {
            continue;
        }

        // Thumbnail: search results expose `thumbnail` as the path (e.g.
        // "galleries/3956745/thumb.jpg.webp"). Fall back to the gallery-detail
        // shape where it's nested under `images.cover.t`.
        let thumbnail = entry
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|p| build_nhentai_path("t", media_id, p))
            .or_else(|| {
                entry
                    .get("images")
                    .and_then(|imgs| imgs.get("cover"))
                    .and_then(|c| c.get("t").and_then(|v| v.as_str()))
                    .map(|t| {
                        let ext = match t {
                            "j" => "jpg",
                            "p" => "png",
                            "g" => "gif",
                            "w" => "webp",
                            _ => "jpg",
                        };
                        build_nhentai_path(
                            "t",
                            media_id,
                            &format!("galleries/{}/cover.{}", media_id, ext),
                        )
                    })
            });

        // Tag list comes back as IDs in search; we don't have names without a
        // second lookup, so just record the page count and any structured tags.
        let mut tags = Vec::new();
        if let Some(arr) = entry.get("tags").and_then(|v| v.as_array()) {
            for t in arr {
                let kind = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                if kind == "language" || kind == "category" || kind == "parody" {
                    tags.push(name.to_string());
                }
            }
        }
        if let Some(n) = entry.get("num_pages").and_then(|v| v.as_u64()) {
            tags.push(format!("{} pages", n));
        }
        if let Some(n) = entry.get("num_favorites").and_then(|v| v.as_u64()) {
            tags.push(format!("{} favorites", n));
        }

        let url = format!("https://nhentai.net/g/{}/", id);
        items.push(SearchResultItem {
            source: "nhentai".to_string(),
            title,
            url,
            thumbnail,
            kind: Some("manga_series".to_string()),
            snippet: None,
            tags,
        });
    }
    items
}

/// Build a sharded nhentai CDN URL.
///   prefix = "i" -> i1..i4.nhentai.net (full-size pages)
///   prefix = "t" -> t1..t4.nhentai.net (thumbnails)
fn build_nhentai_path(prefix: &str, media_id: &str, path: &str) -> String {
    let shard = (media_id
        .chars()
        .last()
        .and_then(|c| c.to_digit(10))
        .unwrap_or(1)
        % 4)
        + 1;
    // Some paths are stored without a leading slash; normalise.
    let p = path.trim_start_matches('/');
    format!("https://{}{}.nhentai.net/{}", prefix, shard, p)
}

#[allow(dead_code)]
fn build_nhentai_cover(media_id: &str, ext: &str) -> String {
    build_nhentai_path(
        "t",
        media_id,
        &format!("galleries/{}/cover.{}", media_id, ext),
    )
}

fn parse_cosplaytele_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    // The Cosplaytele (Flatsome/WordPress) search page lists the *real* search
    // results first, then several recommendation carousels ("Video Cosplayer",
    // "Cosplay Nude", "Cosplay Ero", "Random post") rendered as
    // `<div class="slider" data-flickity-options=...>`. Those carousels are
    // NOT search matches, so we must cut them off before parsing — otherwise
    // unrelated cosplayers (Arty Huang, etc.) leak into results.
    //
    // We slice the HTML at the first recommendation marker and only parse the
    // portion above it.
    let cut = ["section-title-container", "data-flickity-options"]
        .iter()
        .filter_map(|m| html.find(m))
        .min()
        .unwrap_or(html.len());
    let main_html = &html[..cut];

    let parser = HtmlParser::parse(main_html);
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for el in parser.select_all(".col.post-item, article, .post-item") {
        // Title
        let title_el = el
            .select(&scraper::Selector::parse("h2 a, h3 a, h4 a, h5 a, .post-title a").unwrap())
            .next();
        let (title, url) = match title_el {
            Some(a) => (
                a.text().collect::<Vec<_>>().join("").trim().to_string(),
                a.value().attr("href").map(|s| resolve_url(base_url, s)),
            ),
            None => continue,
        };
        let url = match url {
            Some(u) => u,
            None => continue,
        };
        if title.is_empty() || !seen.insert(url.clone()) {
            continue;
        }

        // Thumbnail
        let thumbnail = el
            .select(&scraper::Selector::parse("img").unwrap())
            .next()
            .and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("data-lazy-src"))
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });

        // Snippet from cat-label (categories + cosplayer name)
        let snippet = el
            .select(&scraper::Selector::parse(".cat-label").unwrap())
            .next()
            .map(|n| n.text().collect::<Vec<_>>().join(" ").trim().to_string());

        items.push(SearchResultItem {
            source: "cosplaytele".to_string(),
            title,
            url,
            thumbnail,
            kind: Some("cosplay_post".to_string()),
            snippet,
            tags: Vec::new(),
        });
    }
    items
}

fn parse_anichin_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    let parser = HtmlParser::parse(html);
    let mut items = Vec::new();

    for el in parser.select_all(".listupd article.bs, .listupd article, article.bs") {
        // The single anchor wraps everything
        let anchor = match el
            .select(&scraper::Selector::parse("a[href]").unwrap())
            .next()
        {
            Some(a) => a,
            None => continue,
        };

        let href = match anchor.value().attr("href") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let url = resolve_url(base_url, href);
        if !url.contains("/seri/") {
            continue;
        }

        // Title: prefer anchor's `title` attribute (cleanest), then h2 inside .tt
        let title_from_attr = anchor.value().attr("title").map(|s| s.trim().to_string());
        let title_from_h2 = el
            .select(&scraper::Selector::parse(".tt h2, h2[itemprop='headline']").unwrap())
            .next()
            .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string());

        let title = title_from_attr.or(title_from_h2).unwrap_or_default();
        if title.is_empty() {
            continue;
        }

        let thumbnail = el
            .select(&scraper::Selector::parse("img").unwrap())
            .next()
            .and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });

        // Status / type / sub badges
        let mut tags = Vec::new();
        for sel in [".status", ".typez", ".sb", ".epx"] {
            for t in el.select(&scraper::Selector::parse(sel).unwrap()) {
                let txt = t.text().collect::<Vec<_>>().join("").trim().to_string();
                if !txt.is_empty() && !tags.contains(&txt) {
                    tags.push(txt);
                }
            }
        }

        items.push(SearchResultItem {
            source: "anichin".to_string(),
            title,
            url,
            thumbnail,
            kind: Some("donghua_series".to_string()),
            snippet: None,
            tags,
        });
    }
    items
}

#[allow(dead_code)]
fn parse_mangaku_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    let parser = HtmlParser::parse(html);
    let mut items = Vec::new();

    // Mangaku search results: each result is `<a class="mk-card" href="/komik/<slug>/">`
    for el in parser.select_all("a.mk-card") {
        let v = el.value();
        let href = match v.attr("href") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        if !href.contains("/komik/") {
            continue;
        }
        let url = resolve_url(base_url, href);

        // Title: try mk-card__title, else use anchor's title attribute
        let title_from_inner = el
            .select(&scraper::Selector::parse(".mk-card__title").unwrap())
            .next()
            .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string());
        let title = title_from_inner
            .or_else(|| v.attr("title").map(|s| s.to_string()))
            .or_else(|| {
                el.select(&scraper::Selector::parse("img").unwrap())
                    .next()
                    .and_then(|i| i.value().attr("alt").map(|s| s.to_string()))
            })
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        // Clean common Mangaku prefixes from titles like "Komik X Bahasa Indonesia"
        let title = title
            .trim_start_matches("Baca ")
            .trim_start_matches("Komik ")
            .trim_end_matches(" Bahasa Indonesia")
            .trim()
            .to_string();

        let thumbnail = el
            .select(&scraper::Selector::parse("img").unwrap())
            .next()
            .and_then(|img| {
                img.value()
                    .attr("data-src")
                    .or_else(|| img.value().attr("src"))
                    .map(|s| resolve_url(base_url, s))
            });

        // Type tag (Manga/Manhwa/Manhua)
        let tags = el
            .select(&scraper::Selector::parse(".mk-badge").unwrap())
            .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        items.push(SearchResultItem {
            source: "mangaku".to_string(),
            title,
            url,
            thumbnail,
            kind: Some("manga_series".to_string()),
            snippet: None,
            tags,
        });
    }
    items
}

#[allow(dead_code)]
fn parse_wp_search(base_url: &str, html: &str) -> Vec<SearchResultItem> {
    let parser = HtmlParser::parse(html);
    let mut items = Vec::new();

    for el in parser.select_all("article, .post, .entry") {
        let anchor = el
            .select(&scraper::Selector::parse("h2 a, h1 a, .entry-title a").unwrap())
            .next();
        let (title, url) = match anchor {
            Some(a) => (
                a.text().collect::<Vec<_>>().join("").trim().to_string(),
                a.value().attr("href").map(|s| resolve_url(base_url, s)),
            ),
            None => continue,
        };
        let url = match url {
            Some(u) => u,
            None => continue,
        };
        if title.is_empty() {
            continue;
        }
        let snippet = el
            .select(&scraper::Selector::parse(".entry-summary, .excerpt, p").unwrap())
            .next()
            .map(|n| {
                let txt = n.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if txt.len() > 300 {
                    txt.chars().take(300).collect::<String>() + "..."
                } else {
                    txt
                }
            });
        let thumbnail = el
            .select(&scraper::Selector::parse("img").unwrap())
            .next()
            .and_then(|img| img.value().attr("src").map(|s| resolve_url(base_url, s)));

        items.push(SearchResultItem {
            source: "wordpress".to_string(),
            title,
            url,
            thumbnail,
            kind: None,
            snippet,
            tags: Vec::new(),
        });
    }
    items
}

/// Convert a search result list from the API response of mangaball's smart search.
/// The response shape is: `{ code: 200, data: { manga: [...], authors: [...], tags: [...] } }`.
/// Each manga entry has fields: `title`, `img`, `url`, `views`, `followers`, `rating`, `status`.
pub fn parse_mangaball_search(json: &serde_json::Value) -> Vec<SearchResultItem> {
    let mut items = Vec::new();
    let mangas = json
        .pointer("/data/manga")
        .or_else(|| json.get("data"))
        .and_then(|v| v.as_array());
    let arr = match mangas {
        Some(a) => a,
        None => return items,
    };
    for entry in arr {
        let title = entry
            .get("title")
            .or_else(|| entry.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // mangaball gives multilingual titles separated by "/"; trim to first portion
        let title = title
            .split('/')
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or(title);

        // URL field is relative on mangaball ("/title-detail/...") — resolve it
        let raw_url = entry
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let url = match raw_url {
            Some(u) if u.starts_with("http") => u,
            Some(u) => format!("https://mangaball.net{}", u),
            None => continue,
        };

        if title.is_empty() {
            continue;
        }

        let thumbnail = entry
            .get("img")
            .or_else(|| entry.get("cover"))
            .or_else(|| entry.get("thumbnail"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let snippet = entry
            .get("description")
            .or_else(|| entry.get("synopsis"))
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 200 {
                    s.chars().take(200).collect::<String>() + "..."
                } else {
                    s.to_string()
                }
            });

        // Build tags from views, followers, status
        let mut tags: Vec<String> = Vec::new();
        // Status comes wrapped in HTML — extract just the text token
        if let Some(status_html) = entry.get("status").and_then(|v| v.as_str()) {
            let cleaned = strip_tags(status_html);
            if !cleaned.is_empty() {
                tags.push(cleaned);
            }
        }
        if let Some(views) = entry.get("views").and_then(|v| v.as_u64()) {
            tags.push(format!("👁 {}", format_count(views)));
        }
        if let Some(followers) = entry.get("followers").and_then(|v| v.as_u64()) {
            tags.push(format!("⭐ {}", format_count(followers)));
        }
        // Genre tags (when present in other API contexts)
        if let Some(genres) = entry.get("genres").and_then(|v| v.as_array()) {
            for g in genres {
                if let Some(name) = g.as_str() {
                    tags.push(name.to_string());
                } else if let Some(name) = g.get("name").and_then(|v| v.as_str()) {
                    tags.push(name.to_string());
                }
            }
        }

        items.push(SearchResultItem {
            source: "mangaball".to_string(),
            title,
            url,
            thumbnail,
            kind: Some("manga_series".to_string()),
            snippet,
            tags,
        });
    }
    items
}

/// Strip HTML tags (very lightweight) and collapse whitespace
fn strip_tags(s: &str) -> String {
    static TAG_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
    TAG_RE
        .replace_all(s, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format integer counts compactly (1.2k, 3.4m)
fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Convenience: re-export ContentModel kind name
#[allow(dead_code)]
pub fn content_kind(c: &ContentModel) -> &'static str {
    match c {
        ContentModel::WordPressPost(_) => "wordpress_post",
        ContentModel::MangaSeries(_) => "manga_series",
        ContentModel::MangaChapter(_) => "manga_chapter",
        ContentModel::DonghuaSeries(_) => "donghua_series",
        ContentModel::DonghuaEpisode(_) => "donghua_episode",
        ContentModel::AnimeSeries(_) => "anime_series",
        ContentModel::AnimeEpisode(_) => "anime_episode",
        ContentModel::CosplayPost(_) => "cosplay_post",
        ContentModel::NovelSeries(_) => "novel_series",
        ContentModel::NovelChapter(_) => "novel_chapter",
        ContentModel::Generic(_) => "generic",
        ContentModel::DeepPage(_) => "deep_page",
        ContentModel::JsonApi(_) => "json_api",
    }
}
