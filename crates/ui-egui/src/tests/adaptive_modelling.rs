//! An adaptive family modelled in the Family Editor's view as in Revit's: points clicked on the work plane, a closed profile
//! drawn through them, a solid form made of it, the points made adaptive in order, a point dragged by its gizmo, and the
//! family applied to the project.

use super::*;
use crate::adaptive_view::Draw;
use bimcraft_core::adaptive::{Definition, Evaluated, Host, Operation};
use bimcraft_core::family::Category;

/// The app with a new family open on its Adaptive tab, made adaptive, on a screen wide enough for the view.
fn modelling() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.set_size(vec2(1800., 1000.));
    h.state_mut().new_family(Category::Generic);
    if let Some(ed) = h.state_mut().family_editor.as_mut() {
        ed.tab = crate::editor::Tab::Adaptive;
        ed.adaptive = Some(Definition::default());
    }
    h.run();
    h
}

fn definition(h: &Harness<BimApp>) -> Definition {
    h.state().family_editor.as_ref().and_then(|e| e.adaptive.clone()).expect("an adaptive family in the editor")
}

/// A click in the modelling view at model point `p`, holding `modifiers`.
fn click_model(h: &mut Harness<BimApp>, p: [f64; 3], modifiers: Modifiers) {
    let pos = h.state().family_editor.as_ref().map(|e| e.modeller.screen(p)).unwrap();
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run();
}

/// A tool or button of the view: the one beside Point, not the ribbon's Modify tab or the title bar's Undo.
fn tool(h: &mut Harness<BimApp>, label: &str) {
    if label == "Point" {
        h.get_by_role_and_label(Role::Button, label).click();
        h.run();
    } else {
        // On Point's row: the ribbon's tab and the title bar's button lie above it.
        let row = h.get_by_label("Point").rect().center().y;
        let pos = h.get_all_by_label(label).map(|n| n.rect().center()).min_by(|a, b| (a.y - row).abs().total_cmp(&(b.y - row).abs()));
        click_at(h, pos.unwrap());
    }
}

const CORNERS: [[f64; 3]; 4] = [[0., 0., 0.], [2000., 0., 0.], [2000., 1500., 0.], [0., 1500., 0.]];

/// The four corners clicked with Point, then a closed profile drawn through them with Line.
fn four_points_and_a_profile(h: &mut Harness<BimApp>) {
    // The view fits what there is when it first shows: an empty family, round the work plane's origin.
    tool(h, "Point");
    for p in CORNERS {
        click_model(h, p, Modifiers::NONE);
    }
    tool(h, "Line");
    for p in CORNERS.iter().chain(&CORNERS[..1]) {
        click_model(h, *p, Modifiers::NONE);
    }
}

#[test]
fn points_clicked_on_the_work_plane_and_a_closed_profile_through_them_make_a_solid_form() {
    let mut h = modelling();
    four_points_and_a_profile(&mut h);
    let d = definition(&h);
    let r = d.evaluate(&Default::default(), &d.preview);
    let points: Vec<[f64; 3]> = r.order.iter().filter_map(|id| r.origin(id)).collect();
    assert_eq!(points.len(), 4, "the profile reused the points clicked: {:?}", d.nodes);
    for (p, want) in points.iter().zip(CORNERS) {
        assert!((0..3).all(|i| (p[i] - want[i]).abs() < 10.0), "{p:?} near {want:?}");
    }
    let profile = d.nodes.iter().find(|n| matches!(n.operation, Operation::Polyline { closed: true, .. })).expect("a closed profile");
    assert_eq!(h.state().family_editor.as_ref().unwrap().modeller.selection, vec![profile.id.clone()], "the profile drawn is selected");
    tool(&mut h, "Create Solid Form");
    let d = definition(&h);
    let r = d.evaluate(&Default::default(), &d.preview);
    let form = d.nodes.iter().find(|n| matches!(n.operation, Operation::Extrude { .. })).expect("a solid form");
    assert!(matches!(r.values.get(&form.id), Some(Evaluated::Form { mesh, .. }) if mesh.closed), "{:?}", r.diagnostics);
    // Back to Modify, the points selected in the order 2, 1, 3, 4 with Ctrl and made adaptive: numbered so.
    tool(&mut h, "Modify");
    click_model(&mut h, CORNERS[1], Modifiers::NONE);
    for k in [0, 2, 3] {
        click_model(&mut h, CORNERS[k], Modifiers::CTRL);
    }
    tool(&mut h, "Make Adaptive");
    let d = definition(&h);
    let numbers: Vec<u32> = d
        .nodes
        .iter()
        .filter_map(|n| match &n.operation {
            Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, [2, 1, 3, 4]);
    // Applied, the project has it, adaptive, with its form.
    press_near(&mut h, "Apply", "Cancel");
    let m = &h.state().session.model;
    let f = m.families.values().find(|f| f.adaptive.is_some()).expect("the project has the family");
    assert_eq!(f.adaptive.as_ref().map(|d| d.next_number()), Some(5));
}

#[test]
fn a_gizmo_arrow_dragged_moves_the_point_along_its_axis_and_undo_puts_it_back() {
    let mut h = modelling();
    tool(&mut h, "Point");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    tool(&mut h, "Modify");
    let d = definition(&h);
    let id = d.nodes[0].id.clone();
    assert_eq!(h.state().family_editor.as_ref().unwrap().modeller.selection, vec![id.clone()], "the point made is selected");
    let at = |h: &Harness<BimApp>| {
        let d = definition(h);
        d.evaluate(&Default::default(), &d.preview).origin(&id).unwrap()
    };
    let before = at(&h);
    // The z arrow points up the screen from the point: a drag along it raises the point.
    let (base, tip) = {
        let m = &h.state().family_editor.as_ref().unwrap().modeller;
        let o = before;
        let scale = m.camera.unwrap().scale;
        (m.screen(o), m.screen([o[0], o[1], o[2] + 60. / scale]))
    };
    let grab = base + (tip - base) * 0.7;
    drag_screen(&mut h, grab, grab + vec2(0., -80.), PointerButton::Primary);
    let after = at(&h);
    assert!(after[2] > before[2] + 50., "raised: {before:?} -> {after:?}");
    assert!((after[0] - before[0]).abs() < 1e-6 && (after[1] - before[1]).abs() < 1e-6, "only along z");
    tool(&mut h, "Undo");
    assert_eq!(at(&h), before);
    // Delete takes it away.
    tool(&mut h, "Delete");
    assert!(definition(&h).nodes.is_empty());
}

#[test]
fn a_point_clicked_on_a_line_is_hosted_on_it() {
    let mut h = modelling();
    tool(&mut h, "Line");
    click_model(&mut h, [0., 0., 0.], Modifiers::NONE);
    click_model(&mut h, [2000., 0., 0.], Modifiers::NONE);
    h.key_press(Key::Enter);
    h.run();
    let d = definition(&h);
    assert!(d.nodes.iter().any(|n| matches!(n.operation, Operation::Line { .. })), "{:?}", d.nodes);
    tool(&mut h, "Point");
    click_model(&mut h, [1000., 0., 0.], Modifiers::NONE);
    let d = definition(&h);
    assert!(d.nodes.iter().any(|n| matches!(&n.operation, Operation::Point { host: Host::Curve { .. }, .. })), "{:?}", d.nodes);
    assert!(matches!(h.state().family_editor.as_ref().unwrap().modeller.tool, Draw::Point));
}

/// The four-point panel in the project, the Component tool placing it.
fn placing_the_panel() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.state_mut()
        .session
        .execute("family", &json!({"name":"Adaptive panel","types":{"A":{}},"adaptive":bimcraft_core::adaptive::four_point_panel()}))
        .unwrap();
    h.state_mut().component = Some(("Adaptive panel".into(), "A".into()));
    h.state_mut().set_tool(Tool::Component);
    h.run();
    h
}

