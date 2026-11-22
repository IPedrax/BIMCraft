//! A selected form's shape handles, dragged as a mouse drags them (the button down on the arrow, several frames of
//! moves with it held, then up) on a form just sketched in a new family: each side in plan, Extrusion End in an elevation, and
//! in {3D} with the default camera its sides and its End and Start, a blend's base. A greyed handle refuses saying why, the view
//! not turning (`family_handle_params.rs`); Escape mid-drag puts the form back; a drag is one step Undo takes back. A sweep has no handles.
use super::family_views::{editor, view};
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{Expr, Family};
use egui::accesskit::Role;

fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// A new generic family (its starter box, form 0, tied to Width and Depth), and in Ref. Level `kind` (Extrusion or Blend)
/// sketched as a rectangle from (-1000, -800) to (-600, -500), Finish, then clicked on its outline to select it.
fn sketched(kind: &str) -> Harness<'static, BimApp> {
    let (h, _) = super::family_views::open(r#"family {"name": "Scratch", "category": "generic", "types": {"T": {}}}"#, "Scratch");
    sketched_in(h, kind)
}

fn sketched_in(mut h: Harness<'static, BimApp>, kind: &str) -> Harness<'static, BimApp> {
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    view(&mut h, Some(Which::Plan));
    button(&mut h, kind);
    button(&mut h, "Rectangle");
    for p in [[-1000.0, -800.0, 0.0], [-600.0, -500.0, 0.0]] {
        let at = editor(&h).views.screen_of(p).unwrap();
        click_at(&mut h, at);
    }
    if kind == "Blend" {
        // Its top, smaller, inside.
        button(&mut h, "Edit Top");
        button(&mut h, "Rectangle");
        for p in [[-900.0, -700.0, 0.0], [-700.0, -600.0, 0.0]] {
            let at = editor(&h).views.screen_of(p).unwrap();
            click_at(&mut h, at);
        }
    }
    button(&mut h, "Finish Edit Mode");
    h.run();
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 2, "{}", status(&h));
    let at = editor(&h).views.screen_of([-800.0, -800.0, 0.0]).unwrap();
    click_at(&mut h, at);
    assert_eq!(editor(&h).selected_form(), Some(1), "selected by a click: {}", status(&h));
    h
}

/// A drag as a mouse makes it: over the arrow, the button down, eight frames of moves with it held, then up.
fn mouse_drag(h: &mut Harness<BimApp>, a: egui::Pos2, b: egui::Pos2) {
    mouse_down_and_move(h, a, b);
    h.event(egui::Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    h.run();
}

fn mouse_down_and_move(h: &mut Harness<BimApp>, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for k in 1..=8 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (k as f32 / 8.0)));
        h.run();
    }
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// Form 1's corners' x and y least and greatest, and its Start and End, as numbers.
fn extent(h: &Harness<BimApp>) -> ([f64; 4], (Expr, Expr)) {
    let f: Family = editor(h).draft_family().unwrap();
    let form = &f.forms[1];
    let v = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for l in &form.sketch {
        if let FormCurve::Line { start, end, .. } = &l.curve {
            for p in [start, end] {
                b = [b[0].min(v(&p[0])), b[1].min(v(&p[1])), b[2].max(v(&p[0])), b[3].max(v(&p[1]))];
            }
        }
    }
    (b, (form.from.clone(), form.to.clone()))
}

fn near(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 15.0)
}

/// The 2D view's handle at family point `p`, dragged by `by` mm.
fn drag_2d(h: &mut Harness<BimApp>, p: [f64; 3], by: [f64; 3]) {
    let s = editor(h).views.screen_of(p).unwrap();
    let e = editor(h).views.screen_of([p[0] + by[0], p[1] + by[1], p[2] + by[2]]).unwrap();
    let shapes = editor(h).views.shapes_shown.iter().map(|x| x.0);
    let ends = editor(h).views.ends_shown.iter().map(|x| x.0);
    let a = shapes.chain(ends).min_by(|a, b| a.distance(s).total_cmp(&b.distance(s))).expect("handles shown");
    assert!(a.distance(s) < 2.0, "no handle at {p:?}");
    mouse_drag(h, a, a + (e - s));
}

/// Where family point `p` shows in {3D}.
fn screen3(h: &Harness<BimApp>, p: [f64; 3]) -> egui::Pos2 {
    editor(h).drawn_3d().expect("{3D} drawn").screen(h.state().view_rect, p)
}

/// The {3D} handle nearest family point `p` (within a few points of it): where it shows, and it.
fn handle3(h: &Harness<BimApp>, p: [f64; 3]) -> (egui::Pos2, crate::editor::handles3d::Handle3d) {
    let want = screen3(h, p);
    let best = editor(h)
        .views
        .handles3d_shown
        .iter()
        .min_by(|a, b| a.0.distance(want).total_cmp(&b.0.distance(want)))
        .cloned()
        .expect("handles shown in {3D}");
    assert!(best.0.distance(want) < 2.0, "no handle at {p:?}: {:?}", editor(h).views.handles3d_shown);
    assert!(h.state().view_rect.shrink(20.0).contains(best.0), "in sight: {:?}", best.0);
    best
}

