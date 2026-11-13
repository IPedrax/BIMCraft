//! The chat shared in a Co-Work session: its parts as JSON between the windows (`wire.rs`), and a turn that runs on another
//! window's key, as another person (`Chat::send_shared`).

use std::sync::{Arc, Mutex};

use bimcraft_core::bridge::Access;
use bimcraft_core::engine::Session;
use serde_json::json;

use crate::attach::{Attached, Body};
use crate::chat::{Chat, Line, Pays};
use crate::testing::Fake;
use crate::tools::{Status, Todo};
use crate::{Call, Done, Msg, Provider, Reply, Usage, wire};

fn reply(text: &str, calls: Vec<Call>) -> Reply {
    Reply {
        thinking: "why".into(),
        text: text.into(),
        calls,
        raw: json!([{"type": "text", "text": text}]),
        usage: Usage { input: 10, output: 5 },
    }
}

fn wall_call() -> Call {
    Call { id: "c1".into(), name: "execute".into(), args: json!({"command": "wall", "params": {"points": [[0, 0], [5000, 0]]}}) }
}

#[test]
fn the_chat_goes_between_windows_whole_pictures_and_all() {
    let picture = Attached { name: "plan.png".into(), body: Body::Image { mime: "image/png", b64: "iVBORw0KGgo=".into() } };
    let text = Attached { name: "notes.txt".into(), body: Body::Text("north is up".into()) };
    let history = vec![
        Msg::User("draw a wall".into(), vec![picture.clone(), text.clone()]),
        Msg::Assistant(reply("on it", vec![wall_call()])),
        Msg::Results(vec![Done { id: "c1".into(), name: "execute".into(), text: "{\"id\": 100004}".into(), error: false }]),
    ];
    let back = wire::history_from(&serde_json::from_str(&wire::history(&history).to_string()).unwrap()).unwrap();
    assert_eq!(back, history, "the picture too, which a saved chat leaves out");
    let log =
        vec![Line::You("draw a wall".into()), Line::Files(vec!["plan.png".into()]), Line::Thinks("why".into()), Line::Says("done".into())];
    assert_eq!(wire::log_from(&wire::log(&log)).unwrap(), log);
    let todos = vec![Todo { content: "walls".into(), status: Status::InProgress }];
    assert_eq!(wire::todos_from(&wire::todos(&todos)).unwrap(), todos);
    assert_eq!(wire::files_from(&wire::files(&[picture.clone(), text])).unwrap().len(), 2);
    let r = reply("on it", vec![wall_call()]);
    assert_eq!(wire::reply_from(&wire::reply(&r)).unwrap(), r);
    // What another window sends is read strictly: a picture that isn't one, or too big, and the whole is refused.
    for bad in [
        json!([{"name": "x.exe", "b64": "AAAA"}]),
        json!([{"name": "x.png", "b64": "not base64!"}]),
        json!([{"name": "x.png", "b64": "A".repeat((crate::MAX_FILE_IMAGE as usize) * 2)}]),
        json!([{"name": "x.txt"}]),
        json!({"name": "x.txt", "text": "a"}),
    ] {
        assert!(wire::files_from(&bad).is_none(), "{bad}");
    }
    assert!(wire::files_from(&json!(vec![json!({"name": "a.txt", "text": "a"}); crate::MAX_FILES + 1])).is_none(), "too many");
    assert!(wire::log_from(&json!([{"t": "says"}])).is_none());
    assert!(wire::reply_from(&json!({"t": "user", "text": "x", "files": []})).is_none(), "a reply is the model's");
}

/// A chat on a network that must not be reached: a turn that pays with another window's key never asks it.
fn chat() -> (Chat, Arc<Fake>) {
    let net = Arc::new(Fake::default());
    (Chat::new(Arc::clone(&net) as Arc<dyn crate::net::Net>), net)
}

