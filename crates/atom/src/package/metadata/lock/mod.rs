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
//! - **Atom dependencies** (`atom`) - References to other atoms by label, version, and
//!   cryptographic ID
//! - **Direct Nix dependencies** (`nix`, `nix+git`, `nix+tar`, `nix+build`) - Direct references to
//!   external sources with integrity verification
//!
//! ## Key Types
//!
//! - [`Lockfile`] - The root structure containing all resolved dependencies and sets
//! - [`Dep`] - Enum representing different types of locked dependencies
//! - [`AtomDep`] - Structure for locked atom dependencies with cryptographic verification
//! - [`NixDep`], [`NixGitDep`], [`NixTarDep`], [`BuildSrc`] - Structures for different Nix fetcher
//!   types
//!
//! Note: Some types are marked as `pub(crate)` for internal use within the atom crate.
//!
//! ## Lockfile Structure
//!
//! ```toml
//! version = 1
//!
//! [sets.<root-hash>]
//! tag = "company-atoms"
//! mirrors = ["git@github.com:our-company/atoms", "https://mirror.com/atoms"]
//!
//! [[deps]]
//! type = "atom"
//! label = "auth-service"
//! version = "1.5.2"
//! set = "<root-hash>"
//! rev = "<commit-hash>"
//! id = "<blake3-hash>"
//!
//! [[deps]]
//! type = "nix+git"
//! name = "nixpkgs"
//! url = "https://github.com/NixOS/nixpkgs"
//! rev = "<commit-hash>"
//!
//! [[deps]]
//! type = "nix+tar"
//! name = "master"
//! url = "https://github.com/ekala-project/atom/archive/master.tar.gz"
//! hash = "sha256:..."
//!
//! [[deps]]
//! type = "nix+build"
//! name = "source-archive"
//! url = "https://dist.company.com/my-atom/0.2.0/source.tar.gz"
//! hash = "sha256:..."
//! ```
//!
//! ## Security Features
//!
//! - **Cryptographic identity** using BLAKE3 hashes for atom identification
//! - **Backend-dependent content verification** (currently SHA1 for Git, will migrate to SHA256)
//! - **Nix-compatible hashing** for tarballs and archives with SHA256
//! - **Strict field validation** with `#[serde(deny_unknown_fields)]`
//! - **Type-safe dependency resolution** preventing invalid configurations
//! - **Repository root hash verification** for atom set integrity

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use direct::{NixFetch, NixReq};
use gix::ObjectId;
use gix::protocol::transport::client::Transport;
use id::{AtomDigest, Label, Name, Tag};
use manifest::{AtomReq, SetMirror, direct};
use nix_compat::nixhash::NixHash;
use package::sets::ResolvedAtom;
use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use storage::git::Root;
use storage::{QueryStore, QueryVersion, UnpackedRef};
use uri::{Uri, VERSION_PLACEHOLDER};
use url::Url;

use super::{GitDigest, manifest};
use crate::{AtomId, BoxError, Compute, Origin, id, package, storage, uri};

//================================================================================================
// Types
//================================================================================================

/// Represents a locked atom dependency, referencing a verifiable repository slice.
///
/// This struct captures all the information needed to uniquely identify and
/// fetch a specific version of an atom from a Git repository.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct AtomDep {
    /// The unique identifier of the atom.
    label: Label,
    /// The semantic version of the atom.
    version: Version,
    /// The location of the atom, whether local or remote.
    set: GitDigest,
    /// The resolved Git revision (commit hash) for verification. If it is `None`, it applies a
    /// local only dependency which must be looked up by path. Atom's without revsions for all
    /// other atoms in their lock cannot themsevles be published.
    #[serde(skip_serializing_if = "Option::is_none")]
    rev: Option<GitDigest>,
    /// The the primary url the atom was first resolved from. Needed for legacy tools which can't
    /// resolve mirrors (e.g. nix).
    #[serde(
        default,
        serialize_with = "manifest::maybe_serialize_url",
        deserialize_with = "manifest::maybe_deserialize_url",
        skip_serializing_if = "Option::is_none"
    )]
    mirror: Option<gix::Url>,
    /// The cryptographic identity of the atom.
    id: AtomDigest,
}

