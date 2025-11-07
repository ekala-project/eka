//! # Atom Publishing
//!
//! This module provides the types and logic necessary to efficiently publish atoms
//! to store implementations. The publishing system is designed to be safe, atomic,
//! and provide detailed feedback about the publishing process.
//!
//! ## Architecture
//!
//! The publishing system is built around two main traits:
//!
//! - [`Builder`] - Constructs and validates publishers before publishing
//! - [`Publish`] - Handles the actual publishing of atoms to stores
//!
//! ## Publishing Process
//!
//! 1. **Validation** - All atoms in the workspace are validated for consistency
//! 2. **Deduplication** - Duplicate atoms are detected and skipped
//! 3. **Publishing** - Valid atoms are published to the target store
//! 4. **Reporting** - Detailed statistics and results are provided
//!
//! ## Key Types
//!
//! - [`Record`] - Contains the result of publishing a single atom
//! - [`Stats`] - Aggregated statistics for a publishing operation
//! - [`Content`] - Backend-specific content information
//!
//! ## Backends
//!
//! The architecture is designed to support multiple backends.
//! - **Git** - Publishes atoms as Git objects in repositories (when `git` feature is enabled)
//!
//! ## Safety Features
//!
//! - **Atomic operations** - Failed publishes don't leave partial state
//! - **Duplicate detection** - Prevents accidental overwrites
//! - **Comprehensive validation** - Ensures atom consistency before publishing
//! - **Detailed error reporting** - Clear feedback on what succeeded or failed
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//!
//! use atom::package::publish::git::GitPublisher;
//! use atom::package::publish::{Builder, Publish, Stats};
//! use atom::storage::QueryVersion;
//! use atom::storage::git::Root;
//!
//! let repo = gix::open(".")?;
//! // Create a publisher for a Git repository
//! let progress_span = tracing::info_span!("test");
//! let publisher = GitPublisher::new(&repo, "origin", "main", &progress_span)?;
//!
//! // Build and validate the publisher
//! let (valid_atoms, publisher) = publisher.build()?;
//!
//! // query upstream store for remote atom refs to compare against
//! let remote = publisher.remote();
//! let remote_atoms = remote.remote_atoms(None);
//!
//! // Publish all atoms
//! let results = publisher.publish(vec![PathBuf::from("/path/to/atom")], remote_atoms);
//!
//! // Check results
//! let stats = Stats::default();
//! for result in results {
//!     match result {
//!         Ok(outcome) => todo!(), // e.g. log `outcome`
//!         Err(e) => println!("Failed: {:?}", e),
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use self::git::GitContent;
use crate::AtomId;
use crate::id::Label;
use crate::package::metadata::AtomMap;

pub mod error;
pub mod git;
mod private {
    /// a marker trait to seal the [`Publish<R>`] trait
    pub trait Sealed {}
}

//================================================================================================
// Constants
//================================================================================================

const ATOM_FORMAT_VERSION: &str = "pre1";
const ATOM_MANIFEST: &str = "manifest";
const ATOM_META_REF: &str = "meta";
const ATOM_ORIGIN: &str = "origin";
const ATOM_REF: &str = "atoms";
const EMPTY_SIG: &str = "";
const STORE_ROOT: &str = "eka";

//================================================================================================
// Statics
//================================================================================================

/// The default location where atom refs are stored.
pub static ATOM_REFS: LazyLock<String> =
    LazyLock::new(|| format!("{}/{}", REF_ROOT.as_str(), ATOM_REF));
static META_REFS: LazyLock<String> =
    LazyLock::new(|| format!("{}/{}", REF_ROOT.as_str(), ATOM_META_REF));
static REF_ROOT: LazyLock<String> = LazyLock::new(|| format!("refs/{}", STORE_ROOT));

//================================================================================================
// Types
//================================================================================================

/// Basic statistics collected during a publishing request.
#[derive(Default)]
pub struct Stats {
    /// The number of atoms that were successfully published.
    pub published: u32,
    /// The number of atoms that were skipped because they already existed in the store.
    pub skipped: u32,
    /// The number of atoms that failed to publish due to an error.
    pub failed: u32,
}

