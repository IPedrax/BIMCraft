//! Roof commands exercised through the same public session used by CLI and MCP.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
        s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e}"))
    }

    fn roof(s: &mut Session, id: &Value) -> Value {
        run(s, "elements", json!({"category":"roof"}))["elements"].as_array().unwrap().iter().find(|r| &r["id"] == id).unwrap().clone()
    }

    #[test]
    fn explicit_roof_overhang_extends_eaves_and_keeps_wall_line_height_after_reopen() {
        for boundary in [json!([[0, 0], [10000, 0], [10000, 20000], [0, 20000]]), json!([[0, 20000], [10000, 20000], [10000, 0], [0, 0]])] {
            let mut s = Session::new();
            let id = run(
                &mut s,
                "roof",
                json!({"level":"Level 1", "boundary":boundary, "overhang":750,"slope":10,"gables":[0,1,2],"offset":6000}),
            )["id"]
                .clone();
            let expected = roof(&mut s, &id);
            let mut outline: Vec<[f64; 2]> = serde_json::from_value(expected["outline"].clone()).unwrap();
            outline.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(outline, vec![[-750., -750.], [-750., 20750.], [10750., -750.], [10750., 20750.]]);
            assert!((expected["ridge"].as_f64().unwrap() - (6000. + 10750. * 10f64.to_radians().tan())).abs() < 0.02);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("roof.bimcraft");
            run(&mut s, "save", json!({"path":path}));
            let mut reopened = Session::new();
            run(&mut reopened, "open", json!({"path":path}));
            assert_eq!(roof(&mut reopened, &id), expected);
            run(&mut reopened, "render", json!({"view":"{3D}","path":dir.path().join("roof.png"),"width":400,"height":300}));
            assert!(std::fs::metadata(dir.path().join("roof.png")).unwrap().len() > 1000);
        }
    }

    #[test]
    fn sketch_overhang_preserves_per_edge_pitches_and_edit_boundary_does_not_offset_twice() {
        let mut s = Session::new();
        let id = run(
            &mut s,
            "roof",
            json!({"overhang":400,"slope":30,"sketch":[
                {"start":[0,0],"end":[8000,0]}, {"start":[8000,0],"end":[8000,6000],"slope":45},
                {"start":[8000,6000],"end":[0,6000]}, {"start":[0,6000],"end":[0,0],"slope":45}
            ]}),
        )["id"]
            .clone();
        let before = roof(&mut s, &id);
        assert_eq!(before["outline"], json!([[-400., -400.], [8400., -400.], [8400., 6400.], [-400., 6400.]]));
        assert_eq!(before["slopes"], json!([0., 45., 0., 45.]));
        // Alignment picks the visible eave, rather than the input wall line, before the user edits it.
        let picked = bimcraft_core::align::lines(&s.model, id.as_u64().unwrap());
        assert!(picked.iter().any(|(a, b)| a.1 == -400. && b.1 == -400.), "visible south eave missing from alignment picks: {picked:?}");
        let sketch = run(&mut s, "edit_boundary", json!({"id":id}))["sketch"].clone();
        run(&mut s, "edit_boundary", json!({"id":id,"sketch":sketch}));
        assert_eq!(roof(&mut s, &id)["outline"], before["outline"]);
        assert_eq!(roof(&mut s, &id)["slopes"], before["slopes"]);
    }

    #[test]
    fn courtyards_offset_inward_and_impossible_creation_is_atomic() {
        let mut s = Session::new();
        let before = s.model.to_json().unwrap();
        let err = s.execute("roof", &json!({"boundary":[[0,0],[600,0],[600,600],[0,600]],"slope":0,"overhang":-400})).unwrap_err();
        assert!(err.contains("overhang"), "{err}");
        assert_eq!(s.model.to_json().unwrap(), before);
        let err = s.execute("roof", &json!({"boundary":[[0,0],[10000,0],[10000,10000],[0,10000]],"courtyards":[[[4000,4000],[4000,4600],[4600,4600],[4600,4000]]],"slope":0,"overhang":400})).unwrap_err();
        assert!(err.contains("courtyard"), "{err}");
        let id = run(&mut s,"roof",json!({"boundary":[[0,0],[10000,0],[10000,10000],[0,10000]],"courtyards":[[[3000,3000],[3000,7000],[7000,7000],[7000,3000]]],"slope":0,"overhang":500}))["id"].clone();
        assert_eq!(roof(&mut s, &id)["courtyards"], json!([[[3500., 3500.], [3500., 6500.], [6500., 6500.], [6500., 3500.]]]));
    }

    #[test]
    fn impossible_mass_roof_update_refuses_all_elements_and_preserves_undo_redo() {
        let mut s = Session::new();
        let mass = run(&mut s, "mass", json!({"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]],"height":3000}))["id"].clone();
        let floor = run(&mut s, "floor_by_face", json!({"mass":mass}))["id"].clone();
        let id = run(&mut s, "roof_by_face", json!({"mass":mass,"overhang":-400}))["id"].clone();
        let initial = s.model.to_json().unwrap();
        run(&mut s, "modify", json!({"id":mass,"boundary":[[0,0],[600,0],[600,600],[0,600]]}));
        let before = s.model.to_json().unwrap();
        let history: Vec<String> = s.history().iter().map(|s| s.to_string()).collect();
        let err = s.execute("update_by_face", &json!({"ids":[floor,id]})).unwrap_err();
        assert!(err.contains("overhang") && err.contains(&id.to_string()), "{err}");
        assert_eq!(s.model.to_json().unwrap(), before, "a failed update must leave every element unchanged");
        assert_eq!(s.history(), history);
        run(&mut s, "undo", json!({}));
        assert_eq!(s.model.to_json().unwrap(), initial, "failure must not consume an undo step");
        run(&mut s, "redo", json!({}));
        assert_eq!(s.model.to_json().unwrap(), before);
        run(&mut s, "modify", json!({"id":mass,"boundary":[[0,0],[10000,0],[10000,6000],[0,6000]]}));
        run(&mut s, "update_by_face", json!({"mass":mass}));
        assert_eq!(roof(&mut s, &id)["outline"], json!([[400., 400.], [9600., 400.], [9600., 5600.], [400., 5600.]]));
    }

    #[test]
    fn mass_roof_keeps_positive_overhang_identity_and_pitch_through_update_and_reopen() {
        let mut s = Session::new();
        let mass = run(&mut s, "mass", json!({"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]],"height":3000}))["id"].clone();
        let id = run(&mut s, "roof_by_face", json!({"mass":mass,"overhang":400,"slope":20}))["id"].clone();
        let before = roof(&mut s, &id);
        assert_eq!(before["outline"], json!([[-400., -400.], [8400., -400.], [8400., 6400.], [-400., 6400.]]));
        run(&mut s, "modify", json!({"id":mass,"boundary":[[0,0],[10000,0],[10000,6000],[0,6000]]}));
        run(&mut s, "update_by_face", json!({"mass":mass}));
        let after = roof(&mut s, &id);
        assert_eq!(after["outline"], json!([[-400., -400.], [10400., -400.], [10400., 6400.], [-400., 6400.]]));
        for key in ["id", "guid", "type", "slope", "overhang"] {
            assert_eq!(after[key], before[key], "updating the face must keep {key}");
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mass-roof.bimcraft");
        run(&mut s, "save", json!({"path":path}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path":path}));
        assert_eq!(roof(&mut reopened, &id), after);
        assert_eq!(run(&mut reopened, "update_by_face", json!({"mass":mass}))["unchanged"], json!([id]));
    }

    #[test]
    fn wall_derived_roofs_and_zero_overhang_keep_existing_outlines() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points":[[0,0],[8000,0],[8000,6000],[0,6000]],"closed":true}));
        let id = run(&mut s, "roof", json!({}))["id"].clone();
        let r = roof(&mut s, &id);
        assert!(r["outline"][0][0].as_f64().unwrap() < -500.);
        let id = run(&mut s, "roof", json!({"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]],"slope":0}))["id"].clone();
        assert_eq!(roof(&mut s, &id)["outline"], json!([[0., 0.], [8000., 0.], [8000., 6000.], [0., 6000.]]));
    }
}
