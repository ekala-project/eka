use super::{
    super::{ATOM_FORMAT_VERSION, ATOM_MANIFEST, EMPTY_SIG},
    AtomContext, AtomRef, GitContext, GitResult, RefKind,
};
use crate::{
    publish::{error::GitError, ATOM_LOCK, ATOM_ORIGIN},
    store::git,
    Atom, AtomId, Manifest,
};

use gix::{
    actor::Signature,
    diff::object::Commit as AtomCommit,
    object::tree::Entry,
    objs::{tree::Entry as AtomEntry, WriteTo},
    worktree::object::Tree as AtomTree,
    ObjectId, Reference,
};
use std::{
    io::{self, Read},
    os::unix::ffi::OsStrExt,
    path::Path,
};

impl<'a> GitContext<'a> {
    /// Method to verify the manifest of an entry
    pub(super) fn verify_manifest(&self, obj: &Object, path: &Path) -> GitResult<Atom> {
        let content = read_blob(obj, |reader| {
            let mut content = String::new();
            reader.read_to_string(&mut content)?;
            Ok(content)
        })?;

        Manifest::get_atom(&content).map_err(|e| GitError::Invalid(e, Box::new(path.into())))
    }

    /// Compute the ObjectId of the given proto-object in memory
    fn compute_hash(&self, obj: &dyn WriteTo) -> GitResult<ObjectId> {
        use gix::objs;

        let mut buf = Vec::with_capacity(obj.size() as usize);

        obj.write_to(&mut buf)?;

        let oid = objs::compute_hash(self.repo.object_hash(), obj.kind(), buf.as_ref());

        Ok(oid)
    }

    /// Helper function to write an object to the repository
    fn write_object(&self, obj: impl WriteTo) -> GitResult<gix::ObjectId> {
        Ok(self.repo.write_object(obj).map(|id| id.detach())?)
    }

    /// Helper function to return an entry by path from the repo tree
    pub fn tree_search(&self, path: &Path) -> GitResult<Option<Entry<'a>>> {
        let mut buf = self.buf.borrow_mut();
        let search = path.components().map(|c| c.as_os_str().as_bytes());
        Ok(self.tree.clone().lookup_entry(search, &mut buf)?)
    }

    pub(super) fn find_and_verify_atom(&self, path: &Path) -> GitResult<FoundAtom> {
        use smallvec::smallvec;
        let lock = path.with_extension(ATOM_LOCK);
        let dir = path.with_extension("");
        let entry = self
            .tree_search(path)?
            .ok_or(GitError::NotAFile(path.into()))?;

        if !entry.mode().is_blob() {
            return Err(GitError::NotAFile(path.into()));
        }

        let lock = self
            .tree_search(&lock)?
            .and_then(|e| e.mode().is_blob().then_some(e));

        let dir = self
            .tree_search(&dir)?
            .and_then(|e| e.mode().is_tree().then_some(e));

        self.verify_manifest(&entry.object()?, path)
            .and_then(|spec| {
                let id = AtomId::compute(&self.commit, spec.id.clone())?;
                let entries = match (lock, dir) {
                    (None, None) => smallvec![entry],
                    (None, Some(dir)) => smallvec![entry, dir],
                    (Some(lock), None) => smallvec![entry, lock],
                    (Some(lock), Some(dir)) => smallvec![entry, dir, lock],
                };
                Ok(FoundAtom { spec, id, entries })
            })
    }
}

use semver::Version;

impl<'a> AtomRef<'a> {
    fn new(kind: RefKind, prefix: &'a str, version: &'a Version) -> Self {
        AtomRef {
            prefix,
            kind,
            version,
        }
    }
}

use std::fmt;

impl<'a> fmt::Display for AtomRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RefKind::Content => write!(f, "{}/{}", self.prefix, self.version),
            RefKind::Spec => write!(f, "{}/_{}s/{}", self.prefix, ATOM_MANIFEST, self.version),
            RefKind::Origin => write!(f, "{}/_{}s/{}", self.prefix, ATOM_ORIGIN, self.version),
        }
    }
}

use crate::publish::MaybeSkipped;

impl<'a> AtomContext<'a> {
    fn refs(&self, kind: RefKind) -> AtomRef {
        AtomRef::new(kind, &self.ref_prefix, &self.atom.spec.version)
    }

    fn ref_exists(&self, tree: &AtomTree, atom_ref: AtomRef) -> bool {
        let id = self.git.compute_hash(tree);
        if let Ok(id) = id {
            self.git.repo.find_tree(id).is_ok()
                && self.git.repo.find_reference(&atom_ref.to_string()).is_ok()
        } else {
            false
        }
    }
    /// Method to write the atom tree object
    pub(super) fn write_atom_tree(
        &self,
        entries: super::AtomEntries,
    ) -> GitResult<MaybeSkipped<AtomTreeId>> {
        use Err as Skipped;
        use Ok as Wrote;

        let mut entries: Vec<_> = entries.iter().map(atom_entry).collect();

        //git expects tree entries to be sorted
        if entries.len() > 1 {
            entries.sort_unstable();
        }

        let tree = AtomTree { entries };

        if self.ref_exists(&tree, self.refs(RefKind::Content)) {
            return Ok(Skipped(self.atom.spec.id.clone()));
        }

        let id = self.git.write_object(tree)?;
        Ok(Wrote(AtomTreeId(id)))
    }

