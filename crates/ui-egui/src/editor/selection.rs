//! FE-20: the selection commands of a family document that isn't adaptive, over the one
//! selection set (FE-1, `select_box.rs`). The core says what each finds (`family::select`); this is their dialogs and
//! buttons, which look as the project's do.
//!
//! - **Filter** (the Select panel's button while several are selected, and the status bar's funnel): the kinds the
//!   selection holds, in an order of their own whatever the language (forms, datums, dimensions, symbols, connectors,
//!   nested instances: `select::Kind::rank`), each with how many of it, checked to keep or cleared to leave out, Check All and Check None; OK keeps
//!   what is checked selected. The list is the project's Filter's (`filter::checklist`).
//! - **Select Previous**: the selection as it was before it was last let go or replaced, again
//!   (`Selection::restore_previous`): one set, without what is gone from the draft since.
//! - **Select by ID** (Manage > Inquiry): the parts a list of ids and GUIDs names, selected or added to the selection;
//!   what names nothing is said as it is typed. **IDs of Selection** beside it shows what names each thing selected, to
//!   read or copy: the only place a family document shows an id. A part made since the family was last loaded into a
//!   project has no id yet, and one the document made without a GUID none of that either.
//! - **Select Elements by Face** (the status bar's toggle on a family's tab, a setting the window keeps): on, a click
//!   on a form's or a nested instance's face selects it and a crossing box drawn inside one takes it; off, each is taken
//!   by an edge the view draws it with alone, an L's inner corner and never its hull's chord (`views.rs`,
//!   `select_box::Scene`). Either way a box may begin inside the face of what isn't selected (but see Drag Elements on
//!   Selection, below), but never inside the face of what is. It is of the plans and elevations: greyed
//!   in {3D}, which takes a form by its face either way as the project's 3D view does, and while a form is sketched.
//! - **Drag Elements on Selection** (the status bar's toggle between Select Elements by Face and the funnel, a setting the
//!   window keeps, off at first): on, a drag from the face of a form or a nested instance that isn't selected is the
//!   element's, as one from its edge is, and selects it, where a face selects; off, a selection box begins there. With
//!   Select Elements by Face off a face picks nothing, so it changes nothing. Greyed where that one is.
//!
//! None of it is a step of the document's history or reaches the project: the selection is the session's.
use super::select_box::{How, Picked};
use super::*;
use crate::icons::Icon;
use crate::ribbon;
use bimcraft_core::family::select;
use bimcraft_core::ids::{Guid, Id};
use egui::{Button, Label};

/// What the selection's commands keep of a family document: their dialogs while they are open, and how a click picks.
#[derive(Default)]
pub(crate) struct Selecting {
    pub(crate) filter: Option<Filter>,
    pub(crate) by_id: Option<ById>,
    pub(crate) ids: Option<Ids>,
    /// Select Elements by Face, as the window has it (`BimApp::select_by_face`), given each frame.
    pub(crate) by_face: bool,
    /// Drag Elements on Selection, as the window has it (`BimApp::drag_on_selection`), given each frame.
    pub(crate) drag_faces: bool,
    /// What IDs of Selection copied last: the clipboard's, for a test to read.
    pub(crate) copied: Option<String>,
}

/// The Filter dialog: the selection it opened on, the kinds it holds, and which are checked.
pub(crate) struct Filter {
    items: Vec<Picked>,
    groups: Vec<select::Group>,
    on: Vec<bool>,
}

/// Select by ID: what is typed or pasted, and whether it adds to the selection.
pub(crate) struct ById {
    text: String,
    add: bool,
    /// The text box takes the keys when the dialog opens.
    focus: bool,
}

/// IDs of Selection: what names each thing selected, and what was copied last.
pub(crate) struct Ids {
    rows: Vec<(String, Id, Guid)>,
    copied: Option<String>,
}

/// An id or a GUID as IDs of Selection shows it: a dash for one not given yet.
fn id_text(id: Id) -> String {
    if bimcraft_core::ids::unset(&id) { "\u{2014}".to_string() } else { id.to_string() }
}

fn guid_text(guid: Guid) -> String {
    if guid.is_nil() { "\u{2014}".to_string() } else { guid.to_string() }
}

impl FamilyEditor {
    /// The draft's parts as a family: what Filter, Select by ID and IDs of Selection read. A form or a symbol mistyped is as
    /// it was before it was typed in, so the selection's commands work while a row of Properties doesn't read yet.
    fn parts(&self) -> Family {
        Family {
            forms: self.forms.iter().map(|f| f.form().unwrap_or_else(|_| f.original.clone())).collect(),
            symbols: self
                .symbols
                .iter()
                .map(|y| y.symbol().unwrap_or_else(|_| Symbol { id: y.ident.0, guid: y.ident.1, ..Symbol::default() }))
                .collect(),
            connectors: self.connectors.iter().map(connectors::ConnectorText::connector).collect(),
            planes: self.planes.clone(),
            lines: self.lines.clone(),
            dimensions: self.dimensions.clone(),
            nested: self.nested.clone(),
            nested_instances: self.nested_instances.clone(),
            ..Family::default()
        }
    }

