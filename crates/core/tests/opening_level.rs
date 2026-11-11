//! Opening host inference through the same commands used by the CLI and MCP.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
    }

    fn fixture() -> Session {
        let mut s = Session::new();
        run(&mut s, "level", json!({"name": "Foundation", "elevation": -2400}));
        s
    }

    fn wall(s: &mut Session, level: &str, y: f64) -> Value {
        run(s, "wall", json!({"level": level, "points": [[0, y], [6000, y]], "height": 2400}))["ids"][0].clone()
    }

    #[test]
    fn openings_without_a_level_find_the_ground_wall_and_survive_save_and_reopen() {
        let mut s = fixture();
        let host = wall(&mut s, "Level 1", 0.0);
        let door = run(&mut s, "door", json!({"at": [1500, 0]}));
        let window = run(&mut s, "window", json!({"at": [4200, 0]}));
        assert_eq!(door["host"], host);
        assert_eq!(window["host"], host);
        let before = run(&mut s, "elements", json!({}));
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("openings.bimcraft");
        run(&mut s, "save", json!({"path": path}));
        let mut reopened = Session::new();
        let opened = run(&mut reopened, "open", json!({"path": path}));
        assert!(opened.get("left_out").is_none(), "{opened}");
        assert_eq!(run(&mut reopened, "elements", json!({})), before);
    }

    #[test]
    fn openings_without_a_level_find_an_upper_floor_when_it_is_the_only_host() {
        for command in ["door", "window"] {
            let mut s = fixture();
            let host = wall(&mut s, "Level 2", 0.0);
            assert_eq!(run(&mut s, command, json!({"at": [2000, 0]}))["host"], host);
        }
    }

    #[test]
    fn stacked_hosts_prefer_the_lowest_eligible_level_at_or_above_zero() {
        for command in ["door", "window"] {
            let mut s = fixture();
            wall(&mut s, "Foundation", 0.0);
            wall(&mut s, "Level 2", 0.0);
            let ground = wall(&mut s, "Level 1", 0.0);
            assert_eq!(run(&mut s, command, json!({"at": [2000, 0]}))["host"], ground);
        }
    }

    #[test]
    fn below_ground_hosts_prefer_the_highest_eligible_level() {
        for command in ["door", "window"] {
            let mut s = fixture();
            run(&mut s, "level", json!({"name": "Basement", "elevation": -4800}));
            wall(&mut s, "Basement", 0.0);
            let foundation = wall(&mut s, "Foundation", 0.0);
            assert_eq!(run(&mut s, command, json!({"at": [2000, 0]}))["host"], foundation);
        }
    }

    #[test]
    fn explicit_level_overrides_inference_and_never_falls_back() {
        for command in ["door", "window"] {
            let mut s = fixture();
            let ground = wall(&mut s, "Level 1", 0.0);
            let upper = wall(&mut s, "Level 2", 0.0);
            assert_eq!(run(&mut s, command, json!({"at": [2000, 0], "level": "Level 2"}))["host"], upper);
            let before = run(&mut s, "elements", json!({}));
            let error = s.execute(command, &json!({"at": [2000, 0], "level": "Foundation"})).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "{command}: no wall at [2000, 0] on Foundation; wall {ground} stands there but spans 0 to 2400 mm and Foundation cuts at -1200 mm: give the wall that level, or a view range that cuts it"
                )
            );
            assert!(error.contains("no wall at [2000, 0] on Foundation"), "{error}");
            assert!(error.contains("stands there but spans"), "preserve the cut-range diagnostic: {error}");
            assert_eq!(run(&mut s, "elements", json!({})), before);
        }
    }

    #[test]
    fn no_eligible_host_reports_the_point_and_preserves_the_model_and_undo() {
        for command in ["door", "window"] {
            let mut s = fixture();
            wall(&mut s, "Level 1", 0.0);
            let before = run(&mut s, "elements", json!({}));
            let error = s.execute(command, &json!({"at": [2000, 4000]})).unwrap_err();
            assert!(error.contains("no wall at [2000, 4000] on any level"), "{error}");
            assert!(error.contains("`wall` with `offset`"), "{error}");
            assert_eq!(run(&mut s, "elements", json!({})), before);
            run(&mut s, "undo", json!({}));
            assert_eq!(run(&mut s, "elements", json!({"category": "walls"}))["count"], 0);
        }
    }

    #[test]
    fn an_inferred_host_still_rejects_an_opening_outside_the_wall_atomically() {
        let mut s = fixture();
        wall(&mut s, "Level 1", 0.0);
        let before = run(&mut s, "elements", json!({}));
        assert!(s.execute("door", &json!({"at": [20, 0]})).is_err());
        assert_eq!(run(&mut s, "elements", json!({})), before);
        assert_eq!(run(&mut s, "door", json!({"at": [2000, 0]}))["mark"], "1");
    }

    #[test]
    fn explicit_wall_remains_authoritative_even_with_a_different_level() {
        let mut s = fixture();
        let upper = wall(&mut s, "Level 2", 0.0);
        // The wall stays the host: no other wall is looked for. The level sets the height (#343), and the Foundation's
        // would put the door under that wall's base, so it is refused naming the wall, not moved to another.
        let error = s.execute("door", &json!({"wall": upper, "offset": 2000, "level": "Foundation"})).unwrap_err();
        assert!(error.contains(&format!("wall {upper}'s base")), "{error}");
        assert_eq!(run(&mut s, "door", json!({"wall": upper, "offset": 2000, "level": "Level 2"}))["host"], upper);
    }

    #[test]
    fn an_implicit_level_failure_keeps_wall_span_and_cut_context_without_an_undo_step() {
        for command in ["door", "window"] {
            let mut s = fixture();
            let host = run(&mut s, "wall", json!({"level": "Level 2", "points": [[0, 0], [6000, 0]], "height": 600}))["ids"][0].clone();
            let before = run(&mut s, "elements", json!({}));
            let error = s.execute(command, &json!({"at": [2000, 0]})).unwrap_err();
            assert!(error.contains("no wall at [2000, 0] on any level"), "{error}");
            assert!(error.contains(&format!("wall {host} stands there but spans 3000 to 3600 mm")), "{error}");
            assert!(error.contains("Level 2 cuts at 4200 mm"), "{error}");
            assert!(error.contains("`wall` with `offset`"), "{error}");
            assert_eq!(run(&mut s, "elements", json!({})), before);
            run(&mut s, "undo", json!({}));
            assert_eq!(run(&mut s, "elements", json!({"category": "walls"}))["count"], 0);
        }
    }

    #[test]
    fn inferred_openings_render_in_the_ground_plan_and_export_as_hosted_ifc() {
        use bimcraft_core::step::{self, V};

        let mut s = fixture();
        wall(&mut s, "Level 1", 0.0);
        let folder = tempfile::tempdir().unwrap();
        let before = folder.path().join("before.png");
        let after = folder.path().join("after.png");
        run(&mut s, "render", json!({"view": "Level 1", "path": before}));
        run(&mut s, "door", json!({"at": [1500, 0], "level": null}));
        run(&mut s, "window", json!({"at": [4200, 0]}));
        run(&mut s, "render", json!({"view": "Level 1", "path": after}));
        let before = tiny_skia::Pixmap::load_png(before).unwrap();
        let after = tiny_skia::Pixmap::load_png(after).unwrap();
        assert_eq!((before.width(), before.height()), (after.width(), after.height()));
        assert!(before.pixels().iter().zip(after.pixels()).filter(|(a, b)| a != b).count() > 100, "openings change the plan pixels");

        let path = folder.path().join("openings.ifc");
        run(&mut s, "save", json!({"path": path}));
        let file = step::parse(&std::fs::read(path).unwrap()).unwrap();
        let mut filled = Vec::new();
        for fill in file.entities.values().filter(|e| e.name == "IFCRELFILLSELEMENT") {
            let (V::Ref(opening), V::Ref(element)) = (&fill.args[4], &fill.args[5]) else { panic!("fill references") };
            assert_eq!(file.entities[opening].name, "IFCOPENINGELEMENT");
            filled.push(file.entities[element].name.as_str());
            assert!(
                file.entities.values().any(|e| {
                    e.name == "IFCRELVOIDSELEMENT"
                        && e.args[5] == V::Ref(*opening)
                        && matches!(&e.args[4], V::Ref(wall) if file.entities[wall].name == "IFCWALL")
                }),
                "the filled opening cuts its host wall"
            );
        }
        filled.sort();
        assert_eq!(filled, ["IFCDOOR", "IFCWINDOW"]);
    }
}
