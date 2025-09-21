use std::path::PathBuf;

use nix_compat::nixhash::NixHash;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[cfg(test)]
mod test;

use crate::id::Id;

#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize)]
pub struct WrappedNixHash(pub NixHash);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
#[serde(untagged)]
pub enum GitSha {
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
pub enum AtomLocation {
    /// The Git repository URL (defaults to current repo if absent).
    #[serde(rename = "url")]
    Url(Url),
    /// The relative path to the atom (defaults to root if absent).
    #[serde(rename = "path")]
    Path(PathBuf),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
#[serde(deny_unknown_fields)]
pub struct AtomDep {
    /// The unique identifier of the atom.
    pub id: Id,
    /// The semantic version of the atom.
    pub version: Version,
    /// The resolved Git revision (commit hash) for verification.
    pub rev: GitSha,
    /// The location of the atom, whether local or remote.
    #[serde(flatten)]
    pub location: Option<AtomLocation>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
#[serde(deny_unknown_fields)]
pub struct PinDep {
    /// The name of the pinned source.
    pub name: String,
    /// The URL of the source.
    pub url: Url,
    /// The hash for integrity verification (e.g., sha256).
    pub hash: WrappedNixHash,
    /// The relative path within the source (for Nix imports).
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned Git repository with a specific revision.
#[serde(deny_unknown_fields)]
pub struct PinGitDep {
    /// The name of the pinned Git source.
    pub name: String,
    /// The Git repository URL.
    pub url: Url,
    /// The resolved revision (commit hash).
    pub rev: GitSha,
    /// The relative path within the repo.
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned tarball or archive source.
#[serde(deny_unknown_fields)]
pub struct PinTarDep {
    /// The name of the tar source.
    pub name: String,
    /// The URL to the tarball.
    pub url: Url,
    /// The hash of the tarball.
    pub hash: WrappedNixHash,
    /// The relative path within the extracted archive.
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a cross-atom source reference, acquiring a dependency from another atom.
#[serde(deny_unknown_fields)]
pub struct FromDep {
    /// The name of the sourced dependency.
    pub name: String,
    /// The atom ID from which to source.
    pub from: Id,
    /// The name of the dependency to acquire from the 'from' atom (defaults to `name`).
    pub get: Option<String>,
    /// The relative path for the sourced item (for Nix imports).
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
/// Enum representing the different types of locked dependencies, serialized as tagged TOML tables.
pub enum Dep {
    #[serde(rename = "atom")]
    /// An atom dependency variant.
    Atom(AtomDep),
    #[serde(rename = "pin")]
    /// A direct pin to an external source variant.
    Pin(PinDep),
    #[serde(rename = "pin+git")]
    /// A Git-specific pin variant.
    PinGit(PinGitDep),
    #[serde(rename = "pin+tar")]
    /// A tarball pin variant.
    PinTar(PinTarDep),
    #[serde(rename = "from")]
    /// A cross-atom source reference variant.
    From(FromDep),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
/// Enum representing the different types of locked sources, serialized as tagged TOML tables.
pub enum Src {
    /// A reference to a build source
    #[serde(rename = "build")]
    Build(BuildSrc),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a locked build-time source, such as a registry or configuration.
pub struct BuildSrc {
    /// The name of the source.
    pub name: String,
    /// The URL to fetch the source.
    pub url: Url,
    /// The hash for verification.
    pub hash: WrappedNixHash,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
/// The root structure for the lockfile, containing resolved dependencies and sources.
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// The version of the lockfile schema.
    pub version: u8,
    /// The list of locked dependencies (absent or empty if none).
    pub deps: Option<Vec<Dep>>,
    /// The list of locked build-time sources (absent or empty if none).
    pub srcs: Option<Vec<Src>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The resolution mode for generating the lockfile.
pub enum ResolutionMode {
    /// Shallow resolution: Lock only direct dependencies.
    Shallow,
    /// Deep resolution: Recursively lock all transitive dependencies (future).
    Deep,
}

impl<'de> Deserialize<'de> for WrappedNixHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize into a String to handle owned data
        let s = String::deserialize(deserializer)?;
        // Pass the String as &str to NixHash::from_str
        let hash = NixHash::from_str(&s, None).map_err(|_| {
            serde::de::Error::invalid_value(serde::de::Unexpected::Str(&s), &"NixHash")
        })?;
        Ok(WrappedNixHash(hash))
    }
}
