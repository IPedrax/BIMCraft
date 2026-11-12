//! Revit's "Family Already Exists": a family loaded into the project (Load into Project, Load into Project and Close, or Load
//! Family from a file) under a name the project already has, defined otherwise, asks what to do instead of merging silently.
//! The answer is only the `family` command's `overwrite` (`version`, `version_and_values`; Cancel keeps the project's, as `no`
//! does), so what an agent or a script passes and what the dialog picks are the same command. A family the project hasn't,
//! or one that would load unchanged, asks nothing.
//!
//! A load asks its questions in turn (`Question`): the family's own first, then each shared family nested in it that the
//! project already has ("Shared Family Already Loaded"). Nested families (T6) don't exist yet, so nothing finds the second
//! kind: `document::State::shared_nested` (`FamilyEditor::set_shared_nested`) is where T6 puts them, and until then only a test does.
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

/// What is done once the family is loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum After {
    /// From the family document: close it (Load into Project and Close), place the test placement, give it to the instance.
    Document { close: bool, place: bool, apply_placement: bool },
    /// From Load Family: the Component tool takes the family.
    Component,
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
}

/// What a load decided: the project's family overwritten how (none: not asked), and each shared nested family's version.
/// The last load's is kept (`BimApp::family_decided`), for the `family` command to take the nested ones once T6 gives it them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decided {
    pub(crate) name: String,
    pub(crate) overwrite: Option<Overwrite>,
    pub(crate) shared: Vec<(String, Shared)>,
}

/// The family document's shared nested families the project already has, defined otherwise. T6 works them out from the
/// family's nested families; until then they are what was put in `shared_nested`.
fn shared_nested(ed: &FamilyEditor) -> Vec<String> {
    ed.document.shared_nested.clone()
}

impl BimApp {
    /// A load of family `name`: `source` loads it as the file or definition has it (`{"path"}` or `{"definition", "name"}`),
    /// `legacy` is the command a load runs when the project hasn't the family or the family document was renamed. Whether
    /// the project's would stay as it is (`same`, else the core's dry run says so) decides whether it is asked about.
    fn plan_family_load(&mut self, name: &str, source: Value, legacy: Value, same: bool, nested: Vec<String>, after: After) -> Loading {
        let exists = self.session.model.families.contains_key(name);
        let same = exists && (same || self.loads_unchanged(&source));
        let mut questions = VecDeque::new();
        if exists && !same {
            questions.push_back(Question::Exists(name.to_string()));
        }
        questions.extend(nested.into_iter().map(Question::SharedNested));
        let command = if !exists {
            Some(legacy)
        } else if same {
            None
        } else {
            Some(source)
        };
        Loading { name: name.to_string(), command, questions, decided: Decided { name: name.to_string(), ..Decided::default() }, after }
    }

    /// Whether loading `source` over the project's family would change nothing: the core's dry run says it wasn't overwritten.
    fn loads_unchanged(&mut self, source: &Value) -> bool {
        let mut probe = source.clone();
        probe["overwrite"] = json!("version");
        probe["dry_run"] = json!(true);
        self.session.execute("family", &probe).is_ok_and(|r| r["overwritten"] == json!(false))
    }

    /// Loads the family document's draft into the project, asking first when the project has it otherwise; `after` says
    /// what follows. The document is the window's (`family_editor`) while it waits.
    pub(super) fn load_draft(&mut self, ed: &mut FamilyEditor, after: After) {
        let base = Arc::clone(&self.session.model);
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
        let nested = shared_nested(ed);
        let mut loading = if renamed {
            Loading {
                name: name.clone(),
                command: Some(legacy),
                questions: nested.into_iter().map(Question::SharedNested).collect(),
                decided: Decided { name, ..Decided::default() },
                after,
            }
        } else {
            self.plan_family_load(&name, json!({"definition": definition, "name": name}), legacy, same, nested, after)
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
        let name = match self.session.execute("family", &json!({"path": path.to_string_lossy(), "dry_run": true})) {
            Ok(r) => r["name"].as_str().unwrap_or_default().to_string(),
            Err(text) => {
                self.status = Status { text, error: true };
                return;
            }
        };
        // A file holds no nested families yet: T6 asks about its shared ones here too.
        let loading = self.plan_family_load(&name, source.clone(), source, false, Vec::new(), After::Component);
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
        let After::Document { close, place, apply_placement } = loading.after else { return };
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
        let Some(question) = loading.questions.front().cloned() else {
            self.end_family_load(loading);
            return;
        };
        let (title, text, choices): (&str, String, [&str; 2]) = match &question {
            Question::Exists(name) => (
                t(EXISTS),
                tf(
                    "The family {} you are loading is already in this project, defined differently. What would you like to do?",
                    &[name.as_str()],
                ),
                [OVERWRITE, OVERWRITE_VALUES],
            ),
            Question::SharedNested(name) => (
                t(SHARED),
                tf(
                    "The shared family {} nested in this family is already loaded in the project. Which version should the project use?",
                    &[name.as_str()],
                ),
                [KEEP_PROJECT, TAKE_FAMILY],
            ),
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
            Question::SharedNested(name) => loading.decided.shared.push((name, if i == 0 { Shared::Project } else { Shared::Family })),
        }
        if loading.questions.is_empty() {
            self.end_family_load(loading);
        } else {
            self.family_loading = Some(loading);
        }
    }

    /// A load answered: run it from the document it came from (closing it on Load into Project and Close, when it went in),
    /// or for the Component tool.
    fn end_family_load(&mut self, mut loading: Loading) {
        match loading.after {
            After::Component => self.finish_component_load(loading),
            After::Document { .. } => {
                // Closed meanwhile, the document has nothing to load.
                let Some(mut ed) = self.family_editor.take() else { return };
                self.finish_family_load(&mut ed, &mut loading);
                if !std::mem::take(&mut ed.document.loaded_to_close) {
                    self.family_editor = Some(ed);
                }
            }
        }
    }
}

/// The dialogs' titles and choices, close to Revit's.
const EXISTS: &str = msg!("Family Already Exists");
const OVERWRITE: &str = msg!("Overwrite the existing version");
const OVERWRITE_VALUES: &str = msg!("Overwrite the existing version and its parameter values");
const SHARED: &str = msg!("Shared Family Already Loaded");
const KEEP_PROJECT: &str = msg!("Use the project's version");
const TAKE_FAMILY: &str = msg!("Overwrite it with the version in this family");
