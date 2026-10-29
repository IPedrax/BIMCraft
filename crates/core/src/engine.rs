//! The session every front end drives: one model, its undo history and the command registry.
//! Every user-visible action is a [`Command`]; the CLI, scripts and MCP all reach the same ones.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{Element, Id, Model};

pub type Run = fn(&mut Session, &Value) -> Result<Value, String>;

pub struct Command {
    /// Lower-case id, after Revit's tool name.
    pub id: &'static str,
    pub label: &'static str,
    /// Typed shortcuts, Revit style (WA, DR, WN…).
    pub aliases: &'static [&'static str],
    /// The JSON parameters, documented for agents.
    pub params: &'static str,
    pub run: Run,
    /// Whether a change it makes is an undo step.
    pub undoable: bool,
}

pub const UNDO_LIMIT: usize = 200;

/// An undo (or redo) step: the command it was, and the elements it made and took away, by category, each category
/// counted in the order of its first element's id, so a list can name it: Create Walls, Delete Door.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub label: &'static str,
    pub made: Vec<(&'static str, usize)>,
    pub removed: Vec<(&'static str, usize)>,
}

impl Step {
    /// Without a look at every element: what's new has an id past every one `before` had (ids only go up), and only when
    /// the counts say something went is `before` searched for what.
    fn new(label: &'static str, before: &Model, after: &Model) -> Step {
        let tally = |elements: &mut dyn Iterator<Item = &Element>| {
            let mut counts: Vec<(&'static str, usize)> = Vec::new();
            for e in elements {
                match counts.iter_mut().find(|(c, _)| *c == e.category()) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((e.category(), 1)),
                }
            }
            counts
        };
        let first_new = before.elements.keys().next_back().map_or(0, |id| id + 1).max(before.next_id);
        let made = tally(&mut after.elements.range(first_new..).map(|(_, e)| e));
        let kept = after.elements.len() - made.iter().map(|(_, n)| n).sum::<usize>();
        let removed = if before.elements.len() > kept {
            tally(&mut before.elements.iter().filter(|(id, _)| !after.elements.contains_key(**id)).map(|(_, e)| e))
        } else {
            Vec::new()
        };
        Step { label, made, removed }
    }
}

/// A place in the undo history, which [`Session::squash`] joins the steps after.
#[derive(Clone, Debug)]
pub struct Mark {
    steps: usize,
    dropped: usize,
    resets: usize,
}

/// The commands that copy what stands: a copy is made in the session's phase and isn't taken down with its original.
const COPIES: [&str; 3] = ["copy", "mirror", "rotate"];

#[cfg_attr(test, derive(Clone))]
pub struct Session {
    pub model: Arc<Model>,
    /// Where the model was opened from or last saved to.
    pub path: Option<PathBuf>,
    /// The phase, and the design option (`set: option`), what a command makes is made in; none: the
    /// project's first phase, and no option. Not the model's: set with `phase` and `option`.
    pub phase: Option<String>,
    pub option: Option<String>,
    undo: Vec<(Step, Arc<Model>)>,
    redo: Vec<(Step, Arc<Model>)>,
    /// How many times the project was replaced, so a transaction knows its history went with it.
    resets: usize,
    /// How many of the oldest undo steps were dropped past [`UNDO_LIMIT`], so a [`Mark`] still finds its place.
    dropped: usize,
    /// What reads a DWG drawing, when the app has one (`cad::DwgReader`); none says to save it as DXF.
    pub dwg: Option<crate::cad::DwgReader>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

pub fn commands() -> &'static [Command] {
    crate::commands::ALL
}

/// A command by id or alias, in any case.
pub fn find(name: &str) -> Option<&'static Command> {
    commands().iter().find(|c| c.id.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
}

/// The command catalog as agents read it.
pub fn catalog() -> Value {
    commands().iter().map(|c| json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params})).collect()
}

impl Session {
    pub fn new() -> Session {
        Session {
            model: Arc::new(Model::new("Project")),
            path: None,
            phase: None,
            option: None,
            undo: Vec::new(),
            redo: Vec::new(),
            resets: 0,
            dropped: 0,
            dwg: None,
        }
    }

    /// The model to change, copied first when an undo step still shares it.
    pub fn model_mut(&mut self) -> &mut Model {
        Arc::make_mut(&mut self.model)
    }

    /// Replace the model (new, open), forgetting the history.
    pub fn reset(&mut self, model: Model, path: Option<PathBuf>) {
        self.model = Arc::new(model);
        self.path = path;
        (self.phase, self.option) = (None, None);
        self.resets += 1;
        self.undo.clear();
        self.redo.clear();
    }

