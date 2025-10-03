mod add;
mod new;
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
    /// Resolve dependencies for the specified atom(s).
    ///
    /// This command will resolve and lock each dependency for the given
    /// atom(s) into a well structured lock file format.
    #[command(verbatim_doc_comment)]
    Resolve(resolve::Args),
    /// Add dependencies from a given atom uri to the manifest.
    ///
    /// This command takes atom uri or pin spec and updates the
    /// manifest and lock with the result.
    #[command(verbatim_doc_comment)]
    Add(add::Args),
    /// Create a new atom at the specified path.
    ///
    /// This command takes a path anywhere on the file-system and creates
    /// a new bare atom there.
    #[command(verbatim_doc_comment)]
    New(new::Args),
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let store = store::detect();
    match args.command {
        Commands::Publish(args) => {
            if args.init {
                publish::init::run(store.await?, args.store)?;
            } else {
                publish::run(store.await?, args).await?;
            }
        },
        Commands::Resolve(args) => resolve::run(store.await?, args)?,
        Commands::New(args) => new::run(args)?,
        Commands::Add(args) => add::run(args)?,
    }
    Ok(())
}
