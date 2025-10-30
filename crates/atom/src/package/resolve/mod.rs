//! # Dependency Resolution
//!
//! This module handles the resolution of atom dependencies from manifests into
//! concrete, locked versions that can be reliably fetched and used.
//!
//! ## Overview
//!
//! The resolution process involves:
//! 1. **Manifest Parsing** - Reading and validating atom manifests
//! 2. **Set Resolution** - Resolving package sets and their mirrors
//! 3. **Version Selection** - Finding highest matching versions for requirements
//! 4. **Lockfile Generation** - Creating reproducible lockfiles
//!
//! ## Key Components
//!
//! - [`SetResolver`] - Resolves package sets and validates mirrors
//! - [`ResolvedSets`] - Contains resolved atoms and their metadata
//! - [`Uri`] - Handles atom URI parsing and resolution
//!
//! ## Resolution Process
//!
//! 1. **Set Validation** - Ensure all package sets have consistent roots
//! 2. **Atom Discovery** - Query remotes for available atoms
//! 3. **Version Matching** - Find highest versions satisfying constraints
//! 4. **Dependency Locking** - Record exact versions and hashes
//!
//! ## Error Handling
//!
//! Resolution can fail due to:
//! - Inconsistent mirror configurations
//! - Missing atoms or versions
//! - Network connectivity issues
//! - Invalid manifest specifications

use std::collections::{BTreeSet, HashMap};
use std::ops::Deref;
use std::path::PathBuf;

use either::Either;
use gix::protocol::transport::client::Transport;
use gix::{ObjectId, Repository};
use id::{Name, Origin, Tag};
use lock::direct::NixUrls;
use lock::{AtomDep, SetDetails};
use metadata::manifest::{AtomReq, AtomWriter, SetMirror, WriteDeps};
use metadata::{DocError, GitDigest, lock};
use semver::{Prerelease, VersionReq};
use sets::{MirrorResult, ResolvedAtom, ResolvedSets, SetResolver};
use storage::UnpackedRef;
use storage::git::{AtomQuery, Root};
use uri::Uri;

