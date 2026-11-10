//! The provider clients against recorded replies (a text answer, a tool call, two tool calls, a malformed reply, a refusal), the
//! tools on a session, and the loop of a chat against a fake network. Nothing here reaches a real one.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_core::bridge::Access;
use bimcraft_core::engine::Session;
use serde_json::{Value, json};

use crate::net::{Net, Request, Ureq, allowed};
use crate::testing::Fake;
use crate::{Call, Chat, Done, Line, Msg, Provider, anthropic, clip, complete, gemini, key_ok, model_ok, openai, tools};

const KEY: &str = "sk-test-0123456789abcdef";

fn run(s: &mut Session, name: &str, args: Value) -> Result<Value, String> {
    tools::run(s, &Call { id: "c".into(), name: name.into(), args }, Access::Edit)
}

fn count(s: &mut Session, category: &str) -> u64 {
    run(s, "query_elements", json!({"category": category})).unwrap()["count"].as_u64().unwrap()
}

// ---------- the clients, against recorded replies ----------

const WALL: &str = r#"{"command":"wall","params":{"points":[[0,0],[4000,0]]}}"#;

fn anthropic_reply(content: Value, stop: &str) -> String {
    json!({"id": "msg_01", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5", "content": content,
        "stop_reason": stop, "usage": {"input_tokens": 10, "output_tokens": 5}})
    .to_string()
}

#[test]
fn anthropic_reads_a_text_answer_and_keeps_the_thinking_for_the_next_request() {
    let content = json!([{"type": "thinking", "thinking": "", "signature": "sig"}, {"type": "text", "text": "Hello "}, {"type": "text", "text": "there"}]);
    let r = anthropic::parse(200, &anthropic_reply(content.clone(), "end_turn")).unwrap();
    assert_eq!((r.text.as_str(), r.calls.len()), ("Hello there", 0));
    assert_eq!(r.raw, content);
}

#[test]
fn anthropic_reads_one_tool_call_and_sends_its_answer_back_by_id() {
    let content = json!([{"type": "text", "text": "Drawing."}, {"type": "tool_use", "id": "toolu_01", "name": "execute", "input": serde_json::from_str::<Value>(WALL).unwrap()}]);
    let r = anthropic::parse(200, &anthropic_reply(content.clone(), "tool_use")).unwrap();
    assert_eq!(r.text, "Drawing.");
    assert_eq!(r.calls, vec![Call { id: "toolu_01".into(), name: "execute".into(), args: serde_json::from_str(WALL).unwrap() }]);
    let done = Done { id: "toolu_01".into(), name: "execute".into(), text: "{\"id\":100200}".into(), error: false };
    let sent = anthropic::request("claude-sonnet-5-5", KEY, &[Msg::User("a wall".into()), Msg::Assistant(r), Msg::Results(vec![done])]);
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["model"], "claude-sonnet-5-5");
    assert_eq!(body["tools"].as_array().unwrap().len(), tools::all().len());
    assert!(body["tools"][0]["input_schema"]["properties"].is_object());
    assert_eq!(body["messages"][0], json!({"role": "user", "content": "a wall"}));
    assert_eq!(body["messages"][1], json!({"role": "assistant", "content": content}), "the assistant's turn goes back as it came");
    assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_01");
    assert!(body.get("temperature").is_none(), "the current API version refuses sampling parameters");
    assert_eq!(sent.url, "https://api.anthropic.com/v1/messages");
    assert!(sent.headers.contains(&("x-api-key".into(), KEY.into())));
    assert!(!sent.body.contains(KEY) && !sent.url.contains(KEY), "the key is in a header and nowhere else");
}

#[test]
fn anthropic_reads_two_tool_calls_in_one_reply() {
    let content = json!([
        {"type": "tool_use", "id": "toolu_a", "name": "inspect_model", "input": {}},
        {"type": "tool_use", "id": "toolu_b", "name": "query_elements", "input": {"category": "wall"}},
    ]);
    let r = anthropic::parse(200, &anthropic_reply(content, "tool_use")).unwrap();
    assert_eq!(r.calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["toolu_a", "toolu_b"]);
    assert_eq!(r.calls[1].args["category"], "wall");
}

