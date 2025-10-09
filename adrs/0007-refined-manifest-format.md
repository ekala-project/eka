# 7. A Declarative, Source-Grouped Manifest Format

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-09

## Context and Problem Statement

The `eka` project has reached a stage where a stable, well-defined manifest format (`atom.toml`) is required. This document proposes the first official, stable format, designed to be robust, intuitive, and philosophically aligned with `eka`'s decentralized, backend-agnostic vision.

The key design goals are:

1.  **Clarity and Explicitness:** The format must be declarative and easy to analyze statically.
2.  **Embrace Decentralization:** The format must support source mirrors and make the origin of dependencies obvious, naturally handling potential name conflicts.
3.  **Distinguish Modern and Legacy Dependencies:** The format must provide a primary, backend-agnostic dependency type (`atom`) while also offering a clear, namespaced mechanism for integrating with legacy, platform-specific dependencies (e.g., traditional Nix fetchers).

## Decision

We will adopt a new manifest format that cleanly separates backend-agnostic **atoms** from backend-specific **legacy dependencies**.

This design's core principles are:

1.  **Atoms are the Primary Dependency Type:** They are backend-agnostic and grouped by source, which inherently namespaces them and solves name conflicts.
2.  **Legacy Dependencies are Namespaced:** To support integration with existing ecosystems, the format provides platform-specific tables (e.g., `[legacy.nix]`). This creates a clear separation between the modern `atom` format and any legacy, platform-specific fetchers, leaving the door open for future backends (e.g., `[legacy.guix]`) without cluttering the top-level namespace.
3.  **Content-Addressed Sources:** The identity of an atom source is determined by a hash of its content, making the lock file robust and location-independent.

### The `atom.toml` Manifest Format

#### `[atom]` Table

Defines metadata about the current project. Its `[atom.sources]` sub-table defines the available sources for atom dependencies. A source can be a single URL, a list of mirror URLs, or the special value `"::"` for the local repository.

```toml
[atom]
tag = "my-project"
version = "0.1.0"

[atom.sources]
# A single remote source.
company-atoms = "git@github.com:our-company/atoms"
# A remote source with mirrors. The resolver will try them in order.
public-registry = [ "https://registry-a.com/atoms", "https://registry-b.com/atoms" ]
# A source for atoms within this same repository
# You can specify the remote location of the local repo as a mirror if you like
local-atoms = [ "::", "https://github.com/our-company/more-atoms" ]
```

#### `[atoms.<source>]` Tables

The primary mechanism for declaring dependencies. These are fundamentally backend-agnostic and are always simple key-value pairs of `<atom-tag> = "<version-constraint>"`.

```toml
[atoms.company-atoms]
auth-service = "^1.5"
```

#### `[legacy]` Tables

This top-level table serves as a namespace for all backend-specific, non-atom dependencies.

##### `[legacy.nix.fetch]` Table

This table serves as a compatibility layer for legacy Nix dependencies that are not packaged in the backend-agnostic `atom` format. It provides a direct interface to the underlying Nix fetchers for situations where an atom-based dependency is not available.

This clear separation ensures that the primary dependency mechanism (`atoms`) remains universal, while platform-specific integrations have a dedicated, namespaced home.

### Comprehensive Example (`atom.toml`)

