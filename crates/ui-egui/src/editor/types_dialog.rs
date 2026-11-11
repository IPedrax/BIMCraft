//! Revit's Family Types dialog: the family's types in a drop-down with New Type, Rename Type and Delete Type beside it, a
//! search box, and one table of every parameter (Parameter, Value, Formula, Lock) under its group's heading, in the order the
//! Properties palette lists groups, each heading folding its rows. A type parameter's value is the type's, an instance
//! parameter's (marked "(default)") the value a new instance starts with; a value a formula works out is greyed, showing what
//! it comes to. Under the table New, Edit and Delete Parameter, Move Up and Move Down within a group, and sorting every group.
//! Everything goes to the draft, so the dialog's Apply, OK and Cancel (and the document's history) take it as one.
//! Lock is shown and disabled: locked labelled dimensions arrive with F7. Lookup tables and type catalogs are not here.
use super::*;
use crate::properties::group_header;
use bimcraft_core::family::{Group, Val};
use bimcraft_core::ids::Guid;
use bimcraft_core::shared_parameters::SharedFile;
use std::collections::BTreeSet;

/// The widths of the table's columns: a parameter's name, its value, its formula and its lock.
const NAME_W: f32 = 230.0;
const VALUE_W: f32 = 150.0;
const FORMULA_W: f32 = 270.0;
const LOCK_W: f32 = 40.0;

/// Lock's tooltip: its behaviour arrives with locked labelled dimensions.
const LOCK_TIP: &str = msg!("Locking a parameter's value comes with labelled dimensions: not yet in BIMCraft.");

/// What Family Types keeps while it is open, none of it the draft's: the search, the groups folded, the row selected, and the
/// Name and Parameter Properties dialogs it opens over itself.
#[derive(Default)]
pub(crate) struct TypesDialog {
    search: String,
    collapsed: BTreeSet<Group>,
    /// The parameter selected, by name: what Edit, Delete, Move Up and Move Down act on.
    selected: Option<String>,
    naming: Option<Naming>,
    editing: Option<Editing>,
    /// Why the last thing asked was refused (the last type deleted, a parameter in use).
    said: Option<String>,
    /// Why Apply or OK was refused (a formula that doesn't read), while the draft is as it was then.
    refused: Option<(super::document::Draft, String)>,
}

/// The Name dialog, for New Type (a copy of the type shown) or Rename Type.
struct Naming {
    rename: bool,
    name: String,
    error: Option<String>,
}

/// A shared parameters file's parameter: its GUID, name, kind and, for a family type, its category.
type Listed = (Guid, String, Holds, Option<Category>);

/// Parameter Properties, for New Parameter (`was` none) or Edit Parameter.
struct Editing {
    was: Option<String>,
    name: String,
    kind: Holds,
    instance: bool,
    group: Group,
    reporting: bool,
    of: Option<Category>,
    /// A shared parameter's GUID: one picked from the shared parameters file, or the one it had.
    shared: Option<Guid>,
    /// Family parameter or shared parameter.
    is_shared: bool,
    /// The shared parameters file's path, and what was read from it: each parameter's GUID, name and kind, or why not.
    file: String,
    read: Option<(String, Result<Vec<Listed>, String>)>,
    error: Option<String>,
}

/// The heading a parameter as typed is listed under.
fn group_of(p: &ParamText) -> Group {
    Parameter { kind: p.kind, group: p.original.group, ..Parameter::default() }.group()
}

/// The heading a parameter of `kind` is listed under when it says none.
fn kind_group(kind: Holds) -> Group {
    Parameter { kind, ..Parameter::default() }.group()
}

/// Whether `name` is one the search keeps: case aside, it holds what is typed.
fn found(search: &str, name: &str) -> bool {
    let s = search.trim().to_lowercase();
    s.is_empty() || name.to_lowercase().contains(&s)
}

impl FamilyEditor {
    /// The parameter named `name`'s place.
    fn param_at(&self, name: &str) -> Option<usize> {
        self.params.iter().position(|p| p.name.trim() == name)
    }

