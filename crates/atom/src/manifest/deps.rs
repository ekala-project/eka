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
//! - **Atom dependencies** - References to other atoms by ID and version
//! - **Pin dependencies** - Direct references to external sources (URLs, Git repos, tarballs)
//! - **Source dependencies** - Build-time dependencies like source code or config files
//!
//! ## Key Types
//!
//! - [`Dependency`] - The main dependency structure containing all dependency types
//! - [`AtomReq`] - Requirements for atom dependencies
//! - [`PinReq`] - Requirements for pinned dependencies
//! - [`SrcReq`] - Requirements for build-time sources
//! - [`PinType`] - Enum distinguishing between direct and indirect pins
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
use std::ffi::OsStr;
use std::marker::PhantomData;
use std::path::PathBuf;

use bstr::ByteSlice;
use semver::VersionReq;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;
use url::Url;

use crate::id::AtomTag;
use crate::{Lockfile, Manifest};

/// A Writer struct to ensure modifications to the manifest and lock stay in sync
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
///
/// use atom::id::Name;
/// use atom::manifest::deps::ManifestWriter;
/// use atom::uri::Uri;
///
/// let mut writer = ManifestWriter::new(Path::new("/path/to/atom.toml")).unwrap();
/// let uri = "my-atom@^1.0.0".parse::<Uri>().unwrap();
/// let key = "my-atom".parse::<Name>().unwrap();
/// writer.add_uri(uri, Some(key)).unwrap();
/// writer.write_atomic().unwrap();
/// ```
pub struct ManifestWriter {
    path: PathBuf,
    doc: TypedDocument<Manifest>,
    lock: Lockfile,
}

/// Newtype wrapper to tie DocumentMut to a specific serializable type T.
struct TypedDocument<T> {
    /// The actual document we want associated with our type
    inner: DocumentMut,
    _marker: PhantomData<T>,
}

/// A trait to implement writing to a mutable toml document representing an atom Manifest
trait WriteDeps<T: Serialize> {
    /// The error type returned by the methods.
    type Error;

