//! F5: sketch mode in the family document. Create > Extrusion and Revolve (and Void Forms') sketch on the view's work plane,
//! in a plan, an elevation or {3D}, with the Draw panel's shapes; Finish Edit Mode refuses lines that don't make loops (or a
//! revolve without its axis), saying what to fix and leaving the draft as it was, and otherwise makes the form, one step of the
//! document's history; Cancel Edit Mode leaves the draft as it was. Edit Extrusion sketches a form again, keeping its lines'
//! ids, GUIDs and, where they didn't move, formulas. A selected extrusion's Start and End drag by their handles in an
//! elevation, unless tied to a parameter.
use super::preview_visibility::Pictures;
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, Expr, Frame, PlaneRef, REF_LEVEL};
use egui::accesskit::Role;

/// A box from Ref. Level up to `Height`, its corners formulas of `Width`; and a plane of its own facing -x, `Back`.
const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "parameters": [{"name": "Width", "value": 400}, {"name": "Height", "value": 600}], "types": {"T": {}}, "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 300], [0, 300]], "from": 0, "to": "Height"}], "ref_planes": [{"name": "Back", "start": [-500, 2000, 0], "end": [-500, -2000, 0], "up": [0, 0, 1]}]}"#;

fn open() -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(FAMILY).unwrap();
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .with_max_steps(32)
        .renderer(Pictures::default())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h.state_mut().edit_family("Box");
    h.run();
    h
}

fn editor<'a>(h: &'a Harness<'_, BimApp>) -> &'a crate::editor::FamilyEditor {
    h.state().family_editor.as_ref().expect("a family open")
}

fn editor_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().family_editor.as_mut().expect("a family open")
}

/// The Project Browser's entry for `which` clicked.
fn view(h: &mut Harness<BimApp>, which: Option<Which>) {
    let (label, nth) = match which {
        None => ("{3D}", 0),
        Some(Which::Plan) => ("Ref. Level", 0),
        Some(Which::Ceiling) => ("Ref. Level", 1),
        Some(Which::Elevation(s)) => (s.name(), 0),
    };
    if let Some(n) = h.get_all_by_label(label).nth(nth) {
        n.scroll_to_me();
    }
    h.run();
    let at = h.get_all_by_label(label).nth(nth).map(|n| n.rect().center()).expect("the browser's entry");
    click_at(h, at);
    h.run();
}

/// The ribbon's button `label` pressed (a label the Properties may show too).
fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// The frame of the plane being sketched on.
fn frame(h: &Harness<BimApp>) -> Frame {
    let ed = editor(h);
    ed.plane_set().frame(&ed.sketch.as_ref().expect("sketching").host).unwrap()
}

/// Where family point `p` is on screen, in the view shown.
fn screen(h: &Harness<BimApp>, p: [f64; 3]) -> Pos2 {
    let ed = editor(h);
    match ed.views.shown {
        Some(_) => ed.views.screen_of(p).expect("a view shown"),
        None => ed.preview().expect("the preview drawn").screen(h.state().view_rect, p),
    }
}

/// A click at point `q` of the plane being sketched on (its own axes).
fn click_plane(h: &mut Harness<BimApp>, q: P2) {
    let at = screen(h, frame(h).point(q, 0.0));
    click_at(h, at);
    h.run();
}

/// Each form's solid, built for the type: its least and greatest corners in the family's axes.
fn boxes(h: &Harness<BimApp>) -> Vec<([f64; 3], [f64; 3])> {
    let f = editor(h).draft_family().unwrap();
    let values = f.values("T", &std::collections::BTreeMap::new()).unwrap();
    f.shapes(&values).iter().map(|s| (s.min, s.max)).collect()
}

fn near<const N: usize>(a: [f64; N], b: [f64; N], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

/// A rectangle drawn with Rectangle from `a` to `b` on the plane.
fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    button(h, "Rectangle");
    click_plane(h, a);
    click_plane(h, b);
}

