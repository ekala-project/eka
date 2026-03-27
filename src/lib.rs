//! Eka, a simple, fast, and experimental package manager for Nix.

#![warn(missing_docs)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::sync::OnceLock;

pub mod cli;

/// the name of the nixec target
pub const NIXEC: &str = "nixec";

static STARTUP_INODE: OnceLock<u64> = OnceLock::new();
static STARTUP_DEV: OnceLock<u64> = OnceLock::new();

/// record the startup executable to check against later
pub fn record_startup_exe() -> std::io::Result<OsString> {
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
