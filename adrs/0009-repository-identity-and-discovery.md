# Architectural Decision Record (ADR): Repository Identity and Discovery

## Status

Accepted

## Context

This ADR documents the architecture for `eka` repository identity, atom naming, and metadata discovery. It clarifies existing concepts, formalizes terminology, and introduces a new manifest to serve as the single source of truth for a repository's composition.

The core problems to be solved are:

1.  **Consistent Identity Model**: An atom's identity is a two-part system: a machine-verifiable component (the root commit hash) and a human-readable name (`name`). The repository identity model should follow this same successful pattern. A formal mechanism is needed to add a user-definable naming component to the repository's identity. A primary consequence of this is the robust disambiguation of forks from mirrors.
2.  **Source of Truth**: A repository's composition is implicitly defined by the atoms present on the filesystem. A formal, declarative manifest is needed to act as the single, unambiguous source of truth.
3.  **Discovery Inefficiency**: A performant method is needed to discover all atoms within a local checkout without expensive filesystem traversals.
4.  **Terminology Ambiguity**: The historical use of `tag` for an atom's unique identifier is ambiguous when juxtaposed with the `tags` metadata list.
5.  **Remote Discovery**: The purpose of the existing "Manifest Ref" must be clarified as the primary mechanism for all remote metadata discovery.

## Decision

The architecture is centered on a new root `ekala.toml` manifest as the single source of truth. It also formalizes terminology and clarifies the role of the "Manifest Ref" for remote discovery.

### 1. The Source of Truth: `ekala.toml` (New)

A single `ekala.toml` file **must** exist at the root of the repository. Its primary purpose is to serve as the **single source of truth** for the repository's composition.

- **Function**: It defines the repository's canonical `name` for fork disambiguation and provides a complete, static index of all `packages` (atoms) it contains.
- **Format**:

  ```toml
  # ekala.toml

  [project]
  # The canonical, human-readable name for this repository.
  # This is mixed into the atom ID hash to disambiguate forks.
  name = "my-project"

  # An optional list of tags for logically grouping entire repositories.
  tags = ["ui-kit", "experimental"]

  # A flat list of all atoms in this repository, identified by their path.
  # The publisher will enforce that all atom names are unique within the repository.
  packages = [
    "path/to/ui-kit/button",
    "path/to/core/validator",
  ]
  ```

### 2. The Atom and its Metadata: `atom.toml` (Terminology Change)

Each atom continues to be defined by an `atom.toml` file. This ADR formalizes a critical terminology change to resolve ambiguity.

- **Purpose**: It defines the atom's unique identifier (`name`) and other metadata.
- **Terminology Change**: The ambiguous term `tag` is **deprecated** for the atom's unique identifier. The correct, formal term is now `name`. This resolves the ambiguity between the singular identifier and the `tags` metadata list.
- **Format**:

  ```toml
  # atom.toml

  [package]
  # The unique identifier for this atom within the repository.
  name = "button"
  version = "1.0.0"

  # An optional list of arbitrary strings for logical grouping.
  # This is the foundation for metadata-driven collections.
  tags = ["ui", "interactive"]
  ```

### 3. Atom Identity (Formalized)

An atom's identity is a cryptographic hash. This ADR formalizes its components.

- **Hashing Components**: The ID is derived from two components:
  1.  The repository's **root commit hash**.
  2.  The atom's `name` (as defined in its `atom.toml`).
- **Fork Disambiguation**: The `project.name` from the root `ekala.toml` is incorporated into the hashing process, ensuring that forks with identical roots produce unique atom IDs.

### 4. Remote Discovery: The "Manifest Ref" (Clarified)

To enable efficient remote discovery, the purpose of an **existing** special Git ref is clarified and expanded. This is not a new mechanism, but its role is now formalized.

- **Purpose**: It is the general-purpose, efficient mechanism for all remote metadata discovery, making the entire `atom.toml` file for a specific version discoverable in a single, lightweight remote query.
- **Ref Format**: `refs/eka/meta/<atom-name>/<version>`
- **Ref Content**: The Git object pointed to by this ref is a blob containing the raw text of the corresponding `atom.toml` file.
- **Mechanism**: `eka` can perform a single query to fetch all refs under `refs/eka/meta/`, providing a complete picture of all atoms and versions without cloning the repository.

### 5. Logical Grouping: Tags over Formal Sets

A rigid, filesystem-based `set` hierarchy was considered as a mechanism for grouping atoms. This approach was **rejected** because it conflates **physical layout** with **logical grouping**, is inflexible (an atom can only belong to one set), and does not work across repository boundaries.

The chosen `tags` system is superior because it is a **metadata-driven** approach that is infinitely flexible (an atom can have many tags), is decoupled from the filesystem, and provides the foundation for powerful, cross-repository query operations in the future.

## Consequences

**Pros**:

- **Unambiguous Identity**: The `project.name` in the root `ekala.toml`, combined with the root commit hash and atom `name`, provides a robust, fork-safe identity for all atoms.
- **Clear Terminology**: Deprecating `tag` in favor of `name` for the unique identifier resolves a major point of confusion.
- **Performant Discovery**: Both local discovery (reading the root `ekala.toml`) and remote discovery (querying for manifest refs) are extremely fast and avoid filesystem traversals or repository clones.
- **Clear and Formalized**: The architecture is now based on a clear set of rules, with a single source of truth and precise terminology.
- **Flexible Grouping**: The metadata-driven `tags` system allows for flexible, multi-faceted grouping of atoms, which is not possible with a rigid, filesystem-based hierarchy.

**Cons**:

- **Requires Root `ekala.toml`**: All `eka` repositories must now have an `ekala.toml` file at their root. This is a necessary trade-off for the benefits of a stable anchor.
