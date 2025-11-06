use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;

use anyhow::Context;
use gix::prelude::ReferenceExt;
use gix::{ObjectId, ThreadSafeRepository};
use tempfile::{Builder, NamedTempFile};

use super::super::{Content, Publish, Record};
use crate::storage::{Init, git};

//================================================================================================
// Traits
//================================================================================================

trait MockAtom {
    async fn mock(
        &self,
        id: &str,
        version: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error>;
}

//================================================================================================
// Impls
//================================================================================================

impl MockAtom for gix::ThreadSafeRepository {
    async fn mock(
        &self,
        label: &str,
        version: &str,
    ) -> Result<(NamedTempFile, ObjectId), anyhow::Error> {
        use gix::objs::Tree;
        use gix::objs::tree::Entry;
        use semver::Version;

        use crate::EkalaManager;

        let repo = self.to_thread_local();
        let work_dir = repo.workdir().context("No workdir")?;
        let atom_dir = Builder::new().tempdir_in(work_dir)?;
        let atom_file = atom_dir.as_ref().join(crate::ATOM_MANIFEST_NAME.as_str());

        self.ekala_init(None)?;
        let mut ekala = EkalaManager::new(self)?;
        ekala
            .new_atom_at_path(label.try_into()?, &atom_dir, Version::from_str(version)?)
            .await?;

        let buf = std::fs::read_to_string(&atom_file)?;

        let mode = atom_file.metadata()?.mode();
        let filename = atom_file
            .strip_prefix(&atom_dir)?
            .display()
            .to_string()
            .into();
        let oid = repo.write_blob(buf.as_bytes())?.detach();
        let entry = Entry {
            mode: TryFrom::try_from(mode)
                .map_err(|m| anyhow::anyhow!("invalid entry mode: {}", m))?,
            filename,
            oid,
        };

        let tree = Tree {
            entries: vec![entry],
        };

        let oid = repo.write_object(tree)?.detach();

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

        let oid = repo.write_object(tree)?.detach();

        let head = repo.head_id()?;
        let head_ref = repo.head_ref()?.context("detached HEAD")?;

        let atom_oid = repo
            .commit(
                head_ref.name().as_bstr(),
                format!("init: {}", label),
                oid,
                vec![head],
            )?
            .detach();

        let tmp = NamedTempFile::from_parts(
            std::fs::File::open(&atom_file)?,
            tempfile::TempPath::from_path(atom_file),
        );
        Ok((tmp, atom_oid))
    }
}

//================================================================================================
// Functions
//================================================================================================

#[tokio::test]
async fn publish_atom() -> Result<(), anyhow::Error> {
    use super::{Builder, GitPublisher};
    use crate::id::Label;
    use crate::storage::{Init, QueryStore};
    let (repo, _remote) = git::test::init_repo_and_remote()?;
    let safe = ThreadSafeRepository::open(repo.as_ref())?;
    let repo = safe.to_thread_local();
    let remote = repo.find_remote("origin")?;
    let progress = &tracing::info_span!("test");
    remote.ekala_init(None)?;
    remote.get_refs(Some("refs/heads/*:refs/heads/*"), None)?;

    let label = "foo";
    let (file_path, src) = safe.mock(label, "0.1.0").await?;

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
