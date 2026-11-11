// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local IPC between the daemon and its clients (the `status` command, and
//! later the GUI).
//!
//! Transport: a named pipe on Windows (`\\.\pipe\tpt-workspaces`), a Unix
//! domain socket on Linux (`$XDG_RUNTIME_DIR/tpt-workspaces.sock`). Both are
//! local to the user's session.
//!
//! Protocol: one JSON request per line, one JSON response per line. A
//! connection carries exactly one request.
//!
//! ```text
//! -> {"cmd":"status"}
//! <- {"result":"status","active":"dev","workspaces":[{"id":"dev","name":"Development","active":true}],"rules":3,"paused":[{"pid":4242,"executable":"slack.exe"}]}
//! -> {"cmd":"resume_all"}
//! <- {"result":"done","count":1}
//! <- {"result":"error","message":"..."}
//! ```

use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long a connection waits for the daemon to answer before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Freeze background apps that have rule-matched windows on other workspaces.
    PauseBackground,
    /// Resume every process the daemon has frozen.
    ResumeAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenProcess {
    pub pid: u32,
    pub executable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Config id of the workspace the user is on, if it is one the config names.
    pub active: Option<String>,
    pub workspaces: Vec<WorkspaceStatus>,
    pub rules: usize,
    /// Processes currently frozen by the daemon.
    pub paused: Vec<FrozenProcess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Status(Status),
    /// A request that changed state was carried out; `count` processes were affected.
    Done {
        count: usize,
    },
    Error {
        message: String,
    },
}

/// A request handed to the daemon's main loop, with a channel for the answer.
pub struct Query {
    pub request: Request,
    pub reply: Sender<Response>,
}

/// Answers one request line. Errors while reading or writing the connection are
/// returned to the caller; protocol errors are answered with a `Response::Error`.
fn handle_line(line: &str, queries: &Sender<Query>) -> Response {
    let request: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            return Response::Error {
                message: format!("bad request: {e}"),
            }
        }
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    if queries
        .send(Query {
            request,
            reply: reply_tx,
        })
        .is_err()
    {
        return Response::Error {
            message: "daemon is shutting down".into(),
        };
    }
    reply_rx
        .recv_timeout(REPLY_TIMEOUT)
        .unwrap_or(Response::Error {
            message: "daemon did not answer in time".into(),
        })
}

fn serve_connection<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    queries: &Sender<Query>,
) -> io::Result<()> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(()); // client connected and went away
    }
    let response = handle_line(&line, queries);
    let text = serde_json::to_string(&response).map_err(io::Error::other)?;
    writer.write_all(text.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Sends one request to the running daemon and returns its answer.
fn send_request<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    request: &Request,
) -> Result<Response, String> {
    let text = serde_json::to_string(request).map_err(|e| e.to_string())?;
    writer
        .write_all(text.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|e| format!("sending request: {e}"))?;
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("reading reply: {e}"))?;
    serde_json::from_str(line.trim()).map_err(|e| format!("bad reply from daemon: {e}"))
}

// ---------------------------------------------------------------------------
// Windows: named pipe.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use std::fs::{File, OpenOptions};
    use std::io::{self, BufReader};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::sync::mpsc::Sender;

    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    use super::{send_request, serve_connection, Query, Request, Response};

    /// Pipe path for `CreateFile`; `create_instance` uses the same name as a wide string.
    pub const PIPE_PATH: &str = r"\\.\pipe\tpt-workspaces";

    /// Security descriptor that grants access to the owner (the current user) only.
    /// Without it, any local account could query the daemon.
    struct OwnerOnly(PSECURITY_DESCRIPTOR);

    impl OwnerOnly {
        fn new() -> io::Result<Self> {
            // D: DACL; P: protected (no inheritance); A;;GA;;;OW: full access for the owner.
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    w!("D:P(A;;GA;;;OW)"),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
            }
            .map_err(io::Error::other)?;
            Ok(OwnerOnly(descriptor))
        }
    }

    impl Drop for OwnerOnly {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(HLOCAL(self.0 .0));
            }
        }
    }

    fn create_instance(first: bool) -> io::Result<HANDLE> {
        let mut flags = PIPE_ACCESS_DUPLEX;
        if first {
            flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let descriptor = OwnerOnly::new()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0 .0,
            bInheritHandle: false.into(),
        };
        let handle = unsafe {
            CreateNamedPipeW(
                w!(r"\\.\pipe\tpt-workspaces"),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                Some(&attributes),
            )
        };
        if handle.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }

    /// Serves requests until the process exits. Each connection is answered on
    /// the calling thread, one at a time.
    pub fn serve(queries: Sender<Query>) -> io::Result<()> {
        let mut first = true;
        loop {
            let handle = create_instance(first)?;
            first = false;
            match unsafe { ConnectNamedPipe(handle, None) } {
                Ok(()) => {}
                // The client connected between create and connect; that is fine.
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                Err(e) => {
                    unsafe { CloseHandle(handle).ok() };
                    return Err(io::Error::other(e));
                }
            }
            // SAFETY: the handle is a valid pipe handle that we own; `File` closes it on drop.
            let file = unsafe { File::from_raw_handle(handle.0 as _) };
            if let Err(e) = serve_connection(BufReader::new(&file), &file, &queries) {
                tracing::debug!(error = %e, "ipc connection failed");
            }
            // Wait until the client has read the reply. Disconnecting first would
            // throw away any bytes it has not read yet.
            let raw = HANDLE(file.as_raw_handle() as _);
            unsafe {
                let _ = FlushFileBuffers(raw);
                let _ = DisconnectNamedPipe(raw);
            }
        }
    }

    pub fn request(request: &Request) -> Result<Response, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_PATH)
            .map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => "the daemon is not running".to_string(),
                _ => format!("connecting to the daemon: {e}"),
            })?;
        send_request(BufReader::new(&file), &file, request)
    }
}

