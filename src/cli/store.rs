use atom::store::git;
use gix::ThreadSafeRepository;
use thiserror::Error;

#[non_exhaustive]
#[derive(Clone, Debug)]
pub(super) enum Detected {
    Git(&'static ThreadSafeRepository),
    #[allow(dead_code)]
    None,
}

pub(super) async fn detect() -> Result<Detected, Error> {
    if let Ok(Some(repo)) = git::repo() {
        use std::fs;
        let git_dir = fs::canonicalize(repo.path())
            .ok()
            .map(|p| p.display().to_string());
        let work_dir = repo
            .work_dir()
            .and_then(|dir| fs::canonicalize(dir).ok())
            .map(|p| p.display().to_string());

        tracing::debug!(message = "Detected Git repository", git_dir, work_dir);
        return Ok(Detected::Git(repo));
    }

    Err(Error::FailedDetection)
}

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("No supported repository found in this directory or its parents")]
    FailedDetection,

    #[error(transparent)]
    Discover(#[from] Box<gix::discover::Error>),
}
