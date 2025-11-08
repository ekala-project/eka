# Architectural Decision Record (ADR): Repository Identity and Discovery

## Status

Proposed (WIP)

## Context

This ADR documents the architecture for `eka` repository identity, atom naming, and metadata discovery. It clarifies existing concepts, formalizes terminology, and introduces a new manifest to serve as the single source of truth for a repository's composition.

The core problems to be solved are:

1.  **Consistent Identity Model**: An atom's identity is a two-part system: a machine-verifiable component (the root commit hash) and a human-readable name (`label`). The repository identity model should follow a similar pattern. A formal mechanism is needed to establish repository identity that provides robust disambiguation of forks from mirrors.
2.  **Source of Truth**: A repository's composition is implicitly defined by the atoms present on the filesystem. A formal, declarative manifest is needed to act as the single, unambiguous source of truth.
3.  **Discovery Inefficiency**: A performant method is needed to discover all atoms within a local checkout without expensive filesystem traversals.
4.  **Terminology Ambiguity**: The historical use of `tag` for an atom's unique identifier is ambiguous when juxtaposed with the `tags` metadata list.

## Decision

The architecture is centered on a new root `ekala.toml` manifest as the single source of truth. It establishes repository identity through initialization commits with entropy injection, providing robust fork disambiguation and temporal anchoring.

### 1. The Source of Truth: `ekala.toml` (New)

A single `ekala.toml` file **must** exist at the root of the repository. Its primary purpose is to serve as the **single source of truth** for the repository's composition.

- **Function**: It provides a complete, static index of all `packages` (atoms) it contains. Repository identity is established through the initialization process rather than explicit naming. It also supports optional metadata for enhanced discoverability.
- **Format**:

  ```toml
  # ekala.toml

  # A flat list of all atoms in this repository, identified by their path.
  # The publisher will enforce that all atom names are unique within the repository.
  [set]
  packages = [
    "path/to/ui-kit/button",
    "path/to/core/validator",
  ]


  # Optional key-value metadata for structured filtering and queries
  [metadata]
  domain = "my-company.com"
  license = "MIT"
  # Optional tags for simple categorization
  tags = ["ui-kit", "experimental"]
  ```

### 2. The Atom and its Metadata: `atom.toml` (Terminology Change)

Each atom continues to be defined by an `atom.toml` file. This ADR formalizes a critical terminology change to resolve ambiguity.

- **Purpose**: It defines the atom's unique identifier (`label`) and other metadata.
- **Terminology Change**: The ambiguous term `tag` is **deprecated** for the atom's unique identifier. The correct, formal term is now `label`. This resolves the ambiguity between the singular identifier and the `tags` metadata list.
- **Format**:

  ```toml
  # atom.toml

  [package]
  # The unique identifier for this atom within the repository.
  label = "button"
  version = "1.0.0"


  # Optional key-value metadata for structured filtering and queries
  [metadata]
  license = "MIT"
  maintainer = "ui-team@company.com"
  # An optional list of arbitrary strings for logical grouping.
  # This is the foundation for metadata-driven collections.
  tags = ["ui", "interactive"]
  ```

### 3. Repository Identity (New)

Repository identity is established through an initialization commit with entropy injection, providing robust disambiguation and temporal anchoring. Unlike atoms (which are individual components that benefit from human-readable names), repositories are collections of components where temporal identity provides clearer provenance tracking.

**Note**: This initialization commit mechanism is outlined here but will be implemented post-MVP to avoid delaying the core functionality.

- **Initialization Process**: When `eka init` is run, a special initialization commit is created. This commit includes injected entropy (random data for cryptographic strength) in its header along with a unique "ekala" identifier. Git commits are snapshots of repository state with metadata; headers contain additional information like author details.
- **Identity Components**: Repository identity is defined by this initialization commit, which implicitly includes the repository's complete history (including the original root commit) through Git's ancestry system. Git maintains a chain of commits where each commit references its parent(s), forming a tree structure that links the initialization point to the repository's entire development timeline.
- **Temporal Anchoring**: The init commit establishes a clear point in history when the repository was explicitly configured for Ekala, preventing publication of atoms created before this point and enabling precise analysis of when forks occurred. This creates a temporal boundary that distinguishes "before Ekala" from "after Ekala" in the repository's history.
- **Fork Tracking**: The unique "ekala" identifier in init commit headers allows tracking of repository reinitializations and fork points by marking commits that represent new identity establishments, providing a historical record of when repositories established independent identities.

### 4. Atom Identity (Formalized)

An atom's identity is a cryptographic hash. This ADR formalizes its components.

- **Hashing Components**: The ID is derived from two components:
  1.  The repository's **init commit hash** (which implicitly encodes the entire repository history including the root commit through Git's parent chain system).
  2.  The atom's `label` (as defined in its `atom.toml`).
