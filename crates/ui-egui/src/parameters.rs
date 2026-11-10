//! Manage > Project Parameters: the one place for everything about parameters. Project and shared parameters in one table,
//! edited in bulk and applied at once (`parameter_batch`, one undo step), their categories, new parameters for families in bulk,
//! transfer between families, dictionaries, and import and export with category mapping. The logic is the core's
//! (`bimcraft_core::parameters`): this holds what's typed until Apply and shows what a dry run says.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bimcraft_core::family::Kind as Holds;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Model, PARAMETER_CATEGORIES, ParameterKind, ProjectParameter, Scope};
use bimcraft_core::parameters::{self, Definition, Dictionary, Usage, table};
use bimcraft_i18n::msg;
use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Button, ComboBox, Context, Grid, Id as UiId, Label, Layout, PopupCloseBehavior, RichText, ScrollArea, TextEdit, Ui, WidgetInfo,
    WidgetType, vec2,
};
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
const FAMILY_KINDS: [(Holds, &str); 6] = [
    (Holds::Text, msg!("Text")),
    (Holds::Length, msg!("Length")),
    (Holds::Number, msg!("Number")),
    (Holds::Angle, msg!("Angle")),
    (Holds::YesNo, msg!("Yes/No")),
    (Holds::Material, msg!("Material")),
];

fn family_kind_name(k: Holds) -> &'static str {
    FAMILY_KINDS.iter().find(|(q, _)| *q == k).map_or("Text", |(_, n)| n)
}

fn scope_name(s: Scope) -> &'static str {
    match s {
        Scope::Instance => msg!("Instance"),
        Scope::Type => msg!("Type"),
    }
}

/// A drop-down of Instance and Type, read by assistive tools as `label`. Whether it changed.
fn scope_combo(ui: &mut Ui, salt: impl egui::AsIdSalt, now: &mut Scope, label: &str) -> bool {
    let was = *now;
    let combo = ComboBox::from_id_salt(salt).selected_text(t(scope_name(*now))).width(80.0).show_ui(ui, |ui| {
        for s in [Scope::Instance, Scope::Type] {
            ui.selectable_value(now, s, t(scope_name(s)));
        }
    });
    combo.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label));
    *now != was
}

/// Text `width` wide, breaking onto new lines rather than widening its table and the window.
fn wrapped(ui: &mut Ui, text: impl Into<String>, width: f32) {
    let text: String = text.into();
    // Its height known now: a table's row is centred on what its cells ask for, so a cell that grew later would run over the next.
    let font = egui::TextStyle::Body.resolve(ui.style());
    let height = ui.painter().layout(text.clone(), font, pal().text, width).size().y;
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.add(Label::new(text).wrap());
    });
}

/// How tall a tab's two sides are, whatever they hold.
const SIDES: f32 = 420.0;

/// A tab's two sides, next to each other: `SIDES` tall, so what they hold, a rule between them too, can't stretch the window.
fn sides(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, SIDES), Layout::left_to_right(Align::Min), |ui| {
        ui.set_height(SIDES);
        add(ui);
    });
}

/// A menu of checkboxes that stays open while they're ticked.
fn check_menu(ui: &mut Ui, text: &str, add: impl FnOnce(&mut Ui)) -> egui::Response {
    MenuButton::new(text).config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside)).ui(ui, add).0
}

/// Whether `path` is a dictionary's own file, to edit, not a table or a Revit file a parameter was imported from.
fn is_dictionary_file(path: &str) -> bool {
    path.to_lowercase().ends_with(&format!(".{}", parameters::DICTIONARY_EXTENSION))
}

/// Where the Dictionaries tab remembers the one in use, while the window is closed.
fn pick_memory() -> UiId {
    UiId::new("project-parameters-dictionary")
}

