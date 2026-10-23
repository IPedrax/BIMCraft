//! Sketch mode: Revit's Edit Boundary, and a floor's Boundary Line or a ceiling's Sketch Ceiling. The outline is drawn as
//! loose lines, held here until Finish Edit Mode hands them to the core, which solves them (`bimcraft_core::sketch`) or
//! names what's wrong; the lines it names are drawn in the error colour until the sketch changes.

use std::collections::BTreeSet;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{Element, Id, TOL};
use bimcraft_core::sketch::{self as solver, Line};
use egui::{Color32, Modifiers, Painter, Pos2, Stroke, Ui};
use serde_json::json;

use crate::icons::Icon;
use crate::style::pal;
use crate::{BimApp, Status, Tool, big_button, panel};

/// What Finish Edit Mode makes of the lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Of {
    /// A new floor on the level.
    Floor(Id),
    /// A new ceiling on the level.
    Ceiling(Id),
    /// The element's outline and holes, redrawn.
    Edit(Id),
}

/// A sketch being drawn: its lines, those picked (Modify, for Delete), and those the last Finish found wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub of: Of,
    pub lines: Vec<Line>,
    pub picked: BTreeSet<usize>,
    pub wrong: Vec<usize>,
}

/// Sketch lines in the plan: Revit's magenta.
const SKETCH: Color32 = Color32::from_rgb(0xC0, 0x26, 0xD3);

impl Sketch {
    /// What the contextual tab calls it.
    pub fn title(&self) -> &'static str {
        match self.of {
            Of::Floor(_) => "Create Floor Boundary",
            Of::Ceiling(_) => "Create Ceiling Boundary",
            Of::Edit(_) => "Edit Boundary",
        }
    }

    /// The line nearest `p`, within `reach` mm of it.
    pub fn line_at(&self, p: P2, reach: f64) -> Option<usize> {
        let d = |l: &Line| geom::dist_to_segment(p, l[0], l[1]);
        self.lines.iter().enumerate().filter(|(_, l)| d(l) <= reach).min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map(|(i, _)| i)
    }

    /// The end of a line nearest `p`, within `reach` mm of it: where the next line snaps.
    pub fn end_at(&self, p: P2, reach: f64) -> Option<P2> {
        self.lines.iter().flatten().copied().filter(|q| q.dist(p) <= reach).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
    }

    /// A line added, unless it's too short to be one.
    fn add(&mut self, l: Line) {
        if l[0].dist(l[1]) > TOL {
            self.lines.push(l);
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.picked.clear();
        self.wrong.clear();
    }

    /// The lines, each in its colour: picked or under the cursor as a selection is, wrong in the error colour.
    pub fn paint(&self, p: &Painter, at: impl Fn(P2) -> Pos2, hovered: Option<usize>) {
        for (i, [a, b]) in self.lines.iter().enumerate() {
            let (width, colour) = match () {
                () if self.picked.contains(&i) => (3.0, pal().accent),
                () if hovered == Some(i) => (3.0, pal().hover),
                () if self.wrong.contains(&i) => (3.0, pal().error),
                () => (2.0, SKETCH),
            };
            p.line_segment([at(*a), at(*b)], Stroke::new(width, colour));
        }
    }
}

impl BimApp {
    /// Sketch mode on: the lines given (none for a new outline), drawing them with Line.
    pub(crate) fn start_sketch(&mut self, of: Of, lines: Vec<Line>) {
        self.sketch = Some(Sketch { of, lines, picked: BTreeSet::new(), wrong: Vec::new() });
        self.selection.clear();
        self.status = Status::default();
        self.tool = Tool::SketchLine(Vec::new());
        self.raise_context();
    }

    /// Revit's Edit Boundary on the floor, ceiling, roof or shaft selected: its outline and holes as lines to edit.
    pub(crate) fn edit_boundary(&mut self) {
        let &[id] = self.selection.as_slice() else { return };
        if let Some(r) = self.run("edit_boundary", json!({"id": id}))
            && let Ok(lines) = serde_json::from_value::<Vec<Line>>(r["sketch"].clone())
        {
            self.start_sketch(Of::Edit(id), lines);
        }
    }

    /// The one element selected, when it has an outline Edit Boundary redraws.
    pub(crate) fn boundary_selected(&self) -> bool {
        match self.selection.as_slice() {
            [id] => match self.session.model.elements.get(*id) {
                Some(Element::Floor(_) | Element::Ceiling(_) | Element::Shaft(_)) => true,
                Some(Element::Roof(r)) => r.extrusion.is_none(),
                _ => false,
            },
            _ => false,
        }
    }

    /// Finish Edit Mode: the lines made into what the sketch is for, selected; or, when the core can't solve them, why,
    /// and the lines that are wrong marked, still sketching.
    pub(crate) fn finish_sketch(&mut self) {
        let Some(sk) = &self.sketch else { return };
        let lines = json!(sk.lines);
        let (id, p) = match sk.of {
            Of::Floor(level) => {
                ("floor", self.typed("floor", json!({"level": level, "structural": self.floor_structural, "sketch": lines})))
            }
            Of::Ceiling(level) => {
                let mut p = self.typed("ceiling", json!({"level": level, "sketch": lines}));
                if let Some(offset) = self.session.model.units.parse(&self.ceiling_height) {
                    p["offset"] = json!(offset);
                }
                ("ceiling", p)
            }
            Of::Edit(id) => ("edit_boundary", json!({"id": id, "sketch": lines})),
        };
        match self.run(id, p) {
            Some(made) => {
                self.sketch = None;
                self.tool = Tool::Modify;
                self.selection = made["id"].as_u64().into_iter().collect();
            }
            None => {
                if let Some(sk) = &mut self.sketch {
                    let problems = solver::solve_one(&sk.lines).err().unwrap_or_default();
                    sk.wrong = problems.iter().flat_map(solver::Problem::lines).collect();
                }
            }
        }
    }

