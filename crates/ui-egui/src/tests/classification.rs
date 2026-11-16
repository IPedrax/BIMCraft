//! Manage > Classification Manager, driven as a user does: a table loaded from a file, the system's parameters, a code
//! given to the selection by hand, and an answer reviewed and applied: pasted, or asked of the AI assistant's model on a fake
//! network (no test reaches a provider).

use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_assistant::net::Net;
use bimcraft_assistant::testing::Fake;

use super::*;

const KEY: &str = "sk-test-0123456789abcdef";

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
    // Settings is full at this width: the tool is in its list.
    from_list(&mut h, "Settings", "Classification Manager");
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

/// The type GUID of element `id`: the key a brief's row has.
fn type_key(h: &Harness<BimApp>, id: Id) -> String {
    let m = &h.state().session.model;
    bimcraft_core::parameters::type_guid(m, m.elements.get(id).unwrap()).unwrap().to_string()
}

/// Anthropic's reply with `text` as the model's words, and the tokens it counted.
fn says(text: &str) -> String {
    json!({"type": "message", "role": "assistant", "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 1200, "output_tokens": 80}})
    .to_string()
}

/// The assistant given `net`, the user's key typed (none in the environment), and the AI Agent tab open.
fn ready(h: &mut Harness<BimApp>, net: Arc<dyn Net>) {
    h.state_mut().assistant.attach(net);
    h.state_mut().assistant.env = |_| None;
    h.state_mut().assistant.key = crate::assistant::Secret(KEY.into());
    press(h, "AI Agent");
}

fn busy(h: &Harness<BimApp>) -> bool {
    h.state().classification.as_ref().is_some_and(crate::classification::ClassificationManager::busy)
}

/// Clicks `label`, then frames until the model has answered: the window spins meanwhile, so it never settles to `run`.
fn ask(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click();
    h.step();
    let until = Instant::now() + Duration::from_secs(20);
    while busy(h) {
        assert!(Instant::now() < until, "the model was not done asking");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.run();
}

#[test]
fn the_assistants_model_proposes_codes_on_the_users_key_and_only_what_is_ticked_is_written() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    let key = type_key(&h, wall);
    let answer = json!({"rows": [{"key": key, "table": "Uniclass Ss", "code": "Ss_25_10_30_35", "confidence": 0.92, "reason": "Brick"}]});
    let net = Arc::new(Fake::default().reply(200, says(&format!("```json\n{answer}\n```"))));
    ready(&mut h, net.clone());
    let said = "sends what it classifies to Anthropic Claude, on your API key";
    assert!(h.query_by_label_contains(said).is_some(), "what leaves the machine and for whom, before the first request");
    assert!(h.query_by_label("Ask the Agent").is_none(), "the first one is agreed to");
    ask(&mut h, "Agree and Ask the Agent");
    let seen = net.seen();
    assert_eq!(seen.len(), 1, "one batch, one request");
    assert!(seen[0].url.starts_with("https://api.anthropic.com/"), "the provider of the Assistant settings");
    assert!(seen[0].headers.iter().any(|(name, value)| name == "x-api-key" && value == KEY), "on the user's own key");
    assert!(seen[0].body.contains(&key) && seen[0].body.contains("Uniclass Ss"), "the brief went with it");
    assert!(h.query_by_label("1 proposals").is_some());
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), None, "nothing written until Apply");
    assert_eq!(h.state().assistant.usage.get("").map(|u| u.total()), Some(1280), "its tokens counted as the chat's are");
    press(&mut h, "Apply 1");
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")));
    // Agreed once for the session: the next batch is asked with no second agreement.
    assert!(h.query_by_label("Ask the Agent").is_some());
    assert!(h.query_by_label_contains(said).is_none());
}

