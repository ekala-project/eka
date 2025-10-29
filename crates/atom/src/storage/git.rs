//! # Git Storage Backend
//!
//! This module provides the Git-based implementation of the Atom storage interface.
//! It handles repository initialization, reference management, and query operations
//! for Git-backed atom stores.
//!
//! ## Overview
//!
//! The Git storage backend uses Git repositories to store atoms as orphaned commits
//! with structured reference hierarchies. This provides:
//!
//! - **Distributed storage** with Git's built-in replication
//! - **Cryptographic integrity** through Git's content addressing
//! - **Version control** for atom evolution tracking
//! - **Efficient querying** via Git references
//!
//! ## Key Components
//!
//! - [`Root`] - Represents the repository's root commit for atom identity
//! - [`Error`] - Git-specific error types for storage operations
//! - Repository initialization and root calculation
//! - Reference querying and atom discovery
//!
//! ## Reference Structure
//!
//! Atoms are stored using Git references:
//! - `refs/ekala/init` - Repository root reference
//! - `refs/eka/atoms/{label}/{version}` - Atom content references
//! - `refs/eka/meta/{label}/{version}/manifest` - Manifest references
//! - `refs/eka/meta/{label}/{version}/origin` - Source commit references
//!
//! ## Initialization
//!
//! Repository initialization creates the root reference and ensures consistency
//! between the calculated repository root and the stored reference.

use std::borrow::Cow;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bstr::BStr;
use gix::discover::upwards::Options;
use gix::protocol::handshake::Ref;
use gix::protocol::transport::client::Transport;
use gix::refs::Target;
use gix::sec::Trust;
use gix::sec::trust::Mapping;
use gix::{Commit, ObjectId, Repository, ThreadSafeRepository};
use semver::Version;
use thiserror::Error as ThisError;

use super::{Init, NormalizeStorePath, QueryStore, QueryVersion, UnpackedRef};
use crate::id::Origin;
use crate::package::AtomError;
use crate::package::metadata::{DocError, EkalaManifest, GitDigest};
use crate::{AtomId, Label};

#[cfg(test)]
pub(crate) mod test;

//================================================================================================
// Constants
//================================================================================================

pub(super) const V1_ROOT: &str = "refs/ekala/init";

//================================================================================================
// Statics
//================================================================================================

static DEFAULT_REMOTE: OnceLock<Cow<str>> = OnceLock::new();
/// Provide a lazily instantiated static reference to the git repository.
static REPO: OnceLock<Option<ThreadSafeRepository>> = OnceLock::new();

//================================================================================================
// Types
//================================================================================================