/// Represents a locked build-time source, such as a registry or configuration.
///
/// This struct is used for sources that are fetched during the build process,
/// such as package registries or configuration files that need to be available
/// at build time.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct BuildSrc {
    /// The name of the source.
    pub name: Name,
    /// The URL to fetch the source.
    pub url: Url,
    /// The hash for verification.
    hash: WrappedNixHash,
}

/// Enum representing the different types of locked dependencies, serialized as tagged TOML tables.
///
/// This enum provides a type-safe way to represent different kinds of dependencies
/// in the lockfile, ensuring that each dependency type has the correct fields
/// and validation at compile time.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(tag = "type")]
pub(crate) enum Dep {
    /// An atom dependency variant.
    ///
    /// Represents a dependency on another atom, identified by its ID, version,
    /// and Git revision.
    #[serde(rename = "atom")]
    Atom(AtomDep),
    /// A direct reference to an external source variant.
    ///
    /// Represents a dependency pinned to a specific URL with integrity verification.
    /// Used for dependencies that are not atoms but need to be fetched from external sources.
    #[serde(rename = "nix")]
    Nix(NixDep),
    /// A Git-specific nix variant.
    ///
    /// Represents a dependency pinned to a specific Git repository and commit.
    /// Similar to Pin but specifically for Git repositories.
    #[serde(rename = "nix+git")]
    NixGit(NixGitDep),
    /// A tarball nix variant.
    ///
    /// Represents a dependency pinned to a tarball or archive file.
    /// Used for dependencies distributed as compressed archives.
    #[serde(rename = "nix+tar")]
    NixTar(NixTarDep),
    /// A reference to a build source.
    ///
    /// Represents a source that needs to be fetched and available during the
    /// build process, such as source code or configuration file.
    #[serde(rename = "nix+build")]
    NixSrc(BuildSrc),
}

type DepKey<R> = either::Either<AtomId<R>, Name>;

/// A wrapper for `BTreeMap` that ensures consistent ordering for serialization
/// and minimal diffs in the lockfile. It maps dependency names to their locked
/// representations.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DepMap<R, Deps: Ord>(BTreeMap<DepKey<R>, Deps>);

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// The set of locked mirrors from the manifest
pub struct SetDetails {
    pub(crate) tag: Tag,
    pub(crate) mirrors: BTreeSet<SetMirror>,
}

/// The root structure for the lockfile, containing resolved dependencies and sources.
///
/// This struct represents the complete lockfile format used by atom to capture
/// the exact versions and revisions of all dependencies for reproducible builds.
/// The lockfile ensures that builds are deterministic and can be reproduced
/// across different environments.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// The version of the lockfile schema.
    ///
    /// This field allows for future evolution of the lockfile format while
    /// maintaining backward compatibility.
    pub version: u8,

    pub(crate) sets: BTreeMap<GitDigest, SetDetails>,
    /// The list of locked dependencies (absent or empty if none).
    ///
    /// This field contains all the resolved dependencies with their exact
    /// versions and revisions. It is omitted from serialization if None or empty.
    #[serde(default, skip_serializing_if = "DepMap::is_empty")]
    pub(crate) deps: DepMap<Root, Dep>,
}

/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used for dependencies that are pinned to specific URLs
/// with integrity verification through cryptographic hashes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct NixDep {
    /// The name of the pinned source.
    name: Name,
    /// The URL of the source.
    url: Url,
    /// The hash for integrity verification (e.g., sha256).
    hash: WrappedNixHash,
}

/// Represents a pinned Git repository with a specific revision.
///
/// This struct is used for dependencies that are pinned to specific Git
/// repositories and commits, providing both URL and revision information.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct NixGitDep {
    /// The name of the pinned Git source.
    pub name: Name,
    /// The Git repository URL.
    #[serde(
        serialize_with = "manifest::serialize_url",
        deserialize_with = "manifest::deserialize_url"
    )]
    pub url: gix::Url,
    /// The version which was resolved (if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    /// The resolved revision (commit hash).
    pub rev: GitDigest,
}

/// Represents a pinned tarball or archive source.
///
/// This struct is used for dependencies that are distributed as tarballs
/// or archives, with integrity verification through cryptographic hashes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct NixTarDep {
    /// The name of the tar source.
    pub name: Name,
    /// The URL to the tarball.
    pub url: Url,
    /// The hash of the tarball.
    hash: WrappedNixHash,
}

