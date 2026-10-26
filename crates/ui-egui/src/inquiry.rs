//! Manage > Inquiry: Select by ID and IDs of Selection, as Revit's, and more. An element is found by its id, its GUID or its
//! IFC GlobalId, as many as are pasted at once, and what isn't found is said; the plan that shows them opens, zoomed to them.
//! What names the selection is each element's id, GUID and IFC GlobalId, to read or copy one, a column, or all.

use bimcraft_i18n::msg;

use bimcraft_core::inquiry::{self, Names};
use egui::{Align, Button, Context, Grid, Id as UiId, Label, Layout, Modal, RichText, ScrollArea, TextEdit};

use crate::lang::{category, t, tf};
use crate::{BimApp, Sheet, Status};

/// Select by ID: what's typed or pasted, whether it adds to the selection, and whether the plan that shows it opens.
pub(crate) struct SelectById {
    pub(crate) text: String,
    pub(crate) add: bool,
    pub(crate) show: bool,
    /// The text box takes the keys when the dialog opens.
    focus: bool,
}

impl Default for SelectById {
    fn default() -> SelectById {
        SelectById { text: String::new(), add: false, show: true, focus: true }
    }
}

/// IDs of Selection: what names each selected element, and what was copied last.
pub(crate) struct IdsOfSelection {
    pub(crate) rows: Vec<Names>,
    pub(crate) copied: Option<String>,
}

/// Which names a copy takes.
#[derive(Clone, Copy)]
enum Column {
    Id,
    Guid,
    Ifc,
}

/// `rows` as text: one column, a line each; or all of them, tab-separated under a header, as a spreadsheet pastes them.
fn copied(rows: &[Names], column: Option<Column>) -> String {
    let one = |n: &Names| match column {
        Some(Column::Id) => n.id.to_string(),
        Some(Column::Guid) => n.guid.to_string(),
        Some(Column::Ifc) => n.ifc.clone(),
        None => format!("{}\t{}\t{}\t{}\t{}", n.category, n.name, n.id, n.guid, n.ifc),
    };
    let lines = rows.iter().map(one);
    match column {
        Some(_) => lines.collect::<Vec<_>>().join("\n"),
        None => std::iter::once("Category\tName\tID\tGUID\tIFC GlobalId".to_string()).chain(lines).collect::<Vec<_>>().join("\n"),
    }
}

impl BimApp {
    /// Manage > Select by ID.
    pub(crate) fn open_select_by_id(&mut self) {
        self.select_by_id = Some(SelectById::default());
    }

    /// Manage > IDs of Selection: what names the elements selected.
    pub(crate) fn open_ids_of_selection(&mut self) {
        let rows = self.selection.iter().filter_map(|id| inquiry::names(&self.session.model, *id)).collect();
        self.ids_of_selection = Some(IdsOfSelection { rows, copied: None });
    }

    /// Select what `text` names (added to the selection with `add`), and with `show`, open the plan that shows it, zoomed
    /// to it. What it found, and what named nothing, in the status box.
    pub(crate) fn select_named(&mut self, text: &str, add: bool, show: bool) {
        let found = inquiry::find(&self.session.model, text);
        // What the library has (a type, a material) is no element to select: said by what it is and its name.
        let library: Vec<String> = found
            .library
            .iter()
            .filter_map(|id| inquiry::names(&self.session.model, *id))
            .map(|n| format!("{} {} ({})", t(&n.category), n.name, n.id))
            .collect();
        if found.ids.is_empty() && !library.is_empty() {
            self.status = Status { text: tf("In the library, not elements to select: {}", &[&library.join(", ")]), error: false };
            return;
        }
        if found.ids.is_empty() {
            let text = if found.missing.is_empty() {
                t("Type or paste an ID, a GUID or an IFC GlobalId.").to_string()
            } else {
                tf("Nothing has that ID or GUID: {}", &[&found.missing.join(", ")])
            };
            self.status = Status { text, error: true };
            return;
        }
        if !add {
            self.selection.clear();
        }
        for id in &found.ids {
            if !self.selection.contains(id) {
                self.selection.push(*id);
            }
        }
        if show && let Some((level, [lo, hi])) = inquiry::shown(&self.session.model, &found.ids) {
            self.open_sheet(Sheet::Plan(level));
            self.canvas.show(level, lo, hi);
        }
        let n = found.ids.len().to_string();
        self.status = match found.missing.is_empty() {
            true => Status { text: tf("Selected {}.", &[&n]), error: false },
            false => Status { text: tf("Selected {}; nothing has these: {}", &[&n, &found.missing.join(", ")]), error: false },
        };
    }

