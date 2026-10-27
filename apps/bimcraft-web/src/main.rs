//! `bimcraft-web`: the BIMCraft window in a browser page, drawn on its canvas. The page has no file system: files the user
//! opens are read into it and what they save comes back as a download (`bimcraft_core::disk`).

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_ui_egui::{BimApp, Status, lang};
use serde_json::json;

/// The window the page opens: the sample house when the address asks for it (`#sample`), else the start page over a new
/// project. Built natively too, for its test.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn app(sample: bool) -> BimApp {
    let language = lang::from_env();
    let mut session = Session::new();
    let started = if sample {
        session.script(SAMPLE).map(|_| ())
    } else {
        session.execute("new", &json!({"language": language.project().code()})).map(|_| ())
    };
    // What the page started on is where the user starts: nothing to undo, nothing unsaved.
    let (model, path) = ((*session.model).clone(), session.path.clone());
    session.reset(model, path);
    let mut app = BimApp::new(session);
    app.lang = language;
    // The browser frames the page: the window draws no title bar of its own.
    app.own_title_bar = false;
    if !sample {
        app.show_start_page();
    }
    if let Err(text) = started {
        app.status = Status { text, error: true };
    }
    app
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let sample = document.location().and_then(|l| l.hash().ok()).is_some_and(|h| h == "#sample");
    let Some(canvas) = document.get_element_by_id("bimcraft").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let start = Box::new(move |cc: &eframe::CreationContext| -> Result<Box<dyn eframe::App>, _> {
            let mut app = app(sample);
            // The 3D view may be drawn on the card egui draws on (WebGPU, or WebGL where the browser has no WebGPU).
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                app.use_graphics_card(state);
            }
            Ok(Box::new(app))
        });
        let started = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), start).await;
        // The note under the canvas goes once the window is up, or says what stopped it.
        let Some(note) = document.get_element_by_id("loading") else { return };
        match started {
            Ok(()) => note.remove(),
            Err(e) => note.set_text_content(Some(&format!("BIMCraft couldn't start in this browser: {e:?}"))),
        }
    });
}

/// Off the browser there is nothing to run: the desktop app is `bimcraft`.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("bimcraft-web runs in a browser: `trunk serve` in apps/bimcraft-web. On this computer, run `bimcraft`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_starts_on_a_new_project_or_the_sample_with_nothing_to_undo() {
        let blank = app(false);
        assert!(!blank.status.error, "{}", blank.status.text);
        let sample = app(true);
        assert!(!sample.status.error, "{}", sample.status.text);
        let walls = |a: &BimApp| a.session.model.elements.values().filter(|e| e.category() == "wall").count();
        assert_eq!(walls(&blank), 0);
        assert!(walls(&sample) > 0);
        assert!(sample.session.steps(false).is_empty() && !sample.dirty() && !sample.own_title_bar);
    }

    #[test]
    fn the_page_never_phones_home() {
        let manifest = include_str!("../Cargo.toml");
        assert!(!manifest.contains("bimcraft-update") && !manifest.contains("bimcraft-rvt"), "{manifest}");
    }
}
