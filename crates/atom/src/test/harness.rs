//! # Test Harness
//!
//! Shared utilities for creating mock atoms and repositories in tests.
//!
//! ## Overview
//!
//! This module provides the infrastructure needed to test atom operations:
//! - `init_repo_and_remote()` - Creates temporary git repos with proper config
//! - `MockAtom` trait - Creates mock atoms with manifests and dependencies
//!
//! ## Usage
//!
//! ```ignore
//! use atom::test::harness::{init_repo_and_remote, MockAtom};
//!
//! let (repo_dir, remote_dir) = init_repo_and_remote()?;
//! let repo = ThreadSafeRepository::open(repo_dir.as_ref())?;
//! let (path, oid) = repo.mock("my-atom", "1.0.0").await?;
//! ```

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use gix::{ObjectId, ThreadSafeRepository};
use semver::Version;
use tempfile::TempDir;

use crate::EkalaManager;

//================================================================================================
// Functions
//================================================================================================

/// Creates a pair of temporary git repositories: a local repo and a bare remote.
///
/// The repos are configured with:
/// - A remote named "origin" pointing to the bare remote
/// - User email/name for commits
/// - Initial commits in the remote
/// - Local repo fetches from remote so they share history (same genesis)
///
/// Returns `(local_repo_dir, remote_dir)` - both are `TempDir` that clean up on drop.
pub fn init_repo_and_remote() -> Result<(TempDir, TempDir), anyhow::Error> {
    use gix::actor::SignatureRef;
    use gix::config::{File, Source};

    let sig = SignatureRef::default();
    let repo_dir = tempfile::tempdir()?;
    let remote_dir = tempfile::tempdir()?;
    let repo = gix::init(repo_dir.as_ref())?;
    let remote = gix::init_bare(remote_dir.as_ref())?;

    // Create initial commits in the remote
    let no_parents: Vec<gix::ObjectId> = vec![];
    let init = remote.commit_as(
        sig,
        sig,
        "HEAD",
        "init",
        remote.empty_tree().id(),
        no_parents.clone(),
    )?;
    remote.commit_as(
        sig,
        sig,
        "HEAD",
        "2nd",
        remote.empty_tree().id(),
        vec![init.detach()],
    )?;

    // Configure local repo with remote
    let config_file = repo.git_dir().join("config");
    let mut config = File::from_path_no_includes(config_file.clone(), Source::Local)?;
    let mut repo_remote =
        repo.remote_at(format!("file://{}", remote.git_dir().display()).as_str())?;
    repo_remote.save_as_to("origin", &mut config)?;
    config.set_raw_value(&"user.email", "eka")?;
    config.set_raw_value(&"user.name", "eka")?;
    let mut file = std::fs::File::create(config_file)?;
    config.write_to(&mut file)?;

    // Fetch from remote so local shares history (uses existing storage API)
    use crate::storage::QueryStore;
    let repo = gix::ThreadSafeRepository::open(repo_dir.as_ref())?.to_thread_local();
    let origin = repo.find_remote("origin")?;
    origin.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;

    // Set local HEAD to point to master branch (symbolic ref, not detached)
    if let Ok(head_ref) = repo.find_reference("refs/heads/master") {
        // Create/update refs/heads/master with the fetched commit
        repo.reference(
            "refs/heads/master",
            head_ref.id().detach(),
            gix::refs::transaction::PreviousValue::Any,
            "sync from remote",
        )?;
        // Make HEAD a symbolic reference to master (not detached)
        std::fs::write(repo.git_dir().join("HEAD"), "ref: refs/heads/master\n")?;
    }

    Ok((repo_dir, remote_dir))
}

//================================================================================================
// Traits
//================================================================================================

/// Trait for creating mock atoms in a repository.
///
/// This trait is implemented on `ThreadSafeRepository` to enable creating
/// atoms with manifests for testing purposes.
pub trait MockAtom {
    /// Creates a mock atom with the given label and version.
    ///
    /// Returns `(manifest_path, commit_oid)`.
    fn mock(
        &self,
        label: &str,
        version: &str,
    ) -> impl std::future::Future<Output = Result<(PathBuf, ObjectId), anyhow::Error>>;

