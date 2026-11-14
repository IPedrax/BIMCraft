//! The session every front end drives: one model, its undo history and the command registry.
//! Every user-visible action is a [`Command`]; the CLI, scripts and MCP all reach the same ones.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{Element, Id, Model};

pub type Run = fn(&mut Session, &Value) -> Result<Value, String>;

/// How a command from outside the binary runs: a plugin's function, which needs the instance it lives in.
/// The core knows nothing of WASM; it only keeps the closure a plugin crate hands it.
pub type ExternalRun = Arc<dyn Fn(&mut Session, &Value) -> Result<Value, String> + Send + Sync>;

/// A command registered at run time, by a plugin: the same shape as [`Command`], with the texts owned,
/// and a closure in place of the static function.
#[derive(Clone)]
pub struct ExternalCommand {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub params: String,
    pub menu: Option<String>,
    /// The plugin it belongs to, as the catalog marks it.
    pub plugin: String,
    pub run: ExternalRun,
}

impl ExternalCommand {
    /// Whether `name` is this command's id or one of its aliases, in any case.
    pub fn matches(&self, name: &str) -> bool {
        self.id.eq_ignore_ascii_case(name) || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }
}

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
    /// Where the command sits in the ribbon: `"Manage > Family Editor"`. `None` for what has no
    /// place in the interface (a shortcut, or a command no menu reaches) — the field is always
    /// read, filled in as the catalog learns the interface.
    pub menu: Option<&'static str>,
    /// Whether it reads or writes files, and so needs a plugin's `files` permission. A command that only
    /// touches the disk with some parameters says [`Files::Conditional`] here and decides in
    /// [`touches_files`].
    pub files: Files,
}

/// What a command does with the disk. A plugin call may only reach the disk when its plugin is given the
/// `files` permission, so every command that does is marked here, next to its parameters, rather than in a
/// list somewhere else that a new command would forget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    /// It never touches the disk.
    None,
    /// It reads files.
    Read,
    /// It writes, moves or replaces files.
    Write,
    /// It may: [`touches_files`] reads the parameters to say which.
    Conditional,
}

impl Files {
    /// The catalog's word for it, or `None` for a command that never touches the disk.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Files::None => None,
            Files::Read => Some("read"),
            Files::Write => Some("write"),
            Files::Conditional => Some("conditional"),
        }
    }
}

impl Command {
    /// The same command, reached from a menu: `edit("family", …).menu("Manage > Family Editor")`.
    pub const fn menu(self, menu: &'static str) -> Command {
        Command {
            id: self.id,
            label: self.label,
            aliases: self.aliases,
            params: self.params,
            run: self.run,
            undoable: self.undoable,
            menu: Some(menu),
            files: self.files,
        }
    }
}

pub const UNDO_LIMIT: usize = 200;

/// An undo (or redo) step: the command it was, and the elements it made and took away, by category, each category
/// counted in the order of its first element's id, so a list can name it: Create Walls, Delete Door.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub label: String,
    pub made: Vec<(&'static str, usize)>,
    pub removed: Vec<(&'static str, usize)>,
    /// Who made it, in a Co-Work session ([`Session::author`]); none outside one.
    pub by: Option<String>,
    /// Whether the model the step goes back to passed [`Model::check`]: going back to it needs no check.
    checked: bool,
    /// Its number in the session, which stays its own as the history moves.
    n: u64,
    /// The step this one took back, when it is a person's own undo or redo ([`Session::undo_own`]).
    undoes: Option<u64>,
    /// Whether it is an undo (and so what a redo takes back), rather than an edit or a redo.
    inverse: bool,
    /// Whether a later step of its author's took it back.
    undone: bool,
}

impl Step {
    /// Without a look at every element: what's new has an id past every one `before` had (ids only go up), and only when
    /// the counts say something went is `before` searched for what.
    fn new(label: &str, before: &Model, after: &Model) -> Step {
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
        Step { label: label.to_string(), made, removed, by: None, checked: false, n: 0, undoes: None, inverse: false, undone: false }
    }
}

/// The commands that put another project in the session's place: a session that may only read, and a window's agent, never runs them.
pub const REPLACES: &[&str] = &["new", "open", "recover", "highrise", "tower"];

/// What a refusal of a session that may only read says, after what the command would do and before what would let it edit.
pub const READ_ONLY: &str = "which is read-only for agents";

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
    /// How many times a command's model was checked, for tests that it was or wasn't.
    #[cfg(test)]
    pub(crate) checks: usize,
    /// How many times the project was replaced, so a transaction knows its history went with it.
    resets: usize,
    /// What reads a DWG drawing, when the app has one (`cad::DwgReader`); none says to save it as DXF.
    pub dwg: Option<crate::cad::DwgReader>,
    /// The formats it exports to (`export`): the core's, and any the app registers (the DWG writer, from the DWG bridge).
    pub exports: crate::export::Registry,
    /// The commands plugins registered, beside the static [`commands`]. They belong to this session, not the
    /// process, so two sessions and the tests never share them.
    external: Vec<ExternalCommand>,
    /// Whether the session may only read its project, and if so what would let it edit, which each refusal ends with: an agent's,
    /// when the user lets agents only read (the window's File > Settings > Agents, `bimcraft-cli mcp --read-only`). Then a command
    /// that would replace the project, undo or redo, or write over its own file is refused before it runs, and one that changed the
    /// project, its history or the phase and option the session makes things in is taken back and refused ([`Session::execute`]).
    /// What reads it (inspect, schedules, render, exports to new files, the sheets to PDF) answers; a copy saved as another file
    /// leaves the session with its own.
    pub read_only: Option<&'static str>,
    /// Who the commands run now are for, in a Co-Work session: the host's name, or a guest's while their command runs. Each step
    /// says who made it ([`Step::by`]), and `undo` and `redo` take back only the author's own ([`Session::undo_own`]). None outside a
    /// session.
    pub author: Option<String>,
    /// Whether a Co-Work guest's command is running ([`crate::cowork::run_for_guest`]): then no command may read or write a file, so a
    /// guest never reaches the host's disk.
    pub(crate) no_files: bool,
    /// How many steps were made, which numbers them.
    steps_made: u64,
    /// Whether a dry run is under way ([`Session::script_dry_run`]): then each command is looked at as it is about to run, with
    /// the params it is given, and one that would touch a file or the history is refused ([`dry_run_refuses`]).
    dry_run: bool,
    /// The last model a command left that passed [`Model::check`]. Held here, it is never changed in place: a change
    /// outside a command copies it first, so a model still this one is one that passed, and the next command need only
    /// check what it changes ([`Model::check_changed`]).
    verified: Option<Arc<Model>>,
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
    commands()
        .iter()
        .map(|c| {
            let mut entry = json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params, "menu": c.menu});
            if let Some(files) = c.files.as_str() {
                entry["files"] = json!(files);
            }
            entry
        })
        .collect()
}

