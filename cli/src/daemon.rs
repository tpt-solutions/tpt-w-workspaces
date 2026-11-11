// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The long-running daemon: wires platform events, config changes and Ctrl+C
//! into one engine loop.

use std::path::Path;
use std::sync::mpsc;

use tpt_workspaces_core::config;
use tpt_workspaces_core::sqlite::SqliteStore;
use tpt_workspaces_core::{Engine, Event};
use tracing::{error, info, warn};

use crate::platform;

enum Message {
    Event(Event),
    Shutdown,
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
    while let Ok(message) = rx.recv() {
        match message {
            Message::Event(event) => {
                if let Err(e) = engine.handle(event) {
                    error!(error = %e, "failed to handle event");
                }
            }
            Message::Shutdown => break,
        }
    }

    info!("shutting down");
    engine.shutdown().map(|_| ()).map_err(|e| e.to_string())
}
