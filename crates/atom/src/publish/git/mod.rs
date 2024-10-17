//! # Atom Publishing for a Git Store
//!
//! This module provides the types and logical necessary to efficienctly publish Atoms
//! to a Git repository. Atom's are stored as orphaned git histories so they can be
//! efficiently fetched. For trivial verification, an Atom's commit hash is made
//! reproducible by using constants for the timestamps and meta-data.
//!
//! Additionally, a git reference is stored under the Atom's ref path to the original
//! source, ensuring it is never garbage collected and an Atom can always be verified.
//!
//! A hexadecimal representation of the source commit is also stored in the reproducible
//! Atom commit header, ensuring it is tied to its source in an unforgable manner.
#[cfg(test)]
mod test;

mod inner;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use gix::{Commit, ObjectId, Repository, Tree};
use tokio::task::JoinSet;

use super::error::git::Error;
use super::{Content, PublishOutcome, Record};
use crate::core::AtomPaths;
use crate::store::NormalizeStorePath;
use crate::store::git::Root;
use crate::{Atom, AtomId};

type GitAtomId = AtomId<Root>;
/// The Outcome of an Atom publish attempt to a Git store.
pub type GitOutcome = PublishOutcome<Root>;
/// The Result type used for various methods during publishing to a Git store.
pub type GitResult<T> = Result<T, Error>;
type GitRecord = Record<Root>;

#[derive(Debug)]
/// Holds the shared context needed for publishing Atoms.
pub struct GitContext<'a> {
    /// Reference to the repository we are publish from.
    repo: &'a Repository,
    /// The repository tree object for the given commit.
    tree: Tree<'a>,
    /// The commit to publish from.
    commit: Commit<'a>,
    /// Store the given remote name as a &str for convenient use.
    remote_str: &'a str,
    /// The reported root commit according to the remote.
    root: Root,
    /// A [`JoinSet`] of push tasks to avoid blocking on them.
    push_tasks: RefCell<JoinSet<Result<Vec<u8>, Error>>>,
    /// Path buf for efficient tree searches
    buf: RefCell<Vec<u8>>,
}

struct AtomContext<'a> {
    paths: AtomPaths<PathBuf>,
    atom: FoundAtom<'a>,
    ref_prefix: String,
    git: &'a GitContext<'a>,
}

struct FoundAtom<'a> {
    spec: Atom,
    id: GitAtomId,
    entries: AtomEntries<'a>,
}

use gix::diff::object::Commit as AtomCommit;
use gix::object::tree::Entry;

/// Struct to hold the result of writing atom commits
#[derive(Debug, Clone)]
pub struct CommittedAtom {
    /// The raw structure representing the atom that was successfully committed.
    commit: AtomCommit,
    /// The object id of the Atom commit.
    id: ObjectId,
}

use smallvec::SmallVec;
type AtomEntries<'a> = SmallVec<[Entry<'a>; 3]>;

/// Struct to representing the tree of an atom given by the Git object ID of its contents
struct AtomTreeId(ObjectId);

enum RefKind {
    Spec,
    Content,
    Origin,
}

use semver::Version;

struct AtomRef<'a> {
    prefix: &'a str,
    kind: RefKind,
    version: &'a Version,
}

use gix::Reference;

#[derive(Debug, Clone)]
/// Struct representing the git refs pointing to the atom's parts
pub(super) struct AtomReferences<'a> {
    /// The git ref pointing to the atoms content
    content: Reference<'a>,
    /// Git ref pointing to the tree object containing the atom's manifest and lock
    spec: Reference<'a>,
    /// The git ref pointing the commit the atom was published from
    origin: Reference<'a>,
}

/// The Git specific content which will be returned for presenting to the user after
/// an Atom is successfully published.
#[derive(Debug)]
pub struct GitContent {
    spec: gix::refs::Reference,
    content: gix::refs::Reference,
    origin: gix::refs::Reference,
    path: PathBuf,
    ref_prefix: String,
}

use super::{Builder, ValidAtoms};

/// The type representing a Git specific Atom publisher.
pub struct GitPublisher<'a> {
    repo: &'a Repository,
    remote: &'a str,
    spec: &'a str,
    root: Root,
}

