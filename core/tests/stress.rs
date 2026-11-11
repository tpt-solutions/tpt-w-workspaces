// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stress test: many workspaces, rules and windows, driven by a long, seeded
//! sequence of random events. The engine must never fail, must keep its
//! freeze bookkeeping consistent with what the backend actually did, and must
//! resume everything on shutdown.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::config;
use tpt_workspaces_core::engine::{Engine, Event};
use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};
use tpt_workspaces_core::MemoryStore;

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock error")
    }
}

impl std::error::Error for MockError {}

#[derive(Default)]
struct MockBackend {
    desktops: Vec<Workspace>,
    current: Option<WorkspaceId>,
    windows: Vec<WindowInfo>,
    geometry: HashMap<WindowId, WindowGeometry>,
    suspended: Vec<u32>,
    resumed: Vec<u32>,
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
        let id = WorkspaceId::new(format!("ws-{}", self.desktops.len() + 1));
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
            .ok_or(MockError)?;
        w.workspace = Some(to.clone());
        Ok(())
    }

    fn get_geometry(&self, window: WindowId) -> Result<WindowGeometry, MockError> {
        Ok(self
            .geometry
            .get(&window)
            .copied()
            .unwrap_or(WindowGeometry {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            }))
    }

    fn set_geometry(&mut self, window: WindowId, g: WindowGeometry) -> Result<(), MockError> {
        self.geometry.insert(window, g);
        Ok(())
    }

    fn process_executable(&self, pid: u32) -> Option<String> {
        self.windows
            .iter()
            .find(|w| w.pid == pid)
            .map(|w| w.executable.clone())
    }

    fn suspend_process(&mut self, pid: u32) -> Result<(), MockError> {
        self.suspended.push(pid);
        Ok(())
    }

    fn resume_process(&mut self, pid: u32) -> Result<(), MockError> {
        self.resumed.push(pid);
        Ok(())
    }
}

/// Small deterministic PRNG so failures reproduce from the seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORKSPACES: usize = 40;
const RULES: usize = 200;
const EVENTS: usize = 10_000;
const MAX_WINDOWS: usize = 1_000;

fn config_text() -> String {
    let mut text = String::from("schema_version = 1\n");
    for i in 0..WORKSPACES {
        text.push_str(&format!(
            "\n[[workspace]]\nid = \"w{i}\"\nname = \"Workspace {i}\"\n"
        ));
    }
    for r in 0..RULES {
        text.push_str(&format!(
            "\n[[rule]]\nname = \"rule {r}\"\nexecutable = \"app{r}.exe\"\nworkspace = \"w{}\"\n",
            r % WORKSPACES
        ));
    }
    text
}

#[test]
fn random_event_storm_keeps_bookkeeping_consistent() {
    let started = Instant::now();
    let cfg = config::parse(&config_text()).expect("generated config is valid");

    let backend = MockBackend::default();
    let mut engine = Engine::start(backend, MemoryStore::default(), &cfg).unwrap();
    assert_eq!(engine.workspace_map().len(), WORKSPACES);
    assert_eq!(engine.rules().len(), RULES);

    let workspace_ids: Vec<WorkspaceId> = engine
        .workspace_map()
        .values()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    engine.backend_mut().current = Some(workspace_ids[0].clone());

    let mut rng = Lcg(0x5EED_F00D_CAFE);
    let mut next_window = 1u64;

    for step in 0..EVENTS {
        let event = match rng.below(10) {
            // Open a window: some matching a rule, some not, on a random or no workspace.
            0..=3 if engine.backend().windows.len() < MAX_WINDOWS => {
                let id = next_window;
                next_window += 1;
                let rule = rng.below(RULES + 20);
                let executable = if rule < RULES {
                    format!("app{rule}.exe")
                } else {
                    format!("other{rule}.exe")
                };
                let on = if rng.below(4) == 0 {
                    None
                } else {
                    Some(workspace_ids[rng.below(workspace_ids.len())].clone())
                };
                engine.backend_mut().windows.push(WindowInfo {
                    id: WindowId(id),
                    pid: 1000 + (id as u32),
                    executable,
                    window_class: String::new(),
                    title: format!("window {id}"),
                    workspace: on,
                    geometry: None,
                });
                Event::WindowAppeared(WindowId(id))
            }
            4..=6 => {
                let windows = &engine.backend().windows;
                if windows.is_empty() {
                    continue;
                }
                Event::WindowFocused(windows[rng.below(windows.len())].id)
            }
            _ => Event::WorkspaceFocused(workspace_ids[rng.below(workspace_ids.len())].clone()),
        };

        engine
            .handle(event.clone())
            .unwrap_or_else(|e| panic!("step {step}: {event:?} failed: {e}"));

        // Bookkeeping check: for each pid, suspends minus resumes must be 0 or 1,
        // and equal 1 exactly when the engine reports the pid as frozen.
        if step % 500 == 0 {
            assert_consistent(&engine, step);
        }
    }

    assert_consistent(&engine, EVENTS);

    // Shutdown must resume every process the engine still holds frozen, so the
    // net suspend count of every pid ends at zero.
    let (backend, _store) = engine.shutdown().unwrap();
    assert!(
        backend.suspended.len() > 100 && !backend.resumed.is_empty(),
        "storm should exercise freezing and resuming: {} suspends, {} resumes",
        backend.suspended.len(),
        backend.resumed.len()
    );
    let mut net: HashMap<u32, i64> = HashMap::new();
    for pid in &backend.suspended {
        *net.entry(*pid).or_default() += 1;
    }
    for pid in &backend.resumed {
        *net.entry(*pid).or_default() -= 1;
    }
    for (pid, count) in net {
        assert_eq!(count, 0, "pid {pid} still frozen after shutdown");
    }

    // Generous bound: catches accidental quadratic blow-ups, not normal slowness.
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "stress run took {:?}",
        started.elapsed()
    );
}

fn assert_consistent(engine: &Engine<MockBackend, MemoryStore>, step: usize) {
    let mut net: HashMap<u32, i64> = HashMap::new();
    let backend = engine.backend();
    for pid in &backend.suspended {
        *net.entry(*pid).or_default() += 1;
    }
    for pid in &backend.resumed {
        *net.entry(*pid).or_default() -= 1;
    }
    for (pid, count) in &net {
        assert!(
            (0..=1).contains(count),
            "step {step}: pid {pid} net suspend count {count}"
        );
        let reported = engine.state().paused_pids.contains(pid);
        assert_eq!(
            *count == 1,
            reported,
            "step {step}: pid {pid} backend count {count} but engine paused={reported}"
        );
    }
    for pid in &engine.state().paused_pids {
        assert_eq!(
            net.get(pid).copied().unwrap_or(0),
            1,
            "step {step}: engine thinks pid {pid} is frozen but backend never suspended it"
        );
    }
}
