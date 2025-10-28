//! This module defines the `publish` subcommand.
//!
//! The `publish` subcommand is responsible for publishing atoms to the atom
//! store. It can publish atoms from specified paths or recursively from the
//! current directory.
mod git;

use std::path::PathBuf;

use anyhow::Result;
use atom::package::publish::Stats;
use atom::package::publish::error::PublishError;
use clap::Parser;

use crate::cli::store::Detected;

//================================================================================================
// Types
//================================================================================================

/// The `publish` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "Publish Options")]
pub(in super::super) struct PublishArgs {
    /// Publish all the atoms in and under the current working directory.
    #[arg(long, short, conflicts_with = "path")]
    recursive: bool,

    /// Path(s) to the atom(s) to publish
    #[arg(required_unless_present = "recursive")]
    path: Vec<PathBuf>,
    #[command(flatten)]
    store: StoreArgs,
}

/// Arguments for the atom store.
#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    git: git::GitArgs,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `publish` subcommand.
pub(super) async fn run(store: Detected, args: PublishArgs) -> Result<Stats, PublishError> {
    let mut stats = Stats::default();
    #[allow(clippy::single_match)]
    match store {
        Detected::Git(repo) => {
            use atom::package::publish::{Content, error};
            use {Err as Skipped, Ok as Published};
            let (results, mut errors) = git::run(repo, args).await?;

            for res in results {
                match res {
                    Ok(Published(atom)) => {
                        stats.published += 1;
                        let Content::Git(content) = atom.content();
                        let name = content.content().name.clone();
                        tracing::info!(
                            atom.label = %atom.id().label(),
                            path = %content.path().display(),
                            r#ref = %name,
                            "success"
                        );
                    },
                    Ok(Skipped(label)) => {
                        stats.skipped += 1;
                        tracing::info!(atom.label = %label, "Skipping existing atom")
                    },
                    Err(e) => {
                        stats.failed += 1;
                        errors.push(e)
                    },
                }
            }

            for err in &errors {
                err.warn()
            }

            tracing::info!(stats.published, stats.skipped, stats.failed);

            if !errors.is_empty() {
                return Err(PublishError::Git(error::git::Error::Failed));
            }
        },
        _ => {},
    }

    Ok(stats)
}
