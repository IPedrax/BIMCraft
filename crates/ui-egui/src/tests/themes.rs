//! Themes: the system's by default, presets (Dracula, Midnight, AMOLED), and an accent, a panel, a canvas and a text colour of the
//! user's own over any of them, the same in both profiles.

use super::*;
use crate::settings::Settings;
use crate::style::{MIDNIGHT, Scheme, contrast, pal};
use bimcraft_core::render::PAPER;
use egui::epaint::ColorMode;
use egui::{Color32, Shape, ThemePreference};

#[test]
fn a_theme_preset_and_an_accent_hold_in_both_profiles_and_the_settings_file_keeps_them() {
    for rev in [false, true] {
        let file = std::env::temp_dir().join(format!("bimcraft-ui-themes-{rev}-{}", std::process::id())).join("settings.json");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        h.state_mut().use_settings(file.clone());
        h.run();
        assert_eq!(h.state().scheme, Scheme::Own, "the profile's own colours, light or dark as the system is");
        press(&mut h, "Settings");
        // Below the page's fold: pressed as a screen reader does.
        h.get_by_role_and_label(Role::RadioButton, "Midnight").click_accesskit();
        h.run();
        press(&mut h, "Close");
        assert!(crate::style::dark() && pal().panel == MIDNIGHT.panel, "rev {rev}");
        let kept = |f: &std::path::Path| -> Value { serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap() };
        assert_eq!(kept(&file)["scheme"], "midnight");
        // An accent of the user's, its ink stepped until it reads on every ground.
        h.state_mut().accent = Some([255, 200, 0]);
        h.run();
        let p = pal();
        assert_eq!(p.accent, egui::Color32::from_rgb(255, 200, 0));
        for ground in [p.panel, p.bar, p.surface, p.select_fill, p.hover_fill] {
            assert!(contrast(p.accent_ink, ground) >= 4.5, "rev {rev}: {:?} on {ground:?}", p.accent_ink);
        }
        assert_eq!(kept(&file)["accent"], json!([255, 200, 0]));
        press(&mut h, "Settings");
        h.get_by_role_and_label(Role::RadioButton, "As the System").click_accesskit();
        h.run();
        press(&mut h, "Close");
        assert_eq!(h.state().scheme, Scheme::Own);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The window in BIMCraft's profile, or in Rev's.
fn window(rev: bool, script: &str) -> Harness<'static, BimApp> {
    if rev { app(script) } else { bimcraft(script) }
}

/// The panel, canvas and text colours the window keeps of the user's own.
fn own(h: &Harness<BimApp>) -> [Option<[u8; 3]>; 3] {
    let s = h.state();
    [s.panel_colour, s.canvas_colour, s.text_colour]
}

#[test]
fn your_own_panel_canvas_and_text_colours_hold_over_any_scheme_in_both_profiles_and_the_settings_file_keeps_them() {
    let (panel, canvas, text) = ([250, 244, 230], [16, 24, 48], [120, 20, 20]);
    for rev in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut h = window(rev, BOX);
        h.state_mut().use_settings(file.clone());
        h.run();
        // A cream panel, a navy canvas and a dark red text: the red reads on the cream, so it is drawn as it was picked.
        (h.state_mut().panel_colour, h.state_mut().canvas_colour, h.state_mut().text_colour) = (Some(panel), Some(canvas), Some(text));
        h.run();
        let shown = |what: &str| {
            let p = pal();
            assert_eq!((p.panel, p.text), (rgb(panel), rgb(text)), "rev {rev}, {what}: the panels and their lettering");
            assert_eq!(crate::canvas::color(PAPER), rgb(canvas), "rev {rev}, {what}: the canvas");
        };
        shown("the profile's own");
        let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            [&kept["panel_colour"], &kept["canvas_colour"], &kept["text_colour"]],
            [&json!(panel), &json!(canvas), &json!(text)],
            "rev {rev}"
        );
        // Over a preset: its accent, the user's grounds and lettering.
        press(&mut h, "Settings");
        h.get_by_role_and_label(Role::RadioButton, "Midnight").click_accesskit();
        h.run();
        press(&mut h, "Close");
        shown("over Midnight");
        assert_eq!(pal().accent, MIDNIGHT.accent, "rev {rev}");
        // A window opened on the same file has them, and each one's Reset brings back the scheme's.
        let mut again = window(rev, BOX);
        again.state_mut().use_settings(file.clone());
        again.run();
        assert_eq!(own(&again), [Some(panel), Some(canvas), Some(text)], "rev {rev}");
        shown("read back");
        press(&mut again, "Settings");
        for reset in ["Reset Panels", "Reset Canvas", "Reset Text"] {
            again.get_by_label(reset).click_accesskit();
            again.run();
        }
        press(&mut again, "Close");
        assert_eq!(own(&again), [None; 3], "rev {rev}");
        assert_eq!(pal(), MIDNIGHT, "rev {rev}: Midnight's own colours again");
        assert_eq!(crate::canvas::color(PAPER), Color32::from_gray(26), "rev {rev}: a dark scheme's canvas");
        // A new project keeps how the window looks.
        again.state_mut().text_colour = Some(text);
        again.state_mut().reopened();
        again.run();
        assert_eq!(own(&again), [None, None, Some(text)], "rev {rev}");
    }
}