/// A wrapper around `NixHash` to provide custom serialization behavior for TOML.
#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize, Ord)]
pub(crate) struct WrappedNixHash(pub NixHash);

#[derive(thiserror::Error, Debug)]
pub(in crate::package) enum LockError {
    #[error(transparent)]
    Generic(#[from] BoxError),
    #[error("failed to resolve requested version")]
    Resolve,
}

/// An enum to handle different URL types for filename extraction.
pub(in crate::package) enum NixUrls<'a> {
    Url(&'a Url),
    Git(&'a gix::Url),
}

//================================================================================================
// Impls
//================================================================================================

impl AtomDep {
    pub(crate) fn version(&self) -> &Version {
        &self.version
    }

    pub(crate) fn label(&self) -> &Label {
        &self.label
    }

    pub(crate) fn set(&self) -> GitDigest {
        self.set
    }
}

impl Uri {
    /// Resolves an `Uri` to a fully specified `AtomDep` by querying the
    /// remote Git repository to find the highest matching version and its
    /// corresponding commit hash.
    ///
    /// # Returns
    ///
    /// A `Result` containing the resolved `AtomDep` or a `git::Error` if
    /// resolution fails.
    pub(crate) fn resolve(
        &self,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<(AtomReq, AtomDep), crate::storage::git::Error> {
        let url = self.url();
        let label = self.label();
        if url.is_some_and(|u| u.scheme != gix::url::Scheme::File) {
            let url = url.unwrap();
            let atoms = url.get_atoms(transport)?;
            let ObjectId::Sha1(root) = *atoms.calculate_origin()?;
            let (version, oid) =
                <gix::url::Url as QueryVersion<_, _, _, _, _>>::process_highest_match(
                    atoms.clone(),
                    label,
                    &self.version_req(),
                )
                .ok_or(crate::storage::git::Error::NoMatchingVersion)?;
            let atom_req = if let Some(req) = self.version() {
                AtomReq::new(req.to_owned())
            } else {
                let v = VersionReq::parse(version.to_string().as_str())?;
                AtomReq::new(v)
            };
            let id = AtomId::construct(&atoms, label.to_owned())?;
            Ok((atom_req, AtomDep {
                label: label.to_owned(),
                version,
                mirror: Some(url.to_owned()),
                set: GitDigest::Sha1(root),
                rev: match oid {
                    ObjectId::Sha1(bytes) => Some(GitDigest::Sha1(bytes)),
                },
                id: id.into(),
            }))
        } else {
            // implement path resolution for atoms
            todo!()
        }
    }

    fn _atom_req(&self) -> AtomReq {
        AtomReq::new(self.version_req())
    }

    fn version_req(&self) -> VersionReq {
        self.version()
            .map(ToOwned::to_owned)
            .unwrap_or(VersionReq::STAR)
    }

    fn _get_transport(&self) -> Option<Box<dyn Transport + Send>> {
        self.url().and_then(|u| u.get_transport().ok())
    }
}

impl From<&AtomDep> for AtomId<Root> {
    fn from(dep: &AtomDep) -> Self {
        let root = Root::from(dep.set());
        // unwrap is safe, as calculate_origin will always suceed for src of type Root
        let id = AtomId::construct(&root, dep.label().to_owned()).unwrap();
        id
    }
}

impl<R, T: Serialize + Ord> Serialize for DepMap<R, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // BTreeMap iterates in sorted order automatically.
        let values: Vec<_> = self.as_ref().values().collect();
        values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DepMap<Root, Dep> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use either::Either;

        let entries: Vec<Dep> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for dep in entries {
            match dep {
                Dep::Atom(dep) => {
                    let key = Either::Left(AtomId::from(&dep));
                    map.insert(key, Dep::Atom(dep));
                },
                Dep::Nix(dep) => {
                    map.insert(Either::Right(dep.name().to_owned()), Dep::Nix(dep));
                },
                Dep::NixGit(dep) => {
                    map.insert(Either::Right(dep.name.to_owned()), Dep::NixGit(dep));
                },
                Dep::NixTar(dep) => {
                    map.insert(Either::Right(dep.name.to_owned()), Dep::NixTar(dep));
                },
                Dep::NixSrc(dep) => {
                    map.insert(Either::Right(dep.name.to_owned()), Dep::NixSrc(dep));
                },
            }
        }
        Ok(DepMap(map))
    }
}

impl<R, T: Ord> AsMut<BTreeMap<DepKey<R>, T>> for DepMap<R, T> {
    fn as_mut(&mut self) -> &mut BTreeMap<DepKey<R>, T> {
        let DepMap(map) = self;
        map
    }
}

impl<R, T: Ord> AsRef<BTreeMap<DepKey<R>, T>> for DepMap<R, T> {
    fn as_ref(&self) -> &BTreeMap<DepKey<R>, T> {
        let DepMap(map) = self;
        map
    }
}

impl From<ResolvedAtom<Option<ObjectId>, Root>> for AtomDep {
    fn from(atom: ResolvedAtom<Option<ObjectId>, Root>) -> Self {
        let UnpackedRef { id, version, rev } = atom.unpack();
        AtomDep {
            label: id.label().to_owned(),
            version: version.to_owned(),
            rev: rev.map(GitDigest::from),
            set: GitDigest::from(id.root().deref().to_owned()),
            id: id.compute_hash(),
            mirror: atom.remotes().first().map(ToOwned::to_owned),
        }
    }
}
impl From<ResolvedAtom<ObjectId, Root>> for AtomDep {
    fn from(value: ResolvedAtom<ObjectId, Root>) -> Self {
        let ResolvedAtom {
            unpacked: UnpackedRef { id, version, rev },
            remotes,
        } = value;
        AtomDep::from(ResolvedAtom::new(
            UnpackedRef::new(id, version, Some(rev)),
            remotes,
        ))
    }
}

impl Deref for AtomDep {
    type Target = Label;

