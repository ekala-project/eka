//! This module handles the detection of the underlying version control system.

use std::fs;

use atom::storage::{LocalStoragePath, git};
use gix::ThreadSafeRepository;
use thiserror::Error;

//================================================================================================
// Types
//================================================================================================

/// Represents the detected version control system.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub(super) enum Detected {
    /// A Git repository was detected.
    Git(&'static ThreadSafeRepository),
    FileStorage(atom::storage::LocalStoragePath),
}

/// Errors that can occur during repository detection.
#[derive(Error, Debug)]
pub(crate) enum Error {
    /// No supported repository was found in the current directory or its parents.
    #[error("No supported repository found in this directory or its parents")]
    FailedDetection,
    /// An error occurred while discovering the repository.
    #[error(transparent)]
    Git(#[from] Box<gix::discover::Error>),
    #[error(transparent)]
    Local(#[from] atom::storage::StorageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

//================================================================================================
// Functions
//================================================================================================

/// Detects the version control system in the current directory.
pub(super) fn detect() -> anyhow::Result<Detected> {
    if let Ok(Some(repo)) = git::repo() {
        let git_dir = fs::canonicalize(repo.path())
            .ok()
            .map(|p| p.display().to_string());
        let work_dir = repo
            .work_dir()
            .and_then(|dir| fs::canonicalize(dir).ok())
            .map(|p| p.display().to_string());

        tracing::debug!(message = "Detected Git repository", git_dir, work_dir);
        Ok(Detected::Git(repo))
    } else {
        Ok(Detected::FileStorage(LocalStoragePath::new(
            std::env::current_dir()?,
        )?))
    }
}
