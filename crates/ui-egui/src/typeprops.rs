//! Type Properties, one look for every kind of type, after Revit's: the family and the type at the top, Duplicate and Rename
//! beside them; the type's parameters in a table of two columns, Parameter and Value, under bars naming their groups (each
//! folds); and OK, Cancel and Apply at the foot. The dialogs (`properties.rs` for the project's own types and a family's,
//! `notes.rs` for text types, `fillpatterns.rs` for filled region types) keep their own state and commands, and draw with these.

use bimcraft_i18n::msg;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Button, DragValue, Frame, Grid, Id as UiId, Key, Label, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
    UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::lang::t;
use crate::properties::{combo, group_header};
use crate::style::{dialog_room, dialog_row, pal, semibold};

/// Every Type Properties dialog's width, so that each looks the same, and a layered type's layers fit.
pub(crate) const WIDTH: f32 = 580.0;

/// How tall a row of the table is.
const ROW: f32 = 26.0;

/// How tall the table's body is, where the window has room, as Revit's is: every Type Properties dialog is one size, whatever
/// its type's parameters, and the size is known from the first frame (a body that fitted its rows would grow over a few, and a
/// click meant for one row land on another).
const BODY: f32 = 360.0;

/// The labels' column, a share of the table's width.
const SPLIT: f32 = 0.45;

/// A width each of the buttons beside the type, so they line up.
const SIDE: f32 = 100.0;

/// A new name being typed, under the type: for a copy of it, or for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Naming {
    #[default]
    None,
    Duplicate(String),
    Rename(String),
}

/// What the head of the dialog asks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Head {
    /// Show this type instead.
    Switch(String),
    /// Copy the type shown under this name, and show the copy.
    Duplicate(String),
    /// Rename the type shown to this.
    Rename(String),
}

/// Family and Type, with Duplicate and Rename beside the type; `naming`, the name typed for either, in a row under it. `family`
/// is shown, not picked: a type's family is what Edit Type opened it for. `rename` is false where the type can't be renamed.
pub(crate) fn head(
    ui: &mut Ui,
    salt: &str,
    family: &str,
    names: &[String],
    current: &str,
    naming: &mut Naming,
    rename: bool,
) -> Option<Head> {
    let mut asked = None;
    let focus = UiId::new((salt, "naming-focus"));
    let field = (WIDTH - SIDE - 90.0).max(160.0);
    Grid::new((salt, "head")).num_columns(3).spacing(vec2(10.0, 6.0)).min_col_width(60.0).show(ui, |ui| {
        ui.label(t("Family:"));
        // A field that can't be changed: the family's name, read as the type's is.
        Frame::new()
            .fill(pal().surface)
            .stroke(Stroke::new(1.0, pal().border))
            .corner_radius(ui.visuals().widgets.inactive.corner_radius)
            .inner_margin(Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.set_width(field - 12.0);
                ui.add(Label::new(family).truncate());
            });
        ui.label("");
        ui.end_row();

        let l = ui.label(t("Type:"));
        let c = egui::ComboBox::from_id_salt((salt, "type")).selected_text(current).width(field).height(320.0).show_ui(ui, |ui| {
            for n in names {
                if ui.selectable_label(n == current, n).clicked() && n != current {
                    asked = Some(Head::Switch(n.clone()));
                }
            }
        });
        c.response.labelled_by(l.id);
        let duplicating = matches!(naming, Naming::Duplicate(_));
        if ui.add(Button::new(t("Duplicate…")).selected(duplicating).min_size(vec2(SIDE, 0.0))).clicked() {
            *naming = if duplicating { Naming::None } else { Naming::Duplicate(format!("{current} 2")) };
            ui.data_mut(|d| d.insert_temp(focus, true));
        }
        ui.end_row();

        let (label, accessible, verb, typed) = match naming {
            Naming::Duplicate(s) => (t("Name:"), msg!("New type"), t("Create"), Some(s)),
            Naming::Rename(s) => (t("Name:"), msg!("New name"), t("Rename"), Some(s)),
            Naming::None => ("", "", "", None),
        };
        ui.label(label);
        let done = match typed {
            Some(typed) => {
                ui.horizontal(|ui| {
                    let id = UiId::new((salt, "naming"));
                    let r = ui.add(TextEdit::singleline(typed).id(id).desired_width(field - 80.0));
                    let name = t(accessible).to_string();
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.clone()));
                    // Open, the field has the keyboard and its name selected, to type over.
                    if ui.data_mut(|d| d.remove_temp::<bool>(focus)).is_some() {
                        r.request_focus();
                        if let Some(mut state) = TextEdit::load_state(ui.ctx(), id) {
                            let all = CCursorRange::two(CCursor::new(0), CCursor::new(typed.chars().count()));
                            state.cursor.set_char_range(Some(all));
                            state.store(ui.ctx(), id);
                        }
                    }
                    let entered = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let clicked = ui.add(Button::new(verb).min_size(vec2(70.0, 0.0))).clicked();
                    (entered || clicked).then(|| typed.trim().to_string())
                })
                .inner
            }
            None => {
                ui.label("");
                None
            }
        };
        let renaming = matches!(naming, Naming::Rename(_));
        let r = ui.add_enabled(rename, Button::new(t("Rename…")).selected(renaming).min_size(vec2(SIDE, 0.0)));
        if r.clicked() {
            *naming = if renaming { Naming::None } else { Naming::Rename(current.to_string()) };
            ui.data_mut(|d| d.insert_temp(focus, true));
        }
        ui.end_row();
        if let Some(name) = done.filter(|n| !n.is_empty()) {
            asked = Some(match naming {
                Naming::Rename(_) => Head::Rename(name),
                _ => Head::Duplicate(name),
            });
        }
    });
    ui.add_space(10.0);
    asked
}

