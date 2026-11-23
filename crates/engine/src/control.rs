//! Cross-thread run control: progress, pause/resume and cancellation (Q110),
//! plus the warnings a run raises while it still produces a result (D15).

use crate::error::SimError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

/// Shared between the thread running [`crate::simulate`] and observers.
/// Wrap it in an `Arc` to share it.
#[derive(Debug, Default)]
pub struct SimControl {
    cancelled: AtomicBool,
    /// Mirrors `paused` so that a checkpoint of a run nobody pauses takes no
    /// lock (the transient checks once per Newton iteration).
    pause_requested: AtomicBool,
    paused: Mutex<bool>,
    resumed: Condvar,
    progress_bits: AtomicU64,
    warnings: Mutex<Vec<String>>,
}

impl SimControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. The run stops at its next checkpoint with
    /// [`SimError::Cancelled`]. Also releases a paused run.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.resume();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Pauses the run at its next checkpoint.
    pub fn pause(&self) {
        let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        *paused = true;
        self.pause_requested.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        *paused = false;
        self.pause_requested.store(false, Ordering::SeqCst);
        drop(paused);
        self.resumed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Completed fraction in `0.0..=1.0`.
    pub fn progress(&self) -> f64 {
        f64::from_bits(self.progress_bits.load(Ordering::Relaxed))
    }

    pub(crate) fn set_progress(&self, fraction: f64) {
        let f = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.progress_bits.store(f.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn reset_progress(&self) {
        self.set_progress(0.0);
    }

    /// Warnings raised by the latest run, oldest first (D15), for example
    /// "Node m has no DC path to ground (it is reached only through
    /// capacitors); it was tied to ground through gmin = 1 pS for the
    /// operating point". [`crate::simulate`] clears them when a run starts.
    /// A run that raised warnings still returns its result.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Records a warning for the current run; repeats are ignored.
    pub(crate) fn warn(&self, message: impl Into<String>) {
        let message = message.into();
        let mut warnings = self.warnings.lock().unwrap_or_else(|e| e.into_inner());
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }

    pub(crate) fn clear_warnings(&self) {
        self.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Called by analyses between units of work: blocks while paused and
    /// returns `Err(Cancelled)` once cancellation was requested.
    pub(crate) fn checkpoint(&self) -> Result<(), SimError> {
        if self.pause_requested.load(Ordering::SeqCst) {
            let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
            while *paused && !self.is_cancelled() {
                paused = self.resumed.wait(paused).unwrap_or_else(|e| e.into_inner());
            }
        }
        if self.is_cancelled() {
            Err(SimError::Cancelled {
                progress: self.progress(),
            })
        } else {
            Ok(())
        }
    }
}
