use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// JSON logs to `$log_dir/core.log` and stderr (UC-101).
pub fn init(log_dir: &Path) -> std::io::Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;
    let file_appender = tracing_appender::rolling::never(log_dir, "core.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_writer = non_blocking.with_max_level(tracing::Level::TRACE);
    let stderr_writer = std::io::stderr.with_max_level(tracing::Level::TRACE);

    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(stderr_writer.and(file_writer))
        .with_current_span(true)
        .with_span_list(false)
        .try_init();

    Ok(guard)
}
