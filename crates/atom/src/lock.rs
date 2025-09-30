//! # Atom Lockfile Format
//!
//! This module provides the types and structures for working with Atom lockfiles.
//! Lockfiles capture the exact versions and revisions of dependencies for reproducible
//! builds, similar to Cargo.lock or flake.lock but designed for the Atom ecosystem.
//!
//! ## Overview
//!
//! The lockfile format uses TOML with tagged enums for type safety while maintaining
//! portability across different tools and languages. Each dependency is represented
//! as a tagged union that can represent different types of dependencies:
//!
//! - **Atom dependencies** - References to other atoms by ID and version
//! - **Direct pins** - Direct references to external URLs with integrity verification
//! - **Git pins** - References to specific Git repositories and commits
//! - **Tarball pins** - References to tarball/zip archives
//! - **Cross-atom references** - Dependencies sourced from other atoms
//!
//! ## Key Types
//!
//! - [`Lockfile`] - The root structure containing all resolved dependencies
//! - [`Dep`] - Enum representing different types of dependencies
//! - [`Src`] - Enum representing build-time sources
//! - [`ResolutionMode`] - Controls whether to resolve direct or transitive dependencies
//!
//! ## Example Lockfile
//!
//! ```toml
//! version = 1
//!
//! [[deps]]
//! type = "atom"
//! tag = "my-atom"
//! version = "1.0.0"
//! rev = "abc123..."
//!
//! [[deps]]
//! type = "pin"
//! name = "external-lib"
//! url = "https://example.com/lib.tar.gz"
//! hash = "sha256:def456..."
//!
//! [[srcs]]
//! type = "build"
//! name = "registry"
//! url = "https://registry.example.com"
//! hash = "sha256:ghi789..."
//! ```
//!
//! ## Security Features
//!
//! - **Cryptographic verification** using BLAKE3 hashes for atom content
//! - **Nix-compatible hashing** for tarballs and archives
//! - **Strict field validation** with `#[serde(deny_unknown_fields)]`
//! - **Type-safe dependency resolution** preventing invalid configurations

use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(feature = "git")]
use gix::ObjectId;
use nix_compat::nixhash::NixHash;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[cfg(test)]
mod test;

use crate::Manifest;
use crate::id::AtomTag;

