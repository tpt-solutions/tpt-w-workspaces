// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Model behind the settings tab: the config file as text, validated before it is
//! saved, and the window geometries the daemon has remembered.
//!
//! Nothing here draws anything, so it can be tested without a display.

use std::path::{Path, PathBuf};

use tpt_workspaces_core::config::{self, Config, ConfigError};
use tpt_workspaces_core::sqlite::SqliteStore;
use tpt_workspaces_core::{GeometryKey, WindowGeometry};

pub struct Settings {
    pub config_path: PathBuf,
    pub state_path: PathBuf,
    /// The editor's contents. May differ from the file while the user is editing.
    pub text: String,
    /// What was last read from or written to the file.
    saved_text: String,
    /// Result of the last validate, save or reload: `(is_error, message)`.
    pub message: Option<(bool, String)>,
    pub geometries: Vec<(GeometryKey, WindowGeometry)>,
    pub geometry_error: Option<String>,
}

impl Settings {
    /// Reads the config file and the remembered geometries.
    pub fn open(config_path: PathBuf, state_path: PathBuf) -> Self {
        let mut settings = Settings {
            config_path,
            state_path,
            text: String::new(),
            saved_text: String::new(),
            message: None,
            geometries: Vec::new(),
            geometry_error: None,
        };
        settings.reload();
        settings.refresh_geometries();
        settings
    }

    /// Whether the editor differs from the file on disk.
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Discards edits and reads the file again.
    pub fn reload(&mut self) {
        match std::fs::read_to_string(&self.config_path) {
            Ok(text) => {
                self.text = text.clone();
                self.saved_text = text;
                self.message = Some((false, format!("Loaded {}", self.config_path.display())));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.text = String::new();
                self.saved_text = String::new();
                self.message = Some((
                    false,
                    format!("No config file yet at {}", self.config_path.display()),
                ));
            }
            Err(e) => {
                self.message = Some((true, format!("Could not read the config: {e}")));
            }
        }
    }

    /// Checks the editor's text without touching the file.
    pub fn validate(&mut self) -> Option<Config> {
        match config::parse(&self.text) {
            Ok(cfg) => {
                self.message = Some((
                    false,
                    format!(
                        "Valid: {} workspace(s), {} rule(s)",
                        cfg.workspaces.len(),
                        cfg.rules.len()
                    ),
                ));
                Some(cfg)
            }
            Err(e) => {
                self.message = Some((true, describe(&e)));
                None
            }
        }
    }

    /// Validates the editor's text and, if valid, writes it to the config file.
    /// The daemon reloads the file itself.
    pub fn save(&mut self) -> bool {
        match config::save(&self.config_path, &self.text) {
            Ok(_) => {
                self.saved_text = self.text.clone();
                self.message = Some((
                    false,
                    "Saved. The daemon reloads the config automatically.".into(),
                ));
                true
            }
            Err(e) => {
                self.message = Some((true, describe(&e)));
                false
            }
        }
    }

    /// Reads the geometries the daemon has saved. A missing state file means none yet.
    pub fn refresh_geometries(&mut self) {
        self.geometry_error = None;
        if !self.state_path.exists() {
            self.geometries.clear();
            return;
        }
        match read_geometries(&self.state_path) {
            Ok(list) => self.geometries = list,
            Err(e) => {
                self.geometries.clear();
                self.geometry_error = Some(e);
            }
        }
    }
}

fn read_geometries(path: &Path) -> Result<Vec<(GeometryKey, WindowGeometry)>, String> {
    let store = SqliteStore::open(path).map_err(|e| format!("opening state database: {e}"))?;
    store
        .list_geometries()
        .map_err(|e| format!("reading saved geometries: {e}"))
}

fn describe(error: &ConfigError) -> String {
    format!("Not saved: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-gui-settings-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const VALID: &str =
        "schema_version = 1\n\n[[workspace]]\nid = \"dev\"\nname = \"Development\"\n";

    #[test]
    fn missing_config_opens_as_empty_editor() {
        let dir = temp_dir("missing");
        let settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        assert_eq!(settings.text, "");
        assert!(!settings.dirty());
        assert!(settings.geometries.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn editing_marks_dirty_until_saved() {
        let dir = temp_dir("dirty");
        let path = dir.join("config.toml");
        std::fs::write(&path, VALID).unwrap();
        let mut settings = Settings::open(path.clone(), dir.join("state.db"));

        settings.text.push_str("\n# note\n");
        assert!(settings.dirty());
        assert!(settings.save());
        assert!(!settings.dirty());
        assert!(std::fs::read_to_string(&path).unwrap().contains("# note"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_edit_is_reported_and_not_written() {
        let dir = temp_dir("invalid");
        let path = dir.join("config.toml");
        std::fs::write(&path, VALID).unwrap();
        let mut settings = Settings::open(path.clone(), dir.join("state.db"));

        settings.text = "[[rule]]\nworkspace = \"nope\"\nexecutable = \"x.exe\"\n".into();
        assert!(!settings.save());
        let (is_error, message) = settings.message.clone().unwrap();
        assert!(is_error);
        assert!(message.starts_with("Not saved:"), "{message}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn revert_discards_edits() {
        let dir = temp_dir("revert");
        let path = dir.join("config.toml");
        std::fs::write(&path, VALID).unwrap();
        let mut settings = Settings::open(path, dir.join("state.db"));

        settings.text = "junk".into();
        settings.reload();
        assert_eq!(settings.text, VALID);
        assert!(!settings.dirty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_reports_counts() {
        let dir = temp_dir("validate");
        let mut settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        settings.text = VALID.into();
        let cfg = settings.validate().unwrap();
        assert_eq!(cfg.workspaces.len(), 1);
        assert_eq!(
            settings.message.unwrap().1,
            "Valid: 1 workspace(s), 0 rule(s)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
