//! # Publishing Errors
//!
//! This module contains the error types for errors that might occur during publishing.
use thiserror::Error;

#[derive(Error, Debug)]
/// The error representing a failure during publishing for any store implementation.
pub enum PublishError {
    #[cfg(feature = "git")]
    /// A transparent wrapper for a [`GitError`].
    #[error(transparent)]
    Git(#[from] git::Error),
}

#[cfg(feature = "git")]
pub mod git {
    //! # Git Publishing Errors
    use std::path::PathBuf;

    use gix::object;

    use crate::store::git::Root;
    /// An error representing a failure during publishing to a Git Ekala store.
    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        /// A transparent wrapper for a [`Box<gix::remote::find::existing::Error>`]
        #[error(transparent)]
        RemotNotFound(#[from] Box<gix::remote::find::existing::Error>),
        /// A transparent wrapper for a [`Box<gix::revision::spec::parse::single::Error>`]
        #[error(transparent)]
        RevParseFailed(#[from] Box<gix::revision::spec::parse::single::Error>),
        /// A transparent wrapper for a [`object::find::existing::with_conversion::Error`]
        #[error(transparent)]
        NoCommit(#[from] object::find::existing::with_conversion::Error),
        /// A transparent wrapper for a [`object::commit::Error`]
        #[error(transparent)]
        NoTree(#[from] object::commit::Error),
        /// A transparent wrapper for a [`object::find::existing::Error`]
        #[error(transparent)]
        NoObject(#[from] object::find::existing::Error),
        /// A transparent wrapper for a [`gix::hasher::Error`]
        #[error(transparent)]
        Hash(#[from] gix::hash::hasher::Error),
        /// A transparent wrapper for a [`object::write::Error`]
        #[error(transparent)]
        WriteFailed(#[from] object::write::Error),
        /// A transparent wrapper for a [`gix::reference::edit::Error`]
        #[error(transparent)]
        RefUpdateFailed(#[from] gix::reference::edit::Error),
        /// A transparent wrapper for a [`gix::revision::walk::Error`]
        #[error(transparent)]
        CalculatingRootFailed(#[from] gix::revision::walk::Error),
        /// A transparent wrapper for a [`gix::traverse::commit::simple::Error`]
        #[error(transparent)]
        RootConversionFailed(#[from] gix::traverse::commit::simple::Error),
        /// A transparent wrapper for a [`std::io::Error`]
        #[error(transparent)]
        Io(#[from] std::io::Error),
        /// A transparent wrapper for a [`tokio::task::JoinError`]
        #[error(transparent)]
        JoinFailed(#[from] tokio::task::JoinError),
        /// The reported root & the atom root are inconsistent.
        #[error("Atom does not derive from the initialized history")]
        InconsistentRoot {
            /// The root according to the remote we are publishing to.
            remote: Root,
            /// The root of history for the source from which the atom is derived.
            atom: Root,
        },
        /// The remote is not initialized as an Ekala store.
        #[error("Remote is not initialized")]
        NotInitialized,
        /// The Atom manifest is invalid, and this Atom will be ignored.
        #[error("Ignoring invalid Atom manifest")]
        Invalid(#[source] crate::manifest::AtomError, Box<PathBuf>),
        /// The path given does not point to an Atom.
        #[error("The given path does not point to an Atom")]
        NotAnAtom(PathBuf),
        /// An atom exist's at the repo root.
        #[error("Atoms cannot exist at the repo root")]
        NoRootAtom,
        /// Failed to sync a least one Atom to the remote.
        #[error("Failed to sync some Atoms to the remote")]
        SomePushFailed,
        /// Some Atoms failed to publish
        #[error("Failed to published some of the specified Atoms")]
        Failed,
        /// A transparent wrapper for a [`crate::store::git::Error`]
        #[error(transparent)]
        StoreError(#[from] crate::store::git::Error),
        /// No Atoms found under the given directory.
        #[error("Failed to find any Atoms under the current directory")]
        NotFound,
        /// Atoms with the same Unicode ID were found in the given revision.
        #[error("Duplicate Atoms detected in the given revision, refusing to publish")]
        Duplicates,
    }

    #[cfg(feature = "git")]
    impl Error {
        const INCONSISTENT_ROOT_SUGGESTION: &str =
            "You may need to reinitalize the remote if the issue persists";

        /// Warn the user about specific error conditions encountered during publishing.
        pub fn warn(&self) {
            match self {
                Error::InconsistentRoot { remote, atom } => {
                    tracing::warn!(
                        message = %self,
                        atom_root = %**atom,
                        remote_root = %**remote,
                        suggest = Error::INCONSISTENT_ROOT_SUGGESTION
                    );
                },
                Error::Invalid(e, path) => {
                    tracing::warn!(message = %self, path = %path.display(), message = format!("\n{}", e));
                },
                Error::NotAnAtom(path) => {
                    tracing::warn!(message = %self, path = %path.display());
                },
                Error::Failed => (),
                _ => tracing::warn!(message = %self),
            }
        }
    }
}