/// A wrapper around NixHash to provide custom serialization behavior.
#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize)]
pub struct WrappedNixHash(pub NixHash);

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
#[serde(untagged)]
pub enum LockDigest {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
    /// A BLAKE-3 digest.
    #[serde(rename = "id")]
    Blake3(#[serde(with = "serde_base32")] [u8; 32]),
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

#[cfg(feature = "git")]
use crate::AtomId;
use crate::id::Name;
#[cfg(feature = "git")]
use crate::store::git::Root;
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
///
/// This struct captures all the information needed to uniquely identify and
/// fetch a specific version of an atom from a Git repository.
#[serde(deny_unknown_fields)]
pub struct AtomDep {
    /// The name corresponding to the atom in the manifest at `deps.atoms.<name>`, if diffferent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Name>,
    /// than the tag The unique identifier of the atom.
    pub tag: AtomTag,
    /// The semantic version of the atom.
    pub version: Version,
    /// The location of the atom, whether local or remote.
    ///
    /// This field is flattened in the TOML serialization and omitted if None.
    #[serde(flatten)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<AtomLocation>,
    /// The resolved Git revision (commit hash) for verification.
    pub rev: LockDigest,
    /// than cryptographic identity of the atom.
    pub id: LockDigest,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used for dependencies that are pinned to specific URLs
/// with integrity verification through cryptographic hashes.
#[serde(deny_unknown_fields)]
pub struct PinDep {
    /// The name of the pinned source.
    pub name: Name,
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
    pub name: Name,
    /// The Git repository URL.
    pub url: Url,
    /// The resolved revision (commit hash).
    pub rev: LockDigest,
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
    pub name: Name,
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
    pub name: Name,
    /// The atom ID from which to source.
    pub from: AtomTag,
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
    pub name: Name,
    /// The URL to fetch the source.
    pub url: Url,
    /// The hash for verification.
    pub hash: WrappedNixHash,
}

/// A wrapper type holding our dependencies in a BTreeMap for efficient lookup by key for trivial
/// comparison to the manifest and consistent ordering, for consistent and minimal diffs.
#[derive(Debug, PartialEq, Eq)]
pub struct DepMap<Deps>(BTreeMap<Name, Deps>);

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
    /// The mode of dependency resolution.
    ///
    /// Valid values are are:
    /// * shallow: every atom manages only it's own direct dependencies
    /// * deep (not implemented): a minimal set of dependencies is derived from all transitive
    ///   dependencies
    pub mode: ResolutionMode,
    /// The list of locked dependencies (absent or empty if none).
    ///
    /// This field contains all the resolved dependencies with their exact
    /// versions and revisions. It is omitted from serialization if None or empty.
    #[serde(default, skip_serializing_if = "DepMap::is_empty")]
    pub deps: DepMap<Dep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(rename = "shallow")]
    Shallow,
    /// Deep resolution: Recursively lock all transitive dependencies (future).
    ///
    /// In this mode, all dependencies and their dependencies are recursively
    /// resolved and locked, ensuring complete reproducibility but requiring
    /// more time and resources. This feature is planned for future implementation.
    #[serde(rename = "deep")]
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

#[cfg(feature = "git")]
impl From<ObjectId> for LockDigest {
    fn from(id: ObjectId) -> Self {
        match id {
            ObjectId::Sha1(bytes) => LockDigest::Sha1(bytes),
        }
    }
}

use base32::{self};
use serde::Serializer;

mod serde_base32 {
    use super::*;

    pub fn serialize<S>(hash: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = base32::encode(crate::BASE32, hash);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        base32::decode(crate::BASE32, &s)
            .ok_or_else(|| serde::de::Error::custom("Invalid Base32 string"))
            .and_then(|bytes| {
                bytes
                    .try_into()
                    .map_err(|_| serde::de::Error::custom("Expected 32 bytes for BLAKE3 hash"))
            })
    }
}

#[cfg(feature = "git")]
impl From<AtomId<Root>> for LockDigest {
    fn from(value: AtomId<Root>) -> Self {
        use crate::Compute;

        LockDigest::Blake3(*value.compute_hash())
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: 1,
            mode: ResolutionMode::Shallow,
            deps: Default::default(),
        }
    }
}

impl<T> AsRef<BTreeMap<Name, T>> for DepMap<T> {
    fn as_ref(&self) -> &BTreeMap<Name, T> {
        let DepMap(map) = self;
        map
    }
}

impl<T> AsMut<BTreeMap<Name, T>> for DepMap<T> {
    fn as_mut(&mut self) -> &mut BTreeMap<Name, T> {
        let DepMap(map) = self;
        map
    }
}

impl<T: Clone + Serialize> Serialize for DepMap<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // BTreeMap iterates in sorted order automatically.
        let values: Vec<_> = self.as_ref().values().cloned().collect();
        values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DepMap<Dep> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<Dep> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for dep in entries {
            match dep {
                Dep::Atom(atom_dep) => {
                    let key = if let Some(n) = &atom_dep.name {
                        n
                    } else {
                        &atom_dep.tag
                    };
                    map.insert(key.to_owned(), Dep::Atom(atom_dep));
                },
                Dep::Pin(pin_dep) => {
                    map.insert(pin_dep.name.to_owned(), Dep::Pin(pin_dep));
                },
                Dep::PinGit(pin_git_dep) => {
                    map.insert(pin_git_dep.name.to_owned(), Dep::PinGit(pin_git_dep));
                },
                Dep::PinTar(pin_tar_dep) => {
                    map.insert(pin_tar_dep.name.to_owned(), Dep::PinTar(pin_tar_dep));
                },
                Dep::From(from_dep) => {
                    map.insert(from_dep.name.to_owned(), Dep::From(from_dep));
                },
                Dep::Build(build_dep) => {
                    map.insert(build_dep.name.to_owned(), Dep::Build(build_dep));
                },
            }
        }
        Ok(DepMap(map))
    }
}

impl<T> DepMap<T> {
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }
}

impl<T> Default for DepMap<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl Lockfile {
    /// Retain only those entries which are present in the manifest, maintaining the manifest file
    /// as the single source of truth.
    pub fn sanitize(&mut self, manifest: Manifest) {
        self.deps
            .as_mut()
            .retain(|k, _| manifest.deps.contains_key(k));
    }
}
