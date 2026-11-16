//! How the window's frames reach the screen. On Linux and macOS the system's vertical sync paces them (wgpu's `AutoVsync`). On
//! Windows it doesn't: measured with `bimcraft --bench` on a Radeon RX 590 at 59 Hz (2026-11-15), a frame that took the window
//! 2 to 4 ms came every 31 ms over Vulkan with vsync, every 33 to 47 ms over OpenGL and every 265 ms over DirectX 12, whatever the
//! frame latency or the timer's resolution; without vsync the same frames came every 1.3 to 3.6 ms. So on Windows the window
//! presents without the driver's vsync (DirectX 12's Mailbox, which never tears; Immediate elsewhere, which may tear, Mailbox or
//! Fifo where that is missing) and paces itself to the monitor's refresh rate, so a view that keeps asking for frames never runs faster than the
//! screen shows them. `BIMCRAFT_VSYNC=1` keeps the driver's vsync, as before.

use std::time::{Duration, Instant};

use eframe::wgpu::{Backend, PresentMode};

/// The refresh rate assumed where the monitor doesn't say.
const DEFAULT_HZ: f64 = 60.0;
/// The rates a monitor's is held to: a lower one is a mistake, a higher one leaves the pacing to the card.
const MIN_HZ: f64 = 24.0;
const MAX_HZ: f64 = 500.0;
/// How often the monitor's rate is asked again (the window may have moved to another).
const ASK_EVERY: Duration = Duration::from_secs(1);

/// Whether the window paces its own frames on this system, and with what it presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// What the window presents with before the card is known.
    pub start: PresentMode,
    /// Whether the window waits between frames itself.
    pub paced: bool,
}

/// The policy where `windows` says the system is Windows and `driver_vsync` that the user asked for the driver's vsync.
pub fn policy(windows: bool, driver_vsync: bool) -> Policy {
    if windows && !driver_vsync {
        Policy { start: PresentMode::AutoNoVsync, paced: true }
    } else {
        Policy { start: PresentMode::AutoVsync, paced: false }
    }
}

/// This system's policy: the environment's `BIMCRAFT_VSYNC` (anything but empty or `0`) keeps the driver's vsync.
pub fn system_policy() -> Policy {
    let driver = std::env::var("BIMCRAFT_VSYNC").is_ok_and(|v| !v.is_empty() && v != "0");
    policy(cfg!(windows), driver)
}

/// What a paced window presents with once its card's `backend` is known: DirectX 12 always has Mailbox, which shows the newest
/// frame at the next refresh and never tears; elsewhere the start's choice stays.
pub fn present_for(backend: Backend, start: PresentMode) -> PresentMode {
    if backend == Backend::Dx12 { PresentMode::Mailbox } else { start }
}

/// The time between frames for a monitor of `millihertz` (none where it doesn't say).
pub fn interval(millihertz: Option<u32>) -> Duration {
    let hz = millihertz.map(|m| f64::from(m) / 1000.0).filter(|hz| hz.is_finite() && *hz >= MIN_HZ).unwrap_or(DEFAULT_HZ);
    Duration::from_secs_f64(1.0 / hz.min(MAX_HZ))
}

/// A window pacing its own frames.
#[derive(Debug, Default)]
pub struct Pacer {
    /// When the frame before began, once it has.
    last: Option<Instant>,
    /// The time between frames, and when the monitor was last asked.
    every: Option<(Duration, Instant)>,
    /// The present mode is set once the card is known.
    pub(crate) configured: bool,
    /// What the frame before waited, and this one: eframe counts a frame's wait in its `cpu_usage`, which the benchmark leaves out.
    pub(crate) slept: (Duration, Duration),
}

impl Pacer {
    /// How long a frame beginning at `now` waits so that frames come no closer than `every`: a frame after a pause waits nothing.
    pub fn wait(&mut self, now: Instant, every: Duration) -> Duration {
        let wait = self.last.map_or(Duration::ZERO, |last| every.saturating_sub(now.saturating_duration_since(last)));
        self.last = Some(now + wait);
        self.slept = (self.slept.1, wait);
        wait
    }

    /// The time between frames, from `ask` (the monitor's rate in millihertz) at most once every [`ASK_EVERY`].
    pub fn every(&mut self, now: Instant, ask: impl FnOnce() -> Option<u32>) -> Duration {
        match self.every {
            Some((every, asked)) if now.saturating_duration_since(asked) < ASK_EVERY => every,
            _ => {
                let every = interval(ask());
                self.every = Some((every, now));
                every
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_presents_without_the_drivers_vsync_and_paces_itself_and_other_systems_keep_vsync() {
        assert_eq!(policy(true, false), Policy { start: PresentMode::AutoNoVsync, paced: true });
        assert_eq!(policy(true, true), Policy { start: PresentMode::AutoVsync, paced: false }, "BIMCRAFT_VSYNC keeps the driver's");
        assert_eq!(policy(false, false), Policy { start: PresentMode::AutoVsync, paced: false });
        assert_eq!(present_for(Backend::Dx12, PresentMode::AutoNoVsync), PresentMode::Mailbox, "DirectX 12 never tears");
        assert_eq!(present_for(Backend::Vulkan, PresentMode::AutoNoVsync), PresentMode::AutoNoVsync);
        assert_eq!(present_for(Backend::Gl, PresentMode::AutoNoVsync), PresentMode::AutoNoVsync);
    }

    #[test]
    fn the_interval_is_the_monitors_refresh_and_a_rate_it_cannot_have_is_sixty_hertz() {
        let ms = |m: Option<u32>| interval(m).as_secs_f64() * 1000.0;
        assert!((ms(Some(59_951)) - 16.680).abs() < 0.01, "{}", ms(Some(59_951)));
        assert!((ms(Some(144_000)) - 6.944).abs() < 0.01);
        for odd in [None, Some(0), Some(1), Some(23_000)] {
            assert!((ms(odd) - 16.667).abs() < 0.01, "{odd:?}: {}", ms(odd));
        }
        assert!((ms(Some(u32::MAX)) - 2.0).abs() < 0.01, "held to 500 Hz");
    }

    #[test]
    fn frames_come_no_closer_than_the_interval_and_a_frame_after_a_pause_waits_nothing() {
        let every = Duration::from_millis(16);
        let mut p = Pacer::default();
        let t0 = Instant::now();
        assert_eq!(p.wait(t0, every), Duration::ZERO, "the first frame");
        // Asked 3 ms later: waits the rest, and the next frame counts from when this one began after its wait.
        assert_eq!(p.wait(t0 + Duration::from_millis(3), every), Duration::from_millis(13));
        assert_eq!(p.wait(t0 + Duration::from_millis(20), every), Duration::from_millis(12));
        assert_eq!(p.slept, (Duration::from_millis(13), Duration::from_millis(12)), "the frame before's wait, and this one's");
        // After a pause longer than a frame, at once.
        assert_eq!(p.wait(t0 + Duration::from_secs(5), every), Duration::ZERO);
        assert_eq!(p.slept, (Duration::from_millis(12), Duration::ZERO));
    }

    #[test]
    fn the_monitor_is_asked_at_most_once_a_second() {
        let mut p = Pacer::default();
        let t0 = Instant::now();
        let mut asked = 0;
        for i in 0..100 {
            p.every(t0 + Duration::from_millis(i * 5), || {
                asked += 1;
                Some(60_000)
            });
        }
        assert_eq!(asked, 1);
        assert_eq!(
            p.every(t0 + Duration::from_millis(1500), || {
                asked += 1;
                Some(120_000)
            }),
            interval(Some(120_000))
        );
        assert_eq!(asked, 2);
    }
}
