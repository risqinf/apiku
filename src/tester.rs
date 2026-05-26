//! API tester website (server-rendered with `maud`, HTMX-driven).
//!
//! This page is for *developers testing the API* — not for end users.
//! It provides:
//!
//!   - Live endpoint browser with parameter form
//!   - Pretty-printed JSON response viewer
//!   - Visual rendered preview (manga reader, donghua player, cosplay grid)
//!   - Multi-language code examples (cURL, JS, Python, PHP, Go, C++, Rust)
//!   - Full API reference with response envelope spec
//!
//! Single page, two CSS/JS files (compiled in via `include_str!`).

use crate::api::{api_endpoint_docs, ApiState};
use axum::extract::State;
use axum::response::Html;
use maud::{html, Markup, PreEscaped, DOCTYPE};

const TESTER_CSS: &str = include_str!("tester.css");
const TESTER_JS: &str = include_str!("tester.js");

/// Render the home (tester) page.
pub async fn home(State(state): State<ApiState>) -> Html<String> {
    let s = state.sysspec;
    let body = html! {
        header.tester-header {
            div.brand {
                strong.brand-name { "apiku" }
                span.version { "v" (env!("CARGO_PKG_VERSION")) }
            }
            div.system-badge {
                span.system-cell { "CPU: " strong { (s.cpu_cores) } }
                span.system-cell { "RAM: " strong { (s.total_mem_mib / 1024) } " GB" }
                span.system-cell { "Profile: " strong { (s.profile()) } }
            }
        }

        nav.tabs {
            button.tab.active data-tab="playground" { "Playground" }
            button.tab data-tab="reference" { "API Reference" }
            button.tab data-tab="quickstart" { "Quick Start" }
            button.tab data-tab="examples" { "Code Examples" }
            button.tab data-tab="security" { "Security" }
            button.tab data-tab="info" { "Server Info" }
        }

        section.tab-content.active id="tab-playground" { (playground()) }
        section.tab-content id="tab-reference" { (reference()) }
        section.tab-content id="tab-quickstart" { (quickstart()) }
        section.tab-content id="tab-examples" { (code_examples()) }
        section.tab-content id="tab-security" { (security_section()) }
        section.tab-content id="tab-info" {
            div.loading-info hx-get="/api/v1/info" hx-trigger="load" hx-target="this" hx-swap="innerHTML" {
                "Loading..."
            }
        }
    };

    let page = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "apiku - RESTful Scraping API" }
                meta name="description" content="apiku - REST API for Mangaball, Anichin, Cosplaytele, nhentai, and NovelID content";
                script src="https://unpkg.com/htmx.org@2.0.4" {}
                style { (PreEscaped(TESTER_CSS)) }
            }
            body {
                main.tester { (body) }
                script { (PreEscaped(TESTER_JS)) }
            }
        }
    };
    Html(page.into_string())
}

// ---------------------------------------------------------------------------
// Playground tab
// ---------------------------------------------------------------------------

