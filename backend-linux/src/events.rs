// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Window and workspace events from sway's IPC subscription.

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use serde_json::Value;

use tpt_workspaces_core::model::{WindowId, WorkspaceId};

use crate::error::BackendError;
use crate::ipc::{self, EVENT_FLAG, SUBSCRIBE};

const WORKSPACE_EVENT: u32 = 0;
const WINDOW_EVENT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// A window was created.
    Shown(WindowId),
    /// A window became focused.
    Foreground(WindowId),
    /// A workspace became focused.
    WorkspaceFocused(WorkspaceId),
}

/// Owns the subscription connection and its reader thread. Dropping it closes the
/// connection and joins the thread.
pub struct WindowEventListener {
    stream: UnixStream,
    thread: Option<JoinHandle<()>>,
    receiver: Receiver<WindowEvent>,
}

impl WindowEventListener {
    pub fn start() -> Result<Self, BackendError> {
        let mut stream = UnixStream::connect(ipc::socket_path()?)
            .map_err(|e| BackendError::NotRunning(e.to_string()))?;
        ipc::write_message(&mut stream, SUBSCRIBE, br#"["window","workspace"]"#)?;
        let (reply_type, body) = ipc::read_message(&mut stream)?;
        let reply: Value = serde_json::from_slice(&body)?;
        if reply_type != SUBSCRIBE || reply["success"].as_bool() != Some(true) {
            return Err(BackendError::Ipc(
                "sway rejected the event subscription".into(),
            ));
        }

        let reader = stream.try_clone()?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("tpt-sway-events".into())
            .spawn(move || read_events(reader, tx))?;

        Ok(WindowEventListener {
            stream,
            thread: Some(thread),
            receiver: rx,
        })
    }

    pub fn events(&self) -> &Receiver<WindowEvent> {
        &self.receiver
    }
}

impl Drop for WindowEventListener {
    fn drop(&mut self) {
        // Shutting the socket down unblocks the reader thread.
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_events(mut stream: UnixStream, tx: Sender<WindowEvent>) {
    while let Ok((msg_type, body)) = ipc::read_message(&mut stream) {
        if msg_type & EVENT_FLAG == 0 {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let Some(mapped) = map_event(msg_type & !EVENT_FLAG, &event) else {
            continue;
        };
        if tx.send(mapped).is_err() {
            break;
        }
    }
}

/// Turns one raw sway event into a [`WindowEvent`], ignoring changes we do not use.
pub fn map_event(kind: u32, event: &Value) -> Option<WindowEvent> {
    match kind {
        WINDOW_EVENT => {
            let id = WindowId(u64::try_from(event["container"]["id"].as_i64()?).ok()?);
            match event["change"].as_str()? {
                "new" => Some(WindowEvent::Shown(id)),
                "focus" => Some(WindowEvent::Foreground(id)),
                _ => None,
            }
        }
        WORKSPACE_EVENT => {
            if event["change"].as_str()? != "focus" {
                return None;
            }
            let name = event["current"]["name"].as_str()?;
            Some(WindowEvent::WorkspaceFocused(WorkspaceId::new(name)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_window_and_workspace_events() {
        let new = json!({"change": "new", "container": {"id": 7}});
        assert_eq!(
            map_event(WINDOW_EVENT, &new),
            Some(WindowEvent::Shown(WindowId(7)))
        );

        let focus = json!({"change": "focus", "container": {"id": 8}});
        assert_eq!(
            map_event(WINDOW_EVENT, &focus),
            Some(WindowEvent::Foreground(WindowId(8)))
        );

        let ws = json!({"change": "focus", "current": {"name": "chat"}});
        assert_eq!(
            map_event(WORKSPACE_EVENT, &ws),
            Some(WindowEvent::WorkspaceFocused(WorkspaceId::new("chat")))
        );
    }

    #[test]
    fn ignores_other_changes() {
        let close = json!({"change": "close", "container": {"id": 7}});
        assert_eq!(map_event(WINDOW_EVENT, &close), None);
        let init = json!({"change": "init", "current": {"name": "x"}});
        assert_eq!(map_event(WORKSPACE_EVENT, &init), None);
        assert_eq!(map_event(42, &json!({})), None);
    }
}
