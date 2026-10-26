//! Revit's Draw panel for detail lines, sketches and walls: which shape the next clicks draw (`bimcraft_core::draw`),
//! the Options Bar's Sides, Radius and Offset, what a click makes, and the preview under the cursor. Also Join into
//! Polyline and Explode Polyline on detail lines and sketch lines.

use bimcraft_core::curves::{self, Curve, Fields};
use bimcraft_core::draw::{self, Options};
pub(crate) use bimcraft_core::draw::{Shape, element_curve};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, TOL};
use bimcraft_core::sketch::SketchLine;
use egui::{Painter, PointerButton, Pos2, Rect, Response, Shape as Paint, Stroke, StrokeKind, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Status, Tool, panel, small_button};

/// What a Draw panel draws for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Detail lines, in the view.
    Detail,
    /// Sketch lines, in sketch mode.
    Sketch,
    /// Walls, by their location lines.
    Wall,
}

/// A line picked for Fillet Arc: a detail line, or a sketch line by its place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picked {
    Element(Id),
    Sketch(usize),
}

/// The shapes' state: which each target draws, the Options Bar's settings as typed, the line before (for a tangent arc),
/// and the lines picked for a fillet.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub detail: Shape,
    pub sketch: Shape,
    pub wall: Shape,
    pub sides: String,
    pub radius_on: bool,
    pub radius: String,
    pub offset: String,
    /// Where the last line drawn ended, and the way it ran there.
    pub last: Option<(P2, P2)>,
    /// Fillet Arc's first line, then its second, each where it was picked.
    pub fillet: Vec<(Picked, P2)>,
    /// The selected detail line's handle being dragged: the line, and where the handle was.
    pub grip: Option<(Id, P2)>,
    /// Revit's Insulation: the Detail Line tool draws batt insulation this wide, as typed, instead of lines.
    pub insulation: Option<String>,
    /// Revit's Revision Cloud: the Detail Line tool draws clouds of scallops this long, of this revision, as typed.
    pub cloud: Option<(String, String)>,
    /// While sketching, Trim/Extend's first line (or boundary) picked, and where.
    pub trim: Option<(usize, P2)>,
    /// The Options Bar's Show Centre: new detail arcs and circles take a centre mark.
    pub centre_mark: bool,
    /// Which of a shape's numbers is being typed (a rectangle's width, 0, or height, 1), and what's typed in the other.
    pub field: usize,
    pub other: String,
}

impl Default for Drawing {
    fn default() -> Drawing {
        Drawing {
            detail: Shape::Line,
            sketch: Shape::Line,
            wall: Shape::Line,
            sides: "6".into(),
            radius_on: false,
            radius: "1000".into(),
            offset: "0".into(),
            last: None,
            fillet: Vec::new(),
            grip: None,
            trim: None,
            insulation: None,
            cloud: None,
            centre_mark: false,
            field: 0,
            other: String::new(),
        }
    }
}

/// The line from `a` to `b`, `by` mm to its left (to its right when negative): Line with the Options Bar's Offset.
pub fn offset_line(a: P2, b: P2, by: f64) -> [P2; 2] {
    let n = (b - a).unit().perp() * by;
    [a + n, b + n]
}

/// The icon a shape's button shows.
pub fn icon(s: Shape) -> Icon {
    match s {
        Shape::Line => Icon::Line,
        Shape::Rectangle => Icon::Rectangle,
        Shape::Inscribed => Icon::Inscribed,
        Shape::Circumscribed => Icon::Circumscribed,
        Shape::Circle => Icon::Circle,
        Shape::Arc => Icon::Arc,
        Shape::CentreArc => Icon::CentreArc,
        Shape::TangentArc => Icon::TangentArc,
        Shape::Fillet => Icon::Fillet,
        Shape::Spline => Icon::Spline,
        Shape::FitSpline => Icon::FitSpline,
        Shape::Ellipse => Icon::Ellipse,
        Shape::PartialEllipse => Icon::PartialEllipse,
        Shape::Polyline => Icon::Polyline,
        Shape::PickLines => Icon::PickLines,
        Shape::PickFaces => Icon::PickFaces,
    }
}

/// What a shape's button says it does.
pub fn tip(s: Shape) -> &'static str {
    match s {
        Shape::Line => "Line: a line between each two points clicked",
        Shape::Rectangle => "Rectangle: four lines between two opposite corners",
        Shape::Inscribed => "Inscribed Polygon: click its centre, then a corner; Sides on the Options Bar",
        Shape::Circumscribed => "Circumscribed Polygon: click its centre, then the middle of a side; Sides on the Options Bar",
        Shape::Circle => "Circle: click its centre, then a point on it (or only its centre, with Radius)",
        Shape::Arc => "Start-End-Radius Arc: click its start, its end, then a point it passes through",
        Shape::CentreArc => "Center-ends Arc: click its centre, its start, then where it ends",
        Shape::TangentArc => "Tangent End Arc: click the end of a line, then where the arc ends; it leaves the way the line ran",
        Shape::Fillet => "Fillet Arc: pick two lines, then click to size the arc (or set its Radius); the lines are cut back to it",
        Shape::Spline => "Spline: click its control points; Enter finishes it",
        Shape::FitSpline => "Spline Through Points: click the points it passes through; Enter finishes it",
        Shape::Ellipse => "Ellipse: click its centre, the end of one axis, then how far the other reaches",
        Shape::PartialEllipse => "Partial Ellipse: click the ends of its axis, then how far it bulges",
        Shape::Polyline => "Polyline: one line through every point clicked; Enter finishes it, the first point again closes it",
        Shape::PickLines => "Pick Lines: click a line, a wall or a grid to draw along it (beside it, with Offset)",
        Shape::PickFaces => "Pick Faces: click a face of a mass for a wall on it, as tall as the mass",
    }
}

