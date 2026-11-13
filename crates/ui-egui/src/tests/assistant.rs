//! The Assistant palette (AI) in both profiles: opened by its shortcut, the agreement before the first send, the key from the
//! environment or typed, a message that makes a wall as one undo step the Undo list names, what the settings file keeps of the key,
//! and that a window with no network (every test's, until it is given a fake) says so. No test reaches a provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_assistant::net::Net;
use bimcraft_assistant::testing::Fake;
use egui_kittest::kittest::NodeT;
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

/// Remember, ticked on the Assistant page of Settings, as a user ticks it.
fn tick_remember(h: &mut Harness<BimApp>) {
    settings(h);
    h.get_by_role_and_label(Role::CheckBox, "Remember the key").click();
    h.run();
}

fn hex_of(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_typed_key_is_written_nowhere_and_a_remembered_one_only_scrambled() {
    for (profile, build) in BOTH {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        h.state_mut().assistant.provider = bimcraft_assistant::Provider::Gemini;
        h.state_mut().assistant.model = "gemini-3.8-flash".into();
        set_key(&mut h, KEY);
        assert_eq!(
            written(&file)["assistant"],
            json!({"provider": "gemini", "model": "gemini-3.8-flash", "key_sealed": null}),
            "{profile}"
        );
        assert!(!std::fs::read_to_string(&file).unwrap().contains(KEY), "{profile}: typed, not remembered: nowhere on disk");

        // Remember: what the file gets is the key scrambled, and not the key as typed, nor in hex.
        assert!(h.query_by_label_contains("built into BIMCraft").is_none(), "{profile}: said when it applies");
        tick_remember(&mut h);
        h.get_by_label_contains("anyone who has BIMCraft can unscramble it");
        let text = std::fs::read_to_string(&file).unwrap();
        let sealed = written(&file)["assistant"]["key_sealed"].as_str().unwrap().to_string();
        assert!(bimcraft_assistant::seal::well_formed(&sealed), "{profile}: {sealed}");
        assert!(!text.contains(KEY) && !text.contains(&hex_of(KEY)), "{profile}: not as typed, nor as hex");
        assert_eq!(bimcraft_assistant::seal::open(&sealed).unwrap(), KEY, "{profile}");
        close_settings(&mut h);

        // The next window opens it at the start, with the choices, and sends with it.
        let net = Arc::new(Fake::default().reply(200, anthropic(json!([{"type": "text", "text": "Hi."}]), "end_turn")));
        let mut next = build("");
        next.state_mut().assistant.attach(net.clone());
        next.state_mut().assistant.env = |_| None;
        next.state_mut().use_settings(file.clone());
        next.run();
        let a = &next.state().assistant;
        assert_eq!(
            (a.provider, a.model.as_str(), a.key.0.as_str(), a.remember),
            (bimcraft_assistant::Provider::Gemini, "gemini-3.8-flash", KEY, true),
            "{profile}"
        );
        next.state_mut().assistant.provider = bimcraft_assistant::Provider::Anthropic;
        shortcut(&mut next, "AI");
        assert!(next.query_by_label_contains("No API key yet").is_none(), "{profile}");
        write(&mut next, "hello");
        click_in_palette(&mut next, "Agree and send");
        finish(&mut next);
        assert!(net.seen()[0].headers.contains(&("x-api-key".into(), KEY.into())), "{profile}: sent with the key that came back");

        // Edited, the key is kept as edited; not remembering takes it out of the file at once.
        settings(&mut next);
        type_into(&mut next, "API key:", "sk-test-another-key-0123456789");
        let kept = written(&file)["assistant"]["key_sealed"].as_str().map(str::to_string);
        assert_eq!(bimcraft_assistant::seal::open(&kept.unwrap()).unwrap(), "sk-test-another-key-0123456789", "{profile}");
        next.get_by_role_and_label(Role::CheckBox, "Remember the key").click();
        next.run();
        assert_eq!(written(&file)["assistant"]["key_sealed"], json!(null), "{profile}");
        assert!(!std::fs::read_to_string(&file).unwrap().contains("bc1:"), "{profile}");
        assert_eq!(next.state().assistant.key.0, "sk-test-another-key-0123456789", "{profile}: this session's key is still in use");
    }
}

#[test]
fn remember_keeps_what_is_in_the_key_box_and_nothing_when_there_is_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let (mut h, _net) = open(app, Fake::default());
    h.state_mut().use_settings(file.clone());
    tick_remember(&mut h);
    assert!(!file.exists(), "no key to keep yet: nothing the settings did not already hold");
    type_into(&mut h, "API key:", KEY);
    assert!(written(&file)["assistant"]["key_sealed"].is_string());
    // Emptied as a user empties it: all of it selected, and deleted.
    h.get_by_label("API key:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    key(&mut h, Key::Backspace);
    assert_eq!(written(&file)["assistant"]["key_sealed"], json!(null), "a box emptied keeps nothing");
}

#[test]
fn a_saved_key_this_build_cannot_open_or_a_damaged_or_plain_one_is_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let sealed = bimcraft_assistant::seal::seal(KEY).unwrap();
    let another = bimcraft_assistant::seal::seal_with("another build's secret", KEY, &[1; 16], &[2; 12], 1000).unwrap();
    // A good one comes back; one another kind of build sealed does not open here, and is dropped, not shown as a key.
    for (key_sealed, opens) in [(sealed.clone(), true), (another, false)] {
        std::fs::write(&file, json!({"assistant": {"provider": "openai", "model": "gpt-6-astra", "key_sealed": key_sealed}}).to_string())
            .unwrap();
        let mut h = app("");
        h.state_mut().use_settings(file.clone());
        let a = &h.state().assistant;
        assert_eq!((a.key.0 == KEY, a.remember, a.sealed.is_some()), (opens, opens, opens));
        assert_eq!(a.provider, bimcraft_assistant::Provider::OpenAi);
    }
    // Files edited by hand, damaged, or from a build that kept the key as typed: what is no sealed key is none, and a plain key is not read.
    for bad in [
        json!({"assistant": {"provider": "nope", "model": "bad model!", "key_sealed": "short"}}),
        json!({"assistant": {"provider": 5, "model": null, "key_sealed": 7}}),
        json!({"assistant": {"remember": true, "key": KEY}}),
        json!({"assistant": {"key_sealed": format!("{sealed}0")}}),
        json!({"assistant": []}),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        let mut h = app("");
        h.state_mut().use_settings(file.clone());
        let a = &h.state().assistant;
        assert_eq!((a.provider, a.model.as_str()), (bimcraft_assistant::Provider::Anthropic, "claude-sonnet-5-5"), "{bad}");
        assert!(a.key.0.is_empty() && a.sealed.is_none() && !a.remember, "{bad}");
    }
    assert!(!format!("{:?}", crate::assistant::Secret(KEY.into())).contains(KEY), "a key prints as nothing");
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
        assert!(h.query_by_label_contains("built into BIMCraft").is_none(), "{profile}: said only when Remember is ticked");
        h.get_by_role_and_label(Role::CheckBox, "Remember the key").click();
        h.run();
        assert!(h.state().assistant.remember, "{profile}");
        h.get_by_label_contains("built into BIMCraft");
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

const PASSPHRASE: &str = "correct horse battery";

/// A check box of the Assistant page, ticked or unticked as a user does.
fn tick(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::CheckBox, label).click();
    h.run();
}

