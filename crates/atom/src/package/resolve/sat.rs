//! SAT-based dependency resolution for atoms using resolvo.
//!
//! This module provides the core types and traits for resolving atom dependencies
//! using a CDCL SAT solver (resolvo). Key components include:
//!
//! - [`SemverVersionSet`] - Wrapper for `semver::VersionReq` implementing resolvo's `VersionSet`
//!   trait
//! - [`AtomSolvableRecord`] - Metadata associated with each solvable (version candidate)
//! - [`DiscoveryState`] - Tracks which packages have completed candidate discovery
//! - [`ManifestCache`] - Tracks which manifests have been fetched and cached
//!
//! ## Architecture
//!
//! The resolution process follows these phases:
//! 1. **Discovery** - Query remote refs for available versions (cheap ref queries)
//! 2. **Candidate Registration** - Intern packages and candidates into the pool
//! 3. **SAT Solving** - resolvo explores the search space using CDCL
//! 4. **Lazy Manifest Fetching** - Fetch manifests only when `get_dependencies` is called
//! 5. **Solution Extraction** - Extract resolved versions and build the lock file
//!
//! See ADR-0015 for the full implementation plan.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display};
use std::hash::Hash;

use either::Either;
use gix::hashtable::HashSet;
use gix::protocol::transport::client::Transport;
use resolvo::utils::{Pool, VersionSet};
use resolvo::{
    Candidates, Condition, ConditionId, ConditionalRequirement, Dependencies, DependencyProvider,
    HintDependenciesAvailable, Interner, KnownDependencies, NameId, Requirement, SolvableId,
    StringId, VersionSetId,
};
use semver::{Version, VersionReq};

use crate::id::{AtomDigest, Tag};
use crate::package::metadata::GitDigest;
use crate::package::metadata::manifest::SetMirror;
use crate::package::sets::ResolvedSets;
use crate::storage::git::{Root, to_id};
use crate::storage::{LocalStorage, RemoteAtomCache};
use crate::{AtomId, BoxError, Compute, Label, ValidManifest};

//================================================================================================
// Types
//================================================================================================

/// A version set based on semver version requirements.
///
/// This type wraps `semver::VersionReq` to implement resolvo's `VersionSet` trait,
/// enabling semver-based constraint satisfaction during dependency resolution.
///
/// The `VersionSet` trait requires:
/// - `Clone + Eq + Hash` for interning in the pool
/// - Associated type `V: Display` for the version type
///
/// # Example
///
/// ```rust
/// use atom::package::resolve::sat::SemverVersionSet;
/// use semver::VersionReq;
///
/// let req = VersionReq::parse("^1.0").unwrap();
/// let version_set = SemverVersionSet(req);
///
/// // Can be used with resolvo's Pool
/// // pool.intern_version_set(name_id, version_set);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SemverVersionSet(pub semver::VersionReq);

impl SemverVersionSet {
    /// Creates a new `SemverVersionSet` from a version requirement string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as a valid semver requirement.
    pub fn parse(s: &str) -> Result<Self, semver::Error> {
        semver::VersionReq::parse(s).map(SemverVersionSet)
    }

    /// Returns true if the given version matches this version requirement.
    pub fn matches(&self, version: &semver::Version) -> bool {
        self.0.matches(version)
    }

    /// Returns a reference to the inner `VersionReq`.
    pub fn inner(&self) -> &semver::VersionReq {
        &self.0
    }
}

impl Display for SemverVersionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<semver::VersionReq> for SemverVersionSet {
    fn from(req: semver::VersionReq) -> Self {
        SemverVersionSet(req)
    }
}

impl VersionSet for SemverVersionSet {
    type V = semver::Version;
}

