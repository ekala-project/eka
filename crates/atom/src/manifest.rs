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
//! - [`Atom`] - The core atom metadata (`tag`, `version`, `description`).
//! - [`AtomError`] - Errors that can occur during manifest processing.
//!
//! ## Example Manifest
//!
//! ```toml
//! [atom]
//! tag = "my-atom"
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
//! use atom::{Atom, AtomTag};
//! use semver::Version;
//!
//! // Create a manifest programmatically.
//! let manifest = Manifest::new(
//!     AtomTag::try_from("my-atom").unwrap(),
//!     Version::new(1, 0, 0),
//!     Some("My first atom".to_string()),
//! );
//!
//! // Parse a manifest from a string.
//! let manifest_str = r#"
//! [atom]
//! tag = "parsed-atom"
//! version = "2.0.0"
//! "#;
//! let parsed = Manifest::from_str(manifest_str).unwrap();
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::id::Name;
use crate::{Atom, AtomTag};

pub mod deps;

/// A specialized result type for manifest operations.
pub type AtomResult<T> = Result<T, AtomError>;

/// An error that can occur when parsing or handling an atom manifest.
#[derive(Error, Debug)]
pub enum AtomError {
    /// The manifest is missing the required `[atom]` table.
    #[error("Manifest is missing the `[atom]` key")]
    Missing,
    /// One of the fields in the `[atom]` table is missing or invalid.
    #[error(transparent)]
    InvalidAtom(#[from] de::Error),
    /// The manifest is not valid TOML.
    #[error(transparent)]
    InvalidToml(#[from] toml_edit::TomlError),
    /// An I/O error occurred while reading the manifest file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Represents the structure of an `atom.toml` manifest file.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The required `[atom]` table, containing core metadata.
    pub atom: Atom,
    /// The dependencies of the atom.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) deps: HashMap<Name, deps::Dependency>,
}

impl Manifest {
    /// Creates a new `Manifest` with the given tag, version, and description.
    pub fn new(tag: AtomTag, version: Version, description: Option<String>) -> Self {
        Manifest {
            atom: Atom {
                tag,
                version,
                description,
            },
            deps: HashMap::new(),
        }
    }

    /// Parses an [`Atom`] struct from the `[atom]` table of a TOML document string,
    /// ignoring other tables and fields.
    ///
    /// # Errors
    ///
    /// This function will return an error if the content is invalid TOML,
    /// or if the `[atom]` table is missing.
    pub(crate) fn get_atom(content: &str) -> AtomResult<Atom> {
        let doc = content.parse::<DocumentMut>()?;

        if let Some(v) = doc.get("atom").map(ToString::to_string) {
            let atom = de::from_str::<Atom>(&v)?;
            Ok(atom)
        } else {
            Err(AtomError::Missing)
        }
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
