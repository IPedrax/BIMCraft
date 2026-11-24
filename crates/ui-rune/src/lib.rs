//! BIMCraft's desktop front end in Rune, a thin shell over the core's `Session` (see `docs/areas/rune-ui.md`: stage 1 of a migration
//! the egui front end stays the shipped UI through). The core knows no UI toolkit and this crate is no dependency of it.
//!
//! The layout: `app` is the hub, the state and what its handlers do, which meets the core and holds no widget; `ui` is the
//! elements, a function of that state; the rest are what both need and a test reaches without a window: `camera` (pan and zoom),
//! `paint` (a plan's display list as shapes), `props` (the Properties rows), `status` (the status line) and `lang` (the words).

#![forbid(unsafe_code)]

pub mod app;
pub mod camera;
pub mod lang;
pub mod paint;
pub mod props;
pub mod status;
pub mod ui;

use rune::AppSpec;

/// The window's title and size, points.
pub const TITLE: &str = "BIMCraft (Rune)";
pub const SIZE: (f32, f32) = (1280.0, 800.0);

/// Opens the window on the sample house, in the system's language.
pub fn run() {
    lang::set(lang::from_system());
    rune::set_app_id("com.ipedrax.bimcraft");
    rune::run(AppSpec { on_key: Some(ui::on_key), ..AppSpec::new(TITLE, SIZE, app::App::sample, ui::view) });
}