    /// Labels of the undo steps, oldest first.
    pub fn history(&self) -> Vec<&'static str> {
        self.undo.iter().map(|(st, _)| st.label).collect()
    }

    /// The undo steps, or with `redo` the redo steps, the next to go first.
    pub fn steps(&self, redo: bool) -> Vec<&Step> {
        let from = if redo { &self.redo } else { &self.undo };
        from.iter().rev().map(|(st, _)| st).collect()
    }

    /// Run a command by id or alias with JSON parameters. A command that fails, panics, or leaves
    /// the model failing [`Model::check`] changes nothing.
    pub fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        let cmd = find(name).ok_or_else(|| format!("unknown command `{name}`; list_commands shows them"))?;
        // A library entry's GUID, wherever a command takes a name, is that name.
        let named = named(&self.model, params);
        let params = named.as_ref().unwrap_or(params);
        let before = Arc::clone(&self.model);
        let (run, first_new, session) = (cmd.run, self.model.next_id, (self.phase.clone(), self.option.clone()));
        let result = catch_unwind(AssertUnwindSafe(|| run(self, params)))
            .unwrap_or_else(|p| Err(format!("internal error in `{}`: {}", cmd.id, panic_text(p.as_ref()))))
            .and_then(|v| {
                if Arc::ptr_eq(&before, &self.model) {
                    return Ok(v);
                }
                // A command that splits what stands keeps its phase; one that makes something makes it in the session's;
                // undo, redo and the commands that replace the project make nothing.
                if cmd.undoable && cmd.id != "split" {
                    self.stamp(first_new, COPIES.contains(&cmd.id));
                }
                // A scan goes with the last point cloud that placed it.
                let unplaced = self.model.unplaced_clouds();
                if !unplaced.is_empty() {
                    let m = self.model_mut();
                    for name in unplaced {
                        m.clouds.remove(&name);
                    }
                }
                // A CAD drawing, with the last import.
                let unplaced = self.model.unplaced_cads();
                if !unplaced.is_empty() {
                    let m = self.model_mut();
                    for name in &unplaced {
                        m.cads.remove(name);
                    }
                    m.forget_cad_layers(&unplaced);
                }
                // What a command was given by id points at the element's GUID; what it put in the library has an id and a GUID.
                self.model_mut().resolve();
                self.model_mut().sync_library();
                // Locked and equal dimensions moved what they measure to as the command moved it.
                if cmd.undoable {
                    crate::constraints::keep(self, &before)?;
                }
                // What was made or changed must be as a sketch of it solves too (an outline that is simple), which a file
                // from before may break; Insert from File brings in what it finds.
                // (Before the check, which would name a bow tie of no area by that, not by what to redraw.)
                let edited = if cmd.undoable && cmd.id != "insert" { self.model.check_edits(&before, cmd.id, params) } else { Ok(()) };
                edited.and_then(|()| self.model.check()).map(|()| v)
            });
        match result {
            Err(e) => {
                // A refused command changes nothing: not the model, nor the phase and the option it asked the session for.
                self.model = before;
                (self.phase, self.option) = session;
                Err(e)
            }
            Ok(v) => {
                if cmd.undoable && !Arc::ptr_eq(&before, &self.model) {
                    self.undo.push((Step::new(cmd.label, &before, &self.model), before));
                    if self.undo.len() > UNDO_LIMIT {
                        self.undo.remove(0);
                        self.dropped += 1;
                    }
                    self.redo.clear();
                }
                Ok(v)
            }
        }
    }

    /// Run a script: one command per line, `id` or `id {json params}`; `#` starts a comment.
    /// Stops at the first failing line, naming it; the lines before it stay done.
    pub fn script(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (id, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let params = if rest.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("line {}: bad JSON after `{id}`: {e}", n + 1))?
            };
            out.push(self.execute(id, &params).map_err(|e| format!("line {} ({id}): {e}", n + 1))?);
        }
        Ok(out)
    }

    /// [`Session::script`] all or nothing: a line that fails takes back the lines before it, and one that goes through is
    /// one undo step, `Script`.
    pub fn script_atomic(&mut self, text: &str) -> Result<Vec<Value>, String> {
        self.transaction("Script", |s| s.script(text))
    }

    /// Run `f` as one undo step named `label`, all or nothing: when it fails, the model, the path, the session's phase
    /// and option and the undo and redo history are as they were before it. Inside, undo reaches back no further than
    /// its start. Files a command wrote meanwhile (`save`, `export`) stay written, and one that replaced the project (`new`,
    /// `open`) left the history it began, so what came after it stays the steps it was.
    pub fn transaction<T>(&mut self, label: &'static str, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
        let before = Arc::clone(&self.model);
        let (path, phase, option) = (self.path.clone(), self.phase.clone(), self.option.clone());
        let (mut undo, redo, resets) = (std::mem::take(&mut self.undo), std::mem::take(&mut self.redo), self.resets);
        let result = catch_unwind(AssertUnwindSafe(|| f(self)))
            .unwrap_or_else(|p| Err(format!("internal error in `{label}`: {}", panic_text(p.as_ref()))));
        match result {
            Err(e) => {
                (self.model, self.path, self.phase, self.option) = (before, path, phase, option);
                (self.undo, self.redo) = (undo, redo);
                Err(e)
            }
            Ok(v) if self.resets != resets => Ok(v),
            Ok(v) => {
                // Only an undoable command made a step; one that changed nothing, or only the view of it, makes none here either.
                if self.undo.is_empty() {
                    self.redo = redo;
                } else {
                    undo.push((Step::new(label, &before, &self.model), before));
                    if undo.len() > UNDO_LIMIT {
                        undo.remove(0);
                        self.dropped += 1;
                    }
                    self.redo.clear();
                }
                self.undo = undo;
                Ok(v)
            }
        }
    }

    /// Where the history stands, for [`Session::squash`].
    pub fn mark(&self) -> Mark {
        Mark { steps: self.undo.len(), dropped: self.dropped, resets: self.resets }
    }

    /// The undo steps made since `mark`, as one named `label`: a run of commands that came one at a time (a PyAddin's,
    /// a frame at a time in the window) and is taken back as one. Fewer than two, or a project replaced since, is left.
    pub fn squash(&mut self, mark: &Mark, label: &'static str) {
        if self.resets != mark.resets {
            return;
        }
        let at = mark.steps.saturating_sub(self.dropped - mark.dropped);
        if self.undo.len() < at + 2 {
            return;
        }
        let mut squashed = self.undo.split_off(at);
        let before = squashed.swap_remove(0).1;
        self.undo.push((Step::new(label, &before, &self.model), before));
    }

    /// What elements from id `first` on, which a command just made, are made in: the session's phase
    /// (the first phase is no value at all, but for a door or window, which would follow its wall) and design
    /// option. A name the project no longer has is left out. A copy is made in them whatever it was copied from,
    /// and isn't taken down with its original; anything else holds the phase or the option it was given.
    fn stamp(&mut self, first: Id, copies: bool) {
        if self.phase.is_none() && self.option.is_none() || self.model.next_id <= first {
            return;
        }
        let m = Arc::make_mut(&mut self.model);
        let phase = self.phase.clone().filter(|p| m.phases.contains(p));
        let option = self.option.clone().filter(|o| crate::phasing::option_exists(m, o));
        let is_first = phase.as_ref() == m.phases.first();
        for (_, e) in m.elements.range_mut(first..) {
            let follows = matches!(e, Element::Door(_) | Element::Window(_));
            let Some(params) = e.params_mut() else { continue };
            if let Some(p) = &phase
                && (copies || !params.contains_key(crate::phasing::CREATED))
            {
                if is_first && !follows {
                    params.remove(crate::phasing::CREATED);
                } else {
                    params.insert(crate::phasing::CREATED.into(), json!(p));
                }
                if copies {
                    params.remove(crate::phasing::DEMOLISHED);
                }
            }
            if let Some(o) = &option
                && (copies || !params.contains_key(crate::phasing::OPTION))
            {
                params.insert(crate::phasing::OPTION.into(), json!(o));
            }
        }
    }

    /// Undo (or with `redo`, redo) `count` steps; the reply names the last of them.
    pub(crate) fn step_back(&mut self, redo: bool, count: usize) -> Result<Value, String> {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let (key, what) = if redo { ("redone", "redo") } else { ("undone", "undo") };
        if from.is_empty() {
            return Err(format!("nothing to {what}"));
        }
        if count == 0 || count > from.len() {
            return Err(format!("`count` must be from 1 to {}, the steps there are to {what}", from.len()));
        }
        let mut label = "";
        for _ in 0..count {
            let Some((step, model)) = from.pop() else { break };
            label = step.label;
            to.push((step, std::mem::replace(&mut self.model, model)));
        }
        Ok(json!({ key: label, "count": count }))
    }
}

/// `params` with every string that is a library entry's GUID put as the entry's name; none when there's no such string.
fn named(m: &Model, params: &Value) -> Option<Value> {
    fn walk(m: &Model, v: &mut Value, changed: &mut bool) {
        match v {
            Value::String(s) => {
                if let Some((_, e)) = crate::ids::Guid::parse(s).filter(|g| g.pending().is_none()).and_then(|g| m.library.by_guid(g)) {
                    // A design option's name is `set: option` already.
                    *s = e.name.clone();
                    *changed = true;
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(m, x, changed)),
            Value::Object(o) => o.values_mut().for_each(|x| walk(m, x, changed)),
            _ => {}
        }
    }
    let mut v = params.clone();
    let mut changed = false;
    walk(m, &mut v, &mut changed);
    changed.then_some(v)
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}
