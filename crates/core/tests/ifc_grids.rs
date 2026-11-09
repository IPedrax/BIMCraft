//! Grid exchange through the same commands as the CLI: save, inspect the STEP output and reopen.

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use bimcraft_core::engine::Session;
    use bimcraft_core::step::{self, V};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn grids(s: &mut Session) -> Vec<Value> {
        run(s, "elements", json!({"category": "grid"}))["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| json!([g["name"], g["start"], g["end"]]))
            .collect()
    }

    /// Inspect the exported entity graph independently of BIMCraft's importer.
    fn axis_line(file: &step::File, axis: &V) -> Value {
        let V::Ref(axis) = axis else { panic!("axis reference required") };
        let axis = &file.entities[axis];
        assert_eq!(axis.name, "IFCGRIDAXIS");
        assert_eq!(axis.args[2], V::Enum("T".into()));
        let V::Ref(curve) = &axis.args[1] else { panic!("curve reference required") };
        let curve = &file.entities[curve];
        assert_eq!(curve.name, "IFCPOLYLINE");
        let V::List(points) = &curve.args[0] else { panic!("polyline points required") };
        let coords: Vec<Value> = points
            .iter()
            .map(|p| {
                let V::Ref(p) = p else { panic!("point reference required") };
                let point = &file.entities[p];
                assert_eq!(point.name, "IFCCARTESIANPOINT");
                let V::List(coords) = &point.args[0] else { panic!("point coordinates required") };
                assert_eq!(coords.len(), 2, "IFC grid curves must be two-dimensional");
                json!(coords.iter().map(|v| if let V::Real(v) = v { *v } else { panic!("real coordinate required") }).collect::<Vec<_>>())
            })
            .collect();
        assert_eq!(coords.len(), 2, "the axis is an open straight segment");
        let V::Str(name) = &axis.args[0] else { panic!("axis tag required") };
        json!([name, coords[0], coords[1]])
    }

    #[test]
    fn ifc_grids_keep_native_names_and_oblique_lines_through_save_and_open() {
        let mut s = Session::new();
        run(&mut s, "grid", json!({"name": "U'1", "start": [-500,100], "end": [2500,4100]}));
        run(&mut s, "grid", json!({"name": "U2", "start": [3500,4100], "end": [500,100]}));
        run(&mut s, "grid", json!({"name": "V", "start": [0,2000], "end": [4000,-1000]}));
        let expected = vec![
            json!(["U'1", [-500.0, 100.0], [2500.0, 4100.0]]),
            json!(["U2", [3500.0, 4100.0], [500.0, 100.0]]),
            json!(["V", [0.0, 2000.0], [4000.0, -1000.0]]),
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grids.ifc");
        run(&mut s, "save", json!({"path": path}));
        let text = std::fs::read_to_string(&path).unwrap();
        let f = step::parse(text.as_bytes()).unwrap();
        let grid = f.entities.values().find(|e| e.name == "IFCGRID").expect("native grids must be exported");
        let (V::List(u), V::List(v)) = (&grid.args[7], &grid.args[8]) else { panic!("U and V axis lists required") };
        assert_eq!(u.iter().map(|a| axis_line(&f, a)).collect::<Vec<_>>(), expected[..2]);
        assert_eq!(v.iter().map(|a| axis_line(&f, a)).collect::<Vec<_>>(), expected[2..]);
        assert_eq!(f.entities.values().filter(|e| e.name == "IFCGRIDAXIS").count(), 3);
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        assert_eq!(grids(&mut reopened), expected);
    }

    // A deliberately shared U/V axis, accepted kindly from external files. Its parent placement is translated,
    // its own turned 90 degrees; SameSense reverses the endpoints after that placement.
    const SHARED_AXIS_GRID: &str = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCCARTESIANPOINT((100.,200.,0.));
#2=IFCAXIS2PLACEMENT3D(#1,$,$);
#3=IFCLOCALPLACEMENT($,#2);
#4=IFCCARTESIANPOINT((10.,20.,0.));
#5=IFCDIRECTION((0.,1.,0.));
#6=IFCAXIS2PLACEMENT3D(#4,$,#5);
#7=IFCLOCALPLACEMENT(#3,#6);
#8=IFCCARTESIANPOINT((1.,2.));
#9=IFCCARTESIANPOINT((4.,6.));
#10=IFCPOLYLINE((#8,#9));
#11=IFCGRIDAXIS('A',#10,.F.);
#12=IFCGRID('0000000000000000000012',$,'Grid',$,$,#7,$,(#11),(#11),$,.IRREGULAR.);
#13=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#14=IFCUNITASSIGNMENT((#13));
#15=IFCPROJECT('0000000000000000000015',$,'Shared axis',$,$,$,$,$,#14);
ENDSEC;END-ISO-10303-21;";

    #[test]
    fn ifc_grids_import_a_shared_axis_once_and_keep_reversed_sense_and_placement() {
        let imported = bimcraft_core::import::ifc(SHARED_AXIS_GRID.as_bytes(), "Shared axis").unwrap();
        let mut s = Session::new();
        s.model = imported.model.into();
        assert_eq!(grids(&mut s), vec![json!(["A", [104.0, 224.0], [108.0, 221.0]])]);
    }

    #[test]
    fn ifc_grids_import_keeps_distinct_coincident_axes_w_axes_and_axes_in_different_placements() {
        let text = SHARED_AXIS_GRID.replace("(#11),(#11),$", "(#11),(#16),(#17)").replace(
            "ENDSEC;END-ISO-10303-21;",
            "#16=IFCGRIDAXIS('B',#10,.T.);
#17=IFCGRIDAXIS('C',#10,.T.);
#18=IFCGRID('0000000000000000000018',$,'Other',$,$,#3,$,(#11),(#11),$,.IRREGULAR.);
ENDSEC;END-ISO-10303-21;",
        );
        let imported = bimcraft_core::import::ifc(text.as_bytes(), "Distinct axes").unwrap();
        let mut s = Session::new();
        s.model = imported.model.into();
        assert_eq!(
            grids(&mut s),
            vec![
                json!(["A", [104.0, 224.0], [108.0, 221.0]]),
                json!(["B", [108.0, 221.0], [104.0, 224.0]]),
                json!(["C", [108.0, 221.0], [104.0, 224.0]]),
                json!(["A (2)", [104.0, 206.0], [101.0, 202.0]]),
            ]
        );
    }

    #[test]
    fn ifc_grids_unrelated_annotations_do_not_become_grids_and_missing_axes_are_reported() {
        let text = SHARED_AXIS_GRID.replace(
            "#12=IFCGRID('0000000000000000000012',$,'Grid',$,$,#7,$,(#11),(#11),$,.IRREGULAR.);",
            "#12=IFCANNOTATION('0000000000000000000012',$,'Broken',$,'BIMCraft_GridAxis',#7,$);
#16=IFCANNOTATION('0000000000000000000016',$,'Ordinary',$,'Other',#7,$);",
        );
        let imported = bimcraft_core::import::ifc(text.as_bytes(), "Annotations").unwrap();
        assert!(imported.skipped.iter().any(|s| s.contains("its grid annotation has no axis")), "{:?}", imported.skipped);
        let mut s = Session::new();
        s.model = imported.model.into();
        assert!(grids(&mut s).is_empty());
    }

    #[test]
    fn ifc_grids_parallel_axes_use_real_disjoint_axis_sets_and_reopen_once() {
        let mut s = Session::new();
        for x in [0, 1000, 2000] {
            run(&mut s, "grid", json!({"start": [x,0], "end": [x,3000]}));
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parallel.ifc");
        run(&mut s, "save", json!({"path": path}));
        let text = std::fs::read_to_string(&path).unwrap();
        let f = step::parse(text.as_bytes()).unwrap();
        let grid = f.entities.values().find(|e| e.name == "IFCGRID").expect("parallel grids must be exported");
        let (V::List(u), V::List(v)) = (&grid.args[7], &grid.args[8]) else { panic!("U and V must be nonempty lists") };
        assert!(!u.is_empty() && !v.is_empty());
        assert!(u.iter().all(|axis| !v.contains(axis)), "no axis belongs to both U and V");
        assert_eq!(f.entities.values().filter(|e| e.name == "IFCGRIDAXIS").count(), 3, "no invented axes");
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        assert_eq!(
            grids(&mut reopened),
            vec![
                json!(["1", [0.0, 0.0], [0.0, 3000.0]]),
                json!(["2", [1000.0, 0.0], [1000.0, 3000.0]]),
                json!(["3", [2000.0, 0.0], [2000.0, 3000.0]])
            ]
        );
    }

    #[test]
    fn ifc_grids_a_lone_axis_is_a_named_curve_annotation_and_reopens_as_a_grid() {
        let mut s = Session::new();
        let added = run(&mut s, "grid", json!({"name": "Solo", "start": [100,200], "end": [3100,4200]}));
        let expected_guid = json!(s.model.guid(added["id"].as_u64().unwrap()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("solo.ifc");
        run(&mut s, "save", json!({"path": path}));
        let text = std::fs::read_to_string(&path).unwrap();
        let f = step::parse(text.as_bytes()).unwrap();
        assert!(!f.entities.values().any(|e| e.name == "IFCGRID" || e.name == "IFCGRIDAXIS"), "no dummy or repeated axes");
        let annotation = f.entities.values().find(|e| e.name == "IFCANNOTATION").expect("a lone grid is exported as a curve annotation");
        assert_eq!(annotation.args[2], V::Str("Solo".into()));
        assert_eq!(annotation.args[4], V::Str("BIMCraft_GridAxis".into()));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        assert_eq!(grids(&mut reopened), vec![json!(["Solo", [100.0, 200.0], [3100.0, 4200.0]])]);
        // An annotation has a GlobalId, so this fallback can retain the native element's identity too.
        let back = run(&mut reopened, "elements", json!({"category": "grid"}));
        assert_eq!(back["elements"][0]["guid"], expected_guid);
    }

    #[test]
    fn ifc_grids_without_native_grids_write_no_grid_entities_or_annotations() {
        let text = bimcraft_core::ifc::export(&Session::new().model);
        let f = step::parse(text.as_bytes()).unwrap();
        assert!(!f.entities.values().any(|e| matches!(e.name.as_str(), "IFCGRID" | "IFCGRIDAXIS" | "IFCANNOTATION")));
    }
}
