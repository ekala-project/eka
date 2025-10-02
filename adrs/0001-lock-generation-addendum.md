# Addendum to ADR 0001: Refined Lock Schema with Type Safety and Conditionals

## Status

Implemented

## Changes from Original ADR

The implementation successfully uses tagged enums with `#[serde(tag = "type")]` for both `Dep` and `Src` types, providing compile-time type safety while maintaining TOML portability. The `#[serde(deny_unknown_fields)]` attribute ensures strict validation. Conditional fields are handled through the type system rather than runtime validation.

### Refined Schema

The lockfile TOML structure uses tagged tables for both dependencies and sources:

Example:

```
version = 1

[[deps]]
type = "atom"
id = "nix"
version = "0.1.2"
rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"

[[deps]]
type = "from"
name = "eval-config"
from = "nix"
get = "nixpkgs"
path = "nixos/lib/eval-config.nix"

[[srcs]]
type = "build"
name = "registry"
url = "https://raw.githubusercontent.com/NixOS/flake-registry/refs/heads/master/flake-registry.json"
hash = "sha256-hClMprWwiEQe7mUUToXZAR5wbhoVFi+UuqLL2K/eIPw="
```

### Key Implementation Details

- **Tagged Enums**: `Dep` enum with variants: `Atom(AtomDep)`, `Pin(PinDep)`, `PinGit(PinGitDep)`, `PinTar(PinTarDep)`, `From(FromDep)`
- **Source Types**: `Src` enum with `Build(BuildSrc)` variant
- **Hash Handling**: `WrappedNixHash` wrapper for Nix-compatible hash validation
- **Git Revisions**: `GitSha` enum supporting both sha1 and sha256
- **Location Flexibility**: `AtomLocation` enum for URL/path specification with flattening
- **Strict Validation**: `#[serde(deny_unknown_fields)]` prevents unknown fields
