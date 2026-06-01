//! Web-serving layer.
//!
//! Everything that turns the core scraping engine into an HTTP product lives
//! here, kept separate from the engine internals (`crate::engine` and
//! friends) and the per-site adapters (`crate::adapters`):
//!
//!   - `server`  - axum router, middleware, static-file serving
//!   - `api`     - REST handlers, DTOs, opaque-ID plumbing, image/HLS proxy
//!   - `webapp`  - consumer SPA shell (serves `assets/webapp/*`)
//!   - `tester`  - developer API console (serves `assets/tester/*`)
//!   - `search`  - cross-provider search parsers + ranking
//!   - `cossora` - cossora.stream embed resolver (cosplay video)

pub mod api;
pub mod cossora;
pub mod search;
pub mod server;
pub mod tester;
pub mod webapp;
