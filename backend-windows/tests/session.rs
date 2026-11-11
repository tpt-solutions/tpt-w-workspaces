// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests that touch the live desktop session.
//!
//! The non-ignored tests are safe to run anywhere with a desktop session.
//! The `#[ignore]` tests create or move real desktops and windows, so run them
//! deliberately with: `cargo test -p tpt-workspaces-backend-windows -- --ignored --test-threads=1`

#![cfg(windows)]

use std::process::Command;
use std::time::Duration;

use tpt_workspaces_backend_windows::{WindowEvent, WindowEventListener, WindowsBackend};
use tpt_workspaces_core::backend::DesktopBackend;
use tpt_workspaces_core::model::{WindowGeometry, Workspace};

#[test]
fn enumerating_windows_and_desktops_succeeds() {
    let backend = WindowsBackend::new();
    backend
        .list_windows()
        .expect("list_windows should not fail");
    let desktops: Vec<Workspace> = backend
        .enumerate_desktops()
        .expect("enumerate_desktops should succeed on Windows 11 24H2+");
    assert!(!desktops.is_empty());
    for d in &desktops {
        assert!(d.id.as_str().starts_with('{') && d.id.as_str().ends_with('}'));
        assert!(!d.name.is_empty());
    }
}

#[test]
fn event_listener_starts_and_stops_cleanly() {
    let listener = WindowEventListener::start().expect("hooks should install");
    // Nothing is guaranteed to happen in a short window; we only check the plumbing.
    let _ = listener.events().recv_timeout(Duration::from_millis(50));
    drop(listener);
}

#[test]
#[ignore = "moves the real foreground window; run deliberately"]
fn notepad_geometry_round_trip_and_event() {
    let listener = WindowEventListener::start().unwrap();
    let mut child = Command::new("notepad.exe").spawn().expect("spawn notepad");
    std::thread::sleep(Duration::from_millis(1500));

    let mut backend = WindowsBackend::new();
    let windows = backend.list_windows().unwrap();
    let notepad = windows
        .iter()
        .find(|w| w.executable.eq_ignore_ascii_case("notepad.exe"))
        .expect("notepad window should be listed");

    let target = WindowGeometry {
        x: 120,
        y: 140,
        width: 640,
        height: 480,
    };
    backend.set_geometry(notepad.id, target).unwrap();
    assert_eq!(backend.get_geometry(notepad.id).unwrap(), target);

    let saw_event = std::iter::from_fn(|| listener.events().try_recv().ok())
        .any(|e| matches!(e, WindowEvent::Shown(_) | WindowEvent::Foreground(_)));
    assert!(saw_event, "expected a show or foreground event for notepad");

    child.kill().ok();
    child.wait().ok();
}

#[test]
#[ignore = "creates and removes a real virtual desktop"]
fn create_and_remove_desktop() {
    let mut backend = WindowsBackend::new();
    let id = backend.create_desktop("tpt-workspaces test").unwrap();

    let desktops = backend.enumerate_desktops().unwrap();
    assert!(desktops
        .iter()
        .any(|d| d.id == id && d.name == "tpt-workspaces test"));

    let desktop_guid = tpt_workspaces_backend_windows::guid::parse(id.as_str()).unwrap();
    let current = winvd::get_current_desktop().unwrap();
    winvd::remove_desktop(desktop_guid, current.get_id().unwrap()).unwrap();
}

#[test]
#[ignore = "moves a real window to another virtual desktop and back"]
fn move_window_between_desktops() {
    let mut backend = WindowsBackend::new();
    let mut child = Command::new("notepad.exe").spawn().expect("spawn notepad");
    std::thread::sleep(Duration::from_millis(1500));

    let window = backend
        .list_windows()
        .unwrap()
        .into_iter()
        .find(|w| w.executable.eq_ignore_ascii_case("notepad.exe"))
        .expect("notepad window should be listed");
    let original = window
        .workspace
        .clone()
        .expect("window should have a desktop");

    let scratch = backend.create_desktop("tpt-workspaces move test").unwrap();
    backend.move_window(window.id, &scratch).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let moved = backend
        .list_windows()
        .unwrap()
        .into_iter()
        .find(|w| w.id == window.id)
        .expect("window should still be listed after move");
    assert_eq!(moved.workspace, Some(scratch.clone()));

    backend.move_window(window.id, &original).unwrap();
    let desktop_guid = tpt_workspaces_backend_windows::guid::parse(scratch.as_str()).unwrap();
    let current = winvd::get_current_desktop().unwrap();
    winvd::remove_desktop(desktop_guid, current.get_id().unwrap()).unwrap();
    child.kill().ok();
    child.wait().ok();
}

#[test]
#[ignore = "launches the Windows Calculator (a UWP app) and needs a desktop session"]
fn uwp_window_resolves_to_real_executable() {
    let _ = Command::new("calc.exe").spawn();
    std::thread::sleep(Duration::from_millis(2500));

    let backend = WindowsBackend::new();
    let found = backend
        .list_windows()
        .unwrap()
        .into_iter()
        .any(|w| w.executable.eq_ignore_ascii_case("CalculatorApp.exe"));

    let _ = Command::new("taskkill")
        .args(["/IM", "CalculatorApp.exe", "/F"])
        .status();
    assert!(
        found,
        "UWP calculator should resolve to CalculatorApp.exe, not ApplicationFrameHost.exe"
    );
}

#[test]
#[ignore = "briefly switches the visible virtual desktop and back"]
fn desktop_switch_emits_workspace_focused_even_when_empty() {
    use tpt_workspaces_backend_windows::{guid, WindowEvent, WindowEventListener};

    let mut backend = WindowsBackend::new();
    let origin = backend
        .current_workspace()
        .unwrap()
        .expect("a current desktop");
    let listener = WindowEventListener::start().unwrap();

    let scratch = backend
        .create_desktop("tpt-workspaces switch test")
        .unwrap();
    let scratch_guid = guid::parse(scratch.as_str()).unwrap();
    let origin_guid = guid::parse(origin.as_str()).unwrap();

    winvd::switch_desktop(scratch_guid).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut seen = false;
    while std::time::Instant::now() < deadline && !seen {
        if let Ok(WindowEvent::WorkspaceFocused(ws)) =
            listener.events().recv_timeout(Duration::from_millis(200))
        {
            seen = ws == scratch;
        }
    }

    winvd::switch_desktop(origin_guid).unwrap();
    winvd::remove_desktop(scratch_guid, origin_guid).unwrap();
    assert!(
        seen,
        "expected a WorkspaceFocused event for the scratch desktop"
    );
}
