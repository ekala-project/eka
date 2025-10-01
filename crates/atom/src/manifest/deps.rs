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
use std::marker::PhantomData;
use std::path::PathBuf;

use bstr::ByteSlice;
#[cfg(feature = "git")]
use gix::url as gix_url;
use semver::VersionReq;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;
use url::Url;

use crate::Manifest;
use crate::id::AtomTag;

/// Newtype wrapper to tie DocumentMut to a specific serializable type T.
pub struct TypedDocument<T> {
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
    /// The tag of the atom (if the toml key is different)
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<AtomTag>,
    /// The semantic version request specification of the atom.
    version: VersionReq,
    /// The location of the atom, whether local or remote.
    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    store: gix_url::Url,
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
    /// A git pin, with a ref or version.
    Git(GitPin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a simple pin, with an optional unpack field.
pub struct Pin {
    /// The url of the pin.
    pub pin: Url,
    /// Whether or not to unpack the pin.
    #[serde(skip_serializing_if = "not")]
    pub unpack: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
/// Represents a direct git pin to an external source.
///
/// This struct is used when a dependency is pinned directly to a Git repository.
pub struct GitPin {
    /// The URL of the source.
    pub repo: Url,
    /// The strategy used to fetch the resource, by version (resolving version tags), or by
    /// straight ref
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
    /// The atom id to reference a pin from.
    pub from: AtomTag,
    /// The name of the dependency to acquire from the atom (same as it's name if not present).
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
    /// The relative path within the source (for Nix imports).
    ///
    /// This field is omitted from serialization if None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The type of pin, either direct or indirect.
    ///
    /// This field is flattened in the TOML serialization.
    #[serde(flatten)]
    pub kind: PinType,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a dependency which is fetched at build time as an FOD.
#[serde(deny_unknown_fields)]
pub struct SrcReq {
    /// The URL of the source.
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
    pub fn new(version: VersionReq, store: gix_url::Url, tag: Option<AtomTag>) -> Self {
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
    pub fn store(&self) -> &gix_url::Url {
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
    /// Toml deserialization errors
    #[error(transparent)]
    De(#[from] toml_edit::de::Error),
    /// Toml error
    #[error(transparent)]
    Ser(#[from] toml_edit::TomlError),
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
    pub fn write_atom_dep(
        &mut self,
        key: &str,
        req: &AtomReq,
    ) -> Result<(), toml_edit::ser::Error> {
        req.write_dep(key, self)
    }
}

impl AsMut<AtomReq> for AtomReq {
    fn as_mut(&mut self) -> &mut AtomReq {
        self
    }
}

impl WriteDeps<Manifest> for AtomReq {
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
pub(crate) fn serialize_url<S>(url: &gix_url::Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let str = url.to_string();
    serializer.serialize_str(&str)
}

pub(crate) fn deserialize_url<'de, D>(deserializer: D) -> Result<gix_url::Url, D::Error>
where
    D: Deserializer<'de>,
{
    use bstr::BString;
    let name = BString::deserialize(deserializer)?;
    gix_url::parse(name.as_bstr())
        .map_err(|e| <D::Error as serde::de::Error>::custom(e.to_string()))
}
