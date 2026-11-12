//! Revit's "Family Already Exists": a family loaded into the project (Load into Project, Load into Project and Close, or Load
//! Family from a file) under a name the project already has, defined otherwise, asks what to do instead of merging silently.
//! The answer is only the `family` command's `overwrite` (`version`, `version_and_values`; Cancel keeps the project's, as `no`
//! does), so what an agent or a script passes and what the dialog picks are the same command. A family the project hasn't,
//! or one that would load unchanged, asks nothing.
//!
//! A load asks its questions in turn (`Question`): the family's own first, then each shared family nested in it that the
//! project already has ("Shared Family Already Loaded"), as the core's dry run finds them (`shared_conflicts`); the answers
//! go into the command (`shared_nested`). Loading into a family (T6a: Insert > Load Family in a family document, or Load into
//! Project from a nested family's document opened by Edit Family) asks the same, against that family's nested families.
use super::tabs::DocId;
use super::*;
use std::collections::VecDeque;
use std::path::Path;

/// What a load asks before it goes ahead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Question {
    /// The project has a family of this name, defined otherwise: overwrite it, or keep it.
    Exists(String),
    /// A shared family nested in the one loaded is in the project already: keep the project's, or take the family's.
    SharedNested(String),
    /// Dimensions of the family loaded measure to references it no longer has (T14): delete them, or don't load it. What the
    /// `family` command said of them.
    InvalidDimensions(String),
}

/// How a family already in the project is overwritten, as the `family` command's `overwrite` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overwrite {
    /// The definition replaced, the project's parameter values kept where they still fit.
    Version,
    /// The definition replaced, with the incoming parameter values.
    VersionAndValues,
}

impl Overwrite {
    fn name(self) -> &'static str {
        match self {
            Overwrite::Version => "version",
            Overwrite::VersionAndValues => "version_and_values",
        }
    }
}

/// Which version of a shared nested family the project keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shared {
    /// The project's own, as it is.
    Project,
    /// The one inside the family loaded, overwriting the project's.
    Family,
}

/// What is done once the family is loaded. A load from a family document names it (`doc`): the questions may be answered
/// after another tab came forward, and the load finishes in the document that started it, or nowhere if that one was closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum After {
    /// From family document `doc`: close it (Load into Project and Close), place the test placement, give it to the instance.
    Document { doc: DocId, close: bool, place: bool, apply_placement: bool },
    /// From Load Family: the Component tool takes the family.
    Component,
    /// Insert > Load Family in family document `doc`: the family goes into it (T6a).
    Nest { doc: DocId },
    /// Load into Project from nested family document `doc`: it goes into the document under it, which it closes back to with
    /// `close`.
    IntoHost { doc: DocId, close: bool },
}

impl After {
    /// The family document the load is from; none for Load Family into the project.
    fn doc(self) -> Option<DocId> {
        match self {
            After::Document { doc, .. } | After::Nest { doc } | After::IntoHost { doc, .. } => Some(doc),
            After::Component => None,
        }
    }
}

/// A load waiting for its answers.
pub(crate) struct Loading {
    /// The family's name in the project.
    pub(crate) name: String,
    /// The `family` command it runs, `overwrite` added when the project's family is asked about; none when the family would
    /// load unchanged and only nested families were asked about.
    command: Option<Value>,
    questions: VecDeque<Question>,
    decided: Decided,
    after: After,
    /// The family loaded into a family (`After::Nest`, `After::IntoHost`), in place of a command.
    nest: Option<Family>,
}

/// What a load decided: the project's family overwritten how (none: not asked), and each shared nested family's version.
/// The last load's is kept (`BimApp::family_decided`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decided {
    pub(crate) name: String,
    pub(crate) overwrite: Option<Overwrite>,
    pub(crate) shared: Vec<(String, Shared)>,
}

impl Decided {
    /// The answers about shared nested families, as the core takes them.
    fn answers(&self) -> BTreeMap<String, bimcraft_core::family::Keep> {
        use bimcraft_core::family::Keep;
        self.shared.iter().map(|(n, s)| (n.clone(), if *s == Shared::Project { Keep::Project } else { Keep::Family })).collect()
    }
}

