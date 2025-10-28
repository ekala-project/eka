//! # Direct Dependencies
//!
//! This module handles direct dependencies that are not atoms, including
//! Nix fetchers for external sources like Git repositories, tarballs, and URLs.
//!
//! ## Supported Dependency Types
//!
//! - **Git dependencies** (`NixGit`) - Git repositories with optional version specs
//! - **Tarball dependencies** (`NixTar`) - Compressed archives fetched and unpacked
//! - **URL dependencies** (`NixUrl`) - Direct file downloads
//! - **Build dependencies** (`NixSrc`) - Sources fetched during build time
//!
//! ## Key Types
//!
//! - [`DirectDeps`] - Container for all direct dependencies
//! - [`NixFetch`] - Base type for all Nix fetcher dependencies
//! - [`NixReq`] - Enum representing different fetcher types
//! - [`NixGit`], [`NixTar`], [`NixSrc`] - Specific fetcher implementations

use std::collections::HashMap;
use std::ffi::OsStr;

use bstr::ByteSlice;
use id::{Name, Tag};
use package::GitSpec;
use package::metadata::{DocError, TypedDocument};
use semver::Version;
use serde::{Deserialize, Serialize};
use uri::{AliasedUrl, VERSION_PLACEHOLDER, serde_gix_url};
use url::Url;

use super::WriteDeps;
use crate::{Label, Manifest, id, package, uri};

//================================================================================================
// Types
//================================================================================================

/// Represents different possible types of direct dependencies, i.e. those in the atom format
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectDeps {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    nix: HashMap<Name, NixFetch>,
}

/// Represents the underlying type of Nix dependency
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
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
    #[serde(with = "serde_gix_url")]
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
    #[serde(default, skip_serializing_if = "super::not")]
    pub(crate) unpack: bool,
}

//================================================================================================
// Impls
//================================================================================================

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

impl DirectDeps {
    pub(super) fn new() -> Self {
        Self {
            nix: HashMap::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nix.is_empty()
    }

    pub(crate) fn nix(&self) -> &HashMap<Name, NixFetch> {
        &self.nix
    }
}

impl NixFetch {
    /// Determines the type of `DirectPin` from a given URL and other parameters.
    pub(in crate::package) fn determine(
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
}