use super::{ValidManifest, metadata, sets};
use crate::storage::QueryVersion;
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
    /// This function handles the initial processing of a package set mirror during resolution.
    /// It determines whether the mirror is local or remote and takes appropriate action:
    ///
    /// - **Local mirrors**: Calculates the root hash directly from the current repository's HEAD
    ///   commit and performs immediate consistency checks against existing resolved sets.
    /// - **Remote mirrors**: Spawns an asynchronous task to fetch repository metadata, discover
    ///   available atoms, and perform consistency validation in the background.
    ///
    /// # Parameters
    ///
    /// - `set_tag`: The tag/name identifying the package set this mirror belongs to
    /// - `mirror`: The mirror configuration (either local repository or remote URL)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `BoxError` if:
    /// - Local repository access fails
    /// - Remote mirror URL parsing fails
    /// - Consistency checks fail for local mirrors
    ///
    /// # Algorithm
    ///
    /// 1. **Local Mirror Processing**:
    ///    - Access the local git repository via `self.repo`
    ///    - Parse HEAD commit and calculate its origin hash
    ///    - Validate set consistency with existing resolved sets
    ///    - Update internal set tracking structures
    ///
    /// 2. **Remote Mirror Processing**:
    ///    - Extract URL from mirror configuration
    ///    - Spawn async task that:
    ///      - Creates transport layer for git protocol
    ///      - Fetches atom metadata from remote repository
    ///      - Calculates repository root hash
    ///      - Returns transport, atoms, root hash, set tag, and URL for later processing
    ///
    /// # Edge Cases
    ///
    /// - **No Local Repository**: Returns error if local mirror is specified but no repo available
    /// - **Remote Transport Failure**: Async task will fail and be handled by
    ///   `process_remote_mirror_result`
    /// - **Empty Remote Repository**: Warns if remote has no atoms but continues processing
    ///
    /// # Integration
    ///
    /// This function is called during the set validation phase of resolution (step 1 in the overall
    /// process). Remote mirror results are collected asynchronously and processed later in
    /// `get_and_check_sets`.
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
                    let (atoms, transport, url) = tokio::task::spawn_blocking(move || {
                        let mut transport = url.get_transport().ok();
                        (url.get_atoms(transport.as_mut()), transport, url)
                    })
                    .await?;
                    let atoms = atoms?;
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
    /// This function processes the outcome of a background task that fetched metadata from a remote
    /// package set mirror. It validates consistency, updates internal tracking structures, and
    /// aggregates discovered atoms into the resolution state.
    ///
    /// # Parameters
    ///
    /// - `result`: A tuple containing:
    ///   - `transport`: Optional transport layer for continued git operations
    ///   - `atoms`: Collection of atom metadata discovered from the remote repository
    ///   - `root`: The calculated root hash of the remote repository
    ///   - `set_name`: The tag/name of the package set this mirror belongs to
    ///   - `url`: The URL of the remote mirror
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `BoxError` if:
    /// - Set consistency validation fails
    /// - Atom insertion encounters conflicts or inconsistencies
    ///
    /// # Algorithm
    ///
    /// 1. **Extract Result Components**: Unpack the transport, atoms, root hash, set name, and URL
    /// 2. **Consistency Validation**: Call `check_set_consistency` to ensure the remote mirror
    ///    doesn't conflict with existing sets
    /// 3. **Set Tracking Update**: Update internal structures to track this mirror's root hash
    /// 4. **Transport Storage**: If transport exists, store it for potential future use
    /// 5. **Atom Processing**: For each discovered atom:
    ///    - Reserve capacity in the atoms map if needed for efficiency
    ///    - Call `check_and_insert_atom` to validate and store the atom
    ///
    /// # Edge Cases
    ///
    /// - **Transport None**: No transport stored if remote fetch didn't require persistent
    ///   connection
    /// - **Empty Atoms Collection**: Function still succeeds but no atoms are processed
    /// - **Root Hash Conflicts**: Validation fails if same repository appears in multiple sets
    /// - **Atom Version Conflicts**: Individual atom insertion may fail but doesn't stop processing
    ///   others
    ///
    /// # Assumptions
    ///
    /// - Remote mirror data is trustworthy (basic validation was done during fetch)
    /// - Transport can be reused for subsequent operations on the same remote
    /// - Atom metadata is correctly formatted and contains valid version/commit information
    ///
    /// # Integration
    ///
    /// This function is called asynchronously during the set resolution phase. Multiple remote
    /// mirrors are processed concurrently, and their results are collected and handled here
    /// to build the complete resolved sets structure.
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
    /// This function ensures data integrity when multiple mirrors advertise the same atom.
    /// It prevents version/revision conflicts that could indicate tampering or misconfiguration
    /// by enforcing that identical atom versions always resolve to identical commit hashes.
    ///
    /// # Parameters
    ///
    /// - `atom`: The atom metadata (id, version, revision) discovered from a mirror
    /// - `size`: Estimated number of atoms in the collection (used for capacity optimization)
    /// - `mirror_url`: The URL of the mirror that advertised this atom
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `BoxError` if:
    /// - An atom version conflict is detected between mirrors
    ///
    /// # Algorithm
    ///
    /// 1. **Atom Map Access**: Get or create an entry in `self.atoms` for this atom's ID
    /// 2. **Capacity Management**: Reserve space in the version map if needed for efficiency
    /// 3. **Version Conflict Detection**:
    ///    - Check if this version already exists for this atom
    ///    - If it exists, compare revisions - they must be identical
    ///    - If different, log detailed error and return inconsistency error
    /// 4. **Successful Insertion**:
    ///    - For new versions: Create ResolvedAtom with this mirror as the first remote
    ///    - For existing versions: Add this mirror to the existing atom's remote set
    ///
    /// # Edge Cases
    ///
    /// - **First Mirror**: Atom/version doesn't exist yet - creates new entry
    /// - **Additional Mirrors**: Same version/revision - adds mirror to existing remotes
    /// - **Revision Mismatch**: Same version but different revision - fails with detailed error
    /// - **Large Atom Sets**: Uses size hint for efficient HashMap capacity management
    ///
    /// # Assumptions
    ///
    /// - Atom metadata is valid and properly formatted
    /// - Mirror URLs are unique and correctly identify the source
    /// - Revision hashes are cryptographic and collision-resistant
    ///
    /// # Integration
    ///
    /// Called during remote mirror processing to validate each discovered atom.
    /// Critical for security as it prevents malicious mirrors from advertising
    /// conflicting versions of the same software.
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
    /// This function enforces the integrity constraints of package sets by preventing
    /// ambiguous mappings between set names, repository root hashes, and mirror URLs.
    /// It maintains the invariant that each repository can only belong to one set,
    /// and each set can only map to one repository.
    ///
    /// # Parameters
    ///
    /// - `set_tag`: The name/tag of the package set being validated
    /// - `root`: The cryptographic root hash of the repository
    /// - `mirror`: The mirror configuration (URL or local) being checked
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `BoxError` if consistency violations are detected.
    ///
    /// # Algorithm
    ///
    /// 1. **Root-to-Set Mapping Check**:
    ///    - Attempt to insert root hash -> set name mapping
    ///    - If mapping already exists with different set name, fail with conflict error
    ///    - Log detailed error showing the conflicting sets and mirror
    ///
    /// 2. **Set-to-Root Mapping Check**:
    ///    - Attempt to insert set name -> root hash mapping
    ///    - If mapping already exists with different root hash, fail with conflict error
    ///    - Log detailed error showing the conflicting roots and mirror
    ///
    /// 3. **Mirror-to-Root Mapping**:
    ///    - Record which root hash this mirror URL points to
    ///    - Used for future validation of the same mirror URL
    ///
    /// # Edge Cases
    ///
    /// - **First Occurrence**: Set/root/mirror combination is new - creates mappings
    /// - **Same Set, Same Root**: Valid - no error, mappings already exist
    /// - **Different Set, Same Root**: Invalid - repository belongs to multiple sets
    /// - **Same Set, Different Root**: Invalid - set points to multiple repositories
    ///
    /// # Assumptions
    ///
    /// - Root hashes are cryptographic and uniquely identify repositories
    /// - Set names are user-defined and may conflict due to misconfiguration
    /// - Mirror URLs should consistently point to the same repository
    ///
    /// # Integration
    ///
    /// Called during both local and remote mirror processing to validate set boundaries.
    /// Critical for preventing supply chain attacks where malicious actors attempt to
    /// associate the same repository with multiple package sets.
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