    /// Parameter `old` renamed `new` wherever the draft names it, as `Family::rename_parameter` walks a family: its formulas,
    /// forms, symbols, connectors and types. A draft that doesn't build yet has the parameter and its types' values renamed.
    fn rename_everywhere(&mut self, old: &str, new: &str) {
        match self.family() {
            Ok(mut f) => {
                f.rename_parameter(old, new);
                let e = FamilyEditor::of(&self.name, &f);
                (self.params, self.types, self.forms, self.symbols) = (e.params, e.types, e.forms, e.symbols);
                (self.adaptive, self.connectors, self.planes, self.lines) = (e.adaptive, e.connectors, e.planes, e.lines);
                self.options = e.options;
            }
            Err(_) => {
                for p in self.params.iter_mut().filter(|p| p.name.trim() == old) {
                    p.name = new.to_string();
                }
                for (_, set) in &mut self.types {
                    if let Some(v) = set.remove(old) {
                        set.insert(new.to_string(), v);
                    }
                }
            }
        }
    }

    /// Delete Parameter: the `family` command's `delete_parameter` on a private session holding the draft (as Save Family
    /// prepares one), so it is refused, with the command's reason, while a formula or a form uses it; then taken from the
    /// draft with its types' values, as the command takes it.
    fn delete_parameter(&mut self, base: &Arc<Model>, name: &str) -> Result<(), String> {
        let p = self.command(base)?;
        let mut s = Session::new();
        s.model = Arc::clone(base);
        s.execute("family", &p)?;
        s.execute("family", &json!({"name": self.name.trim(), "delete_parameter": [name]}))?;
        self.params.retain(|p| p.name.trim() != name);
        for (_, set) in &mut self.types {
            set.remove(name);
        }
        Ok(())
    }

    /// Why Apply or OK can't take the draft: what T9 says of a parameter's formula that doesn't read or names what isn't
    /// there, as the `family` command would refuse it.
    pub(super) fn formula_refusal(&self) -> Option<String> {
        let f = self.family().ok()?;
        let named: Vec<String> =
            self.params.iter().filter(|p| !p.formula.trim().is_empty()).map(|p| format!("parameter `{}`", p.name.trim())).collect();
        let (errors, _) = f.problems(&|_| true);
        let bad: Vec<String> = errors.into_iter().filter(|e| named.iter().any(|n| e.starts_with(n.as_str()))).collect();
        (!bad.is_empty()).then(|| bad.join("\n"))
    }

    /// Parameter `name`'s formula set to `formula` in the draft, as a file from before T9 may hold one that Family Types refuses.
    #[cfg(test)]
    pub(crate) fn set_formula(&mut self, name: &str, formula: &str) {
        if let Some(i) = self.param_at(name) {
            self.params[i].formula = formula.to_string();
        }
    }

    /// The dialog refused Apply or OK for `why`: said in it until the draft changes.
    pub(super) fn refuse_types(&mut self, why: String) {
        self.types_dialog.refused = Some((self.draft(), why));
    }

    /// Family Types closed: what it opened over itself goes with it.
    pub(super) fn types_closed(&mut self) {
        let d = &mut self.types_dialog;
        (d.naming, d.editing, d.said, d.refused) = (None, None, None, None);
    }

    /// Move Up (`by` -1) or Move Down (+1): the parameter selected swaps places with the one before or after it in its group.
    fn move_selected(&mut self, by: isize) {
        let Some(at) = self.types_dialog.selected.as_deref().and_then(|n| self.param_at(n)) else { return };
        let g = group_of(&self.params[at]);
        let same: Vec<usize> = (0..self.params.len()).filter(|i| group_of(&self.params[*i]) == g).collect();
        let Some(k) = same.iter().position(|i| *i == at) else { return };
        if let Some(other) = k.checked_add_signed(by).and_then(|k| same.get(k)) {
            self.params.swap(at, *other);
        }
    }

