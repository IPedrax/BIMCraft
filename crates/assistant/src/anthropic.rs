//! Anthropic's Messages API (`POST /v1/messages`) with tool use, written from the public reference for raw HTTP: the
//! conversation goes in as `messages`, the tools as `tools` with an `input_schema`, a tool's answer comes back as a
//! `tool_result` block in a user message naming the `tool_use` block's id, and the assistant's `content` array goes back whole,
//! its thinking blocks included, because the API reads them only in the conversation that made them.

use serde_json::{Value, json};

use crate::net::Request;
use crate::{Call, Msg, Reply, SYSTEM, api_error, clip, json_body, tools};

const URL: &str = "https://api.anthropic.com/v1/messages";
/// The API's dated version of itself, as the current reference sends it. Nothing here sends a sampling parameter, which that version refuses.
const VERSION: &str = "2026-10-21";
/// Room for a model that thinks before it calls: an answer is a few lines, but a reply that runs out here is cut off.
const MAX_TOKENS: u32 = 16_000;

pub fn request(model: &str, key: &str, history: &[Msg]) -> Request {
    let messages: Vec<Value> = history
        .iter()
        .map(|m| match m {
            Msg::User(text) => json!({"role": "user", "content": text}),
            Msg::Assistant(reply) => json!({"role": "assistant", "content": reply.raw}),
            Msg::Results(done) => json!({"role": "user", "content": done.iter().map(|d| {
                json!({"type": "tool_result", "tool_use_id": d.id, "content": d.text, "is_error": d.error})
            }).collect::<Vec<_>>()}),
        })
        .collect();
    let tools: Vec<Value> =
        tools::all().into_iter().map(|t| json!({"name": t.name, "description": t.about, "input_schema": t.schema})).collect();
    let body = json!({"model": model, "max_tokens": MAX_TOKENS, "system": SYSTEM, "tools": tools, "messages": messages});
    Request {
        url: URL.into(),
        headers: vec![("x-api-key".into(), key.into()), ("anthropic-version".into(), VERSION.into())],
        body: body.to_string(),
    }
}

pub fn parse(status: u16, body: &str) -> Result<Reply, String> {
    const WHO: &str = "Anthropic";
    if !(200..300).contains(&status) {
        return Err(api_error(WHO, status, body));
    }
    let v = json_body(WHO, body)?;
    let blocks = v["content"].as_array().ok_or_else(|| format!("{WHO} sent a reply with no content ({})", clip(body.trim(), 80)))?;
    let (mut text, mut calls) = (String::new(), Vec::new());
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap_or_default()),
            Some("tool_use") => {
                let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) else {
                    return Err(format!("{WHO} sent a tool call with no id or name"));
                };
                calls.push(Call { id: id.into(), name: name.into(), args: block["input"].clone() });
            }
            _ => {}
        }
    }
    match v["stop_reason"].as_str() {
        Some("refusal") => {
            let why = v["stop_details"]["category"].as_str().map(|c| format!(" ({c})")).unwrap_or_default();
            return Err(format!("{WHO} declined this request{why}"));
        }
        // A cut-off call's input is a partial object that parses as a whole one: nothing of it runs.
        Some("max_tokens") if !calls.is_empty() => {
            return Err(format!("{WHO}'s reply was cut off at its length limit, in the middle of a tool call: ask for less at once"));
        }
        Some("max_tokens") => text.push_str("\n\n(cut off at the length limit)"),
        _ => {}
    }
    Ok(Reply { text, calls, raw: v["content"].clone() })
}