#[test]
fn anthropic_malformed_replies_are_errors_not_panics() {
    for body in
        ["", "not json", "[]", "{}", r#"{"content": "text"}"#, r#"{"content": [{"type": "tool_use", "name": "execute", "input": {}}]}"#]
    {
        assert!(anthropic::parse(200, body).is_err(), "{body:?}");
    }
    let e = anthropic::parse(401, r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#).unwrap_err();
    assert!(e.contains("401") && e.contains("invalid x-api-key") && e.contains("check the key"), "{e}");
    assert!(anthropic::parse(529, "<html>overloaded</html>").unwrap_err().contains("529"));
    // A reply cut off in a tool call runs nothing; cut off in words it is shown, with a note.
    let call = json!([{"type": "tool_use", "id": "t", "name": "execute", "input": {"command": "wal"}}]);
    assert!(anthropic::parse(200, &anthropic_reply(call, "max_tokens")).unwrap_err().contains("cut off"));
    let words = anthropic::parse(200, &anthropic_reply(json!([{"type": "text", "text": "Half"}]), "max_tokens")).unwrap();
    assert!(words.text.starts_with("Half") && words.text.contains("cut off"));
}

#[test]
fn anthropic_refusal_is_said_with_its_category() {
    let body = json!({"content": [], "stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber"}}).to_string();
    let e = anthropic::parse(200, &body).unwrap_err();
    assert!(e.contains("declined") && e.contains("cyber"), "{e}");
}

fn openai_reply(output: Value, status: &str) -> String {
    json!({"id": "resp_1", "object": "response", "status": status, "output": output}).to_string()
}

#[test]
fn openai_reads_a_text_answer() {
    let output = json!([{"id": "rs_1", "type": "reasoning", "summary": []},
        {"id": "msg_1", "type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hello there", "annotations": []}]}]);
    let r = openai::parse(200, &openai_reply(output, "completed")).unwrap();
    assert_eq!((r.text.as_str(), r.calls.len()), ("Hello there", 0));
}

#[test]
fn openai_reads_a_tool_call_and_replays_only_what_can_be_replayed() {
    let output = json!([
        {"id": "rs_1", "type": "reasoning", "summary": []},
        {"id": "rs_2", "type": "reasoning", "summary": [], "encrypted_content": "blob"},
        {"id": "fc_1", "type": "function_call", "call_id": "call_1", "name": "execute", "arguments": WALL, "status": "completed"},
    ]);
    let r = openai::parse(200, &openai_reply(output, "completed")).unwrap();
    assert_eq!(r.calls, vec![Call { id: "call_1".into(), name: "execute".into(), args: serde_json::from_str(WALL).unwrap() }]);
    let done = Done { id: "call_1".into(), name: "execute".into(), text: "ok".into(), error: false };
    let sent = openai::request("gpt-6-astra", KEY, &[Msg::User("a wall".into()), Msg::Assistant(r), Msg::Results(vec![done])]);
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!((&body["store"], &body["model"]), (&json!(false), &json!("gpt-6-astra")));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["strict"], false);
    let types: Vec<&str> = body["input"].as_array().unwrap().iter().map(|i| i["type"].as_str().unwrap_or("message")).collect();
    assert_eq!(types, ["message", "reasoning", "function_call", "function_call_output"], "the reasoning without its content is left out");
    assert_eq!(body["input"][3]["call_id"], "call_1");
    assert_eq!(sent.url, "https://api.openai.com/v1/responses");
    assert!(sent.headers.contains(&("Authorization".into(), format!("Bearer {KEY}"))));
    assert!(!sent.body.contains(KEY) && !sent.url.contains(KEY));
}

#[test]
fn openai_reads_two_tool_calls_and_keeps_bad_arguments_for_the_tool_to_refuse() {
    let output = json!([
        {"type": "function_call", "call_id": "call_a", "name": "inspect_model", "arguments": "{}"},
        {"type": "function_call", "call_id": "call_b", "name": "execute", "arguments": "{not json"},
    ]);
    let r = openai::parse(200, &openai_reply(output, "completed")).unwrap();
    assert_eq!(r.calls.len(), 2);
    assert_eq!(r.calls[1].args, json!("{not json"));
    let mut s = Session::new();
    assert!(tools::run(&mut s, &r.calls[1], Access::Edit).unwrap_err().contains("JSON object"));
}

