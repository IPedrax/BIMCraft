//! FE-1: what is selected in the document of a family that isn't adaptive, kept as one set, and the selection box its plans
//! and elevations draw.
//!
//! The set (`Selection`) is the only place the selection is kept: forms, reference planes (a datum too, as a click takes
//! one), reference lines, dimensions, plan symbols, connectors and nested instances, each once, in the order picked. The
//! accessors the rest of the editor reads, one for each kind (`selected_form`, `selected_plane`, `selected_line`,
//! `selected_dimension`, `selected_symbol`, `selected_connector`, `selected_nested`), say what the set holds while it holds
//! exactly one thing, of whatever kind, and nothing while it holds several: so Properties, the contextual tab, Align, Pivot,
//! the Start and End handles and the grips act on one element as they always have, never on one element out of several.
//! With several selected, Properties say how many (Multi-Select (N)), the contextual tab is Modify | Multi-Select, Delete
//! takes them all away in one step of the document's history (none of them, saying why, when one can't go), and Escape and
//! Modify let them all go; the Delete key, the ribbon's buttons and Find's commands (`BimApp::family_find_action`) all come
//! to `delete_selected` and `modify_tool`. Each is drawn in the selection's
//! colour, in the views and in {3D}, as one selected alone is.
//!
//! The setters the rest of the editor calls (`select_form_at`, `select_symbol`, `select_connector`, `select_nested`, …)
//! make the set what they select, as a click does. Something taken away by its place in a list (a form, a symbol, a
//! connector, a nested instance) goes from the set and the places after it move up one (`Selection::taken_out`); what is
//! gone from the draft otherwise (undone, redone, or changed while another tab was in front) is dropped from the set
//! (`FamilyEditor::prune_selection`, after each undo and redo, on a tab coming to the front and at the end of every frame).
//!
//! In a plan or an elevation, a drag of the primary button that starts on no element (nor a handle, a grip or a padlock),
//! with no tool in hand, draws a box: left to right a window, solid, taking what lies wholly inside
//! it; right to left a crossing, dashed, taking what it touches too, by the one rule the adaptive family's modelling view
//! boxes by (`geom::SelectBox`), drawn as the project's plans draw theirs (`canvas::box_shapes`). Ctrl adds what it takes,
//! Shift takes that away, else it is the selection; a click on an element with Ctrl or Shift does the same with it. Each
//! element is boxed by what it is drawn with on screen: a form's or a nested instance's edges as the view draws them (an L's
//! inner corner, never the hull's chord across it, `views::outline_edges`), a plane's trace, a
//! reference line, a dimension's line, a symbol's strokes (in the plans, where symbols are drawn), a connector's place.
//! Select Elements by Face (FE-20, `selection.rs`) says what a face does: on, a click on a form's or a nested instance's
//! face selects it and a crossing box takes one by its faces too, so one drawn wholly inside a face takes what the face
//! is of; off, a crossing box wholly inside a face, meeting none of its edges, doesn't take it, and a
//! click selects by an edge alone. The box may begin inside the face of a form or a nested instance that isn't selected
//! (`Scene::under`), unless Drag Elements on Selection is on and a face selects: then that face is its element's as an edge
//! is (`selection.rs`); a drag from an element's edge, a plane, a line or a connector, or from the face of one selected (with
//! Select Elements by Face on or off), is the element's and draws none: it moves the selection (`drag_move.rs`), the
//! element selected first if it isn't. {3D} has no box: a drag there turns the view.
use super::*;
use bimcraft_core::family::constraints::DimMark;
use bimcraft_core::family::views::{self as fv, Staged, Which};
use bimcraft_core::family::{FamilyDimension, Val};
use bimcraft_core::geom::{P2, SelectBox};
use bimcraft_core::ids::Guid;
use bimcraft_core::render::{Drawing, View};
use egui::{Modifiers, Painter, Pos2, Response};

/// One thing selected in a family document that isn't adaptive: the core's, with what any front end needs of a selection
/// (`family::select`: Filter's kinds, Select by ID).
pub(crate) use bimcraft_core::family::select::Picked;

