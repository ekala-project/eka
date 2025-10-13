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

use std::collections::HashMap;
use std::ffi::OsStr;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use bstr::{BString, ByteSlice};
use semver::VersionReq;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use toml_edit::DocumentMut;
use url::Url;

use crate::id::{AtomTag, Name};
use crate::uri::{AliasedUrl, Uri};
use crate::{Lockfile, Manifest};

//================================================================================================
// Types
//================================================================================================

type AtomFrom = HashMap<Name, HashMap<AtomTag, VersionReq>>;

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
    /// The manifest path could not be accessed.
    #[error("the atom directory disappeared or is inaccessible: {0}")]
    Missing(PathBuf),
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
///     let mut writer = ManifestWriter::new(Path::new("/path/to/atom.toml"))
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
}

/// Represents the underlying type of Nix dependency
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
pub enum NixReq {
    /// A nix eval-time fetch
    Fetch(NixFetch),
    /// A nix eval-time git fetch
    Git(NixGit),
    /// A nix build-time fetch, e.g. for build sources
    Build(NixSrc),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a nix fetch, either direct or tarball.
pub struct NixFetch {
    /// The URL of the resource.
    #[serde(flatten)]
    pub url: NixUrl,
    /// An optional path to a resolved atom, tied to its concrete resolved version.
    ///
    /// Only relevant if the Url contains a `"__VERSION__"` place-holder in its path component.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a nix eval-time git fetch.
pub struct NixGit {
    /// The URL of the git repository.
    pub git: gix::Url,
    /// An optional version, tied to a concrete, resolved version of an atom.
    ///
    /// Only relevant if the Url contains a `"{version}"` place-holder in its string
    /// representation.
    ///
    /// This field is omitted from serialization if None.
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

/// Represents the field name in a Nix dependency, which determines how it will be fetched
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum NixUrl {
    /// A tarball url which will be unpacked before being hashed
    #[serde(rename = "tar")]
    Tar(Url),
    /// A straight url which will be fetched and hashed directly
    #[serde(rename = "url")]
    Url(Url),
}

/// Represents different possible types of direct dependencies, i.e. those in the atom format
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectDeps {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    nix: HashMap<Name, NixReq>,
}

/// A newtype wrapper to tie a `DocumentMut` to a specific serializable type `T`.
struct TypedDocument<T> {
    /// The underlying `toml_edit` document.
    inner: DocumentMut,
    _marker: PhantomData<T>,
}

//================================================================================================
// Traits
//================================================================================================

/// A trait for writing dependencies to a mutable TOML document representing an Atom manifest.
trait WriteDeps<T: Serialize> {
    /// The error type returned by the methods.
    type Error;

    /// Writes the dependency to the given TOML document.
    fn write_dep(&self, name: &str, doc: &mut TypedDocument<T>) -> Result<(), Self::Error>;
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

    pub(crate) fn nix(&self) -> &HashMap<Name, NixReq> {
        &self.nix
    }
}

impl NixReq {
    /// Determines the type of `DirectPin` from a given URL and other parameters.
    fn determine(
        url: &AliasedUrl,
        git: Option<GitSpec>,
        tar: Option<bool>,
        build: bool,
        unpack: Option<bool>,
    ) -> Result<Self, DocError> {
        let from = url.from();
        let url = url.url();
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
            NixReq::Git(NixGit {
                git: url.to_owned(),
                spec: git,
            })
        } else if build {
            NixReq::Build(NixSrc {
                build: url.to_string().parse()?,
                unpack: unpack != Some(false) && is_tar() || unpack.is_some_and(|b| b),
            })
        } else if tar != Some(false) && is_tar() {
            NixReq::Fetch(NixFetch {
                url: NixUrl::Tar(url.to_string().parse()?),
                from: from.map(ToOwned::to_owned),
            })
        } else {
            NixReq::Fetch(NixFetch {
                url: NixUrl::Url(url.to_string().parse()?),
                from: from.map(ToOwned::to_owned),
            })
        };
        Ok(dep)
    }
}

impl ManifestWriter {
    /// Constructs a new `ManifestWriter`, ensuring that the manifest and lock file constraints
    /// are respected.
    pub async fn new(path: &Path) -> Result<Self, DocError> {
        use std::fs;
        let path = if path.file_name() == Some(OsStr::new(crate::MANIFEST_NAME.as_str())) {
            path.into()
        } else {
            path.join(crate::MANIFEST_NAME.as_str())
        };
        let lock_path = path.with_file_name(crate::LOCK_NAME.as_str());
        let toml_str = fs::read_to_string(&path).inspect_err(|_| {
            tracing::error!(message = "No atom exists", path = %path.display());
        })?;
        let (doc, manifest) = TypedDocument::new(&toml_str)?;

        let mut lock = if let Ok(lock_str) = fs::read_to_string(&lock_path) {
            toml_edit::de::from_str(&lock_str)?
        } else {
            Lockfile::default()
        };
        lock.sanitize(&manifest).await;

        Ok(ManifestWriter { doc, lock, path })
    }

