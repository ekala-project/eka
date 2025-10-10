//! This module defines the `resolve` subcommand.
//!
//! The `resolve` subcommand is responsible for resolving dependencies for a
//! given set of atoms and writing the results to a lock file.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::cli::store::Detected;

mod git;

//================================================================================================
// Types
//================================================================================================

/// The `resolve` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The path of the atom(s) to resolve dependencies for.
    path: Vec<PathBuf>,
    /// The output file for the lock (default: `atom.lock`).
    #[arg(short, long, default_value = "atom.lock")]
    output: PathBuf,
    /// The resolution mode: `shallow` or `deep` (default: `shallow`).
    #[arg(short, long, default_value = "shallow")]
    mode: String,
    #[command(flatten)]
    store: StoreArgs,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    git: git::GitArgs,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `resolve` subcommand.
pub(super) fn run(_store: Detected, _args: Args) -> Result<()> {
    Ok(())
}
