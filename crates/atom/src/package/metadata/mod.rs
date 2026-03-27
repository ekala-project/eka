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

use bimap::BiBTreeMap;
use id::{Label, Tag};
use manifest::{AtomSet, ComposeError, Manifest, SetMirror};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use super::{AtomError, sets};
use crate::storage::LocalStorage;
use crate::uri::AliasedUrl;
use crate::{ATOM_MANIFEST_NAME, ManifestWriter, id, storage};

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
pub(crate) struct TypedDocument<T> {
    /// The underlying `toml_edit` document.
    inner: DocumentMut,
    _marker: PhantomData<T>,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct AtomMap(BiBTreeMap<Label, PathBuf>);

#[derive(thiserror::Error, Debug)]
/// Errors that can occur when working with a `TypedDocument`.
pub enum DocError {
    /// Missing atom from manifest
    #[error("the atom directory is inaccessible: {0}")]
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
    /// A dynamic atom must specify a composer
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

/// Internal type for raw deserialization of the set section.
/// Contains unvalidated paths - labels not yet resolved.
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEkalaSet {
    #[serde(default)]
    packages: Vec<PathBuf>,
}

impl RawEkalaSet {
    /// Returns the raw package paths.
    pub(crate) fn packages(&self) -> &[PathBuf] {
        &self.packages
    }
}

/// Internal type for raw deserialization of ekala.toml.
/// MUST be validated before use via `EkalaManifest::open_*` constructors.
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEkalaManifest {
    pub(crate) set: RawEkalaSet,
    metadata: Option<MetaData>,
}

/// The section of the manifest describing the Ekala set of atoms.
///
/// Contains a validated `AtomMap` with unique labels.
/// This type can only be constructed via validated constructors,
/// ensuring the label uniqueness invariant is maintained.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct EkalaSet {
    pub(in crate::package) packages: AtomMap,
}

/// The entrypoint for an ekala manifest describing a set of atoms.
///
/// This type can ONLY be constructed via validated constructors like
/// `open_filesystem()` or `from_git_tree()`, ensuring that label
/// uniqueness is validated. This makes invalid state literally
/// unrepresentable at the type level.
#[derive(Serialize, Debug, PartialEq, Eq)]
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
    /// The raw document for TOML editing
    doc: TypedDocument<RawEkalaManifest>,
    pub(super) storage: &'a S,
    /// The validated manifest with unique labels
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
pub(super) struct Singleton<K, V> {
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

    /// take both the label and version discarding self
    pub fn take(self) -> (Label, Version) {
        (self.label, self.version)
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

impl AsRef<BiBTreeMap<Label, PathBuf>> for AtomMap {
    fn as_ref(&self) -> &BiBTreeMap<Label, PathBuf> {
        &self.0
    }
}

impl AsMut<BiBTreeMap<Label, PathBuf>> for AtomMap {
    fn as_mut(&mut self) -> &mut BiBTreeMap<Label, PathBuf> {
        &mut self.0
    }
}

// AtomMap deserialization is handled by RawEkalaSet::resolve_to_atom_map()
// which properly validates label uniqueness with the correct context.

impl Serialize for AtomMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let values: Vec<_> = self
            .as_ref()
            .right_values()
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
    /// Constructs a new, empty Ekala manifest.
    pub fn new() -> Self {
        EkalaManifest {
            set: EkalaSet::new(),
            metadata: Some(MetaData::new()),
        }
    }

    /// Opens and validates an ekala.toml from the filesystem.
    ///
    /// Reads the manifest, resolves all atom labels by reading each atom.toml,
    /// and validates that no duplicate labels exist.
    ///
    /// # Arguments
    /// * `path` - Path to the ekala.toml file
    ///
    /// # Returns
    /// A validated `EkalaManifest` where label uniqueness is guaranteed.
    ///
    /// # Errors
    /// Returns error if:
    /// - The manifest cannot be read or parsed
    /// - Any atom.toml cannot be read
    /// - Duplicate labels are found (critical invariant violation)
    pub fn open_filesystem(path: impl AsRef<Path>) -> Result<Self, DocError> {
        let path = path.as_ref();
        let root = path.parent().ok_or(DocError::MissingEkala)?;
        let content = std::fs::read_to_string(path)?;
        let raw: RawEkalaManifest = toml_edit::de::from_str(&content)?;

        // Validate and resolve labels - this enforces the uniqueness invariant
        let atom_map = raw.set.resolve_to_atom_map(root)?;

        Ok(EkalaManifest {
            set: EkalaSet { packages: atom_map },
            metadata: raw.metadata,
        })
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

    /// Returns the validated AtomMap of packages.
    pub fn packages(&self) -> &AtomMap {
        &self.packages
    }
}

impl RawEkalaSet {
    /// Resolves paths to an AtomMap using filesystem access.
    ///
    /// `root` should be the directory containing ekala.toml (repo root).
    /// Each path is joined with root and the atom.toml is read to get the label.
    /// Returns error if duplicate labels are found.
    fn resolve_to_atom_map<P: AsRef<Path>>(&self, root: P) -> Result<AtomMap, DocError> {
        use path_clean::PathClean;

        let root = root.as_ref();
        let mut map = BiBTreeMap::new();

        for path in &self.packages {
            let normalized = path.clean();
            let abs_path = root.join(&normalized);
            let manifest_path = abs_path.join(crate::ATOM_MANIFEST_NAME.as_str());

            let label = match Manifest::get_atom_label(&manifest_path) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %abs_path.display(),
                        suggestion = "you likely want to remove it from the set, or perhaps recreate it",
                        "atom no longer exists"
                    );
                    continue;
                },
            };

            if let bimap::Overwritten::Both(.., (_, old_path))
            | bimap::Overwritten::Left(.., old_path)
            | bimap::Overwritten::Right(.., old_path)
            | bimap::Overwritten::Pair(.., old_path) =
                map.insert(label.to_owned(), normalized.to_owned())
            {
                tracing::error!(
                    atoms.label = %label,
                    atoms.fst.path = %normalized.display(),
                    atoms.snd.path = %old_path.display(),
                    "two atoms share the same `label`"
                );
                return Err(DocError::DuplicateAtoms);
            }
        }

