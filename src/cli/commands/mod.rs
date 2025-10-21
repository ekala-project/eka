//! This module defines the subcommands for the Eka CLI.
//!
//! Each subcommand is implemented in its own module and is responsible for
//! handling its own arguments and logic. The `run` function in this module
//! dispatches to the appropriate subcommand based on the parsed arguments.

use clap::Subcommand;

use super::Args;
use crate::cli::store;

mod add;
mod init;
mod new;
mod publish;
mod resolve;

//================================================================================================
// Types
//================================================================================================

/// The subcommands for the Eka CLI.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(super) enum Commands {
    /// Add dependencies from a given atom URI to the manifest.
    ///
    /// This command takes an atom URI and updates the manifest and lock with the result.
    Add(add::Args),
    /// Create a new atom at the specified path.
    ///
    /// This command takes a path anywhere on the file-system and creates
    /// a new bare atom there.
    New(new::Args),
    /// Package and publish atoms to a remote location.
    ///
    /// This command efficiently packages and publishes atoms using Git:
    ///
    /// - Creates isolated structures (orphan branches) for each atom
    /// - Uses custom Git refs for versioning and rapid, path-based fetching
    /// - Enables decentralized publishing while minimizing data transfer
    ///
    /// The atom store concept is designed to be extensible, allowing for future support of
    /// alternative storage backends as well.
    Publish(publish::PublishArgs),
    /// Resolve dependencies for the specified atom(s).
    ///
    /// This command will resolve and lock each dependency for the given atom(s) into a well
    /// structured lock file format.
    Resolve(resolve::Args),
    /// Initialize an Ekala package set.
    ///
    /// This command creates an `ekala.toml` to serve as the source of truth for a collection of
    /// atoms in a repository. Optionally, and by default if detected, it will also initialize the
    /// specified remote for publishing atoms if not already setup.
    #[command(verbatim_doc_comment)]
    Init(init::Args),
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the Eka CLI.
pub async fn run(args: Args) -> anyhow::Result<()> {
    let store = store::detect();
    match args.command {
        Commands::Add(args) => add::run(store.await.ok(), args).await?,
        Commands::New(args) => new::run(store, args).await?,
        Commands::Publish(args) => {
            publish::run(store.await?, args).await?;
        },
        Commands::Init(args) => init::run(store.await.ok(), args)?,
        Commands::Resolve(args) => resolve::run(store.await?, args)?,
    }
    Ok(())
}
