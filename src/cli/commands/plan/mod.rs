use atom::storage::{LocalStorage, QueryStore, QueryVersion, git};
use atom::uri::Uri;
use clap::Parser;
use semver::VersionReq;
use tempfile::TempDir;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Plan Options")]
#[group(id = "plan_args")]
pub struct Args {
    /// The atom uris to generate a plan for.
    ///
    /// note: this argument is ignored if the current directory is already inside a git repository
    uri: Vec<Uri>,
}

pub async fn run(_storage: Option<&impl LocalStorage>, args: Args) -> anyhow::Result<()> {
    let mut atom_dirs: Vec<TempDir> = Vec::with_capacity(args.uri.len());
    for uri in args.uri {
        if let Some(url) = uri.url() {
            let mut transport = url.get_transport()?;
            let star = VersionReq::STAR;
            let req = uri.version().unwrap_or(&star);
            if let Some((version, _)) =
                url.get_highest_match(uri.label(), req, Some(&mut transport))
                && let Ok(dir) = git::cache_atom(url, uri.label(), &version, &mut transport)
                    .inspect_err(|e| tracing::error!("{}", e))
            {
                atom_dirs.push(dir);
            } else {
                tracing::warn!(
                    label = %uri.label(),
                    url = %url,
                    requested = %req,
                    "skipped: couldn't acquire requested atom uri from remote"
                );
            }
        } else {
            // TODO: handle local atoms
        }
    }
    // TODO: actually invoke the evaluator
    Ok(())
}
