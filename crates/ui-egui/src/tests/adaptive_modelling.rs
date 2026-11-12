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
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.adaptive = Some(Definition::default());
    }
    h.run();
    h
}

fn definition(h: &Harness<BimApp>) -> Definition {
    h.state().active_family().and_then(|e| e.adaptive.clone()).expect("an adaptive family in the editor")
}

/// A click in the modelling view at model point `p`, holding `modifiers`.
fn click_model(h: &mut Harness<BimApp>, p: [f64; 3], modifiers: Modifiers) {
    let pos = h.state().active_family().map(|e| e.modeller.screen(p)).unwrap();
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run();
}

/// A button of the family document's ribbon (or the title bar's Undo), as a user finds it: Point and Line on the Create tab,
/// Create Solid Form as Create Form's Solid Form, the rest on the tab in front. Of two of a name, the ribbon's: the Modify tool
/// under the Modify tab, Delete over the Delete of the element's Properties.
fn tool(h: &mut Harness<BimApp>, label: &str) {
    let lowest = |h: &Harness<BimApp>, label: &str| {
        h.get_all_by_label(label).map(|n| n.rect().center()).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or_else(|| panic!("no {label}"))
    };
    let label = if label == "Point" { "Point Element" } else { label };
    if matches!(label, "Point Element" | "Line") {
        let tab = h.get_all_by_label("Create").map(|n| n.rect().center()).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
        click_at(h, tab);
    }
    if label == "Create Solid Form" {
        let at = lowest(h, "Create Form");
        click_at(h, at);
        h.get_by_label("Solid Form").click();
        h.run();
        return;
    }
    let at = if label == "Modify" {
        lowest(h, label)
    } else {
        h.get_all_by_label(label).map(|n| n.rect().center()).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or_else(|| panic!("no {label}"))
    };
    click_at(h, at);
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
    assert_eq!(h.state().active_family().unwrap().modeller.selection, vec![profile.id.clone()], "the profile drawn is selected");
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
    // Loaded into the project, the project has it, adaptive, with its form.
    press(&mut h, "Load into Project");
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
    assert_eq!(h.state().active_family().unwrap().modeller.selection, vec![id.clone()], "the point made is selected");
    let at = |h: &Harness<BimApp>| {
        let d = definition(h);
        d.evaluate(&Default::default(), &d.preview).origin(&id).unwrap()
    };
    let before = at(&h);
    // The z arrow points up the screen from the point: a drag along it raises the point.
    let (base, tip) = {
        let m = &h.state().active_family().unwrap().modeller;
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
    assert!(matches!(h.state().active_family().unwrap().modeller.tool, Draw::Point));
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

#[test]
fn a_point_selected_shows_revits_adaptive_fields_and_make_adaptive_toggles_it() {
    let mut h = modelling();
    four_points_and_a_profile(&mut h);
    tool(&mut h, "Modify");
    for (k, mods) in [(0, Modifiers::NONE), (1, Modifiers::CTRL), (2, Modifiers::CTRL), (3, Modifiers::CTRL)] {
        click_model(&mut h, CORNERS[k], mods);
    }
    tool(&mut h, "Make Adaptive");
    // Point 1 alone: Properties shows what Revit's palette does, with Revit's defaults.
    click_model(&mut h, CORNERS[0], Modifiers::NONE);
    assert!(h.query_by_label("Adaptive Component").is_some());
    for (field, value) in [
        ("Point", "Placement Point (Adaptive)"),
        ("Orients to", "Instance (xyz)"),
        ("Show Placement Number", "Always"),
        ("Show Reference Planes", "Always"),
    ] {
        assert_eq!(combo_value(&h, field), value, "Properties shows {field}");
    }
    // Make Adaptive again on an adaptive point: it is a reference point again, and the others close up.
    tool(&mut h, "Make Adaptive");
    let d = definition(&h);
    let first = &d.nodes[0].id;
    assert_eq!(d.number(first), None);
    assert_eq!(d.next_number(), 4, "three placement points are left, numbered 1 to 3");
    assert_eq!(combo_value(&h, "Point"), "Reference Point");
    // And again: adaptive, numbered next.
    tool(&mut h, "Make Adaptive");
    assert_eq!(definition(&h).number(first), Some(4));
}

#[test]
fn an_adaptive_family_is_a_document_with_its_own_ribbon_properties_and_browser_as_in_revit() {
    let mut h = modelling();
    // Its own ribbon, in place of the project's.
    for label in ["Create", "Point Element", "Family Types", "Load into Project", "Load into Project and Close", "Close Family"] {
        assert!(h.query_by_label(label).is_some(), "the family document has {label}");
    }
    assert!(h.query_by_label("Architecture").is_none(), "the project's ribbon is away");
    // The Project Browser lists the family's views.
    assert!(h.query_all_by_label("Ref. Level").count() >= 2, "a floor plan and a ceiling plan");
    assert!(h.query_by_label("{3D}").is_some());
    // A tool in hand, then a point selected: the contextual tab names it, as Revit's.
    tool(&mut h, "Point");
    assert!(h.query_all_by_label("Modify | Place Reference Point").count() >= 1, "the tab, and the Options Bar says it too");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    tool(&mut h, "Modify");
    assert!(h.query_all_by_label("Modify | Reference Points").count() >= 1);
    assert!(h.query_by_label("Make Adaptive").is_some());
    // Family Types opens its dialog of parameters and types; OK closes it.
    tool(&mut h, "Family Types");
    assert!(matches!(h.state().active_family().and_then(|e| e.dialog), Some(crate::editor::DocDialog::Types)));
    press(&mut h, "OK");
    assert!(h.state().active_family().is_some_and(|e| e.dialog.is_none()));
    // Load into Project and Close: the project has the family, and its document is gone.
    press(&mut h, "Load into Project and Close");
    assert!(h.state().active_family().is_none());
    assert!(h.state().session.model.families.values().any(|f| f.adaptive.is_some()));
    assert!(h.query_by_label("Architecture").is_some(), "the project's ribbon is back");
}

#[test]
fn edit_family_on_a_deformed_instance_shows_the_family_straight_until_it_is_replayed() {
    let mut h = app("");
    h.set_size(vec2(1800., 1000.));
    h.state_mut()
        .session
        .execute("family", &json!({"name":"Adaptive panel","types":{"A":{}},"adaptive":bimcraft_core::adaptive::four_point_panel()}))
        .unwrap();
    // Point 3 raised 260 mm in this instance: the instance leans.
    let raised = json!({"family":"Adaptive panel","at":[0,0],"adaptive":{"frames":{"3":{"origin":[1200,900,500]}}}});
    let id = serde_json::from_value(h.state_mut().session.execute("component", &raised).unwrap()["id"].clone()).unwrap();
    h.state_mut().selection = vec![id];
    h.state_mut().edit_family("Adaptive panel");
    h.run();
    let z_of_p3 = |h: &Harness<BimApp>| {
        let ed = h.state().active_family().unwrap();
        let d = ed.adaptive.as_ref().unwrap();
        match d.evaluate(&ed.values(), &d.preview).values.get("p3") {
            Some(Evaluated::Point { frame }) => frame.z,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(z_of_p3(&h), [0., 0., 1.], "as Revit's: the family as defined, its points on its axes");
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.modeller.selection = vec!["p3".into()];
    }
    h.run();
    assert!(h.query_by_label("Rotation X").is_some(), "Properties says how the point is turned");
    // Replayed (on the Create tab), it is the instance as it is: point 3 raised, its axes leaning with the instance's.
    press(&mut h, "Create");
    press(&mut h, "Replay Instance");
    let z = z_of_p3(&h);
    assert!(z[2] < 0.999 && z[2] > 0.9, "{z:?}");
}

#[test]
fn a_point_drawn_on_the_placement_plane_chosen_lies_on_it_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    // Ref. Level to begin with, as Revit's template; Center (Front/Back) chosen in the Options Bar.
    assert_eq!(combo_value(&h, "Placement Plane"), "Level : Ref. Level");
    choose(&mut h, "Placement Plane", "Reference Plane : Center (Front/Back)");
    click_model(&mut h, [800., 0., 600.], Modifiers::NONE);
    let d = definition(&h);
    let id = d.nodes.last().map(|n| n.id.clone()).expect("a point made");
    let at = d.evaluate(&Default::default(), &d.preview).origin(&id).expect("it evaluates");
    assert!(at[1].abs() < 1e-6 && (at[0] - 800.).abs() < 20. && (at[2] - 600.).abs() < 20., "on the plane y = 0: {at:?}");
    assert_eq!(
        d.work_plane(&id),
        Some(bimcraft_core::adaptive::WorkPlane::Plane(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK)))
    );
    // Properties says so, as Revit's Work Plane.
    tool(&mut h, "Modify");
    assert!(h.query_by_label("Work Plane").is_some());
    assert!(h.query_all_by_label("Reference Plane : Center (Front/Back)").count() >= 1);
}

#[test]
fn placement_plane_lists_the_familys_own_named_planes_and_a_point_drawn_on_one_lies_on_it() {
    let mut h = modelling();
    // A reference plane of the family's own, 400 mm in front of Center (Front/Back).
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.planes.push(
            serde_json::from_value(serde_json::json!({"guid": "1b2c3d4e-5f60-4a71-8b92-a3b4c5d6e7f8", "name": "Face",
                "start": [-1000, -400, 0], "end": [1000, -400, 0], "up": [0, 0, 1]}))
            .expect("a plane"),
        );
    }
    h.run();
    tool(&mut h, "Point");
    choose(&mut h, "Placement Plane", "Reference Plane : Face");
    click_model(&mut h, [800., -400., 600.], Modifiers::NONE);
    let d = definition(&h);
    let id = d.nodes.last().map(|n| n.id.clone()).expect("a point made");
    let at = d.evaluate(&Default::default(), &d.preview).origin(&id).expect("it evaluates");
    assert!((at[1] + 400.).abs() < 1e-6 && (at[0] - 800.).abs() < 20. && (at[2] - 600.).abs() < 20., "on the plane y = -400: {at:?}");
    let face = bimcraft_core::family::PlaneRef::Plane(bimcraft_core::ids::Guid::parse("1b2c3d4e-5f60-4a71-8b92-a3b4c5d6e7f8").unwrap());
    assert_eq!(d.work_plane(&id), Some(bimcraft_core::adaptive::WorkPlane::Plane(face)));
    tool(&mut h, "Modify");
    assert!(h.query_all_by_label("Reference Plane : Face").count() >= 1, "Properties names it as its Work Plane");
}

