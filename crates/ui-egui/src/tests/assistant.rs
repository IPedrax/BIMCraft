//! The Assistant palette (AI) in both profiles: opened by its shortcut, the agreement before the first send, the key from the
//! environment or typed, a message that makes a wall as one undo step the Undo list names, what the settings file keeps of the key,
//! and that a window with no network (every test's, until it is given a fake) says so. No test reaches a provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_assistant::testing::Fake;
use serde_json::json;

use super::*;
use crate::dock::{Docking, Palette};

const KEY: &str = "sk-test-0123456789abcdef";
const WALL: &str = r#"{"command":"wall","params":{"points":[[0,0],[4000,0]]}}"#;

/// A window of one profile, built on a script.
type Build = fn(&str) -> Harness<'static, BimApp>;

const BOTH: [(&str, Build); 2] = [("Rev", app), ("BIMCraft", bimcraft)];

fn anthropic(content: serde_json::Value, stop: &str) -> String {
    json!({"type": "message", "role": "assistant", "content": content, "stop_reason": stop}).to_string()
}

fn wall_then_done() -> Fake {
    let call =
        json!([{"type": "tool_use", "id": "t1", "name": "execute", "input": serde_json::from_str::<serde_json::Value>(WALL).unwrap()}]);
    Fake::default()
        .reply(200, anthropic(call, "tool_use"))
        .reply(200, anthropic(json!([{"type": "text", "text": "Drew the wall."}]), "end_turn"))
}

/// A window with a fake network, no environment keys, and the palette open.
fn open(build: Build, net: Fake) -> (Harness<'static, BimApp>, Arc<Fake>) {
    let net = Arc::new(net);
    let mut h = build("");
    h.state_mut().assistant.attach(net.clone());
    h.state_mut().assistant.env = |_| None;
    shortcut(&mut h, "AI");
    (h, net)
}

