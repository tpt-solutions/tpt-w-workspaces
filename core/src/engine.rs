// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The daemon's brain: applies rules, workspace switches and pause/resume
//! on top of a [`DesktopBackend`] and a [`StateStore`].
//!
//! The engine is synchronous and single-threaded. Platform event sources send
//! [`Event`]s to it from whatever thread they run on, and the caller decides how
//! to deliver them (see `cli`).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;

use tracing::{debug, info, warn};

use crate::backend::DesktopBackend;
use crate::config::Config;
use crate::model::{WindowGeometry, WindowId, WindowInfo, WorkspaceId};
use crate::rules::{match_window, AppRule};
use crate::storage::{GeometryKey, PausedProcess, StateStore};
use crate::transition::{execute_best_effort, plan_switch, Action, WorkspaceState};

/// Something that happened in the desktop environment, or a request from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A window was created or shown.
    WindowAppeared(WindowId),
    /// A window became the foreground window. If it lives on another workspace,
    /// the user has switched to that workspace.
    WindowFocused(WindowId),
    /// The platform reports that a workspace became active directly. Used where
    /// the platform can say so, even when the workspace has no windows.
    WorkspaceFocused(WorkspaceId),
    /// The config file changed and has been re-validated.
    ConfigReloaded(Config),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

fn err<E: Display>(context: &str, e: E) -> EngineError {
    EngineError(format!("{context}: {e}"))
}

pub struct Engine<B: DesktopBackend, S: StateStore> {
    backend: B,
    store: S,
    /// Config workspace id -> backend workspace id.
    desktops: HashMap<String, WorkspaceId>,
    /// Rules with targets already translated to backend workspace ids.
    rules: Vec<AppRule>,
    state: WorkspaceState,
    /// Last known geometry of windows, keyed by window id.
    geometry: HashMap<WindowId, WindowGeometry>,
    /// Frozen processes and the executable they were frozen as.
    paused: BTreeMap<u32, String>,
}

impl<B: DesktopBackend, S: StateStore> Engine<B, S> {
    /// Starts the engine: recovers from a previous crash, maps config workspaces
    /// onto backend workspaces, and records the current workspace.
    pub fn start(backend: B, store: S, config: &Config) -> Result<Self, EngineError> {
        let mut engine = Engine {
            backend,
            store,
            desktops: HashMap::new(),
            rules: Vec::new(),
            state: WorkspaceState::default(),
            geometry: HashMap::new(),
            paused: BTreeMap::new(),
        };
        engine.recover_paused()?;
        engine.resolve_workspaces(config)?;
        engine.state.active = engine
            .backend
            .current_workspace()
            .map_err(|e| err("reading current workspace", e))?;
        info!(
            active = engine.state.active.as_ref().map(|w| w.as_str()),
            workspaces = engine.desktops.len(),
            rules = engine.rules.len(),
            "engine started"
        );
        Ok(engine)
    }

    /// Stops the engine and resumes every process it froze. Anything that cannot
    /// be resumed stays recorded, so the next start retries it.
    pub fn shutdown(mut self) -> Result<(B, S), EngineError> {
        let pids: Vec<u32> = self.paused.keys().copied().collect();
        for pid in pids {
            match self.backend.resume_process(pid) {
                Ok(()) => {
                    info!(pid, "resumed process on shutdown");
                    self.paused.remove(&pid);
                }
                Err(e) => {
                    warn!(pid, error = %e, "could not resume process on shutdown; will retry on next start")
                }
            }
        }
        self.state.paused_pids = self.paused.keys().copied().collect();
        self.persist_paused()?;
        if let Some(active) = &self.state.active {
            self.store
                .set_last_active(Some(active))
                .map_err(|e| err("saving last active workspace", e))?;
        }
        Ok((self.backend, self.store))
    }

    /// Applies one event.
    pub fn handle(&mut self, event: Event) -> Result<(), EngineError> {
        match event {
            Event::ConfigReloaded(config) => {
                self.resolve_workspaces(&config)?;
                info!(rules = self.rules.len(), "config reloaded");
                Ok(())
            }
            Event::WindowAppeared(id) => self.on_window_appeared(id),
            Event::WindowFocused(id) => self.on_window_focused(id),
            Event::WorkspaceFocused(target) => {
                if self.state.active.as_ref() == Some(&target) {
                    return Ok(());
                }
                let windows = self.list_windows()?;
                self.switch_to(target, &windows)
            }
        }
    }

    /// Freezes the processes of rule-matched windows that are on a workspace other
    /// than the active one. Returns how many processes were newly frozen.
    pub fn pause_background(&mut self) -> Result<usize, EngineError> {
        let windows = self.list_windows()?;
        let before = self.paused.len();
        for window in &windows {
            let Some(workspace) = &window.workspace else {
                continue; // unknown workspace: not provably in the background
            };
            if self.state.active.as_ref() == Some(workspace) {
                continue;
            }
            if match_window(&self.rules, window).is_none() {
                continue;
            }
            self.freeze_if_background(window.pid, &window.executable, &windows)?;
        }
        Ok(self.paused.len() - before)
    }