#[test]
fn openai_malformed_replies_failures_and_refusals_are_errors() {
    for body in ["", "<html>", "[]", "{}", r#"{"output": {}}"#, r#"{"output": [{"type": "function_call", "name": "x"}]}"#] {
        assert!(openai::parse(200, body).is_err(), "{body:?}");
    }
    let e = openai::parse(429, r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#).unwrap_err();
    assert!(e.contains("429") && e.contains("quota"), "{e}");
    let failed = json!({"status": "failed", "output": [], "error": {"message": "the server had an error"}}).to_string();
    assert!(openai::parse(200, &failed).unwrap_err().contains("the server had an error"));
    let refusal =
        openai_reply(json!([{"type": "message", "content": [{"type": "refusal", "refusal": "I can't help with that."}]}]), "completed");
    let e = openai::parse(200, &refusal).unwrap_err();
    assert!(e.contains("declined") && e.contains("can't help"), "{e}");
    let cut = openai_reply(json!([{"type": "function_call", "call_id": "c", "name": "execute", "arguments": "{"}]), "incomplete");
    assert!(openai::parse(200, &cut).unwrap_err().contains("cut off"));
}

fn gemini_reply(parts: Value, finish: &str) -> String {
    json!({"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": finish}]}).to_string()
}

#[test]
fn gemini_reads_a_text_answer_and_skips_the_thoughts() {
    let parts = json!([{"text": "pondering", "thought": true}, {"text": "Hello there"}]);
    let r = gemini::parse(200, &gemini_reply(parts, "STOP")).unwrap();
    assert_eq!((r.text.as_str(), r.calls.len()), ("Hello there", 0));
}

#[test]
fn gemini_reads_a_tool_call_and_sends_back_its_signature_and_an_answer_by_id() {
    let parts = json!([{"functionCall": {"name": "execute", "args": serde_json::from_str::<Value>(WALL).unwrap(), "id": "c1"}, "thoughtSignature": "sig"}]);
    let r = gemini::parse(200, &gemini_reply(parts, "STOP")).unwrap();
    assert_eq!(r.calls, vec![Call { id: "c1".into(), name: "execute".into(), args: serde_json::from_str(WALL).unwrap() }]);
    let done = vec![Done { id: "c1".into(), name: "execute".into(), text: "ok".into(), error: false }];
    let nameless = vec![Done { id: String::new(), name: "inspect_model".into(), text: "boom".into(), error: true }];
    let sent = gemini::request(
        "gemini-3.8-flash",
        KEY,
        &[Msg::User("a wall".into()), Msg::Assistant(r), Msg::Results(done), Msg::Results(nameless)],
    );
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["contents"][1]["role"], "model");
    assert_eq!(body["contents"][1]["parts"][0]["thoughtSignature"], "sig", "the signature goes back in the part it came in");
    assert_eq!(body["contents"][2]["parts"][0]["functionResponse"], json!({"name": "execute", "id": "c1", "response": {"result": "ok"}}));
    assert_eq!(body["contents"][3]["parts"][0]["functionResponse"], json!({"name": "inspect_model", "response": {"error": "boom"}}));
    assert_eq!(sent.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:generateContent");
    assert!(sent.headers.contains(&("x-goog-api-key".into(), KEY.into())));
    assert!(!sent.body.contains(KEY) && !sent.url.contains(KEY), "the key is in a header, not the address");
    // The declarations are Gemini's subset: no free-form object, none with nothing to pass.
    let declared = body["tools"][0]["functionDeclarations"].as_array().unwrap();
    assert_eq!(declared.len(), tools::all().len());
    assert_eq!(declared[0]["parameters"]["properties"]["params"]["type"], "string");
    assert!(declared.iter().find(|d| d["name"] == "inspect_model").unwrap().get("parameters").is_none());
}

#[test]
fn gemini_reads_two_tool_calls_and_the_free_form_object_written_as_text() {
    let parts = json!([
        {"functionCall": {"name": "inspect_model", "args": {}}},
        {"functionCall": {"name": "execute", "args": {"command": "wall", "params": "{\"points\": [[0,0],[3000,0]]}"}}},
    ]);
    let r = gemini::parse(200, &gemini_reply(parts, "STOP")).unwrap();
    assert_eq!(r.calls.len(), 2);
    let mut s = Session::new();
    run(&mut s, "execute", r.calls[1].args.clone()).unwrap();
    assert_eq!(count(&mut s, "wall"), 1, "the object came back out of its text");
}

