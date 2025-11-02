//! This module defines the `new` subcommand.
//!
//! The `new` subcommand is responsible for creating a new atom in the
//! specified directory.

use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::Result;
use atom::storage::LocalStorage;
use atom::{EkalaManager, Label};
use clap::Parser;
use semver::Version;

//================================================================================================
// Types
//================================================================================================

/// The `new` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "New Options")]
pub struct Args {
    /// The path to create the new atom in.
    path: PathBuf,
    /// The version to initialize the atom at.
    #[arg(short = 'V', long, default_value = "0.1.0")]
    version: Version,
    /// The atom's `label` (defaults the the last part of path)
    #[arg(short, long)]
    label: Option<Label>,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `new` subcommand.
pub(super) fn run(storage: impl LocalStorage, args: Args) -> Result<()> {
    let label: Label = if let Some(label) = args.label {
        label
    } else {
        args.path.file_name().unwrap_or(OsStr::new("")).try_into()?
    };
    if let Ok(mut manager) = EkalaManager::new(&storage).map_err(|error| {
        tracing::error!(%error);
        error
    }) {
        manager.new_atom_at_path(label, args.path, args.version)?;
    } else {
        tracing::warn!(
            message = "package set not yet initialized",
            suggestion = "run eka init"
        );
    };

    Ok(())
}