    /// Whether the selection's commands work here: in a family that isn't adaptive, while no form is sketched.
    fn selects(&self) -> bool {
        self.adaptive.is_none() && !self.sketching()
    }

    /// Whether Select Elements by Face changes anything here: in a plan or an elevation of a family that isn't adaptive,
    /// while no form is sketched. Elsewhere its toggle is greyed and Select Elements by Face does nothing.
    pub(crate) fn picks_by_face(&self) -> bool {
        self.editing_2d() && !self.sketching()
    }

    /// Whether a dialog of the selection's is open.
    pub(super) fn selecting(&self) -> bool {
        self.selecting.filter.is_some() || self.selecting.by_id.is_some() || self.selecting.ids.is_some()
    }

    /// How many things are selected, for the status bar: an adaptive family's points, lines and forms, else the set's.
    pub(crate) fn selected_count(&self) -> usize {
        if self.adaptive.is_some() { self.modeller.selection.len() } else { self.selection.len() }
    }

    /// Whether Filter has something to filter.
    pub(crate) fn can_filter(&self) -> bool {
        self.selects() && !self.selection.is_empty()
    }

    /// Filter: the dialog, on what is selected; nothing with nothing selected.
    pub(crate) fn open_filter(&mut self) {
        if !self.can_filter() {
            return;
        }
        let items = self.selection.items().to_vec();
        let groups = select::groups(&self.parts(), &items);
        let on = vec![true; groups.len()];
        self.selecting.filter = Some(Filter { items, groups, on });
    }

    /// Whether Select Previous has a selection to bring back.
    pub(crate) fn can_select_previous(&self) -> bool {
        self.selects() && self.selection.previous().iter().any(|p| self.holds(*p))
    }

    /// Select Previous: the selection as it was before it was last let go or replaced, without what is gone since.
    pub(crate) fn select_previous(&mut self) {
        if self.can_select_previous() {
            self.prune_selection();
            self.selection.restore_previous();
        }
    }

    /// Select by ID: the dialog.
    pub(crate) fn open_select_by_id(&mut self) {
        if self.selects() {
            self.selecting.by_id = Some(ById { text: String::new(), add: false, focus: true });
        }
    }

    /// What each thing selected is called, as the Project Browser and Properties name it, with its id and its GUID.
    fn named(&self) -> Vec<(String, Id, Guid)> {
        let family = self.parts();
        let name = |p: Picked| match p {
            Picked::Form(i) => format!("{}. {}", i + 1, self.forms.get(i).map(FormText::describe).unwrap_or_default()),
            Picked::Plane(g) => match self.planes.iter().find(|q| q.guid == g) {
                Some(q) if !q.name.trim().is_empty() => t(&q.name).to_string(),
                _ => t("Reference Plane").to_string(),
            },
            Picked::Line(_) => t("Reference Line").to_string(),
            Picked::Dimension(_) => t("Dimension").to_string(),
            Picked::Symbol(i) => self.symbol_name(i),
            Picked::Connector(i) => self.connector_name(i),
            Picked::Nested(i) => {
                self.nested_instances.get(i).map(|n| format!("{}. {} : {}", i + 1, n.family, n.type_name)).unwrap_or_default()
            }
        };
        self.selection.items().iter().filter_map(|p| select::names(&family, *p)).map(|n| (name(n.picked), n.id, n.guid)).collect()
    }

    /// IDs of Selection: what names each thing selected; nothing with nothing selected.
    pub(crate) fn open_ids_of_selection(&mut self) {
        if self.can_filter() {
            self.selecting.ids = Some(Ids { rows: self.named(), copied: None });
        }
    }

    /// The selection's dialogs, while one is open: Filter, Select by ID and IDs of Selection. What Select by ID has to say
    /// once it has selected, for the status bar.
    pub(super) fn selection_dialogs(&mut self, ctx: &Context) -> Option<Status> {
        self.filter_dialog(ctx);
        self.ids_dialog(ctx);
        self.by_id_dialog(ctx)
    }

