//! Curtain walls in the desktop: Edit Pattern, the window where a type's grid is made by adding segments and picking the panel
//! and the mullion of each; and the message when a finished change leaves a wall's last segment too small for a panel.

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::curtain::{self, Address, Continuous, CurtainEdits, CurtainLayout, Direction, Fit, Join, Justify, MullionPick, Way};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::units::Units;
use bimcraft_core::walls;
use bimcraft_i18n::msg;
use egui::{Button, Color32, Context, Id as UiId, Rect, Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{canvas_pal, pal};
use crate::typeprops;

/// A place on the pattern that holds a type or nothing: `row` and `column` of its segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Piece {
    Cell(usize, usize),
    Vertical(usize, usize),
    Horizontal(usize, usize),
}

/// Edit Pattern: one curtain wall type's pattern as it's being made, which OK or Apply puts on the type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurtainEditor {
    name: String,
    l: CurtainLayout,
    columns: Vec<String>,
    rows: Vec<String>,
    smallest: String,
    selected: Vec<Piece>,
    error: Option<String>,
}

/// A column's name: A, B… Z, AA.
fn letter(i: usize) -> String {
    let c = |k: usize| char::from(b'A' + (k % 26) as u8);
    if i < 26 { c(i).to_string() } else { format!("{}{}", c(i / 26 - 1), c(i)) }
}

/// A combo box in a row of its own, named `label`: what was picked.
fn row(ui: &mut Ui, label: &str, current: &str, options: &[String]) -> Option<String> {
    ui.horizontal(|ui| {
        let name = ui.add_sized(vec2(130.0, 20.0), egui::Label::new(label));
        crate::properties::combo(ui, (0, label), name.id, current, options)
    })
    .inner
}

/// A row choosing one of `options`, each a value and its label.
fn choice<T: Copy + PartialEq>(ui: &mut Ui, label: &str, value: &mut T, options: &[(T, &str)]) {
    let labels: Vec<String> = options.iter().map(|(_, l)| t(l).to_string()).collect();
    let now = options.iter().position(|(v, _)| v == value).and_then(|i| labels.get(i)).cloned().unwrap_or_default();
    if let Some(picked) = row(ui, label, &now, &labels)
        && let Some((v, _)) = labels.iter().position(|l| *l == picked).and_then(|i| options.get(i))
    {
        *value = *v;
    }
}

impl CurtainEditor {
    fn new(m: &Model, name: &str) -> Option<CurtainEditor> {
        let l = m.types.wall.get(name)?.curtain.clone()?;
        let typed = |d: &Direction, u: Units| d.segments.iter().map(|s| u.length(*s)).collect::<Vec<_>>();
        Some(CurtainEditor {
            name: name.to_string(),
            columns: typed(&l.columns, m.units),
            rows: typed(&l.rows, m.units),
            smallest: m.units.length(l.min_panel),
            l,
            selected: Vec::new(),
            error: None,
        })
    }

    fn add_column(&mut self) {
        let size = self.l.columns.segments.last().copied().unwrap_or(1000.0);
        self.l.columns.segments.push(size);
        self.columns.push(self.columns.last().cloned().unwrap_or_default());
        for row in &mut self.l.panels {
            row.push(row.last().cloned().flatten());
        }
        for row in self.l.vertical.iter_mut().chain(self.l.horizontal.iter_mut()) {
            row.push(row.last().cloned().flatten());
        }
    }

    fn add_row(&mut self) {
        let size = self.l.rows.segments.last().copied().unwrap_or(1000.0);
        self.l.rows.segments.push(size);
        self.rows.push(self.rows.last().cloned().unwrap_or_default());
        self.l.panels.push(self.l.panels.last().cloned().unwrap_or_default());
        self.l.vertical.push(self.l.vertical.last().cloned().unwrap_or_default());
        self.l.horizontal.push(self.l.horizontal.last().cloned().unwrap_or_default());
    }

    fn remove_column(&mut self) {
        if self.l.columns.segments.len() > 1 {
            self.l.columns.segments.pop();
            self.columns.pop();
            self.l.panels.iter_mut().for_each(|r| drop(r.pop()));
            self.l.vertical.iter_mut().chain(self.l.horizontal.iter_mut()).for_each(|r| drop(r.pop()));
            self.selected.clear();
        }
    }

