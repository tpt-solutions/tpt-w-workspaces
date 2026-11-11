// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! [`DesktopBackend`] implementation for Windows.

use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};

use crate::error::BackendError;
use crate::{guid, process, window};

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsBackend;

impl WindowsBackend {
    pub fn new() -> Self {
        WindowsBackend
    }
}

fn desktop_err(e: winvd::Error) -> BackendError {
    BackendError::Desktop(format!("{e:?}"))
}

fn parse_workspace(id: &WorkspaceId) -> Result<windows::core::GUID, BackendError> {
    guid::parse(id.as_str()).ok_or_else(|| BackendError::InvalidWorkspace(id.to_string()))
}

impl DesktopBackend for WindowsBackend {
    type Error = BackendError;

    fn enumerate_desktops(&self) -> Result<Vec<Workspace>, BackendError> {
        let desktops = winvd::get_desktops().map_err(desktop_err)?;
        desktops
            .iter()
            .map(|d| {
                let id = d.get_id().map_err(desktop_err)?;
                let index = d.get_index().map_err(desktop_err)?;
                let name = d.get_name().map_err(desktop_err)?;
                let name = if name.is_empty() {
                    format!("Desktop {}", index + 1)
                } else {
                    name
                };
                Ok(Workspace {
                    id: WorkspaceId::new(guid::format(&id)),
                    name,
                })
            })
            .collect()
    }

    fn current_workspace(&self) -> Result<Option<WorkspaceId>, BackendError> {
        let current = winvd::get_current_desktop().map_err(desktop_err)?;
        let id = current.get_id().map_err(desktop_err)?;
        Ok(Some(WorkspaceId::new(guid::format(&id))))
    }

    fn create_desktop(&mut self, name: &str) -> Result<WorkspaceId, BackendError> {
        let desktop = winvd::create_desktop().map_err(desktop_err)?;
        if !name.is_empty() {
            desktop.set_name(name).map_err(desktop_err)?;
        }
        let id = desktop.get_id().map_err(desktop_err)?;
        Ok(WorkspaceId::new(guid::format(&id)))
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, BackendError> {
        Ok(window::list_top_level())
    }

    fn move_window(&mut self, window_id: WindowId, to: &WorkspaceId) -> Result<(), BackendError> {
        let desktop = parse_workspace(to)?;
        let hwnd = window::hwnd(window_id);
        winvd::move_window_to_desktop(desktop, &hwnd).map_err(desktop_err)
    }

    fn get_geometry(&self, window_id: WindowId) -> Result<WindowGeometry, BackendError> {
        Ok(window::get_rect(window::hwnd(window_id))?)
    }

    fn set_geometry(
        &mut self,
        window_id: WindowId,
        geometry: WindowGeometry,
    ) -> Result<(), BackendError> {
        Ok(window::set_rect(window::hwnd(window_id), geometry)?)
    }

    fn process_executable(&self, pid: u32) -> Option<String> {
        window::executable_name(pid)
    }

    fn suspend_process(&mut self, pid: u32) -> Result<(), BackendError> {
        process::suspend_process(pid).map(|_| ())
    }

    fn resume_process(&mut self, pid: u32) -> Result<(), BackendError> {
        process::resume_process(pid).map(|_| ())
    }
}