    /// Adds a user-requested atom URI to the manifest and lock files, ensuring they remain in sync.
    pub fn add_uri(&mut self, uri: Uri, set: Option<Name>) -> Result<(), DocError> {
        use crate::lock::Dep;

        let (atom_req, lock_entry) = uri.resolve(None).map_err(Box::new)?;

        let tag = lock_entry.tag().to_owned();
        let id = lock_entry.id().to_owned();

        // self.doc.write_dep(&tag, &dep)?;
        if !self.lock.deps.as_mut().insert(Dep::Atom(lock_entry)) {
            tracing::warn!(message = "updating lock entry", atom.id = %id);
        }

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
        let dep = NixReq::determine(&url, git, tar, build, unpack)?;
        let (key, lock_entry) = dep.resolve(key.as_ref()).await?;

        self.doc.write_dep(&key, todo!())?;
        if self.lock.deps.as_mut().insert(lock_entry) {
            tracing::warn!(message = "updating lock entry", direct.nix = %key);
        }
        Ok(())
    }

    /// Atomically writes the changes to the manifest and lock files on disk.
    /// This method should be called last, after all changes have been processed.
    pub fn write_atomic(&mut self) -> Result<(), DocError> {
        use std::io::Write;

        use tempfile::NamedTempFile;

        let dir = self
            .path
            .parent()
            .ok_or(DocError::Missing(self.path.clone()))?;
        let lock_path = self.path.with_file_name(crate::LOCK_NAME.as_str());
        let mut tmp =
            NamedTempFile::with_prefix_in(format!(".{}", crate::MANIFEST_NAME.as_str()), dir)?;
        let mut tmp_lock =
            NamedTempFile::with_prefix_in(format!(".{}", crate::LOCK_NAME.as_str()), dir)?;
        tmp.write_all(self.doc.as_mut().to_string().as_bytes())?;
        tmp_lock.write_all(toml_edit::ser::to_string_pretty(&self.lock)?.as_bytes())?;
        tmp.persist(&self.path)?;
        tmp_lock.persist(lock_path)?;
        Ok(())
    }
}

impl TypedDocument<Manifest> {
    /// Writes an atom dependency into the manifest document.
    pub fn write_dep(&mut self, key: &str, req: &Dependency) -> Result<(), toml_edit::ser::Error> {
        req.write_dep(key, self)
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

impl WriteDeps<Manifest> for Dependency {
    type Error = toml_edit::ser::Error;

    fn write_dep(&self, key: &str, doc: &mut TypedDocument<Manifest>) -> Result<(), Self::Error> {
        let doc = doc.as_mut();
        let atom_table = toml_edit::ser::to_document(self)?.as_table().to_owned();

        if !doc.contains_table("deps") {
            doc["deps"] = toml_edit::table();
        }

        let deps = doc["deps"].as_table_mut().unwrap();
        deps.set_implicit(true);
        deps.set_position(deps.len() + 1);

        doc["deps"][key] = toml_edit::Item::Table(atom_table);
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
