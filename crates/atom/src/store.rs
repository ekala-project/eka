//! # Atom Store Interface
//!
//! This module defines the core traits and interfaces for implementing storage
//! backends for atoms. The store abstraction allows atoms to be stored and
//! retrieved from different types of storage systems.
//!
//! ## Architecture
//!
//! The store system is designed around two main traits:
//!
//! - [`Init`] - Handles store initialization and root calculation
//! - [`NormalizeStorePath`] - Normalizes paths relative to store roots
//!
//! ## Storage Backends
//!
//! Currently supported backends:
//! - **Git** - Stores atoms as Git objects in repositories (when `git` feature is enabled)
//!
//! Future backends may include:
//! - **HTTP/HTTPS** - Remote storage over HTTP APIs
//! - **Local filesystem** - Direct filesystem storage
//! - **S3-compatible** - Cloud storage backends
//!
//! ## Key Concepts
//!
//! **Store Root**: A unique identifier that represents the base commit or
//! state of the store. This is used as part of atom identity calculation.
//!
//! **Path Normalization**: Converting user-provided paths to canonical paths
//! relative to the store root, handling both relative and absolute paths correctly.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use atom::store::git::Root;
//! use atom::store::{Init, NormalizeStorePath};
//! use gix::Remote;
//!
//! // Initialize a Git store
//! let repo = gix::open(".")?;
//! let remote = repo.find_remote("origin")?;
//! remote.ekala_init()?;
//! let root = remote.ekala_root()?;
//!
//! // Normalize a path
//! let normalized = repo.normalize("path/to/atom")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#[cfg(feature = "git")]
pub mod git;
use std::path::{Path, PathBuf};

use bstr::BStr;

/// A trait representing the methods required to initialize an Ekala store.
pub trait Init<R, O> {
    /// The error type returned by the methods of this trait.
    type Error;
    /// Sync with the Ekala store, for implementations that require it.
    fn sync(&self) -> Result<O, Self::Error>;
    /// Initialize the Ekala store.
    fn ekala_init(&self) -> Result<(), Self::Error>;
    /// Returns the root as reported by the remote store, or an error if it is inconsistent.
    fn ekala_root(&self) -> Result<R, Self::Error>;
}

/// A trait containing a path normalization method, to normalize paths in an Ekala store
/// relative to its root.
pub trait NormalizeStorePath {
    /// The error type returned by the [`NormalizeStorePath::normalize`] function.
    type Error;
    /// Normalizes a given path to be relative to the store root.
    ///
    /// This function takes a path (relative or absolute) and attempts to normalize it
    /// relative to the store root, based on the current working directory within
    /// the store within system.
    ///
    /// # Behavior:
    /// - For relative paths (e.g., "foo/bar" or "../foo"):
    ///   - Interpreted as relative to the current working directory within the repository.
    ///   - Computed relative to the repository root.
    ///
    /// - For absolute paths (e.g., "/foo/bar"):
    ///   - Treated as if the repository root is the filesystem root.
    ///   - The leading slash is ignored, and the path is considered relative to the repo root.
    fn normalize<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf, Self::Error>;
}

/// A trait for querying remote stores to retrieve references.
///
/// This trait provides a unified interface for querying references from remote
/// stores. It supports different query strategies depending on the implementation.
///
/// ## Examples
///
/// ```rust,no_run
/// use atom::store::QueryStore;
/// use gix::Url;
///
/// // Lightweight reference query
/// let url = gix::url::parse("https://github.com/example/repo.git".into())?;
/// let refs = url.get_refs(["main", "refs/tags/v1.0"])?;
/// for ref_info in refs {
///     let (name, target, peeled) = ref_info.unpack();
///     println!("Ref: {}, Target: {}", name, peeled.or(target).unwrap());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait QueryStore<Ref> {
    /// The error type representing errors which can occur during query operations.
    type Error;

    /// Query a remote store for multiple references.
    ///
    /// This method retrieves information about the requested references from
    /// the remote store. The exact network behavior depends on the implementation.
    ///
    /// # Arguments
    /// * `targets` - An iterator of reference specifications (e.g., "main", "refs/tags/v1.0")
    ///
    /// # Returns
    /// An iterator over the requested references, or an error if the query fails.
    fn get_refs<Spec>(
        &self,
        targets: impl IntoIterator<Item = Spec>,
    ) -> Result<impl IntoIterator<Item = Ref>, Self::Error>
    where
        Spec: AsRef<BStr>;

    /// Query a remote store for a single reference.
    ///
    /// This is a convenience method that queries for a single reference and returns
    /// the first result. See [`get_refs`] for details on network behavior.
    ///
    /// # Arguments
    /// * `target` - The reference specification to query (e.g., "main", "refs/tags/v1.0")
    ///
    /// # Returns
    /// The requested reference, or an error if not found or if the query fails.
    fn get_ref<Spec>(&self, target: Spec) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr>;
}
