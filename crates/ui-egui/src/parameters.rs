//! Manage > Project Parameters: the one place for everything about parameters. Project and shared parameters in one table,
//! edited in bulk and applied at once (`parameter_batch`, one undo step), their categories, the families' parameters and
//! transfer between families, dictionaries, and import and export with category mapping. The logic is the core's
//! (`bimcraft_core::parameters`): this holds what's typed until Apply and shows what a dry run says.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bimcraft_core::model::{Model, PARAMETER_CATEGORIES, ParameterKind, ProjectParameter};
use bimcraft_core::parameters::{self, Definition, Dictionary, Usage, table};
use bimcraft_i18n::msg;
use egui::{Align, Button, ComboBox, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, TextEdit, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{self, t, tf};
use crate::popup::{self, Modal};
use crate::style::pal;

/// What a project parameter holds, as the window names it.
const KINDS: [(ParameterKind, &str); 8] = [
    (ParameterKind::Text, msg!("Text")),
    (ParameterKind::Number, msg!("Number")),
    (ParameterKind::Integer, msg!("Integer")),
    (ParameterKind::Length, msg!("Length")),
    (ParameterKind::Area, msg!("Area")),
    (ParameterKind::Volume, msg!("Volume")),
    (ParameterKind::Angle, msg!("Angle")),
    (ParameterKind::YesNo, msg!("Yes/No")),
];

fn kind_name(k: ParameterKind) -> &'static str {
    KINDS.iter().find(|(q, _)| *q == k).map_or("Text", |(_, n)| n)
}

/// What a family's parameter holds, as the window names it.
fn family_kind_name(k: bimcraft_core::family::Kind) -> &'static str {
    use bimcraft_core::family::Kind;
    match k {
        Kind::Length => msg!("Length"),
        Kind::Number => msg!("Number"),
        Kind::Angle => msg!("Angle"),
        Kind::YesNo => msg!("Yes/No"),
        Kind::Material => msg!("Material"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Parameters,
    Categories,
    Families,
    Dictionaries,
}

/// A parameter as it's being edited: `original` is its name in the project, none for one not made yet.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    checked: bool,
    original: Option<String>,
    deleted: bool,
    name: String,
    kind: ParameterKind,
    categories: BTreeSet<String>,
    group: String,
    description: String,
    hidden: bool,
    read_only: bool,
    hide_when_empty: bool,
    dictionary: Option<String>,
    usage: Usage,
}

impl Row {
    fn of(m: &Model, name: &str, p: &ProjectParameter) -> Row {
        Row {
            checked: false,
            original: Some(name.to_string()),
            deleted: false,
            name: name.to_string(),
            kind: p.kind,
            categories: p.categories.iter().cloned().collect(),
            group: p.group.clone(),
            description: p.description.clone(),
            hidden: p.hidden,
            read_only: p.read_only,
            hide_when_empty: p.hide_when_empty,
            dictionary: p.dictionary.clone(),
            usage: parameters::usage(m, name),
        }
    }

    /// Whether it differs from what the project holds.
    fn changed(&self, m: &Model) -> bool {
        let Some(name) = &self.original else { return true };
        let Some(p) = m.parameters.get(name) else { return true };
        let mut was = Row::of(m, name, p);
        was.checked = self.checked;
        *self != was
    }

    /// The change `parameter_batch` makes for it.
    fn op(&self) -> Value {
        let categories: Vec<&String> = self.categories.iter().collect();
        let mut op = json!({
            "name": self.original.as_ref().unwrap_or(&self.name),
            "kind": self.kind,
            "categories": categories,
            "group": self.group,
            "description": self.description,
            "hidden": self.hidden,
            "read_only": self.read_only,
            "hide_when_empty": self.hide_when_empty,
        });
        if self.original.as_ref().is_some_and(|o| *o != self.name) {
            op["rename"] = json!(self.name.trim());
        }
        if self.original.is_some() && self.dictionary.is_none() {
            op["dictionary"] = Value::Null;
        }
        op
    }
}

/// A file being imported: what a dry run says, and what's been decided about it.
struct Review {
    path: String,
    report: Value,
    map: BTreeMap<String, Option<String>>,
    resolve: BTreeMap<String, String>,
    categories: BTreeSet<String>,
    names: Option<Vec<String>>,
    replace: bool,
    skip_conflicts: bool,
}

impl Review {
    fn params(&self, dry_run: bool) -> Value {
        let categories: Vec<&String> = self.categories.iter().collect();
        let mut p = json!({
            "path": self.path,
            "map": self.map,
            "resolve": self.resolve,
            "categories": categories,
            "replace_categories": self.replace,
            "skip_conflicts": self.skip_conflicts,
            "dry_run": dry_run,
        });
        if let Some(n) = &self.names {
            p["names"] = json!(n);
        }
        p
    }
}

/// A dictionary's file, open to edit: each change goes straight to the file.
#[derive(Default)]
struct Open {
    path: String,
    dictionary: Dictionary,
    checked: BTreeSet<String>,
    version: String,
}

/// The window's state.
pub(crate) struct ParameterManager {
    tab: Tab,
    rows: Vec<Row>,
    search: String,
    show_existing: bool,
    message: Option<(String, bool)>,
    review: Option<Review>,
    // Batch rename.
    prefix: String,
    suffix: String,
    find: String,
    replace: String,
    merge_into: String,
    // Categories.
    category_filter: String,
    hide_unselected: bool,
    // Families.
    family: String,
    family_checked: BTreeSet<String>,
    family_to: BTreeSet<String>,
    // Dictionaries.
    open: Option<Open>,
}