    /// Creates a mock atom with dependencies.
    ///
    /// Dependencies are specified as `(set_tag, label, version_req)` tuples.
    /// The set_tag should match a set defined in the ekala.toml.
    ///
    /// Returns `(manifest_path, commit_oid)`.
    ///
    /// `remote_url` is the URL to the remote where dependencies are published.
    fn mock_with_deps(
        &self,
        label: &str,
        version: &str,
        deps: &[(&str, &str, &str)], // (set_tag, dep_label, version_req)
        remote_url: Option<&str>,    // Optional remote URL for the set mirror
    ) -> impl std::future::Future<Output = Result<(PathBuf, ObjectId), anyhow::Error>>;
}

//================================================================================================
// Impls
//================================================================================================

impl MockAtom for ThreadSafeRepository {
    async fn mock(&self, label: &str, version: &str) -> Result<(PathBuf, ObjectId), anyhow::Error> {
        let repo = self.to_thread_local();
        let work_dir = repo.workdir().context("No workdir")?;
        let atom_dir = work_dir.join(label);
        let atom_file = atom_dir.join(crate::ATOM_MANIFEST_NAME.as_str());

        let mut ekala = EkalaManager::open(self)?;
        ekala
            .new_atom_at_path(label.try_into()?, &atom_dir, Version::from_str(version)?)
            .await?;

        // Commit the entire workdir state using build_tree_recursive
        let atom_oid = commit_workdir(&repo, &format!("init: {}", label))?;

        Ok((atom_file, atom_oid))
    }

    /// Creates an atom with dependencies.
    ///
    /// `deps` is a slice of (set_tag, dep_label, version_req) tuples.
    /// `remote_url` is the URL to the remote where dependencies are published.
    async fn mock_with_deps(
        &self,
        label: &str,
        version: &str,
        deps: &[(&str, &str, &str)], // (set_tag, dep_label, version_req)
        remote_url: Option<&str>,    // Optional remote URL for the set mirror
    ) -> Result<(PathBuf, ObjectId), anyhow::Error> {
        let repo = self.to_thread_local();
        let work_dir = repo.workdir().context("No workdir")?;
        let atom_dir = work_dir.join(label);
        let atom_file = atom_dir.join(crate::ATOM_MANIFEST_NAME.as_str());

        // Create base atom
        let mut ekala = EkalaManager::open(self)?;
        ekala
            .new_atom_at_path(label.try_into()?, &atom_dir, Version::from_str(version)?)
            .await?;

        // Add dependencies using add_uri - the high-level API that simulates `eka add`
        if !deps.is_empty() {
            use crate::id::Tag;
            use crate::package::metadata::manifest::ManifestWriter;
            use crate::uri::Uri;

            // Open the atom with ManifestWriter
            let mut writer = ManifestWriter::open_and_resolve(self, &atom_dir, true).await?;

            for (set_tag, dep_label, version_req) in deps {
                // Construct URI with remote URL: "url::dep-label@^version" or just
                // "dep-label@version"
                let uri_str = if let Some(url) = remote_url {
                    format!("{}::{}@{}", url, dep_label, version_req)
                } else {
                    format!("{}@{}", dep_label, version_req)
                };
                let uri: Uri = uri_str.parse()?;
                let tag = Some(Tag::try_from(*set_tag)?);

                // Use add_uri with additional mirrors
                writer.add_uri(uri, tag, vec![])?;
            }

            // Write the changes
            writer.write_atomic()?;
        }

        // Commit the entire workdir state using build_tree_recursive
        let atom_oid = commit_workdir(&repo, &format!("init with deps: {}", label))?;

        Ok((atom_file, atom_oid))
    }
}

/// Helper to commit the entire working directory state.
pub fn commit_workdir(repo: &gix::Repository, message: &str) -> Result<ObjectId, anyhow::Error> {
    use std::path::PathBuf;

    use crate::storage::git::cache::{build_tree_recursive, collect_entries};

    let work_dir = repo.workdir().context("No workdir")?;

    // Build tree from entire workdir using proper recursive function
    let entries = collect_entries(work_dir)?;
    let tree_oid = build_tree_recursive(repo, PathBuf::new().as_path(), &entries, work_dir, 0)?;

    let head = repo.head_id()?;
    let head_ref = repo.head_ref()?.context("detached HEAD")?;

    let commit_oid = repo
        .commit(head_ref.name().as_bstr(), message, tree_oid, vec![head])?
        .detach();

    Ok(commit_oid)
}

//================================================================================================
// Test Helpers
//================================================================================================

/// Initializes tracing for tests (respects RUST_LOG env var).
pub fn init_tracing() {
    use tracing_subscriber::filter::EnvFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let _ = tracing_subscriber::registry()
        .with(fmt::layer().compact())
        .with(EnvFilter::from_default_env())
        .try_init();
}
