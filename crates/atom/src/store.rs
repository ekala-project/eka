//! # Atom Store Interface
//!
//! This module defines the core traits and interfaces for implementing storage
//! backends for atoms. The store abstraction allows atoms to be stored and
//! retrieved from different types of storage systems.
//!
//! ## Architecture
//!
//! The store system is designed around four core traits:
//!
//! - [`Init`] - Handles store initialization and root calculation
//! - [`QueryStore`] - Queries references from remote stores (most important for store operations)
//! - [`QueryVersion`] - Provides high-level atom version querying and filtering
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
//! **Reference Querying**: The ability to efficiently query references from remote
//! stores with different network strategies - lightweight queries for metadata
//! or full fetches for complete store access.
//!
//! **Version Management**: High-level operations for discovering, filtering, and
//! selecting atom versions based on semantic version constraints and tags.
//!
//! **Path Normalization**: Converting user-provided paths to canonical paths
//! relative to the store root, handling both relative and absolute paths correctly.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use atom::AtomTag;
//! use atom::store::git::Root;
//! use atom::store::{Init, NormalizeStorePath, QueryStore, QueryVersion};
//! use gix::{Remote, Url};
//! use semver::VersionReq;
//!
//! // Initialize a Git store
//! let repo = gix::open(".")?;
//! let remote = repo.find_remote("origin")?;
//! remote.ekala_init(None)?;
//! let root = remote.ekala_root(None)?;
//!
//! // Query references from a remote store
//! let url = gix::url::parse("https://github.com/example/repo.git".into())?;
//! let refs = url.get_refs(["main", "refs/tags/v1.0"], None)?;
//! for ref_info in refs {
//!     let (name, target, peeled) = ref_info.unpack();
//!     println!("Ref: {}, Target: {}", name, peeled.or(target).unwrap());
//! }
//!
//! // Query atom versions from a remote store
//! let atoms = url.get_atoms(None)?;
//! for atom in atoms {
//!     println!("Atom: {} v{} -> {}", atom.id.tag(), atom.version, atom.rev);
//! }
//!
//! // Find highest version matching requirements
//! let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
//! if let Some((version, id)) = url.get_highest_match(&AtomTag::try_from("mylib")?, &req, None) {
//!     println!("Selected version {} with id {}", version, id);
//! }
//!
//! // Normalize a path
//! let normalized = repo.normalize("path/to/atom")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bstr::BStr;
use semver::{Version, VersionReq};

use crate::{AtomId, AtomTag};

pub mod git;

//================================================================================================
// Types
//================================================================================================

/// Type alias for unpacked atom reference information.
#[derive(Clone, Debug, Eq)]
pub struct UnpackedRef<Id, R> {
    /// The proper AtomId of the reference
    pub id: AtomId<R>,
    /// The version of this particular reference
    pub version: Version,
    /// The cryptographic identity of the version
    pub rev: Id,
}

//================================================================================================
// Traits
//================================================================================================

/// A trait representing the methods required to initialize an Ekala store.
pub trait Init<R, O, T: Send> {
    /// The error type returned by the methods of this trait.
    type Error;
    /// Sync with the Ekala store, for implementations that require it.
    fn sync(&self, transport: Option<&mut T>) -> Result<O, Self::Error>;
    /// Initialize the Ekala store.
    fn ekala_init(&self, name: &str, transport: Option<&mut T>) -> Result<String, Self::Error>;
    /// Returns the root as reported by the remote store, or an error if it is inconsistent.
    fn ekala_root(&self, transport: Option<&mut T>) -> Result<R, Self::Error>;
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
/// let refs = url.get_refs(["main", "refs/tags/v1.0"], None)?;
/// for ref_info in refs {
///     let (name, target, peeled) = ref_info.unpack();
///     println!("Ref: {}, Target: {}", name, peeled.or(target).unwrap());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait QueryStore<Ref, T: Send> {
    /// The error type representing errors which can occur during query operations.
    type Error;

    /// Query a remote store for multiple references.
    ///
    /// This method retrieves information about the requested references from
    /// the remote store. The exact network behavior depends on the implementation.
    ///
    /// # Arguments
    /// * `targets` - An iterator of reference specifications (e.g., "main", "refs/tags/v1.0")
    /// * `transport` - An optional mutable reference to a persistent transport (obtained by
    ///   `QueryStore::get_transport`). If omitted an ephemeral connection is established.
    ///
    /// # Returns
    /// An iterator over the requested references, or an error if the query fails.
    fn get_refs<Spec>(
        &self,
        targets: impl IntoIterator<Item = Spec>,
        transport: Option<&mut T>,
    ) -> Result<Vec<Ref>, Self::Error>
    where
        Spec: AsRef<BStr>;

