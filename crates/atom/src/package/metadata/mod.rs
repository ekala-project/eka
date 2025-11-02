//! # Package Metadata
//!
//! This module contains the fundamental types that represent atoms and their
//! file system structure. These types form the foundation of the atom format
//! and are used throughout the crate.
//!
//! ## Submodules
//!
//! - [`manifest`] - Atom manifest format and dependency specification
//! - [`lock`] - Lockfile format for capturing resolved dependencies
//!
//! ## Key Types
//!
//! - [`Atom`] - Represents an atom with its metadata and dependencies
//! - [`ValidManifest`] - Publicly exposed manifest type with validation
//! - [`Manifest`] - Internal manifest structure (private implementation detail)
//! - [`Lockfile`] - Resolved dependency lockfile
//! - [`AtomPaths`] - File system paths associated with an atom
//! - [`EkalaManager`] - Manager for Ekala-specific operations

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use id::{Label, Tag};
use manifest::{AtomSet, ComposeError, Manifest, SetMirror};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use super::{AtomError, sets};
use crate::storage::LocalStorage;
use crate::uri::AliasedUrl;
use crate::{ATOM_MANIFEST_NAME, id, storage, uri};

pub mod lock;
pub mod manifest;

//================================================================================================
// Types
//================================================================================================

/// Represents the deserialized form of an Atom, directly constructed from the TOML manifest.
///
/// This struct contains the basic metadata of an Atom but lacks the context-specific
/// [`crate::AtomId`], which must be constructed separately.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Atom {
    /// The verified, human-readable Unicode identifier for the Atom.
    label: Label,

    /// The version of the Atom.
    version: Version,

    /// An set of structured meta-data
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<Meta>,

    /// A table of named atom sets, defining the sources for resolving atom dependencies.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    sets: HashMap<Tag, AtomSet>,
}

