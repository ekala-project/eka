//! # Atom Dependency Handling
//!
//! This module provides the core types for working with an Atom manifest's dependencies.
//! It defines the structure for specifying different types of dependencies in an atom's
//! manifest file, including atom references, direct pins, and build-time sources.
//!
//! ## Dependency Types
//!
//! The manifest supports three main categories of dependencies:
//!
//! - **Atom dependencies** - References to other atoms by ID and version.
//! - **Pin dependencies** - Direct references to external sources (URLs, Git repos, tarballs).
//! - **Source dependencies** - Build-time dependencies like source code or config files.
//!
//! ## Key Types
//!
//! - [`Dependency`] - The main dependency structure containing all dependency types.
//! - [`AtomReq`] - Requirements for atom dependencies.
//! - [`SrcReq`] - Requirements for build-time sources.
//!
//! ## Example Usage
//!
//! ```toml
//! [deps.atoms]
//! # Reference to another atom
//! other-atom = { version = "^1.0.0", path = "../other-atom" }
//!
//! [deps.pins]
//! # pin to external evaluation time source code
//! external-lib = { url = "https://example.com/lib.tar.gz" }
//!
//! # Git pin
//! git-dep = { url = "https://github.com/user/repo.git", ref = "main" }
//!
//! # Indirect pin (from another atom)
//! shared-lib = { from = "other-atom", get = "lib" }
//!
//! [deps.srcs]
//! # Build-time source
//! src-code = { url = "https://registry.example.com/code.tar.gz" }
//! ```
//!
//! ## Validation
//!
//! All dependency types use `#[serde(deny_unknown_fields)]` to ensure strict
//! validation and prevent typos in manifest files. Optional fields are properly
//! handled with `skip_serializing_if` to keep the TOML output clean.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use bstr::{BString, ByteSlice};
use either::Either;
use gix::{Repository, ThreadSafeRepository};
use semver::{Prerelease, VersionReq};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use toml_edit::DocumentMut;
use url::Url;

use crate::id::{Label, Name, Tag, VerifiedName};
use crate::lock::{AtomDep, Dep, GitDigest, SetDetails};
use crate::manifest::sets::{self, ResolvedSets, SetResolver};
use crate::manifest::{AtomError, SetMirror};
use crate::store::UnpackedRef;
use crate::store::git::Root;
use crate::uri::{AliasedUrl, Uri};
use crate::{ATOM_MANIFEST_NAME, AtomId, Lockfile, Manifest, Origin, lock};

//================================================================================================
// Types
//================================================================================================

type AtomFrom = HashMap<Tag, HashMap<Label, VersionReq>>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
pub struct AtomReq {
    /// The semantic version requirement for the atom (e.g., "^1.0.0").
    version: VersionReq,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(deny_unknown_fields)]
/// The dependencies specified in the manifest
pub struct Dependency {
    /// Specify atom dependencies from a specific set outlined in `[package.sets]`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    from: AtomFrom,
    /// Direct dependencies not in the atom format.
    #[serde(default, skip_serializing_if = "DirectDeps::is_empty")]
    direct: DirectDeps,
}