fn instances(h: &Harness<BimApp>) -> Vec<Vec<(u32, [f64; 3])>> {
    let m = &h.state().session.model;
    m.elements
        .values()
        .filter_map(|e| {
            if let Element::Component(c) = e { Some(m.adaptive_points(c).into_iter().map(|(n, f)| (n, f.origin)).collect()) } else { None }
        })
        .collect()
}

#[test]
fn an_adaptive_component_is_placed_by_clicking_its_points_in_order_in_a_plan() {
    let mut h = placing_the_panel();
    let corners = [P2(0., 0.), P2(3000., 500.), P2(3200., 2500.), P2(-200., 2000.)];
    for (k, p) in corners.iter().enumerate() {
        assert!(instances(&h).is_empty(), "nothing placed before the last point");
        click(&mut h, *p);
        if k < 3 {
            assert_eq!(h.state().adaptive_clicks.len(), k + 1);
        }
    }
    let placed = instances(&h);
    assert_eq!(placed.len(), 1, "the fourth click places it");
    for ((n, at), want) in placed[0].iter().zip(corners) {
        assert!((at[0] - want.0).abs() < 1.0 && (at[1] - want.1).abs() < 1.0 && at[2].abs() < 1e-6, "point {n}: {at:?} at {want:?}");
    }
    assert!(h.state().adaptive_clicks.is_empty());
    // A second, its first point clicked on the first's third: it lands exactly there.
    click(&mut h, P2(3205., 2496.));
    assert_eq!(h.state().adaptive_clicks[0], placed[0][2].1);
    // Escape forgets the points clicked and keeps the tool.
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().adaptive_clicks.is_empty());
    assert!(matches!(h.state().tool, Tool::Component));
}

#[test]
fn an_adaptive_instance_selected_shows_grips_and_a_drag_moves_its_point() {
    let mut h = placing_the_panel();
    for p in [P2(0., 0.), P2(3000., 0.), P2(3000., 2000.), P2(0., 2000.)] {
        click(&mut h, p);
    }
    h.key_press(Key::Escape);
    h.run();
    let id = find(&h, |e| matches!(e, Element::Component(_)));
    h.state_mut().selection = vec![id];
    h.state_mut().set_tool(Tool::Modify);
    h.run();
    drag(&mut h, P2(3000., 2000.), P2(4000., 3000.));
    let points = &instances(&h)[0];
    let third = points.iter().find(|(n, _)| *n == 3).unwrap().1;
    assert!((third[0] - 4000.).abs() < 1.0 && (third[1] - 3000.).abs() < 1.0, "{third:?}");
    let first = points.iter().find(|(n, _)| *n == 1).unwrap().1;
    assert!(first[0].abs() < 1.0 && first[1].abs() < 1.0, "the others stay: {first:?}");
    // One undo step puts it back.
    h.state_mut().run("undo", json!({}));
    let third = instances(&h)[0].iter().find(|(n, _)| *n == 3).unwrap().1;
    assert!((third[0] - 3000.).abs() < 1.0, "{third:?}");
}