impl ParameterManager {
    pub(crate) fn new(m: &Model) -> ParameterManager {
        ParameterManager {
            tab: Tab::Parameters,
            rows: rows_of(m),
            search: String::new(),
            show_existing: true,
            message: None,
            review: None,
            prefix: String::new(),
            suffix: String::new(),
            find: String::new(),
            replace: String::new(),
            merge_into: String::new(),
            category_filter: String::new(),
            hide_unselected: false,
            family: m.families.keys().next().cloned().unwrap_or_default(),
            family_checked: BTreeSet::new(),
            family_to: BTreeSet::new(),
            open: None,
        }
    }

    /// The table read again from the project, the same names staying checked.
    fn reload(&mut self, m: &Model) {
        let checked: BTreeSet<String> = self.rows.iter().filter(|r| r.checked).filter_map(|r| r.original.clone()).collect();
        self.rows = rows_of(m);
        for r in &mut self.rows {
            r.checked = r.original.as_ref().is_some_and(|o| checked.contains(o));
        }
    }

    fn counts(&self, m: &Model) -> (usize, usize, usize, usize) {
        let live = self.rows.iter().filter(|r| !r.deleted);
        let create = live.clone().filter(|r| r.original.is_none()).count();
        let modify = live.clone().filter(|r| r.original.is_some() && r.changed(m)).count();
        let existing = live.filter(|r| r.original.is_some() && !r.changed(m)).count();
        (create, modify, existing, self.rows.iter().filter(|r| r.deleted).count())
    }

    /// The changes staged, deletions first so that a name freed can be taken.
    fn ops(&self, m: &Model) -> Vec<Value> {
        let gone = self.rows.iter().filter(|r| r.deleted && r.original.is_some()).filter_map(|r| r.original.as_ref());
        let mut ops: Vec<Value> = gone.map(|n| json!({"op": "delete", "name": n})).collect();
        ops.extend(self.rows.iter().filter(|r| !r.deleted && r.original.is_some() && r.changed(m)).map(Row::op));
        ops.extend(self.rows.iter().filter(|r| !r.deleted && r.original.is_none()).map(Row::op));
        ops
    }

    fn checked_names(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.checked && !r.deleted).filter_map(|r| r.original.clone()).collect()
    }
}

fn rows_of(m: &Model) -> Vec<Row> {
    m.parameters.iter().map(|(n, p)| Row::of(m, n, p)).collect()
}

/// A checkbox with no text beside it, read by assistive tools as `label`.
fn tick(ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    let r = ui.checkbox(on, "");
    let now = *on;
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, now, label));
    r.changed()
}

/// A text field read by assistive tools as `label`.
fn field(ui: &mut Ui, text: &mut String, width: f32, label: &str) -> egui::Response {
    // Sized, not asked for: in a table's grid a field wanting a width gets what's left of the column.
    let height = ui.spacing().interact_size.y;
    let r = ui.add_sized([width, height], TextEdit::singleline(text));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
    r
}

impl BimApp {
    /// Runs a command for the window: an error is said in the window rather than behind it.
    fn manage(&mut self, d: &mut ParameterManager, id: &str, p: Value) -> Option<Value> {
        let r = self.run(id, p);
        if r.is_none() {
            d.message = Some((self.status.text.clone(), true));
        }
        r
    }

    /// Begins the import of the file at `path`: a dry run, for the window to show.
    pub(crate) fn parameters_import_from(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        self.begin_review(&mut d, path, None);
        self.parameter_manager = Some(d);
    }

    fn begin_review(&mut self, d: &mut ParameterManager, path: &str, names: Option<Vec<String>>) {
        let mut review = Review {
            path: path.to_string(),
            report: Value::Null,
            map: BTreeMap::new(),
            resolve: BTreeMap::new(),
            categories: BTreeSet::new(),
            names,
            replace: false,
            skip_conflicts: false,
        };
        if let Some(r) = self.manage(d, "parameter_import", review.params(true)) {
            review.report = r;
            d.message = None;
            d.review = Some(review);
        }
    }

    /// Exports what's checked, or all, to `path`.
    pub(crate) fn parameters_export_to(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        let names = d.checked_names();
        let mut p = json!({"path": path});
        if !names.is_empty() {
            p["names"] = json!(names);
        }
        if let Some(r) = self.manage(&mut d, "parameter_export", p) {
            d.message = Some((tf("{} parameters exported.", &[&r["parameters"].to_string()]), false));
        }
        self.parameter_manager = Some(d);
    }

    /// Opens the dictionary at `path`, making it if there is none yet.
    pub(crate) fn parameters_open_dictionary(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        if !std::path::Path::new(path).exists() && self.manage(&mut d, "dictionary", json!({"path": path})).is_none() {
            self.parameter_manager = Some(d);
            return;
        }
        match table::read_dictionary_file(path) {
            Ok(dictionary) => {
                let version = dictionary.version.clone();
                d.open = Some(Open { path: path.to_string(), dictionary, checked: BTreeSet::new(), version });
                d.tab = Tab::Dictionaries;
                d.message = None;
            }
            Err(e) => d.message = Some((e, true)),
        }
        self.parameter_manager = Some(d);
    }

