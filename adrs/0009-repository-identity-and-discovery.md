# Architectural Decision Record (ADR): Repository Identity and Discovery

## Status

Accepted

## Context

This ADR documents the architecture for `eka` repository identity, atom naming, and metadata discovery. It clarifies existing concepts, formalizes terminology, and introduces a new manifest to serve as the single source of truth for a repository's composition.

The core problems to be solved are:

1.  **Consistent Identity Model**: An atom's identity is a two-part system: a machine-verifiable component (the root commit hash) and a human-readable name (`label`). The repository identity model should follow this same successful pattern. A formal mechanism is needed to add a user-definable naming component to the repository's identity. A primary consequence of this is the robust disambiguation of forks from mirrors.
2.  **Source of Truth**: A repository's composition is implicitly defined by the atoms present on the filesystem. A formal, declarative manifest is needed to act as the single, unambiguous source of truth.
3.  **Discovery Inefficiency**: A performant method is needed to discover all atoms within a local checkout without expensive filesystem traversals.
4.  **Terminology Ambiguity**: The historical use of `tag` for an atom's unique identifier is ambiguous when juxtaposed with the `tags` metadata list.
5.  **Remote Discovery**: The purpose of the existing "Manifest Ref" must be clarified as the primary mechanism for all remote metadata discovery.

## Decision

The architecture is centered on a new root `ekala.toml` manifest as the single source of truth. It also formalizes terminology and clarifies the role of the "Manifest Ref" for remote discovery.

### 1. The Source of Truth: `ekala.toml` (New)

A single `ekala.toml` file **must** exist at the root of the repository. Its primary purpose is to serve as the **single source of truth** for the repository's composition.

- **Function**: It defines the repository's canonical `label` for fork disambiguation and provides a complete, static index of all `packages` (atoms) it contains.
- **Format**:

  ```toml
  # ekala.toml

  [project]
  # The canonical, human-readable name for this repository.
  # This is mixed into the atom ID hash to disambiguate forks.
  label = "my-project"

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

- **Purpose**: It defines the atom's unique identifier (`label`) and other metadata.
- **Terminology Change**: The ambiguous term `tag` is **deprecated** for the atom's unique identifier. The correct, formal term is now `label`. This resolves the ambiguity between the singular identifier and the `tags` metadata list.
- **Format**:

  ```toml
  # atom.toml

  [package]
  # The unique identifier for this atom within the repository.
  label = "button"
  version = "1.0.0"

  # An optional list of arbitrary strings for logical grouping.
  # This is the foundation for metadata-driven collections.
  tags = ["ui", "interactive"]
  ```

### 3. Atom Identity (Formalized)

An atom's identity is a cryptographic hash. This ADR formalizes its components.

- **Hashing Components**: The ID is derived from two components:
  1.  The repository's **root commit hash**.
  2.  The atom's `label` (as defined in its `atom.toml`).
- **Fork Disambiguation**: The `project.label` from the root `ekala.toml` is incorporated into the hashing process, ensuring that forks with identical roots produce unique atom IDs.

### 4. Git Refspec Architecture

To support this architecture, a unified and consistent Git refspec is required. All `ekala`-specific refs will live under the `refs/ekala/` namespace.

- **Repository Identity**: The repository's canonical name is advertised in a single, top-level ref.

  - **Format**: `refs/ekala/project/<project-label>`
  - **Content**: This ref points to the repository's root commit hash.

- **Atom Content**: The primary ref for an atom points directly to its content. This path is optimized for the most common operation.

  - **Format**: `refs/ekala/atoms/<atom-label>/<version>`

- **Atom Metadata**: Secondary information about an atom is stored in parallel hierarchies, correlated by the shared `<atom-label>/<version>` path.
  - **Manifest**: `refs/ekala/manifests/<atom-label>/<version>`
  - **Origin**: `refs/ekala/origins/<atom-label>/<version>`

### 5. Lifecycle Management: Project Renames

A project rename is a critical lifecycle event that must be handled gracefully. This is managed in the manifest, which is the single source of truth.

- **Mechanism**: The `ekala.toml` manifest is extended with an optional `deprecated.labels` field.
  ```toml
  [project]
  label = "new-project-name"
  deprecated.labels = ["old-project-name"]
  ```
- **Publisher Behavior**: The `eka publish` command will publish the primary ref (`refs/ekala/project/new-project-name`) and a special deprecation ref (`refs/ekala/deprecated/old-project-name`).
- **Resolver Behavior**: When resolving a dependency on an old name, the resolver will discover the deprecation ref, follow it to the new name, and emit a warning to the user, ensuring a non-breaking upgrade path.

### 5. Logical Grouping: Tags over Formal Sets

A rigid, filesystem-based `set` hierarchy was considered as a mechanism for grouping atoms. This approach was **rejected** because it conflates **physical layout** with **logical grouping**, is inflexible (an atom can only belong to one set), and does not work across repository boundaries.

The chosen `tags` system is superior because it is a **metadata-driven** approach that is infinitely flexible (an atom can have many tags), is decoupled from the filesystem, and provides the foundation for powerful, cross-repository query operations in the future.

## Consequences

**Pros**:

- **Unambiguous Identity**: The `project.label` in the root `ekala.toml`, combined with the root commit hash and atom `label`, provides a robust, fork-safe identity for all atoms.
- **Clear Terminology**: Deprecating `tag` in favor of `label` for the unique identifier resolves a major point of confusion.
- **Performant Discovery**: Both local discovery (reading the root `ekala.toml`) and remote discovery (querying for manifest refs) are extremely fast and avoid filesystem traversals or repository clones.
- **Clear and Formalized**: The architecture is now based on a clear set of rules, with a single source of truth and precise terminology.
- **Flexible Grouping**: The metadata-driven `tags` system allows for flexible, multi-faceted grouping of atoms, which is not possible with a rigid, filesystem-based hierarchy.

**Cons**:

- **Requires Root `ekala.toml`**: All `eka` repositories must now have an `ekala.toml` file at their root. This is a necessary trade-off for the benefits of a stable anchor.