- **Fork Disambiguation**: The init commit identity ensures that repositories with different initialization histories produce unique atom IDs, even if they share the same root of history.

### 5. Git Refspec Architecture

To support this architecture, a unified and consistent Git refspec is required. All `ekala`-specific refs will live under the `refs/ekala/` namespace.

- **Repository Identity**: Repository identity is established through a single ref that points to the latest initialization commit, leveraging Git's Merkle tree structure (where each commit contains references to its parent commits).
  - **Format**: `refs/ekala/init`
  - **Content**: Points to the entropy-injected initialization commit hash. The root commit is implicitly encoded through the commit's ancestry chain, eliminating the need for a separate root ref.

- **Atom Content**: The primary ref for an atom points directly to its content. This path is optimized for the most common operation.
  - **Format**: `refs/ekala/atoms/<atom-label>/<version>`

- **Atom Metadata**: Secondary information about an atom is stored in parallel hierarchies, correlated by the shared `<atom-label>/<version>` path.
  - **Manifest**: `refs/ekala/manifests/<atom-label>/<version>`
  - **Origin**: `refs/ekala/origins/<atom-label>/<version>`

### 7. Lifecycle Management: Repository Evolution

Repository identity evolution is handled through the immutable initialization commit system, eliminating the need for the complex deprecation mechanisms in the previous draft.

- **Mechanism**: Since repository identity is tied to immutable Git commits rather than mutable labels, identity changes require explicit reinitialization. This provides clean slate evolution without legacy baggage.
- **Publisher Behavior**: The `eka init` command creates an initialization commit with the ekala.toml manifest and publishes the `refs/ekala/init` ref pointing to it, establishing the repository's identity.
- **Resolver Behavior**: Resolvers verify atom authenticity by checking that the atom's identity components match the published repository's initialization commit hash.

### 6. Alternatives Considered

#### User-Managed Repository Labels

A system of user-defined repository labels (similar to atom labels) was considered as an alternative to initialization commits. This would involve adding a `label` field to `ekala.toml` and incorporating it into repository identity calculations.

**Why Rejected:**

- **Collection vs Component**: Atoms are individual components that benefit from human-readable names for coordination. Repositories are collections of components where temporal identity provides clearer provenance tracking and avoids naming conflicts in a decentralized system.
- **Maintenance Complexity**: User-managed labels require deprecation mechanisms for renames, adding complexity that temporal identity avoids through immutable Git commits.
- **Coordination Overhead**: Labels create social coordination challenges (name conflicts, ownership disputes) that temporal identity sidesteps by using cryptographic time-based identity instead of human names.
- **Decentralized Constraints**: Without central registries, label-based coordination becomes impractical at scale, while temporal identity works naturally in distributed environments.

#### Logical Grouping: Tags over Formal Sets

A rigid, filesystem-based `set` hierarchy was considered as a mechanism for grouping atoms. This approach was **rejected** because it conflates **physical layout** with **logical grouping**, is inflexible (an atom can only belong to one set), and does not work across repository boundaries.

The chosen `tags` system is superior because it is a **metadata-driven** approach that is infinitely flexible (an atom can have many tags), is decoupled from the filesystem, and provides the foundation for powerful, cross-repository query operations in the future.

## Consequences

**Pros**:

- **Robust Identity**: The initialization commit system provides mathematically strong provenance with temporal anchoring, enabling precise fork analysis and preventing publication of atoms created before explicit Ekala initialization.
- **Simplified Evolution**: Repository identity changes require clean reinitialization rather than complex deprecation management, providing clearer lifecycle semantics.
- **Cryptographic Strength**: Entropy injection in init commits ensures collision resistance while leveraging Git's immutability for free.
- **Clear Terminology**: Deprecating `tag` in favor of `label` for the unique identifier resolves a major point of confusion.
- **Performant Discovery**: Both local discovery (reading the root `ekala.toml`) and remote discovery (querying for manifest refs) are extremely fast and avoid filesystem traversals or repository clones.
- **Clear and Formalized**: The architecture is now based on a clear set of rules, with a single source of truth and precise terminology.
- **Flexible Grouping**: The metadata-driven `tags` system allows for flexible, multi-faceted grouping of atoms, which is not possible with a rigid, filesystem-based hierarchy.
- **Rich Metadata**: The dual tagging system (tags + key-value metadata) enables both simple categorization and structured queries, supporting advanced decentralized discovery through systems like Eos.

**Cons**:

- **Requires Root `ekala.toml`**: All `eka` repositories must now have an `ekala.toml` file at their root. This is a necessary trade-off for the benefits of a stable anchor.
