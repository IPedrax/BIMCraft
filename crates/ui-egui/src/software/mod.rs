//! The window without a graphics card. Where a machine has none, wgpu draws on a software adapter (WARP, llvmpipe) that
//! emulates a whole graphics device first: measured on a two-core Windows VM, a frame that cost the app 4 to 7 ms took WARP 450 to
//! 1000 ms of both cores (`pacing.rs`). Here egui's own triangles are drawn on the processor instead (`raster.rs`) and shown
//! with `softbuffer`, in a window of winit's run by egui-winit as eframe runs its own (`window.rs`). On a machine with a
//! graphics card nothing changes: eframe and wgpu draw the window.
//!
//! Which one a start takes ([`choose`]): `BIMCRAFT_RENDERER=software` or `gpu`, the user's own, never kept; else Settings >
//! Performance > Drawing ([`Drawing`], in `settings.json`); else the processor when the last rung of the start's graphics
//! ladder was reached (`startup.rs`: every backend failed); else the processor when every adapter wgpu lists draws in
//! software ([`crate::pacing::software`]), or there is none; else the graphics card.

pub mod raster;
pub mod window;

use eframe::wgpu::{Backends, DeviceType};

/// What the window draws on, as the user chose it (Settings > Performance > Drawing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Drawing {
    /// The processor where the machine has no graphics card, else the card.
    #[default]
    Auto,
    Card,
    Processor,
}

impl Drawing {
    pub const ALL: [Drawing; 3] = [Drawing::Auto, Drawing::Card, Drawing::Processor];

    pub fn code(self) -> &'static str {
        match self {
            Drawing::Auto => "auto",
            Drawing::Card => "card",
            Drawing::Processor => "processor",
        }
    }

    pub fn from_code(code: &str) -> Option<Drawing> {
        Drawing::ALL.into_iter().find(|d| d.code() == code)
    }
}

/// What draws a start's window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// eframe on wgpu.
    Card,
    /// The processor, `raster.rs`.
    Processor,
}

/// What draws the window, and why, for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub renderer: Renderer,
    pub why: String,
}

impl Choice {
    fn of(renderer: Renderer, why: impl Into<String>) -> Choice {
        Choice { renderer, why: why.into() }
    }
}

/// What draws the window. `env` is `BIMCRAFT_RENDERER`, `setting` the user's Drawing, `last_rung` whether the starts before
/// failed on every graphics backend, and `adapters` the adapters wgpu lists, by type and name (none where they couldn't be
/// listed): asked only when nothing before decides.
pub fn choose(
    env: Option<&str>,
    setting: Drawing,
    last_rung: bool,
    adapters: impl FnOnce() -> Option<Vec<(DeviceType, String)>>,
) -> Choice {
    match env.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("software" | "processor" | "cpu") => return Choice::of(Renderer::Processor, "BIMCRAFT_RENDERER asks for the processor"),
        Some("gpu" | "card" | "wgpu") => return Choice::of(Renderer::Card, "BIMCRAFT_RENDERER asks for the graphics card"),
        _ => {}
    }
    match setting {
        Drawing::Processor => return Choice::of(Renderer::Processor, "Settings > Performance asks for the processor"),
        Drawing::Card => return Choice::of(Renderer::Card, "Settings > Performance asks for the graphics card"),
        Drawing::Auto => {}
    }
    if last_rung {
        return Choice::of(Renderer::Processor, "the starts before failed on every graphics backend");
    }
    match adapters() {
        None => Choice::of(Renderer::Card, "the adapters couldn't be listed"),
        Some(list) if list.is_empty() => Choice::of(Renderer::Processor, "no graphics adapter found"),
        Some(list) if list.iter().all(|(kind, name)| crate::pacing::software(*kind, name)) => {
            let names: Vec<&str> = list.iter().map(|(_, n)| n.as_str()).collect();
            Choice::of(Renderer::Processor, format!("no graphics card found, only {}", names.join(", ")))
        }
        Some(_) => Choice::of(Renderer::Card, "a graphics card was found"),
    }
}

