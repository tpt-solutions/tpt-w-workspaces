// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Minimal client for sway's i3-compatible IPC protocol.
//!
//! Wire format: the ASCII magic `i3-ipc`, a 4-byte little-endian payload length,
//! a 4-byte little-endian message type, then the payload. Event messages have
//! the high bit of the type set.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::BackendError;

pub const RUN_COMMAND: u32 = 0;
pub const GET_WORKSPACES: u32 = 1;
pub const SUBSCRIBE: u32 = 2;
pub const GET_TREE: u32 = 4;
pub const EVENT_FLAG: u32 = 0x8000_0000;

const MAGIC: &[u8; 6] = b"i3-ipc";

/// Path of the IPC socket, from `$SWAYSOCK`, which sway sets for its children.
pub fn socket_path() -> Result<PathBuf, BackendError> {
    std::env::var_os("SWAYSOCK")
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::NotRunning("SWAYSOCK is not set".into()))
}

pub fn write_message(stream: &mut impl Write, msg_type: u32, payload: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&msg_type.to_le_bytes());
    frame.extend_from_slice(payload);
    stream.write_all(&frame)
}

pub fn read_message(stream: &mut impl Read) -> io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 14];
    stream.read_exact(&mut header)?;
    if &header[..6] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad IPC magic from sway",
        ));
    }
    let len = u32::from_le_bytes([header[6], header[7], header[8], header[9]]) as usize;
    let msg_type = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    Ok((msg_type, payload))
}

/// One request/reply exchange on a fresh connection.
pub fn request(msg_type: u32, payload: &[u8]) -> Result<Value, BackendError> {
    let mut stream =
        UnixStream::connect(socket_path()?).map_err(|e| BackendError::NotRunning(e.to_string()))?;
    write_message(&mut stream, msg_type, payload)?;
    let (reply_type, body) = read_message(&mut stream)?;
    if reply_type != msg_type {
        return Err(BackendError::Ipc(format!(
            "expected reply type {msg_type}, got {reply_type}"
        )));
    }
    Ok(serde_json::from_slice(&body)?)
}

/// Runs a sway command such as `[con_id=3] move container to workspace "x"`.
pub fn run_command(command: &str) -> Result<(), BackendError> {
    let reply = request(RUN_COMMAND, command.as_bytes())?;
    let results = reply
        .as_array()
        .ok_or_else(|| BackendError::Ipc("RUN_COMMAND reply is not an array".into()))?;
    for result in results {
        if result["success"].as_bool() != Some(true) {
            return Err(BackendError::Command {
                command: command.to_string(),
                message: result["error"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string(),
            });
        }
    }
    Ok(())
}

/// Quotes a workspace name for use in a sway command.
pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, GET_TREE, b"hello").unwrap();
        assert_eq!(&buf[..6], b"i3-ipc");
        let (ty, body) = read_message(&mut Cursor::new(buf)).unwrap();
        assert_eq!(ty, GET_TREE);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = b"nope!!\0\0\0\0\0\0\0\0".to_vec();
        assert!(read_message(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn quotes_names() {
        assert_eq!(quote("dev"), "\"dev\"");
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