    fn deref(&self) -> &Self::Target {
        &self.label
    }
}

impl AsRef<AtomDigest> for AtomDep {
    fn as_ref(&self) -> &AtomDigest {
        &self.id
    }
}

impl<R, T: Ord> Default for DepMap<R, T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<R, T: Ord> DepMap<R, T> {
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }
}

impl NixFetch {
    pub(crate) fn new_from_version(&self, version: &Version) -> Self {
        let replace = |s: &str| s.replace(VERSION_PLACEHOLDER, version.to_string().as_ref());

        let mut clone = self.to_owned();

        match &mut clone.kind {
            NixReq::Tar(url) => {
                let new = replace(url.path());
                url.set_path(new.as_ref());
            },
            NixReq::Url(url) => {
                let new = replace(url.path());
                url.set_path(new.as_ref());
            },
            NixReq::Build(dep) => {
                let new = replace(dep.build.path());
                dep.build.set_path(new.as_ref());
            },
            NixReq::Git(dep) => {
                let new = replace(dep.git.path.to_string().as_ref());
                dep.git.path = new.into();
            },
        };

        clone
    }

    pub(in crate::package) fn get_url(&self) -> NixUrls<'_> {
        match &self.kind {
            NixReq::Tar(url) => NixUrls::Url(url),
            NixReq::Url(url) => NixUrls::Url(url),
            NixReq::Build(nix_src) => NixUrls::Url(&nix_src.build),
            NixReq::Git(nix_git) => NixUrls::Git(&nix_git.git),
        }
    }
}

impl From<ObjectId> for GitDigest {
    fn from(id: ObjectId) -> Self {
        match id {
            ObjectId::Sha1(bytes) => GitDigest::Sha1(bytes),
        }
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: 1,
            sets: Default::default(),
            deps: Default::default(),
        }
    }
}

impl NixDep {
    pub(crate) fn new(name: Name, url: Url, hash: WrappedNixHash) -> Self {
        Self { name, url, hash }
    }

    pub(crate) fn name(&self) -> &Name {
        &self.name
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
}

impl NixGitDep {
    pub(crate) fn name(&self) -> &Name {
        &self.name
    }

    pub(crate) fn url(&self) -> &gix::Url {
        &self.url
    }
}

impl NixTarDep {
    pub(in crate::package) fn new(name: Name, url: Url, hash: WrappedNixHash) -> Self {
        Self { name, url, hash }
    }

    pub(crate) fn name(&self) -> &Name {
        &self.name
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
}

impl BuildSrc {
    pub(in crate::package) fn new(name: Name, url: Url, hash: WrappedNixHash) -> Self {
        Self { name, url, hash }
    }

    pub(crate) fn name(&self) -> &Name {
        &self.name
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
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

//================================================================================================
// Tests
//================================================================================================

#[cfg(test)]
mod test;
