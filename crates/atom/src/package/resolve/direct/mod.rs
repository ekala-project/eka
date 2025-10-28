use std::sync::Arc;

use bstr::ByteSlice;
use either::Either;
use gix::protocol::handshake::Ref;
use id::Name;
use lazy_regex::{Lazy, Regex};
use metadata::lock::{BuildSrc, LockError, NixDep, NixGitDep, NixTarDep};
use metadata::manifest::{NixFetch, NixGit, NixReq, WriteDeps};
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
                use lock::NixUrls;

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

    pub(crate) async fn resolve(&self, key: Option<&Name>) -> Result<(Name, lock::Dep), BoxError> {
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
                        lock::WrappedNixHash(hash),
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
                        lock::WrappedNixHash(hash),
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
                        lock::WrappedNixHash(hash),
                    )),
                ))
            },
        }
    }
}

impl NixGit {
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
