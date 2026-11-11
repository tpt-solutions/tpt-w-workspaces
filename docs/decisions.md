# Decisions

Decisions that shape the remaining work in `todo.md`. Each records what was
chosen and why. Revisit one by adding a new entry that supersedes it, not by
editing history.

## D1: GUI toolkit is egui (eframe) with tray-icon

- **Chosen:** `eframe` (egui) for the status and settings window, and
  `tray-icon` for the system tray icon and menu.
- **Why:** pure Rust, one code path for Windows and Linux, no GTK or Qt runtime
  to install, and the window can be drawn with plain widgets.
- **Consequence:** the GUI is a separate binary (`tpt-workspaces-gui`) that talks
  to the daemon over the local IPC in `core::ipc`.

## D2: Linux backend stays on sway IPC; GNOME and KDE are post-v1

- **Chosen:** sway (wlroots) is the only supported Linux environment for v1.
- **Why:** sway IPC already provides workspaces, geometry and window moves. The
  Wayland-protocol items in Phase 4 are superseded by this, because no standard
  protocol covers workspaces, and GNOME and KDE need compositor-specific code
  that cannot be tested here.
- **Consequence:** the Phase 4 GNOME/KDE items are deferred. See
  `docs/linux-support.md`.

## D3: Installers come after the GUI

- **Chosen:** MSI via `cargo-wix` for Windows; `.deb` via `cargo-deb` for Linux.
  RPM and Flatpak are deferred.
- **Why:** an installer that ships without the tray and settings window would
  need rework once they exist. Archives remain the release artifact until then.

## D4: No auto-update in v1

- **Chosen:** no update check. Users download new releases manually.
- **Why:** the todo marks it optional, and an updater needs signing, which is not
  set up yet.

## D5: Multi-monitor and accessibility wait for hardware and a GUI

- **Chosen:** Phase 8 multi-monitor and accessibility items stay open until the
  GUI exists and can be tested on real displays.
- **Why:** neither can be verified in this environment.

## D6: Archon and Phase 9 stretch goals stay unscheduled

- **Chosen:** no work on Phase 9 items. Archon integration stays blocked until
  `tpt-archon` exposes a capability-domain API.
