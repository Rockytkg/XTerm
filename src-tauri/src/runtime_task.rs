//! Shared teardown for long-running service runtime tasks.

use std::time::Duration;

/// Outcome of waiting for a service task after shutdown was signalled. How an
/// outcome is recorded or surfaced differs per service (structured warn,
/// silent, hard error), so that choice stays with the caller.
pub(crate) enum RuntimeTaskOutcome {
    Completed,
    JoinFailed(tauri::Error),
    TimedOut,
}

/// Waits for a service task to finish after shutdown was signalled, aborting
/// it once `drain_timeout` elapses so a stuck task cannot hang the stop path
/// forever.
pub(crate) async fn await_runtime_task(
    mut task: tauri::async_runtime::JoinHandle<()>,
    drain_timeout: Duration,
) -> RuntimeTaskOutcome {
    tokio::select! {
        result = &mut task => match result {
            Ok(()) => RuntimeTaskOutcome::Completed,
            Err(error) => RuntimeTaskOutcome::JoinFailed(error),
        },
        _ = tokio::time::sleep(drain_timeout) => {
            task.abort();
            let _ = task.await;
            RuntimeTaskOutcome::TimedOut
        }
    }
}
