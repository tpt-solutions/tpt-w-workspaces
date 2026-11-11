// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starting the daemon at sign-in.
//!
//! * Windows: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! * Linux: a systemd user service, enabled with `systemctl --user`.

pub const USAGE: &str = "usage: tpt-workspaces autostart enable|disable|status";

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd] if cmd == "enable" => enable(),
        [cmd] if cmd == "disable" => disable(),
        [cmd] if cmd == "status" => status(),
        _ => Err(USAGE.to_string()),
    }
}

#[cfg(windows)]
mod imp {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "tpt-workspaces";

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = format!("\"{}\" run", exe.display());
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("opening Run key: {e}"))?;
        key.set_value(VALUE_NAME, &command)
            .map_err(|e| format!("writing Run key: {e}"))?;
        println!("autostart enabled: {command}");
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("opening Run key: {e}"))?;
        match key.delete_value(VALUE_NAME) {
            Ok(()) => println!("autostart disabled"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("autostart was not enabled")
            }
            Err(e) => return Err(format!("removing Run entry: {e}")),
        }
        Ok(())
    }

    pub fn status() -> Result<(), String> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_QUERY_VALUE)
            .map_err(|e| format!("opening Run key: {e}"))?;
        match key.get_value::<String, _>(VALUE_NAME) {
            Ok(command) => println!("enabled: {command}"),
            Err(_) => println!("disabled"),
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;
    use std::process::Command;

    const UNIT_NAME: &str = "tpt-workspaces.service";

    fn unit_path() -> Result<PathBuf, String> {
        let dir = dirs::config_dir().ok_or("no user config directory")?;
        Ok(dir.join("systemd").join("user").join(UNIT_NAME))
    }

    fn systemctl(args: &[&str]) -> Result<(), String> {
        let status = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()
            .map_err(|e| format!("running systemctl: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl --user {} failed ({status})",
                args.join(" ")
            ))
        }
    }

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let unit = format!(
            "[Unit]\n\
             Description=tpt-workspaces daemon\n\
             PartOf=graphical-session.target\n\
             After=graphical-session.target\n\
             \n\
             [Service]\n\
             ExecStart={} run\n\
             Restart=on-failure\n\
             \n\
             [Install]\n\
             WantedBy=graphical-session.target\n",
            exe.display()
        );
        let path = unit_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, unit).map_err(|e| format!("writing {}: {e}", path.display()))?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "--now", UNIT_NAME])?;
        println!("autostart enabled: {}", path.display());
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        let path = unit_path()?;
        if path.exists() {
            systemctl(&["disable", "--now", UNIT_NAME])?;
            std::fs::remove_file(&path).map_err(|e| format!("removing {}: {e}", path.display()))?;
            systemctl(&["daemon-reload"])?;
            println!("autostart disabled");
        } else {
            println!("autostart was not enabled");
        }
        Ok(())
    }

    pub fn status() -> Result<(), String> {
        let output = Command::new("systemctl")
            .args(["--user", "is-enabled", UNIT_NAME])
            .output()
            .map_err(|e| format!("running systemctl: {e}"))?;
        println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn enable() -> Result<(), String> {
        Err("autostart is not supported on this platform".into())
    }
    pub fn disable() -> Result<(), String> {
        Err("autostart is not supported on this platform".into())
    }
    pub fn status() -> Result<(), String> {
        Err("autostart is not supported on this platform".into())
    }
}

use imp::{disable, enable, status};
