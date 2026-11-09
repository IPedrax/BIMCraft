//! Opening placement through the same public commands as the CLI and MCP.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};

    fn run(session: &mut Session, command: &str, params: Value) -> Value {
        session.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
    }

    #[test]
    fn raised_wall_openings_render_and_keep_their_hosts_after_save_and_open() {
        for command in ["door", "window"] {
            let mut session = Session::new();
            let wall = run(
                &mut session,
                "wall",
                json!({"points": [[0,0],[8000,0]], "level": "Level 1", "base_offset": 6000, "height": 3000}),
            )["ids"][0]
                .clone();
            let before = run(&mut session, "render", json!({"view": "{3D}", "width": 400, "height": 300}))["pngBase64"].clone();
            let params = json!({"at": [4000,0], "level": "Level 1"});
            let preview = bimcraft_core::opening_of(&session.model, &params, command == "door").unwrap();
            let opening = run(&mut session, command, params);
            assert_eq!(session.model.elements[&opening["id"].as_u64().unwrap()], preview, "desktop preview and placement agree");
            assert_eq!(opening["host"], wall);
            assert_eq!(opening["offset"], 4000.0);
            let after = run(&mut session, "render", json!({"view": "{3D}", "width": 400, "height": 300}))["pngBase64"].clone();
            assert!(after.is_string());
            assert_ne!(before, after, "the placed opening must change the rendered wall");

            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("raised-wall.bimcraft");
            let elements = run(&mut session, "elements", json!({}));
            run(&mut session, "save", json!({"path": path}));
            let mut reopened = Session::new();
            let opened = run(&mut reopened, "open", json!({"path": path}));
            assert!(opened.get("left_out").is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)), "{opened}");
            assert_eq!(run(&mut reopened, "elements", json!({})), elements, "opening and GUID host must persist");
            assert_eq!(run(&mut reopened, "render", json!({"view": "{3D}", "width": 400, "height": 300}))["pngBase64"], after);
        }
    }

    #[test]
    fn raised_host_lookup_preserves_cut_wall_priority_and_explicit_wall_control() {
        for command in ["door", "window"] {
            let mut session = Session::new();
            let raised =
                run(&mut session, "wall", json!({"points": [[0,0],[8000,0]], "base_offset": 6000, "height": 3000}))["ids"][0].clone();
            let cut = run(&mut session, "wall", json!({"points": [[0,0],[8000,0]], "height": 3000}))["ids"][0].clone();
            assert_eq!(run(&mut session, command, json!({"at": [2000,0], "level": "Level 1"}))["host"], cut);
            assert_eq!(run(&mut session, command, json!({"wall": raised, "offset": 4000}))["host"], raised);
        }
    }

    #[test]
    fn stacked_raised_hosts_require_a_wall_id_and_refusal_changes_nothing() {
        for command in ["door", "window"] {
            let mut session = Session::new();
            let first =
                run(&mut session, "wall", json!({"points": [[0,0],[8000,0]], "base_offset": 6000, "height": 3000}))["ids"][0].clone();
            let second =
                run(&mut session, "wall", json!({"points": [[0,0],[8000,0]], "base_offset": 10000, "height": 3000}))["ids"][0].clone();
            let before = run(&mut session, "elements", json!({}));
            let error = session.execute(command, &json!({"at": [4000,0], "level": "Level 1"})).unwrap_err();
            assert!(error.contains("ambiguous") && error.contains("wall"), "{error}");
            assert_eq!(run(&mut session, "elements", json!({})), before);
            for wall in [first, second] {
                assert_eq!(run(&mut session, command, json!({"wall": wall, "offset": 4000}))["host"], wall);
            }
        }
    }

    #[test]
    fn raised_wall_lookup_keeps_low_wall_and_other_level_diagnostics() {
        for params in [
            json!({"points": [[0,0],[8000,0]], "level": "Level 1", "height": 250}),
            json!({"points": [[0,0],[8000,0]], "level": "Level 2", "base_offset": 6000, "height": 3000}),
        ] {
            let mut session = Session::new();
            run(&mut session, "wall", params);
            for command in ["door", "window"] {
                let error = session.execute(command, &json!({"at": [4000,0], "level": "Level 1"})).unwrap_err();
                assert!(error.contains("stands there") && error.contains("cuts at 1200"), "{error}");
            }
        }
    }

    #[test]
    fn raised_wall_fallback_still_validates_sill_width_and_overlapping_openings() {
        let mut session = Session::new();
        run(&mut session, "wall", json!({"points": [[0,0],[8000,0]], "base_offset": 6000, "height": 3000}));
        for (params, diagnostic) in [
            (json!({"at": [4000,0], "level": "Level 1", "sill": -1}), "sill"),
            (json!({"at": [4000,0], "level": "Level 1", "sill": 3000}), "top"),
            (json!({"at": [0,0], "level": "Level 1"}), "doesn't fit"),
        ] {
            let before = run(&mut session, "elements", json!({}));
            let error = session.execute("window", &params).unwrap_err();
            assert!(error.contains(diagnostic), "{error}");
            assert_eq!(run(&mut session, "elements", json!({})), before);
        }
        run(&mut session, "window", json!({"at": [4000,0], "level": "Level 1"}));
        let before = run(&mut session, "elements", json!({}));
        assert!(session.execute("window", &json!({"at": [4000,0], "level": "Level 1"})).is_err());
        assert_eq!(run(&mut session, "elements", json!({})), before);
    }
}
