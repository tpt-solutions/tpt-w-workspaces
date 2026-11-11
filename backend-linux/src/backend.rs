// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! [`DesktopBackend`] implementation on top of sway IPC.

use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};

use crate::error::BackendError;
use crate::ipc::{self, quote, GET_TREE, GET_WORKSPACES};
use crate::{process, tree};

/// Backend that talks to a running sway (or sway-compatible) compositor.
#[derive(Debug, Default, Clone, Copy)]
pub struct SwayBackend;

impl SwayBackend {
    /// Checks that sway is reachable. Each operation opens its own IPC connection.
    pub fn connect() -> Result<Self, BackendError> {
        ipc::request(GET_WORKSPACES, &[])?;
        Ok(SwayBackend)
    }
}

fn tree_root() -> Result<serde_json::Value, BackendError> {
    ipc::request(GET_TREE, &[])
}

impl DesktopBackend for SwayBackend {
    type Error = BackendError;

    fn enumerate_desktops(&self) -> Result<Vec<Workspace>, BackendError> {
        let reply = ipc::request(GET_WORKSPACES, &[])?;
        let names = reply
            .as_array()
            .ok_or_else(|| BackendError::Ipc("GET_WORKSPACES reply is not an array".into()))?;
        Ok(names
            .iter()
            .filter_map(|ws| ws["name"].as_str())
            .filter(|name| *name != tree::SCRATCHPAD)
            .map(|name| Workspace {
                id: WorkspaceId::new(name),
                name: name.to_string(),
            })
            .collect())
    }

    fn current_workspace(&self) -> Result<Option<WorkspaceId>, BackendError> {
        let reply = ipc::request(GET_WORKSPACES, &[])?;
        Ok(reply.as_array().and_then(|list| {
            list.iter()
                .find(|ws| ws["focused"].as_bool() == Some(true))
                .and_then(|ws| ws["name"].as_str())
                .map(WorkspaceId::new)
        }))
    }

    fn create_desktop(&mut self, name: &str) -> Result<WorkspaceId, BackendError> {
        // Sway creates workspaces on demand and discards empty ones, so there is
        // nothing to create yet. The name is enough.
        Ok(WorkspaceId::new(name))
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, BackendError> {
        let root = tree_root()?;
        Ok(tree::leaves(&root)
            .into_iter()
            .map(|leaf| WindowInfo {
                id: leaf.id,
                pid: leaf.pid,
                executable: process::executable_name(leaf.pid).unwrap_or_default(),
                window_class: leaf.app_id.or(leaf.class).unwrap_or_default(),
                title: leaf.title,
                workspace: Some(leaf.workspace),
                geometry: Some(leaf.geometry),
            })
            .collect())
    }

    fn move_window(&mut self, window: WindowId, to: &WorkspaceId) -> Result<(), BackendError> {
        ipc::run_command(&format!(
            "[con_id={}] move container to workspace {}",
            window.0,
            quote(to.as_str())
        ))
    }

    fn get_geometry(&self, window: WindowId) -> Result<WindowGeometry, BackendError> {
        tree::find(&tree_root()?, window)
            .map(|leaf| leaf.geometry)
            .ok_or_else(|| BackendError::NotFound(format!("window {}", window.0)))
    }

    fn set_geometry(
        &mut self,
        window: WindowId,
        geometry: WindowGeometry,
    ) -> Result<(), BackendError> {
        // Absolute placement only applies to floating windows, so float first.
        // Size before position: a resize on a floating window can re-centre it,
        // so the move goes last to keep the requested top-left corner.
        ipc::run_command(&format!(
            "[con_id={}] floating enable, resize set {} {}, move absolute position {} {}",
            window.0, geometry.width, geometry.height, geometry.x, geometry.y
        ))
    }

    fn process_executable(&self, pid: u32) -> Option<String> {
        process::executable_name(pid)
    }

    fn suspend_process(&mut self, pid: u32) -> Result<(), BackendError> {
        process::suspend(pid)
    }

    fn resume_process(&mut self, pid: u32) -> Result<(), BackendError> {
        process::resume(pid)
    }
}
