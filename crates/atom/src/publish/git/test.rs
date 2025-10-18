use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;

use anyhow::Context;
use gix::ObjectId;
use gix::prelude::ReferenceExt;
use tempfile::{Builder, NamedTempFile};

use crate::publish::{Content, Publish, Record};
use crate::store::git;

//================================================================================================
// Traits
//================================================================================================

trait MockAtom {
    fn mock(
        &self,
        id: &str,
        version: &str,
        description: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error>;
}

//================================================================================================
// Impls
//================================================================================================

impl MockAtom for gix::Repository {
    fn mock(
        &self,
        label: &str,
        version: &str,
        description: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error> {
        use gix::objs::Tree;
        use gix::objs::tree::Entry;
        use semver::Version;
        use toml_edit::ser;

        use crate::{Atom, Manifest};

        let work_dir = self.workdir().context("No workdir")?;
        let atom_dir = Builder::new().tempdir_in(work_dir)?;
        let mut atom_file = Builder::new()
            .prefix("atom")
            .rand_bytes(0)
            .suffix(".toml")
            .tempfile_in(&atom_dir)?;

        let manifest = Manifest {
            package: Atom {
                label: label.try_into()?,
                version: Version::from_str(version)?,
                description: (!description.is_empty()).then_some(description.into()),
                sets: HashMap::new(),
            },
            deps: Default::default(),
        };

        let buf = ser::to_string_pretty(&manifest)?;
        atom_file.write_all(buf.as_bytes())?;

        let path = atom_file.as_ref().to_path_buf();

        let mode = atom_file.as_file().metadata()?.mode();
        let filename = path.strip_prefix(&atom_dir)?.display().to_string().into();
        let oid = self.write_blob(buf.as_bytes())?.detach();
        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let oid = self.write_object(tree)?.detach();

        let filename = atom_dir
            .as_ref()
            .to_path_buf()
            .strip_prefix(work_dir)?
            .display()
            .to_string()
            .into();

        let entry = Entry {
            mode: TryFrom::try_from(0o40000)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let oid = self.write_object(tree)?.detach();

        let head = self.head_id()?;
        let head_ref = self.head_ref()?.context("detached HEAD")?;

        let atom_oid = self
            .commit(
                head_ref.name().as_bstr(),
                format!("init: {}", label),
                oid,
                vec![head],
            )?
            .detach();

        Ok((atom_file, atom_oid))
    }
}

//================================================================================================
// Functions
//================================================================================================

#[tokio::test]
async fn publish_atom() -> Result<(), anyhow::Error> {
    use crate::id::Label;
    use crate::publish::git::{Builder, GitPublisher};
    use crate::store::{Init, QueryStore};
    let (repo, _remote) = git::test::init_repo_and_remote()?;
    let repo = gix::open(repo.as_ref())?;
    let remote = repo.find_remote("origin")?;
    let progress = &tracing::info_span!("test");
    remote.ekala_init("foo", None)?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;

    let label = "foo";
    let (file_path, src) = repo.mock(label, "0.1.0", "some atom")?;

    let (paths, publisher) = GitPublisher::new(&repo, "origin", "HEAD", progress)?.build()?;
    let path = paths
        .get(&Label::try_from(label)?)
        .context("path is messed up")?;
    let result = publisher.publish_atom(path, &HashMap::new())?;
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
    let dir = file_path.as_ref().to_path_buf();
    let dir = dir
        .parent()
        .and_then(|f| f.file_name())
        .ok_or(anyhow::anyhow!("no parent directory"))?;
    let origin_tree = repo
        .find_commit(origin_id.detach())?
        .tree()?
        .lookup_entry_by_path(dir)?
        .ok_or(anyhow::anyhow!("no tree in orgin"))?
        .object()?;
    let path = file_path.path().strip_prefix(repo.workdir().context("")?)?;

    assert_eq!(origin_id, src);
    assert_eq!(path, content.path);

    assert_eq!(content_tree.data, origin_tree.data);

    Ok(())
}
