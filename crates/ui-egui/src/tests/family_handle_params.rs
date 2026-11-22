//! FE-25: wherever a shape handle shows, it drags. The starter box's sides are `Width / 2` and `Depth / 2` either way of the
//! centre and its End is `Height`: a drag of a side or the End, as a mouse makes it, changes that parameter for the type
//! previewed, the side or end landing where the hand lets go, in plan, an elevation and {3D}; the other type keeps its value.
//! One whose parameter is worked out by a formula is greyed, never lit, and refuses saying why. A drag is one step Undo takes
//! back, and Escape mid-drag puts the value back.
use super::family_views::{editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};

/// The starter box of a new generic family, with a second type, Wide, and a parameter Twice (Width's double, a formula) that
/// a second box's east side reads.
const STARTER: &str = r#"family {"name": "Starter", "category": "generic", "parameters": [{"name": "Width", "value": 1000}, {"name": "Depth", "value": 600}, {"name": "Height", "value": 750}, {"name": "Twice", "formula": "Width * 2"}], "types": {"1000 x 600": {}, "Wide": {"Width": 1400}}, "forms": [{"profile": [["-Width / 2", "-Depth / 2"], ["Width / 2", "-Depth / 2"], ["Width / 2", "Depth / 2"], ["-Width / 2", "Depth / 2"]], "from": 0, "to": "Height"}, {"profile": [[3000, 0], ["3000 + Twice / 4", 0], ["3000 + Twice / 4", 400], [3000, 400]], "from": 0, "to": 300}]}"#;

fn opened(which: Option<Which>) -> Harness<'static, BimApp> {
    opened_on(STARTER, "Starter", which)
}

/// Family `name` of `script` open in view `which` ({3D} fitted to it), its first form selected.
pub(super) fn opened_on(script: &str, name: &str, which: Option<Which>) -> Harness<'static, BimApp> {
    let (mut h, _) = open(script, name);
    view(&mut h, which);
    if which.is_none() {
        // The default camera, fitted to the family.
        h.state_mut().active_family_mut().unwrap().orbit.camera = None;
        h.run();
    }
    h.state_mut().active_family_mut().unwrap().select_form(0);
    h.run();
    h.run();
    h
}

/// Parameter `name`'s value in type `ty`, as the draft holds it (none where the type leaves it to the parameter).
pub(super) fn value(h: &Harness<BimApp>, ty: &str, name: &str) -> Option<f64> {
    editor(h).draft_family().unwrap().types.get(ty)?.get(name)?.as_f64()
}

pub(super) fn status(h: &Harness<BimApp>) -> (String, bool) {
    (h.state().status.text.clone(), h.state().status.error)
}

pub(super) fn press_move(h: &mut Harness<BimApp>, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for k in 1..=8 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (k as f32 / 8.0)));
        h.run();
    }
}

