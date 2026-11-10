//! The Assistant palette (AI) in both profiles: opened by its shortcut, the agreement before the first send, the key from the
//! environment or typed, a message that makes a wall as one undo step the Undo list names, what the settings file keeps of the key,
//! and that a window with no network (every test's, until it is given a fake) says so. No test reaches a provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_assistant::net::Net;
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
    walls_then_done(1)
}

/// `n` messages' worth of replies, each a wall and then a word.
fn walls_then_done(n: usize) -> Fake {
    let call =
        json!([{"type": "tool_use", "id": "t1", "name": "execute", "input": serde_json::from_str::<serde_json::Value>(WALL).unwrap()}]);
    (0..n).fold(Fake::default(), |net, _| {
        net.reply(200, anthropic(call.clone(), "tool_use"))
            .reply(200, anthropic(json!([{"type": "text", "text": "Drew the wall."}]), "end_turn"))
    })
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

/// File > Settings > Assistant, open.
fn settings(h: &mut Harness<BimApp>) {
    h.state_mut().settings_page = Some(crate::settings::Page::Assistant);
    h.run();
}

fn close_settings(h: &mut Harness<BimApp>) {
    h.state_mut().settings_page = None;
    h.run();
}

/// The key typed where the user types it: on the Assistant page of Settings.
fn set_key(h: &mut Harness<BimApp>, key: &str) {
    settings(h);
    type_into(h, "API key:", key);
    close_settings(h);
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
        h.get_by_label("Assistant settings");
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
        set_key(&mut h, KEY);
        write(&mut h, "draw a wall four metres long");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        assert_eq!(ids(&h, "wall").len(), 1, "{profile}");
        assert!(h.query_by_label_contains("sent to Anthropic Claude").is_none(), "{profile}: said once, not at every send");
        h.get_by_label("Send");
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
    set_key(&mut h, KEY);
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
        settings(&mut h);
        h.get_by_label_contains("Using the key in OPENAI_API_KEY");
        assert!(h.query_by_label("API key:").is_none(), "{profile}");
        close_settings(&mut h);
        assert!(h.query_by_label_contains("No API key yet").is_none(), "{profile}: the palette knows it has one");
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
            matches!(h.state().assistant.log().last(), Some(bimcraft_assistant::Line::Note(n)) if n.contains("API key") && n.contains("Assistant settings")),
            "{profile}: {:?} input {:?}",
            h.state().assistant.log(),
            h.state().assistant.input
        );
        assert_eq!(h.state().assistant.input, "hello", "{profile}: the message is still there to send");
        settings(&mut h);
        type_into(&mut h, "API key:", KEY);
        type_into(&mut h, "Model", "not a model!");
        close_settings(&mut h);
        click_in_palette(&mut h, "Agree and send");
        assert!(matches!(h.state().assistant.log().last(), Some(bimcraft_assistant::Line::Note(n)) if n.contains("model id")), "{profile}");
        assert!(net.seen().is_empty(), "{profile}");
    }
}

