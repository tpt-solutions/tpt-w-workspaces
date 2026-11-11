# Contributing to tpt-workspaces

## Licensing

The project is dual-licensed under MIT OR Apache-2.0. By contributing you agree
that your contribution is licensed under the same terms, as stated in the
Apache-2.0 license (section 5).

## Source file headers

Every Rust source file starts with the TPT Solutions copyright header:

```rust
// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0
```

Put the header above any module-level `//!` documentation. Use the current
year for new files.

Crate manifests inherit `license = "MIT OR Apache-2.0"` from the workspace
(`[workspace.package]` in the root `Cargo.toml`), so do not override it per crate.

## Before opening a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
```

CI runs the same checks on Windows and Linux.

## Schema changes

- **TOML config:** bump `CURRENT_SCHEMA_VERSION` in `core/src/config.rs` and
  handle older versions when parsing. Document the change in the README.
- **SQLite state:** append a new entry to `MIGRATIONS` in `core/src/sqlite.rs`.
  Never edit or reorder existing entries.