pub(super) fn release(h: &mut Harness<BimApp>, b: egui::Pos2) {
    h.event(egui::Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    h.run();
}

/// Where family point `p` shows in the view shown.
pub(super) fn screen(h: &Harness<BimApp>, p: [f64; 3]) -> egui::Pos2 {
    match editor(h).views.shown {
        Some(_) => editor(h).views.screen_of(p).unwrap(),
        None => editor(h).drawn_3d().expect("{3D} drawn").screen(h.state().view_rect, p),
    }
}

/// The handle shown nearest family point `p` (within a few points): where it shows and whether it can be dragged.
pub(super) fn handle(h: &Harness<BimApp>, p: [f64; 3]) -> (egui::Pos2, bool) {
    let ed = editor(h);
    let want = screen(h, p);
    let all: Vec<(egui::Pos2, bool)> = match ed.views.shown {
        Some(_) => {
            ed.views.shapes_shown.iter().map(|(at, s)| (*at, s.blocked.is_none())).chain(ed.views.ends_shown.iter().copied()).collect()
        }
        None => ed.views.handles3d_shown.iter().map(|(at, x)| (*at, x.blocked.is_none())).collect(),
    };
    let best = all.iter().copied().min_by(|a, b| a.0.distance(want).total_cmp(&b.0.distance(want))).expect("handles shown");
    assert!(best.0.distance(want) < 2.0, "no handle at {p:?}: {all:?}");
    best
}

/// A drag of the handle at family point `p` by `by` mm, as a mouse makes it; the button held at the end when `hold`.
pub(super) fn drag(h: &mut Harness<BimApp>, p: [f64; 3], by: [f64; 3], hold: bool) -> egui::Pos2 {
    let (a, free) = handle(h, p);
    assert!(free, "the handle at {p:?} drags");
    let b = a + (screen(h, [p[0] + by[0], p[1] + by[1], p[2] + by[2]]) - screen(h, p));
    press_move(h, a, b);
    if !hold {
        release(h, b);
    }
    b
}

#[test]
fn the_starter_boxs_side_drags_width_in_plan_for_the_type_previewed_alone() {
    let mut h = opened(Some(Which::Plan));
    // Every handle of the starter box drags (its sides are Width's and Depth's).
    assert_eq!(editor(&h).views.shapes_shown.len(), 4);
    assert!(editor(&h).views.shapes_shown.iter().all(|s| s.1.blocked.is_none()), "{:?}", editor(&h).views.shapes_shown);
    let steps = editor(&h).steps();
    // The east side (x 500) out 100: it lands under the hand, at 600, so Width (centred) is 1200.
    drag(&mut h, [500.0, 0.0, 0.0], [100.0, 0.0, 0.0], false);
    let w = value(&h, "1000 x 600", "Width").expect("the type's Width set");
    assert!((w - 1200.0).abs() < 10.0, "{w} {:?}", status(&h));
    assert_eq!(value(&h, "Wide", "Width"), Some(1400.0), "the other type keeps its own");
    assert_eq!(editor(&h).steps(), steps + 1, "one step");
    // The north side (y 300) in 50: Depth 500.
    drag(&mut h, [0.0, 300.0, 0.0], [0.0, -50.0, 0.0], false);
    let d = value(&h, "1000 x 600", "Depth").expect("the type's Depth set");
    assert!((d - 500.0).abs() < 10.0, "{d} {:?}", status(&h));
    // Undo takes back the Depth's drag alone, then the Width's.
    h.state_mut().family_undo();
    h.run();
    assert_eq!(value(&h, "1000 x 600", "Depth"), None);
    assert!(value(&h, "1000 x 600", "Width").is_some_and(|x| (x - 1200.0).abs() < 10.0));
    h.state_mut().family_undo();
    h.run();
    assert_eq!(value(&h, "1000 x 600", "Width"), None);
}

#[test]
fn the_starter_boxs_side_and_top_drag_width_and_height_in_3d() {
    let mut h = opened(None);
    // The faces toward the camera: south, east and the top.
    assert_eq!(editor(&h).views.handles3d_shown.len(), 3, "{:?}", editor(&h).views.handles3d_shown);
    let camera = editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation));
    drag(&mut h, [500.0, 0.0, 375.0], [100.0, 0.0, 0.0], false);
    let w = value(&h, "1000 x 600", "Width").expect("the type's Width set");
    assert!((w - 1200.0).abs() < 15.0, "{w} {:?}", status(&h));
    assert_eq!(value(&h, "Wide", "Width"), Some(1400.0));
    // The top (End, Height) up 200.
    drag(&mut h, [0.0, 0.0, 750.0], [0.0, 0.0, 200.0], false);
    let ht = value(&h, "1000 x 600", "Height").expect("the type's Height set");
    assert!((ht - 950.0).abs() < 15.0, "{ht} {:?}", status(&h));
    assert_eq!(editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation)), camera, "the view didn't turn");
}

