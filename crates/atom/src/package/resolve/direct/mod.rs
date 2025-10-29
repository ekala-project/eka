//! # Direct Dependency Resolution
//!
//! This module handles the resolution of direct dependencies (non-atom packages)
//! using Nix fetchers. It supports various fetch types including URLs, Git repos,
//! tarballs, and build sources.
//!
//! ## Supported Fetch Types
//!
//! - **URL Fetch** - Direct file downloads with integrity verification
//! - **Git Fetch** - Git repositories with optional version/tag resolution
//! - **Tarball Fetch** - Compressed archives with unpacking
//! - **Build Source Fetch** - Sources for build-time dependencies
//!
//! ## Version Resolution
//!
//! For Git dependencies, version resolution supports:
//! - **Tag-based** - Semantic version tags (e.g., `v1.2.3`)
//! - **Ref-based** - Specific Git references (branches, commits)
//! - **HEAD** - Latest commit on default branch
//!
//! ## Nix Integration
//!
//! Uses the SNix ecosystem for content-addressed storage and reproducible
//! fetching with cryptographic integrity verification.

use std::sync::Arc;

use bstr::ByteSlice;
use direct::{NixFetch, NixGit, NixReq};
use either::Either;
use gix::protocol::handshake::Ref;
use id::Name;
use lazy_regex::{Lazy, Regex};
use lock_direct::{BuildSrc, NixDep, NixGitDep, NixTarDep};
use metadata::lock::{LockError, direct as lock_direct};
use metadata::manifest::{WriteDeps, direct};
use metadata::{DocError, GitDigest, lock};
use package::{GitSpec, metadata};
use semver::{Version, VersionReq};
use snix_castore::blobservice::BlobService;
use snix_castore::directoryservice::DirectoryService;
use snix_glue::fetchers::Fetcher;
use snix_store::nar::SimpleRenderer;
use snix_store::pathinfoservice::PathInfoService;

use crate::{AtomId, BoxError, Manifest, ManifestWriter, id, package};

//================================================================================================
// Statics
//================================================================================================

static SEMVER_REGEX: Lazy<Regex> = lazy_regex::lazy_regex!(
    r#"^v?(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)(?:-(?P<prerelease>(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+(?P<buildmetadata>[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"#
);

//================================================================================================
// Types
//================================================================================================

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

impl ManifestWriter {
    /// Adds a user-requested direct URL to the manifest and lock files, ensuring they remain in
    /// sync.
    pub async fn add_url(
        &mut self,
        url: crate::uri::AliasedUrl,
        key: Option<Name>,
        git: Option<GitSpec>,
        tar: Option<bool>,
        build: bool,
        unpack: Option<bool>,
    ) -> Result<(), DocError> {
        let dep = NixFetch::determine(url, git, tar, build, unpack)?;
        let (key, lock_entry) = self.resolve_nix(dep.to_owned(), key.as_ref()).await?;

        dep.write_dep(key.to_owned(), self.doc_mut())?;
        self.insert_or_update_and_log(Either::Right(key), &lock_entry);
        Ok(())
    }

    pub(super) async fn synchronize_direct(&mut self, manifest: &Manifest) -> Result<(), DocError> {
        for (name, dep) in manifest.deps().direct().nix() {
            tracing::debug!(direct.nix.name = %name,  "checking sync status");
            let key = Either::Right(name.to_owned());
            let locked = self.lock.deps.as_ref().get(&key);
            if let Some(lock) = locked {
                use lock::direct::NixUrls;

                let url = dep.get_url();
                let mut unmatched = false;
                match (lock, url, &dep.kind) {
                    (lock::Dep::Nix(nix), NixUrls::Url(url), _) => unmatched = nix.url() != url,
                    (
                        lock::Dep::NixGit(git),
                        NixUrls::Git(url),
                        NixReq::Git(NixGit { spec, .. }),
                    ) => {
                        // upstream bug: false positive (it is read later unconditionally)
                        #[allow(unused_assignments)]
                        if let (Some(GitSpec::Version(req)), Some(version)) = (spec, &git.version) {
                            unmatched = !req.matches(version);
                        }
                        unmatched = git.url() != url;
                    },
                    (lock::Dep::NixTar(tar), NixUrls::Url(url), _) => unmatched = tar.url() != url,
                    (lock::Dep::NixSrc(build), NixUrls::Url(url), _) => {
                        unmatched = build.url() != url
                    },
                    _ => {},
                }
                if unmatched {
                    tracing::warn!(message = "locked URL doesn't match, updating...", direct.nix = %name);
                    let (_, dep) = self.resolve_nix(dep.to_owned(), Some(name)).await?;
                    self.lock.deps.as_mut().insert(key, dep);
                }
            } else if let Ok((_, dep)) = self.resolve_nix(dep.to_owned(), Some(name)).await {
                self.lock.deps.as_mut().insert(key, dep);
            } else {
                tracing::warn!(message = Self::RESOLUTION_ERR_MSG, direct.nix = %name);
            }
        }
        Ok(())
    }