#[derive(thiserror::Error, Debug)]
/// Errors that can occur when working with a `TypedDocument`.
pub enum DocError {
    /// Missing atom from manifest
    #[error("the atom directory disappeared or is inaccessible: {0}")]
    Missing(PathBuf),
    /// The manifest path could not be accessed.
    #[error("the ekala.toml could not be located")]
    MissingEkala,
    /// A valid atom id could not be constructed.
    #[error("a valid atom id could not be constructed; aborting: {0}")]
    AtomIdConstruct(String),
    /// Duplicate atoms were found in the ekala manifest
    #[error("there is more than one atom with the same label in the set")]
    DuplicateAtoms,
    /// A local atom by the requested label doesn't exist
    #[error("a local atom by the requested label doesn't exist, or isn't specified")]
    NoLocal,
    /// A TOML deserialization error occurred.
    #[error(transparent)]
    De(#[from] toml_edit::de::Error),
    /// A TOML serialization error occurred.
    #[error(transparent)]
    Ser(#[from] toml_edit::TomlError),
    /// A filesystem error occurred.
    #[error(transparent)]
    Read(#[from] std::io::Error),
    /// A manifest serialization error occurred.
    #[error(transparent)]
    Manifest(#[from] toml_edit::ser::Error),
    /// An error occurred while writing to a temporary file.
    #[error(transparent)]
    Write(#[from] tempfile::PersistError),
    /// A Git resolution error occurred.
    #[error(transparent)]
    Git(#[from] Box<crate::store::git::Error>),
    /// A semantic versioning error occurred.
    #[error(transparent)]
    Semver(#[from] semver::Error),
    /// A UTF-8 conversion error occurred.
    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),
    /// A URL parsing error occurred.
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// A generic error occurred.
    #[error(transparent)]
    Error(#[from] crate::lock::BoxError),
    /// A invalid refname was passed.
    #[error(transparent)]
    BadLabel(#[from] crate::id::Error),
    /// A set error has occurred.
    #[error(transparent)]
    SetError(#[from] super::sets::Error),
}

/// Represents the manner in which we resolve a rev for this git fetch
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum GitSpec {
    /// We will resolve the rev of the given ref.
    #[serde(rename = "ref")]
    Ref(String),
    /// We will resolve a version from the available tags resembling a semantic version.
    #[serde(rename = "version")]
    Version(VersionReq),
}

/// A writer for `atom.toml` manifests that ensures the `atom.lock` file is kept in sync.
///
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
///
/// use atom::id::Name;
/// use atom::manifest::deps::ManifestWriter;
/// use atom::uri::Uri;
///
/// async {
///     let mut writer = ManifestWriter::new(None, Path::new("/path/to/atom.toml"))
///         .await
///         .unwrap();
///     let uri = "my-atom@^1.0.0".parse::<Uri>().unwrap();
///     let key = "my-atom".parse::<Name>().unwrap();
///     writer.add_uri(uri, Some(key)).unwrap();
///     writer.write_atomic().unwrap();
/// };
/// ```
pub struct ManifestWriter {
    path: PathBuf,
    doc: TypedDocument<Manifest>,
    lock: Lockfile,
    resolved: ResolvedSets,
}

/// Represents the underlying type of Nix dependency
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
pub enum NixReq {
    /// A tarball url which will be unpacked before being hashed
    #[serde(rename = "tar")]
    Tar(Url),
    /// A straight url which will be fetched and hashed directly
    #[serde(rename = "url")]
    Url(Url),
    /// A fetch which will be deferred to buildtime
    #[serde(untagged)]
    Build(NixSrc),
    /// A fetch which leverages git
    #[serde(untagged)]
    Git(NixGit),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a nix fetch, either direct or tarball.
pub struct NixFetch {
    /// The URL of the resource.
    #[serde(flatten)]
    pub kind: NixReq,
    /// An optional path to a resolved atom, tied to its concrete resolved version.
    ///
    /// Only relevant if the Url contains a `"__VERSION__"` place-holder in its path component.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_version: Option<(Tag, Label)>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a nix eval-time git fetch.
pub struct NixGit {
    /// The URL of the git repository.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    pub git: gix::Url,
    /// A git ref or version constraint
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub spec: Option<GitSpec>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a dependency which is fetched at build time as an FOD.
pub struct NixSrc {
    /// The URL from which to fetch the build-time source.
    pub(crate) build: Url,
    #[serde(default, skip_serializing_if = "not")]
    pub(crate) unpack: bool,
}

/// Represents different possible types of direct dependencies, i.e. those in the atom format
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectDeps {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    nix: HashMap<Name, NixFetch>,
}

/// A newtype wrapper to tie a `DocumentMut` to a specific serializable type `T`.
#[derive(Debug)]
pub(super) struct TypedDocument<T> {
    /// The underlying `toml_edit` document.
    inner: DocumentMut,
    _marker: PhantomData<T>,
}

struct AtomWriter {
    set_tag: Tag,
    atom_req: AtomReq,
    mirror: SetMirror,
}

//================================================================================================
// Traits
//================================================================================================

/// A trait for writing dependencies to a mutable TOML document representing an Atom manifest.
trait WriteDeps<T: Serialize, K: VerifiedName> {
    /// The error type returned by the methods.
    type Error;

    /// Writes the dependency to the given TOML document.
    fn write_dep(&self, key: K, doc: &mut TypedDocument<T>) -> Result<(), Self::Error>;
}

//================================================================================================
// Impls
//================================================================================================

impl AsMut<AtomReq> for AtomReq {
    fn as_mut(&mut self) -> &mut AtomReq {
        self
    }
}

impl AsMut<Dependency> for Dependency {
    fn as_mut(&mut self) -> &mut Dependency {
        self
    }
}

impl<T: Serialize> AsMut<DocumentMut> for TypedDocument<T> {
    fn as_mut(&mut self) -> &mut DocumentMut {
        &mut self.inner
    }
}

impl FromStr for GitSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(req) = VersionReq::parse(s) {
            Ok(GitSpec::Version(req))
        } else {
            Ok(GitSpec::Ref(s.to_string()))
        }
    }
}

impl AtomReq {
    /// Creates a new `AtomReq` with the specified version requirement and location.
    pub fn new(version: VersionReq) -> Self {
        Self { version }
    }

    /// Returns a reference to the version requirement.
    pub fn version(&self) -> &VersionReq {
        &self.version
    }

    /// Sets the version requirement to a new value.
    pub fn set_version(&mut self, version: VersionReq) {
        self.version = version
    }
}

impl Dependency {
    pub(super) fn new() -> Self {
        Dependency {
            from: HashMap::new(),
            direct: DirectDeps::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.from.is_empty() && self.direct.is_empty()
    }

    pub(crate) fn from(&self) -> &AtomFrom {
        &self.from
    }

    pub(crate) fn direct(&self) -> &DirectDeps {
        &self.direct
    }
}

impl DirectDeps {
    fn new() -> Self {
        Self {
            nix: HashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.nix.is_empty()
    }

    pub(crate) fn nix(&self) -> &HashMap<Name, NixFetch> {
        &self.nix
    }
}

impl NixFetch {
    /// Determines the type of `DirectPin` from a given URL and other parameters.
    fn determine(
        url: AliasedUrl,
        git: Option<GitSpec>,
        tar: Option<bool>,
        build: bool,
        unpack: Option<bool>,
    ) -> Result<Self, DocError> {
        let AliasedUrl { from, url } = url;
        let path = url.path.to_path_lossy();
        let is_tar = || {
            tar.is_some_and(|b| b)
                || path.extension() == Some(OsStr::new("tar"))
                || path
                    .file_name()
                    .is_some_and(|f| f.to_str().is_some_and(|f| f.contains(".tar.")))
        };

        let dep = if url.scheme == gix::url::Scheme::File {
            // TODO: handle file paths to sources; requires anonymous atoms
            todo!()
        } else if url.scheme == gix::url::Scheme::Ssh
            || git.is_some()
            || path.extension() == Some(OsStr::new("git"))
        {
            NixFetch {
                kind: NixReq::Git(NixGit {
                    git: url,
                    spec: git.and_then(|x| {
                        // writing head to the manifest is redundant
                        if x == GitSpec::Ref("HEAD".into()) {
                            None
                        } else {
                            Some(x)
                        }
                    }),
                }),
                from_version: from,
            }
        } else if build {
            NixFetch {
                kind: NixReq::Build(NixSrc {
                    build: url.to_string().parse()?,
                    unpack: unpack != Some(false) && is_tar() || unpack.is_some_and(|b| b),
                }),
                from_version: from,
            }
        } else if tar != Some(false) && is_tar() {
            NixFetch {
                kind: NixReq::Tar(url.to_string().parse()?),
                from_version: from,
            }
        } else {
            NixFetch {
                kind: NixReq::Url(url.to_string().parse()?),
                from_version: from,
            }
        };
        Ok(dep)
    }
}

impl ManifestWriter {
    const ATOM_BUG: &str = "bug, `AtomId` construction is infallible when derived directly from a \
                            root and doesn't need to be calculated";
    const RESOLUTION_ERR_MSG: &str = "unlocked dependency could not be resolved remotely";
    const UPDATE_DEPENDENCY: &str = "updating out of date dependency in accordance with spec";

    /// Constructs a new `ManifestWriter`, ensuring that the manifest and lock file constraints
    /// are respected.
    pub async fn new(repo: Option<&ThreadSafeRepository>, path: &Path) -> Result<Self, AtomError> {
        use std::fs;
        let path = if path.file_name() == Some(OsStr::new(crate::ATOM_MANIFEST_NAME.as_str())) {
            path.into()
        } else {
            path.join(crate::ATOM_MANIFEST_NAME.as_str())
        };
        let lock_path = path.with_file_name(crate::LOCK_NAME.as_str());
        let toml_str = fs::read_to_string(&path).inspect_err(|_| {
            tracing::error!(message = "No atom exists", path = %path.display());
        })?;
        let (doc, manifest) = TypedDocument::new(&toml_str)?;
        let resolved_sets = SetResolver::new(repo, &manifest)?
            .get_and_check_sets()
            .await?;

        let lock = if let Ok(lock_str) = fs::read_to_string(&lock_path) {
            toml_edit::de::from_str(&lock_str)?
        } else {
            Lockfile::default()
        };
        let mut writer = ManifestWriter {
            doc,
            lock,
            path,
            resolved: resolved_sets,
        };
        writer.reconcile(manifest).await?;
        Ok(writer)
    }

    /// Runs the sanitization process, and then the synchronization process to ensure a fully
    /// consistent manifest and lock. This function is called in the `ManifestWriter` constructor
    /// to ensure that we are never operating on a stale manifest.
    async fn reconcile(&mut self, manifest: Manifest) -> Result<(), DocError> {
        self.set_sets();
        self.sanitize(&manifest);
        self.synchronize(manifest).await?;
        Ok(())
    }

    fn set_sets(&mut self) {
        self.lock.sets = self.resolved().details().to_owned();
    }

    /// Removes any dependencies from the lockfile that are no longer present in the
    /// manifest, ensuring the lockfile only contains entries that are still relevant,
    /// then calls into synchronization logic to ensure consistency.
    fn sanitize(&mut self, manifest: &Manifest) {
        self.lock.deps.as_mut().retain(|_, dep| match dep {
            Dep::Atom(atom_dep) => {
                if let Some(SetDetails { tag: name, .. }) = self.lock.sets.get(&atom_dep.set()) {
                    if let Some(set) = manifest.deps().from().get(name) {
                        return set.contains_key(atom_dep.label());
                    } else {
                        false
                    };
                }
                false
            },
            Dep::Nix(nix) => manifest.deps().direct().nix().contains_key(nix.name()),
            Dep::NixGit(nix_git) => manifest.deps().direct().nix().contains_key(&nix_git.name),
            Dep::NixTar(nix_tar) => manifest.deps().direct().nix().contains_key(&nix_tar.name),
            Dep::NixSrc(build_src) => manifest.deps().direct().nix().contains_key(&build_src.name),
        });
    }

    fn insert_or_update_and_log(&mut self, key: Either<AtomId<Root>, Name>, dep: &Dep) {
        if self
            .lock
            .deps
            .as_mut()
            .insert(key, dep.to_owned())
            .is_some()
        {
            match &dep {
                Dep::Atom(dep) => {
                    let tag = self.resolved.details().get(&dep.set()).map(|d| &d.tag);
                    tracing::warn!(
                        message = Self::UPDATE_DEPENDENCY,
                        label = %dep.label(),
                        set = ?tag,
                        r#type = "atom"
                    );
                },
                Dep::Nix(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                Dep::NixGit(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                Dep::NixTar(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                Dep::NixSrc(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
            }
        }
    }

    fn lock_atom(
        &mut self,
        req: VersionReq,
        id: AtomId<Root>,
        set_tag: Tag,
    ) -> Result<Dep, crate::store::git::Error> {
        if let Ok(dep) = self.resolved.resolve_atom(&id, &req) {
            let dep = Dep::Atom(dep);
            self.insert_or_update_and_log(Either::Left(id), &dep);
            Ok(dep)
        } else if let Some(repo) = &self.resolved.repo {
            let uri = Uri::from((id.label().to_owned(), Some(req)));
            let (_, dep) = self.resolve_from_local(&uri, repo)?;
            let dep = Dep::Atom(dep);
            self.insert_or_update_and_log(Either::Left(id), &dep);
            Ok(dep)
        } else {
            let versions: Vec<_> = self
                .resolved()
                .atoms()
                .get(&id)
                .map(|s| s.keys().collect())
                .unwrap_or_default();
            tracing::warn!(
                message = Self::RESOLUTION_ERR_MSG,
                set = %set_tag,
                atom = %id.label(),
                requested.version = %req,
                avaliable.versions = %toml_edit::ser::to_string(&versions).unwrap_or_default()
            );
            Err(DocError::Error(Box::new(crate::store::git::Error::NoMatchingVersion)).into())
        }
    }

    fn synchronize_atom(
        &mut self,
        req: VersionReq,
        id: AtomId<Root>,
        set_tag: Tag,
    ) -> Result<(), crate::store::git::Error> {
        if !self
            .lock
            .deps
            .as_ref()
            .contains_key(&either::Either::Left(id.to_owned()))
        {
            self.lock_atom(req, id, set_tag)?;
        } else if let Some(Dep::Atom(dep)) = self
            .lock
            .deps
            .as_ref()
            .get(&either::Either::Left(id.to_owned()))
        {
            if !req.matches(dep.version()) {
                self.lock_atom(req, id, set_tag)?;
            }
        }
        Ok(())
    }

    /// Updates the lockfile to match the dependencies specified in the manifest.
    /// It resolves any new dependencies, updates existing ones if their version
    /// requirements have changed, and ensures the lockfile is fully consistent.
    pub(crate) async fn synchronize(&mut self, manifest: Manifest) -> Result<(), DocError> {
        for (set_tag, set) in manifest.deps.from {
            let maybe_root = self
                .resolved
                .roots()
                .get(&Either::Left(set_tag.to_owned()))
                .map(ToOwned::to_owned);
            if let Some(root) = maybe_root {
                for (label, req) in set {
                    let id = AtomId::construct(&root, label.to_owned()).map_err(|e| {
                        DocError::AtomIdConstruct(format!(
                            "set: {}, atom: {}, err: {}",
                            &set_tag, &label, e
                        ))
                    })?;
                    self.synchronize_atom(req.to_owned(), id.to_owned(), set_tag.to_owned())
                        .ok();
                }
            } else {
                tracing::warn!(
                    message = "set was not resolved to an origin id, can't syncrhonize it",
                    set = %set_tag,
                );
            }
        }

        for (name, dep) in manifest.deps.direct.nix {
            let key = Either::Right(name.to_owned());
            let locked = self.lock.deps.as_ref().get(&key);
            if let Some(lock) = locked {
                use crate::lock::NixUrls;
                let url = dep.get_url();
                let mut unmatched = false;
                match (lock, url, &dep.kind) {
                    (Dep::Nix(nix), NixUrls::Url(url), _) => unmatched = nix.url() != url,
                    (Dep::NixGit(git), NixUrls::Git(url), NixReq::Git(NixGit { spec, .. })) => {
                        // upstream bug: false positive (it is read later unconditionally)
                        #[allow(unused_assignments)]
                        if let (Some(GitSpec::Version(req)), Some(version)) = (spec, &git.version) {
                            unmatched = !req.matches(version);
                        }
                        unmatched = git.url() != url;
                    },
                    (Dep::NixTar(tar), NixUrls::Url(url), _) => unmatched = tar.url() != url,
                    (Dep::NixSrc(build), NixUrls::Url(url), _) => unmatched = build.url() != url,
                    _ => {},
                }
                if unmatched {
                    tracing::warn!(message = "locked URL doesn't match, updating...", direct.nix = %name);
                    let (_, dep) = self.resolve_nix(dep, Some(&name)).await?;
                    self.lock.deps.as_mut().insert(key, dep);
                }
            } else if let Ok((_, dep)) = self.resolve_nix(dep, Some(&name)).await {
                self.lock.deps.as_mut().insert(key, dep);
            } else {
                tracing::warn!(message = Self::RESOLUTION_ERR_MSG, direct.nix = %name);
            }
        }
        Ok(())
    }

    async fn resolve_nix(
        &self,
        dep: NixFetch,
        key: Option<&Name>,
    ) -> Result<(Name, Dep), DocError> {
        let get_dep = || {
            if let Some((set, atom)) = &dep.from_version {
                if let Some(root) = self.resolved.roots.get(&Either::Left(set.to_owned())) {
                    if let Ok(id) = AtomId::construct(root, atom.to_owned()) {
                        if let Some(Dep::Atom(atom)) =
                            self.lock.deps.as_ref().get(&Either::Left(id))
                        {
                            return dep.new_from_version(atom.version());
                        }
                    }
                }
            }
            dep
        };
        let dep = get_dep();
        dep.resolve(key).await.map_err(Into::into)
    }

    fn resolve_from_uri(
        &self,
        uri: &Uri,
        root: &Root,
    ) -> Result<(AtomReq, AtomDep), crate::store::git::Error> {
        let id = AtomId::construct(root, uri.label().to_owned()).expect(Self::ATOM_BUG);
        let dep = self
            .resolved()
            .resolve_atom(&id, uri.version().unwrap_or(&VersionReq::STAR))?;

        let req = AtomReq::new(
            uri.version()
                .unwrap_or(&VersionReq::parse(dep.version().to_string().as_str())?)
                .to_owned(),
        );
        Ok((req, dep))
    }

    fn resolve_from_local(
        &self,
        uri: &Uri,
        repo: &Repository,
    ) -> Result<(AtomReq, AtomDep), crate::store::git::Error> {
        /* we are in a local git repository */

        // FIXME?: do we need to add a flag to make this configurable?
        let root = repo.head_commit()?.calculate_origin()?;

        if let Ok(res) = self.resolve_from_uri(uri, &root) {
            /* local store has a mirror which resolved this atom successfully */
            Ok(res)
        } else {
            let path = self
                .resolved
                .ekala
                .manifest
                .set
                .packages
                .as_ref()
                .get(uri.label())
                .ok_or(DocError::NoLocal)?;
            let content = std::fs::read_to_string(path.join(ATOM_MANIFEST_NAME.as_str()))?;
            let atom = Manifest::get_atom(&content)?;
            if &atom.label != uri.label() {
                return Err(DocError::SetError(sets::Error::Inconsistent).into());
            }
            let req = AtomReq::new(
                uri.version()
                    .unwrap_or(&VersionReq::parse(atom.version.to_string().as_str())?)
                    .to_owned(),
            );
            let id = AtomId::construct(&root, uri.label().to_owned()).expect(Self::ATOM_BUG);
            let mut version = atom.version.clone();
            version.pre = Prerelease::new("local")?;
            let dep = AtomDep::from(UnpackedRef {
                id,
                version,
                rev: None,
            });
            Ok((req, dep))
        }
    }

    fn resolve_uri(
        &mut self,
        uri: &Uri,
        mirror: &SetMirror,
    ) -> Result<(AtomReq, AtomDep), crate::store::git::Error> {
        // FIXME: we still need to handle when users pass a filepath (i.e. file://)
        if let (Some(root), SetMirror::Url(_)) = (
            self.resolved.roots.get(&Either::Right(mirror.to_owned())),
            &mirror,
        ) {
            /* set is remote and exists in the manifest, we can grab from an already resolved
             * mirror */
            self.resolve_from_uri(uri, root)
        } else if let SetMirror::Url(url) = mirror {
            /* set doesn't exist, we need to resolve from the passed url */
            let transport = self.resolved.transports.get_mut(url);
            uri.resolve(transport)
        } else if let Some(repo) = &self.resolved.repo {
            /* we are in a local git repository */

            self.resolve_from_local(uri, repo)
        } else {
            // TODO: we need a notion of "root" for an ekala set outside of a repository
            // maybe just a constant would do for a basic remoteless store?
            tracing::error!(
                suggestion =
                    "if you add them by hand to the manifest, they will resolve at eval-time",
                "haven't yet implemented adding local dependencies outside of git"
            );
            todo!()
        }
    }

    fn get_set_tag(&self, lock_entry: &AtomDep, uri: &Uri, set_tag_from_user: Option<Tag>) -> Tag {
        use crate::lock;
        self.resolved
            .details()
            .get(&lock_entry.set())
            .map(|s| s.tag.to_owned())
            .or(set_tag_from_user)
            .or_else(|| {
                if let Some(url) = uri.url() {
                    lock::url_filename_as_tag(url).ok()
                } else if let Some(repo) = &self.resolved.repo {
                    repo.workdir()
                        .and_then(|p| p.canonicalize().ok())
                        .and_then(|p| p.file_stem().map(ToOwned::to_owned))
                        .and_then(|f| Tag::try_from(f.as_os_str()).ok())
                } else {
                    Tag::try_from("default").ok()
                }
            })
            .expect("bug; default tag should be infallible")
    }

    fn update_lock_set(&mut self, set: GitDigest, mirror: SetMirror, tag: Tag) {
        use std::collections::btree_map::Entry;
        match self.lock.sets.entry(set) {
            Entry::Vacant(entry) => {
                entry.insert(SetDetails {
                    tag,
                    mirrors: BTreeSet::from([mirror]),
                });
            },
            Entry::Occupied(mut entry) => {
                entry.get_mut().mirrors.insert(mirror);
            },
        };
    }

    /// Adds a user-requested atom URI to the manifest and lock files, ensuring they remain in sync.
    pub fn add_uri(
        &mut self,
        uri: Uri,
        set_tag: Option<Tag>,
    ) -> Result<(), crate::store::git::Error> {
        let mirror = if let Some(url) = uri.url() {
            SetMirror::Url(url.to_owned())
        } else {
            SetMirror::Local
        };
        let (atom_req, lock_entry) = self.resolve_uri(&uri, &mirror)?;

        let label = lock_entry.label().to_owned();
        let id = AtomId::from(&lock_entry);
        let set_tag = self.get_set_tag(&lock_entry, &uri, set_tag);

        let atom_writer = AtomWriter {
            set_tag: set_tag.to_owned(),
            atom_req,
            mirror: mirror.to_owned(),
        };

        let set = lock_entry.set().to_owned();
        atom_writer.write_dep(label, &mut self.doc)?;
        self.insert_or_update_and_log(Either::Left(id.to_owned()), &Dep::Atom(lock_entry));

        self.update_lock_set(set, mirror, set_tag);

        Ok(())
    }

    /// Adds a user-requested pin URL to the manifest and lock files, ensuring they remain in sync.
    pub async fn add_url(
        &mut self,
        url: AliasedUrl,
        key: Option<Name>,
        git: Option<GitSpec>,
        tar: Option<bool>,
        build: bool,
        unpack: Option<bool>,
    ) -> Result<(), DocError> {
        let dep = NixFetch::determine(url, git, tar, build, unpack)?;
        let (key, lock_entry) = self.resolve_nix(dep.to_owned(), key.as_ref()).await?;

        dep.write_dep(key.to_owned(), &mut self.doc)?;
        self.insert_or_update_and_log(Either::Right(key), &lock_entry);
        Ok(())
    }

    /// Atomically writes the changes to the manifest and lock files on disk.
    /// This method should be called last, after all changes have been processed.
    pub fn write_atomic(&mut self) -> Result<(), DocError> {
        use std::io::Write;

        use tempfile::NamedTempFile;

        let _validate: Manifest = toml_edit::de::from_str(&self.doc.as_mut().to_string())?;
        let dir = self
            .path
            .parent()
            .ok_or(DocError::Missing(self.path.clone()))?;
        let lock_path = self.path.with_file_name(crate::LOCK_NAME.as_str());
        let mut tmp =
            NamedTempFile::with_prefix_in(format!(".{}", crate::ATOM_MANIFEST_NAME.as_str()), dir)?;
        let mut tmp_lock =
            NamedTempFile::with_prefix_in(format!(".{}", crate::LOCK_NAME.as_str()), dir)?;
        tmp.write_all(self.doc.as_mut().to_string().as_bytes())?;
        tmp_lock.write_all(
            "# This file is automatically @generated by eka.\n# It is not intended for manual \
             editing.\n"
                .as_bytes(),
        )?;
        tmp_lock.write_all(toml_edit::ser::to_string_pretty(&self.lock)?.as_bytes())?;
        tmp.persist(&self.path)?;
        tmp_lock.persist(lock_path)?;
        Ok(())
    }

    fn resolved(&self) -> &ResolvedSets {
        &self.resolved
    }

    /// acquire a reference to the lockfile structure
    pub fn lock(&self) -> &Lockfile {
        &self.lock
    }
}

impl<T: Serialize + DeserializeOwned> TypedDocument<T> {
    /// Creates a new `TypedDocument` from a serializable instance of `T`.
    /// This enforces that the document is created by serializing `T`.
    pub fn new(doc: &str) -> Result<(Self, T), DocError> {
        let validated: T = toml_edit::de::from_str(doc)?;

        let inner = doc.parse::<DocumentMut>()?;
        Ok((
            Self {
                inner,
                _marker: PhantomData,
            },
            validated,
        ))
    }
}

impl WriteDeps<Manifest, Label> for NixFetch {
    type Error = toml_edit::ser::Error;

    fn write_dep(&self, key: Label, doc: &mut TypedDocument<Manifest>) -> Result<(), Self::Error> {
        use toml_edit::{Item, Value};
        let doc = doc.as_mut();
        let nix_table = toml_edit::ser::to_document(self)?.as_table().to_owned();
        let dotted = nix_table.len() == 1;
        let mut nix_table = nix_table.into_inline_table();
        nix_table.set_dotted(dotted);

        let nix_deps = doc
            .entry("deps")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .and_then(|t| {
                t.set_implicit(true);
                t.entry("direct")
                    .or_insert(toml_edit::table())
                    .as_table_mut()
            })
            .and_then(|t| {
                t.set_implicit(true);
                t.entry("nix").or_insert(toml_edit::table()).as_table_mut()
            })
            .ok_or(toml_edit::ser::Error::Custom(format!(
                "writing `[deps.direct.nix]` dependency failed: {}",
                &key
            )))?;
        nix_deps.set_implicit(true);
        nix_deps[key.as_str()] = Item::Value(Value::InlineTable(nix_table));
        doc.fmt();

        Ok(())
    }
}

impl WriteDeps<Manifest, Label> for AtomWriter {
    type Error = toml_edit::ser::Error;

    fn write_dep(&self, key: Label, doc: &mut TypedDocument<Manifest>) -> Result<(), Self::Error> {
        use toml_edit::{Array, Value};
        let doc = doc.as_mut();
        let mirror = self.mirror.to_string();

        let package = doc
            .entry("package")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        package.set_implicit(true);

        let sets = package
            .entry("sets")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        sets.set_implicit(true);

        let set = sets
            .entry(self.set_tag.as_str())
            .or_insert(toml_edit::value(Value::Array(Array::new())))
            .as_value_mut()
            .and_then(|v| v.as_array_mut())
            .unwrap();

        if !set.iter().any(|x| x.to_string().contains(&mirror)) {
            set.push(mirror);
            set.fmt();
        }

        let deps = doc
            .entry("deps")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        deps.set_implicit(true);

        let from = deps
            .entry("from")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        from.set_implicit(true);

        let set = from
            .entry(self.set_tag.as_str())
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        set.set_implicit(true);

        set[key.as_str()] = toml_edit::Item::Value(self.atom_req.version().to_string().into());

        doc.fmt();

        Ok(())
    }
}

//================================================================================================
// Functions
//================================================================================================

/// Deserializes a `gix::url::Url` from a string.
pub(crate) fn deserialize_url<'de, D>(deserializer: D) -> Result<gix::url::Url, D::Error>
where
    D: Deserializer<'de>,
{
    let name = BString::deserialize(deserializer)?;
    gix::url::parse(name.as_bstr())
        .map_err(|e| <D::Error as serde::de::Error>::custom(e.to_string()))
}

/// A helper function for `serde(skip_serializing_if)` to omit `false` boolean values.
pub(crate) fn not(b: &bool) -> bool {
    !b
}

/// Serializes a `gix::url::Url` to a string.
pub(crate) fn serialize_url<S>(url: &gix::url::Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let str = url.to_string();
    serializer.serialize_str(&str)
}
