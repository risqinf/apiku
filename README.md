# apiku

> RESTful scraping API for **Mangaball**, **Anichin**, **Cosplaytele**, **nhentai**, and **NovelID** — one HTTP service that other developers can build manga readers, donghua players, cosplay galleries, doujinshi browsers, and novel readers against, without ever seeing the upstream URLs.

- **Repo:** <https://github.com/risqinf/apiku>
- **Author:** risqinf
- **License:** MIT
- **Version:** see `Cargo.toml`

---

## Highlights

- **One API, five providers.** Manga, donghua, cosplay archives, doujinshi catalogues, and Indonesian novels behind a uniform JSON envelope.
- **Opaque IDs.** Resource IDs are HMAC-SHA256-signed tokens. Consumers never see upstream URLs.
- **Image proxy.** Every cover, page and thumbnail is rewritten to a signed local proxy. Source CDNs stay hidden.
- **Browser fingerprint rotation.** Outbound requests pick a coherent identity (Windows/Chrome, macOS/Safari, Android/Chrome, iPhone/Safari, Linux/Firefox, ...) per upstream URL — coupled with proper Sec-CH-UA, Sec-Fetch-* and Referer spoofing so origin checks and hotlink protection see a real browser visiting the source site.
- **Adaptive runtime.** CPU and RAM are detected at startup; tokio threads, HTTP concurrency, and cache sizes are tuned automatically.
- **Single-flight cache.** Concurrent requests for the same URL collapse into one upstream fetch.
- **Browse + search + detail + paged chapter list** for every provider.
- **Built-in tester website.** Live request playground, multi-language code examples, full reference, security notes — at `/`.

---

## Table of contents

