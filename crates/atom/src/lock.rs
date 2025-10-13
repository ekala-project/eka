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

use std::collections::BTreeSet;
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

use crate::id::{AtomTag, Name};
use crate::manifest::deps::{
    AtomReq, GitSpec, NixFetch, NixGit, NixReq, NixUrl, deserialize_url, serialize_url,
};
use crate::store::git::{AtomQuery, Root};
use crate::store::{QueryStore, QueryVersion};
use crate::uri::Uri;
use crate::{AtomId, Manifest};

mod serde_base32;

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
#[derive(Serialize, Deserialize, Debug, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct AtomDep {
    /// The unique identifier of the atom.
    tag: AtomTag,
    /// The semantic version of the atom.
    version: Version,
    /// The location of the atom, whether local or remote.
    set: GitDigest,
    /// The resolved Git revision (commit hash) for verification.
    rev: GitDigest,
    /// The cryptographic identity of the atom.
    id: AtomDigest,
}

/// Represents a locked build-time source, such as a registry or configuration.
///
/// This struct is used for sources that are fetched during the build process,
/// such as package registries or configuration files that need to be available
/// at build time.
#[derive(Serialize, Deserialize, Debug, Eq, Clone)]
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
    /// A direct pin to an external source variant.
    ///
    /// Represents a dependency pinned to a specific URL with integrity verification.
    /// Used for dependencies that are not atoms but need to be fetched from external sources.
    #[serde(rename = "nix")]
    Nix(NixDep),
    /// A Git-specific pin variant.
    ///
    /// Represents a dependency pinned to a specific Git repository and commit.
    /// Similar to Pin but specifically for Git repositories.
    #[serde(rename = "nix+git")]
    NixGit(NixGitDep),
    /// A tarball pin variant.
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

/// A wrapper for `BTreeMap` that ensures consistent ordering for serialization
/// and minimal diffs in the lockfile. It maps dependency names to their locked
/// representations.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DepMap<Deps: Ord>(BTreeSet<Deps>);

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum GitDigest {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
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
    /// The list of locked dependencies (absent or empty if none).
    ///
    /// This field contains all the resolved dependencies with their exact
    /// versions and revisions. It is omitted from serialization if None or empty.
    #[serde(default, skip_serializing_if = "DepMap::is_empty")]
    pub(crate) deps: DepMap<Dep>,
}

/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used for dependencies that are pinned to specific URLs
/// with integrity verification through cryptographic hashes.
#[derive(Serialize, Deserialize, Debug, Eq, Clone)]
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
#[derive(Serialize, Deserialize, Debug, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct NixGitDep {
    /// The name of the pinned Git source.
    pub name: Name,
    /// The Git repository URL.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
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
#[derive(Serialize, Deserialize, Debug, Eq, Clone)]
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

/// Represents the BLAKE-3 digest of an atom's identity.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq, PartialOrd, Ord)]
pub(crate) struct AtomDigest(#[serde(with = "serde_base32")] [u8; 32]);

impl std::fmt::Display for AtomDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = toml_edit::ser::to_string(&self).map_err(|_| std::fmt::Error)?;
        write!(f, "{}", d)
    }
}

#[derive(thiserror::Error, Debug)]
enum LockError {
    #[error(transparent)]
    Generic(#[from] BoxError),
    #[error("failed to resolve requested version")]
    Resolve,
}

/// An enum to handle different URL types for filename extraction.
enum Urls<'a> {
    Url(&'a Url),
    Git(&'a gix::Url),
}

/// A type alias for the fetcher used for pinned dependencies.
type PinFetcher = Fetcher<
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

    pub(crate) fn tag(&self) -> &AtomTag {
        &self.tag
    }

    pub(crate) fn id(&self) -> &AtomDigest {
        &self.id
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
        let tag = self.tag();
        if url.is_some_and(|u| u.scheme != gix::url::Scheme::File) {
            let url = url.unwrap();
            let atoms = url.get_atoms(transport)?;
            let ObjectId::Sha1(root) =
                <gix::Url as QueryVersion<_, _, _, _>>::process_root(atoms.to_owned())
                    .ok_or(crate::store::git::Error::RootNotFound)?;
            let (version, oid) =
                <gix::url::Url as QueryVersion<_, _, _, _>>::process_highest_match(
                    atoms.clone(),
                    tag,
                    &self.version_req(),
                )
                .ok_or(crate::store::git::Error::NoMatchingVersion)?;
            let atom_req = if let Some(req) = self.version() {
                AtomReq::new(req.to_owned())
            } else {
                let v = VersionReq::parse(version.to_string().as_str())?;
                AtomReq::new(v)
            };
            let id = AtomId::construct(&atoms, tag.to_owned())?;
            Ok((atom_req, AtomDep {
                tag: tag.to_owned(),
                version,
                set: GitDigest::Sha1(root),
                rev: match oid {
                    ObjectId::Sha1(bytes) => GitDigest::Sha1(bytes),
                },
                id: id.into(),
            }))
        } else {
            // implement path resolution for atoms
            todo!()
        }
    }

    fn atom_req(&self) -> AtomReq {
        AtomReq::new(self.version_req())
    }

    fn version_req(&self) -> VersionReq {
        self.version()
            .map(ToOwned::to_owned)
            .unwrap_or(VersionReq::STAR)
    }

    fn get_transport(&self) -> Option<Box<dyn Transport + Send>> {
        self.url().and_then(|u| u.get_transport().ok())
    }
}

impl<T: Ord> AsMut<BTreeSet<T>> for DepMap<T> {
    fn as_mut(&mut self) -> &mut BTreeSet<T> {
        let DepMap(map) = self;
        map
    }
}

impl<T: Ord> AsRef<BTreeSet<T>> for DepMap<T> {
    fn as_ref(&self) -> &BTreeSet<T> {
        let DepMap(map) = self;
        map
    }
}

impl Deref for AtomDep {
    type Target = AtomTag;

