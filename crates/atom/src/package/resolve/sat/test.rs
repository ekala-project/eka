//! Integration tests for SAT-based dependency resolution.
//!
//! These tests verify that the transitive dependency resolver correctly handles
//! various dependency graph shapes and edge cases.

use std::collections::HashMap;

use gix::ThreadSafeRepository;

use crate::package::metadata::manifest::ManifestWriter;
use crate::package::publish::Builder;
use crate::storage::{Init, QueryStore};
use crate::test::harness::{MockAtom, commit_workdir, init_repo_and_remote, init_tracing};

//================================================================================================
// Tests
//================================================================================================

/// Test linear dependency chain: A → B → C
///
/// Creates three atoms where A depends on B and B depends on C.
/// Verifies that resolution produces correct transitive deps.
#[tokio::test]
async fn test_linear_chain() -> Result<(), anyhow::Error> {
    use crate::id::{Label, Tag};
    use crate::package::metadata::manifest::SetMirror;
    use crate::package::publish::Publish;
    use crate::package::publish::git::GitPublisher;
    use crate::uri::Uri;

    init_tracing();

    // Setup: Create repo with remote
    let (repo_dir, remote_dir) = init_repo_and_remote()?;
    let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
    let local = repo.to_thread_local();

    // Initialize ekala
    local.ekala_init(None)?;

    // Get remote and initialize it
    let remote = local.find_remote("origin")?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;
    remote.ekala_init(None)?;

    // Get the remote URL for later use
    let remote_url = format!("file://{}", remote_dir.path().display());

    // Phase 1: Create all atoms WITHOUT dependencies first
    let repo = local.into_sync();

    let (_c_path, _c_oid) = repo.mock("atom-c", "1.0.0").await?;
    tracing::info!("Created atom-c");

    let (b_path, _b_oid) = repo.mock("atom-b", "1.0.0").await?;
    tracing::info!("Created atom-b");

    let (a_path, _a_oid) = repo.mock("atom-a", "1.0.0").await?;
    tracing::info!("Created atom-a");

    let mirrors = vec![SetMirror::Url(gix::url::parse(remote_url.as_str().into())?)];

    // Phase 2: Add all dependencies to manifests (but don't publish yet)
    // We first need to publish C so B can resolve its dependency on C
    let local = repo.to_thread_local();
    {
        let progress = &tracing::info_span!("publish-c");
        let (paths, publisher) = GitPublisher::new(&local, "origin", "HEAD", progress)?.build()?;
        let label = Label::try_from("atom-c")?;
        if let Some(path) = paths.as_ref().get_by_left(&label) {
            publisher
                .publish_atom(path, &HashMap::new())?
                .expect("atoms failed to publish");
        }
        let mut errors = Vec::new();
        publisher.await_pushes(&mut errors).await;
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("publish C failed: {:?}", errors));
        }
        tracing::info!("Published atom-c");
    }

    // Add B -> C dependency
    let repo = local.into_sync();
    {
        let b_dir = b_path.parent().expect("atom path has parent");
        let mut writer = ManifestWriter::open_and_resolve(&repo, b_dir, true).await?;
        let uri: Uri = format!("{}::atom-c@^1.0", remote_url).parse()?;
        writer.add_uri(uri, Some(Tag::try_from("origin")?), mirrors.clone())?;
        writer.write_atomic()?;
        tracing::info!("Added atom-c dependency to atom-b");
    }

    // Commit B's changes and publish B
    let local = repo.to_thread_local();
    commit_workdir(&local, "add atom-c dep to atom-b")?;
    {
        let progress = &tracing::info_span!("publish-b");
        let (paths, publisher) = GitPublisher::new(&local, "origin", "HEAD", progress)?.build()?;
        let label = Label::try_from("atom-b")?;
        if let Some(path) = paths.as_ref().get_by_left(&label) {
            publisher
                .publish_atom(path, &HashMap::new())?
                .expect("atoms failed to publish");
        }
        let mut errors = Vec::new();
        publisher.await_pushes(&mut errors).await;
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("publish B failed: {:?}", errors));
        }
        tracing::info!("Published atom-b");
    }

    // Add A -> B dependency
    let repo = local.into_sync();
    {
        let a_dir = a_path.parent().expect("atom path has parent");
        let mut writer = ManifestWriter::open_and_resolve(&repo, a_dir, true).await?;
        let uri: Uri = format!("{}::atom-b@^1.0", remote_url).parse()?;
        writer.add_uri(uri, Some(Tag::try_from("origin")?), mirrors.clone())?;
        writer.write_atomic()?;
        tracing::info!("Added atom-b dependency to atom-a");
    }

    // WORKAROUND: Re-open to trigger transitive SAT resolution
    // TODO: Fix add_uri to call synchronize internally
    {
        let a_dir = a_path.parent().expect("atom path has parent");
        let writer = ManifestWriter::open_and_resolve(&repo, a_dir, false).await?;
        writer.write_atomic()?;
        tracing::info!("Ran full SAT resolution on atom-a");
    }

    // Verify lock file for atom-a has transitive deps
    let a_dir = a_path.parent().expect("atom path has parent");
    let lock_path = a_dir.join("atom.lock");
    let lock_content = std::fs::read_to_string(&lock_path)?;
    tracing::info!("Lock file contents:\n{}", lock_content);

    let lock: crate::Lockfile = toml_edit::de::from_str(&lock_content)?;

    let mut has_b = false;
    let mut has_c = false;
    for (_key, dep) in lock.deps.as_ref().iter() {
        if let crate::package::metadata::lock::Dep::Atom(atom_dep) = dep {
            if atom_dep.label().as_ref() == "atom-b" {
                has_b = true;
            }
            if atom_dep.label().as_ref() == "atom-c" {
                has_c = true;
            }
        }
    }

    assert!(has_b, "atom-b should be in lock file as direct dep");
    assert!(has_c, "atom-c should be in lock file as transitive dep");

    tracing::info!("Linear chain test passed!");
    Ok(())
}