    /// Establish a persistent connection to a git server.
    fn get_transport(&self) -> Result<T, Self::Error>;

    /// Query a remote store for a single reference.
    ///
    /// This is a convenience method that queries for a single reference and returns
    /// the first result. See [`get_refs`] for details on network behavior.
    ///
    /// # Arguments
    /// * `target` - The reference specification to query (e.g., "main", "refs/tags/v1.0")
    /// * `transport` - An optional mutable reference to a persistent transport (obtained by
    ///   `QueryStore::get_transport`). If omitted an ephemeral connection is established.
    ///
    /// # Returns
    /// The requested reference, or an error if not found or if the query fails.
    fn get_ref<Spec>(&self, target: Spec, transport: Option<&mut T>) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr>;
}

/// A trait for querying version information about atoms in remote stores.
///
/// This trait extends [`QueryStore`] to provide high-level operations for working
/// with atom versions. It enables efficient querying and filtering of atom versions
/// based on tags and semantic version requirements.
///
/// ## Key Features
///
/// - **Atom Discovery**: Automatically discovers all atoms in the store using the standard
///   reference pattern
/// - **Version Filtering**: Find atoms matching specific tags and version requirements
/// - **Semantic Versioning**: Full support for semantic version constraints and comparison
/// - **Type Safety**: Strongly typed atom identifiers and version information
///
/// ## Architecture
///
/// The trait builds on top of [`QueryStore`] to provide:
/// 1. Low-level reference querying (from QueryStore)
/// 2. Atom reference parsing and unpacking
/// 3. Version-based filtering and selection
/// 4. Semantic version constraint matching
///
/// ## Use Cases
///
/// - **Dependency Resolution**: Find the highest version of an atom matching version requirements
/// - **Atom Discovery**: Enumerate all available atoms in a store
/// - **Version Management**: Check for atom updates and new versions
/// - **Lock File Generation**: Resolve atom versions for reproducible builds
///
/// ## Examples
///
/// ```rust,no_run
/// use atom::AtomTag;
/// use atom::store::{QueryStore, QueryVersion};
/// use gix::Url;
/// use semver::VersionReq;
///
/// // Query for atoms in a remote store
/// let url = gix::url::parse("https://github.com/example/atoms.git".into())?;
///
/// // Get all available atoms
/// let atoms = url.get_atoms(None)?;
/// for atom in atoms {
///     println!("Atom: {} v{} -> {}", atom.id.tag(), atom.version, atom.rev);
/// }
///
/// // Find the highest version matching a requirement
/// let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
///
/// if let Some((version, id)) = url.get_highest_match(&AtomTag::try_from("mylib")?, &req, None) {
///     println!("Selected version {} with id {}", version, id);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait QueryVersion<Ref, Id, C, T, R>: QueryStore<Ref, T>
where
    C: FromIterator<UnpackedRef<Id, R>> + IntoIterator<Item = UnpackedRef<Id, R>>,
    Ref: UnpackRef<Id, R> + std::fmt::Debug,
    Self: std::fmt::Debug,
    T: Send,
{
    /// Processes an iterator of references, unpacking and collecting them into an
    /// iterator of atom information.
    fn process_atoms(refs: Vec<Ref>) -> <C as IntoIterator>::IntoIter {
        let root = refs.iter().find_map(|r| r.find_root_ref());
        refs.into_iter()
            .filter_map(|x| x.unpack_atom_ref(root.as_ref()))
            .collect::<C>()
            .into_iter()
    }
    /// Retrieves all atoms available in the remote store.
    ///
    /// This method queries the store for all atom references using the standard
    /// `refs/atoms/*` pattern and unpacks them into structured atom information.
    ///
    /// # Returns
    /// An iterator over all discovered atoms, where each atom contains:
    /// - `AtomTag`: The atom's identifier/tag name
    /// - `Version`: The semantic version of the atom
    /// - `Id`: The unique identifier for this atom version
    ///
    /// # Errors
    /// Returns an error if the reference query fails or if reference parsing fails.
    fn get_atoms(
        &self,
        transport: Option<&mut T>,
    ) -> Result<<C as IntoIterator>::IntoIter, <Self as QueryStore<Ref, T>>::Error> {
        let r = format!("{}/*", crate::ATOM_REFS.as_str());
        let a = format!("{}:{}", r, r);

        let ro = format!("{}:{}", git::V1_ROOT, git::V1_ROOT);

        let query = [a.as_str(), ro.as_str()];
        let refs = self.get_refs(query, transport)?;
        let atoms = Self::process_atoms(refs);
        Ok(atoms)
    }

    /// Processes an iterator of atoms to find the highest version matching the
    /// given tag and version requirement.
    fn process_highest_match(
        atoms: <C as IntoIterator>::IntoIter,
        tag: &AtomTag,
        req: &VersionReq,
    ) -> Option<(Version, Id)> {
        atoms
            .filter_map(
                |UnpackedRef {
                     id: t,
                     version: v,
                     rev: id,
                 }| (t.tag() == tag && req.matches(&v)).then_some((v, id)),
            )
            .max_by_key(|(ref version, _)| version.to_owned())
    }

    /// Finds the highest version of an atom matching the given version requirement.
    ///
    /// This method searches through all available atom versions for a specific tag
    /// and returns the highest version that satisfies the provided version requirement.
    /// Uses semantic version comparison to determine "highest" version.
    ///
    /// # Arguments
    /// * `tag` - The atom tag to search for (e.g., "mylib", "database")
    /// * `req` - The version requirement to match (e.g., ">=1.0.0", "~2.1.0")
    ///
    /// # Returns
    /// The highest matching version and its identifier, or `None` if no versions match.
    ///
    /// # Examples
    /// ```rust,no_run
    /// use atom::AtomTag;
    /// use atom::store::QueryVersion;
    /// use semver::VersionReq;
    ///
    /// let url = gix::url::parse("https://example.com/my-repo.git".into())?;
    /// let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
    /// let result = url.get_highest_match(&AtomTag::try_from("mylib")?, &req, None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    fn get_highest_match(
        &self,
        tag: &AtomTag,
        req: &VersionReq,
        transport: Option<&mut T>,
    ) -> Option<(Version, Id)> {
        let atoms = self.get_atoms(transport).ok()?;
        Self::process_highest_match(atoms, tag, req)
    }

    /// Retrieves all atoms from the remote store and maps them by their tag.
    ///
    /// This method provides a convenient way to get a comprehensive overview of all
    /// atoms available in the remote store, organized by their unique `AtomTag`.
    /// If an atom has multiple versions, only the highest version is returned.
    ///
    /// # Returns
    ///
    /// A `HashMap` where:
    /// - The key is the `AtomTag`, representing the unique identifier of the atom.
    /// - The value is a tuple containing the `Version` and `Id` of the atom.
    ///
    /// If the remote store cannot be reached or if there are no atoms, an empty
    /// `HashMap` is returned.
    fn remote_atoms(&self, transport: Option<&mut T>) -> HashMap<AtomTag, (Version, Id)> {
        if let Ok(refs) = self.get_atoms(transport) {
            let iter = refs.into_iter();
            let s = match iter.size_hint() {
                (l, None) => l,
                (_, Some(u)) => u,
            };
            iter.fold(
                HashMap::with_capacity(s),
                |mut acc,
                 UnpackedRef {
                     id: t,
                     version: v,
                     rev: id,
                 }| {
                    acc.insert(t.tag().to_owned(), (v, id));
                    acc
                },
            )
        } else {
            HashMap::new()
        }
    }
}