/// Metadata associated with each solvable (version candidate) in the pool.
///
/// This record stores information about a specific version of an atom that
/// is needed during resolution and for generating the lock file.
///
/// The record is stored as the `V` (version) type parameter of resolvo's `Pool<VS, N>`,
/// meaning each solvable in the pool contains this metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtomSolvableRecord {
    /// The semantic version of this candidate.
    pub version: semver::Version,

    /// The Git commit revision for this version.
    ///
    /// This is extracted from the ref name (e.g., `refs/eka/atoms/{label}/{version}`)
    /// during discovery and verified during fetch.
    pub rev: GitDigest,

    /// The computed BLAKE3 atom digest.
    ///
    /// This uniquely identifies the atom (root + label) and is used
    /// for looking up atoms in the lock file.
    pub atom_id: AtomDigest,

    /// Whether the manifest for this version has been fetched and cached.
    ///
    /// When true, we can retrieve dependencies from the local cache rather
    /// than fetching from the remote. This flag is updated after successful
    /// manifest fetches.
    pub manifest_cached: bool,
}

impl AtomSolvableRecord {
    /// Creates a new solvable record.
    pub fn new(version: semver::Version, rev: GitDigest, atom_id: AtomDigest) -> Self {
        Self {
            version,
            rev,
            atom_id,
            manifest_cached: false,
        }
    }

    /// Creates a new solvable record with manifest already cached.
    pub fn with_cached_manifest(
        version: semver::Version,
        rev: GitDigest,
        atom_id: AtomDigest,
    ) -> Self {
        Self {
            version,
            rev,
            atom_id,
            manifest_cached: true,
        }
    }
}

impl Display for AtomSolvableRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.version)
    }
}

impl PartialOrd for AtomSolvableRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AtomSolvableRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Sort by version primarily (for sorting candidates)
        self.version.cmp(&other.version)
    }
}

impl Hash for AtomSolvableRecord {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash by version and atom_id for deduplication
        self.version.hash(state);
        self.atom_id.hash(state);
    }
}

//================================================================================================
// Resolution State Tracking
//================================================================================================

/// Tracks which packages have completed candidate discovery.
///
/// Used to avoid redundant ref queries for the same package.
#[derive(Default, Debug)]
pub struct DiscoveryState {
    /// Packages that have completed discovery.
    pub discovered: BTreeMap<AtomDigest, DiscoveryStatus>,
}

/// Status of candidate discovery for a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryStatus {
    /// Discovery is pending/in-progress.
    Pending,
    /// Discovery completed successfully with the number of candidates found.
    Completed(usize),
    /// Discovery failed with an error message.
    Failed(String),
}

impl DiscoveryState {
    /// Creates a new empty discovery state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a package as having pending discovery.
    pub fn mark_pending(&mut self, atom_id: AtomDigest) {
        self.discovered.insert(atom_id, DiscoveryStatus::Pending);
    }

    /// Marks a package discovery as completed.
    pub fn mark_completed(&mut self, atom_id: AtomDigest, count: usize) {
        self.discovered
            .insert(atom_id, DiscoveryStatus::Completed(count));
    }

    /// Marks a package discovery as failed.
    pub fn mark_failed(&mut self, atom_id: AtomDigest, error: String) {
        self.discovered
            .insert(atom_id, DiscoveryStatus::Failed(error));
    }

    /// Returns true if discovery has been attempted for this package.
    pub fn is_discovered(&self, atom_id: &AtomDigest) -> bool {
        self.discovered.contains_key(atom_id)
    }

    /// Returns the discovery status for a package.
    pub fn status(&self, atom_id: &AtomDigest) -> Option<&DiscoveryStatus> {
        self.discovered.get(atom_id)
    }
}

//================================================================================================
// Manifest Cache State
//================================================================================================

/// Tracks which manifests have been fetched and cached.
///
/// This is used to avoid redundant manifest fetches during resolution.
#[derive(Default, Debug)]
pub struct ManifestCache {
    /// Set of atom versions whose manifests have been fetched.
    ///
    /// Key is (AtomDigest, Version) to uniquely identify a specific version.
    pub cached: BTreeMap<(AtomDigest, semver::Version), ManifestCacheEntry>,
}

