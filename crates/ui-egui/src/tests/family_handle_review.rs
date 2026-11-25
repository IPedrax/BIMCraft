//! A handle that shows works, for every form a family can make. Its parameter is found by moving it (a
//! name that isn't the family's, or one that moves nothing, greys it), the drag searches for the value (a nonlinear formula,
//! an `if()`, one flat where it starts, a whole number), the size before the position (`Offset + Width / 2` drags Width), a
//! label before EQ; a form turned inside out is refused whatever the kind; {3D} writes once the button is up, shows no
//! handle on a face turned away or on a hidden form; the button's last frame counts; Undo and Redo mid-drag give it up. An
//! End's formula is what drags a parameter here: a side's formula, with no label on it, moves that side alone.
use super::family_handle_params::{drag, handle, opened_on, press_move, release, screen, status, value};
use super::family_views::{editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};

/// A box from x -500 to 500, y -300 to 300, from 0 up to `top`, with `params` and `types`.
fn boxed(params: &str, types: &str, top: &str) -> String {
    format!(
        r#"family {{"name": "R", "category": "generic", "parameters": [{params}], "types": {types}, "forms": [{{"profile": [[-500, -300], [500, -300], [500, 300], [-500, 300]], "from": 0, "to": "{top}"}}]}}"#
    )
}

const W: &str = r#"{"name": "Width", "value": 1000}"#;

/// The box seen from the front, which sees its End edge-on.
fn plan(script: &str) -> Harness<'static, BimApp> {
    opened_on(script, "R", Some(Which::Elevation(Side::Front)))
}

/// The End's handle where it stands at `z`: in the front elevation, or in {3D} (`in_3d`).
fn top_at(z: f64, in_3d: bool) -> [f64; 3] {
    [0.0, if in_3d { 0.0 } else { -300.0 }, z]
}

/// The End dragged `by` mm up in the front elevation from where it stands at `z`.
fn east(h: &mut Harness<BimApp>, z: f64, by: f64) {
    drag(h, top_at(z, false), [0.0, 0.0, by], false);
}