    fn remove_row(&mut self) {
        if self.l.rows.segments.len() > 1 {
            self.l.rows.segments.pop();
            self.rows.pop();
            self.l.panels.pop();
            self.l.vertical.pop();
            self.l.horizontal.pop();
            self.selected.clear();
        }
    }

    /// What's typed put on the pattern, or what isn't a length.
    fn read(&mut self, units: Units) -> Result<(), String> {
        let read = |typed: &[String], sizes: &mut [f64]| -> Result<(), String> {
            for (text, size) in typed.iter().zip(sizes.iter_mut()) {
                *size = units.parse(text).ok_or_else(|| tf("`{}` is not a length", &[text]))?;
            }
            Ok(())
        };
        read(&self.columns, &mut self.l.columns.segments)?;
        read(&self.rows, &mut self.l.rows.segments)?;
        self.l.min_panel = units.parse(&self.smallest).ok_or_else(|| tf("`{}` is not a length", &[&self.smallest]))?;
        Ok(())
    }

    /// What the selection's panels hold, if they all hold the same; `None` in the result is their being mixed.
    fn panel_now(&self) -> Option<Option<String>> {
        let mut held = self.selected.iter().filter_map(|p| match p {
            Piece::Cell(r, c) => self.l.panels.get(*r).and_then(|x| x.get(*c)).cloned(),
            _ => None,
        });
        let first = held.next()?;
        held.all(|h| h == first).then_some(first)
    }

    fn edge(&self, p: Piece) -> Option<&Option<MullionPick>> {
        match p {
            Piece::Vertical(r, c) => self.l.vertical.get(r).and_then(|x| x.get(c)),
            Piece::Horizontal(r, c) => self.l.horizontal.get(r).and_then(|x| x.get(c)),
            Piece::Cell(..) => None,
        }
    }

    fn edge_mut(&mut self, p: Piece) -> Option<&mut Option<MullionPick>> {
        match p {
            Piece::Vertical(r, c) => self.l.vertical.get_mut(r).and_then(|x| x.get_mut(c)),
            Piece::Horizontal(r, c) => self.l.horizontal.get_mut(r).and_then(|x| x.get_mut(c)),
            Piece::Cell(..) => None,
        }
    }

    fn name_of(p: Piece) -> String {
        match p {
            Piece::Cell(r, c) => format!("{} {}{}", t("Panel"), letter(c), r + 1),
            Piece::Vertical(r, c) => format!("{} {}{}", t("Vertical"), letter(c), r + 1),
            Piece::Horizontal(r, c) => format!("{} {}{}", t("Horizontal"), letter(c), r + 1),
        }
    }

