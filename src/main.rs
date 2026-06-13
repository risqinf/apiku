//! apiku - main entry point.
//!
//! Wires the CLI together: argument parsing, log setup, runtime construction,
//! and dispatch to one of the subcommands (`serve` / `scrape` / `batch` /
//! `info`). The web-serving layer lives in `web` (HTTP server in
//! `web::server`, all REST handlers in `web::api`, the consumer SPA in
//! `web::webapp`, the developer tester website in `web::tester`).
//!
//! mimalloc is registered as the global allocator for faster allocation in
//! concurrent scraping workloads.

mod adapters;
mod config;
mod deep_extractor;
mod engine;
mod error;
mod fingerprint;
mod log;
mod models;
mod opaque;
mod parser;
mod pipeline;
mod rate_limiter;
mod retry;
mod sysspec;
mod web;

use clap::{Parser as ClapParser, Subcommand};
use config::AppConfig;
use engine::{CleanLevel, ScrapeOptions, ScraperEngine};
use std::path::PathBuf;
use sysspec::SysSpec;
use tracing::info;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const APP_LONG_ABOUT: &str = "\
apiku - RESTful scraping API for Mangaball, Anichin, Cosplaytele, nhentai, and NovelID.\n\
\n\
Examples:\n\
    apiku serve                         Start the API server (recommended)\n\
    apiku serve --bind 0.0.0.0:8080     Bind to all interfaces\n\
    apiku serve --log debug             Verbose logging\n\
    apiku serve --log-file apiku.log    Tee logs to file\n\
    apiku scrape --url <URL> --stdout   One-off scrape via CLI\n\
    apiku batch --file urls.txt         Batch scrape from a URL list\n\
";

#[derive(ClapParser, Debug)]
#[command(
    name = "apiku",
    version,
    author = "risqinf",
    about = "RESTful scraping API for Mangaball, Anichin, Cosplaytele, nhentai, and NovelID",
    long_about = APP_LONG_ABOUT
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to TOML configuration file
    #[arg(short, long, default_value = "config.toml", global = true)]
    config: PathBuf,

    /// Single target URL (CLI mode)
    #[arg(short, long, global = true)]
    url: Option<String>,

    /// Multiple target URLs (CLI mode, repeatable)
    #[arg(long = "urls", value_name = "URL", global = true)]
    urls: Vec<String>,

    /// Output file path (CLI mode). Use "-" for stdout
    #[arg(short, long, global = true)]
    output: Option<PathBuf>,

    /// Write JSON output to stdout (CLI mode)
    #[arg(long, default_value_t = false, global = true)]
    stdout: bool,

    /// Log level: error, warn, info, debug, trace
    #[arg(
        long = "log",
        default_value = "info",
        value_name = "LEVEL",
        global = true
    )]
    log_level: String,

    /// Log output format: pretty, json, compact
    #[arg(
        long = "log-format",
        default_value = "pretty",
        value_name = "FORMAT",
        global = true
    )]
    log_format: String,

    /// Tee logs to a file (in addition to stderr)
    #[arg(long = "log-file", value_name = "PATH", global = true)]
    log_file: Option<PathBuf>,

    /// Override engine concurrency (1-100)
    #[arg(long, global = true)]
    concurrency: Option<usize>,

    /// Override request timeout in seconds (1-300)
    #[arg(long, global = true)]
    timeout: Option<u64>,

    /// Override default rate-limit delay in milliseconds
    #[arg(long, global = true)]
    rate_limit: Option<u64>,

    /// Override max retry attempts
    #[arg(long, global = true)]
    max_retries: Option<u32>,

    /// JSON indentation spaces (0-8). Use 0 for compact JSON
    #[arg(long, global = true)]
    indent: Option<u8>,

    /// Override User-Agent header
    #[arg(long, value_name = "STRING", global = true)]
    user_agent: Option<String>,

    /// Override Referer header
    #[arg(long, value_name = "URL", global = true)]
    referer: Option<String>,

    /// Add custom HTTP header (format: 'Name: Value'). Repeatable
    #[arg(short = 'H', long = "header", value_name = "HEADER", global = true)]
    headers: Vec<String>,

    /// Skip deep page extraction (faster, smaller responses)
    #[arg(long, default_value_t = false, global = true)]
    no_deep: bool,

    /// Print summary to stderr after CLI scrape
    #[arg(long, default_value_t = false, global = true)]
    summary: bool,

    /// Output cleaning preset: none, clean, minimal
    #[arg(long, default_value = "none", value_name = "MODE", global = true)]
    clean: String,

    /// Output bare content object (CLI). Implies --clean clean
    #[arg(long, default_value_t = false, global = true)]
    flat: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scrape one or more URLs (CLI mode)
    Scrape,

    /// Read URLs from a file and scrape them all (CLI mode)
    Batch {
        /// Path to a text file with one URL per line
        #[arg(short, long, value_name = "FILE")]
        file: PathBuf,

        /// Directory to write per-URL JSON files
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,
    },

    /// Print version and adapter list
    Info,

    /// Run as an HTTP API server (recommended)
    Serve {
        /// Address to bind (default 127.0.0.1:3000)
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Logging
    let format = log::LogFormat::parse(&cli.log_format);
    log::init(&cli.log_level, format, cli.log_file.as_deref())?;

    // System spec
    let sysspec = SysSpec::detect();
    let worker_threads = sysspec.worker_threads();

    // Multi-threaded Tokio runtime sized to host CPU
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .thread_name("apiku-worker")
        .build()?;

    info!(
        cores = sysspec.cpu_cores,
        ram_mib = sysspec.total_mem_mib,
        threads = worker_threads,
        profile = sysspec.profile(),
        "system detected"
    );

    runtime.block_on(async_main(cli, sysspec))
}