    fn deref(&self) -> &Self::Target {
        &self.tag
    }
}

impl AsRef<AtomDigest> for AtomDep {
    fn as_ref(&self) -> &AtomDigest {
        &self.id
    }
}

impl<T: Ord> Default for DepMap<T> {
    fn default() -> Self {
        Self(BTreeSet::new())
    }
}

impl<T: Ord> DepMap<T> {
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }
}

/// We enforce equality of a locked atom only by its cryptographic identity. This ensures that no
/// more than one copy of a unique atom can exist in the lock at any given time. This will be
/// important for sane dependency resolution of transitives in the future, and also makes updating
/// the lock more efficient, since we can just insert an updated atom into the BTreeSet and it will
/// be replaced even if it has a newer version.
impl PartialEq for AtomDep {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for AtomDep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for AtomDep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// We only want nix dependencies to be compared by name, so that there is no possibility of a
/// duplicate in the set, just as every key in the manifest must be unique.
impl PartialEq for NixDep {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for NixDep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NixDep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialEq for NixGitDep {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for NixGitDep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NixGitDep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialEq for NixTarDep {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for NixTarDep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NixTarDep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialEq for BuildSrc {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for BuildSrc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BuildSrc {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl NixReq {
    pub(crate) async fn get_fetcher() -> Result<PinFetcher, BoxError> {
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

    pub(crate) async fn resolve(&self, key: Option<&Name>) -> Result<(Name, Dep), BoxError> {
        use snix_glue::fetchers::Fetch;

        let url = match self {
            NixReq::Fetch(NixFetch {
                url: NixUrl::Tar(url),
                ..
            }) => Urls::Url(url),
            NixReq::Fetch(NixFetch {
                url: NixUrl::Url(url),
                ..
            }) => Urls::Url(url),
            NixReq::Git(nix_git) => Urls::Git(&nix_git.git),
            NixReq::Build(nix_src) => Urls::Url(&nix_src.build),
        };

        let key = if let Some(key) = key {
            key
        } else {
            &Name::try_from(get_url_filename(&url))?
        };

        match self {
            NixReq::Fetch(NixFetch {
                url: NixUrl::Url(url),
                ..
            }) => {
                let args = Fetch::URL {
                    url: url.to_owned(),
                    exp_hash: None,
                };
                let fetcher = NixReq::get_fetcher();

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
            NixReq::Fetch(NixFetch {
                url: NixUrl::Tar(url),
                ..
            }) => {
                let args = Fetch::Tarball {
                    url: url.to_owned(),
                    exp_nar_sha256: None,
                };
                let fetcher = NixReq::get_fetcher();

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
                let fetcher = NixReq::get_fetcher();

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

impl From<AtomId<Root>> for AtomDigest {
    fn from(value: AtomId<Root>) -> Self {
        use crate::Compute;

        Self(*value.compute_hash())
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
                let queries = [
                    "refs/tags/v[0-9]*:refs/tags/*",
                    "refs/tags/[0-9]*:refs/tags/*",
                ];
                let refs = self
                    .git
                    .get_refs(&queries, None)
                    .map_err(|e| LockError::Generic(e.into()))?;
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
            deps: Default::default(),
        }
    }
}

type MirrorResult = Result<
    (
        Option<Box<dyn Transport + Send>>,
        <Vec<AtomQuery> as IntoIterator>::IntoIter,
        Root,
        Name,
        gix::Url,
    ),
    BoxError,
>;

impl Lockfile {
    const OUT_OF_SYNC: &str =
        "out of sync dependency could not be resolved, check the version spec";
    const RESOLUTION_ERR_MSG: &str = "unlocked dependency could not be resolved";

    /// Removes any dependencies from the lockfile that are no longer present in the
    /// manifest, ensuring the lockfile only contains entries that are still relevant,
    /// then calls into synchronization logic to ensure consistency.
    pub(crate) async fn sanitize(&mut self, manifest: &Manifest) {
        self.deps.as_mut().retain(|dep| match dep {
            Dep::Atom(atom_dep) => manifest.deps().from().contains_key(atom_dep.deref()),
            Dep::Nix(nix) => manifest.deps().direct().nix().contains_key(nix.name()),
            Dep::NixGit(nix_git) => manifest.deps().direct().nix().contains_key(&nix_git.name),
            Dep::NixTar(nix_tar) => manifest.deps().direct().nix().contains_key(&nix_tar.name),
            Dep::NixSrc(build_src) => manifest.deps().direct().nix().contains_key(&build_src.name),
        });
        // self.synchronize(manifest).await;
    }

    /// Updates the lockfile to match the dependencies specified in the manifest.
    /// It resolves any new dependencies, updates existing ones if their version
    /// requirements have changed, and ensures the lockfile is fully consistent.
    pub(crate) async fn synchronize(&mut self, manifest: &Manifest) {
        todo!()
    }
    //     for (k, v) in manifest.deps.iter() {
    //         if !self.deps.as_ref().contains_key(k) {
    //             match v {
    //                 Dependency::Atom(atom_req) => {
    //                     if let Ok(dep) = atom_req.resolve(k) {
    //                         self.deps.as_mut().insert(k.to_owned(), Dep::Atom(dep));
    //                     } else {
    //                         tracing::warn!(message = Self::RESOLUTION_ERR_MSG, key = %k, r#type =
    // "atom");                     };
    //                 },
    //                 Dependency::Pin(pin_req) => {
    //                     if let Ok((_, dep)) = DirectPin::Straight(pin_req.to_owned())
    //                         .resolve(Some(k))
    //                         .await
    //                     {
    //                         self.deps.as_mut().insert(k.to_owned(), dep);
    //                     } else {
    //                         tracing::warn!(message = Self::RESOLUTION_ERR_MSG, key = %k, r#type =
    // "pin");                     }
    //                 },
    //                 Dependency::TarPin(tar_req) => {
    //                     if let Ok((_, dep)) = DirectPin::Tarball(tar_req.to_owned())
    //                         .resolve(Some(k))
    //                         .await
    //                     {
    //                         self.deps.as_mut().insert(k.to_owned(), dep);
    //                     } else {
    //                         tracing::warn!(message = Self::RESOLUTION_ERR_MSG, key = %k, r#type =
    // "pin+tar");                     }
    //                 },
    //                 Dependency::GitPin(git_req) => {
    //                     if let Ok(dep) = git_req.resolve(k).await {
    //                         self.deps.as_mut().insert(k.to_owned(), Dep::NixGit(dep));
    //                     } else {
    //                         tracing::warn!(message = Self::RESOLUTION_ERR_MSG, key = %k, r#type =
    // "pin+git");                     }
    //                 },
    //                 Dependency::Src(_) => todo!(),
    //             }
    //         } else {
    //             match v {
    //                 Dependency::Atom(atom_req) => {
    //                     let req = atom_req.version();
    //                     if let Some(Dep::Atom(dep)) = self.deps.as_ref().get(k) {
    //                         if !req.matches(&dep.version) || &dep.set != atom_req.store() {
    //                             tracing::warn!(message = "updating out of date dependency in
    // accordance with spec", key = %k, r#type = "atom");                             if let
    // Ok(dep) = atom_req.resolve(k) {
    // self.deps.as_mut().insert(Dep::Atom(dep));                             } else {
    //                                 tracing::warn!(message = Self::OUT_OF_SYNC, key = %k);
    //                             };
    //                         }
    //                     }
    //                 },
    //                 Dependency::GitPin(git_req) => {
    //                     if let Some(Dep::NixGit(dep)) = self.deps.as_ref().get(k) {
    //                         let fetch = async |git_req: &GitPin, deps: &mut BTreeSet<Dep>| {
    //                             if let Ok(dep) = git_req.resolve(k).await {
    //                                 deps.insert(Dep::NixGit(dep));
    //                             } else {
    //                                 tracing::warn!(
    //                                     message = Self::OUT_OF_SYNC,
    //                                     key = %k
    //                                 );
    //                             }
    //                         };
    //                         if dep.url == git_req.repo {
    //                             use crate::manifest::deps::GitStrat;
    //                             match git_req.fetch.to_owned() {
    //                                 GitStrat::Ref(_) => {
    //                                     // do nothing, we don't want to update locked refs unless
    //                                     // explicitly requested
    //                                 },
    //                                 GitStrat::Version(version_req) => {
    //                                     if dep
    //                                         .version
    //                                         .to_owned()
    //                                         .is_none_or(|v| !version_req.matches(&v))
    //                                     {
    //                                         fetch(&git_req, self.deps.as_mut()).await;
    //                                     }
    //                                 },
    //                             }
    //                         } else {
    //                             fetch(&git_req, self.deps.as_mut()).await;
    //                         }
    //                     }
    //                 },
    //                 Dependency::Src(_) => todo!(),
    //                 _ => (),
    //             }
    //         }
    //     }
    // }
}

impl NixDep {
    pub(crate) fn name(&self) -> &Name {
        &self.name
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

/// Extracts a filename from a URL, suitable for use as a dependency name.
fn get_url_filename(url: &Urls) -> String {
    match url {
        Urls::Url(url) => {
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
        Urls::Git(url) => {
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
