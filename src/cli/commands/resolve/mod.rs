//! This module defines the `resolve` subcommand.
//!
//! The `resolve` subcommand is responsible for resolving dependencies for a
//! given set of atoms and writing the results to a lock file.

use anyhow::Result;
use atom::storage::LocalStorage;
use atom::uri::Uri;
use clap::Parser;
use tokio::task::JoinSet;

//================================================================================================
// Types
//================================================================================================

/// The `resolve` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The URI of the local atom(s) to resolve dependencies for.
    uri: Vec<Uri>,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `resolve` subcommand.
pub(super) fn run(storage: impl LocalStorage, args: Args) -> Result<()> {
    // let mut tasks = JoinSet::new();

    let ekala_root = storage.ekala_root_dir()?;

    for uri in args.uri {
        if let Some(url) = uri.url() {
            if url.scheme == gix::url::Scheme::File {
            } else {
                tracing::warn!(%uri, "cannot lock external resources");
                continue;
            }
        }
    }
    Ok(())
}
