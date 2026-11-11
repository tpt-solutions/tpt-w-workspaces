// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fmt;

use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::config::{self, Config};
use tpt_workspaces_core::engine::{Engine, Event};
use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};
use tpt_workspaces_core::{MemoryStore, PausedProcess, StateStore};

#[derive(Debug)]
struct MockError(&'static str);

impl fmt::Display for MockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for MockError {}

#[derive(Default)]
struct MockBackend {
    desktops: Vec<Workspace>,
    current: Option<WorkspaceId>,
    windows: Vec<WindowInfo>,
    geometry: HashMap<WindowId, WindowGeometry>,
    executables: HashMap<u32, String>,
    created: Vec<String>,
    moves: Vec<(WindowId, WorkspaceId)>,
    suspended: Vec<u32>,
    resumed: Vec<u32>,
    fail_suspend: HashSet<u32>,
    fail_resume: HashSet<u32>,
}

impl DesktopBackend for MockBackend {
    type Error = MockError;

    fn enumerate_desktops(&self) -> Result<Vec<Workspace>, MockError> {
        Ok(self.desktops.clone())
    }

    fn current_workspace(&self) -> Result<Option<WorkspaceId>, MockError> {
        Ok(self.current.clone())
    }

    fn create_desktop(&mut self, name: &str) -> Result<WorkspaceId, MockError> {
        let id = WorkspaceId::new(format!("ws-{}", self.created.len() + 1));
        self.created.push(name.to_string());
        self.desktops.push(Workspace {
            id: id.clone(),
            name: name.to_string(),
        });
        Ok(id)
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, MockError> {
        Ok(self.windows.clone())
    }

    fn move_window(&mut self, window: WindowId, to: &WorkspaceId) -> Result<(), MockError> {
        let w = self
            .windows
            .iter_mut()
            .find(|w| w.id == window)
            .ok_or(MockError("no such window"))?;
        w.workspace = Some(to.clone());
        self.moves.push((window, to.clone()));
        Ok(())
    }

    fn get_geometry(&self, window: WindowId) -> Result<WindowGeometry, MockError> {
        Ok(self.geometry.get(&window).copied().unwrap_or(geo(0)))
    }

    fn set_geometry(&mut self, window: WindowId, g: WindowGeometry) -> Result<(), MockError> {
        self.geometry.insert(window, g);
        Ok(())
    }

    fn process_executable(&self, pid: u32) -> Option<String> {
        self.executables.get(&pid).cloned()
    }

    fn suspend_process(&mut self, pid: u32) -> Result<(), MockError> {
        if self.fail_suspend.contains(&pid) {
            return Err(MockError("access denied"));
        }
        self.suspended.push(pid);
        Ok(())
    }

    fn resume_process(&mut self, pid: u32) -> Result<(), MockError> {
        if self.fail_resume.contains(&pid) {
            return Err(MockError("access denied"));
        }
        self.resumed.push(pid);
        Ok(())
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

fn win(id: u64, pid: u32, exe: &str, on: &str) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        pid,
        executable: exe.to_string(),
        window_class: String::new(),
        title: String::new(),
        workspace: Some(WorkspaceId::new(on)),
        geometry: None,
    }
}

const CONFIG: &str = r#"
[[workspace]]
id = "dev"
name = "Development"

[[workspace]]
id = "chat"

[[rule]]
executable = "slack.exe"
workspace = "chat"
"#;

fn config_text() -> Config {
    config::parse(CONFIG).unwrap()
}

/// Backend with a pre-existing "Development" desktop that is currently active.
fn base_backend() -> MockBackend {
    MockBackend {
        desktops: vec![Workspace {
            id: WorkspaceId::new("d-dev"),
            name: "Development".into(),
        }],
        current: Some(WorkspaceId::new("d-dev")),
        ..Default::default()
    }
}

fn start(backend: MockBackend, store: MemoryStore) -> Engine<MockBackend, MemoryStore> {
    Engine::start(backend, store, &config_text()).unwrap()
}

#[test]
fn start_maps_existing_and_creates_missing_workspaces() {
    let engine = start(base_backend(), MemoryStore::default());

    assert_eq!(engine.backend().created, vec!["chat".to_string()]);
    let map = engine.workspace_map();
    assert_eq!(map["dev"], WorkspaceId::new("d-dev"));
    assert_eq!(map["chat"], WorkspaceId::new("ws-1"));
    assert_eq!(engine.rules()[0].target, WorkspaceId::new("ws-1"));
    assert_eq!(engine.state().active, Some(WorkspaceId::new("d-dev")));
}

#[test]
fn new_window_matching_a_rule_is_moved_and_its_app_frozen() {
    let mut backend = base_backend();
    backend.windows = vec![win(5, 50, "slack.exe", "d-dev")];
    let mut engine = start(backend, MemoryStore::default());

    engine.handle(Event::WindowAppeared(WindowId(5))).unwrap();

    assert_eq!(
        engine.backend().moves,
        vec![(WindowId(5), WorkspaceId::new("ws-1"))]
    );
    // Its workspace is not active now, so the app must be frozen.
    assert_eq!(engine.backend().suspended, vec![50]);
    assert!(engine.state().paused_pids.contains(&50));
}

#[test]
fn window_without_matching_rule_is_left_alone() {
    let mut backend = base_backend();
    backend.windows = vec![win(6, 60, "notepad.exe", "d-dev")];
    let mut engine = start(backend, MemoryStore::default());

    engine.handle(Event::WindowAppeared(WindowId(6))).unwrap();

    assert!(engine.backend().moves.is_empty());
    assert!(engine.backend().suspended.is_empty());
}

#[test]
fn focusing_another_workspace_switches_freezes_and_restores_geometry() {
    let chat = WorkspaceId::new("ws-1");
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: chat.clone(),
        name: "chat".into(),
    });
    backend.windows = vec![win(1, 10, "a.exe", "d-dev"), win(2, 20, "b.exe", "ws-1")];
    backend.geometry.insert(WindowId(1), geo(100));
    let mut engine = start(backend, MemoryStore::default());

    // Switch to chat by focusing its window.
    engine.handle(Event::WindowFocused(WindowId(2))).unwrap();
    assert_eq!(engine.state().active, Some(chat.clone()));
    assert_eq!(engine.backend().suspended, vec![10]);

    // The user moves the dev window while away.
    engine.backend_mut().geometry.insert(WindowId(1), geo(500));

    // Switch back.
    engine.handle(Event::WindowFocused(WindowId(1))).unwrap();
    assert_eq!(engine.state().active, Some(WorkspaceId::new("d-dev")));
    assert_eq!(engine.backend().resumed, vec![10]);
    assert_eq!(engine.backend().geometry[&WindowId(1)], geo(100));
    // The chat app is the one leaving now, so it is the frozen process.
    assert_eq!(engine.state().paused_pids, [20u32].into_iter().collect());
}