/// Test that unpublished local atoms are handled with pseudo-lock semantics.
#[tokio::test]
#[ignore = "Local atom pseudo-lock not yet implemented in new SAT resolver"]
async fn test_local_unpublished_atom() -> Result<(), anyhow::Error> {
    init_tracing();

    let (repo_dir, _remote) = init_repo_and_remote()?;
    let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
    let local = repo.to_thread_local();

    local.ekala_init(None)?;

    let repo = local.into_sync();

    // Create a local atom (not published)
    let (_path, _oid) = repo.mock("local-atom", "1.0.0").await?;

    // TODO: Create a root atom that depends on local-atom
    // Verify that local-atom gets a pseudo-lock entry

    Ok(())
}

/// Test diamond dependency pattern:
/// ```text
///     A
///    / \
///   B   C
///    \ /
///     D
/// ```
/// A depends on B and C, both B and C depend on D.
/// Verifies that D appears exactly once in resolution.
#[tokio::test]
async fn test_diamond_dependency() -> Result<(), anyhow::Error> {
    use crate::id::{Label, Tag};
    use crate::package::metadata::manifest::SetMirror;
    use crate::package::publish::Publish;
    use crate::package::publish::git::GitPublisher;
    use crate::uri::Uri;

    init_tracing();

    // Setup: Create repo with remote
    let (repo_dir, remote_dir) = init_repo_and_remote()?;
    let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
    let local = repo.to_thread_local();

    // Initialize ekala
    local.ekala_init(None)?;

    // Get remote and initialize it
    let remote = local.find_remote("origin")?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;
    remote.ekala_init(None)?;

    let remote_url = format!("file://{}", remote_dir.path().display());

    // Phase 1: Create all atoms WITHOUT dependencies
    let repo = local.into_sync();

    let (_d_path, _d_oid) = repo.mock("atom-d", "1.0.0").await?;
    tracing::info!("Created atom-d (shared leaf)");

    let (b_path, _b_oid) = repo.mock("atom-b", "1.0.0").await?;
    tracing::info!("Created atom-b");

    let (c_path, _c_oid) = repo.mock("atom-c", "1.0.0").await?;
    tracing::info!("Created atom-c");

    let (a_path, _a_oid) = repo.mock("atom-a", "1.0.0").await?;
    tracing::info!("Created atom-a");

    let mirrors = vec![SetMirror::Url(gix::url::parse(remote_url.as_str().into())?)];

    // Publish D first (no dependencies)
    let local = repo.to_thread_local();
    {
        let progress = &tracing::info_span!("publish-d");
        let (paths, publisher) = GitPublisher::new(&local, "origin", "HEAD", progress)?.build()?;
        let label = Label::try_from("atom-d")?;
        if let Some(path) = paths.as_ref().get_by_left(&label) {
            publisher
                .publish_atom(path, &HashMap::new())?
                .expect("atoms failed to publish");
        }
        let mut errors = Vec::new();
        publisher.await_pushes(&mut errors).await;
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("publish D failed: {:?}", errors));
        }
        tracing::info!("Published atom-d");
    }

    // Add B -> D dependency
    let repo = local.into_sync();
    {
        let b_dir = b_path.parent().expect("atom path has parent");
        let mut writer = ManifestWriter::open_and_resolve(&repo, b_dir, true).await?;
        let uri: Uri = format!("{}::atom-d@^1.0", remote_url).parse()?;
        writer.add_uri(uri, Some(Tag::try_from("origin")?), mirrors.clone())?;
        writer.write_atomic()?;
        tracing::info!("Added atom-d dependency to atom-b");
    }

    // Add C -> D dependency
    {
        let c_dir = c_path.parent().expect("atom path has parent");
        let mut writer = ManifestWriter::open_and_resolve(&repo, c_dir, true).await?;
        let uri: Uri = format!("{}::atom-d@^1.0", remote_url).parse()?;
        writer.add_uri(uri, Some(Tag::try_from("origin")?), mirrors.clone())?;
        writer.write_atomic()?;
        tracing::info!("Added atom-d dependency to atom-c");
    }

    // Commit B and C changes, then publish them
    let local = repo.to_thread_local();
    commit_workdir(&local, "add atom-d deps to atom-b and atom-c")?;
    {
        let progress = &tracing::info_span!("publish-bc");
        let (paths, publisher) = GitPublisher::new(&local, "origin", "HEAD", progress)?.build()?;
        for label_str in ["atom-b", "atom-c"] {
            let label = Label::try_from(label_str)?;
            if let Some(path) = paths.as_ref().get_by_left(&label) {
                publisher
                    .publish_atom(path, &HashMap::new())?
                    .expect("atoms failed to publish");
            }
        }
        let mut errors = Vec::new();
        publisher.await_pushes(&mut errors).await;
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("publish B/C failed: {:?}", errors));
        }
        tracing::info!("Published atom-b and atom-c");
    }

    // Add A -> B, C dependencies
    let repo = local.into_sync();
    {
        let a_dir = a_path.parent().expect("atom path has parent");
        let mut writer = ManifestWriter::open_and_resolve(&repo, a_dir, true).await?;

        let uri_b: Uri = format!("{}::atom-b@^1.0", remote_url).parse()?;
        writer.add_uri(uri_b, Some(Tag::try_from("origin")?), mirrors.clone())?;

        let uri_c: Uri = format!("{}::atom-c@^1.0", remote_url).parse()?;
        writer.add_uri(uri_c, Some(Tag::try_from("origin")?), mirrors.clone())?;

        writer.write_atomic()?;
        tracing::info!("Added atom-b and atom-c dependencies to atom-a");
    }

    // WORKAROUND: Re-open to trigger transitive SAT resolution
    // TODO: Fix add_uri to call synchronize internally
    {
        let a_dir = a_path.parent().expect("atom path has parent");
        let writer = ManifestWriter::open_and_resolve(&repo, a_dir, false).await?;
        writer.write_atomic()?;
        tracing::info!("Ran full SAT resolution on atom-a");
    }

    // Verify lock file
    let a_dir = a_path.parent().expect("atom path has parent");
    let lock_path = a_dir.join("atom.lock");
    let lock_content = std::fs::read_to_string(&lock_path)?;
    tracing::info!("Lock file contents:\n{}", lock_content);

    let lock: crate::Lockfile = toml_edit::de::from_str(&lock_content)?;

    let mut d_count = 0;
    let mut has_b = false;
    let mut has_c = false;

    for (_key, dep) in lock.deps.as_ref().iter() {
        if let crate::package::metadata::lock::Dep::Atom(atom_dep) = dep {
            let label = atom_dep.label().as_ref();
            if label == "atom-d" {
                d_count += 1;
            }
            if label == "atom-b" {
                has_b = true;
            }
            if label == "atom-c" {
                has_c = true;
            }
        }
    }

    assert_eq!(
        d_count, 1,
        "atom-d should appear exactly once in lock file (diamond dedup), found {} times",
        d_count
    );
    assert!(has_b, "atom-b should be in lock file");
    assert!(has_c, "atom-c should be in lock file");

    tracing::info!("Diamond dependency test passed!");
    Ok(())
}
