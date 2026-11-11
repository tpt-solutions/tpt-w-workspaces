// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Window and workspace events.
//!
//! * Window creation and foreground changes come from `SetWinEventHook`. Hooks
//!   run on a dedicated thread with a message loop, which the out-of-context
//!   hook mechanism requires.
//! * Desktop switches come from `winvd`, so switching to an empty desktop is
//!   still noticed.

use std::sync::{mpsc, Mutex};
use std::thread::JoinHandle;

use tpt_workspaces_core::model::{WindowId, WorkspaceId};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_QUIT,
};

use crate::error::BackendError;
use crate::{guid, window::window_id};

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_OBJECT_SHOW: u32 = 0x8002;
const OBJID_WINDOW: i32 = 0;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// A window became visible; it may be a newly created one.
    Shown(WindowId),
    /// A window became the foreground window.
    Foreground(WindowId),
    /// The user switched to a different virtual desktop.
    WorkspaceFocused(WorkspaceId),
}

/// Read by the hook callback. Set while a listener is running.
static SENDER: Mutex<Option<mpsc::Sender<WindowEvent>>> = Mutex::new(None);

/// Owns the event sources. Dropping it stops them and joins their threads.
pub struct WindowEventListener {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    desktop_events: Option<winvd::DesktopEventThread>,
    desktop_forwarder: Option<JoinHandle<()>>,
    receiver: mpsc::Receiver<WindowEvent>,
}

impl WindowEventListener {
    /// Starts listening. Only one listener may run at a time in this process.
    pub fn start() -> Result<Self, BackendError> {
        let (tx, rx) = mpsc::channel();

        let (desktop_tx, desktop_rx) = mpsc::channel::<winvd::DesktopEvent>();
        let desktop_events = winvd::listen_desktop_events(desktop_tx)
            .map_err(|e| BackendError::Desktop(format!("{e:?}")))?;
        let forward_tx = tx.clone();
        let desktop_forwarder = std::thread::Builder::new()
            .name("tpt-desktop-events".into())
            .spawn(move || {
                for event in desktop_rx {
                    if let winvd::DesktopEvent::DesktopChanged { new, .. } = event {
                        if let Ok(id) = new.get_id() {
                            let workspace = WorkspaceId::new(guid::format(&id));
                            if forward_tx
                                .send(WindowEvent::WorkspaceFocused(workspace))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| BackendError::Hook(e.to_string()))?;

        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let thread = std::thread::Builder::new()
            .name("tpt-window-events".into())
            .spawn(move || run_hook_thread(tx, ready_tx))
            .map_err(|e| BackendError::Hook(e.to_string()))?;

        let thread_id = ready_rx
            .recv()
            .map_err(|_| BackendError::Hook("hook thread exited early".into()))?
            .map_err(BackendError::Hook)?;

        Ok(WindowEventListener {
            thread_id,
            thread: Some(thread),
            desktop_events: Some(desktop_events),
            desktop_forwarder: Some(desktop_forwarder),
            receiver: rx,
        })
    }

    /// Channel of events. Blocks in `recv` or polls with `try_recv`.
    pub fn events(&self) -> &mpsc::Receiver<WindowEvent> {
        &self.receiver
    }
}

impl Drop for WindowEventListener {
    fn drop(&mut self) {
        // Dropping the winvd listener closes its channel, which ends the forwarder.
        drop(self.desktop_events.take());
        if let Some(handle) = self.desktop_forwarder.take() {
            let _ = handle.join();
        }
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_hook_thread(tx: mpsc::Sender<WindowEvent>, ready: mpsc::Sender<Result<u32, String>>) {
    if let Ok(mut sender) = SENDER.lock() {
        *sender = Some(tx);
    }

    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    let (foreground, show) = unsafe {
        (
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(hook_callback),
                0,
                0,
                flags,
            ),
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(hook_callback),
                0,
                0,
                flags,
            ),
        )
    };

    if foreground.is_invalid() || show.is_invalid() {
        unhook(foreground);
        unhook(show);
        clear_sender();
        let _ = ready.send(Err("SetWinEventHook returned no hook".into()));
        return;
    }

    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));

    unsafe {
        let mut msg = MSG::default();
        // GetMessageW returns 0 on WM_QUIT and -1 on error; both end the loop.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unhook(foreground);
    unhook(show);
    clear_sender();
}

fn unhook(hook: HWINEVENTHOOK) {
    if !hook.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
    }
}

fn clear_sender() {
    if let Ok(mut sender) = SENDER.lock() {
        *sender = None;
    }
}

unsafe extern "system" fn hook_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread_id: u32,
    _time_ms: u32,
) {
    // Only top-level window events; child objects (buttons, panes) are noise here.
    if id_object != OBJID_WINDOW || hwnd.0.is_null() {
        return;
    }
    let id = window_id(hwnd);
    let mapped = match event {
        EVENT_OBJECT_SHOW => WindowEvent::Shown(id),
        EVENT_SYSTEM_FOREGROUND => WindowEvent::Foreground(id),
        _ => return,
    };
    if let Ok(guard) = SENDER.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(mapped);
        }
    }
}