    /// Method to write atom commits
    pub(super) fn write_atom_commit(&self, AtomTreeId(id): AtomTreeId) -> GitResult<CommittedAtom> {
        let sig = Signature {
            email: EMPTY_SIG.into(),
            name: EMPTY_SIG.into(),
            time: gix::date::Time {
                seconds: 0,
                offset: 0,
                sign: gix::date::time::Sign::Plus,
            },
        };
        let commit = AtomCommit {
            tree: id,
            parents: smallvec::smallvec![],
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: format!("{}: {}", self.atom.spec.id, self.atom.spec.version).into(),
            extra_headers: [
                (ATOM_ORIGIN.into(), self.git.commit.id.to_string().into()),
                (
                    "path".into(),
                    self.path
                        .parent()
                        .unwrap_or(Path::new("/"))
                        .to_string_lossy()
                        .to_string()
                        .into(),
                ),
                ("format".into(), ATOM_FORMAT_VERSION.into()),
            ]
            .into(),
        };
        let id = self.git.write_object(commit.clone())?;
        Ok(CommittedAtom { commit, id })
    }
}

use super::{CommittedAtom, FoundAtom};

impl<'a> CommittedAtom {
    /// Method to write a single reference to the repository
    fn write_ref(
        &'a self,
        atom: &'a AtomContext,
        id: ObjectId,
        atom_ref: AtomRef,
    ) -> GitResult<Reference> {
        use gix::refs::transaction::PreviousValue;

        tracing::debug!("writing atom ref: {}", atom_ref);

        let AtomContext { atom, git, .. } = atom;

        Ok(git.repo.reference(
            format!("refs/{}", atom_ref),
            id,
            PreviousValue::MustNotExist,
            format!(
                "publish: {}: {}-{}",
                atom.spec.id, atom.spec.version, atom_ref
            ),
        )?)
    }
    /// Method to write references for the committed atom
    pub(super) fn write_refs(&'a self, atom: &'a AtomContext) -> GitResult<AtomReferences> {
        let Self { id, .. } = self;

        // filter out the content tree
        let entries: Vec<_> = atom
            .atom
            .entries
            .clone()
            .into_iter()
            .filter_map(|e| e.mode().is_blob().then_some(atom_entry(&e)))
            .collect();

        let spec_tree = AtomTree { entries };
        let spec = atom.git.repo.write_object(spec_tree)?.detach();
        let src = atom.git.commit.id;

        Ok(AtomReferences {
            spec: self.write_ref(atom, spec, atom.refs(RefKind::Spec))?,
            content: self.write_ref(atom, *id, atom.refs(RefKind::Content))?,
            origin: self.write_ref(atom, src, atom.refs(RefKind::Origin))?,
        })
    }
}

use super::{AtomReferences, AtomTreeId, GitContent};

impl<'a> AtomReferences<'a> {
    /// Publish atom's to the specified git remote
    ///
    /// Currently the implementation just calls the `git` binary.
    /// Once `gix` is further along we can use it directly.
    pub(super) fn push(self, atom: &'a AtomContext) -> GitContent {
        let remote = atom.git.remote_str.to_owned();
        let mut tasks = atom.git.push_tasks.borrow_mut();

        for r in [&self.content, &self.spec, &self.origin] {
            let r = r.name().as_bstr().to_string();
            let remote = remote.clone();
            let task = async move {
                let result =
                    git::run_git_command(&["push", &remote, format!("{}:{}", r, r).as_str()])?;

                Ok(result)
            };
            tasks.spawn(task);
        }

        GitContent {
            spec: self.spec.detach(),
            content: self.content.detach(),
            origin: self.origin.detach(),
            path: atom.path.to_path_buf(),
            ref_prefix: atom.ref_prefix.clone(),
        }
    }
}

use gix::Object;
/// Helper function to read a blob from an object
fn read_blob<F, R>(obj: &Object, mut f: F) -> GitResult<R>
where
    F: FnMut(&mut dyn Read) -> io::Result<R>,
{
    let mut reader = obj.data.as_slice();
    Ok(f(&mut reader)?)
}

/// Helper function to create an atom entry from found entries
fn atom_entry(entry: &Entry) -> AtomEntry {
    AtomEntry {
        mode: entry.mode(),
        filename: entry.filename().into(),
        oid: entry.object_id(),
    }
}

impl CommittedAtom {
    /// Return a reference to the commit object representing the committed Atom.
    pub fn commit(&self) -> &AtomCommit {
        &self.commit
    }
    /// Return a reference to the object ID of the committed Atom.
    pub fn tip(&self) -> &ObjectId {
        &self.id
    }
}
