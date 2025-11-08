# 8. Lockfile Data Model and Synchronization Refactor

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-13

## Context and Problem Statement

The initial implementation of the ADR#7 manifest and lockfile format has revealed several areas of unnecessary complexity and inefficiency in the underlying data models. The primary issues are:

1.  **Redundant Identity Types:** The `lock.rs` module defines an `AtomDigest` struct to represent an atom's cryptographic ID, which is conceptually redundant with the `IdHash` struct defined in the canonical `id` module. This creates a confusing and error-prone translation layer.
2.  **Inefficient In-Memory Lockfile Structure:** The lockfile is currently deserialized into a `BTreeSet`, which requires inefficient linear scans for lookups and updates. This data structure will not scale and makes implementing future features, like transitive dependency resolution, overly complex.
3.  **Complex Synchronization Logic:** The existing synchronization logic is difficult to implement and maintain due to the inefficient data structures and redundant types.

This ADR proposes a refactoring of the core data models to address these issues, creating a more elegant, efficient, and robust system for managing lockfiles.

## Decision

We will refactor the core identity types and the in-memory representation of the lockfile. The implementation will be conducted in three phases.

### Phase 1: Refactor Core Identity Types

The goal of this phase is to create a single, canonical representation for an atom's cryptographic ID.

- **Step 1.1:** In `crates/atom/src/id/mod.rs`, the ephemeral `IdHash` struct will be renamed to `IdHashView` to clarify its purpose as a temporary, referenced view of a hash.
- **Step 1.2:** A new public, storable struct, `pub struct Id([u8; 32]);`, will be created in `crates/atom/src/id/mod.rs`. This will become the canonical, owned type for an atom's cryptographic ID.
- **Step 1.3:** The `AtomId::compute_hash()` method will be modified to return this new `id::Id` struct.
- **Step 1.4:** The redundant `AtomDigest` struct will be completely removed from `crates/atom/src/lock.rs`.
- **Step 1.5:** The `AtomDep` struct in `crates/atom/src/lock.rs` will be updated to use the new `id::Id` for its `id` field.

### Phase 2: Redesign In-Memory Lockfile Structure

This phase will optimize the in-memory data structure for the lockfile for efficient lookups.

- **Step 2.1:** The `Lockfile` struct in `crates/atom/src/lock.rs` will be modified. The `DepMap` (a `BTreeSet`) will be replaced with separate `BTreeMap` collections for each major dependency type.
- **Step 2.2:** The primary map will be `atoms: BTreeMap<id::Id, AtomDep>`, providing efficient O(log n) lookups. Similar maps will be added for Nix dependencies, keyed by their `Name`.
- **Step 2.3:** Custom `Serialize` and `Deserialize` implementations for the `Lockfile` struct will be created to handle the conversion between the efficient in-memory `BTreeMap` representation and the readable on-disk flat list of `[[input]]` tables.

### Phase 3: Implement the New Synchronization Algorithm

With the new data models in place, we will implement a clear and robust synchronization algorithm.

- **Step 3.1:** Implement the loading logic to parse `atom.toml` and deserialize `atom.lock` into the new `BTreeMap`-based `Lockfile` structure.
- **Step 3.2:** Implement the reconciliation logic. This will iterate through manifest dependencies, compute the expected `id::Id` for each, and use the `BTreeMap` for efficient lookups to add, update, or verify entries against the loaded lockfile.
- **Step 3.3:** Implement pruning logic to remove any stale entries from the lockfile that are no longer present in the manifest.
- **Step 3.4:** Implement the final write logic that serializes the in-memory `Lockfile` back to the `atom.lock` on-disk format.

## Consequences

- **Positive:**
  - **Reduced Complexity:** Eliminates redundant types and simplifies the conceptual model.
  - **Improved Performance:** The `BTreeMap` structure provides efficient lookups, making the system more scalable.
  - **Increased Robustness:** A clearer data model and algorithm will be less prone to bugs and easier to maintain.
  - **Future-Proofing:** Provides a solid foundation for future features like transitive dependency resolution.

- **Negative:**
  - **Implementation Effort:** This is a significant refactoring that will require careful implementation and testing.
