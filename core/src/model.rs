// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Core, platform-agnostic domain types.

use serde::{Deserialize, Serialize};

/// Stable identifier of a workspace, as written by the user in the config file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub String);

impl WorkspaceId {
    pub fn new(id: impl Into<String>) -> Self {
        WorkspaceId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A named workspace (a virtual desktop on Windows, a workspace on Linux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
}

/// Opaque, backend-defined handle for a top-level window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// Position and size of a window in desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Snapshot of a top-level window as reported by a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    /// Owning process id.
    pub pid: u32,
    /// Executable file name, e.g. `code.exe`.
    pub executable: String,
    /// Window class (Win32) or app id / WM_CLASS (Wayland/X11).
    pub window_class: String,
    pub title: String,
    /// Workspace the window currently lives on, if known.
    pub workspace: Option<WorkspaceId>,
    pub geometry: Option<WindowGeometry>,
}
