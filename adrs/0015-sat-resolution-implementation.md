# ADR-0015: SAT-Based Transitive Resolution Implementation Plan

## Status

Draft - Ready for Implementation

## Abstract

This ADR provides the detailed implementation plan for transitioning eka from direct-dependency-only resolution to full SAT-based transitive resolution using the resolvo crate. It builds upon ADR-0014's architectural decisions and specifies explicit algorithms, data structures, and integration points.

---

## Part 1: Executive Summary

### 1.1 Current State

The current resolution system (`crates/atom/src/package/resolve/mod.rs`) only resolves direct dependencies:

```rust
// Current: Only processes manifest's direct deps
fn synchronize_atoms(&mut self, manifest: &ValidManifest) -> Result<(), DocError> {
    for (set_tag, set) in manifest.as_ref().deps().from() {
        for (label, req) in set {
            self.synchronize_atom(req.to_owned(), id.to_owned(), set_tag.to_owned())?;
        }
    }
}
```

### 1.2 Target State

Full transitive resolution with lazy manifest fetching:
- SAT solver determines globally consistent version selection
- Manifests fetched only when SAT solver needs dependency information
- Complete transitive closure captured in lock file v2
- Per-dependency `requires` field enables graph reconstruction

### 1.3 Key Design Decisions

1. **Use resolvo's CDCL SAT solver** - Production-tested, async-native, supports lazy clause generation
2. **Implement custom `AtomDependencyProvider`** - Maps atom semantics to resolvo's trait
3. **Leverage `hint_dependencies_available`** - Signal cached vs requires-fetch atoms
4. **Cheap ref queries for candidate discovery** - Use `gix::Url::get_refs()` (metadata only)
5. **Lazy manifest fetch for dependencies** - Only fetch when solver needs deps

---

## Part 2: Data Structure Design

### 2.1 Atom Identity Mapping

**Problem**: Resolvo uses `NameId` for packages and `SolvableId` for versions. Atoms have two-level identity: `(Root, Label)`.

**Solution**: Use the existing `AtomId<Root>` type directly as the package name:

```rust
// AtomId<Root> already captures (Root, Label) - no need for a new type
// The existing implementation in crates/atom/src/id/mod.rs works perfectly:
//
// pub struct AtomId<R> {
//     root: R,
//     label: Label,
// }
//
// It implements Clone + PartialEq + Eq + Hash via derives,
// satisfying resolvo's PackageName trait requirements.

// For display in error messages:
impl<R: std::fmt::Display> std::fmt::Display for AtomId<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.root, self.label)
    }
}
```

The existing `AtomId<Root>` already has the right semantics:
- `Root` = genesis commit hash (repository identity)
- `Label` = atom name within that repository
- Implements `Clone + Eq + Hash` as required by resolvo's `PackageName` trait

### 2.2 Version Set Implementation

**Problem**: Resolvo's `VersionSet` trait requires `Clone + Eq + Hash`. Semver ranges satisfy this.

**Solution**: Use `semver::VersionReq` directly as the version set:

```rust
/// Wrapper for semver::VersionReq that implements resolvo's VersionSet trait
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SemverVersionSet(pub semver::VersionReq);

impl resolvo::utils::VersionSet for SemverVersionSet {
    type V = semver::Version;
}

impl SemverVersionSet {
    pub fn matches(&self, version: &semver::Version) -> bool {
        self.0.matches(version)
    }
}
```

### 2.3 Solvable Record Type

Each solvable (specific version of an atom) needs associated metadata:

```rust
/// Metadata for a specific atom version in the resolver pool
#[derive(Clone, Debug)]
pub struct AtomSolvableRecord {
    /// The concrete semantic version
    pub version: semver::Version,
    /// Git revision (commit hash) for this version
    pub rev: metadata::GitDigest,
    /// Whether we have the manifest cached (deps available without fetch)
    pub manifest_cached: bool,
    /// Machine-computed atom ID for verification
    pub atom_id: id::AtomDigest,
}
```

### 2.4 Lock File v2 Format

Extend `AtomDep` with transitive dependency information. Key changes from v1:
- **Remove `mirror` field** - nix reads from local store only (per ADR-0014)
- **`requires` uses `AtomDigest`** - concise reference by computed hash
- **Add `direct` flag** - distinguishes root's direct deps from transitives

```rust
/// Represents a locked atom dependency with its own requirements
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct AtomDepV2 {
    /// The unique identifier of the atom
    label: Label,
    /// The semantic version of the atom
    version: Version,
    /// Repository identity (root hash)
    set: GitDigest,
    /// Git revision (commit hash) for verification
    rev: GitDigest,
    /// Machine-computed cryptographic identity (BLAKE3 hash of AtomId)
    id: AtomDigest,
    /// NEW: Direct dependencies of this atom, referenced by their AtomDigest
    /// Enables transitive closure reconstruction as a hypergraph
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    requires: Vec<AtomDigest>,
    /// NEW: Whether this is a direct dependency of the root manifest
    #[serde(default, skip_serializing_if = "crate::package::metadata::manifest::not")]
    direct: bool,
}

// Note: AtomRef removed - we use AtomDigest directly for conciseness
// The AtomDigest uniquely identifies an atom and can be looked up in the deps table

/// Lock file with version field for migration
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockfileV2 {
    /// Schema version - 2 for transitive resolution
    pub version: u16,
    /// Set definitions with mirrors (used during resolution/store population only)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sets: BTreeMap<GitDigest, SetDetails>,
    /// Composer configuration
    pub compose: Using,
    /// Full transitive closure of dependencies
    #[serde(default, skip_serializing_if = "DepMap::is_empty")]
    pub deps: DepMap<Root, Dep>,
}
```