    /// Sort Ascending or Descending: each group's parameters by name, each group keeping the places it had.
    fn sort_groups(&mut self, ascending: bool) {
        for g in Group::ALL {
            let places: Vec<usize> = (0..self.params.len()).filter(|i| group_of(&self.params[*i]) == g).collect();
            let mut rows: Vec<ParamText> = places.iter().map(|i| self.params[*i].clone()).collect();
            rows.sort_by_key(|p| p.name.trim().to_lowercase());
            if !ascending {
                rows.reverse();
            }
            for (i, p) in places.into_iter().zip(rows) {
                self.params[i] = p;
            }
        }
    }
}

/// Family Types' body: the Type name row, the search, the table and the buttons under it, and what was refused.
pub(super) fn body(ed: &mut FamilyEditor, ui: &mut Ui, base: &Arc<Model>, materials: &[String]) {
    ed.picked = ed.picked.min(ed.types.len().saturating_sub(1));
    type_row(ed, ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let l = ui.label(t("Search parameters"));
        let r = ui.add(TextEdit::singleline(&mut ed.types_dialog.search).hint_text(t("Search…")).desired_width(260.0));
        r.labelled_by(l.id);
    });
    if ed.category.hosted() {
        let note = t("A door or a window needs lengths named Width and Height; its formulas can read Wall Thickness, the wall's.");
        ui.label(RichText::new(note).color(pal().muted));
    }
    ui.separator();
    table(ed, ui, materials);
    ui.add_space(6.0);
    buttons(ed, ui, base);
    // Said in the dialog: the view's would be under it. One line, the most pressing.
    let refused = ed.types_dialog.refused.as_ref().filter(|(d, _)| *d == ed.draft()).map(|(_, w)| w.clone());
    if refused.is_none() {
        ed.types_dialog.refused = None;
    }
    if let Some(p) = refused.or_else(|| ed.types_dialog.said.clone()).or_else(|| ed.problem().map(str::to_string)) {
        ui.label(RichText::new(p).color(pal().error));
    }
}

/// Type name: the types in a drop-down, the one shown picked, with New Type, Rename Type and Delete Type beside it.
fn type_row(ed: &mut FamilyEditor, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(t("Type name:"));
        let names: Vec<String> = ed.types.iter().map(|(n, _)| n.clone()).collect();
        let now = names.get(ed.picked).cloned().unwrap_or_default();
        if let Some(n) = pick(ui, t("Type Name").to_string(), &now, &names)
            && let Some(i) = names.iter().position(|x| *x == n)
        {
            ed.picked = i;
        }
        let d = &mut ed.types_dialog;
        if ui.button(t("New Type")).on_hover_text(t("A new type, a copy of the type shown")).clicked() {
            let name = (2..).map(|k| format!("{now} {k}")).find(|x| !names.contains(x)).unwrap_or_default();
            (d.naming, d.said) = (Some(Naming { rename: false, name, error: None }), None);
        }
        if ui.add_enabled(!ed.types.is_empty(), egui::Button::new(t("Rename Type"))).clicked() {
            (d.naming, d.said) = (Some(Naming { rename: true, name: now.clone(), error: None }), None);
        }
        if ui.add_enabled(!ed.types.is_empty(), egui::Button::new(t("Delete Type"))).clicked() {
            if ed.types.len() <= 1 {
                d.said = Some(t("The last type can't be deleted: a family needs a type to place.").to_string());
            } else {
                d.said = None;
                ed.types.remove(ed.picked);
                ed.picked = ed.picked.min(ed.types.len() - 1);
            }
        }
    });
}

