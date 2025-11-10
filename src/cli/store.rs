//! This module handles the detection of the underlying version control system.

use std::fs;

use atom::storage::{LocalStoragePath, git};
use gix::ThreadSafeRepository;

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
    None,
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
    } else if let Ok(local) = LocalStoragePath::new(std::env::current_dir()?) {
        Ok(Detected::FileStorage(local))
    } else {
        Ok(Detected::None)
    }
}