    /// Takes a file into the dictionary that's open.
    pub(crate) fn parameters_merge_into_dictionary(&mut self, file: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        let path = d.open.as_ref().map(|o| o.path.clone());
        if let Some(path) = path {
            self.dictionary_change(&mut d, &path, json!({"merge": [file], "skip_conflicts": true}));
        }
        self.parameter_manager = Some(d);
    }

    fn dictionary_change(&mut self, d: &mut ParameterManager, path: &str, mut p: Value) {
        p["path"] = json!(path);
        if self.manage(d, "dictionary", p).is_some() {
            match table::read_dictionary_file(path) {
                Ok(dictionary) => {
                    if let Some(open) = &mut d.open {
                        open.version = dictionary.version.clone();
                        open.dictionary = dictionary;
                    }
                    d.message = None;
                }
                Err(e) => d.message = Some((e, true)),
            }
        }
    }

    /// Manage > Project Parameters.
    pub(crate) fn parameters_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        let model = Arc::clone(&self.session.model);
        let mut close = false;
        let r = Modal::new(UiId::new("project-parameters"), t("Project Parameters")).width(1040.0).min_height(560.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.add_space(4.0);
                if d.review.is_some() {
                    self.review_view(ui, ctx, &mut d, &model);
                } else {
                    ui.horizontal(|ui| {
                        for (tab, label) in [
                            (Tab::Parameters, msg!("Parameters")),
                            (Tab::Categories, msg!("Categories")),
                            (Tab::Families, msg!("Families")),
                            (Tab::Dictionaries, msg!("Dictionaries")),
                        ] {
                            if ui.selectable_label(d.tab == tab, t(label)).clicked() {
                                d.tab = tab;
                                d.message = None;
                            }
                        }
                    });
                    ui.separator();
                    match d.tab {
                        Tab::Parameters => self.parameters_tab(ui, ctx, &mut d, &model),
                        Tab::Categories => categories_tab(ui, &mut d, &model),
                        Tab::Families => self.families_tab(ui, &mut d, &model),
                        Tab::Dictionaries => self.dictionaries_tab(ui, ctx, &mut d, &model),
                    }
                }
                if let Some((text, error)) = &d.message {
                    ui.add_space(4.0);
                    ui.label(if *error { RichText::new(text).color(pal().error) } else { RichText::new(text) });
                }
            });
            popup::footer(ui, |ui| {
                let (create, modify, existing, delete) = d.counts(&model);
                ui.label(tf(
                    "{} parameters | create {} | modify {} | existing {} | delete {}",
                    &[&d.rows.len().to_string(), &create.to_string(), &modify.to_string(), &existing.to_string(), &delete.to_string()],
                ));
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    let ops = d.ops(&model);
                    if ui.add_enabled(!ops.is_empty(), Button::new(t("Apply"))).clicked()
                        && self.manage(&mut d, "parameter_batch", json!({"ops": ops})).is_some()
                    {
                        d.reload(&self.session.model);
                        d.message = Some((t("Applied.").to_string(), false));
                    }
                    if ui.add_enabled(!d.ops(&model).is_empty(), Button::new(t("Discard Changes"))).clicked() {
                        d.reload(&model);
                        d.message = None;
                    }
                });
            });
        });
        if !(close || r.should_close()) {
            self.parameter_manager = Some(d);
        }
    }

    fn parameters_tab(&mut self, ui: &mut Ui, ctx: &Context, d: &mut ParameterManager, model: &Arc<Model>) {
        ui.horizontal(|ui| {
            if ui.button(t("Add Parameter")).clicked() {
                let name = (1..)
                    .map(|n| format!("{} {n}", t("Parameter")))
                    .find(|n| !d.rows.iter().any(|r| r.name == *n) && !model.parameters.contains_key(n))
                    .unwrap_or_default();
                d.rows.push(Row {
                    checked: false,
                    original: None,
                    deleted: false,
                    name,
                    kind: ParameterKind::Text,
                    categories: BTreeSet::from(["wall".to_string()]),
                    group: String::new(),
                    description: String::new(),
                    hidden: false,
                    read_only: false,
                    hide_when_empty: false,
                    dictionary: None,
                    usage: Usage::default(),
                });
            }
            ui.menu_button(t("Import/Export"), |ui| {
                if ui.button(t("Import...")).clicked() {
                    ui.close();
                    let dialog = rfd::AsyncFileDialog::new()
                        .add_filter(t("Parameters"), &["xlsx", "csv", "bimparams", "txt"])
                        .add_filter(t("Revit shared parameters"), &["txt"]);
                    self.ask_file(ctx, dialog, false, |app, path| app.parameters_import_from(&path.to_string_lossy()));
                }
                if ui.button(t("Export...")).clicked() {
                    ui.close();
                    let dialog = rfd::AsyncFileDialog::new()
                        .add_filter(t("Excel workbook"), &["xlsx"])
                        .add_filter(t("CSV table"), &["csv"])
                        .add_filter(t("Dictionary"), &[parameters::DICTIONARY_EXTENSION])
                        .set_file_name("parameters.xlsx");
                    self.ask_file(ctx, dialog, true, |app, path| app.parameters_export_to(&path.to_string_lossy()));
                }
            });
            self.batch_menu(ui, d, model);
            ui.checkbox(&mut d.show_existing, t("Show Existing Parameters"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                field(ui, &mut d.search, 180.0, t("Search"));
                ui.label(t("Search:"));
            });
        });
        ui.collapsing(t("Rename Checked"), |ui| {
            ui.horizontal(|ui| {
                ui.label(t("Prefix:"));
                field(ui, &mut d.prefix, 90.0, t("Prefix"));
                ui.label(t("Suffix:"));
                field(ui, &mut d.suffix, 90.0, t("Suffix"));
                ui.label(t("Find:"));
                field(ui, &mut d.find, 110.0, t("Find"));
                ui.label(t("Replace:"));
                field(ui, &mut d.replace, 110.0, t("Replace"));
                if ui.button(t("Rename")).clicked() {
                    for r in d.rows.iter_mut().filter(|r| r.checked) {
                        let mid = if d.find.is_empty() { r.name.clone() } else { r.name.replace(&d.find, &d.replace) };
                        r.name = format!("{}{mid}{}", d.prefix, d.suffix);
                    }
                }
            });
        });
        ui.add_space(4.0);
        let needle = d.search.trim().to_lowercase();
        let groups: BTreeSet<String> = d.rows.iter().map(|r| r.group.clone()).filter(|g| !g.is_empty()).collect();
        let mut pick_group: Option<(usize, String)> = None;
        ScrollArea::both().max_height(380.0).auto_shrink([false, false]).show(ui, |ui| {
            Grid::new("parameter-table").striped(true).num_columns(11).spacing([8.0, 4.0]).show(ui, |ui| {
                let all = !d.rows.is_empty() && d.rows.iter().all(|r| r.checked);
                let mut every = all;
                if tick(ui, &mut every, t("Check All")) {
                    d.rows.iter_mut().for_each(|r| r.checked = every);
                }
                for head in [
                    "",
                    msg!("Name"),
                    msg!("Source"),
                    msg!("Type of Parameter"),
                    msg!("Group Under"),
                    msg!("Description"),
                    msg!("Visible"),
                    msg!("Editable"),
                    msg!("Hide When Empty"),
                    msg!("Used"),
                ] {
                    ui.label(RichText::new(if head.is_empty() { "" } else { t(head) }).strong());
                }
                ui.end_row();
                for (i, r) in d.rows.iter_mut().enumerate() {
                    if !needle.is_empty() && !r.name.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let changed = r.changed(model);
                    if !d.show_existing && !changed {
                        continue;
                    }
                    let label = r.name.clone();
                    tick(ui, &mut r.checked, &tf("Check {}", &[&label]));
                    let (mark, tip) = match (r.deleted, &r.original, changed) {
                        (true, _, _) => ("×", t("To be deleted")),
                        (_, None, _) => ("+", t("To be made")),
                        (_, _, true) => ("●", t("To be changed")),
                        _ => ("✓", t("As the project has it")),
                    };
                    ui.label(mark).on_hover_text(tip);
                    field(ui, &mut r.name, 180.0, &tf("Name of {}", &[&label]));
                    let source = r.dictionary.as_ref().map_or_else(|| t("Project").to_string(), |n| tf("Shared: {}", &[n]));
                    ui.label(source);
                    let kinds =
                        ComboBox::from_id_salt(("parameter-kind", i)).selected_text(t(kind_name(r.kind))).width(90.0).show_ui(ui, |ui| {
                            for (k, name) in KINDS {
                                ui.selectable_value(&mut r.kind, k, t(name));
                            }
                        });
                    let kind_label = tf("Type of {}", &[&label]);
                    kinds.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, kind_label.clone()));
                    ui.horizontal(|ui| {
                        field(ui, &mut r.group, 100.0, &tf("Group of {}", &[&label]));
                        if !groups.is_empty() {
                            ui.menu_button("...", |ui| {
                                for g in &groups {
                                    if ui.button(g).clicked() {
                                        pick_group = Some((i, g.clone()));
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                    field(ui, &mut r.description, 160.0, &tf("Description of {}", &[&label]));
                    let mut visible = !r.hidden;
                    if tick(ui, &mut visible, &tf("{} visible", &[&label])) {
                        r.hidden = !visible;
                    }
                    let mut editable = !r.read_only;
                    if tick(ui, &mut editable, &tf("{} editable", &[&label])) {
                        r.read_only = !editable;
                    }
                    tick(ui, &mut r.hide_when_empty, &tf("{} hidden when empty", &[&label]));
                    let u = &r.usage;
                    let total = u.values + u.schedules + u.filters + u.tags + u.formulas;
                    ui.label(total.to_string()).on_hover_text(tf(
                        "{} values, {} schedules, {} filters, {} tags, {} formulas",
                        &[
                            &u.values.to_string(),
                            &u.schedules.to_string(),
                            &u.filters.to_string(),
                            &u.tags.to_string(),
                            &u.formulas.to_string(),
                        ],
                    ));
                    ui.end_row();
                }
            });
        });
        if let Some((i, g)) = pick_group
            && let Some(r) = d.rows.get_mut(i)
        {
            r.group = g;
        }
        if d.rows.is_empty() {
            ui.label(
                RichText::new(t("No project parameters yet: Add Parameter makes one, Import/Export brings them from a file."))
                    .color(pal().muted),
            );
        }
    }

    fn batch_menu(&mut self, ui: &mut Ui, d: &mut ParameterManager, model: &Arc<Model>) {
        ui.menu_button(t("Batch Actions"), |ui| {
            let any = d.rows.iter().any(|r| r.checked);
            if ui.add_enabled(any, Button::new(t("Duplicate Checked"))).clicked() {
                ui.close();
                let names = d.checked_names();
                let ops: Vec<Value> = names.iter().map(|n| json!({"op": "duplicate", "name": n})).collect();
                if self.manage(d, "parameter_batch", json!({"ops": ops})).is_some() {
                    d.reload(&self.session.model);
                }
            }
            if ui.add_enabled(any, Button::new(t("Delete Checked"))).clicked() {
                ui.close();
                d.rows.retain(|r| !(r.checked && r.original.is_none()));
                d.rows.iter_mut().filter(|r| r.checked).for_each(|r| r.deleted = true);
            }
            if ui
                .add_enabled(any, Button::new(t("Make Project Parameters")))
                .on_hover_text(t("No longer from a dictionary. They keep their GUIDs."))
                .clicked()
            {
                ui.close();
                d.rows.iter_mut().filter(|r| r.checked).for_each(|r| r.dictionary = None);
            }
            if ui
                .button(t("Purge Unused"))
                .on_hover_text(t("Delete the parameters no value, schedule, filter, tag or formula uses."))
                .clicked()
            {
                ui.close();
                if let Some(r) = self.manage(d, "parameter_purge", json!({})) {
                    d.reload(&self.session.model);
                    d.message = Some((tf("{} parameters purged.", &[&r["purged"].as_array().map_or(0, Vec::len).to_string()]), false));
                }
            }
            ui.separator();
            ui.label(t("Merge Checked Into:"));
            ComboBox::from_id_salt("parameter-merge-into").selected_text(d.merge_into.clone()).width(180.0).show_ui(ui, |ui| {
                for name in model.parameters.keys() {
                    if ui.selectable_label(d.merge_into == *name, name).clicked() {
                        d.merge_into = name.clone();
                    }
                }
            });
            let into = d.merge_into.clone();
            let sources: Vec<String> = d.checked_names().into_iter().filter(|n| *n != into).collect();
            if ui.add_enabled(!into.is_empty() && !sources.is_empty(), Button::new(t("Merge"))).clicked() {
                ui.close();
                for from in sources {
                    if self.manage(d, "parameter_merge", json!({"from": from, "into": into})).is_none() {
                        break;
                    }
                }
                d.reload(&self.session.model);
            }
        });
    }

    fn review_view(&mut self, ui: &mut Ui, _ctx: &Context, d: &mut ParameterManager, model: &Arc<Model>) {
        let Some(mut review) = d.review.take() else { return };
        let mut again = false;
        let (mut cancel, mut apply) = (false, false);
        ui.label(RichText::new(tf("Import from {}", &[&review.path])).strong());
        let r = &review.report;
        let number = |k: &str| r[k].as_u64().unwrap_or(0).to_string();
        ui.label(tf(
            "create {} | modify {} | existing {} | conflict {} | skip {}",
            &[&number("create"), &number("modify"), &number("existing"), &number("conflict"), &number("skip")],
        ));
        // The categories the file names that have no answer here.
        let unmapped: Vec<String> =
            r["unmapped"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut mapped: Vec<String> = review.map.keys().cloned().collect();
        mapped.retain(|k| !unmapped.contains(k));
        if !unmapped.is_empty() || !mapped.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(t("Category Mapping")).strong());
            Grid::new("parameter-mapping").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for raw in unmapped.iter().chain(&mapped) {
                    ui.label(raw);
                    let now = review.map.get(raw).cloned();
                    let shown = match &now {
                        Some(Some(c)) => lang::category(c, true),
                        Some(None) => t("Leave Out").to_string(),
                        None => t("Choose...").to_string(),
                    };
                    let combo = ComboBox::from_id_salt(("map", raw)).selected_text(shown).width(180.0).show_ui(ui, |ui| {
                        if ui.selectable_label(matches!(now, Some(None)), t("Leave Out")).clicked() {
                            review.map.insert(raw.clone(), None);
                            again = true;
                        }
                        for c in PARAMETER_CATEGORIES {
                            if ui.selectable_label(now == Some(Some(c.to_string())), lang::category(c, true)).clicked() {
                                review.map.insert(raw.clone(), Some(c.to_string()));
                                again = true;
                            }
                        }
                    });
                    let label = tf("Map {}", &[raw]);
                    combo.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                    ui.end_row();
                }
            });
        }
        let broad: Vec<&str> = r["broad"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !broad.is_empty() {
            ui.label(
                RichText::new(tf("These go on every component, the nearest category there is here: {}", &[&broad.join(", ")]))
                    .color(pal().muted),
            );
        }
        // A file that names no categories, as a Revit shared parameter file.
        ui.collapsing(t("Categories for rows that name none"), |ui| {
            Grid::new("parameter-default-categories").num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
                for (i, c) in PARAMETER_CATEGORIES.iter().enumerate() {
                    let mut on = review.categories.contains(*c);
                    if ui.checkbox(&mut on, lang::category(c, true)).changed() {
                        if on {
                            review.categories.insert((*c).to_string());
                        } else {
                            review.categories.remove(*c);
                        }
                        again = true;
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            again |= ui.checkbox(&mut review.replace, t("Take off categories the file doesn't list")).changed();
            ui.checkbox(&mut review.skip_conflicts, t("Skip rows that need a decision"));
        });
        ui.add_space(4.0);
        let rows: Vec<Value> = r["rows"].as_array().cloned().unwrap_or_default();
        ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            Grid::new("parameter-review").striped(true).num_columns(5).spacing([10.0, 4.0]).show(ui, |ui| {
                for head in [msg!("Status"), msg!("In the File"), msg!("Here"), msg!("Decision"), msg!("Notes")] {
                    ui.label(RichText::new(t(head)).strong());
                }
                ui.end_row();
                for row in &rows {
                    let name = row["name"].as_str().unwrap_or("").to_string();
                    let status = row["status"].as_str().unwrap_or("");
                    let word = match status {
                        "create" => t("Create"),
                        "modify" => t("Modify"),
                        "existing" => t("Existing"),
                        "conflict" => t("Conflict"),
                        _ => t("Skip"),
                    };
                    ui.label(if status == "conflict" { RichText::new(word).color(pal().error) } else { RichText::new(word) });
                    ui.label(&name);
                    ui.label(row["here"].as_str().unwrap_or(""));
                    let taken = row["decide"].as_bool().unwrap_or(false) || review.resolve.contains_key(&name);
                    if taken {
                        let now = review.resolve.get(&name).cloned().unwrap_or_default();
                        let shown = match now.as_str() {
                            "same" => t("The Same Parameter"),
                            "separate" => t("Keep Separate"),
                            "skip" => t("Skip"),
                            _ => t("Choose..."),
                        };
                        let combo = ComboBox::from_id_salt(("resolve", &name)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                            for (value, label) in
                                [("same", msg!("The Same Parameter")), ("separate", msg!("Keep Separate")), ("skip", msg!("Skip"))]
                            {
                                if ui.selectable_label(now == value, t(label)).clicked() {
                                    review.resolve.insert(name.clone(), value.to_string());
                                    again = true;
                                }
                            }
                        });
                        let label = tf("Decision for {}", &[&name]);
                        combo.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                    } else {
                        ui.label("");
                    }
                    let notes: Vec<&str> =
                        row["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    ui.label(notes.join("; "));
                    ui.end_row();
                }
            });
        });
        let left: Vec<&str> = r["left_out"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !left.is_empty() {
            ui.collapsing(tf("Left out or changed ({})", &[&left.len().to_string()]), |ui| {
                ScrollArea::vertical().max_height(90.0).show(ui, |ui| {
                    for l in left {
                        ui.label(l);
                    }
                });
            });
        }
        ui.add_space(6.0);
        let conflicts = r["conflict"].as_u64().unwrap_or(0);
        let work = r["create"].as_u64().unwrap_or(0) + r["modify"].as_u64().unwrap_or(0) + r["existing"].as_u64().unwrap_or(0);
        ui.horizontal(|ui| {
            cancel = ui.button(t("Cancel Import")).clicked();
            apply = ui.add_enabled(work > 0 && (conflicts == 0 || review.skip_conflicts), Button::new(t("Import"))).clicked();
        });
        let _ = model;
        if cancel {
            d.message = None;
            return;
        }
        if apply {
            if let Some(r) = self.manage(d, "parameter_import", review.params(false)) {
                d.reload(&self.session.model);
                d.message = Some((tf("Imported: {} made, {} changed.", &[&r["create"].to_string(), &r["modify"].to_string()]), false));
                return;
            }
        } else if again && let Some(r) = self.manage(d, "parameter_import", review.params(true)) {
            review.report = r;
            d.message = None;
        }
        d.review = Some(review);
    }

    fn families_tab(&mut self, ui: &mut Ui, d: &mut ParameterManager, model: &Arc<Model>) {
        if model.families.is_empty() {
            ui.label(RichText::new(t("The project has no families.")).color(pal().muted));
            return;
        }
        ui.columns(2, |cols| {
            let [left, right] = cols else { return };
            left.horizontal(|ui| {
                ui.label(t("Family:"));
                let combo = ComboBox::from_id_salt("family-source").selected_text(d.family.clone()).width(240.0).show_ui(ui, |ui| {
                    for name in model.families.keys() {
                        if ui.selectable_label(d.family == *name, name).clicked() {
                            d.family = name.clone();
                            d.family_checked.clear();
                        }
                    }
                });
                combo.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Family")));
            });
            ScrollArea::vertical().id_salt("family-parameters").max_height(320.0).show(left, |ui| {
                Grid::new("family-parameter-table").striped(true).num_columns(5).spacing([8.0, 3.0]).show(ui, |ui| {
                    for head in ["", msg!("Name"), msg!("Type of Parameter"), msg!("Instance/Type"), msg!("Formula")] {
                        ui.label(RichText::new(if head.is_empty() { "" } else { t(head) }).strong());
                    }
                    ui.end_row();
                    for q in model.families.get(&d.family).map_or(&[][..], |f| f.parameters.as_slice()) {
                        let mut on = d.family_checked.contains(&q.name);
                        if tick(ui, &mut on, &tf("Check {}", &[&q.name])) {
                            if on {
                                d.family_checked.insert(q.name.clone());
                            } else {
                                d.family_checked.remove(&q.name);
                            }
                        }
                        ui.label(&q.name);
                        ui.label(t(family_kind_name(q.kind)));
                        ui.label(if q.instance { t("Instance") } else { t("Type") });
                        ui.label(q.formula.clone().unwrap_or_default());
                        ui.end_row();
                    }
                });
            });
            right.label(RichText::new(t("Transfer To")).strong());
            ScrollArea::vertical().id_salt("family-targets").max_height(250.0).show(right, |ui| {
                for name in model.families.keys().filter(|n| **n != d.family) {
                    let mut on = d.family_to.contains(name);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            d.family_to.insert(name.clone());
                        } else {
                            d.family_to.remove(name);
                        }
                    }
                }
            });
            right.horizontal(|ui| {
                ui.label(t("Prefix:"));
                field(ui, &mut d.prefix, 70.0, t("Prefix"));
                ui.label(t("Suffix:"));
                field(ui, &mut d.suffix, 70.0, t("Suffix"));
                ui.label(t("Find:"));
                field(ui, &mut d.find, 70.0, t("Find"));
                ui.label(t("Replace:"));
                field(ui, &mut d.replace, 70.0, t("Replace"));
            });
            let ready = !d.family_checked.is_empty() && !d.family_to.is_empty();
            let transfer = right.horizontal(|ui| ui.add_enabled(ready, Button::new(t("Transfer"))).clicked()).inner;
            if transfer {
                let p = json!({
                    "from": d.family,
                    "to": d.family_to,
                    "names": d.family_checked,
                    "prefix": d.prefix,
                    "suffix": d.suffix,
                    "find": d.find,
                    "replace": d.replace,
                });
                if let Some(r) = self.manage(d, "family_parameter_transfer", p) {
                    let of = |k: &str| {
                        r["families"].as_array().map_or(0, |a| a.iter().map(|f| f[k].as_array().map_or(0, Vec::len)).sum::<usize>())
                    };
                    d.message = Some((
                        tf("{} added, {} skipped: their names are taken.", &[&of("added").to_string(), &of("skipped").to_string()]),
                        false,
                    ));
                }
            }
        });
    }

    fn dictionaries_tab(&mut self, ui: &mut Ui, ctx: &Context, d: &mut ParameterManager, model: &Arc<Model>) {
        ui.label(RichText::new(t("In This Project")).strong());
        if model.dictionaries.is_empty() {
            ui.label(RichText::new(t("No parameter of this project comes from a dictionary yet.")).color(pal().muted));
        }
        let mut update: Option<String> = None;
        Grid::new("dictionaries-used").striped(true).num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
            for (name, r) in &model.dictionaries {
                ui.label(name);
                ui.label(&r.version);
                ui.label(&r.path);
                if ui
                    .add_enabled(!r.path.is_empty(), Button::new(tf("Check {}", &[name])))
                    .on_hover_text(t("Show what its file would change."))
                    .clicked()
                {
                    update = Some(r.path.clone());
                }
                ui.end_row();
            }
        });
        if let Some(path) = update {
            self.begin_review(d, &path, None);
            return;
        }
        ui.separator();
        ui.horizontal(|ui| {
            let filter = parameters::DICTIONARY_EXTENSION;
            if ui.button(t("New Dictionary...")).clicked() {
                let dialog =
                    rfd::AsyncFileDialog::new().add_filter(t("Dictionary"), &[filter]).set_file_name(format!("parameters.{filter}"));
                self.ask_file(ctx, dialog, true, |app, path| app.parameters_open_dictionary(&path.to_string_lossy()));
            }
            if ui.button(t("Open Dictionary...")).clicked() {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Dictionary"), &[filter]);
                self.ask_file(ctx, dialog, false, |app, path| app.parameters_open_dictionary(&path.to_string_lossy()));
            }
        });
        let Some(mut open) = d.open.take() else {
            ui.label(RichText::new(t("A dictionary is a file of parameter definitions to share between projects: keep it on a shared drive or in a Git repository.")).color(pal().muted));
            return;
        };
        ui.add_space(4.0);
        ui.label(RichText::new(tf("{} ({})", &[&open.dictionary.name, &open.path])).strong());
        ui.horizontal(|ui| {
            ui.label(t("Version:"));
            let r = field(ui, &mut open.version, 100.0, t("Version"));
            if r.lost_focus() && open.version != open.dictionary.version {
                d.open = Some(Open {
                    path: open.path.clone(),
                    dictionary: open.dictionary.clone(),
                    checked: open.checked.clone(),
                    version: open.version.clone(),
                });
                self.dictionary_change(d, &open.path, json!({"version": open.version}));
                if let Some(now) = d.open.take() {
                    open = now;
                }
            }
            let checked_project = d.checked_names();
            if ui.add_enabled(!checked_project.is_empty(), Button::new(t("Add Checked Project Parameters"))).clicked() {
                d.open = Some(std::mem::take(&mut open));
                let path = d.open.as_ref().map(|o| o.path.clone()).unwrap_or_default();
                self.dictionary_change(d, &path, json!({"names": checked_project, "skip_conflicts": true}));
                open = d.open.take().unwrap_or_default();
            }
            if ui.button(t("Merge File...")).clicked() {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Parameters"), &["xlsx", "csv", "bimparams", "txt"]);
                self.ask_file(ctx, dialog, false, |app, path| app.parameters_merge_into_dictionary(&path.to_string_lossy()));
            }
            let any = !open.checked.is_empty();
            if ui.add_enabled(any, Button::new(t("Remove Checked"))).clicked() {
                let names: Vec<String> = open.checked.iter().cloned().collect();
                open.checked.clear();
                d.open = Some(std::mem::take(&mut open));
                let path = d.open.as_ref().map(|o| o.path.clone()).unwrap_or_default();
                self.dictionary_change(d, &path, json!({"remove": names}));
                open = d.open.take().unwrap_or_default();
            }
            if ui.add_enabled(any, Button::new(t("Add Checked to Project"))).clicked() {
                let names: Vec<String> = open.checked.iter().cloned().collect();
                let path = open.path.clone();
                self.begin_review(d, &path, Some(names));
            }
        });
        ScrollArea::vertical().id_salt("dictionary-rows").max_height(250.0).show(ui, |ui| {
            Grid::new("dictionary-table").striped(true).num_columns(6).spacing([10.0, 3.0]).show(ui, |ui| {
                for head in ["", msg!("Name"), msg!("Type of Parameter"), msg!("Group Under"), msg!("Categories"), msg!("GUID")] {
                    ui.label(RichText::new(if head.is_empty() { "" } else { t(head) }).strong());
                }
                ui.end_row();
                for q in &open.dictionary.parameters {
                    row_of_dictionary(ui, q, &mut open.checked);
                }
            });
        });
        d.open = Some(open);
    }
}

