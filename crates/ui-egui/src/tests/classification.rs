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
    // A row to click: the pointing hand over it.
    let at = h.get_by_label(code).rect().center();
    h.hover_at(at);
    h.run();
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand);
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
    // Unticked, Select All above the Apply column ticks it again.
    h.state_mut().classification.as_mut().unwrap().review[0].accept = false;
    h.run();
    assert!(h.query_by_label("Apply 0").is_some());
    press(&mut h, "Select All");
    assert!(h.query_by_label("Apply 1").is_some());
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