/// The window's names for `cats`; a file's own names for categories, which a dictionary may hold, as they are.
fn category_names<'a>(cats: impl Iterator<Item = &'a String>) -> String {
    let names: Vec<String> =
        cats.map(|c| if PARAMETER_CATEGORIES.contains(&c.as_str()) { lang::category(c, true) } else { c.clone() }).collect();
    names.join(", ")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Parameters,
    Categories,
    Families,
    Transfer,
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
    read_only: bool,
    scope: Scope,
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
            read_only: p.read_only,
            scope: p.scope,
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
            "read_only": self.read_only,
            "scope": self.scope,
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
    scopes: BTreeMap<String, Scope>,
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
            "scopes": self.scopes,
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

/// A definition being edited in the open dictionary.
#[derive(Clone, Debug, PartialEq)]
struct DictionaryRow {
    checked: bool,
    definition: Definition,
}

/// A dictionary's file, open to edit: what's typed is staged, and Save writes it as one revision.
#[derive(Default)]
struct Open {
    path: String,
    dictionary: Dictionary,
    rows: Vec<DictionaryRow>,
    version: String,
}

impl Open {
    fn of(path: &str, dictionary: Dictionary) -> Open {
        let rows = dictionary.parameters.iter().map(|q| DictionaryRow { checked: false, definition: q.clone() }).collect();
        Open { path: path.to_string(), version: dictionary.version.clone(), dictionary, rows }
    }

    /// The `dictionary` command's parameters for what's staged: changed rows, and those taken out.
    fn staged(&self) -> Value {
        let added: Vec<&Definition> = self.rows.iter().map(|r| &r.definition).filter(|q| !self.dictionary.parameters.contains(q)).collect();
        let kept: BTreeSet<Guid> = self.rows.iter().filter_map(|r| r.definition.guid).collect();
        let gone: Vec<String> =
            self.dictionary.parameters.iter().filter_map(|q| q.guid).filter(|g| !kept.contains(g)).map(|g| g.to_string()).collect();
        let mut p = json!({});
        if !added.is_empty() {
            p["add"] = json!(added);
        }
        if !gone.is_empty() {
            p["remove"] = json!(gone);
        }
        if self.version != self.dictionary.version {
            p["version"] = json!(self.version);
        }
        p
    }

    fn dirty(&self) -> bool {
        self.staged().as_object().is_some_and(|o| !o.is_empty())
    }
}

/// A parameter to be added to the families checked.
#[derive(Clone, Debug, PartialEq)]
struct FamilyRow {
    checked: bool,
    name: String,
    /// The definition it's tied to; where it's from, for the table to say.
    definition: Option<Guid>,
    source: Option<String>,
    kind: Holds,
    group: String,
    instance: bool,
    value: String,
    formula: String,
}

impl FamilyRow {
    fn blank(name: String) -> FamilyRow {
        FamilyRow {
            checked: false,
            name,
            definition: None,
            source: None,
            kind: Holds::Text,
            group: String::new(),
            instance: false,
            value: String::new(),
            formula: String::new(),
        }
    }

    fn of_definition(q: &Definition, source: &str, scope: Scope) -> FamilyRow {
        FamilyRow {
            definition: q.guid,
            source: Some(source.to_string()),
            kind: parameters::family_kind(q.kind),
            group: q.group.clone(),
            instance: scope == Scope::Instance,
            ..FamilyRow::blank(q.name.clone())
        }
    }

    /// What `family_parameter_add` takes for it, or why what's typed isn't what it holds.
    fn param(&self, m: &Model) -> Result<Value, String> {
        let typed = self.value.trim();
        let value = match self.kind {
            _ if typed.is_empty() => Value::Null,
            Holds::Length => json!(m.units.parse(typed).ok_or_else(|| tf("{}: `{}` is not a length", &[&self.name, typed]))?),
            Holds::Number | Holds::Angle => {
                json!(
                    typed
                        .parse::<f64>()
                        .ok()
                        .filter(|x| x.is_finite())
                        .ok_or_else(|| tf("{}: `{}` is not a number", &[&self.name, typed]))?
                )
            }
            Holds::YesNo => json!(typed == "1"),
            Holds::Material | Holds::Text => json!(typed),
        };
        Ok(json!({
            "name": self.name.trim(),
            "kind": self.kind,
            "instance": self.instance,
            "value": value,
            "formula": self.formula,
            "group": self.group,
            "definition": self.definition,
        }))
    }

    /// A definition of it, for a table.
    fn definition_of(&self) -> Definition {
        Definition {
            guid: self.definition,
            name: self.name.clone(),
            kind: parameters::definition_kind(self.kind),
            group: self.group.clone(),
            scope: Some(if self.instance { Scope::Instance } else { Scope::Type }),
            ..Definition::default()
        }
    }
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
    families_checked: BTreeSet<String>,
    family_rows: Vec<FamilyRow>,
    hide_unchecked: bool,
    family_search: String,
    // Transfer.
    family: String,
    family_checked: BTreeSet<String>,
    family_to: BTreeSet<String>,
    // Dictionaries.
    open: Option<Open>,
    /// Each loaded dictionary's file, by its path: read when the tab opens, and none where it can't be read.
    loaded: Option<Arc<BTreeMap<String, Option<Dictionary>>>>,
    /// The name of the loaded dictionary in use.
    pick: Option<String>,
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
            families_checked: BTreeSet::new(),
            family_rows: Vec::new(),
            hide_unchecked: false,
            family_search: String::new(),
            family: m.families.keys().next().cloned().unwrap_or_default(),
            family_checked: BTreeSet::new(),
            family_to: BTreeSet::new(),
            open: None,
            loaded: None,
            pick: None,
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

    /// The loaded dictionaries' files, read once.
    fn loaded(&mut self, m: &Model) -> Arc<BTreeMap<String, Option<Dictionary>>> {
        let loaded = self.loaded.get_or_insert_with(|| {
            let read = |r: &parameters::DictionaryRef| table::read_dictionary_file(&r.path).ok();
            Arc::new(m.dictionaries.values().map(|r| (r.path.clone(), read(r))).collect())
        });
        Arc::clone(loaded)
    }

    /// The dictionary in use, as it's picked: the one picked before, else the first that's a dictionary's file. A Revit shared
    /// parameter file a parameter came from is listed as a source, and has nothing here to edit.
    fn settle_pick(&mut self, m: &Model, remembered: Option<String>) {
        let usable = |n: &String| m.dictionaries.get(n).is_some_and(|r| is_dictionary_file(&r.path));
        let pick = self.pick.clone().or(remembered).filter(usable).or_else(|| m.dictionaries.keys().find(|n| usable(n)).cloned());
        let path = pick.as_ref().and_then(|n| m.dictionaries.get(n)).map(|r| r.path.clone());
        if self.open.as_ref().map(|o| &o.path) != path.as_ref() {
            let loaded = self.loaded(m);
            let file = path.as_ref().and_then(|p| loaded.get(p).cloned().flatten());
            self.open = path.zip(file).map(|(p, f)| Open::of(&p, f));
        }
        self.pick = pick;
    }

    fn checked_names(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.checked && !r.deleted).filter_map(|r| r.original.clone()).collect()
    }

    /// How many new parameters Apply makes on the Families tab, over how many families: a name a family has isn't one.
    fn family_counts(&self, m: &Model) -> (usize, usize) {
        let families = self.families_checked.iter().filter_map(|n| m.families.get(n));
        let made =
            families.map(|f| self.family_rows.iter().filter(|r| !f.parameters.iter().any(|q| q.name == r.name.trim())).count()).sum();
        (made, self.families_checked.len())
    }

    /// What will happen to values on Apply, for each switch between instance and type.
    fn scope_warnings(&self, m: &Model) -> Vec<String> {
        let mut out = Vec::new();
        for r in self.rows.iter().filter(|r| !r.deleted) {
            let Some(p) = r.original.as_ref().and_then(|n| m.parameters.get(n)) else { continue };
            match (p.scope, r.scope) {
                (Scope::Instance, Scope::Type) if r.usage.values > 0 => {
                    out.push(tf("Apply wipes the values {} elements have of {}.", &[&r.usage.values.to_string(), &r.name]));
                }
                (Scope::Type, Scope::Instance) if !p.type_values.is_empty() => {
                    out.push(tf("Apply copies each type's value of {} to its elements.", &[&r.name]));
                }
                _ => {}
            }
        }
        out
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
        self.begin_review(&mut d, path, None, false);
        self.parameter_manager = Some(d);
    }

    fn begin_review(&mut self, d: &mut ParameterManager, path: &str, names: Option<Vec<String>>, replace: bool) {
        let mut review = Review {
            path: path.to_string(),
            report: Value::Null,
            map: BTreeMap::new(),
            resolve: BTreeMap::new(),
            scopes: BTreeMap::new(),
            categories: BTreeSet::new(),
            names,
            replace,
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

    /// Loads the dictionary at `path` in the project and puts it in use, making it if there is none yet.
    pub(crate) fn parameters_open_dictionary(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        let made = std::path::Path::new(path).exists() || self.manage(&mut d, "dictionary", json!({"path": path})).is_some();
        if made && let Some(r) = self.manage(&mut d, "dictionary_mount", json!({"path": path})) {
            d.pick = r["name"].as_str().map(String::from);
            d.open = None;
            d.loaded = None;
            d.tab = Tab::Dictionaries;
            d.message = None;
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

    /// Parameters read from a file, added to the Families tab's table, tied to their definitions.
    pub(crate) fn parameters_family_import(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        match table::read_file(path) {
            Ok(read) => {
                let source = read.dictionary.as_ref().map_or_else(|| table::stem(path), |s| s.name.clone());
                for q in &read.definitions {
                    d.family_rows.push(FamilyRow::of_definition(q, &source, q.scope.unwrap_or_default()));
                }
                d.message = None;
            }
            Err(e) => d.message = Some((e, true)),
        }
        self.parameter_manager = Some(d);
    }

    /// The Families tab's table, to a file.
    pub(crate) fn parameters_family_export(&mut self, path: &str) {
        let Some(mut d) = self.parameter_manager.take() else { return };
        let definitions: Vec<Definition> = d.family_rows.iter().map(FamilyRow::definition_of).collect();
        if let Some(r) = self.manage(&mut d, "parameter_export", json!({"path": path, "definitions": definitions})) {
            d.message = Some((tf("{} parameters exported.", &[&r["parameters"].to_string()]), false));
        }
        self.parameter_manager = Some(d);
    }

    /// A change to the open dictionary, with what's staged in it: its file written once, as one revision.
    fn dictionary_change(&mut self, d: &mut ParameterManager, path: &str, mut p: Value) {
        p["path"] = json!(path);
        if let Some(open) = &d.open
            && let (Some(staged), Some(to)) = (open.staged().as_object().cloned(), p.as_object_mut())
        {
            for (k, v) in staged {
                match (to.get_mut(&k), v) {
                    (Some(Value::Array(have)), Value::Array(more)) => have.extend(more),
                    (None, v) => {
                        to.insert(k, v);
                    }
                    _ => {}
                }
            }
        }
        if self.manage(d, "dictionary", p).is_some() {
            match table::read_dictionary_file(path) {
                Ok(dictionary) => {
                    if let Some(open) = &mut d.open {
                        let checked: BTreeSet<Option<Guid>> = open.rows.iter().filter(|r| r.checked).map(|r| r.definition.guid).collect();
                        *open = Open::of(path, dictionary);
                        for r in &mut open.rows {
                            r.checked = checked.contains(&r.definition.guid);
                        }
                    }
                    d.loaded = None;
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
                            (Tab::Transfer, msg!("Transfer")),
                            (Tab::Dictionaries, msg!("Dictionaries")),
                        ] {
                            if ui.selectable_label(d.tab == tab, t(label)).clicked() {
                                d.tab = tab;
                                d.message = None;
                                d.loaded = None;
                            }
                        }
                    });
                    ui.separator();
                    match d.tab {
                        Tab::Parameters => self.parameters_tab(ui, ctx, &mut d, &model),
                        Tab::Categories => categories_tab(ui, &mut d, &model),
                        Tab::Families => self.families_tab(ui, ctx, &mut d, &model),
                        Tab::Transfer => self.transfer_tab(ui, &mut d, &model),
                        Tab::Dictionaries => self.dictionaries_tab(ui, ctx, &mut d, &model),
                    }
                }
                if let Some((text, error)) = &d.message {
                    ui.add_space(4.0);
                    ui.label(if *error { RichText::new(text).color(pal().error) } else { RichText::new(text) });
                }
            });
            popup::footer(ui, |ui| {
                if d.tab == Tab::Families && d.review.is_none() {
                    let (made, families) = d.family_counts(&model);
                    ui.label(tf("{} parameters will be created in {} families", &[&made.to_string(), &families.to_string()]));
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        close = ui.button(t("Close")).clicked();
                        if ui.add_enabled(made > 0, Button::new(t("Apply"))).clicked() {
                            self.apply_family_rows(&mut d);
                        }
                        if ui.button(t("Clear")).clicked() {
                            d.family_rows.clear();
                            d.families_checked.clear();
                            d.message = None;
                        }
                    });
                    return;
                }
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
                    read_only: false,
                    scope: Scope::Instance,
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
            Grid::new("parameter-table").striped(true).num_columns(10).spacing([8.0, 4.0]).show(ui, |ui| {
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
                    msg!("Instance/Type"),
                    msg!("Group Under"),
                    msg!("Description"),
                    msg!("Editable"),
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
                    scope_combo(ui, ("parameter-scope", i), &mut r.scope, &tf("Instance or Type of {}", &[&label]));
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
                    let mut editable = !r.read_only;
                    if tick(ui, &mut editable, &tf("{} editable", &[&label])) {
                        r.read_only = !editable;
                    }
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
        for warning in d.scope_warnings(model) {
            ui.label(RichText::new(warning).color(pal().muted));
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
        ui.horizontal(|ui| {
            ui.label(t("Make All:"));
            for s in [Scope::Instance, Scope::Type] {
                if ui.button(t(scope_name(s))).clicked() {
                    for row in &rows {
                        review.scopes.insert(row["name"].as_str().unwrap_or("").to_string(), s);
                    }
                    again = true;
                }
            }
        });
        ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            Grid::new("parameter-review").striped(true).num_columns(6).spacing([10.0, 4.0]).show(ui, |ui| {
                for head in [msg!("Status"), msg!("In the File"), msg!("Here"), msg!("Instance/Type"), msg!("Decision"), msg!("Notes")] {
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
                    let mut scope: Scope = serde_json::from_value(row["scope"].clone()).unwrap_or_default();
                    if scope_combo(ui, ("review-scope", &name), &mut scope, &tf("Instance or Type of {}", &[&name])) {
                        review.scopes.insert(name.clone(), scope);
                        again = true;
                    }
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
                    wrapped(ui, notes.join("; "), 360.0);
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

    /// The Families tab's Apply: the table's parameters added to the families checked, at once.
    fn apply_family_rows(&mut self, d: &mut ParameterManager) {
        let model = Arc::clone(&self.session.model);
        let parameters: Result<Vec<Value>, String> = d.family_rows.iter().map(|r| r.param(&model)).collect();
        let parameters = match parameters {
            Ok(p) => p,
            Err(e) => {
                d.message = Some((e, true));
                return;
            }
        };
        if let Some(r) = self.manage(d, "family_parameter_add", json!({"families": d.families_checked, "parameters": parameters})) {
            let of = |k: &str| r["families"].as_array().map_or(0, |a| a.iter().map(|f| f[k].as_array().map_or(0, Vec::len)).sum::<usize>());
            d.message =
                Some((tf("{} added, {} skipped: their names are taken.", &[&of("added").to_string(), &of("skipped").to_string()]), false));
            d.family_rows.clear();
        }
    }

    /// The Families tab: new parameters for many families at once. The families on the left, what they get on the right.
    fn families_tab(&mut self, ui: &mut Ui, ctx: &Context, d: &mut ParameterManager, model: &Arc<Model>) {
        if model.families.is_empty() {
            ui.label(RichText::new(t("The project has no families.")).color(pal().muted));
            return;
        }
        ui.horizontal(|ui| {
            ui.menu_button(t("Add Parameter"), |ui| {
                if ui.button(t("Family Parameter")).clicked() {
                    ui.close();
                    let name = (1..)
                        .map(|n| format!("{} {n}", t("Parameter")))
                        .find(|n| !d.family_rows.iter().any(|r| r.name == *n))
                        .unwrap_or_default();
                    d.family_rows.push(FamilyRow::blank(name));
                }
                ui.menu_button(t("From a Project Parameter"), |ui| {
                    ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for name in model.parameters.keys() {
                            if ui.button(name).clicked() {
                                ui.close();
                                if let (Some(q), Some(p)) = (parameters::definition(model, name), model.parameters.get(name)) {
                                    let source = p.dictionary.clone().unwrap_or_else(|| t("Project").to_string());
                                    d.family_rows.push(FamilyRow::of_definition(&q, &source, p.scope));
                                }
                            }
                        }
                    });
                });
                let loaded = d.loaded(model);
                for (source, r) in &model.dictionaries {
                    let Some(file) = loaded.get(&r.path).and_then(Option::as_ref) else { continue };
                    let mut picked = None;
                    ui.menu_button(tf("From {}", &[source]), |ui| {
                        ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for q in &file.parameters {
                                if ui.button(&q.name).clicked() {
                                    ui.close();
                                    picked = Some(FamilyRow::of_definition(q, source, q.scope.unwrap_or_default()));
                                }
                            }
                        });
                    });
                    d.family_rows.extend(picked);
                }
            });
            ui.menu_button(t("Import/Export"), |ui| {
                if ui.button(t("Import...")).clicked() {
                    ui.close();
                    let dialog = rfd::AsyncFileDialog::new().add_filter(t("Parameters"), &["xlsx", "csv", "bimparams", "txt"]);
                    self.ask_file(ctx, dialog, false, |app, path| app.parameters_family_import(&path.to_string_lossy()));
                }
                if ui.add_enabled(!d.family_rows.is_empty(), Button::new(t("Export..."))).clicked() {
                    ui.close();
                    let dialog = rfd::AsyncFileDialog::new()
                        .add_filter(t("Excel workbook"), &["xlsx"])
                        .add_filter(t("CSV table"), &["csv"])
                        .add_filter(t("Dictionary"), &[parameters::DICTIONARY_EXTENSION])
                        .set_file_name("family parameters.xlsx");
                    self.ask_file(ctx, dialog, true, |app, path| app.parameters_family_export(&path.to_string_lossy()));
                }
            });
            ui.menu_button(t("Batch Actions"), |ui| {
                let any = d.family_rows.iter().any(|r| r.checked);
                if ui.add_enabled(any, Button::new(t("Duplicate Checked"))).clicked() {
                    ui.close();
                    let copies: Vec<FamilyRow> = d
                        .family_rows
                        .iter()
                        .filter(|r| r.checked)
                        .map(|r| FamilyRow {
                            checked: false,
                            name: format!("{} {}", r.name, t("Copy")),
                            definition: None,
                            source: None,
                            ..r.clone()
                        })
                        .collect();
                    d.family_rows.extend(copies);
                }
                if ui.add_enabled(any, Button::new(t("Delete Checked"))).clicked() {
                    ui.close();
                    d.family_rows.retain(|r| !r.checked);
                }
                for (instance, label) in [(true, msg!("Make Checked Instance")), (false, msg!("Make Checked Type"))] {
                    if ui.add_enabled(any, Button::new(t(label))).clicked() {
                        ui.close();
                        d.family_rows.iter_mut().filter(|r| r.checked).for_each(|r| r.instance = instance);
                    }
                }
            });
            ui.checkbox(&mut d.hide_unchecked, t("Hide Unchecked"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                field(ui, &mut d.family_search, 180.0, t("Search parameters"));
                ui.label(t("Search:"));
            });
        });
        ui.add_space(4.0);
        sides(ui, |ui| {
            ui.allocate_ui_with_layout(vec2(280.0, SIDES), Layout::top_down(Align::Min), |ui| {
                ui.set_width(280.0);
                ScrollArea::vertical().id_salt("family-list").max_height(SIDES).auto_shrink([false, false]).show(ui, |ui| {
                    Grid::new("family-table").striped(true).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                        let mut every = model.families.keys().all(|n| d.families_checked.contains(n));
                        if tick(ui, &mut every, t("Check All Families")) {
                            d.families_checked = if every { model.families.keys().cloned().collect() } else { BTreeSet::new() };
                        }
                        ui.label(RichText::new(t("Family Name")).strong());
                        ui.label(RichText::new(t("Category Name")).strong());
                        ui.end_row();
                        for (name, f) in &model.families {
                            let mut on = d.families_checked.contains(name);
                            if d.hide_unchecked && !on {
                                continue;
                            }
                            if tick(ui, &mut on, &tf("Check family {}", &[name])) {
                                if on {
                                    d.families_checked.insert(name.clone());
                                } else {
                                    d.families_checked.remove(name);
                                }
                            }
                            wrapped(ui, name.as_str(), 130.0);
                            wrapped(ui, lang::category(parameters::family_category(f.category), true), 80.0);
                            ui.end_row();
                        }
                    });
                });
            });
            ui.separator();
            let needle = d.family_search.trim().to_lowercase();
            let width = ui.available_width();
            ui.allocate_ui_with_layout(vec2(width, SIDES), Layout::top_down(Align::Min), |ui| {
                ScrollArea::both().id_salt("family-new").max_height(SIDES).auto_shrink([false, false]).show(ui, |ui| {
                    Grid::new("family-new-table").striped(true).num_columns(8).spacing([8.0, 4.0]).show(ui, |ui| {
                        let mut every = !d.family_rows.is_empty() && d.family_rows.iter().all(|r| r.checked);
                        if tick(ui, &mut every, t("Check All Parameters")) {
                            d.family_rows.iter_mut().for_each(|r| r.checked = every);
                        }
                        for head in [
                            msg!("Parameter Name"),
                            msg!("Source"),
                            msg!("Type of Parameter"),
                            msg!("Group Under"),
                            msg!("Instance/Type"),
                            msg!("Value"),
                            msg!("Formula"),
                        ] {
                            ui.label(RichText::new(t(head)).strong());
                        }
                        ui.end_row();
                        for (i, r) in d.family_rows.iter_mut().enumerate() {
                            if !needle.is_empty() && !r.name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            let label = r.name.clone();
                            tick(ui, &mut r.checked, &tf("Check {}", &[&label]));
                            // Tied to a definition, its name and what it holds are the definition's.
                            let tied = r.definition.is_some();
                            if tied {
                                ui.label(&r.name);
                            } else {
                                field(ui, &mut r.name, 120.0, &tf("Name of {}", &[&label]));
                            }
                            match &r.source {
                                Some(n) if n == t("Project") => wrapped(ui, n.as_str(), 80.0),
                                Some(n) => wrapped(ui, tf("Shared: {}", &[n]), 80.0),
                                None => wrapped(ui, t("Family"), 80.0),
                            }
                            if tied {
                                ui.label(t(family_kind_name(r.kind)));
                            } else {
                                let kinds = ComboBox::from_id_salt(("family-kind", i)).selected_text(t(family_kind_name(r.kind))).width(84.0).show_ui(ui, |ui| {
                                    for (k, name) in FAMILY_KINDS {
                                        ui.selectable_value(&mut r.kind, k, t(name));
                                    }
                                });
                                let kind_label = tf("Type of {}", &[&label]);
                                kinds.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, kind_label.clone()));
                            }
                            field(ui, &mut r.group, 80.0, &tf("Group of {}", &[&label]));
                            let mut scope = if r.instance { Scope::Instance } else { Scope::Type };
                            if scope_combo(ui, ("family-scope", i), &mut scope, &tf("Instance or Type of {}", &[&label])) {
                                r.instance = scope == Scope::Instance;
                            }
                            if r.kind == Holds::YesNo {
                                let mut on = r.value == "1";
                                if tick(ui, &mut on, &tf("Value of {}", &[&label])) {
                                    r.value = (if on { "1" } else { "0" }).to_string();
                                }
                            } else {
                                field(ui, &mut r.value, 70.0, &tf("Value of {}", &[&label]));
                            }
                            if matches!(r.kind, Holds::Text | Holds::Material) {
                                ui.label("");
                            } else {
                                field(ui, &mut r.formula, 100.0, &tf("Formula of {}", &[&label]));
                            }
                            ui.end_row();
                        }
                    });
                    if d.family_rows.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new(t("Add Parameter makes one for the families checked. A type parameter's value goes on all their types; each type can then change it in Type Properties.")).color(pal().muted));
                    }
                });
            });
        });
    }

    /// The Transfer tab: parameters a family has, copied to other families.
    fn transfer_tab(&mut self, ui: &mut Ui, d: &mut ParameterManager, model: &Arc<Model>) {
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
            ScrollArea::vertical().id_salt("family-targets").max_height(250.0).auto_shrink([false, false]).show(right, |ui| {
                Grid::new("family-target-table").striped(true).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new(t("Family Name")).strong());
                    ui.label(RichText::new(t("Category Name")).strong());
                    ui.end_row();
                    for (name, f) in model.families.iter().filter(|(n, _)| **n != d.family) {
                        let mut on = d.family_to.contains(name);
                        if tick(ui, &mut on, &tf("Transfer to {}", &[name])) {
                            if on {
                                d.family_to.insert(name.clone());
                            } else {
                                d.family_to.remove(name);
                            }
                        }
                        wrapped(ui, name.as_str(), 220.0);
                        wrapped(ui, lang::category(parameters::family_category(f.category), true), 100.0);
                        ui.end_row();
                    }
                });
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
            let transfer = right.horizontal(|ui| ui.add_enabled(ready, Button::new(t("Transfer Checked"))).clicked()).inner;
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
        let remembered = ctx.data(|m| m.get_temp::<String>(pick_memory()));
        d.settle_pick(model, remembered);
        ctx.data_mut(|m| match &d.pick {
            Some(n) => {
                m.insert_temp(pick_memory(), n.clone());
            }
            None => {
                m.remove_temp::<String>(pick_memory());
            }
        });
        ui.label(RichText::new(t("In This Project")).strong());
        if model.dictionaries.is_empty() {
            ui.label(RichText::new(t("No dictionary is loaded in this project yet.")).color(pal().muted));
        }
        let loaded = d.loaded(model);
        let mut update: Option<String> = None;
        ScrollArea::vertical().id_salt("dictionaries-used-rows").max_height(110.0).auto_shrink([false, true]).show(ui, |ui| {
            Grid::new("dictionaries-used").striped(true).num_columns(5).spacing([12.0, 4.0]).show(ui, |ui| {
                if !model.dictionaries.is_empty() {
                    for head in [msg!("Dictionary"), msg!("Label"), msg!("Revision"), msg!("File"), ""] {
                        ui.label(RichText::new(if head.is_empty() { "" } else { t(head) }).strong());
                    }
                    ui.end_row();
                }
                for (name, r) in &model.dictionaries {
                    ui.label(name);
                    ui.label(&r.version);
                    let file = loaded.get(&r.path).and_then(Option::as_ref).map(|f| f.revision);
                    let said = match file {
                        None if !is_dictionary_file(&r.path) => RichText::new(t("Imported from this file")).color(pal().muted),
                        None => RichText::new(tf("{}; the file can't be read", &[&r.revision.to_string()])).color(pal().error),
                        Some(_) if r.revision == 0 => RichText::new(t("Loaded; nothing taken from it yet")).color(pal().muted),
                        Some(n) if n > r.revision => {
                            RichText::new(tf("This project has {}, the file {}", &[&r.revision.to_string(), &n.to_string()]))
                                .color(pal().error)
                        }
                        Some(_) => RichText::new(tf("{}, up to date", &[&r.revision.to_string()])),
                    };
                    ui.label(said);
                    wrapped(ui, r.path.as_str(), 320.0);
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
        });
        if let Some(path) = update {
            self.begin_review(d, &path, None, false);
            return;
        }
        ui.separator();
        // Unsaved edits would be lost by showing another: Save Dictionary first.
        let dirty = d.open.as_ref().is_some_and(Open::dirty);
        let mut remove: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label(t("Dictionary:"));
            let names: Vec<&String> = model.dictionaries.iter().filter(|(_, r)| is_dictionary_file(&r.path)).map(|(n, _)| n).collect();
            ui.add_enabled_ui(!dirty && !names.is_empty(), |ui| {
                let shown = d.pick.clone().unwrap_or_default();
                let combo = ComboBox::from_id_salt("dictionary-in-use").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                    for n in &names {
                        let tip = model.dictionaries.get(*n).map(|r| r.path.as_str()).unwrap_or_default();
                        if ui.selectable_label(d.pick.as_ref() == Some(*n), n.as_str()).on_hover_text(tip).clicked() {
                            d.pick = Some((*n).clone());
                            d.message = None;
                        }
                    }
                });
                combo.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Dictionary in use")));
                if dirty {
                    combo.response.on_disabled_hover_text(t("Save Dictionary first: there are changes not saved."));
                }
            });
            let filter = parameters::DICTIONARY_EXTENSION;
            if ui.add_enabled(!dirty, Button::new(t("New Dictionary..."))).clicked() {
                let dialog =
                    rfd::AsyncFileDialog::new().add_filter(t("Dictionary"), &[filter]).set_file_name(format!("parameters.{filter}"));
                self.ask_file(ctx, dialog, true, |app, path| app.parameters_open_dictionary(&path.to_string_lossy()));
            }
            if ui.add_enabled(!dirty, Button::new(t("Open Dictionary..."))).clicked() {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Dictionary"), &[filter]);
                self.ask_file(ctx, dialog, false, |app, path| app.parameters_open_dictionary(&path.to_string_lossy()));
            }
            if ui
                .add_enabled(!dirty && d.pick.is_some(), Button::new(t("Remove from Project")))
                .on_hover_text(t("The project stops loading this dictionary. Its file stays where it is."))
                .clicked()
            {
                remove = d.pick.clone();
            }
        });
        if let Some(name) = remove {
            if self.manage(d, "dictionary_mount", json!({"name": name, "remove": true})).is_some() {
                d.pick = None;
                d.open = None;
                d.loaded = None;
                d.message = None;
            }
            return;
        }
        // The one picked may have just changed: the next frame shows it.
        if d.open.as_ref().map(|o| o.path.as_str()) != d.pick.as_ref().and_then(|n| model.dictionaries.get(n)).map(|r| r.path.as_str()) {
            ui.ctx().request_repaint();
        }
        let Some(mut open) = d.open.take() else {
            let said = match d.pick.as_ref().and_then(|n| model.dictionaries.get(n)) {
                Some(r) => RichText::new(tf("{} can't be read. Open Dictionary finds it again where it is now.", &[&r.path])).color(pal().error),
                None => RichText::new(t("A dictionary is a file of parameter definitions to share between projects: keep it on a shared drive or in a Git repository.")).color(pal().muted),
            };
            ui.label(said);
            return;
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tf("Revision {}, changed {}", &[&open.dictionary.revision.to_string(), &open.dictionary.changed]))
                .on_hover_text(t("Counts up by itself each time the file is saved, so a project can tell it is behind."));
            ui.label(t("Label:")).on_hover_text(t("A name of your own for this state, such as 2026 standard. Nothing compares it."));
            field(ui, &mut open.version, 120.0, t("Label"));
        });
        let path = open.path.clone();
        let mut change: Option<Value> = None;
        let mut take: Option<(Vec<String>, bool)> = None;
        ui.horizontal_wrapped(|ui| {
            let any = open.rows.iter().any(|r| r.checked);
            check_menu(ui, t("Edit"), |ui| {
                if ui.button(t("Add Parameter")).clicked() {
                    ui.close();
                    let name = (1..)
                        .map(|n| format!("{} {n}", t("Parameter")))
                        .find(|n| !open.rows.iter().any(|r| r.definition.name == *n))
                        .unwrap_or_default();
                    open.rows.push(DictionaryRow {
                        checked: false,
                        definition: Definition { name, scope: Some(Scope::Instance), ..Definition::default() },
                    });
                }
                let checked_project = d.checked_names();
                if ui
                    .add_enabled(!checked_project.is_empty(), Button::new(t("Add Checked Project Parameters")))
                    .on_disabled_hover_text(t("Check them on the Parameters tab first."))
                    .clicked()
                {
                    ui.close();
                    change = Some(json!({"names": checked_project, "skip_conflicts": true}));
                }
                if ui.button(t("Merge File...")).clicked() {
                    ui.close();
                    let dialog = rfd::AsyncFileDialog::new().add_filter(t("Parameters"), &["xlsx", "csv", "bimparams", "txt"]);
                    self.ask_file(ctx, dialog, false, |app, path| app.parameters_merge_into_dictionary(&path.to_string_lossy()));
                }
                if ui.add_enabled(any, Button::new(t("Remove Checked"))).clicked() {
                    ui.close();
                    open.rows.retain(|r| !r.checked);
                }
                ui.add_enabled_ui(any, |ui| {
                    ui.menu_button(t("Categories of Checked"), |ui| {
                        ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for cat in PARAMETER_CATEGORIES {
                                let has = |r: &DictionaryRow| r.definition.categories.iter().any(|c| c == cat);
                                let checked = || open.rows.iter().filter(|r| r.checked);
                                let (all, some) = (checked().all(has), checked().any(has));
                                let mut on = all;
                                if ui.add(egui::Checkbox::new(&mut on, lang::category(cat, true)).indeterminate(some && !all)).changed() {
                                    for r in open.rows.iter_mut().filter(|r| r.checked) {
                                        r.definition.categories.retain(|c| c != cat);
                                        if on {
                                            r.definition.categories.push(cat.to_string());
                                        }
                                        r.definition.categories.sort();
                                    }
                                }
                            }
                        });
                    });
                });
            });
            if ui.add_enabled(open.dirty(), Button::new(t("Save Dictionary"))).clicked() {
                change = Some(json!({}));
            }
            let saved = !open.dirty();
            if ui.add_enabled(any && saved, Button::new(t("Add Checked to Project"))).clicked() {
                take = Some((open.rows.iter().filter(|r| r.checked).map(|r| r.definition.name.clone()).collect(), false));
            }
            let here: Vec<String> = open
                .rows
                .iter()
                .filter(|r| r.definition.guid.is_some_and(|g| parameters::here_by_guid(model, g).is_some()))
                .map(|r| r.definition.name.clone())
                .collect();
            if ui
                .add_enabled(saved && !here.is_empty(), Button::new(t("Update Project")))
                .on_hover_text(t(
                    "Show what the project's copies of these parameters would change to match this dictionary, categories too.",
                ))
                .clicked()
            {
                take = Some((here, true));
            }
        });
        ScrollArea::both().id_salt("dictionary-rows").max_height(280.0).auto_shrink([false, false]).show(ui, |ui| {
            Grid::new("dictionary-table").striped(true).num_columns(8).spacing([8.0, 4.0]).show(ui, |ui| {
                let mut every = !open.rows.is_empty() && open.rows.iter().all(|r| r.checked);
                if tick(ui, &mut every, t("Check All Definitions")) {
                    open.rows.iter_mut().for_each(|r| r.checked = every);
                }
                for head in [
                    msg!("Name"),
                    msg!("Type of Parameter"),
                    msg!("Instance/Type"),
                    msg!("Group Under"),
                    msg!("Description"),
                    msg!("Editable"),
                    msg!("Categories"),
                ] {
                    ui.label(RichText::new(t(head)).strong());
                }
                ui.end_row();
                for (i, r) in open.rows.iter_mut().enumerate() {
                    dictionary_row(ui, i, r);
                }
            });
        });
        d.open = Some(open);
        if let Some(p) = change {
            self.dictionary_change(d, &path, p);
        }
        if let Some((names, replace)) = take {
            self.begin_review(d, &path, Some(names), replace);
        }
    }
}

