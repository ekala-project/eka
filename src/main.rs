//! The main entry point for the Eka CLI.

#![warn(missing_docs)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use eka::cli::{self, Args};

//================================================================================================
// Functions
//================================================================================================

fn main() -> ExitCode {
    let bin = match eka::record_startup_exe().context("could not determine binary info") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        },
    };
    match PathBuf::from(bin).file_stem().and_then(|p| p.to_str()) {
        Some(eka::NIXEC) => nixec(),
        _ => eka(),
    }
}

/// The main entry point for the Eka CLI.
#[tokio::main]
async fn eka() -> ExitCode {
    let args = Args::parse_from(cli::change_directory());
    let Args { log, .. } = args;

    let _guard = cli::init_global_subscriber(log);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::warn!("Ctrl+C received, terminating...");
            ExitCode::SUCCESS
        }
        res = cli::run(args) => {
            if let Err(e) = res {
                eka::fatal!(e);
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}

fn nixec() -> ExitCode {
    match nixec::main() {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        },
    }
}