/// The {3D} handle at family point `p` dragged `by` mm along its way, the mouse held at the end when `hold`.
fn drag_3d(h: &mut Harness<BimApp>, p: [f64; 3], by: f64, hold: bool) -> egui::Pos2 {
    let (a, hd) = handle3(h, p);
    let to = [p[0] + hd.way[0] * by, p[1] + hd.way[1] * by, p[2] + hd.way[2] * by];
    let b = a + (screen3(h, to) - screen3(h, p));
    if hold {
        mouse_down_and_move(h, a, b);
    } else {
        mouse_drag(h, a, b);
    }
    b
}

fn in_3d(kind: &str) -> Harness<'static, BimApp> {
    let mut h = sketched(kind);
    view(&mut h, None);
    // The default camera, fitted to what the family has now.
    h.state_mut().active_family_mut().unwrap().orbit.camera = None;
    h.run();
    h.run();
    assert_eq!(editor(&h).selected_form(), Some(1));
    h
}

#[test]
fn a_freshly_sketched_extrusions_four_sides_drag_in_plan() {
    let mut h = sketched("Extrusion");
    assert_eq!(editor(&h).views.shapes_shown.len(), 4);
    assert!(editor(&h).views.shapes_shown.iter().all(|s| s.1.blocked.is_none()), "nothing ties a sketched side");
    let mut want = [-1000.0, -800.0, -600.0, -500.0];
    for (by, k, d) in
        [([200.0, 0.0, 0.0], 2, 200.0), ([0.0, 100.0, 0.0], 3, 100.0), ([-100.0, 0.0, 0.0], 0, -100.0), ([0.0, -150.0, 0.0], 1, -150.0)]
    {
        // Each side's handle is at its middle as it stands now.
        let (b, _) = extent(&h);
        let p = match k {
            0 => [b[0], (b[1] + b[3]) / 2.0, 0.0],
            1 => [(b[0] + b[2]) / 2.0, b[1], 0.0],
            2 => [b[2], (b[1] + b[3]) / 2.0, 0.0],
            _ => [(b[0] + b[2]) / 2.0, b[3], 0.0],
        };
        drag_2d(&mut h, p, by);
        want[k] += d;
        assert!(near(extent(&h).0, want), "{:?} {want:?} {}", extent(&h).0, status(&h));
    }
}

#[test]
fn an_elevations_extrusion_end_drags_up() {
    let mut h = sketched("Extrusion");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    assert_eq!(editor(&h).selected_form(), Some(1));
    let (_, (from, to)) = extent(&h);
    let Expr::Number(top) = to else { panic!("{to:?}") };
    drag_2d(&mut h, [-800.0, -800.0, top], [0.0, 0.0, 300.0]);
    let (_, (from2, to2)) = extent(&h);
    assert_eq!(from2, from);
    let Expr::Number(now) = to2 else { panic!("{to2:?}") };
    assert!((now - top - 300.0).abs() < 15.0, "{now} {}", status(&h));
}

#[test]
fn in_3d_a_selected_extrusion_shows_its_sides_and_ends_and_the_top_arrow_drags_its_end() {
    let mut h = in_3d("Extrusion");
    let (_, (_, to)) = extent(&h);
    let Expr::Number(top) = to else { panic!("{to:?}") };
    // The sides facing the default camera (south and east) at mid-depth and the End above; the faces turned away, none.
    assert_eq!(editor(&h).views.handles3d_shown.len(), 3, "{:?}", editor(&h).views.handles3d_shown);
    let camera = editor(&h).orbit.camera;
    drag_3d(&mut h, [-800.0, -650.0, top], 400.0, false);
    let (_, (from, to)) = extent(&h);
    assert_eq!(from, Expr::Number(0.0));
    let Expr::Number(now) = to else { panic!("{to:?}") };
    assert!((now - top - 400.0).abs() < 25.0, "{now} {}", status(&h));
    assert_eq!(editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation)), camera.map(|c| (c.azimuth, c.elevation)), "the view didn't turn");
    // A side, east, at mid-depth.
    let (b, _) = extent(&h);
    drag_3d(&mut h, [-600.0, -650.0, now / 2.0], 200.0, false);
    let (b2, _) = extent(&h);
    assert!(near(b2, [b[0], b[1], b[2] + 200.0, b[3]]), "{b2:?} {}", status(&h));
}

