# 7. Atom Manifest Dependency Format

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-09

## Context and Problem Statement

The `eka` project has reached a stage where a stable, well-defined dependency format for the `atom.toml` manifest is required. This document proposes the first official, stable format for the dependency sections of the manifest, designed to be robust, intuitive, and philosophically aligned with `eka`'s decentralized, backend-agnostic vision.

This ADR is intentionally scoped to the declaration of dependencies. Other manifest concerns, such as build configuration, will be addressed in future ADRs.

The key design goals for the dependency format are:

1.  **Clarity and Explicitness:** The format must be declarative, self-describing, and easy to analyze statically.
2.  **Embrace Decentralization:** The format must support source mirrors and make the origin of dependencies obvious, naturally handling potential name conflicts.
3.  **Distinguish Buildable Atoms from other Dependencies:** The format must provide a primary interface for buildable atoms (i.e. atoms that are intended to produce an artifact), while also offering a clear, namespaced mechanism for integrating with backend-specific dependencies (e.g., traditional Nix fetchers).

## Core Concepts: Atom Identity

To understand the manifest, it's essential to understand how an atom is identified. An atom's identity is a combination of its content and its location, designed to be cryptographically unique and verifiable.

- **Package Set:** A collection of atoms, typically stored in a Git repository. A key principle is that all atoms within a set must have a unique `tag`. See the "Atom Protocol" section of the [README](https://github.com/ekala-project/eka?tab=readme-ov-file#what-is-the-atom-protocol) for more details.
- **Repository Root Hash:** The very first commit in a Git repository's history. This serves as a unique, immutable identifier for that repository, even across different mirrors.
- **Atom Tag:** A user-defined, unique identifier for a package within a given set.
- **Semantic Version:** All atoms must have a semantic version, which is critical for reliable dependency resolution.
- **Cryptographic ID:** In the lock file, an atom's final, globally unique ID is a cryptographic hash derived from its `tag` and the `Repository Root Hash` of its set. This ensures that an atom with the tag `foo` from one set can never be confused with an atom named `foo` from another.

## Decision

We will adopt a new manifest format that cleanly separates atom dependencies, from platform-specific ones, using a clear and conventional naming scheme.

### The `atom.toml` Manifest Format

#### `[package]` Table

Defines metadata about the current project. Its `[package.sets]` sub-table defines the available sources for atom dependencies. A set can be a single URL, a list of mirror URLs, or the special value `"::"` for the local repository.

```toml
[package]
tag = "my-project"
version = "0.1.0"

[package.sets]
# A single remote source.
company-atoms = "git@github.com:our-company/atoms"
# A remote source with mirrors. The resolver will try them in order.
public-registry = [ "https://registry-a.com/atoms", "https://registry-b.com/atoms" ]
# A source for atoms within this same repository
# A source for atoms within this same repository. The `::` syntax enables a
# "local mirror," which is critical for an efficient development workflow in a
# repository containing multiple, interdependent atoms. It allows the resolver
# to find local atoms without requiring a `git push` after every change,
# avoiding a disruptive publish-test cycle. The remote URL is provided as a
# fallback for environments like CI where the local repository context may not
# be available. The precise mechanism for local resolution (e.g., a root index
# vs. a file walk) is a CLI implementation detail.
local-atoms = [ "::", "https://github.com/our-company/more-atoms" ]
```

#### `[deps]` Table

The unified top-level table for all dependencies.

##### `[deps.from.<set-name>]` Tables

The primary mechanism for declaring atom dependencies. The presence of an `atom.toml` implies that an atom is buildable, and this section defines its dependencies. They are always simple key-value pairs of `<atom-tag> = "<version-constraint>"`.

```toml
[deps.from.company-atoms]
auth-service = "^1.5"
```

##### `[deps.direct.nix]` Table

This table serves as a compatibility layer for backend-specific dependencies that are not in the `atom` format. It provides a "direct" interface to the underlying Nix fetchers.

- **Eval-Time Fetches:** Use keys like `url`, `git`, or `tar`.
- **Build-Time Fetches:** Use the `build` key.

### Comprehensive Example (`atom.toml`)

```toml
[package]
tag = "my-server"
version = "0.2.0"

[package.sets]
company = [ "git@github.com:our-company/atoms", "https://github.com/our-companies-mirror/atoms" ]
local = "::"

# =============================================================================
# ATOM DEPENDENCIES
# =============================================================================

[deps.from.company]
auth-service = "^1.5"

[deps.from.local]
local-utility = "^0.1"


# =============================================================================
# DIRECT NIX DEPENDENCIES
# =============================================================================

[deps.direct.nix]
# --- Eval-Time Direct Dependencies ---

nix-installer.url = "https://nixos.org/nix/install"
# A Git dependency with a static ref.
nixpkgs = { git = "https://github.com/NixOS/nixpkgs", ref = "nixos-unstable" }
# A Git dependency with a dynamic version constraint. This provides a
# convenient way to track a dependency without needing to manually update a
# static ref. The resolution process is highly optimized:
# 1. It queries the git remote using server-side filters (leveraging regex
#    character classes) to fetch only tags that resemble a semantic version.
# 2. The client then filters these results using the official semver.org regex.
# 3. Finally, the `semver` crate identifies the highest matching version that
#    satisfies the constraint.
# This process is efficient, even on large repositories.
other-repo = { git = "https://github.com/other/repo", version = "^1.2" }
# A dynamic tarball dependency whose URL is interpolated from a resolved atom
# version. This feature is critical for maintaining a single source of truth
# for version numbers, preventing drift between an atom's declared version and
# the version of the source archive it packages (e.g., a zlib atom packaging
# a zlib source tarball).
#
# The resolution logic is strictly defined to prevent ambiguity:
# 1. All `[deps.from.*]` atom dependencies are resolved first.
# 2. The versions of these resolved atoms can then be interpolated into the
#    fields of `[deps.direct.nix]` dependencies.
# This one-way flow makes circular dependencies impossible.
auth-service-docs = { tar = "https://docs.our-company.com/auth/{version}/docs.tar.gz", version = "from.company.auth-service" }

# --- Build-Time Direct Dependencies ---

# derives it's version variable directly from this local atom, e.g. to get a build source for an atom that defines a build recipe
source-archive = { build = "https://dist.our-company.com/my-server/{version}/source.tar.gz", version = "from.local.my-server" }

# An executable single file fetched at build time
online-builder = { build = "https://example.com/builder.sh", exec = true }

# A build-time archive that should not be unpacked by the fetcher.
# `unpack` is false by default if the extension does not indicate a tar file.
data-archive = { build = "https://example.com/data.tar.gz", unpack = false }
```

### The `atom.lock` Lock File Format

The lock file contains a `[sets]` table and an array of `[[input]]` tables.

1.  **`[sets]` Table:** Maps the Repository Root Hash of a package set to its location(s).
2.  **`[[input]]` Array:** A unified list of all fully resolved dependencies.

### Comprehensive Example (`atom.lock`)

```toml
[sets]
"<hash_of_company_root>" = [ "git@github.com:our-company/atoms", "https://github.com/our-companies-mirror/atoms" ]
"<hash_of_local_root>" = "::"

[[input]]
type = "atom"
tag = "auth-service"
version = "1.5.2"
set = "<hash_of_company_root>"
rev = "<git_rev_of_atom>"
id = "<blake3_hash_of_tag_and_set_hash>"

[[input]]
type = "atom"
tag = "local-utility"
version = "0.1.3"
set = "<hash_of_local_root>"
rev = "<git_rev_of_atom>"
id = "<blake3_hash_of_tag_and_set_hash>"

[[input]]
type = "nix+url"
name = "nix-installer"
url = "https://nixos.org/nix/install"
hash = "sha256-..."

[[input]]
type = "nix+git"
name = "nixpkgs"
url = "https://github.com/NixOS/nixpkgs"
rev = "aa0ebc256a5b0540e9df53c64ef6930471c98407"

[[input]]
type = "nix+git"
name = "other-repo"
url = "https://github.com/other/repo"
rev = "<git_rev_of_highest_matching_tag>"

[[input]]
type = "nix+tar"
name = "auth-service-docs"
url = "https://docs.our-company.com/auth/1.5.2/docs.tar.gz"
hash = "sha256-..."

[[input]]
type = "nix+build"
name = "source-archive"
url = "https://dist.our-company.com/my-server/0.2.0/source.tar.gz"
hash = "sha256-..."

[[input]]
type = "nix+build"
name = "online-builder"
url = "https://example.com/builder.sh"
hash = "sha256-..."
exec = true

[[input]]
type = "nix+build"
name = "data-archive"
url = "https://example.com/data.tar.gz"
hash = "sha256-..."
unpack = false
```

## Consequences

- **Positive:**

  - **Future-Proof:** The clear separation between atom dependencies and `direct` dependencies creates a path for supporting other backends.
  - **Robust Decentralization:** Set mirroring and content-addressed locking make the format resilient.
  - **Explicit and Consistent:** The format is declarative, consistent, and easy to parse.

- **Negative:**
  - **Breaking Change:** This format is not compatible with the previous placeholder format.
  - **Implementation Effort:** The parser and resolver logic will need to be written to support this new, stable structure.
