//! # Atom Publishing for a Git Store
//!
//! This module provides the types and logic necessary to efficiently publish Atoms
//! to a Git repository. Atoms are stored as orphaned Git histories so they can be
//! efficiently fetched. For trivial verification, an Atom's commit hash is made
//! reproducible by using constants for the timestamps and metadata.
//!
//! Additionally, a Git reference is stored under the Atom's ref path to the original
//! source, ensuring it is never garbage collected and an Atom can always be verified.
//!
//! A hexadecimal representation of the source commit is also stored in the reproducible
//! Atom commit header, ensuring it is tied to its source in an unforgable manner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bstr::ByteSlice;
use gix::diff::object::Commit as AtomCommit;
use gix::protocol::transport::client::Transport;
use gix::{Commit, ObjectId, Reference, Remote, Repository, Tree};
use semver::Version;
use tokio::task::JoinSet;

use super::error::git::Error;
use super::{Builder, Content, Publish, PublishOutcome, Record, StateValidator, ValidAtoms};
use crate::id::Label;
use crate::package::metadata::AtomPaths;
use crate::storage::git::Root;
use crate::storage::{NormalizeStorePath, QueryStore};
use crate::{Atom, AtomId};

mod inner;

#[cfg(test)]
mod test;

//================================================================================================
// Types
//================================================================================================

/// The Git-specific content returned after an Atom is successfully published.
#[derive(Debug)]
pub struct GitContent {
    spec: gix::refs::Reference,
    content: gix::refs::Reference,
    origin: gix::refs::Reference,
    path: PathBuf,
}

/// Holds the shared context needed for publishing Atoms.
pub struct GitContext<'a> {
    /// Reference to the repository we are publishing from.
    repo: &'a Repository,
    /// The repository tree object for the given commit.
    tree: Tree<'a>,
    /// The commit to publish from.
    commit: Commit<'a>,
    /// The configured remote repository.
    remote: Remote<'a>,
    /// The name of the remote, for convenient use.
    remote_str: &'a str,
    /// The reported root commit according to the remote.
    root: Root,
    /// A `JoinSet` of asynchronous push tasks to avoid blocking.
    push_tasks: RefCell<JoinSet<Result<Vec<u8>, Error>>>,
    /// A reusable Git server transport to avoid connection overhead.
    transport: Box<dyn Transport + Send>,
    /// A span representing the overall progress bar for easy incrementing.
    progress: &'a tracing::Span,
}

/// The type representing a Git-specific Atom publisher.
pub struct GitPublisher<'a> {
    repo: &'a Repository,
    remote: Remote<'a>,
    remote_str: &'a str,
    spec: &'a str,
    root: Root,
    transport: Box<dyn Transport + Send>,
    progress: &'a tracing::Span,
}

/// The Outcome of an Atom publish attempt to a Git store.
pub type GitOutcome = PublishOutcome<Root>;

/// The Result type used for various methods during publishing to a Git store.
pub type GitResult<T> = Result<T, Error>;

/// Represents a Git reference to a component of a published Atom.
struct AtomRef<'a> {
    label: String,
    kind: RefKind,
    version: &'a Version,
}

/// Holds context specific to a single Atom being published.
struct AtomContext<'a> {
    paths: AtomPaths<PathBuf>,
    atom: FoundAtom,
    git: &'a GitContext<'a>,
}

/// Represents the Git references pointing to an Atom's constituent parts.
#[derive(Debug, Clone)]
pub(super) struct AtomReferences<'a> {
    /// The Git ref pointing to the Atom's content.
    content: Reference<'a>,
    /// The Git ref pointing to the tree object containing the Atom's manifest and lock.
    spec: Reference<'a>,
    /// The Git ref pointing to the commit the Atom was published from.
    origin: Reference<'a>,
}

/// Holds the result of writing an Atom's content as a Git commit.
#[derive(Debug, Clone)]
pub struct CommittedAtom {
    /// The raw structure representing the Atom that was successfully committed.
    commit: AtomCommit,
    /// The object ID of the Atom commit.
    id: ObjectId,
}

