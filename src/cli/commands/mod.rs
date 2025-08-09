mod init;
mod publish;
mod resolve;

use clap::Subcommand;

use super::Args;
use crate::cli::store;

#[derive(Subcommand)]
pub(super) enum Commands {
    /// Package and publish atoms to the atom store.
    ///
    /// This command efficiently packages and publishes atoms using Git:
    ///
    /// - Creates isolated structures (orphan branches) for each atom
    /// - Uses custom Git refs for versioning and rapid, path-based fetching
    /// - Enables decentralized publishing while minimizing data transfer
    ///
    /// The atom store concept is designed to be extensible, allowing for
    /// future support of alternative storage backends as well.
    #[command(verbatim_doc_comment)]
    Publish(publish::PublishArgs),
    /// Initialize the Ekala store.
    ///
    /// This command initializes the repository for use as an Ekala store
    /// fit for publishing atoms to a remote location.
    #[command(verbatim_doc_comment)]
    Init(init::Args),
    /// Resolve dependencies for the specified atom(s).
    ///
    /// This command will resolve and lock each dependency for the given
    /// atom(s) into a well structured lock file format.
    #[command(verbatim_doc_comment)]
    Resolve(resolve::Args),
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let store = store::detect();
    match args.command {
        Commands::Publish(args) => {
            publish::run(store.await?, args).await?;
        },

        Commands::Init(args) => init::run(store.await?, args)?,
        Commands::Resolve(args) => resolve::run(store.await?, args),
        _ => (),
    }
    Ok(())
}
