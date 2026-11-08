//! F6: Blend, Sweep and Swept Blend in the family document. Create > Blend sketches a base, Edit Top its top on the plane at
//! its Second End; Create > Sweep sketches a path, Edit Profile its profile on the plane square to it (in a view that sees
//! that plane flat); Create > Swept Blend a one-line path and Profile 1 and 2. Finish makes the form, one step of the
//! history; what can't be made is refused, the part at fault put on screen. Edit Blend and the like sketch it again with
//! every part, and Edit Vertices' twist is kept.
use super::preview_visibility::Pictures;
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::views::{Side, Which};
use egui::accesskit::Role;

const FAMILY: &str = r#"family {"name": "Box", "category": "generic", "parameters": [{"name": "Width", "value": 400}], "types": {"T": {}}, "forms": [{"profile": [[2000, 2000], [2400, 2000], [2400, 2300], [2000, 2300]], "from": 0, "to": 300}]}"#;

fn open() -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(FAMILY).unwrap();
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .with_max_steps(32)
        .with_step_dt(0.05)
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

fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// A click at point `q` of the plane being sketched on (its own axes), in the view shown.
fn click_plane(h: &mut Harness<BimApp>, q: P2) {
    let ed = editor(h);
    let p = ed.sketch_frame().expect("sketching on a plane").point(q, 0.0);
    let at = match ed.views.shown {
        Some(_) => ed.views.screen_of(p).expect("a view shown"),
        None => ed.preview().expect("the preview drawn").screen(h.state().view_rect, p),
    };
    click_at(h, at);
    h.run();
}

fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    button(h, "Rectangle");
    click_plane(h, a);
    click_plane(h, b);
}

/// The draft's form `i`'s solid's box for the type, in the family's axes.
fn bounds(h: &Harness<BimApp>, i: usize) -> ([f64; 3], [f64; 3]) {
    let f = editor(h).draft_family().unwrap();
    let values = f.values("T", &std::collections::BTreeMap::new()).unwrap();
    let s = f.shapes(&values).into_iter().find(|s| s.index == i).expect("the form builds");
    (s.min, s.max)
}

fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn title(h: &Harness<BimApp>) -> &'static str {
    editor(h).sketch_title().unwrap_or("")
}

#[test]
fn a_blend_is_sketched_base_then_top_and_made_in_one_step() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    button(&mut h, "Blend");
    assert_eq!(title(&h), "Modify | Create Blend Base Boundary");
    rectangle(&mut h, P2(-500.0, -500.0), P2(500.0, 500.0));
    // Finish with no top: refused, the top put on screen to be drawn.
    button(&mut h, "Finish Edit Mode");
    assert!(editor(&h).sketching());
    assert!(h.state().status.error, "said why");
    assert_eq!(title(&h), "Modify | Create Blend Top Boundary");
    rectangle(&mut h, P2(-200.0, -200.0), P2(200.0, 200.0));
    // Its depth, the Second End less the First, typed.
    type_into(&mut h, "Depth", "800");
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 2);
    assert_eq!(f.forms[1].kind(), FormKind::Blend);
    assert_eq!((f.forms[1].sketch.len(), f.forms[1].top.len()), (4, 4));
    let b = bounds(&h, 1);
    assert!(near(b.0, [-500.0, -500.0, 0.0], 15.0) && near(b.1, [500.0, 500.0, 800.0], 15.0), "{b:?}");
    assert_eq!(editor(&h).selected_form(), Some(1));
    // Edit Blend: both parts back, Edit Top on the top; Twist Left kept.
    button(&mut h, "Edit Blend");
    assert!(editor(&h).sketching());
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4);
    button(&mut h, "Edit Top");
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 4);
    button(&mut h, "Twist Left");
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    assert_eq!(editor(&h).draft_family().unwrap().forms[1].twist, 1);
    // One Ctrl+Z takes the edit back: the blend as it was made, untwisted.
    h.state_mut().family_undo();
    h.run();
    let f = editor(&h).draft_family().unwrap();
    assert_eq!((f.forms.len(), f.forms[1].twist), (2, 0));
}

#[test]
fn a_sweep_is_a_path_in_plan_and_a_profile_in_the_elevation_square_to_it() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    button(&mut h, "Sweep");
    assert_eq!(title(&h), "Modify | Sweep > Sketch Path");
    // Edit Profile before a path: refused, saying so.
    button(&mut h, "Edit Profile");
    assert!(h.state().status.error && h.state().status.text.contains("path"), "{}", h.state().status.text);
    // An L: east 1000, then north 800, chained.
    button(&mut h, "Line");
    for q in [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 800.0)] {
        click_plane(&mut h, q);
    }
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 2);
    button(&mut h, "Edit Profile");
    assert_eq!(title(&h), "Modify | Sweep > Edit Profile");
    // The profile's plane faces the path's first line, east: a side elevation sees it flat.
    let shown = editor(&h).views.shown;
    assert!(matches!(shown, Some(Which::Elevation(Side::Left | Side::Right))), "{shown:?}");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms[1].kind(), FormKind::Sweep);
    let b = bounds(&h, 1);
    assert!(near(b.0, [0.0, -100.0, 0.0], 15.0) && near(b.1, [1100.0, 800.0, 200.0], 15.0), "{b:?}");
}

