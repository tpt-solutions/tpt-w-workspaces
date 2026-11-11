// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Windows backend for tpt-workspaces.
//!
//! * Virtual desktops: via `winvd` (Windows 11 24H2+).
//! * Windows: top-level window enumeration, geometry, and UWP resolution via Win32.
//! * Pause/resume: per-thread `SuspendThread` / `ResumeThread`.
//! * Events: `SetWinEventHook` on a dedicated message-loop thread.
//!
//! Workspace ids produced by this backend are desktop GUIDs in the form
//! `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`. The daemon maps user-facing
//! config ids (for example `dev`) onto these.
//!
//! The crate compiles to nothing on non-Windows targets.

#![cfg(windows)]

pub mod backend;
pub mod error;
pub mod events;
pub mod guid;
pub mod process;
pub mod window;

pub use backend::WindowsBackend;
pub use error::BackendError;
pub use events::{WindowEvent, WindowEventListener};
