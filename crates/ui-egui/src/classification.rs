//! Manage > Classification Manager: a classification system picked, its tables loaded from the user's files and its
//! parameters made, then codes given by hand to what's selected, or proposed by the AI assistant's model and reviewed before
//! they're written. It stays open beside the views, so the selection can change under it. The logic is the core's
//! (`bimcraft_core::classification`); the model is asked through `bimcraft_assistant`, with the provider, model and key of the
//! Assistant settings, on a thread of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bimcraft_assistant::net::Net;
use bimcraft_assistant::{Chat, Done, MAX_HISTORY, Msg, Provider, Usage, complete};
use bimcraft_core::classification::{self as cls, Brief, BriefAsk, Proposal, SystemDef, Table};
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Model, Scope};
use bimcraft_i18n::msg;
use egui::{
    Align, Button, Color32, ComboBox, Context, CursorIcon, Frame, Grid, Layout, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
    WidgetInfo, WidgetType, vec2,
};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::style::pal;

/// How many rows of the brief one request asks about: an answer of a few thousand tokens, well inside a reply's length.
const BATCH: usize = 25;

/// What a batch is asked with, before the brief: the assistant's model has its tools, and this task needs none.
const LEAD: &str = "This is a task from BIMCraft's Classification Manager, not a chat: use no tool, everything you need is below. \
Answer with the JSON object only.";

/// How long a code given by hand blinks.
const FLASH: f64 = 0.8;

/// How tall the list of codes and the review's table are.
const LIST_HEIGHT: f32 = 320.0;

/// `a` turned `k` of the way to `b`.
fn lerp_colour(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    // `b` over `a` by its own opacity.
    let a_ = f32::from(b.a()) / 255.0;
    let over = |x: u8, y: u8| (f32::from(x) * (1.0 - a_) + f32::from(y) * a_).round() as u8;
    let (br, bg, bb) = (over(a.r(), b.r()), over(a.g(), b.g()), over(a.b(), b.b()));
    Color32::from_rgb(mix(a.r(), br), mix(a.g(), bg), mix(a.b(), bb))
}

/// The window's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Setup,
    ByHand,
    Agent,
}

/// What the agent is asked to classify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rows {
    Unclassified,
    All,
    Selection,
}

/// A brief's row as the review shows it.
#[derive(Clone, Debug, Default)]
struct RowInfo {
    label: String,
    category: String,
    ids: Vec<Id>,
}

/// What the agent's thread has done so far.
#[derive(Default)]
struct Progress {
    done: usize,
    total: usize,
    log: Vec<String>,
    answers: Vec<Value>,
    error: Option<String>,
    finished: bool,
    /// When the batch being waited for was handed over.
    since: Option<std::time::Instant>,
    /// Tokens the provider counted, not yet added to the project's.
    usage: Usage,
}

/// An agent at work.
struct Run {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    rows: BTreeMap<String, RowInfo>,
}

/// One proposal under review.
#[derive(Clone, Debug)]
pub(crate) struct ReviewRow {
    pub(crate) proposal: Proposal,
    pub(crate) accept: bool,
    label: String,
    category: String,
    ids: Vec<Id>,
}

/// What the by-hand tab shows for the selection, kept until the model, the selection or the table changes.
struct Picked {
    model: Arc<Model>,
    selection: Vec<Id>,
    table: String,
    rows: Vec<cls::Row>,
    candidates: Vec<(String, String)>,
    error: Option<String>,
    coverage: Option<(usize, usize)>,
}

pub(crate) struct ClassificationManager {
    pub(crate) system: String,
    pub(crate) tab: Tab,
    edition: String,
    scopes: BTreeMap<String, Scope>,
    cobie_type: String,
    cobie_space: String,
    /// The table the by-hand tab assigns from.
    pub(crate) table: String,
    search: String,
    show_all: bool,
    picked: Option<Picked>,
    pub(crate) rows: Rows,
    pub(crate) tables_on: BTreeSet<String>,
    pub(crate) threshold: f64,
    run: Option<Run>,
    pub(crate) review: Vec<ReviewRow>,
    refused: Vec<String>,
    pub(crate) message: Option<(String, bool)>,
    /// The code just given by hand, and when: its row blinks blue to say so.
    flash: Option<(String, f64)>,
    /// An answer pasted from an agent run elsewhere (a chat window), to read back.
    pub(crate) paste: String,
    /// The tree of each table, its rows' depths: worked out once a table.
    depths: BTreeMap<String, (usize, Vec<usize>)>,
}

impl ClassificationManager {
    pub(crate) fn new(m: &Model) -> ClassificationManager {
        // The system of the first table loaded, or Uniclass.
        let system = m.classification.tables.values().next().map_or("uniclass".to_string(), |t| t.system.clone());
        let table = bound_tables(m).into_iter().next().unwrap_or_default();
        ClassificationManager {
            system,
            tab: if table.is_empty() { Tab::Setup } else { Tab::ByHand },
            edition: String::new(),
            scopes: BTreeMap::new(),
            cobie_type: String::new(),
            cobie_space: String::new(),
            table,
            search: String::new(),
            show_all: false,
            picked: None,
            rows: Rows::Unclassified,
            tables_on: BTreeSet::new(),
            threshold: 0.7,
            run: None,
            review: Vec::new(),
            refused: Vec::new(),
            message: None,
            paste: String::new(),
            flash: None,
            depths: BTreeMap::new(),
        }
    }

