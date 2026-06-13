use crate::adapters::SiteAdapter;
use crate::error::Result;
use crate::models::{ChapterInfo, ContentModel, MangaChapter, MangaSeries, PageImage};
use crate::parser::{resolve_url, HtmlParser};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static CHAPTER_RE: Lazy<Regex> = Lazy::new(|| {
    // Match "Chapter 1182" but not "Chapter 118210 Mei 2026" — require word boundary
    // or end of input after the chapter number (not followed by another digit)
    Regex::new(r"(?i)(?:chapter|ch|chap)[\s._-]*(\d+(?:\.\d+)?)(?:\D|$)").unwrap()
});

/// Manga/Manhwa site adapter
pub struct MangaAdapter {
    /// Known manga site domains
    known_domains: Vec<&'static str>,
}

impl MangaAdapter {
    pub fn new() -> Self {
        Self {
            known_domains: vec![
                "mangadex",
                "manganato",
                "mangakakalot",
                "mangapark",
                "mangahere",
                "mangafox",
                "mangatown",
                "readmanga",
                "kissmanga",
                "asura",
                "asuracomic",
                "reaperscans",
                "flamescans",
                "luminousscans",
                "manhwa",
                "webtoon",
                "toonily",
                "manhuaplus",
                "komik",
                "komiku",
                "mangaku",
                "rawkuma",
                "westmanga",
                "mangaball",
                "mangabuddy",
                "manhuafast",
                "manhuaus",
                "kissmangas",
                "shinigami",
                "shinmanga",
            ],
        }
    }

    /// Detect if the page is a chapter reader page (vs series detail page)
    fn is_chapter_page(&self, html: &str, url: &str) -> bool {
        // URL pattern is the strongest signal
        if CHAPTER_RE.is_match(url) {
            return true;
        }
        let parser = HtmlParser::parse(html);
        // Chapter pages typically have many sequential images in a reader area
        let images = parser.select_all(
            ".reading-content img, .chapter-content img, #readerarea img, .container-chapter-reader img, .mk-reader img, .mk-reader__page img, .reader-area img, #readerarea > img"
        );
        images.len() > 3
    }

    fn extract_series(&self, url: &str, html: &str) -> MangaSeries {
        let parser = HtmlParser::parse(html);

        // Title: try many common selectors across manga themes
        let title = first_some(&[
            // Mangaku / mk theme
            parser.text(".mk-series__title"),
            // Madara theme (most popular WordPress manga theme)
            parser.text(".post-title h1"),
            // Manganato
            parser.text(".story-info-right h1"),
            parser.text(".panel-story-info .story-info-right h1"),
            // Asura / Reaper
            parser.text(".infox h1"),
            parser.text("h1.entry-title"),
            // Generic fallbacks
            parser.text(".series-title"),
            parser.text(".manga-title"),
            parser.text("article h1"),
            parser.text("h1"),
        ])
        .map(|s| {
            // Strip "Komik" prefix common on Indonesian sites
            s.trim_start_matches("Komik ").trim().to_string()
        })
        .or_else(|| {
            parser
                .attr("meta[property='og:title']", "content")
                .map(|s| s.trim_start_matches("Komik ").trim().to_string())
        });

        // Author: try multiple selectors
        let author = first_some(&[
            parser.text(".mk-series__author"),
            parser.text(".author-content a"),
            parser.text(".manga-authors a"),
            parser.text(".info-author a"),
            extract_table_field(&parser, &["author", "pengarang", "penulis"]),
        ])
        .map(clean_author);

        // Artist
        let artist = first_some(&[
            parser.text(".artist-content a"),
            parser.text(".manga-artists a"),
            extract_table_field(&parser, &["artist", "ilustrator"]),
        ]);

        // Genres
        let mut genres = parser.texts(".mk-series__genres a");
        if genres.is_empty() {
            genres = parser.texts(".genres-content a");
        }
        if genres.is_empty() {
            genres = parser.texts(".manga-info-text a[href*='genre']");
        }
        if genres.is_empty() {
            genres = parser.texts(".tags-content a");
        }
        if genres.is_empty() {
            genres = parser.texts("a[rel='tag']");
        }
        if genres.is_empty() {
            genres = parser.texts(".mgen a");
        }
        if genres.is_empty() {
            genres = parser.texts(".seriestugenre a");
        }

        // Synopsis
        let synopsis = first_some(&[
            parser.text(".mk-series__lead"),
            parser.text(".description-summary .summary__content p"),
            parser.text(".manga-description"),
            parser.text(".summary_content .post-content_item p"),
            parser.text("#noidungm"),
            parser.text(".panel-story-info-description"),
            parser.text(".sinopsis"),
            parser.text("article .entry-content p"),
            parser.attr("meta[property='og:description']", "content"),
            parser.attr("meta[name='description']", "content"),
        ]);

        // Cover image
        let cover_image = first_some(&[
            parser.attr(".mk-series__cover img", "src"),
            parser.attr(".mk-series__cover img", "data-src"),
            parser.attr(".summary_image img", "data-src"),
            parser.attr(".summary_image img", "src"),
            parser.attr(".manga-info-pic img", "src"),
            parser.attr(".info-image img", "src"),
            parser.attr(".thumbook img", "src"),
            parser.attr(".thumb img", "src"),
            parser.attr("meta[property='og:image']", "content"),
        ])
        .map(|u| resolve_url(url, &u));

        // Chapters
        let chapters = self.extract_chapters(&parser, url);

        MangaSeries {
            title,
            author,
            artist,
            genres,
            synopsis,
            cover_image,
            chapters,
            url: url.to_string(),
        }
    }

