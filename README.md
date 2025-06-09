# Eka CLI

> ⚠️ Warning: Eka is in early experimental stages. Features are unstable and subject to change.

Eka is envisioned as a next-generation functional evaluation frontend for the Ekala development platform, aiming to offer a seamless, extensible, and powerful interface for managing complex software projects.

## A Reasonable Interface

CLI interactions with Nix builds (e.g. nix-shell) can halt for undecidable periods before even starting to build. In contrast, commands like `cargo build` operate on static, committable information, allowing them to begin work immediately.

To bridge this gap, Eka's working design incorporates:

1. A statically determinable interface
2. Integration with advanced build schedulers ([Eos][eos])
3. An extensible, schema-driven plugin system
4. The Atom format: Verifiable, versioned repository slices

## The Atom URI

In order to provide a polished and simple UI, atom's have a convenient URI format which naturally help address them.

Conceptually, an atom URI is just a URL with a configurable shortener mechanism (aliases), sane defaults to help elide the scheme in many scenarios, and a custom extension at the end to address atoms unambiguously; abstractly expressed as:

```
[scheme://][[user[:pass]@][url-alias:][url-fragment::]atom-id[@version]
```

### Concrete Examples

Below are some examples of atom URIs, with the URL portion expanded to demonstrate the alias functionality. Aliases are user settable via the `eka.toml` file, and some convenient defaults (`gh -> github.com`) are built in.

Atom's, themselves, are abstracted to a flat namespace within the store, regardless of its physical location. To demonstrate this, in the following examples, say we have an atom manifest in the git repo at `foo/bar/my@.toml` which species the `atom.id` in the TOML as `my-atom`:

- `gh:owner/repo::my-atom@^1 -> https://github.com/owner/repo`
  - the `@^1` is a semantic version request
- `gl:owner/repo::my-atom -> https://gitlab.com/owner/repo`
  - no version (`@`) means fetch the latest
- `org:repo::my-atom@0.1.0 -> https://github.com/work-org/repo`
  - assuming the user sets `org = "gh:work-org"` in the `eka.toml`. Notice that users can refer to other aliases in the config to "compose" them.
- `git@gh:owner/repo::my-atom -> ssh://git@github.com:owner/repo`
  - a URL with a user specification defaults to ssh
- `git:pass@gh:owner/repo::my-atom -> https://git:pass@github.com/owner/repo`
  - a user:pass combo defaults to https
- `http://gh:owner/repo::my-atom -> http://github.com/owner/repo`
  - it is possible to explicate the scheme where necessary, but the heuristics try to make this uncommon

## Usage

Currently, Eka provides the `publish` subcommand:

```
eka publish [OPTIONS] [PATH]...
```

This command implements an in-source publishing strategy for Atoms. It creates snapshots separate from the main repository history, enabling efficient, path-based versioning without a separate registry. This lays the groundwork for future decentralized resolution to a standard lock format _a la_ `eka resolve`.

For more detailed usage, run `eka help`.

_No more half-measures, no more compromises, and please, no more wrappers..._

## Provisional Road Map

Eka is still fairly early in development, however, the foundation piece, the git atom store format is more or less complete. The following is an outline of the steps along the path toward a relatively stable first cut of eka (will be expanded as we go):

- [x] define atom format
  - [x] implement git atom store
  - [ ] implement s3 atom store
  - [ ] ... decide on other atom storage mechanisms for 1.0
- [x] define & implement atom URI format
- [ ] integrate eka with atom modules
  - [x] implement the [atom](https://github.com/ekala-project/atom) module system
  - [ ] define a clean interface between the Nix module system for atom's and eka
    - [ ] work out an EEP to define the Atom manifest API (WIP)
- [ ] implement atom dependencies
  - [ ] `resolve` subcommand to generate a lock file
  - [ ] implement "shallow" dependency resolution algorithm (locking direct deps only)
  - [ ] implement "deep" dependency resolution using an SAT solver (e.g. resolvo crate)
- [ ] implement eka plugins
  - [ ] define cross-language plugin interface
  - [ ] allow plugins to extend the atom manifest in a principled (type-safe) manner
- [ ] implement cli subcommands
  - [x] publish subcommand (for git stores)
  - [ ] init subcommand
    - [x] git store initialization
    - [ ] user friendly initilization flow
    - [ ] init other atom stores (dependent on store implementation)
  - [ ] `list` subcommand
  - [ ] add more here as they are decided

[eos]: https://github.com/ekala-project/eos-gateway
