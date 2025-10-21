//! # Atom Manifest
//!
//! This module provides the core types for working with an Atom's manifest format.
//! The manifest is a TOML file that describes an atom's metadata and dependencies.
//!
//! ## Manifest Structure
//!
//! Every atom must have a manifest file named `atom.toml` that contains at minimum
//! a `[atom]` section with the atom's ID, version, and optional description.
//! Additional sections can specify dependencies and other configuration.
//!
//! ## Key Types
//!
//! - [`Manifest`] - The complete manifest structure, representing the `atom.toml` file.
//! - [`Atom`] - The core atom metadata (`label`, `version`, `description`).
//! - [`AtomError`] - Errors that can occur during manifest processing.
//!
//! ## Example Manifest
//!
//! ```toml
//! [atom]
//! label = "my-atom"
//! version = "1.0.0"
//! description = "A sample atom for demonstration"
//!
//! [deps.atoms]
//! other-atom = { version = "^1.0.0", path = "../other-atom" }
//!
//! [deps.pins]
//! external-lib = { url = "https://example.com/lib.tar.gz", hash = "sha256:abc123..." }
//! ```
//!
//! ## Validation
//!
//! Manifests are strictly validated to ensure they contain all required fields
//! and have valid data. The `#[serde(deny_unknown_fields)]` attribute ensures
//! that only known fields are accepted, preventing typos and invalid configurations.
//!
//! ## Usage
//!
//! Manifests can be created programmatically or parsed from a string or file.
//!
//! ```rust,no_run
//! use std::str::FromStr;
//!
//! use atom::manifest::Manifest;
//! use atom::{Atom, Label};
//! use semver::Version;
//!
//! // Create a manifest programmatically.
//! let manifest = Manifest::new(
//!     Label::try_from("my-atom").unwrap(),
//!     Version::new(1, 0, 0),
//!     Some("My first atom".to_string()),
//! );
//!
//! // Parse a manifest from a string.
//! let manifest_str = r#"
//! [atom]
//! label = "parsed-atom"
//! version = "2.0.0"
//! "#;
//! let parsed = Manifest::from_str(manifest_str).unwrap();
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use gix::{Repository, ThreadSafeRepository};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::id::Tag;
use crate::manifest::deps::{Dependency, DocError, TypedDocument};
use crate::{Atom, Label};

pub mod deps;
pub(crate) mod sets;

//================================================================================================
// Types
//================================================================================================

/// An error that can occur when parsing or handling an atom manifest.
#[derive(Error, Debug)]
pub enum AtomError {
    /// The manifest is missing the required `[atom]` table.
    #[error("Manifest is missing the `[package]` key")]
    Missing,
    /// One of the fields in the `[package]` table is missing or invalid.
    #[error(transparent)]
    InvalidAtom(#[from] de::Error),
    /// The manifest is not valid TOML.
    #[error(transparent)]
    InvalidToml(#[from] toml_edit::TomlError),
    /// An Label is missing or malformed
    #[error("failed to locate Ekala manifest in directory parents")]
    EkalaManifest,
    /// An I/O error occurred while reading the manifest file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An Label is missing or malformed
    #[error(transparent)]
    Id(#[from] crate::id::Error),
    /// A document error
    #[error(transparent)]
    Doc(#[from] DocError),
}

/// A strongly-typed representation of a source for an atom set.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AtomSet {
    /// Represents the local repository, allowing atoms to be resolved by path.
    #[serde(rename = "::")]
    Local,
    /// A URL pointing to a remote repository that serves as a source for an atom set.
    #[serde(
        serialize_with = "deps::serialize_url",
        deserialize_with = "deps::deserialize_url",
        untagged
    )]
    Url(gix::Url),
}

/// Represents the possible values for a named atom set in the manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum AtomSets {
    /// A single source for an atom set.
    Singleton(AtomSet),
    /// A set of mirrors for an atom set.
    ///
    /// Since sets can be determined to be equivalent by their root hash, this allows a user to
    /// provide multiple sources for the same set. The resolver will check for equivalence at
    /// runtime by fetching the root commit from each URL. Operations like `publish` will
    /// error if inconsistent mirrors are detected.
    Mirrors(BTreeSet<AtomSet>),
}

/// Represents the structure of an `atom.toml` manifest file.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The required `[package]` table, containing core metadata.
    pub package: Atom,
    /// The dependencies of the atom.
    #[serde(default, skip_serializing_if = "Dependency::is_empty")]
    pub(crate) deps: Dependency,
}

/// A specialized result type for manifest operations.
pub type AtomResult<T> = Result<T, AtomError>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct MetaData {
    tags: Option<BTreeSet<Tag>>,
}

