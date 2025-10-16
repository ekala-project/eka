//! # Publishing Errors
//!
//! This module contains the error types for errors that might occur during publishing.

use thiserror::Error;

pub mod git {
    //! # Git Publishing Errors
    //!
    //! This module contains error types specific to publishing to a Git-based Ekala store.

    use std::path::PathBuf;

    use gix::object;

    use crate::store::git::Root;

    //================================================================================================
    // Types
    //================================================================================================

    /// An error representing a failure during publishing to a Git Ekala store.
    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        /// Failed to calculate the repository's root commit.
        #[error(transparent)]
        CalculatingRootFailed(#[from] gix::revision::walk::Error),
        /// Atoms with the same Unicode ID were found in the given revision.
        #[error("Duplicate Atoms detected in the given revision, refusing to publish")]
        Duplicates,
        /// Some Atoms failed to publish.
        #[error("Failed to published some of the specified Atoms")]
        Failed,
        /// A hashing-related error occurred.
        #[error(transparent)]
        Hash(#[from] gix::hash::hasher::Error),
        /// The reported root and the atom root are inconsistent.
        #[error("Atom does not derive from the initialized history")]
        InconsistentRoot {
            /// The root according to the remote we are publishing to.
            remote: Root,
            /// The root of history for the source from which the atom is derived.
            atom: Root,
        },
        /// The Atom manifest is invalid, and this Atom will be ignored.
        #[error("Ignoring invalid Atom manifest")]
        Invalid(#[source] crate::manifest::AtomError, Box<PathBuf>),
        /// An I/O error occurred.
        #[error(transparent)]
        Io(#[from] std::io::Error),
        /// A `tokio` task failed to join.
        #[error(transparent)]
        JoinFailed(#[from] tokio::task::JoinError),
        /// Failed to find a commit object.
        #[error(transparent)]
        NoCommit(#[from] object::find::existing::with_conversion::Error),
        /// Failed to find a git object.
        #[error(transparent)]
        NoObject(#[from] object::find::existing::Error),
        /// An atom exists at the repository root, which is not allowed.
        #[error("Atoms cannot exist at the repo root")]
        NoRootAtom,
        /// Failed to find a tree object from a commit.
        #[error(transparent)]
        NoTree(#[from] object::commit::Error),
        /// The path given does not point to an Atom.
        #[error("The given path does not point to an Atom")]
        NotAnAtom(PathBuf),
        /// No Atoms were found under the given directory.
        #[error("Failed to find any Atoms under the current directory")]
        NotFound,
        /// The remote is not initialized as an Ekala store.
        #[error("Remote is not initialized")]
        NotInitialized,
        /// Failed to update a git reference.
        #[error(transparent)]
        RefUpdateFailed(#[from] gix::reference::edit::Error),
        /// The specified remote was not found.
        #[error(transparent)]
        RemoteNotFound(#[from] Box<gix::remote::find::existing::Error>),
        /// Failed to parse a git revision specification.
        #[error(transparent)]
        RevParseFailed(#[from] Box<gix::revision::spec::parse::single::Error>),
        /// Failed to convert a commit for traversal.
        #[error(transparent)]
        RootConversionFailed(#[from] gix::traverse::commit::simple::Error),
        /// Failed to sync at least one Atom to the remote.
        #[error("Failed to sync some Atoms to the remote")]
        SomePushFailed,
        /// An error occurred within the git store.
        #[error(transparent)]
        StoreError(#[from] Box<crate::store::git::Error>),
        /// Failed to write a git object.
        #[error(transparent)]
        WriteFailed(#[from] object::write::Error),
    }

    //================================================================================================
    // Impls
    //================================================================================================

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

/// The error representing a failure during publishing for any store implementation.
#[derive(Error, Debug)]
pub enum PublishError {
    /// A git-related publishing error.
    #[error(transparent)]
    Git(#[from] git::Error),
}
