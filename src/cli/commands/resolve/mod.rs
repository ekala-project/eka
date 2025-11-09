//! This module defines the `resolve` subcommand.
//!
//! The `resolve` subcommand is responsible for resolving dependencies for a
//! given set of atoms and writing the results to a lock file.

use anyhow::Result;
use atom::ManifestWriter;
use atom::storage::LocalStorage;
use atom::uri::LocalAtom;
use clap::Parser;

//================================================================================================
// Types
//================================================================================================

/// The `resolve` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "Resolve Options")]
pub struct Args {
    /// The URI of the local atom(s) to resolve dependencies for.
    atom: Vec<LocalAtom>,
    /// Ignore well specified dependencies in the lock, and update all of them to their highest
    /// matching versions, similar to if you deleted the lock file manually.
    #[clap(long, short)]
    fresh: bool,
    /// Resolve all atoms in the set
    #[clap(long, short, conflicts_with = "atom")]
    all: bool,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `resolve` subcommand.
pub(super) async fn run(storage: impl LocalStorage + 'static, args: Args) -> Result<()> {
    let to_storage_root = storage.rel_from_root(storage.cwd()?)?;

    if args.all {
        let manifest = storage.ekala_manifest()?;
        for (_, path) in manifest.set().packages().as_ref() {
            tracing::debug!(path = %path.display(), "attempting to resolve dependencies");
            let writer =
                ManifestWriter::open_and_resolve(&storage, &to_storage_root.join(path), args.fresh)
                    .await?;
            writer.write_atomic()?;
            tracing::info!(path = %path.display(), "successfully resolved and wrote updates");
        }
    } else {
        for atom in args.atom {
            let path = atom.path_from_storage(&storage)?;
            tracing::debug!(path = %path.as_ref().display(), "attempting to resolve dependencies");

            let writer = ManifestWriter::open_and_resolve(
                &storage,
                &to_storage_root.join(&path),
                args.fresh,
            )
            .await?;
            writer.write_atomic()?;

            tracing::info!(path = %path.as_ref().display(), "successfully resolved and wrote updates");
        }
    }
    Ok(())
}