/// A button of the palette, scrolled into view first: a short stack of palettes shows a part of it.
fn click_in_palette(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

fn write(h: &mut Harness<BimApp>, text: &str) {
    h.get_by_role(Role::MultilineTextInput).focus();
    h.run();
    h.get_by_role(Role::MultilineTextInput).type_text(text);
    h.run();
}

/// Frames until the chat is done, as the window's go while a reply is on its way.
fn finish(h: &mut Harness<BimApp>) {
    let until = Instant::now() + Duration::from_secs(20);
    while h.state().assistant.busy() {
        assert!(Instant::now() < until, "the chat did not finish: {:?}", h.state().assistant.log());
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.run();
}

#[test]
fn ai_opens_and_closes_the_palette_in_both_profiles_and_typing_it_in_the_box_does_not() {
    for (profile, build) in BOTH {
        let mut h = build("");
        assert!(!h.state().docking.shown(Palette::Assistant), "{profile}: closed until asked for");
        h.state_mut().assistant.attach(Arc::new(Fake::default()));
        shortcut(&mut h, "AI");
        assert!(h.state().docking.shown(Palette::Assistant), "{profile}");
        h.get_by_label("Provider");
        h.get_by_label("Model");
        // Typed in the message box, the letters are text, not a shortcut.
        write(&mut h, "AI");
        assert!(h.state().docking.shown(Palette::Assistant), "{profile}: still open");
        assert_eq!(h.state().assistant.input, "AI", "{profile}");
        h.state_mut().assistant.input.clear();
        h.run();
        click_in_palette(&mut h, "New chat");
        // Not while it is in a text field; a click on the canvas first would take the focus.
        click(&mut h, P2(-5000.0, -5000.0));
        shortcut(&mut h, "AI");
        assert!(!h.state().docking.shown(Palette::Assistant), "{profile}: closed again");
        let listed = crate::shortcuts().iter().any(|(keys, what)| *keys == "AI" && *what == "Assistant");
        assert!(listed, "{profile}: Keyboard Shortcuts lists it");
    }
}

#[test]
fn a_window_with_no_network_says_so_and_sends_nothing() {
    for (profile, build) in BOTH {
        let mut h = build("");
        shortcut(&mut h, "AI");
        h.get_by_label_contains("needs the network");
        assert!(h.query_by_label("Agree and send").is_none(), "{profile}");
    }
}

#[test]
fn the_first_send_is_agreed_to_and_a_message_makes_a_wall_in_one_undo_step() {
    for (profile, build) in BOTH {
        let (mut h, net) = open(build, wall_then_done());
        h.get_by_label_contains("sent to Anthropic Claude");
        type_into(&mut h, "API key:", KEY);
        write(&mut h, "draw a wall four metres long");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        assert_eq!(ids(&h, "wall").len(), 1, "{profile}");
        assert!(h.query_by_label_contains("sent to Anthropic Claude").is_none(), "{profile}: said once, not at every send");
        h.get_by_label("Send");
        assert!(!h.state().assistant.setup_open, "{profile}: the setup folds away once the chat is going");
        assert!(h.query_by_label("Provider").is_none(), "{profile}");
        h.get_by_label("Provider, model and key");
        let steps = h.state().session.steps(false);
        assert_eq!(steps.len(), 1, "{profile}: one step");
        assert_eq!(crate::step_name(steps[0]), "Assistant: draw a wall four metres long", "{profile}: Undo's list says where it came from");
        let log = h.state().assistant.log().to_vec();
        assert!(log.iter().any(|l| matches!(l, bimcraft_assistant::Line::Says(t) if t == "Drew the wall.")), "{profile}");
        assert!(
            log.iter().any(|l| matches!(l, bimcraft_assistant::Line::Used { name, error: false, .. } if name == "execute")),
            "{profile}"
        );
        assert!(!format!("{log:?}").contains(KEY), "{profile}");
        let sent = net.seen();
        assert_eq!(sent.len(), 2, "{profile}");
        assert!(sent[0].headers.iter().any(|(k, v)| k == "x-api-key" && v == KEY), "{profile}");
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        h.run();
        assert!(ids(&h, "wall").is_empty(), "{profile}: Undo takes it back");
    }
}

#[test]
fn ctrl_enter_sends_the_message_as_the_button_does() {
    let (mut h, net) = open(app, wall_then_done());
    type_into(&mut h, "API key:", KEY);
    write(&mut h, "a wall");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    finish(&mut h);
    assert_eq!(net.seen().len(), 2);
    assert_eq!(ids(&h, "wall").len(), 1);
}

#[test]
fn the_key_in_the_environment_comes_before_the_one_typed_and_is_not_asked_for() {
    let reply =
        json!({"status": "completed", "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hi."}]}]}).to_string();
    for (profile, build) in BOTH {
        let (mut h, net) = open(build, Fake::default().reply(200, reply.clone()));
        h.state_mut().assistant.provider = bimcraft_assistant::Provider::OpenAi;
        h.state_mut().assistant.model = "gpt-6-astra".into();
        h.state_mut().assistant.env = |name| (name == "OPENAI_API_KEY").then(|| KEY.to_string());
        h.run();
        h.get_by_label_contains("Using the key in OPENAI_API_KEY");
        assert!(h.query_by_label("API key:").is_none(), "{profile}");
        write(&mut h, "hello");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        let sent = net.seen();
        assert_eq!(sent[0].url, "https://api.openai.com/v1/responses", "{profile}");
        assert!(sent[0].headers.contains(&("Authorization".into(), format!("Bearer {KEY}"))), "{profile}");
    }
}

