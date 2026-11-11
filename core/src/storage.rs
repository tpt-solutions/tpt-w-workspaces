// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Storage boundary for runtime state. Core code depends only on this trait;
//! the SQLite implementation lives behind the `sqlite` feature.

use std::collections::HashMap;
use std::convert::Infallible;

use crate::model::{WindowGeometry, WorkspaceId};

/// Key for a per-app saved position: an app's last-known geometry on a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GeometryKey {
    pub workspace: WorkspaceId,
    pub executable: String,
}

/// A process the engine has frozen. Persisted so that a crash does not leave it frozen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedProcess {
    pub pid: u32,
    pub executable: String,
}

pub trait StateStore {
    type Error: std::error::Error;

    fn load_geometry(&self, key: &GeometryKey) -> Result<Option<WindowGeometry>, Self::Error>;

    fn save_geometry(
        &mut self,
        key: &GeometryKey,
        geometry: WindowGeometry,
    ) -> Result<(), Self::Error>;

    fn last_active(&self) -> Result<Option<WorkspaceId>, Self::Error>;

    fn set_last_active(&mut self, workspace: Option<&WorkspaceId>) -> Result<(), Self::Error>;

    /// Processes that were frozen when the state was last saved.
    fn load_paused(&self) -> Result<Vec<PausedProcess>, Self::Error>;

    /// Replaces the set of frozen processes. An empty slice clears it.
    fn save_paused(&mut self, paused: &[PausedProcess]) -> Result<(), Self::Error>;
}

/// Non-persistent store, useful for tests and as a fallback.
#[derive(Debug, Default)]
pub struct MemoryStore {
    geometry: HashMap<GeometryKey, WindowGeometry>,
    last_active: Option<WorkspaceId>,
    paused: Vec<PausedProcess>,
}

impl StateStore for MemoryStore {
    type Error = Infallible;

    fn load_geometry(&self, key: &GeometryKey) -> Result<Option<WindowGeometry>, Infallible> {
        Ok(self.geometry.get(key).copied())
    }

    fn save_geometry(
        &mut self,
        key: &GeometryKey,
        geometry: WindowGeometry,
    ) -> Result<(), Infallible> {
        self.geometry.insert(key.clone(), geometry);
        Ok(())
    }

    fn last_active(&self) -> Result<Option<WorkspaceId>, Infallible> {
        Ok(self.last_active.clone())
    }

    fn set_last_active(&mut self, workspace: Option<&WorkspaceId>) -> Result<(), Infallible> {
        self.last_active = workspace.cloned();
        Ok(())
    }

    fn load_paused(&self) -> Result<Vec<PausedProcess>, Infallible> {
        Ok(self.paused.clone())
    }

    fn save_paused(&mut self, paused: &[PausedProcess]) -> Result<(), Infallible> {
        self.paused = paused.to_vec();
        Ok(())
    }
}