    /// One module, to scale: click a panel or a mullion to pick it, Shift to add to what's picked.
    fn preview(&mut self, ui: &mut Ui, m: &Model) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
        let total = |d: &[f64]| d.iter().sum::<f64>().max(1.0);
        let (width, height) = (total(&self.l.columns.segments), total(&self.l.rows.segments));
        let k = ((rect.width() - 48.0) as f64 / width).min((rect.height() - 48.0) as f64 / height);
        let size = vec2((width * k) as f32, (height * k) as f32);
        let area = Rect::from_center_size(rect.center(), size);
        let starts = |d: &[f64]| {
            let mut at = 0.0;
            d.iter()
                .map(|s| {
                    let from = at;
                    at += s;
                    (from, at)
                })
                .collect::<Vec<_>>()
        };
        let (cols, rows) = (starts(&self.l.columns.segments), starts(&self.l.rows.segments));
        let x = |u: f64| area.left() + (u * k) as f32;
        let y = |v: f64| area.bottom() - (v * k) as f32;
        let p = ui.painter_at(rect);
        // A sheet of the canvas's own ground, for the wall to stand out from the dialog.
        p.rect_filled(rect, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
        // Painted, and named for tests and screen readers: a widget here would move what comes after the preview.
        let hint = t("Select members in the preview panel to edit them.");
        p.text(rect.center_top() + vec2(0.0, 4.0), egui::Align2::CENTER_TOP, hint, egui::FontId::proportional(11.0), canvas_pal().muted);
        ui.interact(Rect::from_min_size(rect.min, vec2(rect.width(), 18.0)), UiId::new("curtain-hint"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, hint));
        let mut clicked = None;
        let shift = ui.input(|i| i.modifiers.shift);
        let mut hit = |ui: &mut Ui, r: Rect, piece: Piece| {
            let name = Self::name_of(piece);
            let response = ui.interact(r, UiId::new(("curtain-piece", piece)), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
            if response.clicked() {
                clicked = Some(piece);
            }
        };
        let colour =
            |material: &str, or: Color32| m.materials.get(material).map_or(or, |x| Color32::from_rgb(x.color[0], x.color[1], x.color[2]));
        for (r, (v0, v1)) in rows.iter().enumerate() {
            for (c, (u0, u1)) in cols.iter().enumerate() {
                let cell = Rect::from_min_max(pos2(x(*u0), y(*v1)), pos2(x(*u1), y(*v0)));
                let piece = Piece::Cell(r, c);
                let held = self.l.panels.get(r).and_then(|x| x.get(c)).cloned().flatten();
                let fill = held
                    .as_ref()
                    .and_then(|n| m.types.curtain_panel.get(n))
                    .map_or(canvas_pal().panel, |t| colour(&t.material, canvas_pal().panel));
                p.rect_filled(cell, 0.0, fill);
                if self.selected.contains(&piece) {
                    p.rect_stroke(cell.shrink(3.0), 0.0, Stroke::new(2.0, canvas_pal().accent), egui::StrokeKind::Inside);
                }
                hit(ui, cell, piece);
            }
        }
        // The edges over the cells: a click near a line is the line's. The module's far sides are its first lines again, faint.
        let bar =
            |pick: Option<&MullionPick>| pick.and_then(|q| m.types.mullion.get(&q.type_name)).map(|t| ((t.width() * k) as f32).max(2.0));
        for (r, (v0, v1)) in rows.iter().enumerate() {
            for (c, (u0, _)) in cols.iter().enumerate() {
                let piece = Piece::Vertical(r, c);
                let at = x(*u0);
                let w = bar(self.edge(piece).and_then(Option::as_ref));
                let line = Rect::from_min_max(pos2(at, y(*v1)), pos2(at, y(*v0)));
                let selected = self.selected.contains(&piece);
                paint_edge(&p, line, w, selected, true);
                hit(ui, line.expand2(vec2(6.0, -6.0)), piece);
            }
            for (c, (u0, u1)) in cols.iter().enumerate() {
                let piece = Piece::Horizontal(r, c);
                let at = y(*v0);
                let w = bar(self.edge(piece).and_then(Option::as_ref));
                let line = Rect::from_min_max(pos2(x(*u0), at), pos2(x(*u1), at));
                let selected = self.selected.contains(&piece);
                paint_edge(&p, line, w, selected, false);
                hit(ui, line.expand2(vec2(-6.0, 6.0)), piece);
            }
        }
        p.rect_stroke(area, 0.0, Stroke::new(1.0, canvas_pal().border), egui::StrokeKind::Middle);
        // The segments' names, as the lists beside it call them.
        let font = egui::FontId::proportional(12.0);
        for (c, (u0, u1)) in cols.iter().enumerate() {
            let at = pos2((x(*u0) + x(*u1)) / 2.0, area.bottom() + 3.0);
            p.text(at, egui::Align2::CENTER_TOP, letter(c), font.clone(), canvas_pal().text);
        }
        for (r, (v0, v1)) in rows.iter().enumerate() {
            let at = pos2(area.left() - 5.0, (y(*v0) + y(*v1)) / 2.0);
            p.text(at, egui::Align2::RIGHT_CENTER, (r + 1).to_string(), font.clone(), canvas_pal().text);
        }
        if let Some(piece) = clicked {
            if !shift {
                self.selected.clear();
            }
            if !self.selected.contains(&piece) {
                self.selected.push(piece);
            }
        }
    }

    /// What's picked: the panel or mullion type of it, and how a mullion joins.
    fn picked(&mut self, ui: &mut Ui, m: &Model) {
        let none = t("None").to_string();
        let with_none = |names: Vec<&str>| std::iter::once(none.clone()).chain(names.into_iter().map(String::from)).collect::<Vec<_>>();
        if self.selected.iter().any(|p| matches!(p, Piece::Cell(..))) {
            let now = self.panel_now().map(|h| h.unwrap_or_else(|| none.clone())).unwrap_or_default();
            if let Some(name) = row(ui, t("Panel Type"), &now, &with_none(m.type_names("curtainpanel"))) {
                let value = (name != none).then_some(name);
                for &p in &self.selected.clone() {
                    if let Piece::Cell(r, c) = p
                        && let Some(cell) = self.l.panels.get_mut(r).and_then(|x| x.get_mut(c))
                    {
                        cell.clone_from(&value);
                    }
                }
            }
        }
        let edges: Vec<Piece> = self.selected.iter().copied().filter(|p| !matches!(p, Piece::Cell(..))).collect();
        if !edges.is_empty() {
            let held: Vec<Option<MullionPick>> = edges.iter().filter_map(|p| self.edge(*p).cloned()).collect();
            let same = held.first().filter(|f| held.iter().all(|h| h == *f));
            let now = same.map_or(String::new(), |h| h.as_ref().map_or_else(|| none.clone(), |q| q.type_name.clone()));
            if let Some(name) = row(ui, t("Mullion Type"), &now, &with_none(m.type_names("mullion"))) {
                for &p in &edges {
                    if let Some(e) = self.edge_mut(p) {
                        let join = e.as_ref().map_or(Join::Butt, |q| q.join);
                        *e = (name != none).then(|| MullionPick { type_name: name.clone(), join });
                    }
                }
            }
            let mut join = held.iter().flatten().next().map_or(Join::Butt, |q| q.join);
            let was = join;
            choice(ui, t("Join"), &mut join, &[(Join::Butt, msg!("Butt")), (Join::Mitre, msg!("Mitre"))]);
            if join != was {
                for &p in &edges {
                    if let Some(Some(q)) = self.edge_mut(p) {
                        q.join = join;
                    }
                }
            }
        }
    }

    /// A list of a way's segments, with its fit and justification.
    fn way(ui: &mut Ui, columns: bool, d: &mut Direction, typed: &mut [String], units: Units) -> (bool, bool) {
        let (one, fit_label, justify_label) = if columns {
            (t("Column"), t("Column Fit"), t("Column Justification"))
        } else {
            (t("Row"), t("Row Fit"), t("Row Justification"))
        };
        for (i, text) in typed.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let id = if columns { letter(i) } else { (i + 1).to_string() };
                let name = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(format!("{one} {id}")));
                ui.add(TextEdit::singleline(text).desired_width(90.0)).labelled_by(name.id);
                ui.label(units.name());
            });
        }
        let (add, remove) = ui
            .horizontal(|ui| {
                let (add, remove) = if columns { (t("Add Column"), t("Remove Column")) } else { (t("Add Row"), t("Remove Row")) };
                (ui.add(Button::new(add)).clicked(), ui.add_enabled(typed.len() > 1, Button::new(remove)).clicked())
            })
            .inner;
        choice(ui, fit_label, &mut d.fit, &[(Fit::Flexible, msg!("Last Grid Flexible")), (Fit::Fixed, msg!("Fixed Grid"))]);
        choice(
            ui,
            justify_label,
            &mut d.justify,
            &[
                (Justify::Start, msg!("Start")),
                (Justify::Centre, msg!("Centre")),
                (Justify::CentreLine, msg!("Centre Line")),
                (Justify::End, msg!("End")),
            ],
        );
        (add, remove)
    }

    fn borders(&mut self, ui: &mut Ui, m: &Model) {
        let none = t("None").to_string();
        let options: Vec<String> = std::iter::once(none.clone()).chain(m.type_names("mullion").into_iter().map(String::from)).collect();
        let b = &mut self.l.borders;
        for (label, border) in [
            (t("Border Left"), &mut b.left),
            (t("Border Right"), &mut b.right),
            (t("Border Bottom"), &mut b.bottom),
            (t("Border Top"), &mut b.top),
        ] {
            let now = border.as_ref().map_or_else(|| none.clone(), |q| q.type_name.clone());
            if let Some(name) = row(ui, label, &now, &options) {
                let join = border.as_ref().map_or(Join::Butt, |q| q.join);
                *border = (name != none).then_some(MullionPick { type_name: name, join });
            }
        }
    }
}

