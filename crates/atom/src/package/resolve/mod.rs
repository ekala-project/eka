use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use either::Either;
use gix::Repository;
use id::{Name, Origin, Tag};
use lock::{AtomDep, NixUrls, SetDetails};
use metadata::manifest::{AtomReq, AtomWriter, SetMirror, WriteDeps};
use metadata::{DocError, GitDigest, lock};
use semver::{Prerelease, VersionReq};
use sets::{MirrorResult, ResolvedAtom, ResolvedSets, SetResolver};
use storage::UnpackedRef;
use storage::git::{AtomQuery, Root};
use uri::Uri;

use super::{Manifest, metadata, sets};
use crate::{ATOM_MANIFEST_NAME, AtomId, BoxError, ManifestWriter, id, storage, uri};

mod direct;

//================================================================================================
// Impls
//================================================================================================

impl ResolvedSets {
    pub(super) fn resolve_atom(
        &self,
        id: &AtomId<Root>,
        req: &VersionReq,
    ) -> Result<AtomDep, DocError> {
        use crate::storage::git;
        let versions = self
            .atoms
            .get(id)
            .ok_or(DocError::Git(Box::new(git::Error::NoMatchingVersion)))?;
        if let Some((_, atom)) = versions
            .iter()
            .filter(|(v, _)| req.matches(v))
            .max_by_key(|(ref version, _)| version.to_owned())
        {
            Ok(AtomDep::from(atom.to_owned()))
        } else {
            Err(Box::new(git::Error::NoMatchingVersion).into())
        }
    }
}

impl<'a> SetResolver<'a> {
    /// Verifies the integrity of declared package sets and collects atom references.
    ///
    /// This function consumes the resolver and performs several critical checks to
    /// ensure the consistency and integrity of the package sets defined in the manifest:
    ///
    /// 1. **Root Consistency**: It ensures that every URL within a named mirror set points to the
    ///    same underlying repository by verifying their advertised root hashes.
    /// 2. **Set Uniqueness**: It guarantees that a given repository URL does not belong to more
    ///    than one mirror set, preventing ambiguity.
    /// 3. **Version and Revision Coherency**: It aggregates all atoms from each mirror, ensuring
    ///    that no two mirrors advertise the same atom version with a different Git revision, which
    ///    could indicate tampering or misconfiguration.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `ResolvedSets` struct on success, which holds the aggregated
    /// results of the validation process.
    ///
    /// # Errors
    ///
    /// Returns a `BoxError` if any of the following conditions are met:
    /// - A repository is found in more than one mirror set.
    /// - The mirrors for a given set do not all point to the same root hash.
    /// - An atom is advertised with the same version but different revisions across mirrors.
    pub(super) async fn get_and_check_sets(mut self) -> Result<ResolvedSets, BoxError> {
        use super::metadata::manifest::AtomSet;

        for (set_tag, set) in self.manifest.package().sets().iter() {
            match set {
                AtomSet::Singleton(mirror) => self.process_mirror(set_tag, mirror)?,
                AtomSet::Mirrors(mirrors) => {
                    for m in mirrors.iter() {
                        self.process_mirror(set_tag, m)?
                    }
                },
            }
        }

        while let Some(res) = self.tasks.join_next().await {
            self.process_remote_mirror_result(res?)?;
        }

        Ok(ResolvedSets {
            atoms: self.atoms,
            ekala: self.ekala,
            transports: self.transports,
            roots: self.roots,
            details: self.sets,
            repo: self.repo,
        })
    }

    /// Processes a single mirror, either local or remote, and initiates consistency checks.
    ///
    /// For local mirrors, it calculates the root hash directly. For remote mirrors,
    /// it spawns an asynchronous task to fetch repository data and perform checks.
    fn process_mirror(&mut self, set_tag: &'a Tag, mirror: &'a SetMirror) -> Result<(), BoxError> {
        use crate::id::Origin;
        use crate::storage::{QueryStore, QueryVersion};

        match mirror {
            SetMirror::Local => {
                if let Some(repo) = self.repo.as_ref() {
                    let root = {
                        let commit = repo
                            .rev_parse_single("HEAD")
                            .map(|s| repo.find_commit(s))
                            .map_err(Box::new)??;
                        commit.calculate_origin()?
                    };
                    self.check_set_consistency(set_tag, root, &SetMirror::Local)?;
                    self.update_sets(set_tag, root, SetMirror::Local);
                } else {
                    return Err(sets::Error::NoLocal.into());
                }
                Ok(())
            },
            SetMirror::Url(url) => {
                let url = url.to_owned();
                let set_name = set_tag.to_owned();
                self.tasks.spawn(async move {
                    let mut transport = url.get_transport().ok();
                    let atoms = url.get_atoms(transport.as_mut())?;
                    let root = atoms.calculate_origin().inspect_err(|_| {
                        tracing::warn!(
                            set.tag = %set_name,
                            set.mirror = %url,
                            "remote advertised no atoms in:"
                        )
                    })?;
                    Ok((transport, atoms, root, set_name, url))
                });
                Ok(())
            },
        }
    }

