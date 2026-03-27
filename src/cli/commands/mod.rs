//! This module defines the subcommands for the Eka CLI.
//!
//! Each subcommand is implemented in its own module and is responsible for
//! handling its own arguments and logic. The `run` function in this module
//! dispatches to the appropriate subcommand based on the parsed arguments.

use atom::storage::LocalStoragePath;
use clap::Subcommand;

use super::Args;
use crate::cli::store;

mod add;
mod init;
mod new;
mod plan;
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
    /// Formulate an atom's build plan.
    ///
    /// This command will evaluate the expressions contained in an atom to produce a build recipe
    /// (e.g. a nix derivation) for later execution.
    #[command(verbatim_doc_comment)]
    Plan(plan::Args),
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the Eka CLI.
pub async fn run(args: Args) -> anyhow::Result<()> {
    let store = store::detect()?;
    match (args.command, store) {
        (Commands::Add(args), store::Detected::Git(repo)) => {
            add::run(repo, args).await?;
        },
        (Commands::Add(args), store::Detected::FileStorage(fs)) => add::run(&fs, args).await?,
        (Commands::New(args), store::Detected::Git(repo)) => {
            new::run(repo, args).await?;
        },
        (Commands::New(args), store::Detected::FileStorage(fs)) => new::run(&fs, args).await?,
        (Commands::Publish(args), store::Detected::Git(repo)) => {
            publish::run(repo, args).await?;
        },
        (Commands::Resolve(args), store::Detected::Git(repo)) => {
            resolve::run(repo.to_owned(), args).await?;
        },
        (Commands::Resolve(args), store::Detected::FileStorage(fs)) => {
            resolve::run(fs, args).await?
        },
        (Commands::Init(args), storage) => {
            init::run(storage, args)?;
        },
        (Commands::Plan(args), store::Detected::Git(repo)) => {
            plan::run(Some(repo), args).await?;
        },
        (Commands::Plan(args), store::Detected::FileStorage(fs)) => {
            plan::run(Some(&fs), args).await?
        },
        (Commands::Plan(args), store::Detected::None) => {
            plan::run(Option::<&LocalStoragePath>::None, args).await?
        },
        _ => (),
    }
    Ok(())
}