    /// Cancel Edit Mode: the lines dropped, the model as it was.
    pub(crate) fn cancel_sketch(&mut self) {
        let edited = match self.sketch.take() {
            Some(Sketch { of: Of::Edit(id), .. }) => Some(id),
            _ => None,
        };
        self.tool = Tool::Modify;
        self.status = Status::default();
        self.selection = edited.into_iter().collect();
    }

    /// Ctrl+Z while sketching: the last line drawn taken back.
    pub(crate) fn unsketch(&mut self) {
        if let Some(sk) = &mut self.sketch {
            sk.lines.pop();
            sk.changed();
        }
        if let Tool::SketchLine(chain) = &mut self.tool {
            chain.clear();
        }
    }

    /// Delete while sketching: the lines picked taken out.
    pub(crate) fn delete_sketch_lines(&mut self) {
        if let Some(sk) = &mut self.sketch {
            let picked = std::mem::take(&mut sk.picked);
            let mut i = 0;
            sk.lines.retain(|_| {
                i += 1;
                !picked.contains(&(i - 1))
            });
            sk.changed();
        }
    }

    /// A click while sketching, at `p` (snapped) or picking near `raw`, `reach` mm round it: Line's next point (back on
    /// the first, the loop is closed and the next line starts anew), Rectangle's corner, or with Modify a line picked.
    pub(crate) fn sketch_click(&mut self, p: P2, raw: P2, reach: f64, mods: Modifiers) {
        let Some(sk) = &mut self.sketch else { return };
        match &mut self.tool {
            Tool::Modify => {
                let hit = sk.line_at(raw, reach);
                if !(mods.shift || mods.command || mods.ctrl) {
                    sk.picked.clear();
                }
                if let Some(i) = hit
                    && !sk.picked.remove(&i)
                {
                    sk.picked.insert(i);
                }
            }
            Tool::SketchLine(chain) => match chain.last().copied() {
                Some(last) if last.dist(p) > TOL => {
                    sk.add([last, p]);
                    if chain.len() >= 2 && chain.first().is_some_and(|f| f.dist(p) <= TOL) {
                        chain.clear();
                    } else {
                        chain.push(p);
                    }
                }
                Some(_) => {}
                None => chain.push(p),
            },
            Tool::SketchRect(start) => match start.take() {
                Some(a) if (a.0 - p.0).abs() > TOL && (a.1 - p.1).abs() > TOL => {
                    let c = [a, P2(p.0, a.1), p, P2(a.0, p.1)];
                    for k in 0..4 {
                        sk.add([c[k], c[(k + 1) % 4]]);
                    }
                }
                Some(a) => *start = Some(a),
                None => *start = Some(p),
            },
            _ => {}
        }
    }

    /// The contextual tab while sketching: Revit's Mode panel (Finish, Cancel) and Draw panel (Line, Rectangle).
    pub(crate) fn sketch_panels(&mut self, ui: &mut Ui) {
        panel(ui, "Mode", |ui| {
            let tip = "Finish Edit Mode: the lines, closed into loops, become the outline and its holes";
            if big_button(ui, Icon::Check, "Finish Edit Mode", false, tip).clicked() {
                self.finish_sketch();
            }
            if big_button(ui, Icon::Close, "Cancel Edit Mode", false, "Cancel Edit Mode: drop the lines, keep it as it was").clicked() {
                self.cancel_sketch();
            }
        });
        panel(ui, "Draw", |ui| {
            let line = matches!(self.tool, Tool::SketchLine(_));
            if big_button(ui, Icon::Line, "Line", line, "Line: a boundary line between each two points clicked").clicked() && !line {
                self.tool = Tool::SketchLine(Vec::new());
            }
            let rect = matches!(self.tool, Tool::SketchRect(_));
            if big_button(ui, Icon::Rectangle, "Rectangle", rect, "Rectangle: four boundary lines between two opposite corners").clicked()
                && !rect
            {
                self.tool = Tool::SketchRect(None);
            }
        });
    }

    /// The Floor and Ceiling tools' Draw panel: their outline sketched instead of found from the walls.
    pub(crate) fn sketch_starters(&mut self, ui: &mut Ui) {
        let Some(level) = self.plan_level() else { return };
        let (of, label, tip) = match self.tool {
            Tool::Floor => (Of::Floor(level), "Boundary Line", "Boundary Line: sketch the floor's outline, and any openings in it"),
            Tool::Ceiling => (Of::Ceiling(level), "Sketch Ceiling", "Sketch Ceiling: sketch the ceiling's outline, and any openings in it"),
            _ => return,
        };
        panel(ui, "Draw", |ui| {
            if big_button(ui, Icon::Line, label, false, tip).clicked() {
                self.start_sketch(of, Vec::new());
            }
        });
    }

    /// What the status bar says while sketching, or None when not.
    pub(crate) fn sketch_prompt(&self) -> Option<&'static str> {
        self.sketch.as_ref()?;
        Some(match &self.tool {
            Tool::SketchLine(chain) if chain.is_empty() => "Click where the line starts.",
            Tool::SketchLine(_) => "Click the line's end; the next starts there. The first point again closes the loop; Esc stops.",
            Tool::SketchRect(None) => "Click a corner of the rectangle.",
            Tool::SketchRect(Some(_)) => "Click the opposite corner. Esc cancels.",
            _ => "Pick sketch lines to delete; draw with Line or Rectangle. Finish Edit Mode when every loop is closed.",
        })
    }
}
