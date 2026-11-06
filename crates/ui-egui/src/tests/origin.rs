//! The origin marker and the view's name in a plan's corner: working aids drawn on the screen, never in a drawing that is printed
//! or exported, each hidden by a box in File > Settings that `settings.json` keeps.

use super::*;
use crate::canvas::ORIGIN_ARM;
use crate::settings::{Page, Settings};
use crate::style::canvas_pal;
use egui::{Color32, Shape};
use egui_kittest::kittest::NodeT;

/// Where the arms of a marker drawn from `o` end: its x arm in the canvas's red, its y arm in its green. Nothing else on the
/// screen is a line in those colours that starts at the origin.
fn arms(h: &Harness<BimApp>, o: Pos2) -> (Option<Pos2>, Option<Pos2>) {
    fn find(s: &Shape, o: Pos2, colour: Color32, out: &mut Option<Pos2>) {
        match s {
            Shape::LineSegment { points, stroke } if stroke.color == colour && points[0].distance(o) < 0.2 => *out = Some(points[1]),
            Shape::Vec(v) => v.iter().for_each(|s| find(s, o, colour, out)),
            _ => {}
        }
    }
    let (mut x, mut y) = (None, None);
    for c in &h.output().shapes {
        find(&c.shape, o, canvas_pal().error, &mut x);
        find(&c.shape, o, canvas_pal().context, &mut y);
    }
    (x, y)
}

/// The lettering of the marker's arms, X and Y, in the arms' colours.
fn letters(h: &Harness<BimApp>) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<String>) {
        match s {
            Shape::Text(t) if [canvas_pal().error, canvas_pal().context].contains(&t.fallback_color) => {
                out.push(t.galley.text().to_string())
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.sort();
    out
}

/// Whether `name` is lettered in the plan canvas's bottom left corner, as the view's name is (the view control bar, whose scale and
/// detail level are in that corner too, is not it).
fn in_corner(h: &Harness<BimApp>, name: &str) -> bool {
    fn walk(s: &Shape, near: &dyn Fn(Pos2) -> bool, out: &mut Vec<String>) {
        match s {
            Shape::Text(t) if near(t.pos) => out.push(t.galley.text().to_string()),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, near, out)),
            _ => {}
        }
    }
    let view = h.state().view_rect;
    let near = |at: Pos2| at.x >= view.left() && at.x < view.left() + 80.0 && at.y > view.bottom() - 120.0 && at.y < view.bottom();
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &near, &mut out));
    out.iter().any(|t| t == name)
}

/// A way to build the app on a script: Rev's, or BIMCraft's own.
type Build = fn(&str) -> Harness<'static, BimApp>;