#[test]
fn a_window_that_lets_agents_only_read_lets_the_assistant_only_read() {
    let (mut h, _net) = open(app, wall_then_done());
    h.state_mut().agents.edit = false;
    set_key(&mut h, KEY);
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

#[test]
fn a_new_project_keeps_the_assistants_network_and_choices_and_forgets_the_conversation() {
    for (profile, build) in BOTH {
        let (mut h, net) = open(build, walls_then_done(2));
        set_key(&mut h, KEY);
        h.state_mut().assistant.provider = bimcraft_assistant::Provider::Anthropic;
        h.state_mut().assistant.model = "claude-opus-5-5".into();
        write(&mut h, "a wall");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        assert_eq!(ids(&h, "wall").len(), 1, "{profile}");
        // File > New, as the window does it: another project in the session's place, and the window starts over.
        h.state_mut().session.execute("new", &json!({})).unwrap();
        h.run();
        assert!(h.query_by_label_contains("needs the network").is_none(), "{profile}: the window still has its network");
        let a = &h.state().assistant;
        assert_eq!((a.key.0.as_str(), a.model.as_str()), (KEY, "claude-opus-5-5"), "{profile}: what the user chose stays");
        assert!(a.log().is_empty() && !a.busy(), "{profile}: the conversation was about the other project");
        assert!(h.state().docking.shown(Palette::Assistant), "{profile}");
        h.get_by_label("Send");
        assert!(h.query_by_label("Agree and send").is_none(), "{profile}: agreed already, once, this session");
        // And it works on the new project, which has none of the old one's walls.
        assert!(ids(&h, "wall").is_empty(), "{profile}");
        write(&mut h, "another wall");
        click_in_palette(&mut h, "Send");
        finish(&mut h);
        assert_eq!(ids(&h, "wall").len(), 1, "{profile}");
        assert_eq!(h.state().session.history(), vec!["Assistant: another wall"], "{profile}");
        assert_eq!(net.seen().len(), 4, "{profile}");
    }
}

/// A network that holds each request until the test lets it through, so a reply arrives exactly when the test says.
struct Gated {
    open: std::sync::atomic::AtomicBool,
    inner: Arc<Fake>,
}

impl Net for Gated {
    fn post(&self, request: &bimcraft_assistant::net::Request) -> Result<(u16, String), String> {
        while !self.open.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.inner.post(request)
    }
}

#[test]
fn a_reply_that_is_waiting_as_the_project_is_replaced_does_nothing_to_the_new_one() {
    let fake = Arc::new(wall_then_done());
    let gate = Arc::new(Gated { open: false.into(), inner: fake.clone() });
    let mut h = app("");
    h.state_mut().assistant.attach(gate.clone());
    h.state_mut().assistant.env = |_| None;
    shortcut(&mut h, "AI");
    set_key(&mut h, KEY);
    write(&mut h, "a wall");
    click_in_palette(&mut h, "Agree and send");
    assert!(h.state().assistant.busy());
    // The reply comes in, and no frame takes it: the next one would run its tool call on whichever project the session holds then.
    gate.open.store(true, std::sync::atomic::Ordering::SeqCst);
    let until = Instant::now() + Duration::from_secs(20);
    while fake.seen().is_empty() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(150));
    h.state_mut().session.execute("new", &json!({})).unwrap();
    h.run();
    std::thread::sleep(Duration::from_millis(100));
    h.run();
    assert!(ids(&h, "wall").is_empty(), "the model's wall was meant for the other project");
    assert!(!h.state().assistant.busy());
    assert!(h.state().assistant.log().is_empty());
    assert_eq!(fake.seen().len(), 1, "and it was not asked again");
    assert!(h.state().session.history().is_empty(), "nor is there an undo step for it");
}

#[test]
fn the_api_config_has_its_own_tab_in_settings_in_both_profiles() {
    for (profile, build) in BOTH {
        let mut h = build("");
        h.state_mut().assistant.attach(Arc::new(Fake::default()));
        h.state_mut().assistant.env = |_| None;
        // The page is picked from the list as a user picks it: the leftmost of the names (the User Interface page has a box for the
        // palette of the same name).
        h.state_mut().settings_page = Some(crate::settings::Page::Interface);
        h.run();
        let entry = h.get_all_by_label("Assistant").min_by(|a, b| a.rect().left().total_cmp(&b.rect().left())).map(|n| n.rect().center());
        click_at(&mut h, entry.unwrap());
        assert_eq!(h.state().settings_page, Some(crate::settings::Page::Assistant), "{profile}");
        h.get_by_label("Provider");
        h.get_by_label("Model");
        h.get_by_label_contains("only when you send a message");
        // The provider, with the model that goes with it.
        h.state_mut().assistant.provider = bimcraft_assistant::Provider::Gemini;
        h.state_mut().assistant.model = "gemini-3.8-flash".into();
        type_into(&mut h, "Model", "gemini-3.5-flash");
        assert_eq!(h.state().assistant.model, "gemini-3.5-flash", "{profile}");
        type_into(&mut h, "API key:", KEY);
        assert_eq!(h.state().assistant.key.0, KEY, "{profile}");
        assert!(h.query_by_label_contains("plain text in settings.json").is_none(), "{profile}: said only when it is true");
        h.get_by_role_and_label(Role::CheckBox, "Remember the key").click();
        h.run();
        assert!(h.state().assistant.remember, "{profile}");
        h.get_by_label_contains("plain text in settings.json");
    }
}

#[test]
fn the_palette_points_at_the_settings_and_says_when_there_is_no_key() {
    for (profile, build) in BOTH {
        let (mut h, _net) = open(build, Fake::default());
        h.get_by_label_contains("Anthropic Claude · claude-sonnet-5-5");
        h.get_by_label_contains("No API key yet");
        assert!(h.query_by_label("API key:").is_none(), "{profile}: the key is not typed in the palette");
        click_in_palette(&mut h, "Assistant settings");
        assert_eq!(h.state().settings_page, Some(crate::settings::Page::Assistant), "{profile}");
        h.get_by_label("API key:");
        close_settings(&mut h);
        set_key(&mut h, KEY);
        assert!(h.query_by_label_contains("No API key yet").is_none(), "{profile}");
    }
}
