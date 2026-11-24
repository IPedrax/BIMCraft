//! How the window's frames reach the screen. On Linux and macOS the system's vertical sync paces them (wgpu's `AutoVsync`). On
//! Windows it doesn't: measured with `bimcraft --bench` on a Radeon RX 590 at 59 Hz (2026-11-15), a frame that took the window
//! 2 to 4 ms came every 31 ms over Vulkan with vsync, every 33 to 47 ms over OpenGL and every 265 ms over DirectX 12, whatever the
//! frame latency or the timer's resolution; without vsync the same frames came every 1.3 to 3.6 ms. So on Windows the window
//! presents without the driver's vsync (DirectX 12's Mailbox, which never tears; Immediate elsewhere, which may tear, Mailbox or
//! Fifo where that is missing) and paces itself to the monitor's refresh rate, so a view that keeps asking for frames never runs faster than the
//! screen shows them. `BIMCRAFT_VSYNC=1` keeps the driver's vsync, as before.
//!
//! **No graphics card.** On a machine without one wgpu draws on a software adapter: Direct3D's WARP ("Microsoft Basic Render
//! Driver") on Windows, llvmpipe or lavapipe on Linux. Measured on a two-core Windows 11 VM with WARP (2026-11-24, 0.12.0), a frame
//! that cost the window 4 to 7 ms took WARP 450 to 1000 ms of both cores: moving the pointer over a plan held the process at 1.78
//! of 2 cores, it stayed there for 2 s after the pointer stopped (two frames queued), and the pan's frames alternated between
//! 1050 ms and 50 to 90 ms, the swap chain's acquire timing out. So on a software adapter ([`software`]) the window presents with
//! Fifo and one frame in flight ([`surface_for`]) and paces frames by their cost ([`Pacer::every_for`]): after a frame that took
//! `c` it waits until `c / BUDGET` has passed, leaving the rest of the processor to the user's other applications. Input that
//! arrives meanwhile isn't lost: egui gathers it into the next frame, and `BimApp::pace` still splits a frame after a click. The
//! user's limit (Settings > Performance, [`Limit`]) caps the rate on any adapter.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use eframe::egui_wgpu::SurfaceConfig;
use eframe::wgpu::{Backend, DeviceType, PresentMode};

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

/// The share of the processor a software adapter's frames may take while the user works: after a frame that took `c`, the next
/// begins no sooner than `c / BUDGET` after it.
pub const BUDGET: f64 = 0.5;
/// A gap between frames longer than this is a pause, not a frame's cost (egui draws nothing while nothing happens). wgpu waits at
/// most a second for the swap chain, so a frame of WARP's that timed out still counts.
const PAUSE: Duration = Duration::from_millis(1500);
/// How much of a new frame's cost the running average takes: one slow frame doesn't stall the next.
const EMA: f64 = 0.3;
/// The longest a frame ever waits for its budget, whatever the frames before cost.
const MAX_WAIT: Duration = Duration::from_millis(1000);

/// The names of software rasterizers, for drivers that call their adapter `Other` rather than `Cpu`.
const SOFTWARE_NAMES: [&str; 5] = ["basic render driver", "llvmpipe", "lavapipe", "swiftshader", "softpipe"];

/// Whether an adapter of `device_type` named `name` draws in software, on the processor: no graphics card.
pub fn software(device_type: DeviceType, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    device_type == DeviceType::Cpu || SOFTWARE_NAMES.iter().any(|n| name.contains(n))
}

/// The surface a window presents to once its adapter is known. A paced window on DirectX 12 presents with Mailbox
/// ([`present_for`]). A software adapter presents with Fifo, which never draws a frame only to throw it away, and keeps one frame in
/// flight rather than two, so the processor is free again as soon as the pointer stops. Anything else keeps `config`.
pub fn surface_for(backend: Backend, software: bool, paced: bool, config: SurfaceConfig) -> SurfaceConfig {
    if software {
        SurfaceConfig { present_mode: PresentMode::AutoVsync, desired_maximum_frame_latency: Some(1) }
    } else if paced {
        SurfaceConfig { present_mode: present_for(backend, config.present_mode), ..config }
    } else {
        config
    }
}

/// The user's limit on the frame rate (Settings > Performance).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Limit {
    /// The monitor's rate on a graphics card; on a software adapter, the rate the frames' cost allows ([`BUDGET`]).
    #[default]
    Auto,
    /// At most this many frames a second.
    Hz(u32),
}

