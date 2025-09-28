//! # Atom Manifest
//!
//! This module provides the core types for working with an Atom's manifest format.
//! The manifest is a TOML file that describes an atom's metadata and dependencies.
//!
//! ## Manifest Structure
//!
//! Every atom must have a manifest file named `atom.toml` that contains at minimum
//! an `[atom]` section with the atom's ID, version, and optional description.
//! Additional sections can specify dependencies and other configuration.
//!
//! ## Key Types
//!
//! - [`Manifest`] - The complete manifest structure
//! - [`Atom`] - The core atom metadata (id, version, description)
//! - [`AtomError`] - Errors that can occur during manifest processing
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
//! ```rust,no_run
//! use atom::manifest::Manifest;
//! use atom::{Atom, AtomTag};
//! use semver::Version;
//!
//! // Create a manifest programmatically
//! let manifest = Manifest::new(
//!     AtomTag::try_from("my-atom").unwrap(),
//!     Version::new(1, 0, 0),
//!     Some("My first atom".to_string()),
//! );
//!
//! // Parse a manifest from a string
//! let manifest_str = r#"
//! [atom]
//! tag = "parsed-atom"
//! version = "2.0.0"
//! "#;
//! let parsed: Manifest = manifest_str.parse().unwrap();
//! ```

pub mod deps;
use std::path::PathBuf;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::{Atom, AtomTag};

/// Errors which occur during manifest (de)serialization.
#[derive(Error, Debug)]
pub enum AtomError {
    /// The manifest is missing the required \[atom] key.
    #[error("Manifest is missing the `[atom]` key")]
    Missing,
    /// One of the fields in the required \[atom] key is missing or invalid.
    #[error(transparent)]
    InvalidAtom(#[from] de::Error),
    /// The manifest is not valid TOML.
    #[error(transparent)]
    InvalidToml(#[from] toml_edit::TomlError),
    /// The manifest could not be read.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type AtomResult<T> = Result<T, AtomError>;

/// The type representing the required fields of an Atom's manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The required \[atom] key of the TOML manifest.
    pub atom: Atom,
    /// The dependencies of the Atom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<deps::Dependency>,
}

impl Manifest {
    /// Create a new atom Manifest with the given values.
    pub fn new(tag: AtomTag, version: Version, description: Option<String>) -> Self {
        Manifest {
            atom: Atom {
                tag,
                version,
                description,
            },
            deps: None,
        }
    }

    /// Build an Atom struct from the \[atom] key of a TOML manifest,
    /// ignoring other fields or keys].
    ///
    /// # Errors
    ///
    /// This function will return an error if the content is invalid
    /// TOML, or if the \[atom] key is missing.
    pub fn get_atom(content: &str) -> AtomResult<Atom> {
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