#[test]
fn pick_in_placement_plane_takes_the_plane_clicked_in_the_view_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    // Pick, first in the list: the next click picks a plane rather than placing a point.
    choose(&mut h, "Placement Plane", "Pick…");
    assert!(h.state().active_family().unwrap().modeller.picking_plane);
    click_model(&mut h, [800., 0., 600.], Modifiers::NONE);
    let ed = h.state().active_family().unwrap();
    assert!(!ed.modeller.picking_plane, "picked");
    assert_eq!(
        ed.modeller.work_plane,
        bimcraft_core::adaptive::WorkPlane::Plane(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK))
    );
    assert!(definition(&h).nodes.is_empty(), "a pick places nothing");
    assert_eq!(combo_value(&h, "Placement Plane"), "Reference Plane : Center (Front/Back)");
}

#[test]
fn a_point_clicked_near_another_snaps_onto_it_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    let parent = definition(&h).nodes[0].id.clone();
    // Drawn on the parent's own XY plane, clicked a few pixels off it: on it exactly, no offset.
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.modeller.work_plane =
            bimcraft_core::adaptive::WorkPlane::Point { point: parent.clone(), plane: bimcraft_core::adaptive::Plane::Xy };
    }
    h.run();
    let near = h.state().active_family().unwrap().modeller.screen([500., 500., 0.]) + vec2(4.0, -3.0);
    click_at(&mut h, near);
    let d = definition(&h);
    let child = d.nodes.last().map(|n| n.id.clone()).unwrap();
    assert_ne!(child, parent, "a new point");
    let r = d.evaluate(&Default::default(), &d.preview);
    assert_eq!(r.origin(&child), r.origin(&parent), "snapped onto its parent");
    assert!(
        matches!(&d.nodes.last().unwrap().operation, Operation::Point { host: Host::Plane { point, .. }, transform } if *point == parent
            && transform.offset.iter().all(|o| *o == bimcraft_core::family::Expr::Number(0.))),
        "hosted on its plane at no offset: {:?}",
        d.nodes.last()
    );
}

