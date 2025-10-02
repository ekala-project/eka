mod commands;
pub mod logging;
mod store;

use std::path::PathBuf;

use clap::Parser;
pub use commands::run;
pub use logging::init_global_subscriber;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short = 'C', value_name = "DIR", global = true, verbatim_doc_comment, value_parser = validate_path)]

    /// Change the current working directory
    ///
    /// If specified, changes the current working directory to the given
    /// path before executing any commands. This affects all file system
    /// operations performed by the program.
    working_directory: Option<PathBuf>,

    #[command(flatten)]
    pub log: LogArgs,

    #[command(subcommand)]
    command: commands::Commands,
}

#[derive(Parser, Clone, Copy, Debug)]
#[command(next_help_heading = "Log Options")]
pub struct LogArgs {
    /// Set the level of verbosity
    ///
    /// This flag can be used multiple times to increase verbosity:
    ///   -v    for INFO level
    ///   -vv   for DEBUG level
    ///   -vvv  for TRACE level
    ///
    /// If not specified, defaults to WARN level.
    ///
    /// Alternatively, set the `RUST_LOG` environment variable
    /// (e.g., `RUST_LOG=info`), which takes precedence over this flag.
    ///
    /// Note: This flag is silently ignored when `--quiet` is also set.
    #[arg(
        short,
        long,
        action = clap::ArgAction::Count,
        global = true,
        help = "Increase logging verbosity",
        verbatim_doc_comment
    )]
    verbosity: u8,

    /// Suppress all output except errors
    ///
    /// This flag overrides any verbosity settings and sets the log
    /// level to ERROR. It takes precedence over both the `--verbosity`
    // flag and the `RUST_LOG` environment variable.
    ///
    /// Use this flag when you want minimal output from the application,
    /// typically in non-interactive or automated environments.
    #[arg(short, long, global = true, verbatim_doc_comment)]
    quiet: bool,
}

fn validate_path(path: &str) -> Result<PathBuf, std::io::Error> {
    std::fs::canonicalize(path)
}

pub fn change_directory() -> Vec<String> {
    let mut seen: Option<bool> = None;
    std::env::args()
        .map(|arg| {
            if seen.is_none() && arg == "-C" {
                seen = Some(true);
                return arg;
            }
            if let Some(cd) = seen {
                if cd {
                    std::env::set_current_dir(&arg).ok();
                    seen = Some(false);
                }
            }
            arg
        })
        .collect()
}