impl Limit {
    /// The choices Settings offers.
    pub const ALL: [Limit; 5] = [Limit::Auto, Limit::Hz(60), Limit::Hz(30), Limit::Hz(15), Limit::Hz(10)];

    /// As the settings file keeps it: `"auto"` or the rate.
    pub fn to_json(self) -> serde_json::Value {
        match self {
            Limit::Auto => serde_json::json!("auto"),
            Limit::Hz(hz) => serde_json::json!(hz),
        }
    }

    /// Read back from the settings file: `"auto"`, or one of the rates offered; anything else is none.
    pub fn from_json(v: &serde_json::Value) -> Option<Limit> {
        if v.as_str() == Some("auto") {
            return Some(Limit::Auto);
        }
        let hz = u32::try_from(v.as_u64()?).ok()?;
        Limit::ALL.into_iter().find(|l| *l == Limit::Hz(hz))
    }

    /// The shortest time between frames it allows: none for Auto.
    pub fn interval(self) -> Option<Duration> {
        match self {
            Limit::Auto => None,
            Limit::Hz(hz) => Some(Duration::from_secs_f64(1.0 / f64::from(hz.clamp(1, 1000)))),
        }
    }
}

/// Frames whose surface couldn't be had (wgpu's acquire timed out, the surface was lost, outdated or hidden), since the start:
/// `main` counts them from egui-wgpu's `on_surface_status`, and the benchmark reports them.
static SURFACE_FAILURES: AtomicU64 = AtomicU64::new(0);

/// A frame's surface couldn't be had.
pub fn surface_failed() {
    SURFACE_FAILURES.fetch_add(1, Ordering::Relaxed);
}

/// The frames whose surface couldn't be had, since the start.
pub fn surface_failures() -> u64 {
    SURFACE_FAILURES.load(Ordering::Relaxed)
}

/// Whether egui-wgpu not getting a frame's surface with `status` counts as a failure: a hidden or minimised window's frames
/// are skipped by design, and aren't counted.
pub fn counts_as_failure(status: &eframe::wgpu::CurrentSurfaceTexture) -> bool {
    use eframe::wgpu::CurrentSurfaceTexture as S;
    !matches!(status, S::Success(_) | S::Suboptimal(_) | S::Occluded)
}

/// The adapter `BIMCRAFT_SOFTWARE=1` draws on, among adapters of these types and names: the first software one
/// ([`software`]), DirectX 12's WARP on Windows, lavapipe or llvmpipe on Linux. None where the system has none.
pub fn software_choice<'a>(adapters: impl IntoIterator<Item = (DeviceType, &'a str)>) -> Option<usize> {
    adapters.into_iter().position(|(kind, name)| software(kind, name))
}

