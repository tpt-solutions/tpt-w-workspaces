// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform-agnostic core of tpt-workspaces: domain model, rule matching,
//! workspace transitions, configuration and storage.

pub mod backend;
pub mod config;
pub mod engine;
pub mod ipc;
pub mod model;
pub mod rules;
pub mod storage;
pub mod transition;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use backend::DesktopBackend;
pub use engine::{Engine, EngineError, Event};
pub use model::{WindowGeometry, WindowId, WindowInfo, Workspace, WorkspaceId};
pub use rules::{match_window, AppRule, Matcher};
pub use storage::{GeometryKey, MemoryStore, PausedProcess, StateStore};
pub use transition::{execute, execute_best_effort, plan_switch, Action, WorkspaceState};
