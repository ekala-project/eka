use std::collections::{BTreeSet, HashMap};

use either::Either;
use id::Tag;
use metadata::lock::{AtomDep, SetDetails};
use metadata::manifest::SetMirror;
use metadata::{DocError, GitDigest};
use semver::VersionReq;
use sets::{MirrorResult, ResolvedAtom, ResolvedSets, SetResolver};
use storage::git::{AtomQuery, Root};

use super::{metadata, sets};
use crate::{AtomId, BoxError, id, storage};

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
                entry.insert(ResolvedAtom {
                    unpacked: atom,
                    remotes: BTreeSet::from([mirror_url.to_owned()]),
                });
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