/// An Atom that has been found within the Git repository structure.
#[derive(Debug)]
struct FoundAtom {
    spec: Atom,
    id: GitAtomId,
    tree_id: ObjectId,
    spec_id: ObjectId,
}

/// The different kinds of Git references created for an Atom.
enum RefKind {
    Spec,
    Content,
    Origin,
}

type GitAtomId = AtomId<Root>;
type GitRecord = Record<Root>;

//================================================================================================
// Impls
//================================================================================================

impl<'a> Builder<'a, Root> for GitPublisher<'a> {
    type Error = Error;
    type Publisher = GitContext<'a>;

    fn build(self) -> Result<(ValidAtoms, Self::Publisher), Self::Error> {
        let publisher = GitContext::set(
            self.repo,
            self.remote.clone(),
            self.remote_str,
            self.spec,
            self.root,
            self.transport,
            self.progress,
        )?;
        let atoms = GitPublisher::validate(&publisher)?;
        Ok((atoms, publisher))
    }
}

impl GitContent {
    /// Returns a reference to the Atom's content ref.
    #[must_use]
    pub fn content(&self) -> &gix::refs::Reference {
        &self.content
    }

    /// Returns a reference to the Atom's source Git ref.
    #[must_use]
    pub fn origin(&self) -> &gix::refs::Reference {
        &self.origin
    }

    /// Returns a reference to the path to the Atom.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Returns a reference to the Atom's spec Git ref.
    #[must_use]
    pub fn spec(&self) -> &gix::refs::Reference {
        &self.spec
    }
}

impl<'a> GitContext<'a> {
    /// Asynchronously awaits the results of concurrently running Git pushes.
    ///
    /// Any errors that occurred during the push operations will be collected into the provided
    /// `errors` vector.
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

    /// Returns a reference to the Git remote.
    pub fn remote(&self) -> Remote<'a> {
        self.remote.clone()
    }

    /// Returns a reference to the Git tree object of the commit the Atom originates from.
    pub fn tree(&self) -> Tree<'a> {
        self.tree.clone()
    }

    fn set(
        repo: &'a Repository,
        remote: Remote<'a>,
        remote_str: &'a str,
        refspec: &str,
        root: Root,
        transport: Box<dyn Transport + Send>,
        progress: &'a tracing::Span,
    ) -> GitResult<Self> {
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
            remote,
            remote_str,
            push_tasks,
            transport,
            progress,
        })
    }
}

impl<'a> GitPublisher<'a> {
    /// Constructs a new `GitPublisher`.
    pub fn new(
        repo: &'a Repository,
        remote_str: &'a str,
        spec: &'a str,
        progress: &'a tracing::Span,
    ) -> GitResult<Self> {
        use crate::storage::Init;
        let remote = repo.find_remote(remote_str).map_err(Box::new)?;
        let mut transport = remote.get_transport().map_err(Box::new)?;
        // TODO: we actually need to verify the label
        let root = remote.ekala_root(Some(&mut transport)).map_err(|e| {
            e.warn();
            tracing::warn!("Did you run `eka init`?");
            Error::NotInitialized
        })?;

        Ok(GitPublisher {
            repo,
            remote,
            remote_str,
            spec,
            transport,
            root,
            progress,
        })
    }
}

impl<'a> Publish<Root> for GitContext<'a> {
    type Error = Error;
    type Id = ObjectId;

