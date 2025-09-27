use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use atom::publish::error::git::Error;
use atom::publish::git::{GitOutcome, GitResult};
use atom::store::git;
use clap::Parser;
use gix::ThreadSafeRepository;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use super::PublishArgs;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Git Options")]
pub(super) struct GitArgs {
    /// The target remote to publish the atom(s) to
    #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET")]
    remote: String,
    /// The revision to publish the atom(s) from
    ///
    /// Specifies a revision using Git's extended SHA-1 syntax.
    /// This can be a commit hash, branch name, tag, or a relative
    /// reference like HEAD~3 or master@{yesterday}.
    #[arg(
        long,
        short,
        default_value = "HEAD",
        verbatim_doc_comment,
        name = "REVSPEC"
    )]
    spec: String,
}

#[tracing::instrument(skip_all)]
pub(super) async fn run(
    repo: &ThreadSafeRepository,
    args: PublishArgs,
) -> GitResult<(Vec<GitResult<GitOutcome>>, Vec<Error>)> {
    use std::path::Path;

    use atom::publish::git::GitPublisher;
    use atom::publish::{Builder, Publish};
    use atom::store::{NormalizeStorePath, QueryVersion};
    use tracing_indicatif::style::ProgressStyle;

    let span = tracing::Span::current();
    span.pb_set_style(
        &ProgressStyle::with_template("{spinner:.green} {msg}: running for [{elapsed}]")
            .unwrap_or(ProgressStyle::default_spinner()),
    );
    span.pb_set_message("✍️ publish");

    let repo = repo.to_thread_local();

    let GitArgs { remote, spec } = args.store.git;

    let progress_span = tracing::info_span!("progress");
    let (atoms, mut publisher) =
        GitPublisher::new(&repo, &remote, &spec, &progress_span)?.build()?;

    let mut errors = Vec::with_capacity(args.path.len());

    let paths = if args.recursive {
        let paths: HashSet<_> = if !repo.is_bare() {
            let cwd = repo.normalize(repo.current_dir())?;
            atoms
                .into_values()
                .filter_map(|path| path.strip_prefix(&cwd).map(Path::to_path_buf).ok())
                .collect()
        } else {
            atoms.into_values().collect()
        };

        if paths.is_empty() {
            return Err(Error::NotFound);
        }
        paths
    } else {
        args.path.into_iter().collect()
    };

    let remote = publisher.remote();
    let remote_atoms = {
        let span = tracing::info_span!("check");
        atom::log::set_sub_task(&span, "✔️ querying remote for existing atoms");
        let _enter = span.enter();
        if let Ok(refs) = remote.get_atoms(Some(publisher.transport())) {
            let iter = refs.into_iter();
            let s = match iter.size_hint() {
                (l, None) => l,
                (_, Some(u)) => u,
            };
            iter.fold(HashMap::with_capacity(s), |mut acc, (t, v, id)| {
                acc.insert(t, (v, id));
                acc
            })
        } else {
            HashMap::new()
        }
    };

    let results = {
        atom::log::set_bar(
            &progress_span,
            "💾 publishing atoms",
            (paths.len() * 3) as u64,
        );

        let _guard = progress_span.enter();
        let results = publisher.publish(paths, remote_atoms);

        publisher.await_pushes(&mut errors).await;
        results
    };

    Ok((results, errors))
}
