// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! SQLite implementation of [`StateStore`](crate::storage::StateStore).
//!
//! Schema changes are applied as an ordered list of migrations. The applied
//! version is kept in SQLite's `PRAGMA user_version`. To change the schema,
//! append a new SQL string to `MIGRATIONS`. Never edit or reorder existing ones.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{WindowGeometry, WorkspaceId};
use crate::storage::{GeometryKey, PausedProcess, StateStore};

const MIGRATIONS: &[&str] = &[
    // v1: per-app window geometry and runtime key/value state.
    "CREATE TABLE window_geometry (
         workspace  TEXT    NOT NULL,
         executable TEXT    NOT NULL,
         x          INTEGER NOT NULL,
         y          INTEGER NOT NULL,
         width      INTEGER NOT NULL,
         height     INTEGER NOT NULL,
         PRIMARY KEY (workspace, executable)
     );
     CREATE TABLE runtime_state (
         key   TEXT PRIMARY KEY,
         value TEXT NOT NULL
     );",
];

const LAST_ACTIVE_KEY: &str = "last_active";
const PAUSED_KEY: &str = "paused_processes";

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens (creating if needed) the database at `path` and applies pending migrations.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// In-memory database, mainly for tests.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> rusqlite::Result<Self> {
        migrate(&mut conn)?;
        Ok(SqliteStore { conn })
    }

    /// Schema version currently applied to the database.
    pub fn schema_version(&self) -> rusqlite::Result<usize> {
        self.conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }
}

/// Applies every migration newer than the database's `user_version`, each in its own transaction.
fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: usize = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > MIGRATIONS.len() {
        return Err(rusqlite::Error::ToSqlConversionFailure(
            format!(
                "database schema version {current} is newer than this build supports ({})",
                MIGRATIONS.len()
            )
            .into(),
        ));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index + 1)?;
        tx.commit()?;
    }
    Ok(())
}

fn from_i64(column: usize, v: i64) -> rusqlite::Result<u32> {
    u32::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, v))
}

impl StateStore for SqliteStore {
    type Error = rusqlite::Error;

    fn load_geometry(&self, key: &GeometryKey) -> rusqlite::Result<Option<WindowGeometry>> {
        self.conn
            .query_row(
                "SELECT x, y, width, height FROM window_geometry
                 WHERE workspace = ?1 AND executable = ?2",
                params![key.workspace.as_str(), key.executable],
                |row| {
                    Ok(WindowGeometry {
                        x: row.get::<_, i32>(0)?,
                        y: row.get::<_, i32>(1)?,
                        width: from_i64(2, row.get(2)?)?,
                        height: from_i64(3, row.get(3)?)?,
                    })
                },
            )
            .optional()
    }

    fn save_geometry(
        &mut self,
        key: &GeometryKey,
        geometry: WindowGeometry,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO window_geometry (workspace, executable, x, y, width, height)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (workspace, executable) DO UPDATE SET
                 x = excluded.x, y = excluded.y,
                 width = excluded.width, height = excluded.height",
            params![
                key.workspace.as_str(),
                key.executable,
                geometry.x,
                geometry.y,
                i64::from(geometry.width),
                i64::from(geometry.height),
            ],
        )?;
        Ok(())
    }

    fn last_active(&self) -> rusqlite::Result<Option<WorkspaceId>> {
        self.conn
            .query_row(
                "SELECT value FROM runtime_state WHERE key = ?1",
                params![LAST_ACTIVE_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|opt| opt.map(WorkspaceId::new))
    }

    fn set_last_active(&mut self, workspace: Option<&WorkspaceId>) -> rusqlite::Result<()> {
        match workspace {
            Some(ws) => {
                self.conn.execute(
                    "INSERT INTO runtime_state (key, value) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    params![LAST_ACTIVE_KEY, ws.as_str()],
                )?;
            }
            None => {
                self.conn.execute(
                    "DELETE FROM runtime_state WHERE key = ?1",
                    params![LAST_ACTIVE_KEY],
                )?;
            }
        }
        Ok(())
    }

    fn load_paused(&self) -> rusqlite::Result<Vec<PausedProcess>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM runtime_state WHERE key = ?1",
                params![PAUSED_KEY],
                |row| row.get(0),
            )
            .optional()?;
        // One "pid<TAB>executable" entry per line. Malformed lines are skipped.
        Ok(raw
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (pid, exe) = line.split_once('\t')?;
                Some(PausedProcess {
                    pid: pid.parse().ok()?,
                    executable: exe.to_string(),
                })
            })
            .collect())
    }

    fn save_paused(&mut self, paused: &[PausedProcess]) -> rusqlite::Result<()> {
        if paused.is_empty() {
            self.conn.execute(
                "DELETE FROM runtime_state WHERE key = ?1",
                params![PAUSED_KEY],
            )?;
            return Ok(());
        }
        let value: String = paused
            .iter()
            .map(|p| format!("{}\t{}\n", p.pid, p.executable))
            .collect();
        self.conn.execute(
            "INSERT INTO runtime_state (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![PAUSED_KEY, value],
        )?;
        Ok(())
    }
}