#[test]
fn an_extrusion_sketched_in_plan_is_one_form_and_one_step_of_the_history() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    button(&mut h, "Extrusion");
    assert!(editor(&h).sketching());
    assert!(h.query_all_by_label("Modify | Create Extrusion").count() > 0, "its contextual tab");
    rectangle(&mut h, P2(-1000.0, -800.0), P2(-600.0, -500.0));
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4);
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1, "nothing in the draft until Finish");
    // Its Depth, typed in the Options Bar.
    type_into(&mut h, "Depth", "450");
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching());
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2);
    assert_eq!(f.forms[1].sketch.len(), 4);
    let b = boxes(&h)[1];
    assert!(near(b.0, [-1000.0, -800.0, 0.0], 15.0) && near(b.1, [-600.0, -500.0, 450.0], 15.0), "{b:?}");
    assert_eq!(editor(&h).selected_form(), Some(1), "the form made is selected");
    // One Ctrl+Z takes the whole sketch back.
    h.state_mut().family_undo();
    h.run();
    assert_eq!(editor(&h).draft_family().unwrap().forms.len(), 1);
}

#[test]
fn cancel_leaves_the_draft_and_its_unsaved_mark_as_they_were() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    let before = editor(&h).draft_family().unwrap();
    assert!(!editor(&h).unsaved());
    button(&mut h, "Extrusion");
    rectangle(&mut h, P2(-1000.0, -800.0), P2(-600.0, -500.0));
    button(&mut h, "Cancel Edit Mode");
    assert!(!editor(&h).sketching());
    assert_eq!(editor(&h).draft_family().unwrap(), before);
    assert!(!editor(&h).unsaved(), "nothing to save");
    // Edit Extrusion cancelled: the form as it was.
    editor_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    let s = editor_mut(&mut h).sketch.as_mut().unwrap();
    s.sketch.lines.clear();
    button(&mut h, "Cancel Edit Mode");
    assert_eq!(editor(&h).draft_family().unwrap(), before);
    assert!(!editor(&h).unsaved());
}

#[test]
fn finish_refuses_an_open_loop_and_a_revolve_without_its_axis_saying_what_to_fix() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    let before = editor(&h).draft_family().unwrap();
    button(&mut h, "Extrusion");
    button(&mut h, "Line");
    for q in [P2(-1000.0, -800.0), P2(-600.0, -800.0), P2(-600.0, -500.0)] {
        click_plane(&mut h, q);
    }
    button(&mut h, "Finish Edit Mode");
    assert!(editor(&h).sketching(), "still sketching");
    let status = h.state().status.text.clone();
    assert!(status.contains("open"), "{status}");
    assert!(!editor(&h).sketch.as_ref().unwrap().sketch.wrong.is_empty(), "the open lines in red");
    assert_eq!(editor(&h).draft_family().unwrap(), before, "the draft as it was");
    button(&mut h, "Cancel Edit Mode");
    // Two lines crossing.
    button(&mut h, "Extrusion");
    button(&mut h, "Line");
    for q in [P2(-1000.0, -800.0), P2(-600.0, -500.0), P2(-600.0, -800.0), P2(-1000.0, -500.0), P2(-1000.0, -800.0)] {
        click_plane(&mut h, q);
    }
    button(&mut h, "Finish Edit Mode");
    assert!(h.state().status.text.contains("intersect"), "{}", h.state().status.text);
    assert_eq!(editor(&h).draft_family().unwrap(), before);
    button(&mut h, "Cancel Edit Mode");
    // A revolve with no axis, then its axis drawn across the profile.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    button(&mut h, "Revolve");
    rectangle(&mut h, P2(200.0, 0.0), P2(400.0, 500.0));
    button(&mut h, "Finish Edit Mode");
    assert!(h.state().status.text.contains("axis"), "{}", h.state().status.text);
    button(&mut h, "Axis Line");
    click_plane(&mut h, P2(300.0, 0.0));
    click_plane(&mut h, P2(300.0, 600.0));
    button(&mut h, "Finish Edit Mode");
    assert!(h.state().status.text.contains("crosses the axis"), "{}", h.state().status.text);
    assert_eq!(editor(&h).draft_family().unwrap(), before);
}