/// The table: a heading per group with rows, in the palette's order, each heading folding its rows.
fn table(ed: &mut FamilyEditor, ui: &mut Ui, materials: &[String]) {
    let values = ed.values();
    let h = ui.spacing().interact_size.y;
    Grid::new("family-types-head").num_columns(4).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        for (head, w) in [(msg!("Parameter"), NAME_W), (msg!("Value"), VALUE_W), (msg!("Formula"), FORMULA_W), (msg!("Lock"), LOCK_W)] {
            ui.add_sized([w, h], egui::Label::new(RichText::new(t(head)).strong()));
        }
        ui.end_row();
    });
    let search = ed.types_dialog.search.clone();
    let mut select = None;
    let mut shown = false;
    for g in Group::ALL {
        let rows: Vec<usize> =
            (0..ed.params.len()).filter(|i| group_of(&ed.params[*i]) == g && found(&search, &ed.params[*i].name)).collect();
        if rows.is_empty() {
            continue;
        }
        shown = true;
        let open = !ed.types_dialog.collapsed.contains(&g);
        if group_header(ui, t(g.label()), open) && !ed.types_dialog.collapsed.remove(&g) {
            ed.types_dialog.collapsed.insert(g);
        }
        if !open {
            continue;
        }
        Grid::new(("family-types-group", g.label())).num_columns(4).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            for i in rows {
                if row(ed, ui, i, &values, materials) {
                    select = Some(ed.params[i].name.trim().to_string());
                }
                ui.end_row();
            }
        });
    }
    if !shown {
        ui.label(RichText::new(t("Nothing matches the search.")).color(pal().muted));
    }
    if select.is_some() {
        ed.types_dialog.selected = select;
    }
}

/// Row `i` of the table: its name (selecting it), its value, its formula and its lock. Whether its name was clicked.
fn row(ed: &mut FamilyEditor, ui: &mut Ui, i: usize, values: &BTreeMap<String, Val>, materials: &[String]) -> bool {
    let h = ui.spacing().interact_size.y;
    let FamilyEditor { params, types, picked, types_dialog, .. } = ed;
    let p = &mut params[i];
    let name = p.name.trim().to_string();
    // Revit marks an instance parameter's: what is typed is the value a new instance starts with.
    let label = if p.instance { format!("{name} {}", t("(default)")) } else { name.clone() };
    let selected = types_dialog.selected.as_deref() == Some(name.as_str());
    let clicked = cell(ui, NAME_W, |ui| ui.add(egui::Button::selectable(selected, label)).clicked());
    let value_label = format!("{} {name}", t("Parameter Value"));
    let own = (!p.instance && !p.original.reporting).then(|| types.get_mut(*picked)).flatten();
    match p.kind {
        _ if !p.formula.trim().is_empty() => computed(ui, value_label, p.kind, values.get(&name), VALUE_W),
        Holds::FamilyType => {
            let mut none = t("(none)").to_string();
            let r = ui.add_enabled_ui(false, |ui| ui.add_sized([VALUE_W, h], TextEdit::singleline(&mut none)));
            r.inner.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, false, value_label.as_str()));
        }
        kind => {
            // A type parameter's value is the type's own, else the family's; an instance's, a reporting one's (no type sets
            // it) or one of a family with no type is the family's.
            let typed = own.as_ref().and_then(|(_, set)| set.get(&name).cloned());
            let mut text = typed.clone().unwrap_or_else(|| p.value.clone());
            let before = text.clone();
            match kind {
                Holds::YesNo => {
                    let mut on = text == "1";
                    cell(ui, VALUE_W, |ui| {
                        let r = ui.checkbox(&mut on, "");
                        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, value_label.as_str()));
                    });
                    text = (if on { "1" } else { "0" }).into();
                }
                Holds::Material => {
                    let mut options = vec![String::new()];
                    options.extend(materials.iter().cloned());
                    if let Some(m) = cell(ui, VALUE_W, |ui| pick(ui, value_label, &text, &options)) {
                        text = m;
                    }
                }
                _ => field(ui, value_label, &mut text, VALUE_W),
            }
            if text != before {
                match own {
                    Some((_, set)) => {
                        set.insert(name.clone(), text);
                    }
                    None => p.value = text,
                }
            }
        }
    }
    // Words take no formula, nor does a reporting parameter: its dimension says what it is.
    let formula_label = format!("{} {name}", t("Parameter Formula"));
    if p.kind.text() || p.original.reporting {
        let r = ui.add_enabled_ui(false, |ui| ui.add_sized([FORMULA_W, h], TextEdit::singleline(&mut p.formula.clone())));
        r.inner.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, false, formula_label.as_str()));
    } else {
        field(ui, formula_label, &mut p.formula, FORMULA_W);
    }
    let lock_label = format!("{} {name}", t("Lock"));
    cell(ui, LOCK_W, |ui| {
        let r = ui.add_enabled(false, egui::Checkbox::without_text(&mut false)).on_disabled_hover_text(t(LOCK_TIP));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, false, false, lock_label.as_str()));
    });
    clicked
}