/// Represents the file system paths associated with an atom.
///
/// This struct manages the relationship between an atom's manifest file
/// (the "spec") and its content directory. It handles the logic for determining
/// these paths based on whether we're given a manifest file or a content directory.
#[derive(Debug)]
pub(crate) struct AtomPaths<P>
where
    P: AsRef<Path>,
{
    /// Path to the atom's manifest file (atom.toml)
    spec: P,
    /// Path to the atom's content directory
    content: P,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    tags: BTreeSet<Tag>,
    /// An optional description of the Atom.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// A newtype wrapper to tie a `DocumentMut` to a specific serializable type `T`.
#[derive(Debug)]
pub(super) struct TypedDocument<T> {
    /// The underlying `toml_edit` document.
    inner: DocumentMut,
    _marker: PhantomData<T>,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub(in crate::package) struct AtomMap(BTreeMap<Label, PathBuf>);

#[derive(thiserror::Error, Debug)]
/// Errors that can occur when working with a `TypedDocument`.
pub enum DocError {
    /// Missing atom from manifest
    #[error("the atom directory disappeared or is inaccessible: {0}")]
    Missing(PathBuf),
    /// The manifest path could not be accessed.
    #[error("the ekala.toml could not be located")]
    MissingEkala,
    /// A valid atom id could not be constructed.
    #[error("a bug occurred, constructing atomid from precalculated root should be infallible")]
    AtomIdConstruct,
    /// Duplicate atoms were found in the ekala manifest
    #[error("there is more than one atom with the same label in the set")]
    DuplicateAtoms,
    /// Dependencies were declared from undeclared sets
    #[error("found atom(s) specified from undeclared set(s)")]
    UndeclaredSets,
    /// Resolving local atoms failed
    #[error("Resolving local atom failed")]
    LocalResolve,
    /// Dependencies are not appropriate for this type of atom
    #[error("A static atom, which is not evaluated, cannot provide dependencies")]
    StaticDependencies,
    /// A local atom by the requested label doesn't exist
    #[error("a local atom by the requested label isn't specified in ekala.toml")]
    NoLocal,
    /// Duplicate atoms were found in the ekala manifest
    #[error("locked atoms could not be synchronized with manifest")]
    SyncFailed,
    #[error("Composer set not declared")]
    ComposerSet,
    /// A TOML deserialization error occurred.
    #[error(transparent)]
    De(#[from] toml_edit::de::Error),
    /// A TOML serialization error occurred.
    #[error(transparent)]
    Ser(#[from] toml_edit::TomlError),
    /// A filesystem error occurred.
    #[error(transparent)]
    Read(#[from] std::io::Error),
    /// A manifest serialization error occurred.
    #[error(transparent)]
    Manifest(#[from] toml_edit::ser::Error),
    /// An error occurred while writing to a temporary file.
    #[error(transparent)]
    Write(#[from] tempfile::PersistError),
    /// A Git resolution error occurred.
    #[error(transparent)]
    Git(#[from] Box<storage::git::Error>),
    /// A semantic versioning error occurred.
    #[error(transparent)]
    Semver(#[from] semver::Error),
    /// A UTF-8 conversion error occurred.
    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),
    /// A URL parsing error occurred.
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// A generic error occurred.
    #[error(transparent)]
    Error(#[from] crate::BoxError),
    /// A invalid refname was passed.
    #[error(transparent)]
    BadLabel(#[from] crate::id::Error),
    /// A set error has occurred.
    #[error(transparent)]
    SetError(#[from] sets::Error),
}

/// The section of the manifest describing the Ekala set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaSet {
    #[serde(default)]
    pub(in crate::package) packages: AtomMap,
}

/// The entrypoint for an ekala manifest describing a set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaManifest {
    pub(super) set: EkalaSet,
    metadata: Option<MetaData>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct MetaData {
    tags: Option<BTreeSet<Tag>>,
}

/// A writer to assist with writing into the Ekala manifest.
#[derive(Debug)]
pub struct EkalaManager<'a, S: LocalStorage> {
    path: PathBuf,
    doc: TypedDocument<EkalaManifest>,
    pub(super) storage: &'a S,
    pub(super) manifest: EkalaManifest,
}

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Copy, Serialize, Deserialize, Debug, PartialEq, Clone, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum GitDigest {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Singleton<K, V> {
    key: K,
    value: V,
}

//================================================================================================
// Impls
//================================================================================================

impl AtomPaths<PathBuf> {
    /// Creates a new `AtomPaths` instance from a given path.
    ///
    /// If the path points to a manifest file (named `atom.toml`), then:
    /// - `spec` is set to that file path
    /// - `content` is set to the parent directory
    ///
    /// If the path points to a directory, then:
    /// - `spec` is set to `path/atom.toml`
    /// - `content` is set to the provided path
    ///
    /// # Arguments
    ///
    /// * `path` - Either a path to a manifest file or content directory
    ///
    /// # Returns
    ///
    /// An `AtomPaths` instance with the appropriate spec and content paths.
    pub(crate) fn new<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();

        if name == crate::ATOM_MANIFEST_NAME.as_str() {
            AtomPaths {
                spec: path.into(),
                content: path.parent().unwrap_or(Path::new("")).into(),
            }
        } else {
            let spec = path.join(crate::ATOM_MANIFEST_NAME.as_str());
            AtomPaths {
                spec: spec.clone(),
                content: path.into(),
            }
        }
    }

    /// Returns the path to the atom's manifest file.
    ///
    /// This is the `atom.toml` file that contains the atom's metadata
    /// and dependency specifications.
    pub fn spec(&self) -> &Path {
        self.spec.as_ref()
    }

    /// Returns the path to the atom's content directory.
    ///
    /// This directory contains the actual source code or files that
    /// make up the atom's content.
    pub fn content(&self) -> &Path {
        self.content.as_ref()
    }
}

impl Atom {
    pub(crate) fn new(label: Label, version: Version) -> Result<Self, ComposeError> {
        let composer = config::CONFIG.default_composer();
        let address: SetMirror = if composer.set.address == "::" {
            composer.set.address.as_ref().parse()?
        } else {
            let url = AliasedUrl::try_from(composer.set.address.as_ref())?.url;
            SetMirror::Url(url)
        };
        Ok(Self {
            label,
            version,
            meta: None,
            sets: HashMap::from([(
                composer.set.tag.as_ref().try_into().inspect_err(|_| {
                    tracing::warn!(configured.set = %composer.set.tag, "default composer set is not a valid set tag")
                })?,
                address.into(),
            )]),
        })
    }

    /// return a reference to the atom's label
    pub fn label(&self) -> &Label {
        &self.label
    }

    /// consume the atom and take ownership of the label
    pub fn take_label(self) -> Label {
        self.label
    }

    /// return a reference to the atom's version
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// consume the atom and take ownership of the version
    pub fn take_version(self) -> Version {
        self.version
    }

    /// return a reference to this atom's metadata, if it has any
    pub fn meta(&self) -> Option<&Meta> {
        if let Some(meta) = &self.meta {
            Some(meta)
        } else {
            None
        }
    }

    /// consume the atom and take ownership of the metadata, if there is any
    pub fn take_meta(self) -> Option<Meta> {
        self.meta
    }

    /// return a reference to this atom's defined sets
    pub fn sets(&self) -> &HashMap<Tag, AtomSet> {
        &self.sets
    }
}

impl Meta {
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }
}

impl AsMut<Option<Meta>> for Atom {
    fn as_mut(&mut self) -> &mut Option<Meta> {
        &mut self.meta
    }
}

impl AsMut<BTreeSet<Tag>> for Meta {
    fn as_mut(&mut self) -> &mut BTreeSet<Tag> {
        &mut self.tags
    }
}

impl AsMut<Meta> for Meta {
    fn as_mut(&mut self) -> &mut Meta {
        self
    }
}

impl AsRef<BTreeMap<Label, PathBuf>> for AtomMap {
    fn as_ref(&self) -> &BTreeMap<Label, PathBuf> {
        &self.0
    }
}

impl AsMut<BTreeMap<Label, PathBuf>> for AtomMap {
    fn as_mut(&mut self) -> &mut BTreeMap<Label, PathBuf> {
        &mut self.0
    }
}

/// # AtomMap Deserialization: Enforcing Repository Path Invariants
///
/// This implementation provides a unique deserialization strategy for `AtomMap` that
/// conditionally enforces path normalization based on repository availability. This
/// behavior is crucial for maintaining the integrity of atom paths within the Ekala
/// ecosystem.
///
/// ## Purpose
///
/// The primary goal is to prevent atom paths from escaping the repository boundary.
/// Since `AtomMap` is constructed from the actual on-disk manifest during deserialization,
/// this provides an opportunity to validate and normalize paths early in the process,
/// failing fast if any path would violate the repository containment invariant.
///
/// ## Behavior
///
/// - **When inside a repository**: Paths are normalized using the repository's `normalize()`
///   method, which ensures all paths are relative to the repository root and contained within the
///   repository boundaries. If normalization fails (indicating a path outside the repository),
///   deserialization fails immediately.
///
/// - **When outside a repository**: Paths are left as-is, allowing normal operation in
///   non-repository contexts (e.g., testing, standalone usage).
///
/// ## Implementation Details
///
/// The implementation uses a global static reference to a lazily initialized repository
/// instance (`git::repo()`). This ensures:
/// - Efficient access: The repository is only discovered once per process
/// - Conditional behavior: Normalization only occurs when a repository is available
/// - Thread safety: Uses `OnceLock` for safe static initialization
///
/// ## Why This Matters
///
/// Atom paths must never escape the repository because they represent internal
/// references that are meaningless outside the repository context. By enforcing
/// this invariant during deserialization, we prevent invalid states that could
/// lead to data corruption, security issues, or inconsistent behavior.
///
/// ## Error Handling
///
/// If path normalization fails when a repository is present, deserialization
/// returns a custom error, preventing the creation of an invalid `AtomMap` instance.
/// This early failure ensures problems are caught during manifest loading rather
/// than later during atom resolution.
///
/// ## Additional Invariants
///
/// Beyond path containment, this implementation also enforces that no two atoms
/// share the same label. Since the map is keyed by `Label`, duplicate labels would
/// overwrite entries, losing atom identity. The implementation detects and rejects
/// such conflicts during deserialization, ensuring each atom maintains its distinct
/// identity.
impl<'de> Deserialize<'de> for AtomMap {
    /// Deserializes a list of paths into an `AtomMap`, enforcing repository path invariants.
    ///
    /// This function transforms a serialized list of paths into a map keyed by atom labels,
    /// acquired directly from each atom's manifest. This approach enables canonical addressing
    /// and efficient lookups while enforcing crucial invariants: path containment within the
    /// repository and uniqueness of atom labels.
    ///
    /// The deserialization process:
    /// 1. Deserializes the input as a `Vec<PathBuf>`
    /// 2. For each path, normalizes it relative to the repository root (when in a repo)
    /// 3. Reads the atom manifest to extract the canonical label
    /// 4. Ensures no duplicate labels exist
    /// 5. Constructs the final `BTreeMap<Label, PathBuf>`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Path normalization fails (paths outside repository when in repo context)
    /// - Atom manifest cannot be read or parsed
    /// - Multiple atoms share the same label
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use path_clean::PathClean;
        use serde::de;
        use storage::{NormalizeStorePath, git};
        let repo = git::repo().ok().flatten().map(|r| r.to_thread_local());
        let entries: Vec<PathBuf> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();

        let rel_to_root = repo
            .as_ref()
            .ok_or(storage::git::Error::NoWorkDir)
            .and_then(|r| r.rel_from_root(r.current_dir()));
        for path in entries {
            let normalized = if let Some(repo) = &repo {
                let rel_to_root = rel_to_root.as_ref().map_err(de::Error::custom)?;
                let rel_path = rel_to_root.join(&path);
                let cwd = repo
                    .normalize(repo.current_dir())
                    .map_err(de::Error::custom)?;
                let normal = repo.normalize(&rel_path).map_err(de::Error::custom)?;
                pathdiff::diff_paths(&normal, cwd)
                    .unwrap_or(rel_path)
                    .clean()
            } else {
                path.clean()
            };
            let label =
                Manifest::get_atom_label(normalized.join(crate::ATOM_MANIFEST_NAME.as_str()))
                    .map_err(de::Error::custom)?;
            if let Some(path) = map.insert(label.to_owned(), normalized.to_owned()) {
                tracing::error!(
                    atoms.label = %label,
                    atoms.fst.path = %normalized.display(),
                    atoms.snd.path = %path.display(),
                    "two atoms share the same `label`"
                );
                return Err(de::Error::custom(
                    "atoms must have unique labels to retain distinct identities",
                ));
            }
        }

        Ok(AtomMap(map))
    }
}

impl Serialize for AtomMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let values: Vec<_> = self
            .as_ref()
            .values()
            .filter(|p| {
                p.join(ATOM_MANIFEST_NAME.as_str()).exists() || {
                    tracing::warn!(path = %p.display(), "atom does not exist, skipping serialization");
                    false
                }
            })
            .collect();
        values.serialize(serializer)
    }
}

impl EkalaManifest {
    /// Constructs a new Ekala manifest with the given set name
    pub fn new() -> Self {
        EkalaManifest {
            set: EkalaSet::new(),
            metadata: Some(MetaData::new()),
        }
    }

    /// Return a reference to the EkalaSet struct
    pub fn set(&self) -> &EkalaSet {
        &self.set
    }
}

impl Default for EkalaManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl EkalaSet {
    fn new() -> Self {
        EkalaSet {
            packages: AtomMap::new(),
        }
    }

    fn _packages(&self) -> &AtomMap {
        &self.packages
    }
}

impl AtomMap {
    fn new() -> Self {
        AtomMap(BTreeMap::new())
    }
}

impl MetaData {
    fn new() -> Self {
        MetaData {
            tags: Some(BTreeSet::new()),
        }
    }
}

impl<'a, S: LocalStorage> EkalaManager<'a, S> {
    /// Create a new manifest writer, traversing upward to locate the nearest ekala.toml if
    /// necessary.
    pub fn new(storage: &'a S) -> Result<Self, AtomError> {
        let path = storage
            .ekala_root_dir()
            .map_err(|e| {
                tracing::error!(message = %e);
                AtomError::EkalaManifest
            })?
            .join(crate::EKALA_MANIFEST_NAME.as_str());

        let (doc, manifest) = {
            let content = std::fs::read_to_string(&path).inspect_err(|_| {
                tracing::error!(
                    suggestion = "did you run `eka init`?",
                    "{}",
                    AtomError::EkalaManifest
                )
            })?;
            TypedDocument::new(&content)?
        };

        Ok(EkalaManager {
            doc,
            path,
            manifest,
            storage,
        })
    }

    /// writes a new, minimal atom.toml to path, and updates the ekala.toml manifest
    pub fn new_atom_at_path(
        &mut self,
        label: Label,
        package_path: impl AsRef<Path>,
        version: semver::Version,
    ) -> Result<(), storage::StorageError> {
        use std::fs;
        use std::io::Write;

        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::with_prefix_in(
            format!(".new_atom-{}-", label.as_str()),
            package_path
                .as_ref()
                .parent()
                .and_then(|p| p.exists().then_some(p))
                .unwrap_or(".".as_ref()),
        )?;

        let atom = Manifest::new(label.to_owned(), version)
            .map_err(|e| Box::new(storage::git::Error::Generic(Box::new(e))))?;
        let atom_str = toml_edit::ser::to_string_pretty(&atom)?;
        let atom_toml = package_path.as_ref().join(ATOM_MANIFEST_NAME.as_str());

        tmp.write_all(atom_str.as_bytes())?;

        if package_path.as_ref().exists() {
            let mut dir = fs::read_dir(&package_path)?;

            if dir.next().is_some() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "Directory exists and is not empty: {:?}",
                        package_path.as_ref().display()
                    ),
                ))?;
            }
            self.write_package(&package_path, label.to_owned())?;
        } else {
            fs::create_dir_all(&package_path)?;
            self.write_package(&package_path, label.to_owned())
                .inspect_err(|_| {
                    fs::remove_dir_all(&package_path).ok();
                })?;
        }
        tmp.persist(atom_toml)?;
        self.write_atomic()?;
        tracing::info!(
            message = "successfully added package to set",
            atom.label = %label,
            atom.path = %package_path.as_ref().display(),
            set = %self.path.display()
        );
        Ok(())
    }

    /// write a new package path into the packages list after verifying it is a valid atom
    fn write_package(
        &mut self,
        package_path: impl AsRef<Path>,
        label: Label,
    ) -> Result<(), storage::StorageError> {
        use toml_edit::{Array, Value};

        if let Some(path) = self.manifest.set.packages.as_ref().get(&label) {
            tracing::error!(
                suggestion = "rename one of them to maintain distinct identities",
                %label,
                manifest = %self.path.display(),
                atoms.existing.path = %path.display(),
                atoms.requested.path = %package_path.as_ref().display(),
                "atom with the given label already exists"
            );
            return Err(DocError::DuplicateAtoms.into());
        }

        let path = self.storage.normalize(package_path).map_err(|e| {
            tracing::error!(message = %e);
            storage::StorageError::NotStorage
        })?;

        let doc = self.doc.as_mut();
        let packages = doc
            .entry("set")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .and_then(|t| {
                t.set_implicit(true);
                t.entry("packages")
                    .or_insert(toml_edit::value(Value::Array(Array::new())))
                    .as_value_mut()
                    .and_then(|v| v.as_array_mut())
            })
            .ok_or(toml_edit::ser::Error::Custom(format!(
                "writing path into `[set.packages]` failed: {}",
                &path.display()
            )))?;

        packages.fmt();
        for v in packages.iter_mut() {
            *v = v.to_owned().decorated("\n\t", "");
        }
        let path: Value = path.display().to_string().into();
        packages.push_formatted(path.decorated("\n\t", ",\n"));
        doc.fmt();

        Ok(())
    }

    /// write the Ekala Manifest back to disk atomically
    fn write_atomic(&mut self) -> Result<(), DocError> {
        use std::io::Write;

        use tempfile::NamedTempFile;
        let dir = self.path.parent().ok_or(DocError::MissingEkala)?;
        let mut tmp = NamedTempFile::with_prefix_in(
            format!(".{}", crate::EKALA_MANIFEST_NAME.as_str()),
            dir,
        )?;
        tmp.write_all(self.doc.as_mut().to_string().as_bytes())?;
        tmp.persist(dir.join(crate::EKALA_MANIFEST_NAME.as_str()))?;
        Ok(())
    }
}

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for Singleton<K, V>
where
    K: Ord,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        let err = "precisely one entry";

        let map: BTreeMap<K, V> = BTreeMap::deserialize(deserializer)?;
        let len = map.len();
        if len > 1 {
            return Err(de::Error::invalid_length(len, &err));
        }
        if let Some((key, value)) = map.into_iter().next() {
            Ok(Self { key, value })
        } else {
            Err(de::Error::invalid_length(len, &err))
        }
    }
}

impl<K: Serialize, V: Serialize> Serialize for Singleton<K, V>
where
    K: Ord,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let map = BTreeMap::from([(&self.key, &self.value)]);
        map.serialize(serializer)
    }
}

impl<T: Serialize + DeserializeOwned> TypedDocument<T> {
    /// Creates a new `TypedDocument` from a serializable instance of `T`.
    /// This enforces that the document is created by serializing `T`.
    pub fn new(doc: &str) -> Result<(Self, T), DocError> {
        let validated: T = toml_edit::de::from_str(doc)?;

        let inner = doc.parse::<DocumentMut>()?;
        Ok((
            Self {
                inner,
                _marker: PhantomData,
            },
            validated,
        ))
    }
}
