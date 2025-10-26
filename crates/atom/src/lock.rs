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
use std::path::PathBuf;
use std::sync::Arc;

use bstr::ByteSlice;
use gix::ObjectId;
use gix::protocol::handshake::Ref;
use gix::protocol::transport::client::Transport;
use lazy_regex::{Lazy, Regex, lazy_regex};
use nix_compat::nixhash::NixHash;
use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use snix_castore::blobservice::BlobService;
use snix_castore::directoryservice::DirectoryService;
use snix_glue::fetchers::Fetcher;
use snix_store::nar::SimpleRenderer;
use snix_store::pathinfoservice::PathInfoService;
use url::Url;

use crate::id::{AtomDigest, Label, Name, Tag};
use crate::manifest::SetMirror;
use crate::manifest::deps::{self, AtomReq, GitSpec, NixFetch, NixGit, NixReq};
use crate::manifest::sets::ResolvedAtom;
use crate::store::git::Root;
use crate::store::{QueryStore, QueryVersion, UnpackedRef};
use crate::uri::{Uri, VERSION_PLACEHOLDER};
use crate::{AtomId, Compute, Origin};
//================================================================================================
// Statics
//================================================================================================

static SEMVER_REGEX: Lazy<Regex> = lazy_regex!(
    r#"^v?(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)(?:-(?P<prerelease>(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+(?P<buildmetadata>[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"#
);

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
        serialize_with = "deps::maybe_serialize_url",
        deserialize_with = "deps::maybe_deserialize_url",
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

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Copy, Serialize, Deserialize, Debug, PartialEq, Clone, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum GitDigest {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

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
        serialize_with = "deps::serialize_url",
        deserialize_with = "deps::deserialize_url"
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

/// A type alias for a boxed error that is sendable and syncable.
pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(thiserror::Error, Debug)]
enum LockError {
    #[error(transparent)]
    Generic(#[from] BoxError),
    #[error("failed to resolve requested version")]
    Resolve,
}

/// An enum to handle different URL types for filename extraction.
pub(crate) enum NixUrls<'a> {
    Url(&'a Url),
    Git(&'a gix::Url),
}

/// A type alias for the fetcher used for pinned dependencies.
type NixFetcher = Fetcher<
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Arc<dyn PathInfoService>,
    SimpleRenderer<Arc<dyn BlobService>, Arc<dyn DirectoryService>>,
>;

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
    ) -> Result<(AtomReq, AtomDep), crate::store::git::Error> {
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
                .ok_or(crate::store::git::Error::NoMatchingVersion)?;
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
        AtomDep::from(ResolvedAtom {
            unpacked: UnpackedRef {
                id,
                version,
                rev: Some(rev),
            },
            remotes,
        })
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
    pub(crate) async fn get_fetcher() -> Result<NixFetcher, BoxError> {
        use snix_castore::{blobservice, directoryservice};
        use snix_glue::fetchers::Fetcher;
        use snix_store::nar::SimpleRenderer;
        use snix_store::pathinfoservice;
        let cache_root = config::CONFIG.cache.root_dir.to_owned();

        let blob_service_url = format!("objectstore+file://{}", cache_root.join("blobs").display());
        let dir_service_url = format!("redb://{}", cache_root.join("dirs.redb").display());
        let path_service_url = format!("redb://{}", cache_root.join("paths.redb").display());
        let blob_service = blobservice::from_addr(&blob_service_url).await?;
        let directory_service = directoryservice::from_addr(&dir_service_url).await?;
        let path_info_service = pathinfoservice::from_addr(&path_service_url, None).await?;
        let nar_calculation_service =
            SimpleRenderer::new(blob_service.clone(), directory_service.clone());

        Ok(Fetcher::new(
            blob_service,
            directory_service,
            path_info_service,
            nar_calculation_service,
            Vec::new(),
            Some(cache_root.join("fetcher.redb")),
        ))
    }

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

    pub(crate) fn get_url(&self) -> NixUrls<'_> {
        match &self.kind {
            NixReq::Tar(url) => NixUrls::Url(url),
            NixReq::Url(url) => NixUrls::Url(url),
            NixReq::Build(nix_src) => NixUrls::Url(&nix_src.build),
            NixReq::Git(nix_git) => NixUrls::Git(&nix_git.git),
        }
    }

