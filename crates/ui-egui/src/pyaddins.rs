//! The PyAddins tab and the Script Automation palette (its Player): the user's Python scripts from their PyAddins folder
//! (`bimcraft_pyaddins`), each a row that opens to a form of what its `main` asks for and runs it, and ribbon buttons made of
//! them. The folder is read again whenever it changes while the tab or the palette is shown, so a script an agent writes
//! is there a moment later. A run is a process of the user's Python; its commands are answered here, a few each frame, as
//! an agent's are, and the whole run is one undo step. Stop kills it; Restart runs the last script again with the same
//! values. The window has no folder until `main` gives it one (`BimApp::use_pyaddins`), so no test touches the user's.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bimcraft_core::bridge;
use bimcraft_core::engine::Mark;
use bimcraft_i18n::msg;
use bimcraft_pyaddins::store::Stamp;
use bimcraft_pyaddins::{Button, Description, Event, Kind, Python, Run, Store, inputs};
use egui::{Align, ComboBox, Context, Frame, Id as UiId, Layout, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::dock::Palette;
use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::ribbon::panel;
use crate::style::pal;
use crate::{BimApp, Status, big_button, stacked, stacked_button};

/// How often the folder is looked at while the tab or the palette shows it.
const LOOK: Duration = Duration::from_millis(800);
/// How many of a script's requests a frame answers.
const PER_FRAME: usize = 64;
/// What a new script starts as.
const TEMPLATE: &str = r#""""Dummy script: every kind of field the Player makes, to copy from. It changes nothing."""
__title__ = "Dummy Script"
from typing import Annotated, Literal

import bimcraft as bc


def main(dummy_level: bc.Level,
         dummy_wall_type: Annotated[str, bc.Library("type:wall")],
         dummy_dropdown: Literal["Dummy A", "Dummy B", "Dummy C"] = "Dummy A",
         dummy_text_field: str = "Dummy text",
         dummy_number: Annotated[int, bc.Range(1, 10)] = 3,
         dummy_length: bc.Length = 1000,
         dummy_checkbox: bool = True,
         dummy_selection: bc.Selection = bc.SELECTION):
    """Dummy script: prints what each dummy field was given, and changes nothing.

    dummy_level: a dummy list of the project's levels
    dummy_wall_type: a dummy list of the project's wall types
    dummy_dropdown: a dummy list of choices written in the script
    dummy_text_field: dummy text
    dummy_number: a dummy whole number, from 1 to 10
    dummy_length: a dummy length, typed in the project's units, given in millimetres
    dummy_checkbox: a dummy yes or no
    dummy_selection: the dummy elements: what is selected
    """
    print("Dummy level:", dummy_level)
    print("Dummy wall type:", dummy_wall_type)
    print("Dummy dropdown:", dummy_dropdown)
    print("Dummy text field:", dummy_text_field)
    print("Dummy number:", dummy_number)
    print("Dummy length:", dummy_length, "mm")
    print("Dummy checkbox:", dummy_checkbox)
    print("Dummy selection:", len(dummy_selection), "elements")
    walls = bc.query(category="wall", level=dummy_level)
    return "Dummy script ran: %d walls on %s, nothing changed" % (len(walls), dummy_level)
"#;

/// What a script asks for, by the time and size of the file it was read from.
type Described = (Option<SystemTime>, u64, Result<Description, String>);

/// A value typed in a script's form.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Field {
    Text(String),
    Bool(bool),
}

/// A run going on.
pub(crate) struct Running {
    run: Run,
    script: String,
    mark: Mark,
}

/// The last run: what it was given (for Restart), what it printed, and how it ended (none while it runs).
#[derive(Clone, Debug, Default)]
pub(crate) struct Last {
    pub(crate) script: String,
    pub(crate) values: Map<String, Value>,
    pub(crate) log: Vec<(String, bool)>,
    pub(crate) result: Option<Result<Option<String>, String>>,
}

/// PyAddins > Python: the Python, the editor, and the skill for agents.
#[derive(Clone, Debug, Default)]
pub(crate) struct PythonDialog {
    pub(crate) python: String,
    pub(crate) editor: String,
    pub(crate) skill: Option<Result<String, String>>,
}

/// What the window knows of the PyAddins folder.
#[derive(Default)]
pub struct PyAddins {
    pub(crate) store: Option<Store>,
    stamp: Stamp,
    /// When the folder was last looked at, on the window's clock.
    looked: Option<f64>,
    pub(crate) scripts: Vec<String>,
    pub(crate) buttons: Vec<Button>,
    python: Option<Result<Python, String>>,
    /// What each script asks for, by the time and size of the file it was read from.
    described: HashMap<String, Described>,
    /// The script whose row is open in the palette.
    pub(crate) open: Option<String>,
    pub(crate) search: String,
    pub(crate) forms: HashMap<String, HashMap<String, Field>>,
    pub(crate) advanced: bool,
    pub(crate) running: Option<Running>,
    pub(crate) last: Option<Last>,
    /// Make Button's label and panel, for the open script.
    draft: (String, String),
    /// PyAddins > Python: what is being typed, and how installing the skill went.
    pub(crate) python_dialog: Option<PythonDialog>,
    /// New Script: the name being typed.
    pub(crate) new_script: Option<String>,
}

