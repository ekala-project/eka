//! Handles logging and progress bars for the CLI.

use std::io::IsTerminal;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_indicatif::IndicatifLayer;
use tracing_indicatif::style::ProgressStyle;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, fmt};

use super::{Args, LogArgs};

//================================================================================================
// Statics
//================================================================================================

/// A flag to indicate whether ANSI escape codes should be used.
pub static ANSI: AtomicBool = AtomicBool::new(true);

//================================================================================================
// Modules
//================================================================================================

/// Contains ANSI escape codes for styling terminal output.
pub mod ansi {
    /// ANSI escape code for magenta text.
    pub const MAGENTA: &str = "\x1b[35m";
    /// ANSI escape code to reset terminal styling.
    pub const RESET: &str = "\x1b[0m";
}

//================================================================================================
// Macros
//================================================================================================

/// A macro to log a fatal error and exit.
#[macro_export]
macro_rules! fatal {
    ($error:expr) => {{
        use $crate::cli::logging::{ANSI, ansi};
        let ansi = ANSI.load(std::sync::atomic::Ordering::SeqCst);
        tracing::error!(
            fatal = true,
            "{}FATAL{} {}",
            if ansi { ansi::MAGENTA } else { "" },
            if ansi { ansi::RESET } else { "" },
            $error
        );
    }};
}

//================================================================================================
// Functions
//================================================================================================

/// Initializes the global tracing subscriber.
pub fn init_global_subscriber(args: LogArgs) -> WorkerGuard {
    let log_level = get_log_level(args);

    let env_filter = EnvFilter::from_default_env().add_directive(log_level.into());

    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stderr());

    let progress_layer = IndicatifLayer::new().with_progress_style(
        ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}").unwrap(),
    );

    let fmt = if std::io::stderr().is_terminal() {
        fmt::layer()
            .without_time()
            .with_writer(progress_layer.get_stderr_writer())
            .with_target(false)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
            .compact()
            .boxed()
    } else {
        ANSI.store(false, Ordering::SeqCst);
        fmt::layer()
            .with_ansi(ANSI.load(Ordering::SeqCst))
            .json()
            .with_writer(non_blocking)
            .boxed()
    };

    tracing_subscriber::registry()
        .with(fmt)
        .with(env_filter)
        .with(progress_layer)
        .init();

    if log_level == LevelFilter::TRACE {
        let _ = Args::parse();
    }

    guard
}

fn get_log_level(args: LogArgs) -> LevelFilter {
    match args.quiet {
        0 => (),
        1 => return LevelFilter::WARN,
        _ => return LevelFilter::ERROR,
    }

    if let Ok(rust_log) = std::env::var(EnvFilter::DEFAULT_ENV) {
        if let Ok(level) = LevelFilter::from_str(&rust_log) {
            return level;
        }
    }

    match args.verbosity {
        0 => LevelFilter::INFO,
        1 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}
