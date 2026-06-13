//! Library surface for `apiku`.
//!
//! The crate ships primarily as a binary (`src/main.rs`), but a handful of
//! self-contained, performance-critical modules are also re-exported here so
//! they can be exercised by integration benchmarks (`benches/`) without
//! pulling in the full HTTP/runtime stack.
//!
//! Only modules with no internal cross-module dependencies are exposed:
//!
//! - [`opaque`]      — HMAC-SHA256 opaque ID codec (runs on every resource ID
//!   and every proxied image/HLS URL).
//! - [`fingerprint`] — deterministic per-URL browser fingerprint / header set
//!   (runs on every upstream request).
//! - [`parser`]      — `scraper`-backed HTML extraction (the core of every
//!   adapter's scrape path).

pub mod fingerprint;
pub mod opaque;
pub mod parser;
