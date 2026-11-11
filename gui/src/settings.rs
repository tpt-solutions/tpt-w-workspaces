// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Model behind the settings tab: the config file as text, validated before it is
//! saved, and the window geometries the daemon has remembered.
//!
//! Nothing here draws anything, so it can be tested without a display.

use std::path::{Path, PathBuf};

use tpt_workspaces_core::config::{
    self, Config, ConfigError, ConfigFile, RuleEntry, WorkspaceEntry,
};
use tpt_workspaces_core::sqlite::SqliteStore;
use tpt_workspaces_core::{GeometryKey, WindowGeometry};

pub struct Settings {
    /// Workspaces and rules as fields, for the form. Applied to the text on request.
    pub workspaces: Vec<WorkspaceEntry>,
    pub rules: Vec<RuleEntry>,
    /// Whether the form has edits that Apply has not yet written to the text.
    pub form_dirty: bool,
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
            workspaces: Vec::new(),
            rules: Vec::new(),
            form_dirty: false,
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

    /// Reads the form's fields from the text. Text that does not parse leaves the
    /// form as it was and reports the error.
    pub fn load_form(&mut self) {
        match config::parse_entries(&self.text) {
            Ok(file) => {
                self.workspaces = file.workspaces;
                self.rules = file.rules;
                self.form_dirty = false;
            }
            Err(e) => self.message = Some((true, format!("Form not loaded: {e}"))),
        }
    }

    /// Writes the form's fields into the text. The text is not saved until Save.
    pub fn apply_form(&mut self) {
        let file = ConfigFile {
            schema_version: config::CURRENT_SCHEMA_VERSION,
            workspaces: self.workspaces.clone(),
            rules: self.rules.clone(),
        };
        match config::render(&file) {
            Ok(text) => {
                self.text = text;
                self.form_dirty = false;
                self.message = Some((
                    false,
                    "Applied the form. Validate and Save to keep it.".into(),
                ));
            }
            Err(e) => self.message = Some((true, format!("Could not write the form: {e}"))),
        }
    }

    pub fn add_workspace(&mut self) {
        let n = self.workspaces.len() + 1;
        self.workspaces.push(WorkspaceEntry {
            id: format!("workspace-{n}"),
            name: Some(format!("Workspace {n}")),
        });
        self.form_dirty = true;
    }