#[test]
fn geometry_survives_engine_restart_through_the_store() {
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: WorkspaceId::new("ws-1"),
        name: "chat".into(),
    });
    backend.windows = vec![win(1, 10, "a.exe", "d-dev"), win(2, 20, "b.exe", "ws-1")];
    backend.geometry.insert(WindowId(1), geo(100));

    // First session: leave dev, which saves the window's position to the store.
    let mut engine = start(backend, MemoryStore::default());
    engine.handle(Event::WindowFocused(WindowId(2))).unwrap();
    let (mut backend, store) = engine.shutdown().unwrap();

    // Second session, new engine, same store. The window was moved while the daemon was down.
    backend.geometry.insert(WindowId(1), geo(900));
    backend.current = Some(WorkspaceId::new("ws-1"));
    let mut engine = start(backend, store);
    engine.handle(Event::WindowFocused(WindowId(1))).unwrap();

    assert_eq!(engine.backend().geometry[&WindowId(1)], geo(100));
}

#[test]
fn crash_recovery_resumes_only_processes_that_still_match() {
    let mut store = MemoryStore::default();
    store
        .save_paused(&[
            PausedProcess {
                pid: 10,
                executable: "a.exe".into(),
            },
            PausedProcess {
                pid: 11,
                executable: "old.exe".into(),
            },
        ])
        .unwrap();

    let mut backend = base_backend();
    backend.executables.insert(10, "a.exe".into());
    backend.executables.insert(11, "reused.exe".into());
    let engine = start(backend, store);

    assert_eq!(engine.backend().resumed, vec![10]);
    let (_, store) = engine.shutdown().unwrap();
    assert!(store.load_paused().unwrap().is_empty());
}

#[test]
fn shutdown_resumes_everything_and_clears_the_persisted_list() {
    let chat = WorkspaceId::new("ws-1");
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: chat.clone(),
        name: "chat".into(),
    });
    backend.windows = vec![win(1, 10, "a.exe", "d-dev"), win(2, 20, "b.exe", "ws-1")];
    let mut engine = start(backend, MemoryStore::default());
    engine.handle(Event::WindowFocused(WindowId(2))).unwrap();

    let (backend, store) = engine.shutdown().unwrap();
    assert_eq!(backend.resumed, vec![10]);
    assert!(store.load_paused().unwrap().is_empty());
}

#[test]
fn failed_resume_on_shutdown_stays_recorded_for_next_start() {
    let chat = WorkspaceId::new("ws-1");
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: chat.clone(),
        name: "chat".into(),
    });
    backend.windows = vec![win(1, 10, "a.exe", "d-dev"), win(2, 20, "b.exe", "ws-1")];
    backend.fail_resume.insert(10);
    let mut engine = start(backend, MemoryStore::default());
    engine.handle(Event::WindowFocused(WindowId(2))).unwrap();

    let (_, store) = engine.shutdown().unwrap();
    assert_eq!(
        store.load_paused().unwrap(),
        vec![PausedProcess {
            pid: 10,
            executable: "a.exe".into()
        }]
    );
}