/// A cached manifest entry.
#[derive(Clone, Debug)]
pub struct ManifestCacheEntry {
    /// The parsed dependencies from the manifest.
    ///
    /// Each entry is (dependency_atom_id, version_requirement).
    pub dependencies: Vec<(AtomDigest, semver::VersionReq)>,
}

impl ManifestCache {
    /// Creates a new empty manifest cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks if a manifest is cached for the given atom version.
    pub fn is_cached(&self, atom_id: &AtomDigest, version: &semver::Version) -> bool {
        self.cached.contains_key(&(*atom_id, version.clone()))
    }

    /// Gets a cached manifest entry.
    pub fn get(
        &self,
        atom_id: &AtomDigest,
        version: &semver::Version,
    ) -> Option<&ManifestCacheEntry> {
        self.cached.get(&(*atom_id, version.clone()))
    }

    /// Inserts a manifest into the cache.
    pub fn insert(
        &mut self,
        atom_id: AtomDigest,
        version: semver::Version,
        dependencies: Vec<(AtomDigest, semver::VersionReq)>,
    ) {
        self.cached
            .insert((atom_id, version), ManifestCacheEntry { dependencies });
    }
}

/// Resolution context holding all state needed for dependency resolution
pub struct AtomResolver<'a, S: LocalStorage> {
    /// The resolvo pool for interning - uses AtomId<Root> directly as package name
    pool: Pool<SemverVersionSet, AtomId<Root>>,

    /// Mapping from AtomId to available versions discovered via ref queries
    /// Uses RefCell for interior mutability since DependencyProvider trait methods use &self
    discovered_candidates: RefCell<HashMap<AtomId<Root>, Vec<DiscoveredVersion>>>,

    /// Cache of fetched manifests: (AtomId, Version) -> parsed dependencies
    /// Uses RefCell for interior mutability since DependencyProvider trait methods use &self
    manifest_cache: RefCell<HashMap<(AtomId<Root>, Version), Vec<AtomDependency>>>,

    /// Set of solvables whose manifests are locally cached (cheap deps access)
    /// Uses RefCell for interior mutability since DependencyProvider trait methods use &self
    locally_available: RefCell<HashSet<SolvableId>>,

    /// Reference to resolved sets from manifest processing
    resolved_sets: &'a ResolvedSets<'a, S>,

    /// Storage backend for git operations
    storage: &'a S,

    /// Transports for remote operations (reusable connections)
    /// Uses RefCell for interior mutability since DependencyProvider trait methods use &self
    transports: RefCell<HashMap<gix::Url, Box<dyn Transport + Send>>>,
}

#[derive(Clone, Debug)]
struct DiscoveredVersion {
    version: Version,
    rev: GitDigest,
    solvable_id: SolvableId,
}

#[derive(Clone, Debug)]
struct AtomDependency {
    /// Target atom identity (Root + Label)
    target: AtomId<Root>,
    /// Version requirement
    version_req: VersionReq,
}

impl<'a, S: LocalStorage> Interner for AtomResolver<'a, S> {
    fn display_solvable(&self, solvable: SolvableId) -> impl std::fmt::Display + '_ {
        let solvable = self.pool.resolve_solvable(solvable);
        let name = self.pool.resolve_package_name(solvable.name);
        format!("{}@{}", name, solvable.record)
    }

    fn display_name(&self, name: NameId) -> impl std::fmt::Display + '_ {
        self.pool.resolve_package_name(name).to_string()
    }

    fn display_version_set(&self, version_set: VersionSetId) -> impl std::fmt::Display + '_ {
        self.pool.resolve_version_set(version_set).0.to_string()
    }

    fn display_string(&self, string_id: StringId) -> impl std::fmt::Display + '_ {
        self.pool.resolve_string(string_id).to_string()
    }

    fn version_set_name(&self, version_set: VersionSetId) -> NameId {
        self.pool.resolve_version_set_package_name(version_set)
    }

    fn solvable_name(&self, solvable: SolvableId) -> NameId {
        self.pool.resolve_solvable(solvable).name
    }

    fn version_sets_in_union(
        &self,
        version_set_union: resolvo::VersionSetUnionId,
    ) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(version_set_union)
    }

    fn resolve_condition(&self, condition: ConditionId) -> Condition {
        self.pool.resolve_condition(condition).clone()
    }
}

