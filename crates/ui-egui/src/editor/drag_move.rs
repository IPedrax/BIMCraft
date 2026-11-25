//! Dragging the selection in a family's plan or elevation moves it. A press on an element (its edge, a
//! form's face, a plane's trace, a line, a dimension, a connector, a selected symbol's strokes) and a drag past the few points a
//! click allows move the whole selection by the drag, in the view's plane; one not selected is selected first (alone, or added
//! with Ctrl). A lone plane of the family's own keeps its own drag, square to itself, through its constraints (`views.rs`).
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
//! The drag is the shape handles': made again from the draft as it was at each step (live, one step of the history
//! once the button is up), the pointer read through the view as it was when the drag began; a drag back where it began
//! leaves nothing to save; Escape, Undo, Redo and another view or tab shown give it up, everything put back.
//!
//! With Ctrl held, the drag copies the selection instead (`Family::copies`), the copies moved by the same rules and
//! selected in place of what they copy; Ctrl as the button comes up decides between a copy and a move.
use super::select_box::{How, Picked};
use super::sketch::reader;
use super::*;
use bimcraft_core::family::constraints::{CopyRefused, Copying, Edit, MoveRefused, Moving, Named, Stays};
use bimcraft_core::family::sketch::FormCurve;
use bimcraft_core::family::views::Which;
use bimcraft_core::family::{DimRef, Family, FamilyDimension, NestedInstance, Unsat, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;

/// What a move may change of the draft, as it was when the drag began: what Escape and a refusal put back.
#[derive(Clone)]
pub(super) struct Kept {
    forms: Vec<FormText>,
    planes: Vec<RefPlane>,
    lines: Vec<RefLine>,
    dimensions: Vec<FamilyDimension>,
    symbols: Vec<SymbolText>,
    connectors: Vec<connectors::ConnectorText>,
    nested_instances: Vec<NestedInstance>,
    /// The parameters and the types' values, which a lone plane's drag through a label changes.
    params: Vec<ParamText>,
    types: Vec<(String, BTreeMap<String, String>)>,
}

impl Kept {
    pub(super) fn of(ed: &FamilyEditor) -> Kept {
        Kept {
            forms: ed.forms.clone(),
            planes: ed.planes.clone(),
            lines: ed.lines.clone(),
            dimensions: ed.dimensions.clone(),
            symbols: ed.symbols.clone(),
            connectors: ed.connectors.clone(),
            nested_instances: ed.nested_instances.clone(),
            params: ed.params.clone(),
            types: ed.types.clone(),
        }
    }

    pub(super) fn put_back(&self, ed: &mut FamilyEditor) {
        ed.forms.clone_from(&self.forms);
        ed.planes.clone_from(&self.planes);
        ed.lines.clone_from(&self.lines);
        ed.dimensions.clone_from(&self.dimensions);
        ed.symbols.clone_from(&self.symbols);
        ed.connectors.clone_from(&self.connectors);
        ed.nested_instances.clone_from(&self.nested_instances);
        ed.params.clone_from(&self.params);
        ed.types.clone_from(&self.types);
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
    /// What was selected as it began: what a copy's preview, a refusal and Escape put back.
    selected: Vec<Picked>,
    /// Whether it copies (Ctrl held) rather than moves: Ctrl as the button comes up decides.
    copy: bool,
    /// A lone plane of the family's own, pressed with Ctrl: moved, it goes as its own drag takes it, square to itself
    /// through its constraints.
    lone: Option<Guid>,
}

impl MoveDrag {
    /// The draft and the selection put back as they were as it began.
    fn put_back(&self, ed: &mut FamilyEditor) {
        self.before.put_back(ed);
        ed.selection.apply(&self.selected, How::Replace);
    }
}

/// The drag's way in the view, from where the button went down (the view's millimetres): what the selection moves by.
/// With Shift held it is square to the view's axes (`bimcraft_core::snap::orthogonal_step`, as Shift keeps a drawn line square), from where the
/// drag began; nothing else reads the pointer for a move.
pub(super) fn delta(from: P2, to: P2, shift: bool) -> P2 {
    let to = if shift { bimcraft_core::snap::orthogonal_step(from, to).0 } else { to };
    P2(to.0 - from.0, to.1 - from.1)
}

/// What a copy's refusal says, naming what isn't copied in `f`.
pub(super) fn copy_said(r: &CopyRefused, f: &Family) -> String {
    match r {
        CopyRefused::Datum(n) => tf("{} is a datum: it isn't copied.", &[&f.said(*n)]),
        CopyRefused::Origin(n) => tf("{} defines the origin: it isn't copied.", &[&f.said(*n)]),
        CopyRefused::Pivot(n) => tf("{} turns about a pivot, or is drawn on what does: it isn't copied.", &[&f.said(*n)]),
    }
}

/// Where what a copy put in the draft came from: a refusal names the original, in the lists as they were before the
/// copies, never a copy, which isn't there once it is refused. As it is (`default`), each thing is itself.
#[derive(Default)]
pub(super) struct Origin {
    /// Each form's place before the copies (a copy's, its original's); none, as it is.
    forms: Option<Vec<usize>>,
    /// The copies added at the end of a list: how long it was, and the original of each.
    symbols: (usize, Vec<usize>),
    connectors: (usize, Vec<usize>),
    nested: (usize, Vec<usize>),
    dimensions: (usize, Vec<usize>),
    /// A copied plane's or line's GUID, and its original's.
    guids: BTreeMap<Guid, Guid>,
}

impl Origin {
    fn back((before, of): &(usize, Vec<usize>), i: usize) -> usize {
        i.checked_sub(*before).and_then(|k| of.get(k).copied()).unwrap_or(i)
    }

    fn form(&self, i: usize) -> usize {
        self.forms.as_ref().and_then(|f| f.get(i).copied()).unwrap_or(i)
    }

    pub(super) fn named(&self, n: Named) -> Named {
        match n {
            Named::Plane(g) => Named::Plane(self.guids.get(&g).copied().unwrap_or(g)),
            Named::Line(g) => Named::Line(self.guids.get(&g).copied().unwrap_or(g)),
            Named::Form(i) => Named::Form(self.form(i)),
        }
    }

    fn dimension(&self, k: usize) -> usize {
        Origin::back(&self.dimensions, k)
    }

    /// `u` naming the originals of the dimensions it names.
    pub(super) fn unsat(&self, u: &Unsat) -> String {
        u.renumbered(&|k| self.dimension(k)).message
    }
}

/// How a refusal names what it is about: the family as the drag found it (before any copy) and where each copy came from.
pub(super) struct Names<'a> {
    pub(super) family: &'a Family,
    pub(super) origin: &'a Origin,
}

impl Names<'_> {
    pub(super) fn said(&self, n: Named) -> String {
        self.family.said(self.origin.named(n))
    }
}