// ---------------------------------------------------------------------------
// Linux: Unix domain socket.
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod imp {
    use std::io::{self, BufReader};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;

    use super::{send_request, serve_connection, Query, Request, Response};

    pub fn socket_path() -> PathBuf {
        dirs::runtime_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("tpt-workspaces.sock")
    }

    pub fn serve(queries: Sender<Query>) -> io::Result<()> {
        let path = socket_path();
        // A socket left behind by a crashed daemon blocks bind; nothing listens on it.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(error = %e, "ipc accept failed");
                    continue;
                }
            };
            let result = stream
                .try_clone()
                .and_then(|clone| serve_connection(BufReader::new(clone), &stream, &queries));
            if let Err(e) = result {
                tracing::debug!(error = %e, "ipc connection failed");
            }
        }
        Ok(())
    }

    pub fn request(request: &Request) -> Result<Response, String> {
        let path = socket_path();
        let stream = UnixStream::connect(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                "the daemon is not running".to_string()
            }
            _ => format!("connecting to the daemon: {e}"),
        })?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        send_request(reader, &stream, request)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use std::io;
    use std::sync::mpsc::Sender;

    use super::{Query, Request, Response};

    pub fn serve(_queries: Sender<Query>) -> io::Result<()> {
        Err(io::Error::other("IPC is not supported on this platform"))
    }

    pub fn request(_request: &Request) -> Result<Response, String> {
        Err("IPC is not supported on this platform".into())
    }
}

/// Serves requests from clients. Blocks; run it on its own thread.
pub fn serve(queries: Sender<Query>) -> io::Result<()> {
    imp::serve(queries)
}

/// Sends `request` to the running daemon and waits for its answer.
pub fn request(request: &Request) -> Result<Response, String> {
    imp::request(request)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    fn answer_with(status: Status) -> Sender<Query> {
        let (tx, rx) = mpsc::channel::<Query>();
        thread::spawn(move || {
            for query in rx.iter() {
                let _ = query.reply.send(Response::Status(status.clone()));
            }
        });
        tx
    }

    fn sample() -> Status {
        Status {
            active: Some("dev".into()),
            workspaces: vec![WorkspaceStatus {
                id: "dev".into(),
                name: "Development".into(),
                active: true,
            }],
            rules: 2,
            paused: vec![FrozenProcess {
                pid: 10,
                executable: "slack.exe".into(),
            }],
        }
    }

    #[test]
    fn status_request_is_answered_as_one_line() {
        let queries = answer_with(sample());
        let mut out = Vec::new();
        serve_connection(
            Cursor::new(b"{\"cmd\":\"status\"}\n".to_vec()),
            &mut out,
            &queries,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.ends_with('\n'));
        let response: Response = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(response, Response::Status(sample()));
    }

    #[test]
    fn unknown_command_is_reported_not_dropped() {
        let queries = answer_with(sample());
        let mut out = Vec::new();
        serve_connection(
            Cursor::new(b"{\"cmd\":\"explode\"}\n".to_vec()),
            &mut out,
            &queries,
        )
        .unwrap();
        let response: Response = serde_json::from_slice(&out).unwrap();
        assert!(matches!(response, Response::Error { .. }));
    }

    #[test]
    fn empty_connection_is_ignored() {
        let queries = answer_with(sample());
        let mut out = Vec::new();
        serve_connection(Cursor::new(Vec::new()), &mut out, &queries).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn client_round_trip_over_in_memory_pipe() {
        let queries = answer_with(sample());
        // Server side: read the request the client wrote, write the reply.
        let mut reply = Vec::new();
        let request_line = b"{\"cmd\":\"status\"}
"
        .to_vec();
        serve_connection(Cursor::new(request_line), &mut reply, &queries).unwrap();
        // Client side: write a request, then read the reply the server produced.
        let mut request_bytes = Vec::new();
        let parsed =
            send_request(Cursor::new(reply), &mut request_bytes, &Request::Status).unwrap();
        assert_eq!(
            request_bytes,
            b"{\"cmd\":\"status\"}
"
        );
        assert_eq!(parsed, Response::Status(sample()));
    }
}
