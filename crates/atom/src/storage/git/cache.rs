use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bstr::ByteSlice;
use gix::create::{Kind, Options};
use gix::objs::tree::EntryKind;
use gix::protocol::transport::client::Transport;
use gix::{ObjectId, Remote, Repository, ThreadSafeRepository};
use semver::Version;

use crate::storage::{QueryStore, RemoteAtomCache};
use crate::{Label, Lockfile};

/// The filename of the file used to run nix import logic
pub const NIX_IMPORT_FILE: &str = "atom.nix";
/// The entrypoint attribute to evaluate inside the atom
pub const NIX_ENTRY_KEY: &str = "main";

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

#[derive(Copy, Clone)]
pub struct CacheIds {
    atom: ObjectId,
    locker: Option<ObjectId>,
}

type RemoteName = String;

impl<'a> RemoteAtomCache for &'a Repository {
    type Atom = CacheIds;
    type Error = Error;
    type RemoteHandle = (RemoteName, Remote<'a>);
    type Transport = Box<dyn Transport + Send>;

    fn ensure_remote(
        &self,
        url: &gix::Url,
        transport: &mut Self::Transport,
    ) -> Result<Self::RemoteHandle, Self::Error> {
        use base58::ToBase58;

        let query = format!("{}:{}", super::V1_ROOT, super::V1_ROOT);
        let root = url
            .get_ref(query.as_str(), Some(transport))
            .map_err(Box::new)?;
        let gix::ObjectId::Sha1(id) = super::to_id(root);
        let name: String = id.to_base58();

        let remote = self
            .find_remote(bstr::BString::from(name.to_owned()).as_bstr())
            .unwrap_or(
                self.find_remote(url.to_bstring().as_bstr())
                    .unwrap_or(self.remote_at(url.to_owned())?),
            );

        Ok((name, remote))
    }

    fn resolve_atom_to_cache(
        &self,
        remote: &mut Self::RemoteHandle,
        label: &Label,
        version: &Version,
        transport: &mut Self::Transport,
        resolve_lock: bool,
    ) -> Result<Self::Atom, Self::Error> {
        let (name, remote) = remote;
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
        let commit = self
            .find_commit(id)
            .map_err(Box::new)
            .map_err(super::Error::NoCommit)
            .map_err(Box::new)?;
        let locker = if resolve_lock
            && let Ok(Some(entry)) = commit
                .tree()?
                .lookup_entry_by_path(crate::LOCK_NAME.as_str())
            && entry.mode().kind() == EntryKind::Blob
            && let Ok(entry) = entry.object()
            && let Ok(lock) = toml_edit::de::from_slice::<Lockfile>(&entry.detach().data)
            && let Some(url) = lock.locker.mirror()
        {
            let mut transport = url.get_transport().map_err(Box::new)?;
            let mut remote = self.ensure_remote(url, &mut transport)?;
            self.resolve_atom_to_cache(
                &mut remote,
                lock.locker.label(),
                lock.locker.version(),
                &mut transport,
                false,
            )
            .map_err(|e| tracing::warn!(error = %e, "couldn't resolve locker atom"))
            .map(|r| r.atom)
            .ok()
        } else {
            None
        };
        Ok(CacheIds {
            atom: commit.id,
            locker,
        })
    }

    fn materialize_from_cache(
        &self,
        cache_ids: Self::Atom,
        to_dir: impl AsRef<Path>,
    ) -> Result<tempfile::TempDir, Self::Error> {
        use std::fs;

        use gix::traverse::tree::Recorder;

        let tree = self
            .find_commit(cache_ids.atom)
            .map_err(|e| super::Error::NoCommit(Box::new(e)))
            .map_err(Box::new)?
            .tree()?;
        let mut record = Recorder::default();
        tree.traverse().depthfirst(&mut record)?;
        let tmp = tempfile::TempDir::with_prefix_in("atom-", to_dir)?;
        for entry in record.records {
            let full_path = tmp.as_ref().join(entry.filepath.to_string());
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
                    let blob = self.find_object(entry.oid)?.try_into_blob()?;
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
                    let blob = self.find_object(entry.oid)?.try_into_blob()?;
                    let target = std::str::from_utf8(&blob.data)?;
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target, &full_path)?;
                },
                EntryKind::Commit => {
                    tracing::warn!(ignoring = %full_path.display(), "subrepos not supported in atoms")
                },
            }
        }
        if let Some(id) = cache_ids.locker
            && !tmp
                .as_ref()
                .join(NIX_IMPORT_FILE)
                .try_exists()
                .is_ok_and(|p| p)
            && let Some(entry) = self
                .find_commit(id)
                .map_err(|e| super::Error::NoCommit(Box::new(e)))
                .map_err(Box::new)?
                .tree()?
                .lookup_entry_by_path(NIX_IMPORT_FILE)?
        {
            fs::write(
                tmp.as_ref().join(NIX_IMPORT_FILE),
                entry.object()?.detach().data,
            )?;
        }
        Ok(tmp)
    }
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

/// Acquire a reference to the configured global cache repository
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