    /// Whether the model is being asked.
    pub(crate) fn busy(&self) -> bool {
        self.run.is_some()
    }

    fn system_def(&self) -> Option<&'static SystemDef> {
        cls::system(&self.system)
    }

    fn depth_of(&mut self, name: &str, table: &Table) -> Vec<usize> {
        if let Some((n, d)) = self.depths.get(name)
            && *n == table.items.len()
        {
            return d.clone();
        }
        let parents = table.parents();
        let depths: Vec<usize> = (0..table.items.len())
            .map(|mut i| {
                let mut d = 0;
                while let Some(Some(p)) = parents.get(i) {
                    d += 1;
                    i = *p;
                    if d > 16 {
                        break;
                    }
                }
                d
            })
            .collect();
        self.depths.insert(name.to_string(), (table.items.len(), depths.clone()));
        depths
    }
}

/// The tables both loaded and filling a parameter: what codes can be given from.
pub(crate) fn bound_tables(m: &Model) -> Vec<String> {
    let set: BTreeSet<String> = m
        .classification
        .bindings
        .iter()
        .filter(|b| m.classification.tables.contains_key(&b.table) && m.parameters.contains_key(&b.parameter))
        .map(|b| b.table.clone())
        .collect();
    set.into_iter().collect()
}

/// A category as a person reads it: `Wall`, `Structural column`.
fn category_label(c: &str) -> String {
    let word = match c {
        "structuralcolumn" => "structural column",
        "stripfooting" => "strip footing",
        "cabletray" => "cable tray",
        "downconductor" => "down conductor",
        "groundconductor" => "ground conductor",
        other => other,
    };
    let word = t(word);
    let mut cs = word.chars();
    cs.next().map_or_else(String::new, |f| f.to_uppercase().chain(cs).collect())
}

fn scope_word(s: Scope) -> &'static str {
    match s {
        Scope::Instance => msg!("Each element"),
        Scope::Type => msg!("Each type"),
    }
}

impl BimApp {
    /// Runs a command for the window: an error is said in it, not behind it.
    fn classifying(&mut self, d: &mut ClassificationManager, id: &str, p: Value) -> Option<Value> {
        let r = self.run(id, p);
        if r.is_none() {
            d.message = Some((self.status.text.clone(), true));
        }
        r
    }

    /// A table's file read into the project, as the system's table `key`.
    pub(crate) fn classification_load_table(&mut self, path: &str, key: &str) {
        let Some(mut d) = self.classification.take() else { return };
        let mut p = json!({"path": path, "system": d.system, "table": key});
        if !d.edition.trim().is_empty() {
            p["edition"] = json!(d.edition.trim());
        }
        if let Some(r) = self.classifying(&mut d, "classification_table", p) {
            let left = r["left_out"].as_array().map_or(0, Vec::len);
            let text = tf("{} codes loaded into {}.", &[&r["codes"].to_string(), r["table"].as_str().unwrap_or_default()]);
            let text = if left > 0 { format!("{text} {}", tf("{} rows left out.", &[&left.to_string()])) } else { text };
            d.message = Some((text, false));
        }
        self.classification = Some(d);
    }

    /// The picked system's parameters made and bound.
    fn classification_load_parameters(&mut self, d: &mut ClassificationManager) {
        let Some(sys) = d.system_def() else { return };
        let mut p = json!({"system": sys.id});
        let scopes: BTreeMap<&String, &Scope> = d.scopes.iter().collect();
        if !scopes.is_empty() {
            p["scopes"] = json!(scopes);
        }
        if !sys.fields.is_empty() {
            let mut cobie = serde_json::Map::new();
            if !d.cobie_type.is_empty() {
                cobie.insert("Type".into(), json!(d.cobie_type));
            }
            if !d.cobie_space.is_empty() {
                cobie.insert("Space".into(), json!(d.cobie_space));
            }
            p["cobie"] = Value::Object(cobie);
        }
        if let Some(r) = self.classifying(d, "classification_parameters", p) {
            let n = r["parameters"].as_array().map_or(0, Vec::len);
            let notes: Vec<&str> = r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let mut text = tf("{} parameters are in the project.", &[&n.to_string()]);
            if !notes.is_empty() {
                text = format!("{text} {}", notes.join(" "));
            }
            d.message = Some((text, false));
            if d.table.is_empty() {
                d.table = bound_tables(&self.session.model).into_iter().next().unwrap_or_default();
            }
        }
    }

    /// Manage > Classification Manager (CI), opened; one already open stays as it is.
    pub(crate) fn open_classification(&mut self) {
        if self.classification.is_none() {
            self.classification = Some(ClassificationManager::new(&self.session.model));
        }
    }

