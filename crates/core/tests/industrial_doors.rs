//! Industrial gates through the same commands used by the CLI and MCP.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Model};
    use bimcraft_core::render::{self, Prim, View};
    use serde_json::{Value, json};

    fn gate(operation: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0,0],[12000,0]], "height": 4500})).unwrap();
        s.execute("type", &json!({"category":"door", "name":"Gate", "width":3000, "height":3200, "operation":operation})).unwrap();
        let door = s.execute("door", &json!({"at":[3500,0], "type":"Gate"})).unwrap()["id"].as_u64().unwrap();
        (s, door)
    }

    #[test]
    fn sectional_and_roller_gates_save_reopen_and_keep_operations_and_guids() {
        for (operation, label, ifc) in [("sectional", "Sectional", ".USERDEFINED."), ("roller", "Roller", ".ROLLINGUP.")] {
            let (mut s, door) = gate(operation);
            let schedule = s.execute("schedule", &json!({"category":"doors", "fields":["Operation", "Width", "Height"]})).unwrap();
            assert_eq!(schedule["rows"], json!([[label, 3000.0, 3200.0]]));
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gate.bimcraft");
            s.execute("save", &json!({"path":path})).unwrap();
            let mut back = Session::new();
            let opened = back.execute("open", &json!({"path":path})).unwrap();
            assert!(opened["left_out"].is_null() || opened["left_out"] == json!([]));
            assert_eq!(back.model.elements.guid(door), s.model.elements.guid(door));
            assert_eq!(back.model.library, s.model.library);
            assert_eq!(back.model.types, s.model.types);
            let path = dir.path().join("gate.ifc");
            back.execute("save", &json!({"path":path})).unwrap();
            let step = std::fs::read_to_string(&path).unwrap();
            for class in ["=IFCDOOR(", "=IFCDOORTYPE("] {
                assert!(step.lines().any(|line| line.contains(class) && line.contains(ifc)), "{class}: {operation}");
            }
            if operation == "sectional" {
                assert_eq!(
                    step.lines().filter(|l| (l.contains("=IFCDOOR(") || l.contains("=IFCDOORTYPE(")) && l.contains("'SECTIONAL'")).count(),
                    2
                );
            }
            let guid = s.model.elements.guid(door).unwrap();
            back.execute("open", &json!({"path":path})).unwrap();
            let imported = back.model.elements.get(guid).unwrap();
            assert!(matches!(imported, Element::Door(_)));
            assert_eq!(back.model.opening_type(imported).unwrap().operation.unwrap().name(), operation);
            assert_eq!(back.model.opening_type(imported).unwrap().width, 3000.0);
            assert_eq!(back.model.opening_type(imported).unwrap().height, 3200.0);
        }
    }

    #[test]
    fn vertical_gates_have_a_closed_panel_in_plan_without_a_horizontal_swing() {
        for operation in ["sectional", "roller"] {
            let (mut s, door) = gate(operation);
            let level = s.model.levels()[0].0;
            let view = View { scale: 0.08, cx: 6000.0, cy: 0.0, width: 1100, height: 650 };
            for (hand, facing) in [(false, false), (true, false), (false, true), (true, true)] {
                s.execute("modify", &json!({"id":door,"flip_hand":hand,"flip_facing":facing})).unwrap();
                let drawing = render::draw(&s.model, level, view);
                let lines: Vec<_> = drawing
                    .prims
                    .iter()
                    .zip(&drawing.owners)
                    .filter_map(|(p, id)| {
                        if *id == Some(door)
                            && let Prim::Line { pts, dash, .. } = p
                        {
                            return Some((pts, dash));
                        }
                        None
                    })
                    .collect();
                assert!(lines.len() >= 3, "jambs and panel must be drawn");
                assert!(lines.iter().any(|(_, dash)| dash.is_some()), "tracks or coil box are dashed overhead");
                for (points, dash) in lines {
                    assert!(points.len() < 10, "a vertical gate must have no swing arc");
                    if dash.is_none() {
                        for point in points {
                            assert!(view.mm(*point).1.abs() <= 181.0, "cut gate parts stay at the wall face: {:?}", view.mm(*point));
                        }
                    }
                }
                assert!(!render::png(&drawing).unwrap().is_empty());
                let scene = bimcraft_core::view3d::Scene::new(&s.model);
                let camera = scene.fit(-90.0, 15.0, 500, 400);
                let picture = scene.render(&camera, &[]);
                assert!(
                    (0..picture.height).any(|y| (0..picture.width).any(|x| picture.pick(f64::from(x), f64::from(y)) == Some(door))),
                    "the closed gate must remain visible and pickable in 3D"
                );
            }
        }
    }

    #[test]
    fn gate_type_edits_keep_host_validation_and_rollback() {
        let (mut s, door) = gate("sectional");
        let original = s.model.elements[&door].clone();
        let before = (*s.model).clone();
        let error = s.execute("type", &json!({"category":"door","name":"Gate","width":13000,"operation":"roller"})).unwrap_err();
        assert!(error.contains("wall"), "{error}");
        assert_eq!(*s.model, before);
        s.execute("type", &json!({"category":"door","name":"Gate","operation":"roller"})).unwrap();
        assert_eq!(s.model.elements[&door], original);
        assert_eq!(s.model.types.door["Gate"].operation.unwrap().name(), "roller");
        let before = (*s.model).clone();
        assert!(s.execute("type", &json!({"category":"door","name":"Gate","operation":"spinning"})).is_err());
        assert_eq!(*s.model, before);
        assert!(s.execute("door", &json!({"at":[3500,0],"type":"Gate"})).is_err(), "overlapping gates remain invalid");
        assert_eq!(*s.model, before);
    }

    #[test]
    fn old_files_keep_unspecified_swing_and_newer_files_are_refused() {
        let mut s = Session::new();
        s.execute("type", &json!({"category":"door","name":"Legacy","width":900,"height":2100})).unwrap();
        let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        file["format"] = json!(17);
        let (back, left_out) = Model::read(&file.to_string()).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(back.types.door["Legacy"].operation, None);
        assert_eq!(back.library, s.model.library);
        assert_eq!(back.elements, s.model.elements);
        file["format"] = json!(bimcraft_core::model::FORMAT + 1);
        assert!(Model::read(&file.to_string()).unwrap_err().contains("newer BIMCraft"));
    }

    #[test]
    fn family_operations_keep_authored_geometry_and_import_from_ifc_type_fallback() {
        for (operation, ifc) in [("sectional", ".USERDEFINED."), ("roller", ".ROLLINGUP.")] {
            let mut s = Session::new();
            let original = s.model.families["Single Door"].clone();
            s.execute("family", &json!({"name":"Single Door","operation":operation})).unwrap();
            let family = &s.model.families["Single Door"];
            assert_eq!(family.forms, original.forms);
            assert_eq!(family.symbols, original.symbols);
            s.script("wall {\"points\":[[0,0],[8000,0]]}\ndoor {\"at\":[2000,0],\"family\":\"Single Door\"}").unwrap();
            let step = bimcraft_core::ifc::export(&s.model);
            let user = if operation == "sectional" { "'SECTIONAL'" } else { "$" };
            let typed = step.replacen(&format!("{ifc},{user})"), "$,$)", 1);
            assert_ne!(step, typed, "remove occurrence operation so the imported type supplies it");
            let back = bimcraft_core::import::ifc(typed.as_bytes(), "Gate").unwrap();
            let door = back.model.elements.values().find(|e| matches!(e, Element::Door(_))).unwrap();
            assert_eq!(back.model.opening_type(door).unwrap().operation.unwrap().name(), operation);
        }
    }

    #[test]
    fn existing_operations_and_handedness_keep_their_ifc_meaning() {
        for (operation, left, right) in [
            ("swing", ".SINGLE_SWING_LEFT.", ".SINGLE_SWING_RIGHT."),
            ("double-swing", ".DOUBLE_DOOR_SINGLE_SWING.", ".DOUBLE_DOOR_SINGLE_SWING."),
            ("sliding", ".SLIDING_TO_LEFT.", ".SLIDING_TO_RIGHT."),
            ("double-sliding", ".DOUBLE_DOOR_SLIDING.", ".DOUBLE_DOOR_SLIDING."),
            ("folding", ".FOLDING_TO_LEFT.", ".FOLDING_TO_RIGHT."),
            ("fixed", ".NOTDEFINED.", ".NOTDEFINED."),
        ] {
            let (mut s, door) = gate(operation);
            for (hand, expected) in [(false, left), (true, right)] {
                s.execute("modify", &json!({"id":door,"flip_hand":hand})).unwrap();
                let step = bimcraft_core::ifc::export(&s.model);
                assert!(step.lines().any(|l| l.contains("=IFCDOOR(") && l.contains(expected)), "{operation}: {expected}");
            }
        }
    }

    #[test]
    fn industrial_parts_have_positive_volume_and_fit_in_plan_with_overhead_equipment() {
        for (operation, count, reach, top) in [("sectional", 9, 3380.0, 3300.0), ("roller", 4, 550.0, 3650.0)] {
            let (mut s, door) = gate(operation);
            for facing in [false, true] {
                s.execute("modify", &json!({"id":door,"flip_facing":facing})).unwrap();
                let pieces = s.model.opening_pieces(&s.model.elements[&door]);
                assert_eq!(pieces.len(), count);
                let pts: Vec<_> = pieces.iter().flat_map(|p| p.tris.iter().flatten()).collect();
                let highest = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
                let furthest = pts.iter().map(|p| p[1].abs()).fold(0.0, f64::max);
                assert!((highest - top).abs() < 1e-8 && (furthest - reach).abs() < 1e-8);
                for piece in pieces {
                    let volume = piece
                        .tris
                        .iter()
                        .map(|[a, b, c]| {
                            a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])
                        })
                        .sum::<f64>()
                        / 6.0;
                    assert!(volume > 0.0, "mirrored parts must keep outward faces");
                }
                let drawing = render::plan(&s.model, s.model.levels()[0].0, 900, 400);
                for (p, owner) in drawing.prims.iter().zip(&drawing.owners) {
                    if *owner == Some(door) {
                        for (x, y) in p.points() {
                            assert!((0.0..900.0).contains(&x) && (0.0..400.0).contains(&y), "Zoom to Fit clipped the gate: {x},{y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn flipped_industrial_gates_reopen_on_the_same_side_of_the_wall_in_ifc() {
        for operation in ["sectional", "roller"] {
            let (mut s, door) = gate(operation);
            s.execute("modify", &json!({"id":door,"flip_facing":true,"flip_hand":true})).unwrap();
            let step = bimcraft_core::ifc::export(&s.model);
            let back = bimcraft_core::import::ifc(step.as_bytes(), "Gate").unwrap();
            let original_guid = s.model.elements.guid(door).unwrap();
            let imported = back.model.elements.get(original_guid).unwrap();
            assert!(matches!(imported, Element::Door(o) if o.flip_facing));
            let before = s.model.opening_pieces(&s.model.elements[&door]);
            let after = back.model.opening_pieces(imported);
            for k in 0..3 {
                let extent = |pieces: &[bimcraft_core::family::Piece]| {
                    pieces
                        .iter()
                        .flat_map(|p| p.tris.iter().flatten())
                        .map(|p| p[k])
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
                };
                let (a, b) = (extent(&before), extent(&after));
                assert!((a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01, "{operation}, coordinate {k}: {a:?} vs {b:?}");
            }
        }
    }
}
