# 7. A Declarative, Source-Grouped Manifest Format

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-08

## Context and Problem Statement

The `eka` project has reached a stage where a stable, well-defined manifest format (`atom.toml`) is required. The previous format was a placeholder to enable initial resolver development. This document proposes the first official, stable format, designed to be robust, intuitive, and philosophically aligned with `eka`'s decentralized nature.

The key design goals are:
1.  **Clarity and Explicitness:** The format must be declarative and easy to analyze statically.
2.  **Ergonomics and DRY:** Common operations should be concise, avoiding repetition.
3.  **Embrace Decentralization:** The format should make the origin of dependencies obvious, naturally handling potential name conflicts.
4.  **Support Multiple Fetching Semantics:** The format must distinguish between dependencies needed at Nix evaluation time and those that can be deferred to build time, mirroring the capabilities of the underlying Nix fetchers.

## Decision

We will adopt a new manifest format structured around the following primary tables: `[atom]`, `[atoms.<source>]`, and `[nix.fetch]`.

This design's core principles are:
1.  **Atoms are Grouped by Source:** This inherently namespaces dependencies, elegantly solving name conflicts while aligning with atom's fundamentally decentralized nature.
2.  **Platform-Specific Concerns are Grouped:** All Nix-related fetching logic is consolidated under a `[nix]` namespace.
3.  **A Consistent Data Model:** All fetchable dependencies are tables, with TOML's dotted key syntax providing an ergonomic shortcut for the common single-key case.
4.  **Directly Maps to Implementation:** The syntax for build-time fetches directly mirrors the arguments of the underlying Nix fetcher, ensuring clarity and maintainability.

### The `atom.toml` Manifest Format

#### `[atom]` Table

Defines metadata about the current project, including the sources it uses for its atom dependencies. A source can be a single URL, the special value `"::"` for the local repository, or a list of URLs which are treated as mirrors.

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

Dependencies on other atoms are declared as key-value pairs, where the key is the atom's tag and the value is a version constraint string.

```toml
[atoms.company-atoms]
auth-service = "^1.5"
```

#### `[nix.fetch]` Table

The `[nix.fetch]` table declares all dependencies to be fetched by the Nix backend. The key used within each dependency's table determines its fetching semantics:

-   **Eval-Time Fetches:** Use keys like `url`, `git`, or `tar`. These are fetched during Nix's evaluation phase.
-   **Build-Time URL Fetches:** Use the special key `build`. These are deferred until the build phase (Fixed-Output Derivations). The `build` key can be combined with `exec` and `unpack` booleans to mirror the arguments of the underlying Nix fetcher.
-   **Build-Time Executable Fetches:** Use the special key `exec`. This is for making a local executable file available at build time.

### Comprehensive Example

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
# NIX FETCH DEPENDENCIES
# =============================================================================

[nix.fetch]
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

-   **Positive:**
    -   **Clear Separation of Concerns:** The `[nix.fetch]` table provides a clear home for all Nix-related fetching.
    -   **Explicit Semantics:** The `build` key makes the distinction between eval-time and build-time fetching unambiguous and directly maps to the underlying implementation.
    -   **Consistent and Ergonomic:** The dotted key syntax provides a concise format for simple cases, while the data model remains a consistent set of tables.

-   **Negative:**
    -   **Breaking Change:** This format is not compatible with the previous placeholder format.
    -   **Implementation Effort:** The parser and resolver logic will need to be written to support this new, stable structure.