#[test]
fn a_revolve_sketched_in_an_elevation_turns_about_its_axis_line() {
    let mut h = open();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    button(&mut h, "Revolve");
    assert_eq!(editor(&h).sketch.as_ref().unwrap().host, PlaneRef::Plane(CENTER_FRONT_BACK));
    rectangle(&mut h, P2(-1300.0, 0.0), P2(-1100.0, 500.0));
    button(&mut h, "Axis Line");
    click_plane(&mut h, P2(-1500.0, 0.0));
    click_plane(&mut h, P2(-1500.0, 700.0));
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    let f = editor(&h).draft_family().unwrap();
    let form = &f.forms[1];
    assert!(form.revolves() && form.sketch.iter().filter(|l| l.axis).count() == 1);
    // A ring about the upright through x -1500: 400 out at most, from 0 to 500 high.
    let (lo, hi) = boxes(&h)[1];
    assert!(near(lo, [-1900.0, -400.0, 0.0], 20.0) && near(hi, [-1100.0, 400.0, 500.0], 20.0), "{lo:?} {hi:?}");
}

#[test]
fn edit_extrusion_keeps_the_forms_guid_and_its_lines_ids_guids_and_formulas() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    let before = editor(&h).draft_family().unwrap();
    let (guid, lines) = (before.forms[0].guid, before.forms[0].sketch.clone());
    assert!(lines.iter().all(|l| l.id != 0));
    editor_mut(&mut h).select_form(0);
    h.run();
    button(&mut h, "Edit Extrusion");
    assert!(h.query_all_by_label("Modify | Edit Extrusion").count() > 0);
    // The far side moved out 100: the line along it and the two that meet it are drawn again; the near side is as it was.
    let s = editor_mut(&mut h).sketch.as_mut().unwrap();
    s.sketch.move_end(P2(400.0, 300.0), P2(400.0, 400.0));
    s.sketch.move_end(P2(0.0, 300.0), P2(0.0, 400.0));
    button(&mut h, "Finish Edit Mode");
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms[0].guid, guid);
    let now = &f.forms[0].sketch;
    assert_eq!(now.iter().map(|l| l.guid).collect::<Vec<_>>(), lines.iter().map(|l| l.guid).collect::<Vec<_>>());
    assert_eq!(now[0], lines[0], "the near side unmoved, its formula `Width` kept");
    assert!(now[0].exprs().contains(&&Expr::Formula("Width".into())));
    let (_, hi) = boxes(&h)[0];
    assert!(near(hi, [400.0, 400.0, 600.0], 1e-6), "{hi:?}");
}

#[test]
fn a_sketch_in_3d_and_on_a_plane_facing_the_other_way_lands_where_clicked() {
    let mut h = open();
    // {3D}, on Ref. Level.
    view(&mut h, None);
    button(&mut h, "Extrusion");
    assert_eq!(editor(&h).sketch.as_ref().unwrap().host, PlaneRef::Plane(REF_LEVEL));
    rectangle(&mut h, P2(450.0, 50.0), P2(700.0, 250.0));
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4, "{}", h.state().status.text);
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    let (lo, hi) = boxes(&h)[1];
    assert!(near(lo, [450.0, 50.0, 0.0], 20.0) && near(hi, [700.0, 250.0, 250.0], 20.0), "{lo:?} {hi:?}");
    // Back faces -x: in Left (looking +x) its own x runs the way the picture's does not; what is clicked is where it lands.
    view(&mut h, Some(Which::Elevation(Side::Left)));
    let back = editor(&h).planes.iter().find(|p| p.name == "Back").unwrap().guid;
    editor_mut(&mut h).views.set_work_plane(PlaneRef::Plane(back));
    editor_mut(&mut h).let_form_go();
    h.run();
    button(&mut h, "Extrusion");
    let f = frame(&h);
    assert!(f.z[0] < -0.99, "Back's normal is -x: {:?}", f.z);
    // Two corners clicked at family points (x -500, y 200 and 500, z 100 and 400), wherever they are in Back's own axes.
    let to_plane = |p: [f64; 3]| {
        let d: [f64; 3] = std::array::from_fn(|i| p[i] - f.origin[i]);
        P2(d[0] * f.x[0] + d[1] * f.x[1] + d[2] * f.x[2], d[0] * f.y[0] + d[1] * f.y[1] + d[2] * f.y[2])
    };
    rectangle(&mut h, to_plane([-500.0, 200.0, 100.0]), to_plane([-500.0, 500.0, 400.0]));
    type_into(&mut h, "Depth", "50");
    button(&mut h, "Finish Edit Mode");
    let (lo, hi) = boxes(&h)[2];
    assert!(near([lo[1], lo[2]], [200.0, 100.0], 15.0) && near([hi[1], hi[2]], [500.0, 400.0], 15.0), "{lo:?} {hi:?}");
    assert!(near([lo[0], hi[0]], [-550.0, -500.0], 1e-6), "50 along -x from Back: {lo:?} {hi:?}");
}

