//! # Atom Dependency Handling
//!
//! This module provides the core types for working with an Atom manifest's dependencies.
//! It defines the structure for specifying different types of dependencies in an atom's
//! manifest file, including atom references, direct pins, and build-time sources.
//!
//! ## Dependency Types
//!
//! The manifest supports three main categories of dependencies:
//!
//! - **Atom dependencies** - References to other atoms by ID and version
//! - **Pin dependencies** - Direct references to external sources (URLs, Git repos, tarballs)
//! - **Source dependencies** - Build-time dependencies like source code or config files
//!
//! ## Key Types
//!
//! - [`Dependency`] - The main dependency structure containing all dependency types
//! - [`AtomReq`] - Requirements for atom dependencies
//! - [`PinReq`] - Requirements for pinned dependencies
//! - [`SrcReq`] - Requirements for build-time sources
//! - [`PinType`] - Enum distinguishing between direct and indirect pins
//!
//! ## Example Usage
//!
//! ```toml
//! [deps.atoms]
//! # Reference to another atom
//! other-atom = { version = "^1.0.0", path = "../other-atom" }
//!
//! [deps.pins]
//! # pin to external evaluation time source code
//! external-lib = { url = "https://example.com/lib.tar.gz" }
//!
//! # Git pin
//! git-dep = { url = "https://github.com/user/repo.git", ref = "main" }
//!
//! # Indirect pin (from another atom)
//! shared-lib = { from = "other-atom", get = "lib" }
//!
//! [deps.srcs]
//! # Build-time source
//! src-code = { url = "https://registry.example.com/code.tar.gz" }
//! ```
//!
//! ## Validation
//!
//! All dependency types use `#[serde(deny_unknown_fields)]` to ensure strict
//! validation and prevent typos in manifest files. Optional fields are properly
//! handled with `skip_serializing_if` to keep the TOML output clean.
use std::collections::HashMap;
use std::path::PathBuf;

use semver::VersionReq;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::id::Id;
use crate::lock::AtomLocation;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// The dependencies specified in the manifest
pub struct Dependency {
    /// An atom dependency variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    atoms: Option<HashMap<Id, AtomReq>>,
    /// A direct pin to an external source variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pins: Option<HashMap<String, PinReq>>,
    /// A dependency fetched at build-time as an FOD.
    #[serde(skip_serializing_if = "Option::is_none")]
    srcs: Option<HashMap<String, SrcReq>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
#[serde(deny_unknown_fields)]
pub struct AtomReq {
    /// The semantic version request specification of the atom.
    version: VersionReq,
    /// The location of the atom, whether local or remote.
    #[serde(flatten)]
    locale: AtomLocation,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
/// Represents the different types of pins for dependencies.
///
/// This enum distinguishes between direct pins (pointing to external URLs)
/// and indirect pins (referencing dependencies from other atoms).
pub enum PinType {
    /// A direct pin to an external source with a URL.
    Direct(DirectPin),
    /// An indirect pin referencing a dependency from another atom.
    Indirect(IndirectPin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a direct pin to an external source.
///
/// This struct is used when a dependency is pinned directly to a URL,
/// such as a Git repository, tarball, or other external source.
pub struct DirectPin {
    /// The URL of the source.
    pub url: Url,
    /// The refspec (e.g. branch or tag) of the source (for git-type pins).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents an indirect pin referencing a dependency from another atom.
///
/// This struct is used when a dependency is sourced from another atom,
/// enabling composition of complex systems from simpler atom components.
pub struct IndirectPin {
    /// The atom id to reference a pin from.
    pub from: Id,
    /// The name of the dependency to acquire from the atom (same as it's name if not present).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used to specify pinned dependencies in the manifest,
/// which can be either direct (pointing to URLs) or indirect (referencing
/// dependencies from other atoms).
#[serde(deny_unknown_fields)]
pub struct PinReq {
    /// The relative path within the source (for Nix imports).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The type of pin, either direct or indirect.
    ///
    /// This field is flattened in the TOML serialization.
    #[serde(flatten)]
    pub kind: PinType,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a dependency which is fetched at build time as an FOD.
#[serde(deny_unknown_fields)]
pub struct SrcReq {
    /// The URL of the source.
    pub url: Url,
}

impl AtomReq {
    /// Creates a new `AtomReq` with the specified version requirement and location.
    ///
    /// # Arguments
    ///
    /// * `version` - The semantic version requirement for the atom
    /// * `locale` - The location of the atom, either as a URL or relative path
    ///
    /// # Returns
    ///
    /// A new `AtomReq` instance with the provided version and location.
    pub fn new(version: VersionReq, locale: AtomLocation) -> Self {
        Self { version, locale }
    }
}
