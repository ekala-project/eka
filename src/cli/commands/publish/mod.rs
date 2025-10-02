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
    git: git::GitArgs,
}

use publish::Stats;
pub(super) async fn run(store: Detected, args: PublishArgs) -> Result<Stats, PublishError> {
    let mut stats = Stats::default();
    #[allow(clippy::single_match)]
    match store {
        Detected::Git(repo) => {
            use atom::publish::{Content, error};
            use {Err as Skipped, Ok as Published};
            let (results, mut errors) = git::run(repo, args).await?;

            for res in results {
                match res {
                    Ok(Published(atom)) => {
                        stats.published += 1;
                        let Content::Git(content) = atom.content();
                        let name = content.content().name.clone();
                        tracing::info!(
                            atom.tag = %atom.id().tag(),
                            path = %content.path().display(),
                            r#ref = %name,
                            "success"
                        );
                    },
                    Ok(Skipped(tag)) => {
                        stats.skipped += 1;
                        tracing::info!(atom.tag = %tag, "Skipping existing atom")
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
