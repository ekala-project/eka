//! # Atom Storage Interface
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
//! selecting atom versions based on semantic version constraints.
//!
//! **Path Normalization**: Converting user-provided paths to canonical paths
//! relative to the store root, handling both relative and absolute paths correctly.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use atom::Label;
//! use atom::storage::git::Root;
//! use atom::storage::{Init, NormalizeStorePath, QueryStore, QueryVersion};
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
//!     println!("Atom: {:#?}", atom,);
//! }
//!
//! // Find highest version matching requirements
//! let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
//! if let Some((version, id)) = url.get_highest_match(&Label::try_from("mylib")?, &req, None) {
//!     println!("Selected version {} with id {}", version, id);
//! }
//!
//! // Normalize a path
//! let normalized = repo.normalize("path/to/atom")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Note: Some methods like `get_atoms` and `get_highest_match` are trait methods
//! that need to be imported explicitly. The `UnpackedRef` fields are not publicly accessible.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bstr::BStr;
use semver::{Version, VersionReq};

use crate::package::{AtomError, EkalaManifest};
use crate::storage::git::Root;
use crate::{AtomId, Label};

pub mod git;

//================================================================================================
// Types
//================================================================================================

/// errors that can occur during local storage lookups
#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    /// couldn't locate the storage root
    #[error("could not locate the storage root directory")]
    NotStorage,
    /// path normalization failed
    #[error("path normalization failed")]
    NotNormal,
    #[error(transparent)]
    /// a document error
    Doc(#[from] crate::package::metadata::DocError),
    /// transparent wrapper for a strip prefix error
    #[error(transparent)]
    Prefix(#[from] std::path::StripPrefixError),
    /// transparent wrapper for an io error
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// a git error
    #[error(transparent)]
    Git(#[from] Box<git::Error>),
    /// a serialization error
    #[error(transparent)]
    Ser(#[from] toml_edit::ser::Error),
    /// a deserialization error
    #[error(transparent)]
    De(#[from] toml_edit::de::Error),
    /// a tempfile persistance error
    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),
    /// a tempfile persistance error
    #[error(transparent)]
    Atom(#[from] AtomError),
}

/// Type alias for unpacked atom reference information.
#[derive(Clone, Debug, Eq)]
pub struct UnpackedRef<Id, R> {
    /// The proper AtomId of the reference
    pub(crate) id: AtomId<R>,
    /// The version of this particular reference
    pub(crate) version: Version,
    /// The cryptographic identity of the version
    pub(crate) rev: Id,
}

/// A path which has been verified to exist within a local Ekala storage layer (has an ekala.toml in
/// a parent)
#[derive(Clone, Debug)]
pub struct LocalStoragePath(PathBuf);

//================================================================================================
// Traits
//================================================================================================

/// A trait representing a type which implements the full resonsibilities of a local ekala storage
/// layer
pub trait LocalStorage: Init + NormalizeStorePath + Sync + Send {}

/// A trait representing the methods required to initialize an Ekala store.
pub trait Init {
    /// The error type returned by the methods of this trait.
    type Error: std::fmt::Display;
    /// The type indicating the report transport (not relevant for local only storage)
    type Transport: Send + 'static;
    /// Initialize the Ekala store.
    fn ekala_init(&self, transport: Option<&mut Self::Transport>) -> Result<(), Self::Error>;
    /// Returns the root as reported by the local or remote store, or an error if it is
    /// inconsistent.
    fn ekala_root(&self, transport: Option<&mut Self::Transport>) -> Result<Root, Self::Error>;
    /// Make initialization atomic by performing the actual transaction in a separate step
    fn commit_init(&self, _content: &str) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// returns the root directory of the storage layer
pub trait EkalaStorage {
    /// The error type returned
    type Error: From<std::path::StripPrefixError>
        + From<toml_edit::de::Error>
        + From<std::io::Error>
        + std::error::Error
        + std::marker::Send
        + std::marker::Sync
        + 'static;
    /// find or return the canonical root of this storage layer
    fn ekala_root_dir(&self) -> Result<PathBuf, Self::Error>;
    /// returns the EkalaManifest instance for this ekala storage layer
    fn ekala_manifest(&self) -> Result<EkalaManifest, Self::Error> {
        let path = self
            .ekala_root_dir()?
            .join(crate::EKALA_MANIFEST_NAME.as_str());
        let ekala: EkalaManifest =
            toml_edit::de::from_str(&std::fs::read_to_string(&path).inspect_err(
                |_| tracing::warn!(path = %path.display(), "could not locate ekala.toml"),
            )?)?;
        Ok(ekala)
    }
    /// returns the corrent working directory inside the store, failing if outside
    fn cwd(&self) -> Result<impl AsRef<Path>, Self::Error>;
}

/// A trait containing a path normalization method, to normalize paths in an Ekala store
/// relative to its root.
pub trait NormalizeStorePath: EkalaStorage {
    /// Normalizes a given path to be relative to the store root.
    ///
    /// This function takes a path (relative or absolute) and attempts to normalize it
    /// relative to the store root, based on the current working directory within
    /// the store system.
    ///
    /// # Behavior:
    /// - For relative paths (e.g., "foo/bar" or "../foo"):
    ///   - Interpreted as relative to the current working directory within the repository.
    ///   - Computed relative to the repository root.
    ///
    /// - For absolute paths (e.g., "/foo/bar"):
    ///   - Treated as if the repository root is the filesystem root.
    ///   - The leading slash is ignored, and the path is considered relative to the repo root.
    fn normalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, Self::Error> {
        use path_clean::PathClean;
        let path = path.as_ref();

        let repo_root = self.ekala_root_dir()?;
        let current = self.cwd()?;
        let rel = current.as_ref().join(path).clean();

        rel.strip_prefix(&repo_root)
            .map_or_else(
                |e| {
                    // handle absolute paths as if they were relative to the repo root
                    if !path.is_absolute() {
                        return Err(e);
                    }
                    let cleaned = path.clean();
                    // Preserve the platform-specific root
                    let p = cleaned.strip_prefix(Path::new("/"))?;
                    repo_root
                        .join(p)
                        .clean()
                        .strip_prefix(&repo_root)
                        .map(Path::to_path_buf)
                },
                |p| Ok(p.to_path_buf()),
            )
            .map_err(|e| {
                tracing::warn!(
                    path = %path.display(),
                    "Ignoring path outside repo root",
                );
                Into::<Self::Error>::into(e)
            })
    }
    /// Same as normalization but gives the relative difference between the path given and the
    /// root of the store (e.g. foo/bar -> ../..). Path must be an ancestor of the store root or
    /// this will fail.
    fn rel_from_root(&self, path: impl AsRef<Path>) -> Result<PathBuf, Self::Error> {
        let path = self.normalize(path)?;
        let mut res = PathBuf::new();
        let mut iter = path.ancestors();
        iter.next();
        for _ in iter {
            res.push("..");
        }
        Ok(res)
    }
}

/// A trait for querying remote stores to retrieve references.
///
/// This trait provides a unified interface for querying references from remote
/// stores. It supports different query strategies depending on the implementation.
///
/// ## Examples
///
/// ```rust,no_run
/// use atom::storage::QueryStore;
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
pub trait QueryStore {
    /// The error type representing errors which can occur during query operations.
    type Error;
    /// The type representing the remote transport (irrelevant for local only storage).
    type Transport: Send;
    /// The type representing the references returned from the storage layer
    type Ref: UnpackRef;

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
        targets: impl IntoIterator<Item = Spec> + std::fmt::Debug,
        transport: Option<&mut Self::Transport>,
    ) -> Result<Vec<Self::Ref>, Self::Error>
    where
        Spec: AsRef<BStr> + std::fmt::Debug;

    /// Establish a persistent connection to a git server.
    fn get_transport(&self) -> Result<Self::Transport, Self::Error>;

    /// Query a remote store for a single reference.
    ///
    /// This is a convenience method that queries for a single reference and returns
    /// the first result. See `get_refs` for details on network behavior.
    ///
    /// # Arguments
    /// * `target` - The reference specification to query (e.g., "main", "refs/tags/v1.0")
    /// * `transport` - An optional mutable reference to a persistent transport (obtained by
    ///   `QueryStore::get_transport`). If omitted an ephemeral connection is established.
    ///
    /// # Returns
    /// The requested reference, or an error if not found or if the query fails.
    fn get_ref<Spec>(
        &self,
        target: Spec,
        transport: Option<&mut Self::Transport>,
    ) -> Result<Self::Ref, Self::Error>
    where
        Spec: AsRef<BStr> + std::fmt::Debug;
}

/// A trait for querying version information about atoms in remote stores.
///
/// This trait extends [`QueryStore`] to provide high-level operations for working
/// with atom versions. It enables efficient querying and filtering of atom versions
/// based on labels and semantic version requirements.
///
/// ## Key Features
///
/// - **Atom Discovery**: Automatically discovers all atoms in the store using the standard
///   reference pattern
/// - **Version Filtering**: Find atoms matching specific label and version requirements
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
/// use atom::Label;
/// use atom::storage::{QueryStore, QueryVersion};
/// use gix::Url;
/// use semver::VersionReq;
///
/// // Query for atoms in a remote store
/// let url = gix::url::parse("https://github.com/example/atoms.git".into())?;
///
/// // Get all available atoms
/// let atoms = url.get_atoms(None)?;
/// for atom in atoms {
///     println!("Atom: {:#?}", atom);
/// }
///
/// // Find the highest version matching a requirement
/// let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
///
/// if let Some((version, id)) = url.get_highest_match(&Label::try_from("mylib")?, &req, None) {
///     println!("Selected version {} with id {}", version, id);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(clippy::type_complexity)]
pub trait QueryVersion: QueryStore {
    /// Processes an iterator of references, unpacking and collecting them into an
    /// iterator of atom information.
    fn process_atoms(
        refs: Vec<<Self as QueryStore>::Ref>,
    ) -> Vec<UnpackedRef<<Self::Ref as UnpackRef>::Object, <Self::Ref as UnpackRef>::Root>> {
        let root = refs.iter().find_map(|r| r.find_root_ref());
        refs.into_iter()
            .filter_map(|x| x.unpack_atom_ref(root.as_ref()))
            .collect()
    }
    /// Retrieves all atoms available in the remote store.
    ///
    /// This method queries the store for all atom references using the standard
    /// `refs/atoms/*` pattern and unpacks them into structured atom information.
    ///
    /// # Returns
    /// An iterator over all discovered atoms, where each atom contains:
    /// - `Label`: The atom's identifying name
    /// - `Version`: The semantic version of the atom
    /// - `Id`: The unique identifier for this atom version
    ///
    /// # Errors
    /// Returns an error if the reference query fails or if reference parsing fails.
    fn get_atoms(
        &self,
        transport: Option<&mut Self::Transport>,
    ) -> Result<
        Vec<UnpackedRef<<Self::Ref as UnpackRef>::Object, <Self::Ref as UnpackRef>::Root>>,
        Self::Error,
    > {
        let r = format!("{}/*", crate::ATOM_REFS.as_str());
        let a = format!("{}:{}", r, r);

        let ro = format!("{}:{}", git::V1_ROOT, git::V1_ROOT);

        let query = [a.as_str(), ro.as_str()];
        let refs = self.get_refs(query, transport)?;
        let atoms = Self::process_atoms(refs);
        Ok(atoms)
    }

    /// Processes an iterator of atoms to find the highest version matching the
    /// given label and version requirement.
    fn process_highest_match(
        atoms: Vec<UnpackedRef<<Self::Ref as UnpackRef>::Object, <Self::Ref as UnpackRef>::Root>>,
        label: &Label,
        req: &VersionReq,
    ) -> Option<(Version, <Self::Ref as UnpackRef>::Object)> {
        atoms
            .into_iter()
            .filter_map(|UnpackedRef { id, version, rev }| {
                (id.label() == label && req.matches(&version)).then_some((version, rev))
            })
            .max_by_key(|(ref version, _)| version.to_owned())
    }

    /// Finds the highest version of an atom matching the given version requirement.
    ///
    /// This method searches through all available atom versions for a specific label
    /// and returns the highest version that satisfies the provided version requirement.
    /// Uses semantic version comparison to determine "highest" version.
    ///
    /// # Arguments
    /// * `label` - The atom label to search for (e.g., "mylib", "database")
    /// * `req` - The version requirement to match (e.g., ">=1.0.0", "~2.1.0")
    ///
    /// # Returns
    /// The highest matching version and its identifier, or `None` if no versions match.
    ///
    /// # Examples
    /// ```rust,no_run
    /// use atom::Label;
    /// use atom::storage::QueryVersion;
    /// use semver::VersionReq;
    ///
    /// let url = gix::url::parse("https://example.com/my-repo.git".into())?;
    /// let req = VersionReq::parse(">=1.0.0,<2.0.0")?;
    /// let result = url.get_highest_match(&Label::try_from("mylib")?, &req, None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    fn get_highest_match(
        &self,
        label: &Label,
        req: &VersionReq,
        transport: Option<&mut Self::Transport>,
    ) -> Option<(Version, <Self::Ref as UnpackRef>::Object)> {
        let atoms = self.get_atoms(transport).ok()?;
        Self::process_highest_match(atoms, label, req)
    }

    /// Retrieves all atoms from the remote store and maps them by their label.
    ///
    /// This method provides a convenient way to get a comprehensive overview of all
    /// atoms available in the remote store, organized by their unique `Label`.
    /// If an atom has multiple versions, only the highest version is returned.
    ///
    /// # Returns
    ///
    /// A `HashMap` where:
    /// - The key is the `Label`, representing the unique identifier of the atom.
    /// - The value is a tuple containing the `Version` and `Id` of the atom.
    ///
    /// If the remote store cannot be reached or if there are no atoms, an empty
    /// `HashMap` is returned.
    fn remote_atoms(
        &self,
        transport: Option<&mut Self::Transport>,
    ) -> HashMap<Label, (Version, <Self::Ref as UnpackRef>::Object)> {
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
                    acc.insert(t.label().to_owned(), (v, id));
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
/// structured atom data including labels, versions, and identifiers.
///
/// ## Reference Format
///
/// Atom references follow the pattern: `refs/ekala/atoms/{label}/{version}`
/// where:
/// - `label` is the atom identifier (e.g., "mylib", "database")
/// - `version` is a semantic version (e.g., "1.2.3")
pub trait UnpackRef {
    /// The type representing the ekala root hash
    type Root;
    /// The type representing the cryptographic identity of specific atom versions
    type Object;
    /// Attempts to unpack this reference as an atom reference.
    ///
    /// # Returns
    /// - `Some((label, version, id))` if the reference follows atom reference format
    /// - `None` if the reference is not an atom reference or is malformed
    fn unpack_atom_ref(
        &self,
        root: Option<&Self::Root>,
    ) -> Option<UnpackedRef<Self::Object, Self::Root>>;
    /// Attempts to find the root reference in the store.
    fn find_root_ref(&self) -> Option<Self::Root>;
}

//================================================================================================
// Impls
//================================================================================================

impl<T: Init + NormalizeStorePath + Send + Sync> LocalStorage for T {}

impl LocalStoragePath {
    /// verifies a path is actually contained in an ekala storage layer before allowing construction
    pub fn new(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let maybe_local = LocalStoragePath(path.as_ref().to_path_buf());
        if let Err(e) = maybe_local.ekala_root_dir().map_err(|e| {
            tracing::error!(message = %e);
            StorageError::NotStorage
        }) {
            Err(e)
        } else {
            Ok(maybe_local)
        }
    }

    /// solves the bootstrap problem of not being able to run ekala_init without there already being
    /// a verified LocalStorePath instance
    pub fn init(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let maybe_init = LocalStoragePath(path.as_ref().to_path_buf());
        if let Ok(root) = maybe_init.ekala_root_dir() {
            LocalStoragePath::new(root)
        } else if maybe_init.as_ref().exists() {
            if maybe_init.as_ref().is_dir() {
                maybe_init.ekala_init(None).map(|_| maybe_init)
            } else {
                let path = maybe_init
                    .as_ref()
                    .parent()
                    .and_then(|p| p.is_dir().then_some(p.to_path_buf()))
                    .unwrap_or(".".into());
                let maybe_init = LocalStoragePath(path);
                maybe_init.ekala_init(None).map(|_| maybe_init)
            }
        } else {
            std::fs::create_dir_all(maybe_init.as_ref())?;
            maybe_init.ekala_init(None).map(|_| maybe_init)
        }
    }
}

impl Init for LocalStoragePath {
    type Error = StorageError;
    type Transport = ();

    fn ekala_init(&self, _: Option<&mut ()>) -> Result<(), Self::Error> {
        if self.ekala_root_dir().is_err() {
            let manifest = EkalaManifest::new();
            let path = self.as_ref().join(crate::EKALA_MANIFEST_NAME.as_str());
            std::fs::write(&path, toml_edit::ser::to_string_pretty(&manifest)?)?;
        }
        Ok(())
    }

    fn ekala_root(&self, _: Option<&mut ()>) -> Result<Root, Self::Error> {
        Ok(git::NULLROOT)
    }
}

impl EkalaStorage for LocalStoragePath {
    type Error = StorageError;

    fn ekala_root_dir(&self) -> Result<PathBuf, Self::Error> {
        let ekala = crate::EKALA_MANIFEST_NAME.as_str();
        let start_dir = self.as_ref();

        for dir in start_dir.ancestors() {
            let file_path = dir.join(ekala);
            if file_path.exists() {
                return Ok(dir.to_owned());
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not locate ekala manifest",
        )
        .into())
    }

    fn cwd(&self) -> Result<impl AsRef<Path>, Self::Error> {
        Ok(self)
    }
}

impl NormalizeStorePath for LocalStoragePath {}

impl AsRef<Path> for LocalStoragePath {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

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

impl<Id, R> UnpackedRef<Id, R> {
    pub(crate) fn new(id: AtomId<R>, version: Version, rev: Id) -> Self {
        Self { id, version, rev }
    }
}