    fn extract_chapters(&self, parser: &HtmlParser, base_url: &str) -> Vec<ChapterInfo> {
        let mut chapters = Vec::new();

        // Try common chapter list selectors
        let chapter_elements = parser.select_all(
            ".mk-chapter-list__item a, \
             .wp-manga-chapter a, \
             .chapter-list a, \
             .row-content-chapter a, \
             ul.version-chap a, \
             #chapterlist a, \
             #chapterlist li a, \
             .eph-num a, \
             .clstyle li a",
        );

        for (idx, el) in chapter_elements.iter().enumerate() {
            let href = el.value().attr("href").unwrap_or("").to_string();

            // Try to get just the chapter name span (ignoring dates etc.)
            let name_text = el
                .select(
                    &scraper::Selector::parse(
                        ".mk-chapter-list__name, .chapternum, .chapter-name, .num-chapter",
                    )
                    .unwrap(),
                )
                .next()
                .map(|n| n.text().collect::<Vec<_>>().join("").trim().to_string());

            // Fall back to full link text
            let full_text = el.text().collect::<Vec<_>>().join("").trim().to_string();
            let text = name_text.unwrap_or_else(|| full_text.clone());

            // Skip if no href or self link
            if href.is_empty() || href == "#" {
                continue;
            }

            let chapter_url = resolve_url(base_url, &href);

            // Try to extract chapter number from URL first (most reliable)
            let number = if let Some(caps) = CHAPTER_RE.captures(&chapter_url) {
                caps[1].parse::<f64>().unwrap_or(idx as f64 + 1.0)
            } else if let Some(caps) = CHAPTER_RE.captures(&text) {
                caps[1].parse::<f64>().unwrap_or(idx as f64 + 1.0)
            } else {
                idx as f64 + 1.0
            };

            // Extract clean title
            let cleaned = text.split('\n').next().unwrap_or("").trim();
            let title = if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            };

            chapters.push(ChapterInfo {
                number,
                title,
                url: chapter_url,
                translations: Vec::new(),
            });
        }

        // Deduplicate by URL keeping first occurrence
        let mut seen = std::collections::HashSet::new();
        chapters.retain(|c| seen.insert(c.url.clone()));