impl BimApp {
    /// What the tool in hand draws for.
    pub(crate) fn target(&self) -> Option<Target> {
        match self.tool {
            Tool::DetailLine(_) => Some(Target::Detail),
            Tool::SketchLine(_) if self.sketch.is_some() => Some(Target::Sketch),
            Tool::Wall(_) => Some(Target::Wall),
            _ => None,
        }
    }

    /// The shape the tool in hand draws: Line for one that draws none.
    pub(crate) fn shape(&self) -> Shape {
        match self.target() {
            Some(Target::Detail) => self.drawing.detail,
            Some(Target::Sketch) => self.drawing.sketch,
            Some(Target::Wall) => self.drawing.wall,
            None => Shape::Line,
        }
    }

    /// The shape `target` draws from now, its points so far dropped: a chain going on from where it is.
    pub(crate) fn set_shape(&mut self, target: Target, shape: Shape) {
        let slot = match target {
            Target::Detail => &mut self.drawing.detail,
            Target::Sketch => &mut self.drawing.sketch,
            Target::Wall => &mut self.drawing.wall,
        };
        let was = std::mem::replace(slot, shape);
        self.drawing.fillet.clear();
        let keep = if was.chains() && shape.chains() { self.points().and_then(|p| p.last().copied()) } else { None };
        if let Some(points) = self.points_mut() {
            *points = keep.into_iter().collect();
        }
        if target == Target::Sketch {
            self.tool = match shape {
                _ if !matches!(self.tool, Tool::SketchLine(_)) => Tool::SketchLine(keep.into_iter().collect()),
                _ => self.tool.clone(),
            };
        }
    }

    /// The points the tool in hand has had clicked.
    pub(crate) fn points(&self) -> Option<&Vec<P2>> {
        match &self.tool {
            Tool::DetailLine(p) | Tool::SketchLine(p) | Tool::Wall(p) => Some(p),
            _ => None,
        }
    }

    fn points_mut(&mut self) -> Option<&mut Vec<P2>> {
        match &mut self.tool {
            Tool::DetailLine(p) | Tool::SketchLine(p) | Tool::Wall(p) => Some(p),
            _ => None,
        }
    }

    /// The Options Bar's settings, read from what's typed.
    pub(crate) fn draw_options(&self) -> Options {
        let units = self.session.model.units;
        let sides = self.drawing.sides.trim().parse::<usize>().unwrap_or(6).clamp(3, draw::MAX_SIDES);
        let radius = self.drawing.radius_on.then(|| units.parse(&self.drawing.radius)).flatten().filter(|r| *r > TOL);
        let offset = units.parse(&self.drawing.offset).filter(|o| o.is_finite()).unwrap_or(0.0);
        Options { sides, radius, offset }
    }

