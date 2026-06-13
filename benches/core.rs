//! Core hot-path micro-benchmarks for `apiku`.
//!
//! These cover the work done on (almost) every request:
//!
//! - opaque ID encode / decode (HMAC-SHA256) — every resource ID in every
//!   response is signed; every incoming ID is verified + decoded.
//! - image-proxy URL sign / verify — every image and HLS segment URL.
//! - per-URL browser fingerprint + header-map build — every upstream fetch.
//! - HTML parse + CSS selection — the core of every adapter's scrape.
//!
//! Run with `cargo bench`. Results land in `target/criterion/`.

use apiku::fingerprint::BrowserFingerprint;
use apiku::opaque::{Kind, OpaqueCodec, Source};
use apiku::parser::HtmlParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

const SAMPLE_URL: &str = "https://mangaball.net/title-detail/one-piece-0a1b2c3d4e5f60718293a4b5/";

/// Build a representative listing page: 30 cards with lazy-loaded images,
/// titles, links and a chapter badge — the shape adapters scrape from a feed.
fn sample_listing_html() -> String {
    let mut s = String::with_capacity(16 * 1024);
    s.push_str(
        "<!doctype html><html><head><title>Latest</title></head><body><div class=\"listupd\">",
    );
    for i in 0..30 {
        s.push_str(&format!(
            "<div class=\"bs\"><div class=\"bsx\"><a href=\"https://example.com/series/title-{i}/\" title=\"Sample Title {i}\">\
             <div class=\"limit\"><img class=\"ts-post-image\" data-src=\"https://cdn.example.com/covers/{i}.jpg\" \
             src=\"https://example.com/wp-content/loading.gif\" alt=\"Sample Title {i}\"></div>\
             <div class=\"bigor\"><div class=\"tt\">Sample Title {i}</div>\
             <div class=\"epxs\">Chapter {ch}</div></div></a></div></div>",
            i = i,
            ch = i * 3 + 1,
        ));
    }
    s.push_str("</div></body></html>");
    s
}

fn bench_opaque(c: &mut Criterion) {
    let codec = OpaqueCodec::new(b"benchmark-secret-key-stable".to_vec());
    let encoded = codec.encode(Source::Mangaball, Kind::Series, SAMPLE_URL);
    let img_payload = "aHR0cHM6Ly9jZG4uZXhhbXBsZS5jb20vY292ZXJzLzQyLmpwZw";
    let img_sig = codec.sign_image(img_payload);

    let mut g = c.benchmark_group("opaque");
    g.throughput(Throughput::Elements(1));
    g.bench_function("encode", |b| {
        b.iter(|| {
            codec.encode(
                black_box(Source::Mangaball),
                black_box(Kind::Series),
                black_box(SAMPLE_URL),
            )
        })
    });
    g.bench_function("decode", |b| {
        b.iter(|| codec.decode(black_box(&encoded)).unwrap())
    });
    g.bench_function("sign_image", |b| {
        b.iter(|| codec.sign_image(black_box(img_payload)))
    });
    g.bench_function("verify_image", |b| {
        b.iter(|| codec.verify_image(black_box(img_payload), black_box(&img_sig)))
    });
    g.finish();
}

fn bench_fingerprint(c: &mut Criterion) {
    let mut g = c.benchmark_group("fingerprint");
    g.throughput(Throughput::Elements(1));
    g.bench_function("for_url", |b| {
        b.iter(|| BrowserFingerprint::for_url(black_box(SAMPLE_URL)))
    });
    g.bench_function("for_url+header_map", |b| {
        b.iter(|| {
            let fp = BrowserFingerprint::for_url(black_box(SAMPLE_URL));
            black_box(fp.as_header_map())
        })
    });
    g.finish();
}

fn bench_parser(c: &mut Criterion) {
    let html = sample_listing_html();
    let mut g = c.benchmark_group("parser");
    g.throughput(Throughput::Bytes(html.len() as u64));
    g.bench_function("parse_document", |b| {
        b.iter(|| black_box(HtmlParser::parse(black_box(&html))))
    });
    g.bench_function("parse+extract_cards", |b| {
        b.iter(|| {
            let p = HtmlParser::parse(black_box(&html));
            let titles = p.texts(".tt");
            let links = p.attrs(".bsx a", "href");
            let imgs = p.image_urls(".ts-post-image");
            black_box((titles.len(), links.len(), imgs.len()))
        })
    });
    g.finish();
}

criterion_group!(benches, bench_opaque, bench_fingerprint, bench_parser);
criterion_main!(benches);
