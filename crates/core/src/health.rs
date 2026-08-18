use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Restarting,
    CrashLoop,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub status: TaskStatus,
    pub restarts: u32,
    pub alerting: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSnapshot {
    pub status: &'static str,
    pub uptime_s: u64,
    pub tasks: BTreeMap<String, TaskSnapshot>,
}

#[derive(Clone)]
pub struct HealthState {
    started: Instant,
    inner: Arc<Mutex<BTreeMap<String, TaskSnapshot>>>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            inner: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn set_task(&self, name: &str, snap: TaskSnapshot) {
        self.inner
            .lock()
            .expect("health")
            .insert(name.to_string(), snap);
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let tasks = self.inner.lock().expect("health").clone();
        let degraded = tasks
            .values()
            .any(|t| t.alerting || t.status == TaskStatus::CrashLoop);
        HealthSnapshot {
            status: if degraded { "degraded" } else { "ok" },
            uptime_s: self.started.elapsed().as_secs(),
            tasks,
        }
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}