#[test]
fn a_nonlinear_formula_is_solved_for_and_kept_to_a_hundredth() {
    // x = Width² / 2000: 500 at 1000; 800 wants Width 1264.91.
    let mut h = plan(&boxed(W, r#"{"T": {}}"#, "Width * Width / 2000"));
    east(&mut h, 500.0, 300.0);
    let w = value(&h, "T", "Width").unwrap_or_else(|| panic!("{:?}", status(&h)));
    assert!((w - 1264.91).abs() < 0.6, "{w}");
    assert!(((w * 100.0).round() - w * 100.0).abs() < 1e-6, "a hundredth: {w}");
}

#[test]
fn an_if_continuous_or_flat_where_it_starts_is_searched() {
    // Width / 2 below 1100, Width - 550 above: 700 wants 1250.
    let mut h = plan(&boxed(W, r#"{"T": {}}"#, "if(Width < 1100, Width / 2, Width - 550)"));
    east(&mut h, 500.0, 200.0);
    let w = value(&h, "T", "Width").unwrap_or_else(|| panic!("{:?}", status(&h)));
    assert!((w - 1250.0).abs() < 1.0, "{w}");
    // 600 until Width passes 1200: no slope where it starts, but further on Width moves it; 800 wants 1600.
    let mut h = plan(&boxed(W, r#"{"T": {}}"#, "if(Width > 1200, Width / 2, 600)"));
    assert!(handle(&h, top_at(600.0, false)).1, "not greyed");
    east(&mut h, 600.0, 200.0);
    let w = value(&h, "T", "Width").unwrap_or_else(|| panic!("{:?}", status(&h)));
    assert!((w - 1600.0).abs() < 1.0, "{w}");
}

#[test]
fn an_integer_parameter_takes_the_nearest_whole_number() {
    let p = r#"{"name": "Count", "kind": "integer", "value": 5}"#;
    let mut h = plan(&boxed(p, r#"{"T": {}}"#, "Count * 100"));
    east(&mut h, 500.0, 130.0);
    assert_eq!(value(&h, "T", "Count"), Some(6.0), "{:?}", status(&h));
}

#[test]
fn a_profile_turned_inside_out_is_refused_whatever_the_kind() {
    // A Number, N, the End: past the Start, refused; the draft as it was.
    let p = r#"{"name": "N", "kind": "number", "value": 500}"#;
    let mut h = plan(&boxed(p, r#"{"T": {}}"#, "N"));
    east(&mut h, 500.0, -1200.0);
    assert_eq!(value(&h, "T", "N"), None, "{:?}", status(&h));
    assert!(status(&h).1, "said why: {:?}", status(&h));
    // A length, the starter's Width, through nothing: refused.
    let mut h = plan(&boxed(W, r#"{"T": {}}"#, "Width / 2"));
    east(&mut h, 500.0, -1100.0);
    assert_eq!(value(&h, "T", "Width"), None);
    assert!(status(&h).1, "{:?}", status(&h));
}

#[test]
fn of_two_parameters_the_size_is_dragged_before_the_position() {
    // Offset + Width / 2, the Start a number: both move the End alone (sizes), Width declared first.
    let p = r#"{"name": "Width", "value": 1000}, {"name": "Offset", "value": 0}"#;
    let mut h = plan(&boxed(p, r#"{"T": {}}"#, "Offset + Width / 2"));
    east(&mut h, 500.0, 100.0);
    assert!(value(&h, "T", "Width").is_some_and(|w| (w - 1200.0).abs() < 1.0), "{:?}", status(&h));
    assert_eq!(value(&h, "T", "Offset"), None);
    // The box centred on Offset, Offset declared first: Offset moves both ends one way (a position), Width the two apart (a
    // size), which goes first.
    let p = r#"{"name": "Offset", "value": 0}, {"name": "Width", "value": 1000}"#;
    let script = boxed(p, r#"{"T": {}}"#, "Offset + Width / 2").replace(r#""from": 0"#, r#""from": "Offset - Width / 2""#);
    let mut h = plan(&script);
    east(&mut h, 500.0, 100.0);
    assert!(value(&h, "T", "Width").is_some_and(|w| (w - 1200.0).abs() < 1.0), "{:?} {:?}", value(&h, "T", "Width"), status(&h));
    assert_eq!(value(&h, "T", "Offset"), None);
}

#[test]
fn a_label_holds_the_side_before_an_eq_listed_first() {
    const L: &str = "55555555-5555-4555-8555-000000000001";
    const WEST: &str = "55555555-5555-4555-8555-000000000002";
    const MID: &str = "55555555-5555-4555-8555-000000000004";
    let pts = [(0, 0), (1000, 0), (1000, 500), (0, 500)];
    let lines: Vec<String> = (0..4)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            format!(r#"{{"guid": "55555555-5555-4555-8555-00000000001{i}", "start": [{}, {}], "end": [{}, {}]}}"#, a.0, a.1, b.0, b.1)
        })
        .collect();
    let eq = format!(
        r#"{{"refs": [{{"plane": "{WEST}"}}, {{"plane": "{MID}"}}, {{"form": "{L}", "line": "55555555-5555-4555-8555-000000000011"}}], "at": [500, -800, 0], "equal": true}}"#
    );
    let label = format!(
        r#"{{"refs": [{{"plane": "{WEST}"}}, {{"form": "{L}", "line": "55555555-5555-4555-8555-000000000011"}}], "at": [500, -400, 0], "label": "Len"}}"#
    );
    let script = format!(
        r#"family {{"name": "R", "category": "generic", "parameters": [{{"name": "Len", "value": 1000}}], "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{WEST}", "name": "West", "start": [0, -2000, 0], "end": [0, 2000, 0], "up": [0, 0, 1]}}, {{"guid": "{MID}", "name": "Mid", "start": [500, -2000, 0], "end": [500, 2000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{L}", "sketch": [{}], "from": 0, "to": 300}}], "dimensions": [{eq}, {label}]}}"#,
        lines.join(", ")
    );
    let mut h = opened_on(&script, "R", Some(Which::Plan));
    assert!(handle(&h, [1000.0, 250.0, 0.0]).1, "the east side shows, Len's");
    drag(&mut h, [1000.0, 250.0, 0.0], [200.0, 0.0, 0.0], false);
    // Len takes what puts the side under the hand: EQ keeps Mid halfway, and the solver lets West give a little, so Len (West
    // to the side, as the dimension measures it) comes to more than 1200.
    assert!(value(&h, "T", "Len").is_some_and(|x| x > 1150.0 && x < 1400.0), "{:?} {:?}", value(&h, "T", "Len"), status(&h));
    assert!(!status(&h).1, "{:?}", status(&h));
}

#[test]
fn every_handle_a_door_shows_drags_or_is_greyed_saying_why() {
    let (mut h, _) = open(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R");
    h.state_mut().new_family(bimcraft_core::family::Category::Door);
    h.run();
    view(&mut h, Some(Which::Plan));
    let forms = editor(&h).draft_family().unwrap().forms.len();
    for i in 0..forms {
        h.state_mut().active_family_mut().unwrap().select_form(i);
        h.run();
        h.run();
        let shown = editor(&h).views.shapes_shown.clone();
        assert!(!shown.is_empty(), "form {i} shows handles");
        for (at, s) in shown {
            if let Some(why) = &s.blocked {
                assert!(!why.is_empty(), "form {i} line {}: greyed, saying why", s.line);
                continue;
            }
            let before = editor(&h).draft_family().unwrap();
            let way = egui::vec2(s.way[0] as f32, -s.way[1] as f32).normalized();
            press_move(&mut h, at, at + way * 15.0);
            release(&mut h, at + way * 15.0);
            assert_ne!(editor(&h).draft_family().unwrap(), before, "form {i} line {}: a blue handle drags: {:?}", s.line, status(&h));
            h.state_mut().family_undo();
            h.run();
            h.state_mut().active_family_mut().unwrap().select_form(i);
            h.run();
            h.run();
        }
    }
}

#[test]
fn a_parameter_set_by_the_instance_previewed_is_greyed_and_refuses() {
    let p = r#"{"name": "Width", "value": 1000, "instance": true}"#;
    let mut h = plan(&boxed(p, r#"{"A": {}}"#, "Width / 2"));
    h.state_mut().active_family_mut().unwrap().set_preview_param("Width", serde_json::json!(1000));
    h.run();
    h.run();
    let (at, free) = handle(&h, top_at(500.0, false));
    assert!(!free, "greyed");
    press_move(&mut h, at, at + vec2(0.0, -40.0));
    release(&mut h, at + vec2(0.0, -40.0));
    assert!(status(&h).1 && status(&h).0.contains("instance"), "{:?}", status(&h));
    // Not set by it, the instance parameter's own value is dragged.
    let mut h = plan(&boxed(p, r#"{"A": {}}"#, "Width / 2"));
    east(&mut h, 500.0, 100.0);
    let f = editor(&h).draft_family().unwrap();
    assert!(f.parameters[0].value.as_f64().is_some_and(|w| (w - 1200.0).abs() < 1.0), "{:?}", f.parameters[0].value);
}

#[test]
fn a_type_that_built_and_wouldnt_refuses_the_drag() {
    // A second box whose width is 1000 - Width: Width at 1000 leaves it nothing, which no type can build.
    let script = boxed(W, r#"{"T": {"Width": 900}}"#, "Width / 2").replace(
        r#""to": "Width / 2"}]"#,
        r#""to": "Width / 2"}, {"profile": [[3000, 0], ["3000 + (1000 - Width)", 0], ["3000 + (1000 - Width)", 400], [3000, 400]], "from": 0, "to": 300}]"#,
    );
    let mut h = plan(&script);
    east(&mut h, 450.0, 50.0);
    assert_eq!(value(&h, "T", "Width"), Some(900.0), "{:?}", status(&h));
    assert!(status(&h).1 && status(&h).0.starts_with("T: "), "{:?}", status(&h));
}

#[test]
fn the_buttons_last_frame_counts_in_plan_and_3d() {
    for which in [Some(Which::Elevation(Side::Front)), None] {
        let mut h = opened_on(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R", which);
        let at = |z: f64| top_at(z, which.is_none());
        let (a, _) = handle(&h, at(500.0));
        let per100 = screen(&h, at(600.0)) - screen(&h, at(500.0));
        press_move(&mut h, a, a + per100);
        // The button comes up 100 mm further than the last move.
        release(&mut h, a + per100 * 2.0);
        let w = value(&h, "T", "Width");
        assert!(w.is_some_and(|w| (w - 1400.0).abs() < 15.0), "{which:?}: {w:?}");
    }
}

#[test]
fn undo_and_redo_mid_drag_give_the_parameter_drag_up() {
    for which in [Some(Which::Elevation(Side::Front)), None] {
        let mut h = opened_on(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R", which);
        let top = |z: f64| top_at(z, which.is_none());
        drag(&mut h, top(500.0), [0.0, 0.0, 100.0], false);
        assert!(value(&h, "T", "Width").is_some_and(|w| (w - 1200.0).abs() < 15.0));
        // Undo mid-drag: the drag's value not written, the last drag undone.
        let at = drag(&mut h, top(600.0), [0.0, 0.0, 100.0], true);
        h.state_mut().family_undo();
        h.run();
        release(&mut h, at);
        assert_eq!(value(&h, "T", "Width"), None, "{which:?}");
        // Redo mid-drag: the undone drag back, this one given up.
        h.state_mut().active_family_mut().unwrap().select_form(0);
        h.run();
        h.run();
        let at = drag(&mut h, top(500.0), [0.0, 0.0, 300.0], true);
        h.state_mut().family_redo();
        h.run();
        release(&mut h, at);
        assert!(value(&h, "T", "Width").is_some_and(|w| (w - 1200.0).abs() < 15.0), "{which:?}: {:?}", value(&h, "T", "Width"));
    }
}

#[test]
fn in_3d_a_drag_is_written_once_the_button_is_up_the_value_said_meanwhile() {
    let mut h = opened_on(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R", None);
    let steps = editor(&h).steps();
    let at = drag(&mut h, top_at(500.0, true), [0.0, 0.0, 100.0], true);
    assert_eq!(value(&h, "T", "Width"), None, "not written mid-drag");
    assert!(status(&h).0.contains("Width") && status(&h).0.contains("1200"), "{:?}", status(&h));
    release(&mut h, at);
    assert!(value(&h, "T", "Width").is_some_and(|w| (w - 1200.0).abs() < 15.0));
    assert_eq!(editor(&h).steps(), steps + 1);
}

#[test]
fn in_3d_no_handle_on_a_face_turned_away_or_a_hidden_form() {
    let mut h = opened_on(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R", None);
    // From above, in front and to the right: the top, the south and the east sides; not the Start under it, the north or the west.
    let ways: Vec<[f64; 3]> = editor(&h).views.handles3d_shown.iter().map(|(_, x)| x.way).collect();
    assert_eq!(ways.len(), 3, "{ways:?}");
    assert!(ways.iter().all(|w| w[2] >= 0.0 && w[1] <= 0.0 + 1e-9 && w[0] >= -1e-9), "{ways:?}");
    // Seen from below, the Start shows and the End doesn't.
    if let Some(c) = h.state_mut().active_family_mut().unwrap().orbit.camera.as_mut() {
        c.elevation = -40.0;
    }
    h.run();
    h.run();
    let ways: Vec<[f64; 3]> = editor(&h).views.handles3d_shown.iter().map(|(_, x)| x.way).collect();
    assert!(ways.iter().any(|w| w[2] < -0.5) && !ways.iter().any(|w| w[2] > 0.5), "{ways:?}");
    // Hidden by Temporary Hide (HH, the document's own key): no handles, though selected again.
    let mut h = opened_on(&boxed(W, r#"{"T": {}}"#, "Width / 2"), "R", None);
    assert!(!editor(&h).views.handles3d_shown.is_empty());
    shortcut(&mut h, "HH");
    h.run();
    h.state_mut().active_family_mut().unwrap().select_form(0);
    h.run();
    h.run();
    assert_eq!(editor(&h).selected_form(), Some(0));
    assert!(editor(&h).views.handles3d_shown.is_empty(), "HH");
}
