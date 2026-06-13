//! Colored, human-friendly logging in the spirit of Go's `slog`/`zap`.
//!
//! This module configures `tracing-subscriber` with a custom event formatter
//! that produces aligned, ANSI-coloured output:
//!
//!     2026-05-25T19:06:27Z  INFO  apiku::server   API listening on http://127.0.0.1:3000
//!     2026-05-25T19:06:28Z  WARN  apiku::engine   rate limit hit, retrying in 1000ms
//!
//! Levels use the same colour palette as `nu-ansi-term`. JSON mode is
//! available for production / log aggregation pipelines.

use nu_ansi_term::{Color, Style};
use std::fmt;
use std::io::IsTerminal;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{FmtSpan, Writer};
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

/// Human-readable, coloured log format
struct PrettyFormat {
    use_colors: bool,
    timer: SystemTime,
}

impl<S, N> FormatEvent<S, N> for PrettyFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let level = meta.level();
        let target = meta.target();

        // Timestamp (dim grey)
        if self.use_colors {
            write!(writer, "{}", Color::DarkGray.prefix())?;
        }
        self.timer.format_time(&mut writer)?;
        if self.use_colors {
            write!(writer, "{}", Color::DarkGray.suffix())?;
        }
        write!(writer, " ")?;

        // Level — coloured + 5-char padded
        let (level_str, level_style) = level_paint(*level);
        if self.use_colors {
            write!(
                writer,
                "{}{:<5}{}",
                level_style.prefix(),
                level_str,
                level_style.suffix()
            )?;
        } else {
            write!(writer, "{:<5}", level_str)?;
        }
        write!(writer, " ")?;

        // Target — cyan
        let target_short = shorten_target(target);
        if self.use_colors {
            write!(
                writer,
                "{}{:<24}{}",
                Color::Cyan.prefix(),
                target_short,
                Color::Cyan.suffix()
            )?;
        } else {
            write!(writer, "{:<24}", target_short)?;
        }
        write!(writer, " ")?;

        // Span context (e.g. `request_id=...`)
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let ext = span.extensions();
                if let Some(fields) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    if !fields.is_empty() {
                        if self.use_colors {
                            write!(
                                writer,
                                "{}{}{} ",
                                Color::DarkGray.prefix(),
                                fields,
                                Color::DarkGray.suffix()
                            )?;
                        } else {
                            write!(writer, "{} ", fields)?;
                        }
                    }
                }
            }
        }

        // Event fields (the message body)
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn level_paint(level: Level) -> (&'static str, Style) {
    match level {
        Level::ERROR => ("ERROR", Color::Red.bold()),
        Level::WARN => ("WARN", Color::Yellow.bold()),
        Level::INFO => ("INFO", Color::Green.bold()),
        Level::DEBUG => ("DEBUG", Color::Blue.bold()),
        Level::TRACE => ("TRACE", Color::Purple.bold()),
    }
}

/// Trim long targets like `apiku::server::handlers` -> `apiku::server`
fn shorten_target(target: &str) -> String {
    let parts: Vec<&str> = target.split("::").collect();
    if parts.len() <= 2 {
        target.to_string()
    } else {
        format!("{}::{}", parts[0], parts[1])
    }
}

/// Output format selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Coloured, human-readable (default for terminals)
    Pretty,
    /// JSON one-line per event (machine-readable)
    Json,
    /// Compact uncoloured
    Compact,
}

impl LogFormat {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => Self::Json,
            "compact" => Self::Compact,
            _ => Self::Pretty,
        }
    }
}

/// Initialise global tracing subscriber.
///
/// `level` accepts: error / warn / info / debug / trace
/// `format` accepts: pretty / json / compact
/// `log_file`: optional path; when set, logs are tee'd to the file too.
pub fn init(
    level: &str,
    format: LogFormat,
    log_file: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let level_filter = match level.to_lowercase().as_str() {
        "error" => "error",
        "warn" => "warn",
        "info" => "info",
        "debug" => "debug",
        "trace" => "trace",
        _ => "info",
    };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "apiku={},reqwest=warn,tower_http::trace::on_failure=off,tower_http=info",
            level_filter
        ))
    });

    let use_colors = std::io::stderr().is_terminal();

    // Optional file appender (no rotation — keeps it simple)
    let file_writer = if let Some(p) = log_file {
        let path = p.to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?,
        )
    } else {
        None
    };

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let registry = tracing_subscriber::registry().with(env_filter);

    match format {
        LogFormat::Json => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr);
            if let Some(f) = file_writer {
                registry
                    .with(stderr_layer)
                    .with(tracing_subscriber::fmt::layer().json().with_writer(f))
                    .try_init()?;
            } else {
                registry.with(stderr_layer).try_init()?;
            }
        }
        LogFormat::Compact => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .compact()
                .with_ansi(use_colors)
                .with_writer(std::io::stderr);
            if let Some(f) = file_writer {
                registry
                    .with(stderr_layer)
                    .with(
                        tracing_subscriber::fmt::layer()
                            .compact()
                            .with_ansi(false)
                            .with_writer(f),
                    )
                    .try_init()?;
            } else {
                registry.with(stderr_layer).try_init()?;
            }
        }
        LogFormat::Pretty => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(use_colors)
                .with_span_events(FmtSpan::NONE)
                .event_format(PrettyFormat {
                    use_colors,
                    timer: SystemTime,
                });
            if let Some(f) = file_writer {
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_writer(f)
                    .with_ansi(false)
                    .event_format(PrettyFormat {
                        use_colors: false,
                        timer: SystemTime,
                    });
                registry.with(stderr_layer).with(file_layer).try_init()?;
            } else {
                registry.with(stderr_layer).try_init()?;
            }
        }
    }

    Ok(())
}

/// Banner printed at startup. Uses colours when stderr is a TTY.
pub fn banner(addr: &std::net::SocketAddr, sysspec: &crate::sysspec::SysSpec) {
    let use_colors = std::io::stderr().is_terminal();
    let bold = if use_colors {
        Color::Cyan.bold()
    } else {
        Style::new()
    };
    let dim = if use_colors {
        Color::DarkGray.normal()
    } else {
        Style::new()
    };
    let info = if use_colors {
        Color::Green.normal()
    } else {
        Style::new()
    };

    eprintln!();
    eprintln!(
        "{}",
        bold.paint("  apiku v".to_string() + env!("CARGO_PKG_VERSION"))
    );
    eprintln!("{}", dim.paint("  RESTful scraping API"));
    eprintln!();
    eprintln!("  {}  http://{}", info.paint("Listening "), addr);
    eprintln!("  {}  http://{}/", info.paint("Web app    "), addr);
    eprintln!("  {}  http://{}/tester", info.paint("API console"), addr);
    eprintln!(
        "  {}  http://{}/api/v1/health",
        info.paint("Health     "),
        addr
    );
    eprintln!(
        "  {}  http://{}/api/v1/info",
        info.paint("Info       "),
        addr
    );
    eprintln!();
    eprintln!(
        "  {}  {} cores, {} MiB RAM, profile: {}",
        dim.paint("System    "),
        sysspec.cpu_cores,
        sysspec.total_mem_mib,
        sysspec.profile()
    );
    eprintln!(
        "  {}  threads={}, http_concurrency={}, scrape_cache={}",
        dim.paint("Runtime   "),
        sysspec.worker_threads(),
        sysspec.http_concurrency(),
        sysspec.scrape_cache_capacity()
    );
    eprintln!();
}
