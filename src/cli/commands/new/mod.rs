//! This module defines the `new` subcommand.
//!
//! The `new` subcommand is responsible for creating a new atom in the
//! specified directory.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use atom::manifest::EkalaManifest;
use atom::{AtomTag, Manifest};
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
    /// The verbatim description of the atom.
    #[arg(short, long)]
    description: Option<String>,
    /// The version to initialize the atom at.
    #[arg(short = 'V', long, default_value = "0.1.0")]
    version: Version,
    /// The atom's `tag` (defaults the the last part of path)
    #[arg(short, long)]
    tag: Option<AtomTag>,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `new` subcommand.
pub(super) fn run(args: Args) -> Result<()> {
    let tag: AtomTag = if let Some(tag) = args.tag {
        tag
    } else {
        args.path.file_name().unwrap_or(OsStr::new("")).try_into()?
    };
    let atom = Manifest::new(tag.to_owned(), args.version, args.description);
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
    tracing::info!(message = "successfully created new atom", %tag);

    let ekala_path = find_upwards(atom::EKALA_MANIFEST_NAME.as_str())?;
    if let Some(path) = ekala_path {
        let mut ekala: EkalaManifest =
            toml_edit::de::from_str(std::fs::read_to_string(path)?.as_str())?;
        ekala.add_package(args.path)?;
        tracing::info!(message = "added to package to set", %tag, set = ekala.set().name());
    } else {
        tracing::warn!(
            message = "package set not yet initialized, atom won't be publishable until `eka \
                       init` is invoked"
        );
    }

    Ok(())
}

fn find_upwards(filename: &str) -> anyhow::Result<Option<PathBuf>> {
    let start_dir = std::env::current_dir()?;

    for ancestor in start_dir.ancestors() {
        let file_path = ancestor.join(filename);
        if file_path.exists() {
            return Ok(Some(file_path));
        }
    }

    Ok(None)
}