/// Whether the environment asks for a software adapter (`BIMCRAFT_SOFTWARE`, anything but empty or `0`): the user's own choice,
/// as `WGPU_BACKEND` is, never kept. It shows on a machine with a graphics card what a machine without one does.
pub fn software_asked() -> bool {
    std::env::var("BIMCRAFT_SOFTWARE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The time between frames for a monitor of `millihertz` (none where it doesn't say).
pub fn interval(millihertz: Option<u32>) -> Duration {
    let hz = millihertz.map(|m| f64::from(m) / 1000.0).filter(|hz| hz.is_finite() && *hz >= MIN_HZ).unwrap_or(DEFAULT_HZ);
    Duration::from_secs_f64(1.0 / hz.min(MAX_HZ))
}

/// A window pacing its own frames.
#[derive(Debug, Default)]
pub struct Pacer {
    /// Whether it paces to the monitor's refresh (Windows: [`Policy::paced`]); elsewhere the driver's vsync does.
    pub monitor: bool,
    /// Whether the window draws on a software adapter ([`software`]): frames are then paced by their cost.
    pub software: bool,
    /// The user's limit.
    pub limit: Limit,
    /// The running average of what a frame costs: the time from its start to the next frame's, while frames follow each other.
    cost: Option<Duration>,
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
    /// Whether it waits between frames at all: where it neither paces to the monitor, draws in software nor has a limit, the
    /// driver's vsync paces the window.
    pub fn active(&self) -> bool {
        self.monitor || self.software || self.limit != Limit::Auto
    }

    /// The running average of a frame's cost, once frames have followed each other.
    pub fn cost(&self) -> Option<Duration> {
        self.cost
    }

    /// The time between frames for a frame beginning at `now`, given the monitor's `every`: the monitor's, no less than the user's
    /// limit, and on a software adapter no less than the frames' cost over [`BUDGET`]. The frame before's cost is taken into the
    /// average first, unless a pause came between them.
    pub fn every_for(&mut self, now: Instant, monitor: Duration) -> Duration {
        if let Some(last) = self.last {
            let took = now.saturating_duration_since(last);
            if took <= PAUSE {
                let ema = self.cost.map_or(took.as_secs_f64(), |c| c.as_secs_f64() + EMA * (took.as_secs_f64() - c.as_secs_f64()));
                self.cost = Some(Duration::from_secs_f64(ema.clamp(0.0, PAUSE.as_secs_f64())));
            }
        }
        let mut every = if self.monitor { monitor } else { Duration::ZERO };
        if let Some(limit) = self.limit.interval() {
            every = every.max(limit);
        }
        if self.software
            && let Some(cost) = self.cost
        {
            every = every.max(cost.div_f64(BUDGET).min(cost + MAX_WAIT));
        }
        every
    }

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
    fn a_software_adapter_is_one_of_type_cpu_or_a_known_rasterizers_name() {
        assert!(software(DeviceType::Cpu, "anything"));
        for name in
            ["Microsoft Basic Render Driver", "llvmpipe (LLVM 19.1.7, 256 bits)", "lavapipe", "SwiftShader Device (Subzero)", "softpipe"]
        {
            assert!(software(DeviceType::Other, name), "{name}");
        }
        assert!(!software(DeviceType::DiscreteGpu, "NVIDIA GeForce RTX 2060"));
        assert!(!software(DeviceType::IntegratedGpu, "AMD Radeon(TM) Graphics"));
        assert!(!software(DeviceType::VirtualGpu, "Red Hat QXL"));
        assert!(!software(DeviceType::Other, ""));
    }

    #[test]
    fn a_software_adapter_presents_with_fifo_and_one_frame_in_flight() {
        let start = SurfaceConfig { present_mode: PresentMode::AutoNoVsync, desired_maximum_frame_latency: Some(2) };
        let soft = SurfaceConfig { present_mode: PresentMode::AutoVsync, desired_maximum_frame_latency: Some(1) };
        assert_eq!(surface_for(Backend::Dx12, true, true, start), soft, "WARP: Fifo, not Mailbox's frames thrown away");
        assert_eq!(surface_for(Backend::Vulkan, true, false, start), soft, "lavapipe on Linux too");
        assert_eq!(surface_for(Backend::Dx12, false, true, start), SurfaceConfig { present_mode: PresentMode::Mailbox, ..start });
        assert_eq!(surface_for(Backend::Vulkan, false, true, start), start);
        assert_eq!(surface_for(Backend::Vulkan, false, false, start), start, "a card on Linux keeps its vsync and its queue");
    }

    #[test]
    fn a_software_adapter_waits_the_frames_cost_again() {
        let mut p = Pacer { software: true, ..Pacer::default() };
        let monitor = Duration::from_millis(16);
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        // The first frame has no cost yet: the monitor's interval doesn't apply (the pacer isn't the monitor's), nothing to wait.
        assert_eq!(p.every_for(t0, monitor), Duration::ZERO);
        assert_eq!(p.wait(t0, Duration::ZERO), Duration::ZERO);
        // The next comes 400 ms later: that frame cost 400 ms, so frames come no closer than 800 ms, and this one waits 400.
        let every = p.every_for(t0 + ms(400), monitor);
        assert_eq!(every, ms(800));
        assert_eq!(p.wait(t0 + ms(400), every), ms(400));
        assert_eq!(p.cost(), Some(ms(400)));
    }

    #[test]
    fn one_slow_frame_moves_the_average_only_part_of_the_way() {
        let mut p = Pacer { software: true, ..Pacer::default() };
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        p.every_for(t0, Duration::ZERO);
        p.wait(t0, Duration::ZERO);
        p.every_for(t0 + ms(100), Duration::ZERO);
        p.wait(t0 + ms(100), Duration::ZERO);
        assert_eq!(p.cost(), Some(ms(100)));
        // A frame of 1100 ms: the average goes to 400, not 1100.
        let every = p.every_for(t0 + ms(1200), Duration::ZERO);
        let cost = p.cost().map_or(0.0, |c| c.as_secs_f64() * 1000.0);
        assert!((cost - 400.0).abs() < 0.01, "{cost}");
        assert!((every.as_secs_f64() * 1000.0 - 800.0).abs() < 0.01);
    }

    #[test]
    fn a_frame_after_a_pause_still_waits_nothing() {
        let mut p = Pacer { software: true, ..Pacer::default() };
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        p.every_for(t0, Duration::ZERO);
        p.wait(t0, Duration::ZERO);
        let every = p.every_for(t0 + ms(400), Duration::ZERO);
        p.wait(t0 + ms(400), every);
        // Ten seconds of nothing: the gap is a pause, not a cost, and the frame that ends it begins at once.
        let later = t0 + Duration::from_secs(10);
        let every = p.every_for(later, Duration::ZERO);
        assert_eq!(p.cost(), Some(ms(400)), "the pause isn't counted");
        assert_eq!(p.wait(later, every), Duration::ZERO);
    }

    #[test]
    fn the_wait_for_a_budget_is_never_more_than_a_second() {
        let mut p = Pacer { software: true, ..Pacer::default() };
        let t0 = Instant::now();
        p.every_for(t0, Duration::ZERO);
        p.wait(t0, Duration::ZERO);
        let every = p.every_for(t0 + Duration::from_millis(1400), Duration::ZERO);
        assert_eq!(every, Duration::from_millis(2400), "1.4 s of frame and at most 1 s of rest");
    }

    #[test]
    fn the_users_limit_caps_the_rate() {
        let ms = |n: u64| Duration::from_millis(n);
        let t0 = Instant::now();
        // On a card paced to a 60 Hz monitor, a limit of 15 makes it 15.
        let mut card = Pacer { monitor: true, limit: Limit::Hz(15), ..Pacer::default() };
        let every = card.every_for(t0, ms(16));
        assert!((every.as_secs_f64() - 1.0 / 15.0).abs() < 1e-9);
        // Where the driver's vsync paces (Linux), a limit still makes the pacer wait.
        assert!(!Pacer::default().active());
        assert!(Pacer { limit: Limit::Hz(30), ..Pacer::default() }.active());
        // On a software adapter, whichever is slower: the budget, or the limit.
        let mut soft = Pacer { software: true, limit: Limit::Hz(10), ..Pacer::default() };
        soft.every_for(t0, Duration::ZERO);
        soft.wait(t0, Duration::ZERO);
        assert_eq!(soft.every_for(t0 + ms(20), Duration::ZERO), ms(100), "a cheap frame: the limit");
        let mut soft = Pacer { software: true, limit: Limit::Hz(10), ..Pacer::default() };
        soft.every_for(t0, Duration::ZERO);
        soft.wait(t0, Duration::ZERO);
        assert_eq!(soft.every_for(t0 + ms(300), Duration::ZERO), ms(600), "a costly frame: the budget");
    }

    #[test]
    fn a_timed_out_or_lost_surface_is_a_failure_and_a_hidden_window_is_not() {
        use eframe::wgpu::CurrentSurfaceTexture as S;
        for status in [S::Timeout, S::Outdated, S::Lost, S::Validation] {
            assert!(counts_as_failure(&status));
        }
        assert!(!counts_as_failure(&S::Occluded));
    }

    #[test]
    fn the_software_adapter_asked_for_is_the_first_one_found() {
        let adapters = [
            (DeviceType::DiscreteGpu, "NVIDIA GeForce RTX 2060"),
            (DeviceType::Cpu, "Microsoft Basic Render Driver"),
            (DeviceType::Cpu, "llvmpipe"),
        ];
        assert_eq!(software_choice(adapters), Some(1));
        assert_eq!(software_choice([(DeviceType::IntegratedGpu, "Intel(R) UHD Graphics 620")]), None);
        assert_eq!(software_choice([]), None);
    }

    #[test]
    fn the_limit_reads_back_as_written_and_a_rate_not_offered_is_none() {
        for limit in Limit::ALL {
            assert_eq!(Limit::from_json(&limit.to_json()), Some(limit));
        }
        for odd in
            [serde_json::json!(7), serde_json::json!(-1), serde_json::json!("fast"), serde_json::json!(1e30), serde_json::Value::Null]
        {
            assert_eq!(Limit::from_json(&odd), None, "{odd}");
        }
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
