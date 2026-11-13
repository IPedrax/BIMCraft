//! Chats kept for each project. The window keeps one file for a project (named for a hash of the project file's path, in a `chats`
//! folder beside `settings.json`, never in the project, which is shared and sent around): the log the palette shows, the conversation
//! the model is sent again, and its checklist. It is written when a turn is over, read when the project is opened, and is only
//! as good as the file is trusted: a file is hostile (a copied settings folder, a hand's edit), so [`Chat::restore`] takes it whole or
//! not at all, within the caps a live chat has, and a chat of another provider is shown but not continued. The key is in none of it, and a
//! picture that went with a message is not kept (its place says so), so a saved chat stays small.

use std::fs;
use std::io;
use std::path::Path;

use ring::digest;
use serde_json::{Map, Value, json};

use crate::tools::{Status, Todo};
use crate::{Attached, Body, Call, Chat, Done, Line, MAX_HISTORY, Msg, Provider, Reply, Usage, clip, count};

/// The most a saved chat's file may be, and is read as: the conversation's own cap, with room for the log and what a model's reasoning
/// carries along.
pub const MAX_SAVED: u64 = 16 << 20;
/// The format of the file: a newer one is left alone.
const VERSION: u64 = 1;
/// The lines of a log that are kept, as a live chat keeps them.
pub(crate) const MAX_LINES: usize = 500;

/// The name of a project's file in the chats folder: the SHA-256 of its path, in hex.
pub fn file_name(project: &Path) -> String {
    let sum = digest::digest(&digest::SHA256, project.to_string_lossy().as_bytes());
    let hex: String = sum.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.json")
}

/// Writes the chat of `project` into `dir`, whole or not at all: a file is written beside it and moved into place.
pub fn write(dir: &Path, project: &Path, chat: &Chat) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let (to, tmp) = (dir.join(file_name(project)), dir.join(format!("{}.tmp", file_name(project))));
    fs::write(&tmp, chat.to_saved(project).to_string())?;
    fs::rename(&tmp, &to)
}

/// The saved chat of `project`, if there is a file within [`MAX_SAVED`] that is JSON.
pub fn read(dir: &Path, project: &Path) -> Option<Value> {
    let path = dir.join(file_name(project));
    if fs::metadata(&path).ok()?.len() > MAX_SAVED {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Takes the chat of `project` away: a new chat forgets it.
pub fn remove(dir: &Path, project: &Path) {
    let _ = fs::remove_file(dir.join(file_name(project)));
}

pub(crate) fn line_json(l: &Line) -> Value {
    match l {
        Line::You(t) => json!({"t": "you", "text": t}),
        Line::Files(names) => json!({"t": "files", "names": names}),
        Line::Thinks(t) => json!({"t": "thinks", "text": t}),
        Line::Says(t) => json!({"t": "says", "text": t}),
        Line::Used { name, args, result, error } => json!({"t": "used", "name": name, "args": args, "result": result, "error": error}),
        Line::Note(t) => json!({"t": "note", "text": t}),
    }
}

/// A file as JSON: a picture as its bytes in base64 with `pictures` (between windows), else as a note that it is not kept.
pub(crate) fn file_json(f: &Attached, pictures: bool) -> Value {
    match &f.body {
        Body::Text(t) => json!({"name": f.name, "text": t}),
        Body::Image { b64, .. } if pictures => json!({"name": f.name, "b64": b64}),
        Body::Image { .. } => {
            json!({"name": f.name, "text": format!("[the picture {} was sent with this message and is not kept]", f.name)})
        }
    }
}

/// A file read back: text, or a picture the providers take, its base64 of a size one may be.
pub(crate) fn file_from(f: &Value) -> Option<Attached> {
    let name = text(f, "name")?;
    let body = match (f.get("text"), f["b64"].as_str()) {
        (Some(t), None) => Body::Text(t.as_str()?.to_string()),
        (None, Some(b64)) => {
            let fits = b64.len() as u64 <= crate::MAX_FILE_IMAGE * 4 / 3 + 4;
            let base64 = b64.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'='));
            (fits && base64).then_some(())?;
            Body::Image { mime: crate::attach::image_mime(&name)?, b64: b64.to_string() }
        }
        _ => return None,
    };
    Some(Attached { name, body })
}

pub(crate) fn msg_json(m: &Msg) -> Value {
    msg_json_with(m, false)
}

pub(crate) fn msg_json_with(m: &Msg, pictures: bool) -> Value {
    match m {
        Msg::User(text, files) => {
            let files: Vec<Value> = files.iter().map(|f| file_json(f, pictures)).collect();
            json!({"t": "user", "text": text, "files": files})
        }
        Msg::Assistant(r) => {
            let calls: Vec<Value> = r.calls.iter().map(|c| json!({"id": c.id, "name": c.name, "args": c.args})).collect();
            json!({"t": "assistant", "thinking": r.thinking, "text": r.text, "calls": calls, "raw": r.raw, "usage": [r.usage.input, r.usage.output]})
        }
        Msg::Results(done) => {
            let done: Vec<Value> = done.iter().map(|d| json!({"id": d.id, "name": d.name, "text": d.text, "error": d.error})).collect();
            json!({"t": "results", "done": done})
        }
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_string)
}

