# tpt-workspaces

**Advanced Virtual Desktop & Workspace Manager** — persistent workspaces for Windows and Linux.

## The problem

Windows Virtual Desktops don't remember which apps belong where. Linux desktop
environments (GNOME, KDE) have better workspaces, but lack deep state management.

## The solution

tpt-workspaces provides persistent workspaces that:

- automatically move specific apps to specific desktops/workspaces,
- remember window geometries per workspace,
- pause (freeze) background apps while their workspace is inactive, freeing RAM and CPU.

On Linux, window placement is enforced through Wayland protocols. On
[Archon](https://github.com/tpt-solutions/tpt-archon), workspaces map onto
capability domains, and inactive workspaces can be frozen at the microkernel
task-suspension level.

## Status

Early development. The platform-agnostic core (domain model, rule matching,
workspace transitions, TOML config with validation and live reload, SQLite
state store) is implemented and tested. Platform backends, the daemon, and the
GUI are not yet implemented; see [todo.md](todo.md) for the phased plan.

## Configuration

Workspaces and app-assignment rules live in a user-editable TOML file:

```toml
schema_version = 1

[[workspace]]
id = "dev"
name = "Development"

[[workspace]]
id = "chat"

[[rule]]
name = "Editor"
executable = "Code.exe"   # case-insensitive file name
workspace = "dev"

[[rule]]
name = "Slack"
window_class = "Slack"    # window class / app id
workspace = "chat"
```

Rules are evaluated in order; the first match wins. Validate a file with:

```sh
cargo run -p tpt-workspaces-cli -- check path/to/config.toml
```

## Building

Requires a recent stable Rust toolchain.

```sh
cargo build --workspace
cargo test --workspace --all-features
```

## Crate layout

| Crate | Purpose |
| --- | --- |
| `core` | Platform-agnostic domain model, rule engine, transitions, config, storage traits |
| `backend-windows` | Windows Virtual Desktop backend (planned) |
| `backend-linux` | Wayland backend (planned) |
| `gui` | Tray icon and settings window (planned) |
| `cli` | `tpt-workspaces` command-line tool |

## Installation

Installers and packages are not yet available.

## License

Dual-licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. Copyright (c) 2026 TPT Solutions.
