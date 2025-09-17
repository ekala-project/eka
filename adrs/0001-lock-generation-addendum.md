# Addendum to ADR 0001: Refined Lock Schema with Type Safety and Conditionals

## Status

Proposed

## Changes from Original ADR

To enhance type safety in Rust while maintaining TOML agnosticism, the Dep structure is refined to a tagged enum. This uses serde's `#[serde(tag = "type")]` to serialize variants as TOML tables with a 'type' key, ensuring compile-time field validation per type. Conditional fields (e.g., 'from' requires 'get') are enforced via runtime validation in `Lockfile::validate`.

### Refined Schema

The lockfile TOML structure remains similar, but Dep variants are represented as inline tables under [[deps]] with the 'type' tag:

Example:

```
version = 1

[[deps]]
type = "atom"
id = "nix"
version
```
