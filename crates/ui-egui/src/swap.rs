//! The warning that the machine has started to swap: memory ran short and the system is paging to disk (swap on Linux and
//! macOS, the pagefile on Windows), so everything will be slower until memory is freed. The core decides
//! ([`bimcraft_core::memory::SwapWatch`]); this samples the machine every few seconds and shows a banner while it lasts.

use std::time::{Duration, Instant};

use bimcraft_core::memory::{self, Swap, SwapWatch};
use egui::{Frame, RichText, Stroke, Ui, vec2};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::style::pal;

/// How often the machine's memory is looked at.
const EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Swapping {
    watch: SwapWatch,
    last: Option<Instant>,
    /// How much more swap was in use when it began, bytes; and whether the user closed the banner.
    grown: Option<u64>,
    dismissed: bool,
}

impl BimApp {
    /// Samples the machine's memory, every [`EVERY`] at most.
    pub(crate) fn poll_swap(&mut self, ctx: &egui::Context) {
        let s = &mut self.swapping;
        if s.last.is_some_and(|l| l.elapsed() < EVERY) {
            return;
        }
        s.last = Some(Instant::now());
        let Some(p) = memory::pressure() else { return };
        match s.watch.sample(p) {
            Some(Swap::Began(grown)) => (s.grown, s.dismissed) = (Some(grown), false),
            Some(Swap::Ended) => s.grown = None,
            None => {}
        }
        // Asked again later even when nothing moves, so a swap that begins while the window idles is still seen.
        ctx.request_repaint_after(EVERY);
    }

    pub(crate) fn has_swap_banner(&self) -> bool {
        self.swapping.grown.is_some() && !self.swapping.dismissed
    }

    pub(crate) fn swap_banner(&mut self, ui: &mut Ui) {
        let Some(grown) = self.swapping.grown else { return };
        let frame = Frame::new().fill(pal().hover_fill).inner_margin(vec2(8.0, 4.0)).stroke(Stroke::new(1.0, pal().error));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("⚠").color(pal().error).strong());
                ui.label(
                    RichText::new(tf(
                        "This computer is out of memory and is using its disk instead ({} swapped): BIMCraft and other programs will be slow until memory is freed. Close other programs or views you don't need.",
                        &[&memory::bytes(grown)],
                    ))
                    .strong(),
                );
                if ui.button(t("Dismiss")).clicked() {
                    self.swapping.dismissed = true;
                }
            });
        });
    }
}
