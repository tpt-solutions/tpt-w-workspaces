// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

#![cfg(feature = "sqlite")]

use tpt_workspaces_core::sqlite::SqliteStore;
use tpt_workspaces_core::{GeometryKey, StateStore, WindowGeometry, WorkspaceId};

fn key(ws: &str, exe: &str) -> GeometryKey {
    GeometryKey {
        workspace: WorkspaceId::new(ws),
        executable: exe.to_string(),
    }
}

fn geo(x: i32) -> WindowGeometry {
    WindowGeometry {
        x,
        y: 5,
        width: 1280,
        height: 720,
    }
}

#[test]
fn geometry_round_trip_and_upsert() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    let k = key("dev", "code.exe");

    assert_eq!(store.load_geometry(&k).unwrap(), None);
    store.save_geometry(&k, geo(10)).unwrap();
    assert_eq!(store.load_geometry(&k).unwrap(), Some(geo(10)));
    store.save_geometry(&k, geo(20)).unwrap();
    assert_eq!(store.load_geometry(&k).unwrap(), Some(geo(20)));

    // Same app on a different workspace is stored separately.
    assert_eq!(store.load_geometry(&key("chat", "code.exe")).unwrap(), None);
}

#[test]
fn last_active_round_trip() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    assert_eq!(store.last_active().unwrap(), None);
    store
        .set_last_active(Some(&WorkspaceId::new("dev")))
        .unwrap();
    assert_eq!(store.last_active().unwrap(), Some(WorkspaceId::new("dev")));
    store.set_last_active(None).unwrap();
    assert_eq!(store.last_active().unwrap(), None);
}

#[test]
fn migrations_bring_fresh_db_to_latest_version() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
}

#[test]
fn reopening_a_file_database_keeps_data_and_does_not_re_migrate() {
    let dir = std::env::temp_dir().join(format!("tpt-ws-sqlite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.db");
    let _ = std::fs::remove_file(&path);

    {
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .save_geometry(&key("dev", "code.exe"), geo(7))
            .unwrap();
    }
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store.load_geometry(&key("dev", "code.exe")).unwrap(),
        Some(geo(7))
    );
    assert_eq!(store.schema_version().unwrap(), 1);

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
