//! In-process bus: validate → persist `event_log` → broadcast (UC-102).

use std::time::Duration;

use homeai_proto::{HomeEvent, SCHEMA_VERSION};
use tokio::sync::broadcast;
use tracing::warn;

use crate::db::{now_ms, Db, DbError, RetentionCuts};

const CAP: usize = 4096;
const DEFAULT_RETENTION: Duration = Duration::from_secs(90 * 24 * 3600);
const PRESENCE_RAW_RETENTION: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("invalid event: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<HomeEvent>,
    db: Db,
}

impl Bus {
    pub fn new(db: Db) -> Self {
        let (tx, _) = broadcast::channel(CAP);
        Self { tx, db }
    }

    /// Validate, persist, then fan out. Ack only after `event_log` insert.
    pub fn publish(&self, event: HomeEvent) -> Result<(), PublishError> {
        validate(&event)?;
        self.db.append_event(event.clone())?;
        let _ = self.tx.send(event);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HomeEvent> {
        self.tx.subscribe()
    }

    pub fn subscribe_type(&self, event_type: impl Into<String>) -> TypedReceiver {
        TypedReceiver {
            rx: self.tx.subscribe(),
            event_type: event_type.into(),
        }
    }

    pub fn purge(&self, now: i64) -> Result<u64, DbError> {
        self.db.purge(now, cuts_at(now))
    }

    pub fn db(&self) -> &Db {
        &self.db
    }
}

pub struct TypedReceiver {
    rx: broadcast::Receiver<HomeEvent>,
    event_type: String,
}

impl TypedReceiver {
    pub async fn recv(&mut self) -> Result<HomeEvent, broadcast::error::RecvError> {
        loop {
            let ev = self.rx.recv().await?;
            if ev.event_type == self.event_type {
                return Ok(ev);
            }
        }
    }
}

pub fn validate(event: &HomeEvent) -> Result<(), PublishError> {
    if event.event_id.is_empty() {
        return Err(PublishError::Invalid("event_id"));
    }
    if event.event_type.is_empty() {
        return Err(PublishError::Invalid("event_type"));
    }
    if event.timestamp_ms <= 0 {
        return Err(PublishError::Invalid("timestamp_ms"));
    }
    if event.schema_version != SCHEMA_VERSION {
        return Err(PublishError::Invalid("schema_version"));
    }
    if !event.confidence.is_finite() || !(0.0..=1.0).contains(&event.confidence) {
        return Err(PublishError::Invalid("confidence"));
    }
    Ok(())
}

pub fn cuts_at(now_ms: i64) -> RetentionCuts {
    RetentionCuts {
        default_before_ms: now_ms - DEFAULT_RETENTION.as_millis() as i64,
        presence_raw_before_ms: now_ms - PRESENCE_RAW_RETENTION.as_millis() as i64,
    }
}

pub fn core_event(
    event_type: &str,
    source: &str,
    unique: &str,
    payload: impl Into<Vec<u8>>,
) -> HomeEvent {
    HomeEvent {
        event_id: format!("{event_type}:{source}:{unique}"),
        event_type: event_type.into(),
        source_id: source.into(),
        room_id: String::new(),
        person_id: String::new(),
        timestamp_ms: now_ms(),
        confidence: 1.0,
        payload: payload.into(),
        schema_version: SCHEMA_VERSION,
    }
}

pub async fn retention_loop(
    bus: Bus,
    interval: Duration,
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(interval) => {
                match bus.purge(now_ms()) {
                    Ok(n) if n > 0 => tracing::info!(deleted = n, "event_log retention"),
                    Ok(_) => {}
                    Err(err) => warn!(error = %err, "event_log retention failed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn sample(id: &str, ty: &str, ts: i64) -> HomeEvent {
        HomeEvent {
            event_id: id.into(),
            event_type: ty.into(),
            source_id: "test".into(),
            room_id: "room-1".into(),
            person_id: String::new(),
            timestamp_ms: ts,
            confidence: 1.0,
            payload: vec![],
            schema_version: SCHEMA_VERSION,
        }
    }

    fn temp_bus() -> (Bus, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("home.db")).unwrap();
        (Bus::new(db), dir)
    }

    #[test]
    fn rejects_invalid_events() {
        let (bus, _dir) = temp_bus();
        let mut ev = sample("ok", "demo", 1);
        ev.event_id.clear();
        assert!(matches!(bus.publish(ev), Err(PublishError::Invalid(_))));
    }

    #[tokio::test]
    async fn publish_1000_received_in_order() {
        let (bus, _dir) = temp_bus();
        let mut rx = bus.subscribe();
        let now = now_ms();
        for i in 0..1000 {
            bus.publish(sample(&format!("e-{i:04}"), "demo.ordered", now + i))
                .unwrap();
        }
        let mut got = Vec::new();
        for _ in 0..1000 {
            got.push(rx.recv().await.unwrap().event_id);
        }
        let expect: Vec<_> = (0..1000).map(|i| format!("e-{i:04}")).collect();
        assert_eq!(got, expect);
    }

    #[test]
    fn persist_before_ack_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.db");
        {
            let db = Db::open(&path).unwrap();
            let bus = Bus::new(db);
            for i in 0..50 {
                bus.publish(sample(&format!("ack-{i}"), "demo.persist", 1_000 + i))
                    .unwrap();
            }
        }
        let db = Db::open(&path).unwrap();
        let rows = db.load_all().unwrap();
        assert_eq!(rows.len(), 50);
        assert_eq!(rows[0].event_id, "ack-0");
        assert_eq!(rows[49].event_id, "ack-49");
    }

    #[tokio::test]
    async fn slow_subscriber_does_not_block_publisher() {
        let (bus, _dir) = temp_bus();
        let _slow = bus.subscribe();
        let started = Instant::now();
        for i in 0..200 {
            bus.publish(sample(&format!("s-{i}"), "demo.slow", 2_000 + i))
                .unwrap();
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "publisher blocked on a silent subscriber: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn retention_deletes_old_rows_by_type() {
        let (bus, _dir) = temp_bus();
        let now = 1_700_000_000_000;
        bus.publish(sample("old-pres", "presence.raw", now - 48 * 3600 * 1000))
            .unwrap();
        bus.publish(sample(
            "old-core",
            "core.health_alert",
            now - 100 * 24 * 3600 * 1000,
        ))
        .unwrap();
        bus.publish(sample("fresh", "core.health_alert", now))
            .unwrap();
        let deleted = bus.purge(now).unwrap();
        assert_eq!(deleted, 2);
        let left: Vec<_> = bus
            .db()
            .load_all()
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        assert_eq!(left, vec!["fresh".to_string()]);
    }

    #[tokio::test]
    async fn subscribe_type_filters() {
        let (bus, _dir) = temp_bus();
        let mut only = bus.subscribe_type("kitchen.ask");
        bus.publish(sample("a", "other", 10)).unwrap();
        bus.publish(sample("b", "kitchen.ask", 11)).unwrap();
        let ev = only.recv().await.unwrap();
        assert_eq!(ev.event_id, "b");
    }
}
