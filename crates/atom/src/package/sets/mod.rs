//! # Package Set Resolution
//!
//! This module handles the resolution and validation of package sets defined
//! in atom manifests. Package sets define sources (mirrors) for atom dependencies
//! and ensure consistency across different repositories.
//!
//! ## Key Concepts
//!
//! - **Package Sets** - Named collections of mirrors providing atoms
//! - **Mirrors** - URLs or local references pointing to atom repositories
//! - **Root Consistency** - All mirrors in a set must have the same root hash
//! - **Atom Resolution** - Finding and validating atoms across mirrors
//!
//! ## Set Types
//!
//! - **Local Sets** (`::`) - Reference atoms in the current repository
//! - **Remote Sets** - URLs pointing to external atom repositories
//! - **Mirror Sets** - Multiple URLs providing the same atoms for redundancy
//!
//! ## Resolution Process
//!
//! 1. **Set Discovery** - Parse package sets from manifest
//! 2. **Mirror Validation** - Ensure all mirrors have consistent roots
//! 3. **Atom Aggregation** - Collect atoms from all valid mirrors
//! 4. **Conflict Resolution** - Handle version conflicts between mirrors

use std::collections::{BTreeMap, BTreeSet, HashMap};

use either::Either;
use gix::ObjectId;
use gix::protocol::transport::client::Transport;
use id::Tag;
use manifest::{Manifest, SetMirror};
use metadata::lock::SetDetails;
use metadata::{EkalaManager, GitDigest, manifest};
use semver::Version;
use storage::UnpackedRef;
use storage::git::{AtomQuery, Root};
use tokio::task::JoinSet;

use super::{AtomError, metadata};
use crate::storage::LocalStorage;
use crate::{AtomId, BoxError, id, storage};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("manifest is in an inconsistent state")]
    Inconsistent,
    #[error("You are not inside a structured local set, `::` has no meaning as a mirror")]
    NoLocal,
}

pub(super) struct ResolvedSets<'a, S: LocalStorage> {
    pub(super) atoms: ResolvedAtoms<ObjectId, Root>,
    pub(super) roots: HashMap<Either<Tag, SetMirror>, Root>,
    pub(super) transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
    pub(super) details: BTreeMap<GitDigest, SetDetails>,
    pub(super) ekala: EkalaManager<'a, S>,
}

#[derive(Clone)]
pub(super) struct ResolvedAtom<Id, R> {
    pub(super) unpacked: UnpackedRef<Id, R>,
    pub(super) remotes: BTreeSet<gix::Url>,
}

type ResolvedAtoms<Id, R> = HashMap<AtomId<R>, HashMap<Version, ResolvedAtom<Id, R>>>;

pub(super) struct SetResolver<'a, 'b, S: LocalStorage> {
    pub(super) manifest: &'b Manifest,
    pub(super) names: HashMap<Root, Tag>,
    pub(super) roots: HashMap<Either<Tag, SetMirror>, Root>,
    pub(super) tasks: JoinSet<MirrorResult>,
    pub(super) atoms: ResolvedAtoms<ObjectId, Root>,
    pub(super) sets: BTreeMap<GitDigest, SetDetails>,
    pub(super) transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
    pub(super) ekala: EkalaManager<'a, S>,
}

pub(super) type MirrorResult = Result<
    (
        Option<Box<dyn Transport + Send>>,
        Vec<AtomQuery>,
        Root,
        Tag,
        gix::Url,
    ),
    BoxError,
>;

impl<'a, 'b, S: LocalStorage> SetResolver<'a, 'b, S> {
    /// Creates a new `SetResolver` to validate the package sets in a manifest.
    pub(super) fn new(storage: &'a S, manifest: &'b Manifest) -> Result<Self, AtomError> {
        let len = manifest.package().sets().len();
        let ekala = EkalaManager::new(storage)?;
        Ok(Self {
            manifest,
            ekala,
            names: HashMap::with_capacity(len),
            roots: HashMap::with_capacity(len),
            tasks: JoinSet::new(),
            atoms: HashMap::with_capacity(len * 10),
            transports: HashMap::with_capacity(len * 3),
            sets: BTreeMap::new(),
        })
    }
}

impl<'a, S: LocalStorage> ResolvedSets<'a, S> {
    pub(super) fn roots(&self) -> &HashMap<Either<Tag, SetMirror>, Root> {
        &self.roots
    }

    pub fn atoms(&self) -> &ResolvedAtoms<ObjectId, Root> {
        &self.atoms
    }

    pub fn details(&self) -> &BTreeMap<GitDigest, SetDetails> {
        &self.details
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