#[test]
fn a_box_dragged_selects_what_is_inside_it_or_what_it_touches_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    click_model(&mut h, [0., 0., 0.], Modifiers::NONE);
    click_model(&mut h, [2000., 0., 0.], Modifiers::NONE);
    tool(&mut h, "Modify");
    let d = definition(&h);
    let (a, b) = (d.nodes[0].id.clone(), d.nodes[1].id.clone());
    let screen = |h: &Harness<BimApp>, p: [f64; 3]| h.state().active_family().unwrap().modeller.screen(p);
    let (pa, pb) = (screen(&h, [0., 0., 0.]), screen(&h, [2000., 0., 0.]));
    let (lo, hi) = (pa.min(pb) - vec2(30., 30.), pa.max(pb) + vec2(30., 30.));
    // Left to right round both: both.
    drag_screen(&mut h, lo, hi, PointerButton::Primary);
    let mut selected = h.state().active_family().unwrap().modeller.selection.clone();
    selected.sort();
    let mut both = vec![a.clone(), b.clone()];
    both.sort();
    assert_eq!(selected, both, "a window takes what is inside it");
    // Right to left round the second only: the second.
    drag_screen(&mut h, pb + vec2(20., 20.), pb - vec2(20., 20.), PointerButton::Primary);
    assert_eq!(h.state().active_family().unwrap().modeller.selection, vec![b.clone()], "a crossing takes what it touches");
    // A drag on empty space selects nothing and leaves the view where it was: it no longer orbits.
    let camera = h.state().active_family().unwrap().modeller.camera.map(|c| (c.azimuth, c.elevation));
    drag_screen(&mut h, pa + vec2(300., 200.), pa + vec2(360., 240.), PointerButton::Primary);
    assert!(h.state().active_family().unwrap().modeller.selection.is_empty());
    assert_eq!(h.state().active_family().unwrap().modeller.camera.map(|c| (c.azimuth, c.elevation)), camera);
}