    pub(super) async fn resolve_nix(
        &self,
        dep: NixFetch,
        key: Option<&Name>,
    ) -> Result<(Name, lock::Dep), DocError> {
        let get_dep = || {
            if let Some((set, atom)) = &dep.from_version {
                if let Some(root) = self.resolved.roots.get(&Either::Left(set.to_owned())) {
                    if let Ok(id) = AtomId::construct(root, atom.to_owned()) {
                        if let Some(lock::Dep::Atom(atom)) =
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
}

impl NixFetch {
    pub(crate) async fn get_fetcher() -> Result<NixFetcher, BoxError> {
        use snix_castore::{blobservice, directoryservice};
        use snix_glue::fetchers::Fetcher;
        use snix_store::nar::SimpleRenderer;
        use snix_store::pathinfoservice;
        let cache_root = config::CONFIG.cache.root.to_owned();

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

    pub(crate) async fn resolve(&self, key: Option<&Name>) -> Result<(Name, lock::Dep), BoxError> {
        use lock::direct::WrappedNixHash;
        use snix_glue::fetchers::Fetch;

        let key = if let Some(key) = key {
            key
        } else {
            let url = self.get_url();
            &Name::try_from(super::get_url_filename(&url))?
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
                    lock::Dep::Nix(NixDep::new(
                        key.to_owned(),
                        url.to_owned(),
                        WrappedNixHash(hash),
                    )),
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
                    lock::Dep::NixTar(NixTarDep::new(
                        key.to_owned(),
                        url.to_owned(),
                        WrappedNixHash(hash),
                    )),
                ))
            },
            NixReq::Git(nix_git) => {
                return Ok((
                    key.to_owned(),
                    lock::Dep::NixGit(nix_git.resolve(key).await?),
                ));
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
                    lock::Dep::NixSrc(BuildSrc::new(
                        key.to_owned(),
                        build_src.build.to_owned(),
                        WrappedNixHash(hash),
                    )),
                ))
            },
        }
    }
}

impl NixGit {
    /// Resolves a Git-based Nix dependency to a concrete version and commit hash.
    ///
    /// This function handles the complex version resolution logic for Git dependencies,
    /// supporting different specification types: version requirements, specific references,
    /// or default HEAD resolution. It queries the remote Git repository to find the
    /// appropriate commit that satisfies the version constraints.
    ///
    /// # Parameters
    ///
    /// - `key`: The name/identifier for this dependency in the lockfile
    ///
    /// # Returns
    ///
    /// Returns `Ok(NixGitDep)` containing the resolved dependency with version and commit hash,
    /// or a `LockError` if resolution fails.
    ///
    /// # Algorithm
    ///
    /// 1. **Specification Branching**:
    ///    - **Version Requirement**: Query refs/tags/*, parse semver, find highest match
    ///    - **Specific Reference**: Query specific ref path, get exact commit
    ///    - **HEAD Default**: Query HEAD ref for latest commit
    ///
    /// 2. **Git Operations**:
    ///    - Query repository references using appropriate patterns
    ///    - Extract commit hash from reference result
    ///    - Convert to GitDigest format
    ///
    /// 3. **Result Construction**: Build NixGitDep with resolved version and commit
    ///
    /// # Edge Cases
    ///
    /// - **No Matching Version**: Version requirement matches no available tags
    /// - **Invalid Reference**: Specified ref doesn't exist in repository
    /// - **Network/Access Issues**: Git operations fail due to connectivity or permissions
    /// - **Malformed Tags**: Tag names don't follow semver format
    ///
    /// # Assumptions
    ///
    /// - Git repository is accessible and properly configured
    /// - Version tags follow semver conventions when version requirements are used
    /// - References are valid Git references (branches, tags, commits)
    ///
    /// # Integration
    ///
    /// Called during Nix dependency resolution when a Git-based dependency needs to be
    /// locked to a specific commit. The result ensures reproducible builds by recording
    /// exact commit hashes rather than mutable references.
    async fn resolve(&self, key: &Name) -> Result<NixGitDep, LockError> {
        use crate::storage::QueryStore;

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
        let ObjectId::Sha1(id) = crate::storage::git::to_id(r);

        Ok(NixGitDep {
            name: key.to_owned(),
            url: self.git.to_owned(),
            rev: GitDigest::Sha1(id),
            version,
        })
    }

