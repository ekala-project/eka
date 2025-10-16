# Architectural Decision Record (ADR): Named Roots MVP

## Status

Accepted

## Context

The current system for identifying an `eka` repository relies solely on the cryptographic hash of its root commit. This creates two primary problems:

1.  **Identity Ambiguity**: When repositories are forked, both the original and the fork share the same identity, leading to potential conflicts.
2.  **Local Atom Discovery**: There is no formal, efficient mechanism for discovering all atoms within a local repository checkout without resorting to expensive and non-performant filesystem traversals.

This ADR defines a minimal, robust, and user-friendly mechanism to solve both problems by introducing a unique, human-readable name for a repository's package set, which is then used to create a static index of all atoms. This approach aligns with `eka`'s design philosophy of operating on static, declarative metadata rather than performing dynamic runtime discovery.

This design is intentionally scoped for a Minimum Viable Product (MVP) to provide this core functionality without introducing unnecessary complexity. It supersedes the proposals in `adrs/0002-eka-add-command.md`.

## Decision

The design is centered on three core principles: a single configuration file for defining the project, a namespaced Git ref for discovering the project's name, and a modification to the atom ID hashing algorithm to incorporate this name.

### 1. Configuration: `ekala.toml`

- A single configuration file, `ekala.toml`, will be located at the root of the repository.
- Its purpose is to define the project's identity and enumerate the packages it contains.
- The format for the MVP is as follows:

```toml
# ekala.toml

[project]
# A mandatory, unique, human-readable name for the repository's package set.
# This tag MUST NOT contain spaces or characters that are invalid in Git refs.
tag = "my-project-name"

# An optional list of paths to the `atom.toml` files for each atom
# defined within this project. Paths are relative to the `ekala.toml` file.
packages = [
  "path/to/atom-a",
  "path/to/atom-b",
]
```

- The parser for this file will be strict. Any keys or tables not specified in this MVP design (e.g., `[set]`, `domain`, `org`) will be rejected with a clear error message, ensuring forward compatibility.

### 2. Discovery: Namespaced Git Ref

- The project's unique name will be advertised in a namespaced Git ref to allow for fast, lightweight discovery without needing to clone or fetch the repository's contents.
- The format of this ref is: `refs/tags/ekala/root/v1/<project-tag>`
  - `<project-tag>` is the value of the `tag` key from the `[project]` table in `ekala.toml`.
- The `eka init` command will be responsible for creating and pushing this tag, and the `eka add` command will discover it to calculate atom ids.

### 3. Identity: Atom ID Hashing Modification

- The core of this design is the modification of the atom ID cryptographic hash computation.
- The `project-tag` will be incorporated as an additional context string into the hashing algorithm, alongside the existing root commit hash.
- This ensures that atoms from two different projects (e.g., an upstream and a fork) will have cryptographically unique IDs, even if their source code and root commit are identical. This resolves the fundamental identity conflict.

### 4. `eka add` Command Behavior and Conflict Resolution

The `eka add` command is designed to be simple for the common case, with a clear, explicit mechanism for handling the exceptional case of a name collision.

#### Default Behavior (No Conflict)

- The `eka add <uri>` command will perform the following steps:
  1.  Query the remote repository to discover its advertised refs. This operation is performed efficiently in pure Rust using the `gitoxide` library, requiring no external `git` binary.
  2.  Extract the `<project-tag>` from the ref.
  3.  Check the project's existing dependencies for a conflict with this tag.
  4.  If no conflict exists, proceed with dependency resolution, incorporating the fetched `<project-tag>` into the atom ID hash calculation.

#### Conflict Detection and Resolution

- A conflict occurs if a user tries to add a dependency with a `project-tag` that is already in use by a dependency from a different repository (i.e., a different root commit hash).
- In this case, the `eka add <uri>` command **must fail** with a clear, actionable error message.
- To resolve the conflict, the user must use the `--as <alias>` flag to provide a unique local name for the dependency:
  ````bash
  eka add --as my-forked-utils <uri>
  ```- This command will then succeed, using the provided alias as the local name for the dependency.
  ````

### 5. Manifest (`atom.toml`) Integration and Aliasing

- The `eka add` command will add the new dependency as a named set in the `[package.sets]` table of the `atom.toml` file, consistent with the format defined in ADR #7.
- The key in this table serves as the local name or alias for the dependency set.

```toml
# in atom.toml

[package.sets]
# Common case: The local name 'eka-utils' is the same as the canonical project tag.
eka-utils = "git://github.com/eka-proj/utils"

# Conflict case: The local alias 'my-utils' is used. The canonical project
# tag discovered from the remote is still 'eka-utils'.
my-utils = "git://github.com/my-fork/utils"
```

- The `eka` resolver will use the local name (`eka-utils` or `my-utils`) when parsing `[deps.from.*]` tables.
- The `atom.lock` file will store the canonical `project-tag` discovered from the remote, ensuring that the atom ID hash is always computed with the correct, unambiguous name. This provides a clear separation of concerns: the manifest uses a local alias for user convenience, while the lockfile uses the canonical name for cryptographic integrity.

## Consequences

**Pros**:

- **Unambiguous Identity**: Solves the forking problem by giving each repository a unique, user-defined name that is cryptographically tied to its atoms.
- **Minimalist and Clear**: The design is simple to understand, implement, and use, with a single configuration file and a clear purpose.
- **Efficient**: Identity discovery is extremely fast, requiring only a lightweight query of the remote's advertised refs.
- **Scalable**: While the MVP is minimal, the `ekala.toml` format and namespaced refs provide a clear and non-breaking path for future extensions like organizations and nested sets.

**Cons**:

- **Introduces a New File**: Projects will now require an `ekala.toml` file to be publishable.
- **Requires `eka publish` Modification**: The `eka publish` command will need to be updated to manage the new root tag.
