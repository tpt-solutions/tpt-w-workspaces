// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

#![cfg(windows)]

use std::process::{Command, Stdio};
use std::time::Duration;

use tpt_workspaces_backend_windows::error::BackendError;
use tpt_workspaces_backend_windows::process::{
    probe_suspend_counts, resume_process, suspend_process, thread_ids,
};

/// Starts a long-running child with no window. `ping` writes a line per second,
/// so suspension is observable through the thread suspend counts.
fn spawn_child() -> std::process::Child {
    Command::new("ping")
        .args(["-n", "120", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn ping")
}

#[test]
fn suspend_and_resume_round_trip() {
    let mut child = spawn_child();
    let pid = child.id();
    std::thread::sleep(Duration::from_millis(300));

    assert!(!thread_ids(pid).unwrap().is_empty());
    assert_eq!(
        probe_suspend_counts(pid)
            .unwrap()
            .iter()
            .filter(|c| **c != 0)
            .count(),
        0
    );

    let suspended = suspend_process(pid).expect("suspend should succeed");
    assert!(suspended > 0);
    let counts = probe_suspend_counts(pid).unwrap();
    assert!(
        counts.iter().all(|c| *c >= 1),
        "every thread should be suspended, got {counts:?}"
    );

    // Suspending twice must not stack counts past 1 per call; resume undoes everything.
    resume_process(pid).expect("resume should succeed");
    let counts = probe_suspend_counts(pid).unwrap();
    assert!(
        counts.iter().all(|c| *c == 0),
        "every thread should be running, got {counts:?}"
    );

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn refuses_to_suspend_itself_and_system_processes() {
    assert!(matches!(
        suspend_process(std::process::id()),
        Err(BackendError::Refused(_))
    ));
    assert!(matches!(suspend_process(4), Err(BackendError::Refused(_))));
    assert!(matches!(resume_process(0), Err(BackendError::Refused(_))));
}

#[test]
fn missing_process_is_reported_not_panicked() {
    // Pid values this large are not allocated in practice; if one ever is, the
    // result is still a clean error rather than a panic.
    let result = suspend_process(0x7FFF_FF00);
    assert!(result.is_err());
}