#[test]
fn a_key_protected_with_a_passphrase_is_saved_encrypted_and_comes_back_locked() {
    for (profile, build) in BOTH {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        set_key(&mut h, KEY);
        tick_remember(&mut h);
        assert!(
            !bimcraft_assistant::seal::needs_passphrase(written(&file)["assistant"]["key_sealed"].as_str().unwrap()),
            "{profile}: scrambled first"
        );
        assert!(h.query_by_label("Passphrase:").is_none(), "{profile}: asked for only when the user wants one");
        tick(&mut h, "Also protect it with a passphrase");
        h.get_by_label("Passphrase:");
        h.get_by_label("Not saved with the passphrase yet.");
        assert!(h.get_by_label("Save the key").accesskit_node().is_disabled(), "{profile}: no passphrase yet");
        type_into(&mut h, "Passphrase:", "short");
        assert!(h.get_by_label("Save the key").accesskit_node().is_disabled(), "{profile}: at least 8 characters");
        type_into(&mut h, "Passphrase:", PASSPHRASE);
        press(&mut h, "Save the key");
        h.get_by_label("Saved, encrypted with the passphrase.");
        let text = std::fs::read_to_string(&file).unwrap();
        let sealed = written(&file)["assistant"]["key_sealed"].as_str().unwrap().to_string();
        assert!(bimcraft_assistant::seal::needs_passphrase(&sealed), "{profile}: {sealed}");
        assert!(
            !text.contains(KEY) && !text.contains(&hex_of(KEY)) && !text.contains(PASSPHRASE),
            "{profile}: not as typed, nor the passphrase"
        );
        assert!(h.state().assistant.passphrase.0.is_empty(), "{profile}: the passphrase is not kept");
        close_settings(&mut h);

        // The next window has the choices and a key that is locked: nothing to send with until the passphrase opens it.
        let net = Arc::new(Fake::default().reply(200, anthropic(json!([{"type": "text", "text": "Hi."}]), "end_turn")));
        let mut next = build("");
        next.state_mut().assistant.attach(net.clone());
        next.state_mut().assistant.env = |_| None;
        next.state_mut().use_settings(file.clone());
        next.run();
        let a = &next.state().assistant;
        assert!(a.key.0.is_empty() && a.remember && a.protect, "{profile}");
        shortcut(&mut next, "AI");
        next.get_by_label_contains("saved API key is locked");
        write(&mut next, "hello");
        click_in_palette(&mut next, "Agree and send");
        assert!(
            matches!(next.state().assistant.log().last(), Some(bimcraft_assistant::Line::Note(n)) if n.contains("locked")),
            "{profile}"
        );
        assert!(net.seen().is_empty(), "{profile}: nothing was sent");

        // A wrong passphrase says so and keeps it locked; the right one opens it for the session.
        settings(&mut next);
        next.get_by_label_contains("A saved key is locked");
        type_into(&mut next, "Passphrase:", "not the passphrase");
        press(&mut next, "Unlock");
        next.get_by_label_contains("Wrong passphrase");
        assert!(next.state().assistant.key.0.is_empty() && next.state().assistant.passphrase.0.is_empty(), "{profile}");
        type_into(&mut next, "Passphrase:", PASSPHRASE);
        press(&mut next, "Unlock");
        assert_eq!(next.state().assistant.key.0, KEY, "{profile}");
        close_settings(&mut next);
        // The refusal from before is in the conversation, as history; cleared, nothing in the palette says locked.
        click_in_palette(&mut next, "New chat");
        assert!(next.query_by_label_contains("saved API key is locked").is_none(), "{profile}");
        write(&mut next, "hello again");
        // The refused send did not count as the agreement: the first message that goes is the one that is agreed to.
        click_in_palette(&mut next, "Agree and send");
        finish(&mut next);
        assert!(net.seen()[0].headers.contains(&("x-api-key".into(), KEY.into())), "{profile}: sent with the key it unlocked");

        // No passphrase any more: the key goes back to being scrambled, and then to not being kept.
        settings(&mut next);
        tick(&mut next, "Also protect it with a passphrase");
        let back = written(&file)["assistant"]["key_sealed"].as_str().unwrap().to_string();
        assert!(!bimcraft_assistant::seal::needs_passphrase(&back), "{profile}");
        assert_eq!(bimcraft_assistant::seal::open(&back).unwrap(), KEY, "{profile}");
        tick(&mut next, "Remember the key");
        assert_eq!(written(&file)["assistant"]["key_sealed"], json!(null), "{profile}");
        assert_eq!(next.state().assistant.key.0, KEY, "{profile}: this session's key is still in use");
    }
}

