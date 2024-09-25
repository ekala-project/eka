use super::{
    AtomContext, GitAtomId, GitRecord, GitResult, PublishGitContext, ATOM_FORMAT_VERSION,
    ATOM_REF_TOP_LEVEL, ATOM_SPEC_REF, ATOM_SRC_REF, ATOM_TIP_REF, EMPTY,
};
use crate::{publish::error::GitError, publish::Content, Atom, AtomId, Manifest};

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
    path::Path,
};

use crate::id::Id;
impl<'a> PublishGitContext<'a> {
    /// Method to publish an atom
    pub fn publish_atom(&self, path: &Path) -> GitResult<Result<GitRecord, Id>> {
        use Err as Skipped;
        use Ok as Published;

        let dir = path.with_extension("");
        let FoundAtom(atom, atom_entry) = self.find_and_verify_atom(path)?;

        let id = AtomId::compute(&self.commit, atom.id.clone())?;

        let context = AtomContext::set(&atom, &id, &dir, self);

        let atom_dir_entry = context.maybe_dir();

        let tree_ids = match context.write_atom_trees(&atom_entry, atom_dir_entry)? {
            Ok(t) => t,
            Skipped(id) => return Ok(Skipped(id)),
        };

        let refs = context
            .write_atom_commits(tree_ids)?
            .write_refs(&context)?
            .push(self);

        Ok(Published(GitRecord {
            id,
            content: Content::Git(refs),
        }))
    }

    /// Method to verify the manifest of an entry
    fn verify_manifest(&self, entry: &Entry, path: &Path) -> GitResult<Atom> {
        if !entry.mode().is_blob() {
            return Err(GitError::NotAFile(path.into()));
        }

        let content = read_blob(entry, |reader| {
            let mut content = String::new();
            reader.read_to_string(&mut content)?;
            Ok(content)
        })?;

        Manifest::get_atom(&content).map_err(GitError::Invalid)
    }

    /// Compute the ObjectId of the given object without writing it to the repo
    fn compute_hash(&self, obj: &dyn WriteTo) -> Option<ObjectId> {
        use gix::objs;
        use std::io::Cursor;

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);

        obj.write_to(&mut cursor).ok()?;

        let oid = objs::compute_hash(self.repo.object_hash(), obj.kind(), buf.as_slice());

        Some(oid)
    }

    /// Helper function to write an object to the repository
    fn write_object(&self, obj: impl WriteTo) -> GitResult<gix::ObjectId> {
        Ok(self.repo.write_object(obj).map(|id| id.detach())?)
    }

    /// Helper function to return an entry by path from the repo tree
    fn tree_search(&self, path: &Path) -> GitResult<Option<Entry<'a>>> {
        Ok(self.tree.clone().peel_to_entry_by_path(path)?)
    }

    fn find_and_verify_atom(&self, path: &Path) -> GitResult<FoundAtom> {
        let entry = self
            .tree_search(path)?
            .ok_or(GitError::NotAFile(path.into()))?;

        self.verify_manifest(&entry, path)
            .map(|atom| FoundAtom(atom, entry))
    }
}

enum RefKind {
    Spec,
    Tip,
    Src,
}

struct AtomRef<'a> {
    prefix: &'a str,
    kind: RefKind,
}

impl<'a> AtomRef<'a> {
    fn new(kind: RefKind, prefix: &'a str) -> Self {
        AtomRef { prefix, kind }
    }
}

use std::fmt;

impl<'a> fmt::Display for AtomRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RefKind::Spec => write!(f, "{}/{}", self.prefix, ATOM_SPEC_REF),
            RefKind::Tip => write!(f, "{}/{}", self.prefix, ATOM_TIP_REF),
            RefKind::Src => write!(f, "{}/{}", self.prefix, ATOM_SRC_REF),
        }
    }
}

use crate::publish::MaybeSkipped;

impl<'a> AtomContext<'a> {
    fn set(
        atom: &'a Atom,
        id: &'a GitAtomId,
        path: &'a Path,
        context: &'a PublishGitContext,
    ) -> Self {
        let prefix = format!("{}/{}/{}", ATOM_REF_TOP_LEVEL, id, atom.version);
        Self {
            atom,
            path,
            context,
            prefix,
        }
    }

    fn refs(&self, kind: RefKind) -> AtomRef {
        AtomRef::new(kind, &self.prefix)
    }

    fn maybe_dir(&self) -> Option<Entry> {
        match self.context.tree_search(self.path).ok()? {
            Some(entry) => entry.mode().is_tree().then_some(entry),
            _ => None,
        }
    }

    fn ref_exists(&self, tree: &AtomTree, atom_ref: AtomRef) -> bool {
        let id = self.context.compute_hash(tree);
        if let Some(id) = id {
            self.context.repo.find_tree(id).is_ok()
                && self
                    .context
                    .repo
                    .find_reference(&atom_ref.to_string())
                    .is_ok()
        } else {
            false
        }
    }

