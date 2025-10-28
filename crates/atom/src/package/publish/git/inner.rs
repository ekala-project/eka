//! This module contains the inner implementation details for Git-based publishing operations.
//!
//! It provides helper functions and methods on context-specific structs (`GitContext`,
//! `AtomContext`) to handle the underlying Git object manipulation, reference updates, and remote
//! interactions required to publish an Atom. The functions here are not intended to be part of the
//! public API.

use std::fmt;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use gix::actor::Signature;
use gix::diff::object::Commit as AtomCommit;
use gix::object::tree::Entry;
use gix::objs::WriteTo;
use gix::protocol::transport::client::Transport;
use gix::{Object, ObjectId, Reference};
use semver::Version;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use super::super::error::git::Error;
use super::super::{
    ATOM_FORMAT_VERSION, ATOM_MANIFEST, ATOM_ORIGIN, ATOM_REFS, EMPTY_SIG, META_REFS,
};
use super::{
    AtomContext, AtomRef, AtomReferences, CommittedAtom, FoundAtom, GitContent, GitContext,
    GitResult, RefKind,
};
use crate::package::metadata::AtomPaths;
use crate::storage::git;
use crate::{Atom, AtomId, Manifest};

//================================================================================================
// Impls
//================================================================================================

impl<'a> AtomContext<'a> {
    /// Creates a new `AtomRef` for a specific reference kind (`Spec`, `Content`, `Origin`).
    pub(super) fn refs(&self, kind: RefKind) -> AtomRef<'_> {
        AtomRef::new(
            self.atom.id.label().to_string(),
            kind,
            self.atom.spec.version(),
        )
    }

    /// Constructs and writes a Git commit object that represents the Atom's content.
    ///
    /// This commit is self-contained and does not have any parents. It captures the state of the
    /// Atom's content tree and includes metadata such as the original source commit and path.
    pub(super) fn write_atom_commit(&self, tree: ObjectId) -> GitResult<CommittedAtom> {
        let sig = Signature {
            email: EMPTY_SIG.into(),
            name: EMPTY_SIG.into(),
            time: gix::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        let commit = AtomCommit {
            tree,
            parents: vec![].into(),
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: format!(
                "publish({}): {}",
                self.atom.spec.label(),
                self.atom.spec.version()
            )
            .into(),
            extra_headers: [
                (ATOM_ORIGIN.into(), self.git.commit.id.to_string().into()),
                ("format".into(), ATOM_FORMAT_VERSION.into()),
            ]
            .into(),
        };
        let id = self.git.write_object(commit.clone())?;
        Ok(CommittedAtom { commit, id })
    }
}

impl<'a> AtomRef<'a> {
    /// Constructs a new `AtomRef`.
    fn new(label: String, kind: RefKind, version: &'a Version) -> Self {
        AtomRef {
            label,
            kind,
            version,
        }
    }
}

impl<'a> fmt::Display for AtomRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RefKind::Content => {
                write!(f, "{}/{}/{}", ATOM_REFS.as_str(), self.label, self.version)
            },
            RefKind::Origin => write!(
                f,
                "{}/{}/{}/{}",
                META_REFS.as_str(),
                self.label,
                self.version,
                ATOM_ORIGIN
            ),
            RefKind::Spec => write!(
                f,
                "{}/{}/{}/{}",
                META_REFS.as_str(),
                self.label,
                self.version,
                ATOM_MANIFEST
            ),
        }
    }
}

impl<'a> AtomReferences<'a> {
    /// Pushes the Atom's Git references (`spec`, `content`, `origin`) to the configured remote.
    ///
    /// This operation is asynchronous and spawns tasks to handle each `git push` command.
    pub(super) fn push(self, atom: &'a AtomContext) -> GitContent {
        let remote = atom.git.remote_str.to_owned();
        let mut tasks = atom.git.push_tasks.borrow_mut();

        tracing::info!(
            message = "pushing",
            atom = %atom.atom.id.label(),
            remote = %remote
        );
        for r in [&self.content, &self.spec, &self.origin] {
            let r = r.name().as_bstr().to_string();
            let remote = remote.clone();
            let parent = tracing::Span::current();
            let progress = atom.git.progress.clone();
            let task = async move {
                let _guard = parent.enter();
                let span = tracing::info_span!("push", msg = %r, %remote);
                crate::log::set_sub_task(&span, &format!("🚀 push: {}", r));
                let _enter = span.enter();
                let result = git::run_git_command(&["push", &remote, format!("{r}:{r}").as_str()])?;

                progress.pb_inc(1);

                Ok(result)
            };
            tasks.spawn(task);
        }

        GitContent {
            spec: self.spec.detach(),
            content: self.content.detach(),
            origin: self.origin.detach(),
            path: atom.paths.spec().to_path_buf(),
        }
    }
}

