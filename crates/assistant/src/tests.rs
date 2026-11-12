//! The provider clients against recorded replies (a text answer, a tool call, two tool calls, a malformed reply, a refusal), the
//! tools on a session, and the loop of a chat against a fake network. Nothing here reaches a real one.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_core::bridge::Access;
use bimcraft_core::engine::Session;
use serde_json::{Value, json};

use crate::attach::base64;
use crate::net::{Net, Request, Ureq, allowed};
use crate::testing::Fake;
use crate::{
    Attached, Body, Call, Chat, Done, Line, MAX_FILES, MAX_FILES_TOTAL, Msg, Provider, Status, Usage, anthropic, clip, complete, gemini,
    key_ok, model_ok, openai, tools,
};

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
    let sent = anthropic::request("claude-sonnet-5-5", KEY, &[Msg::user("a wall"), Msg::Assistant(r), Msg::Results(vec![done])]);
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
    let sent = openai::request("gpt-6-astra", KEY, &[Msg::user("a wall"), Msg::Assistant(r), Msg::Results(vec![done])]);
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
    let sent =
        gemini::request("gemini-3.8-flash", KEY, &[Msg::user("a wall"), Msg::Assistant(r), Msg::Results(done), Msg::Results(nameless)]);
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
    let history = [Msg::user("hi")];
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
    // A field's value is what it holds.
    let wall = run(&mut s, "query_elements", json!({"category": "wall"})).unwrap();
    let kind = wall["elements"][0]["type"].as_str().unwrap().to_string();
    let found = run(&mut s, "query_elements", json!({"category": "wall", "field": "Type", "value": kind})).unwrap();
    assert_eq!(found["count"], 1);
    // The components standing in a room.
    run(
        &mut s,
        "execute",
        json!({"command": "wall", "params": {"points": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]], "closed": true}}),
    )
    .unwrap();
    let room = run(&mut s, "execute", json!({"command": "room", "params": {"point": [12000, 2000]}})).unwrap()["id"].clone();
    run(&mut s, "execute", json!({"command": "component", "params": {"family": "Outlet", "at": [11000, 1000]}})).unwrap();
    assert_eq!(run(&mut s, "query_elements", json!({"room": room})).unwrap()["count"], 1);
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
    chat.history.push(Msg::user("x".repeat(crate::MAX_HISTORY)));
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

// ---------- the saved key, scrambled ----------

use crate::seal;

/// A secret for the test of the format: the build's own is not the test's to know.
const SECRET: &str = "correct horse battery";
const ADDED: &str = "sk-ant-test-0123456789";