/// A cell of the table `w` wide, its content from the left, so the columns line up from group to group.
fn cell<R>(ui: &mut Ui, w: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(w);
        add(ui)
    })
    .inner
}

/// The buttons under the table: New, Edit and Delete Parameter, Move Up and Move Down within a group, and sorting.
fn buttons(ed: &mut FamilyEditor, ui: &mut Ui, base: &Arc<Model>) {
    let chosen = ed.types_dialog.selected.clone().filter(|n| ed.param_at(n).is_some());
    ui.horizontal_wrapped(|ui| {
        if ui.button(t("New Parameter")).clicked() {
            let editing = Editing {
                was: None,
                name: String::new(),
                kind: Holds::Length,
                instance: false,
                group: Group::Dimensions,
                reporting: false,
                of: None,
                shared: None,
                is_shared: false,
                file: String::new(),
                read: None,
                error: None,
            };
            (ed.types_dialog.editing, ed.types_dialog.said) = (Some(editing), None);
        }
        if ui.add_enabled(chosen.is_some(), egui::Button::new(t("Edit Parameter"))).clicked()
            && let Some(p) = chosen.as_deref().and_then(|n| ed.param_at(n)).map(|i| &ed.params[i])
        {
            let editing = Editing {
                was: Some(p.name.trim().to_string()),
                name: p.name.trim().to_string(),
                kind: p.kind,
                instance: p.instance,
                group: group_of(p),
                reporting: p.original.reporting,
                of: p.original.of,
                shared: p.original.shared,
                is_shared: p.original.shared.is_some(),
                file: String::new(),
                read: None,
                error: None,
            };
            (ed.types_dialog.editing, ed.types_dialog.said) = (Some(editing), None);
        }
        if ui.add_enabled(chosen.is_some(), egui::Button::new(t("Delete Parameter"))).clicked()
            && let Some(name) = chosen.as_deref()
        {
            ed.types_dialog.said = match ed.delete_parameter(base, name) {
                Ok(()) => {
                    ed.types_dialog.selected = None;
                    None
                }
                Err(e) => Some(tf("{} can't be deleted: {}", &[name, &e])),
            };
        }
        ui.separator();
        if ui.add_enabled(chosen.is_some(), egui::Button::new(t("Move Up"))).clicked() {
            ed.move_selected(-1);
        }
        if ui.add_enabled(chosen.is_some(), egui::Button::new(t("Move Down"))).clicked() {
            ed.move_selected(1);
        }
        if ui.button(t("Ascending")).on_hover_text(t("Sort each group's parameters by name, A to Z")).clicked() {
            ed.sort_groups(true);
        }
        if ui.button(t("Descending")).on_hover_text(t("Sort each group's parameters by name, Z to A")).clicked() {
            ed.sort_groups(false);
        }
    });
}

/// The dialogs Family Types opens over itself: Name (New Type, Rename Type) and Parameter Properties (New and Edit Parameter).
pub(super) fn over(ctx: &Context, ed: &mut FamilyEditor) {
    naming(ctx, ed);
    editing(ctx, ed);
}

