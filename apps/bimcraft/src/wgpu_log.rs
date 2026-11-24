//! What wgpu, egui-wgpu and eframe warn about, in `bimcraft.log`. They write through the `log` crate, and with no logger
//! installed their warnings went nowhere: a frame dropped because the swap chain timed out said nothing. Only warnings and errors
//! from those crates are kept, and few of them ([`Limiter`]): the first [`FIRST`] lines of a start, then one line a minute that counts
//! the rest, so a storm of dropped frames can't grow the log (which `startup` also starts over past 64 KiB).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// The lines written as they come.
pub const FIRST: u32 = 20;
/// After those, how often a line counts what was held back.
pub const EVERY: Duration = Duration::from_secs(60);
/// The most of one message that's kept.
const MAX_LINE: usize = 400;
/// The crates whose warnings are kept.
const TARGETS: [&str; 5] = ["wgpu", "naga", "egui_wgpu", "eframe", "egui_winit"];

/// What becomes of a warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    /// Written.
    Write,
    /// Held back, and only counted.
    Hold,
    /// Held back, and this many were held back since the last count (this one among them): write that.
    Count(u64),
}

/// The first [`FIRST`] warnings, then a count at most every [`EVERY`].
#[derive(Debug, Default)]
pub struct Limiter {
    written: u32,
    held: u64,
    /// When the last line was written: the [`FIRST`]th, or the last count.
    since: Option<Instant>,
}

impl Limiter {
    pub fn admit(&mut self, now: Instant) -> Admit {
        if self.written < FIRST {
            self.written += 1;
            self.since = Some(now);
            return Admit::Write;
        }
        self.held += 1;
        if self.since.is_none_or(|t| now.saturating_duration_since(t) >= EVERY) {
            self.since = Some(now);
            return Admit::Count(std::mem::take(&mut self.held));
        }
        Admit::Hold
    }

    /// What was held back and not yet counted.
    #[cfg(test)]
    pub fn held(&self) -> u64 {
        self.held
    }
}

/// Whether a record from `target` at `level` is one the log keeps.
pub fn kept(target: &str, level: Level) -> bool {
    level <= Level::Warn && TARGETS.iter().any(|t| target == *t || target.strip_prefix(t).is_some_and(|rest| rest.starts_with(['_', ':'])))
}

/// One line of the log for a record: its level, where it came from, and the message, cut short.
pub fn line(level: Level, target: &str, message: &str) -> String {
    let mut text: String = message.chars().take(MAX_LINE).collect();
    if text.len() < message.len() {
        text.push('…');
    }
    format!("{level} {target}: {}", text.replace(['\r', '\n'], " "))
}

struct Logger {
    path: PathBuf,
    limiter: Mutex<Limiter>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        kept(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let admit = match self.limiter.lock() {
            Ok(mut l) => l.admit(Instant::now()),
            Err(poisoned) => poisoned.into_inner().admit(Instant::now()),
        };
        match admit {
            Admit::Write => crate::startup::note(&self.path, &line(record.level(), record.target(), &record.args().to_string())),
            Admit::Count(n) => crate::startup::note(&self.path, &format!("{n} more graphics warnings held back since the last line")),
            Admit::Hold => {}
        }
    }

    fn flush(&self) {
        let held = match self.limiter.lock() {
            Ok(mut l) => std::mem::take(&mut l.held),
            Err(poisoned) => std::mem::take(&mut poisoned.into_inner().held),
        };
        if held > 0 {
            crate::startup::note(&self.path, &format!("{held} more graphics warnings held back since the last line"));
        }
    }
}

/// Sends the graphics crates' warnings to the log at `path` from now on. Another logger already installed is left alone.
pub fn install(path: PathBuf) {
    if log::set_boxed_logger(Box::new(Logger { path, limiter: Mutex::new(Limiter::default()) })).is_ok() {
        log::set_max_level(LevelFilter::Warn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_lines_are_written_then_one_a_minute_counts_the_rest() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        for _ in 0..FIRST {
            assert_eq!(l.admit(t0), Admit::Write);
        }
        for i in 1..=500 {
            assert_eq!(l.admit(t0 + Duration::from_millis(i * 50)), Admit::Hold, "a storm within the minute is held");
        }
        assert_eq!(l.held(), 500);
        assert_eq!(l.admit(t0 + EVERY), Admit::Count(501), "a minute on, one line counts them");
        assert_eq!(l.held(), 0);
        assert_eq!(l.admit(t0 + EVERY + Duration::from_secs(1)), Admit::Hold);
        assert_eq!(l.admit(t0 + EVERY * 3), Admit::Count(2));
    }

    #[test]
    fn only_the_graphics_crates_warnings_are_kept() {
        for target in ["wgpu", "wgpu_core::present", "wgpu_hal::dx12", "naga::front", "egui_wgpu::winit", "eframe::native", "egui_winit"] {
            assert!(kept(target, Level::Warn), "{target}");
            assert!(kept(target, Level::Error), "{target}");
            assert!(!kept(target, Level::Info), "{target}: not info");
        }
        for target in ["ureq", "rustls", "wgpuish", "bimcraft", ""] {
            assert!(!kept(target, Level::Error), "{target}");
        }
    }

    #[test]
    fn a_line_is_one_line_and_a_long_message_is_cut_short() {
        assert_eq!(
            line(Level::Warn, "egui_wgpu", "Dropped frame with error: Timeout"),
            "WARN egui_wgpu: Dropped frame with error: Timeout"
        );
        assert_eq!(line(Level::Error, "wgpu", "a\nb\r\nc"), "ERROR wgpu: a b  c");
        let long = line(Level::Warn, "wgpu", &"é".repeat(5000));
        assert!(long.chars().count() < MAX_LINE + 40 && long.ends_with('…'), "{}", long.len());
    }

    #[test]
    fn a_poisoned_lock_still_logs() {
        let dir = tempfile_dir();
        let path = dir.join("bimcraft.log");
        let logger = Logger { path: path.clone(), limiter: Mutex::new(Limiter::default()) };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = logger.limiter.lock();
            panic!("poison it");
        }));
        logger.log(&Record::builder().level(Level::Warn).target("wgpu_core").args(format_args!("timed out")).build());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("WARN wgpu_core: timed out"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bimcraft-wgpu-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