        // Sort by chapter number ascending
        chapters.sort_by(|a, b| {
            a.number
                .partial_cmp(&b.number)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        chapters
    }

    fn extract_chapter_pages(&self, url: &str, html: &str) -> MangaChapter {
        let parser = HtmlParser::parse(html);

        // Extract series title from breadcrumb or header
        let series_title = first_some(&[
            parser.text(".mk-chapter__series"),
            parser.text(".breadcrumb li:nth-child(2) a"),
            parser.text("ol.breadcrumb a:nth-child(2)"),
            parser.text(".manga-title"),
            parser.text(".allc a"),
            parser.text(".titlemovie"),
        ]);

        // Extract chapter number
        let chapter_number = CHAPTER_RE
            .captures(url)
            .and_then(|c| c[1].parse::<f64>().ok())
            .or_else(|| {
                parser.text("h1").and_then(|t| {
                    CHAPTER_RE
                        .captures(&t)
                        .and_then(|c| c[1].parse::<f64>().ok())
                })
            })
            .unwrap_or(1.0);

        // Extract page images using lazy-load aware extraction
        let mut image_urls = parser.image_urls(
            ".mk-reader img, \
             .mk-reader__page img, \
             .reading-content img, \
             .chapter-content img, \
             #readerarea img, \
             .container-chapter-reader img, \
             .page-break img, \
             #anime_body_main img, \
             .reader-area img",
        );

        // Filter out covers/thumbnails of related manga (typically smaller images
        // referenced from cover.* or thumbnail keywords)
        image_urls.retain(|u| {
            let lower = u.to_lowercase();
            !lower.contains("cover.")
                && !lower.contains("thumbnail")
                && !lower.contains("?w=300")
                && !lower.contains("logo")
        });

        let pages: Vec<PageImage> = image_urls
            .into_iter()
            .enumerate()
            .map(|(idx, img_url)| PageImage {
                index: idx + 1,
                url: resolve_url(url, &img_url),
            })
            .collect();

        MangaChapter {
            series_title,
            chapter_number,
            pages,
            url: url.to_string(),
        }
    }
}

/// Pick the first Some value from a slice of Option<String>
fn first_some(opts: &[Option<String>]) -> Option<String> {
    opts.iter()
        .find_map(|o| o.clone().filter(|s| !s.trim().is_empty()))
}

/// Clean common author/artist prefixes that appear in scraped text:
/// "oleh Eiichiro Oda"  -> "Eiichiro Oda" (Indonesian "by")
/// "olehEiichiro Oda"  -> "Eiichiro Oda" (no space when text is concatenated from spans)
/// "by Eiichiro Oda"    -> "Eiichiro Oda"
fn clean_author(s: String) -> String {
    let trimmed = s.trim();
    let lower = trimmed.to_lowercase();
    // Try with-space prefixes first, then without-space (concatenated spans)
    for prefix in [
        "oleh ",
        "by ",
        "author: ",
        "penulis: ",
        "pengarang: ",
        "oleh",
        "by",
    ] {
        if lower.starts_with(prefix) {
            // Slice the original (preserving casing) by matching prefix length
            let stripped = &trimmed[prefix.len()..];
            return stripped.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Extract a value from a key/value list table commonly used on info pages.
/// Tries patterns like: "Author: <value>" in rows, dl/dt-dd, or table cells.
fn extract_table_field(parser: &HtmlParser, keywords: &[&str]) -> Option<String> {
    // Pattern: <td>Author</td><td>Value</td>
    for el in parser.select_all("table tr") {
        let text = el.text().collect::<Vec<_>>().join(" ").to_lowercase();
        for kw in keywords {
            if text.contains(kw) {
                // Get last cell text
                if let Some(td) = el
                    .select(&scraper::Selector::parse("td:last-child").unwrap())
                    .next()
                {
                    let val = td.text().collect::<Vec<_>>().join(" ").trim().to_string();
                    if !val.is_empty() {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

#[async_trait]
impl SiteAdapter for MangaAdapter {
    fn name(&self) -> &str {
        "manga"
    }

    fn matches(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        self.known_domains.iter().any(|d| lower.contains(d))
            || lower.contains("manga")
            || lower.contains("manhwa")
            || lower.contains("manhua")
            || lower.contains("komik")
            || lower.contains("comic")
    }

    fn headers(&self) -> Option<HashMap<String, String>> {
        // Some manga sites need specific headers
        let mut headers = HashMap::new();
        headers.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                .to_string(),
        );
        Some(headers)
    }

    async fn extract(&self, url: &str, html: &str) -> Result<Vec<ContentModel>> {
        if self.is_chapter_page(html, url) {
            let chapter = self.extract_chapter_pages(url, html);
            Ok(vec![ContentModel::MangaChapter(chapter)])
        } else {
            let series = self.extract_series(url, html);
            Ok(vec![ContentModel::MangaSeries(series)])
        }
    }
}
