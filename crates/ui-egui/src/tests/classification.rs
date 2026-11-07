//! Manage > Classification Manager, driven as a user does: a table loaded from a file, the system's parameters, a code
//! given to the selection by hand, and an agent's answer reviewed and applied.

use super::*;

const SS: &str = "Code,Group,Sub group,Section,Object,Title\n\
    Ss_25,25,,,,Wall and barrier systems\n\
    Ss_25_10,25,10,,,Framed wall systems\n\
    Ss_25_10_30,25,10,30,,Masonry wall systems\n\
    Ss_25_10_30_35,25,10,30,35,Brick and block wall systems\n\
    Ss_30,30,,,,Roof and floor systems\n";

fn answer(h: &mut Harness<BimApp>, path: &std::path::Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

fn type_value(h: &Harness<BimApp>, param: &str, id: Id) -> Option<Value> {
    let m = &h.state().session.model;
    let g = bimcraft_core::parameters::type_guid(m, m.elements.get(id)?)?;
    m.parameters.get(param)?.type_values.get(&g).cloned()
}

/// The window open on Uniclass with its Ss table and parameters loaded, as a user does it.
fn loaded() -> Harness<'static, BimApp> {
    let dir = tempfile::tempdir().unwrap().keep();
    let file = dir.join("Uniclass2015_Ss.csv");
    std::fs::write(&file, SS).unwrap();
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Classification Manager");
    assert!(h.query_by_label("Ss_25 — Systems").is_none());
    // SL, EF, Ss, Pr: the third row's.
    let load: Vec<_> = h.get_all_by_label("Load Table…").collect();
    assert_eq!(load.len(), 4, "a button for each of Uniclass's tables");
    load[2].click();
    h.run();
    answer(&mut h, &file);
    assert!(h.state().session.model.classification.tables.contains_key("Uniclass Ss"));
    assert!(h.query_by_label("5 codes loaded into Uniclass Ss.").is_some());
    press(&mut h, "Load Parameters");
    assert!(h.state().session.model.parameters.contains_key("Uniclass Ss Number"));
    h
}

#[test]
fn a_table_is_loaded_from_the_users_file_and_the_selection_is_given_a_code_by_hand() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.run();
    press(&mut h, "By Hand");
    assert!(h.query_by_label("0 of 1 classified").is_some());
    let code = "Ss_25_10_30_35  Brick and block wall systems";
    assert!(h.query_by_label(code).is_some(), "a wall's candidates");
    press(&mut h, code);
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")));
    assert_eq!(type_value(&h, "Uniclass Ss Title", wall), Some(json!("Brick and block wall systems")));
    assert!(h.query_by_label("1 of 1 classified").is_some());
}

#[test]
fn an_agents_answer_is_reviewed_and_only_what_is_ticked_is_written() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    let key = {
        let m = &h.state().session.model;
        bimcraft_core::parameters::type_guid(m, m.elements.get(wall).unwrap()).unwrap().to_string()
    };
    h.state_mut().agent.consented = true;
    h.state_mut().classification_answer(&json!({"rows": [
        {"key": key, "table": "Uniclass Ss", "code": "Ss_25_10_30_35", "confidence": 0.92, "reason": "Brick wall type"},
        {"key": key, "table": "Uniclass Ss", "code": "Ss_99", "confidence": 0.9, "reason": "invented"}]}));
    h.run();
    assert!(h.query_by_label("1 proposals").is_some());
    assert!(h.query_by_label("1 answers left out").is_some(), "the invented code");
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), None, "nothing written until Apply");
    press(&mut h, "Apply 1");
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")));
    h.state_mut().run("undo", json!({}));
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), None, "one undo step");
}

#[test]
fn the_agent_asks_before_it_sees_the_project_and_settings_keep_the_agent() {
    let mut h = loaded();
    press(&mut h, "AI Agent");
    assert!(h.query_by_label("I understand; let the agent see this project's types").is_some());
    assert!(h.query_by_label("Ask the Agent").is_none(), "not before the user agrees");
    press(&mut h, "I understand; let the agent see this project's types");
    assert!(h.query_by_label("Ask the Agent").is_some());
    let mut a = h.state().agent.clone();
    a.preset = bimcraft_agent::Preset::Ollama;
    a.model = "qwen2.5".into();
    let mut s = crate::settings::Settings::from_json(&json!({}), crate::settings::Settings::from_json(&json!({}), default_settings()));
    s.agent = a.clone();
    let back = crate::settings::Settings::from_json(&s.to_json(), default_settings());
    assert_eq!(back.agent, a, "kept in settings.json");
}

fn default_settings() -> crate::settings::Settings {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut app = BimApp::new(Session::new());
    app.use_settings(path);
    app.settings_kept.clone().unwrap()
}

#[test]
fn giving_codes_by_hand_again_and_again_keeps_the_windows_height() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.run();
    press(&mut h, "By Hand");
    let window = || egui::Id::new("classification-manager");
    let height = |h: &Harness<BimApp>| h.ctx.memory(|m| m.area_rect(window())).map(|r| r.height()).unwrap();
    let before = height(&h);
    for code in ["Ss_25_10_30_35  Brick and block wall systems", "Ss_25_10  Framed wall systems", "Ss_25  Wall and barrier systems"] {
        press(&mut h, code);
        for _ in 0..6 {
            h.step();
        }
    }
    let after = height(&h);
    assert!(after - before < 30.0, "a message line at most: {before} then {after}");
}

#[cfg(unix)]
#[test]
fn while_the_agent_works_the_window_counts_the_seconds_it_has_waited() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("slow-agent.sh");
    std::fs::write(&script, "#!/bin/sh\ncat > /dev/null\nsleep 30\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut h = loaded();
    {
        let a = &mut h.state_mut().agent;
        a.preset = bimcraft_agent::Preset::Custom;
        a.command = script.display().to_string();
        a.consented = true;
    }
    press(&mut h, "AI Agent");
    h.get_by_label("Ask the Agent").click();
    let waiting = |h: &Harness<BimApp>| h.query_by_label_contains("Batch 1 of 1 · waiting for the answer…").is_some();
    let mut seen = false;
    for _ in 0..40 {
        h.step();
        if waiting(&h) {
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(seen, "the batch and the seconds waited");
    h.get_by_label("Cancel").click();
    for _ in 0..40 {
        h.step();
        if h.state().classification.as_ref().is_some_and(|d| d.message.as_ref().is_some_and(|(m, _)| m == "cancelled")) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(h.state().classification.as_ref().and_then(|d| d.message.clone()).map(|m| m.0).as_deref(), Some("cancelled"));
}