/// A grid line on the pattern: as wide as its mullion, or thin and pale with none.
fn paint_edge(p: &egui::Painter, line: Rect, width: Option<f32>, selected: bool, upright: bool) {
    let colour = if selected {
        canvas_pal().accent
    } else if width.is_some() {
        canvas_pal().text
    } else {
        canvas_pal().border
    };
    let w = width.unwrap_or(1.0).max(if selected { 3.0 } else { 1.0 });
    let (a, b) = if upright { (line.center_top(), line.center_bottom()) } else { (line.left_center(), line.right_center()) };
    p.line_segment([a, b], Stroke::new(w, colour));
}

/// How near a line, mm at least, the cursor must be for Tab to offer its mullion.
pub(crate) const REACH: f64 = 60.0;

/// Whether a place is on the wall: what stays picked while it is.
pub(crate) fn part_is_there(m: &Model, w: &bimcraft_core::model::Wall, at: Address) -> bool {
    curtain::pattern(m, w).is_some() && curtain::layout(m, w).has(at)
}

/// The curtain wall alone in `selection`, if it's in `under`, and what's at the plan's cut plane under `at`.
pub(crate) fn parts_under(m: &Model, view: Id, selection: &[Id], under: &[Id], at: P2, scale: f64) -> Option<(Id, Vec<Address>)> {
    let &[id] = selection else { return None };
    let w = m.wall(id).filter(|w| curtain::pattern(m, w).is_some() && under.contains(&id))?;
    let axis = walls::Axis::of(w);
    let (along, _) = axis.project(at);
    let up = m.plan_range(view).cut - m.own_base(w);
    let reach = REACH.max(f64::from(crate::canvas::PICK) / scale.max(f64::MIN_POSITIVE));
    Some((id, shown_parts(m, view, curtain::layout(m, w).at(along, up, reach))))
}