**Lock file v2 as hypergraph**: The structure naturally encodes a directed hypergraph where:
- **Nodes**: `AtomDigest` values (each resolved atom)
- **Edges**: Each dep's `requires` field forms hyperedges from that atom to its dependencies

This can be deserialized into `hyperdep::HyperGraph<AtomDigest, ()>` for traversal operations like topological sorting, cycle detection, and impact analysis.

---

## Part 3: AtomDependencyProvider Implementation

### 3.1 Core State Structure

```rust
/// Resolution context holding all state needed for dependency resolution
pub struct AtomResolver<'a, S: LocalStorage> {
    /// The resolvo pool for interning - uses AtomId<Root> directly as package name
    pool: Pool<SemverVersionSet, AtomId<Root>>,
    
    /// Mapping from AtomId to available versions discovered via ref queries
    discovered_candidates: HashMap<AtomId<Root>, Vec<DiscoveredVersion>>,
    
    /// Cache of fetched manifests: (AtomId, Version) -> parsed dependencies
    manifest_cache: HashMap<(AtomId<Root>, Version), Vec<AtomDependency>>,
    
    /// Set of solvables whose manifests are locally cached (cheap deps access)
    locally_available: HashSet<SolvableId>,
    
    /// Reference to resolved sets from manifest processing
    resolved_sets: &'a ResolvedSets<'a, S>,
    
    /// Storage backend for git operations
    storage: &'a S,
    
    /// Transports for remote operations (reusable connections)
    transports: HashMap<gix::Url, Box<dyn Transport + Send>>,
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
```

### 3.2 DependencyProvider Implementation

```rust
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
                let matches = vs.matches(&solvable.record.version);
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
        if let Some(versions) = self.discovered_candidates.get(package_name) {
            let candidate_ids: Vec<SolvableId> = versions
                .iter()
                .map(|v| v.solvable_id)
                .collect();
            
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
            }
        }
    }

    /// Sort candidates by version descending (latest first)
    async fn sort_candidates(
        &self,
        _solver: &resolvo::SolverCache<Self>,
        solvables: &mut [SolvableId],
    ) {
        solvables.sort_by(|&a, &b| {
            let va = &self.pool.resolve_solvable(a).record.version;
            let vb = &self.pool.resolve_solvable(b).record.version;
            vb.cmp(va) // Descending order: latest first
        });
    }

    /// Get dependencies for a specific solvable
    /// This is where LAZY MANIFEST FETCHING happens
    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let solvable_data = self.pool.resolve_solvable(solvable);
        let package_name = self.pool.resolve_package_name(solvable_data.name);
        let version = &solvable_data.record.version;
        
        // Check manifest cache first
        let cache_key = (package_name.clone(), version.clone());
        if let Some(deps) = self.manifest_cache.get(&cache_key) {
            return self.convert_deps_to_resolvo(deps);
        }
        
        // Need to fetch manifest - this is the expensive operation
        match self.fetch_and_parse_manifest(package_name, version).await {
            Ok(deps) => {
                // Cache for future use
                self.manifest_cache.insert(cache_key, deps.clone());
                self.convert_deps_to_resolvo(&deps)
            }
            Err(e) => {
                let reason = self.pool.intern_string(format!(
                    "manifest fetch failed: {}",
                    e
                ));
                Dependencies::Unknown(reason)
            }
        }
    }
}
```

### 3.3 Candidate Discovery (Cheap Ref Query)

```rust
impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Discover available versions for a package using cheap ref queries
    /// This does NOT fetch manifests - only refs
    async fn discover_candidates_for_package(
        &mut self,
        package: &AtomId<Root>,
    ) -> Result<Candidates, BoxError> {
        // Construct ref query pattern: refs/eka/atoms/<label>/*
        let ref_pattern = format!(
            "{}/*:{}/*",
            format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
            format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
        );
        
        // Get URL for this package's repository from resolved sets
        let mirror_url = self.get_mirror_for_root(package.root())?;
        
        // Cheap ref query via gix::Url (metadata only, no git objects)
        let transport = self.transports.get_mut(&mirror_url);
        let refs = mirror_url.get_refs([ref_pattern.as_str()], transport)?;
        
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
                
                let solvable_id = self.pool.intern_solvable(name_id, record);
                
                versions.push(DiscoveredVersion {
                    version,
                    rev,
                    solvable_id,
                });
            }
        }
        
        // Cache discovered versions
        self.discovered_candidates.insert(package.clone(), versions.clone());
        
        // Build result
        let candidate_ids: Vec<_> = versions.iter().map(|v| v.solvable_id).collect();
        let available_bitmap = self.build_availability_bitmap(&candidate_ids);
        
        Ok(Candidates {
            candidates: candidate_ids,
            favored: None,
            locked: None,  // TODO: Check if version is already locked
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
        
        let (name, id, ..) = r.unpack();
        let name_str = name.to_str().ok()?;
        
        // Expected format: refs/eka/atoms/<label>/<version>
        let prefix = format!("{}/{}/", crate::ATOM_REFS.as_str(), expected_label);
        let version_str = name_str.strip_prefix(&prefix)?;
        
        let version = Version::parse(version_str).ok()?;
        let rev = match id {
            gix::ObjectId::Sha1(bytes) => GitDigest::Sha1(*bytes),
        };
        
        Some((version, rev))
    }
    
    /// Build availability bitmap based on which manifests are locally cached
    fn build_availability_bitmap(&self, solvables: &[SolvableId]) -> HintDependenciesAvailable {
        if solvables.is_empty() {
            return HintDependenciesAvailable::None;
        }
        
        let all_available = solvables.iter().all(|id| self.locally_available.contains(id));
        let any_available = solvables.iter().any(|id| self.locally_available.contains(id));
        
        if all_available {
            HintDependenciesAvailable::All
        } else if any_available {
            // Build specific bitmap
            let mut indices = Vec::new();
            for (idx, id) in solvables.iter().enumerate() {
                if self.locally_available.contains(id) {
                    indices.push(idx as u32);
                }
            }
            HintDependenciesAvailable::Some(indices.into())
        } else {
            HintDependenciesAvailable::None
        }
    }
}
```

