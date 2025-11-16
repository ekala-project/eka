use atom::storage::{LocalStorage, QueryStore, QueryVersion, RemoteAtomCache, git};
use atom::uri::Uri;
use clap::Parser;
use semver::VersionReq;
use tempfile::TempDir;
use tokio::task::JoinSet;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Plan Options")]
#[group(id = "plan_args")]
pub struct Args {
    /// The atom uris to generate a plan for.
    uri: Vec<Uri>,
}

pub async fn run(_storage: Option<&impl LocalStorage>, args: Args) -> anyhow::Result<()> {
    let mut tasks = JoinSet::new();
    let cache = git::cache_repo()?;
    let mut atom_dirs: Vec<TempDir> = Vec::with_capacity(args.uri.len());
    for uri in args.uri {
        if let Some(url) = uri.url().map(ToOwned::to_owned) {
            let mut transport = url.get_transport()?;
            let task = async move {
                tokio::task::spawn_blocking(move || {
                    let star = VersionReq::STAR;
                    let req = uri.version().unwrap_or(&star);
                    let res = url
                        .get_highest_match(uri.label(), req, Some(&mut transport))
                        .map(|(version, _)| {
                            let repo = &cache.to_thread_local();
                            repo.retrieve_atom(&url, uri.label(), &version, &mut transport)
                                .inspect_err(|e| tracing::error!("{}", e))
                        });
                    if res.is_none() {
                        tracing::warn!(
                            label = %uri.label(),
                            url = %url,
                            requested = %req,
                            "skipped: couldn't acquire requested atom uri from remote"
                        );
                    }
                    res
                })
                .await
            };
            tasks.spawn(task);
        } else {
            // TODO: handle local atoms
        }
    }
    while let Some(dir) = tasks.join_next().await {
        if let Some(dir) = dir?? {
            atom_dirs.push(dir?);
        }
    }
    // TODO: actually invoke the evaluator
    Ok(())
}
