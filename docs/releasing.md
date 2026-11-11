# Releasing

## Versioning

The version lives once, in `[workspace.package]` in the root `Cargo.toml`. Every
crate inherits it. Versions follow [Semantic Versioning](https://semver.org/):

- **MAJOR** for incompatible changes to the config file or the state database
  that cannot be migrated automatically.
- **MINOR** for new features, new platforms, or new config options.
- **PATCH** for fixes that do not change behaviour users depend on.

Before 1.0.0, a MINOR bump may include breaking changes; note them in the changelog.

## Procedure

1. Make sure `master` is green in CI on both Windows and Linux.
2. Move the `[Unreleased]` entries in `CHANGELOG.md` under a new heading
   `## [X.Y.Z]`, and add a fresh empty `[Unreleased]` section above it.
3. Bump `version` in the root `Cargo.toml` to `X.Y.Z`, then run
   `cargo build --workspace` so `Cargo.lock` is updated, and commit both.
4. Tag the commit and push the tag:

   ```sh
   git tag -a vX.Y.Z -m "tpt-workspaces X.Y.Z"
   git push origin vX.Y.Z
   ```

5. The `Release` workflow (`.github/workflows/release.yml`) runs. It:
   - fails early if the tag does not match the `Cargo.toml` version,
   - runs the test suite,
   - builds `tpt-workspaces` for Windows (`.zip`) and Linux (`.tar.gz`),
   - publishes a GitHub release with the archives, `SHA256SUMS.txt`, and
     generated release notes.
6. Check the release page, download one archive, and run
   `tpt-workspaces --version` on each platform.

## Not yet covered

- Installers (MSI/EXE, `.deb`, `.rpm`, Flatpak) are tracked in `todo.md`,
  Phase 7. Archives are the only artifacts published today.
- Code signing is not set up. Windows SmartScreen will warn on unsigned binaries.
- Auto-update checks are not implemented.
