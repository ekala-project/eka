# Rust Style Guide

## 1. Introduction

This style guide codifies a set of idiomatic conventions for writing Rust code in this project. The primary goal is to ensure that the codebase remains consistent, readable, and easy to maintain over time. Adhering to these standards reduces cognitive overhead and makes collaboration more effective.

## 2. File Structure and Item Order

All Rust module files (`.rs`) must follow a strict top-level item order to ensure predictability and ease of navigation. The canonical order is as follows:

1.  **Module-level documentation (`//!`)**: Explains the purpose and scope of the module.
2.  **Outer attributes (`#![...]`)**: Compiler directives like `#![deny(missing_docs)]`.
3.  **`use` declarations**: External and internal imports.
4.  **Public re-exports (`pub use`)**: Items re-exported from other modules.
5.  **Submodules (`mod`)**: Child module declarations.
6.  **Constants (`const`)**: Compile-time constants.
7.  **Static variables (`static`)**: Globally allocated variables.
8.  **Types**: `struct`, `enum`, and `type` aliases.
9.  **Traits**: Trait definitions.
10. **Trait implementations and `impl` blocks**: Implementations of traits and inherent methods.
11. **Free-standing functions**: Module-level functions.
12. **Tests (`#[cfg(test)]` modules)**: Unit and integration tests for the module.

## 3. Sorting and Grouping Rules

Within each category, items must be sorted to maintain a consistent structure.

### `use` Declarations

`use` declarations are grouped in the following order, with each group sorted alphabetically:

1.  **`std`**: Standard library imports.
2.  **External Crates**: Third-party dependencies.
3.  **Local Modules**: Project-internal imports, starting with `crate::` or `super::`.

Example:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use log::info;

use crate::core::Atom;
use super::utils::helper_function;
```

### Other Items

All other top-level items—including modules, constants, types, traits, and functions—must be sorted alphabetically by their identifier.

### Visibility

Within any given category, **public (`pub`) items must always be placed before private items**. This rule applies before alphabetical sorting. For example, a public function `alpha` would come before a private function `beta`, but also before a public function `zeta`.

Example:

```rust
// Public items first, sorted alphabetically
pub const MAX_RETRIES: u32 = 3;
pub fn get_config() -> Config { /* ... */ }

// Private items next, sorted alphabetically
const DEFAULT_TIMEOUT: u64 = 10;
fn process_data() { /* ... */ }
```

## 4. Documentation Comments

Clear and comprehensive documentation is mandatory for maintaining a high-quality codebase.

- **All public items** (modules, functions, types, traits, constants) must have descriptive documentation comments (`///`).
- **Module-level documentation (`//!`)** is required for every module. It should provide a high-level overview of the module's responsibilities and how it fits into the larger system.
- Comments should be clear, concise, and sufficient for a developer to understand the item's purpose and usage without needing to read the underlying source code.

## 5. General Guidelines

- **Use `rustfmt`**: This project uses an opinionated `rustfmt.toml` configuration to enforce a consistent code style. Running `cargo fmt` will automatically handle much of the formatting for you. However, the guidelines in this document (especially regarding item order and documentation) must still be followed manually.
- **Preserve Semantics**: Never alter the meaning or behavior of the code purely for the sake of conforming to style.
- **Preserve Comments and Attributes**: When reordering items, ensure that all associated documentation, comments (`//`), and attributes (`#[...]`) are moved along with the item they describe.