#[test]
fn go_to_view_offers_the_views_that_see_the_work_plane_flat() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    editor_mut(&mut h).views.set_work_plane(PlaneRef::Plane(CENTER_FRONT_BACK));
    button(&mut h, "Extrusion");
    assert!(!editor(&h).sketching() && editor(&h).go_to.is_some(), "asked first");
    assert_eq!(editor(&h).go_to_choices(), [Which::Elevation(Side::Back), Which::Elevation(Side::Front)]);
    press(&mut h, "Elevation: Front");
    assert!(editor(&h).sketching());
    assert_eq!(editor(&h).views.shown, Some(Which::Elevation(Side::Front)));
    assert_eq!(editor(&h).sketch.as_ref().unwrap().host, PlaneRef::Plane(CENTER_FRONT_BACK));
}

#[test]
fn an_extrusions_end_drags_by_its_handle_unless_it_is_tied() {
    let mut h = open();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    // A void box of numbers: its End drags.
    h.get_by_role_and_label(Role::Button, "Void Forms").click();
    h.run();
    press(&mut h, "Void Extrusion");
    // Drawn in Front on Center (Front/Back): a rectangle 200 to 800 across, 100 to 300 up.
    rectangle(&mut h, P2(200.0, 100.0), P2(800.0, 300.0));
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    assert!(editor(&h).draft_family().unwrap().forms[1].void);
    // In Left, Center (Front/Back) is seen edge-on: its End's handle dragged 200 further along its normal (-y).
    view(&mut h, Some(Which::Elevation(Side::Left)));
    editor_mut(&mut h).select_form(1);
    h.run();
    let f = editor(&h).draft_family().unwrap();
    let to = match &f.forms[1].to {
        Expr::Number(x) => *x,
        e => panic!("{e:?}"),
    };
    let end = [500.0, -to, 200.0];
    let (a, b) = (screen(&h, end), screen(&h, [end[0], end[1] - 200.0, end[2]]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let now = match &editor(&h).draft_family().unwrap().forms[1].to {
        Expr::Number(x) => *x,
        e => panic!("{e:?}"),
    };
    assert!((now - (to + 200.0)).abs() < 15.0, "{to} to {now}");
    // The box's End is `Height`: its handle is greyed, and a drag on it is refused, saying why.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    editor_mut(&mut h).select_form(0);
    h.run();
    let (a, b) = (screen(&h, [200.0, 0.0, 600.0]), screen(&h, [200.0, 0.0, 900.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert!(h.state().status.text.contains("tied"), "{}", h.state().status.text);
    assert_eq!(editor(&h).draft_family().unwrap().forms[0].to, Expr::Formula("Height".into()));
}

/// Sketch mode with a rectangle drawn in plan, a revolve with its axis in an elevation, and the forms finished in {3D}, saved
/// where `BIMCRAFT_F5_DUMP` says (only then).
#[test]
fn family_sketch_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F5_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(FAMILY);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Box");
    h.run();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f5-{name}.png"))).unwrap();
    };
    view(&mut h, Some(Which::Plan));
    button(&mut h, "Extrusion");
    rectangle(&mut h, P2(-1000.0, -800.0), P2(-500.0, -400.0));
    save(&mut h, "sketch-rectangle-plan");
    button(&mut h, "Finish Edit Mode");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    editor_mut(&mut h).let_form_go();
    h.run();
    button(&mut h, "Revolve");
    rectangle(&mut h, P2(800.0, 0.0), P2(1000.0, 500.0));
    button(&mut h, "Axis Line");
    click_plane(&mut h, P2(700.0, 0.0));
    click_plane(&mut h, P2(700.0, 700.0));
    save(&mut h, "revolve-axis-elevation");
    button(&mut h, "Finish Edit Mode");
    view(&mut h, None);
    save(&mut h, "finished-3d");
}
