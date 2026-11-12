//! Co-Work (COWORK-PLAN.md): what live co-editing needs of the engine, with no network. The host's session holds the one true
//! model. A guest's command runs on it through [`run_for_guest`], and what each command changed reaches a guest's copy as a
//! [`Delta`], which [`apply`] takes in. A guest's window runs what only reads on its own copy and sends the rest ([`place`]).
//! Each person undoes their own edits (`Session::author`, the `undo` command). The sockets, the handshake and the people are
//! the desktop's.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{REPLACES, Session, find, touches_files};
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
    /// Something besides the elements changed (a type, a material, a setting): the whole model, as a file holds it, and the
    /// models its links read, by their paths, which a file never holds and a guest's disk hasn't.
    Whole { model: String, links: BTreeMap<String, String> },
}

/// Where a guest's window runs a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// On its own copy: it only reads (an export writes the guest's own disk).
    Here,
    /// On the host's model: it changes it.
    Host,
    /// Not in a session, and why.
    Refused(String),
}

/// Where a guest runs `command` with `params`: what changes the project goes to the host, what only reads runs on the copy, and
/// what would replace the project, or bring a file of the guest's into it, isn't done.
pub fn place(command: &str, params: &Value) -> Place {
    let Some(c) = find(command) else { return Place::Host };
    if REPLACES.contains(&c.id) {
        return Place::Refused(format!("`{}` would replace the project: leave the Co-Work session first", c.id));
    }
    let files = touches_files(c, params).is_some();
    if c.undoable && files {
        return Place::Refused(format!(
            "`{}` would bring a file on your machine into the project, which in a Co-Work session the host does",
            c.id
        ));
    }
    if (c.undoable || matches!(c.id, "undo" | "redo")) && !files { Place::Host } else { Place::Here }
}

/// The whole of `m`, for a guest that joins or has lost step.
pub fn whole(m: &Model) -> Result<Delta, String> {
    let links = m.loaded.iter().map(|(path, l)| Ok((path.clone(), l.to_json()?))).collect::<Result<_, String>>()?;
    Ok(Delta::Whole { model: m.to_json()?, links })
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
    let Changed::Ids(ids) = after.changed_since(before) else { return whole(after) };
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
        Delta::Whole { model, links } => {
            let read = |text: &str| {
                let (m, left_out) = Model::read(text)?;
                match left_out.is_empty() {
                    true => Ok(m),
                    false => Err(format!("the host's model holds what this version doesn't know: {}", left_out.join(", "))),
                }
            };
            let mut m = read(model)?;
            for (path, text) in links {
                m.loaded.insert(path.clone(), Arc::new(read(text).map_err(|e| format!("the link {path}: {e}"))?));
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
    may(command, edit)?;
    as_guest(session, guest, made_in, edit, |s| s.execute(command, params))
}

/// [`run_for_guest`] of several commands as one undo step named `label`, the guest's, all or nothing.
pub fn batch_for_guest(
    session: &mut Session,
    guest: &str,
    label: &str,
    commands: &[(String, Value)],
    made_in: (Option<String>, Option<String>),
    edit: bool,
) -> Result<Vec<Value>, String> {
    commands.iter().try_for_each(|(c, _)| may(c, edit))?;
    as_guest(session, guest, made_in, edit, |s| s.transaction(label, |s| commands.iter().map(|(c, p)| s.execute(c, p)).collect()))
}

/// What a guest may not ask whatever its params: to replace the project, or to change it while the host lets guests only look.
fn may(command: &str, edit: bool) -> Result<(), String> {
    let found = find(command);
    if let Some(c) = found.filter(|c| REPLACES.contains(&c.id)) {
        return Err(format!("`{}` would replace the project the host has open: a guest leaves the session to open another", c.id));
    }
    if !edit && found.is_none_or(|c| c.undoable || matches!(c.id, "undo" | "redo")) {
        return Err(format!("`{command}` would change the project, and {LOOK}"));
    }
    Ok(())
}

/// `f` run on the session as `guest`, kept from files and, when `edit` is false, only reading; then the session as it was.
fn as_guest<T>(
    session: &mut Session,
    guest: &str,
    made_in: (Option<String>, Option<String>),
    edit: bool,
    f: impl FnOnce(&mut Session) -> Result<T, String>,
) -> Result<T, String> {
    let author = session.author.replace(guest.to_string());
    let (phase, option) = (std::mem::replace(&mut session.phase, made_in.0), std::mem::replace(&mut session.option, made_in.1));
    let no_files = std::mem::replace(&mut session.no_files, true);
    let result = if edit { f(session) } else { session.reading(LOOK, f) };
    (session.author, session.phase, session.option, session.no_files) = (author, phase, option, no_files);
    result
}