    /// Method to write the atom tree object
    fn write_atom_trees(
        &self,
        atom: &Entry,
        dir: Option<Entry>,
    ) -> GitResult<MaybeSkipped<AtomTreeIds>> {
        use Err as Skipped;
        use Ok as Wrote;

        let mut entries: Vec<AtomEntry> = Vec::with_capacity(2);

        let spec_tree = atom_tree(&mut entries, atom);

        let spec_exists = self.ref_exists(&spec_tree, self.refs(RefKind::Spec));

        if dir.is_none() && spec_exists {
            return Ok(Skipped(self.atom.id.clone()));
        }

        if let Some(entry) = dir {
            let dir_tree = atom_tree(&mut entries, &entry);
            if self.ref_exists(&dir_tree, self.refs(RefKind::Tip)) && spec_exists {
                return Ok(Skipped(self.atom.id.clone()));
            }
            let spec = self.context.write_object(spec_tree)?;
            let dir = Some(self.context.write_object(dir_tree)?);
            Ok(Wrote(AtomTreeIds { spec, dir }))
        } else {
            let spec = self.context.write_object(spec_tree)?;
            Ok(Wrote(AtomTreeIds { spec, dir: None }))
        }
    }

    /// Method to write atom commits
    fn write_atom_commits(
        &self,
        AtomTreeIds { spec, dir }: AtomTreeIds,
    ) -> GitResult<CommittedAtom> {
        let sig = Signature {
            email: EMPTY.into(),
            name: EMPTY.into(),
            time: gix::date::Time {
                seconds: 0,
                offset: 0,
                sign: gix::date::time::Sign::Plus,
            },
        };
        let commit = AtomCommit {
            tree: spec,
            parents: Vec::new().into(),
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: format!("{}: {}", self.atom.id, self.atom.version).into(),
            extra_headers: vec![
                ("origin".into(), self.context.commit.id().as_bytes().into()),
                ("version".into(), ATOM_FORMAT_VERSION.into()),
            ],
        };
        let src = self.context.commit.id;
        let tip = self.context.write_object(commit.clone())?;
        if let Some(tree) = dir {
            let commit = AtomCommit {
                tree,
                parents: vec![tip].into(),
                ..commit
            };
            let tip = self.context.write_object(commit.clone())?;
            Ok(CommittedAtom { commit, tip, src })
        } else {
            Ok(CommittedAtom { commit, tip, src })
        }
    }
}

use super::{CommittedAtom, FoundAtom};

impl<'a> CommittedAtom {
    /// Method to write a single reference to the repository
    fn write_ref(
        &'a self,
        context: &'a AtomContext,
        id: ObjectId,
        atom_ref: AtomRef,
    ) -> GitResult<Reference> {
        use gix::refs::transaction::PreviousValue;

        Ok(context.context.repo.reference(
            format!("refs/{}", atom_ref),
            id,
            PreviousValue::MustNotExist,
            format!(
                "publish: {}: {}-{}",
                context.atom.id, context.atom.version, atom_ref
            ),
        )?)
    }
    /// Method to write references for the committed atom
    fn write_refs(&'a self, context: &'a AtomContext) -> GitResult<AtomReferences> {
        let Self { commit, tip, src } = self;

        Ok(if let Some(spec) = commit.parents.first() {
            AtomReferences {
                spec: self.write_ref(context, *spec, context.refs(RefKind::Spec))?,
                tip: self.write_ref(context, *tip, context.refs(RefKind::Tip))?,
                src: self.write_ref(context, *src, context.refs(RefKind::Src))?,
            }
        } else {
            AtomReferences {
                spec: self.write_ref(context, *tip, context.refs(RefKind::Spec))?,
                tip: self.write_ref(context, *tip, context.refs(RefKind::Tip))?,
                src: self.write_ref(context, *src, context.refs(RefKind::Src))?,
            }
        })
    }
}

use super::{AtomReferences, AtomTreeIds, GitContent};

impl<'a> AtomReferences<'a> {
    /// Publish atom's to the specified git remote
    ///
    /// Currently the implementation just calls the `git` binary.
    /// Once `gix` is further along we can use it directly.
    fn push(self, context: &'a PublishGitContext) -> GitContent {
        let remote = context.remote_str.to_owned();
        let mut tasks = context.push_tasks.borrow_mut();

        for r in [&self.tip, &self.spec, &self.src] {
            let r = r.name().as_bstr().to_string();
            let remote = remote.clone();
            let task = async move {
                let result = run_git_command(&["push", &remote, format!("{}:{}", r, r).as_str()])?;

                Ok(result)
            };
            tasks.spawn(task);
        }

        GitContent {
            spec: self.spec.detach(),
            tip: self.tip.detach(),
            src: self.src.detach(),
        }
    }
}

/// Helper function to read a blob from an entry
fn read_blob<F, R>(entry: &Entry, mut f: F) -> GitResult<R>
where
    F: FnMut(&mut dyn Read) -> io::Result<R>,
{
    let object = entry.object()?;
    let mut reader = object.data.as_slice();
    Ok(f(&mut reader)?)
}

/// Helper function to create an atom tree from entries
fn atom_tree(entries: &mut Vec<AtomEntry>, atom: &Entry) -> AtomTree {
    entries.push(AtomEntry {
        mode: atom.mode(),
        filename: atom.filename().into(),
        oid: atom.object_id(),
    });

    // git expects tree entries to be sorted
    if entries.len() > 1 {
        entries.sort_unstable();
    }

    AtomTree {
        entries: entries.clone(),
    }
}

fn run_git_command(args: &[&str]) -> io::Result<Vec<u8>> {
    use std::process::Command;
    let output = Command::new("git").args(args).output()?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

impl CommittedAtom {
    pub fn commit(&self) -> &AtomCommit {
        &self.commit
    }
    pub fn tip(&self) -> &ObjectId {
        &self.tip
    }
    pub fn src(&self) -> &ObjectId {
        &self.src
    }
}