/// Name: a new type's name (a copy of the type shown) or the type's new name. OK refuses a blank or a name taken.
fn naming(ctx: &Context, ed: &mut FamilyEditor) {
    let Some(mut n) = ed.types_dialog.naming.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let r = Modal::new(UiId::new("family-type-name"), t("Name")).width(360.0).show(ctx, |ui| {
        crate::popup::body(ui, |ui| {
            ui.horizontal(|ui| {
                let l = ui.label(t("Name:"));
                let r = ui.add_sized([240.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut n.name)).labelled_by(l.id);
                ok |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
            if let Some(e) = &n.error {
                ui.label(RichText::new(e).color(pal().error));
            }
        });
        crate::popup::footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                cancel = ui.button(t("Cancel")).clicked();
                ok |= ui.button(t("OK")).clicked();
            });
        });
    });
    if cancel || r.should_close() {
        return;
    }
    if ok {
        let name = n.name.trim().to_string();
        let taken = ed.types.iter().enumerate().any(|(i, (x, _))| *x == name && !(n.rename && i == ed.picked));
        if name.is_empty() {
            n.error = Some(t("A type needs a name.").to_string());
        } else if taken {
            n.error = Some(tf("The family has a type named {} already.", &[&name]));
        } else {
            if n.rename {
                if let Some((was, _)) = ed.types.get_mut(ed.picked) {
                    *was = name;
                }
            } else {
                // Revit's New Type starts from the type shown: its values copied.
                let set = ed.types.get(ed.picked).map(|(_, s)| s.clone()).unwrap_or_default();
                ed.types.push((name, set));
                ed.picked = ed.types.len() - 1;
            }
            return;
        }
    }
    ed.types_dialog.naming = Some(n);
}

/// Parameter Properties, as T2 describes a parameter: family or shared (picked from a shared parameters file, its name and
/// kind the file's), its name, Type or Instance, reporting, what it holds, and the group it is listed under.
fn editing(ctx: &Context, ed: &mut FamilyEditor) {
    let Some(mut e) = ed.types_dialog.editing.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    // The shared parameters file read again when its path changes.
    if e.is_shared && e.read.as_ref().is_none_or(|(path, _)| *path != e.file) && !e.file.trim().is_empty() {
        let read = SharedFile::read(std::path::Path::new(e.file.trim()))
            .map(|f| f.parameters.iter().filter_map(|d| d.kind().ok().map(|(k, of)| (d.guid, d.name.clone(), k, of))).collect());
        e.read = Some((e.file.clone(), read));
    }
    let title = t("Parameter Properties");
    let r = Modal::new(UiId::new("family-parameter-properties"), title).width(520.0).show(ctx, |ui| {
        crate::popup::body(ui, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut e.is_shared, false, t("Family parameter"));
                ui.radio_value(&mut e.is_shared, true, t("Shared parameter"));
            });
            if e.is_shared {
                ui.horizontal(|ui| {
                    let l = ui.label(t("Shared parameters file:"));
                    ui.add_sized([300.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut e.file)).labelled_by(l.id);
                });
                match &e.read {
                    Some((_, Ok(list))) => {
                        let names: Vec<String> = list.iter().map(|(_, n, _, _)| n.clone()).collect();
                        let now = list.iter().find(|(g, ..)| Some(*g) == e.shared).map(|(_, n, ..)| n.clone()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(t("Parameter:"));
                            if let Some(n) = pick(ui, t("Shared parameter").to_string(), &now, &names)
                                && let Some((g, name, kind, of)) = list.iter().find(|(_, x, ..)| *x == n)
                            {
                                (e.shared, e.name, e.kind, e.of) = (Some(*g), name.clone(), *kind, *of);
                            }
                        });
                    }
                    Some((_, Err(why))) => {
                        ui.label(RichText::new(why).color(pal().error));
                    }
                    None => {}
                }
            }
            // A shared parameter's name and kind are the file's.
            let own = !e.is_shared;
            Grid::new("family-parameter-properties-grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let l = ui.label(t("Name:"));
                ui.add_enabled_ui(own, |ui| {
                    ui.add_sized([260.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut e.name)).labelled_by(l.id);
                });
                ui.end_row();
                ui.label("");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut e.instance, false, t("Type"));
                    ui.radio_value(&mut e.instance, true, t("Instance"));
                    let can_report = matches!(e.kind, Holds::Length | Holds::Angle);
                    ui.add_enabled(can_report, egui::Checkbox::new(&mut e.reporting, t("Reporting Parameter")))
                        .on_hover_text(t("Reads a dimension and never drives one: a length or an angle"));
                    e.reporting &= can_report;
                });
                ui.end_row();
                let l = ui.label(t("Type of Parameter:"));
                ui.add_enabled_ui(own, |ui| {
                    let was = e.kind;
                    let c = ComboBox::from_id_salt("family-parameter-kind").selected_text(t(kind_name(e.kind))).width(180.0).show_ui(
                        ui,
                        |ui| {
                            for k in KINDS {
                                ui.selectable_value(&mut e.kind, k, t(kind_name(k)));
                            }
                        },
                    );
                    c.response.labelled_by(l.id);
                    // The group follows the kind while it is the kind's own.
                    if e.kind != was && e.group == kind_group(was) {
                        e.group = kind_group(e.kind);
                    }
                });
                ui.end_row();
                if e.kind == Holds::FamilyType {
                    let l = ui.label(t("Category:"));
                    let shown = e.of.map_or_else(|| t("(none)").to_string(), category_label);
                    let c = ComboBox::from_id_salt("family-parameter-of").selected_text(shown).width(180.0).show_ui(ui, |ui| {
                        for c in CATEGORIES {
                            if ui.selectable_label(e.of == Some(c), category_label(c)).clicked() {
                                e.of = Some(c);
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                }
                let l = ui.label(t("Group parameter under:"));
                let c = ComboBox::from_id_salt("family-parameter-group").selected_text(t(e.group.label())).width(180.0).show_ui(ui, |ui| {
                    for g in Group::ALL {
                        ui.selectable_value(&mut e.group, g, t(g.label()));
                    }
                });
                c.response.labelled_by(l.id);
                ui.end_row();
            });
            if let Some(why) = &e.error {
                ui.label(RichText::new(why).color(pal().error));
            }
        });
        crate::popup::footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
    });
    if cancel || r.should_close() {
        return;
    }
    if ok {
        match commit(ed, &e) {
            Ok(()) => return,
            Err(why) => e.error = Some(why),
        }
    }
    ed.types_dialog.editing = Some(e);
}

