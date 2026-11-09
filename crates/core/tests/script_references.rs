//! Scripts create, reference, persist and reopen synthetic elements through the public session.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::json;

    #[test]
    fn named_script_results_create_a_hosted_door_and_survive_save_reopen() {
        let mut s = Session::new();
        let out = s
            .script(
                r#"@walls = wall {"points": [[0,0],[8000,0],[8000,6000]]}
door {"wall": "@walls.ids.1", "offset": 3000}
modify {"id": "$last", "mark": "Entry"}"#,
            )
            .unwrap();
        let door = out[1]["id"].as_u64().unwrap();
        let wall = out[0]["ids"][1].as_u64().unwrap();
        let before = s.execute("elements", &json!({"category":"doors"})).unwrap();
        let host: bimcraft_core::model::Guid = serde_json::from_value(before["elements"][0]["host"].clone()).unwrap();
        assert!(s.model.same(host, wall));
        assert_eq!(before["elements"][0]["id"], door);
        assert_eq!(before["elements"][0]["mark"], "Entry");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("references.bimcraft");
        s.execute("save", &json!({"path":path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":path})).unwrap();
        assert_eq!(reopened.execute("elements", &json!({"category":"doors"})).unwrap(), before);
        assert_eq!(reopened.model.check(), Ok(()));
    }

    #[test]
    fn named_ids_and_array_results_attach_a_wall_and_delete_selected_grids() {
        let mut s = Session::new();
        let out = s
            .script(
                r#"level {"elevation":6000,"name":"Roof"}
@roof = roof {"level":"Roof","boundary":[[0,0],[8000,0],[8000,6000],[0,6000]],"slope":10,"overhang":0}
wall {"points":[[0,0],[8000,0]],"height":3000,"attach_top":"@roof"}
@a = grid {"start":[0,0],"end":[0,4000]}
@b = grid {"start":[2000,0],"end":[2000,4000]}
delete {"ids":["@a","@b"]}"#,
            )
            .unwrap();
        let wall = out[2]["ids"][0].as_u64().unwrap();
        let Some(bimcraft_core::model::Element::Wall(wall)) = s.model.elements.get(wall) else { panic!("no wall") };
        assert!(wall.attach_top.is_some_and(|guid| s.model.same(guid, out[1]["id"].as_u64().unwrap())));
        assert_eq!(s.execute("elements", &json!({"category":"grid"})).unwrap()["count"], 0);
    }

    #[test]
    fn external_command_results_resolve_nested_values_once_without_interpolating_keys() {
        use bimcraft_core::engine::ExternalCommand;
        use std::sync::Arc;
        let mut s = Session::new();
        s.register(ExternalCommand {
            id: "plugin:reference:echo".into(),
            label: "Echo".into(),
            aliases: vec![],
            params: "{}".into(),
            menu: None,
            plugin: "reference".into(),
            run: Arc::new(|_, params| Ok(params.clone())),
        })
        .unwrap();
        let out = s
            .script(
                r#"@first = plugin:reference:echo {"nested":{"items":[7,true,null,"@@literal"]}}
plugin:reference:echo {"@key":{"values":["@first.nested.items","$last.nested.items.3"]}}"#,
            )
            .unwrap();
        assert_eq!(out[1], json!({"@key":{"values":[[7,true,null,"@literal"],"@literal"]}}));
        s.read_only = Some("read-only test");
        assert!(s.script("@query = plugin:reference:echo {\"id\":42}\nplugin:reference:echo {\"value\":\"@query\"}").is_ok());
    }

    #[test]
    fn relative_script_result_references_skip_comments_and_blank_lines() {
        let mut s = Session::new();
        s.script(
            r#"grid {"start":[0,0],"end":[0,4000],"name":"A"}
# not a result

grid {"start":[2000,0],"end":[2000,4000],"name":"B"}
modify {"id":"$last","name":"B1"}
modify {"id":"$-3","name":"A1"}"#,
        )
        .unwrap();
        let result = s.execute("elements", &json!({"category":"grid"})).unwrap();
        assert_eq!(result["elements"][0]["name"], "A1");
        assert_eq!(result["elements"][1]["name"], "B1");
    }

    #[test]
    fn atomic_reference_errors_restore_history_ids_and_allow_a_clean_retry() {
        let mut s = Session::new();
        s.execute("grid", &json!({"start":[0,0],"end":[0,4000]})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        let before = serde_json::to_value(&s.model).unwrap();
        let history = s.history();
        let history: Vec<String> = history.into_iter().map(str::to_owned).collect();
        let redo = s.steps(true).len();
        let e = s
            .script_atomic(
                r#"@walls = wall {"points":[[0,0],[8000,0]]}
door {"wall":"@walls.ids.99","offset":3000}"#,
            )
            .unwrap_err();
        assert!(e.starts_with("line 2 (door):") && e.contains("99"), "{e}");
        assert_eq!(serde_json::to_value(&s.model).unwrap(), before);
        assert_eq!(s.history(), history);
        assert_eq!(s.steps(true).len(), redo);
        s.script_atomic(
            r#"@walls = wall {"points":[[0,0],[8000,0]]}
door {"wall":"@walls.ids.0","offset":3000}"#,
        )
        .unwrap();
        assert_eq!(s.history(), vec!["Script"]);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.model).unwrap(), before);
        s.execute("redo", &json!({})).unwrap();
        assert_eq!(s.execute("elements", &json!({"category":"doors"})).unwrap()["count"], 1);
    }

    #[test]
    fn bad_references_name_the_failing_line_and_non_atomic_scripts_keep_prior_results() {
        for (reference, message) in [
            ("@missing", "no line before"),
            ("$-5", "lines run before"),
            ("$-0", "positive"),
            ("$-nope", "positive"),
            ("@walls", "no `id`"),
            ("@walls.ids.8", "no `8`"),
            ("@walls.nope", "no `nope`"),
            ("@walls.ids.-1", "no `-1`"),
            ("@walls.", "empty"),
            ("@walls.ids..0", "empty"),
        ] {
            let mut s = Session::new();
            let text = format!(
                "@walls = wall {{\"points\":[[0,0],[8000,0]]}}\ndoor {{\"wall\":{reference:?},\"offset\":3000}}\nlevel {{\"elevation\":9000}}"
            );
            let e = s.script(&text).unwrap_err();
            assert!(e.starts_with("line 2 (door):") && e.contains(reference) && e.contains(message), "{reference}: {e}");
            assert_eq!(s.execute("elements", &json!({"category":"walls"})).unwrap()["count"], 1);
            assert_eq!(s.model.levels().len(), 2, "the line after the failure never ran");
            assert_eq!(s.history().len(), 1);
            assert!(s.script("modify {\"id\":\"@walls.ids.0\"}").unwrap_err().contains("no line before"), "names are scoped to one script");
        }
    }

    #[test]
    fn named_lines_validate_names_and_allow_literal_at_and_dollar_text() {
        for text in ["@ = inspect", "@bad.name = inspect", "@name inspect", "@name =", "@x = inspect\n@x = inspect"] {
            let e = Session::new().script(text).unwrap_err();
            assert!(e.starts_with("line ") && (e.contains("name") || e.contains("missing")), "{text}: {e}");
        }
        let mut s = Session::new();
        s.script(
            r#"text {"text":"@@hello","at":[0,0]}
text {"text":"$$last","at":[0,100]}
text {"text":"Email a@b and $5","at":[0,200]}"#,
        )
        .unwrap();
        let notes = s.execute("elements", &json!({"category":"text"})).unwrap();
        let texts: Vec<_> = notes["elements"].as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["@hello", "$last", "Email a@b and $5"]);
    }

    #[test]
    fn named_control_port_lines_cannot_replace_the_project_or_bypass_read_only() {
        use bimcraft_core::bridge::{self, Access, Request};
        let mut s = Session::new();
        for atomic in [false, true] {
            for name in ["new", "OPEN", "recover", "highrise", "tower"] {
                let text = format!("wall {{\"points\":[[0,0],[8000,0]]}}\n@project = {name}");
                let e = bridge::run(&mut s, &Request::Script { text, atomic, read_only: false }, Access::Edit).unwrap_err();
                assert!(e.starts_with("line 2:") && e.contains("replace the project"), "{e}");
                assert!(s.history().is_empty(), "bridge preflight must refuse before any line runs");
            }
            let text = "@walls = wall {\"points\":[[0,0],[8000,0]]}".to_owned();
            let e = bridge::run(&mut s, &Request::Script { text, atomic, read_only: true }, Access::Edit).unwrap_err();
            assert!(e.contains("read-only"), "{e}");
            assert!(s.history().is_empty());
        }
        s.script("wall {\"points\":[[0,0],[8000,0]]}").unwrap();
        let text =
            "@all = elements {\"category\":\"walls\"}\nelements {\"level\":\"@all.elements.0.level\",\"category\":\"walls\"}".to_owned();
        let results = bridge::run(&mut s, &Request::Script { text, atomic: true, read_only: true }, Access::Read).unwrap();
        assert_eq!(results[1]["count"], 1);
    }

    #[test]
    fn non_atomic_control_port_scripts_keep_completed_lines_before_a_malformed_name() {
        use bimcraft_core::bridge::{self, Access, Request};
        for atomic in [false, true] {
            let mut s = Session::new();
            let text = "wall {\"points\":[[0,0],[8000,0]]}\n@bad.name = inspect".to_owned();
            let error = bridge::run(&mut s, &Request::Script { text, atomic, read_only: false }, Access::Edit).unwrap_err();
            assert!(error.starts_with("line 2:") && error.contains("name"), "{error}");
            assert_eq!(s.history().len(), usize::from(!atomic));
        }
    }
    fn external_reply_session(reply: serde_json::Value) -> (Session, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use bimcraft_core::engine::ExternalCommand;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let mut s = Session::new();
        let calls = Arc::new(AtomicUsize::new(0));
        s.register(ExternalCommand {
            id: "plugin:reference:source".into(),
            label: "Source".into(),
            aliases: vec![],
            params: "{}".into(),
            menu: None,
            plugin: "reference".into(),
            run: Arc::new(move |_, _| Ok(reply.clone())),
        })
        .unwrap();
        let received = Arc::clone(&calls);
        s.register(ExternalCommand {
            id: "plugin:reference:accept".into(),
            label: "Accept".into(),
            aliases: vec![],
            params: "{}".into(),
            menu: None,
            plugin: "reference".into(),
            run: Arc::new(move |_, _| {
                received.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"accepted":true}))
            }),
        })
        .unwrap();
        (s, calls)
    }

    #[test]
    fn reference_expansion_budget_refuses_amplification_before_execution_and_rolls_back_for_retry() {
        use std::sync::atomic::Ordering;
        let (mut s, calls) = external_reply_session(json!({"payload":"x".repeat(16_384)}));
        let before = serde_json::to_value(&s.model).unwrap();
        let prefix = "wall {\"points\":[[0,0],[8000,0]]}\n@large = plugin:reference:source\n";
        let text = format!("{prefix}plugin:reference:accept {}", json!({"copies":vec!["@large.payload";1100]}));
        let result = s.script_atomic(&text);
        assert!(result.is_err(), "unbounded expansion executed the recipient {} times", calls.load(Ordering::SeqCst));
        let error = result.err().unwrap();
        assert!(error.starts_with("line 3 (plugin:reference:accept):") && error.contains("16777216"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(serde_json::to_value(&s.model).unwrap(), before);
        assert!(s.history().is_empty());
        s.script_atomic(&format!("{prefix}plugin:reference:accept {{\"copies\":[\"@large.payload\"]}}")).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(s.history(), vec!["Script"]);
    }

    #[test]
    fn reference_expansion_diagnostics_do_not_copy_large_prior_replies() {
        for reference in ["@large", "@large.missing"] {
            let (mut s, _) = external_reply_session(json!({"payload":"private-payload".repeat(4096)}));
            let error =
                s.script(&format!("@large = plugin:reference:source\nplugin:reference:accept {}", json!({"value":reference}))).unwrap_err();
            assert!(error.contains(reference));
            assert!(error.len() < 1024, "reference diagnostic grew to {} bytes", error.len());
            assert!(!error.contains("private-payload"));
        }
    }

    #[test]
    fn reference_expansion_rejects_deep_external_results_before_execution() {
        use std::sync::atomic::Ordering;
        let mut payload = json!(7);
        for _ in 0..256 {
            payload = json!([payload]);
        }
        let (mut s, calls) = external_reply_session(json!({"payload":payload}));
        let result = s.script("@deep = plugin:reference:source\nplugin:reference:accept {\"value\":\"@deep.payload\"}");
        assert!(result.is_err(), "an excessively nested external result reached the recipient");
        let error = result.err().unwrap();
        assert!(error.contains("128") && error.contains("depth"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reference_expansion_accepts_an_id_list_at_the_existing_command_cap() {
        let mut s = Session::new();
        let id = s.execute("grid", &json!({"start":[0,0],"end":[0,4000]})).unwrap()["id"].as_u64().unwrap();
        let ids = vec![id; bimcraft_core::model::MAX_POINTS];
        let script = "@list = plugin:reference:source\ndelete {\"ids\":\"@list.ids\"}";
        let (mut receiver, _) = external_reply_session(json!({"ids":ids}));
        receiver.execute("grid", &json!({"start":[0,0],"end":[0,4000]})).unwrap();
        receiver.script(script).unwrap();
        assert_eq!(receiver.execute("elements", &json!({"category":"grid"})).unwrap()["count"], 0);
    }
}