async fn async_main(cli: Cli, sysspec: SysSpec) -> anyhow::Result<()> {
    match cli.command.as_ref() {
        Some(Commands::Info) => return handle_info(),
        Some(Commands::Batch { file, output_dir }) => {
            return handle_batch(file, output_dir.as_deref(), &cli).await
        }
        Some(Commands::Serve { bind }) => return handle_serve(bind, &cli, sysspec).await,
        Some(Commands::Scrape) | None => {}
    }
    handle_scrape(&cli).await
}

fn build_config(cli: &Cli) -> anyhow::Result<AppConfig> {
    let mut config = if cli.config.exists() {
        AppConfig::from_file(&cli.config)?
    } else {
        AppConfig::default()
    };

    if let Some(c) = cli.concurrency {
        config.concurrency = c;
    }
    if let Some(t) = cli.timeout {
        config.timeout_secs = t;
    }
    if let Some(r) = cli.rate_limit {
        config.rate_limit_ms = r;
    }
    if let Some(r) = cli.max_retries {
        config.max_retries = r;
    }
    if let Some(i) = cli.indent {
        config.indent = i;
    }
    if let Some(ua) = &cli.user_agent {
        config.headers.insert("User-Agent".to_string(), ua.clone());
    }
    if let Some(r) = &cli.referer {
        config.headers.insert("Referer".to_string(), r.clone());
    }
    for h in &cli.headers {
        if let Some((name, value)) = h.split_once(':') {
            config
                .headers
                .insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    config.validate()?;
    Ok(config)
}

fn collect_urls(cli: &Cli, config: &AppConfig) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(u) = &cli.url {
        urls.push(u.clone());
    }
    urls.extend(cli.urls.clone());
    if urls.is_empty() {
        urls = config.targets.clone();
    }
    urls
}

fn resolve_output_path(cli: &Cli, config: &AppConfig) -> Option<PathBuf> {
    if cli.stdout {
        return None;
    }
    if let Some(o) = &cli.output {
        if o.to_string_lossy() == "-" {
            return None;
        }
        return Some(o.clone());
    }
    Some(PathBuf::from(&config.output_path))
}

async fn handle_scrape(cli: &Cli) -> anyhow::Result<()> {
    let config = build_config(cli)?;
    let urls = collect_urls(cli, &config);
    if urls.is_empty() {
        anyhow::bail!("No URLs to scrape. Provide --url, --urls, or set targets in config.");
    }
    let output_path = resolve_output_path(cli, &config);

    info!(count = urls.len(), "starting scrape");
    let start = std::time::Instant::now();

    let options = ScrapeOptions {
        follow_api: false,
        max_followed_apis: 0,
        deep_only: false,
        no_deep: cli.no_deep,
        clean: parse_clean_level(&cli.clean, cli.flat),
    };
    let indent = config.indent;
    let engine = ScraperEngine::with_options(config, options)?;
    let results = engine.scrape_all(&urls).await?;
    write_results(&results, &output_path, indent, cli.flat)?;
    if cli.summary || output_path.is_some() {
        print_summary(&results);
    }
    info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        "scrape done"
    );
    Ok(())
}

