// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::HashMap;
use std::convert::Infallible;

use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};
use tpt_workspaces_core::transition::{execute, plan_switch, Action, WorkspaceState};

fn ws(id: &str) -> WorkspaceId {
    WorkspaceId::new(id)
}

fn win(id: u64, pid: u32, on: &str) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        pid,
        executable: format!("app{pid}.exe"),
        window_class: String::new(),
        title: String::new(),
        workspace: Some(ws(on)),
        geometry: None,
    }
}

fn geo(x: i32) -> WindowGeometry {
    WindowGeometry {
        x,
        y: 0,
        width: 800,
        height: 600,
    }
}

#[test]
fn switching_to_active_workspace_is_noop() {
    let mut state = WorkspaceState {
        active: Some(ws("a")),
        paused_pids: Default::default(),
    };
    let actions = plan_switch(&mut state, &ws("a"), &[win(1, 10, "a")], &HashMap::new());
    assert!(actions.is_empty());
    assert_eq!(state.active, Some(ws("a")));
}

#[test]
fn leaving_workspace_captures_and_suspends_its_apps() {
    let windows = [win(1, 10, "a"), win(2, 20, "b")];
    let mut state = WorkspaceState {
        active: Some(ws("a")),
        paused_pids: Default::default(),
    };
    let actions = plan_switch(&mut state, &ws("b"), &windows, &HashMap::new());

    assert!(actions.contains(&Action::CaptureGeometry {
        window: WindowId(1)
    }));
    assert!(actions.contains(&Action::Suspend { pid: 10 }));
    assert!(!actions.contains(&Action::Suspend { pid: 20 }));
    assert!(!actions.contains(&Action::Resume { pid: 20 }));
    assert!(state.paused_pids.contains(&10));
    assert_eq!(state.active, Some(ws("b")));
}

#[test]
fn returning_resumes_paused_apps_and_restores_geometry() {
    let windows = [win(1, 10, "a"), win(2, 20, "b")];
    let mut saved = HashMap::new();
    saved.insert(WindowId(1), geo(42));
    let mut state = WorkspaceState {
        active: Some(ws("b")),
        paused_pids: [10u32].into_iter().collect(),
    };

    let actions = plan_switch(&mut state, &ws("a"), &windows, &saved);

    assert!(actions.contains(&Action::Resume { pid: 10 }));
    assert!(actions.contains(&Action::RestoreGeometry {
        window: WindowId(1),
        geometry: geo(42),
    }));
    // Pid 20 is leaving, so it is now the only frozen process.
    assert!(actions.contains(&Action::Suspend { pid: 20 }));
    assert_eq!(state.paused_pids, [20u32].into_iter().collect());
}

#[test]
fn app_with_windows_on_both_workspaces_is_not_suspended() {
    // Pid 10 owns a window on "a" and one on "b". Leaving "a" must not freeze it.
    let windows = [win(1, 10, "a"), win(2, 10, "b")];
    let mut state = WorkspaceState {
        active: Some(ws("a")),
        paused_pids: Default::default(),
    };
    let actions = plan_switch(&mut state, &ws("b"), &windows, &HashMap::new());
    assert!(!actions.iter().any(|a| matches!(a, Action::Suspend { .. })));
    assert!(state.paused_pids.is_empty());
}

#[test]
fn first_activation_has_no_outgoing_side_effects() {
    let windows = [win(1, 10, "a")];
    let mut state = WorkspaceState::default();
    let actions = plan_switch(&mut state, &ws("a"), &windows, &HashMap::new());
    assert!(actions.is_empty());
    assert_eq!(state.active, Some(ws("a")));
}

#[derive(Default)]
struct MockBackend {
    geometry: HashMap<WindowId, WindowGeometry>,
    suspended: Vec<u32>,
    resumed: Vec<u32>,
}

impl DesktopBackend for MockBackend {
    type Error = Infallible;

    fn enumerate_desktops(&self) -> Result<Vec<Workspace>, Infallible> {
        Ok(Vec::new())
    }
    fn current_workspace(&self) -> Result<Option<WorkspaceId>, Infallible> {
        Ok(None)
    }
    fn process_executable(&self, _pid: u32) -> Option<String> {
        None
    }
    fn create_desktop(&mut self, _name: &str) -> Result<WorkspaceId, Infallible> {
        Ok(ws("new"))
    }
    fn list_windows(&self) -> Result<Vec<WindowInfo>, Infallible> {
        Ok(Vec::new())
    }
    fn move_window(&mut self, _w: WindowId, _to: &WorkspaceId) -> Result<(), Infallible> {
        Ok(())
    }
    fn get_geometry(&self, w: WindowId) -> Result<WindowGeometry, Infallible> {
        Ok(self.geometry.get(&w).copied().unwrap_or(geo(0)))
    }
    fn set_geometry(&mut self, w: WindowId, g: WindowGeometry) -> Result<(), Infallible> {
        self.geometry.insert(w, g);
        Ok(())
    }
    fn suspend_process(&mut self, pid: u32) -> Result<(), Infallible> {
        self.suspended.push(pid);
        Ok(())
    }
    fn resume_process(&mut self, pid: u32) -> Result<(), Infallible> {
        self.resumed.push(pid);
        Ok(())
    }
}

#[test]
fn full_round_trip_through_backend_preserves_geometry() {
    let windows = [win(1, 10, "a"), win(2, 20, "b")];
    let mut backend = MockBackend::default();
    backend.geometry.insert(WindowId(1), geo(100));
    let mut saved = HashMap::new();
    let mut state = WorkspaceState {
        active: Some(ws("a")),
        paused_pids: Default::default(),
    };

    // a -> b: capture a's geometry, freeze pid 10.
    let actions = plan_switch(&mut state, &ws("b"), &windows, &saved);
    execute(&mut backend, &actions, &mut saved).unwrap();
    assert_eq!(saved.get(&WindowId(1)), Some(&geo(100)));
    assert_eq!(backend.suspended, vec![10]);

    // The user moves the window while away.
    backend.geometry.insert(WindowId(1), geo(500));

    // b -> a: thaw pid 10 and put the window back where it was on the way out.
    let actions = plan_switch(&mut state, &ws("a"), &windows, &saved);
    execute(&mut backend, &actions, &mut saved).unwrap();
    assert_eq!(backend.resumed, vec![10]);
    assert_eq!(backend.geometry[&WindowId(1)], geo(100));
}
