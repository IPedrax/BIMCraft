//! `bimcraft`: the BIMCraft desktop app.

// A release build on Windows is a window, not a console program: no console opens behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_ui_egui::control::Control;
use bimcraft_ui_egui::revit::{Importer, is_revit};
use bimcraft_ui_egui::{BimApp, Status, lang};
use serde_json::json;

mod startup;

const USAGE: &str = "USAGE: bimcraft [FILE] [--sample | --highrise] [--control PORT]
  FILE       a .bimcraft project to open, or an .ifc or .rvt (Revit) file to import
  --sample   start on the sample house
  --highrise start on the original 20-storey performance sample
  --control  let an agent work on this window's project: `bimcraft-cli mcp --connect PORT` (0: any free port). Only this
             machine can reach it, and the port and a token are in bridge.json beside the favorites
  --version  the release

It looks for a newer version when it starts (File > Check for updates at startup switches that off).
Set BIMCRAFT_NO_UPDATE_CHECK=1 and it never makes a request.";

/// What the desktop knows the app by: Wayland's app id and X11's window class, which a Linux desktop
/// matches to the `.desktop` file of that name for the window's icon, and the macOS bundle's identifier.
const APP_ID: &str = "io.github.ipedrax.bimcraft";

/// The arguments without `--control PORT`, and the port it names: a number, 0 for one that is free.
fn control_port(mut args: Vec<String>) -> Result<(Vec<String>, Option<u16>), String> {
    let Some(at) = args.iter().position(|a| a == "--control") else { return Ok((args, None)) };
    args.remove(at);
    let port = (at < args.len()).then(|| args.remove(at)).ok_or("--control needs a port")?;
    let port = port.parse().map_err(|_| format!("`{port}` is not a port: a number up to 65535, or 0 for one that is free"))?;
    Ok((args, Some(port)))
}