    /// write the dep to the given toml doc
    fn write_dep(&self, name: &str, doc: &mut TypedDocument<T>) -> Result<(), Self::Error>;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// The dependencies specified in the manifest
#[serde(untagged)]
pub enum Dependency {
    /// An atom dependency variant.
    Atom(AtomReq),
    /// A direct pin to an external source variant.
    Pin(PinReq),
    /// A dependency fetched at build-time as an FOD.
    Src(SrcReq),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a locked atom dependency, referencing a verifiable repository slice.
#[serde(deny_unknown_fields)]
pub struct AtomReq {
    /// The tag of the atom, used if the dependency name in the manifest
    /// differs from the atom's actual tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<AtomTag>,
    /// The semantic version requirement for the atom (e.g., "^1.0.0").
    version: VersionReq,
    /// The Git URL or local path where the atom's repository can be found.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    store: gix::url::Url,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
/// Represents the different types of pins for dependencies.
///
/// This enum distinguishes between direct pins (pointing to external URLs)
/// and indirect pins (referencing dependencies from other atoms).
pub enum PinType {
    /// A direct pin to an external source with a URL.
    Direct(DirectPin),
    /// An indirect pin referencing a dependency from another atom.
    Indirect(IndirectPin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
/// Represents the two types of direct pins.
pub enum DirectPin {
    /// A simple pin, with an optional unpack field.
    Straight(Pin),
    /// A pin pointing to a tarball which will be unpacked before hashing.
    Tarball(TarPin),
    /// A git pin, with a ref or version.
    Git(GitPin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a simple pin, with an optional unpack field.
pub struct Pin {
    /// The URL of the pinned resource.
    pub pin: Url,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a simple pin, with an optional unpack field.
pub struct TarPin {
    /// The URL of the tarball resource.
    pub tar: Url,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a direct git pin to an external source.
///
/// This struct is used when a dependency is pinned directly to a Git repository.
pub struct GitPin {
    /// The URL of the Git repository.
    pub repo: gix::Url,
    /// The fetching strategy, either by a specific ref (branch, tag, commit)
    /// or by resolving a semantic version tag.
    #[serde(flatten)]
    pub fetch: GitStrat,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents the two types of git fetch strategies.
pub enum GitStrat {
    #[serde(rename = "ref")]
    /// The refspec (e.g. branch or tag) of the source (for git-type pins).
    Ref(String),
    #[serde(rename = "version")]
    /// The version requirement of the source (for git-type pins).
    Version(VersionReq),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents an indirect pin referencing a dependency from another atom.
///
/// This struct is used when a dependency is sourced from another atom,
/// enabling composition of complex systems from simpler atom components.
pub struct IndirectPin {
    /// The tag of the atom from which to source the dependency.
    pub from: AtomTag,
    /// The name of the dependency to acquire from the source atom. If `None`,
    /// it defaults to the name of the current dependency.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a direct pin to an external source, such as a URL or tarball.
///
/// This struct is used to specify pinned dependencies in the manifest,
/// which can be either direct (pointing to URLs) or indirect (referencing
/// dependencies from other atoms).
#[serde(deny_unknown_fields)]
pub struct PinReq {
    /// An optional relative path within the fetched source, used to import Nix expressions; the
    /// precise behavior of which depends on whether or not the pin is a flake.
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import: Option<PathBuf>,
    /// The kind of pin, which can be a direct URL, a Git repository, or an
    /// indirect reference to a dependency from another atom.
    ///
    /// This field is flattened in the TOML serialization.
    #[serde(flatten)]
    pub kind: PinType,
    /// A bool representing whether the pin represents a Nix flake, changing the behavior of the
    /// `import` field, if so.
    ///
    /// This field is omitted from serialization if false.
    #[serde(skip_serializing_if = "not")]
    pub flake: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a dependency which is fetched at build time as an FOD.
#[serde(deny_unknown_fields)]
pub struct SrcReq {
    /// The URL from which to fetch the build-time source.
    pub src: Url,
}

impl AtomReq {
    /// Creates a new `AtomReq` with the specified version requirement and location.
    ///
    /// # Arguments
    ///
    /// * `version` - The semantic version requirement for the atom
    /// * `locale` - The location of the atom, either as a URL or relative path
    ///
    /// # Returns
    ///
    /// A new `AtomReq` instance with the provided version and location.
    pub fn new(version: VersionReq, store: gix::url::Url, tag: Option<AtomTag>) -> Self {
        Self {
            version,
            store,
            tag,
        }
    }

    /// return a reference to the version
    pub fn version(&self) -> &VersionReq {
        &self.version
    }

    /// set the version to a new value
    pub fn set_version(&mut self, version: VersionReq) {
        self.version = version
    }

    /// return a reference to the store location
    pub fn store(&self) -> &gix::url::Url {
        &self.store
    }

    /// return a reference to the atom tag
    pub fn tag(&self) -> Option<&AtomTag> {
        self.tag.as_ref()
    }
}

#[derive(thiserror::Error, Debug)]
/// transparent errors for TypedDocument
pub enum DocError {
    /// The manifest path access.
    #[error("the atom directory disappeared or is inaccessible: {0}")]
    Missing(PathBuf),
    /// Toml deserialization errors
    #[error(transparent)]
    De(#[from] toml_edit::de::Error),
    /// Toml error
    #[error(transparent)]
    Ser(#[from] toml_edit::TomlError),
    /// Filesystem error
    #[error(transparent)]
    Read(#[from] std::io::Error),
    /// Serialization error
    #[error(transparent)]
    Manifest(#[from] toml_edit::ser::Error),
    /// Serialization error
    #[error(transparent)]
    Write(#[from] tempfile::PersistError),
    /// Git resolution error
    #[error(transparent)]
    Git(#[from] Box<crate::store::git::Error>),
    /// Version resolution error
    #[error(transparent)]
    Semver(#[from] semver::Error),
    /// Utf conversion error
    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),
    /// URL parse error
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// Generic Error
    #[error(transparent)]
    Error(#[from] crate::lock::BoxError),
}

impl<T: Serialize + DeserializeOwned> TypedDocument<T> {
    /// Constructor: Create from a serializable instance of T.
    /// This enforces that the document comes from serializing T.
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

impl<T: Serialize> AsMut<DocumentMut> for TypedDocument<T> {
    fn as_mut(&mut self) -> &mut DocumentMut {
        &mut self.inner
    }
}
impl TypedDocument<Manifest> {
    /// Write an atom dependency into the manifest document
    pub fn write_dep(&mut self, key: &str, req: &Dependency) -> Result<(), toml_edit::ser::Error> {
        req.write_dep(key, self)
    }
}

impl AsMut<AtomReq> for AtomReq {
    fn as_mut(&mut self) -> &mut AtomReq {
        self
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

fn not(b: &bool) -> bool {
    !b
}

use serde::{Deserializer, Serializer};
pub(crate) fn serialize_url<S>(url: &gix::url::Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let str = url.to_string();
    serializer.serialize_str(&str)
}

pub(crate) fn deserialize_url<'de, D>(deserializer: D) -> Result<gix::url::Url, D::Error>
where
    D: Deserializer<'de>,
{
    use bstr::BString;
    let name = BString::deserialize(deserializer)?;
    gix::url::parse(name.as_bstr())
        .map_err(|e| <D::Error as serde::de::Error>::custom(e.to_string()))
}

use std::path::Path;

use crate::id::Name;
use crate::uri::{AliasedUrl, Uri};
impl ManifestWriter {
    /// Construct a new instance of a manifest writer ensuring all the constraints necessary to keep
    /// the lock and manifest in sync are respected.
    pub fn new(path: &Path) -> Result<Self, DocError> {
        use std::ffi::OsStr;
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
        lock.sanitize(&manifest);

        Ok(ManifestWriter { doc, lock, path })
    }

    /// After processing all changes, write the changes to the manifest and lock to disk. This
    /// method should be called last, after processing any requested changes.
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

    /// Function to add a user requested pin url to the manifest and lock files, ensuring they
    /// remain in sync.
    pub async fn add_url(
        &mut self,
        url: AliasedUrl,
        key: Option<Name>,
        import: Option<PathBuf>,
        flake: bool,
    ) -> Result<(), DocError> {
        let direct = DirectPin::determine(&url)?;
        let pin = PinReq {
            import: if let DirectPin::Straight(_) = direct {
                None
            } else {
                import.to_owned()
            },
            // TODO: add flags for indirect pins
            kind: PinType::Direct(direct.to_owned()),
            // add flake to specify flake
            flake,
        };

        let (key, lock_entry) = direct.resolve(key.as_ref()).await?;
        let dep = Dependency::Pin(pin);
        self.doc.write_dep(&key, &dep)?;
        if self
            .lock
            .deps
            .as_mut()
            .insert(key.to_owned(), lock_entry)
            .is_some()
        {
            tracing::warn!("updating lock entry for `{}`", key);
        }
        Ok(())
    }

    /// Function to add a user requested atom uri to the manifest and lock files, ensuring they
    /// remain in sync.
    pub fn add_uri(&mut self, uri: Uri, key: Option<Name>) -> Result<(), DocError> {
        use crate::lock::Dep;

        let tag = uri.tag();
        let maybe_version = uri.version();
        let url = uri.url();

        let req = if let Some(v) = maybe_version {
            v
        } else {
            &VersionReq::STAR
        };

        let key = if let Some(key) = key {
            key
        } else {
            tag.to_owned()
        };

        if let Some(url) = url {
            let mut atom = AtomReq::new(
                req.to_owned(),
                url.to_owned(),
                (&key != tag).then(|| tag.to_owned()),
            );
            let lock_entry = atom.resolve(&key).map_err(Box::new)?;

            if maybe_version.is_none() {
                let version = VersionReq::parse(lock_entry.version.to_string().as_str())?;
                atom.set_version(version);
            };

            let dep = Dependency::Atom(atom);

            self.doc.write_dep(key.as_str(), &dep)?;
            if self
                .lock
                .deps
                .as_mut()
                .insert(key.to_owned(), Dep::Atom(lock_entry))
                .is_some()
            {
                tracing::warn!("updating lock entry for `{}`", key);
            }
        } else {
            // search locally for atom tag
            todo!()
        }

        Ok(())
    }
}

impl DirectPin {
    fn determine(url: &AliasedUrl) -> Result<Self, DocError> {
        let r = url.r#ref();
        let url = url.url();
        let pin = if url.scheme == gix::url::Scheme::File {
            // TODO: handle file paths to sources
            todo!()
        } else if let Some(r) = r {
            let maybe_req = VersionReq::parse(r);
            let fetch = if let Ok(req) = maybe_req {
                GitStrat::Version(req)
            } else {
                GitStrat::Ref(r.to_owned())
            };
            DirectPin::Git(GitPin {
                repo: url.to_owned(),
                fetch,
            })
        } else {
            let path = url.path.to_path()?;
            if path.extension() == Some(OsStr::new("tar"))
                || path
                    .file_name()
                    .is_some_and(|f| f.to_str().is_some_and(|f| f.contains(".tar.")))
            {
                DirectPin::Tarball(TarPin {
                    tar: url.to_string().parse()?,
                })
            } else {
                DirectPin::Straight(Pin {
                    pin: url.to_string().parse()?,
                })
            }
        };
        Ok(pin)
    }
}
