# Changelog

All notable changes to tpt-workspaces are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `tpt-workspaces status` queries the running daemon over local IPC (named pipe
  on Windows, Unix socket on Linux).
- `tpt-workspaces --version`.
- Stress test for the core engine with many workspaces, rules and windows.
- Linux support matrix in `docs/linux-support.md`.
- Release workflow that builds Windows and Linux archives on version tags.
- `tpt-workspaces-gui`: status window and tray icon. Closing the window minimizes
  it; the tray menu offers "Show status" and "Quit".
- Local IPC lives in `core::ipc`, shared by the CLI and the GUI.
- Settings tab in the GUI: edit the config as text, validate it, and save. Invalid
  text is never written, and the daemon reloads a saved file automatically.
  Remembered window positions are listed below the editor.
  Each remembered position can be forgotten, and "Add workspace" and "Add rule"
  append templates to the editor.
- `TPT_WORKSPACES_CONFIG` and `TPT_WORKSPACES_STATE` override the config and
  state file locations.
- Tray menu item "Open settings" opens the window on the Settings tab.
- Settings form: workspaces and rules are edited as fields and written to the
  config text on Apply. Applying rewrites the text, so comments are not kept.
- Windows MSI installer (`cargo wix`) and a Debian package (`cargo deb`) in the
  release workflow.
- `tpt-workspaces pause` freezes background apps (rule-matched windows on other
  workspaces); `tpt-workspaces resume` resumes every frozen app. The same actions
  are in the GUI window and tray menu. A resumed app is frozen again on its next
  window event if it is still in the background.
- The status view lists frozen apps by name and PID.

### Changed
- The daemon refuses to start when another daemon is already running.
- The IPC pipe on Windows is restricted to the current user.
- Windows: elevated processes are reported as not pausable instead of failing
  silently on every window event.
- Sway: floating windows are sized before they are positioned, so the requested
  top-left corner is kept.

### Fixed
- Switching to a managed workspace no longer freezes the apps on the desktop the
  daemon started on. That desktop is not managed, so its apps are left alone.
- Windows of frozen apps are listed without reading their titles. Reading a
  title waits for the owning app, which never answers while it is frozen, so the
  daemon could stop responding.
- The daemon logs each event it handles at debug level, and warns when one takes
  more than two seconds.

## [0.1.0]

Initial development release. Not yet suitable for daily use.

### Added
- Core domain model, rule engine, workspace transitions, TOML config with live
  reload, and SQLite state store.
- Windows backend: virtual desktops (Windows 11 24H2+), window placement,
  geometry restore, thread-level pause/resume, UWP window resolution.
- Linux backend for sway (wlroots) via sway IPC.
- `tpt-workspaces` CLI and daemon, with `check`, `run` and `autostart` commands.
- Structured logging (`RUST_LOG`, `TPT_LOG_JSON=1`).