impl PyAddins {
    /// Looks at the folder, once every [`LOOK`] of the window's clock, and reads it again when it changed.
    fn look(&mut self, time: f64) {
        if self.looked.is_some_and(|at| (time - at).abs() < LOOK.as_secs_f64()) {
            return;
        }
        self.looked = Some(time);
        if self.store.as_ref().is_some_and(|s| s.stamp() != self.stamp) {
            self.reload();
        }
    }

    /// Reads the folder again, and forgets what is no longer there.
    pub(crate) fn reload(&mut self) {
        let Some(store) = &self.store else { return };
        self.stamp = store.stamp();
        let scripts = store.scripts();
        self.scripts = scripts.iter().map(|s| s.name.clone()).collect();
        self.buttons = store.buttons();
        self.described
            .retain(|name, (modified, size, _)| scripts.iter().any(|s| &s.name == name && s.modified == *modified && s.size == *size));
        if self.open.as_ref().is_some_and(|o| !self.scripts.contains(o)) {
            self.open = None;
        }
    }

    /// What a project opened or started keeps: the folder is the user's, with what was read of it and typed in its forms; a run
    /// is stopped (dropped, it is killed), since it would work on the new project, and its log goes with it.
    pub(crate) fn carried(&mut self) -> PyAddins {
        let mut kept = std::mem::take(self);
        kept.running = None;
        kept.last = None;
        kept
    }

    /// What New Script suggests: Dummy Script, or the first Dummy Script 2, 3… not taken.
    pub(crate) fn free_name(&self) -> String {
        let base = t("Dummy Script").to_string();
        let taken = |n: &str| self.scripts.iter().any(|s| s.eq_ignore_ascii_case(n));
        (1..1000).map(|i| if i == 1 { base.clone() } else { format!("{base} {i}") }).find(|n| !taken(n)).unwrap_or(base)
    }

    fn python(&mut self) -> Result<Python, String> {
        if self.python.is_none() {
            let set = self.store.as_ref().and_then(Store::python_setting);
            self.python = Some(bimcraft_pyaddins::python::find(set.as_deref()));
        }
        self.python.clone().unwrap_or_else(|| Err("no Python".into()))
    }

    /// What `name` asks for, read again when its file changed.
    pub(crate) fn describe(&mut self, name: &str) -> Result<Description, String> {
        let store = self.store.clone().ok_or("no PyAddins folder")?;
        let path = store.path(name)?;
        let meta = std::fs::metadata(&path).map_err(|_| format!("there is no script `{name}`"))?;
        let (modified, size) = (meta.modified().ok(), meta.len());
        if let Some((m, s, d)) = self.described.get(name)
            && *m == modified
            && *s == size
        {
            return d.clone();
        }
        let d = self.python().and_then(|py| inputs::describe(&py, &store.client()?, &path));
        self.described.insert(name.to_string(), (modified, size, d.clone()));
        d
    }

    fn running(&self, script: &str) -> bool {
        self.running.as_ref().is_some_and(|r| r.script == script)
    }
}