    /// Filter, as the project's: the selection counted by kind, each checked to keep; OK keeps what is checked selected.
    fn filter_dialog(&mut self, ctx: &Context) {
        let Some(d) = self.selecting.filter.as_mut() else { return };
        let rows: Vec<(String, usize)> = d.groups.iter().map(|g| (t(g.kind.name()).to_string(), g.items.len())).collect();
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("family-selection-filter"), t("Filter")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                crate::filter::checklist(ui, "family-selection-filter-grid", t("Category"), &rows, &mut d.on);
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok && let Some(d) = self.selecting.filter.take() {
            let keep: Vec<select::Kind> = d.groups.iter().zip(&d.on).filter(|(_, on)| **on).map(|(g, _)| g.kind).collect();
            let kept = select::kept(&self.parts(), &d.items, &keep);
            self.selection.apply(&kept, How::Replace);
        } else if close || r.should_close() {
            self.selecting.filter = None;
        }
    }

    /// Select by ID, as the project's: what the list finds is said as it is typed, and what names nothing; OK selects it.
    fn by_id_dialog(&mut self, ctx: &Context) -> Option<Status> {
        self.selecting.by_id.as_ref()?;
        let family = self.parts();
        let d = self.selecting.by_id.as_mut()?;
        let found = select::find(&family, &d.text);
        let any = !found.picked.is_empty();
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("family-select-by-id"), t("Select by ID")).width(440.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                let label = ui.label(t("IDs, GUIDs or IFC GlobalIds, as many as you like:"));
                let edit = ui.add(TextEdit::multiline(&mut d.text).desired_rows(4).desired_width(f32::INFINITY).hint_text("100005, 3f2a…"));
                if std::mem::take(&mut d.focus) {
                    edit.request_focus();
                }
                edit.labelled_by(label.id);
                let summary = match (found.picked.len(), found.missing.is_empty()) {
                    (0, true) => String::new(),
                    (n, true) => tf("Finds {}.", &[&n.to_string()]),
                    (n, false) => tf("Finds {}; nothing has these: {}", &[&n.to_string(), &found.missing.join(", ")]),
                };
                ui.label(summary);
                ui.checkbox(&mut d.add, t("Add to the selection"));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(any, Button::new(t("OK"))).clicked();
                });
                ok |= any && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
            });
        });
        if ok && let Some(d) = self.selecting.by_id.take() {
            self.selection.apply(&found.picked, if d.add { How::Add } else { How::Replace });
            let n = found.picked.len().to_string();
            return Some(match found.missing.is_empty() {
                true => Status { text: tf("Selected {}.", &[&n]), error: false },
                false => Status { text: tf("Selected {}; nothing has these: {}", &[&n, &found.missing.join(", ")]), error: false },
            });
        }
        if close || r.should_close() {
            self.selecting.by_id = None;
        }
        None
    }

    /// IDs of Selection, as the project's: each thing selected with its id and its GUID, to read or copy.
    fn ids_dialog(&mut self, ctx: &Context) {
        let Some(d) = self.selecting.ids.as_mut() else { return };
        let mut close = false;
        let mut copy: Option<(String, String)> = None;
        let r = Modal::new(UiId::new("family-ids-of-selection"), t("IDs of Selection")).width(620.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("family-ids-of-selection-grid").num_columns(3).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        for h in [msg!("Name"), msg!("ID"), msg!("GUID")] {
                            ui.label(RichText::new(t(h)).strong());
                        }
                        ui.end_row();
                        for (name, id, guid) in &d.rows {
                            ui.label(name);
                            // Each selectable, to copy a part of it.
                            ui.add(Label::new(RichText::new(id_text(*id)).monospace()).selectable(true));
                            ui.add(Label::new(RichText::new(guid_text(*guid)).monospace()).selectable(true));
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(6.0);
                ui.label(tf("{} selected.", &[&d.rows.len().to_string()]));
                if d.rows.iter().any(|(_, id, _)| bimcraft_core::ids::unset(id)) {
                    ui.label(
                        RichText::new(t(
                            "What was made since the family was opened has no ID yet: it is given one when the family is loaded into a project.",
                        ))
                        .weak(),
                    );
                }
                if let Some(c) = &d.copied {
                    ui.label(RichText::new(c).weak());
                }
            });
            crate::popup::footer(ui, |ui| {
                let count = d.rows.len().to_string();
                ui.horizontal(|ui| {
                    // What has none yet is left out of a copy: a dash names nothing.
                    let ids: Vec<String> =
                        d.rows.iter().filter(|(_, id, _)| !bimcraft_core::ids::unset(id)).map(|(_, id, _)| id.to_string()).collect();
                    let guids: Vec<String> = d.rows.iter().filter(|(.., g)| !g.is_nil()).map(|(.., g)| g.to_string()).collect();
                    if ui.add_enabled(!ids.is_empty(), Button::new(t("Copy IDs"))).clicked() {
                        copy = Some((ids.join("\n"), tf("Copied {} IDs.", &[&ids.len().to_string()])));
                    }
                    if ui.add_enabled(!guids.is_empty(), Button::new(t("Copy GUIDs"))).clicked() {
                        copy = Some((guids.join("\n"), tf("Copied {} GUIDs.", &[&guids.len().to_string()])));
                    }
                    if ui.add_enabled(!d.rows.is_empty(), Button::new(t("Copy All"))).clicked() {
                        let lines = d.rows.iter().map(|(name, id, guid)| format!("{name}\t{}\t{}", id_text(*id), guid_text(*guid)));
                        let all = std::iter::once("Name\tID\tGUID".to_string()).chain(lines).collect::<Vec<_>>().join("\n");
                        copy = Some((all, tf("Copied {} rows, tab-separated.", &[&count])));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
                });
            });
        });
        if let Some((text, said)) = copy {
            ctx.copy_text(text.clone());
            d.copied = Some(said);
            self.selecting.copied = Some(text);
        }
        if close || r.should_close() {
            self.selecting.ids = None;
        }
    }
}