    pub(crate) async fn resolve(&self, key: Option<&Name>) -> Result<(Name, Dep), BoxError> {
        use snix_glue::fetchers::Fetch;

        let key = if let Some(key) = key {
            key
        } else {
            let url = self.get_url();
            &Name::try_from(get_url_filename(&url))?
        };

        match &self.kind {
            NixReq::Url(url) => {
                let args = Fetch::URL {
                    url: url.to_owned(),
                    exp_hash: None,
                };
                let fetcher = Self::get_fetcher();

                let (_, _, hash, _) = fetcher.await?.ingest_and_persist(key, args).await?;

                Ok((
                    key.to_owned(),
                    Dep::Nix(NixDep {
                        name: key.to_owned(),
                        url: url.to_owned(),
                        hash: WrappedNixHash(hash),
                    }),
                ))
            },
            NixReq::Tar(url) => {
                let args = Fetch::Tarball {
                    url: url.to_owned(),
                    exp_nar_sha256: None,
                };
                let fetcher = Self::get_fetcher();

                let (_, _, hash, _) = fetcher.await?.ingest_and_persist(key, args).await?;
                Ok((
                    key.to_owned(),
                    Dep::NixTar(NixTarDep {
                        name: key.to_owned(),
                        url: url.to_owned(),
                        hash: WrappedNixHash(hash),
                    }),
                ))
            },
            NixReq::Git(nix_git) => {
                return Ok((key.to_owned(), Dep::NixGit(nix_git.resolve(key).await?)));
            },
            NixReq::Build(build_src) => {
                let args = if build_src.unpack {
                    Fetch::Tarball {
                        url: build_src.build.to_owned(),
                        exp_nar_sha256: None,
                    }
                } else {
                    Fetch::URL {
                        url: build_src.build.to_owned(),
                        exp_hash: None,
                    }
                };
                let fetcher = Self::get_fetcher();

                let (_, _, hash, _) = fetcher.await?.ingest_and_persist(key, args).await?;
                Ok((
                    key.to_owned(),
                    Dep::NixSrc(BuildSrc {
                        name: key.to_owned(),
                        url: build_src.build.to_owned(),
                        hash: WrappedNixHash(hash),
                    }),
                ))
            },
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

impl NixGit {
    async fn resolve(&self, key: &Name) -> Result<NixGitDep, LockError> {
        use crate::store::QueryStore;

        let (version, r) = match &self.spec {
            Some(GitSpec::Ref(r)) => (
                None,
                self.git
                    .get_ref(format!("{}:{}", r, r).as_str(), None)
                    .map_err(|e| LockError::Generic(e.into()))?,
            ),
            Some(GitSpec::Version(req)) => {
                let queries = ["refs/tags/*:refs/tags/*"];
                let refs = self
                    .git
                    .get_refs(queries, None)
                    .map_err(|e| LockError::Generic(e.into()))?;
                tracing::trace!(?refs, "returned git refs");
                if let Some((v, r)) = NixGit::match_version(req, refs) {
                    (Some(v), r)
                } else {
                    tracing::error!(message = "could not resolve requested version", %self.git, version = %req);
                    return Err(LockError::Resolve);
                }
            },
            None => {
                let q = "HEAD:HEAD";
                (
                    None,
                    self.git
                        .get_ref(q, None)
                        .map_err(|e| LockError::Generic(e.into()))?,
                )
            },
        };

        use gix::ObjectId;
        let ObjectId::Sha1(id) = crate::store::git::to_id(r);

        Ok(NixGitDep {
            name: key.to_owned(),
            url: self.git.to_owned(),
            rev: GitDigest::Sha1(id),
            version,
        })
    }

    fn match_version(
        req: &VersionReq,
        refs: impl IntoIterator<Item = Ref>,
    ) -> Option<(Version, Ref)> {
        refs.into_iter()
            .filter_map(|r| {
                let (n, ..) = r.unpack();
                let version = extract_and_parse_semver(n.to_str().ok()?)?;
                req.matches(&version).then_some((version, r))
            })
            .max_by_key(|(ref version, _)| version.to_owned())
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
    pub(crate) fn name(&self) -> &Name {
        &self.name
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
}

impl BuildSrc {
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
// Functions
//================================================================================================

fn extract_and_parse_semver(input: &str) -> Option<Version> {
    let re = SEMVER_REGEX.to_owned();
    println!("{}", input);
    let captures = re.captures(input)?;

    // Construct the SemVer string from captured groups
    let version_str = format!(
        "{}.{}.{}{}{}",
        &captures["major"],
        &captures["minor"],
        &captures["patch"],
        captures
            .name("prerelease")
            .map_or(String::new(), |m| format!("-{}", m.as_str())),
        captures
            .name("buildmetadata")
            .map_or(String::new(), |m| format!("+{}", m.as_str()))
    );

    Version::parse(&version_str).ok()
}

pub(crate) fn url_filename_as_tag(url: &gix::Url) -> Result<Tag, crate::id::Error> {
    let str = get_url_filename(&NixUrls::Git(url));
    Tag::try_from(str)
}

/// Extracts a filename from a URL, suitable for use as a dependency name.
fn get_url_filename(url: &NixUrls) -> String {
    match url {
        NixUrls::Url(url) => {
            if url.path() == "/" {
                url.host_str().unwrap_or("source").to_string()
            } else {
                let s = if let Some(mut s) = url.path_segments() {
                    s.next_back()
                        .map(|s| {
                            if let Some((file, _ext)) = s.split_once('.') {
                                file
                            } else {
                                s
                            }
                        })
                        .unwrap_or(url.path())
                } else {
                    url.path()
                };
                s.to_string()
            }
        },
        NixUrls::Git(url) => {
            if url.path_is_root() {
                url.host().unwrap_or("source").to_string()
            } else {
                let path = url.path.to_string();
                let p = PathBuf::from(path.as_str());
                p.file_stem()
                    .and_then(|x| x.to_str().map(ToOwned::to_owned))
                    .unwrap_or(path)
            }
        },
    }
}

//================================================================================================
// Tests
//================================================================================================

#[cfg(test)]
mod test;