impl<'a> GitPublisher<'a> {
    /// Constructs a new [`GitPublisher`].
    pub fn new(repo: &'a Repository, remote: &'a str, spec: &'a str) -> GitResult<Self> {
        use crate::store::Init;
        let root = repo
            .find_remote(remote)
            .map_err(Box::new)?
            .ekala_root()
            .map_err(|e| {
                e.warn();
                Error::NotInitialized
            })?;

        Ok(GitPublisher {
            repo,
            remote,
            spec,
            root,
        })
    }
}

fn calculate_capacity(record_count: usize) -> usize {
    let log_count = (record_count as f64).log2();
    let base_multiplier = 20.0;
    let scaling_factor = (log_count - 10.0).max(0.0).powf(2.0);
    let multiplier = base_multiplier + scaling_factor * 10.0;
    (log_count * multiplier).ceil() as usize
}

use super::StateValidator;

impl<'a> StateValidator<Root> for GitPublisher<'a> {
    type Error = Error;
    type Publisher = GitContext<'a>;

    fn validate(publisher: &Self::Publisher) -> Result<ValidAtoms, Self::Error> {
        use gix::traverse::tree::Recorder;
        let mut record = Recorder::default();

        publisher
            .tree()
            .traverse()
            .breadthfirst(&mut record)
            .map_err(|_| Error::NotFound)?;

        let cap = calculate_capacity(record.records.len());
        let mut atoms: HashMap<Id, PathBuf> = HashMap::with_capacity(cap);

        for entry in record.records {
            if entry.mode.is_blob() && entry.filepath.ends_with(crate::ATOM_EXT.as_ref()) {
                if let Ok(obj) = publisher.repo.find_object(entry.oid) {
                    let path = PathBuf::from(entry.filepath.to_string());
                    match publisher.verify_manifest(&obj, &path) {
                        Ok(atom) => {
                            if let Some(duplicate) = atoms.get(&atom.id) {
                                tracing::warn!(
                                    message = "Two atoms share the same ID",
                                    duplicate.id = %atom.id,
                                    fst = %path.display(),
                                    snd = %duplicate.display(),
                                );
                                return Err(Error::Duplicates);
                            }
                            atoms.insert(atom.id, path);
                        },
                        Err(e) => e.warn(),
                    }
                }
            }
        }

        tracing::trace!(repo.atoms.valid.count = atoms.len());

        Ok(atoms)
    }
}

impl<'a> Builder<'a, Root> for GitPublisher<'a> {
    type Error = Error;
    type Publisher = GitContext<'a>;

    fn build(&self) -> Result<(ValidAtoms, Self::Publisher), Self::Error> {
        let publisher = GitContext::set(self.repo, self.remote, self.spec, self.root)?;
        let atoms = GitPublisher::validate(&publisher)?;
        Ok((atoms, publisher))
    }
}

impl GitContent {
    /// Return a reference to the Atom spec Git ref.
    #[must_use]
    pub fn spec(&self) -> &gix::refs::Reference {
        &self.spec
    }

    /// Return a reference to the Atom src Git ref.
    #[must_use]
    pub fn origin(&self) -> &gix::refs::Reference {
        &self.origin
    }

    /// Return a reference to the Atom content ref.
    #[must_use]
    pub fn content(&self) -> &gix::refs::Reference {
        &self.content
    }

    /// Return a reference to the path to the Atom.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Return a reference to the atom ref prefix.
    #[must_use]
    pub fn ref_prefix(&self) -> &String {
        &self.ref_prefix
    }
}

use std::collections::HashMap;

use super::Publish;
use crate::id::Id;

impl<'a> super::private::Sealed for GitContext<'a> {}

impl<'a> Publish<Root> for GitContext<'a> {
    type Error = Error;

