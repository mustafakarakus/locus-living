//! SQLite WAL. One writer thread. Schema on first open.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use homeai_proto::HomeEvent;

use crate::model::{Device, DeviceState, Floor, Person, Property, Room};

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
    Tables(mpsc::Sender<Result<Vec<String>, rusqlite::Error>>),
    PutProperty(Property, mpsc::Sender<Result<(), rusqlite::Error>>),
    PutFloor(Floor, mpsc::Sender<Result<(), rusqlite::Error>>),
    PutRoom(Room, mpsc::Sender<Result<(), rusqlite::Error>>),
    PutDevice(Device, mpsc::Sender<Result<(), rusqlite::Error>>),
    PutPerson(Person, mpsc::Sender<Result<(), rusqlite::Error>>),
    GetPerson(
        String,
        mpsc::Sender<Result<Option<Person>, rusqlite::Error>>,
    ),
    PutDeviceState(DeviceState, mpsc::Sender<Result<(), rusqlite::Error>>),
    GetDeviceState(
        String,
        mpsc::Sender<Result<Option<DeviceState>, rusqlite::Error>>,
    ),
    Count(String, mpsc::Sender<Result<i64, rusqlite::Error>>),
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

    pub fn table_names(&self) -> Result<Vec<String>, DbError> {
        call(&self.tx, Cmd::Tables)
    }

    pub fn put_property(&self, row: Property) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutProperty(row, s))
    }

    pub fn put_floor(&self, row: Floor) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutFloor(row, s))
    }

    pub fn put_room(&self, row: Room) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutRoom(row, s))
    }

    pub fn put_device(&self, row: Device) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutDevice(row, s))
    }

    pub fn put_person(&self, row: Person) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutPerson(row, s))
    }

    pub fn get_person(&self, id: &str) -> Result<Option<Person>, DbError> {
        call(&self.tx, |s| Cmd::GetPerson(id.into(), s))
    }

    pub fn put_device_state(&self, row: DeviceState) -> Result<(), DbError> {
        call(&self.tx, |s| Cmd::PutDeviceState(row, s))
    }

    pub fn get_device_state(&self, device_id: &str) -> Result<Option<DeviceState>, DbError> {
        call(&self.tx, |s| Cmd::GetDeviceState(device_id.into(), s))
    }

    pub fn count(&self, table: &str) -> Result<i64, DbError> {
        call(&self.tx, |s| Cmd::Count(table.into(), s))
    }

    pub fn close(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

fn call<T>(
    tx: &mpsc::Sender<Cmd>,
    build: impl FnOnce(mpsc::Sender<Result<T, rusqlite::Error>>) -> Cmd,
) -> Result<T, DbError> {
    let (rtx, rrx) = mpsc::channel();
    tx.send(build(rtx)).map_err(|_| DbError::WorkerGone)?;
    Ok(rrx.recv().map_err(|_| DbError::WorkerGone)??)
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
            Cmd::Tables(reply) => {
                let _ = reply.send(list_tables(&conn));
            }
            Cmd::PutProperty(row, reply) => {
                let _ = reply.send(put_property(&conn, &row));
            }
            Cmd::PutFloor(row, reply) => {
                let _ = reply.send(put_floor(&conn, &row));
            }
            Cmd::PutRoom(row, reply) => {
                let _ = reply.send(put_room(&conn, &row));
            }
            Cmd::PutDevice(row, reply) => {
                let _ = reply.send(put_device(&conn, &row));
            }
            Cmd::PutPerson(row, reply) => {
                let _ = reply.send(put_person(&conn, &row));
            }
            Cmd::GetPerson(id, reply) => {
                let _ = reply.send(get_person(&conn, &id));
            }
            Cmd::PutDeviceState(row, reply) => {
                let _ = reply.send(put_device_state(&conn, &row));
            }
            Cmd::GetDeviceState(id, reply) => {
                let _ = reply.send(get_device_state(&conn, &id));
            }
            Cmd::Count(table, reply) => {
                let _ = reply.send(count_table(&conn, &table));
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
    conn.execute_batch(include_str!("schema.sql"))?;
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

fn list_tables(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

fn put_property(conn: &rusqlite::Connection, row: &Property) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO property (id, name) VALUES (?1, ?2)",
        rusqlite::params![row.id, row.name],
    )?;
    Ok(())
}

fn put_floor(conn: &rusqlite::Connection, row: &Floor) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO floor (id, property_id, name) VALUES (?1, ?2, ?3)",
        rusqlite::params![row.id, row.property_id, row.name],
    )?;
    Ok(())
}

fn put_room(conn: &rusqlite::Connection, row: &Room) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO room (id, floor_id, name, kind) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![row.id, row.floor_id, row.name, row.kind],
    )?;
    Ok(())
}

fn put_device(conn: &rusqlite::Connection, row: &Device) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO device (id, room_id, name, kind, protocol) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![row.id, row.room_id, row.name, row.kind, row.protocol],
    )?;
    Ok(())
}

fn put_person(conn: &rusqlite::Connection, row: &Person) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO person (id, name, kind) VALUES (?1, ?2, ?3)",
        rusqlite::params![row.id, row.name, row.kind],
    )?;
    Ok(())
}

fn get_person(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Person>> {
    let mut stmt = conn.prepare("SELECT id, name, kind FROM person WHERE id = ?1")?;
    let mut rows = stmt.query(rusqlite::params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(Person {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
        })),
        None => Ok(None),
    }
}

fn put_device_state(conn: &rusqlite::Connection, row: &DeviceState) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO device_state (device_id, state_json, updated_ms) VALUES (?1, ?2, ?3)",
        rusqlite::params![row.device_id, row.state_json, row.updated_ms],
    )?;
    Ok(())
}

fn get_device_state(
    conn: &rusqlite::Connection,
    device_id: &str,
) -> rusqlite::Result<Option<DeviceState>> {
    let mut stmt = conn.prepare(
        "SELECT device_id, state_json, updated_ms FROM device_state WHERE device_id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![device_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(DeviceState {
            device_id: row.get(0)?,
            state_json: row.get(1)?,
            updated_ms: row.get(2)?,
        })),
        None => Ok(None),
    }
}

fn count_table(conn: &rusqlite::Connection, table: &str) -> rusqlite::Result<i64> {
    if !crate::model::REQUIRED_TABLES.contains(&table) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
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

    #[test]
    fn first_open_creates_every_required_table() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("home.db")).unwrap();
        let names = db.table_names().unwrap();
        for table in crate::model::REQUIRED_TABLES {
            assert!(names.iter().any(|n| n == table), "missing table {table}");
        }
    }
}