impl BimApp {
    /// A load of family `name`: `source` loads it as the file or definition has it (`{"path"}` or `{"definition", "name"}`),
    /// `legacy` is the command a load runs when the project hasn't the family or the family document was renamed. Whether
    /// the project's would stay as it is (`same`, else the core's dry run says so) decides whether it is asked about.
    fn plan_family_load(&mut self, name: &str, source: Value, legacy: Value, same: bool, after: After) -> Loading {
        let exists = self.session.model.families.contains_key(name);
        let same = exists && (same || self.loads_unchanged(&source));
        let mut questions = VecDeque::new();
        if exists && !same {
            questions.push_back(Question::Exists(name.to_string()));
        }
        let command = if !exists {
            Some(legacy)
        } else if same {
            None
        } else {
            Some(source)
        };
        if let Some(c) = &command {
            questions.extend(self.shared_conflicts(c, exists).into_iter().map(Question::SharedNested));
        }
        // Dimensions measuring to what the family hasn't: asked first, an error that can't be passed over (T14).
        if let Some(c) = &command
            && let Some(said) = self.invalid_dimensions(c, exists)
        {
            questions.push_front(Question::InvalidDimensions(said));
        }
        Loading {
            name: name.to_string(),
            command,
            questions,
            decided: Decided { name: name.to_string(), ..Decided::default() },
            after,
            nest: None,
        }
    }

