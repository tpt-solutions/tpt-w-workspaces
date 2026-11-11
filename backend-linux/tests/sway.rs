// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests against a running sway compositor.
//!
//! These need `SWAYSOCK` pointing at a live sway and the `foot` terminal on
//! `PATH`. They are `#[ignore]`d so a plain `cargo test` stays hermetic. CI starts
//! a headless sway and runs them with:
//! `cargo test -p tpt-workspaces-backend-linux -- --ignored --test-threads=1`

#![cfg(target_os = "linux")]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tpt_workspaces_backend_linux::{ipc, SwayBackend, WindowEvent, WindowEventListener};
use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::model::{WindowGeometry, WindowInfo, WorkspaceId};

/// Starts a terminal with the given app id. The terminal runs `sleep`, so its
/// pid is the window's pid.
fn spawn_terminal(app_id: &str) -> Child {
    Command::new("foot")
        .args(["--app-id", app_id, "sleep", "300"])
        .spawn()
        .expect("failed to start foot; is it installed and is WAYLAND_DISPLAY set?")
}

/// Polls sway until a window with this app id shows up.
fn wait_for_window(backend: &SwayBackend, app_id: &str) -> WindowInfo {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let found = backend
            .list_windows()
            .unwrap()
            .into_iter()
            .find(|w| w.window_class == app_id);
        if let Some(window) = found {
            return window;
        }
        assert!(
            Instant::now() < deadline,
            "window '{app_id}' never appeared"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn process_state(pid: u32) -> char {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("process should exist");
    // Format: "pid (comm) STATE ..." where comm may contain spaces, so split after the last ')'.
    let after = &stat[stat.rfind(')').unwrap() + 2..];
    after.chars().next().unwrap()
}

/// Polls `check` until it returns true or `timeout` passes. Signals and sway
/// commands take effect asynchronously, so a single read right after the call can
/// still see the old state.
fn wait_until(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if check() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn current_workspace(backend: &SwayBackend) -> WorkspaceId {
    backend
        .current_workspace()
        .unwrap()
        .expect("sway should report a focused workspace")
}

#[test]
#[ignore = "needs a running sway"]
fn sway_is_reachable_and_reports_workspaces() {
    let backend = SwayBackend::connect().expect("sway should be reachable");
    let current = current_workspace(&backend);
    assert!(!current.as_str().is_empty());
    let names: Vec<String> = backend
        .enumerate_desktops()
        .unwrap()
        .into_iter()
        .map(|w| w.name)
        .collect();
    assert!(names.contains(&current.as_str().to_string()));
}

#[test]
#[ignore = "needs a running sway and foot"]
fn move_window_between_workspaces_and_back() {
    let mut backend = SwayBackend::connect().unwrap();
    let origin = current_workspace(&backend);
    let mut child = spawn_terminal("tpt-test-move");
    let window = wait_for_window(&backend, "tpt-test-move");
    assert_eq!(
        window.pid,
        child.id(),
        "pid should match the terminal process"
    );
    assert_eq!(window.workspace.as_ref(), Some(&origin));

    let scratch = WorkspaceId::new("tpt-test-scratch");
    backend.move_window(window.id, &scratch).unwrap();
    let moved = wait_for_window(&backend, "tpt-test-move");
    assert_eq!(moved.workspace.as_ref(), Some(&scratch));

    backend.move_window(window.id, &origin).unwrap();
    child.kill().ok();
    child.wait().ok();
}

#[test]
#[ignore = "needs a running sway and foot"]
fn suspend_and_resume_really_stop_the_process() {
    let mut backend = SwayBackend::connect().unwrap();
    let mut child = spawn_terminal("tpt-test-freeze");
    let window = wait_for_window(&backend, "tpt-test-freeze");
    let pid = window.pid;

    backend.suspend_process(pid).unwrap();
    let stopped = wait_until(Duration::from_secs(2), || process_state(pid) == 'T');
    assert!(
        stopped,
        "process should be stopped, state is '{}'",
        process_state(pid)
    );

    backend.resume_process(pid).unwrap();
    let running = wait_until(Duration::from_secs(2), || process_state(pid) != 'T');
    assert!(
        running,
        "process should be running again, state is '{}'",
        process_state(pid)
    );

    child.kill().ok();
    child.wait().ok();
}

#[test]
#[ignore = "needs a running sway and foot"]
fn geometry_is_applied_to_floating_window() {
    let mut backend = SwayBackend::connect().unwrap();
    let mut child = spawn_terminal("tpt-test-geometry");
    let window = wait_for_window(&backend, "tpt-test-geometry");

    let target = WindowGeometry {
        x: 120,
        y: 140,
        width: 480,
        height: 320,
    };
    backend.set_geometry(window.id, target).unwrap();

    // Sway includes window decorations in the container rect, so allow a small slack.
    let close = |a: i32, b: i32| (a - b).abs() <= 8;
    let fits = |g: WindowGeometry| {
        close(g.x, target.x)
            && close(g.y, target.y)
            && close(g.width as i32, target.width as i32)
            && close(g.height as i32, target.height as i32)
    };
    // The client may resize itself (terminals snap to whole cells), so wait for the layout to settle.
    let settled = wait_until(Duration::from_secs(5), || {
        backend.get_geometry(window.id).is_ok_and(fits)
    });
    let actual = backend.get_geometry(window.id).unwrap();
    assert!(settled, "geometry did not settle: {actual:?} vs {target:?}");

    child.kill().ok();
    child.wait().ok();
}

#[test]
#[ignore = "needs a running sway and foot"]
fn events_report_new_windows_and_workspace_focus() {
    let backend = SwayBackend::connect().unwrap();
    let listener = WindowEventListener::start().expect("subscription should succeed");
    let origin = current_workspace(&backend);

    let mut child = spawn_terminal("tpt-test-events");
    let window = wait_for_window(&backend, "tpt-test-events");
    let saw_new = drain_until(
        &listener,
        |e| matches!(e, WindowEvent::Shown(id) if *id == window.id),
    );
    assert!(saw_new, "expected a Shown event for the new window");

    let target = "tpt-test-events-ws";
    ipc::run_command(&format!("workspace {}", ipc::quote(target))).unwrap();
    let saw_focus = drain_until(
        &listener,
        |e| matches!(e, WindowEvent::WorkspaceFocused(ws) if ws.as_str() == target),
    );
    assert!(saw_focus, "expected a WorkspaceFocused event");

    ipc::run_command(&format!("workspace {}", ipc::quote(origin.as_str()))).unwrap();
    child.kill().ok();
    child.wait().ok();
}

/// Reads events for a few seconds and reports whether one matches.
fn drain_until(
    listener: &WindowEventListener,
    mut matches: impl FnMut(&WindowEvent) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(event) = listener.events().recv_timeout(Duration::from_millis(200)) {
            if matches(&event) {
                return true;
            }
        }
    }
    false
}
