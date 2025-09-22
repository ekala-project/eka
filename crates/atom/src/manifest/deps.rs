//! # Atom Dependency Handling
//!
//! Provides the core types for working with an Atom manifest's dependencies.
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
pub enum PinType {
    Direct(DirectPin),
    Indirect(IndirectPin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct DirectPin {
    /// The URL of the source.
    pub url: Url,
    /// The refspec (e.g. branch or tag) of the source (for git-type pins).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct IndirectPin {
    /// The atom id to reference a pin from.
    pub from: Id,
    /// The name of the dependency to acquire from the atom (same as it's name if not present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
#[serde(deny_unknown_fields)]
pub struct PinReq {
    /// The relative path within the source (for Nix imports).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
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
    pub fn new(version: VersionReq, locale: AtomLocation) -> Self {
        Self { version, locale }
    }
}
