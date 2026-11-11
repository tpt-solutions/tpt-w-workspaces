// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Trait boundary implemented by each platform backend.

use crate::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};

/// Operations a platform backend must provide. Implemented by
/// `backend-windows` (Virtual Desktop APIs) and `backend-linux` (wlroots/sway IPC).
pub trait DesktopBackend {
    type Error: std::error::Error;

    /// All workspaces/desktops currently known to the system.
    fn enumerate_desktops(&self) -> Result<Vec<Workspace>, Self::Error>;

    /// The workspace the user is currently looking at, if the platform reports one.
    fn current_workspace(&self) -> Result<Option<WorkspaceId>, Self::Error>;

    /// Creates a new workspace/desktop and returns its id.
    fn create_desktop(&mut self, name: &str) -> Result<WorkspaceId, Self::Error>;

    /// All top-level windows that the backend can manage.
    fn list_windows(&self) -> Result<Vec<WindowInfo>, Self::Error>;

    /// Moves a window to the given workspace/desktop.
    fn move_window(&mut self, window: WindowId, to: &WorkspaceId) -> Result<(), Self::Error>;

    fn get_geometry(&self, window: WindowId) -> Result<WindowGeometry, Self::Error>;

    fn set_geometry(
        &mut self,
        window: WindowId,
        geometry: WindowGeometry,
    ) -> Result<(), Self::Error>;

    /// Executable file name of a live process, or `None` if it cannot be determined.
    /// Used to check that a persisted pid still belongs to the app we froze.
    fn process_executable(&self, pid: u32) -> Option<String>;

    /// Freezes every thread of the process (memory and CPU context are kept).
    fn suspend_process(&mut self, pid: u32) -> Result<(), Self::Error>;

    /// Unfreezes a process previously suspended with [`suspend_process`](Self::suspend_process).
    fn resume_process(&mut self, pid: u32) -> Result<(), Self::Error>;
}
