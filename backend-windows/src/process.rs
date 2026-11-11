// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pausing and resuming applications by suspending their threads.
//!
//! Every thread that belongs to the process is suspended with `SuspendThread`
//! (documented Win32 API). Threads created after the snapshot are not frozen.
//! Resuming decrements each thread's suspend count until it reaches zero, so
//! the operation is safe to repeat.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Mutex;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, OpenProcessToken, OpenThread, ResumeThread, SuspendThread,
    PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

use crate::error::BackendError;
use crate::window::HandleGuard;

/// Processes that must never be frozen: the System Idle process and the System process.
const PROTECTED_PIDS: &[u32] = &[0, 4];

/// Processes this backend has suspended and not yet resumed.
static FROZEN: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

/// Whether this backend currently holds `pid` suspended.
pub fn is_frozen(pid: u32) -> bool {
    FROZEN.lock().map(|set| set.contains(&pid)).unwrap_or(false)
}

/// Whether `pid` runs with an elevated token. `None` if the token cannot be read.
pub fn is_elevated(pid: u32) -> Option<bool> {
    unsafe {
        let process = HandleGuard(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut token = HANDLE::default();
        OpenProcessToken(process.0, TOKEN_QUERY, &mut token).ok()?;
        let token = HandleGuard(token);

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

fn check_pid(pid: u32) -> Result<(), BackendError> {
    if PROTECTED_PIDS.contains(&pid) {
        return Err(BackendError::Refused(format!(
            "pid {pid} is a system process"
        )));
    }
    if pid == unsafe { GetCurrentProcessId() } {
        return Err(BackendError::Refused(
            "refusing to suspend tpt-workspaces itself".into(),
        ));
    }
    Ok(())
}

/// An elevated process cannot be suspended by a non-elevated caller, and the
/// attempt would fail on every window event. Report it once, clearly, instead.
fn check_suspendable(pid: u32) -> Result<(), BackendError> {
    let self_elevated = is_elevated(unsafe { GetCurrentProcessId() }) == Some(true);
    if !self_elevated && is_elevated(pid) == Some(true) {
        return Err(BackendError::Refused(format!(
            "pid {pid} is elevated; run tpt-workspaces as administrator to pause it"
        )));
    }
    Ok(())
}

/// Thread ids of every thread that belongs to `pid`.
pub fn thread_ids(pid: u32) -> Result<Vec<u32>, BackendError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? };
    let _guard = HandleGuard(snapshot);

    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut ids = Vec::new();
    unsafe {
        if Thread32First(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID == pid {
                    ids.push(entry.th32ThreadID);
                }
                if Thread32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
    }
    Ok(ids)
}

fn open_thread(tid: u32) -> Result<HandleGuard, windows::core::Error> {
    let handle: HANDLE = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, tid)? };
    Ok(HandleGuard(handle))
}

/// Suspends all threads of `pid`. Returns how many threads were suspended.
///
/// Threads that exit during the operation are skipped. An error is returned only
/// if the process has no threads or none of them could be suspended.
pub fn suspend_process(pid: u32) -> Result<usize, BackendError> {
    check_pid(pid)?;
    check_suspendable(pid)?;
    let ids = thread_ids(pid)?;
    if ids.is_empty() {
        return Err(BackendError::NotFound(format!("no threads for pid {pid}")));
    }

    let mut suspended = 0usize;
    let mut last_error = None;
    for tid in ids {
        match open_thread(tid) {
            Ok(handle) => {
                // Returns the previous suspend count, or u32::MAX on failure.
                if unsafe { SuspendThread(handle.0) } == u32::MAX {
                    last_error = Some(windows::core::Error::from_win32());
                } else {
                    suspended += 1;
                }
            }
            Err(e) => last_error = Some(e),
        }
    }

    if suspended == 0 {
        return Err(match last_error {
            Some(e) => BackendError::Win32(e),
            None => BackendError::NotFound(format!("no suspendable threads for pid {pid}")),
        });
    }
    if let Ok(mut set) = FROZEN.lock() {
        set.insert(pid);
    }
    Ok(suspended)
}

/// Resumes all threads of `pid`, undoing every outstanding suspend.
/// Returns how many threads were resumed.
pub fn resume_process(pid: u32) -> Result<usize, BackendError> {
    check_pid(pid)?;
    let ids = thread_ids(pid)?;
    if ids.is_empty() {
        return Err(BackendError::NotFound(format!("no threads for pid {pid}")));
    }

    let mut resumed = 0usize;
    let mut last_error = None;
    for tid in ids {
        match open_thread(tid) {
            Ok(handle) => loop {
                // ResumeThread returns the previous suspend count.
                let previous = unsafe { ResumeThread(handle.0) };
                if previous == u32::MAX {
                    last_error = Some(windows::core::Error::from_win32());
                    break;
                }
                if previous == 1 {
                    resumed += 1;
                }
                if previous <= 1 {
                    break;
                }
            },
            Err(e) => last_error = Some(e),
        }
    }

    if resumed == 0 {
        if let Some(e) = last_error {
            return Err(BackendError::Win32(e));
        }
    }
    if let Ok(mut set) = FROZEN.lock() {
        set.remove(&pid);
    }
    Ok(resumed)
}

/// Diagnostic: returns each thread's current suspend count. Probes by
/// suspending and immediately resuming each thread, so counts are not changed.
pub fn probe_suspend_counts(pid: u32) -> Result<Vec<u32>, BackendError> {
    let ids = thread_ids(pid)?;
    let mut counts = Vec::with_capacity(ids.len());
    for tid in ids {
        let handle = open_thread(tid)?;
        let previous = unsafe { SuspendThread(handle.0) };
        if previous == u32::MAX {
            return Err(BackendError::Win32(windows::core::Error::from_win32()));
        }
        unsafe { ResumeThread(handle.0) };
        counts.push(previous);
    }
    Ok(counts)
}
