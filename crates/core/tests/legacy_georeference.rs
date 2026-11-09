//! A native file's identifiable legacy georeference survives open, save and IFC exchange.

#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::FORMAT;
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn project(georeference: Value) -> Value {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "height": 3000}));
        let mut value: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        value["georeference"] = georeference;
        value
    }

    fn legacy() -> Value {
        json!({"crs": "EPSG:32632", "eastings": 500_000_000, "northings": 5_000_000_000.0,
            "orthogonal_height": 42_000, "rotation": 30})
    }

    #[test]
    fn legacy_height_and_clockwise_rotation_survive_native_and_ifc_reopen() {
        for scale in [None, Some(0.9996)] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("legacy.bimcraft");
            let native = dir.path().join("current.bimcraft");
            let ifc = dir.path().join("current.ifc");
            let mut geo = legacy();
            if let Some(scale) = scale {
                geo["scale"] = json!(scale);
            }
            let source = project(geo);
            std::fs::write(&input, source.to_string()).unwrap();
            let mut s = Session::new();
            let opened = run(&mut s, "open", json!({"path": input}));
            let g = s.model.georeference.as_ref().unwrap();
            assert_eq!(g.height, 42_000.0, "opening legacy height must not silently zero it");
            assert_eq!(g.rotation, -30.0, "legacy clockwise becomes counterclockwise");
            assert_eq!(g.scale, scale.unwrap_or(1.0));
            assert!(opened.get("left_out").is_none_or(|v| v.as_array().is_some_and(Vec::is_empty)), "{opened}");
            let expected = g.clone();
            let ids: Vec<_> = s.model.elements.with_guids().map(|(id, guid, _)| (id, guid)).collect();
            run(&mut s, "save", json!({"path": native}));
            run(&mut s, "save", json!({"path": ifc}));
            let saved: Value = serde_json::from_str(&std::fs::read_to_string(&native).unwrap()).unwrap();
            assert!(saved["georeference"].get("orthogonal_height").is_none());
            let mut reopened = Session::new();
            run(&mut reopened, "open", json!({"path": native}));
            assert_eq!(reopened.model.georeference.as_ref(), Some(&expected));
            assert_eq!(ids, reopened.model.elements.with_guids().map(|(id, guid, _)| (id, guid)).collect::<Vec<_>>());
            run(&mut reopened, "open", json!({"path": ifc}));
            let imported = reopened.model.georeference.as_ref().unwrap();
            assert!((imported.height - expected.height).abs() < 1e-6);
            assert!((imported.rotation - expected.rotation).abs() < 1e-8);
            assert!((imported.scale - expected.scale).abs() < 1e-12);
        }
    }

    #[test]
    fn modern_fields_and_unknown_field_reporting_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.bimcraft");
        for old in [false, true] {
            let mut geo = legacy();
            if !old {
                geo.as_object_mut().unwrap().remove("orthogonal_height");
                geo["height"] = json!(42_000);
            }
            geo["scale"] = json!(1.0002);
            geo["future_metadata"] = json!("retained in left_out report");
            std::fs::write(&path, project(geo).to_string()).unwrap();
            let mut s = Session::new();
            let report = run(&mut s, "open", json!({"path": path}));
            assert_eq!(report["left_out"], json!(["/georeference/future_metadata"]));
            let g = s.model.georeference.as_ref().unwrap();
            assert_eq!((g.height, g.rotation, g.scale), (42_000.0, if old { -30.0 } else { 30.0 }, 1.0002));
        }
    }

    #[test]
    fn conflicting_invalid_and_newer_payloads_leave_the_open_project_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refused.bimcraft");
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
        let before = s.model.to_json().unwrap();
        let mut cases = Vec::new();
        let mut both = legacy();
        both["height"] = json!(42_000);
        cases.push(project(both));
        for (key, invalid) in [("orthogonal_height", json!("bad")), ("rotation", json!("30")), ("scale", json!(0))] {
            let mut geo = legacy();
            geo[key] = invalid;
            cases.push(project(geo));
        }
        for value in cases {
            std::fs::write(&path, value.to_string()).unwrap();
            assert!(s.execute("open", &json!({"path": path})).is_err(), "must refuse ambiguous, invalid or newer georeference input");
            assert_eq!(s.model.to_json().unwrap(), before);
        }
        let mut newer = project(legacy());
        newer["format"] = json!(FORMAT + 1);
        newer["georeference"]["height"] = json!("ambiguous and invalid");
        std::fs::write(&path, newer.to_string()).unwrap();
        let error = s.execute("open", &json!({"path": path})).unwrap_err();
        assert!(error.contains("newer BIMCraft"), "version guard must precede migration: {error}");
        assert_eq!(s.model.to_json().unwrap(), before);
        std::fs::write(&path, project(legacy()).to_string()).unwrap();
        s.read_only = Some("read-only migration acceptance");
        assert!(s.execute("open", &json!({"path": path})).is_err());
        assert_eq!(s.model.to_json().unwrap(), before);
    }

    #[test]
    fn absent_legacy_rotation_and_scale_keep_the_existing_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("defaults.bimcraft");
        let mut geo = legacy();
        geo.as_object_mut().unwrap().remove("rotation");
        std::fs::write(&path, project(geo).to_string()).unwrap();
        let mut s = Session::new();
        run(&mut s, "open", json!({"path": path}));
        let g = s.model.georeference.as_ref().unwrap();
        assert_eq!((g.height, g.rotation, g.scale), (42_000.0, 0.0, 1.0));
    }
}
