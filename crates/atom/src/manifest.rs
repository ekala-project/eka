//! # Atom Manifest
//!
//! Provides the core types for working with an Atom's manifest format.
mod depends;

use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::Atom;
use crate::id::Id;

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
}

type AtomResult<T> = Result<T, AtomError>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Dependencies {
    /// The \[atoms] key, specifying other atom dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atoms: Option<HashMap<Id, depends::Atoms>>,
    /// The \[pins] key, specifying eval dependencies not in the atom format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pins: Option<HashMap<String, depends::Srcs>>,
    /// The \[srcs] key, specifying buildtime sources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srcs: Option<HashMap<String, depends::Srcs>>,
}

/// The type representing the required fields of an Atom's manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The required \[atom] key of the TOML manifest.
    pub atom: Atom,
    /// The \[deps] key, specifying dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<Dependencies>,
}

impl Manifest {
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