/// A row of the open dictionary's table, every cell to edit.
fn dictionary_row(ui: &mut Ui, i: usize, r: &mut DictionaryRow) {
    let label = r.definition.name.clone();
    let q = &mut r.definition;
    tick(ui, &mut r.checked, &tf("Check {}", &[&label]));
    let name = field(ui, &mut q.name, 150.0, &tf("Name of {}", &[&label]));
    if let Some(g) = q.guid {
        name.on_hover_text(g.to_string());
    }
    let kinds = ComboBox::from_id_salt(("dictionary-kind", i)).selected_text(t(kind_name(q.kind))).width(80.0).show_ui(ui, |ui| {
        for (k, name) in KINDS {
            ui.selectable_value(&mut q.kind, k, t(name));
        }
    });
    let kind_label = tf("Type of {}", &[&label]);
    kinds.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, kind_label.clone()));
    let mut scope = q.scope.unwrap_or_default();
    if scope_combo(ui, ("dictionary-scope", i), &mut scope, &tf("Instance or Type of {}", &[&label])) {
        q.scope = Some(scope);
    }
    field(ui, &mut q.group, 90.0, &tf("Group of {}", &[&label]));
    field(ui, &mut q.description, 130.0, &tf("Description of {}", &[&label]));
    let mut editable = !q.read_only;
    if tick(ui, &mut editable, &tf("{} editable", &[&label])) {
        q.read_only = !editable;
    }
    ui.horizontal_top(|ui| {
        let categories_label = tf("Categories of {}", &[&label]);
        let menu = check_menu(ui, "...", |ui| {
            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for cat in PARAMETER_CATEGORIES {
                    let mut on = q.categories.iter().any(|c| c == cat);
                    if ui.checkbox(&mut on, lang::category(cat, true)).changed() {
                        q.categories.retain(|c| c != cat);
                        if on {
                            q.categories.push(cat.to_string());
                        }
                        q.categories.sort();
                    }
                }
            });
        });
        menu.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, categories_label.clone()));
        wrapped(ui, category_names(q.categories.iter()), 200.0);
    });
    ui.end_row();
}

