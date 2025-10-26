# 10. Electron Sources: Content-Addressed Build Dependencies

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-26

## Context and Problem Statement

Atoms provide two key benefits: **performance** and **decoupling**. They allow referencing specific points in a repository's history without carrying the entire repository's baggage (potentially multiple times). Atoms are purposefully self-contained by design, however, if atoms cannot reference source code and assets from their repository, they would be forced to fetch these sources separately when needed, defeating their core purpose. This would require pulling repository history and all its unrelated files again, negating the performance and decoupling benefits atoms were designed to provide.

Electrons fill this crucial gap by enabling atoms to reference repository sources in a minimal, content-addressed manner, preserving the essential motivational goals of atoms themselves. The current dependency system (ADR #7) only supports atom-to-atom references and direct Nix fetchers, leaving this critical gap unfilled.

This ADR builds on the manifest format outlined in ADR#7, and the repository identity concepts established in ADR #9.

## Decision

Introduce "electrons" - content-addressed sources that atoms can reference by name. Electrons are defined at the repository level in `ekala.toml` and referenced by atoms in context-scoped dependency declarations. Electrons are strictly scoped to the local repository (the set marked with `"::"`).

### Repository-Level Source Definitions (`ekala.toml`)

```toml
[build.sources]
src = "src"
docs = "docs"
assets = "assets/images"
config = "config/templates"
```

Source names serve as stable, unique identifiers for referencing electrons independent of their filesystem paths. While the true identity of an electron is its tree object ID (stored in the lockfile), names provide:

- **Unique identification**: TOML key uniqueness ensures no naming conflicts during local resolution
- **Filesystem decoupling**: Atoms reference sources by name, not path, allowing sources to be moved without affecting atom content hashes
- **Efficient resolution**: Names enable fast lookups in both local (`ekala.toml`) and remote (tree ID) contexts
- **Conflict avoidance**: Using path components directly could cause issues with directories sharing the same name

### Atom-Level Source Dependencies (`atom.toml`)

```toml
# ADR#7 spec
[package]
label = "my-atom"
version = "1.0.0"

[package.sets]
local = "::"  # Reference to the local repository; must exist to use electrons

[deps.from.local]
other-atom = "^1.0"

# ADR#10 proposed electron dependency declarations
[deps.for.build]
sources = ["src", "config"]

[deps.for.env]
sources = ["docs", "assets"]
```

### Lockfile Format

```toml
[[deps]]
type = "electron"
name = "src"
tree = "<tree-hash>"
rev = "<orphaned-commit-hash>"
context = "build"
```

## Key Design Principles

1. **Content-Addressed**: Like atoms, electrons are stored as reproducible orphaned commits containing their source tree, ensuring deduplication and integrity. Unlike atoms (which are versioned and resolved via semantic version constraints), electrons are purely content-addressed by their tree object hash.

2. **Repository-Scoped**: Electrons are only accessible within their defining repository (the local set with `"::"`), maintaining encapsulation and preventing cross-repository coupling.

3. **Context-Bound**: Sources are declared per evaluation context (package builds, development environments, etc) to ensure atoms only access sources appropriate to their use case. For example, build sources are only injected during package building, while env sources are only available during development (e.g., devshells). This prevents potential security issues and maintains clear separation of concerns.

4. **Anonymous**: Atoms reference electrons by stable names defined in `ekala.toml`, not filesystem paths, preserving content-addressing and decoupling from incidental file locations. Remotely electrons are totally anonymous, and resolved only by their content hash.

5. **Build-Time Deferred**: Electron fetching is always deferred to build time for efficiency, as sources represent build-time dependencies that don't need to be resolved during manifest evaluation.

## Git Storage

Electrons are stored under `refs/ekala/ca/<tree-id>`, where the ref name is content-addressed by the tree object ID. Each ref points to a reproducible orphaned commit containing the electron's source tree.

## Design Validation

The electron concept elegantly extends both physics and mathematical analogies:

- **Physics**: Just as atoms share electrons in chemistry, atoms in eka can share content-addressed sources. Electrons orbit atoms, providing the materials they need without being part of their core identity.

- **Mathematics**: Atoms represent unique elements in a set (guaranteed by their cryptographic IDs), while electrons represent content-addressed sources that can be shared between multiple atoms within the same repository scope without polluting the atom set.

## Consequences

**Positive:**

- Enables atoms to reference arbitrary repository sources without breaking filesystem abstraction
- Maintains clean separation between atoms and supporting sources
- Provides context-appropriate source access with clear scoping
- Leverages existing atom storage and resolution patterns
- Ensures build-time efficiency through deferred fetching
- Maintains benefits of atom's core model to retain decoupled, secure and efficient access to repo history.

**Negative:**

- Adds complexity to the manifest format
- Requires additional resolution logic for context enforcement
- Limits electron sharing to repository boundaries (by design)
