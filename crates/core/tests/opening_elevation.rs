//! Requested-level opening placement, exercised through the shared CLI/MCP command entry point.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Id};
    use bimcraft_core::render::{self, View};
    use serde_json::json;

    const WALL: &str = r#"level {"name":"Upper","elevation":3400}
wall {"points":[[0,0],[8000,0]],"level":"Level 1","height":7200}"#;

    fn sill_z(s: &Session, id: Id) -> f64 {
        let (Element::Door(o) | Element::Window(o)) = s.model.elements.get(id).unwrap() else { panic!("not an opening") };
        s.model.opening_frame(o).unwrap().2
    }

    #[test]
    fn upper_level_window_is_visible_on_its_plan_and_survives_save_reopen() {
        let mut s = Session::new();
        s.script(WALL).unwrap();
        let id = s.execute("window", &json!({"at":[4000,0],"level":"Upper","sill":900})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(sill_z(&s, id), 4300.0, "requested level + sill, not host base + sill");
        let upper = s.model.levels().into_iter().find(|(_, l)| l.name == "Upper").unwrap().0;
        let view = View { scale: 0.1, cx: 4000.0, cy: 0.0, width: 1000, height: 400 };
        let drawing = render::draw(&s.model, upper, view);
        assert!(
            drawing.pick_all(view.px(bimcraft_core::geom::P2(4000.0, 0.0)), 5.0).contains(&id),
            "window visible and pickable on upper plan"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upper.bimcraft");
        s.execute("save", &json!({"path":path})).unwrap();
        let guid = s.model.guid(id);
        let mut reopened = Session::new();
        let opened = reopened.execute("open", &json!({"path":path})).unwrap();
        assert!(opened.get("left_out").is_none(), "native file must be fully understood: {opened}");
        let id = reopened.model.elements.id(guid).unwrap();
        assert_eq!(sill_z(&reopened, id), 4300.0);
        assert!(render::draw(&reopened.model, upper, view).owners.contains(&Some(id)));
    }

    #[test]
    fn explicit_host_honors_requested_level_for_doors_and_default_window_sills() {
        for command in ["door", "window"] {
            let mut s = Session::new();
            s.script(WALL).unwrap();
            let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).unwrap().0;
            let result = s.execute(command, &json!({"wall":wall,"offset":4000,"level":"Upper"})).unwrap();
            let id = result["id"].as_u64().unwrap();
            let expected = if command == "door" { 3400.0 } else { 4300.0 };
            assert_eq!(sill_z(&s, id), expected, "{command}");
        }
    }

    #[test]
    fn host_relative_placement_without_a_different_requested_level_is_preserved() {
        for level in [None, Some("Level 1")] {
            let mut s = Session::new();
            s.script(r#"wall {"points":[[0,0],[8000,0]],"base_offset":6000,"height":3000}"#).unwrap();
            let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).unwrap().0;
            let result = s.execute("window", &json!({"wall":wall,"level":level,"sill":900})).unwrap();
            assert_eq!(sill_z(&s, result["id"].as_u64().unwrap()), 6900.0);
        }
    }

    #[test]
    fn different_requested_level_accounts_for_the_host_base_offset() {
        let mut s = Session::new();
        s.script(
            r#"level {"name":"Upper","elevation":3400}
wall {"points":[[0,0],[8000,0]],"base_offset":500,"height":7200}"#,
        )
        .unwrap();
        let id = s.execute("window", &json!({"at":[4000,0],"level":"Upper","sill":900})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(sill_z(&s, id), 4300.0);
        let Element::Window(o) = s.model.elements.get(id).unwrap() else { panic!("not a window") };
        assert_eq!(o.sill, 3800.0, "native sill remains relative to host own base");
    }

    #[test]
    fn upper_level_family_window_uses_its_default_sill_in_preview_and_placement() {
        let mut s = Session::new();
        s.script(WALL).unwrap();
        let params = json!({"at":[4000,0],"level":"Upper","family":"Fixed Window","type":"600 x 900"});
        let Element::Window(preview) = bimcraft_core::opening_of(&s.model, &params, false).unwrap() else { panic!("not a window") };
        assert_eq!(s.model.opening_frame(&preview).unwrap().2, 4500.0);
        let result = s.execute("window", &params).unwrap();
        assert_eq!(sill_z(&s, result["id"].as_u64().unwrap()), 4500.0);
    }

    #[test]
    fn requested_level_outside_host_is_rejected_atomically() {
        let mut s = Session::new();
        s.script(WALL).unwrap();
        s.execute("level", &json!({"name":"Too high","elevation":9000})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).unwrap().0;
        let before = serde_json::to_value(&*s.model).unwrap();
        let error = s.execute("window", &json!({"wall":wall,"level":"Too high"})).unwrap_err();
        assert!(error.contains("top of wall"), "{error}");
        assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
        let error = s.execute("window", &json!({"wall":wall,"level":"Missing"})).unwrap_err();
        assert!(error.contains("no level"), "{error}");
        assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
        s.execute("level", &json!({"name":"Below","elevation":-1000})).unwrap();
        let before = serde_json::to_value(&*s.model).unwrap();
        let error = s.execute("window", &json!({"wall":wall,"level":"Below"})).unwrap_err();
        assert!(error.contains("under wall"), "{error}");
        assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
    }
}
