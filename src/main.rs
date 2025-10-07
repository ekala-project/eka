//! The main entry point for the Eka CLI.

#![warn(missing_docs)]

use std::process::ExitCode;

use clap::Parser;
use eka::cli::{self, Args};

/// The main entry point for the Eka CLI.
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse_from(cli::change_directory());
    let Args { log, .. } = args;

    let _guard = cli::init_global_subscriber(log);

    if let Err(e) = cli::run(args).await {
        eka::fatal!(e);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