pub(crate) fn line_from(v: &Value) -> Option<Line> {
    Some(match v["t"].as_str()? {
        "you" => Line::You(text(v, "text")?),
        "files" => Line::Files(v["names"].as_array()?.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<_>>()?),
        "thinks" => Line::Thinks(text(v, "text")?),
        "says" => Line::Says(text(v, "text")?),
        "used" => Line::Used { name: text(v, "name")?, args: text(v, "args")?, result: text(v, "result")?, error: v["error"].as_bool()? },
        "note" => Line::Note(text(v, "text")?),
        _ => return None,
    })
}

pub(crate) fn msg_from(v: &Value) -> Option<Msg> {
    Some(match v["t"].as_str()? {
        "user" => {
            let files = v["files"].as_array()?.iter().map(file_from).collect::<Option<_>>()?;
            Msg::User(text(v, "text")?, files)
        }
        "assistant" => {
            let calls = v["calls"]
                .as_array()?
                .iter()
                .map(|c| Some(Call { id: text(c, "id")?, name: text(c, "name")?, args: c["args"].clone() }))
                .collect::<Option<_>>()?;
            let usage = Usage { input: count(&v["usage"][0]), output: count(&v["usage"][1]) };
            Msg::Assistant(Reply { thinking: text(v, "thinking")?, text: text(v, "text")?, calls, raw: v["raw"].clone(), usage })
        }
        "results" => Msg::Results(
            v["done"]
                .as_array()?
                .iter()
                .map(|d| Some(Done { id: text(d, "id")?, name: text(d, "name")?, text: text(d, "text")?, error: d["error"].as_bool()? }))
                .collect::<Option<_>>()?,
        ),
        _ => return None,
    })
}

pub(crate) fn todo_from(v: &Value) -> Option<Todo> {
    let status = match v["status"].as_str()? {
        "pending" => Status::Pending,
        "in_progress" => Status::InProgress,
        "completed" => Status::Completed,
        _ => return None,
    };
    Some(Todo { content: clip(v["content"].as_str()?, crate::MAX_TODO_TEXT), status })
}

/// A saved chat read whole: what [`Chat::restore`] puts back.
struct Parsed {
    log: Vec<Line>,
    history: Vec<Msg>,
    todos: Vec<Todo>,
}

fn parse(v: &Value) -> Option<Parsed> {
    if v["v"].as_u64()? > VERSION {
        return None;
    }
    let log: Vec<Line> = v["log"].as_array()?.iter().map(line_from).collect::<Option<_>>()?;
    let history: Vec<Msg> = v["history"].as_array()?.iter().map(msg_from).collect::<Option<_>>()?;
    let todos: Vec<Todo> = v["todos"].as_array()?.iter().map(todo_from).collect::<Option<_>>()?;
    let sent: usize = history.iter().map(Msg::size).sum();
    let files: usize = history.iter().map(Msg::file_bytes).sum();
    (log.len() <= MAX_LINES && todos.len() <= crate::MAX_TODOS && sent <= MAX_HISTORY && files <= crate::MAX_FILES_TOTAL)
        .then_some(Parsed { log, history, todos })
}

impl Chat {
    /// The chat as the file of `project` keeps it.
    pub fn to_saved(&self, project: &Path) -> Value {
        let mut v = Map::new();
        v.insert("v".into(), json!(VERSION));
        v.insert("project".into(), json!(project.to_string_lossy()));
        v.insert("provider".into(), json!(self.provider.key()));
        v.insert("model".into(), json!(self.model));
        v.insert("log".into(), self.log.iter().map(line_json).collect());
        v.insert("history".into(), self.history.iter().map(msg_json).collect());
        v.insert("todos".into(), self.todos.iter().map(todo_json).collect());
        Value::Object(v)
    }

    /// Puts a saved chat back, whole or not at all (`false`, and the chat as it was, for a file that is not one, is too long or is of a
    /// newer version). The conversation goes back to the model only when it was had with `current`: another provider's reasoning and
    /// calls mean nothing to this one, so the log is shown and a note says the model starts afresh.
    pub fn restore(&mut self, v: &Value, current: Provider) -> bool {
        if self.busy() {
            return false;
        }
        let Some(p) = parse(v) else { return false };
        let same = v["provider"].as_str().and_then(Provider::from_key) == Some(current);
        (self.log, self.todos, self.history) = (p.log, p.todos, if same { p.history } else { Vec::new() });
        self.paid = None;
        if !same {
            let was = v["provider"].as_str().and_then(Provider::from_key).map_or("another provider", Provider::name);
            self.log.push(Line::Note(format!(
                "This chat was with {was}, not {}: shown here, and the model starts afresh from your next message.",
                current.name()
            )));
        }
        true
    }

    /// Changes whenever the log or the conversation does, which is when the saved file is out of date.
    pub fn revision(&self) -> usize {
        self.log.len() + self.history.len() * 1_000
    }
}

pub(crate) fn todo_json(t: &Todo) -> Value {
    let status = match t.status {
        Status::Pending => "pending",
        Status::InProgress => "in_progress",
        Status::Completed => "completed",
    };
    json!({"content": t.content, "status": status})
}