fn playground() -> Markup {
    html! {
        div.layout {
            section.panel id="request-panel" {
                h2 { "Request" }

                div.field {
                    label for="endpoint-select" { "Endpoint" }
                    select id="endpoint-select" {
                        optgroup label="Discovery" {
                            option value="/api/v1/health" data-method="GET" { "GET /api/v1/health" }
                            option value="/api/v1/info" data-method="GET" { "GET /api/v1/info" }
                        }
                        optgroup label="Search" {
                            option value="/api/v1/search" data-method="GET" data-params="q,source,page" { "GET /api/v1/search" }
                        }
                        optgroup label="Browse (Home / Popular / Latest)" {
                            option value="/api/v1/browse/mangaball" data-method="GET" data-params="feed,page,size" { "GET /api/v1/browse/mangaball" }
                            option value="/api/v1/browse/anichin" data-method="GET" data-params="feed,page" { "GET /api/v1/browse/anichin" }
                            option value="/api/v1/browse/cosplaytele" data-method="GET" data-params="feed,page" { "GET /api/v1/browse/cosplaytele" }
                            option value="/api/v1/browse/nhentai" data-method="GET" data-params="feed,page" { "GET /api/v1/browse/nhentai" }
                            option value="/api/v1/browse/novelid" data-method="GET" data-params="feed,page" { "GET /api/v1/browse/novelid" }
                        }
                        optgroup label="Manga (Mangaball)" {
                            option value="/api/v1/manga/{id}" data-method="GET" data-params="id,page,size" { "GET /api/v1/manga/{id}" }
                            option value="/api/v1/manga/chapter/{id}" data-method="GET" data-params="id" { "GET /api/v1/manga/chapter/{id}" }
                        }
                        optgroup label="Donghua (Anichin)" {
                            option value="/api/v1/donghua/{id}" data-method="GET" data-params="id,page,size" { "GET /api/v1/donghua/{id}" }
                            option value="/api/v1/donghua/episode/{id}" data-method="GET" data-params="id" { "GET /api/v1/donghua/episode/{id}" }
                        }
                        optgroup label="Cosplay (Cosplaytele)" {
                            option value="/api/v1/cosplay/{id}" data-method="GET" data-params="id" { "GET /api/v1/cosplay/{id}" }
                        }
                        optgroup label="Novel (NovelID)" {
                            option value="/api/v1/novel/{id}" data-method="GET" data-params="id,page,size" { "GET /api/v1/novel/{id}" }
                            option value="/api/v1/novel/chapter/{id}" data-method="GET" data-params="id" { "GET /api/v1/novel/chapter/{id}" }
                        }
                        optgroup label="Doujin (nhentai)" {
                            option value="/api/v1/nhentai/{id}" data-method="GET" data-params="id" { "GET /api/v1/nhentai/{id}" }
                            option value="/api/v1/nhentai/chapter/{id}" data-method="GET" data-params="id" { "GET /api/v1/nhentai/chapter/{id}" }
                        }
                    }
                }

                div id="param-fields" {
                    (search_param_fields())
                }

                div.actions {
                    button.btn.primary type="button" id="send-btn" { "Send Request" }
                    button.btn.ghost type="button" id="copy-curl-btn" { "Copy as cURL" }
                }

                div id="last-url" .url-preview { }
            }

            section.panel id="response-panel" {
                div.response-tabs {
                    button.rtab.active data-rtab="json" { "JSON" }
                    button.rtab data-rtab="rendered" { "Rendered" }
                    button.rtab data-rtab="headers" { "Headers" }
                }

                div.rtab-content.active id="rtab-json" {
                    div.response-meta id="response-meta" {
                        span.status { "Idle" }
                    }
                    pre.response-output id="response-json" { "// Send a request to see the response here." }
                }
                div.rtab-content id="rtab-rendered" {
                    div id="rendered-view" {
                        div.placeholder { "Send a request to render the response visually." }
                    }
                }
                div.rtab-content id="rtab-headers" {
                    pre.response-output id="response-headers" { "" }
                }
            }
        }
    }
}

fn search_param_fields() -> Markup {
    html! {
        div.field {
            label for="p_q" { "Query (q)" }
            input type="text" id="p_q" name="q" placeholder="e.g. one piece, or 'Genshin Impact [full color]' for nhentai" value="one piece";
        }
        div.field {
            label for="p_source" { "Source" }
            select id="p_source" name="source" {
                option value="all" selected { "all (parallel)" }
                option value="manga" { "manga (Mangaball)" }
                option value="donghua" { "donghua (Anichin)" }
                option value="cosplay" { "cosplay (Cosplaytele)" }
                option value="nhentai" { "nhentai (doujin)" }
                option value="novel" { "novel (NovelID)" }
            }
        }
        div.field {
            label for="p_page" { "Page" }
            input type="number" id="p_page" name="page" min="1" value="1";
        }
    }
}

// ---------------------------------------------------------------------------
// API Reference tab
// ---------------------------------------------------------------------------

