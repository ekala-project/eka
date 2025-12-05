use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf, StripPrefixError};
use std::sync::OnceLock;

use bstr::ByteSlice;
use gix::create::{Kind, Options};
use gix::objs::tree::EntryKind;
use gix::protocol::transport::client::Transport;
use gix::{Commit, ObjectId, Remote, Repository, ThreadSafeRepository};
use package::publish::git;
use semver::{BuildMetadata, Prerelease, Version};
use storage::git::NULLROOT;
use storage::{QueryStore, RemoteAtomCache};

use crate::package::AtomError;
use crate::storage::git::Root;
use crate::{AtomId, Compute, DocError, Genesis, Label, ValidManifest, package, storage};

/// The filename of the file used to run nix import logic
pub const NIX_IMPORT_FILE: &str = "atom.nix";
/// The entrypoint attribute to evaluate inside the atom
pub const NIX_ENTRY_KEY: &str = "main";

static CACHE_REPO: OnceLock<Option<ThreadSafeRepository>> = OnceLock::new();

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("couldn't open cache repository: {0}")]
    Repo(PathBuf),
    #[error("the path passed is not an atom: {0}")]
    NotAnAtom(PathBuf),
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
    #[error(transparent)]
    RepoWrite(#[from] gix::object::write::Error),
    #[error(transparent)]
    Ignore(#[from] ignore::Error),
    #[error(transparent)]
    WriteAtom(#[from] Box<package::publish::error::git::Error>),
    #[error("couldn't determine filemode: {0}")]
    Mode(u32),
    #[error(transparent)]
    Atom(#[from] AtomError),
    #[error(transparent)]
    Doc(#[from] DocError),
    #[error(transparent)]
    SemverParse(#[from] semver::Error),
    #[error(transparent)]
    Prefix(#[from] StripPrefixError),
    #[error("Directory depth exceeds maximum of {MAX_DEPTH}")]
    RecursionLimit,
    #[error("Invalid filename")]
    InvalidFile,
}

impl<'a> RemoteAtomCache for &'a Repository {
    type Atom = Commit<'a>;
    type Error = Error;
    type RemoteHandle = (Root, Remote<'a>);
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
        let id = super::to_id(root);
        let gix::ObjectId::Sha1(oid) = id;
        let name: String = oid.to_base58();
        let root = Root(id);

        let remote = self
            .find_remote(bstr::BString::from(name.to_owned()).as_bstr())
            .unwrap_or(
                self.find_remote(url.to_bstring().as_bstr())
                    .unwrap_or(self.remote_at(url.to_owned())?),
            );

        Ok((root, remote))
    }

    fn resolve_atom_to_cache(
        &self,
        remote: &mut Self::RemoteHandle,
        label: &Label,
        version: &Version,
        transport: &mut Self::Transport,
    ) -> Result<Self::Atom, Self::Error> {
        let (root, remote) = remote;
        let id = AtomId::from((*root, label.to_owned()));
        let cache_ref = format!("refs/{}/{}", id.compute_hash(), version);
        let query = format!(
            "{}/{}/{}:{}",
            crate::ATOM_REFS.as_str(),
            label,
            version,
            cache_ref
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
        Ok(commit)
    }

    fn materialize_from_cache(
        &self,
        cached: Self::Atom,
        to_dir: impl AsRef<Path>,
    ) -> Result<tempfile::TempDir, Self::Error> {
        use std::fs;

        use gix::traverse::tree::Recorder;

        let tree = cached.tree()?;
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

        Ok(tmp)
    }

    fn path_to_cache(&self, path: impl AsRef<Path>) -> Result<(Version, Self::Atom), Self::Error> {
        let manifest_path = path.as_ref().join(crate::ATOM_MANIFEST_NAME.as_str());
        if manifest_path.try_exists().is_ok_and(|b| b) {
            return Err(Error::NotAnAtom(manifest_path));
        }

        let atom = ValidManifest::get_atom(
            std::fs::read(&manifest_path)?
                .to_str()
                .map_err(DocError::Utf8)?,
        )?;

        let root = if let Some(g) = gix::discover(path.as_ref()).ok().and_then(|r| {
            r.head_commit()
                .ok()
                .and_then(|c| c.calculate_genesis().ok())
        }) {
            g
        } else {
            NULLROOT
        };

        let (label, mut version) = atom.take();

        let id = AtomId::from((root, label));
        let entry_map = collect_entries(path.as_ref())?;
        let tree =
            build_tree_recursive(self, PathBuf::new().as_path(), &entry_map, path.as_ref(), 0)?;

        version.pre = Prerelease::new(format!("dev.{}", tree.to_hex_with_len(10)).as_str())?;
        version.build = BuildMetadata::EMPTY;

        let obj = *git::write_atom_commit_to_repo(
            self,
            tree,
            id.label(),
            &version,
            root.to_hex().to_string(),
        )
        .map_err(Box::new)?
        .tip();

        let digest = id.compute_hash();

        self.reference(
            format!("refs/{}/{}", digest, version),
            obj,
            gix::refs::transaction::PreviousValue::ExistingMustMatch(gix::refs::Target::Object(
                obj,
            )),
            format!("atom({}): {}", digest, version),
        )
        .map_err(package::publish::error::git::Error::RefUpdateFailed)
        .map_err(Box::new)?;

        Ok((
            version,
            self.find_commit(obj)
                .map_err(package::publish::error::git::Error::NoCommit)
                .map_err(Box::new)?,
        ))
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

// Structure to hold our collected entries
#[derive(Debug)]
struct FsEntry {
    path: PathBuf,
    is_dir: bool,
}

// 1. Traverse directory with ignore crate
fn collect_entries(root: &Path) -> Result<HashMap<PathBuf, Vec<FsEntry>>, Error> {
    use ignore::Walk;
    let mut entries_by_dir: HashMap<PathBuf, Vec<FsEntry>> = HashMap::new();

    for result in Walk::new(root) {
        let entry = result?;
        let path = entry.path().strip_prefix(root)?.to_path_buf();
        let parent = path.parent().unwrap_or(Path::new("")).to_path_buf();

        let fs_entry = if path.is_dir() {
            FsEntry { path, is_dir: true }
        } else {
            FsEntry {
                path,
                is_dir: false,
            }
        };

        entries_by_dir.entry(parent).or_default().push(fs_entry);
    }

    Ok(entries_by_dir)
}

const MAX_DEPTH: usize = 100;

fn build_tree_recursive(
    repo: &Repository,
    current_dir: &Path,
    entries_by_dir: &HashMap<PathBuf, Vec<FsEntry>>,
    root_path: &Path,
    depth: usize,
) -> Result<ObjectId, Error> {
    use gix::objs::tree;
    if depth > MAX_DEPTH {
        return Err(Error::RecursionLimit);
    }

    let mut tree_entries = Vec::new();

    if let Some(entries) = entries_by_dir.get(current_dir) {
        for entry in entries {
            let filename = entry
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(Error::InvalidFile)?;

            if entry.is_dir {
                let subtree_id =
                    build_tree_recursive(repo, &entry.path, entries_by_dir, root_path, depth + 1)?;

                tree_entries.push(tree::Entry {
                    mode: gix::object::tree::EntryKind::Tree.into(),
                    oid: subtree_id,
                    filename: filename.into(),
                });
            } else {
                let full_path = root_path.join(&entry.path);
                let content = std::fs::read(&full_path)?;
                let blob_id = repo.write_blob(&content)?;

                let metadata = full_path.metadata()?;
                let mode = metadata.mode();

                tree_entries.push(tree::Entry {
                    mode: mode.try_into().map_err(Error::Mode)?,
                    oid: blob_id.detach(),
                    filename: filename.into(),
                });
            }
        }
    }

    tree_entries.sort_by(|a, b| a.filename.cmp(&b.filename));

    let tree = gix::objs::Tree {
        entries: tree_entries,
    };
    Ok(repo.write_object(&tree)?.detach())
}
