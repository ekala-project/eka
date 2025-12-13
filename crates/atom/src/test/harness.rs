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

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use gix::objs::Tree;
use gix::objs::tree::Entry;
use gix::{ObjectId, ThreadSafeRepository};
use semver::Version;
use tempfile::TempDir;

use crate::EkalaManager;
use crate::storage::EkalaStorage;

//================================================================================================
// Functions
//================================================================================================

/// Creates a pair of temporary git repositories: a local repo and a bare remote.
///
/// The repos are configured with:
/// - A remote named "origin" pointing to the bare remote
/// - User email/name for commits
/// - An initial commit in the remote
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

    let no_parents: Vec<gix::ObjectId> = vec![];
    let init = remote.commit_as(
        sig,
        sig,
        "HEAD",
        "init",
        repo.empty_tree().id(),
        no_parents.clone(),
    )?;
    remote.commit_as(
        sig,
        sig,
        "HEAD",
        "2nd",
        repo.empty_tree().id(),
        vec![init.detach()],
    )?;

    let config_file = repo.git_dir().join("config");
    let mut config = File::from_path_no_includes(config_file.clone(), Source::Local)?;
    let mut repo_remote =
        repo.remote_at(format!("file://{}", remote.git_dir().display()).as_str())?;
    repo_remote.save_as_to("origin", &mut config)?;
    config.set_raw_value(&"user.email", "eka")?;
    config.set_raw_value(&"user.name", "eka")?;
    let mut file = std::fs::File::create(config_file)?;
    config.write_to(&mut file)?;

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
    fn mock_with_deps(
        &self,
        label: &str,
        version: &str,
        deps: &[(&str, &str, &str)], // (set_tag, dep_label, version_req)
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

        let buf = std::fs::read_to_string(&atom_file)?;

        let mode = atom_file.metadata()?.mode();
        let filename = atom_file
            .strip_prefix(&atom_dir)?
            .display()
            .to_string()
            .into();
        let oid = repo.write_blob(buf.as_bytes())?.detach();
        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let oid = repo.write_object(tree)?.detach();

        let filename = atom_dir
            .to_path_buf()
            .strip_prefix(work_dir)?
            .display()
            .to_string()
            .into();

        let entry_dir = Entry {
            mode: TryFrom::try_from(0o40000)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let filename = crate::EKALA_MANIFEST_NAME.to_string().into();
        let buf = std::fs::read(
            repo.ekala_root_dir()?
                .join(crate::EKALA_MANIFEST_NAME.as_str()),
        )?;
        let oid = repo.write_blob(buf)?.detach();

        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry, entry_dir],
        };

        let oid = repo.write_object(tree)?.detach();

        let head = repo.head_id()?;
        let head_ref = repo.head_ref()?.context("detached HEAD")?;

        let atom_oid = repo
            .commit(
                head_ref.name().as_bstr(),
                format!("init: {}", label),
                oid,
                vec![head],
            )?
            .detach();

        Ok((atom_file, atom_oid))
    }

    async fn mock_with_deps(
        &self,
        label: &str,
        version: &str,
        deps: &[(&str, &str, &str)], // (set_tag, dep_label, version_req)
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

        // Read and modify manifest to add deps
        let manifest_content = std::fs::read_to_string(&atom_file)?;
        let mut doc: toml_edit::DocumentMut = manifest_content.parse()?;

        // Add deps.from.<set_tag>.<label> = "<version_req>" for each dep
        if !deps.is_empty() {
            let deps_table = doc
                .entry("deps")
                .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                .as_table_mut()
                .context("deps is not a table")?;

            let from_table = deps_table
                .entry("from")
                .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                .as_table_mut()
                .context("from is not a table")?;

            for (set_tag, dep_label, version_req) in deps {
                let set_table = from_table
                    .entry(*set_tag)
                    .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                    .as_table_mut()
                    .context("set is not a table")?;
                set_table.set_implicit(true);
                set_table[*dep_label] = toml_edit::value(version_req.to_string());
            }
        }

        // Write modified manifest
        std::fs::write(&atom_file, doc.to_string())?;

        // Now create the git objects (same as mock())
        let buf = std::fs::read_to_string(&atom_file)?;

        let mode = atom_file.metadata()?.mode();
        let filename = atom_file
            .strip_prefix(&atom_dir)?
            .display()
            .to_string()
            .into();
        let oid = repo.write_blob(buf.as_bytes())?.detach();
        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let oid = repo.write_object(tree)?.detach();

        let filename = atom_dir
            .to_path_buf()
            .strip_prefix(work_dir)?
            .display()
            .to_string()
            .into();

        let entry_dir = Entry {
            mode: TryFrom::try_from(0o40000)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let filename = crate::EKALA_MANIFEST_NAME.to_string().into();
        let buf = std::fs::read(
            repo.ekala_root_dir()?
                .join(crate::EKALA_MANIFEST_NAME.as_str()),
        )?;
        let oid = repo.write_blob(buf)?.detach();

        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry, entry_dir],
        };

        let oid = repo.write_object(tree)?.detach();

        let head = repo.head_id()?;
        let head_ref = repo.head_ref()?.context("detached HEAD")?;

        let atom_oid = repo
            .commit(
                head_ref.name().as_bstr(),
                format!("init with deps: {}", label),
                oid,
                vec![head],
            )?
            .detach();

        Ok((atom_file, atom_oid))
    }
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