/// The type's parameters: a table of two columns under its header, its rows drawn by `add`. It scrolls in what the window has
/// left over the dialog's foot, `below` tall (its buttons, and what it says under the table).
pub(crate) fn table(ui: &mut Ui, salt: &str, below: f32, add: impl FnOnce(&mut Table)) {
    ui.label(RichText::new(t("Type Parameters")).font(semibold(13.0)));
    ui.add_space(4.0);
    let body = dialog_room(ui, below + dialog_row(ui) + 26.0).min(BODY);
    Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        let split = (width * SPLIT).round();
        // The header, which stays as the rows scroll.
        let (r, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 0.0, pal().panel);
        for (x, name) in [(r.left(), t("Parameter")), (r.left() + split, t("Value"))] {
            let cell = Rect::from_x_y_ranges(x..=x + split, r.y_range());
            p.text(cell.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, name, semibold(12.5), pal().muted);
        }
        p.line_segment([pos2(r.left() + split, r.top()), pos2(r.left() + split, r.bottom())], Stroke::new(1.0, pal().border));
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal().border));
        ScrollArea::vertical().id_salt((salt, "rows")).max_height(body).min_scrolled_height(body).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let top = ui.cursor().top();
                let mut table = Table { ui, salt: UiId::new((salt, "table")), split, open: true, rows: Vec::new() };
                add(&mut table);
                let Table { ui, rows, .. } = table;
                // The column's rule runs down through the empty rows under the last.
                let used = ui.cursor().top() - top;
                if used < body {
                    ui.allocate_exact_size(vec2(width, body - used), Sense::hover());
                }
                let x = ui.min_rect().left() + split;
                for (a, b) in rows.into_iter().chain([(top + used, ui.cursor().top())]) {
                    ui.painter().line_segment([pos2(x, a), pos2(x, b)], Stroke::new(1.0, pal().border));
                }
            },
        );
    });
}

/// The rows of the table, as [`table`] gives them to fill.
pub(crate) struct Table<'u> {
    ui: &'u mut Ui,
    salt: UiId,
    /// Where the value column starts, from the table's left.
    split: f32,
    /// Whether the group the rows are in is open.
    open: bool,
    /// Where each run of two-column rows starts and ends, which the column's rule is drawn down.
    rows: Vec<(f32, f32)>,
}

