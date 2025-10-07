//! This module contains the command-line interface for Eka.
//!
//! It uses the `clap` crate to parse command-line arguments and subcommands.
//! The main entry point is the `run` function, which executes the appropriate
//! command based on the parsed arguments.

use std::path::PathBuf;

use clap::Parser;

pub use self::commands::run;
pub use self::logging::init_global_subscriber;

mod commands;
pub mod logging;
mod store;

/// The top-level command-line arguments for Eka.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Change the current working directory.
    ///
    /// If specified, changes the current working directory to the given
    /// path before executing any commands. This affects all file system
    /// operations performed by the program.
    #[arg(short = 'C', value_name = "DIR", global = true, value_parser = validate_path)]
    working_directory: Option<PathBuf>,

    /// Arguments for controlling logging behavior.
    #[command(flatten)]
    pub log: LogArgs,

    #[command(subcommand)]
    command: commands::Commands,
}

/// Arguments for controlling logging behavior.
#[derive(Parser, Clone, Copy, Debug)]
#[command(next_help_heading = "Log Options")]
pub struct LogArgs {
    /// Set the level of verbosity.
    ///
    /// This flag can be used multiple times to increase verbosity:
    /// - `-v` for DEBUG level
    /// - `-vv` for TRACE level
    ///
    /// If not specified, defaults to INFO level.
    ///
    /// Alternatively, set the `RUST_LOG` environment variable (e.g., `RUST_LOG=info`), which takes
    /// precedence over this flag.
    ///
    /// **Note**: This flag is silently ignored when `--quiet` is also set.
    #[arg(
        short,
        long,
        action = clap::ArgAction::Count,
        global = true,
        help = "Increase logging verbosity",
    )]
    verbosity: u8,

    /// Suppress verbosity, taking precedence over other flags.
    ///
    /// This flag can be used multiple times to decrease verbosity:
    /// - `-q` for WARN level
    /// - `-qq` for ERROR level
    ///
    /// This flag *overrides* any verbosity settings. It takes precedence over both the
    /// `--verbosity` flag and the `RUST_LOG` environment variable.
    ///
    /// Use this flag when you want minimal output from the application, typically in
    /// non-interactive or automated environments.
    #[arg(
        short,
        long,
        action = clap::ArgAction::Count,
        global = true,
    )]
    quiet: u8,
}

/// Changes the current working directory based on the `-C` flag.
///
/// This function is a bit of a hack to get around `clap`'s limitations.
/// It manually parses the command-line arguments to find the `-C` flag
/// and changes the current directory before `clap` does its parsing.
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

fn validate_path(path: &str) -> Result<PathBuf, std::io::Error> {
    std::fs::canonicalize(path)
}
