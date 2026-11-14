//! The chat as it goes between the windows of a Co-Work session (`bimcraft_cowork`): the log and the checklist the guests are
//! shown, a guest's message's files, the conversation a turn sends to the window whose key pays, and its reply. The forms of a
//! saved chat (`saved.rs`), with the pictures kept, read as strictly: what another window sends is hostile, and a part that isn't
//! what it should be refuses the whole.

use serde_json::Value;

use crate::saved::{MAX_LINES, file_from, file_json, line_from, line_json, msg_from, msg_json_with, todo_from, todo_json};
use crate::{Attached, Line, MAX_FILES, MAX_FILES_TOTAL, MAX_HISTORY, MAX_TODOS, Msg, Reply, Todo};

fn list<T>(v: &Value, max: usize, read: impl Fn(&Value) -> Option<T>) -> Option<Vec<T>> {
    v.as_array().filter(|l| l.len() <= max)?.iter().map(read).collect()
}

pub fn log(lines: &[Line]) -> Value {
    lines.iter().map(line_json).collect()
}

pub fn log_from(v: &Value) -> Option<Vec<Line>> {
    list(v, MAX_LINES, line_from)
}

pub fn todos(todos: &[Todo]) -> Value {
    todos.iter().map(todo_json).collect()
}

pub fn todos_from(v: &Value) -> Option<Vec<Todo>> {
    list(v, MAX_TODOS, todo_from)
}

pub fn files(files: &[Attached]) -> Value {
    files.iter().map(|f| file_json(f, true)).collect()
}

pub fn files_from(v: &Value) -> Option<Vec<Attached>> {
    list(v, MAX_FILES, file_from)
}

pub fn history(history: &[Msg]) -> Value {
    history.iter().map(|m| msg_json_with(m, true)).collect()
}

/// A conversation, within what one sends at once.
pub fn history_from(v: &Value) -> Option<Vec<Msg>> {
    let h = list(v, usize::MAX, msg_from)?;
    let (sent, files) = (h.iter().map(Msg::size).sum::<usize>(), h.iter().map(Msg::file_bytes).sum::<usize>());
    (sent <= MAX_HISTORY && files <= MAX_FILES_TOTAL).then_some(h)
}

pub fn reply(r: &Reply) -> Value {
    msg_json_with(&Msg::Assistant(r.clone()), true)
}

pub fn reply_from(v: &Value) -> Option<Reply> {
    match msg_from(v)? {
        Msg::Assistant(r) => Some(r),
        _ => None,
    }
}
