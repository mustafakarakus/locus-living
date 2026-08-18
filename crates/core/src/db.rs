//! SQLite WAL + `event_log` (UC-102). Remaining tables are UC-104.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use homeai_proto::HomeEvent;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db worker gone")]
    WorkerGone,
}

enum Cmd {
    Ping(mpsc::Sender<Result<(), rusqlite::Error>>),
    Append(HomeEvent, mpsc::Sender<Result<(), rusqlite::Error>>),
    Purge(
        i64,
        RetentionCuts,
        mpsc::Sender<Result<u64, rusqlite::Error>>,
    ),
    Load(mpsc::Sender<Result<Vec<HomeEvent>, rusqlite::Error>>),
    Shutdown,
}

/// Cutoff timestamps (ms) per class. Older rows are deleted.
#[derive(Clone, Copy)]
pub struct RetentionCuts {
    pub default_before_ms: i64,
    pub presence_raw_before_ms: i64,
}

/// One dedicated writer thread owns the rusqlite connection (techstack §1).
#[derive(Clone)]
pub struct Db {
    tx: mpsc::Sender<Cmd>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("homeai-db-writer".into())
            .spawn(move || worker(path, rx))?;
        let db = Self { tx };
        db.ping()?;
        Ok(db)
    }

    pub fn ping(&self) -> Result<(), DbError> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(Cmd::Ping(rtx))
            .map_err(|_| DbError::WorkerGone)?;
        rrx.recv().map_err(|_| DbError::WorkerGone)??;
        Ok(())
    }

    /// Append one event. Returns after the row is durable (WAL). Idempotent on `event_id`.
    pub fn append_event(&self, event: HomeEvent) -> Result<(), DbError> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(Cmd::Append(event, rtx))
            .map_err(|_| DbError::WorkerGone)?;
        rrx.recv().map_err(|_| DbError::WorkerGone)??;
        Ok(())
    }

    pub fn purge(&self, now_ms: i64, cuts: RetentionCuts) -> Result<u64, DbError> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(Cmd::Purge(now_ms, cuts, rtx))
            .map_err(|_| DbError::WorkerGone)?;
        Ok(rrx.recv().map_err(|_| DbError::WorkerGone)??)
    }

    pub fn load_all(&self) -> Result<Vec<HomeEvent>, DbError> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(Cmd::Load(rtx))
            .map_err(|_| DbError::WorkerGone)?;
        Ok(rrx.recv().map_err(|_| DbError::WorkerGone)??)
    }

    pub fn close(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

fn worker(path: PathBuf, rx: mpsc::Receiver<Cmd>) {
    let conn = match open_and_migrate(&path) {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(error = %err, path = %path.display(), "sqlite open failed");
            return;
        }
    };
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Ping(reply) => {
                let r = conn.query_row("SELECT 1", [], |_| Ok(()));
                let _ = reply.send(r);
            }
            Cmd::Append(event, reply) => {
                let _ = reply.send(insert_event(&conn, &event));
            }
            Cmd::Purge(_now, cuts, reply) => {
                let _ = reply.send(purge_events(&conn, cuts));
            }
            Cmd::Load(reply) => {
                let _ = reply.send(load_events(&conn));
            }
            Cmd::Shutdown => break,
        }
    }
}

fn open_and_migrate(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS event_log (
            event_id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            source_id TEXT NOT NULL,
            room_id TEXT NOT NULL,
            person_id TEXT NOT NULL,
            timestamp_ms INTEGER NOT NULL,
            confidence REAL NOT NULL,
            payload BLOB NOT NULL,
            schema_version INTEGER NOT NULL,
            ingested_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS event_log_type_ts
            ON event_log (event_type, timestamp_ms);
        ",
    )?;
    Ok(conn)
}

fn insert_event(conn: &rusqlite::Connection, event: &HomeEvent) -> rusqlite::Result<()> {
    let ingested = now_ms();
    conn.execute(
        "INSERT OR IGNORE INTO event_log (
            event_id, event_type, source_id, room_id, person_id,
            timestamp_ms, confidence, payload, schema_version, ingested_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            event.event_id,
            event.event_type,
            event.source_id,
            event.room_id,
            event.person_id,
            event.timestamp_ms,
            event.confidence,
            event.payload.as_slice(),
            event.schema_version,
            ingested,
        ],
    )?;
    Ok(())
}

fn purge_events(conn: &rusqlite::Connection, cuts: RetentionCuts) -> rusqlite::Result<u64> {
    let n = conn.execute(
        "DELETE FROM event_log WHERE
            (event_type IN ('presence.raw', 'presence.signal') AND timestamp_ms < ?1)
            OR (event_type NOT IN ('presence.raw', 'presence.signal') AND timestamp_ms < ?2)",
        rusqlite::params![cuts.presence_raw_before_ms, cuts.default_before_ms],
    )?;
    Ok(n as u64)
}

fn load_events(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<HomeEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_id, event_type, source_id, room_id, person_id,
                timestamp_ms, confidence, payload, schema_version
         FROM event_log ORDER BY timestamp_ms ASC, event_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(HomeEvent {
            event_id: row.get(0)?,
            event_type: row.get(1)?,
            source_id: row.get(2)?,
            room_id: row.get(3)?,
            person_id: row.get(4)?,
            timestamp_ms: row.get(5)?,
            confidence: row.get(6)?,
            payload: row.get(7)?,
            schema_version: row.get(8)?,
        })
    })?;
    rows.collect()
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_wal_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.db");
        let db = Db::open(&path).unwrap();
        db.ping().unwrap();
        db.close();
        let db = Db::open(&path).unwrap();
        db.ping().unwrap();
    }
}