    /// Publishes atoms.
    ///
    /// This function processes a collection of paths, each representing an atom to be published.
    /// The publishing process includes path normalization, existence checks, and actual
    /// publishing attempts.
    ///
    /// # Path Normalization
    /// - First attempts to interpret each path as relative to the caller's current location inside
    ///   the repository.
    /// - If normalization fails (e.g., in a bare repository), falls back to treating the path as
    ///   already relative to the repo root.
    /// - The normalized path is used to search the Git history, not the file system.
    ///
    /// # Publishing Process
    /// For each path:
    /// 1. Normalizes the path (as described above).
    /// 2. Checks if the atom already exists in the repository.
    ///    - If it exists, the atom is skipped, and a log message is generated.
    /// 3. Attempts to publish the atom.
    ///    - If successful, the atom is added to the repository.
    ///    - If any error occurs during publishing, the atom is skipped, and an error is logged.
    ///
    /// # Error Handling
    /// - The function aims to process all provided paths, even if some fail.
    /// - Errors and skipped atoms are collected as results but do not halt the overall process.
    /// - The function continues until all the atoms have been processed.
    ///
    /// # Return Value
    /// Returns a vector of results types (`Vec<Result<PublishOutcome<T>, Self::Error>>`), where the
    /// outter result represents whether an atom has failed, and the inner result determines whether
    /// an atom was safely skipped, e.g. because it already exists..
    fn publish<C>(&self, paths: C) -> Vec<GitResult<GitOutcome>>
    where
        C: IntoIterator<Item = PathBuf>,
    {
        use crate::store::git;
        paths
            .into_iter()
            .map(|path| {
                let path = match self.repo.normalize(&path) {
                    Ok(path) => path,
                    Err(git::Error::NoWorkDir) => path,
                    Err(e) => return Err(e.into()),
                };
                self.publish_atom(&path)
            })
            .collect()
    }

    fn publish_atom<P: AsRef<Path>>(&self, path: P) -> GitResult<GitOutcome> {
        use {Err as Skipped, Ok as Published};

        let atom = AtomContext::set(path.as_ref(), self)?;

        let tree_id = match atom.write_atom_tree(&atom.atom.entries)? {
            Ok(t) => t,
            Skipped(id) => return Ok(Skipped(id)),
        };

        let refs = atom
            .write_atom_commit(tree_id)?
            .write_refs(&atom)?
            .push(&atom);

        Ok(Published(GitRecord {
            id: atom.atom.id.clone(),
            content: Content::Git(refs),
        }))
    }
}

impl<'a> AtomContext<'a> {
    fn set(path: &'a Path, git: &'a GitContext) -> GitResult<Self> {
        let (atom, paths) = git.find_and_verify_atom(path)?;
        let ref_prefix = format!("{}/{}", super::ATOM_REF_TOP_LEVEL, atom.id.id());
        Ok(Self {
            paths,
            atom,
            ref_prefix,
            git,
        })
    }
}

impl<'a> GitContext<'a> {
    fn set(
        repo: &'a Repository,
        remote_str: &'a str,
        refspec: &str,
        root: Root,
    ) -> GitResult<Self> {
        // short-circuit publishing if the passed remote doesn't exist
        let _remote = repo.find_remote(remote_str).map_err(Box::new)?;
        let commit = repo
            .rev_parse_single(refspec)
            .map(|s| repo.find_commit(s))
            .map_err(Box::new)??;

        let tree = commit.tree()?;

        let push_tasks = RefCell::new(JoinSet::new());

        Ok(Self {
            repo,
            root,
            tree,
            commit,
            remote_str,
            push_tasks,
            buf: RefCell::new(Vec::with_capacity(64)),
        })
    }

    /// A method used to await the results of the concurrently running Git pushes,
    /// which were offloaded to a seperate thread of execution of Tokio's runtime.
    ///
    /// An errors that occurred will be collected into a [`Vec`].
    pub async fn await_pushes(&self, errors: &mut Vec<Error>) {
        use tokio::sync::Mutex;

        let tasks = Mutex::new(self.push_tasks.borrow_mut());

        while let Some(task) = tasks.lock().await.join_next().await {
            match task {
                Ok(Ok(output)) => {
                    if !output.is_empty() {
                        tracing::info!(output = %String::from_utf8_lossy(&output));
                    }
                },
                Ok(Err(e)) => {
                    errors.push(e);
                },
                Err(e) => {
                    errors.push(Error::JoinFailed(e));
                },
            }
        }
    }

    /// Return a reference to the git tree object of the commit the Atom originates from.
    pub fn tree(&self) -> Tree<'a> {
        self.tree.clone()
    }
}