/// A trait for unpacking atom references into structured version information.
///
/// This trait defines how to parse atom references (from git refs) into
/// structured atom data including tags, versions, and identifiers.
///
/// ## Reference Format
///
/// Atom references follow the pattern: `refs/atoms/{tag}/{version}/{id}`
/// where:
/// - `tag` is the atom identifier (e.g., "mylib", "database")
/// - `version` is a semantic version (e.g., "1.2.3")
/// - `id` is a unique identifier for this atom version
pub trait UnpackRef<Id, R> {
    /// Attempts to unpack this reference as an atom reference.
    ///
    /// # Returns
    /// - `Some((tag, version, id))` if the reference follows atom reference format
    /// - `None` if the reference is not an atom reference or is malformed
    fn unpack_atom_ref(&self, root: Option<&R>) -> Option<UnpackedRef<Id, R>>;
    /// Attempts to find the root reference in the store.
    fn find_root_ref(&self) -> Option<R>;
}

//================================================================================================
// Impls
//================================================================================================

/// We purposefully avoid comparing version so we can update sets with new versions easily.
impl<Id: PartialEq, R: PartialEq> PartialEq for UnpackedRef<Id, R> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.version == other.version
    }
}

impl<Id: Ord, R: Ord> Ord for UnpackedRef<Id, R> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.id.cmp(&other.id) {
            std::cmp::Ordering::Equal => {},
            ord => return ord,
        }
        self.version.cmp(&other.version)
    }
}

impl<Id: PartialOrd, R: PartialOrd> PartialOrd for UnpackedRef<Id, R> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.id.partial_cmp(&other.id) {
            Some(core::cmp::Ordering::Equal) => {},
            ord => return ord,
        }
        self.version.partial_cmp(&other.version)
    }
}