/// Of the parts in `list`, those view `view` draws: Tab goes only through what can be seen.
pub(crate) fn shown_parts(m: &Model, view: Id, mut list: Vec<Address>) -> Vec<Address> {
    let look = bimcraft_core::graphics::Look::of(m, view);
    list.retain(|a| look.shows_part(a.kind()));
    list
}

/// Under the cursor in a section or an elevation, or a 3D view's camera: the selected curtain wall `id`'s parts on the ray
/// `from` to `to`, as view `view` draws them.
pub(crate) fn parts_on_ray(m: &Model, view: Id, id: Id, from: [f64; 3], to: [f64; 3]) -> Option<Vec<Address>> {
    let w = m.wall(id).filter(|w| curtain::pattern(m, w).is_some())?;
    let (along, up) = curtain::on_plane(m, w, from, to)?;
    Some(shown_parts(m, view, curtain::layout(m, w).at(along, up, REACH)))
}

/// The part after `now` in `list`, then none: the whole wall again.
pub(crate) fn next_part(now: Option<(Id, Address)>, id: Id, list: &[Address]) -> Option<(Id, Address)> {
    let at = now.filter(|(of, _)| *of == id).and_then(|(_, at)| list.iter().position(|x| *x == at));
    let next = at.map_or(0, |i| i + 1);
    list.get(next).map(|a| (id, *a))
}