fn hex_of(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_sealed_key_is_the_standard_pbkdf2_and_aes_gcm_that_another_program_reads() {
    // Made by Python's hashlib.pbkdf2_hmac and cryptography's AESGCM, from the same secret, salt, nonce and iterations: the format is
    // the standards', not just something this code can read back.
    let known = "bc1:1000:000102030405060708090a0b0c0d0e0f:a0a1a2a3a4a5a6a7a8a9aaab:1aa41a32de7c6c9b027a51f888b267496b05424b7bcb042e180176d81381304cd41de8ace4eb";
    assert_eq!(seal::open_with(SECRET, known).unwrap(), ADDED);
    let salt: Vec<u8> = (0..16).collect();
    let nonce: Vec<u8> = (0xa0..0xac).collect();
    assert_eq!(seal::seal_with(SECRET, ADDED, &salt, &nonce, 1000).unwrap(), known);
}

#[test]
fn a_key_sealed_opens_under_the_builds_secret_and_under_no_other() {
    let sealed = seal::seal(ADDED).unwrap();
    assert!(sealed.starts_with("bc1:") && seal::well_formed(&sealed));
    assert!(!sealed.contains(ADDED) && !sealed.contains(&hex_of(ADDED)), "the key is not in it, in any form a reader would see");
    assert_eq!(seal::open(&sealed).unwrap(), ADDED);
    for other in ["", "another secret", SECRET, seal::DEV_SECRET.to_uppercase().as_str()] {
        if other != if seal::built_with_secret() { "" } else { seal::DEV_SECRET } {
            assert!(seal::open_with(other, &sealed).is_err(), "{other:?}");
        }
    }
    // The same key seals differently each time: a salt and a nonce of its own. A key pasted with its line break is the key.
    assert_ne!(sealed, seal::seal(ADDED).unwrap());
    assert_eq!(seal::open(&seal::seal(&format!("  {ADDED}\n")).unwrap()).unwrap(), ADDED);
    let long = "k".repeat(crate::KEY_MAX);
    assert_eq!(seal::open(&seal::seal(&long).unwrap()).unwrap(), long);
    for no_key in ["", "short", "has a space in it", "line\nbreak-in-it-0123"] {
        assert!(seal::seal(no_key).is_err(), "{no_key:?}");
    }
    assert!(seal::seal(&"k".repeat(crate::KEY_MAX + 1)).is_err());
}

#[test]
fn a_sealed_key_that_was_changed_is_refused() {
    let sealed = seal::seal(ADDED).unwrap();
    let parts: Vec<&str> = sealed.split(':').collect();
    // Any hex digit of the salt, the nonce or the text, changed, is a key that does not open: the tag says so.
    for part in 2..5 {
        for at in [0, parts[part].len() / 2, parts[part].len() - 1] {
            let mut p: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
            let other = if p[part].as_bytes()[at] == b'0' { "1" } else { "0" };
            p[part].replace_range(at..=at, other);
            assert!(seal::open(&p.join(":")).is_err(), "part {part} at {at}");
        }
    }
    // The iterations are not covered by the tag: they only decide the key derived, so a changed number is the wrong key.
    let slower = sealed.replacen(&format!(":{}:", seal::ITERATIONS), &format!(":{}:", seal::ITERATIONS + 1), 1);
    assert_ne!(slower, sealed);
    assert!(seal::open(&slower).is_err());
}

#[test]
fn what_is_not_a_sealed_key_is_refused_quickly_and_never_panics() {
    let sealed = seal::seal(ADDED).unwrap();
    let parts: Vec<&str> = sealed.split(':').collect();
    let (salt, nonce, text) = (parts[2], parts[3], parts[4]);
    let huge = format!("bc1:4294967295:{salt}:{nonce}:{text}");
    let zero = format!("bc1:0:{salt}:{nonce}:{text}");
    let started = std::time::Instant::now();
    for bad in [
        "",
        "bc1",
        "bc1:::::",
        "bc2:10000:00:00:00",
        "x:10000:00:00:00",
        &sealed[..sealed.len() - 3],
        &format!("{sealed}0"),
        &format!("{sealed}:extra"),
        &sealed.to_uppercase(),
        &format!("bc1:10000:{}:{nonce}:{text}", "zz".repeat(16)),
        &format!("bc1:10000:{}:{nonce}:{text}", &salt[..30]),
        &format!("bc1:10000:{salt}:{}:{text}", &nonce[..22]),
        &format!("bc1:10000:{salt}:{nonce}:"),
        &format!("bc1:10000:{salt}:{nonce}:00"),
        "\u{0}\u{0}",
        huge.as_str(),
        zero.as_str(),
        &format!("bc1:{}:{salt}:{nonce}:{text}", "9".repeat(40)),
    ] {
        assert!(!seal::well_formed(bad), "{bad:?}");
        assert!(seal::open(bad).is_err(), "{bad:?}");
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "a hostile file does not make the window derive a key for minutes: {:?}",
        started.elapsed()
    );
    assert!(seal::seal_with("", ADDED, &[0; 16], &[0; 12], 1000).is_err(), "a secret to seal with");
}

#[test]
fn a_release_is_built_with_its_own_secret() {
    // `built_with_secret` is what the build was given. The release workflow says a release must have one (`BIMCRAFT_REQUIRE_KEY_SECRET`),
    // and this is the test that fails it when the GitHub secret is missing or empty: it would otherwise seal with the public one.
    assert_eq!(seal::built_with_secret(), option_env!("BIMCRAFT_KEY_SECRET").is_some_and(|s| !s.trim().is_empty()));
    if std::env::var_os("BIMCRAFT_REQUIRE_KEY_SECRET").is_some() {
        assert!(
            seal::built_with_secret(),
            "this build has no BIMCRAFT_KEY_SECRET: set the repository secret of that name before a release"
        );
    }
}

// ---------- the saved key, under a passphrase ----------

const PASSPHRASE: &str = "correct horse battery";

#[test]
fn a_key_sealed_with_a_passphrase_is_the_standard_pbkdf2_and_aes_gcm_too() {
    // Made by Python's hashlib.pbkdf2_hmac and cryptography's AESGCM, from the same passphrase, salt, nonce, iterations and associated data.
    let known = "bc2:1000:000102030405060708090a0b0c0d0e0f:a0a1a2a3a4a5a6a7a8a9aaab:1aa41a32de7c6c9b027a51f888b267496b05424b7bcbc2e537752aa2aed4b277b826908f57a0";
    assert_eq!(seal::open_passphrase(PASSPHRASE, known).unwrap(), ADDED);
    let salt: Vec<u8> = (0..16).collect();
    let nonce: Vec<u8> = (0xa0..0xac).collect();
    assert_eq!(seal::seal_passphrase_with(PASSPHRASE, ADDED, &salt, &nonce, 1000).unwrap(), known);
    assert!(seal::needs_passphrase(known) && seal::well_formed(known));
}

#[test]
fn a_key_under_a_passphrase_opens_with_it_alone_and_only_with_it() {
    let sealed = seal::seal_passphrase(PASSPHRASE, ADDED).unwrap();
    assert!(sealed.starts_with("bc2:600000:") && seal::needs_passphrase(&sealed) && seal::well_formed(&sealed));
    assert!(!sealed.contains(ADDED) && !sealed.contains(&hex_of(ADDED)) && !sealed.contains(PASSPHRASE));
    assert_eq!(seal::open_passphrase(PASSPHRASE, &sealed).unwrap(), ADDED);
    for wrong in ["", "correct horse batter", "Correct horse battery", " correct horse battery", seal::DEV_SECRET] {
        assert!(seal::open_passphrase(wrong, &sealed).is_err(), "{wrong:?}");
    }
    // It does not open without the passphrase, whatever the build's secret is, and a key sealed under the build's is not a passphrase's.
    assert!(seal::open(&sealed).unwrap_err().contains("passphrase"));
    let built_in = seal::seal(ADDED).unwrap();
    assert!(!seal::needs_passphrase(&built_in) && seal::open_passphrase(PASSPHRASE, &built_in).is_err());
    assert_ne!(sealed, seal::seal_passphrase(PASSPHRASE, ADDED).unwrap(), "a salt and a nonce of its own");
    // A passphrase in any script, a key as long as keys come, and a key pasted with its line break.
    let long = "k".repeat(crate::KEY_MAX);
    for (p, k) in [("пароль-у-мене-є", ADDED), ("pass phrase with spaces", long.as_str())] {
        assert_eq!(seal::open_passphrase(p, &seal::seal_passphrase(p, k).unwrap()).unwrap(), k);
    }
    assert_eq!(seal::open_passphrase(PASSPHRASE, &seal::seal_passphrase(PASSPHRASE, &format!(" {ADDED}\n")).unwrap()).unwrap(), ADDED);
    // Nothing to keep, or nothing to keep it with.
    for (p, k) in [("short", ADDED), ("", ADDED), (PASSPHRASE, ""), (PASSPHRASE, "short")] {
        assert!(seal::seal_passphrase(p, k).is_err(), "{p:?} {k:?}");
    }
    assert!(seal::seal_passphrase(&"p".repeat(seal::PASSPHRASE_MAX + 1), ADDED).is_err());
    assert!(seal::seal_passphrase(PASSPHRASE, &"k".repeat(crate::KEY_MAX + 1)).is_err());
}

#[test]
fn a_key_under_a_passphrase_that_was_changed_is_refused_and_a_bad_text_never_panics() {
    let salt: Vec<u8> = (0..16).collect();
    let nonce: Vec<u8> = (0xa0..0xac).collect();
    let sealed = seal::seal_passphrase_with(PASSPHRASE, ADDED, &salt, &nonce, 1000).unwrap();
    let parts: Vec<&str> = sealed.split(':').collect();
    for part in 2..5 {
        for at in [0, parts[part].len() / 2, parts[part].len() - 1] {
            let mut p: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
            let other = if p[part].as_bytes()[at] == b'0' { "1" } else { "0" };
            p[part].replace_range(at..=at, other);
            assert!(seal::open_passphrase(PASSPHRASE, &p.join(":")).is_err(), "part {part} at {at}");
        }
    }
    assert!(seal::open_passphrase(PASSPHRASE, &sealed.replacen(":1000:", ":1001:", 1)).is_err());
    // A text of the format's name that is not that format's, or any other, is refused before anything is derived.
    let (salt, nonce, text) = (parts[2], parts[3], parts[4]);
    let huge = format!("bc2:4294967295:{salt}:{nonce}:{text}");
    let started = std::time::Instant::now();
    for bad in [
        "bc2",
        "bc2:::::",
        "bc3:1000:00:00:00",
        &format!("bc2:1000:{}:{nonce}:{text}", &salt[..30]),
        &format!("bc2:1000:{salt}:{}:{text}", &nonce[..22]),
        &format!("bc2:1000:{salt}:{nonce}:00"),
        &format!("bc2:1000:{salt}:{nonce}:{text}:extra"),
        &sealed.to_uppercase(),
        &format!("bc2:0:{salt}:{nonce}:{text}"),
        &format!("bc2:99999999999:{salt}:{nonce}:{text}"),
        huge.as_str(),
    ] {
        assert!(!seal::well_formed(bad), "{bad:?}");
        assert!(seal::open_passphrase(PASSPHRASE, bad).is_err(), "{bad:?}");
    }
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert!(!seal::needs_passphrase("") && !seal::needs_passphrase("bc1:1000:00:00:00") && !seal::needs_passphrase("garbage"));
}

// ---------- thinking, the checklist, files and tokens ----------

#[test]
fn every_provider_gives_its_thinking_apart_from_the_answer_and_asks_for_it() {
    let a = anthropic_reply(
        json!([{"type": "thinking", "thinking": "Levels first.", "signature": "s"}, {"type": "text", "text": "Hi"}]),
        "end_turn",
    );
    let r = anthropic::parse(200, &a).unwrap();
    assert_eq!((r.thinking.as_str(), r.text.as_str()), ("Levels first.", "Hi"));
    let body: Value = serde_json::from_str(&anthropic::request("claude-sonnet-5-5", KEY, &[Msg::user("x")]).body).unwrap();
    assert_eq!(body["thinking"], json!({"type": "adaptive", "display": "summarized"}), "the default hides the summary");

    let o = openai_reply(
        json!([{"type": "reasoning", "summary": [{"type": "summary_text", "text": "One."}, {"type": "summary_text", "text": "Two."}]},
               {"type": "message", "content": [{"type": "output_text", "text": "Hi"}]}]),
        "completed",
    );
    let r = openai::parse(200, &o).unwrap();
    assert_eq!((r.thinking.as_str(), r.text.as_str()), ("One.\n\nTwo.", "Hi"));
    let body: Value = serde_json::from_str(&openai::request("gpt-6-astra", KEY, &[Msg::user("x")]).body).unwrap();
    assert_eq!(body["reasoning"]["summary"], "auto");

    let g = gemini_reply(json!([{"text": "Checking.", "thought": true}, {"text": "Hi"}]), "STOP");
    let r = gemini::parse(200, &g).unwrap();
    assert_eq!((r.thinking.as_str(), r.text.as_str()), ("Checking.", "Hi"));
    let body: Value = serde_json::from_str(&gemini::request("gemini-3.8-flash", KEY, &[Msg::user("x")]).body).unwrap();
    assert_eq!(body["generationConfig"]["thinkingConfig"]["includeThoughts"], true);
}

#[test]
fn every_provider_counts_the_tokens_of_a_reply_in_its_own_fields() {
    let a = json!({"content": [], "stop_reason": "end_turn", "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "cache_creation_input_tokens": 5, "output_tokens": 7}});
    assert_eq!(anthropic::parse(200, &a.to_string()).unwrap().usage, Usage { input: 105, output: 7 });
    let o = json!({"output": [], "status": "completed", "usage": {"input_tokens": 40, "output_tokens": 9}});
    assert_eq!(openai::parse(200, &o.to_string()).unwrap().usage, Usage { input: 40, output: 9 });
    let g = json!({"candidates": [{"finishReason": "STOP", "content": {"parts": []}}], "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 4, "thoughtsTokenCount": 6}});
    assert_eq!(gemini::parse(200, &g.to_string()).unwrap().usage, Usage { input: 30, output: 10 });
    // A reply with no count, or a hostile one, counts as none and never panics.
    let hostile = json!({"content": [], "usage": {"input_tokens": "lots", "output_tokens": -1}});
    assert_eq!(anthropic::parse(200, &hostile.to_string()).unwrap().usage, Usage::default());
    assert_eq!(Usage { input: u64::MAX, output: 1 }.total(), u64::MAX);
}

#[test]
fn files_go_before_the_words_in_each_providers_own_form() {
    let files = vec![
        Attached { name: "notes.txt".into(), body: Body::Text("two walls".into()) },
        Attached { name: "plan.png".into(), body: Body::Image { mime: "image/png", b64: "AAAA".into() } },
    ];
    let history = [Msg::User("what is this?".into(), files)];
    let body = |r: Request| serde_json::from_str::<Value>(&r.body).unwrap();
    let a = body(anthropic::request("claude-sonnet-5-5", KEY, &history));
    assert_eq!(a["messages"][0]["content"][0], json!({"type": "text", "text": "<file name=\"notes.txt\">\ntwo walls\n</file>"}));
    assert_eq!(
        a["messages"][0]["content"][1],
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}})
    );
    assert_eq!(a["messages"][0]["content"][2], json!({"type": "text", "text": "what is this?"}));
    let o = body(openai::request("gpt-6-astra", KEY, &history));
    assert_eq!(o["input"][0]["content"][1], json!({"type": "input_image", "image_url": "data:image/png;base64,AAAA"}));
    assert_eq!(o["input"][0]["content"][2], json!({"type": "input_text", "text": "what is this?"}));
    let g = body(gemini::request("gemini-3.8-flash", KEY, &history));
    assert_eq!(g["contents"][0]["parts"][1], json!({"inlineData": {"mimeType": "image/png", "data": "AAAA"}}));
    assert_eq!(g["contents"][0]["parts"][2], json!({"text": "what is this?"}));
    assert!(!a.to_string().contains(KEY));
}

#[test]
fn base64_is_the_standard_one_with_padding() {
    for (plain, coded) in
        [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
    {
        assert_eq!(base64(plain.as_bytes()), coded);
    }
    assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
}

#[test]
fn a_checklist_is_read_whole_and_a_bad_one_is_told_what_is_wrong() {
    let ok = tools::todos(&json!({"todos": [{"content": " Draw walls ", "status": "in_progress"}, {"content": "Add doors", "status": "pending"}, {"content": "x".repeat(500), "status": "completed"}]})).unwrap();
    assert_eq!(
        (ok[0].content.as_str(), ok[0].status, ok[1].status, ok[2].status),
        ("Draw walls", Status::InProgress, Status::Pending, Status::Completed)
    );
    assert_eq!(ok[2].content.chars().count(), crate::MAX_TODO_TEXT + 1, "cut, with its ellipsis");
    for bad in [
        json!({}),
        json!({"todos": "all of it"}),
        json!({"todos": [{"content": "", "status": "pending"}]}),
        json!({"todos": [{"content": "a", "status": "done"}]}),
        json!({"todos": (0..=crate::MAX_TODOS).map(|i| json!({"content": format!("step {i}"), "status": "pending"})).collect::<Vec<_>>()}),
    ] {
        assert!(tools::todos(&bad).is_err(), "{bad}");
    }
    // The model is offered it, and it changes nothing in the project.
    assert!(tools::all().iter().any(|t| t.name == tools::TODO));
    let mut s = Session::new();
    run(&mut s, "todo_write", json!({"todos": [{"content": "a", "status": "pending"}]})).unwrap();
    assert_eq!(s.history(), Vec::<String>::new());
}

#[test]
fn the_checklist_is_kept_for_the_palette_not_logged_as_a_call_and_cleared_by_the_next_message() {
    let list = r#"{"todos":[{"content":"Draw walls","status":"in_progress"},{"content":"Add doors","status":"pending"}]}"#;
    let net = Arc::new(
        Fake::default()
            .reply(200, tool_reply(&[("t1", "todo_write", list), ("t2", "execute", WALL)]))
            .reply(200, text_reply("Done."))
            .reply(200, text_reply("Again.")),
    );
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("two walls and doors", KEY);
    drive(&mut chat, &mut s);
    assert_eq!(chat.todos.len(), 2);
    assert_eq!((chat.todos[0].content.as_str(), chat.todos[0].status), ("Draw walls", Status::InProgress));
    let used: Vec<_> = chat.log.iter().filter_map(|l| if let Line::Used { name, .. } = l { Some(name.as_str()) } else { None }).collect();
    assert_eq!(used, ["execute"], "the checklist is not a line of the log");
    assert_eq!(count(&mut s, "wall"), 1);
    chat.send("and again", KEY);
    assert!(chat.todos.is_empty(), "a new message starts a new list");
    drive(&mut chat, &mut s);
    // A list that is refused is shown as the failed call it is.
    let net = Arc::new(
        Fake::default()
            .reply(200, tool_reply(&[("t1", "todo_write", r#"{"todos":[{"content":"a","status":"nope"}]}"#)]))
            .reply(200, text_reply("ok")),
    );
    let mut chat = chat_on(&net);
    chat.send("plan", KEY);
    drive(&mut chat, &mut s);
    assert!(chat.todos.is_empty() && chat.log.iter().any(|l| matches!(l, Line::Used { name, error: true, .. } if name == "todo_write")));
}

#[test]
fn thinking_comes_before_the_answer_in_the_log_and_the_tokens_are_handed_over_once() {
    let first = anthropic_reply(
        json!([{"type": "thinking", "thinking": "Look first.", "signature": "s"}, {"type": "tool_use", "id": "t1", "name": "inspect_model", "input": {}}]),
        "tool_use",
    );
    let net = Arc::new(Fake::default().reply(200, first).reply(200, text_reply("Two levels.")));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.send("what is in it?", KEY);
    drive(&mut chat, &mut s);
    let kinds: Vec<&str> = chat
        .log
        .iter()
        .map(|l| match l {
            Line::You(_) => "you",
            Line::Thinks(_) => "thinks",
            Line::Says(_) => "says",
            Line::Used { .. } => "used",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["you", "thinks", "used", "says"]);
    assert_eq!(chat.take_usage(), Usage { input: 20, output: 10 }, "two replies of 10 and 5 tokens each");
    assert_eq!(chat.take_usage(), Usage::default(), "handed over once");
}

/// A folder with one file of each kind a user might pick.
fn picked() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "two walls").unwrap();
    std::fs::write(dir.path().join("plan.png"), [0x89, b'P', b'N', b'G']).unwrap();
    std::fs::write(dir.path().join("data.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    std::fs::write(dir.path().join("big.txt"), "x".repeat(crate::MAX_FILE_TEXT as usize + 1)).unwrap();
    std::fs::create_dir(dir.path().join("folder")).unwrap();
    dir
}

#[test]
fn a_picked_file_is_copied_to_a_folder_of_the_chats_and_what_is_not_sendable_is_refused_with_a_note() {
    let (from, net) = (picked(), Arc::new(Fake::default()));
    let mut chat = chat_on(&net);
    assert!(chat.stage_dir().is_none(), "no folder until a file is picked");
    chat.attach(&from.path().join("notes.txt"));
    chat.attach(&from.path().join("plan.png"));
    let stage = chat.stage_dir().unwrap().to_path_buf();
    assert_eq!(chat.attached.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["notes.txt", "plan.png"]);
    assert!(chat.attached.iter().all(|a| std::fs::metadata(&stage).is_ok() && a.size > 0));
    assert_eq!(std::fs::read_dir(&stage).unwrap().count(), 2);
    // The copy is the chat's: the original may go.
    std::fs::remove_file(from.path().join("notes.txt")).unwrap();
    for (bad, why) in [("data.bin", "not text"), ("big.txt", "at most"), ("folder", "not a file"), ("missing.txt", "could not read")] {
        let before = chat.attached.len();
        chat.attach(&from.path().join(bad));
        assert_eq!(chat.attached.len(), before, "{bad}");
        assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains(why)), "{bad}: {:?}", chat.log.last());
    }
    for _ in 0..MAX_FILES {
        chat.attach(&from.path().join("plan.png"));
    }
    assert_eq!(chat.attached.len(), MAX_FILES);
    assert!(matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("at most go with one message")));
    chat.detach(0);
    assert_eq!(chat.attached.len(), MAX_FILES - 1);
    assert_eq!(std::fs::read_dir(&stage).unwrap().count(), MAX_FILES - 1, "the removed one's copy is deleted");
}

#[test]
fn a_file_name_that_could_end_the_block_early_is_cleaned() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a\"><b.txt"), "x").unwrap();
    let mut chat = chat_on(&Arc::new(Fake::default()));
    chat.attach(&dir.path().join("a\"><b.txt"));
    assert_eq!(chat.attached[0].name, "a___b.txt");
}

#[test]
fn files_go_with_the_message_and_their_folder_goes_with_the_chat() {
    let (from, net) = (picked(), Arc::new(Fake::default().reply(200, text_reply("Two walls and a picture."))));
    let (mut chat, mut s) = (chat_on(&net), Session::new());
    chat.attach(&from.path().join("notes.txt"));
    chat.attach(&from.path().join("plan.png"));
    let stage = chat.stage_dir().unwrap().to_path_buf();
    chat.send("what is in these?", KEY);
    drive(&mut chat, &mut s);
    assert!(chat.attached.is_empty(), "sent files are not sent again");
    assert!(chat.log.contains(&Line::Files(vec!["notes.txt".into(), "plan.png".into()])));
    let body: Value = serde_json::from_str(&net.seen()[0].body).unwrap();
    let content = &body["messages"][0]["content"];
    assert!(content[0]["text"].as_str().unwrap().contains("<file name=\"notes.txt\">\ntwo walls\n</file>"));
    assert_eq!(content[1]["source"]["data"], base64(&[0x89, b'P', b'N', b'G']));
    assert_eq!(content[2]["text"], "what is in these?");
    assert!(stage.exists(), "the copies stay until the chat goes");
    chat.clear();
    assert!(!stage.exists() && chat.stage_dir().is_none(), "a new chat removes the folder");
    // Dropping the chat, as the window closing does, removes it too.
    chat.attach(&from.path().join("plan.png"));
    let stage = chat.stage_dir().unwrap().to_path_buf();
    assert!(stage.exists());
    drop(chat);
    assert!(!stage.exists(), "closing removes the folder");
}

#[test]
fn more_file_bytes_than_a_conversation_may_carry_are_refused() {
    let (from, net) = (picked(), Arc::new(Fake::default()));
    let mut chat = chat_on(&net);
    chat.history.push(Msg::User("x".into(), vec![Attached { name: "old.txt".into(), body: Body::Text("y".repeat(MAX_FILES_TOTAL)) }]));
    chat.attach(&from.path().join("notes.txt"));
    chat.send("again", KEY);
    assert!(!chat.busy() && matches!(chat.log.last(), Some(Line::Note(n)) if n.contains("more than can be sent")));
    assert_eq!(chat.attached.len(), 1, "what was picked is still there to send after a new chat");
}

// ---------- chats kept for each project ----------

/// A chat that has had a turn: a message with a text file and a picture, thinking, a call, a checklist and an answer.
fn had_a_turn(net: &Arc<Fake>) -> (Chat, Session) {
    let (from, mut chat, mut s) = (picked(), chat_on(net), Session::new());
    chat.attach(&from.path().join("notes.txt"));
    chat.attach(&from.path().join("plan.png"));
    chat.send("two walls", KEY);
    drive(&mut chat, &mut s);
    (chat, s)
}

fn first_turn_net() -> Arc<Fake> {
    let first = anthropic_reply(
        json!([{"type": "thinking", "thinking": "Walls first.", "signature": "s"},
               {"type": "tool_use", "id": "t1", "name": "todo_write", "input": {"todos": [{"content": "Draw", "status": "in_progress"}]}},
               {"type": "tool_use", "id": "t2", "name": "execute", "input": serde_json::from_str::<Value>(WALL).unwrap()}]),
        "tool_use",
    );
    Arc::new(Fake::default().reply(200, first).reply(200, text_reply("Drew it.")).reply(200, text_reply("More.")))
}

#[test]
fn a_saved_chat_comes_back_as_it_was_and_the_model_is_sent_the_same_conversation_again() {
    let net = first_turn_net();
    let (chat, mut s) = had_a_turn(&net);
    let project = std::path::Path::new("/work/house.bimc");
    let saved = chat.to_saved(project);
    assert!(!saved.to_string().contains(KEY), "the key is in none of it");
    let mut back = chat_on(&net);
    assert!(back.restore(&saved, Provider::Anthropic));
    assert_eq!(back.log, chat.log);
    assert_eq!((back.todos.clone(), back.model.clone()), (chat.todos.clone(), chat.model.clone()));
    assert_eq!(back.history.len(), chat.history.len());
    // A picture is not kept, its place says so; the text file is.
    let Msg::User(_, files) = &back.history[0] else { panic!("first is the user's") };
    assert_eq!(files[0].body, Body::Text("two walls".into()));
    assert!(matches!(&files[1].body, Body::Text(t) if t.contains("plan.png") && t.contains("not kept")));
    // The next message goes out after what was said before, the model's own turn (its thinking block) untouched.
    back.send("and more", KEY);
    drive(&mut back, &mut s);
    let body: Value = serde_json::from_str(&net.seen().last().unwrap().body).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[1]["content"][0]["type"], "thinking");
    assert_eq!(messages.last().unwrap()["content"], "and more");
}

#[test]
fn a_chat_had_with_another_provider_is_shown_and_not_continued() {
    let net = first_turn_net();
    let (chat, _s) = had_a_turn(&net);
    let saved = chat.to_saved(std::path::Path::new("/work/house.bimc"));
    let mut back = chat_on(&net);
    assert!(back.restore(&saved, Provider::OpenAi));
    assert!(back.history.is_empty(), "another provider's reasoning means nothing to this one");
    assert_eq!(back.log.len(), chat.log.len() + 1);
    assert!(matches!(back.log.last(), Some(Line::Note(n)) if n.contains("Anthropic Claude") && n.contains("OpenAI")));
}

#[test]
fn a_file_that_is_not_a_saved_chat_is_refused_whole_and_never_panics() {
    let net = first_turn_net();
    let (chat, _s) = had_a_turn(&net);
    let good = chat.to_saved(std::path::Path::new("/p.bimc"));
    let mut bad = vec![json!(null), json!([]), json!("x"), json!({}), json!({"v": 99, "log": [], "history": [], "todos": []})];
    for key in ["log", "history", "todos"] {
        let mut v = good.clone();
        v[key] = json!("not a list");
        bad.push(v);
        let mut v = good.clone();
        v[key] = json!([{"t": "nonsense"}]);
        bad.push(v);
        let mut v = good.clone();
        v[key] = json!([5, null]);
        bad.push(v);
    }
    let mut v = good.clone();
    v["log"][0]["text"] = json!(7);
    bad.push(v);
    let mut v = good.clone();
    v["todos"] = json!([{"content": "a", "status": "nope"}]);
    bad.push(v);
    let mut v = good.clone();
    v["history"] = json!([{"t": "user", "text": "x".repeat(crate::MAX_HISTORY + 1), "files": []}]);
    bad.push(v);
    let mut v = good.clone();
    v["log"] = json!((0..501).map(|_| json!({"t": "says", "text": "a"})).collect::<Vec<_>>());
    bad.push(v);
    for v in bad {
        let mut c = chat_on(&net);
        c.log.push(Line::Note("mine".into()));
        assert!(!c.restore(&v, Provider::Anthropic), "{v}");
        assert_eq!(c.log, vec![Line::Note("mine".into())], "a refused file changes nothing");
    }
}

#[test]
fn a_chat_is_written_into_a_folder_by_its_projects_path_read_back_and_removed() {
    let net = first_turn_net();
    let (chat, _s) = had_a_turn(&net);
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (std::path::Path::new("/work/a.bimc"), std::path::Path::new("/work/b.bimc"));
    assert_ne!(crate::saved::file_name(a), crate::saved::file_name(b));
    assert_eq!(crate::saved::file_name(a).len(), 64 + ".json".len());
    assert!(crate::saved::read(dir.path(), a).is_none());
    crate::saved::write(&dir.path().join("chats"), a, &chat).unwrap();
    let kept = crate::saved::read(&dir.path().join("chats"), a).unwrap();
    assert_eq!(kept["project"], "/work/a.bimc");
    assert!(crate::saved::read(&dir.path().join("chats"), b).is_none(), "each project has its own");
    assert_eq!(std::fs::read_dir(dir.path().join("chats")).unwrap().count(), 1, "no half-written file is left beside it");
    crate::saved::remove(&dir.path().join("chats"), a);
    assert!(crate::saved::read(&dir.path().join("chats"), a).is_none());
    // A file past the cap is not read.
    std::fs::create_dir_all(dir.path().join("big")).unwrap();
    let big = std::fs::File::create(dir.path().join("big").join(crate::saved::file_name(a))).unwrap();
    big.set_len(crate::saved::MAX_SAVED + 1).unwrap();
    assert!(crate::saved::read(&dir.path().join("big"), a).is_none());
}