/// The Select panel's Filter while several are selected, on Modify | Multi-Select; with one selected
/// the status bar's funnel opens it.
pub(super) fn filter_button(ui: &mut Ui, ed: &mut FamilyEditor) {
    if !ed.can_filter() || ed.selection.len() < 2 {
        return;
    }
    let tip = msg!("Filter: keep some of the selection, by what each thing selected is");
    if crate::big_button(ui, Icon::Filter, "Filter", ed.selecting.filter.is_some(), tip).clicked() {
        ed.open_filter();
    }
}

/// Manage > Inquiry, as the project's: Select by ID, and IDs of Selection while something is selected.
pub(super) fn inquiry_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    if ed.adaptive.is_some() {
        return;
    }
    ribbon::panel(ui, msg!("Inquiry"), |ui| {
        let tip = msg!("Select by ID: select what the family holds by its IDs or GUIDs, as many as you paste");
        if crate::big_button(ui, Icon::SelectById, "Select by ID", ed.selecting.by_id.is_some(), tip).clicked() {
            ed.open_select_by_id();
        }
        let tip = msg!("IDs of Selection: the ID and the GUID of each thing selected, to read or copy");
        let some = ed.can_filter();
        if ui
            .add_enabled_ui(some, |ui| crate::big_button(ui, Icon::IdsOfSelection, "IDs of Selection", ed.selecting.ids.is_some(), tip))
            .inner
            .clicked()
        {
            ed.open_ids_of_selection();
        }
    });
}

/// The status bar's Select Elements by Face, left of the funnel: lit while it is `on`,
/// crossed out while it is off.
pub(crate) fn face_toggle(ui: &mut Ui, on: bool) -> egui::Response {
    let tip = msg!(
        "Select Elements by Face: on, a click on a face selects what it is of and a crossing box inside one takes it; off, only its edges do"
    );
    status_toggle(ui, on, Icon::PickFaces, "Select Elements by Face", tip)
        .on_disabled_hover_text(t(msg!("Select Elements by Face is of a family's plans and elevations.")))
}

/// The status bar's Drag Elements on Selection, between Select Elements by Face and the funnel, drawn as that one is.
pub(crate) fn drag_toggle(ui: &mut Ui, on: bool) -> egui::Response {
    let tip = msg!(
        "Drag Elements on Selection: on, a drag from the face of a form or a nested instance that isn't selected is that element's and selects it; off, a selection box begins there. With Select Elements by Face off a face picks nothing, so it changes nothing"
    );
    status_toggle(ui, on, Icon::DragOnSelection, "Drag Elements on Selection", tip)
        .on_disabled_hover_text(t(msg!("Drag Elements on Selection is of a family's plans and elevations.")))
}

/// A toggle of the status bar's, 16 px square: `icon` lit while it is `on`, crossed out while it is off, greyed while the
/// `ui` is disabled; `label` and `tip` translated here.
fn status_toggle(ui: &mut Ui, on: bool, icon: Icon, label: &str, tip: &str) -> egui::Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    let ink = match (enabled, on) {
        (false, _) => pal().icon.gamma_multiply(0.35),
        (true, true) => pal().accent_ink,
        (true, false) => pal().icon,
    };
    if on && enabled {
        ui.painter().rect(rect.expand(2.0), 3.0, pal().select_fill, Stroke::new(1.0, pal().accent), StrokeKind::Inside);
    }
    crate::icons::paint(ui.painter(), icon, rect, ink);
    if !on {
        ui.painter().line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.5, ink));
    }
    r.on_hover_text(t(tip))
}
