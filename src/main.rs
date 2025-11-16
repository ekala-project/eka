//! The main entry point for the Eka CLI.

#![warn(missing_docs)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::OnceLock;

use anyhow::Context;
use clap::Parser;
use eka::cli::{self, Args};

const NIXEC: &str = "nixec";

static STARTUP_INODE: OnceLock<u64> = OnceLock::new();
static STARTUP_DEV: OnceLock<u64> = OnceLock::new();

//================================================================================================
// Functions
//================================================================================================

fn main() -> ExitCode {
    let arg0 = match record_startup_exe().context("could not determine binary info") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        },
    };
    match PathBuf::from(arg0).file_stem().and_then(|p| p.to_str()) {
        Some(NIXEC) => nixec(),
        _ => eka(),
    }
}

/// The main entry point for the Eka CLI.
#[tokio::main]
async fn eka() -> ExitCode {
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

fn nixec() -> ExitCode {
    match nixec::main() {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        },
    }
}

fn record_startup_exe() -> std::io::Result<OsString> {
    let path = std::env::current_exe()?;
    let meta = fs::metadata(&path)?;
    STARTUP_INODE.get_or_init(|| meta.ino());
    STARTUP_DEV.get_or_init(|| meta.dev());
    let arg0 = std::env::args_os().next().unwrap_or(OsString::from("eka"));
    Ok(arg0)
}

fn is_same_exe(path: &std::path::Path) -> std::io::Result<bool> {
    let meta = fs::metadata(path)?;
    Ok(Some(&meta.ino()) == STARTUP_INODE.get() && Some(&meta.dev()) == STARTUP_DEV.get())
}