/// What a refusal before the constraints says, naming it as `names` does.
fn stays_said(s: &Stays, names: &Names) -> String {
    let said = |n: &Named| names.said(*n);
    match s {
        Stays::Datum(n) => tf("{} is a datum: it stays where it is.", &[&said(n)]),
        Stays::Origin(n) => tf("{} defines the origin: it stays where it is.", &[&said(n)]),
        Stays::Pivot(n) => tf("{} turns about its pivot: turn it by its rotate grip.", &[&said(n)]),
        Stays::Formula(n) => tf("A number of {} the move would change is worked out by a formula: change it in Family Types.", &[&said(n)]),
        Stays::Hosted(n) => tf("{} would leave the plane it is drawn on: move that plane with it.", &[&said(n)]),
        Stays::Mesh(n) => tf("{} is a file's triangles: it has no sketch to move.", &[&said(n)]),
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
    /// Whether the selection holds a connector with no place of its own (an electrical device's may have none): a move leaves
    /// it where it is, which the status bar says as the move begins.
    pub(super) fn unplaced_connector_selected(&self) -> bool {
        self.selection.items().iter().any(|p| match *p {
            Picked::Connector(i) => self.connectors.get(i).is_some_and(|c| c.connector().at.is_none()),
            _ => false,
        })
    }

    /// A move of the selection begun at `from` (the view's millimetres, `view` as it was then) in view `which`, drawn in pass
    /// `seen`.
    /// `shown`: what of the selection the view shows, which alone moves.
    /// `copy`: Ctrl held as it begins, a copy.
    pub(super) fn move_drag(
        &self,
        (from, view, which, seen): (P2, bimcraft_core::render::View, Which, u64),
        shown: &[Picked],
        copy: bool,
        lone: Option<Guid>,
    ) -> MoveDrag {
        // The types that don't solve or build as it is: a move isn't refused for them.
        let broken = self.broken_types();
        let items = self.selection.items().iter().copied().filter(|p| self.holds(*p) && shown.contains(p)).collect();
        let selected = self.selection.items().to_vec();
        MoveDrag {
            which,
            seen,
            from,
            view,
            items,
            before: Kept::of(self),
            broken,
            sound: self.sound(),
            last: [0.0; 3],
            selected,
            copy,
            lone,
        }
    }

    /// Whether the selection is being dragged.
    pub(crate) fn moving(&self) -> bool {
        self.views.moving.is_some()
    }

    /// Whether the selection being dragged is copied (Ctrl held), not moved.
    pub(crate) fn copying(&self) -> bool {
        self.views.moving.as_ref().is_some_and(|d| d.copy)
    }

    /// The selection being dragged, the pointer `d` (the family's mm) from where the button went down: the draft made again
    /// from what it was with the selection moved that far, or with copies of it put there and selected in its place where
    /// `copy` (Ctrl held), where everything it holds takes it; else left as it was, saying why.
    pub(crate) fn drag_move_to(&mut self, d: [f64; 3], copy: bool) -> Result<(), String> {
        let Some(drag) = self.views.moving.as_mut() else { return Ok(()) };
        if !d.iter().all(|v| v.is_finite()) || (drag.copy == copy && d.iter().zip(drag.last).all(|(a, b)| (a - b).abs() < 1e-9)) {
            return Ok(());
        }
        (drag.last, drag.copy) = (d, copy);
        let drag = drag.clone();
        drag.put_back(self);
        // Back where it began: the draft exactly as it was, so the drag leaves nothing to save (a copy there makes none).
        if d.iter().all(|v| v.abs() < 1e-9) {
            return Ok(());
        }
        let mut chosen = None;
        let as_it_was = self.family()?;
        let done = if copy {
            self.copy_picked(&drag.items).and_then(|(moved, selected, origin)| {
                chosen = Some(selected);
                // A refusal names what is copied, not its copy, which isn't there once it is refused.
                self.move_picked(&moved, d, &drag.broken, &Names { family: &as_it_was, origin: &origin })
            })
        } else if let Some(g) = drag.lone {
            // A lone plane pressed with Ctrl and let go without it: the plane's own drag, as without Ctrl.
            match self.plane_dragged(g, d) {
                Err(Ok((e, at))) => {
                    // What holds it asks, as the plane's own drag does, and the drag ends.
                    drag.put_back(self);
                    self.views.moving = None;
                    self.unsatisfied(e, super::dimensions::Retry::Drag(g, at));
                    return Ok(());
                }
                Err(Err(e)) => Err(e),
                Ok(()) => Ok(()),
            }
        } else {
            self.move_picked(&drag.items, d, &drag.broken, &Names { family: &as_it_was, origin: &Origin::default() })
        };
        let done = done.and_then(|()| {
            // What it leaves must pass the family's checks, a reference line still on its plane among them, if it did before.
            if drag.sound
                && let Err(e) = self.family().and_then(|f| f.check(&|_| true))
            {
                return Err(e);
            }
            Ok(())
        });
        match (&done, chosen) {
            (Err(_), _) => drag.put_back(self),
            // The copies selected in place of what they copy.
            (Ok(()), Some(copies)) => self.selection.apply(&copies, How::Replace),
            (Ok(()), None) => {}
        }
        done
    }

    /// The drag let go with Ctrl held or not (`copy`): made again that way if it was the other (Ctrl as the button
    /// comes up decides).
    pub(crate) fn settle_drag(&mut self, copy: bool) -> Result<(), String> {
        match self.views.moving.as_ref() {
            Some(d) if d.copy != copy => {
                let last = d.last;
                self.drag_move_to(last, copy)
            }
            _ => Ok(()),
        }
    }

    /// Copies of `items` made where they are (`Family::copies`) and added to the draft: what then moves (every copy,
    /// the dimensions that hold constraints among them too), what is selected after (the copies of `items`), and where each
    /// came from. A form's copy goes right after it, so a void's cuts what its original cuts; the rest go at the end.
    pub(super) fn copy_picked(&mut self, items: &[Picked]) -> Result<(Vec<Picked>, Vec<Picked>, Origin), String> {
        let f = self.family()?;
        let mut c = Copying::default();
        for p in items {
            match *p {
                Picked::Plane(g) => c.planes.push(g),
                Picked::Line(g) => c.lines.push(g),
                Picked::Form(i) => c.forms.push(i),
                Picked::Symbol(i) => c.symbols.push(i),
                Picked::Connector(i) => c.connectors.push(i),
                Picked::Nested(i) => c.nested.push(i),
                Picked::Dimension(g) => c.dimensions.push(g),
            }
        }
        let copies = f.copies(&c).map_err(|r| copy_said(&r, &f))?;
        let (moved, selected, origin) = self.add_copies(copies, Some(&c));
        if moved.is_empty() {
            return Err(t("Nothing selected here can be copied.").to_string());
        }
        Ok((moved, selected, origin))
    }

    /// `copies` added to the draft: what then moves (every copy, the dimensions that hold constraints among them too), what is
    /// selected after (the copies of what was selected), and where each came from. A form's copy goes right after the form
    /// `of` names in its place (as a Ctrl+drag's, so a void's cuts what its original cuts); with none, and the rest, at the end.
    pub(super) fn add_copies(
        &mut self,
        copies: bimcraft_core::family::constraints::Copies,
        of: Option<&Copying>,
    ) -> (Vec<Picked>, Vec<Picked>, Origin) {
        let none = Copying::default();
        let c = of.unwrap_or(&none);
        let c_forms = &c.forms;
        let mut origin = Origin { guids: copies.guids.iter().map(|(a, b)| (*b, *a)).collect(), ..Origin::default() };
        let mut moved = Vec::new();
        // The forms again, each copy after its original.
        let mut forms = Vec::with_capacity(self.forms.len() + copies.forms.len());
        let mut was = Vec::with_capacity(forms.capacity());
        for (i, ft) in self.forms.iter().enumerate() {
            forms.push(ft.clone());
            was.push(i);
            if let Some(k) = c_forms.iter().position(|j| *j == i)
                && let Some(form) = copies.forms.get(k)
            {
                moved.push(Picked::Form(forms.len()));
                forms.push(ft.copied(form));
                was.push(i);
            }
        }
        // Copies of no form of the draft's (a paste): at the end, each its own.
        if of.is_none() {
            for form in &copies.forms {
                moved.push(Picked::Form(forms.len()));
                was.push(forms.len());
                forms.push(FormText::of(form));
            }
        }
        self.forms = forms;
        origin.forms = Some(was);
        for p in copies.planes {
            moved.push(Picked::Plane(p.guid));
            self.planes.push(p);
        }
        for l in copies.lines {
            moved.push(Picked::Line(l.guid));
            self.lines.push(l);
        }
        origin.symbols = (self.symbols.len(), c.symbols.clone());
        for y in &copies.symbols {
            moved.push(Picked::Symbol(self.symbols.len()));
            self.symbols.push(SymbolText::of(y));
        }
        origin.connectors = (self.connectors.len(), c.connectors.clone());
        for k in &copies.connectors {
            moved.push(Picked::Connector(self.connectors.len()));
            self.connectors.push(connectors::ConnectorText::of(k));
        }
        origin.nested = (self.nested_instances.len(), c.nested.clone());
        for n in copies.nested {
            moved.push(Picked::Nested(self.nested_instances.len()));
            self.nested_instances.push(n);
        }
        let mut selected: Vec<Picked> = moved.clone();
        let of_dims = if of.is_some() { copies.dimensions_of.clone() } else { Vec::new() };
        origin.dimensions = (self.dimensions.len(), of_dims);
        for m in copies.dimensions {
            moved.push(Picked::Dimension(m.guid));
            if copies.selected.contains(&m.guid) {
                selected.push(Picked::Dimension(m.guid));
            }
            self.dimensions.push(m);
        }
        (moved, selected, origin)
    }

    /// Plane `g` moved by `d` (the family's axes) as the plane's own drag moves it: along its normal, through the constraints where
    /// a dimension measures to it (a label it changes taking the new length), else by itself, the reference lines on it
    /// with it. Refused by the constraints, why and where it was going (for Constraints Not Satisfied's dialog); else what went wrong.
    #[allow(clippy::type_complexity)]
    fn plane_dragged(&mut self, g: Guid, d: [f64; 3]) -> Result<(), Result<(Unsat, f64), String>> {
        let f = self.family().map_err(Err)?;
        let Some(p) = f.planes.iter().find(|p| p.guid == g) else { return Ok(()) };
        let n = p.frame().z;
        let dot = |a: [f64; 3]| a[0] * n[0] + a[1] * n[1] + a[2] * n[2];
        let along = dot(d);
        let held = f.constrained() && f.dimensions.iter().any(|m| m.refs.contains(&DimRef::Plane(g)));
        if held {
            let at = dot(p.start) + along;
            let edits = f.drag_plane(&self.values(), g, at).map_err(|e| Ok((e, at)))?;
            self.apply_edits(edits);
        } else if let Some(p) = self.planes.iter_mut().find(|p| p.guid == g) {
            for (i, k) in n.iter().enumerate() {
                p.start[i] += k * along;
                p.end[i] += k * along;
            }
            carry_lines(&mut self.lines, g, std::array::from_fn(|i| n[i] * along));
        }
        Ok(())
    }

    /// Whether the draft passes the family's checks (`Family::check`, its materials aside): what a drag must leave it doing.
    pub(super) fn sound(&self) -> bool {
        self.family().is_ok_and(|f| f.check(&|_| true).is_ok())
    }

    /// The move given up, the draft put back as it was before it: whether there was one.
    pub(super) fn cancel_move(&mut self) -> bool {
        match self.views.moving.take() {
            Some(d) => {
                d.put_back(self);
                true
            }
            None => false,
        }
    }

    /// `items` moved by `d` (the family's axes): planes, lines and forms through the constraints, the rest placed again;
    /// then each type solved and built. Refused, why; the caller puts the draft back.
    /// A refusal names what it is about as `names` does.
    pub(super) fn move_picked(&mut self, items: &[Picked], d: [f64; 3], broken: &[String], names: &Names) -> Result<(), String> {
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
                MoveRefused::Stays(s) => stays_said(&s, names),
                MoveRefused::Unsat(u) => names.origin.unsat(&u),
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
                        let was = Origin::back(&names.origin.nested, i);
                        return Err(refused(tf("nested instance {}", &[&(was + 1).to_string()])));
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
                        let was = Origin::back(&names.origin.symbols, i);
                        return Err(refused(tf("symbol {}", &[&(was + 1).to_string()])));
                    }
                    self.symbols[i] = SymbolText::of(&y);
                }
                Picked::Connector(i) => {
                    let Some(c) = self.connectors.get(i) else { continue };
                    let mut c = c.connector();
                    if let Some(at) = c.at.as_mut() {
                        if !at.iter_mut().zip(d).all(|(e, by)| shifted(e, by)) {
                            let was = Origin::back(&names.origin.connectors, i);
                            return Err(refused(tf("connector {}", &[&(was + 1).to_string()])));
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
        self.builds(broken, names)
    }

    /// The types that don't solve or build as the draft is: an edit isn't refused for them.
    pub(super) fn broken_types(&self) -> Vec<String> {
        self.family()
            .map(|f| {
                f.types
                    .keys()
                    .filter(|name| f.values(name, &BTreeMap::new()).map_or(true, |v| f.solve(&v).is_err() || f.pieces(&v).is_err()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Each type but `broken` solved and built as the draft is now, as Finish builds a form; else why not, naming what it is
    /// about as `names` does.
    pub(super) fn builds(&self, broken: &[String], names: &Names) -> Result<(), String> {
        let f = self.family()?;
        for name in f.types.keys().filter(|n| !broken.contains(n)) {
            let values = f.values(name, &BTreeMap::new())?;
            if let Err(e) = f.solve(&values) {
                return Err(format!("{name}: {}", names.origin.unsat(&e)));
            }
            if let Err(e) = f.pieces(&values) {
                return Err(format!("{name}: {e}"));
            }
        }
        Ok(())
    }

    /// Point `spot` of line `k` (of its sketch and top) of form `i` put at `p` (its host's axes): only the coordinates that
    /// change written, as numbers; the rest keep what they are, a formula too.
    pub(super) fn shift_sketch_point(
        &mut self,
        i: usize,
        k: usize,
        spot: bimcraft_core::family::Spot,
        p: [f64; 2],
        values: &BTreeMap<String, Val>,
    ) {
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