#[test]
fn the_viewcube_looks_from_the_face_clicked_and_its_house_goes_home_as_in_revit() {
    let mut h = modelling();
    let (cam, rect) = {
        let m = &h.state().active_family().unwrap().modeller;
        (m.camera.expect("a camera"), m.rect)
    };
    let faces = crate::orbit::Orbit::cube_faces(&cam, rect);
    let front = faces.iter().find(|(_, label, ..)| *label == "FRONT").expect("the front face shows");
    let centre = front.0.iter().fold(Pos2::ZERO, |a, q| a + q.to_vec2() / 4.0);
    click_at(&mut h, centre);
    let now = h.state().active_family().unwrap().modeller.camera.unwrap();
    assert_eq!((now.azimuth, now.elevation), (-90., 0.), "seen from the front");
    click_at(&mut h, crate::orbit::Orbit::home_rect(rect).center());
    let home = h.state().active_family().unwrap().modeller.camera.unwrap();
    assert_eq!((home.azimuth, home.elevation), (bimcraft_core::view3d::AZIMUTH, bimcraft_core::view3d::ELEVATION), "home again");
    assert!(definition(&h).nodes.is_empty(), "the cube's clicks select and place nothing");
}

#[test]
fn move_copy_and_rotate_take_the_selection_from_two_clicks_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Line");
    click_model(&mut h, [0., 0., 0.], Modifiers::NONE);
    click_model(&mut h, [1000., 0., 0.], Modifiers::NONE);
    h.key_press(Key::Enter);
    h.run();
    tool(&mut h, "Modify");
    let d = definition(&h);
    let line = d.nodes.iter().find(|n| matches!(n.operation, Operation::Line { .. })).map(|n| n.id.clone()).expect("a line");
    let (a, b) = match &d.nodes.iter().find(|n| n.id == line).unwrap().operation {
        Operation::Line { a, b } => (a.clone(), b.clone()),
        other => panic!("{other:?}"),
    };
    let at = |h: &Harness<BimApp>, id: &str| {
        let d = definition(h);
        d.evaluate(&Default::default(), &d.preview).origin(id).unwrap()
    };
    let close = |p: [f64; 3], q: [f64; 3]| (0..3).all(|i| (p[i] - q[i]).abs() < 20.);
    // Move: the line selected, from its first point (snapped) to 500 mm north of it.
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.modeller.selection = vec![line.clone()];
    }
    h.run();
    tool(&mut h, "Move");
    click_model(&mut h, [0., 0., 0.], Modifiers::NONE);
    click_model(&mut h, [0., 500., 0.], Modifiers::NONE);
    assert!(close(at(&h, &a), [0., 500., 0.]) && close(at(&h, &b), [1000., 500., 0.]), "moved: {:?} {:?}", at(&h, &a), at(&h, &b));
    assert_eq!(h.state().active_family().unwrap().modeller.tool, Draw::Modify, "back to Modify after one move");
    // Copy: the line again, 500 mm further; the original stays, the copy is selected.
    let count = definition(&h).nodes.len();
    tool(&mut h, "Copy");
    click_model(&mut h, [0., 500., 0.], Modifiers::NONE);
    click_model(&mut h, [0., 1000., 0.], Modifiers::NONE);
    assert_eq!(definition(&h).nodes.len(), count + 3, "a line and its two points copied");
    assert!(close(at(&h, &a), [0., 500., 0.]), "the original stays");
    // Rotate the original a quarter turn about its middle: its ends swap round it.
    if let Some(ed) = h.state_mut().active_family_mut() {
        ed.modeller.selection = vec![line.clone()];
    }
    h.run();
    tool(&mut h, "Rotate");
    click_model(&mut h, [1000., 500., 0.], Modifiers::NONE);
    click_model(&mut h, [500., 1000., 0.], Modifiers::NONE);
    assert!(close(at(&h, &b), [500., 1000., 0.]) && close(at(&h, &a), [500., 0., 0.]), "turned: {:?} {:?}", at(&h, &a), at(&h, &b));
}