    /// The shared families nested in what `command` loads that the project has otherwise: the core's dry run says them, as a
    /// reload (`overwrite`) where the project has the family.
    fn shared_conflicts(&mut self, command: &Value, exists: bool) -> Vec<String> {
        let mut probe = command.clone();
        probe["dry_run"] = json!(true);
        if exists && probe.get("overwrite").is_none() && (probe.get("path").is_some() || probe.get("definition").is_some()) {
            probe["overwrite"] = json!("version");
        }
        match self.session.execute("family", &probe) {
            Ok(r) => r["shared_conflicts"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Family `family`, named `name`, loaded into a family: document `doc` (`After::Nest`) or, from a nested family's
    /// document, the one under it (`After::IntoHost`); asking first when that family nests one of the name defined otherwise,
    /// or a shared family `family` nests otherwise.
    pub(super) fn nest_family(&mut self, name: String, family: Family, after: After) {
        let target = match after {
            After::IntoHost { doc, .. } => self.doc(doc).and_then(|d| d.host).and_then(|h| self.family_of(h)),
            After::Nest { doc } => self.family_of(doc),
            _ => None,
        };
        let Some(target) = target else {
            self.document_closed();
            return;
        };
        let (exists, shared) = target.nest_conflicts(&name, &family);
        let mut questions: VecDeque<Question> = shared.into_iter().map(Question::SharedNested).collect();
        if exists {
            questions.push_front(Question::Exists(name.clone()));
        }
        let mut loading = Loading {
            name: name.clone(),
            command: None,
            questions,
            decided: Decided { name, ..Decided::default() },
            after,
            nest: Some(family),
        };
        if loading.questions.is_empty() {
            self.end_family_load(loading);
        } else {
            loading.decided.overwrite = None;
            self.family_loading = Some(loading);
        }
    }

    /// A load into a family answered: the family loaded into document `doc`, whose draft `ed` is (taken out), or with
    /// `into_host` into the one under it, as answered. Whether it went in; a refusal is said in the document it came from.
    fn finish_nest(&mut self, doc: DocId, ed: &mut FamilyEditor, loading: &mut Loading, into_host: bool) -> bool {
        let Some(family) = loading.nest.take() else { return false };
        let overwrite = loading.decided.overwrite.map(|o| o == Overwrite::Version);
        let answers = loading.decided.answers();
        let name = loading.name.clone();
        let done = if into_host {
            match self.doc(doc).and_then(|d| d.host).and_then(|h| self.family_of_mut(h)) {
                Some(host) => host.nest(&name, family, overwrite, &answers),
                None => Err(t("The family it was nested in is closed.").to_string()),
            }
        } else {
            ed.nest(&name, family, overwrite, &answers)
        };
        self.family_decided = Some(std::mem::take(&mut loading.decided));
        match done {
            Ok(_) => {
                ed.document.refused = None;
                if into_host {
                    ed.original = Some(name);
                    ed.saved_as(ed.draft());
                }
                true
            }
            Err(e) => {
                ed.document.refused = Some(e);
                false
            }
        }
    }

    /// What the `family` command says of `command`'s dimensions measuring to references the family hasn't, if it says so: its
    /// dry run, as a reload (`overwrite`) where the project has the family.
    fn invalid_dimensions(&mut self, command: &Value, exists: bool) -> Option<String> {
        let mut probe = command.clone();
        probe["dry_run"] = json!(true);
        if exists && probe.get("overwrite").is_none() && (probe.get("path").is_some() || probe.get("definition").is_some()) {
            probe["overwrite"] = json!("version");
        }
        match self.session.execute("family", &probe) {
            Err(e) if e.contains(bimcraft_core::family::constraints::INVALID_DIMENSIONS) => Some(e),
            _ => None,
        }
    }

    /// Whether loading `source` over the project's family would change nothing: the core's dry run says it wasn't overwritten.
    fn loads_unchanged(&mut self, source: &Value) -> bool {
        let mut probe = source.clone();
        probe["overwrite"] = json!("version");
        probe["dry_run"] = json!(true);
        self.session.execute("family", &probe).is_ok_and(|r| r["overwritten"] == json!(false))
    }

    /// Loads the family document's draft into the project, asking first when the project has it otherwise; `after` says
    /// what follows, and which document it finishes in.
    pub(super) fn load_draft(&mut self, ed: &mut FamilyEditor, after: After) {
        let base = Arc::clone(&self.session.model);
        // The project may have changed under the open document (an agent, Purge, a project undo): a family it no longer has
        // loads as new, and an instance it was opened from that is gone no longer replays.
        if ed.original.as_ref().is_some_and(|was| !base.families.contains_key(was)) {
            ed.original = None;
        }
        if ed.source.is_some_and(|id| !base.elements.contains_key(id)) {
            (ed.source, ed.replay) = (None, None);
        }
        let prepared = ed.command(&base).and_then(|legacy| {
            let family = ed.family()?;
            let definition = serde_json::to_value(&family).map_err(|e| e.to_string())?;
            Ok((legacy, family, definition))
        });
        let (legacy, family, definition) = match prepared {
            Ok(p) => p,
            Err(e) => {
                ed.document.refused = Some(e);
                return;
            }
        };
        let name = ed.name.trim().to_string();
        // Renamed in the document, it renames the project's family: nothing of that name is overwritten.
        let renamed = ed.original.as_deref().is_some_and(|was| was != name);
        // The family as the document would open it from the project: the same draft, so loading it changes nothing.
        let same = base.families.get(&name).and_then(|f| FamilyEditor::of(&name, f).family().ok()).is_some_and(|f| f == family);
        let mut loading = if renamed {
            let questions = self.shared_conflicts(&legacy, true).into_iter().map(Question::SharedNested).collect();
            Loading {
                name: name.clone(),
                command: Some(legacy),
                questions,
                decided: Decided { name, ..Decided::default() },
                after,
                nest: None,
            }
        } else {
            self.plan_family_load(&name, json!({"definition": definition, "name": name}), legacy, same, after)
        };
        ed.document.refused = None;
        if loading.questions.is_empty() {
            self.finish_family_load(ed, &mut loading);
        } else {
            self.family_loading = Some(loading);
        }
    }

    /// Load Family: the family in the file at `path`, asking first when the project has it otherwise; the Component tool
    /// then takes it.
    pub(crate) fn load_family_file(&mut self, path: &Path) {
        let source = json!({"path": path.to_string_lossy()});
        // The name it loads under, as the command works it out; a file that isn't a family says so as the load would.
        // A file whose dimensions measure to nothing loads under its name all the same, once they are deleted: asked below.
        let named = self.session.execute("family", &json!({"path": path.to_string_lossy(), "dry_run": true})).or_else(|e| {
            if e.contains(bimcraft_core::family::constraints::INVALID_DIMENSIONS) {
                self.session.execute("family", &json!({"path": path.to_string_lossy(), "dry_run": true, "invalid_dimensions": "delete"}))
            } else {
                Err(e)
            }
        });
        let name = match named {
            Ok(r) => r["name"].as_str().unwrap_or_default().to_string(),
            Err(text) => {
                self.status = Status { text, error: true };
                return;
            }
        };
        // Its shared nested families the project has otherwise are asked about too.
        let loading = self.plan_family_load(&name, source.clone(), source, false, After::Component);
        if loading.questions.is_empty() {
            self.finish_component_load(loading);
        } else {
            self.family_loading = Some(loading);
        }
    }

    /// Runs a load whose questions are answered: its command, then what follows it. A refusal is said in the document.
    fn finish_family_load(&mut self, ed: &mut FamilyEditor, loading: &mut Loading) {
        let made = match loading.command.take() {
            None => true,
            Some(p) => self.run("family", p).is_some(),
        };
        self.family_decided = Some(std::mem::take(&mut loading.decided));
        let After::Document { close, place, apply_placement, .. } = loading.after else {
            return;
        };
        if !made {
            // Said in the document; the box over the view's corner would lie on its buttons. Kept apart from what the
            // preview says, which is worked out again every frame, so drawing it never hides this.
            ed.document.refused = Some(std::mem::take(&mut self.status).text);
            return;
        }
        ed.original = Some(loading.name.clone());
        ed.document.refused = None;
        ed.saved_as(ed.draft());
        if let Some(d) = &ed.adaptive {
            if apply_placement && let Some(id) = ed.replay {
                self.run("modify", json!({"id":id,"adaptive":d.preview}));
            }
            // The test frames are taken as the project's, so a captured context builds where it was captured.
            if place && let Some((type_name, _)) = ed.types.get(ed.picked) {
                self.run(
                    "adaptive_place",
                    json!({"family":loading.name,"type":type_name,"placements":[{"params":ed.preview_params,"adaptive":d.completed(&d.preview)}]}),
                );
            }
        }
        ed.document.loaded_to_close = close;
    }

    /// Load Family's load done: the Component tool takes the family and its first type.
    fn finish_component_load(&mut self, mut loading: Loading) {
        let made = match loading.command.take() {
            None => true,
            Some(p) => self.run("family", p).is_some(),
        };
        self.family_decided = Some(std::mem::take(&mut loading.decided));
        if made && let Some(f) = self.session.model.families.get(&loading.name) {
            let first = f.types.keys().next().cloned().unwrap_or_default();
            self.component = Some((loading.name, first));
        }
    }

    /// The question a waiting load asks, as Revit's dialogs ask it; answered, the next, and once none is left the load.
    /// Cancel, the corner's × or Escape drop the load: the project keeps what it has, and the document stays open.
    pub(crate) fn family_load_dialog(&mut self, ctx: &Context) {
        let Some(mut loading) = self.family_loading.take() else { return };
        // The document that asked, closed meanwhile: nothing left to load into, nor from.
        if loading.after.doc().is_some_and(|doc| self.doc(doc).is_none()) {
            self.document_closed();
            return;
        }
        let Some(question) = loading.questions.front().cloned() else {
            self.end_family_load(loading);
            return;
        };
        let into_family = loading.nest.is_some();
        let (title, text, choices): (&str, String, Vec<&str>) = match &question {
            Question::Exists(name) if into_family => (
                t(EXISTS),
                tf(
                    "The family {} you are loading is already in this family, defined differently. What would you like to do?",
                    &[name.as_str()],
                ),
                vec![OVERWRITE, OVERWRITE_VALUES],
            ),
            Question::Exists(name) => (
                t(EXISTS),
                tf(
                    "The family {} you are loading is already in this project, defined differently. What would you like to do?",
                    &[name.as_str()],
                ),
                vec![OVERWRITE, OVERWRITE_VALUES],
            ),
            Question::SharedNested(name) if into_family => (
                t(SHARED),
                tf(
                    "The shared family {} nested in the family you are loading is already in this family. Which version should it use?",
                    &[name.as_str()],
                ),
                vec![KEEP_HOST, TAKE_FAMILY],
            ),
            Question::SharedNested(name) => (
                t(SHARED),
                tf(
                    "The shared family {} nested in this family is already loaded in the project. Which version should the project use?",
                    &[name.as_str()],
                ),
                vec![KEEP_PROJECT, TAKE_FAMILY],
            ),
            Question::InvalidDimensions(said) => {
                (t(INVALID), tf("Dimensions of the family you are loading can't be kept: {}", &[said.as_str()]), vec![DELETE_DIMENSIONS])
            }
        };
        let (mut chosen, mut cancel) = (None, false);
        let r = Modal::new(UiId::new("family-already-exists"), title).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(text);
                ui.add_space(10.0);
                for (i, label) in choices.iter().enumerate() {
                    let size = vec2(ui.available_width(), 32.0);
                    if ui.add_sized(size, egui::Button::new(RichText::new(t(label)).strong())).clicked() {
                        chosen = Some(i);
                    }
                    ui.add_space(4.0);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if cancel || (chosen.is_none() && r.should_close()) {
            return;
        }
        let Some(i) = chosen else {
            self.family_loading = Some(loading);
            return;
        };
        loading.questions.pop_front();
        match question {
            Question::Exists(_) => {
                let how = if i == 0 { Overwrite::Version } else { Overwrite::VersionAndValues };
                loading.decided.overwrite = Some(how);
                if let Some(p) = loading.command.as_mut() {
                    p["overwrite"] = json!(how.name());
                }
            }
            Question::SharedNested(name) => {
                let keep = if i == 0 { Shared::Project } else { Shared::Family };
                if let Some(p) = loading.command.as_mut() {
                    p["shared_nested"][&name] = json!(if keep == Shared::Project { "project" } else { "family" });
                }
                loading.decided.shared.push((name, keep));
            }
            Question::InvalidDimensions(_) => {
                if let Some(p) = loading.command.as_mut() {
                    p["invalid_dimensions"] = json!("delete");
                }
            }
        }
        if loading.questions.is_empty() {
            self.end_family_load(loading);
        } else {
            self.family_loading = Some(loading);
        }
    }

    /// A load answered: run it from the document it came from, whichever tab is active (closing it on Load into Project and
    /// Close, when it went in), or for the Component tool. That document closed meanwhile, nothing is loaded anywhere.
    fn end_family_load(&mut self, mut loading: Loading) {
        match loading.after {
            After::Component => self.finish_component_load(loading),
            After::Nest { doc } => {
                let Some(mut ed) = self.take_family_of(doc) else { return self.document_closed() };
                self.finish_nest(doc, &mut ed, &mut loading, false);
                self.put_family_of(doc, ed);
            }
            After::IntoHost { doc, close } => {
                let Some(mut ed) = self.take_family_of(doc) else { return self.document_closed() };
                let made = self.finish_nest(doc, &mut ed, &mut loading, true);
                self.put_family_of(doc, ed);
                // Load into Family and Close: back to the family it was nested in.
                if made && close {
                    self.close_family_document(doc);
                }
            }
            After::Document { doc, .. } => {
                let Some(mut ed) = self.take_family_of(doc) else { return self.document_closed() };
                self.finish_family_load(&mut ed, &mut loading);
                let closed = std::mem::take(&mut ed.document.loaded_to_close);
                self.put_family_of(doc, ed);
                if closed {
                    self.close_family_document(doc);
                }
            }
        }
    }

    /// A load whose family document was closed before it finished: said in the status bar, nothing loaded anywhere.
    pub(super) fn document_closed(&mut self) {
        self.status =
            Status { text: t("The family document was closed before the load finished: nothing was loaded.").to_string(), error: true };
    }
}

/// The dialogs' titles and choices, close to Revit's.
const EXISTS: &str = msg!("Family Already Exists");
const OVERWRITE: &str = msg!("Overwrite the existing version");
const OVERWRITE_VALUES: &str = msg!("Overwrite the existing version and its parameter values");
const SHARED: &str = msg!("Shared Family Already Loaded");
const KEEP_PROJECT: &str = msg!("Use the project's version");
const KEEP_HOST: &str = msg!("Use this family's version");
const TAKE_FAMILY: &str = msg!("Overwrite it with the version in this family");
const INVALID: &str = msg!("Invalid Dimensions");
const DELETE_DIMENSIONS: &str = msg!("Delete Dimension(s)");