impl BimApp {
    /// The part Tab went to: what it is, another type or none, what its pattern says, or a wall in its place.
    pub(crate) fn curtain_part_panel(&mut self, ui: &mut Ui, model: &Model) {
        let Some((id, at)) = self.curtain_part else { return };
        let Some(w) = model.wall(id) else { return };
        let l = curtain::layout(model, w);
        let (word, column, row, names) = match at {
            Address::Cell { column, row } => (t("Panel"), column, row, model.type_names("curtainpanel")),
            Address::Vertical { column, row } => (t("Vertical Mullion"), column, row, model.type_names("mullion")),
            Address::Horizontal { column, row } => (t("Horizontal Mullion"), column, row, model.type_names("mullion")),
        };
        ui.label(egui::RichText::new(tf("{}: column {}, row {}", &[word, &column.to_string(), &row.to_string()])).strong());
        let walled = matches!(at, Address::Cell { column, row } if l.walls.iter().any(|h| (h.column, h.row) == (column, row)));
        if walled {
            ui.label(t("A wall stands here: select it, or delete it to have the panel back."));
            ui.separator();
            return;
        }
        let now = l.parts.iter().find(|p| p.address == at).and_then(|p| l.shapes.get(p.shape as usize)).map(|s| s.type_name.clone());
        let none = t("None");
        let mut pick: Option<Option<String>> = None;
        egui::ComboBox::from_id_salt("curtain-part-type").selected_text(now.clone().unwrap_or_else(|| none.to_string())).show_ui(
            ui,
            |ui| {
                if ui.selectable_label(now.is_none(), none).clicked() {
                    pick = Some(None);
                }
                for n in &names {
                    if ui.selectable_label(now.as_deref() == Some(*n), *n).clicked() {
                        pick = Some(Some(n.to_string()));
                    }
                }
            },
        );
        let edited = w.curtain.as_ref().is_some_and(|e| e.parts.iter().any(|x| x.at == at));
        let reset = edited && ui.button(t("As in the Pattern")).clicked();
        let replace = matches!(at, Address::Cell { .. }) && ui.button(t("Replace with Wall")).clicked();
        ui.separator();
        let mut params = serde_json::to_value(at).unwrap_or_default();
        params["wall"] = json!(id);
        if let Some(name) = pick {
            params["type"] = json!(name);
            self.run("curtain_part", params);
        } else if reset {
            params["reset"] = json!(true);
            self.run("curtain_part", params);
        } else if replace && let Address::Cell { column, row } = at {
            let made = self.run("curtain_panel_wall", json!({"wall": id, "column": column, "row": row}));
            if let Some(wall) = made.and_then(|v| v["id"].as_u64()) {
                self.selection = vec![wall];
                self.curtain_part = None;
            }
        }
    }

    /// Delete with a part picked: that part, and never the wall.
    pub(crate) fn delete_curtain_part(&mut self, id: Id, at: Address) {
        let mut params = serde_json::to_value(at).unwrap_or_default();
        params["wall"] = json!(id);
        params["type"] = serde_json::Value::Null;
        self.run("curtain_part", params);
    }

    /// Edit Pattern on wall type `name`, if it's a curtain wall's.
    pub(crate) fn open_curtain_editor(&mut self, name: &str) {
        self.curtain_editor = CurtainEditor::new(&self.session.model, name);
    }

    /// The pattern of the type a wall being drawn gets, while it's a Fixed Grid along the wall.
    pub(crate) fn drawing_fixed_curtain(&self) -> Option<&Direction> {
        if !matches!(self.tool, crate::Tool::Wall(_)) {
            return None;
        }
        let name = self.types.get("wall")?;
        self.session.model.types.wall.get(name)?.curtain.as_ref().map(|l| &l.columns).filter(|d| d.fit == Fit::Fixed)
    }

    /// Where a wall drawn from `from` toward `to` ends: at its pattern's nearest length, on a Fixed Grid.
    pub(crate) fn fixed_end(
        &self,
        d: Option<&Direction>,
        from: bimcraft_core::geom::P2,
        to: bimcraft_core::geom::P2,
    ) -> bimcraft_core::geom::P2 {
        match d {
            Some(d) if from.dist(to) > bimcraft_core::model::TOL => {
                let at = from + (to - from).unit() * curtain::snap_length(d, from.dist(to));
                bimcraft_core::geom::P2((at.0 * 1e3).round() / 1e3, (at.1 * 1e3).round() / 1e3)
            }
            _ => to,
        }
    }