#[test]
fn failed_suspend_is_not_tracked_as_frozen() {
    let chat = WorkspaceId::new("ws-1");
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: chat.clone(),
        name: "chat".into(),
    });
    backend.windows = vec![
        win(1, 10, "elevated.exe", "d-dev"),
        win(2, 20, "b.exe", "ws-1"),
    ];
    backend.fail_suspend.insert(10);
    let mut engine = start(backend, MemoryStore::default());

    engine.handle(Event::WindowFocused(WindowId(2))).unwrap();

    assert_eq!(engine.state().active, Some(chat));
    assert!(!engine.state().paused_pids.contains(&10));
}

#[test]
fn config_reload_replaces_rules() {
    let mut engine = start(base_backend(), MemoryStore::default());
    assert_eq!(engine.rules().len(), 1);

    let reloaded = config::parse(
        r#"
[[workspace]]
id = "dev"
name = "Development"
"#,
    )
    .unwrap();
    engine.handle(Event::ConfigReloaded(reloaded)).unwrap();
    assert!(engine.rules().is_empty());
}

#[test]
fn workspace_focus_event_switches_even_when_the_workspace_is_empty() {
    let chat = WorkspaceId::new("ws-1");
    let mut backend = base_backend();
    backend.desktops.push(Workspace {
        id: chat.clone(),
        name: "chat".into(),
    });
    backend.windows = vec![win(1, 10, "a.exe", "d-dev")];
    let mut engine = start(backend, MemoryStore::default());

    // Chat has no windows, so no window focus event could reveal the switch.
    engine
        .handle(Event::WorkspaceFocused(chat.clone()))
        .unwrap();

    assert_eq!(engine.state().active, Some(chat));
    assert_eq!(engine.backend().suspended, vec![10]);
}

#[test]
fn pause_background_freezes_only_matched_windows_on_other_workspaces() {
    let mut backend = base_backend();
    backend.windows = vec![
        // Matches the rule, lives on the inactive "chat" workspace: frozen.
        win(1, 50, "slack.exe", "ws-1"),
        // Matches the rule but is on the active workspace: left alone.
        win(2, 51, "slack.exe", "d-dev"),
        // Not matched by any rule: left alone even though it is in the background.
        win(3, 52, "notepad.exe", "ws-1"),
    ];
    let mut engine = start(backend, MemoryStore::default());

    let count = engine.pause_background().unwrap();

    assert_eq!(count, 1);
    assert_eq!(engine.backend().suspended, vec![50]);
    assert_eq!(
        engine.paused_processes(),
        vec![(50, "slack.exe".to_string())]
    );
}

#[test]
fn pause_background_skips_windows_with_unknown_workspace() {
    let mut backend = base_backend();
    let mut window = win(1, 50, "slack.exe", "ws-1");
    window.workspace = None;
    backend.windows = vec![window];
    let mut engine = start(backend, MemoryStore::default());

    assert_eq!(engine.pause_background().unwrap(), 0);
    assert!(engine.backend().suspended.is_empty());
}

#[test]
fn resume_all_resumes_every_frozen_process_and_clears_state() {
    let mut backend = base_backend();
    backend.windows = vec![win(1, 50, "slack.exe", "ws-1")];
    let mut engine = start(backend, MemoryStore::default());
    engine.pause_background().unwrap();

    assert_eq!(engine.resume_all().unwrap(), 1);
    assert_eq!(engine.backend().resumed, vec![50]);
    assert!(engine.state().paused_pids.is_empty());
    assert!(engine.paused_processes().is_empty());

    // Nothing left to resume the second time.
    assert_eq!(engine.resume_all().unwrap(), 0);
}

#[test]
fn resume_all_keeps_a_process_it_could_not_resume() {
    let mut backend = base_backend();
    backend.windows = vec![win(1, 50, "slack.exe", "ws-1")];
    backend.fail_resume.insert(50);
    let mut engine = start(backend, MemoryStore::default());
    engine.pause_background().unwrap();

    assert_eq!(engine.resume_all().unwrap(), 0);
    assert!(engine.state().paused_pids.contains(&50));
    assert_eq!(
        engine.paused_processes(),
        vec![(50, "slack.exe".to_string())]
    );
}

#[test]
fn leaving_an_unmanaged_desktop_does_not_freeze_its_apps() {
    // The daemon starts on the user's own desktop, which the config does not name.
    let mut backend = base_backend();
    backend.current = Some(WorkspaceId::new("d-personal"));
    backend.windows = vec![
        // An app on the user's own desktop. Nothing manages it.
        win(1, 70, "code.exe", "d-personal"),
        // A rule-matched window on the managed "chat" workspace.
        win(2, 71, "slack.exe", "ws-1"),
    ];
    let mut engine = start(backend, MemoryStore::default());

    engine
        .handle(Event::WorkspaceFocused(WorkspaceId::new("ws-1")))
        .unwrap();

    assert!(
        !engine.backend().suspended.contains(&70),
        "an app on the unmanaged desktop was frozen: {:?}",
        engine.backend().suspended
    );
    assert_eq!(engine.state().active, Some(WorkspaceId::new("ws-1")));
}