/// The Categories tab: the parameters checked on the left, what they're for on the right. Staged with the rest, until Apply.
fn categories_tab(ui: &mut Ui, d: &mut ParameterManager, model: &Arc<Model>) {
    let _ = model;
    sides(ui, |ui| {
        // The table's width is its own, whatever its rows say: long lists of categories break onto new lines.
        ui.allocate_ui_with_layout(vec2(CATEGORY_TABLE, SIDES), Layout::top_down(Align::Min), |left| {
            left.set_width(CATEGORY_TABLE);
            category_table(left, d);
        });
        ui.separator();
        let width = ui.available_width();
        ui.allocate_ui_with_layout(vec2(width, SIDES), Layout::top_down(Align::Min), |right| {
            right.set_width(width);
            category_picker(right, d);
        });
    });
}

/// How wide the Categories tab's table is.
const CATEGORY_TABLE: f32 = 540.0;

/// The Categories tab's left side: the parameters as a table.
fn category_table(left: &mut Ui, d: &mut ParameterManager) {
    ScrollArea::vertical().id_salt("category-parameters").max_height(SIDES).auto_shrink([false, false]).show(left, |ui| {
        Grid::new("category-parameter-table").striped(true).num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
            let mut every = !d.rows.is_empty() && d.rows.iter().filter(|r| !r.deleted).all(|r| r.checked);
            if tick(ui, &mut every, t("Check All")) {
                d.rows.iter_mut().filter(|r| !r.deleted).for_each(|r| r.checked = every);
            }
            for head in [msg!("Name"), msg!("Source"), msg!("Type of Parameter"), msg!("Categories")] {
                ui.label(RichText::new(t(head)).strong());
            }
            ui.end_row();
            for r in d.rows.iter_mut().filter(|r| !r.deleted) {
                let label = r.name.clone();
                tick(ui, &mut r.checked, &tf("Check {}", &[&label]));
                wrapped(ui, label.as_str(), 120.0);
                wrapped(ui, r.dictionary.as_ref().map_or_else(|| t("Project").to_string(), |n| tf("Shared: {}", &[n])), 70.0);
                ui.label(t(kind_name(r.kind)));
                wrapped(ui, category_names(r.categories.iter()), 170.0);
                ui.end_row();
            }
        });
    });
}

/// The Categories tab's right side: what the parameters checked are for.
fn category_picker(right: &mut Ui, d: &mut ParameterManager) {
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
}
