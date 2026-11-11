// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! User-editable TOML configuration: parsing, validation and (optionally) live reload.
//!
//! Example:
//! ```toml
//! schema_version = 1
//!
//! [[workspace]]
//! id = "dev"
//! name = "Development"
//!
//! [[rule]]
//! name = "Editor"
//! executable = "Code.exe"
//! workspace = "dev"
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Workspace, WorkspaceId};
use crate::rules::{AppRule, Matcher};

/// Version of the TOML schema this build understands. Bump it when the format
/// changes, and add an upgrade path for older files.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Raw on-disk representation, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, rename = "workspace")]
    pub workspaces: Vec<WorkspaceEntry>,
    #[serde(default, rename = "rule")]
    pub rules: Vec<RuleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEntry {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleEntry {
    pub name: Option<String>,
    /// Exactly one of `executable` or `window_class` must be set.
    pub executable: Option<String>,
    pub window_class: Option<String>,
    pub workspace: String,
}

/// A parsed and validated configuration, ready for use by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub workspaces: Vec<Workspace>,
    pub rules: Vec<AppRule>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("config schema version {found} is newer than supported version {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("invalid config:\n  - {}", .0.join("\n  - "))]
    Invalid(Vec<String>),
}

/// Parses and validates config text. All validation problems are collected
/// and reported together.
pub fn parse(source: &str) -> Result<Config, ConfigError> {
    let file: ConfigFile = toml::from_str(source)?;
    validate(file)
}

/// Reads, parses and validates the config file at `path`.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let source = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&source)
}

fn validate(file: ConfigFile) -> Result<Config, ConfigError> {
    if file.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema {
            found: file.schema_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    let mut errors = Vec::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut workspaces = Vec::new();

    for ws in &file.workspaces {
        let id = ws.id.trim();
        if id.is_empty() {
            errors.push("workspace id must not be empty".to_string());
            continue;
        }
        if !seen_ids.insert(id) {
            errors.push(format!("duplicate workspace id '{id}'"));
            continue;
        }
        workspaces.push(Workspace {
            id: WorkspaceId::new(id),
            name: ws
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| id.to_string()),
        });
    }

    let known: HashSet<&str> = workspaces.iter().map(|w| w.id.as_str()).collect();
    let mut rules = Vec::new();

    for (index, rule) in file.rules.iter().enumerate() {
        let label = rule
            .name
            .clone()
            .unwrap_or_else(|| format!("rule #{}", index + 1));
        let target = rule.workspace.trim();

        let matcher = match (&rule.executable, &rule.window_class) {
            (Some(_), Some(_)) | (None, None) => {
                errors.push(format!(
                    "{label}: exactly one of 'executable' or 'window_class' must be set"
                ));
                None
            }
            (Some(exe), None) => {
                let exe = exe.trim();
                if exe.is_empty() || exe.contains(['/', '\\']) {
                    errors.push(format!(
                        "{label}: 'executable' must be a file name without path separators"
                    ));
                    None
                } else {
                    Some(Matcher::Executable(exe.to_string()))
                }
            }
            (None, Some(class)) => {
                let class = class.trim();
                if class.is_empty() {
                    errors.push(format!("{label}: 'window_class' must not be empty"));
                    None
                } else {
                    Some(Matcher::WindowClass(class.to_string()))
                }
            }
        };

        let target_known = known.contains(target);
        if !target_known {
            errors.push(format!("{label}: unknown workspace '{target}'"));
        }

        if let (Some(matcher), true) = (matcher, target_known) {
            rules.push(AppRule {
                name: rule.name.clone(),
                matcher,
                target: WorkspaceId::new(target),
            });
        }
    }

    if !errors.is_empty() {
        return Err(ConfigError::Invalid(errors));
    }
    Ok(Config { workspaces, rules })
}

#[cfg(feature = "watch")]
pub mod watch {
    //! Live reload: re-parses the config whenever the file changes on disk.

    use super::{load, Config, ConfigError};
    use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// Starts watching `path` and calls `on_change` with the freshly loaded
    /// config after each change. The returned watcher must be kept alive.
    ///
    /// The parent directory is watched rather than the file itself, so editors
    /// that save via rename-and-replace are still detected.
    pub fn watch_config<F>(path: PathBuf, on_change: F) -> notify::Result<RecommendedWatcher>
    where
        F: Fn(Result<Config, ConfigError>) + Send + 'static,
    {
        let target_name: Option<OsString> = path.file_name().map(|n| n.to_owned());
        let reload_path = path.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            if !(event.kind.is_modify() || event.kind.is_create()) {
                return;
            }
            let touches_config = event
                .paths
                .iter()
                .any(|p| p.file_name().map(|n| n.to_owned()) == target_name);
            if touches_config {
                on_change(load(&reload_path));
            }
        })?;

        let parent = path
            .parent()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        watcher.watch(&parent, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    }
}
