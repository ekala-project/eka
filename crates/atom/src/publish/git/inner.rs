use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use gix::actor::Signature;
use gix::diff::object::Commit as AtomCommit;
use gix::object::tree::Entry;
use gix::objs::WriteTo;
use gix::{ObjectId, Reference};

use super::{AtomContext, AtomRef, GitContext, GitResult, RefKind};
use crate::core::AtomPaths;
use crate::publish::error::git::Error;
use crate::publish::{
    ATOM_FORMAT_VERSION, ATOM_MANIFEST, ATOM_META_REF, ATOM_ORIGIN, ATOM_REF, ATOMIC_ROOT,
    EMPTY_SIG,
};
use crate::store::git;
use crate::{Atom, AtomId, Manifest};
impl<'a> GitContext<'a> {
    /// Method to verify the manifest of an entry
    pub(super) fn verify_manifest(&self, obj: &Object, path: &Path) -> GitResult<Atom> {
        let content = read_blob(obj, |reader| {
            let mut content = String::new();
            reader.read_to_string(&mut content)?;
            Ok(content)
        })?;

        Manifest::get_atom(&content).map_err(|e| Error::Invalid(e, Box::new(path.into())))
    }

    /// Compute the [`ObjectId`] of the given proto-object in memory
    fn _compute_hash(&self, obj: &dyn WriteTo) -> GitResult<ObjectId> {
        use gix::objs;

        let mut buf = Vec::with_capacity(obj.size() as usize);

        obj.write_to(&mut buf)?;

        let oid = objs::compute_hash(self.repo.object_hash(), obj.kind(), buf.as_ref());

        Ok(oid?)
    }

    /// Helper function to write an object to the repository
    fn write_object(&self, obj: impl WriteTo) -> GitResult<gix::ObjectId> {
        Ok(self.repo.write_object(obj).map(gix::Id::detach)?)
    }

    /// Helper function to return an entry by path from the repo tree
    ///
    /// # Errors
    ///
    /// This function will return an error if the call to
    /// [`gix::object::tree::Tree::lookup_entry`] fails.
    pub fn tree_search(&self, path: &Path) -> GitResult<Option<Entry<'a>>> {
        let search = path.components().map(|c| c.as_os_str().as_bytes());
        Ok(self.tree.clone().lookup_entry(search)?)
    }

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
                let id = AtomId::construct(&self.commit, spec.tag.clone())?;
                if self.root != *id.root() {
                    return Err(Error::InconsistentRoot {
                        remote: self.root,
                        atom: *id.root(),
                    });
                };
                Ok((
                    FoundAtom {
                        spec,
                        tag: id,
                        tree_id,
                        spec_id,
                    },
                    paths,
                ))
            })
    }
}

use semver::Version;

impl<'a> AtomRef<'a> {
    fn new(tag: String, kind: RefKind, version: &'a Version) -> Self {
        AtomRef { tag, kind, version }
    }
}

use std::fmt;

impl<'a> fmt::Display for AtomRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RefKind::Content => {
                write!(
                    f,
                    "{}/{}/{}/{}",
                    ATOMIC_ROOT, ATOM_REF, self.tag, self.version
                )
            },
            RefKind::Origin => write!(
                f,
                "{}/{}/{}/{}/{}",
                ATOMIC_ROOT, ATOM_META_REF, self.tag, self.version, ATOM_ORIGIN
            ),
            RefKind::Spec => write!(
                f,
                "{}/{}/{}/{}/{}",
                ATOMIC_ROOT, ATOM_META_REF, self.tag, self.version, ATOM_MANIFEST
            ),
        }
    }
}

impl<'a> AtomContext<'a> {
    pub(super) fn refs(&self, kind: RefKind) -> AtomRef<'_> {
        AtomRef::new(
            self.atom.tag.tag().to_string(),
            kind,
            &self.atom.spec.version,
        )
    }

    /// Method to write atom commits
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
            message: format!("{}: {}", self.atom.spec.tag, self.atom.spec.version).into(),
            extra_headers: [
                (ATOM_ORIGIN.into(), self.git.commit.id.to_string().into()),
                (
                    "path".into(),
                    self.paths
                        .content()
                        .to_str()
                        .and_then(|s| if s.is_empty() { None } else { Some(s) })
                        .unwrap_or("/")
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

/// Method to write a single reference to the repository
fn write_ref<'a>(
    atom: &'a AtomContext,
    id: ObjectId,
    atom_ref: AtomRef,
) -> GitResult<Reference<'a>> {
    use gix::refs::transaction::PreviousValue;

    tracing::debug!("writing atom ref: {}", atom_ref);

    let AtomContext { atom, git, .. } = atom;

    Ok(git.repo.reference(
        format!("refs/{atom_ref}"),
        id,
        PreviousValue::MustNotExist,
        format!(
            "publish: {}: {}-{}",
            atom.spec.tag, atom.spec.version, atom_ref
        ),
    )?)
}
use super::{CommittedAtom, FoundAtom};

impl<'a> CommittedAtom {
    /// Method to write references for the committed atom
    pub(super) fn write_refs(&'a self, atom: &'a AtomContext) -> GitResult<AtomReferences<'a>> {
        let Self { id, .. } = self;

        // filter out the content tree
        let spec = atom.atom.spec_id;
        let src = atom.git.commit.id;

        Ok(AtomReferences {
            spec: write_ref(atom, spec, atom.refs(RefKind::Spec))?,
            content: write_ref(atom, *id, atom.refs(RefKind::Content))?,
            origin: write_ref(atom, src, atom.refs(RefKind::Origin))?,
        })
    }
}

use super::{AtomReferences, GitContent};

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
                let result = git::run_git_command(&["push", &remote, format!("{r}:{r}").as_str()])?;

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

use gix::Object;
/// Helper function to read a blob from an object
fn read_blob<F, R>(obj: &Object, mut f: F) -> GitResult<R>
where
    F: FnMut(&mut dyn Read) -> io::Result<R>,
{
    let mut reader = obj.data.as_slice();
    Ok(f(&mut reader)?)
}

impl CommittedAtom {
    #[must_use]
    /// Returns a reference to the commit of this [`CommittedAtom`].
    pub fn commit(&self) -> &AtomCommit {
        &self.commit
    }

    #[must_use]
    /// Returns a reference to the tip of this [`CommittedAtom`].
    pub fn tip(&self) -> &ObjectId {
        &self.id
    }
}