#[test]
fn a_swept_blend_takes_one_line_and_two_profiles_and_a_void_cuts() {
    let mut h = open();
    view(&mut h, Some(Which::Plan));
    // A Void Swept Blend, from the Void Forms menu.
    button(&mut h, "Void Forms");
    button(&mut h, "Void Swept Blend");
    assert!(editor(&h).sketch.as_ref().is_some_and(|s| s.void && s.kind == FormKind::SweptBlend));
    button(&mut h, "Cancel Edit Mode");
    button(&mut h, "Swept Blend");
    assert_eq!(title(&h), "Modify | Swept Blend > Sketch Path");
    button(&mut h, "Line");
    click_plane(&mut h, P2(0.0, 0.0));
    click_plane(&mut h, P2(0.0, 1000.0));
    click_plane(&mut h, P2(500.0, 1000.0));
    h.key_press(Key::Escape);
    h.run();
    // Two lines: refused, the path put back on screen.
    button(&mut h, "Profile 1");
    rectangle(&mut h, P2(-100.0, 0.0), P2(100.0, 200.0));
    button(&mut h, "Finish Edit Mode");
    assert!(editor(&h).sketching());
    assert!(h.state().status.text.contains("one line"), "{}", h.state().status.text);
    assert_eq!(title(&h), "Modify | Swept Blend > Sketch Path");
    let s = editor_mut(&mut h).sketch.as_mut().unwrap();
    s.sketch.lines.truncate(1);
    button(&mut h, "Profile 2");
    assert_eq!(title(&h), "Modify | Swept Blend > Edit Profile 2");
    rectangle(&mut h, P2(-50.0, 0.0), P2(50.0, 100.0));
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", h.state().status.text);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms[1].kind(), FormKind::SweptBlend);
    let b = bounds(&h, 1);
    assert!(near(b.0, [-100.0, 0.0, 0.0], 15.0) && near(b.1, [100.0, 1000.0, 200.0], 15.0), "{b:?}");
}

/// A blend, a sweep along an L and a swept blend along an arc, side by side.
const SHOWN: &str = r#"family {"name": "Forms", "category": "generic", "types": {"T": {}}, "forms": [ {"sketch": [{"start": [-2600, -400], "end": [-1800, -400]}, {"start": [-1800, -400], "end": [-1800, 400]}, {"start": [-1800, 400], "end": [-2600, 400]}, {"start": [-2600, 400], "end": [-2600, -400]}], "top": [{"centre": [-2200, 0], "radius": 200}], "from": 0, "to": 900}, {"sketch": [{"start": [-800, -600], "end": [600, -600]}, {"start": [600, -600], "end": [600, 600]}], "sweep": {"profile": [{"start": [-100, 0], "end": [100, 0]}, {"start": [100, 0], "end": [100, 300]}, {"start": [100, 300], "end": [-100, 300]}, {"start": [-100, 300], "end": [-100, 0]}]}}, {"sketch": [{"start": [1600, -600], "end": [1600, 600], "turn": -120}], "sweep": {"profile": [{"start": [-200, 0], "end": [200, 0]}, {"start": [200, 0], "end": [200, 400]}, {"start": [200, 400], "end": [-200, 400]}, {"start": [-200, 400], "end": [-200, 0]}], "end": [{"centre": [0, 100], "radius": 100}]}}, {"sketch": [{"centre": [0, 1600], "radius": 500}], "sweep": {"profile": [{"centre": [0, 100], "radius": 80}]}}]}"#;

/// Each form in plan, an elevation and {3D}, and each in sketch mode, saved where `BIMCRAFT_F6_DUMP` says (only then).
#[test]
fn family_blend_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F6_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(SHOWN);
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Forms");
    h.run();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f6-{name}.png"))).unwrap();
    };
    view(&mut h, Some(Which::Plan));
    save(&mut h, "plan");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    save(&mut h, "front");
    view(&mut h, None);
    save(&mut h, "3d");
    // The blend: its base in plan, its top in plan, and in {3D}.
    view(&mut h, Some(Which::Plan));
    editor_mut(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    save(&mut h, "blend-base");
    button(&mut h, "Edit Top");
    save(&mut h, "blend-top");
    button(&mut h, "Cancel Edit Mode");
    // The sweep: its path in plan, its profile in the side elevation.
    view(&mut h, Some(Which::Plan));
    editor_mut(&mut h).edit_form_sketch(1).unwrap();
    h.run();
    save(&mut h, "sweep-path");
    button(&mut h, "Edit Profile");
    save(&mut h, "sweep-profile");
    button(&mut h, "Cancel Edit Mode");
    // The swept blend: its path, and its second profile in {3D}.
    view(&mut h, Some(Which::Plan));
    editor_mut(&mut h).edit_form_sketch(2).unwrap();
    h.run();
    save(&mut h, "swept-path");
    view(&mut h, None);
    button(&mut h, "Profile 2");
    save(&mut h, "swept-profile2");
    button(&mut h, "Cancel Edit Mode");
}
