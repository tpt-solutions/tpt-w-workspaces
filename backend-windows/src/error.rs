// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("Win32 call failed: {0}")]
    Win32(#[from] windows::core::Error),
    #[error("virtual desktop API failed: {0}")]
    Desktop(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("'{0}' is not a valid desktop id")]
    InvalidWorkspace(String),
    #[error("could not install window event hook: {0}")]
    Hook(String),
}