impl<'a> CommittedAtom {
    /// Writes the Git references for the committed Atom.
    ///
    /// This creates three references:
    /// - `spec`: Points to the manifest blob.
    /// - `content`: Points to the newly created atom commit.
    /// - `origin`: Points to the original source commit from which the atom was published.
    pub(super) fn write_refs(&'a self, atom: &'a AtomContext) -> GitResult<AtomReferences<'a>> {
        let Self { id, .. } = self;

        let spec = atom.atom.spec_id;
        let src = atom.git.commit.id;

        Ok(AtomReferences {
            spec: write_ref(atom, spec, atom.refs(RefKind::Spec))?,
            content: write_ref(atom, *id, atom.refs(RefKind::Content))?,
            origin: write_ref(atom, src, atom.refs(RefKind::Origin))?,
        })
    }

    #[must_use]
    /// Returns a reference to the underlying Git commit object.
    pub fn commit(&self) -> &AtomCommit {
        &self.commit
    }

    #[must_use]
    /// Returns the `ObjectId` of the commit, which is the tip of this `CommittedAtom`.
    pub fn tip(&self) -> &ObjectId {
        &self.id
    }
}

impl<'a> GitContext<'a> {
    /// Finds an Atom within the Git tree and verifies its manifest and structure.
    ///
    /// This function ensures that the given path corresponds to a valid Atom by checking for
    /// the existence of the manifest and content tree, and validating the manifest's contents.
    pub(super) fn find_and_verify_atom(
        &self,
        path: &Path,
    ) -> GitResult<(FoundAtom, AtomPaths<PathBuf>)> {
        let paths = AtomPaths::new(path);
        let entry = self
            .tree_search(paths.spec())?
            .ok_or(Error::NotAnAtom(path.into()))?;

        if !entry.mode().is_blob() || !paths.spec().starts_with(paths.content()) {
            return Err(Error::NotAnAtom(path.into()));
        }

        if paths.content().to_str() == Some("") {
            return Err(Error::NoRootAtom);
        }

        let content = self
            .tree_search(paths.content())?
            .and_then(|e| e.mode().is_tree().then_some(e))
            .ok_or(Error::NotAnAtom(path.into()))?
            .detach();

        let tree_id = content.oid;
        let spec_id = entry.id().detach();

        self.verify_manifest(&entry.object()?, paths.spec())
            .and_then(|spec| {
                let id = AtomId::construct(&self.commit, spec.label().clone()).map_err(Box::new)?;
                if self.root != *id.root() {
                    return Err(Error::InconsistentRoot {
                        remote: self.root,
                        atom: *id.root(),
                    });
                };
                Ok((
                    FoundAtom {
                        spec,
                        id,
                        tree_id,
                        spec_id,
                    },
                    paths,
                ))
            })
    }

    /// Provides mutable access to the underlying Git transport.
    pub fn transport(&mut self) -> &mut Box<dyn Transport + Send> {
        &mut self.transport
    }

    /// Searches for a tree entry at a given path starting from the context's root tree.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying `gix::object::tree::Tree::lookup_entry` call fails.
    pub fn tree_search(&self, path: &Path) -> GitResult<Option<Entry<'a>>> {
        let search = path.components().map(|c| c.as_os_str().as_bytes());
        Ok(self.tree.clone().lookup_entry(search)?)
    }

    /// Reads an Atom manifest from a Git object and verifies its contents.
    pub(super) fn verify_manifest(&self, obj: &Object, path: &Path) -> GitResult<Atom> {
        let content = read_blob(obj, |reader| {
            let mut content = String::new();
            reader.read_to_string(&mut content)?;
            Ok(content)
        })?;

        Manifest::get_atom(&content).map_err(|e| Error::Invalid(e, Box::new(path.into())))
    }

    /// Writes a Git object to the repository's object database.
    fn write_object(&self, obj: impl WriteTo) -> GitResult<gix::ObjectId> {
        Ok(self.repo.write_object(obj).map(gix::Id::detach)?)
    }
}

//================================================================================================
// Functions
//================================================================================================

/// Reads the full content of a Git blob object into a specified output format.
fn read_blob<F, R>(obj: &Object, mut f: F) -> GitResult<R>
where
    F: FnMut(&mut dyn Read) -> io::Result<R>,
{
    let mut reader = obj.data.as_slice();
    Ok(f(&mut reader)?)
}

/// Writes a single Git reference to the repository.
fn write_ref<'a>(
    atom: &'a AtomContext,
    id: ObjectId,
    atom_ref: AtomRef,
) -> GitResult<Reference<'a>> {
    use gix::refs::transaction::PreviousValue;

    tracing::debug!("writing atom ref: {}", atom_ref);

    let AtomContext { atom, git, .. } = atom;

    Ok(git.repo.reference(
        atom_ref.to_string(),
        id,
        PreviousValue::MustNotExist,
        format!(
            "publish: {}: {}-{}",
            atom.spec.label(),
            atom.spec.version(),
            atom_ref
        ),
    )?)
}