    /// Publishes a collection of Atoms to the Git store.
    ///
    /// This function processes a collection of paths, each representing an Atom to be published.
    /// The publishing process includes path normalization, existence checks, and the actual
    /// publishing attempt.
    ///
    /// # Path Normalization
    /// - First attempts to interpret each path as relative to the caller's current location inside
    ///   the repository.
    /// - If normalization fails (e.g., in a bare repository), it falls back to treating the path as
    ///   already relative to the repository root.
    /// - The normalized path is used to search the Git history, not the local file system.
    ///
    /// # Publishing Process
    /// For each path:
    /// 1. Normalizes the path as described above.
    /// 2. Checks if the Atom already exists in the repository and on the remote.
    ///    - If it exists and is identical, the Atom is skipped.
    /// 3. Attempts to publish the Atom.
    ///    - On success, the Atom's content and references are pushed to the remote.
    ///    - On failure, the Atom is skipped, and an error is logged.
    ///
    /// # Error Handling
    /// - The function processes all provided paths, even if some fail.
    /// - Errors and skipped Atoms are collected as results but do not halt the overall process.
    ///
    /// # Return Value
    /// Returns a vector of `GitResult<GitOutcome>`, where each item represents the result of a
    /// single Atom's publishing attempt.
    fn publish<C>(
        &self,
        paths: C,
        remotes: HashMap<Label, (Version, ObjectId)>,
    ) -> Vec<GitResult<GitOutcome>>
    where
        C: IntoIterator<Item = PathBuf>,
    {
        use crate::storage::git;
        let iter = paths.into_iter();
        iter.map(|path| {
            let path = match self.repo.normalize(&path) {
                Ok(path) => path,
                Err(git::Error::NoWorkDir) => path,
                Err(e) => return Err(Box::new(e).into()),
            };
            self.publish_atom(&path, &remotes)
        })
        .collect()
    }

    fn publish_atom<P: AsRef<Path>>(
        &self,
        path: P,
        remotes: &HashMap<Label, (Version, ObjectId)>,
    ) -> GitResult<GitOutcome> {
        use {Err as Skipped, Ok as Published};
        let context = AtomContext::set(path.as_ref(), self)?;
        let span = tracing::info_span!("publish atom", atom=%context.atom.id.label());
        crate::log::set_sub_task(&span, &format!("⚛️ `{}`", context.atom.id.label()));
        let _enter = span.enter();

        let r = &context.refs(RefKind::Content);
        let lr = self.repo.find_reference(&r.to_string());

        if let Ok(lr) = lr {
            if let Some((v, id)) = remotes.get(context.atom.id.label()) {
                if r.version == v && lr.id().detach() == *id {
                    // Remote and local atoms are identical; skip.
                    return Ok(Skipped(context.atom.spec.take_label()));
                }
            }
        }

        let refs = context
            .write_atom_commit(context.atom.tree_id)?
            .write_refs(&context)?
            .push(&context);

        Ok(Published(GitRecord {
            id: context.atom.id.clone(),
            content: Content::Git(refs),
        }))
    }
}

impl<'a> super::private::Sealed for GitContext<'a> {}

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
        let mut atoms: HashMap<Label, PathBuf> = HashMap::with_capacity(cap);

        for entry in record.records {
            let path = PathBuf::from(entry.filepath.to_str_lossy().as_ref());
            if entry.mode.is_blob() && path.file_name() == Some(crate::ATOM_MANIFEST_NAME.as_ref())
            {
                if let Ok(obj) = publisher.repo.find_object(entry.oid) {
                    match publisher.verify_manifest(&obj, &path) {
                        Ok(atom) => {
                            if let Some(duplicate) = atoms.get(atom.label()) {
                                tracing::warn!(
                                    message = "Two atoms share the same ID",
                                    duplicate.label = %atom.label(),
                                    fst = %path.display(),
                                    snd = %duplicate.display(),
                                );
                                return Err(Error::Duplicates);
                            }
                            atoms.insert(atom.take_label(), path);
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

impl<'a> AtomContext<'a> {
    fn set(path: &'a Path, git: &'a GitContext) -> GitResult<Self> {
        let (atom, paths) = git.find_and_verify_atom(path)?;
        Ok(Self { paths, atom, git })
    }
}

//================================================================================================
// Functions
//================================================================================================

/// Calculates a reasonable capacity for a HashMap based on the number of records.
fn calculate_capacity(record_count: usize) -> usize {
    let log_count = (record_count as f64).log2();
    let base_multiplier = 20.0;
    let scaling_factor = (log_count - 10.0).max(0.0).powf(2.0);
    let multiplier = base_multiplier + scaling_factor * 10.0;
    (log_count * multiplier).ceil() as usize
}
