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
use std::sync::Arc;

use bstr::ByteSlice;
use gix::ObjectId;
use gix::protocol::handshake::Ref;
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
    AtomReq, Dependency, DirectPin, GitPin, deserialize_url, not, serialize_url,
};
use crate::store::QueryVersion;
use crate::store::git::Root;
use crate::{AtomId, Manifest};

mod serde_base32;

/// A type alias for a boxed error that is sendable and syncable.
pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(thiserror::Error, Debug)]
enum LockError {
    #[error(transparent)]
    Generic(#[from] BoxError),
    #[error("failed to resolve requested version")]
    Resolve,
}

/// A type alias for the fetcher used for pinned dependencies.
type PinFetcher = Fetcher<
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Arc<dyn PathInfoService>,
    SimpleRenderer<Arc<dyn BlobService>, Arc<dyn DirectoryService>>,
>;

/// Represents a locked atom dependency, referencing a verifiable repository slice.
///
/// This struct captures all the information needed to uniquely identify and
/// fetch a specific version of an atom from a Git repository.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct AtomDep {
    /// The unique identifier of the atom.
    pub tag: AtomTag,
    /// The name corresponding to the atom in the manifest at `deps.atoms.<name>`, if different
    /// than the tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Name>,
    /// The semantic version of the atom.
    pub version: Version,
    /// The location of the atom, whether local or remote.
    ///
    /// This field is flattened in the TOML serialization and omitted if None.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    pub source: gix::Url,
    /// The resolved Git revision (commit hash) for verification.
    pub rev: GitDigest,
    /// The cryptographic identity of the atom.
    id: AtomDigest,
}

/// Represents a locked build-time source, such as a registry or configuration.
///
/// This struct is used for sources that are fetched during the build process,
/// such as package registries or configuration files that need to be available
/// at build time.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct BuildSrc {
    /// The name of the source.
    pub name: Name,
    /// The URL to fetch the source.
    pub url: Url,
    /// The hash for verification.
    hash: WrappedNixHash,
}

/// Represents a cross-atom source reference, acquiring a dependency from another atom.
///
/// This struct enables atoms to reference dependencies from other atoms,
/// creating a composition mechanism for building complex systems from simpler parts.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct FromDep {
    /// The name of the sourced dependency.
    pub name: Name,
    /// The atom ID from which to source.
    from: AtomDigest,
    /// The name of the dependency to acquire from the 'from' atom (defaults to `name`).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    set: Option<Name>,
    /// The path to import inside the tarball.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import: Option<PathBuf>,
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
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct PinDep {
    /// The name of the pinned source.
    pub name: Name,
    /// The URL of the source.
    pub url: Url,
    /// The hash for integrity verification (e.g., sha256).
    hash: WrappedNixHash,
    /// Whether the file imported represents a nix flake.
    #[serde(default, skip_serializing_if = "not")]
    flake: bool,
}

/// Represents a pinned Git repository with a specific revision.
///
/// This struct is used for dependencies that are pinned to specific Git
/// repositories and commits, providing both URL and revision information.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct PinGitDep {
    /// The name of the pinned Git source.
    pub name: Name,
    /// The Git repository URL.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    pub url: gix::Url,
    /// The resolved revision (commit hash).
    pub rev: GitDigest,
    /// The path to import inside the repo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import: Option<PathBuf>,
    /// Whether the file imported represents a nix flake.
    #[serde(default, skip_serializing_if = "not")]
    pub flake: bool,
}

/// Represents a pinned tarball or archive source.
///
/// This struct is used for dependencies that are distributed as tarballs
/// or archives, with integrity verification through cryptographic hashes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct PinTarDep {
    /// The name of the tar source.
    pub name: Name,
    /// The URL to the tarball.
    pub url: Url,
    /// The hash of the tarball.
    hash: WrappedNixHash,
    /// The path to import inside the tarball.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import: Option<PathBuf>,
    /// Whether the file imported represents a nix flake.
    #[serde(default, skip_serializing_if = "not")]
    pub flake: bool,
}