#[test]
fn a_send_with_no_key_or_a_bad_model_says_what_is_missing_and_goes_nowhere() {
    for (profile, build) in BOTH {
        let (mut h, net) = open(build, Fake::default());
        write(&mut h, "hello");
        click_in_palette(&mut h, "Agree and send");
        assert!(
            matches!(h.state().assistant.log().last(), Some(bimcraft_assistant::Line::Note(n)) if n.contains("API key")),
            "{profile}: {:?} input {:?}",
            h.state().assistant.log(),
            h.state().assistant.input
        );
        assert_eq!(h.state().assistant.input, "hello", "{profile}: the message is still there to send");
        type_into(&mut h, "API key:", KEY);
        type_into(&mut h, "Model", "not a model!");
        click_in_palette(&mut h, "Agree and send");
        assert!(matches!(h.state().assistant.log().last(), Some(bimcraft_assistant::Line::Note(n)) if n.contains("model id")), "{profile}");
        assert!(net.seen().is_empty(), "{profile}");
    }
}

#[test]
fn a_window_that_lets_agents_only_read_lets_the_assistant_only_read() {
    let (mut h, _net) = open(app, wall_then_done());
    h.state_mut().agents.edit = false;
    type_into(&mut h, "API key:", KEY);
    write(&mut h, "a wall");
    click_in_palette(&mut h, "Agree and send");
    finish(&mut h);
    assert!(ids(&h, "wall").is_empty());
    assert!(h.state().assistant.log().iter().any(|l| matches!(l, bimcraft_assistant::Line::Used { error: true, .. })));
}

fn written(file: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

#[test]
fn the_key_is_written_to_the_settings_only_when_remembered_and_read_back_with_care() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    h.state_mut().assistant.key = crate::assistant::Secret(KEY.into());
    h.state_mut().assistant.provider = bimcraft_assistant::Provider::Gemini;
    h.state_mut().assistant.model = "gemini-3.8-flash".into();
    h.run();
    let kept = written(&file);
    assert_eq!(kept["assistant"], json!({"provider": "gemini", "model": "gemini-3.8-flash", "remember": false, "key": ""}));
    assert!(!std::fs::read_to_string(&file).unwrap().contains(KEY), "typed, not remembered: nowhere on disk");
    h.state_mut().assistant.remember = true;
    h.run();
    assert_eq!(written(&file)["assistant"]["key"], KEY);
    // The next window has it all, and the key does not show in a debug print of the settings.
    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().assistant.key.0, KEY);
    assert_eq!(again.state().assistant.provider, bimcraft_assistant::Provider::Gemini);
    assert!(!format!("{:?}", crate::assistant::Secret(KEY.into())).contains(KEY), "a key prints as nothing");
    // A file edited by hand or damaged: what is no provider, model or key is left out.
    for bad in [
        json!({"assistant": {"provider": "nope", "model": "bad model!", "remember": true, "key": "short"}}),
        json!({"assistant": {"provider": 5, "model": null, "remember": "yes", "key": 7}}),
        json!({"assistant": []}),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        let mut h = app("");
        h.state_mut().use_settings(file.clone());
        let a = &h.state().assistant;
        assert_eq!((a.provider, a.model.as_str()), (bimcraft_assistant::Provider::Anthropic, "claude-sonnet-5-5"), "{bad}");
        assert!(a.key.0.is_empty(), "{bad}");
    }
}

#[test]
fn a_settings_file_from_before_the_assistant_leaves_it_closed() {
    let old = json!({"left": [{"tabs": ["Properties"], "active": 0}], "right": [{"tabs": ["Project Browser"], "active": 0}],
        "bottom": [], "floats": [], "closed": ["Tools"], "home": {}});
    let d = Docking::from_json(&old);
    assert!(!d.shown(Palette::Assistant) && d.shown(Palette::Properties));
    let mut d = d;
    d.show(Palette::Assistant, true);
    assert_eq!(d.docked(Palette::Assistant), Some(crate::dock::Side::Right), "it opens on the Browser's side");
    assert!(
        d.right.iter().any(|s| s.tabs.contains(&Palette::Assistant) && s.tabs.contains(&Palette::Browser)),
        "as a tab of its stack, which has the height"
    );
}
