//! F7d: pivots in a family's views. A reference line selected in plan, Pivot and a click on where two planes cross; its
//! Swing Angle tied to a parameter with "="; the rotate grip dragged (writing the parameter, stopped at the limits); a pivoted
//! plane's grip; and an unpivoted line's end dragged by its grip (T18).
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{PivotAt, PivotPoint, Val};
use bimcraft_core::ids::Guid;

const HINGE: &str = "88888888-8888-4888-8888-000000000001";
const LEAF: &str = "88888888-8888-4888-8888-000000000002";
const PANEL: &str = "88888888-8888-4888-8888-000000000003";
const CFB: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2";

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// A door's swing waiting for its pivot: Hinge, a plane square to x at -450; a reference line from (-450, 0) along x, 900
/// long; Swing Angle 0..90 (types Shut 0 and Half 45); `planes`, more planes as JSON.
fn family(pivot: &str, planes: &str) -> String {
    format!(
        r#"family {{"name": "Swing", "category": "generic", "parameters": [{{"name": "Swing Angle", "kind": "angle", "value": 0}}], "types": {{"Shut": {{"Swing Angle": 0}}, "Half": {{"Swing Angle": 45}}}}, "ref_planes": [{{"guid": "{HINGE}", "name": "Hinge", "start": [-450, -1500, 0], "end": [-450, 1500, 0], "up": [0, 0, 1]}}{planes}], "ref_lines": [{{"guid": "{LEAF}", "start": [-450, 0, 0], "end": [450, 0, 0]{pivot}}}], "forms": [{{"host": {{"line": "{LEAF}", "face": "vertical"}}, "profile": [[0, 0], [900, 0], [900, 2000], [0, 2000]], "to": 40}}]}}"#
    )
}

fn in_plan(pivot: &str, planes: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(pivot, planes), "Swing");
    view(&mut h, Some(Which::Plan));
    h
}

/// What the type previewed holds of `name`.
fn value_of(h: &Harness<BimApp>, name: &str) -> f64 {
    match editor(h).values().get(name) {
        Some(Val::Number(v)) => *v,
        other => panic!("{name}: {other:?}"),
    }
}

/// The selected element's grip on screen.
fn grip(h: &Harness<BimApp>) -> egui::Pos2 {
    editor(h).views.grips_shown.first().map(|(_, p)| *p).expect("a grip shown")
}

/// A point on screen `r` mm from (-450, 0) at `deg` degrees in plan.
fn round_the_hinge(h: &Harness<BimApp>, r: f64, deg: f64) -> egui::Pos2 {
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
    editor(h).views.screen_of([-450.0 + r * c, r * s, 0.0]).unwrap()
}

#[test]
fn pivot_pins_a_selected_line_where_two_planes_cross_and_its_grip_swings_it_within_its_limits() {
    let mut h = in_plan("", "");
    // A click on the line selects it.
    click_family(&mut h, [100.0, 0.0, 0.0]);
    assert_eq!(editor(&h).selected_line(), Some(g(LEAF)));
    assert!(h.query_all_by_label("Modify | Reference Lines").count() > 0);
    // Pivot, then a click near where Hinge crosses Center (Front/Back): pinned there, at its start.
    press(&mut h, "Pivot");
    assert!(editor(&h).views.pivoting.is_some());
    click_family(&mut h, [-447.0, 2.0, 0.0]);
    let p = editor(&h).draft_pivot(g(LEAF)).cloned().expect("a pivot");
    assert!(matches!(p.to, PivotPoint::Crossing(a, b) if [a, b] == [g(HINGE), g(CFB)] || [b, a] == [g(HINGE), g(CFB)]), "{:?}", p.to);
    assert_eq!(p.at, PivotAt::Start);
    // Its Swing Angle tied to the parameter with "=".
    press(&mut h, "Associate Family Parameter: Swing Angle");
    press_near(&mut h, "Swing Angle", "<None>");
    press(&mut h, "OK");
    assert_eq!(editor(&h).draft_pivot(g(LEAF)).unwrap().parameter.as_deref(), Some("Swing Angle"));
    // Limits typed in Properties: 0 to 90.
    for (label, v) in [("Minimum Angle", "0"), ("Maximum Angle", "90")] {
        h.get_by_label(label).click();
        h.run();
        h.get_by_label(label).type_text(v);
        key(&mut h, Key::Enter);
    }
    let p = editor(&h).draft_pivot(g(LEAF)).cloned().unwrap();
    assert_eq!((p.min, p.max), (Some(0.0), Some(90.0)));
    // The rotate grip, at the far end, dragged to 30°: the parameter takes it.
    let from = grip(&h);
    let to = round_the_hinge(&h, 900.0, 30.2);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert_eq!(value_of(&h, "Swing Angle"), 30.0, "{}", h.state().status.text);
    // Past its maximum it stops there, and below its minimum at that.
    let from = grip(&h);
    let to = round_the_hinge(&h, 900.0, 140.0);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert_eq!(value_of(&h, "Swing Angle"), 90.0);
    let from = grip(&h);
    let to = round_the_hinge(&h, 900.0, -30.0);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert_eq!(value_of(&h, "Swing Angle"), 0.0);
    // A minimum typed past the maximum is refused, saying why, and the limits stay.
    h.get_by_label("Minimum Angle").click();
    h.run();
    h.get_by_label("Minimum Angle").type_text("120");
    key(&mut h, Key::Enter);
    assert_eq!(editor(&h).draft_pivot(g(LEAF)).unwrap().min, Some(0.0), "{}", h.state().status.text);
    assert!(h.state().status.text.contains("past its maximum"), "{}", h.state().status.text);
}