/// The value of `name` in the type the family document previews.
fn parameter(h: &Harness<BimApp>, name: &str) -> Option<f64> {
    match h.state().active_family()?.values().get(name)? {
        bimcraft_core::family::Val::Number(x) => Some(*x),
        bimcraft_core::family::Val::Text(_) => None,
    }
}

/// How high the only point of the family stands, as the type it previews has it.
fn height(h: &Harness<BimApp>) -> f64 {
    let ed = h.state().active_family().unwrap();
    let d = definition(h);
    let r = d.evaluate(&ed.values(), &d.preview);
    r.origin(&d.nodes[0].id).map(|p| p[2]).unwrap()
}

/// The "=" of the row of Properties named `row`, scrolled to first, as a user scrolls the palette to it.
fn equals(h: &mut Harness<BimApp>, row: &str) {
    let label = format!("Associate Family Parameter: {row}");
    h.get_by_label(&label).scroll_to_me();
    h.run();
    press(h, &label);
}

#[test]
fn the_equals_button_ties_each_offset_and_rotation_of_a_point_to_a_family_parameter_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    tool(&mut h, "Modify");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    // One row for each offset and rotation, no Offset or Rotation Angle beside them.
    assert!(h.query_by_label("Associate Family Parameter: Offset").is_none());
    assert!(h.query_by_label("Associate Family Parameter: Rotation Angle").is_none());
    // OffsetZ, typed: the point rises off its work plane.
    type_role(&mut h, Role::TextInput, "OffsetZ (mm)", "250");
    assert!((height(&h) - 250.).abs() < 1e-6, "typed, the offset lifts the point: {}", height(&h));
    // "=" opens Associate Family Parameter; New Parameter makes a length named Height, a type's, of the value the field had.
    equals(&mut h, "OffsetZ (mm)");
    assert!(h.query_by_label("Associate Family Parameter").is_some());
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Panel Offset");
    press(&mut h, "OK");
    assert_eq!(parameter(&h, "Panel Offset"), Some(250.), "the new parameter takes the value the field had");
    press(&mut h, "OK");
    assert!(h.query_by_label("Associate Family Parameter").is_none(), "OK closes it");
    let tied = |h: &Harness<BimApp>| match &definition(h).nodes[0].operation {
        Operation::Point { transform, .. } => transform.offset[2].clone(),
        _ => panic!("a point"),
    };
    assert_eq!(tied(&h), bimcraft_core::family::Expr::Formula("Panel Offset".into()));
    // The type's value changed in Family Types moves the point.
    tool(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value Panel Offset", "800");
    press(&mut h, "OK");
    assert!((height(&h) - 800.).abs() < 1e-6, "the parameter drives the offset: {}", height(&h));
    // A rotation takes angles only: Panel Offset is not offered.
    equals(&mut h, "RotZ (degrees)");
    assert!(h.query_by_label("Panel Offset").is_none(), "a length is not offered for an angle");
    press(&mut h, "Cancel");
    // <None> unties it, keeping the value it had.
    equals(&mut h, "OffsetZ (mm)");
    press(&mut h, "<None>");
    press(&mut h, "OK");
    assert_eq!(tied(&h), bimcraft_core::family::Expr::Number(800.));
}

