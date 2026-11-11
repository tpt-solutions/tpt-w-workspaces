# tpt-workspaces — Project Todo

Persistent virtual-desktop/workspace manager for Windows & Linux, dual-licensed
MIT OR Apache-2.0 (TPT Solutions).

## Phase 0 — Project Setup & Licensing
- [x] Init git repo, `.gitignore` for Rust
- [x] Create Cargo workspace (`Cargo.toml` with members: `core`, `backend-windows`, `backend-linux`, `gui`, `cli`)
- [x] Add `LICENSE-MIT` and `LICENSE-APACHE` (dual MIT OR Apache-2.0), set `license = "MIT OR Apache-2.0"` in crate manifests
- [x] Add copyright/attribution header convention for TPT Solutions
- [x] Set up CI (build + test on Windows and Linux runners)
- [x] Write initial `README.md` (problem statement, vision, install instructions placeholder)

## Phase 1 — Core Domain Model (`core` crate, platform-agnostic)
- [x] Define core types: `Workspace`, `AppRule` (executable/window-class matcher → target workspace), `WindowGeometry`, `WorkspaceState`
- [x] Define trait boundary for platform backends: `DesktopBackend` (enumerate desktops, move window to desktop, list windows, get/set geometry, suspend/resume process)
- [x] Implement rule-matching engine (match new/existing windows against `AppRule`s)
- [x] Implement workspace activation/deactivation event model (desktop switch → fire pause/resume + geometry restore)
- [x] Unit tests for rule matching and state transitions

## Phase 2 — Persistence Layer
- [x] Define TOML schema for workspace/app-assignment config (user-editable)
- [x] Implement config loader/validator + file-watch for live reload
- [x] Set up SQLite schema for runtime state (window geometries, last-active workspace, per-app last-known position)
- [x] Implement state read/write layer (core reads/writes through a storage trait, not directly coupled to SQLite/TOML)
- [x] Migration strategy for schema changes

## Phase 3 — Windows Backend (`backend-windows`)
- [x] Enumerate/create virtual desktops via Windows Virtual Desktop APIs
- [x] Detect window creation/foreground events (winevent hooks)
- [x] Move windows to target desktop based on matched `AppRule`
- [x] Save/restore window position & size (geometry) per desktop
- [x] Implement app pause/resume via thread suspension (`NtSuspendProcess`/`NtResumeProcess` or per-thread `SuspendThread`)
- [ ] Handle edge cases: multi-window apps, elevated processes, UWP apps

## Phase 4 — Linux Backend (`backend-linux`, Wayland)
- [ ] Implement Wayland protocol client for window placement (research and pick protocol(s) usable across GNOME/KDE, e.g. `wlr-foreign-toplevel-management`, or compositor-specific extensions where needed)
- [ ] Enumerate/create workspaces via compositor-specific integration (GNOME Shell extension / KDE scripting interface, since no universal Wayland workspace-switch protocol exists)
- [ ] Move windows to target workspace based on matched `AppRule`
- [ ] Save/restore window geometry per workspace
- [ ] Implement app pause/resume via `SIGSTOP`/`SIGCONT` or cgroup freezer
- [ ] Document GNOME vs KDE support matrix/limitations

## Phase 5 — Background Service / Daemon
- [ ] Long-running background process wiring core + active backend together
- [ ] Startup/autostart registration (Windows Task Scheduler / registry run key; Linux systemd user service)
- [x] Graceful shutdown & crash recovery (reload last known state)
- [x] Structured logging

## Phase 6 — Tray Icon + Settings GUI (`gui` crate)
- [ ] Tray icon with quick actions (pause/resume all, open settings, quit)
- [ ] Settings window: manage workspaces, add/edit app-assignment rules
- [ ] Settings window: view/edit saved window geometries
- [ ] Live status view (which apps are paused, which workspace is active)
- [ ] Wire GUI to daemon (local IPC — pick mechanism: named pipe/unix socket)

## Phase 7 — Packaging & Distribution
- [ ] Windows installer (MSI/EXE via e.g. `cargo-wix` or WiX directly)
- [ ] Linux packaging: `.deb`, `.rpm`, and/or Flatpak
- [ ] Versioning & release process
- [ ] Basic auto-update check (optional for v1)

## Phase 8 — Testing & Hardening
- [ ] End-to-end manual test matrix (Windows 10/11, GNOME, KDE)
- [ ] Stress test with many workspaces/rules/windows
- [ ] Handle multi-monitor setups
- [ ] Accessibility pass on GUI

## Phase 9 — Stretch Goals (post-v1, not committed)
- [ ] Working-set/RAM trim on suspend (Windows `EmptyWorkingSet`, Linux cgroup memory reclaim) in addition to thread suspension
- [ ] Cloud sync of workspace config across machines
- [ ] Archon integration — revisit once `tpt-archon` has a windowing/capability-domain API to integrate against; currently blocked, not scheduled
