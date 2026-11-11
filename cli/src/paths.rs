// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Default locations for the config file and state database.

use std::path::PathBuf;

const APP_DIR: &str = "tpt-workspaces";

/// `config.toml` in the per-user config directory
/// (`%APPDATA%` on Windows, `~/.config` on Linux).
pub fn config_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("config.toml")
}

/// `state.db` in the per-user local data directory
/// (`%LOCALAPPDATA%` on Windows, `~/.local/share` on Linux).
pub fn state_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("state.db")
}