impl Uri {
    /// Resolves an `Uri` to a fully specified `AtomDep` by querying the
    /// remote Git repository to find the highest matching version and its
    /// corresponding commit hash.
    ///
    /// # Returns
    ///
    /// A `Result` containing the resolved `AtomDep` or a `git::Error` if
    /// resolution fails.
    pub(crate) fn resolve(
        &self,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<(AtomReq, AtomDep), crate::storage::git::Error> {
        let url = self.url();
        let label = self.label();
        if url.is_some_and(|u| u.scheme != gix::url::Scheme::File) {
            let url = url.unwrap();
            let atoms = url.get_atoms(transport)?;
            let ObjectId::Sha1(root) = *atoms.calculate_origin()?;
            let (version, oid) =
                <gix::url::Url as QueryVersion<_, _, _, _, _>>::process_highest_match(
                    atoms.clone(),
                    label,
                    &self.version_req(),
                )
                .ok_or(crate::storage::git::Error::NoMatchingVersion)?;
            let atom_req = if let Some(req) = self.version() {
                AtomReq::new(req.to_owned())
            } else {
                let v = VersionReq::parse(version.to_string().as_str())?;
                AtomReq::new(v)
            };
            let id = AtomId::construct(&atoms, label.to_owned())?;
            Ok((
                atom_req,
                AtomDep::new(
                    label.to_owned(),
                    version,
                    GitDigest::Sha1(root),
                    match oid {
                        ObjectId::Sha1(bytes) => Some(GitDigest::Sha1(bytes)),
                    },
                    Some(url.to_owned()),
                    id.into(),
                ),
            ))
        } else {
            // implement path resolution for atoms
            tracing::warn!("specifying atoms by path not implemented");
            todo!()
        }
    }

    fn version_req(&self) -> VersionReq {
        self.version()
            .map(ToOwned::to_owned)
            .unwrap_or(VersionReq::STAR)
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

        let _validate: ValidManifest = toml_edit::de::from_str(&doc_str)?;
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
    pub(super) fn sanitize(&mut self, manifest: &ValidManifest) {
        let manifest = manifest.as_ref();
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
    pub(super) async fn synchronize(&mut self, manifest: ValidManifest) -> Result<(), DocError> {
        self.synchronize_atoms(&manifest).await?;
        self.synchronize_direct(&manifest).await?;
        Ok(())
    }

    async fn synchronize_atoms(&mut self, manifest: &ValidManifest) -> Result<(), DocError> {
        for (set_tag, set) in manifest.as_ref().deps().from() {
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

    /// Synchronizes a single atom's lockfile entry with the manifest requirements.
    ///
    /// This function ensures that the lockfile contains the correct version and metadata
    /// for an atom specified in the manifest. It handles both new atoms (not yet in lockfile)
    /// and existing atoms that may need version updates.
    ///
    /// # Parameters
    ///
    /// - `req`: The version requirement specified in the manifest for this atom
    /// - `id`: The unique identifier for this atom within its package set
    /// - `set_tag`: The name of the package set containing this atom
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `git::Error` if:
    /// - No matching version can be found in resolved atoms
    /// - Local resolution fails when remote resolution is unavailable
    ///
    /// # Algorithm
    ///
    /// 1. **Check Lockfile Presence**: Determine if atom already exists in lockfile
    /// 2. **New Atom Resolution**: If not present, resolve to highest matching version
    /// 3. **Existing Atom Validation**: If present, check if current version still matches
    ///    requirement
    /// 4. **Version Update**: If requirement no longer matches, resolve to new version
    ///
    /// # Edge Cases
    ///
    /// - **New Atom**: Not in lockfile - resolves and adds new entry
    /// - **Version Mismatch**: Existing atom version doesn't satisfy new requirement - updates
    /// - **Version Match**: Existing atom still valid - no changes needed
    /// - **Resolution Failure**: No version satisfies requirement - returns error
    ///
    /// # Assumptions
    ///
    /// - Manifest requirements are valid and parseable
    /// - Resolved atoms contain all available versions for this atom
    /// - Lockfile structure is consistent with manifest
    ///
    /// # Integration
    ///
    /// Called during manifest synchronization for each atom in each package set.
    /// Part of the broader dependency resolution and locking process that ensures
    /// reproducible builds by recording exact versions and commit hashes.
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
            if dep.rev().is_some() && !req.matches(dep.version()) {
                self.lock_atom(req, id, set_tag)?;
            }
        }
        Ok(())
    }

    /// Locks an atom to a specific version and commit hash for reproducible builds.
    ///
    /// This function resolves a version requirement to a concrete version and commit,
    /// creating a lockfile entry that ensures deterministic dependency resolution.
    /// It first attempts resolution against pre-resolved remote atoms, falling back
    /// to local repository resolution if needed.
    ///
    /// # Parameters
    ///
    /// - `req`: The version requirement to satisfy (e.g., "^1.0.0", "2.1.*")
    /// - `id`: The atom identifier within its package set
    /// - `set_tag`: The name of the package set containing this atom
    ///
    /// # Returns
    ///
    /// Returns `Ok(lock::Dep)` containing the locked atom dependency, or a `git::Error` if:
    /// - No version satisfies the requirement
    /// - Resolution fails for both remote and local sources
    ///
    /// # Algorithm
    ///
    /// 1. **Remote Resolution Attempt**: Try to resolve against pre-resolved atoms from mirrors
    /// 2. **Local Fallback**: If remote resolution fails and local repo available:
    ///    - Construct URI from atom ID and requirement
    ///    - Resolve against local repository
    /// 3. **Lockfile Update**: Insert/update the resolved dependency in the lockfile
    ///
    /// # Edge Cases
    ///
    /// - **No Remote Match**: Falls back to local resolution
    /// - **No Local Repo**: Returns error if both remote and local resolution fail
    /// - **Local Resolution Success**: Uses local version with "local" prerelease tag
    /// - **Version Conflicts**: Prefers remote resolution, only uses local as fallback
    ///
    /// # Assumptions
    ///
    /// - Remote atoms have been pre-resolved and are available in `self.resolved`
    /// - Local repository (if present) contains valid atom manifests
    /// - Version requirements are valid semver expressions
    ///
    /// # Integration
    ///
    /// Called during synchronization when an atom needs to be locked to a specific version.
    /// The resulting lockfile entry ensures that subsequent builds use identical dependency
    /// versions and commit hashes, enabling reproducible builds.
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

    /// Resolves an atom URI against the local git repository.
    ///
    /// This function handles resolution when the user is working within a local git repository
    /// that contains atom packages. It attempts to resolve the URI first against pre-resolved
    /// remote atoms, and if that fails, falls back to reading the atom manifest directly
    /// from the local filesystem.
    ///
    /// # Parameters
    ///
    /// - `uri`: The atom URI specifying the package name and version requirement
    /// - `repo`: The local git repository to resolve against
    ///
    /// # Returns
    ///
    /// Returns `Ok((AtomReq, AtomDep))` containing the resolved requirement and dependency,
    /// or a `git::Error` if resolution fails.
    ///
    /// # Algorithm
    ///
    /// 1. **Calculate Local Root**: Get the origin hash of the repository's HEAD commit
    /// 2. **Remote Resolution Attempt**: Try to resolve against pre-resolved atoms first
    /// 3. **Local Fallback**: If remote resolution fails:
    ///    - Construct atom ID from root and URI label
    ///    - Find the atom's manifest file in the local package directory
    ///    - Parse the manifest and validate it matches the requested atom
    ///    - Create a dependency with a "local" prerelease version tag
    ///
    /// # Edge Cases
    ///
    /// - **Remote Resolution Success**: Uses remote resolution result directly
    /// - **Missing Local Manifest**: Returns error if atom not found in local packages
    /// - **Manifest Mismatch**: Returns error if local manifest label doesn't match URI
    /// - **Invalid Local Version**: Returns error if local atom version is invalid
    ///
    /// # Assumptions
    ///
    /// - Local repository contains valid atom manifests in expected locations
    /// - Local packages are stored in a directory structure accessible via `self.resolved.ekala`
    /// - Local resolution is used for development/testing, not production dependencies
    ///
    /// # Integration
    ///
    /// Called as a fallback during URI resolution when remote resolution is unavailable
    /// or when explicitly working with local development versions of atoms.
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
            let atom = ValidManifest::get_atom(&content)?;
            if atom.label() != uri.label() {
                tracing::error!(
                    labels.uri = %uri.label(),
                    labels.atom = %atom.label(),
                    "bug; somehow the atom's name changed"
                );
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

    /// Resolves an atom URI to concrete dependency information based on mirror configuration.
    ///
    /// This function handles the complex logic of determining how to resolve an atom URI
    /// based on whether it's from a remote mirror or local repository, and whether the
    /// mirror has already been resolved. It implements a priority system where remote
    /// mirrors are preferred over local resolution.
    ///
    /// # Parameters
    ///
    /// - `uri`: The atom URI containing package name and version requirements
    /// - `mirror`: The mirror configuration (remote URL or local) for this atom
    ///
    /// # Returns
    ///
    /// Returns `Ok((AtomReq, AtomDep))` with the resolved requirement and dependency,
    /// or a `git::Error` if resolution fails.
    ///
    /// # Algorithm
    ///
    /// 1. **Remote Mirror with Known Root**: If mirror is remote and root hash is known:
    ///    - Use `resolve_from_uri` to resolve against pre-resolved atoms
    /// 2. **Remote Mirror Unknown**: If mirror is remote but root unknown:
    ///    - Use transport from stored connections if available
    ///    - Call URI's `resolve` method to fetch from remote repository
    /// 3. **Local Repository**: If mirror is local and repo available:
    ///    - Use `resolve_from_local` for local development resolution
    /// 4. **Fallback Error**: If no resolution path available, return error with guidance
    ///
    /// # Edge Cases
    ///
    /// - **Mixed Remote/Local**: Handles atoms that exist in both remote and local contexts
    /// - **Transport Reuse**: Reuses existing transport connections for efficiency
    /// - **Local Development**: Supports development workflow with local atom packages
    /// - **Missing Resolution**: Provides helpful error messages for unsupported scenarios
    ///
    /// # Assumptions
    ///
    /// - Mirror configurations are valid and consistent
    /// - Transport connections are properly managed and reusable
    /// - Local repositories contain valid atom manifests when specified
    ///
    /// # Integration
    ///
    /// Called during URI addition to manifest, ensuring atoms are resolved consistently
    /// whether they come from remote mirrors or local development repositories.
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

    /// Inserts or updates a dependency in the lockfile and logs the operation.
    ///
    /// This function manages the lockfile's dependency map, handling both insertions of new
    /// dependencies and updates to existing ones. It provides detailed logging to track
    /// changes during the resolution process, helping with debugging and audit trails.
    ///
    /// # Parameters
    ///
    /// - `key`: Either an `AtomId<Root>` for atom dependencies or a `Name` for direct dependencies
    /// - `dep`: The dependency information to store (atom, nix, git, tar, or source types)
    ///
    /// # Returns
    ///
    /// This function doesn't return a value - it modifies the lockfile in place.
    ///
    /// # Algorithm
    ///
    /// 1. **Insert/Update Operation**: Attempt to insert the dependency into the lockfile
    /// 2. **Change Detection**: Check if this was an update (key already existed) or new insertion
    /// 3. **Logging**: Log appropriate messages based on dependency type and operation:
    ///    - Atom dependencies: Include set information and operation type
    ///    - Direct dependencies: Log the dependency name and type
    ///
    /// # Edge Cases
    ///
    /// - **New Dependency**: Logs as "updated" but actually inserted (HashMap behavior)
    /// - **Type-Specific Logging**: Different log formats for different dependency types
    /// - **Set Resolution**: For atoms, attempts to resolve set name for better logging
    ///
    /// # Assumptions
    ///
    /// - Lockfile structure is properly initialized
    /// - Dependency data is valid and consistent
    /// - Logging is configured and available
    ///
    /// # Integration
    ///
    /// Called whenever dependencies are resolved or updated during manifest synchronization.
    /// The logging helps track the dependency resolution process and identify when
    /// dependencies change between resolution runs.
    fn insert_or_update_and_log(&mut self, key: Either<AtomId<Root>, Name>, dep: &lock::Dep) {
        if let Some(old) = &self.lock.deps.as_mut().insert(key, dep.to_owned()) {
            if old != dep {
                match dep {
                    lock::Dep::Atom(dep) => {
                        let tag = self.resolved.details().get(&dep.set()).map(|d| &d.tag);
                        tracing::warn!(
                            message = Self::UPDATE_DEPENDENCY,
                            label = %dep.label(),
                            set = %tag.map(|t| t.deref().as_str()).unwrap_or("<missing>"),
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
            } else {
                tracing::info!(%dep, "lock change requested, but version still matches")
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