/// What a click or a box does with what it takes, by the keys held: Ctrl adds it, Shift takes it away, else it
/// is the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum How {
    Replace,
    Add,
    Remove,
}

impl How {
    pub(crate) fn of(m: Modifiers) -> How {
        if m.ctrl || m.command {
            How::Add
        } else if m.shift {
            How::Remove
        } else {
            How::Replace
        }
    }
}

/// What is selected in a family document that isn't adaptive, and the box being dragged to select more.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Selection {
    items: Vec<Picked>,
    /// What was selected before the selection was last let go or replaced by another: what Select Previous brings back
    /// (FE-20, `selection.rs`). Kept in step with the draft as the selection is.
    previous: Vec<Picked>,
    /// A box being dragged in a plan or an elevation: where it began, on screen.
    boxing: Option<Pos2>,
}

impl Selection {
    /// What is selected, in the order it was picked.
    pub(crate) fn items(&self) -> &[Picked] {
        &self.items
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn contains(&self, p: Picked) -> bool {
        self.items.contains(&p)
    }

    /// The one thing selected, while there is exactly one.
    pub(crate) fn only(&self) -> Option<Picked> {
        match self.items.as_slice() {
            [p] => Some(*p),
            _ => None,
        }
    }

    /// Everything let go: what was selected is what Select Previous brings back.
    pub(crate) fn clear(&mut self) {
        if !self.items.is_empty() {
            self.previous = std::mem::take(&mut self.items);
        }
    }

    /// What Select Previous would bring back: the selection as it was before it was last let go or replaced.
    pub(crate) fn previous(&self) -> &[Picked] {
        &self.previous
    }

    /// What Select Previous would bring back, each thing of it where `to` says it is now (none: let go): after an undo or
    /// a redo, which may put another in a form's place.
    pub(crate) fn follow_previous(&mut self, to: impl FnMut(Picked) -> Option<Picked>) {
        let mut followed: Vec<Picked> = Vec::new();
        for p in std::mem::take(&mut self.previous).into_iter().filter_map(to) {
            if !followed.contains(&p) {
                followed.push(p);
            }
        }
        self.previous = followed;
    }

    /// Select Previous: the selection as it was before it was last let go or replaced, again; what it replaces is the
    /// previous one from then on, so the two change places. Nothing with none before.
    pub(crate) fn restore_previous(&mut self) {
        if !self.previous.is_empty() {
            let previous = std::mem::take(&mut self.previous);
            self.apply(&previous, How::Replace);
        }
    }

    /// `p` let go, if it was selected.
    pub(crate) fn forget(&mut self, p: Picked) {
        self.items.retain(|q| *q != p);
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&Picked) -> bool) {
        self.items.retain(keep);
    }

    /// What a click or a box took, `taken`, made the selection, added to it or taken away from it, as `how` says.
    pub(crate) fn apply(&mut self, taken: &[Picked], how: How) {
        if how == How::Replace {
            // Replaced by another selection, it is the previous one; taken again as it is, it stays the selection.
            let was = std::mem::take(&mut self.items);
            if !was.is_empty() && (was.len() != taken.len() || was.iter().any(|p| !taken.contains(p))) {
                self.previous = was;
            }
        }
        for p in taken {
            match how {
                How::Remove => self.forget(*p),
                How::Replace | How::Add => {
                    if !self.items.contains(p) {
                        self.items.push(*p);
                    }
                }
            }
        }
    }

    /// `gone`, held by its place in a list (a form, a symbol, a connector, a nested instance), taken out of that list: it
    /// goes from the selection, and what is selected of the same list after it moves up one place.
    pub(crate) fn taken_out(&mut self, gone: Picked) {
        self.forget(gone);
        self.previous.retain(|q| *q != gone);
        for p in self.items.iter_mut().chain(&mut self.previous) {
            *p = match (*p, gone) {
                (Picked::Form(j), Picked::Form(i)) if j > i => Picked::Form(j - 1),
                (Picked::Symbol(j), Picked::Symbol(i)) if j > i => Picked::Symbol(j - 1),
                (Picked::Connector(j), Picked::Connector(i)) if j > i => Picked::Connector(j - 1),
                (Picked::Nested(j), Picked::Nested(i)) if j > i => Picked::Nested(j - 1),
                (same, _) => same,
            };
        }
    }

