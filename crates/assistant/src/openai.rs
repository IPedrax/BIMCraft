//! OpenAI's Responses API (`POST /v1/responses`) with function tools. Not Chat Completions: OpenAI's current models (GPT-6 and
//! later) take tool calls only through the Responses API. The request is stateless (`store: false`, nothing is kept on OpenAI's
//! side for a later call): the conversation goes in as `input` items, the model's `output` items go back as they came (a reasoning
//! item is replayable only with its encrypted content, which is asked for), and a tool's answer is a `function_call_output`
//! item naming the `function_call`'s `call_id`.

use serde_json::{Value, json};

use crate::net::Request;
use crate::{Call, Msg, Reply, SYSTEM, Usage, api_error, clip, count, json_body, tools, user_parts};

const URL: &str = "https://api.openai.com/v1/responses";

pub fn request(model: &str, key: &str, history: &[Msg]) -> Request {
    let mut input: Vec<Value> = Vec::new();
    for m in history {
        match m {
            Msg::User(text, files) if files.is_empty() => input.push(json!({"role": "user", "content": text})),
            Msg::User(text, files) => input.push(json!({"role": "user", "content": user_parts(
                text,
                files,
                |t| json!({"type": "input_text", "text": t}),
                |mime, b64| json!({"type": "input_image", "image_url": format!("data:{mime};base64,{b64}")}),
            )})),
            // With nothing stored, a reasoning item without its encrypted content names an item OpenAI no longer has.
            Msg::Assistant(reply) => input.extend(
                reply
                    .raw
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| i["type"] != "reasoning" || i["encrypted_content"].is_string())
                    .cloned(),
            ),
            Msg::Results(done) => {
                input.extend(done.iter().map(|d| json!({"type": "function_call_output", "call_id": d.id, "output": d.text})))
            }
        }
    }
    // Not strict: strict mode wants every property required and no free-form object, and `execute`'s params is one.
    let tools: Vec<Value> = tools::all()
        .into_iter()
        .map(|t| json!({"type": "function", "name": t.name, "description": t.about, "parameters": t.schema, "strict": false}))
        .collect();
    let body = json!({
        "model": model, "store": false, "instructions": SYSTEM, "input": input, "tools": tools,
        "include": ["reasoning.encrypted_content"], "reasoning": {"summary": "auto"},
    });
    Request { url: URL.into(), headers: vec![("Authorization".into(), format!("Bearer {key}"))], body: body.to_string() }
}

pub fn parse(status: u16, body: &str) -> Result<Reply, String> {
    const WHO: &str = "OpenAI";
    if !(200..300).contains(&status) {
        return Err(api_error(WHO, status, body));
    }
    let v = json_body(WHO, body)?;
    let items = v["output"].as_array().ok_or_else(|| format!("{WHO} sent a reply with no output ({})", clip(body.trim(), 80)))?;
    let (mut text, mut thinking, mut calls) = (String::new(), String::new(), Vec::new());
    for item in items {
        match item["type"].as_str() {
            Some("message") => {
                for part in item["content"].as_array().into_iter().flatten() {
                    match part["type"].as_str() {
                        Some("output_text") => text.push_str(part["text"].as_str().unwrap_or_default()),
                        Some("refusal") => {
                            return Err(format!(
                                "{WHO} declined this request: {}",
                                clip(part["refusal"].as_str().unwrap_or_default(), 300)
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Some("reasoning") => {
                for part in item["summary"].as_array().into_iter().flatten() {
                    if let Some(t) = part["text"].as_str() {
                        if !thinking.is_empty() {
                            thinking.push_str("\n\n");
                        }
                        thinking.push_str(t);
                    }
                }
            }
            Some("function_call") => {
                let (Some(id), Some(name)) = (item["call_id"].as_str(), item["name"].as_str()) else {
                    return Err(format!("{WHO} sent a tool call with no id or name"));
                };
                // The arguments are JSON written as text, and a model may write it badly: what is not an object is answered as an error.
                let raw = item["arguments"].as_str().unwrap_or("{}");
                let args = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
                calls.push(Call { id: id.into(), name: name.into(), args });
            }
            _ => {}
        }
    }
    match v["status"].as_str() {
        Some("failed") => return Err(format!("{WHO} failed: {}", clip(v["error"]["message"].as_str().unwrap_or("no reason given"), 300))),
        Some("incomplete") if !calls.is_empty() => {
            return Err(format!(
                "{WHO}'s reply was cut off ({}), in the middle of a tool call: ask for less at once",
                v["incomplete_details"]["reason"].as_str().unwrap_or("no reason")
            ));
        }
        Some("incomplete") => {
            text.push_str(&format!("\n\n(cut off: {})", v["incomplete_details"]["reason"].as_str().unwrap_or("no reason")))
        }
        Some("completed") | None => {}
        Some(other) => return Err(format!("{WHO} answered with status {other}")),
    }
    let usage = Usage { input: count(&v["usage"]["input_tokens"]), output: count(&v["usage"]["output_tokens"]) };
    Ok(Reply { thinking, text, calls, raw: v["output"].clone(), usage })
}