fn main() -> ExitCode {
    // An update runs the installer from a copy of this program, once the app has exited: nothing else here applies to it.
    if let Some(finish) = bimcraft_update::install::parse(std::env::args_os().skip(1)) {
        return bimcraft_update::install::run(&finish);
    }
    // The window's settings sit beside the favorites, in the folder the core finds for the user's own files: the theme,
    // the language picked, the ribbon and where the palettes are.
    let settings = bimcraft_core::favorites::default_path().map(|favorites| bimcraft_ui_egui::settings::beside(&favorites));
    let language = settings.as_deref().and_then(bimcraft_ui_egui::settings::language_in).unwrap_or_else(lang::from_env);
    // The window starts at the size it was left at; if that is too big for the screen the window brings itself within it (`window.rs`).
    let window = settings.as_deref().and_then(bimcraft_ui_egui::settings::window_in).unwrap_or_default();
    let (args, control) = match control_port(std::env::args().skip(1).collect()) {
        Ok(taken) => taken,
        Err(e) => {
            eprintln!("bimcraft: {e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let mut session = Session::new();
    // A blank project starts in the interface's language: its levels named in it.
    if args.is_empty() {
        let _ = session.execute("new", &json!({"language": language.project().code()}));
    }
    let mut problem = None;
    let mut opened = Vec::new();
    // A Revit file takes minutes to read, so the window comes up first and reads it (`BimApp::open_later`): the last file wins.
    let mut later = None;
    let mut failed_project = None;
    for arg in &args {
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
            "--highrise" => session.execute("highrise", &json!({})).map(|_| ()),
            flag if flag.starts_with('-') => {
                eprintln!("bimcraft: unknown option `{flag}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            file if is_revit(Path::new(file)) => {
                later = Some(PathBuf::from(file));
                Ok(())
            }
            file => {
                later = None;
                let done = session.execute("open", &json!({ "path": file })).map(|_| ());
                if done.is_ok() {
                    opened.push(PathBuf::from(file));
                }
                done
            }
        };
        if done.is_err() && !arg.starts_with('-') && !arg.to_ascii_lowercase().ends_with(".ifc") {
            failed_project = Some(std::path::PathBuf::from(&arg));
        }
        problem = done.err().or(problem);
    }
    // From here the window is the point: what stops it is written down and said (`startup`), and a start that dies unseen
    // makes the next one try other graphics.
    let favorites = bimcraft_core::favorites::default_path();
    let files = favorites.as_deref().map(startup::Files::beside);
    let graphics = startup::begin(files.as_ref(), language);
    // What the arguments built is where the user starts: nothing to undo, nothing unsaved.
    let (model, path) = ((*session.model).clone(), session.path.clone());
    session.reset(model, path);
    let mut app = BimApp::new(session);
    app.lang = language;
    // Given nothing to open, the window opens on the start page.
    if args.is_empty() {
        app.show_start_page();
    }
    app.revit = revit_reader();
    if let Some(file) = later {
        app.open_later(file);
    }
    if let Some(path) = settings {
        app.use_settings(path);
    }
    // The recent files sit beside the favorites too; the files the arguments opened are the newest.
    if let Some(path) = &favorites {
        app.keep_recent_in(bimcraft_ui_egui::recent::beside(path));
        for file in &opened {
            app.note_recent(file);
        }
    }
    // An agent can work on the window's project through the control port, when it was started with one.
    if let Some(port) = control {
        match Control::start(port, bimcraft_core::bridge::entry_path()) {
            Ok(control) => {
                eprintln!("bimcraft: an agent can work on this project: bimcraft-cli mcp --connect {}", control.entry.port);
                app.use_control(control);
            }
            Err(e) => problem = Some(format!("couldn't listen on port {port} for an agent: {e}")),
        }
    }
    if let Some(text) = problem {
        app.status = Status { text, error: true };
    }
    // On Windows and Linux the window draws its own title bar (the app bar), unless the settings ask for the system's; macOS
    // keeps its own.
    app.own_title_bar = !cfg!(target_os = "macos");
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("BIMCraft")
        .with_decorations(!app.own_title_bar || app.system_title_bar)
        .with_app_id(APP_ID)
        .with_inner_size(window.size.unwrap_or([1440.0, 900.0]))
        .with_maximized(window.maximized)
        .with_min_inner_size(bimcraft_ui_egui::window::MIN_SIZE);
    // The logo, for the title bar and the taskbar.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/app-icon/bimcraft-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let mut options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    if let Some(backends) = graphics.backends() {
        let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = backends;
        options.wgpu_options.wgpu_setup = eframe::egui_wgpu::WgpuSetup::CreateNew(setup);
    }
    let up_files = files.clone();
    // Registered before the window opens, to catch the event that launched the app from Finder, and
    // held until it closes.
    #[cfg(target_os = "macos")]
    let (_registration, inbox) = fmv_macos_events::Registration::install();
    // The update check runs on a thread of its own, started once the window exists: it never delays the start.
    let start = move |cc: &eframe::CreationContext| -> Result<Box<dyn eframe::App>, _> {
        app.enable_autosave(std::env::temp_dir().join("bimcraft-autosave"), &cc.egui_ctx);
        if let Some(project) = failed_project {
            app.inspect_recovery(project);
        }
        #[cfg(target_os = "macos")]
        apple_events(&mut app, &cc.egui_ctx, inbox);
        let adapter = cc.wgpu_render_state.as_ref().map(|state| {
            let info = state.adapter.get_info();
            format!("{} ({:?}, driver {} {})", info.name, info.backend, info.driver, info.driver_info)
        });
        startup::up(up_files.as_ref(), graphics, adapter.as_deref().unwrap_or("no wgpu adapter"));
        // The 3D view may be drawn on the card egui draws on, if the user asks (Settings > Performance); it is made then.
        if let Some(state) = cc.wgpu_render_state.as_ref() {
            app.use_graphics_card(state);
        }
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
        Ok(()) => {
            startup::closed(files.as_ref(), graphics);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bimcraft: {e}");
            if let Some(files) = &files {
                startup::note(&files.log, &format!("start failed on {} graphics: {e}", graphics.name()));
            }
            startup::tell(language, &e.to_string(), files.as_ref());
            ExitCode::FAILURE
        }
    }
}

/// The reader of Revit files the window is given: `bimcraft-rvt`'s, which is Apache-2.0 only (docs/rvt-provenance.md) and so sits
/// behind the `rvt` feature. A build without it hands the window none, which then offers no `.rvt` and says why when given one.
#[cfg(feature = "rvt")]
fn revit_reader() -> Option<Importer> {
    Some(std::sync::Arc::new(|path: &Path| {
        let stem = path.file_stem().map_or_else(|| "Project".into(), |s| s.to_string_lossy().into_owned());
        bimcraft_rvt::import_path(path, &stem).map_err(|e| e.to_string())
    }))
}

#[cfg(not(feature = "rvt"))]
fn revit_reader() -> Option<Importer> {
    None
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

#[cfg(test)]
#[cfg(feature = "rvt")]
mod tests {
    use super::*;

    #[test]
    fn the_reader_the_window_is_given_reads_a_revit_project_and_names_it_for_its_file() {
        let path = std::env::temp_dir().join(format!("bimcraft-desktop-rvt-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let file = path.join("Barn House.rvt");
        std::fs::write(&file, bimcraft_rvt::project_fixture::Project::new(2024).file()).unwrap();
        let read = revit_reader().expect("a build with the feature has a reader");
        let imported = read(&file).unwrap();
        assert_eq!((imported.model.name.as_str(), imported.schema.as_str()), ("Barn House", "Revit 2024"));
        // A file that is not Revit's is refused, in words, not a panic.
        std::fs::write(&file, b"not a Revit file").unwrap();
        assert!(read(&file).is_err());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn what_the_reader_leaves_out_is_what_the_window_counts_as_left_out() {
        use bimcraft_rvt::project_fixture::{Lev, Project};
        let mut p = Project::new(2024);
        // One level the reader keeps, one inside a family and one with no name: two left out.
        p.levels = vec![
            Lev::new(10, "Level 1", 0.0),
            Lev { owner: Some(4321), ..Lev::new(11, "Inside", 5.0) },
            Lev { named: false, ..Lev::new(12, "x", 9.0) },
        ];
        let imported = bimcraft_rvt::import(&p.file(), "house").unwrap();
        let left: usize = imported.skipped.iter().filter_map(|n| bimcraft_ui_egui::revit::count_left_out(n)).sum();
        assert_eq!(left, 2, "{:?}", imported.skipped);
    }
}

#[cfg(test)]
mod control_tests {
    use super::control_port;

    fn control_args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_control_port_is_taken_out_of_the_arguments_wherever_it_stands() {
        assert_eq!(control_port(control_args("house.bimcraft")), Ok((control_args("house.bimcraft"), None)));
        assert_eq!(control_port(control_args("--control 7000 house.bimcraft")), Ok((control_args("house.bimcraft"), Some(7000))));
        assert_eq!(
            control_port(control_args("house.bimcraft --sample --control 0")),
            Ok((control_args("house.bimcraft --sample"), Some(0)))
        );
        assert_eq!(
            control_port(control_args("--control 65535")),
            Ok((vec![], Some(65535))),
            "nothing else: the start page and a new project"
        );
        for bad in ["--control", "--control x", "--control 70000", "--control -1", "a --control"] {
            assert!(control_port(control_args(bad)).is_err(), "{bad}");
        }
    }
}