### 3.4 Manifest Fetching (Lazy, On-Demand)

```rust
impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Fetch and parse manifest for a specific atom version
    /// This is the EXPENSIVE operation - deferred until SAT solver needs deps
    async fn fetch_and_parse_manifest(
        &mut self,
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
        let atom_ref = format!(
            "{}/{}/{}",
            crate::ATOM_REFS.as_str(),
            package.label(),
            version
        );
        
        // This fetches git objects - the expensive part
        let cache_repo = storage::git::cache::repo()?;
        let local_repo = cache_repo.to_thread_local();
        let transport = self.transports.get_mut(&mirror_url);
        
        let (root, mut remote) = local_repo.ensure_remote(&mirror_url, transport)?;
        let commit = local_repo.resolve_atom_to_cache(
            &mut (root, remote),
            package.label(),
            version,
            transport,
        )?;
        
        // Parse manifest from commit tree
        let tree = commit.tree()?;
        let manifest_entry = tree.lookup_entry_by_path(crate::ATOM_MANIFEST_NAME.as_str())?
            .ok_or("atom.toml not found in commit")?;
        
        let blob = local_repo.find_object(manifest_entry.object_id())?.into_blob();
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
        if let Some(versions) = self.discovered_candidates.get(package) {
            for v in versions {
                if &v.version == version {
                    self.locally_available.insert(v.solvable_id);
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
```

### 3.5 Interner Implementation

```rust
impl<'a, S: LocalStorage> Interner for AtomResolver<'a, S> {
    fn display_solvable(&self, solvable: SolvableId) -> impl std::fmt::Display + '_ {
        let solvable = self.pool.resolve_solvable(solvable);
        let name = self.pool.resolve_package_name(solvable.name);
        format!("{}@{}", name, solvable.record.version)
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
        version_set_union: VersionSetUnionId,
    ) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(version_set_union)
    }

    fn resolve_condition(&self, condition: ConditionId) -> Condition {
        self.pool.resolve_condition(condition).clone()
    }
}
```

---

## Part 4: Resolution Algorithm