fn reference() -> Markup {
    html! {
        h2 { "API Reference" }
        p.muted {
            "Base URL: " code { "http://127.0.0.1:3000" } " (local)"
        }

        h3 { "Response Envelope" }
        p { "Every endpoint returns the same JSON envelope shape." }
        pre.code-block {
r#"// Success
{
  "status": 200,
  "ok": true,
  "data": { ... },
  "meta": {
    "took_ms": 123,
    "cached": false,
    "request_id": "1f8b2c4d-..."
  }
}

// Error
{
  "status": 404,
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "..."
  },
  "meta": {
    "took_ms": 5,
    "cached": false,
    "request_id": "..."
  }
}"# }

        h3 { "HTTP Status Codes" }
        table.endpoint-table {
            thead { tr { th { "Status" } th { "Meaning" } } }
            tbody {
                tr { td { code { "200 OK" } } td { "Request succeeded" } }
                tr { td { code { "400 Bad Request" } } td { "Malformed query, invalid opaque ID, query too long" } }
                tr { td { code { "403 Forbidden" } } td { "Bad image-proxy signature, host not on allowlist" } }
                tr { td { code { "404 Not Found" } } td { "Unknown route" } }
                tr { td { code { "502 Bad Gateway" } } td { "Upstream provider returned an error or unparseable content" } }
                tr { td { code { "503 Service Unavailable" } } td { "Engine unavailable (rare)" } }
            }
        }

        h3 { "Error Codes" }
        table.endpoint-table {
            thead { tr { th { "Code" } th { "When" } } }
            tbody {
                tr { td { code { "missing_query" } } td { "/api/v1/search called without `q`" } }
                tr { td { code { "query_too_long" } } td { "`q` longer than 200 chars" } }
                tr { td { code { "invalid_id" } } td { "Opaque ID is malformed or has bad signature" } }
                tr { td { code { "wrong_source" } } td { "ID belongs to a different provider than the endpoint" } }
                tr { td { code { "wrong_kind" } } td { "Scraped page doesn't match endpoint expectation" } }
                tr { td { code { "scrape_failed" } } td { "Upstream scrape returned no content" } }
                tr { td { code { "upstream_error" } } td { "Network / 5xx from upstream" } }
                tr { td { code { "bad_signature" } } td { "Image proxy signature failed verification" } }
                tr { td { code { "host_not_allowed" } } td { "Image URL host not on allowlist" } }
            }
        }

        h3 { "Endpoints" }
        table.endpoint-table {
            thead { tr { th { "Method" } th { "Path" } th { "Description" } } }
            tbody {
                @for ep in api_endpoint_docs() {
                    tr {
                        td { span class=(format!("method-tag {}", ep.method.to_lowercase())) { (ep.method) } }
                        td { code { (ep.path) } }
                        td { (ep.description) }
                    }
                }
            }
        }

        h3 { "Headers" }
        table.endpoint-table {
            thead { tr { th { "Header" } th { "Set By" } th { "Notes" } } }
            tbody {
                tr { td { code { "x-request-id" } } td { "Server (or echoed)" } td { "UUID per request, also returned in `meta.request_id`" } }
                tr { td { code { "cache-control" } } td { "Server" } td { "Image proxy responses set `public, max-age=86400, immutable`" } }
                tr { td { code { "access-control-allow-origin" } } td { "Server" } td { "CORS open by default for local development" } }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Quick Start tab
// ---------------------------------------------------------------------------

fn quickstart() -> Markup {
    html! {
        h2 { "Quick Start" }
        p.muted { "Two complete walkthroughs covering the most common API consumption flows." }

        h3 { "Browse a provider's home / popular / latest feed" }
        ol.steps {
            li {
                strong { "Pick a provider and a feed" } br;
                code { "GET /api/v1/browse/{provider}?feed={home|popular|latest|...}" }
            }
            li {
                "Each provider has a different default feed. Examples:"
                br;
                code { "GET /api/v1/browse/mangaball?feed=popular" } br;
                code { "GET /api/v1/browse/anichin?feed=latest" } br;
                code { "GET /api/v1/browse/cosplaytele?feed=home" } br;
                code { "GET /api/v1/browse/nhentai?feed=popular-today" } br;
                code { "GET /api/v1/browse/novelid?feed=romantis" }
            }
            li {
                "Items have the same shape as search results: each carries an opaque " code { "id" } " you can pass to a detail endpoint."
            }
            li {
                strong { "Paginate" } br;
                code { "&page=2" } " (and for mangaball: " code { "&size=30" } ")"
            }
        }

        h3 { "Read a manga" }
        ol.steps {
            li {
                strong { "Search" } br;
                code { "GET /api/v1/search?q=dark+mortal&source=manga" }
            }
            li {
                strong { "Pick an item" } br;
                "Each item in `data.items[]` has an opaque " code { "id" } " field."
            }
            li {
                strong { "Get series detail" } br;
                code { "GET /api/v1/manga/{id}" } br;
                "Returns " code { "data.chapters[]" } ", each with their own opaque IDs."
            }
            li {
                strong { "Read a chapter" } br;
                code { "GET /api/v1/manga/chapter/{chapter_id}" } br;
                "Returns " code { "data.pages[]" } " with proxied image URLs."
            }
            li {
                strong { "Render pages" } br;
                "Display each page using " code { "<img src=\"http://127.0.0.1:3000{page.url}\">" }
            }
        }

        h3 { "Watch a donghua episode" }
        ol.steps {
            li {
                strong { "Search" } br;
                code { "GET /api/v1/search?q=peerless&source=donghua" }
            }
            li {
                strong { "Get series" } br;
                code { "GET /api/v1/donghua/{id}" } br;
                "Returns the full episode list."
            }
            li {
                strong { "Get episode" } br;
                code { "GET /api/v1/donghua/episode/{episode_id}" } br;
                "Returns video servers and download mirror groups by quality."
            }
            li {
                strong { "Player" } br;
                "Embed " code { "servers[i].embed_url" } " in an " code { "<iframe>" } "."
            }
        }

        h3 { "Browse a cosplay post" }
        ol.steps {
            li { code { "GET /api/v1/search?q=raiden+shogun&source=cosplay" } }
            li { code { "GET /api/v1/cosplay/{id}" } " - returns gallery + downloads + unzip password" }
            li { "Render " code { "data.images[]" } " with " code { "<img src=\"...\">" } " (already proxied)" }
        }

        h3 { "Read an nhentai gallery" }
        ol.steps {
            li {
                strong { "Browse popular" } br;
                code { "GET /api/v1/browse/nhentai?feed=popular-today" }
                br;
                "Feeds: " code { "popular-today" } " | " code { "popular-week" } " | " code { "popular" } " (all-time) | " code { "home" } " (recent)"
            }
            li {
                strong { "Search with tag filter" } br;
                code { r#"GET /api/v1/search?q=Genshin+Impact+%5Bfull+color%5D&source=nhentai"# }
                br;
                "Use " code { "[tag]" } " syntax inside `q` to filter by tag, e.g. " code { "Genshin Impact [full color]" } "."
            }
            li {
                strong { "Get gallery" } br;
                code { "GET /api/v1/nhentai/{id}" } br;
                "Returns title, cover, tags and one chapter that holds every page."
            }
            li {
                strong { "Read pages" } br;
                code { "GET /api/v1/nhentai/chapter/{id}" } br;
                "Returns " code { "data.pages[]" } " with proxied image URLs (browser-spoofed Referer + User-Agent)."
            }
        }

        h3 { "Read a novel (NovelID)" }
        ol.steps {
            li {
                strong { "Search" } br;
                code { "GET /api/v1/search?q=Martial+Universe&source=novel" }
                br;
                "Returns NovelID matches in Indonesian. Each result has an opaque " code { "id" } "."
            }
            li {
                strong { "Get series" } br;
                code { "GET /api/v1/novel/{id}" } br;
                "Returns " code { "data.title" } ", " code { "data.author" } ", " code { "data.synopsis" } ", " code { "data.cover" } " (proxied), and " code { "data.chapters[]" } " (each with its own opaque ID)."
            }
            li {
                strong { "Read a chapter" } br;
                code { "GET /api/v1/novel/chapter/{id}" } br;
                "Returns " code { "data.body" } " (plain text, paragraphs separated by blank lines), " code { "data.body_html" } " (sanitised HTML), plus " code { "prev_id" } "/" code { "next_id" } " for pagination."
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Code Examples tab
// ---------------------------------------------------------------------------

fn code_examples() -> Markup {
    let example_url = "http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga";

    html! {
        h2 { "Code Examples" }
        p.muted { "All examples make the same request: search for `one piece` on Mangaball." }

        nav.lang-tabs {
            @for (i, (id, label)) in [
                ("curl", "cURL"),
                ("js", "JavaScript"),
                ("ts", "TypeScript"),
                ("python", "Python"),
                ("php", "PHP"),
                ("go", "Go"),
                ("cpp", "C++"),
                ("rust", "Rust"),
            ].iter().enumerate() {
                button class=(if i == 0 { "lang-tab active" } else { "lang-tab" }) data-lang=(id) { (label) }
            }
        }

        div.lang-content.active id="lang-curl" {
            (code_block("bash", &format!(
r#"# Plain GET request
curl '{example_url}'

# Pretty-print with jq
curl '{example_url}' | jq .

# Show timing + headers
curl -i '{example_url}' -w '\n%{{time_total}}s\n'"#,
                example_url = example_url
            )))
        }

        div.lang-content id="lang-js" {
            (code_block("javascript",
r#"// Vanilla fetch (no dependencies)
const res = await fetch('http://127.0.0.1:3000/api/v1/search?q=one piece&source=manga');
const json = await res.json();

if (!json.ok) {
  console.error('Error:', json.error.code, json.error.message);
  return;
}

console.log(`Found ${json.data.total} results in ${json.meta.took_ms}ms`);
for (const item of json.data.items) {
  console.log(`- [${item.source}] ${item.title}`);
  console.log(`  id: ${item.id}`);
}

// Then fetch a specific manga:
const series = await fetch(`http://127.0.0.1:3000/api/v1/manga/${json.data.items[0].id}`)
  .then(r => r.json());
console.log('Chapters:', series.data.chapter_count);"#))
        }

        div.lang-content id="lang-ts" {
            (code_block("typescript",
r#"interface ApiEnvelope<T> {
  status: number;
  ok: boolean;
  data: T;
  meta: { took_ms: number; cached: boolean; request_id: string };
}
interface ApiError {
  status: number;
  ok: false;
  error: { code: string; message: string };
  meta: { took_ms: number; request_id: string };
}
type Response<T> = ApiEnvelope<T> | ApiError;

interface SearchData {
  query: string;
  source: string;
  page: number;
  total: number;
  items: Array<{
    id: string;
    source: 'mangaball' | 'anichin' | 'cosplaytele';
    kind: 'manga' | 'donghua' | 'cosplay';
    title: string;
    thumbnail?: string;
    snippet?: string;
    tags: string[];
  }>;
}

const url = 'http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga';
const res = await fetch(url);
const json: Response<SearchData> = await res.json();

if (!json.ok) {
  throw new Error(`${json.error.code}: ${json.error.message}`);
}
console.log(`${json.data.total} results in ${json.meta.took_ms}ms`);"#))
        }

        div.lang-content id="lang-python" {
            (code_block("python",
r#"# Requires: pip install requests
import requests

BASE = 'http://127.0.0.1:3000'

def search(query: str, source: str = 'all', page: int = 1):
    r = requests.get(f'{BASE}/api/v1/search', params={
        'q': query, 'source': source, 'page': page
    })
    r.raise_for_status()
    body = r.json()
    if not body.get('ok'):
        raise RuntimeError(f"{body['error']['code']}: {body['error']['message']}")
    return body['data']

def manga_series(opaque_id: str):
    r = requests.get(f'{BASE}/api/v1/manga/{opaque_id}')
    return r.json()['data']

def manga_chapter(opaque_id: str):
    r = requests.get(f'{BASE}/api/v1/manga/chapter/{opaque_id}')
    return r.json()['data']

# Workflow: search -> series -> chapter
results = search('one piece', source='manga')
print(f"Found {results['total']} items")

if results['items']:
    series = manga_series(results['items'][0]['id'])
    print(f"Title: {series['title']}, chapters: {series['chapter_count']}")

    if series['chapters']:
        chapter = manga_chapter(series['chapters'][0]['id'])
        print(f"Pages: {chapter['page_count']}")
        for page in chapter['pages'][:3]:
            print(f"  page {page['index']}: {BASE}{page['url']}")"#))
        }

        div.lang-content id="lang-php" {
            (code_block("php",
r#"<?php
// PHP 7.4+ with curl extension
const BASE = 'http://127.0.0.1:3000';

function api_get(string $path): array {
    $ch = curl_init(BASE . $path);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    curl_setopt($ch, CURLOPT_HTTPHEADER, ['Accept: application/json']);
    $body = curl_exec($ch);
    $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
    curl_close($ch);

    $json = json_decode($body, true);
    if (!$json['ok']) {
        throw new RuntimeException(
            "API error {$json['error']['code']}: {$json['error']['message']}"
        );
    }
    return $json['data'];
}

// Workflow
$search = api_get('/api/v1/search?' . http_build_query([
    'q' => 'one piece', 'source' => 'manga',
]));
echo "Found {$search['total']} results\n";

$first = $search['items'][0];
$series = api_get('/api/v1/manga/' . urlencode($first['id']));
echo "Title: {$series['title']}\n";
echo "Chapters: {$series['chapter_count']}\n";

$chapter = api_get('/api/v1/manga/chapter/' . urlencode($series['chapters'][0]['id']));
echo "Pages in first chapter: {$chapter['page_count']}\n";"#))
        }

        div.lang-content id="lang-go" {
            (code_block("go",
r#"package main

import (
    "encoding/json"
    "fmt"
    "io"
    "net/http"
    "net/url"
)

const Base = "http://127.0.0.1:3000"

type Envelope[T any] struct {
    Status int  `json:"status"`
    Ok     bool `json:"ok"`
    Data   T    `json:"data,omitempty"`
    Error  *struct {
        Code    string `json:"code"`
        Message string `json:"message"`
    } `json:"error,omitempty"`
    Meta struct {
        TookMs    int    `json:"took_ms"`
        Cached    bool   `json:"cached"`
        RequestID string `json:"request_id"`
    } `json:"meta"`
}

type SearchItem struct {
    ID, Source, Kind, Title string
    Thumbnail, Snippet      *string
    Tags                    []string
}
type SearchData struct {
    Query, Source string
    Page, Total   int
    Items         []SearchItem
}

func apiGet[T any](path string) (*Envelope[T], error) {
    resp, err := http.Get(Base + path)
    if err != nil { return nil, err }
    defer resp.Body.Close()
    body, _ := io.ReadAll(resp.Body)
    var env Envelope[T]
    if err := json.Unmarshal(body, &env); err != nil {
        return nil, err
    }
    if !env.Ok {
        return nil, fmt.Errorf("%s: %s", env.Error.Code, env.Error.Message)
    }
    return &env, nil
}

func main() {
    qs := url.Values{
        "q": {"one piece"}, "source": {"manga"},
    }
    res, err := apiGet[SearchData]("/api/v1/search?" + qs.Encode())
    if err != nil { panic(err) }
    fmt.Printf("Found %d (took %dms)\n", res.Data.Total, res.Meta.TookMs)
    for _, it := range res.Data.Items {
        fmt.Printf(" - %s [%s]\n", it.Title, it.Source)
    }
}"#))
        }

        div.lang-content id="lang-cpp" {
            (code_block("cpp",
r#"// C++17, requires libcurl + nlohmann::json
#include <iostream>
#include <string>
#include <stdexcept>
#include <curl/curl.h>
#include <nlohmann/json.hpp>

using json = nlohmann::json;

static size_t write_cb(void* p, size_t s, size_t n, std::string* out) {
    out->append((char*)p, s * n);
    return s * n;
}

json api_get(const std::string& path) {
    CURL* curl = curl_easy_init();
    std::string body;
    curl_easy_setopt(curl, CURLOPT_URL, ("http://127.0.0.1:3000" + path).c_str());
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_cb);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &body);
    CURLcode rc = curl_easy_perform(curl);
    curl_easy_cleanup(curl);
    if (rc != CURLE_OK) throw std::runtime_error(curl_easy_strerror(rc));

    auto j = json::parse(body);
    if (!j["ok"].get<bool>()) {
        throw std::runtime_error(
            j["error"]["code"].get<std::string>() + ": " +
            j["error"]["message"].get<std::string>());
    }
    return j["data"];
}

int main() {
    auto data = api_get("/api/v1/search?q=one+piece&source=manga");
    std::cout << "Total: " << data["total"] << "\n";
    for (auto& it : data["items"]) {
        std::cout << " - " << it["title"].get<std::string>() << "\n";
    }
}"#))
        }

        div.lang-content id="lang-rust" {
            (code_block("rust",
r#"// Cargo.toml: reqwest = { version = "0.12", features = ["json"] }
//             serde = { version = "1", features = ["derive"] }
//             tokio = { version = "1", features = ["full"] }
use serde::Deserialize;

const BASE: &str = "http://127.0.0.1:3000";

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    meta: Meta,
}
#[derive(Debug, Deserialize)]
struct ApiError { code: String, message: String }
#[derive(Debug, Deserialize)]
struct Meta { took_ms: u64, cached: bool, request_id: String }

#[derive(Debug, Deserialize)]
struct SearchData {
    total: usize,
    items: Vec<SearchItem>,
}
#[derive(Debug, Deserialize)]
struct SearchItem { id: String, source: String, title: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp: Envelope<SearchData> = reqwest::get(
        format!("{BASE}/api/v1/search?q=one+piece&source=manga")
    ).await?.json().await?;

    if !resp.ok {
        let e = resp.error.unwrap();
        return Err(format!("{}: {}", e.code, e.message).into());
    }

    let data = resp.data.unwrap();
    println!("Total: {} ({}ms)", data.total, resp.meta.took_ms);
    for it in &data.items {
        println!("  - {} [{}]", it.title, it.source);
    }
    Ok(())
}"#))
        }
    }
}

fn code_block(lang: &str, code: &str) -> Markup {
    html! {
        div.code-wrap {
            div.code-header {
                span.code-lang { (lang) }
                button.copy-code-btn type="button" { "Copy" }
            }
            pre.code-block { code class=(format!("lang-{}", lang)) { (code) } }
        }
    }
}

// ---------------------------------------------------------------------------
// Security tab
// ---------------------------------------------------------------------------

fn security_section() -> Markup {
    html! {
        h2 { "Security" }
        p.muted { "How URLs and images are protected from disclosure and tampering." }

        h3 { "Opaque IDs" }
        p {
            "All resource IDs returned by the API are opaque, HMAC-SHA256 signed tokens. "
            "They never reveal the upstream URL at a glance, and any tampering is detected "
            "via constant-time signature verification."
        }
        pre.code-block {
r#"<source 2 chars><kind 1 char><nonce 3 chars>.<base64url-payload>.<base64url-mac-16-bytes>

Example:
mbsabc.aHR0cHM6Ly9tYW5nYWJhbGwubmV0L3RpdGxlLWRldGFpbC8.J9k1Nz5pQq3v7L2HjT7M5w"# }
        ul {
            li { "128-bit MAC truncated from HMAC-SHA256" }
            li { "Constant-time comparison prevents timing attacks" }
            li { "3-character nonce prevents identical IDs for same URL" }
            li { "Server secret rotated on every restart unless " code { "APIKU_SECRET" } " env is set" }
        }

        h3 { "Image Proxy" }
        p {
            "Every image URL in API responses is rewritten to point at the local proxy: "
            code { "/img?p={base64url-encoded-url}&s={hmac-signature}" }
        }
        ul {
            li { strong { "Layer 1 - HMAC-SHA256 signature." } " Signature is verified server-side per request, in constant time." }
            li { strong { "Layer 2 - Host allowlist." } " Even with a forged signature the proxy will only fetch from the allow-listed CDN hosts." }
            li { strong { "Layer 3 - Referer spoofing." } " Outgoing requests carry the source domain as the Referer to bypass hotlink protection." }
            li { strong { "Cache-Control." } " Responses are tagged " code { "max-age=86400, immutable" } " so browsers cache aggressively." }
        }

        h3 { "Allow-listed Image Hosts" }
        ul {
            li { code { "*.poke-black-and-white.net" } ", " code { "*.red-and-blue.net" } " (Mangaball)" }
            li { code { "*.pokemon-gold-silver.net" } ", " code { "*.pokemon-ruby-sapphire.net" } " (Mangaball)" }
            li { code { "anichin.cafe" } ", " code { "anichin.care" } ", " code { "anichin.cloud" } }
            li { code { "i0.wp.com" } "..." code { "i3.wp.com" } " (Jetpack CDN used by Anichin)" }
            li { code { "cosplaytele.com" } ", " code { "*.cosplaytele.com" } }
        }

        h3 { "Rate Limiting" }
        ul {
            li { "Per-domain delay (default 400ms in server mode) on outbound requests" }
            li { "Single-flight cache: 10 simultaneous requests for the same URL collapse into one upstream fetch" }
            li { "Scrape result TTL: 10 minutes / Search TTL: 5 minutes" }
        }

        h3 { "Hardening Recommendations" }
        ul {
            li { "Set " code { "APIKU_SECRET" } " env var to a long random string for stable IDs across restarts." }
            li { "Run behind a reverse proxy (nginx, caddy) for TLS, IP rate limiting, and access logging." }
            li { "Restrict " code { "Access-Control-Allow-Origin" } " in production (currently open for local dev)." }
            li { "Optionally add HTTP basic auth or API keys at the reverse proxy layer if exposing publicly." }
        }
    }
}
