# apiku

[![CI](https://github.com/risqinf/apiku/actions/workflows/ci.yml/badge.svg)](https://github.com/risqinf/apiku/actions/workflows/ci.yml)
[![Release](https://github.com/risqinf/apiku/actions/workflows/release.yml/badge.svg)](https://github.com/risqinf/apiku/actions/workflows/release.yml)
[![Live](https://img.shields.io/website?url=https%3A%2F%2Fapi.risqinf.web.id%2Fapi%2Fv1%2Fhealth&label=api.risqinf.web.id)](https://api.risqinf.web.id)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> RESTful scraping API for **Mangaball**, **Anichin**, **Otakudesu**, **Cosplaytele**, **nhentai**, and **NovelID** — one HTTP service that other developers can build manga readers, anime & donghua players, cosplay galleries, doujinshi browsers, and novel readers against, without ever seeing the upstream URLs.

- **Live demo:** <https://api.risqinf.web.id> (hosted on AWS — same binary, same endpoints)
- **Repo:** <https://github.com/risqinf/apiku>
- **Releases:** <https://github.com/risqinf/apiku/releases> (pre-built binaries for Linux x86_64 / ARM64, macOS Intel / Apple Silicon, Windows x86_64 / ARM64)
- **Author:** [@risqinf](https://github.com/risqinf)
- **License:** MIT
- **Version:** 0.2.2 (see `Cargo.toml`)

---

## Highlights

- **One API, six providers.** Anime, donghua, manga/komik, cosplay archives, doujinshi catalogues, and Indonesian novels behind a uniform JSON envelope.
- **Opaque IDs.** Resource IDs are HMAC-SHA256-signed tokens. Consumers never see upstream URLs.
- **Image proxy.** Every cover, page and thumbnail is rewritten to a signed local proxy. Source CDNs stay hidden.
- **Cosplay video → HLS, server-resolved.** Cosplaytele videos are served via an encrypted third-party embed (`cossora.stream`) that blocks plain iframes. apiku fetches the embed with the right Referer, **decrypts the real `.m3u8` URL server-side (AES-256-CBC)**, and hands the client a playable HLS stream. Only the tiny playlists are proxied — the heavy `.ts` segments stream **directly from the CDN to the client** to save bandwidth. No iframe embeds anywhere.
- **High-precision search.** Cosplaytele's loose WordPress search and its recommendation carousels are stripped out, then results are relevance-filtered so a query like `xiaoyaoyaoyao` returns only that cosplayer. Cosplayer names and doujin tags are **clickable** and jump to a filtered search.
- **Browser fingerprint rotation.** Outbound requests pick a coherent identity (Windows/Chrome, macOS/Safari, Android/Chrome, iPhone/Safari, Linux/Firefox, ...) per upstream URL — coupled with proper Sec-CH-UA, Sec-Fetch-* and Referer spoofing so origin checks and hotlink protection see a real browser visiting the source site.
- **Adaptive runtime.** CPU and RAM are detected at startup; tokio threads, HTTP concurrency, and cache sizes are tuned automatically.
- **Client-side prefetch.** The web app warms the next likely detail/episode/chapter/page during idle time, so navigation feels instant.
- **Single-flight cache.** Concurrent requests for the same URL collapse into one upstream fetch.
- **Browse + search + detail + paged chapter list** for every provider.
- **Consumer web app at `/`.** A dependency-free SPA streaming/reading platform with a modern UI: animated aurora + color-flow grid background, home rows, per-provider browse with feed filters, search with per-source filter chips, donghua player with server switching, manga/doujin reader with fullscreen, novel text reader, cosplay galleries with inline HLS video, and per-detail recommendations. Manga detail pages **group chapters by language** with one-tap language tabs that persist across pagination. Includes a responsive navbar (desktop bar + mobile drawer with real-time toggle switches), light/dark theme toggle, an in-app **API Docs** page, an inline **API Explorer** with copy-ready multi-language code samples, and an **18+ toggle** (clear age-verification modal) that hides the adult providers (Cosplay, Doujin) until explicitly enabled.
- **Configurable branding, no recompile.** Site name, tagline, logo, footer, ad slots, and SEO/ad-network verification snippets are driven by a `[web]` block in `config.toml` and/or environment variables — see [Branding & customization](#branding--customization). Drop a `logo.*` into `public/` and it's auto-detected.
- **Developer API console at `/tester`.** Live request playground, multi-language code examples, full reference, security notes.

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
17. [Branding & customization](#branding--customization)
18. [Deployment](#deployment)
19. [Logging](#logging)
20. [Project layout](#project-layout)
21. [Roadmap](#roadmap)

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

Open `http://127.0.0.1:3000/` for the **web app** — a full streaming/reading platform (browse, search, watch donghua, read manga & novels, view cosplay galleries) with a light/dark theme toggle, an inline API Explorer, and an 18+ toggle. The developer API console lives at `http://127.0.0.1:3000/tester`. No API key required.

### Try it without building

The same binary is hosted at **<https://api.risqinf.web.id>** (AWS). Every endpoint shown in this README also works there:

```bash
curl 'https://api.risqinf.web.id/api/v1/info'
curl 'https://api.risqinf.web.id/api/v1/search?q=Martial+Universe&source=novel'
curl 'https://api.risqinf.web.id/api/v1/browse/nhentai?feed=popular-today'
```

The tester website is also live at <https://api.risqinf.web.id/>.

### Pre-built binaries

Releases are auto-built by GitHub Actions on every `v*.*.*` tag. Download from <https://github.com/risqinf/apiku/releases> for:

- `x86_64-unknown-linux-gnu` (Linux glibc, generic)
- `x86_64-unknown-linux-musl` (Linux static, portable)
- `aarch64-unknown-linux-gnu` (Linux ARM64 — Raspberry Pi 4/5, AWS Graviton)
- `x86_64-apple-darwin` (macOS Intel)
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-pc-windows-msvc` (Windows 64-bit)
- `aarch64-pc-windows-msvc` (Windows ARM64)

Each archive ships with a SHA-256 checksum; a combined `SHA256SUMS` file is also attached to the release.

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
| `GET` | `/api/v1/anime/{id}` | Anime series detail (Otakudesu) — full metadata + episode list |
| `GET` | `/api/v1/anime/episode/{id}` | Anime episode — quality-grouped streaming mirrors + downloads |
| `GET` | `/api/v1/anime-stream?id=...` | Resolve an anime mirror token into a playable embed URL |
| `GET` | `/api/v1/cosplay/{id}` | Cosplay post (gallery + resolved video + downloads) |
| `GET` | `/api/v1/cosplay-video?p=...&s=...` | Resolve a Cosplaytele embed into a playable HLS stream URL |
| `GET` | `/api/v1/novel/{id}?page=N&size=N` | Novel series detail (NovelID) — chapter list paginated, supports upstream-paginated novels with thousands of chapters |
| `GET` | `/api/v1/novel/chapter/{id}` | Novel chapter (text body, plus prev/next IDs) |
| `GET` | `/api/v1/nhentai/{id}` | nhentai gallery (browser-fingerprint spoofed) |
| `GET` | `/api/v1/nhentai/chapter/{id}` | nhentai gallery as a chapter (proxied page list) |
| `GET` | `/img?p={payload}&s={signature}` | Signed image proxy |
| `GET` | `/hls?p={payload}&s={signature}` | HLS playlist proxy (segments stream direct from CDN to the client) |

Every response carries a generated `X-Request-Id` header echoed in `meta.request_id`.

Providers are: `mangaball` | `anichin` | `otakudesu` | `cosplaytele` | `nhentai` | `novelid`.

---

## Browse feeds

`GET /api/v1/browse/{provider}?feed={feed}&page={n}&size={N}` surfaces home / popular / latest content for any provider, with the same envelope shape as `/search`.

| Provider | Feed values |
|---|---|
| `mangaball` | `home` (featured), `popular`, `latest`, `recommend` (page-sliced from a single API response, `size` defaults to 30, max 60) |
| `anichin` | `home` (= latest update), `popular`, `rating`, `title` (A-Z), `latest-added` |
| `otakudesu` | `ongoing`, `complete`, or any genre slug (`action`, `romance`, `comedy`, `fantasy`, `adventure`, `drama`, ...) |
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

- `source`: `all` (default) | `manga` | `donghua` | `anime` | `cosplay` | `nhentai` | `novel`
- `page`: 1-based, applies to providers that support upstream pagination
- nhentai accepts inline `[tag]` syntax — e.g. `?q=Genshin+Impact+%5Bfull+color%5D&source=nhentai`
- results are relevance-ranked (closest title matches first) across all providers

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
    "videos": ["/api/v1/cosplay-video?p=...&s=..."],
    "downloads": [
      { "name": "Download Telegram", "url": "https://t.me/+..." }
    ],
    "unzip_password": "cosplaytele"
  },
  "meta": { "took_ms": 220, "cached": false, "request_id": "..." }
}
```

`videos[]` entries that point at `/api/v1/cosplay-video?...` resolve to a playable HLS stream — call that endpoint to get `{ "type": "hls", "url": "/hls?..." }`, then play the `/hls` URL with hls.js (or natively on Safari). Heavy video segments stream straight from the CDN to the client; only the playlist passes through the server.

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
   - `otakudesu.blog` and its mirror domains (anime covers)
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
# Local
curl 'http://127.0.0.1:3000/api/v1/search?q=one+piece&source=manga'

# Live demo (same shape, hosted on AWS)
curl 'https://api.risqinf.web.id/api/v1/search?q=one+piece&source=manga' | jq .
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

## Branding & customization

The consumer web app served at `/` can be fully rebranded and monetized **without recompiling** — everything is read at server start and injected into the SPA. Configure it in the `[web]` block of `config.toml`:

```toml
[web]
# Shown in the header, drawer, and browser tab title.
site_name = "NontonKu"

# Home hero tagline.
tagline = "Streaming donghua, baca komik & novel, galeri cosplay - semua dalam satu platform."

# Custom logo. Leave empty to auto-detect (see below) or use the built-in mark.
# Absolute URL or a root path served from `static_dir` (e.g. "/logo.svg").
logo_url = ""

# Footer HTML. Empty -> a minimal "<site_name> (c) <year>" line.
footer_html = ""

# Raw HTML injected into <head> (SEO / ad-network verification, analytics).
head_html = '<meta name="google-site-verification" content="XXXX">'

# Raw HTML injected just before </body> (deferred scripts).
body_html = ""

# Directory served at the site root for verification files, ads.txt,
# sitemap.xml, robots.txt, favicons, and custom logos.
static_dir = "public"

# Named ad slots rendered at fixed positions. Known slots: home, browse, reader.
[web.ads]
home   = '<ins class="adsbygoogle" ...></ins>'
reader = ''
```

### Changing the site name (no rebuild)

Two ways, both outside the binary:

- **Config file:** set `site_name` in `[web]` and restart `apiku serve`.
- **Environment variables** (win over the config file — handy for Docker / systemd):

  | Variable | Overrides |
  |---|---|
  | `APIKU_SITE_NAME` | site name |
  | `APIKU_TAGLINE` | hero tagline |
  | `APIKU_LOGO_URL` | logo |
  | `APIKU_STATIC_DIR` | static directory |

  ```bash
  APIKU_SITE_NAME="NontonKu" apiku serve
  ```

### Custom logo

- **Auto-detect (easiest):** drop a `logo.*` (or `favicon.*`) file into `public/` — `logo.svg`, `logo.png`, `logo.webp`, `logo.jpg`, `logo.gif`, `logo.ico` are detected in that priority order, no config needed. Restart and it appears in the header, drawer, and as the favicon.
- **Manual:** set `logo_url = "/brand.png"` (file in `public/`) or an absolute URL. Manual value always wins over auto-detect.

### Static files & verification (`public/`)

Anything in `static_dir` (default `public/`) is served at the site root for a single path segment — e.g. `public/google1234.html` → `https://your-domain/google1234.html`. Use it for `ads.txt` / `app-ads.txt`, search-engine verification files, `robots.txt`, `sitemap.xml`, favicons, and logos. Path traversal is rejected, and API / SPA / proxy routes always take precedence. See [`public/README.md`](public/README.md) for the full guide.

---

## Deployment

The reference deployment runs on AWS at <https://api.risqinf.web.id>. Architecture:

- **Compute:** EC2 instance (`apiku serve --bind 127.0.0.1:3000` behind systemd)
- **Reverse proxy:** nginx terminates TLS, forwards `/` and `/api/v1/*` and `/img` to the local apiku
- **TLS:** Let's Encrypt via Certbot, auto-renewed
- **Domain:** `api.risqinf.web.id` → AWS via Route 53 / external DNS provider
- **Logs:** systemd journald (`apiku --log-format json --log-file /var/log/apiku/apiku.log`)
- **Image proxy:** the host allowlist already covers every upstream CDN we use, so no extra firewall rules are needed

### Suggested systemd unit

```ini
# /etc/systemd/system/apiku.service
[Unit]
Description=apiku - RESTful scraping API
After=network.target

[Service]
User=apiku
Group=apiku
Environment=APIKU_SECRET=<long-random-secret>
ExecStart=/usr/local/bin/apiku serve --bind 127.0.0.1:3000 --log info --log-format json
Restart=on-failure
RestartSec=2s
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

### Suggested nginx site

```nginx
server {
    listen 443 ssl http2;
    server_name api.risqinf.web.id;

    ssl_certificate     /etc/letsencrypt/live/api.risqinf.web.id/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.risqinf.web.id/privkey.pem;

    # Long-lived image-proxy responses can be safely cached at the edge
    location /img {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_buffering on;
        proxy_cache_valid 200 1d;
    }

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }
}
```

Hardening that the live deployment uses (and you should consider too):

- `APIKU_SECRET` env set to a 64+ character random value so opaque IDs survive restarts
- nginx-level rate limiting (`limit_req_zone`) on `/api/v1/search`
- IP allowlist on `/api/v1/info` if you don't want server tuning info public
- Cloudflare (or another CDN) in front for DDoS protection and TLS termination if you prefer

### Continuous delivery

Pushing a tag like `v0.3.0` runs `.github/workflows/release.yml`, which:

1. Cross-builds the binary for 7 targets (Linux x86_64 glibc + musl, Linux ARM64, macOS Intel + Apple Silicon, Windows x86_64 + ARM64) using `cross` for non-native ones
2. Strips the binary on Unix targets
3. Packs each target as `apiku-vX.Y.Z-<target>.{tar.gz,zip}` together with `README.md`, `LICENSE`, and `config.toml`
4. Generates a per-archive `.sha256` plus a combined `SHA256SUMS`
5. Publishes a GitHub Release with auto-generated notes

`.github/workflows/ci.yml` runs on every push and PR (Linux, macOS, Windows) and gates merges on `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build`, and `cargo test`.

To cut a release locally:

```bash
git tag v0.3.0
git push origin v0.3.0
# Or: trigger the "Release" workflow manually from the Actions tab
```

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
├── tester.rs          Developer API console (maud), embedded CSS/JS
├── tester.css         API console stylesheet (compiled in)
├── tester.js          API console client-side script (compiled in)
├── webapp.rs          Consumer streaming/reading SPA shell
├── app.css            Web app stylesheet (compiled in)
├── app.js             Web app SPA router + views (compiled in)
├── opaque.rs          HMAC-SHA256 opaque ID + image-proxy signing
├── cossora.rs         Cosplaytele video resolver (AES-256-CBC decrypt of cossora.stream embeds -> HLS)
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
    ├── otakudesu.rs   Otakudesu anime streaming adapter (HTML + AJAX mirror resolver + genre feeds)
    ├── cosplaytele.rs Cosplaytele cosplay archive adapter (HTML + categories)
    ├── nhentai.rs     nhentai doujinshi adapter (JSON API + sharded CDN + popular feeds)
    └── novelid.rs     NovelID Indonesian novel adapter (HTML, upstream-paginated chapter lists)
```

---

## Roadmap

Planned work, roughly in priority order. Contributions and suggestions welcome via [issues](https://github.com/risqinf/apiku/issues).

### Next up

- [ ] **Nekopoi** — add the Nekopoi provider (adult anime / hentai streaming) behind the 18+ toggle, reusing the existing HLS resolver + episode/mirror player pipeline. **This is the next major target.**

### Backlog

- [ ] More providers (additional manga / donghua / novel sources) behind the same envelope.
- [ ] Watch/read history & resume (client-side, then optional sync).
- [ ] Favorites / bookmarks with import-export.
- [ ] Server-rendered meta tags per detail page for richer link previews & SEO.
- [ ] Optional API-key / rate-tier layer for public deployments.
- [ ] PWA: offline shell + installable web app.
- [ ] More language samples and an OpenAPI spec for the Explorer.

### Done

- [x] **Stream Anime (Otakudesu)** — full anime streaming provider: search, rich detail metadata, episode list, quality-grouped streaming mirrors resolved on demand, downloads, and genre feeds.
- [x] Cosplay video playback via server-side HLS resolution (no iframe embeds).
- [x] High-precision Cosplaytele search + clickable cosplayer/tag pills.
- [x] Relevance-ranked cross-provider search with per-source filter counts.
- [x] Configurable branding (name/tagline/logo/footer/ads) via config + env, with logo auto-detection.
- [x] Reworked API Explorer with grouped endpoints and copy-ready multi-language samples.
- [x] Modern UI: animated background, real-time toggle switches, light/dark theme, overflow nav menu.
- [x] Full episode lists for donghua, language-grouped manga chapters, NovelID upstream pagination.

---

## License

MIT