/// Whether a call of `cmd` with `params` reads or writes a file, which a plugin may only do with the `files`
/// permission. A command marked at the [`Command`] is the answer, unless [`Files::Conditional`] says the
/// parameters decide: a command that only sometimes takes a path. The plugin does not keep a list of its own.
pub fn touches_files(cmd: &Command, params: &Value) -> Option<Files> {
    let given = |key: &str| params.get(key).is_some_and(|v| !v.is_null());
    match cmd.files {
        Files::None => None,
        Files::Read => Some(Files::Read),
        Files::Write => Some(Files::Write),
        Files::Conditional => {
            // A texture's picture is a file of its own, nested in the material's parameters.
            let texture = params.get("texture").is_some_and(|t| t.get("path").is_some_and(|p| !p.is_null()));
            // The favorites are a file of their own: saving, loading or deleting one writes it, listing only reads it.
            let favorites = params.get("action").and_then(Value::as_str).is_some_and(|a| a != "list");
            if given("path") || given("save") || texture || favorites { Some(Files::Write) } else { None }
        }
    }
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
            #[cfg(test)]
            checks: 0,
            resets: 0,
            dwg: None,
            exports: crate::export::Registry::default(),
            external: Vec::new(),
            read_only: None,
            author: None,
            no_files: false,
            steps_made: 0,
            dry_run: false,
            verified: None,
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

    /// Takes `model` as the project, with no history of its own: a Co-Work guest's copy taking in the host's
    /// (`cowork::apply`, which checked it). What the session may do (`read_only`) and its file stay.
    pub fn adopt(&mut self, model: Model) {
        self.model = Arc::new(model);
        self.verified = Some(Arc::clone(&self.model));
        self.undo.clear();
        self.redo.clear();
    }

    /// Labels of the undo steps, oldest first.
    pub fn history(&self) -> Vec<&str> {
        self.undo.iter().map(|(st, _)| st.label.as_str()).collect()
    }

    /// The undo steps, or with `redo` the redo steps, the next to go first.
    pub fn steps(&self, redo: bool) -> Vec<&Step> {
        let from = if redo { &self.redo } else { &self.undo };
        from.iter().rev().map(|(st, _)| st).collect()
    }

    /// Registers a plugin's command. A command may not take the id or an alias of one there already, static
    /// or of another plugin: a plugin is refused, not allowed to shadow BIMCraft.
    pub fn register(&mut self, command: ExternalCommand) -> Result<(), String> {
        let taken = |s: &str| {
            find(s).is_some()
                || self.external.iter().any(|c| c.id.eq_ignore_ascii_case(s) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(s)))
        };
        if taken(&command.id) {
            return Err(format!("the command `{}` is already taken", command.id));
        }
        if let Some(alias) = command.aliases.iter().find(|a| taken(a)) {
            return Err(format!("the command `{}` would take the alias `{alias}`, already taken", command.id));
        }
        self.external.push(command);
        Ok(())
    }

    /// Takes out every command a plugin registered.
    pub fn unregister_plugin(&mut self, plugin: &str) {
        self.external.retain(|c| c.plugin != plugin);
    }

    /// Takes out every command from outside, as a plugin folder is read again.
    pub fn clear_external(&mut self) {
        self.external.clear();
    }

    /// The commands from outside, as a front end lists them.
    pub fn external(&self) -> &[ExternalCommand] {
        &self.external
    }

    /// The command catalog as agents read it: [`catalog`] with this session's plugin commands, each marked
    /// with the plugin it belongs to. Without plugins it is exactly [`catalog`].
    pub fn catalog(&self) -> Value {
        let mut all = catalog();
        if let (Some(list), false) = (all.as_array_mut(), self.external.is_empty()) {
            list.extend(self.external.iter().map(
                |c| json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params, "menu": c.menu, "plugin": c.plugin}),
            ));
        }
        all
    }

    /// A command from outside by id or alias, in any case.
    pub fn external_command(&self, name: &str) -> Option<&ExternalCommand> {
        self.external.iter().find(|c| c.matches(name))
    }

    /// Run a command by id or alias with JSON parameters. A command that fails, panics, or leaves
    /// the model failing [`Model::check`] changes nothing. In a session that may only read ([`Session::read_only`]), one that would
    /// change the project is refused, and changes nothing either.
    pub fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        if let Some(cmd) = find(name) {
            return self.execute_found(cmd, params);
        }
        let Some(cmd) = self.external.iter().find(|c| c.matches(name)).cloned() else {
            return Err(format!("unknown command `{name}`; list_commands shows them"));
        };
        self.execute_external_checked(cmd, name, params)
    }

    /// [`Session::execute`] of a command in the static list, refused or taken back when the session may only read.
    fn execute_found(&mut self, cmd: &'static Command, params: &Value) -> Result<Value, String> {
        if let Some(what) = dry_run_refuses(cmd, params).filter(|_| self.dry_run) {
            return Err(format!(
                "`{}` would {what}, and a dry run touches no file and keeps the history: run the script without `dry_run`",
                cmd.id
            ));
        }
        if self.no_files && (touches_files(cmd, params).is_some() || file_key(params).is_some()) {
            return Err(format!(
                "`{}` would read or write a file on the host's machine, which a Co-Work guest never does: the host opens, imports \
                 and saves",
                cmd.id
            ));
        }
        let Some(how) = self.read_only else { return self.perform(cmd, params) };
        self.may_read(cmd, params).map_err(|what| format!("`{}` would {what}, {READ_ONLY}: {how}", cmd.id))?;
        let kept = Kept::of(self);
        let result = self.perform(cmd, params);
        kept.keep(self).map_err(|()| format!("`{}` would change the project, {READ_ONLY}: {how}", cmd.id)).and(result)
    }

    /// What a session that may only read refuses `cmd` before it runs, by what it would do: replace the project, step through its
    /// history, or write over its own file (`save` with no path saves there, and no command is given it as its `path` or `save`).
    fn may_read(&self, cmd: &Command, params: &Value) -> Result<(), String> {
        if REPLACES.contains(&cmd.id) {
            return Err("replace the project".into());
        }
        if matches!(cmd.id, "undo" | "redo") {
            return Err("change the project".into());
        }
        let Some(own) = self.path.as_deref() else { return Ok(()) };
        if cmd.id == "save" && params.get("path").is_none_or(Value::is_null) {
            return Err("write over the project's own file".into());
        }
        let given =
            ["path", "save"].into_iter().find(|key| params.get(key).and_then(Value::as_str).is_some_and(|p| same_file(own, Path::new(p))));
        match given {
            Some(_) if cmd.id == "save" => Err("write over the project's own file".into()),
            Some(key) => Err(format!("use the project's own file (`{key}`)")),
            None => Ok(()),
        }
    }

    /// [`Session::execute`] of a command found, as any session runs it.
    fn perform(&mut self, cmd: &'static Command, params: &Value) -> Result<Value, String> {
        // A library entry's GUID, wherever a command takes a name, is that name.
        let named = named(&self.model, params);
        let params = named.as_ref().unwrap_or(params);
        let before = Arc::clone(&self.model);
        let verified = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &before));
        let (run, first_new, session) = (cmd.run, self.model.next_id, (self.phase.clone(), self.option.clone()));
        let result = catch_unwind(AssertUnwindSafe(|| run(self, params)))
            .unwrap_or_else(|p| Err(format!("internal error in `{}`: {}", cmd.id, panic_text(p.as_ref()))))
            .and_then(|v| {
                if Arc::ptr_eq(&before, &self.model) {
                    return Ok(v);
                }
                // Back to a model that passed (undo, redo): it is as it was then.
                if self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model)) {
                    #[cfg(test)]
                    assert_eq!(self.model.check(), Ok(()), "`{}` went back to a model that doesn't pass", cmd.id);
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
                // Each loaded link placed by base points or shared coordinates remembers where it stands, as Revit's file does,
                // so Reset Coordinates can pin it there when its file is gone.
                let moved = self.model.links_moved();
                if !moved.is_empty() {
                    let m = self.model_mut();
                    for (id, placed) in moved {
                        if let Some(Element::Link(l)) = m.elements.get_mut(id) {
                            l.placed = Some(placed);
                        }
                    }
                }
                // What a command was given by id points at the element's GUID; what it put in the library has an id and a GUID.
                if verified {
                    self.model_mut().resolve_changed(&before);
                } else {
                    self.model_mut().resolve();
                }
                // A view deleted leaves the print sets it was in.
                if self.model.print_sets_dangle() {
                    self.model_mut().prune_print_sets();
                }
                // A print set deleted leaves the publishing sets it was in.
                if self.model.publish_sets_dangle() {
                    self.model_mut().prune_publish_sets();
                }
                // A library in step needs nothing; after a verified model, what the command changed says whether it is.
                let in_step = if verified { self.model.library_in_step_since(&before) } else { self.model.library_in_step() };
                #[cfg(test)]
                if verified {
                    assert_eq!(in_step, self.model.library_in_step(), "library_in_step_since differs after `{}` {params}", cmd.id);
                }
                if !in_step {
                    // After a verified model, what the command changed is synced; else (or when more than elements
                    // changed) the whole library.
                    #[cfg(test)]
                    let oracle = verified.then(|| sync_oracle(&self.model, &before));
                    if !(verified && self.model_mut().sync_since(&before)) {
                        self.model_mut().sync_library();
                    }
                    #[cfg(test)]
                    if let Some(Some((incremental, whole))) = oracle {
                        assert!(
                            incremental.elements.iter().eq(whole.elements.iter()) && incremental.library.parts().eq(whole.library.parts()),
                            "the library synced by what `{}` changed isn't as the whole sync makes it",
                            cmd.id
                        );
                        assert!(self.model.library_in_step(), "the library isn't in step after `{}`", cmd.id);
                    }
                }
                // A room frozen keeps the boundary it has now; one thawed lets it go. After a verified model, only a room the
                // command changed can be either.
                let unsettled = |e: &Element| matches!(e, Element::Room(r) if r.frozen != r.kept.is_some());
                let unsettled = match (verified, self.model.changed_since(&before)) {
                    (true, crate::ids::Changed::Ids(ids)) => ids.iter().any(|id| self.model.elements.get(*id).is_some_and(unsettled)),
                    _ => self.model.elements.values().any(unsettled),
                };
                if unsettled {
                    self.model_mut().keep_rooms();
                }
                // A family whose origin the command moved (a reload, an edit of its planes) moves its instances by as much, so
                // they stay where they stand (O1); a project the command replaced was settled as it was read.
                if !REPLACES.contains(&cmd.id) {
                    let settled = !self.model.origins_unsettled(&before);
                    if !settled {
                        self.model_mut().settle_origins(Some(&before));
                    }
                }
                // A hosted component goes where its plane is, or onto its plane where the command moved it.
                let unsettled = self.model.unsettled(Some(&before));
                if !unsettled.is_empty() {
                    self.model_mut().settle(unsettled);
                }
                // A 3D view's work plane on a reference plane goes where the plane is; the plane gone, it stays, dissociated.
                if !self.model.unsettled_work_planes().is_empty() {
                    self.model_mut().settle_work_planes();
                }
                // An elevation that looks at a reference looks at it from wherever the command left its mark.
                if cmd.undoable {
                    crate::elevation::keep(self, &before, verified);
                }
                // Locked and equal dimensions moved what they measure to as the command moved it.
                if cmd.undoable {
                    crate::constraints::keep(self, &before)?;
                    // A room's own floor and ceiling follow its boundary, on the levels the command changed.
                    crate::rooms::follow(self, &before)?;
                    // Tests hold that to what following every level makes.
                    #[cfg(test)]
                    {
                        let mut whole = self.clone();
                        crate::rooms::follow_all(&mut whole)?;
                        assert!(
                            whole.model.elements.iter().eq(self.model.elements.iter()),
                            "the floors and ceilings of rooms followed by what `{}` changed aren't as following every level makes them",
                            cmd.id
                        );
                    }
                }
                // What was made or changed must be as a sketch of it solves too (an outline that is simple), which a file
                // from before may break; Insert from File brings in what it finds.
                // (Before the check, which would name a bow tie of no area by that, not by what to redraw.)
                // A family made or changed says only names it has in its formulas, which a file from before may break.
                let edited = if cmd.undoable && cmd.id != "insert" {
                    self.model.check_edits(&before, cmd.id, params).and_then(|()| crate::family::check_family_edits(&self.model, &before))
                } else {
                    Ok(())
                };
                edited
                    .and_then(|()| {
                        #[cfg(test)]
                        {
                            self.checks += 1;
                        }
                        let checked = if verified { self.model.check_changed(&before) } else { self.model.check() };
                        // Tests hold the two to the same answer after every command they run.
                        #[cfg(test)]
                        if verified {
                            let full = self.model.check();
                            assert_eq!(checked, full, "check_changed differs from check after `{}` {params}", cmd.id);
                        }
                        checked
                    })
                    .map(|()| v)
            });
        match result {
            Err(e) => {
                // A refused command changes nothing: not the model, nor the phase and the option it asked the session for.
                self.model = before;
                (self.phase, self.option) = session;
                Err(e)
            }
            Ok(v) => {
                self.verified = Some(Arc::clone(&self.model));
                if cmd.undoable && !Arc::ptr_eq(&before, &self.model) {
                    let step = self.stamped(Step { checked: verified, ..Step::new(cmd.label, &before, &self.model) });
                    self.undo.push((step, before));
                    if self.undo.len() > UNDO_LIMIT {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                }
                Ok(v)
            }
        }
    }

    /// [`Session::execute`] of a command a plugin registered. The plugin's closure runs the function in its
    /// sandbox and wraps it in its own transaction; here the model is only guarded, as for any command.
    fn execute_external(&mut self, cmd: ExternalCommand, name: &str, params: &Value) -> Result<Value, String> {
        let named = named(&self.model, params);
        let params = named.as_ref().unwrap_or(params);
        let before = Arc::clone(&self.model);
        let verified = self.verified.clone();
        let (undo, redo, session) = (self.undo.clone(), self.redo.clone(), (self.phase.clone(), self.option.clone()));
        let result = catch_unwind(AssertUnwindSafe(|| (cmd.run)(self, params)))
            .unwrap_or_else(|p| Err(format!("internal error in `{name}`: {}", panic_text(p.as_ref()))))
            .and_then(|v| {
                if Arc::ptr_eq(&before, &self.model) {
                    return Ok(v);
                }
                self.model_mut().resolve();
                if self.model.print_sets_dangle() {
                    self.model_mut().prune_print_sets();
                }
                // A print set deleted leaves the publishing sets it was in.
                if self.model.publish_sets_dangle() {
                    self.model_mut().prune_publish_sets();
                }
                self.model_mut().sync_library();
                self.model.check().map(|()| v)
            });
        match result {
            Err(e) => {
                (self.model, self.phase, self.option) = (before, session.0, session.1);
                (self.undo, self.redo) = (undo, redo);
                self.verified = verified;
                Err(e)
            }
            Ok(v) => {
                self.verified = Some(Arc::clone(&self.model));
                Ok(v)
            }
        }
    }

    /// [`Session::execute_external`] of a session that may only read: what the plugin changed is taken back and refused.
    fn execute_external_checked(&mut self, cmd: ExternalCommand, name: &str, params: &Value) -> Result<Value, String> {
        let Some(how) = self.read_only else { return self.execute_external(cmd, name, params) };
        let kept = Kept::of(self);
        let result = self.execute_external(cmd, name, params);
        kept.keep(self).map_err(|()| format!("`{name}` would change the project, {READ_ONLY}: {how}")).and(result)
    }

    /// Whether the model now is the one the last command left that passed the check, so the next checks only what it changes.
    #[cfg(test)]
    pub(crate) fn verified_now(&self) -> bool {
        self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model))
    }

    /// Runs `f` as a session that may only read ([`Session::read_only`]), `how` saying what would let it edit, unless the
    /// session already says; then gives the session back as it was. What `f` changed of the project, however it got at it, is
    /// taken back and refused too.
    pub fn reading<T>(&mut self, how: &'static str, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
        let was = self.read_only;
        let how = *self.read_only.get_or_insert(how);
        let kept = Kept::of(self);
        let result = catch_unwind(AssertUnwindSafe(|| f(self)))
            .unwrap_or_else(|p| Err(format!("internal error while reading: {}", panic_text(p.as_ref()))));
        self.read_only = was;
        kept.keep(self).map_err(|()| format!("that would change the project, {READ_ONLY}: {how}")).and(result)
    }

    /// Run a script: one command per line, `id` or `id {json params}`; `#` starts a comment. A line may be named,
    /// `@name = id {json params}`, and a later line's params point at what an earlier one returned ([`refer`]): `@name`,
    /// `$last` (the line before), `$-2` (the one before that), each its reply's `id`, or a field of it, `@name.ids.0`.
    /// Stops at the first failing line, naming it; the lines before it stay done.
    pub fn script(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        // Each name, the line it names: its place in `out` and its line number.
        let mut names: Vec<(&str, usize, usize)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let at = |e: String| format!("line {}: {e}", n + 1);
            let Some(ScriptLine { name, command: id, params: rest }) = script_line(line).map_err(at)? else { continue };
            if let Some(&(_, _, first)) = names.iter().find(|(m, _, _)| Some(*m) == name) {
                return Err(at(format!("`@{}` already names line {first}", reference_text(name.unwrap_or_default()))));
            }
            let params = if rest.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| at(format!("bad JSON after `{id}`: {e}")))?
            };
            let params = refer(&params, &out, &names).map_err(|e| format!("line {} ({id}): {e}", n + 1))?;
            out.push(self.execute(id, &params).map_err(|e| format!("line {} ({id}): {e}", n + 1))?);
            if let Some(name) = name {
                names.push((name, out.len() - 1, n + 1));
            }
        }
        Ok(out)
    }

    /// [`Session::script`] all or nothing: a line that fails takes back the lines before it, and one that goes through is
    /// one undo step, `Script`.
    pub fn script_atomic(&mut self, text: &str) -> Result<Vec<Value>, String> {
        self.transaction("Script", |s| s.script(text))
    }

    /// [`Session::script_atomic`] tried and taken back: what the script would make, change and take away, element by element,
    /// whether it would change anything else (the library, the phases, the settings), and the warnings the model would then
    /// have, with each line's result. The project, its history, the phase and option things are made in and its file are left
    /// as they were, and the next id with them. A dry run touches no file and keeps the history: a line that would open, write
    /// or name a file (a `path` or `save`), start or replace the project, save, undo or redo is refused as it comes, with the params
    /// it is given (a named line's and a reference's too), before it runs; the lines before it are taken back with the rest.
    /// It only reads the project, so a session that may only read ([`Session::read_only`]) may try a script too.
    pub fn script_dry_run(&mut self, text: &str) -> Result<Value, String> {
        let kept = Kept::of(self);
        let read_only = self.read_only.take();
        let dry_run = std::mem::replace(&mut self.dry_run, true);
        let result = self.script_atomic(text);
        let after = Arc::clone(&self.model);
        (self.read_only, self.dry_run) = (read_only, dry_run);
        // The project as it was, whatever the script did to it; what a dry run says is only what it would do.
        let _ = kept.keep(self);
        let results = result?;
        let before = &self.model;
        let (mut made, mut changed, mut removed) = (Vec::new(), Vec::new(), Vec::new());
        let ids: Vec<Id> = match after.elements.changed_since(&before.elements) {
            crate::ids::Changed::Ids(ids) => ids.into_iter().collect(),
            crate::ids::Changed::All => {
                let mut all: Vec<Id> = before.elements.iter().map(|(id, _)| *id).chain(after.elements.iter().map(|(id, _)| *id)).collect();
                all.sort_unstable();
                all.dedup();
                all
            }
        };
        for id in ids {
            match (before.elements.get(id), after.elements.get(id)) {
                (None, Some(e)) => made.push(json!({"id": id, "category": e.category()})),
                (Some(a), Some(b)) if a != b || before.elements.guid(id) != after.elements.guid(id) => {
                    changed.push(json!({"id": id, "guid": before.elements.guid(id), "category": b.category()}))
                }
                (Some(a), None) => removed.push(json!({"id": id, "guid": before.elements.guid(id), "category": a.category()})),
                _ => {}
            }
        }
        let other = {
            let mut rest = (*after).clone();
            (rest.elements, rest.next_id) = (before.elements.clone(), before.next_id);
            rest != **before
        };
        Ok(json!({
            "dry_run": true, "results": results, "made": made, "changed": changed, "removed": removed, "other_changes": other,
            "warnings": after.warnings(),
        }))
    }

    /// Run `f` as one undo step named `label`, all or nothing: when it fails, the model, the path, the session's phase
    /// and option and the undo and redo history are as they were before it. Inside, undo reaches back no further than
    /// its start. Files a command wrote meanwhile (`save`, `export`) stay written, and one that replaced the project (`new`,
    /// `open`) left the history it began, so what came after it stays the steps it was.
    pub fn transaction<T>(&mut self, label: &str, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
        let before = Arc::clone(&self.model);
        let checked = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &before));
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
                    let step = self.stamped(Step { checked, ..Step::new(label, &before, &self.model) });
                    undo.push((step, before));
                    if undo.len() > UNDO_LIMIT {
                        undo.remove(0);
                    }
                    self.redo.clear();
                }
                self.undo = undo;
                Ok(v)
            }
        }
    }

    /// The undo steps on top of the stack that are named exactly `from`, merged into one named `to`, which goes back to the
    /// model the first of them did. A front end that works in several transactions and wants one step for the whole (an
    /// assistant's turn) names each `from`, then renames at the end; a run of none or of one is only renamed.
    pub fn merge_steps(&mut self, from: &str, to: &str) {
        let run = self.undo.iter().rev().take_while(|(step, _)| step.label == from).count();
        let at = self.undo.len() - run;
        let mut steps = self.undo.drain(at..);
        let Some((first, before)) = steps.next() else { return };
        drop(steps);
        let step = Step { checked: first.checked, by: first.by, ..self.stamped(Step::new(to, &before, &self.model)) };
        self.undo.push((step, before));
    }

    /// `step` numbered, and by the session's author.
    fn stamped(&mut self, step: Step) -> Step {
        self.steps_made += 1;
        Step { by: self.author.clone(), n: self.steps_made, ..step }
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
        let mut label = String::new();
        // Whether the model now passed: the step that comes back to it says so.
        let mut checked = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model));
        for _ in 0..count {
            let Some((mut step, model)) = from.pop() else { break };
            label = step.label.clone();
            std::mem::swap(&mut step.checked, &mut checked);
            to.push((step, std::mem::replace(&mut self.model, model)));
        }
        if checked {
            self.verified = Some(Arc::clone(&self.model));
        }
        Ok(json!({ key: label, "count": count }))
    }

    /// Undo (or with `redo`, redo) `count` of `by`'s own steps, all or nothing: what `undo` and `redo` do in a Co-Work session, where
    /// each person takes back their own edits wherever they are in the history. A step is taken back by a new step on top, which
    /// puts what it changed (its elements, and the project's settings if it changed them) as they were before it. That is refused,
    /// and nothing changes, when any of it is no longer as the step left it, since someone else changed it after; the error says
    /// who and what. It is refused too when the model it leaves fails [`Model::check`] (Bo's door in Ana's wall, which her undo
    /// would take away), and then names whose steps came after.
    pub(crate) fn undo_own(&mut self, by: &str, redo: bool, count: usize) -> Result<Value, String> {
        if count == 0 {
            return Err("`count` must be 1 or more".into());
        }
        let kept = (Arc::clone(&self.model), self.verified.clone(), self.undo.clone(), self.steps_made);
        let mut label = String::new();
        for _ in 0..count {
            match self.own_step(by, redo) {
                Ok(l) => label = l,
                Err(e) => {
                    (self.model, self.verified, self.undo, self.steps_made) = kept;
                    return Err(e);
                }
            }
        }
        self.redo.clear();
        Ok(json!({ if redo { "redone" } else { "undone" }: label, "count": count }))
    }

    /// What `by`'s undo (or with `redo`, redo) would take back in a Co-Work session, the next first, as [`Session::undo_own`] takes
    /// them: their edits still standing, or the edits their undos took back, named by those edits. A list's place is the `count`.
    pub fn own_steps(&self, by: &str, redo: bool) -> Vec<&Step> {
        let standing = |st: &Step| st.by.as_deref() == Some(by) && !st.undone;
        let mut list = Vec::new();
        for (st, _) in self.undo.iter().rev() {
            if redo && st.by.as_deref() == Some(by) && st.undoes.is_none() {
                break;
            }
            if standing(st) && st.inverse == redo {
                list.push(self.origin(st));
            }
        }
        list
    }

    /// The edit an undo or a redo goes back to, through the steps it undid, as far as the history still has them.
    fn origin<'a>(&'a self, mut st: &'a Step) -> &'a Step {
        for _ in 0..self.undo.len() {
            match st.undoes.and_then(|n| self.undo.iter().find(|(o, _)| o.n == n)) {
                Some((o, _)) => st = o,
                None => break,
            }
        }
        st
    }

    /// One of [`Session::undo_own`]'s steps; the new step's label.
    fn own_step(&mut self, by: &str, redo: bool) -> Result<String, String> {
        let what = if redo { "redo" } else { "undo" };
        let standing = |st: &Step| st.by.as_deref() == Some(by) && !st.undone;
        // Undo takes their last edit still standing (an edit or a redo); redo their last undo still standing, unless an edit of
        // theirs came after it.
        let at = if redo {
            let edited = self.undo.iter().rposition(|(st, _)| st.by.as_deref() == Some(by) && st.undoes.is_none());
            self.undo.iter().rposition(|(st, _)| standing(st) && st.inverse).filter(|at| edited.is_none_or(|e| e < *at))
        } else {
            self.undo.iter().rposition(|(st, _)| standing(st) && !st.inverse)
        };
        let at = at.ok_or_else(|| format!("nothing of yours to {what}"))?;
        let now = Arc::clone(&self.model);
        let before = Arc::clone(&self.undo[at].1);
        let after = self.undo.get(at + 1).map_or_else(|| Arc::clone(&now), |(_, m)| Arc::clone(m));
        let step = &self.undo[at].0;
        let base = step.label.strip_prefix("Undo ").or_else(|| step.label.strip_prefix("Redo ")).unwrap_or(&step.label).to_string();
        let (ids, settings) = after.diff(&before);
        // What it changed, as it left it.
        let moved: Vec<Id> = ids
            .iter()
            .copied()
            .filter(|id| now.elements.get(*id) != after.elements.get(*id) || now.elements.guid(*id) != after.elements.guid(*id))
            .collect();
        let resettled = settings && !now.same_settings(&after);
        if !moved.is_empty() || resettled {
            return Err(self.whose(at, &format!("can't {what} your {base}"), &moved, resettled, &now, &after));
        }
        let mut m = (*now).clone();
        if settings {
            let mut back = (*before).clone();
            (back.elements, back.next_id) = (m.elements.clone(), m.next_id);
            // What is worked out from the elements is worked out again.
            back.derived = Default::default();
            m = back;
        }
        for &id in &ids {
            match (before.elements.get(id), before.elements.guid(id)) {
                (Some(e), Some(g)) => m.elements.insert_as(id, g, e.clone())?,
                _ => {
                    m.elements.remove(id);
                }
            }
        }
        if let Err(e) = m.check_changed(&now) {
            let after_it: Vec<String> = self.undo[at + 1..]
                .iter()
                .filter(|(st, _)| st.by.as_deref() != Some(by))
                .map(|(st, _)| format!("{}'s {}", st.by.as_deref().unwrap_or("someone"), st.label))
                .take(3)
                .collect();
            let after_it = if after_it.is_empty() { String::new() } else { format!(" (after it: {})", after_it.join(", ")) };
            return Err(format!("can't {what} your {base}: {e}{after_it}"));
        }
        let label = format!("{} {base}", if redo { "Redo" } else { "Undo" });
        let (n, inverse) = (step.n, !step.inverse);
        self.undo[at].0.undone = true;
        let checked = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &now));
        let step = Step { by: Some(by.into()), undoes: Some(n), inverse, checked, ..self.stamped(Step::new(&label, &now, &m)) };
        self.undo.push((step, now));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        // Checked: the command that called this goes straight on, as after a step back.
        self.model = Arc::new(m);
        self.verified = Some(Arc::clone(&self.model));
        Ok(label)
    }

    /// Why a person's undo of step `at` is refused, `head` first: who last changed what it changed (`moved`, or the settings
    /// when `settings`), by the steps after it.
    fn whose(&self, at: usize, head: &str, moved: &[Id], settings: bool, now: &Model, after: &Model) -> String {
        let things = |ids: &[Id]| {
            let named: Vec<String> = ids
                .iter()
                .take(5)
                .map(|id| format!("{} {id}", now.elements.get(*id).or_else(|| after.elements.get(*id)).map_or("element", |e| e.category())))
                .collect();
            named.join(", ")
        };
        for j in (at + 1..self.undo.len()).rev() {
            let (st, before_j) = &self.undo[j];
            let after_j = self.undo.get(j + 1).map_or(now, |(_, m)| &**m);
            let (ids_j, settings_j) = after_j.diff(before_j);
            let hit: Vec<Id> = moved.iter().copied().filter(|id| ids_j.contains(id)).collect();
            let who = st.by.as_deref().unwrap_or("someone");
            if !hit.is_empty() {
                return format!("{head}: {who} changed {} after it ({who}'s {})", things(&hit), st.label);
            }
            if settings && settings_j {
                return format!("{head}: {who} changed the project's settings after it ({who}'s {})", st.label);
            }
        }
        let what = if moved.is_empty() { "the project's settings".to_string() } else { things(moved) };
        format!("{head}: {what} changed after it")
    }
}