#[test]
fn a_pivoted_planes_grip_writes_its_own_angle_and_a_lines_end_grip_moves_it() {
    // Panel, pivoted at its start on a fixed point, swinging from -90 to 0.
    let panel = format!(
        r#", {{"guid": "{PANEL}", "name": "Panel", "start": [-450, -600, 0], "end": [450, -600, 0], "up": [0, 0, 1], "pivot": {{"to": {{"fixed": [-450, -600, 0]}}, "min": -90, "max": 0}}}}"#
    );
    let mut h = in_plan("", &panel);
    click_family(&mut h, [300.0, -600.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(g(PANEL)), "{}", h.state().status.text);
    // Its grip dragged to -45° (clockwise): the pivot's own angle, no parameter labelling it.
    let from = grip(&h);
    let to = editor(&h)
        .views
        .screen_of([-450.0 + 600.0 * std::f64::consts::FRAC_1_SQRT_2, -600.0 - 600.0 * std::f64::consts::FRAC_1_SQRT_2, 0.0])
        .unwrap();
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert_eq!(editor(&h).draft_pivot(g(PANEL)).unwrap().angle, -45.0, "{}", h.state().status.text);
    // The plane as the type flexes it runs at -45°, through its pivot.
    let f = editor(&h).draft_family().unwrap();
    let flexed = f.flexed(&editor(&h).values()).unwrap();
    let p = flexed.planes.iter().find(|p| p.guid == g(PANEL)).unwrap();
    let d = [p.end[0] - p.start[0], p.end[1] - p.start[1]];
    assert!((d[1] / d[0] + 1.0).abs() < 1e-6 && (p.start[0] + 450.0).abs() < 1e-6, "{p:?}");
    // An unpivoted line's end grip: the line selected, its end dragged 200 north.
    key(&mut h, Key::Escape);
    click_family(&mut h, [100.0, 0.0, 0.0]);
    assert_eq!(editor(&h).selected_line(), Some(g(LEAF)));
    let ends = editor(&h).views.grips_shown.clone();
    assert_eq!(ends.len(), 2, "its two ends");
    let end = editor(&h).views.screen_of([450.0, 0.0, 0.0]).unwrap();
    let to = editor(&h).views.screen_of([450.0, 200.0, 0.0]).unwrap();
    drag_screen(&mut h, end, to, PointerButton::Primary);
    let l = editor(&h).lines[0].clone();
    assert!((l.end[1] - 200.0).abs() < 1.0 && l.start == [-450.0, 0.0, 0.0], "{l:?}");
}

/// A door's swing in plan at 0°, 45° and 90° with its pivot's marker and swing range, the leaf's line selected; the rotate
/// grip while it is dragged; a pivoted plane with a slab on it; and the 3D view. Saved where `BIMCRAFT_F7D_DUMP` says (only
/// then).
#[test]
fn family_pivots_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F7D_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let pivot = format!(r#", "pivot": {{"to": {{"planes": ["{HINGE}", "{CFB}"]}}, "parameter": "Swing Angle", "min": 0, "max": 90}}"#);
    let panel = format!(
        r#", {{"guid": "{PANEL}", "name": "Panel", "start": [-450, -900, 0], "end": [450, -900, 0], "up": [0, 0, 1], "pivot": {{"to": {{"fixed": [-450, -900, 0]}}, "angle": -30, "min": -60, "max": 0}}}}"#
    );
    let script = family(&pivot, &panel).replace(
        r#""types": {"Shut": {"Swing Angle": 0}, "Half": {"Swing Angle": 45}}"#,
        r#""types": {"Shut": {"Swing Angle": 0}, "Half": {"Swing Angle": 45}, "Open": {"Swing Angle": 90}}"#,
    );
    let script = script.replace(
        r#"], "to": 40}]}"#,
        &format!(
            r#"], "to": 40}}, {{"host": {{"plane": "{PANEL}"}}, "profile": [[-400, 0], [400, 0], [400, 1200], [-400, 1200]], "to": 30}}]}}"#
        ),
    );
    let mut h = app(&script);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Swing");
    h.run();
    view(&mut h, Some(Which::Plan));
    let save = |h: &mut Harness<BimApp>, name: &str| {
        deliver(h, vec![Event::PointerMoved(pos2(1590.0, 600.0))]);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(name)).unwrap();
    };
    h.state_mut().active_family_mut().unwrap().preview_type("Shut");
    h.run();
    click_family(&mut h, [100.0, 0.0, 0.0]);
    for (t, name) in [("Shut", "f7d-plan-0.png"), ("Half", "f7d-plan-45.png"), ("Open", "f7d-plan-90.png")] {
        h.state_mut().active_family_mut().unwrap().preview_type(t);
        h.run();
        save(&mut h, name);
    }
    // The grip held half way to 20°.
    h.state_mut().active_family_mut().unwrap().preview_type("Shut");
    h.run();
    let from = grip(&h);
    let to = round_the_hinge(&h, 900.0, 20.0);
    h.hover_at(from);
    h.run();
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run();
    }
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7d-grip-drag.png")).unwrap();
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    // The pivoted plane, selected.
    key(&mut h, Key::Escape);
    h.state_mut().active_family_mut().unwrap().preview_type("Half");
    h.run();
    let (c, sn) = ((-30f64).to_radians().cos(), (-30f64).to_radians().sin());
    click_family(&mut h, [-450.0 + 700.0 * c, -900.0 + 700.0 * sn, 0.0]);
    save(&mut h, "f7d-plane.png");
    view(&mut h, None);
    save(&mut h, "f7d-3d.png");
}
