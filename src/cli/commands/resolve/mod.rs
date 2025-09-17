use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

mod git;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The path of the atom(s) to resolve dependencies for
    path: Vec<PathBuf>,
    #[command(flatten)]
    store: StoreArgs,
    /// Output file for the lock (default: atom.lock)
    #[arg(short, long, default_value = "atom.lock")]
    output: PathBuf,
    /// Resolution mode: shallow or deep (default: shallow)
    #[arg(short, long, default_value = "shallow")]
    mode: String,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    #[cfg(feature = "git")]
    git: git::GitArgs,
}

pub(super) fn run(_store: Detected, _args: Args) -> Result<()> {
    Ok(())
}