    pub fn remove_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.workspaces.remove(index);
            self.form_dirty = true;
        }
    }

    pub fn add_rule(&mut self) {
        let workspace = self
            .workspaces
            .first()
            .map(|w| w.id.clone())
            .unwrap_or_default();
        self.rules.push(RuleEntry {
            name: None,
            executable: Some("app.exe".into()),
            window_class: None,
            workspace,
        });
        self.form_dirty = true;
    }

    pub fn remove_rule(&mut self, index: usize) {
        if index < self.rules.len() {
            self.rules.remove(index);
            self.form_dirty = true;
        }
    }

    /// Whether the editor differs from the file on disk.
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Discards edits and reads the file again, including the form.
    pub fn reload(&mut self) {
        match std::fs::read_to_string(&self.config_path) {
            Ok(text) => {
                self.text = text.clone();
                self.saved_text = text;
                self.message = Some((false, format!("Loaded {}", self.config_path.display())));
                self.load_form();
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

    /// Appends a commented-out-free workspace block for the user to fill in.
    /// Nothing is saved until the user validates and saves.
    pub fn add_workspace_template(&mut self) {
        append_block(
            &mut self.text,
            "[[workspace]]
id = \"new-workspace\"
name = \"New workspace\"
",
        );
        self.message = Some((
            false,
            "Added a workspace. Edit it, then Validate and Save.".into(),
        ));
    }

    /// Appends a rule block that refers to the first workspace in the file, or a
    /// placeholder if there is none.
    pub fn add_rule_template(&mut self) {
        let target = first_workspace_id(&self.text).unwrap_or_else(|| "new-workspace".into());
        append_block(
            &mut self.text,
            &format!(
                "[[rule]]
name = \"New rule\"
executable = \"app.exe\"
workspace = \"{target}\"
"
            ),
        );
        self.message = Some((
            false,
            "Added a rule. Edit it, then Validate and Save.".into(),
        ));
    }

    /// Forgets a saved window position. The app is placed normally next time.
    pub fn forget_geometry(&mut self, key: &GeometryKey) {
        match SqliteStore::open(&self.state_path)
            .map_err(|e| e.to_string())
            .and_then(|store| store.delete_geometry(key).map_err(|e| e.to_string()))
        {
            Ok(_) => {
                self.message = Some((false, format!("Forgot the position of {}", key.executable)));
                self.refresh_geometries();
            }
            Err(e) => self.message = Some((true, format!("Could not forget the position: {e}"))),
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

/// Adds `block` after the existing text, separated by a blank line.
fn append_block(text: &mut String, block: &str) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(block);
}

/// The id of the first `[[workspace]]` block, found by a simple line scan. Used only
/// to suggest a default target; validation is still done by the core parser.
fn first_workspace_id(text: &str) -> Option<String> {
    let mut in_workspace = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[workspace]]" {
            in_workspace = true;
        } else if line.starts_with("[[") {
            in_workspace = false;
        } else if in_workspace {
            if let Some(rest) = line.strip_prefix("id") {
                if let Some(value) = rest.trim_start().strip_prefix('=') {
                    return Some(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
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

    #[test]
    fn templates_append_blocks_that_validate_once_filled_in() {
        let dir = temp_dir("templates");
        let mut settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        settings.text = VALID.into();

        settings.add_rule_template();
        assert!(
            settings.text.contains("workspace = \"dev\""),
            "{}",
            settings.text
        );
        assert!(settings.validate().is_some());

        settings.add_workspace_template();
        assert!(settings.text.contains("new-workspace"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn forget_removes_a_saved_position() {
        use tpt_workspaces_core::sqlite::SqliteStore;
        use tpt_workspaces_core::{GeometryKey, StateStore, WindowGeometry, WorkspaceId};

        let dir = temp_dir("forget");
        let state = dir.join("state.db");
        let key = GeometryKey {
            workspace: WorkspaceId::new("dev"),
            executable: "code.exe".into(),
        };
        {
            let mut store = SqliteStore::open(&state).unwrap();
            store
                .save_geometry(
                    &key,
                    WindowGeometry {
                        x: 1,
                        y: 2,
                        width: 3,
                        height: 4,
                    },
                )
                .unwrap();
        }
        let mut settings = Settings::open(dir.join("config.toml"), state);
        assert_eq!(settings.geometries.len(), 1);

        settings.forget_geometry(&key);
        assert!(settings.geometries.is_empty());
        assert!(!settings.message.unwrap().0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn form_applies_to_text_that_validates() {
        let dir = temp_dir("form");
        let mut settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        settings.text = VALID.into();
        settings.load_form();
        assert_eq!(settings.workspaces.len(), 1);

        settings.add_rule();
        assert!(settings.form_dirty);
        settings.rules[0].executable = Some("Code.exe".into());
        settings.apply_form();
        assert!(!settings.form_dirty);
        let cfg = settings.validate().expect("applied form is valid");
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].target.as_str(), "dev");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn form_adds_and_removes_entries() {
        let dir = temp_dir("form-ops");
        let mut settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        settings.add_workspace();
        settings.add_workspace();
        settings.remove_workspace(0);
        assert_eq!(settings.workspaces.len(), 1);
        settings.add_rule();
        settings.remove_rule(0);
        assert!(settings.rules.is_empty());
        settings.remove_rule(9); // out of range: no effect, no panic
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn form_does_not_load_text_it_cannot_parse() {
        let dir = temp_dir("form-bad");
        let mut settings = Settings::open(dir.join("config.toml"), dir.join("state.db"));
        settings.add_workspace();
        settings.text = "this is = not [valid toml".into();
        settings.load_form();
        assert_eq!(settings.workspaces.len(), 1, "form kept its fields");
        assert!(settings.message.unwrap().0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
