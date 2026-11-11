// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The long-running daemon: wires platform events, config changes and Ctrl+C
//! into one engine loop.

use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;

use tpt_workspaces_core::config;
use tpt_workspaces_core::sqlite::SqliteStore;
use tpt_workspaces_core::{Engine, Event};
use tracing::{debug, error, info, warn};

use crate::platform;
use tpt_workspaces_core::ipc::{self, FrozenProcess, Query, Status, WorkspaceStatus};

enum Message {
    Event(Event),
    Query(Query),
    Shutdown,
}

/// Config id of the workspace the engine is on, and the rest of the status.
fn status(engine: &Engine<platform::Backend, SqliteStore>, names: &[(String, String)]) -> Status {
    let active_backend = engine.state().active.as_ref();
    let active = active_backend.and_then(|active| {
        engine
            .workspace_map()
            .iter()
            .find(|(_, backend)| *backend == active)
            .map(|(config_id, _)| config_id.clone())
    });
    let workspaces = names
        .iter()
        .map(|(id, name)| WorkspaceStatus {
            id: id.clone(),
            name: name.clone(),
            active: active.as_deref() == Some(id.as_str()),
        })
        .collect();
    let mut paused: Vec<FrozenProcess> = engine
        .paused_processes()
        .into_iter()
        .map(|(pid, executable)| FrozenProcess { pid, executable })
        .collect();
    paused.sort_by_key(|p| p.pid);
    Status {
        active,
        workspaces,
        rules: engine.rules().len(),
        paused,
    }
}

/// Events slower than this are logged at warning level, so a stall shows up in the log.
const SLOW_EVENT_MS: u128 = 2000;

fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::WindowAppeared(_) => "window_appeared",
        Event::WindowFocused(_) => "window_focused",
        Event::WorkspaceFocused(_) => "workspace_focused",
        Event::ConfigReloaded(_) => "config_reloaded",
    }
}

fn workspace_names(cfg: &config::Config) -> Vec<(String, String)> {
    cfg.workspaces
        .iter()
        .map(|ws| (ws.id.as_str().to_string(), ws.name.clone()))
        .collect()
}

fn to_event(event: platform::WindowEvent) -> Event {
    match event {
        platform::WindowEvent::Shown(id) => Event::WindowAppeared(id),
        platform::WindowEvent::Foreground(id) => Event::WindowFocused(id),
        platform::WindowEvent::WorkspaceFocused(ws) => Event::WorkspaceFocused(ws),
    }
}

/// Runs until Ctrl+C or SIGTERM, then resumes every frozen process.
pub fn run(config_path: &Path, state_path: &Path) -> Result<(), String> {
    // Two daemons would fight over windows and frozen processes, so refuse to start
    // if one already answers on the IPC channel.
    if ipc::request(&ipc::Request::Status).is_ok() {
        return Err("another tpt-workspaces daemon is already running".into());
    }
    let cfg = config::load(config_path).map_err(|e| e.to_string())?;

    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let store = SqliteStore::open(state_path)
        .map_err(|e| format!("opening state database {}: {e}", state_path.display()))?;

    let backend = platform::connect()?;
    let mut engine = Engine::start(backend, store, &cfg).map_err(|e| e.to_string())?;

    let (tx, rx) = mpsc::channel::<Message>();

    let shutdown_tx = tx.clone();
    ctrlc::set_handler(move || {
        let _ = shutdown_tx.send(Message::Shutdown);
    })
    .map_err(|e| format!("installing signal handler: {e}"))?;

    // The listener owns its threads, so it lives on a forwarder thread. That
    // thread exits with the process.
    let listener = platform::WindowEventListener::start()
        .map_err(|e| format!("starting window events: {e}"))?;
    let events_tx = tx.clone();
    std::thread::Builder::new()
        .name("tpt-event-forwarder".into())
        .spawn(move || {
            for event in listener.events().iter() {
                if events_tx.send(Message::Event(to_event(event))).is_err() {
                    break;
                }
            }
            drop(listener);
        })
        .map_err(|e| e.to_string())?;

    // Status queries from IPC clients. The server runs on its own thread; its
    // requests are forwarded into the main loop so the engine stays single-threaded.
    let (query_tx, query_rx) = mpsc::channel::<Query>();
    let query_forward_tx = tx.clone();
    std::thread::Builder::new()
        .name("tpt-ipc-forwarder".into())
        .spawn(move || {
            for query in query_rx.iter() {
                if query_forward_tx.send(Message::Query(query)).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("tpt-ipc-server".into())
        .spawn(move || {
            if let Err(e) = ipc::serve(query_tx) {
                error!(error = %e, "ipc server stopped; status queries will not work");
            }
        })
        .map_err(|e| e.to_string())?;

    let config_tx = tx.clone();
    let _watcher =
        config::watch::watch_config(config_path.to_path_buf(), move |result| match result {
            Ok(cfg) => {
                let _ = config_tx.send(Message::Event(Event::ConfigReloaded(cfg)));
            }
            Err(e) => warn!(error = %e, "config changed but is invalid; keeping the previous one"),
        })
        .map_err(|e| format!("watching {}: {e}", config_path.display()))?;

    info!(config = %config_path.display(), state = %state_path.display(), "daemon running");
    let mut names: Vec<(String, String)> = workspace_names(&cfg);
    while let Ok(message) = rx.recv() {
        match message {
            Message::Event(event) => {
                if let Event::ConfigReloaded(new) = &event {
                    names = workspace_names(new);
                }
                let kind = event_kind(&event);
                debug!(event = kind, "handling event");
                let started = Instant::now();
                if let Err(e) = engine.handle(event) {
                    error!(error = %e, "failed to handle event");
                }
                let elapsed = started.elapsed();
                if elapsed.as_millis() >= SLOW_EVENT_MS {
                    warn!(
                        event = kind,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "slow event"
                    );
                } else {
                    debug!(
                        event = kind,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "handled event"
                    );
                }
            }
            Message::Query(query) => {
                let response = match query.request {
                    ipc::Request::Status => ipc::Response::Status(status(&engine, &names)),
                    ipc::Request::PauseBackground => match engine.pause_background() {
                        Ok(count) => ipc::Response::Done { count },
                        Err(e) => ipc::Response::Error {
                            message: e.to_string(),
                        },
                    },
                    ipc::Request::ResumeAll => match engine.resume_all() {
                        Ok(count) => ipc::Response::Done { count },
                        Err(e) => ipc::Response::Error {
                            message: e.to_string(),
                        },
                    },
                };
                // The client may have given up already; that is not an error for the daemon.
                let _ = query.reply.send(response);
            }
            Message::Shutdown => break,
        }
    }

    info!("shutting down");
    engine.shutdown().map(|_| ()).map_err(|e| e.to_string())
}