/// A script's line, read.
pub struct ScriptLine<'a> {
    /// The name it is given, `@name = `.
    pub name: Option<&'a str>,
    pub command: &'a str,
    /// The text of its params, JSON, or nothing.
    pub params: &'a str,
}

/// A script's line read; none for a blank line or a comment.
pub fn script_line(line: &str) -> Result<Option<ScriptLine<'_>>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let (name, command) = match line.strip_prefix('@') {
        Some(named) => {
            let (name, command) = named.split_once('=').ok_or("a named line is `@name = command {params}`: the `=` is missing")?;
            let name = name.trim();
            if !is_name(name) {
                return Err(format!("`@{}` isn't a line's name: one is letters, digits, `_` and `-`", reference_text(name)));
            }
            (Some(name), command.trim())
        }
        None => (None, line),
    };
    if command.is_empty() {
        return Err("no command after the line's name".into());
    }
    let (command, params) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
    Ok(Some(ScriptLine { name, command, params }))
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `params` with each string that is a reference put as what it points at, in what the script's lines before returned
/// (`done`, `names` their names: place in `done`, line number). A reference is a whole string: `@name`, `$last` or `$-n`,
/// standing for that reply's `id`, or followed by a path into the reply, `.key` or `.index` (`@walls.ids.0`). A string that
/// begins `@@` or `$$` is itself without the first character; any other string is itself.
fn refer(params: &Value, done: &[Value], names: &[(&str, usize, usize)]) -> Result<Value, String> {
    expand(params, done, names, &mut ScriptBudget { remaining: SCRIPT_EXPANSION_LIMIT }, 1)
}