/// The adapters wgpu lists on `backends`, by type and name: none where wgpu couldn't list them (it panicked, or didn't answer at
/// once). Listing makes no device.
pub fn adapters(backends: Backends) -> Option<Vec<(DeviceType, String)>> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    std::panic::catch_unwind(|| {
        let mut descriptor = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle().instance_descriptor;
        descriptor.backends = backends;
        let instance = eframe::wgpu::Instance::new(descriptor);
        let mut listing = std::pin::pin!(instance.enumerate_adapters(backends));
        // Native backends list at once; the future is ready on its first poll.
        match listing.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(list) => Some(
                list.iter()
                    .map(|a| {
                        let info = a.get_info();
                        (info.device_type, info.name)
                    })
                    .collect(),
            ),
            Poll::Pending => None,
        }
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(adapters: &[(DeviceType, &str)]) -> impl FnOnce() -> Option<Vec<(DeviceType, String)>> {
        let v: Vec<(DeviceType, String)> = adapters.iter().map(|(t, n)| (*t, n.to_string())).collect();
        move || Some(v)
    }

    #[test]
    fn only_software_adapters_or_none_draw_on_the_processor_and_any_card_draws_on_it() {
        let warp = [(DeviceType::Cpu, "Microsoft Basic Render Driver")];
        assert_eq!(choose(None, Drawing::Auto, false, list(&warp)).renderer, Renderer::Processor);
        assert_eq!(choose(None, Drawing::Auto, false, list(&[])).renderer, Renderer::Processor);
        let both = [(DeviceType::Cpu, "Microsoft Basic Render Driver"), (DeviceType::IntegratedGpu, "Intel(R) UHD Graphics")];
        assert_eq!(choose(None, Drawing::Auto, false, list(&both)).renderer, Renderer::Card);
        assert_eq!(choose(None, Drawing::Auto, false, list(&[(DeviceType::Other, "llvmpipe (LLVM 19)")])).renderer, Renderer::Processor);
        assert_eq!(choose(None, Drawing::Auto, false, || None).renderer, Renderer::Card, "not listed: as before");
        let why = choose(None, Drawing::Auto, false, list(&warp)).why;
        assert!(why.contains("Microsoft Basic Render Driver"), "{why}");
    }

    #[test]
    fn the_environment_then_the_setting_then_the_ladder_override_the_adapters_both_ways() {
        let card = [(DeviceType::DiscreteGpu, "NVIDIA GeForce RTX 2060")];
        let warp = [(DeviceType::Cpu, "Microsoft Basic Render Driver")];
        let never = || -> Option<Vec<(DeviceType, String)>> { panic!("not listed when something else decides") };
        assert_eq!(choose(Some("software"), Drawing::Card, false, never).renderer, Renderer::Processor);
        assert_eq!(choose(Some(" GPU "), Drawing::Processor, true, never).renderer, Renderer::Card);
        assert_eq!(choose(None, Drawing::Processor, false, never).renderer, Renderer::Processor);
        assert_eq!(choose(None, Drawing::Card, true, never).renderer, Renderer::Card);
        assert_eq!(choose(Some("bogus"), Drawing::Card, false, never).renderer, Renderer::Card, "a word it doesn't know is not set");
        assert_eq!(choose(None, Drawing::Auto, true, never).renderer, Renderer::Processor, "every backend failed");
        assert_eq!(choose(None, Drawing::Auto, false, list(&card)).renderer, Renderer::Card);
        assert_eq!(choose(Some(""), Drawing::Auto, false, list(&warp)).renderer, Renderer::Processor);
    }

    #[test]
    fn the_drawing_setting_reads_back_as_written() {
        for d in Drawing::ALL {
            assert_eq!(Drawing::from_code(d.code()), Some(d));
        }
        assert_eq!(Drawing::from_code("gpu"), None);
    }
}
