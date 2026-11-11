# Linux support matrix

tpt-workspaces on Linux is built on sway's IPC. It is not yet built on the
Wayland protocols or on the GNOME and KDE desktop APIs. This page records what
works today on each environment and what is missing.

## Support matrix

| Environment | Window placement | Workspaces | Geometry save/restore | Pause/resume | Status |
| --- | --- | --- | --- | --- | --- |
| sway (wlroots) | Yes, via sway IPC | Yes, via sway IPC | Yes, floating windows only | Yes, `SIGSTOP`/`SIGCONT` | Supported; the only backend implemented |
| Other wlroots compositors (Hyprland, river, Wayfire) | No | No | No | No | Not supported. Each has its own IPC; no adapter exists |
| GNOME (Mutter) | No | No | No | No | Not supported. Planned route: GNOME Shell extension |
| KDE Plasma (KWin) | No | No | No | No | Not supported. Planned route: KWin scripting interface |
| X11 | No | No | No | No | Not supported |

## Limitations on sway

- **Empty workspaces disappear.** Sway drops a workspace once it has no windows.
  `create_desktop` only reserves a name; the workspace appears when the first
  window is moved to it.
- **Geometry only applies to floating windows.** Setting a window's geometry
  enables floating on it. Tiled windows keep their layout.
- **Client-chosen sizes win.** Terminals and other apps that snap to a grid can
  end up slightly different from the requested size. The integration tests allow
  for this.
- **Requires `SWAYSOCK`.** The daemon finds sway through the `SWAYSOCK`
  environment variable, which sway sets for its children. Start the daemon from
  inside the sway session, for example through the systemd user unit that
  `tpt-workspaces autostart enable` writes.

## Why not Wayland protocols

There is no standard Wayland protocol for workspaces. The
`wlr-foreign-toplevel-management` protocol cannot report geometry or move
windows between workspaces, so sway IPC is used instead. GNOME and KDE do not
implement the wlroots protocols at all.