#[test]
fn a_locked_key_may_be_forgotten_and_one_under_a_passphrase_opens_in_any_build() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    // Sealed by a passphrase alone, so no build's own secret is part of it: it opens here, whichever build made it.
    let sealed = bimcraft_assistant::seal::seal_passphrase(PASSPHRASE, KEY).unwrap();
    std::fs::write(&file, json!({"assistant": {"provider": "openai", "model": "gpt-6-astra", "key_sealed": sealed}}).to_string()).unwrap();
    let mut h = app("");
    h.state_mut().assistant.attach(Arc::new(Fake::default()));
    h.state_mut().use_settings(file.clone());
    assert!(h.state().assistant.key.0.is_empty() && h.state().assistant.remember && h.state().assistant.protect);
    settings(&mut h);
    type_into(&mut h, "Passphrase:", PASSPHRASE);
    press(&mut h, "Unlock");
    assert_eq!(h.state().assistant.key.0, KEY);
    // Locked again in the next window, and forgotten there: the file keeps nothing.
    let mut again = app("");
    again.state_mut().assistant.attach(Arc::new(Fake::default()));
    again.state_mut().use_settings(file.clone());
    settings(&mut again);
    press(&mut again, "Forget the saved key");
    assert!(again.state().assistant.sealed.is_none() && !again.state().assistant.remember && !again.state().assistant.protect);
    assert_eq!(written(&file)["assistant"]["key_sealed"], json!(null));
}

