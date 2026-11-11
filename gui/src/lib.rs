// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Support code for the tpt-workspaces GUI: a background poller that keeps the
//! latest daemon state for the window to draw.
//!
//! The window never talks to the daemon directly. A slow or hung daemon must
//! not freeze the UI, so requests happen on the poller's thread.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tpt_workspaces_core::ipc::{self, Request, Response, Status};

/// What the window can show about the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    /// No answer yet.
    Connecting,
    Connected(Status),
    /// The daemon is not running, or did not answer properly.
    Offline(String),
}

/// Shared handle to the latest [`Connection`].
pub type SharedState = Arc<Mutex<Connection>>;

pub fn new_state() -> SharedState {
    Arc::new(Mutex::new(Connection::Connecting))
}

/// Asks the daemon for its status once.
pub fn poll_once() -> Connection {
    match ipc::request(&Request::Status) {
        Ok(Response::Status(status)) => Connection::Connected(status),
        Ok(Response::Error { message }) => Connection::Offline(message),
        Ok(Response::Done { count }) => {
            Connection::Offline(format!("unexpected reply: done({count})"))
        }
        Err(message) => Connection::Offline(message),
    }
}

/// Sends a state-changing request (pause or resume) and returns how many processes
/// it affected. Blocks until the daemon answers, so call it off the UI thread.
pub fn run_action(request: Request) -> Result<usize, String> {
    match ipc::request(&request)? {
        Response::Done { count } => Ok(count),
        Response::Status(_) => Err("unexpected reply from daemon".into()),
        Response::Error { message } => Err(message),
    }
}

/// Polls the daemon every `interval` on a background thread, storing each answer
/// in `state`. `on_change` runs after each update, so the window can repaint.
pub fn spawn_poller(
    state: SharedState,
    interval: Duration,
    on_change: impl Fn() + Send + 'static,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("tpt-gui-poller".into())
        .spawn(move || loop {
            let next = poll_once();
            let changed = match state.lock() {
                Ok(mut current) => {
                    let changed = *current != next;
                    *current = next;
                    changed
                }
                Err(_) => return, // the window side panicked; nothing left to update
            };
            if changed {
                on_change();
            }
            thread::sleep(interval);
        })
        .map(|_| ())
}

/// One-line summary used by the window and the tray tooltip.
pub fn summary(connection: &Connection) -> String {
    match connection {
        Connection::Connecting => "Connecting to the daemon…".to_string(),
        Connection::Offline(_) => "Daemon not running".to_string(),
        Connection::Connected(status) => match status.active.as_deref() {
            Some(active) => format!("Active workspace: {active}"),
            None => "Active workspace: (not in config)".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_workspaces_core::ipc::WorkspaceStatus;

    fn status(active: Option<&str>) -> Status {
        Status {
            active: active.map(str::to_string),
            workspaces: vec![WorkspaceStatus {
                id: "dev".into(),
                name: "Development".into(),
                active: active == Some("dev"),
            }],
            rules: 1,
            paused: vec![],
        }
    }

    #[test]
    fn summary_describes_each_state() {
        assert_eq!(
            summary(&Connection::Connecting),
            "Connecting to the daemon…"
        );
        assert_eq!(
            summary(&Connection::Offline("x".into())),
            "Daemon not running"
        );
        assert_eq!(
            summary(&Connection::Connected(status(Some("dev")))),
            "Active workspace: dev"
        );
        assert_eq!(
            summary(&Connection::Connected(status(None))),
            "Active workspace: (not in config)"
        );
    }

    #[test]
    fn new_state_starts_connecting() {
        let state = new_state();
        assert_eq!(*state.lock().unwrap(), Connection::Connecting);
    }
}