        Ok(AtomMap(map))
    }
}

impl AtomMap {
    fn new() -> Self {
        AtomMap(BiBTreeMap::new())
    }
}

impl From<BiBTreeMap<Label, PathBuf>> for AtomMap {
    fn from(map: BiBTreeMap<Label, PathBuf>) -> Self {
        AtomMap(map)
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
    ///
    /// This validates the manifest on open, ensuring label uniqueness.
    pub fn open(storage: &'a S) -> Result<Self, AtomError> {
        let path = storage
            .ekala_root_dir()
            .map_err(|e| {
                tracing::error!(message = %e);
                AtomError::EkalaManifest
            })?
            .join(crate::EKALA_MANIFEST_NAME.as_str());

        let content = std::fs::read_to_string(&path).inspect_err(|_| {
            tracing::error!(
                suggestion = "did you run `eka init`?",
                "{}",
                AtomError::EkalaManifest
            )
        })?;

        // Parse raw document for editing capability
        let (doc, raw): (TypedDocument<RawEkalaManifest>, RawEkalaManifest) =
            TypedDocument::new(&content)?;

        // Validate and resolve labels
        let root = path.parent().ok_or(AtomError::EkalaManifest)?;
        let atom_map = raw.set.resolve_to_atom_map(root)?;
        let manifest = EkalaManifest {
            set: EkalaSet { packages: atom_map },
            metadata: raw.metadata,
        };

        Ok(EkalaManager {
            doc,
            path,
            manifest,
            storage,
        })
    }

    /// Returns the validated `AtomMap` from the contained Ekala manifest.
    ///
    /// Since labels are validated on construction, this is infallible.
    pub fn atoms(&self) -> &AtomMap {
        self.manifest.set().packages()
    }

    /// writes a new, minimal atom.toml to path, and updates the ekala.toml manifest
    pub async fn new_atom_at_path(
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
        tmp.persist(&atom_toml)?;
        self.write_atomic()?;
        tracing::info!(
            message = "successfully added package to set",
            atom.label = %label,
            atom.path = %package_path.as_ref().display(),
            set = %self.path.display()
        );
        let writer = ManifestWriter::open_and_resolve(self.storage, &atom_toml, true).await?;
        writer.write_atomic()?;

        Ok(())
    }

    /// write a new package path into the packages list after verifying it is a valid atom
    fn write_package(
        &mut self,
        package_path: impl AsRef<Path>,
        label: Label,
    ) -> Result<(), storage::StorageError> {
        use toml_edit::{Array, Value};

        // Check for duplicate labels using the already-validated AtomMap
        if let Some(path) = self.manifest.set().packages().as_ref().get_by_left(&label) {
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
            *v = v.to_owned().decorated("\n  ", "");
        }
        let path: Value = path.display().to_string().into();
        packages.push_formatted(path.decorated("\n  ", ",\n"));
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