fn row_of_dictionary(ui: &mut Ui, q: &Definition, checked: &mut BTreeSet<String>) {
    let mut on = checked.contains(&q.name);
    if tick(ui, &mut on, &tf("Check {}", &[&q.name])) {
        if on {
            checked.insert(q.name.clone());
        } else {
            checked.remove(&q.name);
        }
    }
    ui.label(&q.name);
    ui.label(t(kind_name(q.kind)));
    ui.label(&q.group);
    ui.label(q.categories.join(", "));
    ui.label(q.guid.map(|g| g.to_string()).unwrap_or_default());
    ui.end_row();
}

/// The Categories tab: the parameters checked on the left, what they're for on the right. Staged with the rest, until Apply.
fn categories_tab(ui: &mut Ui, d: &mut ParameterManager, model: &Arc<Model>) {
    let _ = model;
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        left.label(RichText::new(t("Parameter")).strong());
        ScrollArea::vertical().id_salt("category-parameters").max_height(390.0).show(left, |ui| {
            for r in d.rows.iter_mut().filter(|r| !r.deleted) {
                ui.horizontal(|ui| {
                    let label = r.name.clone();
                    ui.checkbox(&mut r.checked, label);
                    let names: Vec<String> = r.categories.iter().map(|c| lang::category(c, true)).collect();
                    ui.label(RichText::new(names.join(", ")).color(pal().muted));
                });
            }
        });
        right.horizontal(|ui| {
            ui.label(t("Filter:"));
            field(ui, &mut d.category_filter, 150.0, t("Filter"));
            ui.checkbox(&mut d.hide_unselected, t("Hide Unselected"));
        });
        let picked: Vec<usize> = d.rows.iter().enumerate().filter(|(_, r)| r.checked && !r.deleted).map(|(i, _)| i).collect();
        if picked.is_empty() {
            right.label(RichText::new(t("Check parameters to give them categories.")).color(pal().muted));
            return;
        }
        // Copy another's.
        let names: Vec<String> = d.rows.iter().filter(|r| !r.deleted).map(|r| r.name.clone()).collect();
        let mut copy_from: Option<String> = None;
        right.horizontal(|ui| {
            ui.menu_button(t("Copy Categories From"), |ui| {
                for n in &names {
                    if ui.button(n).clicked() {
                        copy_from = Some(n.clone());
                        ui.close();
                    }
                }
            });
        });
        if let Some(n) = copy_from {
            let from = d.rows.iter().find(|r| r.name == n).map(|r| r.categories.clone()).unwrap_or_default();
            for &i in &picked {
                if let Some(r) = d.rows.get_mut(i) {
                    r.categories = from.clone();
                }
            }
        }
        let needle = d.category_filter.trim().to_lowercase();
        ScrollArea::vertical().id_salt("category-list").max_height(330.0).show(right, |ui| {
            Grid::new("parameter-categories").num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
                let mut column = 0;
                for cat in PARAMETER_CATEGORIES {
                    let name = lang::category(cat, true);
                    let on_all = picked.iter().all(|i| d.rows.get(*i).is_some_and(|r| r.categories.contains(cat)));
                    let on_any = picked.iter().any(|i| d.rows.get(*i).is_some_and(|r| r.categories.contains(cat)));
                    if (!needle.is_empty() && !name.to_lowercase().contains(&needle)) || (d.hide_unselected && !on_any) {
                        continue;
                    }
                    let mut on = on_all;
                    if ui.add(egui::Checkbox::new(&mut on, name).indeterminate(on_any && !on_all)).changed() {
                        for &i in &picked {
                            if let Some(r) = d.rows.get_mut(i) {
                                if on {
                                    r.categories.insert(cat.to_string());
                                } else {
                                    r.categories.remove(cat);
                                }
                            }
                        }
                    }
                    column += 1;
                    if column % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    });
}