/// Contains backend-specific content information for reporting results to the user.
pub enum Content {
    /// Content specific to the Git implementation.
    Git(GitContent),
}

/// Represents the result of publishing a single atom, for reporting to the user.
pub struct Record<R> {
    id: AtomId<R>,
    content: Content,
}

/// A `Result` indicating that an atom may have been skipped.
///
/// This is used instead of an `Option` to provide information about *which*
/// atom was skipped, which is useful for reporting but does not represent a
/// failure condition.
type MaybeSkipped<T> = Result<T, Label>;

/// The outcome of an attempt to publish a single atom.
///
/// This is either a [`Record`] for a successful publication or an [`Label`]
/// if the atom was safely skipped.
type PublishOutcome<R> = MaybeSkipped<Record<R>>;

//================================================================================================
// Traits
//================================================================================================

/// A builder for a [`Publish`] implementation.
///
/// This trait is central to ensuring that vital invariants for maintaining a
/// clean and consistent state in the store are verified before any publishing
/// can occur. A [`Publish`] implementation can only be constructed through a
/// builder.
pub trait Builder {
    /// The error type returned by the [`Builder::build`] method.
    type Error;
    /// The [`Publish`] implementation to construct.
    type Publisher: Publish;
    /// Collects and validates all atoms in the worktree.
    ///
    /// This method must be called before publishing to ensure that there are
    /// no duplicate atoms. It is the only way to construct a [`Publish`]
    /// implementation.
    fn build(self) -> Result<(AtomMap, Self::Publisher), Self::Error>;
}

/// The primary trait for exposing atom publishing logic for a given store.
pub trait Publish: private::Sealed {
    /// The error type returned by the publisher.
    type Error;
    /// Represents the type which serves as the genesis in atom id calculation
    type Genesis;
    /// The type representing the machine-readable identity for a specific version of an atom.
    type Id;

    /// Publishes a collection of atoms.
    ///
    /// This function processes a collection of paths, each representing an atom
    /// to be published, by calling [`Publish::publish_atom`] for each path.
    ///
    /// # Error Handling
    ///
    /// The function processes all provided paths, even if some fail. Errors
    /// and skipped atoms are collected as results but do not halt the overall
    /// process.
    ///
    /// # Return Value
    ///
    /// Returns a vector of results. The outer `Result` represents a failure
    /// to publish an atom, while the inner `Result` (PublishOutcome)
    /// indicates whether an atom was published or safely skipped because it
    /// already exists.
    fn publish<C>(
        &self,
        paths: C,
        remotes: HashMap<Label, (semver::Version, Self::Id)>,
    ) -> Vec<Result<PublishOutcome<Self::Genesis>, Self::Error>>
    where
        C: IntoIterator<Item = PathBuf>;

    /// Publishes a single atom.
    ///
    /// This function takes a single path and publishes the atom located there.
    ///
    /// # Return Value
    ///
    /// - An [`Ok`] variant containing a PublishOutcome, which is either the [`Record<R>`] of the
    ///   successfully published atom or the [`Label`] if it was safely skipped.
    /// - An [`Err`] variant containing a [`Self::Error`] if the atom could not be published for any
    ///   reason (e.g., an invalid manifest).
    fn publish_atom<P: AsRef<Path>>(
        &self,
        path: P,
        remotes: &HashMap<Label, (semver::Version, Self::Id)>,
    ) -> Result<PublishOutcome<Self::Genesis>, Self::Error>;
}

/// Validates the state of the atom source.
///
/// This trait is called during the construction of a publisher to ensure that
/// the store is never allowed to enter an inconsistent state. Any conditions
/// that would result in an inconsistent state will return an error, making it
/// impossible to construct a publisher until the state is corrected.
trait StateValidator {
    type Error;
    type Publisher: Publish;

    /// Validates the state of the atom source.
    fn validate(publisher: &Self::Publisher) -> Result<AtomMap, Self::Error>;
}

//================================================================================================
// Impls
//================================================================================================

impl<R> Record<R> {
    /// Returns a reference to the [`AtomId`] in the record.
    pub fn id(&self) -> &AtomId<R> {
        &self.id
    }

    /// Returns a reference to the [`Content`] of the record.
    pub fn content(&self) -> &Content {
        &self.content
    }
}