```toml
[atom]
tag = "my-server"
version = "0.2.0"

[atom.sources]
# sources support mirrors
company-atoms = [ "git@github.com:our-company/atoms", "https://github.com/our-companies-mirror/atoms" ]
local-project = "::"

# =============================================================================
# ATOM DEPENDENCIES
# =============================================================================

[atoms.company-atoms]
auth-service = "^1.5"

[atoms.local-project]
local-utility = "^0.1"

# =============================================================================
# LEGACY NIX DEPENDENCIES
# =============================================================================

[legacy.nix.fetch]
# --- Eval-Time Dependencies ---

# A generic URL dependency.
nix-installer.url = "https://nixos.org/nix/install"

# A Git dependency with a static ref.
nixpkgs = { git = "https://github.com/NixOS/nixpkgs", ref = "nixos-unstable" }

# A Git dependency with a dynamic version constraint.
other-repo = { git = "https://github.com/other/repo", version = "^1.2" }

# A dynamic tarball dependency that depends on a resolved atom version.
auth-service-docs = { tar = "https://docs.our-company.com/auth/{version}/docs.tar.gz", version = "company-atoms.auth-service" }


# --- Build-Time Dependencies ---

# A build-time source dependency, using the `build` key.
# This is ideal for sources that are only needed during a build.
source-archive = { build = "https://dist.our-company.com/my-server/{version}/source.tar.gz", version = "local-project.my-server" }

# A build-time dependency from a URL that is marked as executable.
online-builder = { build = "https://example.com/builder.sh", exec = true }

# A build-time archive that should not be unpacked by the fetcher.
data-archive = { build = "https://example.com/data.tar.gz", unpack = false }

```

### The `atom.lock` Lock File Format

The lock file contains two main sections: a `[sources]` table and an array of atomic `[[bonds]]` tables. This structure ensures that the identity of a source is content-addressed by it's unambiguous root id, making the lock file robust and truly decentralized.

1.  **`[sources]` Table:** This table maps a content hash of a source repository's root to its location(s). This hash serves as the canonical, location-independent identifier for the source.
2.  **`[[bonds]]` Array:** This is a unified list of all fully resolved dependencies. Atom dependencies refer to their source via its content hash, creating a stable link.

### Comprehensive Example (`atom.lock`)

```toml
# The `[sources]` table maps the content hash of a source to its mirrors.
[sources]
"<hash_of_company_atoms_root>" = [ "git@github.com:our-company/atoms", "https://github.com/our-companies-mirror/atoms" ]
"<hash_of_local_project_root>" = [ "::", "https://github.com/our-company/more-atoms" ]

# The `[[bonds]]` array lists all resolved dependencies.
[[bonds]]
type = "atom"
tag = "auth-service"
version = "1.5.2"
source = "<hash_of_company_atoms_root>"
rev = "<git_rev_of_atom>"
id = "<blake3_sum_of_atom_id>"

[[bonds]]
type = "atom"
tag = "local-utility"
version = "0.1.3"
source = "<hash_of_local_project_root>"
rev = "<git_rev_of_atom>"
id = "<blake3_sum_of_atom_id>"

[[bonds]]
type = "nix+url"
name = "nix-installer"
url = "https://nixos.org/nix/install"
hash = "sha256-..."

[[bonds]]
type = "nix+git"
name = "nixpkgs"
url = "https://github.com/NixOS/nixpkgs"
rev = "aa0ebc256a5b0540e9df53c64ef6930471c98407"

[[bonds]]
type = "nix+git"
name = "other-repo"
url = "https://github.com/other/repo"
rev = "<resolved_git_rev>"

[[bonds]]
type = "nix+tar"
name = "auth-service-docs"
url = "https://docs.our-company.com/auth/1.5.2/docs.tar.gz"
hash = "sha256-..."

[[bonds]]
type = "nix+build"
name = "source-archive"
url = "https://dist.our-company.com/my-server/0.2.0/source.tar.gz"
hash = "sha256-..."

[[bonds]]
type = "nix+build"
name = "online-builder"
url = "https://example.com/builder.sh"
hash = "sha256-..."
exec = true

[[bonds]]
type = "nix+build"
name = "data-archive"
url = "https://example.com/data.tar.gz"
hash = "sha256-..."
unpack = false
```

## Consequences

- **Positive:**

  - **Future-Proof:** The clear separation between backend-agnostic `atoms` and `legacy.nix.fetch` dependencies creates a path for supporting other backends in the future.
  - **Robust Decentralization:** Source mirroring and content-addressed locking make the format resilient.
  - **Explicit and Consistent:** The format is declarative, consistent, and easy to parse.

- **Negative:**
  - **Breaking Change:** This format is not compatible with the previous placeholder format.
  - **Implementation Effort:** The parser and resolver logic will need to be written to support this new, stable structure.
