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
            // Held to the memory this machine has free, as a file is (`memory.rs`); the wire held what came to it already.
            let read = |text: &str| {
                let budget = crate::memory::Budget::start(text.len() as u64, crate::memory::PROJECT)
                    .map_err(|e| format!("the host's project: {e}"))?
                    .unwrap_or_else(crate::memory::Budget::none);
                let (m, left_out) = Model::read_within(text, &budget).map_err(|e| match budget.check() {
                    Ok(()) => e,
                    Err(_) => format!("the host's project: {e}"),
                })?;
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

/// The commands a guest brings a file of its own into the project with. The file goes to the host, which reads it once from a
/// temporary folder of its own and keeps nothing of it but what the command makes; a link would point at that folder, so linking
/// is the host's.
pub const IMPORTS: [&str; 5] = ["insert", "import_cad", "import_mesh", "point_cloud", "import_fill_patterns"];
/// How a guest's window writes the file it sends: base64, as [`import_for_guest`] reads it.
pub use crate::commands::base64;

/// The biggest file a guest sends to be imported, in bytes.
pub const MAX_IMPORT: usize = 32 << 20;

/// The file on the guest's machine that `command` with `params` imports, when it is a guest's import: one of [`IMPORTS`], one
/// file at `path`, no link and nothing saved.
pub fn guest_import(command: &str, params: &Value) -> Option<String> {
    find(command).filter(|c| IMPORTS.contains(&c.id))?;
    let link = params.get("link").is_some_and(|l| *l == Value::Bool(true));
    let save = params.get("save").is_some_and(|v| !v.is_null());
    if link || save {
        return None;
    }
    params["path"].as_str().map(str::to_string)
}

/// A guest's import, as the host runs it: the file's `bytes` (base64) written as `name` in a temporary folder of the host's, the
/// command run on it as the guest's step, and the folder removed.
#[allow(clippy::too_many_arguments)]
pub fn import_for_guest(
    session: &mut Session,
    guest: &str,
    command: &str,
    params: &Value,
    name: &str,
    bytes: &str,
    made_in: (Option<String>, Option<String>),
    edit: bool,
) -> Result<Value, String> {
    let c = find(command)
        .filter(|_| guest_import(command, params).is_some())
        .ok_or_else(|| format!("`{command}` isn't a file a guest imports: the host links, saves and opens"))?;
    may(c.id, edit)?;
    let bytes = crate::texture::unbase64(bytes)
        .filter(|b| b.len() <= MAX_IMPORT)
        .ok_or_else(|| format!("the file didn't come whole, or is bigger than {} MB", MAX_IMPORT >> 20))?;
    // Only the file's own name, of plain characters: never a path into the host's disk.
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let name: String = base.chars().filter(|c| c.is_alphanumeric() || "._- ()".contains(*c)).take(100).collect();
    if name.trim_start_matches('.').is_empty() {
        return Err("the file has no name the host can keep it by".into());
    }
    let dir = tempfile::Builder::new().prefix("bimcraft-cowork-").tempdir().map_err(|e| format!("the host couldn't keep the file: {e}"))?;
    let path = dir.path().join(&name);
    std::fs::write(&path, bytes).map_err(|e| format!("the host couldn't keep the file: {e}"))?;
    let mut params = params.clone();
    params["path"] = Value::String(path.to_string_lossy().into_owned());
    as_guest(session, guest, made_in, edit, |s| {
        // This one file, which the guest sent: the fence is back up after it.
        s.no_files = false;
        s.execute(c.id, &params)
    })
}

/// A guest's script (Manage > Run Script) as one undo step, `Script`, the guest's: run only once its dry run passes, which refuses
/// a line that would replace the project, undo, redo or touch a file before anything runs.
pub fn script_for_guest(
    session: &mut Session,
    guest: &str,
    text: &str,
    made_in: (Option<String>, Option<String>),
    edit: bool,
) -> Result<Vec<Value>, String> {
    as_guest(session, guest, made_in, edit, |s| {
        s.script_dry_run(text)?;
        s.script_atomic(text)
    })
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

/// The most elements a person's selection is shown with.
pub const MAX_SELECTION: usize = 10_000;

/// What one person in a session shows the others: the view on their screen, where their pointer is on it, how their plan is
/// zoomed, their 3D camera, and what they have selected. A window sends its own when it changes and draws the others'.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presence {
    #[serde(default)]
    pub view: Option<On>,
    /// The pointer, in the drawing of the view it is over: a model point on a plan, a point of a section's drawing or of a sheet's
    /// paper, in mm.
    #[serde(default)]
    pub cursor: Option<crate::geom::P2>,
    /// A plan's middle and pixels per mm, which a window following this person copies.
    #[serde(default)]
    pub zoom: Option<(f64, f64, f64)>,
    #[serde(default)]
    pub camera: Option<Look>,
    #[serde(default)]
    pub selection: Vec<Guid>,
}

/// A view: its kind (`plan`, `ceiling`, `3d`, `section`, `sheet`, `schedule`) and the GUID of its level, section, sheet or
/// schedule (none for the default 3D view).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct On {
    pub kind: String,
    #[serde(default)]
    pub of: Option<Guid>,
}

