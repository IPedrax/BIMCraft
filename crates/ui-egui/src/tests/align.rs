use super::*;
use bimcraft_core::sketch::SketchLine;

#[test]
fn sketch_align_uses_a_model_edge_keeps_the_loop_closed_and_undoes_without_editing_the_model() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 4000.0));
    let model = Arc::clone(&h.state().session.model);
    let before = h.state().sketch.as_ref().unwrap().lines.clone();
    shortcut(&mut h, "AL");
    assert!(h.state().prompt().contains("alignment reference"));
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().prompt().contains("sketch line to move"));
    click(&mut h, P2(3000.0, 1000.0));
    let sk = h.state().sketch.as_ref().unwrap();
    assert!(sk.lines.iter().any(|l| l.start().1 == 0.0 && l.end().1 == 0.0), "{:?}", sk.lines);
    assert!(bimcraft_core::sketch::solve_one(&sk.lines).is_ok(), "the two joined sides follow the aligned edge");
    assert_eq!(h.state().session.model.elements, model.elements);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().sketch.as_ref().unwrap().lines, before);
    assert_eq!(h.state().session.model.elements, model.elements);
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(3000.0, 1000.0));
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{:?}", h.state().status);
    assert_eq!(field(&h, ids(&h, "floor")[0], "area_m2"), json!(16.0));
}

#[test]
fn sketch_align_turns_a_sketch_line_onto_another_sketch_line() {
    let mut h = app(BOX);
    let reference = SketchLine::new(P2(1000.0, 1000.0), P2(4000.0, 1000.0));
    let target = SketchLine::new(P2(2000.0, 3000.0), P2(4000.0, 5000.0));
    h.state_mut().start_sketch(crate::sketch::Of::Floor(100000), vec![reference.clone(), target.clone()]);
    h.run();
    let model = Arc::clone(&h.state().session.model);
    shortcut(&mut h, "AL");
    click(&mut h, P2(2500.0, 1000.0));
    assert_eq!(h.state().tool, Tool::Align(Some((reference.start(), reference.end()))));
    click(&mut h, P2(3000.0, 4000.0));
    let sk = h.state().sketch.as_ref().unwrap();
    assert_eq!(sk.lines[0], reference);
    let aligned = &sk.lines[1];
    assert!((aligned.start().1 - 1000.0).abs() < 0.002 && (aligned.end().1 - 1000.0).abs() < 0.002);
    assert!((aligned.start().dist(aligned.end()) - target.start().dist(target.end())).abs() < 0.002);
    assert_eq!(h.state().session.model.elements, model.elements);
    assert_eq!(h.state().tool, Tool::Align(None));
    press(&mut h, "Cancel Edit Mode");
    assert_eq!(h.state().session.model.elements, model.elements);
}

#[test]
fn sketch_align_only_edits_the_active_sketch_and_escape_drops_its_reference() {
    let mut h = app(BOX);
    let line = SketchLine::new(P2(1000.0, 1000.0), P2(4000.0, 1000.0));
    h.state_mut().start_sketch(crate::sketch::Of::Floor(100000), vec![line.clone()]);
    h.run();
    let model = Arc::clone(&h.state().session.model);
    shortcut(&mut h, "AL");
    click(&mut h, P2(2500.0, 1000.0));
    assert!(matches!(h.state().tool, Tool::Align(Some(_))));
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().session.model.elements, model.elements);
    assert_eq!(h.state().sketch.as_ref().unwrap().lines, [line]);
    assert!(matches!(h.state().tool, Tool::Align(Some(_))));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Align(None));
    assert!(h.state().sketch.is_some());
    press(&mut h, "Cancel Edit Mode");
    assert_eq!(h.state().session.model.elements, model.elements);
}

#[test]
fn sketch_align_picks_a_straight_segment_of_a_polyline_and_highlights_it() {
    let mut h = app(BOX);
    let polyline: SketchLine = bimcraft_core::curves::Curve::Poly {
        points: vec![P2(1000.0, 1000.0), P2(4000.0, 1000.0), P2(4000.0, 3000.0)],
        arcs: Vec::new(),
        closed: false,
    }
    .into();
    let target = SketchLine::new(P2(1000.0, 4000.0), P2(3000.0, 4000.0));
    h.state_mut().start_sketch(crate::sketch::Of::Floor(100000), vec![polyline.clone(), target]);
    h.run();
    shortcut(&mut h, "AL");
    let a = h.state().screen(P2(4000.0, 1000.0));
    let b = h.state().screen(P2(4000.0, 3000.0));
    let at = h.state().screen(P2(4000.0, 2000.0));
    h.hover_at(at);
    h.run();
    assert!(
        h.output().shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::LineSegment {points, stroke} if stroke.width >= 3.0 && points[0].distance(a)<0.2 && points[1].distance(b)<0.2
        )),
        "the exact polyline segment is highlighted"
    );
    click(&mut h, P2(4000.0, 2000.0));
    click(&mut h, P2(2000.0, 4000.0));
    let sk = h.state().sketch.as_ref().unwrap();
    assert_eq!(sk.lines[0], polyline);
    assert!((sk.lines[1].start().0 - 4000.0).abs() < 0.002 && (sk.lines[1].end().0 - 4000.0).abs() < 0.002);
    assert!((sk.lines[1].start().dist(sk.lines[1].end()) - 2000.0).abs() < 0.002);
}