// ---------- the redesigned palette: thinking, checklist, files, tokens ----------

fn reply_with(content: serde_json::Value, stop: &str, input: u64, output: u64) -> String {
    json!({"type": "message", "role": "assistant", "content": content, "stop_reason": stop,
        "usage": {"input_tokens": input, "output_tokens": output}})
    .to_string()
}

#[test]
fn the_checklist_the_model_writes_is_drawn_step_by_step_and_a_new_message_clears_it() {
    let list = json!({"todos": [
        {"content": "Draw the walls", "status": "completed"},
        {"content": "Place the doors", "status": "in_progress"},
        {"content": "Name the rooms", "status": "pending"},
    ]});
    for (profile, build) in BOTH {
        let net = Fake::default()
            .reply(200, anthropic(json!([{"type": "tool_use", "id": "t1", "name": "todo_write", "input": list}]), "tool_use"))
            .reply(200, anthropic(json!([{"type": "text", "text": "On it."}]), "end_turn"));
        let (mut h, _net) = open(build, net);
        set_key(&mut h, KEY);
        write(&mut h, "a house");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        for step in ["Draw the walls", "Place the doors", "Name the rooms"] {
            h.get_by_label(step);
        }
        let todos = &h.state().assistant.chat.as_ref().unwrap().todos;
        let statuses: Vec<_> = todos.iter().map(|t| t.status).collect();
        use bimcraft_assistant::Status::*;
        assert_eq!(statuses, [Completed, InProgress, Pending], "{profile}");
        let used = h.state().assistant.log().iter().filter(|l| matches!(l, bimcraft_assistant::Line::Used { .. })).count();
        assert_eq!(used, 0, "{profile}: the list is not a line of the log");
    }
}

#[test]
fn the_models_reasoning_is_folded_apart_from_its_answer_until_opened() {
    let content =
        json!([{"type": "thinking", "thinking": "Levels first, then walls.", "signature": "s"}, {"type": "text", "text": "Two levels."}]);
    let (mut h, net) = open(app, Fake::default().reply(200, anthropic(content, "end_turn")));
    set_key(&mut h, KEY);
    write(&mut h, "what is in it?");
    click_in_palette(&mut h, "Agree and send");
    finish(&mut h);
    h.get_by_label("Two levels.");
    assert!(h.query_by_label_contains("Levels first").is_none(), "folded");
    h.get_by_label("Thinking").click();
    h.run();
    h.get_by_label_contains("Levels first");
    // The request asked for the summary, which the API hides by default.
    let body: serde_json::Value = serde_json::from_str(&net.seen()[0].body).unwrap();
    assert_eq!(body["thinking"]["display"], "summarized");
}

