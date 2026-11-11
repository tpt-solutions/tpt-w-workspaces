// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Default locations for the config file and state database.

use std::path::PathBuf;

const APP_DIR: &str = "tpt-workspaces";

/// Environment variable that overrides the config file location.
pub const CONFIG_ENV: &str = "TPT_WORKSPACES_CONFIG";
/// Environment variable that overrides the state database location.
pub const STATE_ENV: &str = "TPT_WORKSPACES_STATE";

/// `config.toml` in the per-user config directory
/// (`%APPDATA%` on Windows, `~/.config` on Linux). `TPT_WORKSPACES_CONFIG` overrides it.
pub fn config_file() -> PathBuf {
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("config.toml")
}

/// `state.db` in the per-user local data directory
/// (`%LOCALAPPDATA%` on Windows, `~/.local/share` on Linux). `TPT_WORKSPACES_STATE` overrides it.
pub fn state_file() -> PathBuf {
    if let Some(path) = std::env::var_os(STATE_ENV) {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("state.db")
}
