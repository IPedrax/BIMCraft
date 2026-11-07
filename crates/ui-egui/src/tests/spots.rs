//! Spot coordinates in the window, driven as a user drives them: Annotate > Spot Coordinate places one by its point and its
//! leader's end, Edit Type opens Spot Coordinate Type Properties, the leader's end is a grip; and Manage > Acquire
//! Coordinates takes the shared coordinates from a CAD drawing clicked.

use bimcraft_core::spots::{Arrowhead, DEFAULT};

use super::*;

fn spot(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::Spot {
    let Some(Element::Spot(sp)) = h.state().session.model.elements.get(id) else { panic!("{id} isn't a spot") };
    sp.clone()
}

#[test]
fn spot_coordinate_is_placed_by_its_point_and_its_leader_s_end_both_snapped() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Spot Coordinate");
    assert_eq!(h.state().tool, Tool::SpotCoordinate(None));
    // Near the corner: on it.
    click(&mut h, P2(8030.0, 6020.0));
    assert_eq!(h.state().tool, Tool::SpotCoordinate(Some(P2(8000.0, 6000.0))), "the point snaps to the corner");
    assert!(ids(&h, "spotcoordinate").is_empty(), "nothing is placed until its leader ends");
    click(&mut h, P2(10_000.0, 8000.0));
    let placed = ids(&h, "spotcoordinate");
    assert_eq!(placed.len(), 1);
    let sp = spot(&h, placed[0]);
    assert_eq!(sp.at, P2(8000.0, 6000.0));
    assert!(sp.end.is_some_and(|e| e.dist(P2(10_000.0, 8000.0)) < 300.0), "{:?}", sp.end);
    assert_eq!(sp.type_name.as_deref(), Some(DEFAULT));
    assert_eq!(field(&h, placed[0], "text"), json!(["N 6000", "E 8000"]));
    assert_eq!(h.state().tool, Tool::SpotCoordinate(None), "ready for the next");
}

#[test]
fn edit_type_on_a_spot_coordinate_opens_its_type_properties_with_every_arrowhead() {
    let mut h = app(&format!("{BOX}\nspot_coordinate {{\"at\": [8000, 6000], \"end\": [9000, 7000]}}"));
    let id = ids(&h, "spotcoordinate")[0];
    h.state_mut().selection = vec![id];
    h.run();
    assert!(h.query_by_label("Has Shoulder").is_some(), "Properties shows its leader and shoulder");
    press(&mut h, "Edit Type");
    assert!(h.state().spot_type_dialog.is_some());
    for label in ["Leader Arrowhead", "Arrowhead Size", "Leader Line Weight", "Leader Color", "Text Color", "Coordinate Origin"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Site Dots");
    press(&mut h, "Create");
    assert!(h.state().spot_type_dialog.as_ref().is_some_and(|d| d.error().is_none()));
    if let Some(d) = h.state_mut().spot_type_dialog.as_mut() {
        d.typed().leader_arrowhead = Arrowhead::EmptyDot;
        d.typed().arrowhead_size = 4.0;
        d.typed().leader_color = [200, 0, 0];
    }
    press(&mut h, "OK");
    assert!(h.state().spot_type_dialog.is_none(), "{}", h.state().status.text);
    let t = &h.state().session.model.spot_coordinate_types["Site Dots"];
    assert_eq!((t.leader_arrowhead, t.arrowhead_size, t.leader_color), (Arrowhead::EmptyDot, 4.0, [200, 0, 0]));
    assert_eq!(spot(&h, id).type_name.as_deref(), Some("Site Dots"), "OK puts it on the type shown");
}

#[test]
fn a_spot_coordinate_s_leader_end_is_a_grip() {
    let mut h = app(&format!("{BOX}\nspot_coordinate {{\"at\": [4000, 3000], \"end\": [5000, 4000]}}"));
    let id = ids(&h, "spotcoordinate")[0];
    h.state_mut().selection = vec![id];
    h.run();
    drag(&mut h, P2(5000.0, 4000.0), P2(2000.0, 4500.0));
    let sp = spot(&h, id);
    assert_eq!(sp.at, P2(4000.0, 3000.0), "the point stays");
    assert!(sp.end.is_some_and(|e| e.dist(P2(2000.0, 4500.0)) < 300.0), "{:?}", sp.end);
}

/// A DXF of a square 4 m across in metres, its south-west corner at map coordinates (500 000, 7 000 000).
fn far_drawing() -> String {
    let pairs = |list: &[(i32, &str)]| list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>();
    let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)];
    let lines: String = corners
        .windows(2)
        .map(|w| {
            let (a, b) = ((500_000.0 + w[0].0, 7_000_000.0 + w[0].1), (500_000.0 + w[1].0, 7_000_000.0 + w[1].1));
            pairs(&[(0, "LINE"), (8, "0"), (10, &a.0.to_string()), (20, &a.1.to_string()), (11, &b.0.to_string()), (21, &b.1.to_string())])
        })
        .collect();
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    let text = format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{lines}0\nENDSEC\n0\nEOF\n");
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-acquire-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("survey.dxf");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn acquire_coordinates_takes_them_from_the_cad_drawing_clicked() {
    let mut h = app(BOX);
    // The survey's square, its middle on the box's middle.
    h.state_mut().run("import_cad", json!({"path": far_drawing(), "at": [4000, 3000]}));
    h.state_mut().selection.clear();
    h.run();
    press(&mut h, "Manage");
    from_list(&mut h, "Project Location", "Acquire Coordinates");
    assert_eq!(h.state().tool, Tool::Acquire);
    // A click on the square's west edge.
    click(&mut h, P2(2000.0, 3000.0));
    let q = h.state().session.model.coordinates.to_shared([4000.0, 3000.0, 0.0]);
    assert!((q[0] - 500_002_000.0).abs() < 1e-3 && (q[1] - 7_000_002_000.0).abs() < 1e-3, "{q:?}");
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.state().status.text.contains("survey"), "{}", h.state().status.text);
}
