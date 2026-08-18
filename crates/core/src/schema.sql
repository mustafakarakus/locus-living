-- Local store. Created on first open. event_log is also used by the bus.

CREATE TABLE IF NOT EXISTS property (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS floor (
    id TEXT PRIMARY KEY,
    property_id TEXT NOT NULL REFERENCES property(id) ON DELETE CASCADE,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS room (
    id TEXT PRIMARY KEY,
    floor_id TEXT NOT NULL REFERENCES floor(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'indoor'
);

CREATE TABLE IF NOT EXISTS device (
    id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL REFERENCES room(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'unknown',
    protocol TEXT
);

CREATE TABLE IF NOT EXISTS sensor (
    id TEXT PRIMARY KEY,
    room_id TEXT REFERENCES room(id) ON DELETE SET NULL,
    device_id TEXT REFERENCES device(id) ON DELETE SET NULL,
    kind TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device_state (
    device_id TEXT PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    state_json TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS person (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'resident'
);

CREATE TABLE IF NOT EXISTS identity_signal (
    id TEXT PRIMARY KEY,
    person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    enrolled_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS presence_event (
    id TEXT PRIMARY KEY,
    room_id TEXT,
    person_id TEXT,
    present INTEGER NOT NULL,
    timestamp_ms INTEGER NOT NULL,
    confidence REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS automation (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS automation_rule (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL REFERENCES automation(id) ON DELETE CASCADE,
    trigger_json TEXT NOT NULL,
    action_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_execution (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL REFERENCES automation(id) ON DELETE CASCADE,
    started_ms INTEGER NOT NULL,
    result TEXT
);

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

CREATE TABLE IF NOT EXISTS home_memory (
    id TEXT PRIMARY KEY,
    person_id TEXT,
    content TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_preference (
    person_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (person_id, key)
);

CREATE TABLE IF NOT EXISTS conversation_session (
    id TEXT PRIMARY KEY,
    room_id TEXT,
    person_id TEXT,
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER
);

CREATE TABLE IF NOT EXISTS conversation_attachment (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES conversation_session(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS system_health (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
