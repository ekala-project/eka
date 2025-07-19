#[cfg(feature = "git")]
mod git;

use std::path::PathBuf;

use atom::publish::error::PublishError;
use atom::publish::{self};
use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub(in super::super) struct PublishArgs {
    /// Publish all the atoms in and under the current working directory
    #[arg(long, short, conflicts_with = "path")]
    recursive: bool,

    /// Path(s) to the atom(s) to publish
    #[arg(required_unless_present = "recursive")]
    path: Vec<PathBuf>,
    #[command(flatten)]
    store: StoreArgs,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    #[cfg(feature = "git")]
    git: git::GitArgs,
}

use publish::Stats;
pub(super) async fn run(store: Detected, args: PublishArgs) -> Result<Stats, PublishError> {
    #[cfg_attr(not(feature = "stores"), allow(unused_mut))]
    let mut stats = Stats::default();
    match store {
        #[cfg(feature = "git")]
        Detected::Git(repo) => {
            use atom::publish::{Content, error};
            use {Err as Skipped, Ok as Published};
            let (results, mut errors) = git::run(repo, args).await?;

            for res in results {
                match res {
                    Ok(Published(atom)) => {
                        stats.published += 1;
                        let Content::Git(content) = atom.content();
                        tracing::info!(
                            atom.id = %atom.id().id(),
                            path = %content.path().display(),
                            "Atom successfully published"
                        );
                    },
                    Ok(Skipped(id)) => {
                        stats.skipped += 1;
                        tracing::info!(atom.id = %id, "Skipping existing atom")
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
