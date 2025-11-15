use std::path::PathBuf;
use std::sync::OnceLock;

use bstr::ByteSlice;
use gix::ThreadSafeRepository;
use gix::config::File;
use gix::create::{Kind, Options};
use gix::protocol::transport::client::Transport;
use gix::remote::Direction;
use semver::Version;
use tempfile::TempDir;

use crate::Label;
use crate::storage::QueryStore;

static CACHE_REPO: OnceLock<Option<ThreadSafeRepository>> = OnceLock::new();

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("couldn't open cache repository: {0}")]
    Repo(PathBuf),
    #[error(transparent)]
    Init(#[from] Box<gix::init::Error>),
    #[error(transparent)]
    RemoteInit(#[from] gix::remote::init::Error),
    #[error(transparent)]
    GitStorage(#[from] Box<super::Error>),
    #[error(transparent)]
    GitConfig(#[from] gix::config::file::init::from_paths::Error),
    #[error(transparent)]
    SaveRemote(#[from] Box<gix::remote::save::AsError>),
    #[error(transparent)]
    GitTree(#[from] gix::object::commit::Error),
    #[error(transparent)]
    Traverse(#[from] gix::traverse::tree::breadthfirst::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Find(#[from] gix::object::find::existing::Error),
    #[error(transparent)]
    TryObject(#[from] gix::object::try_into::Error),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
}

fn get_cache() -> Result<ThreadSafeRepository, Error> {
    let cache_dir = config::CONFIG.cache.root.join("git");
    Ok(ThreadSafeRepository::open(&cache_dir)
        .or_else(|_| {
            ThreadSafeRepository::init(
                cache_dir,
                Kind::Bare,
                Options {
                    destination_must_be_empty: true,
                    ..Default::default()
                },
            )
        })
        .map_err(Box::new)?)
}

/// Get the cache repository used to store queried atoms locally
pub fn repo() -> Result<&'static ThreadSafeRepository, Error> {
    let mut error = None;
    let repo = CACHE_REPO.get_or_init(|| match get_cache() {
        Ok(repo) => Some(repo),
        Err(e) => {
            error = Some(e);
            None
        },
    });
    if let Some(e) = error {
        Err(e)
    } else if let Some(repo) = repo {
        Ok(repo)
    } else {
        let cache_dir = config::CONFIG.cache.root.join("git");
        Err(Error::Repo(cache_dir))
    }
}

/// Get a specific atom from a remote, cache it in the cache repository, and build and return a
/// temporary directory of its contents
pub fn get_atom(
    url: &gix::Url,
    label: &Label,
    version: &Version,
    transport: &mut Box<dyn Transport + Send>,
) -> Result<TempDir, Error> {
    use std::fs;

    use base58::ToBase58;
    use gix::config::Source;
    use gix::objs::tree::EntryKind;
    use gix::traverse::tree::Recorder;

    let repo = repo()?.to_thread_local();

    let query = format!("{}:{}", super::V1_ROOT, super::V1_ROOT);
    let root = url
        .get_ref(query.as_str(), Some(transport))
        .map_err(Box::new)?;
    let gix::ObjectId::Sha1(id) = super::to_id(root);
    let name: String = id.to_base58();
    let mut remote = repo
        .find_remote(bstr::BString::from(name.to_owned()).as_bstr())
        .unwrap_or(
            repo.find_remote(url.to_bstring().as_bstr())
                .unwrap_or(repo.remote_at(url.to_owned())?),
        );
    if remote.url(Direction::Fetch) != Some(url) || remote.name().is_none() {
        let config_file = repo.git_dir().join("config");
        let mut config = File::from_path_no_includes(config_file.clone(), Source::Local)?;
        remote
            .save_as_to(name.to_owned(), &mut config)
            .map_err(Box::new)?;
    }

    let query = format!(
        "{}/{}/{}:refs/{}/{}/{}",
        crate::ATOM_REFS.as_str(),
        label,
        version,
        name,
        label,
        version
    );
    let r = remote
        .get_ref(query.as_str(), Some(transport))
        .map_err(Box::new)?;
    let id = super::to_id(r);
    let tree = repo
        .find_commit(id)
        .map_err(Box::new)
        .map_err(super::Error::NoCommit)
        .map_err(Box::new)?
        .tree()?;
    let mut record = Recorder::default();
    tree.traverse().depthfirst(&mut record)?;
    let tmp = tempfile::tempdir()?;
    for entry in record.records {
        let full_path = tmp.as_ref().join(entry.filepath.to_string());
        tracing::info!(kind = ?entry.mode.kind(), "{}", &full_path.display());
        match entry.mode.kind() {
            EntryKind::Tree => {
                if full_path.try_exists().is_ok_and(|p| !p) {
                    fs::create_dir_all(full_path)?;
                }
            },
            EntryKind::Blob | EntryKind::BlobExecutable => {
                if let Some(parent) = full_path.parent()
                    && parent.try_exists().is_ok_and(|p| !p)
                {
                    fs::create_dir_all(parent)?;
                }
                let blob = repo.find_object(entry.oid)?.try_into_blob()?;
                fs::write(&full_path, blob.detach().data)?;

                if entry.mode.is_executable() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perms = fs::metadata(&full_path)?.permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&full_path, perms)?;
                    }
                    // TODO: Windows?
                }
            },
            EntryKind::Link => {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let blob = repo.find_object(entry.oid)?.try_into_blob()?;
                let target = std::str::from_utf8(&blob.data)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &full_path)?;
            },
            EntryKind::Commit => {
                tracing::warn!(ignoring = %full_path.display(), "subrepos not supported in atoms")
            },
        }
    }
    Ok(tmp)
}
