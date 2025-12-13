//! Tests for git-based atom publishing.

use std::collections::HashMap;

use anyhow::{Context, anyhow};
use gix::ThreadSafeRepository;
use gix::prelude::ReferenceExt;

use super::super::{Content, Publish, Record};
use crate::storage::{Init, QueryStore};
use crate::test::harness::{MockAtom, init_repo_and_remote, init_tracing};

//================================================================================================
// Tests
//================================================================================================

#[tokio::test]
async fn publish_atom() -> Result<(), anyhow::Error> {
    use super::{Builder, GitPublisher};
    use crate::id::Label;
    init_tracing();

    let (repo_dir, _remote) = init_repo_and_remote()?;
    std::env::set_current_dir(&repo_dir)?;
    let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
    let repo = repo.to_thread_local();
    let remote = repo.find_remote("origin")?;
    let progress = &tracing::info_span!("test");
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;
    repo.ekala_init(None)?;
    remote.ekala_init(None)?;

    let label = "foo";
    let repo = repo.into_sync();
    let (file_path, src) = repo.mock(label, "0.1.0").await?;
    let repo = repo.to_thread_local();

    let (paths, publisher) = GitPublisher::new(&repo, "origin", "HEAD", progress)?.build()?;
    let path = paths
        .as_ref()
        .get_by_left(&Label::try_from(label)?)
        .context("path is messed up")?;
    let result = publisher.publish_atom(path, &HashMap::new())?;
    let mut errors = Vec::with_capacity(1);
    publisher.await_pushes(&mut errors).await;
    if !errors.is_empty() {
        for e in errors {
            tracing::error!(%e)
        }
        return Err(anyhow!("push errors"));
    }

    let content = match result {
        Ok(Record {
            content: Content::Git(c),
            ..
        }) => c,
        _ => return Err(anyhow::anyhow!("atom publishing failed")),
    };

    let origin_id = content.origin.attach(&repo).into_fully_peeled_id()?;
    let content_ref = content.content.attach(&repo);
    let content_tree = repo
        .find_commit(content_ref.into_fully_peeled_id()?)?
        .tree()?
        .detach();
    let dir = file_path.to_path_buf();
    let dir = dir
        .parent()
        .and_then(|f| f.file_name())
        .ok_or(anyhow::anyhow!("no parent directory"))?;
    let origin_tree = repo
        .find_commit(origin_id.detach())?
        .tree()?
        .lookup_entry_by_path(dir)?
        .ok_or(anyhow::anyhow!("no tree in orgin"))?
        .object()?;
    let path = file_path.strip_prefix(repo.workdir().context("")?)?;

    assert_eq!(origin_id, src);
    assert_eq!(path, content.path);

    assert_eq!(content_tree.data, origin_tree.data);

    Ok(())
}