#[test]
fn sketch_align_keeps_existing_boundary_ids_and_the_sketch_reference_when_an_alignment_is_refused() {
    let mut h = app(&format!(
        "{BOX}\nfloor {{\"sketch\": [[[1000,1000],[5000,1000]],[[5000,1000],[5000,4000]],[[5000,4000],[1000,4000]],[[1000,4000],[1000,1000]]]}}"
    ));
    let floor = ids(&h, "floor")[0];
    h.state_mut().selection = vec![floor];
    h.run();
    press(&mut h, "Modify | Floors");
    press(&mut h, "Edit Boundary");
    let before = h.state().sketch.as_ref().unwrap().lines.clone();
    assert!(before.iter().all(|l| l.id != 0));
    shortcut(&mut h, "AL");
    // The draft boundary coincides with its saved floor: the visible sketch is the reference.
    click(&mut h, P2(3000.0, 1000.0));
    assert!(matches!(h.state().drawing.align_reference, Some(crate::draw::Picked::Sketch(_))));
    click(&mut h, P2(3000.0, 1000.0));
    assert!(h.state().status.error);
    assert_eq!(h.state().sketch.as_ref().unwrap().lines, before);
    assert!(matches!(h.state().tool, Tool::Align(Some(_))));
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(3000.0, 1000.0));
    let after = &h.state().sketch.as_ref().unwrap().lines;
    assert!(
        before.iter().zip(after).all(|(a, b)| (a.id, a.guid, a.style.clone(), a.invisible) == (b.id, b.guid, b.style.clone(), b.invisible))
    );
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{:?}", h.state().status);
    let kept = field(&h, floor, "sketch");
    assert!(before.iter().all(|l| kept.as_array().unwrap().iter().any(|v| v["id"] == l.id)));
}

#[test]
fn align_turns_a_nonparallel_wall_face_onto_the_reference_and_undoes_in_one_step() {
    let mut h = app("wall {\"points\": [[0, 0], [6000, 0]]}\nwall {\"points\": [[1000, 2000], [4000, 4000]]}");
    let before = Arc::clone(&h.state().session.model);
    shortcut(&mut h, "AL");
    click(&mut h, P2(3000.0, -100.0));
    click(&mut h, P2(2500.0, 2900.0));
    assert!(!h.state().status.error, "{:?}", h.state().status);
    let target = walls(&h)[1];
    assert!((target.0.1 - target.1.1).abs() < 0.002, "{target:?}");
    assert!((target.0.1 - 0.0).abs() < 0.002, "the lower face lies on y=-100: {target:?}");
    assert!((target.0.dist(target.1) - P2(1000.0, 2000.0).dist(P2(4000.0, 4000.0))).abs() < 0.002);
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(h.state().session.model.elements, before.elements);
}

#[test]
fn align_picks_column_faces_run_edges_floor_boundaries_and_component_edges() {
    for (script, pick, category) in [
        ("column {\"at\": [3000,2000]}", P2(2850.0, 2000.0), "column"),
        ("beam {\"points\": [[3000,1000],[3000,4000]]}", P2(2850.0, 2500.0), "beam"),
        ("floor {\"boundary\": [[3000,1000],[5000,1000],[5000,4000],[3000,4000]]}", P2(3000.0, 2500.0), "floor"),
        (
            "family {\"name\": \"Box\", \"types\": {\"A\": {}}, \"forms\": [{\"profile\": [[0,0],[1000,0],[1000,500],[0,500]], \"to\": 1000}]}\ncomponent {\"family\": \"Box\", \"at\": [3000,1000]}",
            P2(3000.0, 1250.0),
            "component",
        ),
    ] {
        let mut h = app(&format!("grid {{\"start\": [0,0], \"end\": [0,5000]}}\n{script}"));
        let target = ids(&h, category)[0];
        let before = h.state().session.model.elements[&target].clone();
        shortcut(&mut h, "AL");
        click(&mut h, P2(0.0, 2500.0));
        assert!(matches!(h.state().tool, Tool::Align(Some(_))), "{category}: reference pick");
        click(&mut h, pick);
        assert!(!h.state().status.error, "{category}: {:?}", h.state().status);
        assert_eq!(h.state().tool, Tool::Align(None), "{category}: target pick");
        assert_ne!(h.state().session.model.elements[&target], before, "{category} didn't move");
        assert!(h.state().session.model.check().is_ok());
    }
}

#[test]
fn align_highlights_the_exact_line_under_the_cursor_before_both_picks() {
    fn segment(shape: &egui::Shape, a: Pos2, b: Pos2) -> bool {
        match shape {
            egui::Shape::LineSegment { points, stroke } => {
                stroke.width >= 3.0 && points[0].distance(a) < 0.2 && points[1].distance(b) < 0.2
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| segment(s, a, b)),
            _ => false,
        }
    }
    let mut h = app("wall {\"points\": [[0,0],[6000,0]]}\nwall {\"points\": [[0,2000],[6000,2000]]}");
    shortcut(&mut h, "AL");
    for y in [-100.0, 1900.0] {
        let at = h.state().screen(P2(3000.0, y));
        h.hover_at(at);
        h.run();
        let (a, b) = (h.state().screen(P2(0.0, y)), h.state().screen(P2(6000.0, y)));
        assert!(h.output().shapes.iter().any(|s| segment(&s.shape, a, b)), "no line highlight at y={y}");
        if y < 0.0 {
            click(&mut h, P2(3000.0, y));
        }
    }
}
