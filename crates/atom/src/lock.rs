use std::path::PathBuf;

use nix_compat::nixhash::NixHash;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use toml_edit::de::Error as TomlError;
use url::Url;

#[cfg(test)]
mod test;

use crate::id::Id;

#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize)]
pub struct WrappedNixHash(pub NixHash);

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

#[derive(Error, Debug)]
/// Errors that can occur during lockfile generation, validation, or serialization.
pub enum LockError {
    /// The lockfile version is not supported.
    #[error("Invalid lockfile version: expected 1, got {0}")]
    InvalidVersion(u32),
    /// An error occurred during TOML deserialization.
    #[error("TOML deserialization error: {0}")]
    Toml(#[from] TomlError),
    /// A required field is missing for a specific dependency type.
    #[error("Missing required field for type {typ}: {field}")]
    MissingField {
        /// The dependency type with the missing field.
        typ: String,
        /// The name of the missing field.
        field: &'static str,
    },
    /// A conditional requirement (e.g., 'from' requires 'get') is not met.
    #[error("Conditional validation failed: {msg}")]
    Conditional {
        /// The description of the validation failure.
        msg: String,
    },
    /// An error occurred while parsing an atom ID.
    #[error(transparent)]
    Id(#[from] crate::id::Error),
    /// An error occurred while parsing a URL.
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
#[serde(untagged)]
pub enum GitSha {
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
pub struct AtomDep {
    /// The unique identifier of the atom.
    pub id: Id,
    /// The Git repository URL (defaults to current repo if absent).
    pub url: Option<gix_url::Url>,
    /// The semantic version of the atom.
    pub version: Version,
    /// The resolved Git revision (commit hash) for verification.
    pub rev: GitSha,
    /// The relative path to the atom (defaults to root if absent).
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
pub struct PinDep {
    /// The name of the pinned source.
    pub name: String,
    /// The URL of the source.
    pub url: Url,
    /// The hash for integrity verification (e.g., sha256).
    #[serde(serialize_with = "to_nix_hash")]
    pub hash: WrappedNixHash,
    /// The relative path within the source (for Nix imports).
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned Git repository with a specific revision.
pub struct PinGitDep {
    /// The name of the pinned Git source.
    pub name: String,
    /// The Git repository URL.
    pub url: gix_url::Url,
    /// The resolved revision (commit hash).
    pub rev: GitSha,
    /// The relative path within the repo.
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned tarball or archive source.
pub struct PinTarDep {
    /// The name of the tar source.
    pub name: String,
    /// The URL to the tarball.
    pub url: Url,
    /// The hash of the tarball.
    #[serde(serialize_with = "to_nix_hash")]
    pub hash: WrappedNixHash,
    /// The relative path within the extracted archive.
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a cross-atom source reference, acquiring a dependency from another atom.
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
    #[serde(serialize_with = "to_nix_hash")]
    pub hash: WrappedNixHash,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
/// The root structure for the lockfile, containing resolved dependencies and sources.
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

fn to_nix_hash<S>(nix_hash: &WrappedNixHash, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let WrappedNixHash(nix_hash) = nix_hash;
    let hash = nix_hash.to_nix_lowerhex_string();
    serializer.serialize_str(&hash)
}