#[test]
fn in_3d_a_blends_base_side_drags_its_top_staying() {
    let mut h = in_3d("Blend");
    let top = editor(&h).draft_family().unwrap().forms[1].top.clone();
    assert!(!top.is_empty());
    // Its base's sides facing the camera at its base, its Second End.
    assert_eq!(editor(&h).views.handles3d_shown.len(), 3, "{:?}", editor(&h).views.handles3d_shown);
    drag_3d(&mut h, [-600.0, -650.0, 0.0], 200.0, false);
    assert!(near(extent(&h).0, [-1000.0, -800.0, -400.0, -500.0]), "{:?} {}", extent(&h).0, status(&h));
    assert_eq!(editor(&h).draft_family().unwrap().forms[1].top, top, "the top stays");
}

#[test]
fn a_sweep_has_no_handles_in_plan_or_3d() {
    let (mut h, _) = super::family_views::open(r#"family {"name": "Scratch", "category": "generic", "types": {"T": {}}}"#, "Scratch");
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    view(&mut h, Some(Which::Plan));
    button(&mut h, "Sweep");
    button(&mut h, "Line");
    for p in [[-1000.0, -800.0, 0.0], [0.0, -800.0, 0.0]] {
        let at = editor(&h).views.screen_of(p).unwrap();
        click_at(&mut h, at);
    }
    h.key_press(Key::Escape);
    h.run();
    button(&mut h, "Edit Profile");
    h.run();
    // A rectangle round the path's start, as the elevation square to it shows it.
    button(&mut h, "Rectangle");
    let s0 = editor(&h).views.screen_of([-1000.0, -800.0, 0.0]).unwrap();
    for at in [s0 + vec2(-30.0, 0.0), s0 + vec2(30.0, -50.0)] {
        click_at(&mut h, at);
    }
    button(&mut h, "Finish Edit Mode");
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2, "{} {:?}", status(&h), editor(&h).sketch.as_ref().map(|s| s.sketch.lines.len()));
    assert_eq!(f.forms[1].kind(), FormKind::Sweep, "{}", status(&h));
    h.state_mut().active_family_mut().unwrap().select_form(1);
    view(&mut h, Some(Which::Plan));
    assert!(editor(&h).views.shapes_shown.is_empty() && editor(&h).views.ends_shown.is_empty());
    view(&mut h, None);
    h.run();
    assert_eq!(editor(&h).selected_form(), Some(1));
    assert!(editor(&h).views.handles3d_shown.is_empty());
}

#[test]
fn in_3d_escape_mid_drag_puts_the_form_back_and_a_drag_is_one_step_undo_takes_back() {
    let mut h = in_3d("Extrusion");
    let before = extent(&h);
    let steps = editor(&h).steps();
    let Expr::Number(top) = before.1.1.clone() else { panic!() };
    // Escape mid-drag: back as it was, the rest of the drag doing nothing.
    let at = drag_3d(&mut h, [-800.0, -650.0, top], 300.0, true);
    // {3D} writes once the button is up: meanwhile the status bar says where the End would go.
    assert_eq!(extent(&h), before, "not written mid-drag");
    assert!(status(&h).contains("Extrusion End"), "{}", status(&h));
    key(&mut h, Key::Escape);
    assert_eq!(extent(&h), before);
    h.event(egui::Event::PointerMoved(at + vec2(0.0, -30.0)));
    h.run();
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(extent(&h), before);
    assert_eq!(editor(&h).steps(), steps, "no step");
    assert_eq!(editor(&h).selected_form(), Some(1));
    // A side and the End dragged: two steps, Undo takes each back.
    drag_3d(&mut h, [-600.0, -650.0, top / 2.0], 200.0, false);
    let sided = extent(&h);
    assert_ne!(sided, before);
    drag_3d(&mut h, [-700.0, -650.0, top], 100.0, false);
    assert_ne!(extent(&h), sided);
    assert_eq!(editor(&h).steps(), steps + 2);
    h.state_mut().family_undo();
    h.run();
    assert_eq!(extent(&h), sided);
    h.state_mut().family_undo();
    h.run();
    assert_eq!(extent(&h), before);
}

/// A form selected with its handles in plan and in {3D}, and after a drag in {3D}, in the three looks, saved where
/// `BIMCRAFT_HANDLES_3D_DUMP` says (only then).
#[test]
fn family_handles_3d_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_HANDLES_3D_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
        let mut h = app("");
        h.set_size(vec2(1400.0, 900.0));
        h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        h.state_mut().light_canvas = light_canvas;
        h.run();
        let mut h = sketched_in(h, "Extrusion");
        let save = |h: &mut Harness<BimApp>, name: &str| {
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe25-{name}-{look}.png"))).unwrap();
        };
        save(&mut h, "plan");
        view(&mut h, None);
        h.state_mut().active_family_mut().unwrap().orbit.camera = None;
        h.run();
        h.run();
        save(&mut h, "3d");
        let Expr::Number(top) = extent(&h).1.1 else { panic!() };
        drag_3d(&mut h, [-800.0, -650.0, top], 400.0, false);
        drag_3d(&mut h, [-600.0, -650.0, (top + 400.0) / 2.0], 300.0, false);
        save(&mut h, "3d-dragged");
    }
}
