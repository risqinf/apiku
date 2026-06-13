# Benchmarks

Performance numbers for the hot paths that run on (almost) every request, plus
end-to-end HTTP throughput of the running server.

All figures below were measured on the environment in the table — treat them as
**orders of magnitude**, not guarantees. Re-run them on your own hardware with
the commands in [Reproducing](#reproducing).

## Environment

| | |
|---|---|
| CPU | AMD Ryzen 3 7320U (4 cores / 8 threads) |
| RAM | 7.0 GiB |
| OS | Fedora Linux 44, kernel 6.19 |
| Toolchain | rustc 1.96.0, `--release` (`opt-level = 3`, thin LTO) |
| Allocator | mimalloc |

## Micro-benchmarks

[Criterion](https://github.com/bheisler/criterion.rs) benchmarks of the
self-contained hot-path modules (`benches/core.rs`). Median of 100 samples.

| Path | Operation | Median | Ops/sec |
|---|---|---:|---:|
| **Opaque ID** | `encode` (sign new ID) | 952 ns | ~1.05 M/s |
| | `decode` (verify + parse) | 570 ns | ~1.75 M/s |
| | `sign_image` (proxy URL) | 307 ns | ~3.26 M/s |
| | `verify_image` | 325 ns | ~3.08 M/s |
| **Fingerprint** | `for_url` (pick identity) | 110 ns | ~9.08 M/s |
| | `for_url` + `as_header_map` | 1.24 µs | ~0.81 M/s |
| **HTML parser** | parse 30-card listing (~14 KiB) | 404 µs | ~2,475/s |
| | parse + extract titles/links/images | 444 µs | ~2,255/s |

Takeaways:

- The crypto envelope (opaque IDs + image-URL signing) is effectively free:
  signing every ID and image URL in a response costs sub-microsecond per item.
- Header/fingerprint construction is negligible against any network round-trip.
- HTML parsing dominates per-scrape CPU cost, as expected — the `scraper`
  (html5ever) parse of a full listing page is ~0.4 ms; selector extraction adds
  only ~40 µs on top. Scrape latency is therefore network-bound, not CPU-bound.

## End-to-end HTTP

Release server, measured with [`oha`](https://github.com/hatoo/oha) at
concurrency 50.

### Health endpoint (`/api/v1/health`) — pure server-stack overhead, no upstream

| Metric | Value |
|---|---|
| Throughput | **15,270 req/s** |
| Average | 3.27 ms |
| p50 | 2.80 ms |
| p99 | 12.2 ms |
| Success rate | 100% |

### Image proxy (`/img`) — warm in-memory (moka) cache hit

| Metric | Value |
|---|---|
| Throughput | **24,647 req/s** |
| Data rate | 1.10 GiB/s |
| Average | 2.02 ms |
| p50 | 1.74 ms |
| p99 | 6.59 ms |
| Cold prime (single upstream fetch) | ~124 ms* |

\* Cold latency is whatever the upstream CDN takes (observed 0.1 s on fast
sources, up to several seconds on slow ones). The first request primes the
cache; every subsequent hit for the same image is served from memory at the
warm numbers above — a ~70× speed-up on this upstream, and far larger on slow
ones. This is what keeps the gallery-heavy front page responsive.

## Reproducing

```bash
# Micro-benchmarks (Criterion). HTML report: target/criterion/report/index.html
cargo bench

# Run a single group
cargo bench --bench core -- opaque

# End-to-end HTTP (needs `oha`: cargo install oha)
APIKU_SECRET=devtest cargo run --release -- serve --bind 127.0.0.1:3000 &
oha -z 8s -c 50 --no-tui http://127.0.0.1:3000/api/v1/health
```

> Numbers vary with CPU, upstream network, and cache state. The micro-benchmarks
> are deterministic; the HTTP throughput depends on your client and machine.