    fn update_sets(&mut self, name: &Tag, root: Root, set: SetMirror) {
        let digest = GitDigest::from(*root);
        self.sets
            .entry(digest)
            .and_modify(|e| {
                e.mirrors.insert(set.to_owned());
            })
            .or_insert(SetDetails {
                tag: name.to_owned(),
                mirrors: BTreeSet::from([set]),
            });
    }

    /// Handles the result of an asynchronous remote mirror check.
    ///
    /// This function processes the data fetched from a remote mirror, performs
    /// consistency checks, and aggregates the results into the provided hashmaps.
    fn process_remote_mirror_result(&mut self, result: MirrorResult) -> Result<(), BoxError> {
        let (transport, atoms, root, set_name, url) = result?;
        let mirror = SetMirror::Url(url.to_owned());
        self.check_set_consistency(&set_name, root, &mirror)?;
        self.update_sets(&set_name, root, SetMirror::Url(url.to_owned()));
        if let Some(t) = transport {
            self.transports.insert(url.to_owned(), t);
        }

        let cap = self.atoms.capacity();
        let len = atoms.len();
        if cap < len {
            self.atoms.reserve(len - cap);
        }
        for atom in atoms {
            self.check_and_insert_atom(atom, len, &url)?;
        }

        Ok(())
    }

    /// Verifies the consistency of a single atom against the existing set of resolved atoms.
    ///
    /// This check ensures that if an atom is advertised by multiple mirrors, it always
    /// has the same revision for the same version.
    fn check_and_insert_atom(
        &mut self,
        atom: AtomQuery,
        size: usize,
        mirror_url: &gix::Url,
    ) -> Result<(), BoxError> {
        use std::collections::hash_map::Entry;
        let entry = self
            .atoms
            .entry(atom.id.to_owned())
            .or_insert(HashMap::with_capacity(size));
        match entry.entry(atom.version.to_owned()) {
            Entry::Occupied(mut entry) => {
                let existing = entry.get();
                if existing.unpacked.rev == atom.rev {
                    entry.get_mut().remotes.insert(mirror_url.to_owned());
                } else {
                    let existing_mirrors: Vec<_> =
                        existing.remotes.iter().map(|url| url.to_string()).collect();
                    tracing::error!(
                        message = "mirrors for the same set are advertising an atom at \
                                   the same version but different revisions. This could \
                                   be the result of possible tampering. Remove the faulty \
                                   mirror to continue.",
                        existing.mirrors = %toml_edit::ser::to_string(&existing_mirrors)?,
                        existing.rev = %existing.unpacked.rev,
                        conflicting.url = %mirror_url.to_string(),
                        conflicting.label = %atom.id,
                        conflicting.version = %atom.version,
                        conflicting.rev = %atom.rev,
                    );
                    return Err(sets::Error::Inconsistent.into());
                }
            },
            Entry::Vacant(entry) => {
                entry.insert(ResolvedAtom::new(
                    atom,
                    BTreeSet::from([mirror_url.to_owned()]),
                ));
            },
        }

        Ok(())
    }

    /// Ensures that a given package set is consistent across all its mirrors.
    ///
    /// This check verifies two conditions:
    /// 1. A repository root hash is not associated with more than one package set name.
    /// 2. A package set name is not associated with more than one repository root hash.
    fn check_set_consistency(
        &mut self,
        set_tag: &Tag,
        root: Root,
        mirror: &SetMirror,
    ) -> Result<(), BoxError> {
        let prev = self.names.insert(root, set_tag.to_owned());
        if let Some(prev_tag) = &prev {
            if prev_tag != set_tag {
                tracing::error!(
                    message = "the same mirror exists in more than one set",
                    set.mirror = %mirror,
                    set.conflict.a = %set_tag,
                    set.conflict.b = %prev_tag,
                );
                return Err(sets::Error::Inconsistent.into());
            }
        }
        let prev = self.roots.insert(Either::Left(set_tag.to_owned()), root);
        if let Some(prev) = &prev {
            if prev != &root {
                tracing::error!(
                    message = "the mirrors in this set do not all point at the same set",
                    set.name = %set_tag,
                    set.mirror = %mirror,
                    set.root.mirror = %*root,
                    set.root.previous = %**prev,
                );
                return Err(sets::Error::Inconsistent.into());
            }
        }
        self.roots.insert(Either::Right(mirror.to_owned()), root);
        Ok(())
    }
}