    pub(crate) fn select_by_id_window(&mut self, ctx: &Context) {
        let Some(d) = self.select_by_id.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let found = inquiry::find(&self.session.model, &d.text);
        let any = !found.ids.is_empty() || !found.library.is_empty();
        let r = Modal::new(UiId::new("select-by-id")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(t("Select by ID")).heading());
            ui.add_space(6.0);
            let label = ui.label(t("IDs, GUIDs or IFC GlobalIds, as many as you like:"));
            let edit = ui.add(TextEdit::multiline(&mut d.text).desired_rows(4).desired_width(f32::INFINITY).hint_text("100005, 3f2a…"));
            if std::mem::take(&mut d.focus) {
                edit.request_focus();
            }
            edit.labelled_by(label.id);
            // What it finds as it's typed.
            let summary = match (found.ids.len(), found.missing.is_empty()) {
                (0, true) if !found.library.is_empty() => tf("Finds {} in the library, not elements.", &[&found.library.len().to_string()]),
                (0, true) => String::new(),
                (n, true) => tf("Finds {}.", &[&n.to_string()]),
                (n, false) => tf("Finds {}; nothing has these: {}", &[&n.to_string(), &found.missing.join(", ")]),
            };
            ui.label(summary);
            ui.checkbox(&mut d.add, t("Add to the selection"));
            ui.checkbox(&mut d.show, t("Show: open the plan they are in, zoomed to them"));
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                close = ui.button(t("Cancel")).clicked();
                ok = ui.add_enabled(any, Button::new(t("OK"))).clicked();
            });
            ok |= any && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
        });
        if ok && let Some(d) = self.select_by_id.take() {
            self.select_named(&d.text, d.add, d.show);
        } else if close || r.should_close() {
            self.select_by_id = None;
        }
    }

    pub(crate) fn ids_of_selection_window(&mut self, ctx: &Context) {
        let Some(d) = self.ids_of_selection.as_mut() else { return };
        let mut close = false;
        let mut copy: Option<(String, String)> = None;
        let r = Modal::new(UiId::new("ids-of-selection")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.label(RichText::new(t("IDs of Selection")).heading());
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                Grid::new("ids-of-selection-grid").num_columns(6).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                    for h in ["Category", "Name", msg!("ID"), msg!("GUID"), msg!("IFC GlobalId"), ""] {
                        ui.label(RichText::new(t(h)).strong());
                    }
                    ui.end_row();
                    for n in &d.rows {
                        ui.label(category(&n.category, false));
                        ui.label(&n.name);
                        // Each name selectable, to copy a part of it.
                        ui.add(Label::new(RichText::new(n.id.to_string()).monospace()).selectable(true));
                        ui.add(Label::new(RichText::new(n.guid.to_string()).monospace()).selectable(true));
                        ui.add(Label::new(RichText::new(&n.ifc).monospace()).selectable(true));
                        if ui.button(t("Copy")).on_hover_text(t("Copy its ID, GUID and IFC GlobalId")).clicked() {
                            let text = format!("{}\t{}\t{}", n.id, n.guid, n.ifc);
                            copy = Some((text, tf("Copied the names of {} {}.", &[&category(&n.category, false), &n.id.to_string()])));
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(6.0);
            let count = d.rows.len().to_string();
            ui.label(tf("{} selected.", &[&count]));
            if let Some(c) = &d.copied {
                ui.label(RichText::new(c).weak());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, column, what) in [
                    (msg!("Copy IDs"), Some(Column::Id), msg!("Copied {} IDs.")),
                    (msg!("Copy GUIDs"), Some(Column::Guid), msg!("Copied {} GUIDs.")),
                    (msg!("Copy IFC GlobalIds"), Some(Column::Ifc), msg!("Copied {} IFC GlobalIds.")),
                    (msg!("Copy All"), None, msg!("Copied {} rows, tab-separated.")),
                ] {
                    if ui.add_enabled(!d.rows.is_empty(), Button::new(t(label))).clicked() {
                        copy = Some((copied(&d.rows, column), tf(what, &[&count])));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
            });
        });
        if let Some((text, said)) = copy {
            ctx.copy_text(text.clone());
            d.copied = Some(said);
            self.copied = Some(text);
        }
        if close || r.should_close() {
            self.ids_of_selection = None;
        }
    }

    /// What the selection's names say, for the Manage tab's tip: none, one's id, or how many.
    pub(crate) fn selection_ids_tip(&self) -> String {
        match self.selection.as_slice() {
            [] => t("IDs of Selection: select elements first").to_string(),
            [one] => tf("IDs of Selection: the ID, GUID and IFC GlobalId of {}", &[&one.to_string()]),
            many => tf("IDs of Selection: the IDs, GUIDs and IFC GlobalIds of the {} selected", &[&many.len().to_string()]),
        }
    }
}