#[test]
fn pick_takes_a_points_plane_even_with_a_reference_plane_in_front_of_it_as_in_revit() {
    let mut h = modelling();
    tool(&mut h, "Point");
    click_model(&mut h, [500., 500., 0.], Modifiers::NONE);
    let point = definition(&h).nodes[0].id.clone();
    // Center (Front/Back) lies between the eye and the point: the point's own planes, drawn around it, are what is picked there.
    for off in [vec2(5., 3.), vec2(-6., 2.), vec2(3., -6.)] {
        choose(&mut h, "Placement Plane", "Pick…");
        let at = h.state().active_family().unwrap().modeller.screen([500., 500., 0.]) + off;
        click_at(&mut h, at);
        let ed = h.state().active_family().unwrap();
        assert!(!ed.modeller.picking_plane, "picked");
        assert!(
            matches!(&ed.modeller.work_plane, bimcraft_core::adaptive::WorkPlane::Point { point: p, .. } if *p == point),
            "one of the point's planes, {off:?}: {:?}",
            ed.modeller.work_plane
        );
    }
    // Away from any point, a reference plane is still picked.
    choose(&mut h, "Placement Plane", "Pick…");
    click_model(&mut h, [800., 0., 600.], Modifiers::NONE);
    let ed = h.state().active_family().unwrap();
    assert_eq!(
        ed.modeller.work_plane,
        bimcraft_core::adaptive::WorkPlane::Plane(bimcraft_core::family::PlaneRef::Plane(bimcraft_core::family::CENTER_FRONT_BACK))
    );
}
