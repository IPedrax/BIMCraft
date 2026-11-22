//! FE-7: dragging the selection in a family's plan or elevation moves it. A press on an element (its edge, a
//! form's face, a plane's trace, a line, a dimension, a connector, a selected symbol's strokes) and a drag past the few points a
//! click allows move the whole selection by the drag, in the view's plane; one not selected is selected first (alone, or added
//! with Ctrl). A lone plane of the family's own keeps F4's drag, through its constraints (`views.rs`).
//!
//! What moves and how: planes, reference lines and forms through the constraints (`family::constraints::Moving`, `move_by`):
//! a form's sketch in its host plane where the view sees it flat; where it sees it edge-on, the part of the drag along the
//! plane's normal moves an extrusion's or a blend's Start and End together; what is locked to what moves follows where the
//! constraints let it, and a lock, a label, EQ or an Align lock to what stays refuses the move, naming it. A nested instance,
//! a symbol and a connector are placed again by the drag; a dimension's line goes with it, never what it measures. Only the
//! numbers that move are written, as numbers: one that is a formula refuses the move. A datum, a plane that defines the
//! origin, what a pivot turns, and what would leave the plane it is drawn on stay, refusing the move. After it every type
//! must still solve and build (as Finish checks), or it is refused. A refusal leaves everything as it was and says why.
//!
//! The drag is the shape handles' (FE-5): made again from the draft as it was at each step (live, one step of the history
//! once the button is up), the pointer read through the view as it was when the drag began; a drag back where it began
//! leaves nothing to save; Escape, Undo, Redo and another view or tab shown give it up, everything put back.
use super::select_box::Picked;
use super::sketch::reader;
use super::*;
use bimcraft_core::family::constraints::{Edit, MoveRefused, Moving, Stays};
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{FamilyDimension, NestedInstance, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;

/// What a move may change of the draft, as it was when the drag began: what Escape and a refusal put back.
#[derive(Clone)]
struct Kept {
    forms: Vec<FormText>,
    planes: Vec<RefPlane>,
    lines: Vec<RefLine>,
    dimensions: Vec<FamilyDimension>,
    symbols: Vec<SymbolText>,
    connectors: Vec<connectors::ConnectorText>,
    nested_instances: Vec<NestedInstance>,
}

impl Kept {
    fn of(ed: &FamilyEditor) -> Kept {
        Kept {
            forms: ed.forms.clone(),
            planes: ed.planes.clone(),
            lines: ed.lines.clone(),
            dimensions: ed.dimensions.clone(),
            symbols: ed.symbols.clone(),
            connectors: ed.connectors.clone(),
            nested_instances: ed.nested_instances.clone(),
        }
    }

    fn put_back(&self, ed: &mut FamilyEditor) {
        ed.forms.clone_from(&self.forms);
        ed.planes.clone_from(&self.planes);
        ed.lines.clone_from(&self.lines);
        ed.dimensions.clone_from(&self.dimensions);
        ed.symbols.clone_from(&self.symbols);
        ed.connectors.clone_from(&self.connectors);
        ed.nested_instances.clone_from(&self.nested_instances);
    }
}

/// The selection being dragged: the view it is dragged in and the last pass that view was drawn in (shown no longer, the
/// drag is given up), where the button went down (the view's millimetres) and the view as it was then (the pointer is read
/// through it), what moves, the draft as it was, the types that didn't build before it, and how far it has gone.
#[derive(Clone)]
pub(crate) struct MoveDrag {
    pub(super) which: Which,
    pub(super) seen: u64,
    pub(super) from: P2,
    pub(super) view: bimcraft_core::render::View,
    items: Vec<Picked>,
    before: Kept,
    broken: Vec<String>,
    /// Whether the family passed its checks (`Family::check`) as it was: then a move that fails them is refused.
    sound: bool,
    last: [f64; 3],
}

/// The drag's way in the view, from where the button went down (the view's millimetres): what the selection moves by.
/// With Shift held it is square to the view's axes (FE-2's Ortho, `bimcraft_core::snap::orthogonal_step`), from where the
/// drag began; nothing else reads the pointer for a move.
pub(super) fn delta(from: P2, to: P2, shift: bool) -> P2 {
    let to = if shift { bimcraft_core::snap::orthogonal_step(from, to).0 } else { to };
    P2(to.0 - from.0, to.1 - from.1)
}

/// What a refusal before the constraints says.
fn stays_said(s: &Stays) -> String {
    match s {
        Stays::Datum(n) => tf("{} is a datum: it stays where it is.", &[n]),
        Stays::Origin(n) => tf("{} defines the origin: it stays where it is.", &[n]),
        Stays::Pivot(n) => tf("{} turns about its pivot: turn it by its rotate grip.", &[n]),
        Stays::Formula(n) => tf("A number of {} the move would change is worked out by a formula: change it in Family Types.", &[n]),
        Stays::Hosted(n) => tf("{} would leave the plane it is drawn on: move that plane with it.", &[n]),
        Stays::Mesh(n) => tf("{} is a file's triangles: it has no sketch to move.", &[n]),
    }
}

/// `e` moved by `by`, a number; a formula refuses it (none for no move).
fn shifted(e: &mut Expr, by: f64) -> bool {
    if by.abs() <= 1e-9 {
        return true;
    }
    match e {
        Expr::Number(v) => {
            *v = ((*v + by) * 1e6).round() / 1e6;
            true
        }
        _ => false,
    }
}

impl FamilyEditor {
    /// A move of the selection begun at `from` (the view's millimetres, `view` as it was then) in view `which`, drawn in pass
    /// `seen`.
    /// `shown`: what of the selection the view shows, which alone moves.
    pub(super) fn move_drag(&self, (from, view, which, seen): (P2, bimcraft_core::render::View, Which, u64), shown: &[Picked]) -> MoveDrag {
        // The types that don't solve or build as it is: a move isn't refused for them.
        let broken = self
            .family()
            .map(|f| {
                f.types
                    .keys()
                    .filter(|name| f.values(name, &BTreeMap::new()).map_or(true, |v| f.solve(&v).is_err() || f.pieces(&v).is_err()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let items = self.selection.items().iter().copied().filter(|p| self.holds(*p) && shown.contains(p)).collect();
        MoveDrag { which, seen, from, view, items, before: Kept::of(self), broken, sound: self.sound(), last: [0.0; 3] }
    }

    /// Whether the selection is being dragged.
    pub(crate) fn moving(&self) -> bool {
        self.views.moving.is_some()
    }

    /// The selection being dragged, the pointer `d` (the family's mm) from where the button went down: the draft made again
    /// from what it was with the selection moved that far, where everything it holds takes it; else left as it was, saying
    /// why.
    pub(crate) fn drag_move_to(&mut self, d: [f64; 3]) -> Result<(), String> {
        let Some(drag) = self.views.moving.as_mut() else { return Ok(()) };
        if !d.iter().all(|v| v.is_finite()) || d.iter().zip(drag.last).all(|(a, b)| (a - b).abs() < 1e-9) {
            return Ok(());
        }
        drag.last = d;
        let drag = drag.clone();
        drag.before.put_back(self);
        // Back where it began: the draft exactly as it was, so the drag leaves nothing to save.
        if d.iter().all(|v| v.abs() < 1e-9) {
            return Ok(());
        }
        let done = self.move_picked(&drag.items, d, &drag.broken).and_then(|()| {
            // What it leaves must pass the family's checks, a reference line still on its plane among them, if it did before.
            if drag.sound
                && let Err(e) = self.family().and_then(|f| f.check(&|_| true))
            {
                return Err(e);
            }
            Ok(())
        });
        if done.is_err() {
            drag.before.put_back(self);
        }
        done
    }

    /// Whether the draft passes the family's checks (`Family::check`, its materials aside): what a drag must leave it doing.
    pub(super) fn sound(&self) -> bool {
        self.family().is_ok_and(|f| f.check(&|_| true).is_ok())
    }

    /// The move given up, the draft put back as it was before it: whether there was one.
    pub(super) fn cancel_move(&mut self) -> bool {
        match self.views.moving.take() {
            Some(d) => {
                d.before.put_back(self);
                true
            }
            None => false,
        }
    }

    /// `items` moved by `d` (the family's axes): planes, lines and forms through the constraints, the rest placed again;
    /// then each type solved and built. Refused, why; the caller puts the draft back.
    fn move_picked(&mut self, items: &[Picked], d: [f64; 3], broken: &[String]) -> Result<(), String> {
        let f = self.family()?;
        let values = self.values();
        let mut moving = Moving::default();
        for p in items {
            match *p {
                Picked::Plane(g) => moving.planes.push(g),
                Picked::Line(g) => moving.lines.push(g),
                Picked::Form(i) => moving.forms.push(i),
                _ => {}
            }
        }
        if !moving.is_empty() {
            let edits = f.move_by(&values, &moving, d).map_err(|e| match e {
                MoveRefused::Stays(s) => stays_said(&s),
                MoveRefused::Unsat(u) => u.message,
            })?;
            let (points, rest): (Vec<Edit>, Vec<Edit>) = edits.into_iter().partition(|e| matches!(e, Edit::Point(..)));
            self.apply_edits(rest);
            for e in points {
                if let Edit::Point(i, k, spot, p) = e {
                    self.shift_sketch_point(i, k, spot, p, &values);
                }
            }
            // A plane's trace slides along itself by the rest of the drag: its constraints saw only its normal's part.
            for g in &moving.planes {
                if let Some(p) = self.planes.iter_mut().find(|p| p.guid == *g) {
                    let z = p.frame().z;
                    let along = d[0] * z[0] + d[1] * z[1] + d[2] * z[2];
                    for k in 0..3 {
                        let by = d[k] - z[k] * along;
                        p.start[k] += by;
                        p.end[k] += by;
                    }
                }
            }
        }
        let refused =
            |what: String| tf("A number of {} the move would change is worked out by a formula: change it in Family Types.", &[&what]);
        for p in items {
            match *p {
                Picked::Nested(i) => {
                    let Some(n) = self.nested_instances.get_mut(i) else { continue };
                    let fr = f.frame(&n.host)?;
                    let host = moving.host_shift(&f, &n.host, d);
                    let own = [d[0] - host[0], d[1] - host[1], d[2] - host[2]];
                    let dot = |a: [f64; 3]| own[0] * a[0] + own[1] * a[1] + own[2] * a[2];
                    let [x, y] = &mut n.at;
                    if !(shifted(x, dot(fr.x)) && shifted(y, dot(fr.y)) && shifted(&mut n.offset, dot(fr.z))) {
                        return Err(refused(tf("nested instance {}", &[&(i + 1).to_string()])));
                    }
                }
                Picked::Symbol(i) => {
                    let Some(s) = self.symbols.get(i) else { continue };
                    let mut y = s.symbol()?;
                    let by = |p: &mut [Expr; 2]| shifted(&mut p[0], d[0]) && shifted(&mut p[1], d[1]);
                    let mut ok = y.line.iter_mut().all(by);
                    if let Some(c) = y.arc.as_mut() {
                        ok &= by(&mut c.centre);
                    }
                    if let Some(l) = y.text.as_mut() {
                        ok &= by(&mut l.at);
                    }
                    if let Some(c) = y.dot.as_mut() {
                        ok &= by(&mut c.centre);
                    }
                    if !ok {
                        return Err(refused(tf("symbol {}", &[&(i + 1).to_string()])));
                    }
                    self.symbols[i] = SymbolText::of(&y);
                }
                Picked::Connector(i) => {
                    let Some(c) = self.connectors.get(i) else { continue };
                    let mut c = c.connector();
                    if let Some(at) = c.at.as_mut() {
                        if !at.iter_mut().zip(d).all(|(e, by)| shifted(e, by)) {
                            return Err(refused(tf("connector {}", &[&(i + 1).to_string()])));
                        }
                        self.connectors[i] = connectors::ConnectorText::of(&c);
                    }
                }
                Picked::Dimension(g) => {
                    if let Some(m) = self.dimensions.iter_mut().find(|m| m.guid == g) {
                        for (k, by) in d.iter().enumerate() {
                            m.at[k] += by;
                        }
                    }
                }
                Picked::Form(_) | Picked::Plane(_) | Picked::Line(_) => {}
            }
        }
        // Each type solved and built with it, as Finish builds a form.
        let f = self.family()?;
        for name in f.types.keys().filter(|n| !broken.contains(n)) {
            let values = f.values(name, &BTreeMap::new())?;
            if let Err(e) = f.solve(&values) {
                return Err(format!("{name}: {e}"));
            }
            if let Err(e) = f.pieces(&values) {
                return Err(format!("{name}: {e}"));
            }
        }
        Ok(())
    }

    /// Point `spot` of line `k` (of its sketch and top) of form `i` put at `p` (its host's axes): only the coordinates that
    /// change written, as numbers; the rest keep what they are, a formula too.
    fn shift_sketch_point(&mut self, i: usize, k: usize, spot: bimcraft_core::family::Spot, p: [f64; 2], values: &BTreeMap<String, Val>) {
        use bimcraft_core::family::Spot;
        let Some(ft) = self.forms.get(i) else { return };
        let Ok(mut f) = ft.form() else { return };
        let Some(line) = f.plane_line_mut(k) else { return };
        let at = match (&mut line.curve, spot) {
            (FormCurve::Line { start, .. }, Spot::Start) => start,
            (FormCurve::Line { end, .. }, Spot::End) => end,
            (FormCurve::Circle { centre, .. }, Spot::Centre) => centre,
            _ => return,
        };
        let mut num = reader(values);
        for (e, v) in at.iter_mut().zip(p) {
            if !num(e).is_ok_and(|was| (was - v).abs() <= 1e-9) {
                *e = Expr::Number(v);
            }
        }
        let tag = ft.tag;
        self.forms[i] = FormText::again(&f, tag);
    }
}

/// The reference lines drawn on plane `plane`, and on them, moved `by` (the family's axes) with it.
pub(super) fn carry_lines(lines: &mut [RefLine], plane: Guid, by: [f64; 3]) {
    let mut moved: Vec<Guid> = Vec::new();
    for l in lines.iter_mut() {
        let on = match l.host {
            PlaneRef::Plane(h) => h == plane,
            PlaneRef::Line(h, _) => moved.contains(&h),
        };
        if on {
            for (k, by) in by.iter().enumerate() {
                l.start[k] += by;
                l.end[k] += by;
            }
            moved.push(l.guid);
        }
    }
}
