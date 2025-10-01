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
use gix::protocol::handshake::Ref;
use gix::protocol::transport::client::Transport;
use gix::sec::Trust;
use gix::sec::trust::Mapping;
use gix::{Commit, ObjectId, ThreadSafeRepository};
use thiserror::Error as ThisError;

use crate::id::Origin;
use crate::store::QueryVersion;

/// An error encountered during initialization or other git store operations.
#[derive(ThisError, Debug)]
pub enum Error {
    /// No git ref found.
    #[error("No ref named `{0}` found for remote `{1}`")]
    NoRef(String, String),
    /// No remote url configured
    #[error("No `{0}` url configured for remote `{1}`")]
    NoUrl(String, String),
    /// This git repository does not have a working directory.
    #[error("Repository does not have a working directory")]
    NoWorkDir,
    /// The repository root calculation failed.
    #[error("Failed to calculate the repositories root commit")]
    RootNotFound,
    /// The calculated root does not match what was reported by the remote.
    #[error("The calculated root does not match the reported one")]
    RootInconsistent,
    /// The requested version is not contained on the remote.
    #[error("The version requested does not exist on the remote")]
    NoMatchingVersion,
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
    /// A transparent wrapper for a [`gix::protocol::transport::client::connect::Error`]
    #[error(transparent)]
    Connection(#[from] gix::protocol::transport::client::connect::Error),
    /// A transparent wrapper for a [`gix::config::credential_helpers::Error`]
    #[error(transparent)]
    Creds(#[from] gix::config::credential_helpers::Error),
    /// A transparent wrapper for a [`gix::config::file::init::from_paths::Error`]
    #[error(transparent)]
    File(#[from] gix::config::file::init::from_paths::Error),
    /// A transparent wrapper for a [`gix::protocol::handshake::Error`]
    #[error(transparent)]
    Handshake(#[from] Box<gix::protocol::handshake::Error>),
    /// A transparent wrapper for a [`gix::refspec::parse::Error`]
    #[error(transparent)]
    Refspec(#[from] gix::refspec::parse::Error),
    /// A transparent wrapper for a [`gix::refspec::parse::Error`]
    #[error(transparent)]
    Refmap(#[from] gix::protocol::fetch::refmap::init::Error),
    /// A transparent wrapper for a [`gix::refspec::parse::Error`]
    #[error(transparent)]
    UrlParse(#[from] gix::url::parse::Error),
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
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr)))
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

type AtomQuery = (AtomTag, Version, ObjectId);
impl Origin<Root> for std::vec::IntoIter<AtomQuery> {
    type Error = Error;

    fn calculate_origin(&self) -> Result<Root, Self::Error> {
        let root = <gix::Url as QueryVersion<_, _, _, _>>::process_root(self.to_owned())
            .ok_or(Error::RootNotFound)?;
        Ok(Root(root))
    }
}

impl<'a> Origin<Root> for Commit<'a> {
    type Error = Error;

    fn calculate_origin(&self) -> Result<Root, Self::Error> {
        use gix::revision::walk::Sorting;
        use gix::traverse::commit::simple::CommitTimeOrder;
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

pub(super) const V1_ROOT: &str = "refs/tags/ekala/root/v1";
const V1_ROOT_SEMVER: &str = "1.0.0";

fn to_id(r: Ref) -> ObjectId {
    let (_, t, p) = r.unpack();
    // unwrap can't fail here as at least one of these is guaranteed Some
    p.or(t).map(ToOwned::to_owned).unwrap()
}

use super::Init;
impl<'repo> Init<Root, Ref, Box<dyn Transport + Send>> for gix::Remote<'repo> {
    type Error = Error;

    /// Determines if this remote is a valid Ekala store by pulling HEAD and the root
    /// tag, ensuring the latter is actually the root of HEAD, returning the root.
    #[tracing::instrument(skip(transport))]
    fn ekala_root(
        &self,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Root, Self::Error> {
        use crate::id::Origin;

        let span = tracing::Span::current();
        crate::log::set_sub_task(&span, "💪 ensuring consistency with remote");

        let repo = self.repo();
        self.get_refs(["HEAD", V1_ROOT], transport).map(|i| {
            let mut i = i.into_iter();
            let root_for = |i: &mut dyn Iterator<Item = Ref>| {
                i.next()
                    .ok_or(Error::NoRef(V1_ROOT.to_owned(), self.symbol().to_owned()))
                    .and_then(|r| {
                        let id = to_id(r);
                        Ok(repo.find_commit(id).map_err(Box::new)?)
                    })
                    .and_then(|c| {
                        if c.parent_ids().count() != 0 {
                            c.calculate_origin().map(|r| *r)
                        } else {
                            Ok(c.id)
                        }
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
    fn sync(&self, transport: Option<&mut Box<dyn Transport + Send>>) -> Result<Ref, Error> {
        self.get_ref("HEAD", transport)
    }

    /// Initialize the repository by calculating the root, according to the latest HEAD.
    fn ekala_init(&self, transport: Option<&mut Box<dyn Transport + Send>>) -> Result<(), Error> {
        use gix::refs::transaction::PreviousValue;

        use crate::Origin;

        let name = self.try_symbol()?;
        let head = to_id(self.sync(transport)?);
        let repo = self.repo();
        let root = *repo
            .find_commit(head)
            .map_err(Box::new)?
            .calculate_origin()?;

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

type ProgressRange = std::ops::RangeInclusive<prodash::progress::key::Level>;
const STANDARD_RANGE: ProgressRange = 2..=2;

fn setup_line_renderer(
    progress: &std::sync::Arc<prodash::tree::Root>,
) -> prodash::render::line::JoinHandle {
    prodash::render::line(
        std::io::stderr(),
        std::sync::Arc::downgrade(progress),
        prodash::render::line::Options {
            level_filter: Some(STANDARD_RANGE),
            initial_delay: Some(std::time::Duration::from_millis(500)),
            throughput: true,
            ..prodash::render::line::Options::default()
        }
        .auto_configure(prodash::render::line::StreamKind::Stderr),
    )
}

impl super::QueryStore<Ref, Box<dyn Transport + Send>> for gix::Url {
    type Error = Error;

    /// Efficiently queries git references from a remote repository URL.
    ///
    /// This implementation performs a lightweight network operation that only retrieves
    /// reference information (branch/tag names and their commit IDs) without downloading
    /// the actual repository objects. This makes it ideal for scenarios where you need
    /// to check reference existence or get commit IDs without the overhead of a full
    /// repository fetch.
    ///
    /// ## Network Behavior
    /// - **Lightweight**: Only queries reference metadata, not repository content
    /// - **Fast**: Minimal network overhead compared to full fetch operations
    /// - **Efficient**: Suitable for checking reference existence and getting commit IDs
    ///
    /// ## Use Cases
    /// - Checking if specific branches or tags exist on a remote
    /// - Getting commit IDs for references without downloading objects
    /// - Lightweight remote repository inspection
    ///
    /// ## Performance
    /// This is significantly faster than the [`gix::Remote`] implementation since it
    /// avoids downloading actual git objects, making it appropriate for read-only
    /// reference queries.
    fn get_refs<Spec>(
        &self,
        targets: impl IntoIterator<Item = Spec>,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> std::result::Result<
        impl std::iter::IntoIterator<Item = Ref>,
        <Self as super::QueryStore<Ref, Box<dyn Transport + Send>>>::Error,
    >
    where
        Spec: AsRef<BStr>,
    {
        use gix::open::permissions::Environment;
        use gix::refspec::RefSpec;
        use gix::sec::Permission;

        let transport = if let Some(transport) = transport {
            transport
        } else {
            &mut self.get_transport()?
        };

        let config = gix::config::File::from_globals()?;
        let (mut cascade, _, prompt_opts) = gix::config::credential_helpers(
            self.to_owned(),
            &config,
            true,
            gix::config::section::is_trusted,
            Environment {
                xdg_config_home: Permission::Allow,
                home: Permission::Allow,
                http_transport: Permission::Allow,
                identity: Permission::Allow,
                objects: Permission::Allow,
                git_prefix: Permission::Allow,
                ssh_prefix: Permission::Allow,
            },
            false,
        )?;

        let authenticate = Box::new(move |action| cascade.invoke(action, prompt_opts.clone()));

        let mut handshake = gix::protocol::fetch::handshake(
            &mut *transport,
            authenticate,
            Vec::new(),
            &mut prodash::progress::Discard,
        )
        .map_err(Box::new)?;

        use gix::refspec::parse::Operation;
        let refs: Vec<_> = targets
            .into_iter()
            .map(|t| gix::refspec::parse(t.as_ref(), Operation::Fetch).map(RefSpec::from))
            .collect::<Result<Vec<_>, _>>()?;

        use gix::protocol::fetch::refmap::init::Options as RefOptions;
        use gix::protocol::fetch::{Context, RefMap};

        let context = Context {
            handshake: &mut handshake,
            transport,
            user_agent: ("agent", Some(gix::env::agent().into())),
            trace_packetlines: true,
        };

        let refmap = RefMap::new(
            prodash::progress::Discard,
            refs.as_slice(),
            context,
            RefOptions::default(),
        )?;
        Ok(refmap.remote_refs)
    }

    fn get_transport(&self) -> Result<Box<dyn Transport + Send>, Self::Error> {
        use gix::protocol::transport::client::connect::Options;
        let transport = gix::protocol::transport::connect(self.to_owned(), Options::default())?;
        Ok(Box::new(transport))
    }

    fn get_ref<Spec>(
        &self,
        target: Spec,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr>,
    {
        let name = target.as_ref().to_string();
        self.get_refs(Some(target), transport).and_then(|r| {
            r.into_iter()
                .next()
                .ok_or(Error::NoRef(name, self.to_string()))
        })
    }
}

impl<'repo> super::QueryStore<Ref, Box<dyn Transport + Send>> for gix::Remote<'repo> {
    type Error = Error;

    /// Performs a full git fetch operation to retrieve references and repository data.
    ///
    /// This implementation executes a complete git fetch operation, which downloads
    /// both reference information and the actual repository objects (commits, trees,
    /// blobs) from the remote. This provides full access to the repository content
    /// but is significantly more expensive than the URL-based implementation.
    ///
    /// ## Network Behavior
    /// - **Heavyweight**: Performs a full git fetch operation, downloading all objects
    /// - **Complete**: Provides access to the entire repository state after fetching
    /// - **Expensive**: Higher network usage and longer execution time
    ///
    /// ## Use Cases
    /// - When you need to access repository content after fetching references
    /// - When working with local repositories that need to sync with remotes
    /// - When you require the complete repository state, not just reference metadata
    ///
    /// ## Performance
    /// This implementation is slower and uses more network bandwidth than the
    /// [`gix::Url`] implementation because it downloads actual git objects.
    /// Use it only when you need access to repository content beyond reference metadata.
    ///
    /// ## Progress Reporting
    /// The fetch operation includes progress reporting for sync and initialization phases.
    /// Progress is displayed when the log level is set above WARN.
    fn get_refs<Spec>(
        &self,
        references: impl IntoIterator<Item = Spec>,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> std::result::Result<
        impl IntoIterator<Item = Ref>,
        <Self as super::QueryStore<Ref, Box<dyn Transport + Send>>>::Error,
    >
    where
        Spec: AsRef<BStr>,
    {
        use std::sync::atomic::AtomicBool;

        use gix::progress::prodash::tree::Root;
        use gix::remote::Direction;
        use gix::remote::fetch::Tags;
        use gix::remote::ref_map::Options;
        use tracing::level_filters::LevelFilter;

        let tree = Root::new();
        let sync_progress = tree.add_child("sync");
        let init_progress = tree.add_child("init");
        let _ = if LevelFilter::current() > LevelFilter::WARN {
            Some(setup_line_renderer(&tree))
        } else {
            None
        };

        let mut remote = self.clone().with_fetch_tags(Tags::None);

        remote
            .replace_refspecs(references, Direction::Fetch)
            .map_err(Box::new)?;

        let transport = if let Some(transport) = transport {
            transport
        } else {
            &mut remote.get_transport()?
        };

        let client = remote.to_connection_with_transport(transport);

        let query = client
            .prepare_fetch(sync_progress, Options::default())
            .map_err(Box::new)?;

        let outcome = query
            .with_write_packed_refs_only(true)
            .receive(init_progress, &AtomicBool::new(false))
            .map_err(Box::new)?;

        Ok(outcome.ref_map.remote_refs)
    }

    fn get_transport(&self) -> Result<Box<dyn Transport + Send>, Self::Error> {
        use gix::remote::Direction;
        let url = self
            .url(Direction::Fetch)
            .ok_or_else(|| Error::NoUrl("fetch".to_string(), self.symbol().to_string()))?;
        url.get_transport()
    }

    fn get_ref<Spec>(
        &self,
        target: Spec,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr>,
    {
        let name = target.as_ref().to_string();
        self.get_refs(Some(target), transport).and_then(|r| {
            r.into_iter()
                .next()
                .ok_or(Error::NoRef(name, self.symbol().to_owned()))
        })
    }
}

use semver::Version;

use crate::AtomTag;
impl super::UnpackRef<ObjectId> for Ref {
    fn unpack_atom_ref(&self) -> Option<super::UnpackedRef<ObjectId>> {
        let maybe_root = self.find_root_ref();
        if let Some(root) = maybe_root {
            return Some((
                AtomTag::root_tag(),
                Version::parse(V1_ROOT_SEMVER).ok()?,
                root,
            ));
        }
        let (n, t, p) = self.unpack();
        let mut path = PathBuf::from(n.to_string());
        let v_str = path.file_name()?.to_str()?;
        let version = Version::parse(v_str).ok()?;
        path.pop();
        let a_str = path.file_name()?.to_str()?;
        let tag = AtomTag::try_from(a_str).ok()?;
        let id = p.or(t).map(ToOwned::to_owned)?;

        Some((tag, version, id))
    }

    fn find_root_ref(&self) -> Option<ObjectId> {
        if let Ref::Direct {
            full_ref_name: name,
            object: id,
        } = self
        {
            if name == V1_ROOT {
                return Some(id.to_owned());
            }
        }
        None
    }
}

type Refs = Vec<super::UnpackedRef<ObjectId>>;
impl QueryVersion<Ref, ObjectId, Refs, Box<dyn Transport + Send>> for gix::Url {}
impl<'repo> QueryVersion<Ref, ObjectId, Refs, Box<dyn Transport + Send>> for gix::Remote<'repo> {}
