# 3. Pin Dependency Resolution via a `snix`-based Fetch Cache

Date: 2025-10-02

## Status

Proposed

## Context

The resolution of `pin` dependencies in `eka` is currently unimplemented. The architectural goal is to handle dependency resolution within `eka` itself, while deferring all true evaluation and building to a future backend daemon (`eos`). This requires a pure Rust solution for fetching and hashing pins without shelling out to `nix`.

Our exploration of `snix` has revealed a high-level `Fetcher` service in the `snix-glue` crate that orchestrates the entire process of fetching sources and calculating their Nix-compatible NAR hashes. This `Fetcher` is designed to work with backend services for blob and directory storage, as defined by the `snix-castore` crate.

This presents an opportunity to not only implement pin resolution but also to introduce a robust, efficient caching mechanism for `eka` that aligns with its long-term architectural goals.

## Decision

We will implement pin dependency resolution by leveraging the `snix` stack as a dedicated "fetch cache" for `eka`. We will depend on the `snix-glue`, `snix-store`, and `snix-castore` crates and use their on-disk storage backends to create a persistent, content-addressed cache for fetched dependencies.

This approach treats the fetching and hashing process as a caching operation, which is an appropriate responsibility for the `eka` frontend. It avoids re-implementing complex logic, provides significant efficiency gains through deduplication, and aligns with the future separation of concerns between `eka` and `eos`.

### Detailed Design

#### 1. Add `snix` Dependencies

We will add the following crates from the `snix` workspace as dependencies to `crates/atom`:

- `snix-glue`
- `snix-store`
- `snix-castore`
- `nix-compat` (transitive)

#### 2. Configuration and Cache Initialization

To make the cache location configurable, we will add a new section to the `eka` config.

**`crates/config/src/lib.rs`:**

A `cache` field will be added to the `Config` struct.

```rust
#[derive(Deserialize, Serialize)]
pub struct CacheConfig<'a> {
    #[serde(borrow)]
    pub root_dir: Option<&'a str>,
}

#[derive(Deserialize, Serialize)]
pub struct Config<'a> {
    #[serde(borrow)]
    aliases: Aliases<'a>,
    #[serde(borrow)]
    pub cache: Option<CacheConfig<'a>>,
}
```

**ADR: Cache Initialization**

The `resolve` command will use this configuration to determine the cache location, falling back to a standard user cache directory if not specified.

```rust
// Conceptual setup in the resolve command
use snix_castore::blob::fs::BlobService;
use snix_castore::directory::redb::DirectoryService;
use snix_store::path_info::redb::PathInfoService;
use snix_store::nar::SimpleRenderer;
use etcetera::BaseStrategy;

let cache_root = CONFIG.cache.and_then(|c| c.root_dir).map(PathBuf::from).unwrap_or_else(|| {
    let strategy = BaseStrategy::new().unwrap();
    strategy.cache_dir().join("eka/fetch_cache")
});

let blob_service = BlobService::new(&cache_root.join("blobs"))?;
let directory_service = DirectoryService::new(&cache_root.join("dirs.redb"))?;
let path_info_service = PathInfoService::new(&cache_root.join("path_info.redb"))?;
let nar_calculation_service = SimpleRenderer::new(blob_service.clone(), directory_service.clone());
```

#### 3. Use the `Fetcher`

With the persistent services in place, we will create an instance of the `Fetcher`.

```rust
// Conceptual setup in the resolve command
use snix_glue::fetchers::Fetcher;

let fetcher = Fetcher::new(
    blob_service,
    directory_service,
    path_info_service,
    nar_calculation_service,
);
```

#### 4. Integration with `lock.rs`

The logic in `Lockfile::synchronize` will remain the same as in the previous ADR version. It will construct the appropriate `Fetch` enum from the manifest's `PinReq` and call `fetcher.ingest_and_persist`. The `Fetcher` will now automatically handle caching: if the content has been fetched before, it will be served directly from the on-disk store, and the NAR hash will be recalculated from there, avoiding any network access.

```rust
// In crates/atom/src/lock.rs, inside Lockfile::synchronize
// (Assuming `fetcher` is passed in or available in the context)

crate::manifest::deps::Dependency::Pin(pin_req) => {
    let fetch_args = convert_pin_req_to_fetch_args(pin_req, k); // Helper function
    match fetcher.ingest_and_persist(k.as_str(), fetch_args).await {
        Ok((_store_path, _node, nix_hash, _metadata)) => {
            let resolved_dep = create_dep_from_hash(pin_req, k, nix_hash); // Helper
            self.deps.as_mut().insert(k.to_owned(), resolved_dep);
        }
        Err(e) => {
            tracing::warn!(message = "failed to resolve pin dependency", key = %k, error = ?e);
        }
    }
},
```

## Consequences

- **Pros:**
  - Provides a robust, persistent fetch cache for `eka`, improving performance and reducing network usage on subsequent resolutions.
  - Perfectly aligns with the long-term architectural separation of `eka` (frontend/cache management) and `eos` (backend/evaluation).
  - Maximizes reuse of the `snix` stack, from high-level orchestration down to the storage layer.
  - The content-addressed nature of the cache provides optimal storage efficiency.
- **Cons:**
  - `eka` now takes on the responsibility of managing an on-disk cache directory.
  - Introduces a dependency on `redb` (via `snix-castore`), a pure Rust embedded database. This is a reasonable trade-off for a persistent key-value store.
