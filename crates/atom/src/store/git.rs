//! # Atom Git Store
//!
//! This module contains the foundational types for the Git implementation of an Ekala store.
//!
//! In particular, the implementation to initialize ([`Init`]) a Git repository as an Ekala store
//! is contained here, as well as the type representing the [`Root`] of history used for an
//! [`crate::AtomId`].
#[cfg(test)]
pub(crate) mod test;

use std::sync::OnceLock;

use bstr::BStr;
use gix::discover::upwards::Options;
use gix::sec::Trust;
use gix::sec::trust::Mapping;
use gix::{Commit, ObjectId, ThreadSafeRepository};
use thiserror::Error as ThisError;

use crate::id::CalculateRoot;

/// An error encountered during initialization or other git store operations.
#[derive(ThisError, Debug)]
pub enum Error {
    /// No git ref found.
    #[error("No ref named `{0}` found for remote `{1}`")]
    NoRef(String, String),
    /// This git repository does not have a working directory.
    #[error("Repository does not have a working directory")]
    NoWorkDir,
    /// The repository root calculation failed.
    #[error("Failed to calculate the repositories root commit")]
    RootNotFound,
    /// The calculated root does not match what was reported by the remote.
    #[error("The calculated root does not match the reported one")]
    RootInconsistent,
    /// A transparent wrapper for a [`gix::revision::walk::Error`]
    #[error(transparent)]
    WalkFailure(#[from] gix::revision::walk::Error),
    /// A transparent wrapper for a [`std::io::Error`]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A transparent wrapper for a [`std::path::StripPrefixError`]
    #[error(transparent)]
    NormalizationFailed(#[from] std::path::StripPrefixError),
    /// A transparent wrapper for a [`Box<gix::remote::find::existing::Error>`]
    #[error(transparent)]
    NoRemote(#[from] Box<gix::remote::find::existing::Error>),
    /// A transparent wrapper for a [`Box<gix::remote::connect::Error>`]
    #[error(transparent)]
    Connect(#[from] Box<gix::remote::connect::Error>),
    /// A transparent wrapper for a [`Box<gix::remote::fetch::prepare::Error>`]
    #[error(transparent)]
    Refs(#[from] Box<gix::remote::fetch::prepare::Error>),
    /// A transparent wrapper for a [`Box<gix::remote::fetch::Error>`]
    #[error(transparent)]
    Fetch(#[from] Box<gix::remote::fetch::Error>),
    /// A transparent wrapper for a [`Box<gix::object::find::existing::with_conversion::Error>`]
    #[error(transparent)]
    NoCommit(#[from] Box<gix::object::find::existing::with_conversion::Error>),
    /// A transparent wrapper for a [`Box<gix::refspec::parse::Error>`]
    #[error(transparent)]
    AddRefFailed(#[from] Box<gix::refspec::parse::Error>),
    /// A transparent wrapper for a [`Box<gix::reference::edit::Error>`]
    #[error(transparent)]
    WriteRef(#[from] Box<gix::reference::edit::Error>),
}

impl Error {
    pub(crate) fn warn(self) -> Self {
        tracing::warn!(message = %self);
        self
    }
}

/// Provide a lazyily instantiated static reference to the git repository.
static REPO: OnceLock<Option<ThreadSafeRepository>> = OnceLock::new();

use std::borrow::Cow;
static DEFAULT_REMOTE: OnceLock<Cow<str>> = OnceLock::new();

/// The wrapper type for the underlying type which will be used to represent
/// the "root" identifier for an [`crate::AtomId`]. For git, this is a [`gix::ObjectId`]
/// representing the original commit made in the repositories history.
///
/// The wrapper helps disambiguate at the type level between object ids and the root id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root(ObjectId);

/// Return a static reference the the local Git repository.
pub fn repo() -> Result<Option<&'static ThreadSafeRepository>, Box<gix::discover::Error>> {
    let mut error = None;
    let repo = REPO.get_or_init(|| match get_repo() {
        Ok(repo) => Some(repo),
        Err(e) => {
            error = Some(e);
            None
        },
    });
    if let Some(e) = error {
        Err(e)
    } else {
        Ok(repo.as_ref())
    }
}

use std::io;
/// Run's the git binary, returning the output or the err, depending on the return value.
///
/// Note: We rely on this only for operations that are not yet implemented in GitOxide.
///       Once push is implemented upstream, we can, and should, remove this.
pub fn run_git_command(args: &[&str]) -> io::Result<Vec<u8>> {
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

fn get_repo() -> Result<ThreadSafeRepository, Box<gix::discover::Error>> {
    let opts = Options {
        required_trust: Trust::Full,
        ..Default::default()
    };
    ThreadSafeRepository::discover_opts(".", opts, Mapping::default()).map_err(Box::new)
}

/// Return a static reference to the default remote configured for pushing
pub fn default_remote() -> &'static str {
    use gix::remote::Direction;
    DEFAULT_REMOTE
        .get_or_init(|| {
            repo()
                .ok()
                .flatten()
                .and_then(|repo| {
                    repo.to_thread_local()
                        .remote_default_name(Direction::Push)
                        .map(|s| s.to_string().into())
                })
                .unwrap_or("origin".into())
        })
        .as_ref()
}

use std::ops::Deref;
impl Deref for Root {
    type Target = ObjectId;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> CalculateRoot<Root> for Commit<'a> {
    type Error = Error;

    fn calculate_root(&self) -> Result<Root, Self::Error> {
        use gix::revision::walk::Sorting;
        use gix::traverse::commit::simple::CommitTimeOrder;
        // FIXME: we rely on a custom crate patch to search the commit graph
        // with a bias for older commits. The default gix behavior is the opposite
        // starting with bias for newer commits.
        //
        // it is based on the more general concept of an OldestFirst traversal
        // introduce by @nrdxp upstream: https://github.com/Byron/gitoxide/pull/1610
        //
        // However, that work tracks main and the goal of this patch is to remain
        // as minimal as possible on top of a release tag, for easier maintenance
        // assuming it may take a while to merge upstream.
        let mut walk = self
            .ancestors()
            .use_commit_graph(true)
            .sorting(Sorting::ByCommitTime(CommitTimeOrder::OldestFirst))
            .all()?;

        while let Some(Ok(info)) = walk.next() {
            if info.parent_ids.is_empty() {
                return Ok(Root(info.id));
            }
        }

        Err(Error::RootNotFound)
    }
}

use std::path::{Path, PathBuf};

use gix::Repository;

use super::{NormalizeStorePath, QueryStore};

impl NormalizeStorePath for Repository {
    type Error = Error;

    fn normalize<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf, Error> {
        use std::fs;

        use path_clean::PathClean;
        let path = path.as_ref();

        let rel_repo_root = self.workdir().ok_or(Error::NoWorkDir)?;
        let repo_root = fs::canonicalize(rel_repo_root)?;
        let current = self.current_dir();
        let rel = current.join(path).clean();

        rel.strip_prefix(&repo_root)
            .map_or_else(
                |e| {
                    // handle absolute paths as if they were relative to the repo root
                    if !path.is_absolute() {
                        return Err(e);
                    }
                    let cleaned = path.clean();
                    // Preserve the platform-specific root
                    let p = cleaned.strip_prefix(Path::new("/"))?;
                    repo_root
                        .join(p)
                        .clean()
                        .strip_prefix(&repo_root)
                        .map(Path::to_path_buf)
                },
                |p| Ok(p.to_path_buf()),
            )
            .map_err(|e| {
                tracing::warn!(
                    message = "Ignoring path outside repo root",
                    path = %path.display(),
                );
                Error::NormalizationFailed(e)
            })
    }
}

impl AsRef<[u8]> for Root {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

trait EkalaRemote {
    type Error;
    const ANONYMOUS: &str = "<unamed>";
    fn try_symbol(&self) -> Result<&str, Self::Error>;
    fn symbol(&self) -> &str {
        self.try_symbol().unwrap_or(Self::ANONYMOUS)
    }
}

impl<'repo> EkalaRemote for gix::Remote<'repo> {
    type Error = Error;

    fn try_symbol(&self) -> Result<&str, Self::Error> {
        use gix::remote::Name;
        self.name()
            .and_then(Name::as_symbol)
            .ok_or(Error::NoRemote(Box::new(
                gix::remote::find::existing::Error::NotFound {
                    name: Self::ANONYMOUS.into(),
                },
            )))
    }
}

const V1_ROOT: &str = "refs/tags/ekala/root/v1";

use super::Init;
impl<'repo> Init<Root, ObjectId> for gix::Remote<'repo> {
    type Error = Error;

    /// Determines if this remote is a valid Ekala store by pulling HEAD and the root
    /// tag, ensuring the latter is actually the root of HEAD, returning the root.
    fn ekala_root(&self) -> Result<Root, Self::Error> {
        use crate::id::CalculateRoot;

        let repo = self.repo();
        self.get_refs(["HEAD", V1_ROOT]).map(|i| {
            let mut i = i.into_iter();
            let root_for = |i: &mut dyn Iterator<Item = ObjectId>| {
                i.next()
                    .ok_or(Error::NoRef(V1_ROOT.to_owned(), self.symbol().to_owned()))
                    .and_then(|id| Ok(repo.find_commit(id).map_err(Box::new)?))
                    .and_then(|c| {
                        (c.parent_ids().count() != 0)
                            .then(|| c.calculate_root().map(|r| *r))
                            .unwrap_or(Ok(c.id))
                    })
            };

            let fst = root_for(&mut i)?;
            let snd = root_for(&mut i)?;
            if fst == snd {
                Ok(Root(fst))
            } else {
                Err(Error::RootInconsistent)
            }
        })?
    }

    /// Sync with the given remote and get the most up to date HEAD according to it.
    fn sync(&self) -> Result<ObjectId, Error> {
        self.get_ref("HEAD")
    }

    /// Initialize the repository by calculating the root, according to the latest HEAD.
    fn ekala_init(&self) -> Result<(), Error> {
        use gix::refs::transaction::PreviousValue;

        use crate::CalculateRoot;

        let name = self.try_symbol()?;
        let head = self.sync()?;
        let repo = self.repo();
        let root = *repo.find_commit(head).map_err(Box::new)?.calculate_root()?;

        let root_ref = repo
            .reference(V1_ROOT, root, PreviousValue::MustNotExist, "init: root")
            .map_err(Box::new)?
            .name()
            .as_bstr()
            .to_string();

        // FIXME: use gix for push once it supports it
        run_git_command(&[
            "-C",
            repo.git_dir().to_string_lossy().as_ref(),
            "push",
            name,
            format!("{root_ref}:{root_ref}").as_str(),
        ])?;
        tracing::info!(remote = name, message = "Successfully initialized");
        Ok(())
    }
}

impl<'repo> super::QueryStore<ObjectId> for gix::Remote<'repo> {
    type Error = Error;

    /// returns the git object ids for the given references
    fn get_refs<Spec>(
        &self,
        references: impl IntoIterator<Item = Spec>,
    ) -> Result<impl IntoIterator<Item = gix::ObjectId>, Self::Error>
    where
        Spec: AsRef<BStr>,
    {
        use std::collections::HashSet;
        use std::sync::atomic::AtomicBool;

        use gix::remote::Direction;
        use gix::remote::fetch::Tags;
        use gix::remote::ref_map::Options;

        let mut remote = self.clone().with_fetch_tags(Tags::None);

        remote
            .replace_refspecs(references, Direction::Fetch)
            .map_err(Box::new)?;

        let requested: HashSet<_> = remote
            .refspecs(Direction::Fetch)
            .iter()
            .filter_map(|r| r.to_ref().source().map(ToOwned::to_owned))
            .collect();

        let client = remote.connect(Direction::Fetch).map_err(Box::new)?;
        let sync = client
            .prepare_fetch(gix::progress::Discard, Options::default())
            .map_err(Box::new)?;

        let outcome = sync
            .receive(gix::progress::Discard, &AtomicBool::new(false))
            .map_err(Box::new)?;

        let refs = outcome.ref_map.remote_refs;

        refs.iter()
            .filter_map(|r| {
                let (name, target, peeled) = r.unpack();
                requested.get(name)?;
                Some(
                    peeled
                        .or(target)
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| Error::NoRef(name.to_string(), self.symbol().to_owned())),
                )
            })
            .collect::<Result<HashSet<_>, _>>()
    }

    fn get_ref<Spec>(&self, target: Spec) -> Result<ObjectId, Self::Error>
    where
        Spec: AsRef<BStr>,
    {
        let name = target.as_ref().to_string();
        self.get_refs(Some(target)).and_then(|r| {
            r.into_iter()
                .next()
                .ok_or(Error::NoRef(name, self.symbol().to_owned()))
        })
    }
}