/// Represents the BLAKE-3 digest of an atom's identity.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
struct AtomDigest(#[serde(with = "serde_base32")] [u8; 32]);

/// A wrapper for `BTreeMap` that ensures consistent ordering for serialization
/// and minimal diffs in the lockfile. It maps dependency names to their locked
/// representations.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DepMap<Deps>(BTreeMap<Name, Deps>);

/// A wrapper around `NixHash` to provide custom serialization behavior for TOML.
#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Serialize)]
pub(crate) struct WrappedNixHash(pub NixHash);

/// Represents the location of an atom, either as a URL or a relative path.
///
/// This enum is used to specify where an atom can be found, supporting both
/// remote Git repositories and local relative paths within a repository.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
pub enum AtomLocation {
    /// A URL pointing to a Git repository containing the atom.
    ///
    /// When this variant is used, the atom will be fetched from the specified
    /// Git repository URL. If not provided, defaults to the current repository.
    #[serde(
        rename = "url",
        serialize_with = "serialize_url",
        deserialize_with = "deserialize_url"
    )]
    Url(gix::url::Url),
    /// A relative path within the repository where the atom is located.
    ///
    /// When this variant is used, the atom is located at the specified path
    /// relative to the current atom. If not provided, defaults to the root.
    #[serde(rename = "path")]
    Path(PathBuf),
}

/// Enum representing the different types of locked dependencies, serialized as tagged TOML tables.
///
/// This enum provides a type-safe way to represent different kinds of dependencies
/// in the lockfile, ensuring that each dependency type has the correct fields
/// and validation at compile time.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
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
    /// Represents a pin dependency that is sourced from another atom, enabling
    /// composition of complex systems from simpler atom components.
    #[serde(rename = "pin+from")]
    From(FromDep),
    /// A reference to a build source.
    ///
    /// Represents a source that needs to be fetched and available during the
    /// build process, such as source code or configuration file.
    #[serde(rename = "build")]
    Build(BuildSrc),
}

