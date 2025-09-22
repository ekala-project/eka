//! # Atom Lockfile Format
//!
//! This module provides the types and structures for working with Atom lockfiles.
//! Lockfiles capture the exact versions and revisions of dependencies for reproducible
//! builds, similar to Cargo.lock or flake.lock but designed for the Atom ecosystem.
//!
//! The lockfile format uses TOML with tagged enums for type safety while maintaining
//! portability across different tools and languages.

use std::path::PathBuf;

use nix_compat::nixhash::NixHash;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[cfg(test)]
mod test;

use crate::id::Id;

/// A wrapper around NixHash to provide custom serialization behavior.
#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize)]
pub struct WrappedNixHash(pub NixHash);

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
#[serde(untagged)]
pub enum GitSha {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
/// Represents the location of an atom, either as a URL or a relative path.
///
/// This enum is used to specify where an atom can be found, supporting both
/// remote Git repositories and local relative paths within a repository.
pub enum AtomLocation {
    /// A URL pointing to a Git repository containing the atom.
    ///
    /// When this variant is used, the atom will be fetched from the specified
    /// Git repository URL. If not provided, defaults to the current repository.
    #[serde(rename = "url")]
    Url(Url),
    /// A relative path within the repository where the atom is located.
    ///
    /// When this variant is used, the atom is located at the specified path
    /// relative to the current atom. If not provided, defaults to the root.
    #[serde(rename = "path")]
    Path(PathBuf),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
///
/// This struct captures all the information needed to uniquely identify and
/// fetch a specific version of an atom from a Git repository.
#[serde(deny_unknown_fields)]
pub struct AtomDep {
    /// The unique identifier of the atom.
    pub id: Id,
    /// The semantic version of the atom.
    pub version: Version,
    /// The resolved Git revision (commit hash) for verification.
    pub rev: GitSha,
    /// The location of the atom, whether local or remote.
    ///
    /// This field is flattened in the TOML serialization and omitted if None.
    #[serde(flatten)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<AtomLocation>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used for dependencies that are pinned to specific URLs
/// with integrity verification through cryptographic hashes.
#[serde(deny_unknown_fields)]
pub struct PinDep {
    /// The name of the pinned source.
    pub name: String,
    /// The URL of the source.
    pub url: Url,
    /// The hash for integrity verification (e.g., sha256).
    pub hash: WrappedNixHash,
    /// The relative path within the source (for Nix imports).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned Git repository with a specific revision.
///
/// This struct is used for dependencies that are pinned to specific Git
/// repositories and commits, providing both URL and revision information.
#[serde(deny_unknown_fields)]
pub struct PinGitDep {
    /// The name of the pinned Git source.
    pub name: String,
    /// The Git repository URL.
    pub url: Url,
    /// The resolved revision (commit hash).
    pub rev: GitSha,
    /// The relative path within the repo.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a pinned tarball or archive source.
///
/// This struct is used for dependencies that are distributed as tarballs
/// or archives, with integrity verification through cryptographic hashes.
#[serde(deny_unknown_fields)]
pub struct PinTarDep {
    /// The name of the tar source.
    pub name: String,
    /// The URL to the tarball.
    pub url: Url,
    /// The hash of the tarball.
    pub hash: WrappedNixHash,
    /// The relative path within the extracted archive.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a cross-atom source reference, acquiring a dependency from another atom.
///
/// This struct enables atoms to reference dependencies from other atoms,
/// creating a composition mechanism for building complex systems from simpler parts.
#[serde(deny_unknown_fields)]
pub struct FromDep {
    /// The name of the sourced dependency.
    pub name: String,
    /// The atom ID from which to source.
    pub from: Id,
    /// The name of the dependency to acquire from the 'from' atom (defaults to `name`).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<String>,
    /// The relative path for the sourced item (for Nix imports).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
/// Enum representing the different types of locked dependencies, serialized as tagged TOML tables.
///
/// This enum provides a type-safe way to represent different kinds of dependencies
/// in the lockfile, ensuring that each dependency type has the correct fields
/// and validation at compile time.
pub enum Dep {
    /// An atom dependency variant.
    ///
    /// Represents a dependency on another atom, identified by its ID, version,
    /// and Git revision.
    #[serde(rename = "atom")]
    Atom(AtomDep),
    /// A direct pin to an external source variant.
    ///
    /// Represents a dependency pinned to a specific URL with integrity verification.
    /// Used for dependencies that are not atoms but need to be fetched from external sources.
    #[serde(rename = "pin")]
    Pin(PinDep),
    /// A Git-specific pin variant.
    ///
    /// Represents a dependency pinned to a specific Git repository and commit.
    /// Similar to Pin but specifically for Git repositories.
    #[serde(rename = "pin+git")]
    PinGit(PinGitDep),
    /// A tarball pin variant.
    ///
    /// Represents a dependency pinned to a tarball or archive file.
    /// Used for dependencies distributed as compressed archives.
    #[serde(rename = "pin+tar")]
    PinTar(PinTarDep),
    /// A cross-atom source reference variant.
    ///
    /// Represents a dependency that is sourced from another atom, enabling
    /// composition of complex systems from simpler atom components.
    #[serde(rename = "from")]
    From(FromDep),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
/// Enum representing the different types of locked sources, serialized as tagged TOML tables.
///
/// This enum provides a type-safe way to represent different kinds of sources
/// that need to be available during the build process, such as registries
/// or configuration files.
pub enum Src {
    /// A reference to a build source.
    ///
    /// Represents a source that needs to be fetched and available during the
    /// build process, such as source code or configuration file.
    #[serde(rename = "build")]
    Build(BuildSrc),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a locked build-time source, such as a registry or configuration.
///
/// This struct is used for sources that are fetched during the build process,
/// such as package registries or configuration files that need to be available
/// at build time.
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
///
/// This struct represents the complete lockfile format used by atom to capture
/// the exact versions and revisions of all dependencies for reproducible builds.
/// The lockfile ensures that builds are deterministic and can be reproduced
/// across different environments.
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// The version of the lockfile schema.
    ///
    /// This field allows for future evolution of the lockfile format while
    /// maintaining backward compatibility.
    pub version: u8,
    /// The list of locked dependencies (absent or empty if none).
    ///
    /// This field contains all the resolved dependencies with their exact
    /// versions and revisions. It is omitted from serialization if None or empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<Dep>>,
    /// The list of locked build-time sources (absent or empty if none).
    ///
    /// This field contains sources that need to be available during the build
    /// process, such as registries or configuration files. It is omitted from
    /// serialization if None or empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srcs: Option<Vec<Src>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The resolution mode for generating the lockfile.
///
/// This enum controls how dependencies are resolved when generating a lockfile,
/// determining whether to lock only direct dependencies or recursively resolve
/// all transitive dependencies.
pub enum ResolutionMode {
    /// Shallow resolution: Lock only direct dependencies.
    ///
    /// In this mode, only the immediate dependencies declared in the manifest
    /// are resolved and locked. Transitive dependencies are not included in
    /// the lockfile, making it faster but less comprehensive.
    Shallow,
    /// Deep resolution: Recursively lock all transitive dependencies (future).
    ///
    /// In this mode, all dependencies and their dependencies are recursively
    /// resolved and locked, ensuring complete reproducibility but requiring
    /// more time and resources. This feature is planned for future implementation.
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