/// An error encountered during initialization or other git store operations.
#[derive(ThisError, Debug)]
pub enum Error {
    /// A transparent wrapper for a [`Box<gix::refspec::parse::Error>`]
    #[error(transparent)]
    AddRefFailed(#[from] Box<gix::refspec::parse::Error>),
    /// A transparent wrapper for a [`tempfile::PersistError`]
    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),
    /// A transparent wrapper for a [`gix::reference::head_commit::Error`]
    #[error(transparent)]
    HeadCommit(#[from] gix::reference::head_commit::Error),
    /// A transparent wrapper for a [`Box<gix::remote::connect::Error>`]
    #[error(transparent)]
    Connect(#[from] Box<gix::remote::connect::Error>),
    /// A transparent wrapper for a [`gix::protocol::transport::client::connect::Error`]
    #[error(transparent)]
    Connection(#[from] gix::protocol::transport::client::connect::Error),
    /// A transparent wrapper for a [`gix::config::credential_helpers::Error`]
    #[error(transparent)]
    Creds(#[from] gix::config::credential_helpers::Error),
    /// A transparent wrapper for a [`Box<gix::remote::fetch::Error>`]
    #[error(transparent)]
    Fetch(#[from] Box<gix::remote::fetch::Error>),
    /// A transparent wrapper for a [`gix::config::file::init::from_paths::Error`]
    #[error(transparent)]
    File(#[from] gix::config::file::init::from_paths::Error),
    /// A transparent wrapper for a [`gix::protocol::handshake::Error`]
    #[error(transparent)]
    Handshake(#[from] Box<gix::protocol::handshake::Error>),
    /// A transparent wrapper for a [`std::io::Error`]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The requested version is not contained on the remote.
    #[error("The version requested does not exist on the remote")]
    NoMatchingVersion,
    /// A transparent wrapper for a [`Box<gix::object::find::existing::with_conversion::Error>`]
    #[error(transparent)]
    NoCommit(#[from] Box<gix::object::find::existing::with_conversion::Error>),
    /// No git ref found.
    #[error("No ref named `{0}` found for remote `{1}`")]
    NoRef(String, String),
    /// A transparent wrapper for a [`Box<gix::remote::find::existing::Error>`]
    #[error(transparent)]
    NoRemote(#[from] Box<gix::remote::find::existing::Error>),
    /// No remote url configured
    #[error("No `{0}` url configured for remote `{1}`")]
    NoUrl(String, String),
    /// This git repository does not have a working directory.
    #[error("Repository does not have a working directory")]
    NoWorkDir,
    /// A transparent wrapper for a [`std::path::StripPrefixError`]
    #[error(transparent)]
    NormalizationFailed(#[from] std::path::StripPrefixError),
    /// A transparent wrapper for a [`gix::refspec::parse::Error`]
    #[error(transparent)]
    Refmap(#[from] gix::protocol::fetch::refmap::init::Error),
    /// A transparent wrapper for a [`Box<gix::remote::fetch::prepare::Error>`]
    #[error(transparent)]
    Refs(#[from] Box<gix::remote::fetch::prepare::Error>),
    /// A transparent wrapper for a [`gix::refspec::parse::Error`]
    #[error(transparent)]
    Refspec(#[from] gix::refspec::parse::Error),
    /// The calculated root does not match what was reported by the remote.
    #[error("The calculated root does not match the reported one")]
    RootInconsistent,
    /// The repository root calculation failed.
    #[error("Failed to calculate the repositories root commit")]
    RootNotFound,
    /// The repository root calculation failed.
    #[error("The remote is initialized, but reported no published atoms")]
    NoAtoms,
    /// Repo is in a detached head state
    #[error("The repository is in a detached head state")]
    DetachedHead,
    /// A transparent wrapper for a [`gix::url::parse::Error`]
    #[error(transparent)]
    UrlParse(#[from] gix::url::parse::Error),
    /// A transparent wrapper for a [`gix::revision::walk::Error`]
    #[error(transparent)]
    WalkFailure(#[from] gix::revision::walk::Error),
    /// A transparent wrapper for a [`Box<gix::reference::edit::Error>`]
    #[error(transparent)]
    WriteRef(#[from] Box<gix::reference::edit::Error>),
    /// A transparent wrapper for a [`Box<gix::reference::edit::Error>`]
    #[error(transparent)]
    Semver(#[from] semver::Error),
    /// A transparent wrapper for a [`crate::id::Error`]
    #[error(transparent)]
    LabelError(#[from] crate::id::Error),
    /// A transparent wrapper for a [`crate::id::Error`]
    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),
    /// A transparent wrapper for a [`toml_edit::ser::Error`]
    #[error(transparent)]
    Serial(#[from] toml_edit::ser::Error),
    /// A transparent wrapper for a [`AtomError`]
    #[error(transparent)]
    Atom(#[from] AtomError),
    /// A transparent wrapper for a [`DocError`]
    #[error(transparent)]
    Doc(#[from] DocError),
    /// A generic boxed error variant
    #[error(transparent)]
    Generic(Box<dyn std::error::Error + Send + Sync>),
}

/// The wrapper type for the underlying type which will be used to represent
/// the "root" identifier for an [`crate::AtomId`]. For git, this is a [`gix::ObjectId`]
/// representing the original commit made in the repositories history.
///
/// The wrapper helps disambiguate at the type level between object ids and the root id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Root(ObjectId);

pub(crate) type AtomQuery = UnpackedRef<ObjectId, Root>;
type ProgressRange = std::ops::RangeInclusive<prodash::progress::key::Level>;
type Refs = Vec<super::UnpackedRef<ObjectId, Root>>;

//================================================================================================
// Traits
//================================================================================================

trait EkalaRemote {
    type Error;
    const ANONYMOUS: &str = "<unamed>";
    fn try_symbol(&self) -> Result<&str, Self::Error>;
    fn symbol(&self) -> &str {
        self.try_symbol().unwrap_or(Self::ANONYMOUS)
    }
}

//================================================================================================
// Impls
//================================================================================================

impl AsRef<[u8]> for Root {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
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

impl Deref for Root {
    type Target = ObjectId;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Error {
    pub(crate) fn warn(self) -> Self {
        tracing::warn!(message = %self);
        self
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

impl Init<Root, Ref, ()> for gix::Repository {
    type Error = Error;

    fn sync(&self, _: Option<&mut ()>) -> Result<Ref, Self::Error> {
        todo!()
    }

    fn ekala_init(&self, _: Option<&mut ()>) -> Result<String, Self::Error> {
        let workdir = self.workdir().ok_or(Error::DetachedHead)?;
        let manifest_filename = crate::EKALA_MANIFEST_NAME.as_str();
        let manifest_path = workdir.join(manifest_filename);

        let content = if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            let _manifest: EkalaManifest =
                toml_edit::de::from_str(&content).map_err(|e| Error::Generic(Box::new(e)))?;
            content
        } else {
            let manifest = EkalaManifest::new();
            let content = toml_edit::ser::to_string_pretty(&manifest)?;
            std::fs::write(&manifest_path, &content)?;
            content
        };

        if !self
            .head_tree()
            .ok()
            .map(|t| t.find_entry(manifest_filename).is_some())
            .is_some_and(|b| b)
        {
            self.commit_init(&content)?;
        };
        Ok(content)
    }

    fn commit_init(&self, content: &str) -> Result<(), Self::Error> {
        use gix::objs::tree;

        let blob = self
            .write_blob(content)
            .map_err(|e| Error::Generic(Box::new(e)))?;
        let tree = self.head_tree().map_err(|e| Error::Generic(Box::new(e)))?;
        let mut tree = tree.decode().map_err(|e| Error::Generic(Box::new(e)))?;
        let entry = tree::Entry {
            mode: tree::EntryKind::Blob.into(),
            filename: crate::EKALA_MANIFEST_NAME.as_str().into(),
            oid: blob.detach(),
        };
        tree.entries.push((&entry).into());
        tree.entries.sort_unstable();
        let id = self
            .write_object(&tree)
            .map_err(|e| Error::Generic(Box::new(e)))?;

        let mut index = gix::index::File::clone(
            &*self
                .index_or_empty()
                .map_err(|e| Error::Generic(Box::new(e)))?,
        );
        index.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            blob.detach(),
            gix::index::entry::Flags::from_stage(gix::index::entry::Stage::Unconflicted),
            gix::index::entry::Mode::FILE,
            crate::EKALA_MANIFEST_NAME.as_str().into(),
        );
        index.sort_entries();
        index
            .write(gix::index::write::Options::default())
            .map_err(|e| Error::Generic(Box::new(e)))?;

        self.commit("HEAD", "init: ekala project", id, vec![
            self.head_id().map_err(|e| Error::Generic(Box::new(e)))?,
        ])
        .map_err(|e| Error::Generic(Box::new(e)))?;

        Ok(())
    }

    fn ekala_root(&self, _: Option<&mut ()>) -> Result<Root, Self::Error> {
        self.head_commit()
            .map_err(|e| Error::Generic(Box::new(e)))?
            .calculate_origin()
    }
}

impl<'repo> Init<Root, Ref, Box<dyn Transport + Send>> for gix::Remote<'repo> {
    type Error = Error;

    /// Verifies the consistency of a remote Ekala store and returns its root.
    ///
    /// This function ensures the remote repository is properly initialized as an Ekala store
    /// by checking that the declared root reference exists and is consistent with the repository's
    /// actual root commit.
    ///
    /// ## Behavior
    ///
    /// The function performs the following steps:
    ///
    /// 1. **Fetches References**: It requests two specific references from the remote:
    ///     - `HEAD`: To get the current head commit.
    ///     - `refs/ekala/init`: The Ekala root reference.
    ///
    /// 2. **Validates References**: It ensures both `HEAD` and the Ekala root reference exist in
    ///    the fetched refs. If either is missing, it returns a RootNotFound error.
    ///
    /// 3. **Calculates Roots**: For both the HEAD commit and the Ekala root reference:
    ///     - If the commit has no parents (is the initial commit), uses that commit's ID directly.
    ///     - Otherwise, traverses the commit history back to the initial commit to find the true
    ///       root.
    ///
    /// 4. **Verifies Consistency**: Compares the calculated root from HEAD with the calculated root
    ///    from the Ekala reference. If they match, the store is consistent. If they differ, returns
    ///    a RootInconsistent error.
    ///
    /// ## Purpose
    ///
    /// This verification ensures that the Ekala store's root of trust is properly established
    /// and hasn't been corrupted. It prevents operations on misconfigured stores and ensures
    /// all atoms are anchored to a consistent project origin.
    ///
    /// On success, it returns the verified [`Root`] commit ID.
    #[tracing::instrument(skip(transport))]
    fn ekala_root(
        &self,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Root, Self::Error> {
        use crate::id::Origin;

        let span = tracing::Span::current();
        crate::log::set_sub_task(&span, "💪 ensuring consistency with remote");

        let repo = self.repo();
        self.get_refs(["HEAD", V1_ROOT], transport).map(|refs| {
            let ekala_root = refs
                .iter()
                .find(|r| {
                    let (n, ..) = r.unpack();
                    n.starts_with(V1_ROOT.as_bytes())
                })
                .ok_or(Error::RootNotFound)?;

            let head = refs
                .iter()
                .find(|r| {
                    if let Ref::Symbolic { full_ref_name, .. } = r {
                        full_ref_name == "HEAD"
                    } else {
                        false
                    }
                })
                .ok_or(Error::RootNotFound)?;

            let mut i = vec![head.to_owned(), ekala_root.to_owned()].into_iter();
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

            let calculated_root = root_for(&mut i)?;
            let reported_root = root_for(&mut i)?;
            if calculated_root == reported_root {
                Ok(Root(calculated_root))
            } else {
                Err(Error::RootInconsistent)
            }
        })?
    }

    /// Initializes a remote Git repository as an Ekala store.
    ///
    /// This function sets up a remote repository to serve as an Ekala store by creating
    /// the necessary root reference. It ensures the repository is properly configured
    /// before atoms can be published to it.
    ///
    /// ## Behavior
    ///
    /// The initialization process involves several key steps:
    ///
    /// 1. **Transport Setup**: Obtains or uses the provided transport for remote communication.
    ///
    /// 2. **Sync with Remote**: Calls `sync` to fetch the latest `HEAD` from the remote, ensuring
    ///    initialization is based on the current repository state.
    ///
    /// 3. **Root Calculation**: Calculates the repository's true root commit by traversing the
    ///    commit history from the synced `HEAD` back to the initial commit (the one with no
    ///    parents).
    ///
    /// 4. **Consistency Check**: Attempts to call `ekala_root` to check if the remote is already
    ///    initialized.
    ///     - If already initialized, verifies that the existing root matches the calculated root.
    ///     - If they match, returns the existing root reference name (idempotent behavior).
    ///     - If they differ, returns a RootInconsistent error.
    ///
    /// 5. **Root Reference Creation**: If not already initialized, creates a new Git reference
    ///    named `refs/ekala/init` that points directly to the calculated root commit.
    ///
    /// 6. **Push to Remote**: Uses the `git` command-line tool to push the newly created root
    ///    reference to the remote repository. This finalizes the initialization.
    ///
    /// ## Idempotency
    ///
    /// The function is idempotent: if the remote is already initialized with the same root,
    /// it will succeed without making changes. If initialized with a different root, it fails.
    ///
    /// ## Purpose
    ///
    /// By establishing a stable root reference, `ekala_init` provides the foundation for
    /// atom publishing operations. It ensures all atoms are anchored to a consistent
    /// project origin, maintaining the integrity of the distributed store.
    ///
    /// On success, it returns the name of the root reference (`refs/ekala/init`).
    fn ekala_init(
        &self,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<String, Error> {
        use gix::refs::transaction::PreviousValue;

        use crate::Origin;

        let transport = if let Some(transport) = transport {
            transport
        } else {
            &mut self.get_transport()?
        };
        let remote = self.try_symbol()?;
        let head = to_id(self.sync(Some(transport))?);
        let repo = self.repo();
        let root = *repo
            .find_commit(head)
            .map_err(Box::new)?
            .calculate_origin()?;

        if let Ok(id) = self.ekala_root(Some(transport)) {
            if root != *id {
                tracing::error!(
                    reported.root = %*id,
                    requested.root = %root,
                    "remote is already initialized to a different state than the one \
                               reported; bailing...",
                );
                return Err(Error::RootInconsistent);
            } else {
                tracing::info!(
                    ekala.root = %*id,
                    ekala.remote = %remote,
                    "remote is already initialized"
                );
                return Ok(V1_ROOT.into());
            }
        }

        let root_ref = repo
            .reference(
                V1_ROOT,
                root,
                PreviousValue::ExistingMustMatch(Target::from(root)),
                "init: ekala",
            )
            .map_err(Box::new)?
            .name()
            .as_bstr()
            .to_string();

        // FIXME: use gix for push once it supports it
        run_git_command(&[
            "-C",
            repo.git_dir().to_string_lossy().as_ref(),
            "push",
            remote,
            format!("{root_ref}:{root_ref}").as_str(),
        ])?;
        tracing::info!(ekala.remote = %remote, ekala.root = %*root, "Successfully initialized");
        Ok(root_ref)
    }

    /// Sync with the given remote and get the most up to date HEAD according to it.
    fn sync(&self, transport: Option<&mut Box<dyn Transport + Send>>) -> Result<Ref, Error> {
        self.get_ref("HEAD", transport)
    }
}

impl<P: AsRef<Path>> NormalizeStorePath<P> for Repository {
    type Error = Error;

    fn normalize(&self, path: P) -> Result<PathBuf, Error> {
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
                    path = %path.display(),
                    "Ignoring path outside repo root",
                );
                Error::NormalizationFailed(e)
            })
    }
}

impl Origin<Root> for std::vec::IntoIter<AtomQuery> {
    type Error = Error;

    fn calculate_origin(&self) -> Result<Root, Self::Error> {
        let mut iter = self.clone();
        iter.try_fold(None, |first, item| match first {
            Some(r) if &r == item.id.root() => Ok(first),
            None => Ok(Some(item.id.root().to_owned())),
            _ => Err(Error::RootInconsistent),
        })
        .and_then(|x| x.ok_or(Error::NoAtoms))
    }
}

impl From<GitDigest> for Root {
    fn from(value: GitDigest) -> Self {
        let oid = match value {
            GitDigest::Sha1(b) => ObjectId::Sha1(b),
            // TODO: implement when gix gets sha256 support
            GitDigest::Sha256(_) => todo!(),
        };
        Root(oid)
    }
}

impl Origin<Root> for Root {
    type Error = String;

    fn calculate_origin(&self) -> Result<Root, Self::Error> {
        Ok(self.to_owned())
    }
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
        targets: impl IntoIterator<Item = Spec> + std::fmt::Debug,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> std::result::Result<
        Vec<Ref>,
        <Self as super::QueryStore<Ref, Box<dyn Transport + Send>>>::Error,
    >
    where
        Spec: AsRef<BStr> + std::fmt::Debug,
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
        .map_err(|e| {
            tracing::error!(url = %self, "couldn't establish a handshake with the remote");
            Box::new(e)
        })?;

        tracing::debug!(?targets, url = %self, "checking remote for refs");
        use gix::refspec::parse::Operation;
        let refs: Vec<_> = targets
            .into_iter()
            .map(|t| {
                gix::refspec::parse(t.as_ref(), Operation::Fetch)
                    .map(RefSpec::from)
                    .inspect_err(|_| tracing::error!(ref = ?t, "failed to parse ref"))
            })
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

    fn get_ref<Spec>(
        &self,
        target: Spec,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr> + std::fmt::Debug,
    {
        let name = target.as_ref().to_string();
        self.get_refs(Some(target), transport).and_then(|r| {
            r.into_iter()
                .next()
                .ok_or(Error::NoRef(name, self.to_string()))
        })
    }

    fn get_transport(&self) -> Result<Box<dyn Transport + Send>, Self::Error> {
        use gix::protocol::transport::client::connect::Options;
        let transport = gix::protocol::transport::connect(self.to_owned(), Options::default())?;
        Ok(Box::new(transport))
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
        Vec<Ref>,
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

    fn get_ref<Spec>(
        &self,
        target: Spec,
        transport: Option<&mut Box<dyn Transport + Send>>,
    ) -> Result<Ref, Self::Error>
    where
        Spec: AsRef<BStr> + std::fmt::Debug,
    {
        let name = target.as_ref().to_string();
        self.get_refs(Some(target), transport).and_then(|r| {
            r.into_iter()
                .next()
                .ok_or(Error::NoRef(name, self.symbol().to_owned()))
        })
    }

    fn get_transport(&self) -> Result<Box<dyn Transport + Send>, Self::Error> {
        use gix::remote::Direction;
        let url = self
            .url(Direction::Fetch)
            .ok_or_else(|| Error::NoUrl("fetch".to_string(), self.symbol().to_string()))?;
        url.get_transport()
    }
}

impl super::UnpackRef<ObjectId, Root> for Ref {
    fn find_root_ref(&self) -> Option<Root> {
        if let Ref::Direct {
            full_ref_name: name,
            object: id,
        } = self
        {
            if name == V1_ROOT {
                return Some(Root(id.to_owned()));
            }
        }
        None
    }

    fn unpack_atom_ref(&self, root: Option<&Root>) -> Option<super::UnpackedRef<ObjectId, Root>> {
        let root = root?;
        let (n, t, p) = self.unpack();
        let mut path = PathBuf::from(n.to_string());
        let v_str = path.file_name()?.to_str()?;
        let version = Version::parse(v_str).ok()?;
        path.pop();
        let a_str = path.file_name()?.to_str()?;
        let label = Label::try_from(a_str).ok()?;
        let rev = p.or(t).map(ToOwned::to_owned)?;

        Some(UnpackedRef {
            id: AtomId::construct(root, label).ok()?,
            version,
            rev,
        })
    }
}

impl<'repo> QueryVersion<Ref, ObjectId, Refs, Box<dyn Transport + Send>, Root>
    for gix::Remote<'repo>
{
}
impl QueryVersion<Ref, ObjectId, Refs, Box<dyn Transport + Send>, Root> for gix::Url {}

//================================================================================================
// Functions
//================================================================================================

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

fn get_repo() -> Result<ThreadSafeRepository, Box<gix::discover::Error>> {
    let opts = Options {
        required_trust: Trust::Full,
        ..Default::default()
    };
    ThreadSafeRepository::discover_opts(".", opts, Mapping::default()).map_err(Box::new)
}

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

/// Runs the `git` binary with the given arguments, returning its output.
///
/// Note: This function is a temporary workaround for operations not yet implemented in `gix`.
/// It should be removed once `gix` supports all necessary functionality (e.g., push).
pub fn run_git_command(args: &[&str]) -> io::Result<Vec<u8>> {
    use std::process::Command;
    let output = Command::new("git").args(args).output()?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr)))
    }
}

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

pub(crate) fn to_id(r: Ref) -> ObjectId {
    let (_, t, p) = r.unpack();
    // unwrap can't fail here as at least one of these is guaranteed Some
    p.or(t).map(ToOwned::to_owned).unwrap()
}