    /// Edit Pattern, while it's open.
    pub(crate) fn curtain_editor_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.curtain_editor.take() else { return };
        let model = Arc::clone(&self.session.model);
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let r = Modal::new(UiId::new("curtain-pattern"), t("Edit Pattern")).width(860.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.strong(t("Vertical Grid"));
                        let (typed, dir) = (&mut d.columns, &mut d.l.columns);
                        let (add, remove) = CurtainEditor::way(ui, true, dir, typed, model.units);
                        if add {
                            d.add_column();
                        }
                        if remove {
                            d.remove_column();
                        }
                        ui.separator();
                        ui.strong(t("Horizontal Grid"));
                        let (typed, dir) = (&mut d.rows, &mut d.l.rows);
                        let (add, remove) = CurtainEditor::way(ui, false, dir, typed, model.units);
                        if add {
                            d.add_row();
                        }
                        if remove {
                            d.remove_row();
                        }
                        ui.separator();
                        ui.strong(t("Borders"));
                        d.borders(ui, &model);
                        ui.separator();
                        choice(
                            ui,
                            t("Of One Size, Runs Through"),
                            &mut d.l.continuous,
                            &[(Continuous::Vertical, msg!("Vertical Mullions")), (Continuous::Horizontal, msg!("Horizontal Mullions"))],
                        );
                        ui.horizontal(|ui| {
                            let name = ui.add_sized(vec2(130.0, 20.0), egui::Label::new(t("Smallest Panel")));
                            ui.add(TextEdit::singleline(&mut d.smallest).desired_width(90.0)).labelled_by(name.id);
                            ui.label(model.units.name());
                        });
                    });
                    ui.vertical(|ui| {
                        // What's typed shows as it's typed; what isn't a length yet waits, and OK says so.
                        let _ = d.read(model.units);
                        d.preview(ui, &model);
                        let module = |w: &Direction| model.units.length(w.segments.iter().sum());
                        let thick = curtain::envelope(&model, &d.l).map_or(0.0, |(a, b)| a + b);
                        ui.label(tf(
                            "Module {} x {}, thickness {}",
                            &[&module(&d.l.columns), &module(&d.l.rows), &model.units.length(thick)],
                        ));
                        ui.separator();
                        if d.selected.is_empty() {
                            ui.label(t("Click a panel or a mullion to set its type; Shift adds to the selection."));
                        } else {
                            d.picked(ui, &model);
                        }
                    });
                });
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if ok || apply {
            let done = match d.read(model.units) {
                Ok(()) => {
                    let params = json!({"category": "wall", "name": d.name, "curtain": d.l});
                    self.run_for_type("type", params, &mut d.error)
                }
                Err(e) => {
                    d.error = Some(e);
                    false
                }
            };
            ok &= done;
            if done && let Some(open) = &mut self.type_dialog {
                open.refresh(&self.session.model);
            }
        }
        if !(ok || cancel || r.should_close()) {
            self.curtain_editor = Some(d);
        }
    }

    /// Asks about walls a finished change has just left with a segment too small for a panel: once for a wall, while it stays
    /// so, and never for what a project opened with.
    pub(crate) fn curtain_message(&mut self, ctx: &Context) {
        let same = self.curtain_seen.as_ref().is_some_and(|m| Arc::ptr_eq(m, &self.session.model));
        if !same {
            let model = Arc::clone(&self.session.model);
            let now: BTreeSet<Id> = curtain::malformed_walls(&model).into_iter().collect();
            let opened = self.curtain_seen.as_ref().is_none_or(|m| m.uid != model.uid);
            let new: Vec<Id> = now.difference(&self.curtain_known).copied().collect();
            if !opened && !new.is_empty() {
                self.curtain_asking.extend(new);
            }
            self.curtain_asking.retain(|id| now.contains(id));
            self.curtain_known = now;
            self.curtain_seen = Some(model);
        }
        if self.curtain_asking.is_empty() {
            return;
        }
        let (mut remove, mut myself) = (false, false);
        let r = Modal::new(UiId::new("curtain-malformed"), t("Curtain Wall")).width(460.0).show(ctx, |ui| {
            ui.add(egui::Label::new(t(curtain::MALFORMED)).wrap());
            if self.curtain_asking.len() > 1 {
                ui.label(tf("{} walls", &[&self.curtain_asking.len().to_string()]));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                remove = ui.button(t("Remove last interior grid")).clicked();
                myself = ui.button(t("I will fix it myself")).clicked();
            });
        });
        if remove {
            let m = Arc::clone(&self.session.model);
            let params: Vec<serde_json::Value> = self
                .curtain_asking
                .iter()
                .filter_map(|&id| {
                    let Some(Element::Wall(w)) = m.elements.get(id) else { return None };
                    let ways = curtain::layout(&m, w).malformed;
                    let mut edits: CurtainEdits = w.curtain.clone().unwrap_or_default();
                    edits.absorb_columns |= ways.contains(&Way::Columns);
                    edits.absorb_rows |= ways.contains(&Way::Rows);
                    Some(json!({"id": id, "curtain": edits}))
                })
                .collect();
            self.run_all(msg!("Remove last interior grid"), "modify", params);
        }
        if remove || myself || r.should_close() {
            self.curtain_asking.clear();
        }
    }
}
