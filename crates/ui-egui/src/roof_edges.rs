//! Explicit roof pitch selection. Numbered eaves identify their sloping planes; draft pitches
//! reach the shared `modify` command together, without changing the roof's default/eave datum.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{Element, Id, Model};
use egui::{Align2, FontId, Modifiers, Painter, Pos2, Shape, Stroke, Ui, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::style::canvas_pal;
use crate::{BimApp, Sheet, Status, Tool};

struct Edge {
    rings: Vec<Vec<[f64; 3]>>,
    eaves: Vec<[[f64; 3]; 2]>,
}

pub(crate) struct RoofEdgeEdit {
    before: Arc<Model>,
    id: Id,
    edges: BTreeMap<usize, Edge>,
    picked: BTreeSet<usize>,
    slopes: Vec<f64>,
    default: f64,
    original: Vec<f64>,
    error: Option<String>,
}

impl RoofEdgeEdit {
    fn new(before: Arc<Model>, id: Id) -> Result<Self, String> {
        let Some(Element::Roof(r)) = before.elements.get(id) else { return Err(t("Select a footprint roof.").into()) };
        let mut edges = BTreeMap::<usize, Edge>::new();
        for (index, (loops, plane)) in before.roof_faces_by_edge(r)? {
            let Some((a, b)) = r.edges().nth(index) else { continue };
            let rise = before.roof_rise(r, &plane);
            let lift = |q: P2| [q.0, q.1, plane.at(q) + rise];
            let edge = edges.entry(index).or_insert_with(|| Edge { rings: Vec::new(), eaves: Vec::new() });
            for ring in loops {
                edge.eaves.extend(
                    geom::ring_edges(&ring)
                        .filter(|(c, d)| geom::dist_to_segment(*c, a, b) < 1e-5 && geom::dist_to_segment(*d, a, b) < 1e-5)
                        .map(|(c, d)| [lift(c), lift(d)]),
                );
                edge.rings.push(ring.into_iter().map(lift).collect());
            }
        }
        if edges.is_empty() {
            return Err(t("This roof has no slope-defining footprint edges.").into());
        }
        let mut slopes = r.slopes.clone();
        slopes.resize(r.edges().count(), 0.0);
        let default = r.slope;
        Ok(Self { before, id, edges, picked: BTreeSet::new(), original: slopes.clone(), slopes, default, error: None })
    }

    fn pending(&self) -> Vec<usize> {
        self.slopes.iter().zip(&self.original).enumerate().filter_map(|(i, (a, b))| (a != b).then_some(i)).collect()
    }

    pub(crate) fn hit(&self, at: Pos2, project: impl Fn([f64; 3]) -> Pos2) -> Option<usize> {
        self.edges
            .iter()
            .flat_map(|(i, edge)| edge.eaves.iter().map(|line| (*i, crate::sketch3d::to_segment(at, project(line[0]), project(line[1])))))
            .filter(|(_, distance)| *distance <= crate::canvas::PICK)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    pub(crate) fn paint(&self, p: &Painter, project: impl Fn([f64; 3]) -> Pos2, hover: Option<Pos2>) {
        let aimed = hover.and_then(|at| self.hit(at, &project));
        let ink = canvas_pal();
        for (i, edge) in &self.edges {
            let selected = self.picked.contains(i);
            let lit = selected || aimed == Some(*i);
            let color = if selected {
                ink.accent
            } else if lit {
                ink.hover
            } else {
                ink.text
            };
            if lit {
                for ring in &edge.rings {
                    p.add(Shape::closed_line(ring.iter().map(|q| project(*q)).collect(), Stroke::new(3.0, color)));
                }
            }
            for line in &edge.eaves {
                let ends = line.map(&project);
                p.line_segment(ends, Stroke::new(if lit { 7.0 } else { 4.0 }, ink.surface));
                p.line_segment(ends, Stroke::new(if lit { 4.0 } else { 2.0 }, color));
            }
            if let Some(line) = edge.eaves.first() {
                let at = project(line[0]).lerp(project(line[1]), 0.5) + vec2(0.0, -12.0);
                let label = tf("Edge {}", &[&(i + 1).to_string()]);
                let font = FontId::proportional(13.0);
                let galley = p.layout_no_wrap(label.clone(), font.clone(), color);
                let rect = egui::Rect::from_center_size(at, galley.size() + vec2(10.0, 6.0));
                p.rect_filled(rect, 3.0, ink.surface);
                p.text(at, Align2::CENTER_CENTER, label, font, color);
            }
        }
    }
}

impl BimApp {
    pub(crate) fn roof_edge_launcher(&mut self, ui: &mut Ui) -> bool {
        let [id] = self.selection.as_slice() else { return false };
        let id = *id;
        let Some(Element::Roof(r)) = self.session.model.elements.get(id) else { return false };
        if r.extrusion.is_some() || r.slope == 0.0 || self.sketch.is_some() {
            return false;
        }
        ui.label(t("Whole roof selected"));
        if !ui.button(t("Edit roof pitches")).clicked() {
            return false;
        }
        match RoofEdgeEdit::new(Arc::clone(&self.session.model), id) {
            Ok(edit) => {
                let level = self.session.model.elements.id(r.level);
                self.set_tool(Tool::Modify);
                if !matches!(self.sheet, Sheet::Plan(_) | Sheet::View3D)
                    && let Some(level) = level
                {
                    self.open_sheet(Sheet::Plan(level));
                }
                self.selection.clear();
                self.roof_edge_edit = Some(edit);
                self.status = Status::default();
            }
            Err(text) => self.status = Status { text, error: true },
        }
        true
    }

    pub(crate) fn roof_edge_pick(&mut self, edge: Option<usize>, mods: Modifiers) {
        let Some(edit) = &mut self.roof_edge_edit else { return };
        if !(mods.ctrl || mods.command || mods.shift) {
            edit.picked.clear();
        }
        if let Some(edge) = edge {
            if mods.shift {
                edit.picked.remove(&edge);
            } else if !edit.picked.remove(&edge) {
                edit.picked.insert(edge);
            }
        }
        self.selection.clear();
    }

    pub(crate) fn cancel_roof_edge_edit(&mut self) -> bool {
        let Some(edit) = self.roof_edge_edit.take() else { return false };
        self.selection = self.session.model.elements.get(edit.id).map(|_| edit.id).into_iter().collect();
        true
    }

    pub(crate) fn check_roof_edge_edit(&mut self) {
        if self.roof_edge_edit.is_some() && !self.selection.is_empty() {
            self.roof_edge_edit = None;
            self.status = Status { text: t("Pitch edits cancelled because the selection changed.").into(), error: false };
        }
        if self.roof_edge_edit.is_some() && !matches!(self.sheet, Sheet::Plan(_) | Sheet::View3D) {
            self.cancel_roof_edge_edit();
            self.status = Status { text: t("Pitch edits cancelled. Edit roof pitches in a plan or 3D view.").into(), error: false };
        }
        if self.roof_edge_edit.as_ref().is_some_and(|edit| !Arc::ptr_eq(&edit.before, &self.session.model)) {
            self.cancel_roof_edge_edit();
            self.status =
                Status { text: t("The project changed; select the roof edges again before editing their pitches.").into(), error: true };
        }
    }

    pub(crate) fn roof_edge_properties(&mut self, ui: &mut Ui) {
        self.check_roof_edge_edit();
        let Some(mut edit) = self.roof_edge_edit.take() else { return };
        ui.heading(tf("Roof edges: {} selected", &[&edit.picked.len().to_string()]));
        ui.label(t("Click an eave to select its outlined plane. Ctrl/Command-click toggles edges; Shift-click removes."));
        ui.separator();
        if !edit.picked.is_empty() {
            let effective = |i: &usize| edit.slopes.get(*i).copied().filter(|s| *s > 0.0).unwrap_or(edit.default);
            let first = edit.picked.first().map(effective).unwrap_or(edit.default);
            let mixed = edit.picked.iter().any(|i| (effective(i) - first).abs() > 1e-9);
            let current = if mixed { t("Varies").to_string() } else { crate::properties::roof_pitch_text(first) };
            let mut typed = None;
            egui::Grid::new(("roof-edge-pitch", &edit.picked)).num_columns(2).show(ui, |ui| {
                let label = ui.label(t("Selected pitch"));
                typed = self.field(ui, edit.id, "roof-edge-pitch", current, label.id, t("Degrees, 6:12 or 50%"));
                ui.end_row();
            });
            if let Some(typed) = typed {
                let pitch = if typed.trim().is_empty() { Some(0.0) } else { crate::properties::roof_pitch(&typed) };
                match pitch {
                    Some(pitch) => {
                        for i in &edit.picked {
                            if let Some(slope) = edit.slopes.get_mut(*i) {
                                *slope = pitch;
                            }
                        }
                        edit.error = None;
                    }
                    None => edit.error = Some(t("Enter degrees from 0 to 75, or a positive pitch such as 6:12 or 50%.").into()),
                }
            }
            ui.label(t("Degrees, 6:12 or 50%. Press Enter, then Apply. Blank/0 uses the default."));
        }
        let pending = edit.pending();
        if !pending.is_empty() {
            let edges = pending.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(", ");
            ui.label(tf("Pending pitch edits: edges {}", &[&edges]));
        }

        if let Some(error) = &edit.error {
            ui.colored_label(crate::style::pal().error, error);
        }
        let (apply, cancel) = ui
            .horizontal_wrapped(|ui| {
                let apply = ui.add_enabled(!pending.is_empty() && edit.error.is_none(), egui::Button::new(t("Apply pitches"))).clicked();
                (apply, ui.button(t("Cancel pitch edit")).clicked())
            })
            .inner;
        ui.label(t("Apply updates all pending edges. Cancel discards them."));
        ui.label(t("The shared eave underside stays at its height; ridges and attached walls follow the edited planes."));
        if apply {
            if self.run("modify", json!({"id":edit.id,"slopes":edit.slopes})).is_some() {
                self.selection = vec![edit.id];
            } else {
                self.roof_edge_edit = Some(edit);
            }
        } else if cancel {
            self.selection = vec![edit.id];
        } else {
            self.roof_edge_edit = Some(edit);
        }
    }
}
