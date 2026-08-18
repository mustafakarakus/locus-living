//! Supervised tokio tasks with panic isolation and bounded restart (UC-101).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::bus::{core_event, Bus};
use crate::health::{HealthState, TaskSnapshot, TaskStatus};

/// First restart is well under the 2s acceptance bound; repeats cap at 2s.
const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(2);
const ALERT_WINDOW: Duration = Duration::from_secs(60);
const ALERT_BURST: u32 = 5;

type BoxedTask = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send>>;
type Factory = Arc<dyn Fn(CancellationToken) -> BoxedTask + Send + Sync>;

#[derive(Clone)]
pub struct Supervisor {
    bus: Bus,
    health: HealthState,
    shutdown: CancellationToken,
    children: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Supervisor {
    pub fn new(bus: Bus, health: HealthState, shutdown: CancellationToken) -> Self {
        Self {
            bus,
            health,
            shutdown,
            children: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Spawn a long-running task. `factory` is called on every (re)start.
    pub fn spawn<F, Fut>(&self, name: &'static str, factory: F)
    where
        F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), anyhow::Error>> + Send + 'static,
    {
        let factory: Factory = Arc::new(move |child| Box::pin(factory(child)));
        let shutdown = self.shutdown.clone();
        let bus = self.bus.clone();
        let health = self.health.clone();
        let handle = tokio::spawn(async move {
            supervise(name, factory, shutdown, bus, health).await;
        });
        self.children.lock().expect("supervisor").push(handle);
    }

    pub async fn shutdown(&self, timeout: Duration) {
        self.shutdown.cancel();
        let handles: Vec<_> = {
            let mut g = self.children.lock().expect("supervisor");
            g.drain(..).collect()
        };
        let join_all = async {
            for h in handles {
                let _ = h.await;
            }
        };
        if tokio::time::timeout(timeout, join_all).await.is_err() {
            warn!("supervisor shutdown timed out");
        }
    }
}

async fn supervise(
    name: &'static str,
    factory: Factory,
    shutdown: CancellationToken,
    bus: Bus,
    health: HealthState,
) {
    let mut backoff = INITIAL_BACKOFF;
    let mut crashes: Vec<Instant> = Vec::new();
    let mut total_restarts: u32 = 0;

    health.set_task(
        name,
        TaskSnapshot {
            status: TaskStatus::Running,
            restarts: 0,
            alerting: false,
        },
    );

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        let child = shutdown.child_token();
        let fut = factory(child.clone());
        info!(task = name, "task starting");

        let handle = tokio::spawn(fut);
        let abort = handle.abort_handle();
        tokio::pin!(handle);
        let outcome = tokio::select! {
            _ = shutdown.cancelled() => {
                child.cancel();
                abort.abort();
                return;
            }
            joined = &mut handle => joined,
        };

        if shutdown.is_cancelled() {
            return;
        }

        let crashed = match outcome {
            Ok(Ok(())) => {
                warn!(task = name, "task exited cleanly; treating as failure");
                true
            }
            Ok(Err(err)) => {
                error!(task = name, error = %err, "task returned error");
                true
            }
            Err(join) if join.is_panic() => {
                error!(task = name, "task panicked");
                true
            }
            Err(_) => {
                warn!(task = name, "task cancelled");
                false
            }
        };

        if !crashed {
            return;
        }

        total_restarts += 1;
        let now = Instant::now();
        crashes.push(now);
        crashes.retain(|t| now.duration_since(*t) <= ALERT_WINDOW);
        let in_window = crashes.len() as u32;
        let alerting = in_window >= ALERT_BURST;
        let status = if alerting {
            TaskStatus::CrashLoop
        } else {
            TaskStatus::Restarting
        };

        health.set_task(
            name,
            TaskSnapshot {
                status,
                restarts: total_restarts,
                alerting,
            },
        );

        let _ = bus.publish(core_event(
            "core.task_restarted",
            name,
            &format!("{total_restarts}"),
            format!(r#"{{"attempt":{total_restarts}}}"#),
        ));

        if alerting {
            error!(
                task = name,
                restarts_in_window = in_window,
                "health alert: crash loop"
            );
            let _ = bus.publish(core_event(
                "core.health_alert",
                name,
                &format!("loop-{in_window}"),
                format!(r#"{{"restarts_in_window":{in_window}}}"#),
            ));
        }

        info!(task = name, ?backoff, "restarting task");
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);

        health.set_task(
            name,
            TaskSnapshot {
                status: TaskStatus::Running,
                restarts: total_restarts,
                alerting,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn test_bus() -> (Bus, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("home.db")).unwrap();
        (Bus::new(db), dir)
    }

    #[tokio::test]
    async fn restarts_a_panicking_task_within_two_seconds() {
        let (bus, _dir) = test_bus();
        let health = HealthState::new();
        let shutdown = CancellationToken::new();
        let supervisor = Supervisor::new(bus, health.clone(), shutdown.clone());
        let starts = Arc::new(AtomicU32::new(0));
        let starts_c = starts.clone();

        supervisor.spawn("boom", move |_child| {
            let starts = starts_c.clone();
            async move {
                let n = starts.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    panic!("boom");
                }
                std::future::pending::<()>().await;
                Ok(())
            }
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if starts.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        shutdown.cancel();
        supervisor.shutdown(Duration::from_secs(1)).await;
        assert!(
            starts.load(Ordering::SeqCst) >= 2,
            "expected a restart within 2s"
        );
        let snap = health.snapshot();
        assert!(snap.tasks["boom"].restarts >= 1);
    }

    #[tokio::test]
    async fn crash_loop_raises_health_alert() {
        let (bus, _dir) = test_bus();
        let mut rx = bus.subscribe();
        let health = HealthState::new();
        let shutdown = CancellationToken::new();
        let supervisor = Supervisor::new(bus, health.clone(), shutdown.clone());

        supervisor.spawn("loop", |_child| async {
            panic!("loop");
            #[allow(unreachable_code)]
            Ok(())
        });

        let mut saw_alert = false;
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(ev)) if ev.event_type == "core.health_alert" && ev.source_id == "loop" => {
                    saw_alert = true;
                    break;
                }
                _ => {}
            }
        }
        shutdown.cancel();
        supervisor.shutdown(Duration::from_secs(1)).await;
        assert!(saw_alert, "expected HealthAlert after burst");
        assert_eq!(health.snapshot().status, "degraded");
    }
}
