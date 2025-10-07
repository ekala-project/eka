# ADR: Internal, Unified HTTP Caching for the `snix-glue` Fetcher

**Status:** Implemented

**Context:**

The `snix-glue` `Fetcher` is responsible for retrieving URL-based dependencies. Currently, it makes unconditional `GET` requests, which can lead to unnecessary network traffic and slow performance when the remote content has not changed. To improve efficiency, the `Fetcher` needs a robust HTTP caching mechanism.

Previous proposals involved either a separate cache directory or modifying the `Fetcher`'s public API. A superior approach is to make caching an internal, opportunistic feature of the `Fetcher` that leverages the existing storage backend, requiring no API changes.

**Decision:**

We will integrate a standards-compliant HTTP caching layer into the `snix-glue` `Fetcher` using the `http-cache-reqwest` middleware. Caching is configured explicitly by providing a cache path to the `Fetcher::new` function.

The implementation introduces a `BlobCacheManager` that stores cache metadata in a `redb` key-value database and response bodies in the existing `BlobService`. This design decouples the cache from any specific storage backend, allowing it to work with any `BlobService` implementation while keeping cached data alongside other content-addressed artifacts.

**Detailed Design:**

1.  **`BlobCacheManager` Implementation in `snix-castore`:**

    A new `BlobCacheManager` is implemented in `snix-castore`. It acts as the cache backend for the `http-cache-reqwest` middleware.

    - **Storage:** It uses a `redb` database to store a mapping from cache keys (URLs) to the `B3Digest` of the cached response body. The response bodies themselves are stored in the `BlobService`.
    - **Serialization:** The `HttpResponse` and `CachePolicy` are serialized together into the blob store using `bincode`.

    ```rust
    // In snix/castore/src/blobservice/mod.rs
    pub struct BlobCacheManager<BS> {
        blobs: BS,
        kv: Database,
    }

    #[async_trait]
    impl<BS: BlobService + 'static> CacheManager for BlobCacheManager<BS> {
        async fn get(&self, cache_key: &str) -> CacheResult<Option<(HttpResponse, CachePolicy)>> {
            // ... implementation ...
        }

        async fn put(
            &self,
            cache_key: String,
            response: HttpResponse,
            policy: CachePolicy,
        ) -> CacheResult<HttpResponse> {
            // ... implementation ...
        }

        async fn delete(&self, cache_key: &str) -> CacheResult<()> {
            // ... implementation ...
        }
    }
    ```

2.  **`Fetcher` Modification in `snix-glue`:**

    The `Fetcher` is updated to use a `reqwest_middleware::ClientWithMiddleware` and its constructor is modified to accept an optional cache path.

    - **`Fetcher::new`:** The constructor now takes an optional `cache_path: Option<PathBuf>`. If a path is provided, it initializes the `BlobCacheManager` and configures the `http-cache-reqwest` middleware. If not, it falls back to a non-caching client.
    - **Error Handling:** The `FetcherError` enum is extended to include errors from the middleware.

    ```rust
    // In snix/glue/src/fetchers/mod.rs
    pub struct Fetcher<BS, DS, PS, NS> {
        http_client: ClientWithMiddleware,
        // ... other fields
    }

    impl<BS: BlobService + Clone + 'static, DS, PS, NS> Fetcher<BS, DS, PS, NS> {
        pub fn new(
            blob_service: BS,
            // ... other args
            cache_path: Option<PathBuf>,
        ) -> Self {
            let client = reqwest::Client::builder() /* ... */ .build().unwrap();
            let builder = if let Some(path) = cache_path {
                if let Ok(manager) = BlobCacheManager::new(blob_service.clone(), path) {
                    ClientBuilder::new(client).with(Cache(HttpCache {
                        mode: CacheMode::Default,
                        manager,
                        options: HttpCacheOptions::default(),
                    }))
                } else {
                    ClientBuilder::new(client)
                }
            } else {
                ClientBuilder::new(client)
            };
            Self {
                http_client: builder.build(),
                // ...
            }
        }
    }
    ```

**Consequences:**

- **Pros:**

  - **Explicit Configuration:** The caching behavior is made explicit through the `cache_path` parameter, improving clarity.
  - **Unified Storage:** The cache leverages the existing `BlobService`, storing cached HTTP responses alongside other content-addressed data. This simplifies storage management and allows for potential deduplication.
  - **Backend Agnostic:** The solution is not tied to a specific `BlobService` implementation (like `ObjectStoreBlobService`) and works with any compatible backend.

- **Cons:**
  - **API Change:** The `Fetcher::new` signature was modified to accept the `cache_path`, which is a deviation from the original "zero API change" goal.
  - **Added Dependencies:** The implementation introduces new dependencies like `redb`, `http-cache-reqwest`, and `bincode`.
