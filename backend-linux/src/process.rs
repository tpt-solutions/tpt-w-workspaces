// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process identification and freezing via `SIGSTOP` / `SIGCONT`.

use crate::error::BackendError;

/// Executable file name of a process, read from `/proc`. Falls back to the
/// (possibly truncated) `comm` name when `/proc/<pid>/exe` is not readable.
pub fn executable_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let from_exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let name = from_exe.or_else(|| {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim_end().to_string())
    })?;
    // A binary replaced while running shows up as "... (deleted)".
    Some(
        name.strip_suffix(" (deleted)")
            .unwrap_or(name.as_str())
            .to_string(),
    )
}

pub fn suspend(pid: u32) -> Result<(), BackendError> {
    signal(pid, libc::SIGSTOP)
}

pub fn resume(pid: u32) -> Result<(), BackendError> {
    signal(pid, libc::SIGCONT)
}

fn signal(pid: u32, sig: libc::c_int) -> Result<(), BackendError> {
    if pid <= 1 {
        return Err(BackendError::Refused(format!(
            "pid {pid} is a system process"
        )));
    }
    if pid == std::process::id() {
        return Err(BackendError::Refused(
            "refusing to signal tpt-workspaces itself".into(),
        ));
    }
    let target = libc::pid_t::try_from(pid)
        .map_err(|_| BackendError::NotFound(format!("pid {pid} is out of range")))?;

    // SAFETY: kill(2) has no memory-safety preconditions; errors are reported via errno.
    let rc = unsafe { libc::kill(target, sig) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Err(BackendError::NotFound(format!("no process {pid}"))),
        Some(libc::EPERM) => Err(BackendError::Refused(format!(
            "permission denied signalling pid {pid}"
        ))),
        _ => Err(BackendError::Io(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_executable_name() {
        let name = executable_name(std::process::id()).expect("own exe should be readable");
        assert!(!name.is_empty());
    }

    #[test]
    fn refuses_system_pids_and_self() {
        assert!(matches!(suspend(1), Err(BackendError::Refused(_))));
        assert!(matches!(
            resume(std::process::id()),
            Err(BackendError::Refused(_))
        ));
    }
}