    /// The Draw panel for `target`: a button for each shape it draws, two rows of them as Revit's are.
    pub(crate) fn draw_panel(&mut self, ui: &mut Ui, target: Target) {
        let shapes: Vec<Shape> = Shape::ALL
            .into_iter()
            .filter(|s| match target {
                Target::Wall => s.for_walls(),
                Target::Detail | Target::Sketch => s.for_lines(),
            })
            .collect();
        let now = match target {
            Target::Detail => self.drawing.detail,
            Target::Sketch => self.drawing.sketch,
            Target::Wall => self.drawing.wall,
        };
        let picking_walls = matches!(self.tool, Tool::SketchPick);
        panel(ui, "Draw", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let half = shapes.len().div_ceil(2).max(1);
                for (k, row) in shapes.chunks(half).enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for &s in row {
                            let on = s == now && !picking_walls;
                            if small_button(ui, icon(s), s.label(), on, tip(s)).clicked() {
                                self.set_shape(target, s);
                            }
                        }
                        if target == Target::Sketch && k == 1 {
                            let tip = "Pick Walls: a boundary line along each wall clicked";
                            if small_button(ui, Icon::Wall, "Pick Walls", picking_walls, tip).clicked() {
                                self.tool = Tool::SketchPick;
                            }
                        }
                    });
                }
            });
        });
    }

    /// The Options Bar's Sides, Radius and Offset, for the shapes that take them.
    pub(crate) fn shape_options(&mut self, ui: &mut Ui) {
        let shape = self.shape();
        if matches!(shape, Shape::Inscribed | Shape::Circumscribed) {
            let l = ui.label(t("Sides:"));
            ui.add(TextEdit::singleline(&mut self.drawing.sides).desired_width(40.0)).labelled_by(l.id);
        }
        if matches!(shape, Shape::Inscribed | Shape::Circumscribed | Shape::Circle | Shape::Fillet) {
            ui.checkbox(&mut self.drawing.radius_on, t("Radius:"));
            let r = ui.add_enabled(self.drawing.radius_on, TextEdit::singleline(&mut self.drawing.radius).desired_width(70.0));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, t("Radius")));
        }
        // A detail arc or circle can mark its centre, as Revit's Center Mark Visible.
        let round = matches!(shape, Shape::Circle | Shape::Arc | Shape::CentreArc | Shape::TangentArc);
        if round && self.target() == Some(Target::Detail) {
            ui.checkbox(&mut self.drawing.centre_mark, t("Show Centre"));
        }
        if !matches!(shape, Shape::Spline | Shape::FitSpline | Shape::Ellipse | Shape::PartialEllipse | Shape::Fillet) {
            let l = ui.label(t("Offset:"));
            ui.add(TextEdit::singleline(&mut self.drawing.offset).desired_width(70.0)).labelled_by(l.id);
        }
    }

    /// The way the line before ran where it ended at `p`, for a tangent arc: the last one drawn, else any detail line,
    /// sketch line or wall ending (or starting) there.
    pub(crate) fn tangent_at(&self, p: P2) -> Option<P2> {
        if let Some((end, dir)) = self.drawing.last
            && end.dist(p) <= TOL
        {
            return Some(dir);
        }
        let mut curves: Vec<Curve> = Vec::new();
        if let Some(sk) = &self.sketch {
            curves.extend(sk.lines.iter().map(|l| l.curve.clone()));
        }
        let m = &self.session.model;
        curves.extend(
            m.elements.iter().filter_map(|(id, e)| {
                matches!(e, Element::DetailLine(_) | Element::Wall(_)).then(|| draw::element_curve(m, *id)).flatten()
            }),
        );
        curves.iter().find_map(|c| {
            let (t0, t1) = c.domain();
            if c.end().dist(p) <= TOL {
                Some(c.tangent(t1))
            } else if c.start().dist(p) <= TOL {
                Some(c.tangent(t0) * -1.0)
            } else {
                None
            }
        })
    }

    /// A click at `p` of a shape drawn point by point (all but Line, Fillet Arc and Pick Lines): what it makes once it
    /// has its points, which then start over, a chain going on from its end. Spline and Polyline gather points until
    /// [`BimApp::finish_shape`].
    pub(crate) fn shape_click(&mut self, p: P2) -> Option<Vec<Curve>> {
        let (shape, o) = (self.shape(), self.draw_options());
        let tangent = self.points().and_then(|pts| pts.first().copied()).or(Some(p)).and_then(|start| self.tangent_at(start));
        if shape == Shape::TangentArc && self.points().is_some_and(Vec::is_empty) && self.tangent_at(p).is_none() {
            self.status = Status { text: t("Tangent End Arc starts at the end of a line: click one.").into(), error: true };
            return None;
        }
        let chain = match self.target() {
            Some(Target::Wall) => self.chain,
            Some(Target::Detail) => self.line_chain,
            _ => true,
        };
        let points = self.points_mut()?;
        if points.last().is_some_and(|q| q.dist(p) <= TOL) && !(shape == Shape::Polyline && points.len() >= 3) {
            return None;
        }
        points.push(p);
        if shape.open_ended() {
            // A polyline back on its first point closes and is made.
            let closes = shape == Shape::Polyline && points.len() > 3 && points.first().is_some_and(|f| f.dist(p) <= TOL);
            return if closes { self.finish_shape() } else { None };
        }
        if points.len() < shape.clicks(&o) {
            return None;
        }
        let made = draw::make(shape, points, &o, tangent);
        match &made {
            Some(curves) => {
                // Where it ended, as clicked when it ends on a point clicked.
                let end = curves.last().map(|c| {
                    let e = c.end();
                    (points.iter().copied().find(|q| q.dist(e) < 1e-6).unwrap_or(e), c.tangent(c.domain().1))
                });
                points.clear();
                if chain
                    && shape.chains()
                    && let Some((e, _)) = end
                {
                    points.push(e);
                }
                self.drawing.last = end;
            }
            // Points that make nothing (on each other, in a line): the last is taken back.
            None => {
                points.pop();
            }
        }
        made
    }

    /// Enter with Spline, Spline Through Points or Polyline: what the points clicked make, and the points dropped.
    pub(crate) fn finish_shape(&mut self) -> Option<Vec<Curve>> {
        let (shape, o) = (self.shape(), self.draw_options());
        if !shape.open_ended() {
            return None;
        }
        let points = std::mem::take(self.points_mut()?);
        let made = draw::make(shape, &points, &o, None);
        self.drawing.last = made.as_ref().and_then(|c| c.last()).map(|c| (c.end(), c.tangent(c.domain().1)));
        made
    }

    /// Whether a spline or a polyline has points enough to be made now: what Enter, a double click and the right-click
    /// menu's Finish Drawing do.
    pub(crate) fn finishable(&self) -> bool {
        (self.shape().open_ended() || self.one_line()) && self.points().is_some_and(|p| p.len() >= 2)
    }

    /// The spline or polyline of the points clicked, made: detail lines in the plan on screen, or sketch lines.
    pub(crate) fn finish_open_shape(&mut self) {
        match self.tool {
            Tool::DetailLine(_) => {
                if let Some(level) = self.plan_level() {
                    self.finish_detail_line(level);
                }
            }
            Tool::SketchLine(_) => {
                if let Some(made) = self.finish_shape() {
                    self.add_sketch_lines(made);
                }
            }
            _ => {}
        }
    }

    /// How many numbers can be typed for the shape in hand now (see [`draw::numbers`]): none when what's typed is a
    /// length toward the cursor.
    pub(crate) fn typed_fields(&self) -> usize {
        if self.target().is_none() {
            return 0;
        }
        draw::numbers(self.shape(), self.points().map_or(0, Vec::len)).len()
    }

    /// What's typed dropped: a length, or a shape's numbers.
    pub(crate) fn clear_typed(&mut self) {
        self.typed_length.clear();
        self.drawing.other.clear();
        self.drawing.field = 0;
    }

    /// Enter with a shape's numbers typed: its next point where they say, toward the cursor at `cursor`. False when
    /// the shape takes no numbers now (what's typed is a length).
    pub(crate) fn place_numbers(&mut self, level: Id, cursor: P2) -> bool {
        let (shape, n) = (self.shape(), self.points().map_or(0, Vec::len));
        let names = draw::numbers(shape, n);
        if self.target().is_none() || names.is_empty() {
            return false;
        }
        let (typed, other, field) = (std::mem::take(&mut self.typed_length), std::mem::take(&mut self.drawing.other), self.drawing.field);
        self.drawing.field = 0;
        let units = self.session.model.units;
        let mut values = vec![None; names.len()];
        for (i, text) in [(field, typed), (1 - field.min(1), other)] {
            let Some(name) = names.get(i) else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let value = if *name == "Angle" { text.trim().trim_end_matches('°').parse::<f64>().ok() } else { units.parse(&text) };
            match value.filter(|v| v.is_finite() && *v > 0.0 && *v <= bimcraft_core::model::MAX_COORD) {
                Some(v) => values[i] = Some(v),
                None => {
                    let what = if *name == "Angle" { "`{}` is not an angle" } else { "`{}` is not a length" };
                    self.status = Status { text: crate::lang::tf(what, &[text.trim()]), error: true };
                    return true;
                }
            }
        }
        let points = self.points().cloned().unwrap_or_default();
        match draw::typed(shape, &points, cursor, &values) {
            Some(p) => self.put(level, p),
            None => {
                self.status = Status {
                    text: t("That can't be drawn: an arc's radius is at least half the distance between its ends, and a centre-ends arc turns less than 180°.").into(),
                    error: true,
                }
            }
        }
        true
    }

    /// The tool's next point put at `p`, as a click there would (a sketch's line or shape, else [`BimApp::place`]).
    pub(crate) fn put(&mut self, level: Id, p: P2) {
        if matches!(self.tool, Tool::SketchLine(_)) {
            let m = std::sync::Arc::clone(&self.session.model);
            self.sketch_click(&m, p, p, 0.0, None, egui::Modifiers::NONE);
        } else {
            self.place(level, p);
        }
    }

    /// What the shape in hand shows with the cursor at `cursor`.
    pub(crate) fn shape_preview(&self, cursor: P2) -> Vec<Curve> {
        let (shape, o) = (self.shape(), self.draw_options());
        let Some(points) = self.points() else { return Vec::new() };
        // Fillet Arc, its two lines picked: the arc that passes nearest the cursor, and the lines cut back to it.
        if shape == Shape::Fillet {
            let made = self.fillet_curves().zip(self.fillet_radius_at(cursor)).and_then(|((a, b), r)| draw::fillet_lines(&a, &b, r));
            return made.map(|(a, arc, b)| vec![a, arc, b]).unwrap_or_default();
        }
        if matches!(shape, Shape::Line | Shape::PickLines) || (points.is_empty() && shape.clicks(&o) > 1) {
            return Vec::new();
        }
        let tangent = points.first().copied().and_then(|s| self.tangent_at(s));
        draw::preview(shape, points, cursor, &o, tangent)
    }

    /// What the status bar asks for next with the shape in hand, or None for Line (each tool says its own).
    pub(crate) fn shape_prompt(&self) -> Option<&'static str> {
        let shape = self.shape();
        let n = self.points().map_or(0, Vec::len);
        let radius = self.draw_options().radius.is_some();
        Some(match (shape, n) {
            (Shape::Line, _) => return None,
            (Shape::Rectangle, 0) => "Click a corner of the rectangle.",
            (Shape::Rectangle, _) => "Click the opposite corner. Esc cancels.",
            (Shape::Inscribed | Shape::Circumscribed | Shape::Circle, 0) => "Click the centre.",
            (Shape::Inscribed, _) => "Click where a corner goes. Esc cancels.",
            (Shape::Circumscribed, _) => "Click where the middle of a side goes. Esc cancels.",
            (Shape::Circle, _) if !radius => "Click a point on the circle. Esc cancels.",
            (Shape::Circle, _) => "Click the centre.",
            (Shape::Arc, 0) => "Click where the arc starts.",
            (Shape::Arc, 1) => "Click where the arc ends.",
            (Shape::Arc, _) => "Click a point the arc passes through. Esc cancels.",
            (Shape::CentreArc, 0) => "Click the arc's centre.",
            (Shape::CentreArc, 1) => "Click where the arc starts.",
            (Shape::CentreArc, _) => "Click where the arc ends, the shorter way round. Esc cancels.",
            (Shape::TangentArc, 0) => "Click the end of a line: the arc leaves it the way the line ran.",
            (Shape::TangentArc, _) => "Click where the arc ends. Esc cancels.",
            (Shape::Fillet, _) => match self.drawing.fillet.len() {
                0 => "Pick the first line to round.",
                1 => "Pick the second line.",
                _ => "Click to size the arc. Esc cancels.",
            },
            (Shape::Spline, 0) => "Click the spline's first control point.",
            (Shape::Spline, _) => "Click its next control point; Enter finishes it. Esc cancels.",
            (Shape::FitSpline, 0) => "Click the first point the spline passes through.",
            (Shape::FitSpline, _) => "Click the next point it passes through; Enter finishes it. Esc cancels.",
            (Shape::Ellipse, 0) => "Click the ellipse's centre.",
            (Shape::Ellipse, 1) => "Click the end of its first axis.",
            (Shape::Ellipse, _) => "Click how far its other axis reaches. Esc cancels.",
            (Shape::PartialEllipse, 0) => "Click one end of its axis.",
            (Shape::PartialEllipse, 1) => "Click the other end.",
            (Shape::PartialEllipse, _) => "Click how far it bulges, on the side it goes. Esc cancels.",
            (Shape::Polyline, 0) => "Click where the polyline starts.",
            (Shape::Polyline, _) => "Click its next point; Enter finishes it, the first point again closes it. Esc cancels.",
            (Shape::PickLines, _) => "Click a line, a wall or a grid to draw along it.",
            (Shape::PickFaces, _) => "Click a face of a mass: a wall stands on it.",
        })
    }

    /// What a new detail line takes from the Options Bar: its line style, and with Insulation its width.
    pub(crate) fn line_extras(&self, p: &mut Value) {
        if !self.line_style.is_empty() {
            p["style"] = json!(self.line_style);
        }
        if let Some(w) = self.drawing.insulation.as_ref().and_then(|w| self.session.model.units.parse(w)) {
            p["insulation"] = json!(w);
        }
        let round = matches!(self.shape(), Shape::Circle | Shape::Arc | Shape::CentreArc | Shape::TangentArc);
        if self.drawing.centre_mark && round {
            p["centre_mark"] = json!(true);
        }
        if let Some((arc, revision)) = &self.drawing.cloud {
            p["cloud"] = json!(self.session.model.units.parse(arc).unwrap_or(500.0));
            if !revision.trim().is_empty() {
                p["revision"] = json!(revision.trim());
            }
        }
    }

    /// Whether the Detail Line tool's clicks make one line through every point: with Polyline, or a cloud's Line (a cloud
    /// is one line round, as Revit's is one sketch).
    pub(crate) fn one_line(&self) -> bool {
        self.shape() == Shape::Polyline || (self.drawing.cloud.is_some() && self.shape() == Shape::Line)
    }

    /// Annotate's Detail panel: Detail Line, Insulation (detail lines drawn as batt insulation), Revision Cloud, and detail
    /// groups.
    pub(crate) fn detail_panel(&mut self, ui: &mut Ui) {
        let drawing = matches!(self.tool, Tool::DetailLine(_));
        panel(ui, "Detail", |ui| {
            let tip = "Detail Line (DL): each click makes a line from the one before, in the Options Bar's line style; the Draw panel has the other shapes";
            let plain = self.drawing.insulation.is_none() && self.drawing.cloud.is_none();
            if crate::big_button(ui, Icon::Line, "Detail Line", drawing && plain, tip).clicked() {
                self.set_tool(Tool::DetailLine(Vec::new()));
                (self.drawing.insulation, self.drawing.cloud) = (None, None);
            }
            let tip = "Insulation: batt insulation drawn along each line, its width on the Options Bar";
            if crate::big_button(ui, Icon::Insulation, "Insulation", drawing && self.drawing.insulation.is_some(), tip).clicked() {
                self.set_tool(Tool::DetailLine(Vec::new()));
                (self.drawing.insulation, self.drawing.cloud) = (Some("100".into()), None);
            }
            let tip = "Revision Cloud: a cloud round what changed, of the revision on the Options Bar; a rectangle by default, any shape from the Draw panel";
            if crate::big_button(ui, Icon::RevisionCloud, "Revision Cloud", drawing && self.drawing.cloud.is_some(), tip).clicked() {
                self.set_tool(Tool::DetailLine(Vec::new()));
                (self.drawing.insulation, self.drawing.cloud) = (None, Some(("500".into(), String::new())));
                self.drawing.detail = Shape::Rectangle;
            }
            // Groups, stacked three high as Revit's small buttons are.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let tip = "Detail Group: click to place the group on the Options Bar";
                if crate::stacked_button(ui, Icon::DetailGroup, "Detail Group", self.tool == Tool::DetailGroup, tip).clicked() {
                    self.set_tool(Tool::DetailGroup);
                }
                let tip = "Repeating Detail: click its start and end; the group on the Options Bar repeats along it every spacing";
                let on = matches!(self.tool, Tool::RepeatingDetail(_));
                if crate::stacked_button(ui, Icon::RepeatingDetail, "Repeating Detail", on, tip).clicked() {
                    self.set_tool(Tool::RepeatingDetail(None));
                }
                let tip = "Create Group (GP): the selected detail lines, filled regions and text made one detail group";
                if crate::stacked_button(ui, Icon::CreateGroup, "Create Group", false, tip).clicked() {
                    self.create_group();
                }
            });
        });
    }

    /// The Options Bar's Width while drawing insulation.
    pub(crate) fn insulation_options(&mut self, ui: &mut Ui) {
        if let Some(w) = &mut self.drawing.insulation {
            let l = ui.label(t("Width:"));
            ui.add(TextEdit::singleline(w).desired_width(70.0)).labelled_by(l.id);
        }
        if let Some((arc, revision)) = &mut self.drawing.cloud {
            let l = ui.label(t("Arc Length:"));
            ui.add(TextEdit::singleline(arc).desired_width(70.0)).labelled_by(l.id);
            let l = ui.label(t("Revision:"));
            ui.add(TextEdit::singleline(revision).desired_width(90.0)).labelled_by(l.id);
        }
    }

    /// Detail lines along `curves`, in view `view` and the Options Bar's line style, as one step.
    pub(crate) fn make_detail_lines(&mut self, view: Id, curves: &[Curve]) -> Vec<Id> {
        // A cloud's rectangle or polygon is one cloud round, its sides joined.
        let joined = match curves {
            [_, _, ..] if self.drawing.cloud.is_some() => curves::join(curves).map(|c| vec![c]),
            _ => None,
        };
        let params: Vec<Value> = joined
            .as_deref()
            .unwrap_or(curves)
            .iter()
            .map(|c| {
                let mut p = serde_json::to_value(Fields::of(c)).unwrap_or_default();
                p["view"] = json!(view);
                self.line_extras(&mut p);
                p
            })
            .collect();
        self.run_all("Detail Line", "detail_line", params).into_iter().filter_map(|r| r["id"].as_u64()).collect()
    }

    /// Walls along `curves` on `level` (lines, arcs and circles; the rest make none), as one step: a run of lines or of
    /// arcs turning alike end to end is one wall command, as the Wall tool's chain is.
    pub(crate) fn make_walls(&mut self, level: Id, curves: &[Curve]) -> bool {
        let segments = draw::wall_segments(curves);
        let mut runs: Vec<(Vec<P2>, f64)> = Vec::new();
        for (a, b, arc) in segments {
            match runs.last_mut() {
                Some((pts, turn)) if *turn == arc && pts.last().is_some_and(|q| q.dist(a) <= TOL) => pts.push(b),
                _ => runs.push((vec![a, b], arc)),
            }
        }
        let mut params = Vec::new();
        for (mut pts, arc) in runs {
            let closed = pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(f, l)| f.dist(*l) <= TOL);
            if closed {
                pts.pop();
            }
            match self.wall_params(level, &pts, closed, arc) {
                Some(p) => params.push(p),
                None => return false,
            }
        }
        !self.run_all("Wall", "wall", params).is_empty()
    }

    /// Sketch lines along `curves`.
    pub(crate) fn add_sketch_lines(&mut self, curves: Vec<Curve>) {
        if let Some(sk) = &mut self.sketch {
            for c in curves {
                sk.add(SketchLine::from(c));
            }
        }
    }

    /// Several of one command as one undo step, named `label`: what each gave, or none and the error said.
    pub(crate) fn run_all(&mut self, label: &'static str, command: &str, params: Vec<Value>) -> Vec<Value> {
        match params.as_slice() {
            [] => return Vec::new(),
            [one] => return self.run(command, one.clone()).into_iter().collect(),
            _ => {}
        }
        let done =
            self.session.transaction(label, |s| params.iter().map(|p| s.execute(command, p)).collect::<Result<Vec<Value>, String>>());
        match done {
            Ok(v) => {
                self.status = Status::default();
                v
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                Vec::new()
            }
        }
    }

    /// Fillet Arc's pick of `line` at `at`: the first line, the second (made at once with Radius), or with both picked
    /// the click that sizes the arc.
    pub(crate) fn fillet_pick(&mut self, line: Option<Picked>, at: P2) {
        let radius = self.draw_options().radius;
        match (self.drawing.fillet.as_slice(), line) {
            ([], Some(l)) => self.drawing.fillet.push((l, at)),
            (&[(a, _)], Some(l)) if l != a => {
                self.drawing.fillet.push((l, at));
                if let Some(r) = radius {
                    self.fillet_now(r);
                }
            }
            (&[_, _], _) => {
                if let Some(r) = self.fillet_radius_at(at) {
                    self.fillet_now(r);
                }
            }
            _ => {}
        }
    }

    /// The two lines picked for a fillet, as curves.
    fn fillet_curves(&self) -> Option<(Curve, Curve)> {
        let curve = |p: Picked| match p {
            Picked::Element(id) => draw::element_curve(&self.session.model, id),
            Picked::Sketch(i) => self.sketch.as_ref().and_then(|sk| sk.lines.get(i)).map(|l| l.curve.clone()),
        };
        match self.drawing.fillet.as_slice() {
            &[(a, _), (b, _)] => Some((curve(a)?, curve(b)?)),
            _ => None,
        }
    }

    /// The radius of the arc rounding the two lines picked that passes nearest `at`.
    pub(crate) fn fillet_radius_at(&self, at: P2) -> Option<f64> {
        let (a, b) = self.fillet_curves()?;
        let (pa, pb) = (a.vertices(), b.vertices());
        let (&[a0, a1], &[b0, b1]) = (pa.as_slice(), pb.as_slice()) else { return None };
        let corner = bimcraft_core::geom::line_intersection(a0, a1, b0, b1)?;
        let far = |p: P2, q: P2| if p.dist(corner) > q.dist(corner) { p } else { q };
        curves::fillet_radius(far(a0, a1), corner, far(b0, b1), at)
    }

    /// The fillet of the two lines picked, with an arc of radius `r`: the detail lines cut back and the arc added, or the
    /// sketch lines.
    fn fillet_now(&mut self, r: f64) {
        let picks = std::mem::take(&mut self.drawing.fillet);
        match *picks.as_slice() {
            [(Picked::Element(a), _), (Picked::Element(b), _)] => {
                self.run("fillet", json!({"ids": [a, b], "radius": r}));
            }
            [(Picked::Sketch(a), _), (Picked::Sketch(b), _)] => {
                let Some(sk) = &mut self.sketch else { return };
                let (Some(la), Some(lb)) = (sk.lines.get(a).cloned(), sk.lines.get(b).cloned()) else { return };
                match draw::fillet_lines(&la.curve, &lb.curve, r) {
                    Some((ca, arc, cb)) => {
                        if let Some(l) = sk.lines.get_mut(a) {
                            l.curve = ca;
                        }
                        if let Some(l) = sk.lines.get_mut(b) {
                            l.curve = cb;
                        }
                        sk.add(SketchLine::from(arc));
                    }
                    None => {
                        self.status = Status {
                            text: t("The arc doesn't fit: pick two straight lines that meet at an angle, long enough for it.").into(),
                            error: true,
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Pick Lines' click on element `id` near `at`: its line, beside it by the Options Bar's offset on the side clicked.
    pub(crate) fn picked_curve(&self, id: Id, at: P2) -> Option<Curve> {
        let c = draw::element_curve(&self.session.model, id)?;
        draw::pick(&c, at, self.draw_options().offset)
    }

    /// Join into Polyline (PJ): the detail lines selected, or while sketching the lines picked, joined end to end into one.
    pub(crate) fn join_polyline(&mut self) {
        if let Some(sk) = &mut self.sketch {
            let picked: Vec<usize> = sk.picked.iter().copied().collect();
            let curves: Vec<Curve> = picked.iter().filter_map(|&i| sk.lines.get(i)).map(|l| l.curve.clone()).collect();
            if let Some(joined) = curves::join(&curves) {
                let mut i = 0;
                sk.lines.retain(|_| {
                    i += 1;
                    !picked.contains(&(i - 1))
                });
                sk.lines.push(SketchLine::from(joined));
                sk.picked.clear();
                sk.picked.insert(sk.lines.len() - 1);
                sk.wrong.clear();
            }
            return;
        }
        if self.selection.len() >= 2 {
            let ids = self.selection.clone();
            if let Some(r) = self.run("join_polyline", json!({"ids": ids})) {
                self.selection = r["id"].as_u64().into_iter().collect();
            }
        }
    }

    /// Explode Polyline (PX): the polyline selected, or while sketching those picked, taken apart into lines and arcs.
    pub(crate) fn explode_polyline(&mut self) {
        if let Some(sk) = &mut self.sketch {
            let picked: Vec<usize> = sk.picked.iter().copied().collect();
            let mut lines = Vec::new();
            for (i, l) in sk.lines.iter().enumerate() {
                if picked.contains(&i) {
                    lines.extend(curves::explode(&l.curve).into_iter().map(SketchLine::from));
                } else {
                    lines.push(l.clone());
                }
            }
            sk.lines = lines;
            sk.picked.clear();
            sk.wrong.clear();
            return;
        }
        if let &[id] = self.selection.as_slice()
            && let Some(r) = self.run("explode", json!({"id": id}))
        {
            self.selection = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        }
    }

    /// Modify's Split, Trim/Extend to Corner, Trim/Extend Single and Multiple Elements and Offset on the sketch's lines: a
    /// click at `p` (snapped), picking the line nearest `raw` within `reach`.
    pub(crate) fn sketch_modify(&mut self, p: P2, raw: P2, reach: f64) {
        let offset = self.session.model.units.parse(&self.offset).filter(|d| *d > 0.0);
        let copy = self.offset_copy;
        let tool = self.tool.clone();
        let Some(sk) = &mut self.sketch else { return };
        let Some(i) = sk.line_at(raw, reach) else { return };
        let Some(l) = sk.lines.get(i).cloned() else { return };
        let mut status = None;
        match tool {
            Tool::Split => match l.curve.split_at(p) {
                Some((a, b)) => {
                    if let Some(line) = sk.lines.get_mut(i) {
                        line.curve = a;
                    }
                    if let Some(b) = b {
                        sk.lines.push(SketchLine::from(b));
                    }
                    sk.changed();
                }
                None => status = Some("Split between the line's ends."),
            },
            Tool::Trim(_) => match self.drawing.trim.take() {
                Some((first, keep)) if first != i => match sk.lines.get(first).and_then(|f| draw::corner(&f.curve, keep, &l.curve, raw)) {
                    Some((a, b)) => {
                        if let Some(line) = sk.lines.get_mut(first) {
                            line.curve = a;
                        }
                        if let Some(line) = sk.lines.get_mut(i) {
                            line.curve = b;
                        }
                        sk.changed();
                    }
                    None => status = Some("The lines don't meet: they're parallel, or an arc's circle misses the other."),
                },
                _ => self.drawing.trim = Some((i, raw)),
            },
            Tool::TrimTo { many, .. } => match self.drawing.trim {
                None => self.drawing.trim = Some((i, raw)),
                Some((b, _)) if b != i => {
                    let edge = sk.lines.get(b).map(|e| e.curve.clone());
                    match edge.and_then(|e| draw::meet(&l.curve, &e, raw)).and_then(|x| draw::trim_to(&l.curve, raw, x)) {
                        Some(c) => {
                            if let Some(line) = sk.lines.get_mut(i) {
                                line.curve = c;
                            }
                            sk.changed();
                        }
                        None => status = Some("The lines don't meet: they're parallel, or an arc's circle misses the other."),
                    }
                    if !many {
                        self.drawing.trim = None;
                    }
                }
                Some(_) => {}
            },
            Tool::Offset => match offset.and_then(|d| draw::pick(&l.curve, raw, d)) {
                Some(c) => {
                    if copy {
                        sk.lines.push(SketchLine::from(c));
                    } else if let Some(line) = sk.lines.get_mut(i) {
                        line.curve = c;
                    }
                    sk.changed();
                }
                None => status = Some("It can't be offset: give a distance, and pick a line, an arc, a polyline or a circle."),
            },
            _ => {}
        }
        if let Some(text) = status {
            self.status = Status { text: t(text).into(), error: true };
        }
    }

    /// Trim/Extend to Corner on the two detail lines or walls selected, or the two sketch lines picked: true when it
    /// joined them (or tried, and said why not).
    pub(crate) fn corner_selected(&mut self) -> bool {
        if let Some(sk) = &mut self.sketch {
            let &[a, b] = sk.picked.iter().copied().collect::<Vec<_>>().as_slice() else { return false };
            let (Some(la), Some(lb)) = (sk.lines.get(a).cloned(), sk.lines.get(b).cloned()) else { return false };
            match draw::corner(&la.curve, la.curve.middle(), &lb.curve, lb.curve.middle()) {
                Some((ca, cb)) => {
                    if let Some(l) = sk.lines.get_mut(a) {
                        l.curve = ca;
                    }
                    if let Some(l) = sk.lines.get_mut(b) {
                        l.curve = cb;
                    }
                    sk.wrong.clear();
                    self.status = Status::default();
                }
                None => {
                    self.status = Status {
                        text: t("The lines don't meet: they're parallel, or an arc's circle misses the other.").into(),
                        error: true,
                    }
                }
            }
            return true;
        }
        let m = &self.session.model;
        let lines = |id: &Id| matches!(m.elements.get(*id), Some(Element::DetailLine(_)));
        let both = self.selection.len() == 2 && (self.selection.iter().all(lines) || self.selection.iter().all(|id| m.wall(*id).is_some()));
        if !both {
            return false;
        }
        let ids = self.selection.clone();
        self.run("corner", json!({"ids": ids}));
        true
    }

    /// Modify | Lines' Add Vertex, Delete Vertex and Convert Spline (for one spline selected, or picked while sketching).
    pub(crate) fn vertex_buttons(&mut self, ui: &mut Ui) {
        for (icon, label, tip, delete) in [
            (
                Icon::AddVertex,
                "Add Vertex",
                "Add Vertex: click a polyline or a spline where a vertex goes; a spline keeps its shape",
                false,
            ),
            (Icon::DeleteVertex, "Delete Vertex", "Delete Vertex: click a polyline's corner or a spline's point to take it out", true),
        ] {
            let on = self.tool == Tool::Vertex { delete };
            if crate::big_button(ui, icon, label, on, tip).clicked() {
                self.set_tool(Tool::Vertex { delete });
            }
        }
        let spline = |c: Option<Curve>| matches!(c, Some(Curve::Spline { .. } | Curve::Fit { .. }));
        let one = match (&self.sketch, self.selection.as_slice()) {
            (Some(sk), _) => {
                sk.picked.len() == 1 && spline(sk.picked.iter().next().and_then(|i| sk.lines.get(*i)).map(|l| l.curve.clone()))
            }
            (None, &[id]) => spline(element_curve(&self.session.model, id)),
            _ => false,
        };
        let tip = "Convert Spline: a spline through points becomes the same curve by its control points, or one by its control points a spline through points";
        if ui.add_enabled_ui(one, |ui| crate::big_button(ui, Icon::ConvertSpline, "Convert Spline", false, tip)).inner.clicked() {
            self.convert_spline();
        }
    }

    /// Convert Spline on the one spline selected, or picked while sketching.
    pub(crate) fn convert_spline(&mut self) {
        if let Some(sk) = &mut self.sketch {
            if let Some(&i) = sk.picked.iter().next()
                && let Some(l) = sk.lines.get_mut(i)
                && let Some(c) = l.curve.converted()
            {
                l.curve = c;
                sk.wrong.clear();
            }
            return;
        }
        if let &[id] = self.selection.as_slice() {
            self.run("convert_spline", json!({"id": id}));
        }
    }

    /// Add or Delete Vertex's click at `at` on detail line `id`, or while sketching on the sketch line nearest `at`.
    pub(crate) fn vertex_click(&mut self, id: Option<Id>, at: P2, reach: f64, delete: bool) {
        if let Some(sk) = &mut self.sketch {
            if let Some(i) = sk.line_at(at, reach)
                && let Some(l) = sk.lines.get_mut(i)
            {
                let c = if delete { l.curve.without_vertex(at) } else { l.curve.with_vertex(at) };
                match c {
                    Some(c) => {
                        l.curve = c;
                        sk.wrong.clear();
                    }
                    None => {
                        self.status = Status {
                            text: t("Only a polyline or a spline takes a vertex, and a line keeps its two ends.").into(),
                            error: true,
                        }
                    }
                }
            }
            return;
        }
        if let Some(id) = id {
            self.run("vertex", json!({"id": id, "at": [at.0, at.1], "delete": delete}));
        }
    }

    /// The detail line selected alone, with Modify: the one whose handles are grips.
    pub(crate) fn line_gripped(&self) -> Option<Id> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) if matches!(self.session.model.elements.get(id), Some(Element::DetailLine(_))) => Some(id),
            _ => None,
        }
    }

    /// A drag from a handle of the selected detail line (a corner, a control point, a point it passes through, a centre)
    /// moves it there, snapped; on release the line is redrawn so, as one step.
    pub(crate) fn line_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &bimcraft_core::render::View, level: Id) {
        let Some(id) = self.line_gripped() else {
            self.drawing.grip = None;
            return;
        };
        let Some(c) = element_curve(&self.session.model, id) else { return };
        let at = |p: P2| {
            let (x, y) = view.px(p);
            rect.min + vec2(x, y)
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |p: P2| at(p).distance(o);
            self.drawing.grip =
                c.handles().into_iter().filter(|p| near(*p) <= 8.0).min_by(|a, b| near(*a).total_cmp(&near(*b))).map(|p| (id, p));
        }
        let Some((held, from)) = self.drawing.grip else { return };
        if held != id {
            self.drawing.grip = None;
            return;
        }
        if response.drag_stopped() {
            self.drawing.grip = None;
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let m = std::sync::Arc::clone(&self.session.model);
                let to = self.snap(&m, level, view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at;
                self.snap_once = None;
                if to.dist(from) > 1e-6 {
                    self.redraw_line(id, &c.moved(from, to));
                }
            }
        }
    }

    /// Detail line `id` redrawn along `c`, whatever shape it was: its curve's fields set, the rest taken away.
    pub(crate) fn redraw_line(&mut self, id: Id, c: &Curve) {
        let mut p = json!({"id": id});
        for k in curves::FIELD_NAMES {
            p[*k] = if *k == "closed" { json!(false) } else { Value::Null };
        }
        if let Ok(Value::Object(f)) = serde_json::to_value(Fields::of(c)) {
            for (k, v) in f {
                p[k] = v;
            }
        }
        self.run("modify", p);
    }

    /// The selected detail line's grips, and while one is dragged to the cursor, the line as it would be.
    pub(crate) fn line_grip_overlay(&self, p: &Painter, rect: Rect, view: &bimcraft_core::render::View, cursor: Option<P2>) {
        let Some(id) = self.line_gripped() else { return };
        let Some(c) = element_curve(&self.session.model, id) else { return };
        let at = |q: P2| -> Pos2 {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let (shown, live) = match (self.drawing.grip, cursor) {
            (Some((g, from)), Some(to)) if g == id => (c.moved(from, to), Some(to)),
            _ => (c.clone(), None),
        };
        if live.is_some() {
            p.add(Paint::line(shown.points().into_iter().map(at).collect(), Stroke::new(1.5, pal().accent)));
        }
        // A spline's control points, joined as its frame.
        if let Curve::Spline { control, .. } = &shown {
            p.add(Paint::dashed_line(&control.iter().map(|q| at(*q)).collect::<Vec<_>>(), Stroke::new(1.0, pal().accent), 6.0, 4.0));
        }
        for h in shown.handles() {
            let r = Rect::from_center_size(at(h), vec2(9.0, 9.0));
            let held = live.is_some_and(|l| l.dist(h) < 1e-9);
            p.rect_filled(r, 1.0, if held { pal().accent } else { pal().surface });
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
    }

    /// The detail lines selected, when there are any and nothing else: what Join and Explode work on.
    pub(crate) fn lines_selected(&self) -> bool {
        !self.selection.is_empty()
            && self.selection.iter().all(|id| matches!(self.session.model.elements.get(*id), Some(Element::DetailLine(_))))
    }
}