/// Represents different types of Git commit hashes.
///
/// This enum supports both SHA-1 and SHA-256 hashes, which are serialized
/// as untagged values in TOML for maximum compatibility.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Eq)]
#[serde(untagged)]
pub enum GitDigest {
    /// A SHA-1 commit hash.
    #[serde(rename = "sha1")]
    Sha1(#[serde(with = "hex")] [u8; 20]),
    /// A SHA-256 commit hash.
    #[serde(rename = "sha256")]
    Sha256(#[serde(with = "hex")] [u8; 32]),
}

/// The resolution mode for generating the lockfile.
///
/// This enum controls how dependencies are resolved when generating a lockfile,
/// determining whether to lock only direct dependencies or recursively resolve
/// all transitive dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// An enum to handle different URL types for filename extraction.
enum Urls<'a> {
    Url(&'a Url),
    Git(&'a gix::Url),
}

impl AtomReq {
    /// Resolves an `AtomReq` to a fully specified `AtomDep` by querying the
    /// remote Git repository to find the highest matching version and its
    /// corresponding commit hash.
    ///
    /// # Arguments
    ///
    /// * `key` - The name of the dependency as specified in the manifest, which may differ from the
    ///   atom's tag.
    ///
    /// # Returns
    ///
    /// A `Result` containing the resolved `AtomDep` or a `git::Error` if
    /// resolution fails.
    pub(crate) fn resolve(&self, key: &Name) -> Result<AtomDep, crate::store::git::Error> {
        let url = self.store();

        let atoms = url.get_atoms(None)?;
        let tag = if let Some(tag) = self.tag() {
            tag.to_owned()
        } else {
            // TODO: see if we can find a way to avoid incorrectly encoding the wrong tag here if
            // the wrong key is passed. Perhaps a non-serialized field which unconditionally stores
            // the `AtomId`, to remain unambiguous?
            key.to_owned()
        };
        let (version, oid) = <gix::url::Url as QueryVersion<_, _, _, _>>::process_highest_match(
            atoms.clone(),
            &tag,
            self.version(),
        )
        .ok_or(crate::store::git::Error::NoMatchingVersion)?;
        let name = (key != &tag).then_some(key.to_owned());
        let id = AtomId::construct(&atoms, tag.to_owned())?;

        Ok(AtomDep {
            tag: tag.to_owned(),
            name,
            version,
            source: url.to_owned(),
            rev: match oid {
                ObjectId::Sha1(bytes) => GitDigest::Sha1(bytes),
            },
            id: id.into(),
        })
    }
}

impl<T> AsMut<BTreeMap<Name, T>> for DepMap<T> {
    fn as_mut(&mut self) -> &mut BTreeMap<Name, T> {
        let DepMap(map) = self;
        map
    }
}

impl<T> AsRef<BTreeMap<Name, T>> for DepMap<T> {
    fn as_ref(&self) -> &BTreeMap<Name, T> {
        let DepMap(map) = self;
        map
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

impl<T> Default for DepMap<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> DepMap<T> {
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
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

impl Dependency {
    // Do we need this actually?
    // pub(crate) async fn resolve(&self, key: Option<&Name>) -> Result<Dep, BoxError> {
    //     use crate::manifest::deps::{DirectPin, PinType};
    //     let fetcher = Dependency::get_fetcher();
    //     match self {
    //         Dependency::Atom(atom_req) => Ok(Dep::Atom(atom_req.resolve(key)?)),
    //         Dependency::Pin(pin_req) => match &pin_req.kind {
    //             PinType::Direct(direct_pin) => {
    //                 let (_, dep) = direct_pin.resolve(key, import).await?;
    //                 Ok(dep)
    //             },
    //             PinType::Indirect(indirect_pin) => Ok(Dep::From(FromDep {
    //                 name: key.to_owned(),
    //                 from: indirect_pin.from.to_owned(),
    //                 set: indirect_pin.set.to_owned(),
    //                 import: pin_req.import.to_owned(),
    //             })),
    //         },
    //         Dependency::Src(_src_req) => todo!(),
    //     }
    // }

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
}

impl DirectPin {
    pub(crate) async fn resolve(
        &self,
        key: Option<&Name>,
        import: Option<PathBuf>,
        flake: bool,
    ) -> Result<(Name, Dep), BoxError> {
        use snix_glue::fetchers::Fetch;

        let url = match self {
            DirectPin::Straight(pin) => Urls::Url(&pin.pin),
            DirectPin::Tarball(tar_pin) => Urls::Url(&tar_pin.tar),
            DirectPin::Git(git_pin) => Urls::Git(&git_pin.repo),
        };

        let key = if let Some(key) = key {
            key
        } else {
            &Name::try_from(get_url_filename(&url))?
        };

        let fetch_args = match self {
            DirectPin::Straight(pin) => Fetch::URL {
                url: pin.pin.clone(),
                exp_hash: None,
            },
            DirectPin::Tarball(tar_pin) => Fetch::Tarball {
                url: tar_pin.tar.to_owned(),
                exp_nar_sha256: None,
            },
            DirectPin::Git(git_pin) => {
                return Ok((
                    key.to_owned(),
                    Dep::PinGit(git_pin.resolve(key, import, flake).await?),
                ));
            },
        };

        let fetcher = Dependency::get_fetcher();

        let (_, _, hash, _) = fetcher.await?.ingest_and_persist(key, fetch_args).await?;

        match self {
            DirectPin::Straight(pin) => Ok((
                key.to_owned(),
                Dep::Pin(PinDep {
                    name: key.to_owned(),
                    url: pin.pin.to_owned(),
                    hash: WrappedNixHash(hash),
                    flake,
                }),
            )),
            DirectPin::Tarball(tar_pin) => Ok((
                key.to_owned(),
                Dep::PinTar(PinTarDep {
                    name: key.to_owned(),
                    url: tar_pin.tar.to_owned(),
                    hash: WrappedNixHash(hash),
                    import,
                    flake,
                }),
            )),
            // we have already returned in the previous match
            DirectPin::Git(_) => unreachable!(),
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

impl GitPin {
    async fn resolve(
        &self,
        key: &Name,
        import: Option<PathBuf>,
        flake: bool,
    ) -> Result<PinGitDep, LockError> {
        use crate::manifest::deps::GitStrat;
        use crate::store::QueryStore;

        let r = match &self.fetch {
            GitStrat::Ref(r) => self
                .repo
                .get_ref(format!("{}:{}", r, r).as_str(), None)
                .map_err(|e| LockError::Generic(e.into()))?,
            GitStrat::Version(req) => {
                let query = "refs/tags/v*";
                let refs = self
                    .repo
                    .get_refs([query], None)
                    .map_err(|e| LockError::Generic(e.into()))?;
                if let Some(r) = GitPin::match_version(req, refs) {
                    r
                } else {
                    tracing::error!(message = "could not resolve requested version", %self.repo, version = %req);
                    return Err(LockError::Resolve);
                }
            },
        };

        use gix::ObjectId;
        let ObjectId::Sha1(id) = crate::store::git::to_id(r);

        Ok(PinGitDep {
            name: key.to_owned(),
            url: self.repo.to_owned(),
            rev: GitDigest::Sha1(id),
            import,
            flake,
        })
    }

    fn match_version(req: &VersionReq, refs: impl IntoIterator<Item = Ref>) -> Option<Ref> {
        refs.into_iter()
            .filter_map(|r| {
                let (n, ..) = r.unpack();
                let path = PathBuf::from(n.to_str().ok()?);
                let v_str = &path.file_name()?.to_str()?[1..];
                let version = Version::parse(v_str).ok()?;
                req.matches(&version).then_some((version, r))
            })
            .max_by_key(|(ref version, _)| version.to_owned())
            .map(|(_, r)| r)
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

impl Lockfile {
    /// Removes any dependencies from the lockfile that are no longer present in the
    /// manifest, ensuring the lockfile only contains entries that are still relevant.
    pub(crate) fn sanitize(&mut self, manifest: &Manifest) {
        self.deps
            .as_mut()
            .retain(|k, _| manifest.deps.contains_key(k));
        self.synchronize(manifest);
    }

    /// Updates the lockfile to match the dependencies specified in the manifest.
    /// It resolves any new dependencies, updates existing ones if their version
    /// requirements have changed, and ensures the lockfile is fully consistent.
    pub(crate) fn synchronize(&mut self, manifest: &Manifest) {
        for (k, v) in manifest.deps.iter() {
            if !self.deps.as_ref().contains_key(k) {
                match v {
                    crate::manifest::deps::Dependency::Atom(atom_req) => {
                        if let Ok(dep) = atom_req.resolve(k) {
                            self.deps.as_mut().insert(k.to_owned(), Dep::Atom(dep));
                        } else {
                            tracing::warn!(message = "unlocked dependency could not be resolved", key = %k);
                        };
                    },
                    crate::manifest::deps::Dependency::Src(_) => todo!(),
                    _ => (),
                }
            } else {
                match v {
                    crate::manifest::deps::Dependency::Atom(atom_req) => {
                        let req = atom_req.version();
                        if let Some(Dep::Atom(dep)) = self.deps.as_ref().get(k) {
                            if !req.matches(&dep.version) || &dep.source != atom_req.store() {
                                tracing::warn!(message = "updating out of date dependency in accordance with spec", key = %k);
                                if let Ok(dep) = atom_req.resolve(k) {
                                    self.deps.as_mut().insert(k.to_owned(), Dep::Atom(dep));
                                } else {
                                    tracing::warn!(message = "out of sync dependency could not be resolved, check the version spec", key = %k);
                                };
                            }
                        } else if let Ok(dep) = atom_req.resolve(k) {
                            self.deps.as_mut().insert(k.to_owned(), Dep::Atom(dep));
                        } else {
                            tracing::warn!(message = "dependency is mislabeled as inproper type, and attempts to rectify failed", key = %k);
                        };
                    },
                    crate::manifest::deps::Dependency::Src(_) => todo!(),
                    _ => (),
                }
            }
        }
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

#[cfg(test)]
mod test;
