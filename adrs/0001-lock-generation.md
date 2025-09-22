# Architectural Decision Record (ADR): Lock Generation in Eka CLI

## Status

Implemented

## Context

Eka requires a lockfile mechanism to capture resolved dependencies for reproducible builds, similar to Cargo.lock or flake.lock. This enables pinning exact versions/revisions of atoms and sources, supporting shallow/deep resolution. The lock schema must be language-agnostic for cross-ecosystem use (e.g., Nix integration), efficient for generation, and verifiable. Current Atom crate handles manifests; extend for locks. Challenges: Efficient remote querying without full clones, handling cross-atom 'from' references, type safety in Rust while keeping portable.

## Decision

1. **Schema Definition**: Use TOML as the primary format (aligns with Atom manifests). Define Rust structs with serde serialization using tagged enums for type safety. Use `#[serde(tag = "type")]` for TOML serialization as tagged tables, ensuring both Rust type safety and TOML portability. Implement strict validation with `#[serde(deny_unknown_fields)]`.

    Example Lockfile Structure (TOML):

    ```
    version = 1

    [[deps]]
    type = "atom"
    id = "nix"
    version = "0.1.2"
    rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"

    [[deps]]
    type = "atom"
    id = "home"
    version = "0.1.8"
    rev = "795ae541b7fd67dd3c6e1a9dddf903312696aa17"
    url = "https://git.example.com/my-repo.git"

    [[deps]]
    type = "from"
    name = "eval-config"
    path = "nixos/lib/eval-config.nix"
    from = "nix"
    get = "nixpkgs"

    [[deps]]
    type = "pin+git"
    name = "hm-module"
    url = "https://github.com/nix-community/home-manager.git"
    rev = "d0300c8808e41da81d6edfc202f3d3833c157daf"
    path = "nixos"

    [[deps]]
    type = "pin"
    name = "foks"
    url = "https://raw.githubusercontent.com/NixOS/nixpkgs/393d5e815a19acad8a28fc4b27085e42c483b4f6/pkgs/by-name/fo/foks/package.nix"
    hash = "sha256:1spc2lsx16xy612lg8rsyd34j9fy6kmspxcvcfmawkxmyvi32g9v"

    [[srcs]]
    type = "build"
    name = "registry"
    url = "https://raw.githubusercontent.com/NixOS/flake-registry/refs/heads/master/flake-registry.json"
    hash = "sha256-hClMprWwiEQe7mUUToXZAR5wbhoVFi+UuqLL2K/eIPw="
    ```

    **Dependency Types**:

    - **atom**: `AtomDep` with id, version, rev, optional url/path
    - **pin**: `PinDep` with name, url, hash, optional path
    - **pin+git**: `PinGitDep` with name, url, rev, optional path
    - **pin+tar**: `PinTarDep` with name, url, hash, optional path
    - **from**: `FromDep` with name, from, optional get/path

    **Source Types**:

    - **build**: `BuildSrc` with name, url, hash

    Validation rules:

    - Tagged enum ensures type determines required fields at compile time
    - `#[serde(deny_unknown_fields)]` prevents unknown fields
    - `WrappedNixHash` for Nix-compatible hash validation
    - `GitSha` enum supports both sha1 and sha256 git revisions
    - Paths relative to declaring atom; no cycles in 'from' refs

2. **API Design**:

    - In `crates/atom/src/lock.rs`: Define `Lockfile` struct with `Option<Vec<Dep>>`, `Option<Vec<Src>>` for optional serialization.
      - Tagged enums: `Dep` with variants (Atom, Pin, PinGit, PinTar, From), `Src` with variants (Build).
      - `WrappedNixHash` wrapper for Nix-compatible hash handling.
      - `GitSha` enum supporting both sha1 and sha256 git revisions.
      - `AtomLocation` enum for flexible URL/path specification.
      - `ResolutionMode` enum: Shallow (direct deps only), Deep (recursive - future).
    - Integration with URI resolution: Parse atom URIs, fetch refs via store.
    - Serialization: Uses serde with `#[serde(tag = "type")]` for tagged TOML tables.
    - CLI Integration: In `src/cli/commands/resolve/mod.rs`, parse args (path, output, mode), call API, write atom.lock.

3. **Remote Querying Strategy**:

    - Use `gix` crate for efficient operations to fetch refs without cloning.
    - Cache: Local directory `~/.eka/cache/refs/` with JSON files per repo (TTL 1h, invalidate on version change).
    - For atoms: Query `refs/atoms/{id}/*` patterns to list versions/revisions.
    - Fallback: Full shallow clone if ls-remote fails (e.g., auth issues).
    - Efficiency: Parallel fetches via Tokio for multi-dep resolution; limit to necessary refs (e.g., semver ranges).
    - Git remote handling: Support for custom remotes via `--remote` flag in CLI.

4. **Resolution Flow**:

    - Parse manifest for direct deps (URIs/ids).
    - For each dependency: Resolve URI to store ref, fetch available versions via querying.
    - Select matching version (latest/default or specified).
    - For 'from' refs: Recurse shallowly, resolve path in source atom.
    - Generate lock with resolved revs/checksums using tagged enum variants.
    - Validate: Checksum fetches for pins; git verify for atoms.
    - Handle different dependency types: atoms, pins, pin+git, pin+tar, from.
    - Support for build-time sources (registries, etc.).

5. **Integration & Extensibility**:

    - Nix: Generate Nix-compatible imports from lock (future: export to flake.lock).
    - Type Safety: Tagged enums ensure compile-time validation of dependency types.
    - Testing: Use handwritten examples as golden tests via insta in `crates/atom/src/lock/test.rs`.
    - Extensibility: `#[serde(deny_unknown_fields)]` prevents unknown fields while allowing future type additions.

## Consequences

- **Pros**: Tagged enum approach provides compile-time type safety while maintaining TOML portability; gix ensures performance; modular API fits Eka's design; strict field validation prevents runtime errors.
- **Cons**: Tagged enum approach requires more complex serde setup; caching needs eviction policy. Initial impl shallow-only.
- **Risks**: Auth for private repos (handle via git config); deep resolution complexity deferred; hash validation requires careful Nix integration.
- **Alternatives Considered**:
  - Pure Rust structs without tagged enums: Less type-safe.
  - YAML/JSON for lock: TOML preferred for Nix alignment.
  - Full clone always: Inefficient startup.
  - JSON Schema validation: More complex build setup than tagged enums.

## References

- Lock examples in `crates/atom/src/lock/test.rs`
- gix docs: https://docs.rs/gix/latest/gix/
- Serde tagged enums: https://serde.rs/enum-representations.html
- NixHash integration: nix-compat crate

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
