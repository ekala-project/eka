use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;

use anyhow::Context;
use gix::ObjectId;
use gix::prelude::ReferenceExt;
use tempfile::{Builder, NamedTempFile};

use crate::publish::{Content, Publish, Record};
use crate::store::git;

trait MockAtom {
    fn mock(
        &self,
        id: &str,
        version: &str,
        description: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error>;
}

impl MockAtom for gix::Repository {
    fn mock(
        &self,
        id: &str,
        version: &str,
        description: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error> {
        use gix::objs::Tree;
        use gix::objs::tree::{Entry, EntryMode};
        use semver::Version;
        use toml_edit::ser;

        use crate::{Atom, Manifest};

        let work_dir = self.work_dir().context("No workdir")?;
        let mut atom_file = Builder::new()
            .suffix(crate::ATOM_EXT.as_str())
            .tempfile_in(work_dir)?;
        let manifest = Manifest {
            atom: Atom {
                id: id.try_into()?,
                version: Version::from_str(version)?,
                description: (!description.is_empty()).then_some(description.into()),
            },
            deps: None,
        };

        let buf = ser::to_string_pretty(&manifest)?;
        atom_file.write_all(buf.as_bytes())?;

        let path = atom_file.as_ref().to_path_buf();

        let mode = atom_file.as_file().metadata()?.mode();
        let filename = path.strip_prefix(work_dir)?.display().to_string().into();
        let oid = self.write_blob(buf.as_bytes())?.detach();
        let entry = Entry {
            mode: EntryMode(mode as u16),
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let tree_id = self.write_object(tree)?;

        let head = self.head_id()?;
        let head_ref = self.head_ref()?.context("detached HEAD")?;

        let atom_oid = self
            .commit(
                head_ref.name().as_bstr(),
                format!("init: {}", id),
                tree_id,
                vec![head],
            )?
            .detach();

        Ok((atom_file, atom_oid))
    }
}

#[tokio::test]
async fn publish_atom() -> Result<(), anyhow::Error> {
    use crate::id::Id;
    use crate::publish::git::{Builder, GitPublisher};
    use crate::store::{Init, QueryStore};
    let (repo, _remote) = git::test::init_repo_and_remote()?;
    let repo = gix::open(repo.as_ref())?;
    let remote = repo.find_remote("origin")?;
    remote.ekala_init()?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"))?;

    let id = "foo";
    let (file_path, src) = repo.mock(id, "0.1.0", "some atom")?;

    let (paths, publisher) = GitPublisher::new(&repo, "origin", "HEAD")?.build()?;

    let path = paths.get(&Id::try_from(id)?).context("path is messed up")?;
    let result = publisher.publish_atom(path)?;
    let mut errors = Vec::with_capacity(1);
    publisher.await_pushes(&mut errors).await;
    (!errors.is_empty()).then_some(0).context("push errors")?;

    let content = match result {
        Ok(Record {
            content: Content::Git(c),
            ..
        }) => c,
        _ => return Err(anyhow::anyhow!("atom publishing failed")),
    };

    let origin_id = content.origin.attach(&repo).into_fully_peeled_id()?;
    let content_ref = content.content.attach(&repo);
    let content_tree = repo
        .find_commit(content_ref.into_fully_peeled_id()?)?
        .tree()?
        .detach();
    let origin_tree = repo.find_commit(origin_id.detach())?.tree()?;
    let spec_id = content.spec.attach(&repo).into_fully_peeled_id()?;
    let spec_tree = repo.find_tree(spec_id)?;
    let prefix = format!("{}/{}", crate::publish::ATOM_REF_TOP_LEVEL, id);
    let path = file_path
        .path()
        .strip_prefix(repo.work_dir().context("")?)?;

    assert_eq!(origin_id, src);
    assert_eq!(path, content.path);
    assert_eq!(content.ref_prefix, prefix);

    // our repo has no other contents but the atom so all 3 trees should be equal
    // this is not always the case, but its a good simplifying assumption
    assert_eq!(content_tree.data, origin_tree.detach().data);
    assert_eq!(content_tree.data, spec_tree.detach().data);

    Ok(())
}
