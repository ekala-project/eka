//! # Lockfile Format
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
use std::path::PathBuf;

use direct::{BuildSrc, NixDep, NixGitDep, NixTarDep};
use gix::ObjectId;
use hex::ToHex;
use id::{AtomDigest, Label, Name, Tag};
use manifest::SetMirror;
use package::sets::ResolvedAtom;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use storage::UnpackedRef;
use storage::git::Root;
use uri::serde_gix_url;

use super::{GitDigest, manifest};
use crate::{AtomId, BoxError, Compute, id, package, storage, uri};

pub(in crate::package) mod direct;

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
        with = "serde_gix_url::maybe",
        skip_serializing_if = "Option::is_none"
    )]
    mirror: Option<gix::Url>,
    /// The cryptographic identity of the atom.
    id: AtomDigest,
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
    #[serde(rename = "nix+src")]
    NixSrc(BuildSrc),
}

/// A wrapper for `BTreeMap` that ensures consistent ordering for serialization
/// and minimal diffs in the lockfile. It maps dependency names to their locked
/// representations.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DepMap<R, Deps: Ord>(BTreeMap<DepKey<R>, Deps>);

type DepKey<R> = either::Either<AtomId<R>, Name>;

/// The set of locked mirrors from the manifest
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SetDetails {
    pub(crate) tag: Tag,
    pub(crate) mirrors: BTreeSet<SetMirror>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(tag = "use")]
#[allow(clippy::large_enum_variant)]
pub(super) enum Using {
    /// an atom containing a nix expression that is just evaluated by calling `import`
    #[serde(rename = "nix")]
    NixTrivial { entry: PathBuf },
    /// an atom containing a nix expression that is evaluated with the contained `NixComposer` atom
    #[serde(rename = "atom")]
    Atom {
        #[serde(flatten)]
        atom: AtomDep,
        entry: PathBuf,
    },
    /// an atom that contains only static configuration for use at evaluation time to other atoms
    #[serde(rename = "static")]
    #[default]
    Config,
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

    pub(super) compose: Using,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) sets: BTreeMap<GitDigest, SetDetails>,
    /// The list of locked dependencies (absent or empty if none).
    ///
    /// This field contains all the resolved dependencies with their exact
    /// versions and revisions. It is omitted from serialization if None or empty.
    #[serde(default, skip_serializing_if = "DepMap::is_empty")]
    pub(crate) deps: DepMap<Root, Dep>,
}

#[derive(thiserror::Error, Debug)]
pub(in crate::package) enum LockError {
    #[error(transparent)]
    Generic(#[from] BoxError),
    #[error("failed to resolve requested version")]
    Resolve,
}

//================================================================================================
// Impls
//================================================================================================

impl AtomDep {
    pub(in crate::package) fn new(
        label: Label,
        version: Version,
        set: GitDigest,
        rev: Option<GitDigest>,
        mirror: Option<gix::Url>,
        id: AtomDigest,
    ) -> Self {
        Self {
            label,
            version,
            set,
            rev,
            mirror,
            id,
        }
    }

    pub(crate) fn version(&self) -> &Version {
        &self.version
    }

    pub(crate) fn label(&self) -> &Label {
        &self.label
    }

    pub(crate) fn set(&self) -> GitDigest {
        self.set
    }

    pub(crate) fn rev(&self) -> Option<GitDigest> {
        self.rev
    }
}

impl std::fmt::Display for Dep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dep::Atom(atom_dep) => write!(
                f,
                "{:.8}::{}@{}",
                atom_dep.set(),
                atom_dep.label(),
                atom_dep.version()
            ),
            Dep::Nix(nix_dep) => write!(f, "nix:{}", nix_dep.name()),
            Dep::NixGit(nix_git_dep) => write!(f, "nix:{}", nix_git_dep.name()),
            Dep::NixTar(nix_tar_dep) => write!(f, "nix:{}", nix_tar_dep.name()),
            Dep::NixSrc(build_src) => write!(f, "nix:{}", build_src.name()),
        }
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

impl From<ObjectId> for GitDigest {
    fn from(id: ObjectId) -> Self {
        match id {
            ObjectId::Sha1(bytes) => GitDigest::Sha1(bytes),
        }
    }
}

impl std::fmt::Display for GitDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(max_width) = f.precision() {
            match self {
                GitDigest::Sha1(o) => write!(f, "{:.max_width$}", o.encode_hex::<String>()),
                GitDigest::Sha256(o) => write!(f, "{:.max_width$}", o.encode_hex::<String>()),
            }
        } else {
            match self {
                GitDigest::Sha1(o) => write!(f, "{}", o.encode_hex::<String>()),
                GitDigest::Sha256(o) => write!(f, "{}", o.encode_hex::<String>()),
            }
        }
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: 1,
            sets: Default::default(),
            compose: Using::default(),
            deps: Default::default(),
        }
    }
}

//================================================================================================
// Tests
//================================================================================================

#[cfg(test)]
mod test;