/// `wall_type` as a person reads it: Wall Type.
fn spoken(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A small triangle that opens and closes a row.
fn disclosure(ui: &mut Ui, open: bool, label: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let c = rect.center();
    let ink = if r.hovered() { pal().text } else { pal().muted };
    let points = if open {
        vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
    } else {
        vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
    r
}

/// The play button of a script's row: a triangle in a tinted square, a square while it runs.
fn play(ui: &mut Ui, running: bool, label: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if r.hovered() { pal().select_fill } else { pal().select_fill.gamma_multiply(0.6) };
    ui.painter().rect(rect, 3.0, fill, Stroke::new(1.0, pal().accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    let c = rect.center();
    let ink = pal().accent_ink;
    if running {
        ui.painter().rect_filled(egui::Rect::from_center_size(c, vec2(8.0, 8.0)), 1.0, ink);
    } else {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![c + vec2(-3.0, -5.0), c + vec2(5.0, 0.0), c + vec2(-3.0, 5.0)],
            ink,
            Stroke::NONE,
        ));
    }
    r.on_hover_text(label)
}

impl BimApp {
    /// The PyAddins folder the window keeps its scripts in: `main` gives the user's.
    pub fn use_pyaddins(&mut self, store: Store) {
        store.seed();
        self.pyaddins.store = Some(store);
        self.pyaddins.reload();
    }

    /// Once a frame: the folder looked at while it is shown, and a run's requests answered and its end taken in.
    pub(crate) fn poll_pyaddins(&mut self, ctx: &Context) {
        let shown = self.tab == crate::Tab::PyAddins || self.docking.shown(Palette::Scripts);
        if shown && self.pyaddins.store.is_some() {
            let time = ctx.input(|i| i.time);
            self.pyaddins.look(time);
            ctx.request_repaint_after(LOOK);
        }
        let Some(mut running) = self.pyaddins.running.take() else { return };
        self.made_in();
        let session = &mut self.session;
        let events = running.run.poll(&mut |q| bridge::run(session, q), PER_FRAME, Duration::ZERO);
        self.active_option = self.session.option.clone();
        let mut select = None;
        if let Some(last) = &mut self.pyaddins.last {
            for e in events {
                match e {
                    Event::Print { text, err } => {
                        if last.log.len() >= bimcraft_pyaddins::run::MAX_LOG {
                            last.log.remove(0);
                        }
                        last.log.push((text, err));
                    }
                    Event::Done { message, select: s } => {
                        last.result = Some(Ok(message));
                        select = s;
                    }
                    Event::Failed(e) => last.result = Some(Err(e)),
                }
            }
        }
        if !running.run.finished() {
            self.pyaddins.running = Some(running);
            ctx.request_repaint_after(Duration::from_millis(30));
            return;
        }
        self.session.squash(&running.mark, "PyAddin");
        if let Some(ids) = select {
            self.selection = ids.into_iter().filter(|id| self.session.model.elements.contains_key(*id)).collect();
        }
        let script = running.script;
        self.status = match self.pyaddins.last.as_ref().and_then(|l| l.result.clone()) {
            Some(Ok(Some(message))) => Status { text: format!("{script}: {message}"), error: false },
            Some(Ok(None)) => Status { text: tf("{} ran.", &[&script]), error: false },
            Some(Err(e)) => Status { text: format!("{script}: {}", e.lines().last().unwrap_or_default()), error: true },
            None => Status { text: tf("{} was stopped.", &[&script]), error: false },
        };
    }

    /// Runs a script with `given` values (the rest take their defaults; a selection, what is selected).
    pub(crate) fn run_pyaddin(&mut self, ctx: &Context, script: &str, given: &Map<String, Value>) {
        if self.pyaddins.running.is_some() {
            self.status = Status { text: t("A script is running: stop it first.").to_string(), error: true };
            return;
        }
        let selection: Vec<u64> = self.selection.clone();
        let started = (|| {
            let store = self.pyaddins.store.clone().ok_or("no PyAddins folder")?;
            let python = self.pyaddins.python()?;
            let d = self.pyaddins.describe(script)?;
            let values = inputs::resolve(&d.inputs, given, &selection)?;
            let c = ctx.clone();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || c.request_repaint());
            let run = Run::start(&python, &store.client()?, &store.path(script)?, &values, &selection, Some(wake))?;
            Ok::<_, String>((run, values))
        })();
        match started {
            Ok((run, values)) => {
                self.pyaddins.last = Some(Last { script: script.to_string(), values, log: Vec::new(), result: None });
                self.pyaddins.running = Some(Running { run, script: script.to_string(), mark: self.session.mark() });
                self.status = Status { text: tf("Running {}…", &[script]), error: false };
            }
            Err(e) => {
                self.pyaddins.last =
                    Some(Last { script: script.to_string(), values: Map::new(), log: Vec::new(), result: Some(Err(e.clone())) });
                self.status = Status { text: format!("{script}: {e}"), error: true };
            }
        }
    }

    pub(crate) fn stop_pyaddin(&mut self) {
        if let Some(r) = &mut self.pyaddins.running {
            r.run.stop();
        }
    }

    /// Stops the script running, if one is, and runs the last one again with the same values.
    pub(crate) fn restart_pyaddin(&mut self, ctx: &Context) {
        let Some(last) = self.pyaddins.last.clone() else { return };
        if let Some(mut r) = self.pyaddins.running.take() {
            r.run.stop();
            self.session.squash(&r.mark, "PyAddin");
        }
        // The values it ran with, but a selection's as the selection is now.
        let mut values = last.values.clone();
        if let Ok(d) = self.pyaddins.describe(&last.script) {
            for i in d.inputs.iter().filter(|i| i.kind == Kind::Selection && i.default == Some(json!("selection"))) {
                values.remove(&i.name);
            }
        }
        self.run_pyaddin(ctx, &last.script, &values);
    }

    /// The PyAddins tab: the Player, the folder and Python; Stop and Restart; and the buttons made of scripts, by panel.
    pub(crate) fn pyaddins_tab(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        panel(ui, msg!("Scripts"), |ui| {
            let shown = self.docking.shown(Palette::Scripts);
            let tip = msg!("Player: your Python scripts, each with a form of what it asks for, to run on the model");
            if big_button(ui, Icon::Player, "Player", shown, tip).clicked() {
                self.show_palette(Palette::Scripts, !shown);
                self.pyaddins.reload();
            }
            stacked(ui, |ui| {
                let tip = msg!("Reload: read the scripts and buttons again, and look for Python again");
                if stacked_button(ui, Icon::Reload, "Reload", false, tip).clicked() {
                    self.pyaddins.python = None;
                    self.pyaddins.described.clear();
                    self.pyaddins.reload();
                    self.status = Status {
                        text: tf(
                            "{} scripts, {} buttons.",
                            &[&self.pyaddins.scripts.len().to_string(), &self.pyaddins.buttons.len().to_string()],
                        ),
                        error: false,
                    };
                }
                let tip = msg!("Python: the Python scripts run in; BIMCraft has none of its own (3.9 or newer)");
                if stacked_button(ui, Icon::Settings, "Python", self.pyaddins.python_dialog.is_some(), tip).clicked() {
                    let setting = |key: &str| self.pyaddins.store.as_ref().and_then(|s| s.setting(key)).unwrap_or_default();
                    self.pyaddins.python_dialog = Some(PythonDialog { python: setting("python"), editor: setting("editor"), skill: None });
                }
                let tip = msg!("New Script: a script to start from, in the Player");
                if stacked_button(ui, Icon::Script, "New Script", self.pyaddins.new_script.is_some(), tip).clicked() {
                    self.pyaddins.new_script = Some(self.pyaddins.free_name());
                }
            });
        });
        panel(ui, msg!("Run"), |ui| {
            stacked(ui, |ui| {
                let running = self.pyaddins.running.is_some();
                let r =
                    ui.add_enabled_ui(running, |ui| stacked_button(ui, Icon::Stop, "Stop", false, msg!("Stop the script running"))).inner;
                if r.clicked() {
                    self.stop_pyaddin();
                }
                let last = self.pyaddins.last.as_ref().map(|l| l.script.clone());
                let tip = match &last {
                    Some(s) => tf("Restart: run {} again, with the same values", &[s]),
                    None => t("Restart: run the last script again, with the same values").to_string(),
                };
                let r = ui.add_enabled_ui(last.is_some(), |ui| stacked_button(ui, Icon::Reload, "Restart", false, &tip)).inner;
                if r.clicked() {
                    self.restart_pyaddin(&ctx);
                }
            });
        });
        // The buttons, in panels in the order they first come.
        let mut panels: Vec<String> = Vec::new();
        for b in &self.pyaddins.buttons {
            if !panels.contains(&b.panel) {
                panels.push(b.panel.clone());
            }
        }
        for name in panels {
            let buttons: Vec<Button> = self.pyaddins.buttons.iter().filter(|b| b.panel == name).cloned().collect();
            panel(ui, &name, |ui| {
                for b in &buttons {
                    let running = self.pyaddins.running(&b.script);
                    let tip = if b.tip.is_empty() { tf("Runs {}", &[&b.script]) } else { b.tip.clone() };
                    let r = big_button(ui, Icon::Script, &b.label, running, &tip);
                    if r.clicked() {
                        if running {
                            self.stop_pyaddin();
                        } else {
                            self.run_pyaddin(&ctx, &b.script, &b.inputs);
                        }
                    }
                    r.context_menu(|ui| {
                        if ui.button(t("Show in Player")).clicked() {
                            self.docking.show(Palette::Scripts, true);
                            self.pyaddins.open = Some(b.script.clone());
                            ui.close();
                        }
                        if ui.button(t("Remove Button")).clicked() {
                            if let Some(store) = &self.pyaddins.store
                                && let Err(e) = store.remove_button(&b.label)
                            {
                                self.status = Status { text: e, error: true };
                            }
                            self.pyaddins.reload();
                            ui.close();
                        }
                    });
                }
            });
        }
    }

    /// The Script Automation palette: a search, and a row for each script that opens to its form and its Run.
    pub(crate) fn scripts_palette(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        Frame::new().inner_margin(8.0).show(ui, |ui| {
            if self.pyaddins.store.is_none() {
                ui.label(
                    RichText::new(t("There is no PyAddins folder: BIMCraft found no home folder to keep scripts in.")).color(pal().muted),
                );
                return;
            }
            ui.horizontal(|ui| {
                // Exactly what is left beside the button: a field asked for more (its margin comes on top of the width it is
                // given) would widen the docked palette a little every frame.
                let w = (ui.available_width() - 26.0 - ui.spacing().item_spacing.x).max(40.0);
                let edit = TextEdit::singleline(&mut self.pyaddins.search).hint_text(t("Search scripts…"));
                ui.add_sized([w, 22.0], edit);
                if crate::icon_button(ui, Icon::Script, msg!("New Script")).clicked() {
                    self.pyaddins.new_script = Some(self.pyaddins.free_name());
                }
            });
            ui.add_space(6.0);
            let search = self.pyaddins.search.to_lowercase();
            let names: Vec<String> = self.pyaddins.scripts.iter().filter(|n| n.to_lowercase().contains(&search)).cloned().collect();
            ScrollArea::vertical().id_salt("scripts").auto_shrink([false, false]).show(ui, |ui| {
                if names.is_empty() {
                    let empty = if self.pyaddins.scripts.is_empty() {
                        t("No scripts yet: New Script starts one, and an agent can write them (pyaddin_save).")
                    } else {
                        t("No script matches.")
                    };
                    ui.label(RichText::new(empty).color(pal().muted));
                }
                for name in names {
                    self.script_row(ui, &ctx, &name);
                    ui.add_space(4.0);
                }
            });
        });
    }

    /// One script's row: its name, play and chevron; open, its description, form, advanced settings and Run Script.
    fn script_row(&mut self, ui: &mut Ui, ctx: &Context, name: &str) {
        let open = self.pyaddins.open.as_deref() == Some(name);
        let running = self.pyaddins.running(name);
        let border = if open { pal().accent.gamma_multiply(0.6) } else { pal().border };
        Frame::new().stroke(Stroke::new(1.0, border)).corner_radius(4.0).inner_margin(6.0).fill(pal().surface).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut toggle = false;
            ui.horizontal(|ui| {
                toggle |= disclosure(ui, open, if open { msg!("Close") } else { msg!("Open") }).clicked();
                let title = match self.pyaddins.described.get(name) {
                    Some((_, _, Ok(d))) => d.title.clone().unwrap_or_else(|| name.to_string()),
                    _ => name.to_string(),
                };
                toggle |= ui.add(egui::Label::new(RichText::new(title).color(pal().text)).sense(Sense::click()).truncate()).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    toggle |= disclosure(ui, open, msg!("Settings")).clicked();
                    let label = if running { t("Stop") } else { t("Run") };
                    if play(ui, running, label).clicked() {
                        if running {
                            self.stop_pyaddin();
                        } else {
                            let given = self.form_values(name).unwrap_or_default();
                            self.run_pyaddin(ctx, name, &given);
                        }
                    }
                });
            });
            if toggle {
                self.pyaddins.open = if open { None } else { Some(name.to_string()) };
                self.pyaddins.draft = (name.to_string(), crate::pyaddins::DEFAULT_PANEL.to_string());
            }
            if open {
                self.script_form(ui, ctx, name);
            }
        });
    }

    /// The values of a script's form, as `main` takes them: what can't be read is said.
    fn form_values(&mut self, name: &str) -> Result<Map<String, Value>, String> {
        let d = self.pyaddins.describe(name)?;
        let units = self.session.model.units;
        let form = self.filled_form(name, &d);
        let mut out = Map::new();
        for i in &d.inputs {
            let Some(f) = form.get(&i.name) else { continue };
            let v = match (&i.kind, f) {
                (_, Field::Bool(b)) => json!(b),
                (Kind::Selection, _) => continue,
                (_, Field::Text(s)) if s.trim().is_empty() => continue,
                (Kind::Int | Kind::Float, Field::Text(s)) => {
                    json!(s.trim().parse::<f64>().map_err(|_| format!("`{}`: {} is no number", i.name, s.trim()))?)
                }
                (Kind::Length, Field::Text(s)) => {
                    json!(units.parse(s).ok_or_else(|| format!("`{}`: {} is no length", i.name, s.trim()))?)
                }
                (Kind::Choice(_), Field::Text(s)) => serde_json::from_str(s).unwrap_or(json!(s)),
                (_, Field::Text(s)) => json!(s),
            };
            out.insert(i.name.clone(), v);
        }
        Ok(out)
    }

    /// A script's form, each field it hasn't had yet as it starts.
    fn filled_form(&self, name: &str, d: &Description) -> HashMap<String, Field> {
        let mut form = self.pyaddins.forms.get(name).cloned().unwrap_or_default();
        let mut taken = Vec::new();
        for i in &d.inputs {
            if !form.contains_key(&i.name) {
                form.insert(i.name.clone(), self.first_field(i, &taken));
            }
            if let Some(Field::Text(t)) = form.get(&i.name) {
                taken.push(t.clone());
            }
        }
        form
    }

    /// A field's value as the form starts it: its default, in the project's units; a library name with none, the first of its
    /// kind that `taken` (the fields before it) hasn't, so two lists of types start on two types.
    fn first_field(&self, i: &bimcraft_pyaddins::Input, taken: &[String]) -> Field {
        let units = self.session.model.units;
        match (&i.kind, &i.default) {
            (Kind::Library(kind), None) => {
                let names: Vec<&str> = self
                    .session
                    .model
                    .library
                    .entries()
                    .filter(|(_, e)| e.kind.to_string() == *kind)
                    .map(|(_, e)| e.name.as_str())
                    .collect();
                let name = names.iter().find(|n| !taken.iter().any(|t| t == *n)).or(names.first());
                Field::Text(name.map(|n| n.to_string()).unwrap_or_default())
            }
            (Kind::Bool, d) => Field::Bool(d.as_ref().and_then(Value::as_bool).unwrap_or(false)),
            (Kind::Length, Some(d)) => Field::Text(d.as_f64().map(|mm| units.length(mm)).unwrap_or_default()),
            (Kind::Choice(_), Some(d)) => Field::Text(d.to_string()),
            (Kind::Level, None) => Field::Text(self.session.model.levels().first().map(|(_, l)| l.name.clone()).unwrap_or_default()),
            (_, Some(Value::String(s))) => Field::Text(s.clone()),
            (_, Some(d)) => Field::Text(d.to_string()),
            (_, None) => Field::Text(String::new()),
        }
    }

    fn script_form(&mut self, ui: &mut Ui, ctx: &Context, name: &str) {
        ui.add_space(4.0);
        let d = match self.pyaddins.describe(name) {
            Ok(d) => d,
            Err(e) => {
                ui.label(RichText::new(e).color(pal().error));
                self.advanced(ui, name);
                return;
            }
        };
        if !d.doc.is_empty() {
            ui.label(RichText::new(&d.doc).color(pal().muted));
            ui.add_space(4.0);
        }
        let mut form = self.filled_form(name, &d);
        let width = ui.available_width();
        let levels: Vec<String> = self.session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
        for i in &d.inputs {
            let Some(field) = form.get_mut(&i.name) else { continue };
            let label = spoken(&i.name);
            match (&i.kind, field) {
                (_, Field::Bool(b)) => {
                    let r = ui.checkbox(b, &label);
                    if !i.hint.is_empty() {
                        r.on_hover_text(&i.hint);
                    }
                }
                (Kind::Selection, _) => {
                    ui.label(RichText::new(&label).small().color(pal().muted));
                    let n = self.selection.len();
                    let text = if n == 0 { t("Nothing selected").to_string() } else { tf("{} selected", &[&n.to_string()]) };
                    ui.label(text);
                }
                (kind, Field::Text(s)) => {
                    let r = ui.label(RichText::new(&label).small().color(pal().muted));
                    // The field is named by its label, for screen readers (and tests).
                    let named = r.id;
                    if !i.hint.is_empty() {
                        r.on_hover_text(&i.hint);
                    }
                    let id = UiId::new(("pyaddin-field", name, &i.name));
                    let choices: Option<Vec<(String, String)>> = match kind {
                        Kind::Level => Some(levels.iter().map(|l| (l.clone(), l.clone())).collect()),
                        Kind::Choice(c) => {
                            Some(c.iter().map(|v| (v.to_string(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect())
                        }
                        Kind::Library(kind) => Some(
                            self.session
                                .model
                                .library
                                .entries()
                                .filter(|(_, e)| e.kind.to_string() == *kind)
                                .map(|(_, e)| (e.name.clone(), e.name.clone()))
                                .collect(),
                        ),
                        _ => None,
                    };
                    match choices {
                        Some(choices) => {
                            let shown = choices.iter().find(|(v, _)| v == s).map_or_else(|| s.clone(), |(_, l)| l.clone());
                            ComboBox::from_id_salt(id)
                                .selected_text(shown)
                                .width(width - 8.0)
                                .show_ui(ui, |ui| {
                                    for (v, l) in &choices {
                                        ui.selectable_value(s, v.clone(), l);
                                    }
                                })
                                .response
                                .labelled_by(named);
                        }
                        None => {
                            let hint = match kind {
                                Kind::Length => self.session.model.units.length(0.0),
                                _ => String::new(),
                            };
                            ui.add_sized([width - 8.0, 22.0], TextEdit::singleline(s).id(id).hint_text(hint)).labelled_by(named);
                        }
                    }
                }
            }
            ui.add_space(2.0);
        }
        self.pyaddins.forms.insert(name.to_string(), form);
        self.advanced(ui, name);
        ui.add_space(4.0);
        let running = self.pyaddins.running(name);
        let (label, color) = if running { (t("Stop"), pal().error) } else { (t("Run Script"), pal().accent_ink) };
        let run =
            egui::Button::new(RichText::new(format!("{} {label}", if running { "■" } else { "▶" })).color(color)).fill(pal().select_fill);
        if ui.add_sized([width - 8.0, 26.0], run).clicked() {
            if running {
                self.stop_pyaddin();
            } else {
                match self.form_values(name) {
                    Ok(given) => self.run_pyaddin(ctx, name, &given),
                    Err(e) => self.status = Status { text: e, error: true },
                }
            }
        }
        self.run_log(ui, name);
    }

    /// Advanced Settings: make a ribbon button of the script, open it in an editor, delete it.
    fn advanced(&mut self, ui: &mut Ui, name: &str) {
        let open = self.pyaddins.advanced;
        let r = ui.horizontal(|ui| {
            let a = disclosure(ui, open, msg!("Advanced Settings")).clicked();
            a | ui.add(egui::Label::new(RichText::new(t("Advanced Settings")).color(pal().accent_ink)).sense(Sense::click())).clicked()
        });
        if r.inner {
            self.pyaddins.advanced = !open;
        }
        if !open {
            return;
        }
        let Some(store) = self.pyaddins.store.clone() else { return };
        let width = ui.available_width() - 8.0;
        ui.label(RichText::new(t("Button label")).small().color(pal().muted));
        ui.add_sized([width, 22.0], TextEdit::singleline(&mut self.pyaddins.draft.0));
        ui.label(RichText::new(t("Ribbon panel")).small().color(pal().muted));
        ui.add_sized([width, 22.0], TextEdit::singleline(&mut self.pyaddins.draft.1));
        let tip = t("A button on the PyAddins tab that runs this script with the values in the form now");
        if ui.button(t("Make Button")).on_hover_text(tip).clicked() {
            let made = self.form_values(name).and_then(|inputs| {
                let (label, panel) = self.pyaddins.draft.clone();
                let b = Button::from_json(&json!({"label": label, "script": name, "panel": panel, "inputs": inputs}))?;
                let label = b.label.clone();
                store.set_button(b).map(|()| label)
            });
            self.status = match made {
                Ok(label) => Status { text: tf("{} is on the PyAddins tab.", &[&label]), error: false },
                Err(e) => Status { text: e, error: true },
            };
            self.pyaddins.reload();
        }
        ui.horizontal(|ui| {
            if ui.button(t("Edit")).on_hover_text(t("Open the script in your editor")).clicked()
                && let Err(e) = store.edit(name)
            {
                self.status = Status { text: e, error: true };
            }
            if ui.button(t("Delete Script")).clicked() {
                if let Err(e) = store.delete(name) {
                    self.status = Status { text: e, error: true };
                }
                self.pyaddins.reload();
            }
        });
        if let Ok(path) = store.path(name) {
            ui.label(RichText::new(path.display().to_string()).small().color(pal().muted));
        }
    }

    /// What the last run of the script printed, and how it ended.
    fn run_log(&mut self, ui: &mut Ui, name: &str) {
        let Some(last) = self.pyaddins.last.as_ref().filter(|l| l.script == name) else { return };
        ui.add_space(4.0);
        Frame::new().fill(pal().panel).corner_radius(3.0).inner_margin(4.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical().id_salt(("pyaddin-log", name)).max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                for (text, err) in &last.log {
                    let color = if *err { pal().error } else { pal().text };
                    ui.label(RichText::new(text).monospace().small().color(color));
                }
                match &last.result {
                    None => {
                        ui.label(RichText::new(t("Running…")).color(pal().muted));
                    }
                    Some(Ok(Some(m))) => {
                        ui.label(RichText::new(m).strong());
                    }
                    Some(Ok(None)) => {
                        ui.label(RichText::new(t("Done.")).color(pal().muted));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).monospace().small().color(pal().error));
                    }
                }
            });
        });
    }

    /// PyAddins > Python and New Script.
    pub(crate) fn pyaddins_windows(&mut self, ctx: &Context) {
        if let Some(mut d) = self.pyaddins.python_dialog.take() {
            let (mut close, mut save, mut find, mut install) = (false, false, false, false);
            let found = self.pyaddins.python();
            let home = bimcraft_pyaddins::skill::home();
            let installed = home.as_deref().is_some_and(bimcraft_pyaddins::skill::installed);
            let r = Modal::new(UiId::new("pyaddins-python"), t("Python")).width(560.0).show(ctx, |ui| {
                crate::popup::body(ui, |ui| {
                    ui.label(
                        RichText::new(t("Scripts run in your own Python, 3.9 or newer: BIMCraft has none of its own. Leave the path empty to use the one found."))
                            .color(pal().muted),
                    );
                    ui.add_space(6.0);
                    match &found {
                        Ok(p) => ui.label(p.label()),
                        Err(e) => ui.label(RichText::new(e).color(pal().error)),
                    };
                    ui.add_space(6.0);
                    let r = ui.label(t("Path to Python"));
                    ui.add(TextEdit::singleline(&mut d.python).hint_text("python3").desired_width(f32::INFINITY)).labelled_by(r.id);
                    ui.add_space(6.0);
                    let r = ui.label(t("Editor"));
                    let hint = t("the system's editor for Python, else for text");
                    ui.add(TextEdit::singleline(&mut d.editor).hint_text(hint).desired_width(f32::INFINITY)).labelled_by(r.id);
                    if let Some(store) = &self.pyaddins.store {
                        ui.add_space(6.0);
                        ui.label(RichText::new(tf("Scripts are kept in {}", &[&store.dir.display().to_string()])).small().color(pal().muted));
                    }
                    ui.add_space(10.0);
                    ui.separator();
                    ui.label(RichText::new(t("AI agents")).strong());
                    ui.label(
                        RichText::new(t("Claude Code and Codex write PyAddins for you with BIMCraft's skill: install it, then give the agent BIMCraft's tools."))
                            .color(pal().muted),
                    );
                    ui.add_space(4.0);
                    let label = if installed { t("Reinstall Skill") } else { t("Install Skill") };
                    install = ui.add_enabled(home.is_some(), egui::Button::new(label)).clicked();
                    match &d.skill {
                        Some(Ok(text)) => {
                            ui.label(RichText::new(text).small());
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).color(pal().error));
                        }
                        None if installed => {
                            ui.label(RichText::new(t("The skill is installed.")).small().color(pal().muted));
                        }
                        None => {}
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(t("Then, in a terminal, with the window started as bimcraft --control 0:")).small().color(pal().muted));
                    for c in bimcraft_pyaddins::skill::mcp_commands(&bimcraft_pyaddins::skill::cli_beside()) {
                        ui.add(egui::Label::new(RichText::new(c).monospace().small()).selectable(true));
                    }
                });
                crate::popup::footer(ui, |ui| {
                    ui.horizontal(|ui| {
                        save = ui.button(t("OK")).clicked();
                        find = ui.button(t("Find Again")).clicked();
                        close = ui.button(t("Cancel")).clicked();
                    });
                });
            });
            if install && let Some(home) = &home {
                d.skill = Some(bimcraft_pyaddins::skill::install(home).map(|written| {
                    written.iter().map(|(agent, file)| format!("{agent}: {}", file.display())).collect::<Vec<_>>().join("\n")
                }));
            }
            if save || find {
                if let Some(store) = &self.pyaddins.store
                    && let Err(e) = store.set_python(Some(&d.python)).and_then(|()| store.set_setting("editor", Some(&d.editor)))
                {
                    self.status = Status { text: e, error: true };
                }
                self.pyaddins.python = None;
                self.pyaddins.described.clear();
            }
            if !(save || close || r.should_close()) {
                self.pyaddins.python_dialog = Some(d);
            }
        }
        if let Some(mut name) = self.pyaddins.new_script.take() {
            let (mut close, mut make) = (false, false);
            let r = Modal::new(UiId::new("pyaddins-new"), t("New Script")).width(420.0).show(ctx, |ui| {
                crate::popup::body(ui, |ui| {
                    ui.label(t("Name"));
                    let r = ui.add(TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
                    r.request_focus();
                    make |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
                crate::popup::footer(ui, |ui| {
                    ui.horizontal(|ui| {
                        make |= ui.button(t("Create")).clicked();
                        close = ui.button(t("Cancel")).clicked();
                    });
                });
            });
            let mut keep = !(close || r.should_close());
            if make {
                let name = name.trim().to_string();
                let made = self.pyaddins.store.clone().ok_or_else(|| "no PyAddins folder".to_string()).and_then(|store| {
                    if store.path(&name)?.exists() {
                        return Err(tf("There is a script named {} already.", &[&name]));
                    }
                    store.save(&name, TEMPLATE)
                });
                match made {
                    Ok(_) => {
                        self.pyaddins.reload();
                        self.pyaddins.open = Some(name.clone());
                        self.docking.show(Palette::Scripts, true);
                        keep = false;
                    }
                    Err(e) => self.status = Status { text: e, error: true },
                }
            }
            if keep {
                self.pyaddins.new_script = Some(name);
            }
        }
    }
}

/// The panel a new button goes in.
pub(crate) const DEFAULT_PANEL: &str = bimcraft_pyaddins::store::PANEL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_name_reads_as_words() {
        assert_eq!(spoken("wall_type"), "Wall Type");
        assert_eq!(spoken("level"), "Level");
        assert_eq!(spoken("__x__y"), "X Y");
        assert_eq!(spoken(""), "");
    }
}