#[test]
fn enter_sends_and_shift_enter_does_not() {
    let (mut h, net) = open(app, wall_then_done());
    set_key(&mut h, KEY);
    write(&mut h, "a wall");
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    h.run();
    assert!(net.seen().is_empty() && !h.state().assistant.busy(), "Shift+Enter is a new line");
    key(&mut h, Key::Enter);
    finish(&mut h);
    assert_eq!(net.seen().len(), 2);
    assert_eq!(ids(&h, "wall").len(), 1);
}

#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("a test's file is not read".into())
    }
}

#[test]
fn files_are_picked_or_dropped_on_the_palette_shown_as_chips_and_sent_with_the_message() {
    for (profile, build) in BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (notes, plan) = (dir.path().join("notes.txt"), dir.path().join("plan.png"));
        std::fs::write(&notes, "two walls").unwrap();
        std::fs::write(&plan, [0x89, b'P', b'N', b'G']).unwrap();
        let (mut h, net) = open(build, Fake::default().reply(200, anthropic(json!([{"type": "text", "text": "Seen."}]), "end_turn")));
        set_key(&mut h, KEY);
        // Dropped with the pointer over the palette, a file is for the message and does not open as a project.
        let now = h.ctx.input(|i| i.time);
        h.state_mut().assistant.over_at = Some(now);
        h.input_mut().dropped_files.push(Arc::new(Dropped(notes.clone())));
        h.step();
        h.state_mut().assistant.chat.as_mut().unwrap().attach(&plan);
        h.run();
        assert!(h.state().opens.is_empty(), "{profile}: not opened as a project");
        h.get_by_label("Attached files go to Anthropic Claude with your message.");
        h.get_by_label("plan.png ×");
        // A chip takes its file off the message, and the copy with it.
        h.get_by_label("notes.txt ×").click();
        h.run();
        assert!(h.query_by_label("notes.txt ×").is_none(), "{profile}");
        let stage = h.state().assistant.chat.as_ref().unwrap().stage_dir().unwrap().to_path_buf();
        assert_eq!(std::fs::read_dir(&stage).unwrap().count(), 1, "{profile}");
        write(&mut h, "what is this?");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        assert!(h.query_by_label("plan.png ×").is_none(), "{profile}: sent once");
        h.get_by_label("@plan.png");
        let body: serde_json::Value = serde_json::from_str(&net.seen()[0].body).unwrap();
        assert_eq!(body["messages"][0]["content"][0]["type"], "image", "{profile}");
        assert_eq!(body["messages"][0]["content"][1]["text"], "what is this?", "{profile}");
        // A new chat takes the folder away.
        click_in_palette(&mut h, "New chat");
        assert!(!stage.exists(), "{profile}: the copies go with the chat");
    }
}

#[test]
fn a_file_dropped_away_from_the_palette_is_not_taken_for_the_message() {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "two walls").unwrap();
    let (mut h, _net) = open(app, Fake::default());
    h.input_mut().dropped_files.push(Arc::new(Dropped(notes)));
    h.step();
    assert!(h.state().assistant.chat.as_ref().unwrap().attached.is_empty());
}

#[test]
fn tokens_are_counted_for_the_project_kept_in_the_settings_file_and_shown_by_project_or_for_all() {
    for (profile, build) in BOTH {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let reply =
            |input, output| Fake::default().reply(200, reply_with(json!([{"type": "text", "text": "Hi."}]), "end_turn", input, output));
        let net = Arc::new(reply(1_200, 34).reply(200, reply_with(json!([{"type": "text", "text": "Again."}]), "end_turn", 5_000_000, 1)));
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        h.state_mut().assistant.attach(net.clone());
        h.state_mut().assistant.env = |_| None;
        shortcut(&mut h, "AI");
        set_key(&mut h, KEY);
        write(&mut h, "hello");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        // A project that is not saved yet is one of its own; once it has a file, its tokens are that file's.
        h.state_mut().session.path = Some("/work/house.bimc".into());
        write(&mut h, "again");
        click_in_palette(&mut h, "Send");
        finish(&mut h);
        let kept = written(&file)["assistant"]["usage"].clone();
        assert_eq!(kept, json!({"": [1200, 34], "/work/house.bimc": [5000000, 1]}), "{profile}");
        settings(&mut h);
        h.get_by_label("5,001,200 in · 35 out · 5,001,235 total");
        h.state_mut().assistant.usage_filter = Some("/work/house.bimc".into());
        h.run();
        h.get_by_label("5,000,000 in · 1 out · 5,000,001 total");
        // Reset takes out the one project shown; with all shown, everything.
        h.get_by_label("Reset the counts").click();
        h.run();
        assert_eq!(h.state().assistant.usage.len(), 1, "{profile}");
        h.get_by_label("1,200 in · 34 out · 1,234 total");
        h.get_by_label("Reset the counts").click();
        h.run();
        assert!(h.state().assistant.usage.is_empty(), "{profile}");
        assert!(written(&file)["assistant"].get("usage").is_none(), "{profile}: nothing counted, nothing written");
    }
}

