use std::collections::{BTreeSet, HashMap};

use gix::ThreadSafeRepository;
use gix::protocol::transport::client::Transport;
use tokio::task::JoinSet;

use crate::Manifest;
use crate::id::Name;
use crate::lock::BoxError;
use crate::store::UnpackedRef;
use crate::store::git::{AtomQuery, Root};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("manifest is in an inconsistent state")]
    Inconsistent,
}

pub struct ResolvedSets {
    pub atom_sets: HashMap<Name, BTreeSet<AtomQuery>>,
    pub names: HashMap<Root, Name>,
    pub transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
}

pub(crate) struct SetResolver<'a> {
    manifest: &'a Manifest,
    repo: gix::Repository,
    names: HashMap<Root, Name>,
    roots: HashMap<Name, Root>,
    tasks: JoinSet<MirrorResult>,
    atom_sets: HashMap<Name, BTreeSet<AtomQuery>>,
    transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
}

type MirrorResult = Result<
    (
        Option<Box<dyn Transport + Send>>,
        <Vec<AtomQuery> as IntoIterator>::IntoIter,
        Root,
        Name,
        gix::Url,
    ),
    BoxError,
>;

impl<'a> SetResolver<'a> {
    /// Creates a new `SetResolver` to validate the package sets in a manifest.
    pub(crate) fn new(repo: &ThreadSafeRepository, manifest: &'a Manifest) -> Self {
        let len = manifest.package.sets.len();
        Self {
            manifest,
            repo: repo.to_thread_local(),
            names: HashMap::with_capacity(len),
            roots: HashMap::with_capacity(len),
            tasks: JoinSet::new(),
            atom_sets: HashMap::with_capacity(manifest.package.sets.len()),
            transports: HashMap::with_capacity(len * 3),
        }
    }

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
    pub(crate) async fn get_and_check_sets(mut self) -> Result<ResolvedSets, BoxError> {
        use crate::manifest::AtomSets;

        for (k, v) in self.manifest.package.sets.iter() {
            match v {
                AtomSets::Singleton(mirror) => self.process_mirror(k, mirror)?,
                AtomSets::Mirrors(mirrors) => {
                    for m in mirrors.iter() {
                        self.process_mirror(k, m)?
                    }
                },
            }
        }

        while let Some(res) = self.tasks.join_next().await {
            self.process_remote_mirror_result(res?)?;
        }

        Ok(ResolvedSets {
            atom_sets: self.atom_sets,
            names: self.names,
            transports: self.transports,
        })
    }

    /// Processes a single mirror, either local or remote, and initiates consistency checks.
    ///
    /// For local mirrors, it calculates the root hash directly. For remote mirrors,
    /// it spawns an asynchronous task to fetch repository data and perform checks.
    fn process_mirror(
        &mut self,
        k: &'a Name,
        mirror: &'a crate::manifest::AtomSet,
    ) -> Result<(), BoxError> {
        use crate::id::Origin;
        use crate::manifest::AtomSet;
        use crate::store::{QueryStore, QueryVersion};

        match mirror {
            AtomSet::Local => {
                let root = {
                    let commit = self
                        .repo
                        .rev_parse_single("HEAD")
                        .map(|s| self.repo.find_commit(s))
                        .map_err(Box::new)??;
                    commit.calculate_origin()?
                };
                self.check_set_consistency(k, root)?;
                Ok(())
            },
            AtomSet::Url(url) => {
                let url = url.to_owned();
                let k = k.to_owned();
                self.tasks.spawn(async move {
                    let mut transport = url.get_transport().ok();
                    let atoms = url.get_atoms(transport.as_mut())?;
                    let root = atoms.calculate_origin()?;
                    Ok((transport, atoms, root, k, url))
                });
                Ok(())
            },
        }
    }

    /// Handles the result of an asynchronous remote mirror check.
    ///
    /// This function processes the data fetched from a remote mirror, performs
    /// consistency checks, and aggregates the results into the provided hashmaps.
    fn process_remote_mirror_result(&mut self, result: MirrorResult) -> Result<(), BoxError> {
        let (transport, atoms, root, k, url) = result?;
        self.check_set_consistency(&k, root)?;
        if let Some(t) = transport {
            self.transports.insert(url.to_owned(), t);
        }

        for atom in atoms {
            self.check_and_insert_atom(atom, &k, &url)?;
        }

        Ok(())
    }

    /// Verifies the consistency of a single atom against the existing set of resolved atoms.
    ///
    /// This check ensures that if an atom is advertised by multiple mirrors, it always
    /// has the same revision for the same version.
    fn check_and_insert_atom(
        &mut self,
        atom @ UnpackedRef(_, _, rev): AtomQuery,
        set_name: &Name,
        mirror_url: &gix::Url,
    ) -> Result<(), BoxError> {
        let existing_atoms = self.atom_sets.entry(set_name.to_owned()).or_default();

        if let Some(UnpackedRef(.., r)) = existing_atoms.get(&atom) {
            if r != &rev {
                tracing::error!(
                    message = "mirrors for the same set are advertising an atom at \
                               the same version but different revisions. This could \
                               be the result of possible tampering. Remove the faulty \
                               mirror to continue.",
                    checking.url = %mirror_url.to_string(),
                    atom.tag = %atom.0,
                    atom.version = %atom.1
                );
                return Err(Error::Inconsistent.into());
            }
        } else {
            existing_atoms.insert(atom);
        };
        Ok(())
    }

    /// Ensures that a given package set is consistent across all its mirrors.
    ///
    /// This check verifies two conditions:
    /// 1. A repository root hash is not associated with more than one package set name.
    /// 2. A package set name is not associated with more than one repository root hash.
    fn check_set_consistency(&mut self, k: &Name, root: Root) -> Result<(), BoxError> {
        let prev = self.names.insert(root, k.to_owned());
        if prev.is_some() && prev.as_ref() != Some(k) {
            tracing::error!(
                message = "a repository exists in more than one mirror set",
                set.a = %k,
                set.b = ?prev,
                set.hash = %*root
            );
            return Err(Error::Inconsistent.into());
        }
        let prev = self.roots.insert(k.to_owned(), root);
        if prev.is_some() && prev.as_ref() != Some(&root) {
            tracing::error!(
                message = "the mirrors for this set do not all point at the same set",
                set.name = %k,
                set.a = %*root,
                set.b = ?prev,
            );
            return Err(Error::Inconsistent.into());
        }
        Ok(())
    }
}