/// The entrypoint for an ekala manifest describing a set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaManifest {
    set: EkalaSet,
    metadata: Option<MetaData>,
}

/// The section of the manifest describing the Ekala set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaSet {
    #[serde(default)]
    packages: AtomMap,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub(crate) struct AtomMap(BTreeMap<Label, PathBuf>);

/// A writer to assist with writing into the Ekala manifest.
#[derive(Debug)]
pub struct EkalaWriter {
    path: PathBuf,
    doc: TypedDocument<EkalaManifest>,
    repo: Option<Repository>,
}

//================================================================================================
// Impls
//================================================================================================

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

impl Manifest {
    /// Creates a new `Manifest` with the given label, version, and description.
    pub fn new(label: Label, version: Version, description: Option<String>) -> Self {
        Manifest {
            package: Atom {
                label,
                version,
                description,
                sets: HashMap::new(),
            },
            deps: Dependency::new(),
        }
    }

    /// Parses an [`Atom`] struct from the `[package]` table of a TOML document string,
    /// ignoring other tables and fields.
    ///
    /// # Errors
    ///
    /// This function will return an error if the content is invalid TOML,
    /// or if the `[package]` table is missing.
    pub(crate) fn get_atom(content: &str) -> AtomResult<Atom> {
        let doc = content.parse::<DocumentMut>()?;

        if let Some(v) = doc.get("package").map(ToString::to_string) {
            let atom = de::from_str::<Atom>(&v)?;
            Ok(atom)
        } else {
            Err(AtomError::Missing)
        }
    }

    pub(crate) fn get_atom_label<P: AsRef<Path>>(path: P) -> AtomResult<Label> {
        let content = std::fs::read_to_string(&path)?;
        let atom = Self::get_atom(&content)?;
        Ok(atom.label)
    }

    pub(crate) fn deps(&self) -> &Dependency {
        &self.deps
    }
}

impl FromStr for Manifest {
    type Err = de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        de::from_str(s)
    }
}

impl TryFrom<PathBuf> for Manifest {
    type Error = AtomError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let content = std::fs::read_to_string(path)?;
        Ok(Manifest::from_str(&content)?)
    }
}

impl<'de> Deserialize<'de> for AtomMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let entries: Vec<PathBuf> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();

        for path in entries {
            let label = Manifest::get_atom_label(path.join(crate::ATOM_MANIFEST_NAME.as_str()))
                .map_err(serde::de::Error::custom)?;
            map.insert(label, path);
        }

        Ok(AtomMap(map))
    }
}

impl Serialize for AtomMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let values: Vec<_> = self.as_ref().values().collect();
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

impl EkalaWriter {
    /// Create a new manifest writer, traversing upward to locate the nearest ekala.toml
    pub fn new(repo: Option<&ThreadSafeRepository>) -> Result<Self, AtomError> {
        let path = if let Some(repo) = repo {
            repo.work_dir()
                .map(|p| p.join(crate::EKALA_MANIFEST_NAME.as_str()))
        } else {
            find_upwards(crate::EKALA_MANIFEST_NAME.as_str())?
        }
        .ok_or(AtomError::EkalaManifest)?;

        let (doc, _) = {
            let content = std::fs::read_to_string(&path)?;
            TypedDocument::new(&content)?
        };

        Ok(EkalaWriter {
            doc,
            path,
            repo: repo.map(|r| r.to_thread_local()),
        })
    }

    /// write a new package path into the packages list
    pub fn write_package(
        &mut self,
        package_path: impl AsRef<Path>,
    ) -> Result<(), crate::store::git::Error> {
        use toml_edit::{Array, Value};

        use crate::store::NormalizeStorePath;
        let path = if let Some(repo) = &self.repo {
            repo.normalize(package_path)?
        } else {
            package_path.as_ref().into()
        };

        let doc = self.doc.as_mut();
        let set = doc
            .entry("set")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();

        let packages = set
            .entry("packages")
            .or_insert(toml_edit::value(Value::Array(Array::new())))
            .as_value_mut()
            .and_then(|v| v.as_array_mut())
            .unwrap();

        packages.push(path.display().to_string());
        packages.fmt();

        Ok(())
    }

    /// write the Ekala Manifest back to disk atomically
    pub fn write_atomic(&mut self) -> Result<(), DocError> {
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

fn find_upwards(filename: &str) -> Result<Option<PathBuf>, std::io::Error> {
    let start_dir = std::env::current_dir()?;

    for ancestor in start_dir.ancestors() {
        let file_path = ancestor.join(filename);
        if file_path.exists() {
            return Ok(Some(file_path));
        }
    }

    Ok(None)
}