/// A 3D view's camera as it travels: every field of [`crate::view3d::Camera`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub azimuth: f64,
    pub elevation: f64,
    pub scale: f64,
    pub target: crate::view3d::P3,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub lens: Option<f64>,
}

impl Look {
    pub fn of(c: &crate::view3d::Camera) -> Look {
        Look {
            azimuth: c.azimuth,
            elevation: c.elevation,
            scale: c.scale,
            target: c.target,
            width: c.width,
            height: c.height,
            lens: c.lens,
        }
    }

    pub fn camera(&self) -> crate::view3d::Camera {
        let Look { azimuth, elevation, scale, target, width, height, lens } = *self;
        crate::view3d::Camera { azimuth, elevation, scale, target, width, height, lens }
    }

    /// Where it looks from, to draw it in another's 3D view: the eye in perspective; in a parallel view, which has none, back
    /// from its target along its line by as much model as its picture is wide.
    pub fn eye(&self) -> crate::view3d::P3 {
        let c = self.camera();
        c.eye().unwrap_or_else(|| {
            let (back, ..) = c.axes();
            let far = f64::from(self.width) / self.scale;
            [0, 1, 2].map(|i| self.target[i] + back[i] * far)
        })
    }
}

impl Presence {
    /// Whether it can be drawn: every number finite and in the model's range, a camera a 3D view could have, a selection of
    /// GUIDs no longer than [`MAX_SELECTION`]. What comes from another window is checked so before it is kept.
    pub fn check(&self) -> Result<(), String> {
        let near = |v: f64| v.is_finite() && v.abs() <= crate::model::MAX_COORD;
        let scale = |s: f64| s.is_finite() && s > 0.0 && s <= 1e6;
        if let Some(v) = &self.view
            && (v.kind.is_empty() || v.kind.len() > 16 || !v.kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
        {
            return Err("a view's kind is a short word".into());
        }
        if self.cursor.is_some_and(|p| !(near(p.0) && near(p.1))) {
            return Err("the pointer is out of the model".into());
        }
        if self.zoom.is_some_and(|(x, y, s)| !(near(x) && near(y) && scale(s))) {
            return Err("a plan's zoom is no zoom".into());
        }
        if let Some(c) = &self.camera {
            let lens = c.lens.is_none_or(|l| (crate::view3d::MIN_LENS..=crate::view3d::MAX_LENS).contains(&l));
            let size = (1..=16_384).contains(&c.width) && (1..=16_384).contains(&c.height);
            if !(c.azimuth.is_finite()
                && (-90.0..=90.0).contains(&c.elevation)
                && scale(c.scale)
                && c.target.iter().all(|v| near(*v))
                && lens
                && size)
            {
                return Err("a camera no 3D view could have".into());
            }
        }
        if self.selection.len() > MAX_SELECTION || self.selection.iter().any(|g| g.is_nil() || g.pending().is_some()) {
            return Err(format!("a selection is at most {MAX_SELECTION} elements' GUIDs"));
        }
        Ok(())
    }
}
