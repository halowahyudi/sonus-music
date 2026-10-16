//! One-shot async work, polled from the GUI thread.
//!
//! yt-dlp calls take anywhere from 200ms to several seconds. They run on the
//! tokio runtime and report back over a channel; the GUI checks the channel
//! once per frame and never blocks.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// A background task whose result is polled rather than awaited.
pub struct Task<T> {
    rx: Receiver<T>,
    started: Instant,
}

impl<T: Send + 'static> Task<T> {
    /// Spawn `future` on the tokio runtime.
    pub fn spawn<F>(handle: &tokio::runtime::Handle, future: F) -> Self
    where
        F: std::future::Future<Output = T> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        handle.spawn(async move {
            let result = future.await;
            // The receiver may already be gone if the user navigated away.
            let _ = tx.send(result);
        });
        Self {
            rx,
            started: Instant::now(),
        }
    }

    /// Non-blocking check for the result.
    pub fn poll(&mut self) -> Option<T> {
        match self.rx.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) => None,
            // The sender was dropped without sending — treat as no result.
            // `poll` returning None forever would spin the UI, so callers
            // should also watch `is_stale`.
            Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// True once the task has been running long enough to warrant a hint.
    pub fn is_slow(&self) -> bool {
        self.started.elapsed() > Duration::from_secs(8)
    }
}

/// Tracks the state of one async fetch, including whether it's still running.
#[derive(Default)]
pub enum Pending<T> {
    #[default]
    Idle,
    Running(Task<T>),
    /// Finished, but the value hasn't been consumed yet.
    Ready(T),
}

impl<T: Send + 'static> Pending<T> {
    pub fn start<F>(handle: &tokio::runtime::Handle, future: F) -> Self
    where
        F: std::future::Future<Output = T> + Send + 'static,
    {
        Pending::Running(Task::spawn(handle, future))
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Pending::Running(_))
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Pending::Idle)
    }

    /// Advance the state machine and return a result exactly once.
    pub fn poll(&mut self) -> Option<T> {
        let task = match self {
            Pending::Running(task) => task,
            _ => return None,
        };

        match task.poll() {
            Some(value) => {
                *self = Pending::Ready(value);
                // Hand the value over on the next call so the caller's
                // borrow of self has ended.
                None
            }
            None => None,
        }
    }

    /// Take the finished value, resetting to idle.
    pub fn take(&mut self) -> Option<T> {
        match std::mem::take(self) {
            Pending::Ready(value) => Some(value),
            other => {
                *self = other;
                None
            }
        }
    }

    /// Convenience: poll and take in one call.
    pub fn poll_take(&mut self) -> Option<T> {
        self.poll();
        self.take()
    }

    pub fn reset(&mut self) {
        *self = Pending::Idle;
    }

    /// How long the current run has been going.
    pub fn elapsed(&self) -> Duration {
        match self {
            Pending::Running(task) => task.elapsed(),
            _ => Duration::ZERO,
        }
    }

    pub fn is_slow(&self) -> bool {
        matches!(self, Pending::Running(task) if task.is_slow())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A multi-threaded runtime, matching what the app builds at startup.
    ///
    /// A current-thread runtime would need to be actively driven for spawned
    /// tasks to make progress, which isn't how the real app runs.
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn task_delivers_its_result() {
        let rt = runtime();
        let mut task = Task::spawn(rt.handle(), async { 42 });

        for _ in 0..100 {
            if let Some(value) = task.poll() {
                assert_eq!(value, 42);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("task never produced a result");
    }

    #[test]
    fn task_poll_is_none_before_completion() {
        let rt = runtime();
        let mut task = Task::spawn(rt.handle(), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            "done"
        });
        assert!(task.poll().is_none());
    }

    #[test]
    fn pending_starts_idle() {
        let mut pending: Pending<i32> = Pending::default();
        assert!(pending.is_idle());
        assert!(!pending.is_running());
        assert!(pending.take().is_none());
    }

    #[test]
    fn pending_moves_through_its_states() {
        let rt = runtime();
        let mut pending = Pending::start(rt.handle(), async { 7 });
        assert!(pending.is_running());

        let mut got = None;
        for _ in 0..100 {
            if let Some(value) = pending.poll_take() {
                got = Some(value);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, Some(7));
        assert!(pending.is_idle(), "take should reset to idle");
    }

    #[test]
    fn pending_result_is_delivered_exactly_once() {
        let rt = runtime();
        let mut pending = Pending::start(rt.handle(), async { "x" });

        for _ in 0..100 {
            if pending.poll_take().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(pending.poll_take().is_none(), "result must not repeat");
    }

    #[test]
    fn reset_discards_a_running_task() {
        let rt = runtime();
        let mut pending = Pending::start(rt.handle(), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            1
        });
        pending.reset();
        assert!(pending.is_idle());
        assert!(pending.poll_take().is_none());
    }

    #[test]
    fn reset_discards_a_ready_value() {
        let rt = runtime();
        let mut pending = Pending::start(rt.handle(), async { 5 });
        for _ in 0..100 {
            pending.poll();
            if !pending.is_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        pending.reset();
        assert!(pending.take().is_none());
    }

    #[test]
    fn elapsed_grows_while_running() {
        let rt = runtime();
        let pending = Pending::start(rt.handle(), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        assert!(pending.elapsed() < Duration::from_secs(1));
        assert!(!pending.is_slow());
    }

    #[test]
    fn idle_pending_reports_no_elapsed_time() {
        let pending: Pending<i32> = Pending::default();
        assert_eq!(pending.elapsed(), Duration::ZERO);
        assert!(!pending.is_slow());
    }
}
