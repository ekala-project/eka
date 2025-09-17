# Architectural Decision Record (ADR): Lock Generation in Eka CLI

## Status

Proposed

## Context

Eka requires a lockfile mechanism to capture resolved dependencies for reproducible builds, similar to Cargo.lock or flake.lock. This enables pinning exact versions/revisions of atoms and sources, supporting shallow/deep resolution. The lock schema must be language-agnostic for cross-ecosystem use (e.g., Nix integration), efficient for generation, and verifiable. Current Atom crate handles manifests; extend for locks. Challenges: Efficient remote querying without full clones, handling cross-atom 'from' references, type safety in Rust while keeping portable.

## Decision

1. **Schema Definition**: Use TOML as the primary format (aligns with Atom manifests). Define a formal schema using JSON Schema (via schemars crate) for validation. Generate Rust structs from schema using build.rs script with serde and toml_edit. This ensures agnosticism (TOML readable in any lang) while providing Rust type safety.

   Example Formal Schema (TOML structure):

   ```
   version = 1  # Lockfile version

   [[deps]]  # Array of dependencies
   name = "optional_name"  # For pins/sources; id for atoms
   id = "atom_id"  # For atom types
   version = "semver_or_ref"  # Optional for atoms
   rev = "git_rev"  # Resolved commit hash
   type = "atom | pin | pin+git | pin+tar"  # Required
   path = "relative_path"  # Optional; defaults to atom root if absent
   from = "source_atom_id"  # Optional; for cross-atom sourcing
   url = "source_url"  # For pins
   checksum = "sha256_hash"  # For tar/pins

   [[srcs]]  # Array for build-time sources (e.g., registries)
   name = "source_name"
   url = "fetch_url"
   type = "build"  # Or other types
   checksum = "sha256_hash"
   ```

   Validation rules:

   - Exactly one of id/url per dep.
   - type determines required fields (e.g., atom needs id/rev; pin needs url/checksum).
   - Paths relative to declaring atom; no cycles in 'from' refs.
   - Version 1 supports shallow resolution; future versions for deep.

2. **API Design**:

   - In `crates/atom/src/lock.rs`: Define `Lockfile` struct with `Vec<Dep>`, `Vec<Src>`, derive Serialize/Deserialize.
     - `pub fn generate_lock(manifest: &Manifest, store: &Store, mode: ResolutionMode) -> Result<Lockfile>`
     - Modes: Shallow (direct deps only), Deep (recursive with SAT via resolvo).
   - Integrate with URI resolution: Parse atom URIs, fetch refs via store.
   - Serialization: `Lockfile::to_toml()` using toml_edit for pretty-printing.
   - CLI Integration: In `src/cli/commands/resolve/mod.rs`, parse args (e.g., --shallow, --output), call API, write atom.lock.

3. **Remote Querying Strategy**:

   - Use `gix` crate for efficient operations: `gix::remote::ls_remote(url, refs_patterns)` to fetch refs without cloning.
   - Cache: Local dir `~/.eka/cache/refs/` with JSON files per repo (TTL 1h, invalidate on version change).
   - For atoms: Query `refs/atoms/{id}/*` patterns to list versions/revisions.
   - Fallback: Full shallow clone if ls-remote fails (e.g., auth issues).
   - Efficiency: Parallel fetches via Tokio for multi-dep resolution; limit to necessary refs (e.g., semver ranges).

4. **Resolution Flow**:

   - Parse manifest for direct deps (URIs/ids).
   - For each: Resolve URI to store ref, fetch available versions via querying.
   - Select matching version (latest/default or specified).
   - For 'from' refs: Recurse shallowly, resolve path in source atom.
   - Generate lock with resolved revs/checksums.
   - Validate: Checksum fetches for pins; git verify for atoms.

5. **Integration & Extensibility**:
   - Nix: Generate Nix-compatible imports from lock (future: export to flake.lock).
   - Plugins: Allow extending Dep fields via schema (e.g., custom types).
   - Testing: Use provided handwritten examples as golden tests via insta.

## Consequences

- **Pros**: Portable schema enables multi-lang tools; gix ensures performance; modular API fits Eka's design.
- **Cons**: JSON Schema adds build dep; caching needs eviction policy. Initial impl shallow-only.
- **Risks**: Auth for private repos (handle via git config); deep resolution complexity deferred.
- **Alternatives Considered**:
  - Pure Rust structs without schema: Less agnostic.
  - YAML/JSON for lock: TOML preferred for Nix alignment.
  - Full clone always: Inefficient startup.

## References

- Lock examples provided.
- gix docs: https://docs.rs/gix/latest/gix/
- JSON Schema in Rust: schemars crate.

```mermaid
flowchart TD
    A[Parse Manifest] --> B[Extract Direct Deps URIs/IDs]
    B --> C[For Each Dep: Resolve URI]
    C --> D{Fetch Refs via gix ls-remote<br/>(Cache Check First)}
    D -->|Hit| E[Select Version/Rev]
    D -->|Miss| F[Query Remote & Cache]
    F --> E
    E --> G[Handle 'from' Refs?]
    G -->|Yes| H[Recurse Shallowly]
    H --> E
    G -->|No| I[Compute Checksum/Path]
    I --> J[Build Lockfile]
    J --> K[Validate & Serialize to TOML]
    K --> L[Write atom.lock]
    style D fill:#f9f
```