#[test]
fn the_token_counts_come_back_with_the_settings_and_a_damaged_entry_is_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    std::fs::write(
        &file,
        json!({"assistant": {"usage": {"/a.bimc": [7, 3], "/b.bimc": "many", "/c.bimc": [1], "/d.bimc": [-1, 2]}}}).to_string(),
    )
    .unwrap();
    let mut h = app("");
    h.state_mut().use_settings(file);
    h.run();
    let usage = &h.state().assistant.usage;
    assert_eq!(usage.keys().collect::<Vec<_>>(), ["/a.bimc"]);
    assert_eq!((usage["/a.bimc"].input, usage["/a.bimc"].output), (7, 3));
}

/// What the palette looks like, for a person to look at: `BIMCRAFT_ASSISTANT_DUMP=<folder>` writes a conversation with a file, a
/// folded Thinking, a tool call, a failed one, the checklist and the box, in both profiles, light and dark.
#[test]
fn assistant_screenshots() {
    use bimcraft_assistant::{Line, Status, Todo};
    let Ok(folder) = std::env::var("BIMCRAFT_ASSISTANT_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let todo = |content: &str, status| Todo { content: content.into(), status };
    for (profile, build) in BOTH {
        for (dark, look) in [(false, "light"), (true, "dark")] {
            let (mut h, _net) = open(build, Fake::default());
            let chat = h.state_mut().assistant.chat.as_mut().unwrap();
            chat.log = vec![
                Line::You("Draw a four room house and name the rooms from the attached brief".into()),
                Line::Files(vec!["brief.txt".into(), "plan.png".into()]),
                Line::Thinks("Look at the levels first, then draw the outer walls and split them.".into()),
                Line::Says("I'll start with the outer walls, then the partitions.".into()),
                Line::Used {
                    name: "execute".into(),
                    args: r#"{"command":"wall","params":{"points":[[0,0],[8000,0]]}}"#.into(),
                    result: r#"{"id":100200}"#.into(),
                    error: false,
                },
                Line::Used {
                    name: "execute".into(),
                    args: r#"{"command":"door"}"#.into(),
                    result: "`door` needs an `at`".into(),
                    error: true,
                },
                Line::Says("Walls are in. Doors next.".into()),
            ];
            chat.todos = vec![
                todo("Map canvas, 3D view, title bar and Properties hooks", Status::Completed),
                todo("Core: presence type, its checks, tested", Status::InProgress),
                todo("Live layer: send, relay and keep others' presence", Status::Pending),
                todo("Tests, screenshots, translations, docs", Status::Pending),
            ];
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("assistant-{profile}-{look}.png"))).unwrap();
            // The palette is short: the checklist, and the box, too.
            h.get_by_label("Core: presence type, its checks, tested").scroll_to_me();
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("assistant-{profile}-{look}-plan.png"))).unwrap();
            h.get_by_role(Role::MultilineTextInput).scroll_to_me();
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("assistant-{profile}-{look}-bottom.png"))).unwrap();
        }
    }
}

// ---------- a chat for each project ----------