### 4.1 High-Level Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         RESOLUTION FLOW                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. INITIALIZATION                                                           │
│     ├── Read root manifest (atom.toml)                                       │
│     ├── Create AtomResolver with empty caches                                │
│     └── Build initial Problem from root's direct deps                        │
│                                                                              │
│  2. CANDIDATE DISCOVERY (cheap ref queries)                                  │
│     ├── For each required package, query refs/eka/atoms/<label>/*            │
│     ├── Parse versions from ref names                                        │
│     ├── Intern into pool as SolvableIds                                      │
│     └── Set hint_dependencies_available based on cache state                 │
│                                                                              │
│  3. SAT SOLVING (resolvo CDCL)                                               │
│     ├── Solver picks variable (package) with smallest domain                 │
│     ├── Solver picks value (version) - latest first                          │
│     ├── Solver calls get_dependencies() → LAZY MANIFEST FETCH               │
│     ├── New dependencies discovered → new clauses added                      │
│     ├── Propagation, conflict detection, backtracking                        │
│     └── Repeat until solution or UNSAT                                       │
│                                                                              │
│  4. SOLUTION EXTRACTION                                                      │
│     ├── Collect all selected solvables                                       │
│     ├── For each: (package, version, rev, deps)                              │
│     └── Build complete transitive closure                                    │
│                                                                              │
│  5. LOCK FILE GENERATION                                                     │
│     ├── Convert solution to AtomDepV2 entries                                │
│     ├── Include requires field for each dep                                  │
│     ├── Mark direct deps with direct: true                                   │
│     └── Write atom.lock v2 format                                            │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Entry Point

```rust
/// Resolve all transitive dependencies for a manifest
pub async fn resolve_transitive<S: LocalStorage>(
    manifest: &ValidManifest,
    resolved_sets: &ResolvedSets<'_, S>,
    storage: &S,
) -> Result<ResolutionResult, ResolutionError> {
    // 1. Initialize resolver
    let mut resolver = AtomResolver::new(resolved_sets, storage);
    
    // 2. Build initial problem from root manifest's direct deps
    let problem = build_problem_from_manifest(&mut resolver, manifest)?;
    
    // 3. Create solver and solve
    let solver = Solver::new(resolver);
    let solution = solver.solve(problem).await?;
    
    // 4. Extract and return result
    match solution {
        Ok(solvables) => {
            let result = extract_resolution_result(&resolver, &solvables, manifest)?;
            Ok(result)
        }
        Err(unsolvable) => {
            // TODO: Generate human-readable conflict explanation
            Err(ResolutionError::Unsolvable(format_conflict(&resolver, unsolvable)))
        }
    }
}

fn build_problem_from_manifest<S: LocalStorage>(
    resolver: &mut AtomResolver<'_, S>,
    manifest: &ValidManifest,
) -> Result<Problem, ResolutionError> {
    let mut requirements = Vec::new();
    
    // Process each direct dependency
    for (set_tag, deps) in manifest.as_ref().deps().from() {
        let root = resolver.resolve_set_tag_to_root(set_tag)
            .ok_or(ResolutionError::UnknownSet(set_tag.clone()))?;
        
        for (label, version_req) in deps {
            let package = AtomId::from((root, label.clone()));
            let name_id = resolver.pool.intern_package_name(package);
            let vs = SemverVersionSet(version_req.clone());
            let vs_id = resolver.pool.intern_version_set(name_id, vs);
            
            requirements.push(Requirement::Single(vs_id));
        }
    }
    
    // Include composer if it's an atom dependency
    if let Compose::With(composer) = manifest.as_ref().composer() {
        let root = resolver.resolve_set_tag_to_root(&composer.value.from)
            .ok_or(ResolutionError::UnknownSet(composer.value.from.clone()))?;
        
        let package = AtomId::from((root, composer.key.clone()));
        let name_id = resolver.pool.intern_package_name(package);
        let version_req = composer.value.version().cloned().unwrap_or(VersionReq::STAR);
        let vs = SemverVersionSet(version_req);
        let vs_id = resolver.pool.intern_version_set(name_id, vs);
        
        requirements.push(Requirement::Single(vs_id));
    }
    
    Ok(Problem {
        requirements,
        constraints: vec![],
        soft_requirements: vec![],
    })
}
```

### 4.3 Result Extraction

```rust
/// Result of successful resolution
pub struct ResolutionResult {
    /// All resolved dependencies with their own requirements
    pub deps: Vec<ResolvedDep>,
    /// Set details for lock file
    pub sets: BTreeMap<GitDigest, SetDetails>,
}

/// A single resolved dependency
pub struct ResolvedDep {
    pub label: Label,
    pub version: Version,
    pub set: GitDigest,
    pub rev: GitDigest,
    pub atom_id: AtomDigest,
    /// Direct dependencies referenced by their AtomDigest
    pub requires: Vec<AtomDigest>,
    pub direct: bool,
}

fn extract_resolution_result<S: LocalStorage>(
    resolver: &AtomResolver<'_, S>,
    solvables: &[SolvableId],
    root_manifest: &ValidManifest,
) -> Result<ResolutionResult, ResolutionError> {
    let direct_deps = collect_direct_deps(root_manifest, resolver)?;
    
    let mut deps = Vec::with_capacity(solvables.len());
    
    for &solvable_id in solvables {
        let solvable = resolver.pool.resolve_solvable(solvable_id);
        let package = resolver.pool.resolve_package_name(solvable.name);
        
        // Get dependencies for this solvable from cache
        let cache_key = (package.clone(), solvable.record.version.clone());
        let requires: Vec<AtomDigest> = resolver.manifest_cache
            .get(&cache_key)
            .map(|deps| {
                deps.iter()
                    .map(|d| d.target.compute_hash())
                    .collect()
            })
            .unwrap_or_default();
        
        deps.push(ResolvedDep {
            label: package.label().clone(),
            version: solvable.record.version.clone(),
            set: GitDigest::from(*package.root()),
            rev: solvable.record.rev,
            atom_id: solvable.record.atom_id,
            requires,
            direct: direct_deps.contains(&package),
        });
    }
    
    // Collect set details
    let mut sets = BTreeMap::new();
    for dep in &deps {
        if let Some(details) = resolver.resolved_sets.details().get(&dep.set) {
            sets.insert(dep.set, details.clone());
        }
    }
    
    Ok(ResolutionResult { deps, sets })
}
```

---

## Part 5: Lock File Migration

### 5.1 Version Detection and Upgrade

```rust
impl Lockfile {
    /// Detect lock file version and upgrade if necessary
    pub fn from_str_with_upgrade(content: &str) -> Result<Self, LockError> {
        // Try parsing as-is first
        if let Ok(lock) = toml_edit::de::from_str::<Lockfile>(content) {
            if lock.version >= 2 {
                return Ok(lock);
            }
            // v1 lock detected - needs upgrade at resolution time
            tracing::info!("detected v1 lock file, will upgrade to v2 on next resolve");
            return Ok(lock);
        }
        
        Err(LockError::ParseFailed)
    }
    
    /// Check if lock file is v2 with full transitive closure
    pub fn is_v2_complete(&self) -> bool {
        self.version >= 2 && self.deps.as_ref().values().all(|dep| {
            match dep {
                Dep::Atom(atom) => atom.requires.is_some(),
                _ => true,
            }
        })
    }
}
```

### 5.2 Writing Lock File v2

```rust
impl ResolutionResult {
    /// Convert resolution result to lock file format
    pub fn to_lockfile(&self, compose: Using) -> Lockfile {
        let mut deps = DepMap::default();
        
        for resolved in &self.deps {
            let id = AtomId::from((
                Root::from(resolved.set),
                resolved.label.clone(),
            ));
            
            let atom_dep = AtomDepV2 {
                label: resolved.label.clone(),
                version: resolved.version.clone(),
                set: resolved.set,
                rev: resolved.rev,
                mirror: None, // Mirrors looked up from sets table
                id: resolved.atom_id,
                requires: resolved.requires.clone(),
                direct: resolved.direct,
            };
            
            deps.as_mut().insert(
                Either::Left(id),
                Dep::Atom(atom_dep.into()),
            );
        }
        
        Lockfile {
            version: 2,
            sets: self.sets.clone(),
            compose,
            deps,
        }
    }
}
```

---

## Part 6: Integration with Existing Code

### 6.1 ManifestWriter Changes

Update `synchronize_atoms` to use full resolution:

```rust
impl<'a, S: LocalStorage> ManifestWriter<'a, S> {
    /// Synchronize lock file with manifest using full transitive resolution
    pub(super) async fn synchronize_full(&mut self, manifest: &ValidManifest) -> Result<(), DocError> {
        // Check if existing lock is v2 complete and still valid
        if self.lock.is_v2_complete() && self.check_lock_validity(manifest)? {
            tracing::debug!("existing v2 lock is valid, skipping re-resolution");
            return Ok(());
        }
        
        // Perform full transitive resolution
        let result = resolve_transitive(
            manifest,
            &self.resolved,
            self.resolved.ekala.storage,
        ).await.map_err(|e| {
            tracing::error!(error = %e, "transitive resolution failed");
            DocError::ResolutionFailed
        })?;
        
        // Update lock file with resolved deps
        self.lock = result.to_lockfile(self.lock.compose.clone());
        
        // Synchronize direct (non-atom) dependencies
        self.synchronize_direct(manifest).await?;
        
        Ok(())
    }
    
    /// Check if existing lock file is still compatible with manifest requirements
    fn check_lock_validity(&self, manifest: &ValidManifest) -> Result<bool, DocError> {
        for (set_tag, deps) in manifest.as_ref().deps().from() {
            let root = self.resolved.roots()
                .get(&Either::Left(set_tag.clone()))
                .ok_or(DocError::SetNotResolved)?;
            
            for (label, version_req) in deps {
                let id = AtomId::from((*root, label.clone()));
                
                match self.lock.deps.as_ref().get(&Either::Left(id)) {
                    Some(Dep::Atom(locked)) => {
                        if !version_req.matches(locked.version()) {
                            return Ok(false);
                        }
                    }
                    _ => return Ok(false),
                }
            }
        }
        
        Ok(true)
    }
}
```

### 6.2 CLI Command Updates

Update `eka resolve` command:

```rust
/// Resolution command implementation
pub async fn run_resolve(args: ResolveArgs) -> Result<(), Error> {
    let storage = get_storage()?;
    let manifest_path = args.manifest.unwrap_or_else(|| PathBuf::from("atom.toml"));
    
    // Open and validate manifest
    let mut writer = ManifestWriter::open_and_resolve(
        &storage,
        &manifest_path,
        args.fresh,
    ).await?;
    
    // Perform full transitive resolution
    let manifest = writer.manifest();
    writer.synchronize_full(manifest).await?;
    
    // Write updated lock file
    writer.write_atomic()?;
    
    tracing::info!(
        deps_count = writer.lock().deps.as_ref().len(),
        "resolution complete"
    );
    
    Ok(())
}
```

---

## Part 7: Optimizations

### 7.1 Speculative Prefetching

When the solver picks a package, speculatively fetch manifests for top-k candidates in parallel:

```rust
impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Speculatively prefetch manifests for likely candidates
    async fn prefetch_candidates(&mut self, name: NameId, k: usize) {
        let package: &AtomId<Root> = self.pool.resolve_package_name(name);
        
        if let Some(versions) = self.discovered_candidates.get(package) {
            // Take top k versions (already sorted descending)
            let to_prefetch: Vec<_> = versions.iter()
                .take(k)
                .filter(|v| !self.manifest_cache.contains_key(&(package.clone(), v.version.clone())))
                .cloned()
                .collect();
            
            if to_prefetch.is_empty() {
                return;
            }
            
            tracing::debug!(
                package = %package,
                count = to_prefetch.len(),
                "prefetching candidate manifests"
            );
            
            // Spawn parallel fetch tasks
            let mut handles = Vec::new();
            for v in to_prefetch {
                let package = package.clone();
                let version = v.version.clone();
                let handle = tokio::spawn(async move {
                    // Fetch logic here - use shared state carefully
                    (package, version)
                });
                handles.push(handle);
            }
            
            // Wait for all to complete (or timeout)
            for handle in handles {
                if let Ok((pkg, ver)) = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    handle,
                ).await.ok().and_then(|r| r.ok()) {
                    // Mark as prefetched
                }
            }
        }
    }
}
```

### 7.2 Lock File as Cache Hint

Use existing lock file to hint `favored` and `locked` candidates:

```rust
fn apply_lock_hints(
    resolver: &AtomResolver<'_, impl LocalStorage>,
    lock: &Lockfile,
    candidates: &mut Candidates,
    package: &AtomId<Root>,
) {
    if lock.version < 2 {
        return;
    }
    
    // Find existing locked version for this package
    for (key, dep) in lock.deps.as_ref() {
        if let (Either::Left(id), Dep::Atom(atom)) = (key, dep) {
            if id.root() == package.root()
                && id.label() == package.label()
            {
                // Find the solvable for this version
                if let Some(solvable_id) = candidates.candidates.iter().find(|&&sid| {
                    let solvable = resolver.pool.resolve_solvable(sid);
                    &solvable.record.version == atom.version()
                }) {
                    candidates.favored = Some(*solvable_id);
                    candidates.locked = Some(*solvable_id);
                }
                break;
            }
        }
    }
}
```

### 7.3 Parallel Candidate Discovery

Use tokio's JoinSet for parallel ref queries:

```rust
impl<'a, S: LocalStorage> AtomResolver<'a, S> {
    /// Discover candidates for multiple packages in parallel
    async fn discover_candidates_batch(
        &mut self,
        packages: Vec<AtomId<Root>>,
    ) -> Result<(), BoxError> {
        use tokio::task::JoinSet;
        
        let mut tasks = JoinSet::new();
        
        for package in packages {
            if self.discovered_candidates.contains_key(&package) {
                continue;
            }
            
            let ref_pattern = format!(
                "{}/*:{}/*",
                format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
                format!("{}/{}", crate::ATOM_REFS.as_str(), package.label()),
            );
            let mirror_url = self.get_mirror_for_root(package.root())?;
            
            tasks.spawn(async move {
                // Cheap ref query
                let refs = tokio::task::spawn_blocking(move || {
                    mirror_url.get_refs([ref_pattern.as_str()], None)
                }).await??;
                
                Ok::<_, BoxError>((package, refs))
            });
        }
        
        while let Some(result) = tasks.join_next().await {
            let (package, refs) = result??;
            self.process_discovered_refs(package, refs)?;
        }
        
        Ok(())
    }
}
```

---

## Part 8: Error Handling and Diagnostics

### 8.1 Resolution Error Types

```rust
#[derive(thiserror::Error, Debug)]
pub enum ResolutionError {
    #[error("no candidates found for package: {0}")]
    NoCandidates(AtomId<Root>),
    
    #[error("no version satisfies requirements for {package}: {requirement}")]
    NoMatchingVersion {
        package: AtomId<Root>,
        requirement: VersionReq,
    },
    
    #[error("package set not found: {0}")]
    UnknownSet(Tag),
    
    #[error("manifest fetch failed for {package}@{version}: {reason}")]
    ManifestFetchFailed {
        package: AtomId<Root>,
        version: Version,
        reason: String,
    },
    
    #[error("resolution conflict:\n{explanation}")]
    Unsolvable {
        explanation: String,
    },
    
    #[error(transparent)]
    Storage(#[from] storage::git::Error),
    
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

### 8.2 Conflict Explanation

Resolvo provides built-in conflict reporting via the `Problem` type when solving fails.
We leverage this directly rather than rolling our own:

```rust
use resolvo::UnsolvableOrCancelled;

fn handle_resolution_failure<S: LocalStorage>(
    resolver: &AtomResolver<'_, S>,
    result: UnsolvableOrCancelled<Box<dyn std::any::Any>>,
) -> ResolutionError {
    match result {
        UnsolvableOrCancelled::Unsolvable(problem) => {
            // Resolvo's Problem type has display_user_friendly() for conflict explanation
            // It walks the conflict graph and explains:
            // - Which packages have incompatible requirements
            // - What version constraints led to the conflict
            // - The derivation chain showing how we got there
            
            // Use resolvo's built-in formatting
            let explanation = problem.display_user_friendly(resolver).to_string();
            
            ResolutionError::Unsolvable { explanation }
        }
        UnsolvableOrCancelled::Cancelled(reason) => {
            ResolutionError::Cancelled {
                reason: format!("{:?}", reason),
            }
        }
    }
}
```

The `Problem::display_user_friendly()` method from resolvo generates clear explanations like:

```
The following packages are incompatible:
├─ root requires foo ^2.0.0
│  └─ foo 2.0.0 requires bar ^3.0.0
│     └─ no version of bar satisfies ^3.0.0 AND ^2.0.0
└─ root requires baz ^1.0.0
   └─ baz 1.0.0 requires bar ^2.0.0
```

---

## Part 9: Testing Strategy

### 9.1 Testing Strategy

**Principle**: Avoid MockResolver with different semantics. Instead:

1. **Unit tests**: Test individual functions (ref parsing, version matching, constraint conversion)
2. **Integration tests**: Use git fixtures that match real atom repository structure
3. **Snapshot tests**: Compare resolution results against known-good outputs

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    /// Create a test git repository with atom refs
    fn setup_test_repo() -> (TempDir, gix::Repository) {
        let tmp = TempDir::new().unwrap();
        let repo = gix::init_bare(&tmp).unwrap();
        
        // Create atom refs matching real structure:
        // refs/eka/atoms/<label>/<version> -> commit
        create_test_atom(&repo, "foo", "1.0.0", &[]);
        create_test_atom(&repo, "foo", "1.1.0", &[("bar", "^1.0")]);
        create_test_atom(&repo, "bar", "1.0.0", &[]);
        create_test_atom(&repo, "bar", "2.0.0", &[]);
        
        (tmp, repo)
    }
    
    fn create_test_atom(
        repo: &gix::Repository,
        label: &str,
        version: &str,
        deps: &[(&str, &str)],
    ) {
        // Create atom.toml content with deps
        let manifest = format!(
            r#"[package]
label = "{}"
version = "{}"
[compose.as.nix]
nix = "."
[deps.from.test-set]
{}
"#,
            label,
            version,
            deps.iter()
                .map(|(l, v)| format!("{} = \"{}\"", l, v))
                .collect::<Vec<_>>()
                .join("\n")
        );
        
        // Create tree with atom.toml
        let blob = repo.write_blob(manifest.as_bytes()).unwrap();
        let tree = repo.write_object(&gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::object::tree::EntryKind::Blob.into(),
                oid: blob.detach(),
                filename: "atom.toml".into(),
            }],
        }).unwrap();
        
        // Create commit
        let commit = git::write_atom_commit_to_repo(
            repo, tree.detach(), label, version, "test-root"
        ).unwrap();
        
        // Create ref
        repo.reference(
            format!("refs/eka/atoms/{}/{}", label, version),
            commit.tip(),
            gix::refs::transaction::PreviousValue::Any,
            "test setup",
        ).unwrap();
    }
    
    #[tokio::test]
    async fn test_resolve_simple_chain() {
        let (_tmp, repo) = setup_test_repo();
        let storage = TestStorage::new(&repo);
        
        // Resolve foo ^1.0 which depends on bar ^1.0
        let manifest = parse_manifest(r#"
            [package]
            label = "root"
            version = "0.1.0"
            [deps.from.test-set]
            foo = "^1.0"
        "#);
        
        let result = resolve_transitive(&manifest, &storage).await.unwrap();
        
        // Should select foo 1.1.0 (latest matching ^1.0) and bar 2.0.0 (latest matching ^1.0)
        assert_eq!(result.get_version("foo"), Some(&Version::parse("1.1.0").unwrap()));
        assert_eq!(result.get_version("bar"), Some(&Version::parse("2.0.0").unwrap()));
    }
    
    #[tokio::test]
    async fn test_conflict_produces_readable_error() {
        let (_tmp, repo) = setup_test_repo();
        // Add conflicting atoms
        create_test_atom(&repo, "left", "1.0.0", &[("shared", "^1.0")]);
        create_test_atom(&repo, "right", "1.0.0", &[("shared", "^2.0")]);
        create_test_atom(&repo, "shared", "1.0.0", &[]);
        create_test_atom(&repo, "shared", "2.0.0", &[]);
        
        let storage = TestStorage::new(&repo);
        let manifest = parse_manifest(r#"
            [package]
            label = "root"
            version = "0.1.0"
            [deps.from.test-set]
            left = "*"
            right = "*"
        "#);
        
        let result = resolve_transitive(&manifest, &storage).await;
        assert!(result.is_err());
        
        // Check error message is useful
        let err = result.unwrap_err();
        assert!(err.to_string().contains("shared"));
        assert!(err.to_string().contains("^1.0"));
        assert!(err.to_string().contains("^2.0"));
    }
}
```

### 9.2 Integration Tests

```rust
#[test]
fn test_lockfile_v2_roundtrip() {
    let lock_content = r#"
version = 2

[sets."abc123"]
tag = "test-atoms"
mirrors = ["https://github.com/test/atoms"]

[[deps]]
type = "atom"
label = "foo"
version = "1.0.0"
set = "abc123"
rev = "def456"
id = "789ghi"
requires = [{ set = "abc123", label = "bar" }]
direct = true

[[deps]]
type = "atom"
label = "bar"
version = "2.0.0"
set = "abc123"
rev = "jkl012"
id = "mno345"
requires = []
"#;

    let lock: Lockfile = toml_edit::de::from_str(lock_content).unwrap();
    assert_eq!(lock.version, 2);
    
    let reserialized = toml_edit::ser::to_string_pretty(&lock).unwrap();
    let reparsed: Lockfile = toml_edit::de::from_str(&reserialized).unwrap();
    assert_eq!(lock, reparsed);
}
```

---

## Part 10: Implementation Phases

### Phase 1: Data Structures (Week 1)

- [ ] Add `Display` impl to `AtomId<Root>` for error messages
- [ ] Implement `SemverVersionSet` wrapper for resolvo's `VersionSet` trait
- [ ] Implement `AtomSolvableRecord` type for version metadata
- [ ] Extend `AtomDep` with `requires: Vec<AtomDigest>` and `direct: bool` fields
- [ ] Update `Lockfile` for v2 support (remove `mirror` from deps, add version field)
- [ ] Add migration path for v1 → v2 lock files
- [ ] Add unit tests for new types

### Phase 2: AtomResolver Core (Week 2)

- [ ] Implement `AtomResolver` struct with caches
- [ ] Implement `DependencyProvider` trait methods
- [ ] Implement `Interner` trait methods
- [ ] Implement cheap ref queries for `get_candidates`
- [ ] Implement lazy manifest fetch for `get_dependencies`
- [ ] Add unit tests with mock transport

### Phase 3: Resolution Entry Point (Week 3)

- [ ] Implement `resolve_transitive` function
- [ ] Implement `build_problem_from_manifest`
- [ ] Implement `extract_resolution_result`
- [ ] Implement conflict formatting
- [ ] Update `ManifestWriter::synchronize` to use new resolver
- [ ] Add integration tests

### Phase 4: Optimization (Week 4)

- [ ] Implement speculative prefetching
- [ ] Implement lock file hints (`favored`, `locked`)
- [ ] Implement parallel candidate discovery
- [ ] Performance benchmarking
- [ ] Document optimization strategies

### Phase 5: CLI & Testing (Week 5)

- [ ] Update `eka resolve` command
- [ ] Update `eka lock` command
- [ ] Add `--fresh` flag for force re-resolution
- [ ] Add `--dry-run` flag for preview
- [ ] Integration tests with git fixtures (not mocks)
- [ ] Documentation updates

### Phase 6: Nix Integration (Week 6)

- [ ] Update evaluation manifest generation
- [ ] Update nix-lock static code for v2 format
- [ ] Test `eka plan` with new resolution
- [ ] Verify store population from v2 lock
- [ ] Migration guide documentation

---

## Part 11: Hypergraph Integration

### 11.1 Lock File as Hypergraph

The lock file v2 format naturally encodes a directed hypergraph:

```
Lock File Structure:
┌─────────────────────────────────────────────────────────────────────┐
│  deps[0]: { id: A, requires: [B, C] }  →  Hyperedge: A → {B, C}    │
│  deps[1]: { id: B, requires: [D] }     →  Hyperedge: B → {D}       │
│  deps[2]: { id: C, requires: [D, E] }  →  Hyperedge: C → {D, E}    │
│  deps[3]: { id: D, requires: [] }      →  Leaf node: D             │
│  deps[4]: { id: E, requires: [] }      →  Leaf node: E             │
└─────────────────────────────────────────────────────────────────────┘

Hypergraph Representation:
       A
      / \
     B   C
     |  / \
     D    E
```

### 11.2 Using hyperdep for Analysis

```rust
use hyperdep::HyperGraph;

impl LockfileV2 {
    /// Convert lock file to hypergraph for analysis operations
    pub fn to_hypergraph(&self) -> HyperGraph<AtomDigest, ()> {
        let mut graph = HyperGraph::new();
        
        for (_key, dep) in self.deps.as_ref() {
            if let Dep::Atom(atom) = dep {
                // Add node
                graph.add_node(atom.id);
                
                // Add hyperedge from this atom to its requirements
                if !atom.requires.is_empty() {
                    graph.add_edge(atom.id, atom.requires.clone(), ());
                }
            }
        }
        
        graph
    }
    
    /// Get topological order for evaluation (leaves first)
    pub fn topo_order(&self) -> Vec<AtomDigest> {
        self.to_hypergraph().topo_sort().unwrap_or_default()
    }
    
    /// Find all atoms that depend on a given atom (reverse deps)
    pub fn dependents_of(&self, atom: AtomDigest) -> Vec<AtomDigest> {
        self.to_hypergraph().predecessors(&atom)
    }
    
    /// Detect dependency cycles (should be empty for valid lock)
    pub fn find_cycles(&self) -> Vec<Vec<AtomDigest>> {
        self.to_hypergraph().find_cycles()
    }
}
```

### 11.3 Use Cases

| Operation | hyperdep Method | Use Case |
|-----------|-----------------|----------|
| `topo_sort()` | Topological order | `eka plan` evaluation order |
| `predecessors(node)` | Reverse dependencies | "What will break if I change X?" |
| `successors(node)` | Forward dependencies | "What does X need?" |
| `find_cycles()` | Cycle detection | Lock file validation |
| `subgraph(nodes)` | Extract subset | Partial resolution |

### 11.4 Note on Resolution vs. Hypergraph

The hypergraph representation is for **post-resolution analysis**, not resolution itself:

- **Resolution**: Handled by resolvo's SAT solver with its internal clause representation
- **Analysis**: Once resolved, the lock file → hypergraph conversion enables graph queries

This separation keeps concerns clean: resolvo handles the hard constraint satisfaction, hyperdep handles the graph traversal for downstream operations.

---

## Appendix A: Comparison with Cargo and Pixi

| Aspect | Cargo | Pixi/Rattler | Eka (proposed) |
|--------|-------|--------------|----------------|
| Solver | Custom PubGrub | resolvo (CDCL) | resolvo (CDCL) |
| Lock format | Cargo.lock | pixi.lock | atom.lock v2 |
| Transitive deps | Stored with `requires` | Stored flat | Stored with `requires` |
| Lazy loading | Version index | Full index | Cheap refs + lazy manifest |
| Package identity | name@registry | name::channel | root::label |
| Version constraint | semver | conda VersionSpec | semver |

---

## Appendix B: Glossary

| Term | Definition |
|------|------------|
| **Atom** | Orphaned git commit containing a package's source tree |
| **Root** | Repository identity (genesis commit hash) |
| **Label** | Atom name within a repository |
| **AtomId** | (Root, Label) pair uniquely identifying an atom package |
| **Solvable** | Specific version of a package in resolvo's model |
| **VersionSet** | Constraint on acceptable versions (e.g., `^1.0.0`) |
| **CDCL** | Conflict-Driven Clause Learning (SAT algorithm) |
| **Cheap ref query** | Git ls-remote that fetches ref metadata without objects |
| **Manifest** | atom.toml file declaring dependencies |
| **Lock** | atom.lock file pinning exact versions |

---

## References

- [resolvo crate documentation](https://docs.rs/resolvo)
- [ADR-0014: Resolution Architecture](./0014-resolution-architecture.md)
- [Cargo resolver algorithm](https://doc.rust-lang.org/cargo/reference/resolver.html)
- [PubGrub solver](https://nex3.medium.com/pubgrub-2fb6470504f)
- [CDCL algorithm overview](https://en.wikipedia.org/wiki/Conflict-driven_clause_learning)