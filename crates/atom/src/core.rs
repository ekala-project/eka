//! # Atom Core Types
//!
//! This module contains the fundamental types that represent atoms and their
//! file system structure. These types form the foundation of the atom format
//! and are used throughout the crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use super::id::{Label, Name};
use crate::manifest::AtomSets;

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
    pub label: Label,

    /// The version of the Atom.
    pub version: Version,

    /// An optional description of the Atom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A table of named atom sets, defining the sources for resolving atom dependencies.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub sets: HashMap<Name, AtomSets>,
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