#[test]
fn gemini_malformed_blocked_and_refused_replies_are_errors() {
    for body in ["", "nope", "[]", "{}", r#"{"candidates": []}"#, r#"{"candidates": [{"content": {"parts": [{"functionCall": {}}]}}]}"#] {
        assert!(gemini::parse(200, body).is_err(), "{body:?}");
    }
    let blocked = r#"{"promptFeedback": {"blockReason": "SAFETY"}}"#;
    assert!(gemini::parse(200, blocked).unwrap_err().contains("SAFETY"));
    let e = gemini::parse(400, r#"{"error":{"code":400,"message":"API key not valid.","status":"INVALID_ARGUMENT"}}"#).unwrap_err();
    assert!(e.contains("400") && e.contains("API key not valid"), "{e}");
    assert!(gemini::parse(200, &gemini_reply(json!([]), "SAFETY")).unwrap_err().contains("declined"));
    assert!(gemini::parse(200, &gemini_reply(json!([]), "MALFORMED_FUNCTION_CALL")).unwrap_err().contains("tool call"));
    // A model turn with no content at all still goes back as a model turn.
    let r = gemini::parse(200, r#"{"candidates": [{"finishReason": "STOP"}]}"#).unwrap();
    assert_eq!(r.raw["role"], "model");
}

#[test]
fn a_key_or_model_that_is_no_key_or_model_never_reaches_a_request() {
    let net = Fake::default();
    let history = [Msg::User("hi".into())];
    for model in ["", "a b", "x/../y", "m?key=1", "m:generate"] {
        assert!(!model_ok(model), "{model:?}");
        assert!(complete(Provider::Gemini, &net, model, KEY, &history).is_err());
    }
    for key in ["", "short", "has space in it ok", "line\nbreak-in-it-0123", "ключ-0123456789"] {
        assert!(!key_ok(key), "{key:?}");
        assert!(complete(Provider::OpenAi, &net, "gpt-6-astra", key, &history).is_err());
    }
    assert!(net.seen().is_empty());
}

#[test]
fn every_provider_lists_models_the_ids_of_which_pass() {
    for p in Provider::ALL {
        assert!(p.models().iter().all(|m| model_ok(m)), "{p:?}");
        assert_eq!(Provider::from_key(p.key()), Some(p));
    }
    assert_eq!(Provider::Anthropic.default_model(), "claude-sonnet-5-5");
}

// ---------- the network ----------

#[test]
fn the_assistant_talks_to_its_three_hosts_over_https_and_to_nothing_else() {
    for yes in [
        "https://api.anthropic.com/v1/messages",
        "https://api.openai.com/v1/responses",
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:generateContent",
        "https://API.OPENAI.COM/v1/responses",
    ] {
        assert!(allowed(yes), "{yes}");
    }
    for no in [
        "http://api.anthropic.com/v1/messages",
        "https://api.anthropic.com.evil.example/v1/messages",
        "https://evil.example/api.anthropic.com",
        "https://api.anthropic.com:8443/v1/messages",
        "https://user:pass@api.anthropic.com/v1/messages",
        "https://api.anthropic.com@evil.example/",
        "https://github.com/",
        "ftp://api.openai.com/",
        "api.openai.com",
        "",
    ] {
        assert!(!allowed(no), "{no}");
    }
}

/// Serves one reply to each of `replies` on this machine, and gives back what the requests said.
fn serve(replies: Vec<(u16, Vec<u8>)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut said = Vec::new();
        for (status, body) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let length = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|n| n.trim().parse().ok()))
                .unwrap_or(0);
            let mut sent = vec![0u8; length];
            stream.read_exact(&mut sent).unwrap();
            said.push(format!("{head}{}", String::from_utf8_lossy(&sent)));
            let _ =
                stream.write_all(format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
            let _ = stream.write_all(&body);
        }
        said
    });
    (base, server)
}

