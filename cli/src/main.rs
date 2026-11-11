// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `tpt-workspaces` command-line tool and daemon entry point.

mod autostart;
mod daemon;
mod paths;
mod platform;

use std::path::PathBuf;
use std::process::ExitCode;

use tpt_workspaces_core::config;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
usage:
  tpt-workspaces check [CONFIG]              validate a config file
  tpt-workspaces run [--config PATH] [--state PATH]
                                             run the daemon in the foreground
  tpt-workspaces autostart enable|disable|status

Logging: set RUST_LOG (for example RUST_LOG=debug). Set TPT_LOG_JSON=1 for JSON output.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check(args.get(1).map(PathBuf::from)),
        Some("run") => run(&args[1..]),
        Some("autostart") => autostart::run(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn check(path: Option<PathBuf>) -> Result<(), String> {
    let path = path.unwrap_or_else(paths::config_file);
    let cfg = config::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{}: OK ({} workspace(s), {} rule(s))",
        path.display(),
        cfg.workspaces.len(),
        cfg.rules.len()
    );
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let mut config_path = paths::config_file();
    let mut state_path = paths::state_file();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--config" => config_path = PathBuf::from(value),
            "--state" => state_path = PathBuf::from(value),
            other => return Err(format!("unknown option '{other}'")),
        }
    }

    init_logging();
    daemon::run(&config_path, &state_path)
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if std::env::var_os("TPT_LOG_JSON").is_some() {
        builder.json().init();
    } else {
        builder.init();
    }
}
