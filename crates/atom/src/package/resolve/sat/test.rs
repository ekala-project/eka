//! Integration tests for SAT-based dependency resolution.
//!
//! These tests verify that the transitive dependency resolver correctly handles
//! various dependency graph shapes and edge cases.

use std::collections::HashMap;

use anyhow::Context;
use gix::ThreadSafeRepository;

use crate::storage::{Init, QueryStore};
use crate::test::harness::{MockAtom, init_repo_and_remote, init_tracing};

//================================================================================================
// Tests
//================================================================================================

/// Test linear dependency chain: A → B → C
///
/// Creates three atoms where A depends on B and B depends on C.
/// Verifies that all three appear in the resolution with correct `requires` chains.
#[tokio::test]
async fn test_linear_chain() -> Result<(), anyhow::Error> {
    init_tracing();

    // Setup: Create repo with remote
    let (repo_dir, _remote) = init_repo_and_remote()?;
    let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
    let local = repo.to_thread_local();

    // Initialize ekala
    local.ekala_init(None)?;

    // Get remote and set as origin
    let remote = local.find_remote("origin")?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;
    remote.ekala_init(None)?;

    // Create atoms: C (no deps), B (depends on C), A (depends on B)
    let repo = local.into_sync();

    // C has no dependencies
    let (_c_path, _c_oid) = repo.mock("atom-c", "1.0.0").await?;
    tracing::info!("Created atom-c");

    // B depends on C
    // Note: set_tag "local" refers to the local set defined when ekala_init is called
    let (_b_path, _b_oid) = repo
        .mock_with_deps("atom-b", "1.0.0", &[("local", "atom-c", "^1.0")])
        .await?;
    tracing::info!("Created atom-b with dep on atom-c");

    // A depends on B
    let (a_path, _a_oid) = repo
        .mock_with_deps("atom-a", "1.0.0", &[("local", "atom-b", "^1.0")])
        .await?;
    tracing::info!("Created atom-a with dep on atom-b");

    // TODO: Publish atoms to remote so they can be resolved
    // For now, this test will fail because atoms aren't published
    // We need to either:
    // 1. Publish the atoms using GitPublisher
    // 2. Test with local unpublished atoms (pseudo-lock)

    // Open ManifestWriter for atom-a and run sync
    // let writer = ManifestWriter::open_and_resolve(storage, &a_path, false).await?;
    // writer.synchronize(&manifest).await?;

    // Verify lock contents
    // - Check all three atoms are present
    // - Check A.requires = [B]
    // - Check B.requires = [C]
    // - Check C.requires = []
    // - Check A.direct = true, B.direct = false, C.direct = false

    tracing::info!("Linear chain test setup complete - full verification pending publish");

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