#[test]
fn an_end_tied_to_height_drags_it_in_an_elevation() {
    let mut h = opened(Some(Which::Elevation(Side::Front)));
    drag(&mut h, [0.0, -300.0, 750.0], [0.0, 0.0, 250.0], false);
    let ht = value(&h, "1000 x 600", "Height").expect("the type's Height set");
    assert!((ht - 1000.0).abs() < 10.0, "{ht} {:?}", status(&h));
    assert!(editor(&h).draft_family().unwrap().forms[0].to == bimcraft_core::family::Expr::Formula("Height".into()), "still Height");
}

#[test]
fn a_side_a_formula_drives_is_greyed_never_lit_and_refuses_saying_why() {
    for which in [Some(Which::Plan), None] {
        let mut h = opened(which);
        h.state_mut().active_family_mut().unwrap().select_form(1);
        h.run();
        h.run();
        let before = editor(&h).draft_family().unwrap();
        let steps = editor(&h).steps();
        // The east side, at 3000 + Twice / 4 = 3500.
        let (at, free) = handle(&h, [3500.0, 200.0, if which.is_none() { 150.0 } else { 0.0 }]);
        assert!(!free, "greyed");
        h.hover_at(at);
        h.run();
        assert_eq!(editor(&h).views.lit_handle, None, "never lit");
        // A free one is lit under the pointer.
        // The south side, free, faces the camera in both.
        let (west, ok) = handle(&h, [3250.0, 0.0, if which.is_none() { 150.0 } else { 0.0 }]);
        assert!(ok);
        h.hover_at(west);
        h.run();
        assert_eq!(editor(&h).views.lit_handle, Some(west), "a free one lit");
        let camera = editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation));
        press_move(&mut h, at, at + vec2(40.0, 0.0));
        release(&mut h, at + vec2(40.0, 0.0));
        assert_eq!(editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation)), camera, "the view didn't turn");
        assert_eq!(editor(&h).steps(), steps, "no step");
        let (text, error) = status(&h);
        assert!(error && text.contains("Twice") && text.contains("formula"), "{text}");
        assert_eq!(editor(&h).draft_family().unwrap(), before);
    }
}

#[test]
fn escape_mid_drag_puts_the_parameter_back() {
    for which in [Some(Which::Plan), None] {
        let mut h = opened(which);
        let steps = editor(&h).steps();
        let z = if which.is_none() { 375.0 } else { 0.0 };
        let at = drag(&mut h, [500.0, 0.0, z], [100.0, 0.0, 0.0], true);
        // Written as it goes in plan; {3D} writes once the button is up.
        assert_eq!(value(&h, "1000 x 600", "Width").is_some(), which.is_some(), "live in plan alone");
        key(&mut h, Key::Escape);
        assert_eq!(value(&h, "1000 x 600", "Width"), None);
        release(&mut h, at);
        assert_eq!(value(&h, "1000 x 600", "Width"), None);
        assert_eq!(editor(&h).steps(), steps);
    }
}

/// The starter box after its east side was dragged in {3D}, Family Types open on the new Width, in the three looks, saved where
/// `BIMCRAFT_HANDLE_PARAMS_DUMP` says (only then).
#[test]
fn family_handle_params_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_HANDLE_PARAMS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        let mut h = app(STARTER);
        h.set_size(vec2(1400.0, 900.0));
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        h.state_mut().edit_family("Starter");
        h.run();
        view(&mut h, None);
        h.state_mut().active_family_mut().unwrap().orbit.camera = None;
        h.run();
        h.state_mut().active_family_mut().unwrap().select_form(0);
        h.run();
        h.run();
        // Mid-drag: the arrow where the hand has it, the value the status bar says; then let go.
        let at = drag(&mut h, [500.0, 0.0, 375.0], [200.0, 0.0, 0.0], true);
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe25-param-3d-mid-{look}.png"))).unwrap();
        release(&mut h, at);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe25-param-3d-{look}.png"))).unwrap();
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Family Types").click();
        h.run();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe25-param-types-{look}.png"))).unwrap();
    }
}