#[test]
fn a_bad_colour_in_the_settings_file_is_ignored() {
    for rev in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"panel_colour": [999, 0, 0], "canvas_colour": "navy", "text_colour": [0, 0]}"#).unwrap();
        let mut h = window(rev, BOX);
        let (before, ground) = (pal(), crate::canvas::color(PAPER));
        h.state_mut().use_settings(file);
        h.run();
        assert_eq!(own(&h), [None; 3], "rev {rev}");
        assert_eq!((pal(), crate::canvas::color(PAPER)), (before, ground), "rev {rev}: the scheme's colours");
        let base = h.state().settings_kept.clone().unwrap();
        for bad in
            [json!("red"), json!([300, 0, 0]), json!([-1, 0, 0]), json!([1, 2]), json!([1.5, 2, 3]), json!([1, 2, 3, 4]), json!({"r": 1})]
        {
            let read = Settings::from_json(&json!({"panel_colour": bad, "canvas_colour": bad, "text_colour": bad}), base.clone());
            assert_eq!([read.panel_colour, read.canvas_colour, read.text_colour], [None; 3], "{bad}");
        }
    }
}

#[test]
fn a_text_colour_too_close_to_its_ground_is_stepped_to_4_5_to_1_and_the_dialog_says_so() {
    const STEPPED: &str = "Your text colour was too close to the panels: it is made darker or lighter to read at 4.5:1";
    for rev in [false, true] {
        let mut h = window(rev, BOX);
        // Grey on grey.
        (h.state_mut().panel_colour, h.state_mut().text_colour) = (Some([90, 90, 90]), Some([110, 110, 110]));
        h.run();
        let p = pal();
        assert_eq!(p.panel, rgb([90, 90, 90]));
        assert_ne!(p.text, rgb([110, 110, 110]), "rev {rev}");
        for ground in [p.panel, p.bar, p.control, p.surface, p.hover_fill, p.select_fill] {
            let c = contrast(p.text, ground);
            assert!(c >= 4.5, "rev {rev}: {:?} on {ground:?} is {c:.2}:1", p.text);
        }
        press(&mut h, "Settings");
        assert!(h.query_by_label(STEPPED).is_some(), "rev {rev}: the dialog says why the text isn't the colour picked");
        // White reads on that grey: drawn as picked, and nothing to say.
        h.state_mut().text_colour = Some([255, 255, 255]);
        h.run();
        assert_eq!(pal().text, Color32::WHITE, "rev {rev}");
        assert!(h.query_by_label(STEPPED).is_none(), "rev {rev}");
    }
}

/// The colours of the solid strokes (a path's, a segment's) the last frame painted over the plan on `ground` and that show: what is
/// clipped to the rectangle the ground fills. None if no ground of that colour was painted.
fn plan_ink(h: &Harness<BimApp>, ground: Color32) -> Vec<Color32> {
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
    let shapes = &h.output().shapes;
    // The last ground: the window's backdrop is painted under the view first, clipped to more than the plan.
    let plan = shapes.iter().rev().find(|c| matches!(&c.shape, Shape::Rect(r) if r.fill == ground)).map(|c| c.clip_rect);
    let mut out = Vec::new();
    shapes.iter().filter(|c| Some(c.clip_rect) == plan).for_each(|c| walk(&c.shape, &mut out));
    // A fill drawn without an outline has a stroke nobody sees.
    out.retain(|c| c.a() > 0);
    out
}

#[test]
fn a_dark_canvas_colour_draws_the_plan_in_light_ink_and_a_light_one_in_dark_whatever_the_interface() {
    for rev in [false, true] {
        let mut h = window(rev, BOX);
        // A navy canvas in a light interface, a cream one in a dark interface: never black lines on black, nor white on white.
        for (theme, canvas, dark) in [(ThemePreference::Light, [16, 24, 48], true), (ThemePreference::Dark, [250, 244, 230], false)] {
            h.ctx.set_theme(theme);
            h.state_mut().canvas_colour = Some(canvas);
            h.run();
            let ground = rgb(canvas);
            assert_eq!(crate::style::dark(), theme == ThemePreference::Dark, "rev {rev}: the interface keeps its theme");
            assert_eq!(crate::style::canvas_dark(), dark, "rev {rev}: the drawing takes the ink {canvas:?} wants");
            // A card of the interface over the view (its frame, its icons) reads on its own ground, not the canvas's.
            let p = pal();
            let mut ink = plan_ink(&h, ground);
            ink.retain(|c| ![p.icon, p.border, p.text, p.muted].contains(c));
            assert!(!ink.is_empty(), "rev {rev}: the plan is drawn on {canvas:?}");
            for c in ink {
                assert!(contrast(c, ground) >= 3.0, "rev {rev}: a line in {c:?} on {canvas:?} is {:.2}:1", contrast(c, ground));
            }
        }
    }
}

#[test]
fn a_new_canvas_colour_draws_a_sections_picture_again_on_it() {
    for rev in [false, true] {
        let mut h = window(rev, &format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
        browse(&mut h, "Section 1");
        h.run();
        // The picture is the same; it goes into the texture again with its paper in the new colour.
        let drawn = |h: &Harness<BimApp>| h.state().cut.uploads;
        let still = drawn(&h);
        // Two dark ones: the drawing stays dark, its ground is new.
        for canvas in [[16, 24, 48], [48, 16, 16]] {
            let before = drawn(&h);
            h.state_mut().canvas_colour = Some(canvas);
            h.run();
            assert!(drawn(&h) > before, "rev {rev}: the picture of {canvas:?} is drawn again, its paper in the canvas's colour");
        }
        assert!(still >= 1, "rev {rev}");
    }
}
