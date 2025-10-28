use id::Name;
use nix_compat::nixhash::NixHash;
use package::metadata::GitDigest;
use package::metadata::manifest::direct::{NixFetch, NixReq};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};
use uri::serde_gix_url;
use url::Url;

use crate::{id, package, uri};

//================================================================================================
// Types
//================================================================================================

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
    #[serde(with = "serde_gix_url")]
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

/// An enum to handle different URL types for filename extraction.
pub(in crate::package) enum NixUrls<'a> {
    Url(&'a Url),
    Git(&'a gix::Url),
}

//================================================================================================
// Impls
//================================================================================================

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

impl NixFetch {
    pub(in crate::package) fn get_url(&self) -> NixUrls<'_> {
        match &self.kind {
            NixReq::Tar(url) => NixUrls::Url(url),
            NixReq::Url(url) => NixUrls::Url(url),
            NixReq::Build(nix_src) => NixUrls::Url(&nix_src.build),
            NixReq::Git(nix_git) => NixUrls::Git(&nix_git.git),
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
