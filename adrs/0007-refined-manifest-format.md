# 7. Refined Manifest and Lock File Format

- **Status:** Proposed
- **Deciders:** eka-devs
- **Date:** 2025-10-07

## Context and Problem Statement

The initial `atom.toml` manifest format has served its purpose but has several areas for improvement. As we add more features and dependency types, the need for a more robust, intuitive, and consistent format has become clear.

The key issues with the current format are:

1.  **Ambiguous Dependency Types:** The type of a dependency is often inferred from the presence of certain keys (e.g., `store`), which is not always clear.
2.  **Verbose and Repetitive:** Defining sources for decentralized atoms requires repeating URLs.
3.  **Inconsistent Structure:** Different dependency types are defined in slightly different ways.
4.  **Unclear Terminology:** Terms like `deps` and `store` could be more formal and intuitive.

We need a new format that is explicit, ergonomic, consistent, and philosophically aligned with the decentralized, repository-aware nature of `eka`.

## Decision Drivers

- **Clarity and Explicitness:** The format should be self-documenting. It must be easy to understand what each dependency is and where it comes from just by reading the manifest.
- **Ergonomics:** The most common operations should be simple and concise.
- **Consistency:** All dependency types should follow a similar, predictable structure.
- **Don't Repeat Yourself (DRY):** The format should avoid forcing users to repeat information, such as source URLs.
- **Future-Proofing:** The design should be extensible to support new dependency types without requiring another breaking change.

## Considered Options

1.  **Incremental Changes:** Making small tweaks to the existing format. This was rejected as it would not fully address the underlying structural issues.
2.  **Nested/Dotted-Key Syntax:** Using a format like `source_a.bar = "^1"`. This was rejected as it scales poorly for more complex definitions and for non-atom dependency types.
3.  **Decoupled Sources with Explicit Input Types:** A design where sources are defined separately and dependencies explicitly declare their type. This was chosen as it best meets all decision drivers.

## Decision

We will adopt a new manifest and lock file format. The manifest (`atom.toml`) will be structured with three primary tables: `[atom]`, `[sources]`, and `[bonds]`.

### The `atom.toml` Manifest Format

#### `[atom]` Table

Defines metadata about the current project (the "atom" being defined).

```toml
[atom]
tag = "atom_name"
version = "1.0.1"
```

#### `[sources]` Table

Provides short, memorable aliases for remote source URLs. This table is the single source of truth for remote locations and is used to resolve aliases in the `[bonds]` table and CLI commands.

```toml
[sources]
git_source = "https://my.com/cool/repo"
source_a = "https://some.com/atom/repo"
```

#### `[bonds]` Table

A single, unified table that declares all project dependencies, or "bonds". The bond's type is determined by a unique, explicit key (`atom`, `rel`, `git`, `tar`, `pin`, `src`).

```toml
[bonds]
# A relative (in-repo) atom. The version is resolved from its own manifest at lock time.
foo = { rel = true }

# An external atom from a named source.
bar = { atom = "source_a", version = "^1" }

# An external atom from a direct URL. The `tag` key is required to avoid a name conflict
# with the dependency's own tag.
bun = { atom = "https://some.com/atom/repo", version = "^2", tag = "bar" }

# A tarball pin.
baz = { tar = "https://example/my/tarball.tar.gz" }

# A git pin using a named source.
buz = { git = "git_source", ref = "master" }

# A git pin with a version constraint, resolved via git tags.
my_repo = { git = "https://gitlab.com/foo/bar.git", version = "^2" }

# A generic pin from a URL.
buzz = { pin = "https://some.com/external/pin.nix" }

# A build-time source dependency.
my_src = { src = "https://foo.com/my/build/src.tar.gz" }
```

### The `atom.lock` Lock File Format

The lock file will contain an array of tables, `[[deps]]`, where each entry represents a fully resolved dependency.

```toml
[[deps]]
type = "atom"
tag = "foo"
version = "1.0.1" # Exact resolved version
rev = "<git_rev>"
# The source is a relative path for local development, which is crucial for
# ensuring the revision hash remains stable across different clones.
source = "../local/resolved/path"
id = "<blake3_sum_of_atom_id>"

[[deps]]
type = "atom"
tag = "bar"
version = "1.0.3"
rev = "<git_rev>"
source = "https://some.com/atom/repo"
id = "<blake3_sum_of_atom_id>"

[[deps]]
type = "atom"
tag = "bar"
# The `key` field is present when the manifest key differs from the atom's tag.
key = "bun"
version = "2.0.3"
rev = "<git_rev>"
source = "ssh://git@some.com/atom/repo"
id = "<blake3_sum_of_atom_id>"

[[deps]]
type = "pin+tar"
name = "baz"
url = "https://example/my/tarball.tar.gz"
hash = "sha256:0lkjn8q6p0c18acj43pj1cbiyixnf98wvkbgppr5vz73qkypii2g"

[[deps]]
type = "git"
name = "my_repo"
url = "https://my.com/cool/repo"
rev = "aa0ebc256a5b0540e9df53c64ef6930471c98407"

[[deps]]
type = "pin"
name = "buzz"
url = "https://some.com/external/pin.nix"
hash = "sha256:1spc2lsx16xy612lg8rsyd34j9fy6kmspxcvcfmawkxmyvi32g9v"

[[deps]]
type = "build"
name = "my_src"
url = "https://foo.com/my/build/src.tar.gz"
hash = "sha256-hClMprWwiEQe7mUUToXZAR5wbhoVFi+UuqLL2K/eIPw="
```

## Consequences

- **Positive:**

  - The manifest format will be significantly more intuitive and readable.
  - The structure is consistent across all dependency types.
  - Redundancy is eliminated through the `[sources]` table.
  - The handling of relative (in-repo) dependencies is more robust and ergonomic.
  - The format is easily extensible for future dependency types.

- **Negative:**
  - This is a breaking change and will require a migration path for existing `atom.toml` files.
  - The parser and resolver logic will need to be rewritten to support the new format.

### Synergy with CLI URI Resolution

A key benefit of this design is the extension of the existing CLI URI resolution feature. Currently, users can define global aliases in their `eka.toml` configuration. This proposal allows the `[sources]` table in a project's `atom.toml` to serve as a set of temporary, project-specific aliases.

This creates a powerful, layered aliasing system. When a user runs a command like `eka add source_a::my-atom@^1`, the resolver will:

1.  First, check for a `source_a` alias in the current project's `[sources]` table.
2.  If not found, it will fall back to checking for `source_a` in the user's global `eka.toml` aliases.

This provides a powerful and intuitive workflow, allowing project-specific sources to be used as convenient aliases for adding dependencies without requiring modification of the user's global configuration. It demonstrates the extensibility of the original URI alias design.

### Simplification and Reduced Scope

A key goal of this redesign is to clarify the domain of concern for the manifest. The manifest's responsibility is to declare project bonds, not to handle language-specific integration details.

To that end, the following features are explicitly **removed** from the manifest and lock file formats:

- **`import` and `flake` keys:** These Nix-specific concepts do not belong in the generic dependency manifest. The responsibility for importing a Nix flake from a resolved dependency should be handled by library code within the Nix integration layer (e.g., via a dedicated function that operates on a resolved input).
- **`from` key:** The ability to extract a sub-path from a dependency is also a language- or build-system-specific concern. This logic should be handled by the consuming code, not by the dependency resolver, e.g. re-exporting an input after calling one of the specialized import functions to handle various input types in atom-nix.

This simplification ensures that the manifest remains a universal, language-agnostic declaration of project dependencies, preventing implementation details from leaking into the API.