#[test]
fn a_chat_is_kept_for_each_saved_project_and_comes_back_when_it_is_opened_again() {
    for (profile, build) in BOTH {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let chats = dir.path().join("chats");
        let (house, shed) = (std::path::PathBuf::from("/work/house.bimc"), std::path::PathBuf::from("/work/shed.bimc"));
        let net = Arc::new(wall_then_done_net().reply(200, anthropic(json!([{"type": "text", "text": "A shed."}]), "end_turn")));
        let mut h = build("");
        h.state_mut().use_settings(file.clone());
        h.state_mut().assistant.attach(net.clone());
        h.state_mut().assistant.env = |_| None;
        shortcut(&mut h, "AI");
        set_key(&mut h, KEY);
        // An unsaved project has no file to keep a chat in.
        h.state_mut().session.path = None;
        h.run();
        assert!(!chats.exists(), "{profile}");
        h.state_mut().session.path = Some(house.clone());
        write(&mut h, "a wall");
        click_in_palette(&mut h, "Agree and send");
        finish(&mut h);
        h.run();
        let kept = bimcraft_assistant::saved::read(&chats, &house).expect("kept once the turn was over");
        assert_eq!(kept["project"], "/work/house.bimc", "{profile}");
        assert!(!std::fs::read_to_string(chats.join(bimcraft_assistant::saved::file_name(&house))).unwrap().contains(KEY), "{profile}");

        // Another project starts with none, and what is said there is kept apart.
        h.state_mut().session.path = Some(shed.clone());
        h.state_mut().assistant.project_replaced();
        h.run();
        assert!(h.state().assistant.log().is_empty(), "{profile}");
        assert!(bimcraft_assistant::saved::read(&chats, &house).is_some(), "{profile}: the first one's is still there");
        assert!(bimcraft_assistant::saved::read(&chats, &shed).is_none(), "{profile}");

        // Back to the first, in a window of its own: the conversation is there, and the model is sent it again.
        let mut next = build("");
        next.state_mut().use_settings(file.clone());
        next.state_mut().assistant.attach(net.clone());
        next.state_mut().assistant.env = |_| None;
        next.state_mut().session.path = Some(house.clone());
        shortcut(&mut next, "AI");
        next.run();
        assert!(
            next.state().assistant.log().iter().any(|l| matches!(l, bimcraft_assistant::Line::Says(t) if t == "Drew the wall.")),
            "{profile}: {:?}",
            next.state().assistant.log()
        );
        // The restored conversation fills the short palette, so the message goes the way the box sends it.
        h_send(&mut next, "and a shed");
        finish(&mut next);
        let last: serde_json::Value = serde_json::from_str(&net.seen().last().unwrap().body).unwrap();
        assert!(last["messages"].as_array().unwrap().len() >= 5, "{profile}: it came with the first turn's messages");
        // New chat forgets it, in the file too.
        next.state_mut().assistant.chat.as_mut().unwrap().clear();
        next.run();
        assert!(bimcraft_assistant::saved::read(&chats, &house).is_none(), "{profile}");
    }
}

fn h_send(h: &mut Harness<BimApp>, text: &str) {
    h.state_mut().assistant.chat.as_mut().unwrap().send(text, KEY);
}

fn wall_then_done_net() -> Fake {
    wall_then_done()
}

#[test]
fn a_chat_follows_its_project_when_it_is_saved_and_the_saved_chats_can_be_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let chats = dir.path().join("chats");
    let (mut h, _net) = open(app, wall_then_done());
    h.state_mut().use_settings(file);
    set_key(&mut h, KEY);
    write(&mut h, "a wall");
    click_in_palette(&mut h, "Agree and send");
    finish(&mut h);
    h.run();
    assert!(!chats.exists(), "no file yet: the project is not saved");
    // Save As gives the project a file, and the conversation, which was not lost, goes with it.
    let saved = std::path::PathBuf::from("/work/new.bimc");
    h.state_mut().session.path = Some(saved.clone());
    h.run();
    assert!(bimcraft_assistant::saved::read(&chats, &saved).is_some());
    settings(&mut h);
    h.get_by_label_contains("never in the project");
    h.get_by_label("Delete the saved chats").click();
    h.run();
    assert!(!chats.exists());
    assert!(!h.state().assistant.log().is_empty(), "what is on screen stays");
    h.run();
    assert!(!chats.exists(), "and is not written again until it changes");
}
