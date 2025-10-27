//! # Atom Core Types
//!
//! This module contains the fundamental types that represent atoms and their
//! file system structure. These types form the foundation of the atom format
//! and are used throughout the crate.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use super::id::{Label, Tag};
use crate::manifest::AtomSet;

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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    tags: BTreeSet<Tag>,
    /// An optional description of the Atom.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
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

impl Atom {
    pub(crate) fn new(label: Label, version: Version) -> Self {
        Self {
            label,
            version,
            meta: None,
            sets: HashMap::new(),
        }
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
