//! `bimcraft`: the BIMCraft desktop app.

// A release build on Windows is a window, not a console program: no console opens behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_ui_egui::{BimApp, Status, lang};
use serde_json::json;

const USAGE: &str = "USAGE: bimcraft [FILE] [--sample]
  FILE       a .bimcraft project to open, or an .ifc file to import
  --sample   start on the sample house
  --version  the release

It looks for a newer version when it starts (File > Check for updates at startup switches that off).
Set BIMCRAFT_NO_UPDATE_CHECK=1 and it never makes a request.";

/// What the desktop knows the app by: Wayland's app id and X11's window class, which a Linux desktop
/// matches to the `.desktop` file of that name for the window's icon, and the macOS bundle's identifier.
const APP_ID: &str = "io.github.ipedrax.bimcraft";

fn main() -> ExitCode {
    // An update runs the installer from a copy of this program, once the app has exited: nothing else here applies to it.
    if let Some(finish) = bimcraft_update::install::parse(std::env::args_os().skip(1)) {
        return bimcraft_update::install::run(&finish);
    }
    let language = lang::from_env();
    let mut session = Session::new();
    // A blank project starts in the interface's language: its levels named in it.
    if std::env::args().len() == 1 {
        let _ = session.execute("new", &json!({"language": language.code()}));
    }
    let mut problem = None;
    let mut opened = Vec::new();
    for arg in std::env::args().skip(1) {
        let done = match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("{}", bimcraft_update::version_line(env!("CARGO_PKG_VERSION")));
                return ExitCode::SUCCESS;
            }
            "--sample" => session.script(SAMPLE).map(|_| ()),
            flag if flag.starts_with('-') => {
                eprintln!("bimcraft: unknown option `{flag}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            file => {
                let done = session.execute("open", &json!({ "path": file })).map(|_| ());
                if done.is_ok() {
                    opened.push(PathBuf::from(file));
                }
                done
            }
        };
        problem = done.err().or(problem);
    }
    // What the arguments built is where the user starts: nothing to undo, nothing unsaved.
    let (model, path) = ((*session.model).clone(), session.path.clone());
    session.reset(model, path);
    let mut app = BimApp::new(session);
    app.lang = language;
    // The recent files sit beside the favorites too; the files the arguments opened are the newest.
    if let Some(path) = bimcraft_core::favorites::default_path() {
        app.keep_recent_in(bimcraft_ui_egui::recent::beside(&path));
        for file in &opened {
            app.note_recent(file);
        }
    }
    if let Some(text) = problem {
        app.status = Status { text, error: true };
    }
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("BIMCraft")
        .with_app_id(APP_ID)
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    // The logo, for the title bar and the taskbar.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/app-icon/bimcraft-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    // Registered before the window opens, to catch the event that launched the app from Finder, and
    // held until it closes.
    #[cfg(target_os = "macos")]
    let (_registration, inbox) = fmv_macos_events::Registration::install();
    // The update check runs on a thread of its own, started once the window exists: it never delays the start.
    let start = move |cc: &eframe::CreationContext| -> Result<Box<dyn eframe::App>, _> {
        #[cfg(target_os = "macos")]
        apple_events(&mut app, &cc.egui_ctx, inbox);
        let ctx = cc.egui_ctx.clone();
        // The settings sit beside the favorites, in the folder the core finds for the user's own files.
        let prefs = bimcraft_core::favorites::default_path().map(|path| bimcraft_update::prefs::beside(&path));
        app.updates = bimcraft_update::Updater::desktop(prefs, move || ctx.request_repaint());
        if let Some(updates) = app.updates.as_mut() {
            updates.check_at_startup();
        }
        Ok(Box::new(app))
    };
    match eframe::run_native("BIMCraft", options, Box::new(start)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bimcraft: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Finder, the Dock and Open With hand macOS apps their files, and the Dock its Quit, as Apple events,
/// not as arguments: the app reads them each frame, and one arriving wakes it.
#[cfg(target_os = "macos")]
fn apple_events(app: &mut BimApp, ctx: &eframe::egui::Context, inbox: fmv_macos_events::Inbox) {
    use bimcraft_ui_egui::OsEvent;
    use fmv_macos_events::Event;
    let ctx = ctx.clone();
    inbox.set_wake(move || ctx.request_repaint());
    app.os_events = Some(Box::new(move || {
        let mut events = Vec::new();
        for event in inbox.drain() {
            match event {
                Event::Open(paths) => events.extend(paths.into_iter().map(OsEvent::Open)),
                Event::Quit => events.push(OsEvent::Quit),
            }
        }
        events
    }));
}