    /// Forms `i` and `j` swapped in their list (Move Up, Move Down): each selected stays so where it went.
    pub(crate) fn forms_swapped(&mut self, i: usize, j: usize) {
        for p in self.items.iter_mut().chain(&mut self.previous) {
            *p = match *p {
                Picked::Form(k) if k == i => Picked::Form(j),
                Picked::Form(k) if k == j => Picked::Form(i),
                same => same,
            };
        }
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// Whether a selection box is being dragged: what a test reads mid-drag.
    pub(crate) fn boxing(&self) -> bool {
        self.selection.boxing.is_some()
    }
}

impl FamilyEditor {
    /// `p` alone selected, whatever was selected let go: as a click or the Project Browser selects one.
    pub(crate) fn select_only(&mut self, p: Picked) {
        self.selection.apply(&[p], How::Replace);
    }

    /// The one thing selected, while exactly one is and the draft still has it: what the accessors of each kind read.
    pub(crate) fn only_selected(&self) -> Option<Picked> {
        self.selection.only().filter(|p| self.holds(*p))
    }

    /// Whether the draft has `p`.
    pub(crate) fn holds(&self, p: Picked) -> bool {
        match p {
            Picked::Form(i) => i < self.forms.len(),
            Picked::Plane(g) => self.planes.iter().any(|q| q.guid == g),
            Picked::Line(g) => self.lines.iter().any(|l| l.guid == g),
            Picked::Dimension(g) => self.dimensions.iter().any(|d| d.guid == g),
            Picked::Symbol(i) => i < self.symbols.len(),
            Picked::Connector(i) => i < self.connectors.len(),
            Picked::Nested(i) => i < self.nested_instances.len(),
        }
    }

    /// The forms selected, by place, while the draft has them.
    pub(crate) fn picked_forms(&self) -> impl Iterator<Item = usize> + '_ {
        self.selection.items().iter().filter_map(|p| match p {
            Picked::Form(i) if *i < self.forms.len() => Some(*i),
            _ => None,
        })
    }

