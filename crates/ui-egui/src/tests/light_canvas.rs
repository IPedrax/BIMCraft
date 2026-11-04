//! Dark UI, Light Canvas: the drawing (plan, section, 3D view) takes its colours from the canvas, the interface from the
//! interface. Each test changes the look as a user does, in View > Dark Mode, and reads what the next frames paint.

use super::*;
use crate::settings::Settings;
use crate::style::{CANVAS_LIGHT, DARK, LIGHT};
use egui::epaint::ColorMode;
use egui::{Color32, ImageData, Shape, ThemePreference};
use egui_kittest::kittest::NodeT;

/// View > Dark Mode > the look named `label`.
fn pick_look(h: &mut Harness<BimApp>, label: &str) {
    press(h, "View");
    press(h, "Dark Mode");
    pick(h, Role::RadioButton, label);
}

/// What the last frame painted: the colours of its fills (a polygon's, a mesh's vertices') and of its closed outlines.
fn painted(h: &Harness<BimApp>) -> (Vec<Color32>, Vec<Color32>) {
    fn walk(s: &Shape, out: &mut (Vec<Color32>, Vec<Color32>)) {
        match s {
            Shape::Path(p) => {
                out.0.push(p.fill);
                if p.closed
                    && let ColorMode::Solid(c) = p.stroke.color
                {
                    out.1.push(c);
                }
            }
            Shape::Mesh(m) => out.0.extend(m.vertices.iter().map(|v| v.color)),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = (Vec::new(), Vec::new());
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The corner of each whole picture the harness was handed for a texture, in order: a view's ground, where nothing is drawn. The
/// harness takes a frame's uploads for a renderer, and keeps none of them without one.
#[derive(Clone, Default)]
struct Uploads(Arc<std::sync::Mutex<Vec<(egui::TextureId, Color32)>>>);

impl Uploads {
    fn take(&self) -> Vec<(egui::TextureId, Color32)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl egui_kittest::TestRenderer for Uploads {
    #[cfg(feature = "visual-tests")]
    fn render(&mut self, _ctx: &egui::Context, _output: &egui::FullOutput) -> Result<image::RgbaImage, String> {
        Err("This renderer captures texture uploads only".into())
    }

    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut kept = self.0.lock().unwrap();
        for (id, deltas) in &delta.set {
            for d in deltas.iter().filter(|d| d.pos.is_none()) {
                let ImageData::Color(image) = &d.image;
                kept.extend(image.pixels.first().map(|c| (*id, *c)));
            }
        }
        // Nothing here draws them, and a delta dropped unhandled panics.
        delta.clear();
    }
}

/// [`app`], with the pictures it puts in textures kept.
fn app_keeping_uploads(script: &str) -> (Harness<'static, BimApp>, Uploads) {
    let mut s = Session::new();
    s.script(script).unwrap();
    let uploads = Uploads::default();
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .renderer(uploads.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    (h, uploads)
}

/// The corner of the last picture put in the texture named `texture` since the last call, or none if there was none.
fn ground(h: &Harness<BimApp>, uploads: &Uploads, texture: &str) -> Option<Color32> {
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == texture);
    uploads.take().into_iter().rev().find(|(id, _)| named(id)).map(|(_, corner)| corner)
}

#[test]
fn a_selected_or_hovered_element_is_filled_in_the_canvas_palette_not_the_interfaces() {
    let mut h = app(BOX);
    let (south, east) = (P2(4000.0, 0.0), h.state().screen(P2(8000.0, 3000.0)));
    // On a dark canvas, the dark palette's.
    pick_look(&mut h, "Dark");
    click(&mut h, south);
    h.hover_at(east);
    h.run();
    assert!(painted(&h).0.contains(&DARK.hover_fill), "a dark canvas hovers in the dark palette's fill");
    // On a light canvas inside a dark interface, the light palette's: the dark ones are near-black on white.
    pick_look(&mut h, "Dark UI, Light Canvas");
    h.hover_at(east);
    h.run();
    let (fills, _) = painted(&h);
    assert!(fills.contains(&CANVAS_LIGHT.select_fill), "the selected wall: {fills:?}");
    assert!(fills.contains(&CANVAS_LIGHT.hover_fill), "the hovered wall: {fills:?}");
}

/// The colours of every solid stroke the last frame painted: a path's, a segment's.
fn strokes(h: &Harness<BimApp>) -> Vec<Color32> {
    fn walk(s: &Shape, out: &mut Vec<Color32>) {
        match s {
            Shape::Path(p) => out.extend(match p.stroke.color {
                ColorMode::Solid(c) => Some(c),
                _ => None,
            }),
            Shape::LineSegment { stroke, .. } => out.push(stroke.color),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn a_selected_or_hovered_element_is_stroked_in_the_canvas_palette_and_the_two_are_told_apart_on_a_light_canvas() {
    let mut h = app(BOX);
    let (south, east) = (P2(4000.0, 0.0), h.state().screen(P2(8000.0, 3000.0)));
    pick_look(&mut h, "Dark UI, Light Canvas");
    click(&mut h, south);
    h.hover_at(east);
    h.run();
    let ink = strokes(&h);
    // The dark palette's blues are for a dark ground: on white the hover one held 2.5:1.
    assert!(ink.contains(&LIGHT.accent), "the selected wall's outline: the light palette's accent");
    assert!(ink.contains(&CANVAS_LIGHT.hover), "the hovered wall's outline: the canvas's own hover");
    let (fills, _) = painted(&h);
    let apart = |a: Color32, b: Color32| (i32::from(a.r()) - i32::from(b.r())).abs() + (i32::from(a.g()) - i32::from(b.g())).abs();
    assert!(fills.contains(&CANVAS_LIGHT.select_fill) && fills.contains(&CANVAS_LIGHT.hover_fill));
    assert!(
        apart(CANVAS_LIGHT.select_fill, CANVAS_LIGHT.hover_fill) > 30,
        "a glance tells the selected wall's tint from the hovered one's"
    );
}

#[test]
fn the_crop_regions_outline_is_drawn_for_the_canvas_not_the_interface() {
    let mut h = app(BOX);
    press(&mut h, "View");
    pick(&mut h, Role::Button, "Crop View");
    pick_look(&mut h, "Dark");
    assert!(painted(&h).1.contains(&DARK.muted), "pale on a dark canvas");
    pick_look(&mut h, "Dark UI, Light Canvas");
    let outlines = painted(&h).1;
    assert!(outlines.contains(&LIGHT.muted), "dark enough to read on white: {outlines:?}");
}

/// What the last frame painted of a sketch: the colours of its strokes, and the fills of its 8 px squares that have one (a
/// picked line's grips).
fn sketch_ink(h: &Harness<BimApp>) -> (Vec<Color32>, Vec<Color32>) {
    fn walk(s: &Shape, out: &mut (Vec<Color32>, Vec<Color32>)) {
        match s {
            Shape::Path(p) => out.0.extend(match p.stroke.color {
                ColorMode::Solid(c) => Some(c),
                _ => None,
            }),
            Shape::Rect(r) if r.rect.size() == egui::vec2(8.0, 8.0) && r.fill != Color32::TRANSPARENT => out.1.push(r.fill),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = (Vec::new(), Vec::new());
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn a_sketchs_wrong_lines_and_grips_are_drawn_for_the_canvas_not_the_interface() {
    let mut h = app(&format!("{BOX}\nfloor {{}}"));
    let f = ids(&h, "floor")[0];
    h.state_mut().selection = vec![f];
    h.run();
    press(&mut h, "Modify | Floors");
    press(&mut h, "Edit Boundary");
    key(&mut h, Key::Escape);
    // Taking the north line out leaves the two lines that met it loose; Finish says so and the lines show it.
    click(&mut h, P2(4000.0, 6000.0));
    key(&mut h, Key::Delete);
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.as_ref().is_some_and(|s| s.wrong.len() == 2), "{}", h.state().status.text);
    // A picked line's ends are grips.
    click(&mut h, P2(4000.0, 0.0));
    pick_look(&mut h, "Dark UI, Light Canvas");
    let (strokes, grips) = sketch_ink(&h);
    assert!(strokes.contains(&LIGHT.error), "wrong lines read on white: {strokes:?}");
    assert!(!grips.is_empty() && grips.iter().all(|c| *c == LIGHT.surface), "grips: {grips:?}");
}

#[test]
fn the_3d_views_picture_is_redrawn_when_the_canvas_turns_light_and_dark_again() {
    let (mut h, uploads) = app_keeping_uploads(BOX);
    pick_look(&mut h, "Dark");
    press(&mut h, "Default 3D View");
    assert!(ground(&h, &uploads, "view3d").is_some_and(|c| c.r() < 40), "drawn dark at first");
    // The interface stays dark, so only the canvas changes: the picture's ground was baked into the texture.
    pick_look(&mut h, "Dark UI, Light Canvas");
    assert_eq!(ground(&h, &uploads, "view3d"), Some(Color32::WHITE));
    pick_look(&mut h, "Dark");
    assert!(ground(&h, &uploads, "view3d").is_some_and(|c| c.r() < 40), "dark again");
}

#[test]
fn a_sections_picture_is_redrawn_when_the_canvas_turns_light_and_dark_again() {
    let (mut h, uploads) = app_keeping_uploads(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    pick_look(&mut h, "Dark");
    browse(&mut h, "Section 1");
    assert!(ground(&h, &uploads, "section").is_some_and(|c| c.r() < 40), "drawn dark at first");
    pick_look(&mut h, "Dark UI, Light Canvas");
    assert_eq!(ground(&h, &uploads, "section"), Some(Color32::WHITE));
    pick_look(&mut h, "Dark");
    assert!(ground(&h, &uploads, "section").is_some_and(|c| c.r() < 40), "dark again");
}

#[test]
fn new_and_open_keep_the_look_and_the_settings_file_does_too() {
    let dir = tempfile::tempdir().unwrap();
    let (file, saved) = (dir.path().join("settings.json"), dir.path().join("saved.bimcraft"));
    let mut s = Session::new();
    s.script(BOX).unwrap();
    s.execute("save", &json!({"path": saved})).unwrap();
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    pick_look(&mut h, "Dark UI, Light Canvas");
    // What the window shows, and what the settings file keeps.
    let looks = |h: &Harness<BimApp>| {
        let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let shown = (h.ctx.options(|o| o.theme_preference), h.state().light_canvas, crate::style::dark(), crate::style::canvas_dark());
        (shown, kept["theme"].clone(), kept["light_canvas"].clone())
    };
    let want = ((ThemePreference::Dark, true, true, false), json!("dark"), json!(true));
    assert_eq!(looks(&h), want);
    // File > New starts a window over again; the look is the user's and stays, in the window and in the file.
    press(&mut h, "File");
    h.get_by_label_contains("New").click();
    h.run();
    assert!(ids(&h, "wall").is_empty(), "a new project");
    assert_eq!(looks(&h), want, "after New");
    // And File > Open.
    assert!(h.state_mut().open_path(&saved));
    h.run();
    assert!(!ids(&h, "wall").is_empty(), "the saved project");
    assert_eq!(looks(&h), want, "after Open");
}

#[test]
fn a_settings_file_with_a_hostile_or_stray_light_canvas_reads_as_sense() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    // A light canvas is only a choice of the dark theme: on a light or the system's theme no radio would be picked.
    std::fs::write(&file, r#"{"theme":"light","light_canvas":true}"#).unwrap();
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    h.run();
    assert!(!h.state().light_canvas);
    press(&mut h, "Settings");
    let lit = |h: &Harness<BimApp>, label: &str| h.get_by_role_and_label(Role::RadioButton, label).accesskit_node().toggled();
    assert_eq!(
        (lit(&h, "Light"), lit(&h, "Dark UI, Light Canvas")),
        (Some(egui::accesskit::Toggled::True), Some(egui::accesskit::Toggled::False))
    );
    let base = h.state().settings_kept.clone().unwrap();
    let dark_light = Settings { theme: ThemePreference::Dark, light_canvas: true, ..base.clone() };
    for hostile in [json!("yes"), json!(1), json!(null), json!([true]), json!({})] {
        let v = json!({"theme": "dark", "light_canvas": hostile});
        assert!(!Settings::from_json(&v, base.clone()).light_canvas, "{hostile}: the base's choice stays (none)");
        assert!(Settings::from_json(&v, dark_light.clone()).light_canvas, "{hostile}: the base's choice stays (light)");
    }
    for theme in ["light", "system"] {
        let v = json!({"theme": theme, "light_canvas": true});
        assert!(!Settings::from_json(&v, base.clone()).light_canvas, "{theme}");
    }
    assert!(Settings::from_json(&json!({"theme": "dark", "light_canvas": true}), base.clone()).light_canvas);
    assert!(!Settings::from_json(&json!({"light_canvas": true}), base).light_canvas, "no theme: the system's");
}