fn run(chat: &mut Chat, session: &mut Session) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while chat.busy() {
        assert!(std::time::Instant::now() < until, "the turn never ended");
        chat.poll(session, Access::Edit);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn a_turn_runs_on_another_windows_key_as_the_person_who_asked() {
    let (mut chat, net) = chat();
    let mut session = Session::new();
    session.author = Some("Ana".into());
    // The other window answers: first a call, then words.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let remote: crate::chat::Remote = Arc::new(move |provider: Provider, model: &str, history: &[Msg]| {
        let mut seen = seen.lock().unwrap();
        seen.push((provider, model.to_string(), history.len()));
        Ok(if seen.len() == 1 { reply("on it", vec![wall_call()]) } else { reply("a wall it is", vec![]) })
    });
    chat.send_shared("draw a wall", Vec::new(), "Bo", "Bo", Provider::OpenAi, "gpt-6-astra", Pays::Remote(remote));
    run(&mut chat, &mut session);
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked, vec![(Provider::OpenAi, "gpt-6-astra".to_string(), 1), (Provider::OpenAi, "gpt-6-astra".to_string(), 3)]);
    assert!(net.seen().is_empty(), "this window's network is never used");
    assert_eq!(chat.take_usage(), Usage::default(), "the tokens are counted where the key is");
    assert!(chat.log.contains(&Line::Says("a wall it is".into())));
    // The wall is Bo's, one step, named for him.
    let top = session.steps(false)[0];
    assert_eq!((top.label.as_str(), top.by.as_deref()), ("Assistant (Bo): draw a wall", Some("Bo")));
    assert_eq!(session.author.as_deref(), Some("Ana"), "the session's own author after");
}

#[test]
fn another_payer_or_provider_starts_the_model_afresh_and_the_log_stays() {
    let (mut chat, _) = chat();
    let mut session = Session::new();
    let lengths = Arc::new(Mutex::new(Vec::new()));
    let remote = |lengths: Arc<Mutex<Vec<usize>>>| -> crate::chat::Remote {
        Arc::new(move |_: Provider, _: &str, history: &[Msg]| {
            lengths.lock().unwrap().push(history.len());
            Ok(reply("ok", vec![]))
        })
    };
    let say = |chat: &mut Chat, session: &mut Session, payer: &str, provider: Provider| {
        chat.send_shared("hello", Vec::new(), "Cy", payer, provider, "m1", Pays::Remote(remote(Arc::clone(&lengths))));
        run(chat, session);
    };
    say(&mut chat, &mut session, "Bo", Provider::Anthropic);
    say(&mut chat, &mut session, "Bo", Provider::Anthropic);
    say(&mut chat, &mut session, "Di", Provider::Anthropic);
    say(&mut chat, &mut session, "Di", Provider::Gemini);
    assert_eq!(*lengths.lock().unwrap(), vec![1, 3, 1, 1], "the same payer and provider go on; another starts afresh");
    let notes = chat.log.iter().filter(|l| matches!(l, Line::Note(n) if n.contains("afresh"))).count();
    assert_eq!(notes, 2);
    assert_eq!(chat.log.iter().filter(|l| matches!(l, Line::You(_))).count(), 4, "the log keeps every message");
    // The model that starts afresh is told what was said, as text, in the message.
    let Some(Msg::User(told, _)) = chat.history.first() else { panic!("{:?}", chat.history) };
    assert!(told.contains("[assistant] ok") && told.ends_with("Now: hello"), "{told}");
    assert_eq!(chat.log.iter().filter(|l| matches!(l, Line::You(t) if t == "hello")).count(), 4, "the log shows what was typed");
}

#[test]
fn a_turn_whose_other_window_fails_says_why_and_ends() {
    let (mut chat, _) = chat();
    let mut session = Session::new();
    let remote: crate::chat::Remote = Arc::new(|_: Provider, _: &str, _: &[Msg]| Err("Bo left: ask again".into()));
    chat.send_shared("hello", Vec::new(), "Cy", "Bo", Provider::Anthropic, "m1", Pays::Remote(remote));
    run(&mut chat, &mut session);
    assert!(chat.log.contains(&Line::Note("Bo left: ask again".into())));
}
