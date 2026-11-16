//! How far a long command has got, and whether whoever waits for it has given up: what a front end shows while it opens a big
//! project on a thread of its own, and its Cancel. A session holds one ([`crate::engine::Session::progress`]); a command that
//! reads a file passes it to the read's [`crate::memory::Budget`], so every place a read checks its memory is a place it stops
//! when cancelled, with nothing changed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// What a cancelled command says.
pub const CANCELLED: &str = "cancelled";

/// A long command's progress: the stage it is at, how much of it is done of how much, and whether to stop.
#[derive(Debug, Default)]
pub struct Progress {
    stage: Mutex<&'static str>,
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    /// A new stage, `total` steps long (0: not counted).
    pub fn stage(&self, stage: &'static str, total: u64) {
        if let Ok(mut s) = self.stage.lock() {
            *s = stage;
        }
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// `n` more steps of the stage done.
    pub fn step(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    /// The stage, and how far through it: a fraction from 0 to 1, none where it isn't counted.
    pub fn now(&self) -> (&'static str, Option<f32>) {
        let stage = self.stage.lock().map_or("", |s| *s);
        let (done, total) = (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed));
        (stage, (total > 0).then(|| (done.min(total) as f64 / total as f64) as f32))
    }

    /// Asks the command to stop at the next place it looks.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// An error once cancelled.
    pub fn check(&self) -> Result<(), String> {
        if self.cancelled() { Err(CANCELLED.into()) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_says_its_stage_and_fraction_and_stops_once_cancelled() {
        let p = Progress::new();
        assert_eq!(p.now(), ("", None));
        p.stage("Reading elements", 4);
        p.step(1);
        assert_eq!(p.now(), ("Reading elements", Some(0.25)));
        p.step(10);
        assert_eq!(p.now().1, Some(1.0), "never past the whole");
        assert!(p.check().is_ok());
        p.cancel();
        assert_eq!(p.check(), Err(CANCELLED.to_string()));
    }
}