#[test]
fn a_reply_is_read_with_its_status_and_capped_and_a_redirect_goes_nowhere() {
    let big = vec![b'a'; crate::MAX_REPLY as usize + 10];
    let (base, server) = serve(vec![
        (200, br#"{"ok":1}"#.to_vec()),
        (401, br#"{"error":{"message":"bad key"}}"#.to_vec()),
        (200, big),
        (302, b"moved".to_vec()),
    ]);
    let net = Ureq::loopback(&base);
    let ask = |path: &str| {
        net.post(&Request { url: format!("{base}/{path}"), headers: vec![("x-api-key".into(), KEY.into())], body: "{\"q\":1}".into() })
    };
    assert_eq!(ask("a").unwrap(), (200, r#"{"ok":1}"#.to_string()));
    assert_eq!(ask("b").unwrap(), (401, r#"{"error":{"message":"bad key"}}"#.to_string()), "an error status is a reply to read");
    assert!(ask("c").is_err(), "a reply past the cap is not read");
    assert_eq!(ask("d").unwrap().0, 302, "a redirect is handed back, not followed");
    let said = server.join().unwrap();
    assert!(said[0].starts_with("POST /a "), "{}", said[0]);
    assert!(said[0].to_ascii_lowercase().contains("x-api-key: sk-test"), "the key went in a header");
    assert!(said[0].ends_with("{\"q\":1}"));
    let e = net.post(&Request { url: "https://api.openai.com/v1/responses".into(), headers: vec![], body: String::new() }).unwrap_err();
    assert!(e.contains("won't send") && !e.contains("/v1/responses"), "{e}");
}

// ---------- the tools ----------

#[test]
fn execute_makes_a_wall_and_a_failed_command_changes_nothing() {
    let mut s = Session::new();
    let made = run(&mut s, "execute", json!({"command": "wall", "params": {"points": [[0, 0], [4000, 0]]}})).unwrap();
    assert!(made["ids"][0].is_u64(), "{made}");
    assert_eq!(count(&mut s, "wall"), 1);
    let e = run(&mut s, "execute", json!({"command": "wall", "params": {"points": [[0, 0]]}})).unwrap_err();
    assert!(!e.is_empty());
    assert_eq!(count(&mut s, "wall"), 1);
    assert!(run(&mut s, "execute", json!({"command": "nonsense"})).unwrap_err().contains("unknown command"));
    assert!(run(&mut s, "execute", json!({})).unwrap_err().contains("command"));
    assert!(run(&mut s, "execute", json!({"command": "wall", "params": [1]})).unwrap_err().contains("JSON object"));
    assert!(run(&mut s, "nope", json!({})).unwrap_err().contains("unknown tool"));
    assert!(tools::run(&mut s, &Call { id: "c".into(), name: "execute".into(), args: json!([1]) }, Access::Edit).is_err());
}

#[test]
fn the_tools_read_the_model_and_list_commands_by_filter() {
    let mut s = Session::new();
    run(&mut s, "execute", json!({"command": "wall", "params": {"points": [[0, 0], [4000, 0]]}})).unwrap();
    assert!(run(&mut s, "inspect_model", json!({})).unwrap().is_object());
    let some = run(&mut s, "list_commands", json!({"filter": "wall"})).unwrap();
    let all = run(&mut s, "list_commands", json!({})).unwrap();
    assert!(some.as_array().unwrap().iter().any(|c| c["id"] == "wall"));
    assert!(some.as_array().unwrap().len() < all.as_array().unwrap().len());
    // A page of elements is held to 200, however many were asked for.
    let page = run(&mut s, "query_elements", json!({"limit": 100000})).unwrap();
    assert!(page["elements"].as_array().unwrap().len() <= 200);
}

#[test]
fn what_would_touch_a_file_or_replace_the_project_or_make_a_picture_is_refused() {
    let mut s = Session::new();
    run(&mut s, "execute", json!({"command": "wall", "params": {"points": [[0, 0], [4000, 0]]}})).unwrap();
    for (command, params) in [
        ("save", json!({"path": "/tmp/bimcraft-assistant-test.bimcraft"})),
        ("open", json!({"path": "/etc/passwd"})),
        ("highrise", json!({})),
        ("render", json!({})),
        ("new", json!({})),
        ("NEW", json!({})),
    ] {
        let e = run(&mut s, "execute", json!({"command": command, "params": params})).unwrap_err();
        assert!(e.contains("file") || e.contains("picture") || e.contains("replace"), "{command}: {e}");
    }
    assert!(!std::path::Path::new("/tmp/bimcraft-assistant-test.bimcraft").exists());
    assert_eq!(count(&mut s, "wall"), 1, "the project is still the user's");
}

#[test]
fn a_session_that_may_only_read_is_read_through_the_tools_too() {
    let mut s = Session::new();
    let call = Call { id: "c".into(), name: "execute".into(), args: json!({"command": "wall", "params": {"points": [[0, 0], [4000, 0]]}}) };
    assert!(tools::run(&mut s, &call, Access::Read).is_err());
    assert_eq!(count(&mut s, "wall"), 0);
}

#[test]
fn a_long_result_is_cut_where_a_character_ends_and_says_so() {
    let long = json!({"text": "é".repeat(crate::MAX_RESULT)});
    let text = tools::to_text(&long);
    assert!(text.len() < crate::MAX_RESULT + 200, "{}", text.len());
    assert!(text.contains("more bytes left out"));
    assert_eq!(tools::to_text(&json!({"a": 1})), r#"{"a":1}"#);
    assert_eq!(clip("héllo", 2), "hé…");
    assert_eq!(clip("hi", 5), "hi");
}

// ---------- the loop ----------

fn tool_reply(calls: &[(&str, &str, &str)]) -> String {
    let blocks: Vec<Value> = calls
        .iter()
        .map(
            |(id, name, input)| json!({"type": "tool_use", "id": id, "name": name, "input": serde_json::from_str::<Value>(input).unwrap()}),
        )
        .collect();
    anthropic_reply(Value::Array(blocks), "tool_use")
}

fn text_reply(text: &str) -> String {
    anthropic_reply(json!([{"type": "text", "text": text}]), "end_turn")
}

/// Polls until the turn is over, the way frames do.
fn drive(chat: &mut Chat, s: &mut Session) {
    let until = Instant::now() + Duration::from_secs(20);
    while chat.busy() {
        assert!(Instant::now() < until, "the turn did not end: {:?}", chat.log);
        if !chat.poll(s, Access::Edit) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn chat_on(net: &Arc<Fake>) -> Chat {
    Chat::new(net.clone())
}

#[test]
fn a_message_that_makes_a_wall_is_one_undo_step_named_for_the_assistant_and_undo_takes_it_back() {
    let net = Arc::new(Fake::default().reply(200, tool_reply(&[("t1", "execute", WALL)])).reply(200, text_reply("Drew the wall.")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("draw a wall four metres long", KEY);
    assert!(chat.busy());
    drive(&mut chat, &mut s);
    assert_eq!(count(&mut s, "wall"), 1);
    assert_eq!(s.history(), vec!["Assistant: draw a wall four metres long"]);
    assert_eq!(chat.log.iter().filter(|l| matches!(l, Line::Used { error: false, .. })).count(), 1);
    assert_eq!(chat.log.last(), Some(&Line::Says("Drew the wall.".into())));
    // The second request carried the first one's tool call and its answer.
    let second: Value = serde_json::from_str(&net.seen()[1].body).unwrap();
    assert_eq!(second["messages"][2]["content"][0]["tool_use_id"], "t1");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(count(&mut s, "wall"), 0);
}

#[test]
fn several_batches_of_one_message_are_still_one_undo_step() {
    let second = r#"{"command":"wall","params":{"points":[[0,3000],[4000,3000]]}}"#;
    let net = Arc::new(
        Fake::default()
            .reply(200, tool_reply(&[("a", "execute", WALL), ("b", "inspect_model", "{}")]))
            .reply(200, tool_reply(&[("c", "execute", second)]))
            .reply(200, text_reply("Two walls.")),
    );
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    s.execute("level", &json!({"elevation": 6000})).unwrap();
    chat.send("two walls", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(count(&mut s, "wall"), 2);
    assert_eq!(s.history(), vec!["Level", "Assistant: two walls"]);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((count(&mut s, "wall"), s.history()), (0, vec!["Level"]));
}

#[test]
fn a_failing_call_is_told_to_the_model_and_the_others_still_run() {
    let net = Arc::new(
        Fake::default()
            .reply(200, tool_reply(&[("a", "execute", r#"{"command":"wall","params":{"points":[[0,0]]}}"#), ("b", "execute", WALL)]))
            .reply(200, text_reply("One of them failed.")),
    );
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("two", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(count(&mut s, "wall"), 1);
    let results: Value = serde_json::from_str(&net.seen()[1].body).unwrap();
    let results = &results["messages"][2]["content"];
    assert_eq!((&results[0]["is_error"], &results[1]["is_error"]), (&json!(true), &json!(false)));
}

#[test]
fn a_provider_error_is_a_note_in_the_chat_and_the_project_is_untouched() {
    let net = Arc::new(Fake::default().reply(401, r#"{"error":{"message":"invalid x-api-key"}}"#).fail("connection reset"));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("hello", KEY);
    drive(&mut chat, &mut s);
    assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("invalid x-api-key")), "{:?}", chat.log);
    assert!(s.history().is_empty());
    chat.send("again", KEY);
    drive(&mut chat, &mut s);
    assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("connection reset")), "{:?}", chat.log);
    // Nothing left to send: the fake has run dry, and that is an error in the chat too.
    chat.send("once more", KEY);
    drive(&mut chat, &mut s);
    assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("no more replies")));
}

#[test]
fn a_model_that_keeps_calling_tools_is_stopped_and_what_it_did_stays_one_step() {
    let mut fake = Fake::default();
    for _ in 0..=crate::MAX_ROUNDS {
        fake = fake.reply(200, tool_reply(&[("t", "execute", WALL)]));
    }
    let net = Arc::new(fake);
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("loop", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(net.seen().len(), crate::MAX_ROUNDS);
    assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("Stopped after")));
    assert_eq!(s.history(), vec!["Assistant: loop"]);
}

#[test]
fn more_calls_than_the_cap_are_answered_with_errors_not_run() {
    let calls: Vec<(String, &str, &str)> = (0..crate::MAX_CALLS + 2).map(|i| (format!("t{i}"), "execute", WALL)).collect();
    let calls: Vec<(&str, &str, &str)> = calls.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
    let net = Arc::new(Fake::default().reply(200, tool_reply(&calls)).reply(200, text_reply("ok")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("many", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(count(&mut s, "wall"), crate::MAX_CALLS as u64);
    let errors = chat.log.iter().filter(|l| matches!(l, Line::Used { error: true, .. })).count();
    assert_eq!(errors, 2);
}

#[test]
fn the_key_is_in_one_header_and_never_in_the_log_the_history_or_a_debug_print() {
    let net = Arc::new(Fake::default().reply(200, tool_reply(&[("t", "execute", WALL)])).reply(200, text_reply("done")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("a wall", KEY);
    drive(&mut chat, &mut s);
    for sent in net.seen() {
        assert!(!sent.body.contains(KEY) && !sent.url.contains(KEY) && !format!("{sent:?}").contains(KEY));
        assert_eq!(sent.headers.iter().filter(|(_, v)| v.contains(KEY)).count(), 1);
    }
    assert!(!format!("{:?}", chat.log).contains(KEY));
}

#[test]
fn a_message_is_refused_while_one_runs_and_when_it_is_too_long_or_the_key_is_bad() {
    let net = Arc::new(Fake::default().reply(200, text_reply("hi")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("   ", KEY);
    assert!(!chat.busy() && chat.log.is_empty());
    chat.send("hello", "bad key");
    assert!(!chat.busy() && matches!(chat.log.last(), Some(Line::Note(_))));
    chat.send(&"x".repeat(crate::MAX_TEXT + 1), KEY);
    assert!(!chat.busy() && net.seen().is_empty());
    chat.send("hello", KEY);
    chat.send("second while the first runs", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(net.seen().len(), 1);
    // A conversation past the cap is not sent again.
    chat.history.push(Msg::User("x".repeat(crate::MAX_HISTORY)));
    chat.send("one more", KEY);
    assert!(!chat.busy() && matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("too long")));
    chat.clear();
    assert!(chat.log.is_empty());
}

#[test]
fn stopping_ends_the_turn_and_keeps_what_was_done() {
    let net = Arc::new(Fake::default().reply(200, tool_reply(&[("t", "execute", WALL)])).reply(200, text_reply("never read")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("a wall", KEY);
    // Wait for the first reply, take it, and stop before the second is read.
    let until = Instant::now() + Duration::from_secs(20);
    while !chat.poll(&mut s, Access::Edit) {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    chat.stop(&mut s);
    assert!(!chat.busy());
    assert!(!chat.poll(&mut s, Access::Edit));
    assert_eq!((count(&mut s, "wall"), s.history()), (1, vec!["Assistant: a wall"]));
}