    /// Resumes every process the engine has frozen. Background windows are frozen
    /// again on their next event, or by [`Engine::pause_background`].
    /// Returns how many processes were resumed.
    pub fn resume_all(&mut self) -> Result<usize, EngineError> {
        let pids: Vec<u32> = self.paused.keys().copied().collect();
        let mut resumed = 0;
        for pid in pids {
            match self.backend.resume_process(pid) {
                Ok(()) => {
                    self.paused.remove(&pid);
                    self.state.paused_pids.remove(&pid);
                    resumed += 1;
                }
                Err(e) => warn!(pid, error = %e, "could not resume process"),
            }
        }
        self.persist_paused()?;
        Ok(resumed)
    }

    /// Processes the engine currently holds frozen, with their executable names.
    pub fn paused_processes(&self) -> Vec<(u32, String)> {
        self.paused
            .iter()
            .map(|(pid, exe)| (*pid, exe.clone()))
            .collect()
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }

    pub fn rules(&self) -> &[AppRule] {
        &self.rules
    }

    /// Config workspace ids mapped to backend workspace ids.
    pub fn workspace_map(&self) -> &HashMap<String, WorkspaceId> {
        &self.desktops
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, EngineError> {
        self.backend
            .list_windows()
            .map_err(|e| err("listing windows", e))
    }

    /// Finds every config workspace on the backend, creating those that are missing.
    /// Workspaces are matched by name, which the platform keeps across restarts.
    fn resolve_workspaces(&mut self, config: &Config) -> Result<(), EngineError> {
        let existing = self
            .backend
            .enumerate_desktops()
            .map_err(|e| err("enumerating workspaces", e))?;

        let mut map = HashMap::new();
        for ws in &config.workspaces {
            let id = match existing.iter().find(|d| d.name == ws.name) {
                Some(found) => found.id.clone(),
                None => {
                    let created = self
                        .backend
                        .create_desktop(&ws.name)
                        .map_err(|e| err("creating workspace", e))?;
                    info!(workspace = %ws.id, name = %ws.name, "created workspace");
                    created
                }
            };
            map.insert(ws.id.as_str().to_string(), id);
        }

        self.rules = config
            .rules
            .iter()
            .filter_map(|rule| match map.get(rule.target.as_str()) {
                Some(target) => Some(AppRule {
                    name: rule.name.clone(),
                    matcher: rule.matcher.clone(),
                    target: target.clone(),
                }),
                None => {
                    warn!(target = %rule.target, "rule refers to an unknown workspace; skipped");
                    None
                }
            })
            .collect();
        self.desktops = map;
        Ok(())
    }

    /// Resumes processes frozen before a crash, provided the pid still belongs to
    /// the same executable. Then clears the persisted list.
    fn recover_paused(&mut self) -> Result<(), EngineError> {
        let previous = self
            .store
            .load_paused()
            .map_err(|e| err("loading paused processes", e))?;
        for PausedProcess { pid, executable } in previous {
            match self.backend.process_executable(pid) {
                Some(current) if current.eq_ignore_ascii_case(&executable) => {
                    match self.backend.resume_process(pid) {
                        Ok(()) => info!(pid, %executable, "recovered frozen process"),
                        Err(e) => {
                            warn!(pid, %executable, error = %e, "could not recover frozen process")
                        }
                    }
                }
                _ => {
                    debug!(pid, %executable, "frozen process no longer exists; nothing to recover")
                }
            }
        }
        self.store
            .save_paused(&[])
            .map_err(|e| err("clearing paused processes", e))
    }

    fn persist_paused(&mut self) -> Result<(), EngineError> {
        let list: Vec<PausedProcess> = self
            .paused
            .iter()
            .map(|(pid, exe)| PausedProcess {
                pid: *pid,
                executable: exe.clone(),
            })
            .collect();
        self.store
            .save_paused(&list)
            .map_err(|e| err("saving paused processes", e))
    }

    fn on_window_appeared(&mut self, id: WindowId) -> Result<(), EngineError> {
        let mut windows = self.list_windows()?;
        let Some(index) = windows.iter().position(|w| w.id == id) else {
            // Closed before we got to it.
            return Ok(());
        };
        let window = windows[index].clone();
        let Some(rule) = match_window(&self.rules, &window) else {
            return Ok(());
        };
        let target = rule.target.clone();

        if window.workspace.as_ref() != Some(&target) {
            self.backend
                .move_window(id, &target)
                .map_err(|e| err("moving window", e))?;
            info!(window = id.0, executable = %window.executable, target = %target, "moved window per rule");
            windows[index].workspace = Some(target.clone());
        }

        if self.state.active.as_ref() != Some(&target) {
            self.freeze_if_background(window.pid, &window.executable, &windows)?;
        }
        Ok(())
    }

    /// Freezes `pid` unless it has a window on the active workspace.
    fn freeze_if_background(
        &mut self,
        pid: u32,
        executable: &str,
        windows: &[WindowInfo],
    ) -> Result<(), EngineError> {
        if self.paused.contains_key(&pid) {
            return Ok(());
        }
        let on_active = windows
            .iter()
            .any(|w| w.pid == pid && w.workspace.is_some() && w.workspace == self.state.active);
        if on_active {
            return Ok(());
        }
        match self.backend.suspend_process(pid) {
            Ok(()) => {
                info!(pid, %executable, "froze background process");
                self.paused.insert(pid, executable.to_string());
                self.state.paused_pids.insert(pid);
                self.persist_paused()?;
            }
            Err(e) => warn!(pid, %executable, error = %e, "could not freeze process"),
        }
        Ok(())
    }

    fn on_window_focused(&mut self, id: WindowId) -> Result<(), EngineError> {
        let windows = self.list_windows()?;
        let Some(window) = windows.iter().find(|w| w.id == id) else {
            return Ok(());
        };
        let Some(target) = window.workspace.clone() else {
            return Ok(());
        };
        if self.state.active.as_ref() == Some(&target) {
            return Ok(());
        }
        self.switch_to(target, &windows)
    }

    fn switch_to(
        &mut self,
        target: WorkspaceId,
        windows: &[WindowInfo],
    ) -> Result<(), EngineError> {
        // Load remembered geometry for windows arriving on the target workspace.
        for w in windows
            .iter()
            .filter(|w| w.workspace.as_ref() == Some(&target))
        {
            if self.geometry.contains_key(&w.id) {
                continue;
            }
            let key = GeometryKey {
                workspace: target.clone(),
                executable: w.executable.clone(),
            };
            if let Some(saved) = self
                .store
                .load_geometry(&key)
                .map_err(|e| err("loading window geometry", e))?
            {
                self.geometry.insert(w.id, saved);
            }
        }

        // Only a managed workspace can be left behind. The desktop the daemon
        // started on is not one of them, so its apps are never frozen.
        if let Some(active) = &self.state.active {
            if !self.desktops.values().any(|managed| managed == active) {
                self.state.active = None;
            }
        }
        let outgoing = self.state.active.clone();
        let actions = plan_switch(&mut self.state, &target, windows, &self.geometry);
        let failures = execute_best_effort(&mut self.backend, &actions, &mut self.geometry);
        for (action, error) in &failures {
            warn!(?action, error = %error, "switch action failed");
        }
        self.apply_pause_changes(&actions, &failures, windows);

        // Remember where the outgoing windows were so they can be restored later.
        if let Some(out) = &outgoing {
            for w in windows.iter().filter(|w| w.workspace.as_ref() == Some(out)) {
                if let Some(g) = self.geometry.get(&w.id) {
                    let key = GeometryKey {
                        workspace: out.clone(),
                        executable: w.executable.clone(),
                    };
                    self.store
                        .save_geometry(&key, *g)
                        .map_err(|e| err("saving window geometry", e))?;
                }
            }
        }

        self.persist_paused()?;
        self.store
            .set_last_active(Some(&target))
            .map_err(|e| err("saving last active workspace", e))?;
        info!(
            from = outgoing.as_ref().map(|w| w.as_str()),
            to = %target,
            actions = actions.len(),
            failures = failures.len(),
            "switched workspace"
        );
        Ok(())
    }

    /// Keeps `self.paused` in step with the suspend/resume actions that succeeded.
    fn apply_pause_changes(
        &mut self,
        actions: &[Action],
        failures: &[(Action, impl Display)],
        windows: &[WindowInfo],
    ) {
        let failed = |a: &Action| failures.iter().any(|(f, _)| f == a);
        for action in actions {
            match action {
                Action::Suspend { pid } => {
                    if failed(action) {
                        // Not frozen after all.
                        self.state.paused_pids.remove(pid);
                    } else {
                        let exe = windows
                            .iter()
                            .find(|w| w.pid == *pid)
                            .map(|w| w.executable.clone())
                            .or_else(|| self.backend.process_executable(*pid))
                            .unwrap_or_default();
                        self.paused.insert(*pid, exe);
                    }
                }
                Action::Resume { pid } => {
                    if !failed(action) {
                        self.paused.remove(pid);
                    } else {
                        // Still frozen; keep tracking it so it is retried.
                        self.state.paused_pids.insert(*pid);
                    }
                }
                _ => {}
            }
        }
    }
}