    /// The planes selected, while the draft has them.
    pub(crate) fn picked_planes(&self) -> impl Iterator<Item = Guid> + '_ {
        self.selection.items().iter().filter_map(|p| match p {
            Picked::Plane(g) if self.planes.iter().any(|q| q.guid == *g) => Some(*g),
            _ => None,
        })
    }

    /// What is gone from the draft let go: after an undo or a redo, on its tab coming to the front, after each frame.
    pub(crate) fn prune_selection(&mut self) {
        let mut items = std::mem::take(&mut self.selection.items);
        items.retain(|p| self.holds(*p));
        self.selection.items = items;
        // And from what Select Previous brings back.
        let mut previous = std::mem::take(&mut self.selection.previous);
        previous.retain(|p| self.holds(*p));
        self.selection.previous = previous;
    }

    /// A click on `hit` in a plan or an elevation (none: on nothing), with the keys `mods` held: Ctrl adds it, Shift takes
    /// it away, else it alone is selected; a click on nothing lets the selection go, unless Ctrl or Shift is held.
    pub(crate) fn click_select(&mut self, hit: Option<Picked>, mods: Modifiers) {
        match (hit, How::of(mods)) {
            (Some(p), how) => self.selection.apply(&[p], how),
            (None, How::Replace) => self.let_everything_go(),
            (None, _) => {}
        }
    }

    /// Whether Delete takes something of the selection away: anything but a datum plane.
    pub(super) fn deletable_picked(&self) -> bool {
        self.selection.items().iter().any(|p| match p {
            Picked::Plane(g) => self.planes.iter().any(|q| q.guid == *g && !q.datum()),
            other => self.holds(*other),
        })
    }

    /// Delete (DE, the Delete key, the ribbon's Delete) on the selection: everything in it taken away at once, so one step of
    /// the document's history, each with what depends on it, a datum left as it is. What depends on another goes first (a
    /// dimension, a nested instance, a form, a symbol, a connector, then a reference line, then a plane), and each list from
    /// its end, so the places of the rest hold. Where one can't go (a plane something not selected is drawn on), nothing does,
    /// and the error says why.
    pub(super) fn delete_picked(&mut self) -> Result<(), String> {
        let kept = (
            self.forms.clone(),
            self.planes.clone(),
            self.lines.clone(),
            self.dimensions.clone(),
            self.symbols.clone(),
            self.connectors.clone(),
            self.nested_instances.clone(),
            self.selection.clone(),
        );
        let items: Vec<Picked> = self.selection.items().iter().copied().filter(|p| self.holds(*p)).collect();
        let places = |of: fn(Picked) -> Option<usize>| {
            let mut v: Vec<usize> = items.iter().filter_map(|p| of(*p)).collect();
            v.sort_unstable_by(|a, b| b.cmp(a));
            v
        };
        let guids = |of: fn(Picked) -> Option<Guid>| items.iter().filter_map(|p| of(*p)).collect::<Vec<Guid>>();
        let done = (|| -> Result<(), String> {
            for g in guids(|p| if let Picked::Dimension(g) = p { Some(g) } else { None }) {
                self.delete_dimension(g);
            }
            for i in places(|p| if let Picked::Nested(i) = p { Some(i) } else { None }) {
                self.remove_nested(i);
            }
            for i in places(|p| if let Picked::Form(i) = p { Some(i) } else { None }) {
                self.remove_form(i);
            }
            for i in places(|p| if let Picked::Symbol(i) = p { Some(i) } else { None }) {
                self.remove_symbol(i);
            }
            for i in places(|p| if let Picked::Connector(i) = p { Some(i) } else { None }) {
                self.remove_connector(i);
            }
            for g in guids(|p| if let Picked::Line(g) = p { Some(g) } else { None }) {
                self.delete_line(g)?;
            }
            for g in guids(|p| if let Picked::Plane(g) = p { Some(g) } else { None }) {
                if self.planes.iter().any(|q| q.guid == g && !q.datum()) {
                    self.delete_plane(g)?;
                }
            }
            Ok(())
        })();
        if let Err(e) = done {
            (self.forms, self.planes, self.lines, self.dimensions, self.symbols, self.connectors, self.nested_instances, self.selection) =
                kept;
            return Err(e);
        }
        self.prune_selection();
        Ok(())
    }

    /// Nested instance `i` taken away.
    pub(crate) fn remove_nested(&mut self, i: usize) {
        if i < self.nested_instances.len() {
            self.nested_instances.remove(i);
            self.selection.taken_out(Picked::Nested(i));
        }
    }
}

/// What a plan or an elevation shows of the family, for the box and the selection's outlines: the family as the type previewed
/// flexes it and its values, the scratch model and drawing it is drawn from, where the drawing is on screen, and the
/// dimensions' and connectors' marks the view draws over it.
pub(super) struct Scene<'a> {
    pub(super) family: &'a Family,
    pub(super) values: &'a BTreeMap<String, Val>,
    pub(super) staged: &'a Staged,
    pub(super) drawing: &'a Drawing,
    pub(super) which: Which,
    pub(super) view: View,
    /// The drawing's top left corner on screen.
    pub(super) min: Pos2,
    pub(super) dims: &'a [DimMark],
    pub(super) dimensions: &'a [FamilyDimension],
    pub(super) connectors: &'a [super::connector_marks::Mark],
    /// The document's Detail Level, which says which symbols its plans draw.
    pub(super) detail: bimcraft_core::graphics::Detail,
    /// The forms the view leaves out, by place: Temporary Hide/Isolate's, and while Preview Visibility is on those their
    /// Visibility Settings hide there (F8). No box takes them, and no click or drag picks them.
    pub(super) hidden: &'a [bool],
    /// Select Elements by Face (FE-20): a face selects what it is of, by a click and by a crossing box.
    pub(super) by_face: bool,
    /// Drag Elements on Selection: a drag from the face of a form or a nested instance not selected is the element's, as
    /// one from its edge is, where a face selects (`by_face`); else that face is the box's to begin in.
    pub(super) drag_faces: bool,
}