// A conservative accounting limit for each expanded line: 128 bytes per JSON value plus string/key
// bytes. Charge a whole selected subtree before cloning it, so repeated small references cannot
// amplify an earlier reply without bound. The depth limit also applies to replies from external
// commands, which need not have passed serde_json's input depth limit.
const SCRIPT_EXPANSION_LIMIT: usize = 16 << 20;
const SCRIPT_DEPTH_LIMIT: usize = 128;

struct ScriptBudget {
    remaining: usize,
}

impl ScriptBudget {
    fn take(&mut self, bytes: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| format!("expanded parameters exceed the {SCRIPT_EXPANSION_LIMIT} byte script expansion budget"))?;
        Ok(())
    }

    fn enter(&mut self, depth: usize) -> Result<(), String> {
        if depth > SCRIPT_DEPTH_LIMIT {
            return Err(format!("expanded parameters exceed the script depth limit of {SCRIPT_DEPTH_LIMIT}"));
        }
        self.take(128)
    }

    fn value(&mut self, value: &Value, depth: usize) -> Result<(), String> {
        self.enter(depth)?;
        match value {
            Value::String(s) => self.take(s.len())?,
            Value::Array(a) => {
                for value in a {
                    self.value(value, depth + 1)?;
                }
            }
            Value::Object(o) => {
                for (key, value) in o {
                    self.take(key.len())?;
                    self.value(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Keep reference diagnostics independent of the size of an earlier reply or a hostile field name.
fn reference_text(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

fn expand(
    params: &Value,
    done: &[Value],
    names: &[(&str, usize, usize)],
    budget: &mut ScriptBudget,
    depth: usize,
) -> Result<Value, String> {
    Ok(match params {
        Value::String(s) if s.starts_with("@@") || s.starts_with("$$") => {
            budget.enter(depth)?;
            budget.take(s.len() - 1)?;
            Value::String(s[1..].to_string())
        }
        Value::String(s) => {
            let reference = reference_text(s);
            let (head, path) = s.split_once('.').map_or((s.as_str(), None), |(head, path)| (head, Some(path)));
            let target = if let Some(name) = head.strip_prefix('@').filter(|n| is_name(n)) {
                let (_, i, line) = names
                    .iter()
                    .rev()
                    .find(|(m, _, _)| *m == name)
                    .ok_or_else(|| format!("`{reference}`: no line before it is named @{}", reference_text(name)))?;
                Some((*i, format!("line {line}")))
            } else {
                let back = match head {
                    "$last" => Some(1),
                    _ => match head.strip_prefix("$-") {
                        Some(n) => Some(
                            n.parse::<usize>()
                                .ok()
                                .filter(|n| *n > 0)
                                .ok_or_else(|| format!("`{reference}`: a relative reference needs a positive command count, as `$-2`"))?,
                        ),
                        None => None,
                    },
                };
                match back {
                    Some(b) => {
                        let i = done.len().checked_sub(b).ok_or_else(|| format!("`{reference}`: lines run before it: {}", done.len()))?;
                        Some((i, format!("the line {b} back")))
                    }
                    None => None,
                }
            };
            match target {
                None => {
                    budget.value(params, depth)?;
                    params.clone()
                }
                Some((i, which)) => {
                    let mut v = done.get(i).ok_or_else(|| format!("`{reference}`: {which} returned nothing"))?;
                    if path.is_none() {
                        let id = v.get("id").ok_or_else(|| {
                            format!("`{reference}`: {which} returned no `id`; name a field of its reply, as `@name.ids.0`")
                        })?;
                        budget.value(id, depth)?;
                        id.clone()
                    } else {
                        for key in path.unwrap_or_default().split('.') {
                            if key.is_empty() {
                                return Err(format!("`{reference}`: a reference path has an empty field"));
                            }
                            v = match v {
                                Value::Array(a) => key.parse::<usize>().ok().and_then(|k| a.get(k)),
                                _ => v.get(key),
                            }
                            .ok_or_else(|| format!("`{reference}`: what {which} returned has no `{}`", reference_text(key)))?;
                        }
                        budget.value(v, depth)?;
                        v.clone()
                    }
                }
            }
        }
        Value::Array(a) => {
            budget.enter(depth)?;
            Value::Array(a.iter().map(|x| expand(x, done, names, budget, depth + 1)).collect::<Result<_, _>>()?)
        }
        Value::Object(o) => {
            budget.enter(depth)?;
            Value::Object(
                o.iter()
                    .map(|(k, x)| {
                        budget.take(k.len())?;
                        let value = expand(x, done, names, budget, depth + 1)?;
                        Ok((k.clone(), value))
                    })
                    .collect::<Result<_, String>>()?,
            )
        }
        _ => {
            budget.value(params, depth)?;
            params.clone()
        }
    })
}

/// What a session that may only read keeps through a command: its project, its history, its file, the phase and option it makes
/// things in, and how many times it was replaced.
/// What a dry run refuses `cmd` with `params` for, before it runs: replacing the project, saving it, stepping through its history,
/// or any file read or written ([`touches_files`], and a `path` or `save` any command is given).
fn dry_run_refuses(cmd: &Command, params: &Value) -> Option<String> {
    if REPLACES.contains(&cmd.id) {
        return Some("replace the project".into());
    }
    if matches!(cmd.id, "save" | "autosave" | "undo" | "redo") {
        return Some(format!("{} the project", if cmd.id == "autosave" { "save" } else { cmd.id }));
    }
    if let Some(key) = file_key(params) {
        return Some(format!("name a file (`{key}`)"));
    }
    touches_files(cmd, params).map(|files| match files {
        Files::Read => "read a file".to_string(),
        _ => "write a file".to_string(),
    })
}

/// The key of `params` that names a file, whatever the command: `path` or `save`.
fn file_key(params: &Value) -> Option<&'static str> {
    ["path", "save"].into_iter().find(|key| params.get(key).is_some_and(|v| !v.is_null()))
}

struct Kept {
    model: Arc<Model>,
    /// The model the last command left that passed the check, as it was: put back with the model, the next command checks
    /// only what it changes.
    verified: Option<Arc<Model>>,
    path: Option<PathBuf>,
    phase: Option<String>,
    option: Option<String>,
    undo: Vec<(Step, Arc<Model>)>,
    redo: Vec<(Step, Arc<Model>)>,
    resets: usize,
}

impl Kept {
    fn of(s: &Session) -> Kept {
        Kept {
            model: Arc::clone(&s.model),
            verified: s.verified.clone(),
            path: s.path.clone(),
            phase: s.phase.clone(),
            option: s.option.clone(),
            undo: s.undo.clone(),
            redo: s.redo.clone(),
            resets: s.resets,
        }
    }

    /// Puts the session back as it was kept, and says whether anything had changed but its file: a copy saved as another file
    /// leaves the session with its own, and is no change.
    fn keep(self, s: &mut Session) -> Result<(), ()> {
        let same = (Arc::ptr_eq(&self.model, &s.model) || *self.model == *s.model)
            && (s.phase == self.phase && s.option == self.option && s.resets == self.resets)
            && (s.undo.len() == self.undo.len() && s.redo.len() == self.redo.len());
        (s.model, s.verified, s.path, s.phase, s.option) = (self.model, self.verified, self.path, self.phase, self.option);
        (s.undo, s.redo, s.resets) = (self.undo, self.redo, self.resets);
        if same { Ok(()) } else { Err(()) }
    }
}

/// Whether two paths name the same file: as the system resolves them where both are there, else as they are written.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
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
            // A `guid` field says which thing it is, as a definition for a dictionary does, and `definition` which one a family's parameter is tied to: they stay GUIDs.
            Value::Object(o) => {
                o.iter_mut().filter(|(k, _)| !matches!(k.as_str(), "guid" | "definition")).for_each(|(_, x)| walk(m, x, changed))
            }
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

/// The library synced both ways, by what changed since `before` and whole, each new GUID made the same way in order:
/// what the engine holds the incremental sync to in tests. None where the incremental one can't say.
#[cfg(test)]
fn sync_oracle(m: &Model, before: &Model) -> Option<(Model, Model)> {
    let counter = || {
        let mut n = 0u64;
        move |_: &Model, _: Option<&crate::library::Kind>, _: &str| {
            n += 1;
            crate::ids::Guid::seeded(0x5eed, n)
        }
    };
    let mut incremental = m.clone();
    if !incremental.sync_since_with(before, &mut counter()) {
        return None;
    }
    let mut whole = m.clone();
    whole.sync_library_with(&mut counter());
    Some((incremental, whole))
}
