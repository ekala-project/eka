//! This module defines the `new` subcommand.
//!
//! The `new` subcommand is responsible for creating a new atom in the
//! specified directory.

use std::ffi::OsStr;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use atom::manifest::EkalaWriter;
use atom::{Label, Manifest};
use clap::Parser;
use semver::Version;

use crate::cli::store::Detected;

//================================================================================================
// Types
//================================================================================================

/// The `new` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "New Options")]
pub struct Args {
    /// The path to create the new atom in.
    path: PathBuf,
    /// The verbatim description of the atom.
    #[arg(short, long)]
    description: Option<String>,
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
pub(super) async fn run(
    store: impl Future<Output = Result<Detected, crate::cli::store::Error>>,
    args: Args,
) -> Result<()> {
    let label: Label = if let Some(label) = args.label {
        label
    } else {
        args.path.file_name().unwrap_or(OsStr::new("")).try_into()?
    };
    let atom = Manifest::new(label.to_owned(), args.version, args.description);
    let atom_str = toml_edit::ser::to_string_pretty(&atom)?;
    let atom_toml = args.path.join(atom::ATOM_MANIFEST_NAME.as_str());

    fs::create_dir_all(&args.path)?;

    let mut dir = fs::read_dir(&args.path)?;

    if dir.next().is_some() {
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("Directory exists and is not empty: {:?}", args.path),
        ))?;
    }

    let mut toml_file = fs::File::create(atom_toml)?;
    toml_file.write_all(atom_str.as_bytes())?;
    tracing::info!(message = "successfully created new atom", %label);

    let repo = if let Ok(Detected::Git(repo)) = store.await {
        Some(repo)
    } else {
        None
    };
    if let Ok(mut writer) = EkalaWriter::new(repo).map_err(|error| {
        tracing::error!(%error);
        error
    }) {
        writer.write_package(&args.path)?;
        writer.write_atomic()?;
        tracing::info!(message = "successfully added to package to set", atom = %label);
    } else {
        tracing::warn!(
            message = "package set not yet initialized",
            suggestion = "run eka init"
        );
    };

    Ok(())
}
