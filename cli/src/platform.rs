// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Selects the backend for the current operating system.

#[cfg(windows)]
mod imp {
    pub use tpt_workspaces_backend_windows::{
        WindowEvent, WindowEventListener, WindowsBackend as Backend,
    };

    pub fn connect() -> Result<Backend, String> {
        Ok(Backend::new())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub use tpt_workspaces_backend_linux::{
        SwayBackend as Backend, WindowEvent, WindowEventListener,
    };

    pub fn connect() -> Result<Backend, String> {
        Backend::connect().map_err(|e| format!("cannot reach sway: {e}"))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("tpt-workspaces supports Windows and Linux (sway/wlroots) only");

pub use imp::*;
