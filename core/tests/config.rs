// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

use tpt_workspaces_core::config::{parse, ConfigError};
use tpt_workspaces_core::rules::Matcher;
use tpt_workspaces_core::WorkspaceId;

const VALID: &str = r#"
schema_version = 1

[[workspace]]
id = "dev"
name = "Development"

[[workspace]]
id = "chat"

[[rule]]
name = "Editor"
executable = "Code.exe"
workspace = "dev"

[[rule]]
window_class = "Slack"
workspace = "chat"
"#;

#[test]
fn parses_valid_config() {
    let cfg = parse(VALID).unwrap();
    assert_eq!(cfg.workspaces.len(), 2);
    assert_eq!(cfg.workspaces[0].name, "Development");
    // Missing name falls back to the id.
    assert_eq!(cfg.workspaces[1].name, "chat");
    assert_eq!(cfg.rules.len(), 2);
    assert_eq!(cfg.rules[0].matcher, Matcher::Executable("Code.exe".into()));
    assert_eq!(cfg.rules[0].target, WorkspaceId::new("dev"));
    assert_eq!(cfg.rules[1].matcher, Matcher::WindowClass("Slack".into()));
}

#[test]
fn missing_schema_version_defaults_to_current() {
    let cfg = parse("[[workspace]]\nid = \"a\"\n").unwrap();
    assert_eq!(cfg.workspaces.len(), 1);
}

#[test]
fn rejects_newer_schema_version() {
    let err = parse("schema_version = 99\n").unwrap_err();
    assert!(matches!(
        err,
        ConfigError::UnsupportedSchema { found: 99, .. }
    ));
}

#[test]
fn rejects_unknown_keys() {
    assert!(matches!(
        parse("[[workspace]]\nid = \"a\"\ncolour = \"red\"\n"),
        Err(ConfigError::Parse(_))
    ));
}

fn invalid_messages(src: &str) -> Vec<String> {
    match parse(src) {
        Err(ConfigError::Invalid(msgs)) => msgs,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn collects_all_validation_errors() {
    let msgs = invalid_messages(
        r#"
[[workspace]]
id = "dev"
[[workspace]]
id = "dev"
[[workspace]]
id = "  "

[[rule]]
executable = "a.exe"
window_class = "b"
workspace = "dev"

[[rule]]
executable = "C:\\bin\\x.exe"
workspace = "dev"

[[rule]]
executable = "x.exe"
workspace = "nope"

[[rule]]
name = "empty"
workspace = "dev"
"#,
    );
    let joined = msgs.join("\n");
    assert!(joined.contains("duplicate workspace id 'dev'"), "{joined}");
    assert!(
        joined.contains("workspace id must not be empty"),
        "{joined}"
    );
    assert!(joined.contains("rule #1: exactly one of"), "{joined}");
    assert!(joined.contains("path separators"), "{joined}");
    assert!(joined.contains("unknown workspace 'nope'"), "{joined}");
    assert!(joined.contains("empty: exactly one of"), "{joined}");
    assert_eq!(msgs.len(), 6, "{joined}");
}

#[test]
fn rule_pointing_at_unknown_workspace_is_rejected() {
    let msgs = invalid_messages("[[rule]]\nexecutable = \"a.exe\"\nworkspace = \"ghost\"\n");
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("ghost"));
}

#[test]
fn load_reports_missing_file() {
    let path = std::env::temp_dir().join("tpt-workspaces-does-not-exist.toml");
    let err = tpt_workspaces_core::config::load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Io { .. }));
}