async fn handle_batch(
    file: &PathBuf,
    output_dir: Option<&std::path::Path>,
    cli: &Cli,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file)?;
    let urls: Vec<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if urls.is_empty() {
        anyhow::bail!("No URLs found in {}", file.display());
    }
    info!(count = urls.len(), file = %file.display(), "starting batch scrape");

    let config = build_config(cli)?;
    let options = ScrapeOptions {
        follow_api: false,
        max_followed_apis: 0,
        deep_only: false,
        no_deep: cli.no_deep,
        clean: parse_clean_level(&cli.clean, cli.flat),
    };
    let indent = config.indent;
    let engine = ScraperEngine::with_options(config, options)?;
    let results = engine.scrape_all(&urls).await?;

    if let Some(dir) = output_dir {
        std::fs::create_dir_all(dir)?;
        for (i, r) in results.iter().enumerate() {
            let name = format!("{:04}_{}.json", i + 1, sanitize_filename(&r.url));
            let path = dir.join(name);
            let json = if indent == 0 {
                serde_json::to_string(r)?
            } else {
                serde_json::to_string_pretty(r)?
            };
            std::fs::write(&path, json)?;
        }
        info!(count = results.len(), dir = %dir.display(), "batch done");
    } else {
        let output_path = resolve_output_path(cli, &AppConfig::default());
        write_results(&results, &output_path, indent, cli.flat)?;
    }
    print_summary(&results);
    Ok(())
}

fn handle_info() -> anyhow::Result<()> {
    println!("apiku v{}", env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    println!("Built-in adapters:");
    println!("  mangaball     - manga, manhwa, manhua (global database)");
    println!("  anichin       - donghua streaming with Indonesian subs");
    println!("  cosplaytele   - cosplay photoset archive");
    println!("  nhentai       - doujinshi catalogue (multi-mirror, browser-spoofed)");
    println!("  novelid       - Indonesian novel translations (text body)");
    println!();
    println!("Run `apiku serve` to start the API server.");
    Ok(())
}

async fn handle_serve(bind: &str, cli: &Cli, sysspec: SysSpec) -> anyhow::Result<()> {
    let mut config = build_config(cli).unwrap_or_else(|_| AppConfig::default());
    config.concurrency = sysspec.http_concurrency();
    if config.rate_limit_ms == 1000 {
        config.rate_limit_ms = 400;
    }

    // Capture web/branding config before the engine takes ownership of `config`.
    let mut web_config = config.web.clone();

    // Environment-variable overrides (handy for Docker / systemd deployments
    // where editing config.toml is awkward). Any of these, when set and
    // non-empty, wins over the config file — so the site can be rebranded
    // entirely from outside the binary AND outside the config file.
    apply_web_env_overrides(&mut web_config);

    let static_dir = web_config.static_dir.clone();

    // Auto-detect a custom logo when `logo_url` isn't set: look for a
    // `logo.*` / `favicon.*` image dropped into the static dir, in priority
    // order. Lets operators just drop `public/logo.png` (or .svg/.jpg/...)
    // without editing config.
    if web_config.logo_url.trim().is_empty() {
        if let Some(found) = detect_logo(&static_dir) {
            tracing::info!(logo = %found, "auto-detected custom logo from static dir");
            web_config.logo_url = found;
        }
    }

    let options = ScrapeOptions {
        follow_api: false,
        max_followed_apis: 0,
        deep_only: false,
        no_deep: true,
        clean: CleanLevel::None,
    };
    let engine = ScraperEngine::with_options(config, options)?;
    let codec = opaque::OpaqueCodec::from_env_or_persisted();
    let state = web::api::ApiState::new(engine, codec, sysspec, web_config);

    let addr: std::net::SocketAddr = bind
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind '{}': {}", bind, e))?;

    log::banner(&addr, &sysspec);
    web::server::run(state, addr, &static_dir).await
}

/// Apply environment-variable overrides to the web/branding config.
///
/// Lets operators rebrand from outside both the binary and `config.toml`
/// (e.g. `APIKU_SITE_NAME="MySite" apiku serve`). Empty / unset vars are
/// ignored, so the config-file value remains.
fn apply_web_env_overrides(web: &mut config::WebConfig) {
    fn env_nonempty(key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.trim().is_empty())
    }
    if let Some(v) = env_nonempty("APIKU_SITE_NAME") {
        web.site_name = v;
    }
    if let Some(v) = env_nonempty("APIKU_TAGLINE") {
        web.tagline = v;
    }
    if let Some(v) = env_nonempty("APIKU_LOGO_URL") {
        web.logo_url = v;
    }
    if let Some(v) = env_nonempty("APIKU_STATIC_DIR") {
        web.static_dir = v;
    }
}