    /// Finds the highest version from Git references that satisfies a version requirement.
    ///
    /// This function implements semantic version matching against Git references (typically tags).
    /// It parses version strings from reference names, filters for those matching the requirement,
    /// and returns the highest matching version along with its reference.
    ///
    /// # Parameters
    ///
    /// - `req`: The version requirement to match against (e.g., "^1.0.0", ">=2.0.0")
    /// - `refs`: Iterator over Git references to search through
    ///
    /// # Returns
    ///
    /// Returns `Some((Version, Ref))` containing the highest matching version and its reference,
    /// or `None` if no references match the requirement.
    ///
    /// # Algorithm
    ///
    /// 1. **Reference Processing**: For each reference:
    ///    - Extract the reference name/path
    ///    - Attempt to parse it as a semantic version string
    ///    - Skip references that don't parse as valid semver
    ///
    /// 2. **Requirement Filtering**: Keep only versions that satisfy the requirement
    ///
    /// 3. **Maximum Selection**: Find the highest version among matches using semver ordering
    ///
    /// # Edge Cases
    ///
    /// - **No Valid Semver**: References with non-semver names are ignored
    /// - **No Matches**: Returns None if no versions satisfy the requirement
    /// - **Multiple Matches**: Returns the highest version (semver precedence)
    /// - **Malformed Refs**: Invalid UTF-8 reference names are skipped
    ///
    /// # Assumptions
    ///
    /// - Reference names contain version information in semver-compatible format
    /// - Version requirement is valid and parseable
    /// - Semver ordering correctly identifies "highest" version
    ///
    /// # Integration
    ///
    /// Used during Git dependency resolution when a version requirement needs to be
    /// matched against available tagged versions in a repository. Ensures that
    /// the latest compatible version is selected for reproducible builds.
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

//================================================================================================
// Functions
//================================================================================================

/// Extracts and parses a semantic version from a string input.
///
/// This function uses a regular expression to identify semantic version patterns
/// within strings (typically Git reference names like tags) and constructs
/// valid semver Version objects from the captured components.
///
/// # Parameters
///
/// - `input`: The string to parse for semantic version information
///
/// # Returns
///
/// Returns `Some(Version)` if a valid semver is found and parsed successfully,
/// or `None` if no semver pattern is found or parsing fails.
///
/// # Algorithm
///
/// 1. **Regex Matching**: Apply SEMVER_REGEX to extract version components:
///    - Major, minor, patch numbers
///    - Optional prerelease identifiers
///    - Optional build metadata
///
/// 2. **String Construction**: Build a properly formatted semver string from captures
///
/// 3. **Version Parsing**: Use semver::Version::parse to validate and create Version object
///
/// # Edge Cases
///
/// - **No Match**: Input doesn't contain semver-like pattern
/// - **Invalid Components**: Captured groups don't form valid semver
/// - **Partial Match**: Regex matches but parsing fails (e.g., invalid numbers)
/// - **Complex Prerelease**: Handles complex prerelease identifiers with dots
///
/// # Assumptions
///
/// - Input strings may contain version information mixed with other text
/// - Version components follow semver specification
/// - Regex pattern correctly identifies semver-compatible strings
///
/// # Integration
///
/// Used as a helper during Git reference processing to convert tag names
/// and reference paths into structured version objects for comparison and matching.
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
