//! BIMCraft's desktop front end in Rune, a thin shell over the core's `Session` (see `docs/areas/rune-ui.md`: stage 1 of a migration
//! the egui front end stays the shipped UI through). The core knows no UI toolkit and this crate is no dependency of it.
//!
//! The layout: `app` is the hub, the state and what its handlers do, which meets the core and holds no widget; `ui` is the
//! elements, a function of that state; the rest are what both need and a test reaches without a window: `camera` (pan and zoom),
//! `paint` (a plan's display list as shapes), `props` (the Properties rows), `select` (what a click or a box does to the
//! selection), `snapping` (where a picked point lands), `status` (the status line) and `lang` (the words).

#![forbid(unsafe_code)]

pub mod app;
pub mod camera;
pub mod finder;
pub mod forms;
pub mod lang;
pub mod paint;
pub mod profile;
pub mod project;
pub mod props;
pub mod select;
pub mod settings;
pub mod snapping;
pub mod software;
pub mod status;
pub mod tools;
pub mod typeprops;
pub mod ui;

use rune::AppSpec;

/// The window's title and size, points.
pub const TITLE: &str = "BIMCraft (Rune)";
pub const SIZE: (f32, f32) = (1280.0, 800.0);

/// The state the window opens with: the arguments Rune did not take, read as `project::parse` reads them.
fn start() -> app::App {
    let args: Vec<std::ffi::OsString> = rune::extra_args().iter().map(std::ffi::OsString::from).collect();
    let mut app = app::App::start(&project::parse(&args));
    // The user's settings: the language, the theme and the profile (unless the arguments asked for one), and the window's size.
    if let Some(path) = settings::default_path() {
        let asked = args.iter().any(|a| a == "--rev" || a == "--bimcraft");
        app.use_settings(path, asked);
    }
    app
}

/// The window's size: the one the settings kept, else [`SIZE`].
fn size() -> (f32, f32) {
    settings::default_path().and_then(|path| settings::load(&path).window.size).map_or(SIZE, |[w, h]| (w, h))
}

/// Opens the window on the sample house, in the system's language. With `--software` it starts again on Rune's CPU renderer first.
pub fn run() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let (software, cpu_first) = (std::env::var("BIMCRAFT_SOFTWARE").ok(), std::env::var("RUNE_CPU_FIRST").ok());
    if software::wanted(&args, software.as_deref(), cpu_first.as_deref()) {
        // `relaunch` returns only to say it could not: its Ok holds no value (it is `Infallible`), so this pattern is the whole of it.
        let Err(why) = software::relaunch(&args);
        eprintln!("bimcraft-rune: could not start again on the CPU renderer ({why}); going on as it is");
    }
    lang::set(lang::from_system());
    rune::set_app_id("com.ipedrax.bimcraft");
    rune::run(AppSpec { on_key: Some(ui::on_key), poll: Some(ui::poll), ..AppSpec::new(TITLE, size(), start, ui::view) });
}