/// Auto-detect a custom logo file in the static directory.
///
/// Looks for `logo.*` then `favicon.*` with common image extensions, in
/// priority order, and returns its root path (e.g. `/logo.png`) so it can be
/// used as `logo_url`. Returns `None` if nothing matches.
fn detect_logo(static_dir: &str) -> Option<String> {
    let stems = ["logo", "favicon"];
    // SVG first (sharpest), then raster formats.
    let exts = ["svg", "png", "webp", "jpg", "jpeg", "gif", "ico"];
    for stem in stems {
        for ext in exts {
            let name = format!("{}.{}", stem, ext);
            let path = std::path::Path::new(static_dir).join(&name);
            if path.is_file() {
                return Some(format!("/{}", name));
            }
        }
    }
    None
}

fn parse_clean_level(s: &str, flat: bool) -> CleanLevel {
    if flat {
        return CleanLevel::Clean;
    }
    match s.to_lowercase().as_str() {
        "clean" => CleanLevel::Clean,
        "minimal" => CleanLevel::Minimal,
        _ => CleanLevel::None,
    }
}

fn serialize_value(v: &serde_json::Value, indent: u8) -> anyhow::Result<String> {
    if indent == 0 {
        Ok(serde_json::to_string(v)?)
    } else {
        let buf = Vec::new();
        let indent_str = " ".repeat(indent as usize);
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent_str.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(buf, formatter);
        serde::Serialize::serialize(v, &mut ser)?;
        Ok(String::from_utf8(ser.into_inner())?)
    }
}

fn bare_content(r: &crate::models::ScrapeResult) -> serde_json::Value {
    if let Some(content) = &r.content {
        let mut value = serde_json::to_value(content).unwrap_or(serde_json::Value::Null);
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "source_url".to_string(),
                serde_json::Value::String(r.url.clone()),
            );
        }
        value
    } else {
        serde_json::json!({
            "source_url": r.url,
            "success": r.success,
            "error": r.error,
        })
    }
}

fn write_results(
    results: &[crate::models::ScrapeResult],
    output_path: &Option<PathBuf>,
    indent: u8,
    flat: bool,
) -> anyhow::Result<()> {
    let value: serde_json::Value = if flat {
        let arr: Vec<serde_json::Value> = results.iter().map(bare_content).collect();
        if arr.len() == 1 {
            arr.into_iter().next().unwrap()
        } else {
            serde_json::Value::Array(arr)
        }
    } else {
        serde_json::to_value(results)?
    };

    let json = serialize_value(&value, indent)?;

    match output_path {
        Some(path) => {
            std::fs::write(path, &json)?;
            info!(path = %path.display(), "wrote output");
        }
        None => println!("{}", json),
    }
    Ok(())
}

fn print_summary(results: &[crate::models::ScrapeResult]) {
    let success = results.iter().filter(|r| r.success).count();
    let fail = results.len() - success;
    eprintln!();
    eprintln!("---");
    eprintln!(
        " Total: {}, OK: {}, Failed: {}",
        results.len(),
        success,
        fail
    );
    eprintln!("---");
}

fn sanitize_filename(url: &str) -> String {
    url.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}