    /// Manage > Classification Manager: a window beside the views.
    pub(crate) fn classification_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.classification.take() else { return };
        self.agent_tick(&mut d);
        let mut open = true;
        egui::Window::new(t("Classification Manager"))
            .id(egui::Id::new("classification-manager"))
            .open(&mut open)
            .resizable(true)
            .default_size(vec2(760.0, 560.0))
            .min_size(vec2(420.0, 300.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [(Tab::Setup, msg!("System")), (Tab::ByHand, msg!("By Hand")), (Tab::Agent, msg!("AI Agent"))] {
                        if ui.selectable_label(d.tab == tab, t(label)).clicked() {
                            d.tab = tab;
                            d.message = None;
                        }
                    }
                });
                ui.separator();
                match d.tab {
                    Tab::Setup => self.setup_tab(ui, ctx, &mut d),
                    Tab::ByHand => self.by_hand_tab(ui, &mut d),
                    Tab::Agent => self.agent_tab(ui, &mut d),
                }
                if let Some((text, error)) = &d.message {
                    ui.add_space(4.0);
                    ui.label(if *error { RichText::new(text).color(pal().error) } else { RichText::new(text) });
                }
            });
        if open {
            self.classification = Some(d);
        } else if let Some(r) = d.run {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn setup_tab(&mut self, ui: &mut Ui, ctx: &Context, d: &mut ClassificationManager) {
        let model = Arc::clone(&self.session.model);
        ui.horizontal(|ui| {
            ui.label(t("Classification system"));
            let shown = d.system_def().map_or(d.system.clone(), |s| s.name.to_string());
            ComboBox::from_id_salt("classification-system").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                for s in cls::SYSTEMS {
                    if ui.selectable_label(d.system == s.id, s.name).clicked() {
                        d.system = s.id.to_string();
                        d.scopes.clear();
                    }
                }
            });
            ui.label(t("Edition"));
            ui.add(TextEdit::singleline(&mut d.edition).desired_width(100.0).hint_text("2024-07"));
        });
        let Some(sys) = d.system_def() else { return };
        ui.label(RichText::new(format!("{} · {}", sys.publisher, sys.region)).color(pal().muted));
        ui.label(RichText::new(sys.note).color(pal().muted).small());
        ui.add_space(6.0);
        if !sys.tables.is_empty() {
            ui.label(RichText::new(t("1. Load the tables you hold, from their .csv or .xlsx. BIMCraft ships none.")).strong());
            Grid::new("classification-tables").striped(true).num_columns(5).show(ui, |ui| {
                for h in [msg!("Table"), msg!("Whose code"), msg!("Loaded"), "", ""] {
                    ui.label(RichText::new(t(h)).strong());
                }
                ui.end_row();
                for tdef in sys.tables {
                    let name = sys.table_name(tdef);
                    ui.label(format!("{name} — {}", tdef.name)).on_hover_text(t(tdef.why));
                    let mut scope = d.scopes.get(tdef.key).copied().unwrap_or(tdef.scope);
                    ComboBox::from_id_salt(("classification-scope", tdef.key)).selected_text(t(scope_word(scope))).show_ui(ui, |ui| {
                        for s in [Scope::Type, Scope::Instance] {
                            ui.selectable_value(&mut scope, s, t(scope_word(s)));
                        }
                    });
                    if scope != tdef.scope {
                        d.scopes.insert(tdef.key.to_string(), scope);
                    } else {
                        d.scopes.remove(tdef.key);
                    }
                    match model.classification.tables.get(&name) {
                        Some(tb) => ui.label(tf(
                            "{} codes, {}",
                            &[&tb.items.len().to_string(), if tb.edition.is_empty() { &tb.source } else { &tb.edition }],
                        )),
                        None => ui.label(RichText::new(t("not loaded")).color(pal().muted)),
                    };
                    if ui.button(t("Load Table…")).clicked() {
                        let key = tdef.key.to_string();
                        let dialog = rfd::AsyncFileDialog::new().add_filter(t("Tables"), &["csv", "xlsx", "txt"]);
                        self.ask_file(ctx, dialog, false, move |app, path| app.classification_load_table(&path.to_string_lossy(), &key));
                    }
                    if model.classification.tables.contains_key(&name) && ui.button(t("Remove")).clicked() {
                        self.classifying(d, "classification_table", json!({"name": name, "remove": true}));
                    }
                    ui.end_row();
                }
            });
        } else {
            ui.label(
                RichText::new(t("COBie's Category fields take another system's codes: pick the tables, loaded under that system."))
                    .strong(),
            );
            let loaded: Vec<String> = model.classification.tables.keys().cloned().collect();
            for (label, slot) in [(msg!("Type Category from"), &mut d.cobie_type), (msg!("Space Category from"), &mut d.cobie_space)] {
                ui.horizontal(|ui| {
                    ui.label(t(label));
                    ComboBox::from_id_salt(("cobie-table", label))
                        .selected_text(if slot.is_empty() { t("none").to_string() } else { slot.clone() })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(slot, String::new(), t("none"));
                            for n in &loaded {
                                ui.selectable_value(slot, n.clone(), n);
                            }
                        });
                });
            }
            let sheets: [(&str, Scope, usize); 3] = ["Type", "Component", "Space"].map(|s| {
                (
                    s,
                    sys.fields.iter().find(|f| f.sheet == s).map_or(Scope::Instance, |f| f.scope),
                    sys.fields.iter().filter(|f| f.sheet == s).count(),
                )
            });
            for (sheet, scope, n) in sheets {
                ui.label(RichText::new(tf("{}: {} fields, {}", &[sheet, &n.to_string(), t(scope_word(scope))])).color(pal().muted));
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new(t("2. Make its parameters in the project.")).strong());
        ui.horizontal(|ui| {
            if ui.button(t("Load Parameters")).clicked() {
                self.classification_load_parameters(d);
            }
            let made: Vec<String> = sys
                .tables
                .iter()
                .map(|tdef| sys.parameters_of(tdef).0)
                .chain(sys.fields.iter().map(|f| f.parameter()))
                .filter(|n| model.parameters.contains_key(n))
                .collect();
            let all = sys.tables.len() + sys.fields.len();
            ui.label(RichText::new(tf("{} of {} in the project", &[&made.len().to_string(), &all.to_string()])).color(pal().muted));
        });
        ui.add_space(8.0);
        ui.label(RichText::new(t("3. Classify: by hand, or with your AI agent.")).strong());
    }

    /// What the by-hand tab shows for the selection: worked out again only when something changed.
    fn picked(&self, d: &mut ClassificationManager) {
        let fresh = d
            .picked
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(&p.model, &self.session.model) && p.selection == self.selection && p.table == d.table);
        if fresh {
            return;
        }
        let m = &self.session.model;
        let ask = BriefAsk {
            tables: vec![d.table.clone()],
            rows: Some("all".into()),
            elements: self.selection.iter().map(|id| id.to_string()).collect(),
            candidates: Some(120),
        };
        let (rows, candidates, error) = match (self.selection.is_empty(), cls::brief(m, &ask)) {
            (true, _) => (Vec::new(), Vec::new(), None),
            (false, Ok(b)) => {
                let mut c: Vec<(String, String)> = Vec::new();
                for list in b.candidates.get(&d.table).map(|by| by.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
                    for x in list {
                        if !c.contains(&x) {
                            c.push(x);
                        }
                    }
                }
                (b.rows, c, None)
            }
            (false, Err(e)) => (Vec::new(), Vec::new(), Some(e)),
        };
        let coverage = cls::coverage(m, &d.table);
        d.picked = Some(Picked {
            model: Arc::clone(m),
            selection: self.selection.clone(),
            table: d.table.clone(),
            rows,
            candidates,
            error,
            coverage,
        });
    }

    fn by_hand_tab(&mut self, ui: &mut Ui, d: &mut ClassificationManager) {
        let model = Arc::clone(&self.session.model);
        let tables = bound_tables(&model);
        if tables.is_empty() {
            ui.label(t("Load a system's table and its parameters first, on the System tab."));
            return;
        }
        if !tables.contains(&d.table) {
            d.table = tables[0].clone();
        }
        ui.horizontal(|ui| {
            ui.label(t("Table"));
            ComboBox::from_id_salt("classification-hand-table").selected_text(&d.table).show_ui(ui, |ui| {
                for n in &tables {
                    ui.selectable_value(&mut d.table, n.clone(), n);
                }
            });
            if ui
                .button(t("Next Unclassified"))
                .on_hover_text(t("Select the elements of the next type (or element) with no code"))
                .clicked()
            {
                let ask = BriefAsk { tables: vec![d.table.clone()], ..BriefAsk::default() };
                match cls::brief(&model, &ask) {
                    Ok(b) => match b.rows.first() {
                        Some(r) => self.selection = r.ids.clone(),
                        None => d.message = Some((t("Everything this table is for has a code.").to_string(), false)),
                    },
                    Err(e) => d.message = Some((e, true)),
                }
            }
            if let Some((done, of)) = d.picked.as_ref().filter(|p| p.table == d.table).and_then(|p| p.coverage) {
                ui.label(RichText::new(tf("{} of {} classified", &[&done.to_string(), &of.to_string()])).color(pal().muted));
            }
        });
        self.picked(d);
        let Some(picked) = d.picked.as_ref() else { return };
        if let Some(e) = &picked.error {
            ui.label(RichText::new(e).color(pal().error));
        }
        if picked.rows.is_empty() {
            ui.label(RichText::new(t("Select what to classify: its category's codes come up here.")).color(pal().muted));
            return;
        }
        let b = model.classification.bindings.iter().find(|b| b.table == d.table);
        ui.add_space(4.0);
        for r in picked.rows.iter().take(6) {
            let now = b.and_then(|b| cls::current(&model, b, &cls::Target::Element(r.ids.first().copied().unwrap_or_default())));
            let what = if r.type_name.is_empty() { r.name.clone() } else { r.type_name.clone() };
            ui.label(tf(
                "{} {} ({} elements): {}",
                &[&category_label(&r.category), &what, &r.count.to_string(), now.unwrap_or(t("no code"))],
            ));
        }
        if picked.rows.len() > 6 {
            ui.label(RichText::new(tf("and {} more", &[&(picked.rows.len() - 6).to_string()])).color(pal().muted));
        }
        ui.horizontal(|ui| {
            ui.label(t("Search"));
            ui.add(TextEdit::singleline(&mut d.search).desired_width(240.0).hint_text(t("code or title")));
            ui.checkbox(&mut d.show_all, t("Whole table"));
        });
        let Some(table) = model.classification.tables.get(&d.table) else { return };
        let depths = d.depth_of(&d.table.clone(), table);
        let needle = d.search.trim().to_lowercase();
        let list: Vec<usize> = if !needle.is_empty() {
            table
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.code.to_lowercase().contains(&needle) || i.title.to_lowercase().contains(&needle))
                .map(|(n, _)| n)
                .collect()
        } else if d.show_all {
            (0..table.items.len()).collect()
        } else {
            let picked = d.picked.as_ref().map(|p| p.candidates.clone()).unwrap_or_default();
            picked.iter().filter_map(|(code, _)| table.items.iter().position(|i| &i.code == code)).collect()
        };
        let mut chosen: Option<String> = None;
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
        let dark = ui.visuals().dark_mode;
        // A list of things to click: on white (the panel's surface when dark), grey under the pointer, blue a moment after a click.
        let ground = if dark { pal().surface } else { Color32::WHITE };
        let hover = if dark { pal().hover_fill } else { Color32::from_gray(232) };
        let now = ui.input(|i| i.time);
        let flash = d.flash.as_ref().filter(|(_, at)| now - at < FLASH).map(|(code, at)| (code.clone(), now - at));
        if flash.is_some() {
            ui.ctx().request_repaint();
        }
        // A height of its own, not what the window has left: that grows with the message under it, and the window with it.
        let height = LIST_HEIGHT;
        Frame::new().fill(ground).stroke(Stroke::new(1.0, pal().border)).corner_radius(4.0).inner_margin(2.0).show(ui, |ui| {
            ScrollArea::vertical().id_salt("classification-codes").auto_shrink([false, false]).max_height(height).show_rows(
                ui,
                row_h,
                list.len(),
                |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for n in &list[range] {
                        let item = &table.items[*n];
                        let indent = if needle.is_empty() && d.show_all { depths.get(*n).copied().unwrap_or(0) as f32 * 14.0 } else { 0.0 };
                        let label = format!("{}  {}", item.code, item.title);
                        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
                        let r = r.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(t("Give the selection this code"));
                        let fill = match &flash {
                            // Blue at once, fading back to what's under it.
                            Some((code, age)) if *code == item.code => {
                                let k = (1.0 - age / FLASH) as f32;
                                let under = if r.hovered() { hover } else { ground };
                                lerp_colour(under, pal().select_fill, k)
                            }
                            _ if r.hovered() => hover,
                            _ => ground,
                        };
                        ui.painter().rect_filled(rect, 3.0, fill);
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let at = rect.left_center() + vec2(6.0 + indent, 0.0);
                        let galley = ui.painter().layout_no_wrap(label.clone(), font, pal().text);
                        ui.painter().with_clip_rect(rect).galley(at - vec2(0.0, galley.size().y / 2.0), galley, pal().text);
                        if r.clicked() {
                            chosen = Some(item.code.clone());
                        }
                    }
                },
            );
        });
        if let Some(code) = chosen {
            d.flash = Some((code.clone(), ui.input(|i| i.time)));
            let assign: Vec<Value> =
                self.selection.iter().map(|id| json!({"target": id.to_string(), "table": d.table, "code": code})).collect();
            if let Some(r) = self.classifying(d, "classify", json!({"assign": assign})) {
                let n = r["rows"].as_array().map_or(0, Vec::len);
                d.message = Some((tf("{} given {}.", &[&n.to_string(), &code]), false));
            }
        }
    }

    fn agent_tab(&mut self, ui: &mut Ui, d: &mut ClassificationManager) {
        let model = Arc::clone(&self.session.model);
        let tables = bound_tables(&model);
        if tables.is_empty() {
            ui.label(t("Load a system's table and its parameters first, on the System tab."));
            return;
        }
        // The assistant's provider and model, and where they're set: the AI chat's settings, its key included.
        ui.horizontal(|ui| {
            let a = &self.assistant;
            ui.label(RichText::new(format!("{} · {}", a.provider.name(), a.model.trim())).color(pal().muted));
            if ui.button(t("Assistant settings")).clicked() {
                self.settings_page = Some(crate::settings::Page::Assistant);
            }
        });
        let unready = self.assistant.unready();
        let agreed = self.assistant.classify_agreed.contains(self.assistant.provider.key());
        if !d.busy() && d.review.is_empty() {
            ui.horizontal(|ui| {
                ui.label(t("Classify"));
                ui.radio_value(&mut d.rows, Rows::Unclassified, t("what has no code"));
                ui.radio_value(&mut d.rows, Rows::All, t("everything"));
                ui.radio_value(&mut d.rows, Rows::Selection, t("the selection"));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(t("by"));
                for n in &tables {
                    let mut on = d.tables_on.is_empty() || d.tables_on.contains(n);
                    if ui.checkbox(&mut on, n).changed() {
                        if d.tables_on.is_empty() {
                            d.tables_on = tables.iter().cloned().collect();
                        }
                        if on {
                            d.tables_on.insert(n.clone());
                        } else {
                            d.tables_on.remove(n);
                        }
                    }
                }
            });
            if let Some(why) = &unready {
                ui.label(RichText::new(why).color(pal().muted));
            } else if !agreed {
                // Said before the first request of the session to this provider, as the AI chat says it before its first message.
                ui.label(
                    RichText::new(tf(
                        "Asking the agent sends what it classifies to {}, on your API key: each type's category, family, type name, materials, levels, hosts and sizes, and the tables' candidate codes. Nothing is sent until you ask, and nothing is written until you apply what you tick.",
                        &[self.assistant.provider.name()],
                    ))
                    .color(pal().muted),
                );
            }
            ui.horizontal(|ui| {
                // With no key (or no network) the button isn't there: the line above says why, and Assistant settings is the way.
                let label = if agreed { t("Ask the Agent") } else { t("Agree and Ask the Agent") };
                if unready.is_none() && ui.add(Button::new(label)).clicked() {
                    self.agent_start(ui.ctx(), d);
                }
                if ui
                    .button(t("Copy Brief"))
                    .on_hover_text(t("For an agent with no command line (a chat window): copy the instructions and the brief, then paste its answer below"))
                    .clicked()
                {
                    self.classification_copy_brief(ui.ctx(), d);
                }
            });
            ui.add_space(6.0);
            ui.label(t("Or paste an answer:"));
            ui.add(TextEdit::multiline(&mut d.paste).desired_rows(3).desired_width(f32::INFINITY).hint_text("{\"rows\": [...]}"));
            if ui.add_enabled(!d.paste.trim().is_empty(), Button::new(t("Read Answer"))).clicked() {
                let text = std::mem::take(&mut d.paste);
                match cls::json_in(&text) {
                    Some(v) => {
                        self.classification = Some(std::mem::replace(d, ClassificationManager::new(&self.session.model)));
                        self.classification_answer(&v);
                        if let Some(back) = self.classification.take() {
                            *d = back;
                        }
                    }
                    None => d.message = Some((t("That holds no JSON answer.").to_string(), true)),
                }
            }
            return;
        }
        if let Some(run) = &d.run {
            let (done, total, log, error, since) = match run.progress.lock() {
                Ok(p) => (p.done, p.total, p.log.clone(), p.error.clone(), p.since),
                Err(_) => (0, 0, Vec::new(), None, None),
            };
            let batch = (done + 1).min(total).to_string();
            // The seconds the agent has had the batch: something is happening, even when it says nothing for a minute.
            let text = match since {
                Some(at) => {
                    tf("Batch {} of {} · waiting for the answer… {} s", &[&batch, &total.to_string(), &at.elapsed().as_secs().to_string()])
                }
                None => tf("Batch {} of {} · handing it over…", &[&batch, &total.to_string()]),
            };
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add(egui::ProgressBar::new(if total == 0 { 0.0 } else { done as f32 / total as f32 }).desired_width(380.0).text(text));
                if ui.button(t("Cancel")).clicked() {
                    run.cancel.store(true, Ordering::Relaxed);
                }
            });
            if let Some(e) = error {
                ui.label(RichText::new(e).color(pal().error));
            }
            for l in log.iter().rev().take(4) {
                ui.label(RichText::new(l).color(pal().muted).small());
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        self.classification_review(ui, d);
    }

    fn classification_review(&mut self, ui: &mut Ui, d: &mut ClassificationManager) {
        let mut apply = false;
        ui.horizontal(|ui| {
            ui.label(tf("{} proposals", &[&d.review.len().to_string()]));
            ui.label(t("Accept from confidence"));
            ui.add(egui::Slider::new(&mut d.threshold, 0.0..=1.0).fixed_decimals(2));
            if ui.button(t("Tick by Confidence")).clicked() {
                for r in &mut d.review {
                    r.accept = r.proposal.code.is_some() && r.proposal.confidence >= d.threshold && r.proposal.code != r.proposal.current;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = d.review.iter().filter(|r| r.accept).count();
                apply = ui.add_enabled(n > 0, Button::new(tf("Apply {}", &[&n.to_string()]))).clicked();
                if ui.button(t("Discard")).clicked() {
                    d.review.clear();
                    d.refused.clear();
                }
            });
        });
        let mut select: Option<Vec<Id>> = None;
        ScrollArea::both().id_salt("classification-review").auto_shrink([false, false]).max_height(LIST_HEIGHT).show(ui, |ui| {
            Grid::new("classification-review-grid").striped(true).num_columns(8).show(ui, |ui| {
                // Above the Apply column: every proposal with a code ticked at once.
                if ui.add(Button::new(t("Select All")).small()).on_hover_text(t("Tick every proposal that has a code")).clicked() {
                    for r in &mut d.review {
                        r.accept = r.proposal.code.is_some();
                    }
                }
                ui.end_row();
                for h in [
                    msg!("Apply"),
                    msg!("Type or element"),
                    msg!("Table"),
                    msg!("Now"),
                    msg!("Proposed"),
                    msg!("Title"),
                    msg!("Confidence"),
                    msg!("Why"),
                ] {
                    ui.label(RichText::new(t(h)).strong());
                }
                ui.end_row();
                for r in &mut d.review {
                    ui.add_enabled(r.proposal.code.is_some(), egui::Checkbox::without_text(&mut r.accept));
                    if ui.link(format!("{} {}", category_label(&r.category), r.label)).on_hover_text(t("Select its elements")).clicked() {
                        select = Some(r.ids.clone());
                    }
                    ui.label(&r.proposal.table);
                    ui.label(r.proposal.current.as_deref().unwrap_or("—"));
                    ui.label(RichText::new(r.proposal.code.as_deref().unwrap_or("—")).strong());
                    ui.label(r.proposal.title.as_deref().unwrap_or_default());
                    let c = r.proposal.confidence;
                    let ink = if c >= 0.8 {
                        pal().text
                    } else if c >= 0.5 {
                        pal().muted
                    } else {
                        pal().error
                    };
                    ui.label(RichText::new(format!("{:.0}%", c * 100.0)).color(ink));
                    ui.label(RichText::new(&r.proposal.reason).small());
                    ui.end_row();
                }
            });
            if !d.refused.is_empty() {
                ui.collapsing(tf("{} answers left out", &[&d.refused.len().to_string()]), |ui| {
                    for x in &d.refused {
                        ui.label(RichText::new(x).small());
                    }
                });
            }
        });
        if let Some(ids) = select {
            self.selection = ids;
        }
        if apply {
            self.classification_apply_review(d);
        }
    }

    /// The ticked proposals written, as one step.
    pub(crate) fn classification_apply_review(&mut self, d: &mut ClassificationManager) {
        let assign: Vec<Value> = d
            .review
            .iter()
            .filter(|r| r.accept)
            .filter_map(|r| Some(json!({"target": r.proposal.key, "table": r.proposal.table, "code": r.proposal.code.as_ref()?})))
            .collect();
        if let Some(r) = self.classifying(d, "classify", json!({"assign": assign})) {
            let n = r["rows"].as_array().map_or(0, Vec::len);
            d.message = Some((tf("{} codes written; Undo takes them back.", &[&n.to_string()]), false));
            d.review.clear();
            d.refused.clear();
        }
    }

    /// The instructions and the whole brief on the clipboard, for an agent BIMCraft can't run.
    fn classification_copy_brief(&mut self, ctx: &Context, d: &mut ClassificationManager) {
        let ask = BriefAsk {
            tables: d.tables_on.iter().cloned().collect(),
            rows: Some(if d.rows == Rows::All { "all" } else { "unclassified" }.into()),
            elements: if d.rows == Rows::Selection { self.selection.iter().map(|i| i.to_string()).collect() } else { Vec::new() },
            candidates: Some(40),
        };
        match cls::brief(&self.session.model, &ask) {
            Ok(b) => {
                let h = cls::handover(&self.session.model, &b, false);
                ctx.copy_text(format!("{}\n\n## Brief\n\n{}\n", h.instructions, h.inline));
                d.message =
                    Some((tf("{} rows copied: paste them into your agent, and its answer here.", &[&b.rows.len().to_string()]), false));
            }
            Err(e) => d.message = Some((e, true)),
        }
    }

    /// The brief made, cut into batches, and asked of the assistant's model on a thread of its own, on the user's key.
    fn agent_start(&mut self, ctx: &Context, d: &mut ClassificationManager) {
        if let Some(why) = self.assistant.unready() {
            d.message = Some((why, true));
            return;
        }
        let Some(net) = self.assistant.chat.as_ref().map(Chat::net) else { return };
        let (provider, model_id, key) = (self.assistant.provider, self.assistant.model.trim().to_string(), self.assistant.key());
        let model = Arc::clone(&self.session.model);
        let ask = BriefAsk {
            tables: if d.tables_on.is_empty() { Vec::new() } else { d.tables_on.iter().cloned().collect() },
            rows: Some(if d.rows == Rows::All { "all" } else { "unclassified" }.into()),
            elements: if d.rows == Rows::Selection { self.selection.iter().map(|i| i.to_string()).collect() } else { Vec::new() },
            candidates: Some(30),
        };
        if d.rows == Rows::Selection && self.selection.is_empty() {
            d.message = Some((t("Nothing is selected.").to_string(), true));
            return;
        }
        let brief = match cls::brief(&model, &ask) {
            Ok(b) => b,
            Err(e) => {
                d.message = Some((e, true));
                return;
            }
        };
        if brief.rows.is_empty() {
            d.message = Some((t("Nothing to classify: everything asked for has a code.").to_string(), false));
            return;
        }
        let rows: BTreeMap<String, RowInfo> = brief
            .rows
            .iter()
            .map(|r| {
                let label = match (r.family.is_empty(), r.type_name.is_empty()) {
                    (_, false) if !r.family.is_empty() => format!("{}: {}", r.family, r.type_name),
                    (_, false) => r.type_name.clone(),
                    _ => r.name.clone(),
                };
                (r.key.clone(), RowInfo { label, category: r.category.clone(), ids: r.ids.clone() })
            })
            .collect();
        let prompts: Vec<String> = brief.chunks(BATCH).iter().map(|b| prompt(&model, b)).collect();
        let progress = Arc::new(Mutex::new(Progress { total: prompts.len(), ..Progress::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p2, c2, ctx2) = (Arc::clone(&progress), Arc::clone(&cancel), ctx.clone());
        let spawned = std::thread::Builder::new().name("classify".into()).spawn(move || {
            let note = |f: &dyn Fn(&mut Progress)| {
                if let Ok(mut p) = p2.lock() {
                    f(&mut p);
                }
                ctx2.request_repaint();
            };
            for (n, text) in prompts.iter().enumerate() {
                if c2.load(Ordering::Relaxed) {
                    break;
                }
                let at = std::time::Instant::now();
                note(&|p| p.since = Some(at));
                let mut used = Usage::default();
                let result = ask_model(provider, net.as_ref(), &model_id, &key, text, &mut used);
                note(&|p| p.usage.add(used));
                match result {
                    Ok(answer) => match cls::json_in(&answer) {
                        Some(v) => note(&|p| {
                            p.answers.push(v.clone());
                            p.log.push(format!("batch {} answered in {} s", n + 1, at.elapsed().as_secs()));
                        }),
                        None => note(&|p| p.log.push(format!("batch {}: the answer held no JSON", n + 1))),
                    },
                    Err(e) => {
                        note(&|p| p.error = Some(e.clone()));
                        break;
                    }
                }
                note(&|p| p.done = n + 1);
            }
            note(&|p| p.finished = true);
        });
        if let Err(e) = spawned {
            d.message = Some((format!("could not start the request: {e}"), true));
            return;
        }
        self.assistant.classify_agreed.insert(provider.key());
        d.message = None;
        d.refused.clear();
        d.run = Some(Run { progress, cancel, rows });
    }

    /// Once a frame: the tokens counted go to the project's, as the AI chat's do, and a run that's done or cancelled has the
    /// answers it got read against the project for review. A request still out when Cancel was pressed is given up on: its
    /// answer goes nowhere.
    fn agent_tick(&mut self, d: &mut ClassificationManager) {
        let Some(run) = &d.run else { return };
        let cancelled = run.cancel.load(Ordering::Relaxed);
        let (finished, answers, error, used) = match run.progress.lock() {
            Ok(mut p) => (p.finished, p.answers.clone(), p.error.clone(), std::mem::take(&mut p.usage)),
            Err(_) => (true, Vec::new(), Some("the agent's thread failed".into()), Usage::default()),
        };
        if used != Usage::default() {
            let project = self.session.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            self.assistant.add_usage(project, used);
        }
        if !finished && !cancelled {
            return;
        }
        let rows = run.rows.clone();
        d.run = None;
        for a in &answers {
            self.classification_take_answer(d, a, &rows);
        }
        if cancelled {
            d.message = Some((t("Cancelled.").to_string(), false));
        } else if let Some(e) = error {
            d.message = Some((e, true));
        } else if d.review.is_empty() && !d.refused.is_empty() {
            // Nothing to review: what was wrong with the answers is said here, or the window would show nothing.
            let n = tf("{} answers left out", &[&d.refused.len().to_string()]);
            d.message = Some((format!("{n}: {}", d.refused.iter().take(3).cloned().collect::<Vec<_>>().join("; ")), true));
        } else if answers.is_empty() {
            d.message = Some((t("The agent gave no answer.").to_string(), true));
        }
    }

    /// An answer read against the project and put up for review.
    fn classification_take_answer(&mut self, d: &mut ClassificationManager, answer: &Value, rows: &BTreeMap<String, RowInfo>) {
        let read = cls::read_answer(&self.session.model, answer);
        d.refused.extend(read.refused);
        for p in read.proposals {
            let info = rows.get(&p.key).cloned().unwrap_or_else(|| RowInfo { label: p.key.clone(), ..RowInfo::default() });
            let accept = p.code.is_some() && p.confidence >= d.threshold && p.code != p.current;
            d.review.push(ReviewRow { proposal: p, accept, label: info.label, category: info.category, ids: info.ids });
        }
        d.review.sort_by(|a, b| a.category.cmp(&b.category).then(a.label.cmp(&b.label)));
    }

    /// An agent's answer put up for review as if it had just come back: for tests, and for an answer pasted in.
    pub(crate) fn classification_answer(&mut self, answer: &Value) {
        let Some(mut d) = self.classification.take() else { return };
        let ask = BriefAsk { rows: Some("all".into()), ..BriefAsk::default() };
        let rows: BTreeMap<String, RowInfo> = cls::brief(&self.session.model, &ask)
            .map(|b| {
                b.rows
                    .into_iter()
                    .map(|r| (r.key.clone(), RowInfo { label: r.type_name.clone(), category: r.category, ids: r.ids }))
                    .collect()
            })
            .unwrap_or_default();
        self.classification_take_answer(&mut d, answer, &rows);
        d.tab = Tab::Agent;
        self.classification = Some(d);
    }
}

/// What one batch is asked: the brief's instructions, the answer's JSON Schema and the brief itself, as Copy Brief puts them.
fn prompt(m: &Model, b: &Brief) -> String {
    let h = cls::handover(m, b, false);
    format!("{LEAD}\n\n{}\nThe answer's JSON Schema:\n{}\n\n## Brief\n\n{}\n", h.instructions, cls::answer_schema(), h.inline)
}

/// One batch asked of the assistant's model through the assistant's own client and network (`complete`): the prompt as the
/// user's message, and the model's words back, its tokens added to `used`. Its tools go with every request the assistant makes;
/// a model that calls one anyway is told this task has none, twice at most. What goes out is held to the chat's cap.
fn ask_model(provider: Provider, net: &dyn Net, model: &str, key: &str, prompt: &str, used: &mut Usage) -> Result<String, String> {
    if prompt.len() > MAX_HISTORY {
        return Err(format!("a batch is larger than the {} KB the assistant sends at once: classify fewer tables", MAX_HISTORY / 1000));
    }
    let mut history = vec![Msg::user(prompt)];
    for _ in 0..3 {
        let reply = complete(provider, net, model, key, &history)?;
        used.add(reply.usage);
        if reply.calls.is_empty() {
            return Ok(reply.text);
        }
        let no = "This task has no tools: answer with the JSON object only.";
        let done = reply.calls.iter().map(|c| Done { id: c.id.clone(), name: c.name.clone(), text: no.into(), error: true }).collect();
        history.push(Msg::Assistant(reply));
        history.push(Msg::Results(done));
    }
    Err(format!("{} kept calling its tools instead of answering", provider.name()))
}
