//! A thread that makes the latest of what it is asked, for the views whose picture costs more than a frame (a section, later the 3D
//! view). One request is made at a time; a newer request cancels the one being made (the job asks `cancelled` between its chunks, the
//! core's `*_cancellable` calls take exactly that) and replaces any that were waiting, so only the newest is ever answered. The window
//! polls `take` every frame (`ui::poll`) and asks for a frame while `busy`, so the thread needs no handle on the window.
//!
//! The same rule as egui's `view_worker.rs` (bounded mailboxes, latest request wins), in one thread of its own for each view that
//! wants it, which `Drop` stops. A job that panics is an error, not a crash: the next request goes on.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

/// What a job asks between its chunks: whether a newer request has made it pointless.
pub type Cancelled<'a> = &'a (dyn Fn() -> bool + Sync);

pub struct Latest<I: Send + 'static, O: Send + 'static> {
    ask: mpsc::Sender<(u64, I)>,
    answers: mpsc::Receiver<(u64, Option<O>)>,
    /// The number of the newest request: a job with an older number is cancelled.
    newest: Arc<AtomicU64>,
    /// The last number the thread is through with (it skips what a newer request replaced).
    finished: Arc<AtomicU64>,
    asked: u64,
}

impl<I: Send + 'static, O: Send + 'static> Latest<I, O> {
    /// Starts the thread, which owns `render`. An Err when the system gives no thread.
    pub fn new(mut render: impl FnMut(I, Cancelled<'_>) -> Option<O> + Send + 'static) -> Result<Self, String> {
        let (ask, requests) = mpsc::channel::<(u64, I)>();
        let (reply, answers) = mpsc::channel::<(u64, Option<O>)>();
        let (newest, finished) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let (seen, done) = (Arc::clone(&newest), Arc::clone(&finished));
        std::thread::Builder::new()
            .name("bimcraft-view".into())
            .spawn(move || {
                while let Ok((mut n, mut input)) = requests.recv() {
                    // Only the newest of what is waiting is worth making.
                    while let Ok(next) = requests.try_recv() {
                        (n, input) = next;
                    }
                    let cancelled = || seen.load(Ordering::Acquire) != n;
                    let made = catch_unwind(AssertUnwindSafe(|| render(input, &cancelled))).ok().flatten();
                    // A cancelled job says nothing; the answer is sent before the thread says it is through.
                    let sent = cancelled() || reply.send((n, made)).is_ok();
                    done.store(n, Ordering::Release);
                    if !sent {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Latest { ask, answers, newest, finished, asked: 0 })
    }

    /// Asks for `input` to be made: the job being made is cancelled and whatever waits is dropped.
    pub fn request(&mut self, input: I) -> Result<(), String> {
        self.asked += 1;
        self.newest.store(self.asked, Ordering::Release);
        self.ask.send((self.asked, input)).map_err(|_| "the view's thread has stopped".to_string())
    }

    /// The answer to the newest request, once there is one: an Err when the job failed (or panicked). Older answers are dropped.
    pub fn take(&mut self) -> Option<Result<O, String>> {
        let mut got = None;
        while let Ok((n, made)) = self.answers.try_recv() {
            if n == self.asked {
                got = Some(made.ok_or_else(|| "the view could not be made".to_string()));
            }
        }
        got
    }

    /// Whether the newest request is not answered yet.
    pub fn busy(&self) -> bool {
        self.finished.load(Ordering::Acquire) < self.asked
    }
}

impl<I: Send + 'static, O: Send + 'static> Drop for Latest<I, O> {
    fn drop(&mut self) {
        // The job running now is cancelled: no request is numbered this.
        self.newest.store(u64::MAX, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