impl Scene<'_> {
    fn screen(&self, q: P2) -> Pos2 {
        let (x, y) = self.view.px(q);
        self.min + vec2(x, y)
    }

    /// Point `p` of the family's axes on screen.
    fn at(&self, p: [f64; 3]) -> Pos2 {
        self.screen(self.staged.to_view(self.which, p))
    }

    /// The edges form or nested instance `p` is seen with, on screen, as the view draws them (`views::outline_edges`: an L's
    /// inner corner, never the hull's chord across it); none for anything else.
    fn edges(&self, p: Picked) -> Vec<[Pos2; 2]> {
        let edges = match p {
            Picked::Form(i) => fv::outline_edges(self.family, self.values, self.staged, self.which, i),
            Picked::Nested(i) => fv::nested_outline_edges(self.family, self.values, self.staged, self.which, i),
            _ => Vec::new(),
        };
        edges.into_iter().map(|(a, b)| [self.screen(a), self.screen(b)]).collect()
    }

    /// What `p` is drawn with on screen, as segments (a point, one of no length): what a box takes it by, and what lights it.
    pub(super) fn pieces(&self, p: Picked) -> Vec<[Pos2; 2]> {
        let open = |pts: &[P2]| -> Vec<[Pos2; 2]> {
            let on: Vec<Pos2> = pts.iter().map(|q| self.at([q.0, q.1, 0.0])).collect();
            match on.as_slice() {
                [one] => vec![[*one, *one]],
                _ => on.windows(2).map(|w| [w[0], w[1]]).collect(),
            }
        };
        match p {
            Picked::Form(_) | Picked::Nested(_) => self.edges(p),
            Picked::Plane(g) => {
                let strokes = super::views::plane_strokes(self.drawing, self.staged, g, self.min);
                if !strokes.is_empty() {
                    return strokes;
                }
                let trace = self
                    .family
                    .planes
                    .iter()
                    .find(|q| q.guid == g)
                    .and_then(|q| super::views::plane_trace(self.staged, self.which, &q.frame()));
                trace.map(|(a, b)| vec![[self.screen(a), self.screen(b)]]).unwrap_or_default()
            }
            Picked::Line(g) => {
                let Some(l) = self.family.lines.iter().find(|l| l.guid == g) else { return Vec::new() };
                vec![[self.at(l.start), self.at(l.end)]]
            }
            Picked::Dimension(g) => {
                let Some(m) = self.dims.iter().find(|m| !m.alignment && self.dimensions.get(m.index).is_some_and(|d| d.guid == g)) else {
                    return Vec::new();
                };
                let pts: Vec<Pos2> = m.points.iter().map(|q| self.screen(*q)).collect();
                match (m.kind, pts.split_first()) {
                    // An angle's: from where its lines cross out along each.
                    (bimcraft_core::family::DimKind::Angular, Some((v, rays))) => rays.iter().map(|r| [*v, *r]).collect(),
                    _ => pts.windows(2).map(|w| [w[0], w[1]]).collect(),
                }
            }
            Picked::Symbol(i) => {
                // Symbols are annotation of the plans: the elevations draw none.
                if !matches!(self.which, Which::Plan | Which::Ceiling) {
                    return Vec::new();
                }
                use bimcraft_core::family::Mark;
                match self.family.symbol_mark(i, self.values, Some(self.detail)) {
                    Ok(Some(Mark::Line(pts))) => open(&pts),
                    Ok(Some(Mark::Dot { centre, radius })) => {
                        let ring: Vec<P2> = (0..=12)
                            .map(|k| {
                                let (s, c) = (std::f64::consts::TAU * f64::from(k) / 12.0).sin_cos();
                                P2(centre.0 + radius * c, centre.1 + radius * s)
                            })
                            .collect();
                        open(&ring)
                    }
                    Ok(Some(Mark::Label { at, height, text, align })) => {
                        bimcraft_core::family::lettering(&text, at, height, align).iter().flat_map(|stroke| open(stroke)).collect()
                    }
                    _ => Vec::new(),
                }
            }
            Picked::Connector(i) => {
                let Some(m) = self.connectors.iter().find(|m| m.index == i) else { return Vec::new() };
                let c = self.at(m.at);
                vec![[c, c]]
            }
        }
    }

    /// Everything the view shows that a box may take: what a click there could select, and the symbols the plans draw.
    fn candidates(&self) -> Vec<Picked> {
        let mut out: Vec<Picked> = (0..self.family.forms.len())
            .filter(|i| !self.family.forms[*i].void && !self.hidden.get(*i).copied().unwrap_or(false))
            .map(Picked::Form)
            .collect();
        out.extend((0..self.family.nested_instances.len()).map(Picked::Nested));
        let mut planes: Vec<Guid> = Vec::new();
        for g in self.drawing.owners.iter().filter_map(|o| o.and_then(|o| self.staged.plane_of(o))) {
            if !planes.contains(&g) {
                planes.push(g);
            }
        }
        out.extend(planes.into_iter().map(Picked::Plane));
        out.extend(self.family.lines.iter().map(|l| Picked::Line(l.guid)));
        out.extend(
            self.dims.iter().filter(|m| !m.alignment).filter_map(|m| self.dimensions.get(m.index)).map(|d| Picked::Dimension(d.guid)),
        );
        out.extend((0..self.family.symbols.len()).map(Picked::Symbol));
        out.extend(self.connectors.iter().map(|m| Picked::Connector(m.index)));
        out
    }

    /// What a box from `from` to `to` on screen takes: left to right what lies wholly inside it, right to left what it
    /// touches too. With Select Elements by Face on, a crossing takes a form or a nested instance by its faces as by its
    /// edges, so one drawn wholly inside a face takes what the face is of (`views::face_in_box`).
    pub(super) fn in_box(&self, from: Pos2, to: Pos2) -> Vec<Picked> {
        let mm = |p: Pos2| P2(f64::from(p.x), f64::from(p.y));
        let boxed = SelectBox::dragged(mm(from), mm(to));
        // The same box in the view's millimetres, where its faces are.
        let at = |p: Pos2| self.view.mm((p.x - self.min.x, p.y - self.min.y));
        let across = SelectBox { crossing: boxed.crossing, ..SelectBox::dragged(at(from), at(to)) };
        let by_face = |p: Picked| {
            let part = match p {
                Picked::Form(i) => fv::Part::Form(i),
                Picked::Nested(i) => fv::Part::Nested(i),
                _ => return false,
            };
            self.by_face && across.crossing && fv::face_in_box(self.family, self.values, self.staged, self.which, part, &across)
        };
        self.candidates()
            .into_iter()
            .filter(|p| {
                let pieces: Vec<(P2, P2)> = self.pieces(*p).into_iter().map(|[a, b]| (mm(a), mm(b))).collect();
                boxed.takes(&pieces) || by_face(*p)
            })
            .collect()
    }

    /// How far a click reaches, in the view's millimetres.
    fn reach(&self) -> f64 {
        6.0 / self.view.scale.max(1e-9)
    }

    /// Whether the view draws form `i`: not one it leaves out (`hidden`), which nothing picks, as no box takes it.
    fn shows(&self, i: usize) -> bool {
        !self.hidden.get(i).copied().unwrap_or(false)
    }

    /// The form whose face the view draws at `q` (the view's millimetres), nearest the eye: past one it leaves out.
    fn form_face(&self, q: P2) -> Option<usize> {
        fv::form_at_among(self.family, self.values, self.staged, self.which, q, &|i| self.shows(i))
    }

    /// The form an edge of which, as the view draws it, passes within `reach` of `q`: none it leaves out.
    fn form_edge(&self, q: P2, reach: f64) -> Option<usize> {
        fv::form_edge_at_among(self.family, self.values, self.staged, self.which, q, reach, &|i| self.shows(i))
    }

    /// The form a click at `q` (the view's millimetres) takes: with Select Elements by Face on, the one whose face is
    /// there; else, and off, the one an edge of which, as the view draws it, passes within the click's reach.
    pub(super) fn form_hit(&self, q: P2) -> Option<usize> {
        let face = if self.by_face { self.form_face(q) } else { None };
        face.or_else(|| self.form_edge(q, self.reach()))
    }

    /// The nested instance whose face a click at `q` is on, while Select Elements by Face is on: what a click takes before
    /// a reference line or a plane there, as it always did.
    pub(super) fn nested_face(&self, q: P2) -> Option<usize> {
        if self.by_face { fv::nested_at(self.family, self.values, self.staged, self.which, q) } else { None }
    }

    /// What a click at `q` takes where no reference line and no plane is: the form whose face is there (while a face
    /// selects), else the nested instance, else the form, an edge of which, as the view draws it, passes within the click's reach. An
    /// edge comes after a plane and a line, so one aligned to a plane leaves the plane beside it to be clicked.
    pub(super) fn body_hit(&self, q: P2) -> Option<Picked> {
        let reach = self.reach();
        let face = if self.by_face { self.form_face(q) } else { None };
        face.map(Picked::Form)
            .or_else(|| fv::nested_edge_at(self.family, self.values, self.staged, self.which, q, reach).map(Picked::Nested))
            .or_else(|| self.form_edge(q, reach).map(Picked::Form))
    }

    /// The nested instance or the form of `selected` whose face the view draws at `q`, nearest the eye, with Select
    /// Elements by Face on or off: what is selected is held by its face, so a drag from there draws no box. Whether a face selects is a click's question, never one about what is selected already.
    fn held_face(&self, q: P2, selected: &[Picked]) -> Option<Picked> {
        let nested = fv::nested_at(self.family, self.values, self.staged, self.which, q).map(Picked::Nested);
        nested.filter(|p| selected.contains(p)).or_else(|| self.form_face(q).map(Picked::Form).filter(|p| selected.contains(p)))
    }

    /// Whether a drag from `p` is an element's and so draws no box: a connector, a reference line, a plane, or a form or a
    /// nested instance by an edge the view draws it with. The face of a form or a nested instance is the box's to begin in
    /// (FE-20), but for one of `selected`, with Select Elements by Face on or off: dragging what is selected is its
    /// own. With Drag Elements on Selection on, a face that selects (`by_face`) is its form's or its nested instance's
    /// selected or not, so a drag from it selects that; off, or with faces selecting nothing, it is as above.
    pub(super) fn under(&self, p: Pos2, selected: &[Picked]) -> bool {
        let q = self.view.mm((p.x - self.min.x, p.y - self.min.y));
        let reach = self.reach();
        let project = |x: [f64; 3]| Some(self.at(x));
        super::connector_marks::pick(self.connectors, &project, p).is_some()
            || fv::nested_edge_at(self.family, self.values, self.staged, self.which, q, reach).is_some()
            || self.staged.line_at(self.which, q, reach).is_some()
            || self.drawing.pick_all((p.x - self.min.x, p.y - self.min.y), 6.0).into_iter().any(|id| self.staged.plane_of(id).is_some())
            || self.form_edge(q, reach).is_some()
            || self.held_face(q, selected).is_some()
            || (self.drag_faces && self.by_face && (self.nested_face(q).is_some() || self.form_face(q).is_some()))
    }

    /// What a press at `p` is on, as a click there would select it (in FE-20's order): a connector, a nested instance
    /// by its face while a face selects, a reference line, a plane (`plane`, the one Tab has come to), then a form by its
    /// face while a face selects, a nested instance or a form by an edge (`body_hit`); else, a face selecting or not, one of
    /// `selected` by its face (`held_face`), or a symbol of `selected` by its strokes, which a click doesn't select in a view
    /// but a drag moves. Whether a drag from there is the element's is [`under`](Self::under)'s to say.
    pub(super) fn hit(&self, p: Pos2, plane: Option<Guid>, selected: &[Picked]) -> Option<Picked> {
        let q = self.view.mm((p.x - self.min.x, p.y - self.min.y));
        let project = |x: [f64; 3]| Some(self.at(x));
        if let Some(i) = super::connector_marks::pick(self.connectors, &project, p) {
            return Some(Picked::Connector(i));
        }
        if let Some(i) = self.nested_face(q) {
            return Some(Picked::Nested(i));
        }
        let line = self.staged.line_at(self.which, q, self.reach()).and_then(|i| self.family.lines.get(i)).map(|l| Picked::Line(l.guid));
        let body = || self.body_hit(q);
        let symbol = || {
            selected.iter().copied().filter(|s| matches!(s, Picked::Symbol(_))).find(|s| {
                self.pieces(*s).iter().any(|[a, b]| {
                    let (a, b, c) = (P2(a.x.into(), a.y.into()), P2(b.x.into(), b.y.into()), P2(p.x.into(), p.y.into()));
                    bimcraft_core::geom::dist_to_segment(c, a, b) <= 6.0
                })
            })
        };
        line.or(plane.map(Picked::Plane)).or_else(body).or_else(|| self.held_face(q, selected)).or_else(symbol)
    }

    /// What is selected, over the picture, as one selected alone is drawn: a form's and a nested instance's edges as seen, a
    /// plane's trace, a reference line, a symbol's strokes, in the selection's colour. A dimension and a connector are lit by
    /// their own painters.
    pub(super) fn paint_selected(&self, painter: &Painter, selected: &[Picked]) {
        let picked = super::views::PICKED;
        for p in selected {
            match p {
                // A form or a nested instance by its own edges as the view sees it (its notches, its holes): what it is
                // picked and boxed by too.
                Picked::Form(_) | Picked::Nested(_) => {
                    for s in self.edges(*p) {
                        painter.line_segment(s, Stroke::new(2.0, picked));
                    }
                }
                Picked::Plane(_) | Picked::Line(_) => {
                    for s in self.pieces(*p) {
                        painter.line_segment(s, Stroke::new(2.5, picked));
                    }
                }
                Picked::Symbol(_) => {
                    for s in self.pieces(*p) {
                        painter.line_segment(s, Stroke::new(2.0, picked));
                    }
                }
                Picked::Dimension(_) | Picked::Connector(_) => {}
            }
        }
    }
}