impl ManifestWriter {
    /// Adds a user-requested atom URI to the manifest and lock files, ensuring they remain in sync.
    pub fn add_uri(&mut self, uri: Uri, set_tag: Option<Tag>) -> Result<(), storage::git::Error> {
        let mirror = if let Some(url) = uri.url() {
            SetMirror::Url(url.to_owned())
        } else {
            SetMirror::Local
        };
        let (atom_req, lock_entry) = self.resolve_uri(&uri, &mirror)?;

        let label = lock_entry.label().to_owned();
        let id = AtomId::from(&lock_entry);
        let set_tag = self.get_set_tag(&lock_entry, &uri, set_tag);

        let atom_writer = AtomWriter::new(set_tag.to_owned(), atom_req, mirror.to_owned());

        let set = lock_entry.set().to_owned();
        atom_writer.write_dep(label, self.doc_mut())?;
        self.insert_or_update_and_log(Either::Left(id.to_owned()), &lock::Dep::Atom(lock_entry));

        self.update_lock_set(set, mirror, set_tag);

        Ok(())
    }

    /// Atomically writes the changes to the manifest and lock files on disk.
    /// This method should be called last, after all changes have been processed.
    ///
    /// To enforce this, the writer instance will be consumed and dropped after calling this method.
    pub fn write_atomic(mut self) -> Result<(), DocError> {
        use std::io::Write;

        use tempfile::NamedTempFile;

        let doc_str = self.doc_mut().as_mut().to_string();

        let _validate: Manifest = toml_edit::de::from_str(&doc_str)?;
        let dir = self
            .path()
            .parent()
            .ok_or_else(|| DocError::Missing(self.path().to_owned()))?;
        let lock_path = self.path().with_file_name(crate::LOCK_NAME.as_str());
        let mut tmp =
            NamedTempFile::with_prefix_in(format!(".{}", crate::ATOM_MANIFEST_NAME.as_str()), dir)?;
        let mut tmp_lock =
            NamedTempFile::with_prefix_in(format!(".{}", crate::LOCK_NAME.as_str()), dir)?;
        tmp.write_all(doc_str.as_bytes())?;
        tmp_lock.write_all(
            "# This file is automatically @generated by eka.\n# It is not intended for manual \
             editing.\n"
                .as_bytes(),
        )?;
        tmp_lock.write_all(toml_edit::ser::to_string_pretty(self.lock())?.as_bytes())?;
        tmp.persist(self.path())?;
        tmp_lock.persist(lock_path)?;
        Ok(())
    }

    /// Removes any dependencies from the lockfile that are no longer present in the
    /// manifest, ensuring the lockfile only contains entries that are still relevant,
    /// then calls into synchronization logic to ensure consistency.
    pub(super) fn sanitize(&mut self, manifest: &Manifest) {
        self.lock.deps.as_mut().retain(|_, dep| match dep {
            lock::Dep::Atom(atom_dep) => {
                if let Some(SetDetails { tag: name, .. }) = self.lock.sets.get(&atom_dep.set()) {
                    if let Some(set) = manifest.deps().from().get(name) {
                        return set.contains_key(atom_dep.label())
                            && (atom_dep.version().pre.is_empty()
                                || self
                                    .resolved
                                    .ekala
                                    .manifest
                                    .set
                                    .packages
                                    .as_ref()
                                    .contains_key(atom_dep.label()));
                    } else {
                        false
                    };
                }
                false
            },
            lock::Dep::Nix(nix) => manifest.deps().direct().nix().contains_key(nix.name()),
            lock::Dep::NixGit(nix_git) => {
                manifest.deps().direct().nix().contains_key(&nix_git.name)
            },
            lock::Dep::NixTar(nix_tar) => {
                manifest.deps().direct().nix().contains_key(&nix_tar.name)
            },
            lock::Dep::NixSrc(build_src) => {
                manifest.deps().direct().nix().contains_key(&build_src.name)
            },
        });
    }

    /// Updates the lockfile to match the dependencies specified in the manifest.
    /// It resolves any new dependencies, updates existing ones if their version
    /// requirements have changed, and ensures the lockfile is fully consistent.
    pub(super) async fn synchronize(&mut self, manifest: Manifest) -> Result<(), DocError> {
        self.synchronize_atoms(&manifest).await?;
        self.synchronize_direct(&manifest).await?;
        Ok(())
    }