impl Table<'_> {
    /// A group's bar, which folds it; the rows after are in it.
    pub(crate) fn group(&mut self, name: &str) {
        let id = self.salt.with(("folded", name));
        let folded = self.ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        if group_header(self.ui, t(name), !folded) {
            self.ui.data_mut(|d| d.insert_temp(id, !folded));
        }
        self.open = !folded;
    }

    /// A row: its parameter's name, and what `value` draws in the value column, given the name's id to be labelled by.
    pub(crate) fn row<R>(&mut self, label: &str, value: impl FnOnce(&mut Ui, UiId) -> R) -> Option<R> {
        if !self.open {
            return None;
        }
        let r = Rect::from_min_size(self.ui.cursor().min, vec2(self.ui.available_width(), ROW));
        self.ui.advance_cursor_after_rect(r);
        let x = r.left() + self.split;
        match self.rows.last_mut() {
            Some((_, end)) if (*end - r.top()).abs() < 0.5 => *end = r.bottom(),
            _ => self.rows.push((r.top(), r.bottom())),
        }
        self.ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal().border.gamma_multiply(0.6)));
        let name = Rect::from_x_y_ranges(r.left() + 8.0..=x - 6.0, r.y_range());
        let mut cell =
            self.ui.new_child(UiBuilder::new().id_salt(("name", label)).max_rect(name).layout(Layout::left_to_right(Align::Center)));
        let l = cell.add(Label::new(label).truncate());
        let cell = Rect::from_x_y_ranges(x + 4.0..=r.right() - 4.0, r.y_range().shrink(2.0));
        let mut child =
            self.ui.new_child(UiBuilder::new().id_salt(("row", label)).max_rect(cell).layout(Layout::left_to_right(Align::Center)));
        child.spacing_mut().item_spacing = vec2(4.0, 0.0);
        Some(value(&mut child, l.id))
    }

    /// A row across both columns, for what a pair of them can't hold: a type's layers, a sample.
    pub(crate) fn full(&mut self, add: impl FnOnce(&mut Ui)) {
        if !self.open {
            return;
        }
        Frame::new().inner_margin(Margin::symmetric(8, 6)).show(self.ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.set_width(ui.available_width());
            add(ui);
        });
        let r = self.ui.min_rect();
        let y = self.ui.cursor().top();
        self.ui.painter().line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, pal().border.gamma_multiply(0.6)));
    }

    /// A section's title over the groups in it (Model, Graphics), and what they are about under it: always open.
    pub(crate) fn section(&mut self, name: &str, about: &str) {
        self.open = true;
        Frame::new().fill(pal().panel).inner_margin(Margin { left: 8, right: 8, top: 8, bottom: 6 }).show(self.ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(t(name)).font(semibold(14.0)).color(pal().accent));
            if !about.is_empty() {
                ui.add(Label::new(RichText::new(t(about)).color(pal().muted).small()).wrap());
            }
        });
    }

    /// A line of help across both columns, in small muted text.
    pub(crate) fn note(&mut self, text: &str) {
        self.full(|ui| {
            ui.add(Label::new(RichText::new(text).color(pal().muted).small()).wrap());
        });
    }

    /// A row of text typed, with what it does when hovered.
    pub(crate) fn text_about(&mut self, label: &str, typed: &mut String, hint: &str, about: &str) {
        self.row(label, |ui, l| {
            ui.add(TextEdit::singleline(typed).hint_text(hint).desired_width(ui.available_width())).labelled_by(l).on_hover_text(about)
        });
    }

    /// A row of yes or no, with what it does when hovered.
    pub(crate) fn check_about(&mut self, label: &str, on: &mut bool, about: &str) {
        self.row(label, |ui, l| {
            let r = ui.checkbox(on, "");
            let now = *on;
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, now, label));
            r.labelled_by(l).on_hover_text(about)
        });
    }

    /// A row of text typed: a length, a number, a name.
    pub(crate) fn text(&mut self, label: &str, typed: &mut String, hint: &str) {
        self.row(label, |ui, l| ui.add(TextEdit::singleline(typed).hint_text(hint).desired_width(ui.available_width())).labelled_by(l));
    }

    /// A row of yes or no.
    pub(crate) fn check(&mut self, label: &str, on: &mut bool) {
        self.row(label, |ui, l| {
            let r = ui.checkbox(on, "");
            let now = *on;
            // A checkbox without text names itself "" for screen readers: give it its row's name.
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, now, label));
            r.labelled_by(l)
        });
    }

    /// A row picking one of `options`.
    pub(crate) fn pick(&mut self, label: &str, current: &mut String, options: &[String]) {
        let salt = self.salt;
        self.row(label, |ui, l| {
            if let Some(picked) = combo(ui, (salt.value(), label), l, current, options) {
                *current = picked;
            }
        });
    }

    /// A row of a number dragged or typed.
    pub(crate) fn drag(&mut self, label: &str, value: DragValue) {
        self.row(label, |ui, l| ui.add_sized([ui.available_width(), ui.spacing().interact_size.y], value).labelled_by(l));
    }

    /// A row of a colour.
    pub(crate) fn color(&mut self, label: &str, rgb: &mut [u8; 3]) {
        self.row(label, |ui, l| ui.color_edit_button_srgb(rgb).labelled_by(l));
    }

    /// A row of what is worked out, not typed: a formula's value.
    pub(crate) fn shown(&mut self, label: &str, value: &str) {
        self.row(label, |ui, _| ui.label(RichText::new(value).color(pal().muted)));
    }
}

/// What stopped OK or Apply, shown above the fixed footer.
pub(crate) fn error(ui: &mut Ui, error: Option<&str>) {
    if let Some(e) = error {
        ui.add_space(6.0);
        crate::style::boxed_text(ui, "type-properties-error", pal().error, e, crate::style::ERROR_HEIGHT);
    }
}

/// OK, Cancel and Apply in the fixed footer. Which were clicked.
pub(crate) fn foot(ui: &mut Ui) -> (bool, bool, bool) {
    let (mut ok, mut cancel, mut apply) = (false, false, false);
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let size = vec2(76.0, 0.0);
        apply = ui.add(Button::new(t("Apply")).min_size(size)).clicked();
        cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
        ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
    });
    (ok, cancel, apply)
}

/// How tall [`foot`] is: what the table leaves under it.
pub(crate) fn foot_height(ui: &Ui, error: bool) -> f32 {
    10.0 + dialog_row(ui) + if error { 6.0 + crate::style::ERROR_HEIGHT } else { 0.0 }
}

impl crate::BimApp {
    /// Run a command from a Type Properties dialog: whether it was done, and, when it wasn't, why in `error`, which the dialog
    /// says in itself (a status in the view's corner can lie under it).
    pub(crate) fn run_for_type(&mut self, id: &str, params: serde_json::Value, error: &mut Option<String>) -> bool {
        let done = self.run(id, params).is_some();
        *error = (!done).then(|| self.status.text.clone());
        done
    }
}