1. [Quick start](#quick-start)
2. [CLI reference](#cli-reference)
3. [HTTP API at a glance](#http-api-at-a-glance)
4. [Browse feeds](#browse-feeds)
5. [Search](#search)
6. [Series and chapter pagination](#series-and-chapter-pagination)
7. [Response envelope](#response-envelope)
8. [Sample payloads](#sample-payloads)
9. [Status codes and error codes](#status-codes-and-error-codes)
10. [Opaque ID format](#opaque-id-format)
11. [Image proxy](#image-proxy)
12. [Browser fingerprint rotation](#browser-fingerprint-rotation)
13. [Code examples](#code-examples)
14. [Adaptive tuning](#adaptive-tuning)
15. [Security model](#security-model)
16. [Configuration](#configuration)
17. [Logging](#logging)
18. [Project layout](#project-layout)

---

## Quick start

```bash
# Build (release)
cargo build --release

# Run (auto-tunes for the host)
./target/release/apiku serve

# Custom bind + verbose logs + log to file
./target/release/apiku serve --bind 0.0.0.0:8080 --log debug --log-file apiku.log
```

Open `http://127.0.0.1:3000/` for the tester website. No API key required.

---

## CLI reference

```
apiku [OPTIONS] [COMMAND]

Commands:
  serve         Run as an HTTP API server (recommended)
  scrape        Scrape one or more URLs (CLI)
  batch         Read URLs from a file and scrape them all
  info          Print version and adapter list

Global options:
  -c, --config <PATH>            Path to TOML config file (default: config.toml)
      --log <LEVEL>              error|warn|info|debug|trace (default: info)
      --log-format <FMT>         pretty|json|compact (default: pretty)
      --log-file <PATH>          Tee logs to a file as well
      --concurrency <N>          Override engine concurrency (1-100)
      --timeout <SECONDS>        Override request timeout (1-300)
      --rate-limit <MS>          Override rate-limit delay (100-60000)
      --max-retries <N>          Override max retry attempts
  -H, --header 'Name: Value'     Add custom HTTP header (repeatable)
      --user-agent <STRING>      Override User-Agent
      --referer <URL>            Override Referer
      --no-deep                  Skip deep page extraction

CLI mode only:
  -u, --url <URL>                Single target URL
      --urls <URL>...            Multiple target URLs
  -o, --output <PATH>            Output file (use '-' for stdout)
      --stdout                   Write JSON to stdout
      --indent <0-8>             JSON indentation (0 = compact)
      --flat                     Bare content object instead of full envelope
      --clean <MODE>             none|clean|minimal
      --summary                  Print summary table to stderr

serve options:
      --bind <ADDR>              Bind address (default: 127.0.0.1:3000)
```

---

## HTTP API at a glance

Base URL: `http://127.0.0.1:3000` (local) — base path: `/api/v1`.

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/health` | Liveness probe |
| `GET` | `/api/v1/info` | Server info, system tuning, providers, endpoints |
| `GET` | `/api/v1/search?q=...&source=...&page=N` | Cross-provider search |
| `GET` | `/api/v1/browse/{provider}?feed=...&page=N&size=N` | Provider home / popular / latest feed |
| `GET` | `/api/v1/manga/{id}?page=N&size=N` | Manga series detail (Mangaball) — chapter list paginated |
| `GET` | `/api/v1/manga/chapter/{id}` | Manga chapter pages |
| `GET` | `/api/v1/donghua/{id}?page=N&size=N` | Donghua series detail (Anichin) — episode list paginated |
| `GET` | `/api/v1/donghua/episode/{id}` | Donghua episode (servers + downloads) |
| `GET` | `/api/v1/cosplay/{id}` | Cosplay post (gallery + downloads) |
| `GET` | `/api/v1/novel/{id}?page=N&size=N` | Novel series detail (NovelID) — chapter list paginated, supports upstream-paginated novels with thousands of chapters |
| `GET` | `/api/v1/novel/chapter/{id}` | Novel chapter (text body, plus prev/next IDs) |
| `GET` | `/api/v1/nhentai/{id}` | nhentai gallery (browser-fingerprint spoofed) |
| `GET` | `/api/v1/nhentai/chapter/{id}` | nhentai gallery as a chapter (proxied page list) |
| `GET` | `/img?p={payload}&s={signature}` | Signed image proxy |

Every response carries a generated `X-Request-Id` header echoed in `meta.request_id`.

Providers are: `mangaball` | `anichin` | `cosplaytele` | `nhentai` | `novelid`.

---

## Browse feeds

`GET /api/v1/browse/{provider}?feed={feed}&page={n}&size={N}` surfaces home / popular / latest content for any provider, with the same envelope shape as `/search`.

| Provider | Feed values |
|---|---|
| `mangaball` | `home` (featured), `popular`, `latest`, `recommend` (page-sliced from a single API response, `size` defaults to 30, max 60) |
| `anichin` | `home` (= latest update), `popular`, `rating`, `title` (A-Z), `latest-added` |
| `cosplaytele` | `home` (latest), `popular` / `hot`, or any category slug (e.g. `genshin-impact`, `azur-lane`) |
| `nhentai` | `home` (recent), `popular-today`, `popular-week`, `popular` (all-time) |
| `novelid` | `home` (semua), `popular` (alias of `tamat`), or any genre slug: `novel-translate`, `fantasi`, `romantis`, `religi`, `motivasi`, `horror`, `aksi`, `komedi`, `sastra`, `novel-anak` |

Pagination is page-based: `?page=2`, `?page=3`, ...

```bash
# Today's popular nhentai galleries
curl 'http://127.0.0.1:3000/api/v1/browse/nhentai?feed=popular-today'

# Anichin most popular donghua, page 2
curl 'http://127.0.0.1:3000/api/v1/browse/anichin?feed=popular&page=2'

# Cosplaytele latest posts
curl 'http://127.0.0.1:3000/api/v1/browse/cosplaytele?feed=home'

# NovelID Romantis genre
curl 'http://127.0.0.1:3000/api/v1/browse/novelid?feed=romantis'

# Mangaball popular, with explicit page size
curl 'http://127.0.0.1:3000/api/v1/browse/mangaball?feed=popular&size=30&page=1'
```

---

## Search

`GET /api/v1/search?q={query}&source={source}&page={n}`

- `source`: `all` (default) | `manga` | `donghua` | `cosplay` | `nhentai` | `novel`
- `page`: 1-based, applies to providers that support upstream pagination
- nhentai accepts inline `[tag]` syntax — e.g. `?q=Genshin+Impact+%5Bfull+color%5D&source=nhentai`

```bash
curl 'http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga'
curl 'http://127.0.0.1:3000/api/v1/search?q=peerless&source=donghua'
curl 'http://127.0.0.1:3000/api/v1/search?q=Martial+Universe&source=novel'
```

Sort options for nhentai (popular today / week / all-time) live under `/browse/nhentai` instead of `/search`, since they apply to discovery rather than keyword queries.

---

## Series and chapter pagination

Series detail endpoints (`/manga/{id}`, `/donghua/{id}`, `/novel/{id}`) accept `page` and `size` query parameters to paginate the chapter / episode list inside the response. The series metadata (title, cover, synopsis, ...) is unchanged on every page; only the `chapters[]` / `episodes[]` array contains the requested window.

The response includes pagination metadata so a client can build prev/next navigation without a second round-trip:

| Field | Meaning |
|---|---|
| `chapter_count` / `episode_count` | Total count across all pages |
| `chapter_page` / `episode_page` | Current page (1-indexed) |
| `chapter_page_size` / `episode_page_size` | Items per page |
| `chapter_total_pages` / `episode_total_pages` | Total pages |

```bash
# Page 3 of a novel's chapters, 20 per page
curl 'http://127.0.0.1:3000/api/v1/novel/<id>?page=3&size=20'
```

### Upstream-paginated novels (NovelID)

NovelID itself paginates the chapter list at the source — each `?page=N` upstream returns ~30 chapters out of potentially thousands. `apiku` handles this transparently:

- Page 1 always fetches the canonical URL (gives metadata + first 30 chapters) and the **last** upstream page in parallel, so `chapter_count` is exactly accurate even on the first request.
- Any API page request computes which upstream pages cover its window, fetches them concurrently (via the per-URL single-flight cache), and slices by chapter number.
- Subsequent pages are sub-millisecond because every upstream page is cached.

Tested against *Martial Universe* (1,309 chapters, 44 upstream pages):

| Request | Result |
|---|---|
| `?page=1&size=30` | chapters 1-30, `chapter_count: 1309`, `chapter_total_pages: 44` |
| `?page=2&size=30` | chapters 31-60 |
| `?page=20&size=30` | chapters 571-600 |
| `?page=44&size=30` | chapters 1291-1309 (last page, 19 items) |
| `?page=1&size=50` | chapters 1-50, `chapter_total_pages: 27` (spans 2 upstream pages) |

---

## Response envelope

Every endpoint shares the same JSON envelope.

### Success

```json
{
  "status": 200,
  "ok": true,
  "data": { /* endpoint-specific payload */ },
  "meta": {
    "took_ms": 123,
    "cached": false,
    "request_id": "1f8b2c4d-..."
  }
}
```

### Error

```json
{
  "status": 404,
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "Route not found: /api/v1/nope"
  },
  "meta": {
    "took_ms": 0,
    "cached": false,
    "request_id": "..."
  }
}
```

---

## Sample payloads

### `GET /api/v1/search?q=Martial+Universe&source=novel`

```json
{
  "status": 200, "ok": true,
  "data": {
    "query": "Martial Universe",
    "source": "novel",
    "page": 1,
    "total": 1,
    "items": [
      {
        "id": "nvsxyz....",
        "source": "novelid",
        "kind": "novel",
        "title": "Martial Universe (Wu Dong Qian Kun Terjemah Indo)",
        "thumbnail": "/img?p=...&s=...",
        "snippet": null,
        "tags": ["Novel Translate", "Tamat"]
      }
    ]
  },
  "meta": { "took_ms": 320, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/manga/{id}?page=1&size=60`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "mbsabc....",
    "title": "Dark Mortal",
    "description": "...",
    "author": null,
    "artist": null,
    "genres": [],
    "cover": "/img?p=...&s=...",
    "chapter_count": 85,
    "chapter_page": 1,
    "chapter_page_size": 60,
    "chapter_total_pages": 2,
    "chapters": [
      {
        "id": "mbiabc....",
        "number": 1.0,
        "title": "Family",
        "translations": [
          { "id": "mbixyz....", "language": "English", "group": "Articuno",
            "date": "2026-02-10", "pages": 71 }
        ]
      }
    ]
  },
  "meta": { "took_ms": 180, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/manga/chapter/{id}`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "mbiabc....",
    "series_title": "Dark Mortal Vol. 1",
    "chapter_number": 1.0,
    "page_count": 71,
    "pages": [
      { "index": 1, "url": "/img?p=...&s=..." },
      { "index": 2, "url": "/img?p=...&s=..." }
    ]
  },
  "meta": { "took_ms": 270, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/donghua/episode/{id}`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "aciabc....",
    "series_title": "Peerless Martial Spirit",
    "series_id": "acsdef....",
    "episode_number": 440,
    "prev_id": "aci...",
    "next_id": null,
    "servers": [
      { "label": "Dailymotion", "embed_url": "https://geo.dailymotion.com/...", "format": "embed" }
    ],
    "downloads": [
      {
        "quality": "720p",
        "mirrors": [
          { "name": "Mirrored", "url": "https://www.mirrored.to/multilinks/..." }
        ]
      }
    ]
  },
  "meta": { "took_ms": 410, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/cosplay/{id}`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "ctpghi....",
    "title": "ChuChu Magic cosplay Raiden Shogun ...",
    "cosplayer": "ChuChu Magic",
    "character": "Raiden Shogun",
    "series": "Genshin Impact",
    "photo_count": 23,
    "video_count": 1,
    "categories": ["Cosplay Game", "Genshin Impact"],
    "tags": ["Raiden Shogun"],
    "published_at": "2026-05-25T16:16:34+08:00",
    "cover": "/img?p=...&s=...",
    "images": ["/img?p=...&s=...", "/img?p=...&s=..."],
    "videos": [],
    "downloads": [
      { "name": "Download Telegram", "url": "https://t.me/+..." }
    ],
    "unzip_password": "cosplaytele"
  },
  "meta": { "took_ms": 220, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/novel/{id}?page=1&size=30`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "nvsabc....",
    "title": "Martial Universe (Wu Dong Qian Kun Terjemah Indo)",
    "author": "Fight007",
    "status": "Tamat",
    "genres": ["Romantis"],
    "synopsis": "Lin Dong, dia menjalani kehidupan berat penuh dengan hinaan ...",
    "cover": "/img?p=...&s=...",
    "rating": "8.00",
    "chapter_count": 1309,
    "chapter_page": 1,
    "chapter_page_size": 30,
    "chapter_total_pages": 44,
    "chapters": [
      { "id": "nviabc....", "number": 1, "title": "Lin Dong - Bagian 1" },
      { "id": "nvixyz....", "number": 2, "title": "Tinju Penetrasi - 2" }
    ]
  },
  "meta": { "took_ms": 310, "cached": false, "request_id": "..." }
}
```

### `GET /api/v1/novel/chapter/{id}`

```json
{
  "status": 200, "ok": true,
  "data": {
    "id": "nviabc....",
    "series_title": "Martial Universe (Wu Dong Qian Kun Terjemah Indo)",
    "series_id": "nvsabc....",
    "chapter_number": 1,
    "chapter_title": "Lin Dong - Bagian 1",
    "body": "\u201CWuu.\u201D\n\nKetika Lin Dong mengumpulkan setiap ons kekuatan...",
    "body_html": "<p>...</p><p>...</p>",
    "prev_id": null,
    "next_id": "nvinext....",
    "word_count": 1842
  },
  "meta": { "took_ms": 280, "cached": false, "request_id": "..." }
}
```

---

## Status codes and error codes

### HTTP status codes

| Code | Meaning |
|---|---|
| `200 OK` | Request succeeded |
| `400 Bad Request` | Malformed query, invalid opaque ID, query too long |
| `403 Forbidden` | Bad image-proxy signature, host not on allowlist |
| `404 Not Found` | Unknown route |
| `502 Bad Gateway` | Upstream provider returned an error or unparseable content |

### Error codes (in `error.code`)

| Code | When |
|---|---|
| `missing_query` | `/api/v1/search` called without `q` |
| `query_too_long` | `q` longer than 200 chars |
| `invalid_id` | Opaque ID malformed or has bad signature |
| `wrong_source` | ID belongs to a different provider than the endpoint |
| `wrong_kind` | Scraped page does not match endpoint expectation |
| `scrape_failed` | Upstream scrape returned no content |
| `upstream_error` | Network or 5xx from upstream |
| `upstream_status` | Upstream returned a non-success HTTP status |
| `bad_signature` | Image proxy signature failed verification |
| `host_not_allowed` | Image URL host not on the proxy allowlist |
| `bad_payload` | Image proxy payload is not valid base64url / utf-8 |
| `not_found` | Unknown route (404) |

---

## Opaque ID format

Every resource ID returned by the API is HMAC-SHA256 signed.

```
<source 2 chars><kind 1 char><nonce 3 chars>.<base64url-payload>.<base64url-mac-16-bytes>
```

Example: `mbsijk.aHR0cHM6Ly9tYW5nYWJhbGwubmV0L3RpdGxlLWRldGFpbC8.J9k1Nz5pQq3v7L2HjT7M5w`

- **source** — `mb` (mangaball), `ac` (anichin), `ct` (cosplaytele), `nh` (nhentai), `nv` (novelid)
- **kind** — `s` (series), `i` (item: chapter or episode), `p` (post)
- **nonce** — 3 random base32-ish chars to prevent identical IDs for the same URL
- **payload** — base64url-encoded raw URL
- **mac** — first 16 bytes of `HMAC-SHA256(secret, header || "." || payload)`, base64url-encoded (~22 chars, 128-bit security)

Constant-time comparison rejects any tampering. The server secret is regenerated on every restart unless `APIKU_SECRET` is set, in which case IDs remain stable across restarts.

---

## Image proxy

All cover, thumbnail, gallery and page-image URLs in API responses are rewritten to:

```
/img?p={base64url-encoded-url}&s={hmac-signature}
```

Four layers of defence:

1. **HMAC-SHA256 signature** — verified server-side per request, in constant time.
2. **Host allowlist** — even with a forged signature the proxy will only fetch from these hosts:
   - `*.poke-black-and-white.net`, `*.red-and-blue.net`, `*.pokemon-gold-silver.net`, `*.pokemon-ruby-sapphire.net` (Mangaball CDNs)
   - `anichin.cafe`, `anichin.care`, `anichin.cloud`, `i0..i3.wp.com` (Anichin)
   - `cosplaytele.com`, `*.cosplaytele.com`
   - `nhentai.net`, `nhentai.xxx`, `nhentai.to`, `i1..i4.nhentai.net`, `t1..t4.nhentai.net`
   - `novelid.org` and the wp.com mirror it uses
3. **Referer spoofing** — outgoing requests carry the source domain as `Referer` to bypass hotlink protection.
4. **Browser fingerprint rotation** — every outgoing image request applies a coherent browser identity (User-Agent, Sec-CH-UA, Sec-Fetch-* tailored for an `<img>` request, narrow image-Accept) picked deterministically per upstream URL.

Responses set `Cache-Control: public, max-age=86400, immutable` so browsers cache aggressively.

---

## Browser fingerprint rotation

apiku ships a curated catalogue of internally consistent browser identities:

- Windows / Chrome 121
- Windows / Edge 121
- macOS / Safari 17
- macOS / Chrome 121
- Linux / Firefox 122
- Linux / Chrome 121
- Android / Chrome 121
- Android / Samsung Internet 23
- iOS / Safari 17 (iPhone)
- iPadOS / Safari 17

Each entry sets `User-Agent`, `Accept`, `Accept-Language`, `Accept-Encoding`, `Sec-CH-UA*` and `Sec-Fetch-*` as a unit so the request is internally coherent. Selection is deterministic: a SHA-256 of the upstream URL picks the index, so the same URL always uses the same identity (defeats simple rotation-detection heuristics).

Used wherever apiku makes outbound requests to fingerprint-sensitive hosts: nhentai's JSON API and CDN, the image proxy, and the cross-provider search calls.

---

## Code examples

All examples make the same request: search "one piece" on Mangaball.

### cURL

```bash
curl 'http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga'
curl 'http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga' | jq .
```

### JavaScript (browser / Node 18+)

```js
const res = await fetch('http://127.0.0.1:3000/api/v1/search?q=one piece&source=manga');
const json = await res.json();

if (!json.ok) {
  throw new Error(`${json.error.code}: ${json.error.message}`);
}

console.log(`Found ${json.data.total} (${json.meta.took_ms}ms)`);
for (const item of json.data.items) {
  console.log(`- [${item.source}] ${item.title}`);
}
```

### TypeScript

```ts
interface Envelope<T> {
  status: number;
  ok: boolean;
  data: T;
  meta: { took_ms: number; cached: boolean; request_id: string };
}
interface ApiErr {
  status: number;
  ok: false;
  error: { code: string; message: string };
  meta: { took_ms: number; request_id: string };
}
type Response<T> = Envelope<T> | ApiErr;

interface SearchData {
  query: string;
  source: string;
  page: number;
  total: number;
  items: Array<{
    id: string;
    source: 'mangaball' | 'anichin' | 'cosplaytele' | 'nhentai' | 'novelid';
    kind: 'manga' | 'donghua' | 'cosplay' | 'doujin' | 'novel';
    title: string;
    thumbnail?: string;
    snippet?: string;
    tags: string[];
  }>;
}

const res = await fetch('http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga');
const json: Response<SearchData> = await res.json();
if (!json.ok) throw new Error(`${json.error.code}: ${json.error.message}`);
console.log(json.data.total, json.meta.took_ms);
```

### Python

```python
import requests

BASE = 'http://127.0.0.1:3000'

def api_get(path, params=None):
    r = requests.get(f'{BASE}{path}', params=params)
    r.raise_for_status()
    body = r.json()
    if not body.get('ok'):
        raise RuntimeError(f"{body['error']['code']}: {body['error']['message']}")
    return body['data']

# Search
results = api_get('/api/v1/search', {'q': 'one piece', 'source': 'manga'})
print(f"Found {results['total']} items")

# Detail with chapter pagination
if results['items']:
    series = api_get(f"/api/v1/manga/{results['items'][0]['id']}", {'page': 1, 'size': 30})
    print(f"{series['title']}: {series['chapter_count']} chapters across {series['chapter_total_pages']} pages")
```

### PHP

```php
<?php
const BASE = 'http://127.0.0.1:3000';

function api_get(string $path): array {
    $ch = curl_init(BASE . $path);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    curl_setopt($ch, CURLOPT_HTTPHEADER, ['Accept: application/json']);
    $body = curl_exec($ch);
    curl_close($ch);

    $json = json_decode($body, true);
    if (!$json['ok']) {
        throw new RuntimeException("{$json['error']['code']}: {$json['error']['message']}");
    }
    return $json['data'];
}

$search = api_get('/api/v1/search?' . http_build_query([
    'q' => 'one piece', 'source' => 'manga',
]));
echo "Found {$search['total']} results\n";
```

### Go

```go
package main

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
        Code, Message string
    } `json:"error,omitempty"`
    Meta struct {
        TookMs    int    `json:"took_ms"`
        Cached    bool   `json:"cached"`
        RequestID string `json:"request_id"`
    } `json:"meta"`
}

type SearchData struct {
    Total int `json:"total"`
    Items []struct {
        ID, Source, Kind, Title string
    } `json:"items"`
}

func apiGet[T any](path string) (*Envelope[T], error) {
    resp, err := http.Get(Base + path)
    if err != nil { return nil, err }
    defer resp.Body.Close()
    body, _ := io.ReadAll(resp.Body)
    var env Envelope[T]
    if err := json.Unmarshal(body, &env); err != nil { return nil, err }
    if !env.Ok { return nil, fmt.Errorf("%s: %s", env.Error.Code, env.Error.Message) }
    return &env, nil
}

func main() {
    qs := url.Values{"q": {"one piece"}, "source": {"manga"}}
    res, err := apiGet[SearchData]("/api/v1/search?" + qs.Encode())
    if err != nil { panic(err) }
    fmt.Printf("Found %d (took %dms)\n", res.Data.Total, res.Meta.TookMs)
}
```

### C++

```cpp
// Requires libcurl + nlohmann::json
#include <iostream>
#include <string>
#include <curl/curl.h>
#include <nlohmann/json.hpp>

using json = nlohmann::json;

static size_t cb(void* p, size_t s, size_t n, std::string* out) {
    out->append((char*)p, s * n); return s * n;
}

json api_get(const std::string& path) {
    CURL* curl = curl_easy_init();
    std::string body;
    curl_easy_setopt(curl, CURLOPT_URL, ("http://127.0.0.1:3000" + path).c_str());
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, cb);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &body);
    curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    auto j = json::parse(body);
    if (!j["ok"].get<bool>())
        throw std::runtime_error(j["error"]["code"].get<std::string>());
    return j["data"];
}

int main() {
    auto data = api_get("/api/v1/search?q=one+piece&source=manga");
    std::cout << "Total: " << data["total"] << "\n";
}
```

### Rust

```rust
// reqwest = { version = "0.12", features = ["json"] }
// serde = { version = "1", features = ["derive"] }
// tokio = { version = "1", features = ["full"] }
use serde::Deserialize;

#[derive(Deserialize)] struct Envelope<T> { ok: bool, data: Option<T>, error: Option<ApiError>, meta: Meta }
#[derive(Deserialize)] struct ApiError { code: String, message: String }
#[derive(Deserialize)] struct Meta { took_ms: u64, request_id: String, cached: bool }
#[derive(Deserialize)] struct SearchData { total: usize, items: Vec<SearchItem> }
#[derive(Deserialize)] struct SearchItem { id: String, source: String, title: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp: Envelope<SearchData> = reqwest::get(
        "http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga"
    ).await?.json().await?;

    if !resp.ok { let e = resp.error.unwrap(); return Err(format!("{}: {}", e.code, e.message).into()); }
    let data = resp.data.unwrap();
    println!("Total: {} ({}ms)", data.total, resp.meta.took_ms);
    Ok(())
}
```

---

## Adaptive tuning

On startup, the server detects CPU cores and RAM and chooses values from this table:

| Parameter | Logic | Min | Max |
|---|---|---|---|
| Tokio worker threads | `= cores` | 2 | 32 |
| HTTP concurrency | `= cores * 4` | 8 | 100 |
| TCP pool / host | `= cores * 8` | 16 | 256 |
| Scrape cache capacity | scaled with RAM | 500 | 50,000 |
| Search cache capacity | `= scrape_cache / 4` | - | - |

Profile tier:

- **minimal** (≤ 1 GB RAM)
- **small** (1-4 GB)
- **standard** (4-16 GB)
- **production** (> 16 GB)

Sample startup log:

```
INFO  apiku                    system detected cores=8 ram_mib=7455 threads=8 profile="standard (workstation / 8-16GB)"
INFO  apiku::server            server listening addr=127.0.0.1:3000
```

---

## Security model

### Opaque IDs

- 128-bit MAC truncated from HMAC-SHA256
- Constant-time comparison
- 3-character nonce in the header
- Tamper-detected with `invalid_id` 400 errors

### Image proxy

- HMAC-SHA256 96-bit signature, verified per request
- Host allowlist (see [Image proxy](#image-proxy))
- `Referer` spoofed to source domain to defeat hotlink protection
- Browser fingerprint applied per request
- Long-lived `Cache-Control` for browser caching

### Rate limiting

- Per-domain delay (default 400 ms in server mode)
- Single-flight cache: simultaneous requests for the same URL collapse into a single upstream fetch
- Scrape result TTL: 10 minutes — Search TTL: 5 minutes

### Hardening recommendations for production

- Set `APIKU_SECRET=<long-random>` in env so IDs survive restarts
- Run behind a reverse proxy (nginx / caddy) for TLS, IP rate limiting, access logs
- Restrict `Access-Control-Allow-Origin` instead of `*`
- Add HTTP basic auth or API keys at the reverse proxy layer if exposed publicly

---

## Configuration

Optional `config.toml`:

```toml
# Default headers applied to every outbound request
[headers]
"User-Agent" = "Mozilla/5.0 (compatible; apiku/0.2)"

# Per-domain rate limits in milliseconds
[rate_limits]
"mangaball.net"   = 400
"anichin.cafe"    = 800
"cosplaytele.com" = 600
"nhentai.net"     = 600
"novelid.org"     = 400

# Per-site overrides (referer, user-agent, headers)
[sites."mangaball.net"]
referer = "https://mangaball.net/"
```

All values are optional and have sensible defaults; the file is not required.

---

## Logging

Logs go to stderr by default. Three formats:

| `--log-format` | Use case |
|---|---|
| `pretty` (default) | Coloured, aligned, human-readable |
| `json` | Machine-readable, ideal for log aggregation |
| `compact` | Single-line, no colours |

Use `--log-file <path>` to tee logs to a file in addition to stderr.

Sample colourised output:

```
2026-05-25T20:11:46Z  INFO  apiku::engine            Starting scrape of 1 URL(s) with concurrency 32
2026-05-25T20:11:46Z  INFO  apiku::engine            HTTP 200 for https://mangaball.net/title-detail/dark-mortal/
2026-05-25T20:11:47Z  INFO  apiku::engine            [1/1] Completed | Remaining: 0
```

---

## Project layout

```
src/
├── main.rs            CLI entry point, argument parsing, runtime setup
├── log.rs             Coloured tracing-subscriber setup, banner
├── api.rs             REST handlers, DTOs, response envelopes, browse + paging
├── server.rs          axum router, middleware (request-id, CORS, compression)
├── tester.rs          Tester website (maud), embedded CSS/JS
├── tester.css         Tester stylesheet (compiled in)
├── tester.js          Tester client-side script (compiled in)
├── opaque.rs          HMAC-SHA256 opaque ID + image-proxy signing
├── fingerprint.rs     Browser fingerprint catalogue (Win/macOS/Linux/Android/iOS)
├── search.rs          Cross-provider search abstraction
├── sysspec.rs         CPU/RAM detection and tuning
├── engine.rs          Scraping orchestrator (used by CLI and API)
├── pipeline.rs        Outbound request header pipeline
├── rate_limiter.rs    Per-domain rate limiter
├── retry.rs           Retry handler with exponential backoff
├── deep_extractor.rs  Generic page extractor (links, images, OG, JSON-LD)
├── parser.rs          scraper wrapper with infallible CSS queries
├── models.rs          Domain models (ContentModel and friends)
├── error.rs           ScraperError variants
├── config.rs          TOML configuration loading + validation
└── adapters/
    ├── mod.rs         SiteAdapter trait + registry
    ├── mangaball.rs   Mangaball SPA adapter (multi-step API + browse search_types)
    ├── anichin.rs     Anichin donghua streaming adapter (HTML + browse orders)
    ├── cosplaytele.rs Cosplaytele cosplay archive adapter (HTML + categories)
    ├── nhentai.rs     nhentai doujinshi adapter (JSON API + sharded CDN + popular feeds)
    └── novelid.rs     NovelID Indonesian novel adapter (HTML, upstream-paginated chapter lists)
```

---

## License

MIT