    async fn synchronize_atoms(&mut self, manifest: &Manifest) -> Result<(), DocError> {
        for (set_tag, set) in manifest.deps().from() {
            let maybe_root = self
                .resolved
                .roots()
                .get(&Either::Left(set_tag.to_owned()))
                .map(ToOwned::to_owned);
            if let Some(root) = maybe_root {
                for (label, req) in set {
                    tracing::debug!(
                        atom.label = %label,
                        atom.specified = %req,
                        set = %set_tag,
                        "checking sync status"
                    );
                    let id = AtomId::construct(&root, label.to_owned()).map_err(|e| {
                        DocError::AtomIdConstruct(format!(
                            "set: {}, atom: {}, err: {}",
                            &set_tag, &label, e
                        ))
                    })?;
                    self.synchronize_atom(req.to_owned(), id.to_owned(), set_tag.to_owned())
                        .map_err(|error| {
                            tracing::error!(
                                atom.label = %label,
                                atom.requested = %req,
                                set = %set_tag,
                                %error, "lock synchronization failed"
                            );
                            DocError::SyncFailed
                        })?;
                }
            } else {
                tracing::warn!(
                    message = "set was not resolved to an origin id, can't syncrhonize it",
                    set = %set_tag,
                );
            }
        }
        Ok(())
    }

    fn synchronize_atom(
        &mut self,
        req: VersionReq,
        id: AtomId<Root>,
        set_tag: Tag,
    ) -> Result<(), crate::storage::git::Error> {
        if !self
            .lock
            .deps
            .as_ref()
            .contains_key(&either::Either::Left(id.to_owned()))
        {
            self.lock_atom(req, id, set_tag)?;
        } else if let Some(lock::Dep::Atom(dep)) = self
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

    fn lock_atom(
        &mut self,
        req: VersionReq,
        id: AtomId<Root>,
        set_tag: Tag,
    ) -> Result<lock::Dep, crate::storage::git::Error> {
        if let Ok(dep) = self.resolved.resolve_atom(&id, &req) {
            let dep = lock::Dep::Atom(dep);
            self.insert_or_update_and_log(Either::Left(id), &dep);
            Ok(dep)
        } else if let Some(repo) = &self.resolved.repo {
            let uri = Uri::from((id.label().to_owned(), Some(req)));
            let (_, dep) = self.resolve_from_local(&uri, repo)?;
            let dep = lock::Dep::Atom(dep);
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
            Err(DocError::Error(Box::new(crate::storage::git::Error::NoMatchingVersion)).into())
        }
    }

    fn resolve_from_uri(
        &self,
        uri: &Uri,
        root: &Root,
    ) -> Result<(AtomReq, AtomDep), crate::storage::git::Error> {
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
    ) -> Result<(AtomReq, AtomDep), crate::storage::git::Error> {
        use sets;

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
            if atom.label() != uri.label() {
                return Err(DocError::SetError(sets::Error::Inconsistent).into());
            }
            let req = AtomReq::new(
                uri.version()
                    .unwrap_or(&VersionReq::parse(atom.version().to_string().as_str())?)
                    .to_owned(),
            );
            let id = AtomId::construct(&root, uri.label().to_owned()).expect(Self::ATOM_BUG);
            let mut version = atom.version().clone();
            version.pre = Prerelease::new("local")?;
            let unpacked = UnpackedRef {
                id,
                version,
                rev: None,
            };
            let dep = AtomDep::from(ResolvedAtom {
                unpacked,
                remotes: BTreeSet::new(),
            });
            Ok((req, dep))
        }
    }

    fn resolve_uri(
        &mut self,
        uri: &Uri,
        mirror: &SetMirror,
    ) -> Result<(AtomReq, AtomDep), crate::storage::git::Error> {
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

    fn insert_or_update_and_log(&mut self, key: Either<AtomId<Root>, Name>, dep: &lock::Dep) {
        if self
            .lock
            .deps
            .as_mut()
            .insert(key, dep.to_owned())
            .is_some()
        {
            match &dep {
                lock::Dep::Atom(dep) => {
                    let tag = self.resolved.details().get(&dep.set()).map(|d| &d.tag);
                    tracing::warn!(
                        message = Self::UPDATE_DEPENDENCY,
                        label = %dep.label(),
                        set = ?tag,
                        r#type = "atom"
                    );
                },
                lock::Dep::Nix(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                lock::Dep::NixGit(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                lock::Dep::NixTar(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
                lock::Dep::NixSrc(dep) => {
                    tracing::warn!(message = "updating lock entry", direct.nix = %dep.name())
                },
            }
        }
    }
}

//================================================================================================
// Functions
//================================================================================================

pub(crate) fn url_filename_as_tag(url: &gix::Url) -> Result<Tag, crate::id::Error> {
    let str = get_url_filename(&NixUrls::Git(url));
    Tag::try_from(str)
}

/// Extracts a filename from a URL, suitable for use as a dependency name.
fn get_url_filename(url: &NixUrls) -> String {
    match url {
        NixUrls::Url(url) => {
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
        NixUrls::Git(url) => {
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