/// Whether `at` is still where a box began at `from`: within the few pixels each way (`canvas::STILL`) the project's
/// canvas lets a drag go and still be a click. A move of the selection and a plane's drag wait for the hand to leave it too.
pub(super) fn still(from: Pos2, at: Pos2) -> bool {
    (from.x - at.x).abs().max((from.y - at.y).abs()) < crate::canvas::STILL
}

impl Selection {
    /// Where a box let go this frame began, when it was let go still there (`still`): a slow click or a shaky one, which
    /// egui calls a drag, and which is the click where it began, not a box (FE-20).
    pub(super) fn box_click(&self, ui: &Ui, response: &Response) -> Option<Pos2> {
        let from = self.boxing.filter(|_| response.drag_stopped())?;
        still(from, ui.input(|i| i.pointer.latest_pos()).unwrap_or(from)).then_some(from)
    }
}

/// The selection box of a plan or an elevation: begun at `start` (a drag from nothing), drawn while the drag lasts over
/// `pane`, solid left to right and dashed right to left as the project's plans draw theirs; where it began and where it was
/// let go, once it is. Within the few pixels a click may move (`still`) it is no box yet, and let go there none at all, but
/// the click where it began (`Selection::box_click`).
pub(super) fn track(
    sel: &mut Selection,
    ui: &Ui,
    response: &Response,
    painter: &Painter,
    pane: Rect,
    start: Option<Pos2>,
) -> Option<(Pos2, Pos2)> {
    if start.is_some() {
        sel.boxing = start;
    }
    let from = sel.boxing?;
    let now = ui.input(|i| i.pointer.latest_pos()).unwrap_or(from);
    if response.drag_stopped() {
        sel.boxing = None;
        return (!still(from, now)).then_some((from, now));
    }
    if !response.dragged_by(PointerButton::Primary) {
        sel.boxing = None;
        return None;
    }
    if !still(from, now) {
        let local = |p: Pos2| (p.x - pane.min.x, p.y - pane.min.y);
        painter.extend(crate::canvas::box_shapes(pane, local(from), local(now)));
    }
    None
}