/// Parameter Properties' OK: the parameter added to the draft, or the one edited changed (renamed wherever it is named).
fn commit(ed: &mut FamilyEditor, e: &Editing) -> Result<(), String> {
    let name = e.name.trim().to_string();
    if name.is_empty() {
        return Err(t("A parameter needs a name.").to_string());
    }
    if e.is_shared && e.shared.is_none() {
        return Err(t("Pick a parameter of the shared parameters file.").to_string());
    }
    if e.kind == Holds::FamilyType && e.of.is_none() {
        return Err(t("A family type parameter needs the category of the families whose types it picks.").to_string());
    }
    if e.was.as_deref() != Some(name.as_str()) && ed.param_at(&name).is_some() {
        return Err(tf("The family has a parameter named {} already.", &[&name]));
    }
    let group = Some(e.group).filter(|g| *g != kind_group(e.kind));
    let shared = e.shared.filter(|_| e.is_shared);
    let at = match &e.was {
        None => {
            ed.params.push(ParamText {
                original: Parameter::default(),
                name: name.clone(),
                kind: e.kind,
                instance: e.instance,
                value: String::new(),
                formula: String::new(),
            });
            ed.params.len() - 1
        }
        Some(was) => {
            if *was != name {
                ed.rename_everywhere(was, &name);
            }
            ed.param_at(&name).ok_or_else(|| tf("the family has no parameter `{}`", &[&name]))?
        }
    };
    let p = &mut ed.params[at];
    if p.kind != e.kind {
        // A value typed for another kind starts again from the new kind's.
        p.value = String::new();
        for (_, set) in &mut ed.types {
            set.remove(&name);
        }
    }
    let p = &mut ed.params[at];
    (p.kind, p.instance) = (e.kind, e.instance);
    p.original.group = group;
    p.original.shared = shared;
    p.original.reporting = e.reporting;
    p.original.of = e.of.filter(|_| e.kind == Holds::FamilyType);
    if e.reporting {
        p.formula.clear();
    }
    ed.types_dialog.selected = Some(name);
    Ok(())
}