#[test]
fn an_answer_that_is_not_one_is_refused_and_the_project_is_untouched() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    let invented =
        json!({"rows": [{"key": type_key(&h, wall), "table": "Uniclass Ss", "code": "Ss_99", "confidence": 0.9, "reason": "?"}]});
    let net = Fake::default()
        .reply(200, says("It looks like a brick wall to me."))
        .reply(200, says(&invented.to_string()))
        .reply(200, "<html>a proxy's page</html>");
    ready(&mut h, Arc::new(net));
    let before = Arc::clone(&h.state().session.model);
    ask(&mut h, "Agree and Ask the Agent");
    assert!(h.query_by_label("The agent gave no answer.").is_some(), "words with no JSON in them");
    ask(&mut h, "Ask the Agent");
    assert!(h.query_by_label_contains("1 answers left out: row 1: `Ss_99` isn't in Uniclass Ss").is_some(), "a code the table hasn't");
    ask(&mut h, "Ask the Agent");
    assert!(h.query_by_label_contains("Anthropic sent a reply that is not JSON").is_some(), "a reply that is not the provider's");
    assert!(h.state().classification.as_ref().unwrap().review.is_empty());
    assert!(*before == *h.state().session.model, "none of it touched the project");
}

#[test]
fn cancel_gives_up_on_the_request_and_its_answer_goes_nowhere() {
    let mut h = loaded();
    let wall = ids(&h, "wall")[0];
    let answer =
        json!({"rows": [{"key": type_key(&h, wall), "table": "Uniclass Ss", "code": "Ss_25", "confidence": 0.9, "reason": "Wall"}]});
    let fake = Arc::new(Fake::default().reply(200, says(&answer.to_string())));
    let gate = Arc::new(super::assistant::Gated { open: false.into(), inner: fake.clone() });
    ready(&mut h, gate.clone());
    h.get_by_label("Agree and Ask the Agent").click();
    h.step();
    h.step();
    assert!(busy(&h), "asking");
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert!(!busy(&h), "Cancel doesn't wait for the reply");
    h.run();
    assert!(h.query_by_label("Cancelled.").is_some());
    // The reply comes in after all: nothing takes it.
    gate.open.store(true, std::sync::atomic::Ordering::SeqCst);
    let until = Instant::now() + Duration::from_secs(20);
    while fake.seen().is_empty() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(150));
    h.run();
    assert!(h.state().classification.as_ref().unwrap().review.is_empty(), "its answer goes nowhere");
    assert_eq!(type_value(&h, "Uniclass Ss Number", wall), None);
    assert!(h.query_by_label("Ask the Agent").is_some(), "and the window is ready to ask again");
}

#[test]
fn with_no_key_the_ai_button_says_so_and_offers_the_assistant_settings() {
    let mut h = loaded();
    let net = Arc::new(Fake::default());
    h.state_mut().assistant.attach(net.clone());
    h.state_mut().assistant.env = |_| None;
    press(&mut h, "AI Agent");
    assert!(h.query_by_label("No API key yet. Add one in the Assistant settings.").is_some());
    assert!(h.query_by_label("Agree and Ask the Agent").is_none() && h.query_by_label("Ask the Agent").is_none());
    assert!(h.query_by_label("Copy Brief").is_some(), "a brief still goes by hand");
    press(&mut h, "Assistant settings");
    assert_eq!(h.state().settings_page, Some(crate::settings::Page::Assistant));
    assert!(net.seen().is_empty(), "nothing was sent");
    // A window started without the network (every test's, until it is given a fake) says so instead.
    let mut bare = loaded();
    press(&mut bare, "AI Agent");
    assert!(bare.query_by_label("The assistant needs the network, which this window was started without.").is_some());
    assert!(bare.query_by_label("Agree and Ask the Agent").is_none());
}

#[test]
fn ci_opens_the_classification_manager_and_ks_lists_it() {
    let mut h = app(BOX);
    shortcut(&mut h, "CI");
    assert!(h.state().classification.is_some(), "CI opens Manage > Classification Manager, as its tooltip says");
    assert!(crate::shortcuts().contains(&("CI", "Classification Manager")), "and Keyboard Shortcuts (KS) lists it");
}
