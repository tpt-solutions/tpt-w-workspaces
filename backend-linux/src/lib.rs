// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linux backend for tpt-workspaces, targeting wlroots-based Wayland compositors
//! (sway first) through sway's i3-compatible IPC.
//!
//! Why sway IPC rather than raw Wayland protocols: there is no standard Wayland
//! protocol for workspaces, and `wlr-foreign-toplevel-management` cannot report
//! geometry or move windows between workspaces. Sway's IPC exposes all of that.
//!
//! * Workspaces are created on demand. Sway drops empty workspaces, so
//!   `create_desktop` only reserves a name; the workspace appears when the first
//!   window is moved to it.
//! * Pause and resume use `SIGSTOP` and `SIGCONT`.
//! * Geometry restore only works on floating windows. Setting geometry enables floating.
//! * GNOME and KDE are not supported by this backend.
//!
//! The crate compiles to nothing on non-Linux targets.

#![cfg(target_os = "linux")]

pub mod backend;
pub mod error;
pub mod events;
pub mod ipc;
pub mod process;
pub mod tree;

pub use backend::SwayBackend;
pub use error::BackendError;
pub use events::{WindowEvent, WindowEventListener};
