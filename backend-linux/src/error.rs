// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("sway is not reachable: {0}")]
    NotRunning(String),
    #[error("I/O error talking to sway: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed JSON from sway: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected IPC reply: {0}")]
    Ipc(String),
    #[error("sway rejected '{command}': {message}")]
    Command { command: String, message: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("refused: {0}")]
    Refused(String),
}