fn both_profiles() -> [(&'static str, Build); 2] {
    [("Rev", app), ("BIMCraft", bimcraft)]
}

#[test]
fn a_plan_shows_a_marker_at_the_origin_and_its_name_in_the_corner_until_the_settings_say_not() {
    for (profile, build) in both_profiles() {
        let mut h = build(BOX);
        assert!(h.state().show_origin && h.state().show_view_name, "{profile}: both on by default");
        let o = h.state().screen(P2(0.0, 0.0));
        let (x, y) = arms(&h, o);
        assert_eq!(x, Some(o + vec2(ORIGIN_ARM, 0.0)), "{profile}: the x arm runs east along the screen");
        assert_eq!(y, Some(o - vec2(0.0, ORIGIN_ARM)), "{profile}: the y arm runs north, up the screen");
        assert_eq!(letters(&h), ["X", "Y"], "{profile}: and each is lettered");
        assert!(in_corner(&h, "Level 1"), "{profile}");

        h.state_mut().show_origin = false;
        h.run();
        let o = h.state().screen(P2(0.0, 0.0));
        assert_eq!(arms(&h, o), (None, None), "{profile}: off, no marker");
        assert!(letters(&h).is_empty(), "{profile}");
        assert!(in_corner(&h, "Level 1"), "{profile}: the name is the other box's");

        h.state_mut().show_view_name = false;
        h.run();
        assert!(!in_corner(&h, "Level 1"), "{profile}: off, no name in the corner");

        h.state_mut().show_origin = true;
        h.run();
        let o = h.state().screen(P2(0.0, 0.0));
        assert!(arms(&h, o).0.is_some() && !in_corner(&h, "Level 1"), "{profile}: each box is its own");
    }
}

#[test]
fn a_ceiling_plan_has_them_too_and_the_other_views_do_not() {
    for (profile, build) in both_profiles() {
        let mut h = build(BOX);
        h.state_mut().open_sheet(Sheet::Ceiling(100000));
        h.run();
        assert_eq!(h.state().sheet, Sheet::Ceiling(100000), "{profile}");
        let o = h.state().screen(P2(0.0, 0.0));
        assert_eq!(arms(&h, o).0, Some(o + vec2(ORIGIN_ARM, 0.0)), "{profile}: the ceiling plan's marker");
        assert!(in_corner(&h, "Level 1 (Ceiling Plan)"), "{profile}: and its name says what it is");

        // A marker is a plan's aid: the 3D view draws neither.
        shortcut(&mut h, "3D");
        assert_eq!(h.state().sheet, Sheet::View3D, "{profile}");
        let o = h.state().screen(P2(0.0, 0.0));
        assert_eq!(arms(&h, o), (None, None), "{profile}: none in the 3D view");
        assert!(letters(&h).is_empty() && !in_corner(&h, "Level 1"), "{profile}");
    }
}

#[test]
fn a_marker_whose_origin_is_off_the_screen_is_not_drawn_and_the_name_still_is() {
    let mut h = app("wall {\"points\": [[50000,50000],[58000,50000]]}");
    h.run();
    let o = h.state().screen(P2(0.0, 0.0));
    let view = h.state().view_rect;
    assert!(!view.contains(o), "the plan is fitted round the wall, far from the origin");
    assert_eq!(arms(&h, o), (None, None), "so the marker, which is at the origin, is not on the screen");
    assert!(in_corner(&h, "Level 1"), "the name is the corner's, wherever the plan is");
}

#[test]
fn the_boxes_in_settings_turn_them_off_and_the_file_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    for (profile, build) in both_profiles() {
        let _ = std::fs::remove_file(&file);
        let mut h = build(BOX);
        h.state_mut().use_settings(file.clone());
        h.state_mut().settings_page = Some(Page::Interface);
        h.run();
        for label in ["Show the origin marker in plans", "Show the view's name in the corner of plans"] {
            let box_ = h.get_by_role_and_label(Role::CheckBox, label);
            assert_eq!(box_.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "{profile}: {label} is on to begin with");
            box_.scroll_to_me();
            h.run();
            pick(&mut h, Role::CheckBox, label);
        }
        assert!(!h.state().show_origin && !h.state().show_view_name, "{profile}");
        press(&mut h, "Close");
        let o = h.state().screen(P2(0.0, 0.0));
        assert_eq!(arms(&h, o), (None, None), "{profile}: the box took the marker off the plan");
        assert!(!in_corner(&h, "Level 1"), "{profile}: and the name");
        let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!((kept["origin_marker"].as_bool(), kept["view_name"].as_bool()), (Some(false), Some(false)), "{profile}");

        // The next window starts without them, and a box ticked again is kept too.
        let mut again = build(BOX);
        again.state_mut().use_settings(file.clone());
        again.run();
        assert!(!again.state().show_origin && !again.state().show_view_name, "{profile}");
        let o = again.state().screen(P2(0.0, 0.0));
        assert_eq!(arms(&again, o), (None, None), "{profile}");
        assert!(!in_corner(&again, "Level 1"), "{profile}");
        again.state_mut().show_origin = true;
        again.run();
        let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!((kept["origin_marker"].as_bool(), kept["view_name"].as_bool()), (Some(true), Some(false)), "{profile}");
    }
}

#[test]
fn file_new_and_open_keep_the_boxes() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app(BOX);
    h.state_mut().use_settings(dir.path().join("settings.json"));
    (h.state_mut().show_origin, h.state_mut().show_view_name) = (false, false);
    h.run();
    h.state_mut().reopened();
    h.run();
    assert!(!h.state().show_origin && !h.state().show_view_name, "a new project is drawn as the last was");
}

#[test]
fn a_settings_file_that_does_not_say_or_says_nonsense_leaves_both_on() {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    let base = h.state().settings_kept.clone().unwrap();
    assert!(base.origin_marker && base.view_name, "on to begin with");
    for text in [json!({}), json!({"origin_marker": "no", "view_name": 0}), json!({"origin_marker": null, "view_name": [false]})] {
        let read = Settings::from_json(&text, base.clone());
        assert!(read.origin_marker && read.view_name, "{text}");
    }
    let read = Settings::from_json(&json!({"origin_marker": false}), base.clone());
    assert!(!read.origin_marker && read.view_name, "each is read on its own");
}
