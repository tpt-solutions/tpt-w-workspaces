// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Top-level window enumeration, geometry, and process identification.

use std::ffi::c_void;
use std::path::Path;

use tpt_workspaces_core::model::{WindowGeometry, WindowId, WindowInfo, WorkspaceId};
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetWindowPos, GWL_EXSTYLE, GW_OWNER,
    SWP_NOACTIVATE, SWP_NOZORDER, WS_EX_TOOLWINDOW,
};

use crate::guid;

/// Classes that belong to the shell rather than to apps.
const SHELL_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
];

/// Host process for UWP (Store) apps. Its windows must be resolved to the real app process.
const UWP_HOST_EXE: &str = "ApplicationFrameHost.exe";
const UWP_CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

pub fn window_id(hwnd: HWND) -> WindowId {
    WindowId(hwnd.0 as usize as u64)
}

pub fn hwnd(id: WindowId) -> HWND {
    HWND(id.0 as usize as *mut c_void)
}

/// Lists visible, user-facing top-level windows on all virtual desktops.
///
/// Windows on inactive desktops are cloaked by DWM but still enumerated, which is
/// required so their owning apps can be paused and moved.
pub fn list_top_level() -> Vec<WindowInfo> {
    let mut found: Vec<WindowInfo> = Vec::new();
    unsafe {
        // Errors here only mean enumeration stopped early; whatever was collected is still valid.
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut found as *mut Vec<WindowInfo> as isize),
        );
    }
    found
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
    if let Some(info) = describe(hwnd) {
        out.push(info);
    }
    BOOL(1)
}

fn describe(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        // Owned windows are dialogs and secondary panels; the owner is what the user sees.
        if GetWindow(hwnd, GW_OWNER)
            .map(|h| !h.is_invalid())
            .unwrap_or(false)
        {
            return None;
        }
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
    }

    let title = read_string(|buf| unsafe { GetWindowTextW(hwnd, buf) });
    if title.is_empty() {
        return None;
    }
    let class = read_string(|buf| unsafe { GetClassNameW(hwnd, buf) });
    if SHELL_CLASSES.iter().any(|c| c.eq_ignore_ascii_case(&class)) {
        return None;
    }

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let mut executable = executable_name(pid).unwrap_or_default();

    // UWP apps are hosted by ApplicationFrameHost; the real app owns a CoreWindow child.
    if executable.eq_ignore_ascii_case(UWP_HOST_EXE) {
        if let Some(app_pid) = find_uwp_app_pid(hwnd, pid) {
            pid = app_pid;
            executable = executable_name(pid).unwrap_or_default();
        }
    }

    let workspace = winvd::get_desktop_by_window(hwnd)
        .ok()
        .and_then(|d| d.get_id().ok())
        .map(|g| WorkspaceId::new(guid::format(&g)));

    Some(WindowInfo {
        id: window_id(hwnd),
        pid,
        executable,
        window_class: class,
        title,
        workspace,
        geometry: get_rect(hwnd).ok(),
    })
}

/// Reads a UTF-16 string using the Win32 "copy into buffer, return length" convention.
fn read_string(mut call: impl FnMut(&mut [u16]) -> i32) -> String {
    let mut buf = vec![0u16; 512];
    let len = call(&mut buf);
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// Executable file name (e.g. `code.exe`) for a process, if it can be queried.
/// Returns `None` for elevated or protected processes.
pub fn executable_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let _guard = HandleGuard(handle);
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .ok()?;
    }
    let full = String::from_utf16_lossy(&buf[..size as usize]);
    Path::new(&full)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

struct UwpSearch {
    host_pid: u32,
    found: Option<u32>,
}

unsafe extern "system" fn uwp_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut UwpSearch);
    let class = read_string(|buf| GetClassNameW(hwnd, buf));
    if class == UWP_CORE_WINDOW_CLASS {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && pid != search.host_pid {
            search.found = Some(pid);
            return BOOL(0); // stop enumerating
        }
    }
    BOOL(1)
}

fn find_uwp_app_pid(host: HWND, host_pid: u32) -> Option<u32> {
    let mut search = UwpSearch {
        host_pid,
        found: None,
    };
    unsafe {
        let _ = EnumChildWindows(
            host,
            Some(uwp_child_proc),
            LPARAM(&mut search as *mut UwpSearch as isize),
        );
    }
    search.found
}

pub fn get_rect(hwnd: HWND) -> Result<WindowGeometry, windows::core::Error> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    Ok(WindowGeometry {
        x: rect.left,
        y: rect.top,
        width: (rect.right - rect.left).max(0) as u32,
        height: (rect.bottom - rect.top).max(0) as u32,
    })
}

pub fn set_rect(hwnd: HWND, geometry: WindowGeometry) -> Result<(), windows::core::Error> {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            geometry.x,
            geometry.y,
            geometry.width as i32,
            geometry.height as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
}

/// Closes a process/thread handle when dropped.
pub(crate) struct HandleGuard(pub HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
