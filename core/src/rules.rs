// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rule-matching engine: maps windows to the workspace they belong on.

use crate::model::{WindowInfo, WorkspaceId};

/// What a rule matches against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
    /// Case-insensitive match on the executable file name.
    Executable(String),
    /// Case-insensitive match on the window class / app id.
    WindowClass(String),
}

impl Matcher {
    pub fn matches(&self, window: &WindowInfo) -> bool {
        match self {
            Matcher::Executable(exe) => exe.eq_ignore_ascii_case(&window.executable),
            Matcher::WindowClass(class) => class.eq_ignore_ascii_case(&window.window_class),
        }
    }
}

/// Routes windows matching `matcher` to the `target` workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRule {
    pub name: Option<String>,
    pub matcher: Matcher,
    pub target: WorkspaceId,
}

impl AppRule {
    pub fn new(matcher: Matcher, target: WorkspaceId) -> Self {
        AppRule {
            name: None,
            matcher,
            target,
        }
    }
}

/// Returns the first rule (in declaration order) that matches `window`.
/// Earlier rules take priority over later ones.
pub fn match_window<'a>(rules: &'a [AppRule], window: &WindowInfo) -> Option<&'a AppRule> {
    rules.iter().find(|rule| rule.matcher.matches(window))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{WindowId, WindowInfo};

    fn window(exe: &str, class: &str) -> WindowInfo {
        WindowInfo {
            id: WindowId(1),
            pid: 100,
            executable: exe.to_string(),
            window_class: class.to_string(),
            title: String::new(),
            workspace: None,
            geometry: None,
        }
    }

    #[test]
    fn executable_match_is_case_insensitive() {
        let m = Matcher::Executable("Code.exe".into());
        assert!(m.matches(&window("CODE.EXE", "")));
        assert!(m.matches(&window("code.exe", "")));
        assert!(!m.matches(&window("code.exe.bak", "")));
    }

    #[test]
    fn window_class_match_is_case_insensitive() {
        let m = Matcher::WindowClass("org.mozilla.firefox".into());
        assert!(m.matches(&window("firefox", "Org.Mozilla.Firefox")));
        assert!(!m.matches(&window("firefox", "other")));
    }

    #[test]
    fn first_matching_rule_wins() {
        let rules = vec![
            AppRule::new(
                Matcher::Executable("code.exe".into()),
                WorkspaceId::new("dev"),
            ),
            AppRule::new(
                Matcher::Executable("code.exe".into()),
                WorkspaceId::new("later"),
            ),
            AppRule::new(
                Matcher::WindowClass("slack".into()),
                WorkspaceId::new("chat"),
            ),
        ];
        let hit = match_window(&rules, &window("Code.exe", "")).unwrap();
        assert_eq!(hit.target, WorkspaceId::new("dev"));
    }

    #[test]
    fn no_match_returns_none() {
        let rules = vec![AppRule::new(
            Matcher::Executable("code.exe".into()),
            WorkspaceId::new("dev"),
        )];
        assert!(match_window(&rules, &window("notepad.exe", "Notepad")).is_none());
        assert!(match_window(&[], &window("code.exe", "")).is_none());
    }
}