impl<'a, S: LocalStorage> DependencyProvider for AtomResolver<'a, S> {
    /// Filter candidates by checking if version satisfies the version set
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        version_set: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        let vs = self.pool.resolve_version_set(version_set);
        candidates
            .iter()
            .filter(|&&solvable_id| {
                let solvable = self.pool.resolve_solvable(solvable_id);
                let matches = vs.matches(&solvable.record);
                if inverse { !matches } else { matches }
            })
            .copied()
            .collect()
    }

    /// Get all candidate versions for a package
    /// This uses CHEAP ref queries - no manifest fetching
    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let package_name = self.pool.resolve_package_name(name);

        // Check if we've already discovered candidates for this package
        if let Some(versions) = self.discovered_candidates.borrow().get(package_name) {
            let candidate_ids: Vec<SolvableId> = versions.iter().map(|v| v.solvable_id).collect();

            // Build hint_dependencies_available bitmap
            let available_bitmap = self.build_availability_bitmap(&candidate_ids);

            return Some(Candidates {
                candidates: candidate_ids,
                favored: None,
                locked: None,
                hint_dependencies_available: available_bitmap,
                excluded: vec![],
            });
        }

        // Need to discover candidates via cheap ref query
        match self.discover_candidates_for_package(package_name).await {
            Ok(candidates) => Some(candidates),
            Err(e) => {
                tracing::warn!(
                    package = %package_name,
                    error = %e,
                    "failed to discover candidates"
                );
                None
            },
        }
    }

    /// Sort candidates by version descending (latest first)
    async fn sort_candidates(
        &self,
        _solver: &resolvo::SolverCache<Self>,
        solvables: &mut [SolvableId],
    ) {
        solvables.sort_by(|&a, &b| {
            let va = &self.pool.resolve_solvable(a).record;
            let vb = &self.pool.resolve_solvable(b).record;
            vb.cmp(va) // Descending order: latest first
        });
    }

    /// Get dependencies for a specific solvable
    /// This is where LAZY MANIFEST FETCHING happens
    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let solvable_data = self.pool.resolve_solvable(solvable);
        let package_name = self.pool.resolve_package_name(solvable_data.name);
        let version = &solvable_data.record;

        // Check manifest cache first
        let cache_key = (package_name.clone(), version.clone());
        if let Some(deps) = self.manifest_cache.borrow().get(&cache_key) {
            return self.convert_deps_to_resolvo(deps);
        }

        // Need to fetch manifest - this is the expensive operation
        match self.fetch_and_parse_manifest(package_name, version).await {
            Ok(deps) => {
                // Cache for future use
                self.manifest_cache.borrow_mut().insert(cache_key, deps.clone());
                self.convert_deps_to_resolvo(&deps)
            },
            Err(e) => {
                let reason = self
                    .pool
                    .intern_string(format!("manifest fetch failed: {}", e));
                Dependencies::Unknown(reason)
            },
        }
    }
}

impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Discover available versions for a package using cheap ref queries
    /// This does NOT fetch manifests - only refs
    async fn discover_candidates_for_package(
        &self,
        package: &AtomId<Root>,
    ) -> Result<Candidates, BoxError> {
        use crate::storage::QueryStore;

        // Construct ref query pattern: refs/eka/atoms/<label>/*
        let ref_pattern = format!(
            "{}/*:{}/*",
            format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
            format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
        );

        // Get URL for this package's repository from resolved sets
        let mirror_url = self.get_mirror_for_root(package.root())?;

        // Cheap ref query via gix::Url (metadata only, no git objects)
        let refs = {
            let mut transports = self.transports.borrow_mut();
            let transport = transports.get_mut(&mirror_url);
            mirror_url.get_refs([ref_pattern.as_str()], transport)?
        };

        // Parse refs into versions
        let mut versions = Vec::new();
        let name_id = self.pool.intern_package_name(package.clone());

        for r in refs {
            if let Some((version, rev)) = self.parse_atom_ref(&r, package.label()) {
                let record = AtomSolvableRecord {
                    version: version.clone(),
                    rev,
                    manifest_cached: self.is_manifest_cached(package, &version),
                    atom_id: package.compute_hash(),
                };

                let solvable_id = self.pool.intern_solvable(name_id, record.version);

                versions.push(DiscoveredVersion {
                    version,
                    rev,
                    solvable_id,
                });
            }
        }

        // Cache discovered versions
        self.discovered_candidates
            .borrow_mut()
            .insert(package.clone(), versions.clone());

        // Build result
        let candidate_ids: Vec<_> = versions.iter().map(|v| v.solvable_id).collect();
        let available_bitmap = self.build_availability_bitmap(&candidate_ids);

        Ok(Candidates {
            candidates: candidate_ids,
            favored: None,
            locked: None, // TODO: Check if version is already locked
            hint_dependencies_available: available_bitmap,
            excluded: vec![],
        })
    }

    /// Parse an atom ref like "refs/eka/atoms/foo/1.2.3" into (Version, Rev)
    fn parse_atom_ref(
        &self,
        r: &gix::protocol::handshake::Ref,
        expected_label: &Label,
    ) -> Option<(Version, GitDigest)> {
        use bstr::ByteSlice;

        let (name, ..) = r.unpack();
        let id = to_id(r.clone());
        let name_str = name.to_str().ok()?;

        // Expected format: refs/eka/atoms/<label>/<version>
        let prefix = format!("{}/{}/", crate::ATOM_REFS.as_str(), expected_label);
        let version_str = name_str.strip_prefix(&prefix)?;

        let version = Version::parse(version_str).ok()?;
        let rev = match id {
            gix::ObjectId::Sha1(bytes) => GitDigest::Sha1(bytes),
        };

        Some((version, rev))
    }

    /// Build availability bitmap based on which manifests are locally cached
    fn build_availability_bitmap(&self, solvables: &[SolvableId]) -> HintDependenciesAvailable {
        if solvables.is_empty() {
            return HintDependenciesAvailable::None;
        }

        let locally_available = self.locally_available.borrow();
        let all_available = solvables.iter().all(|id| locally_available.contains(id));
        let any_available = solvables.iter().any(|id| locally_available.contains(id));

        if all_available {
            HintDependenciesAvailable::All
        } else if any_available {
            // Build specific bitmap of available solvable IDs
            let available: Vec<SolvableId> = solvables
                .iter()
                .filter(|id| locally_available.contains(*id))
                .copied()
                .collect();
            HintDependenciesAvailable::Some(available)
        } else {
            HintDependenciesAvailable::None
        }
    }

    /// Get the mirror URL for a given root hash from resolved sets
    fn get_mirror_for_root(&self, root: &Root) -> Result<gix::Url, BoxError> {
        let root_digest = GitDigest::from(**root);
        // Look up set details by root hash
        if let Some(details) = self.resolved_sets.details().get(&root_digest) {
            // Get the first mirror URL from the set
            for mirror in &details.mirrors {
                if let SetMirror::Url(url) = mirror {
                    return Ok(url.clone());
                }
            }
        }
        Err(format!("No mirror found for root: {:?}", root).into())
    }

    /// Check if the manifest for a specific atom version is cached locally
    fn is_manifest_cached(&self, package: &AtomId<Root>, version: &Version) -> bool {
        let cache_key = (package.clone(), version.clone());
        self.manifest_cache.borrow().contains_key(&cache_key)
    }

    /// Resolve a set tag to its root hash
    fn resolve_set_tag_to_root(&self, set_tag: &Tag) -> Option<Root> {
        self.resolved_sets
            .roots()
            .get(&Either::Left(set_tag.clone()))
            .copied()
    }
}

impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Fetch and parse manifest for a specific atom version
    /// This is the EXPENSIVE operation - deferred until SAT solver needs deps
    async fn fetch_and_parse_manifest(
        &self,
        package: &AtomId<Root>,
        version: &Version,
    ) -> Result<Vec<AtomDependency>, BoxError> {
        tracing::debug!(
            package = %package,
            version = %version,
            "fetching manifest (lazy)"
        );

        // Get mirror URL
        let mirror_url = self.get_mirror_for_root(package.root())?;

        // Fetch the atom commit
        let _atom_ref = format!(
            "{}/{}/{}",
            crate::ATOM_REFS.as_str(),
            package.label(),
            version
        );

        // This fetches git objects - the expensive part
        let cache_repo = crate::storage::git::cache::repo()?;
        let local_repo = &cache_repo.to_thread_local();

        // Get or create transport, handling the RefCell borrow properly
        let commit = {
            let mut transports = self.transports.borrow_mut();
            let transport = transports
                .get_mut(&mirror_url)
                .ok_or_else(|| format!("No transport for mirror: {}", mirror_url))?;

            let (root, remote) = local_repo.ensure_remote(&mirror_url, transport)?;
            local_repo.resolve_atom_to_cache(
                &mut (root, remote),
                package.label(),
                version,
                transport,
            )?
        };

        // Parse manifest from commit tree
        let tree = commit.tree()?;
        let manifest_entry = tree
            .lookup_entry_by_path(crate::ATOM_MANIFEST_NAME.as_str())?
            .ok_or("atom.toml not found in commit")?;

        let blob = local_repo
            .find_object(manifest_entry.object_id())?
            .into_blob();
        let manifest_str = std::str::from_utf8(&blob.data)?;
        let manifest: ValidManifest = toml_edit::de::from_str(manifest_str)?;

        // Extract dependencies
        let mut deps = Vec::new();
        for (set_tag, set_deps) in manifest.as_ref().deps().from() {
            // Resolve set tag to root hash
            if let Some(set_root) = self.resolve_set_tag_to_root(set_tag) {
                for (label, version_req) in set_deps {
                    deps.push(AtomDependency {
                        target: AtomId::from((set_root, label.clone())),
                        version_req: version_req.clone(),
                    });
                }
            }
        }

        // Mark this solvable as now having deps available
        if let Some(versions) = self.discovered_candidates.borrow().get(package) {
            for v in versions {
                if &v.version == version {
                    self.locally_available.borrow_mut().insert(v.solvable_id);
                    break;
                }
            }
        }

        Ok(deps)
    }

    /// Convert parsed dependencies to resolvo's format
    fn convert_deps_to_resolvo(&self, deps: &[AtomDependency]) -> Dependencies {
        let requirements: Vec<ConditionalRequirement> = deps
            .iter()
            .filter_map(|dep| {
                let name_id = self.pool.lookup_package_name(&dep.target)?;
                let version_set = SemverVersionSet(dep.version_req.clone());
                let vs_id = self.pool.intern_version_set(name_id, version_set);

                Some(ConditionalRequirement {
                    condition: None,
                    requirement: Requirement::Single(vs_id),
                })
            })
            .collect();

        Dependencies::Known(KnownDependencies {
            requirements,
            constrains: vec![],
        })
    }
}
