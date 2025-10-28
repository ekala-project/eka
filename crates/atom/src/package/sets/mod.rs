use std::collections::{BTreeMap, BTreeSet, HashMap};

use either::Either;
use gix::protocol::transport::client::Transport;
use gix::{ObjectId, ThreadSafeRepository};
use id::Tag;
use manifest::{Manifest, SetMirror};
use metadata::lock::SetDetails;
use metadata::{EkalaManager, GitDigest, manifest};
use semver::Version;
use storage::UnpackedRef;
use storage::git::{AtomQuery, Root};
use tokio::task::JoinSet;

use super::{AtomError, metadata};
use crate::{AtomId, BoxError, id, storage};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("manifest is in an inconsistent state")]
    Inconsistent,
    #[error("You are not inside a structured local set, `::` has no meaning as a mirror")]
    NoLocal,
}

pub(super) struct ResolvedSets {
    pub(super) atoms: ResolvedAtoms<ObjectId, Root>,
    pub(super) roots: HashMap<Either<Tag, SetMirror>, Root>,
    pub(super) transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
    pub(super) details: BTreeMap<GitDigest, SetDetails>,
    pub(super) ekala: EkalaManager,
    pub(super) repo: Option<gix::Repository>,
}

#[derive(Clone)]
pub(super) struct ResolvedAtom<Id, R> {
    pub(super) unpacked: UnpackedRef<Id, R>,
    pub(super) remotes: BTreeSet<gix::Url>,
}

type ResolvedAtoms<Id, R> = HashMap<AtomId<R>, HashMap<Version, ResolvedAtom<Id, R>>>;

pub(super) struct SetResolver<'a> {
    pub(super) manifest: &'a Manifest,
    pub(super) repo: Option<gix::Repository>,
    pub(super) names: HashMap<Root, Tag>,
    pub(super) roots: HashMap<Either<Tag, SetMirror>, Root>,
    pub(super) tasks: JoinSet<MirrorResult>,
    pub(super) atoms: ResolvedAtoms<ObjectId, Root>,
    pub(super) sets: BTreeMap<GitDigest, SetDetails>,
    pub(super) transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
    pub(super) ekala: EkalaManager,
}

pub(super) type MirrorResult = Result<
    (
        Option<Box<dyn Transport + Send>>,
        <Vec<AtomQuery> as IntoIterator>::IntoIter,
        Root,
        Tag,
        gix::Url,
    ),
    BoxError,
>;

impl<'a> SetResolver<'a> {
    /// Creates a new `SetResolver` to validate the package sets in a manifest.
    pub(super) fn new(
        repo: Option<&ThreadSafeRepository>,
        manifest: &'a Manifest,
    ) -> Result<Self, AtomError> {
        let len = manifest.package().sets().len();
        let ekala = EkalaManager::new(repo)?;
        Ok(Self {
            manifest,
            ekala,
            repo: repo.map(|r| r.to_thread_local()),
            names: HashMap::with_capacity(len),
            roots: HashMap::with_capacity(len),
            tasks: JoinSet::new(),
            atoms: HashMap::with_capacity(len * 10),
            transports: HashMap::with_capacity(len * 3),
            sets: BTreeMap::new(),
        })
    }
}

impl ResolvedSets {
    pub(super) fn roots(&self) -> &HashMap<Either<Tag, SetMirror>, Root> {
        &self.roots
    }

    pub fn atoms(&self) -> &ResolvedAtoms<ObjectId, Root> {
        &self.atoms
    }

    pub fn details(&self) -> &BTreeMap<GitDigest, SetDetails> {
        &self.details
    }

    pub fn ekala(&self) -> &EkalaManager {
        &self.ekala
    }
}

impl<Id, R> ResolvedAtom<Id, R> {
    pub(super) fn new(unpacked: UnpackedRef<Id, R>, remotes: BTreeSet<gix::Url>) -> Self {
        Self { unpacked, remotes }
    }

    pub(super) fn unpack(&self) -> &UnpackedRef<Id, R> {
        &self.unpacked
    }

    pub(super) fn remotes(&self) -> &BTreeSet<gix::Url> {
        &self.remotes
    }
}
