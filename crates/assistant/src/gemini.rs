//! Google's Gemini API (`POST /v1beta/models/{model}:generateContent`) with function calling: the conversation goes in as
//! `contents` of `user` and `model` turns, the tools as `functionDeclarations`, a `functionCall` part comes back, and the answer is
//! a `functionResponse` part in a user turn, with the call's `id` when it had one. The model's own turn goes back whole (its
//! `thoughtSignature` must come back in the part it came in). Declarations take a subset of OpenAPI, which has no free-form
//! object: [`schema`] writes one as text that is a JSON object, and the tools read it back (`tools::run`).

use serde_json::{Value, json};

use crate::net::Request;
use crate::{Call, Msg, Reply, SYSTEM, api_error, clip, json_body, tools};

pub fn request(model: &str, key: &str, history: &[Msg]) -> Request {
    let contents: Vec<Value> = history
        .iter()
        .map(|m| match m {
            Msg::User(text) => json!({"role": "user", "parts": [{"text": text}]}),
            Msg::Assistant(reply) => reply.raw.clone(),
            Msg::Results(done) => json!({"role": "user", "parts": done.iter().map(|d| {
                let mut call = json!({"name": d.name, "response": if d.error { json!({"error": d.text}) } else { json!({"result": d.text}) }});
                if !d.id.is_empty() {
                    call["id"] = json!(d.id);
                }
                json!({"functionResponse": call})
            }).collect::<Vec<_>>()}),
        })
        .collect();
    let declarations: Vec<Value> = tools::all()
        .into_iter()
        .map(|t| {
            let mut declaration = json!({"name": t.name, "description": t.about});
            // A function with nothing to pass declares no parameters: an object with no properties is refused.
            if t.schema["properties"].as_object().is_some_and(|p| !p.is_empty()) {
                declaration["parameters"] = schema(&t.schema);
            }
            declaration
        })
        .collect();
    let body = json!({
        "systemInstruction": {"parts": [{"text": SYSTEM}]},
        "contents": contents,
        "tools": [{"functionDeclarations": declarations}],
    });
    Request {
        url: format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"),
        headers: vec![("x-goog-api-key".into(), key.into())],
        body: body.to_string(),
    }
}

/// A tool's JSON Schema as Gemini takes it: an object with no properties (a free-form one) is declared as a string that holds
/// a JSON object, and `additionalProperties` is left out.
fn schema(v: &Value) -> Value {
    let Some(map) = v.as_object() else { return v.clone() };
    if map.get("type").and_then(Value::as_str) == Some("object")
        && !map.get("properties").and_then(Value::as_object).is_some_and(|p| !p.is_empty())
    {
        let said = map.get("description").and_then(Value::as_str).unwrap_or("a JSON object");
        return json!({"type": "string", "description": format!("{said} (written as text: a JSON object)")});
    }
    map.iter().filter(|(k, _)| *k != "additionalProperties").map(|(k, v)| (k.clone(), schema(v))).collect::<serde_json::Map<_, _>>().into()
}

pub fn parse(status: u16, body: &str) -> Result<Reply, String> {
    const WHO: &str = "Gemini";
    if !(200..300).contains(&status) {
        return Err(api_error(WHO, status, body));
    }
    let v = json_body(WHO, body)?;
    let Some(candidate) = v["candidates"].as_array().and_then(|c| c.first()) else {
        let why = v["promptFeedback"]["blockReason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default();
        return Err(format!("{WHO} blocked the request or sent no answer{why}"));
    };
    let finish = candidate["finishReason"].as_str().unwrap_or("STOP");
    let (mut text, mut calls) = (String::new(), Vec::new());
    for part in candidate["content"]["parts"].as_array().into_iter().flatten() {
        if let Some(call) = part.get("functionCall") {
            let Some(name) = call["name"].as_str() else { return Err(format!("{WHO} sent a tool call with no name")) };
            let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            calls.push(Call { id: call["id"].as_str().unwrap_or_default().into(), name: name.into(), args });
        } else if let Some(t) = part["text"].as_str().filter(|_| part["thought"] != true) {
            text.push_str(t);
        }
    }
    match finish {
        "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" | "RECITATION" | "IMAGE_SAFETY" => {
            return Err(format!("{WHO} declined this request ({})", clip(finish, 40)));
        }
        "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => {
            return Err(format!("{WHO} wrote a tool call it could not send ({finish}): try again"));
        }
        "MAX_TOKENS" if !calls.is_empty() => {
            return Err(format!("{WHO}'s reply was cut off at its length limit, in the middle of a tool call: ask for less at once"));
        }
        "MAX_TOKENS" => text.push_str("\n\n(cut off at the length limit)"),
        _ => {}
    }
    let mut raw = candidate["content"].clone();
    if !raw.is_object() {
        raw = json!({"role": "model", "parts": []});
    }
    raw["role"] = json!("model");
    Ok(Reply { text, calls, raw })
}
