//! Co-Work (COWORK-PLAN.md): what live co-editing needs of the engine, with no network. The host's session holds the one true
//! model. A guest's command runs on it through [`run_for_guest`], and what each command changed reaches a guest's copy as a
//! [`Delta`], which [`apply`] takes in. Each person undoes their own edits (`Session::author`, the `undo` command). The sockets,
//! the handshake and the people are the desktop's.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{REPLACES, Session, find};
use crate::ids::{Changed, Guid, LINKED};
use crate::model::{Element, Id, Model};

/// What a guest's edit is refused with while the host lets guests only look.
pub const LOOK: &str = "the host lets guests only look: the host lets them edit in the Co-Work dialog, with Guests may edit";

/// What changed from one model to the next, as a guest's copy takes it in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "lowercase")]
pub enum Delta {
    /// Only elements changed: each one as it is now, each id gone, and the id the next element will get.
    Elements { next_id: Id, put: Vec<Put>, gone: Vec<Id> },
    /// Something besides the elements changed (a type, a material, a setting): the whole model, as a file holds it.
    Whole { model: String },
}

/// An element as it is now, with the id and GUID it has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Put {
    pub id: Id,
    pub guid: Guid,
    pub element: Element,
}

/// What changed from `before` to `after`, for a copy that has `before`: the elements alone when nothing else changed, else the
/// whole model.
pub fn delta(before: &Model, after: &Model) -> Result<Delta, String> {
    let Changed::Ids(ids) = after.changed_since(before) else { return Ok(Delta::Whole { model: after.to_json()? }) };
    let (mut put, mut gone) = (Vec::new(), Vec::new());
    for id in ids {
        match (after.elements.get(id), after.elements.guid(id)) {
            (Some(e), Some(guid)) => {
                // Touched but the same needn't travel.
                if before.elements.get(id) != Some(e) || before.elements.guid(id) != Some(guid) {
                    put.push(Put { id, guid, element: e.clone() });
                }
            }
            _ if before.elements.contains_key(id) => gone.push(id),
            _ => {}
        }
    }
    Ok(Delta::Elements { next_id: after.next_id, put, gone })
}

/// `copy` with `delta` taken in, checked as a command's model is; an error, and `copy` as it was, when the delta doesn't fit it.
/// The host is as hostile as any file: what comes in is read strictly and must pass [`Model::check`].
pub fn apply(copy: &Model, delta: &Delta) -> Result<Model, String> {
    let (next_id, put, gone) = match delta {
        Delta::Whole { model } => {
            let (m, left_out) = Model::read(model)?;
            if !left_out.is_empty() {
                return Err(format!("the host's model holds what this version doesn't know: {}", left_out.join(", ")));
            }
            m.check()?;
            return Ok(m);
        }
        Delta::Elements { next_id, put, gone } => (*next_id, put, gone),
    };
    let mut m = copy.clone();
    for &id in gone {
        m.elements.remove(id).ok_or_else(|| format!("element {id} is gone, but this copy never had it: ask for the whole model"))?;
    }
    for p in put {
        if p.id >= LINKED {
            return Err(format!("element {} has an id past where a project's ids end", p.id));
        }
        m.elements.insert_as(p.id, p.guid, p.element.clone())?;
    }
    if next_id > LINKED || m.elements.keys().next_back().is_some_and(|last| *last >= next_id) {
        return Err(format!("the next id, {next_id}, isn't past every element's and below {LINKED}: ask for the whole model"));
    }
    m.next_id = next_id;
    m.check_changed(copy)?;
    Ok(m)
}

/// Runs a guest's command on the host's session: as `guest`, whose steps say so and whose `undo` takes back only theirs, in the
/// phase and option the guest makes things in (`made_in`), and only reading when the host lets guests only look (`edit`
/// false). A guest never replaces the project, and never reads or writes a file on the host's machine, however the command
/// asks (`Session::execute` refuses it while the guest's command runs, a plugin's too). The session is the host's again after.
pub fn run_for_guest(
    session: &mut Session,
    guest: &str,
    command: &str,
    params: &Value,
    made_in: (Option<String>, Option<String>),
    edit: bool,
) -> Result<Value, String> {
    let found = find(command);
    if let Some(c) = found.filter(|c| REPLACES.contains(&c.id)) {
        return Err(format!("`{}` would replace the project the host has open: a guest leaves the session to open another", c.id));
    }
    let changes = found.is_none_or(|c| c.undoable || matches!(c.id, "undo" | "redo"));
    if !edit && changes {
        return Err(format!("`{command}` would change the project, and {LOOK}"));
    }
    let author = session.author.replace(guest.to_string());
    let (phase, option) = (std::mem::replace(&mut session.phase, made_in.0), std::mem::replace(&mut session.option, made_in.1));
    let no_files = std::mem::replace(&mut session.no_files, true);
    let result = if edit { session.execute(command, params) } else { session.reading(LOOK, |s| s.execute(command, params)) };
    (session.author, session.phase, session.option, session.no_files) = (author, phase, option, no_files);
    result
}
