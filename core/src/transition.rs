// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Workspace activation/deactivation: turns a workspace switch into a
//! list of backend actions (geometry capture/restore, pause/resume).

use std::collections::{BTreeSet, HashMap};

use crate::backend::DesktopBackend;
use crate::model::{WindowGeometry, WindowId, WindowInfo, WorkspaceId};

/// Tracks which workspace is active and which processes are frozen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceState {
    pub active: Option<WorkspaceId>,
    pub paused_pids: BTreeSet<u32>,
}

/// A single side effect the daemon must perform on a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Read the window's current geometry so it can be restored later.
    CaptureGeometry {
        window: WindowId,
    },
    /// Re-apply a previously captured geometry.
    RestoreGeometry {
        window: WindowId,
        geometry: WindowGeometry,
    },
    Suspend {
        pid: u32,
    },
    Resume {
        pid: u32,
    },
}

/// Plans the actions needed to switch from `state.active` to `target`, and
/// updates `state` to reflect the result.
///
/// Rules:
/// * Switching to the already-active workspace is a no-op.
/// * Windows on the outgoing workspace have their geometry captured.
/// * Processes that own windows *only* on the outgoing workspace are suspended.
/// * Processes that own windows on the target workspace are resumed if paused.
/// * Windows on the target workspace get their saved geometry restored, if any.
pub fn plan_switch(
    state: &mut WorkspaceState,
    target: &WorkspaceId,
    windows: &[WindowInfo],
    saved_geometry: &HashMap<WindowId, WindowGeometry>,
) -> Vec<Action> {
    if state.active.as_ref() == Some(target) {
        return Vec::new();
    }

    let outgoing = state.active.clone();
    let mut actions = Vec::new();

    let on = |ws: &Option<WorkspaceId>, w: &WindowInfo| {
        w.workspace.as_ref() == ws.as_ref() && ws.is_some()
    };

    // 1. Capture geometry of windows we are about to leave.
    for w in windows.iter().filter(|w| on(&outgoing, w)) {
        actions.push(Action::CaptureGeometry { window: w.id });
    }

    let target_opt = Some(target.clone());
    let target_pids: BTreeSet<u32> = windows
        .iter()
        .filter(|w| on(&target_opt, w))
        .map(|w| w.pid)
        .collect();
    let outgoing_pids: BTreeSet<u32> = windows
        .iter()
        .filter(|w| on(&outgoing, w))
        .map(|w| w.pid)
        .collect();

    // 2. Freeze apps that are leaving entirely.
    for pid in outgoing_pids.difference(&target_pids) {
        if state.paused_pids.insert(*pid) {
            actions.push(Action::Suspend { pid: *pid });
        }
    }

    // 3. Thaw apps that have windows on the target workspace.
    for pid in target_pids.iter() {
        if state.paused_pids.remove(pid) {
            actions.push(Action::Resume { pid: *pid });
        }
    }

    // 4. Restore geometry for windows arriving on the target workspace.
    for w in windows.iter().filter(|w| on(&target_opt, w)) {
        if let Some(geometry) = saved_geometry.get(&w.id) {
            actions.push(Action::RestoreGeometry {
                window: w.id,
                geometry: *geometry,
            });
        }
    }

    state.active = Some(target.clone());
    actions
}

/// Executes planned actions against a backend. Geometry captured by
/// `CaptureGeometry` is stored in `saved_geometry`.
pub fn execute<B: DesktopBackend>(
    backend: &mut B,
    actions: &[Action],
    saved_geometry: &mut HashMap<WindowId, WindowGeometry>,
) -> Result<(), B::Error> {
    for action in actions {
        match action {
            Action::CaptureGeometry { window } => {
                let g = backend.get_geometry(*window)?;
                saved_geometry.insert(*window, g);
            }
            Action::RestoreGeometry { window, geometry } => {
                backend.set_geometry(*window, *geometry)?;
            }
            Action::Suspend { pid } => backend.suspend_process(*pid)?,
            Action::Resume { pid } => backend.resume_process(*pid)?,
        }
    }
    Ok(())
}

/// Like [`execute`], but keeps going after a failed action and returns every
/// failure. A switch should not stop halfway because one elevated process
/// refuses to be suspended.
pub fn execute_best_effort<B: DesktopBackend>(
    backend: &mut B,
    actions: &[Action],
    saved_geometry: &mut HashMap<WindowId, WindowGeometry>,
) -> Vec<(Action, B::Error)> {
    let mut failures = Vec::new();
    for action in actions {
        let result = match action {
            Action::CaptureGeometry { window } => backend.get_geometry(*window).map(|g| {
                saved_geometry.insert(*window, g);
            }),
            Action::RestoreGeometry { window, geometry } => {
                backend.set_geometry(*window, *geometry)
            }
            Action::Suspend { pid } => backend.suspend_process(*pid),
            Action::Resume { pid } => backend.resume_process(*pid),
        };
        if let Err(e) = result {
            failures.push((action.clone(), e));
        }
    }
    failures
}
