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

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::manifest::deps::{Dependency, DocError};
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
    /// An I/O error occurred while reading the manifest file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An Label is missing or malformed
    #[error(transparent)]
    Id(#[from] crate::id::Error),
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

/// The entrypoint for an ekala manifest describing a set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaManifest {
    set: EkalaSet,
    #[serde(default, skip_serializing_if = "AtomMap::is_empty")]
    packages: AtomMap,
}

/// The section of the manifest describing the Ekala set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaSet {
    name: String,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub(crate) struct AtomMap(BTreeMap<Label, PathBuf>);

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
        let doc = content.parse::<DocumentMut>()?;

        let label = doc["package"]["label"].to_string();
        tracing::error!(message = "atom label is missing or malformed", %label, path = %path.as_ref().display());
        Label::try_from(label).map_err(Into::into)
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
            let label = Manifest::get_atom_label(&path).map_err(serde::de::Error::custom)?;
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
    pub fn new(name: String) -> Result<Self, DocError> {
        Ok(EkalaManifest {
            set: EkalaSet::new(name)?,
            packages: Default::default(),
        })
    }

    /// Return a reference to the EkalaSet struct
    pub fn set(&self) -> &EkalaSet {
        &self.set
    }

    /// Add an atom to the manifest, assuming it's valid
    pub fn add_package<P: AsRef<Path>>(&mut self, path: P) -> AtomResult<Label> {
        let packages = self.packages.as_mut();
        let name =
            Manifest::get_atom_label(path.as_ref().join(crate::ATOM_MANIFEST_NAME.as_str()))?;
        packages.insert(name.to_owned(), path.as_ref().into());
        Ok(name)
    }
}

impl EkalaSet {
    fn new(name: String) -> Result<Self, DocError> {
        use gix::validate::reference;
        reference::name_partial(name.as_str().into())?;

        Ok(EkalaSet { name })
    }

    /// return a refernce to the name of this set
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

impl AtomMap {
    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }
}
