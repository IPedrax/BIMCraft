//! F4: the family document's floor and ceiling plans and its four elevations, as Revit's family editor has them, beside
//! {3D}. Each is the project's own drawing of a scratch model the family stands in (`family::views`: its planes made project
//! reference planes there, mapped back by GUID), drawn on the interface's thread (a one-family model is small) and kept until
//! what it shows changes. In a family that isn't adaptive a click selects a form (its Properties, as T11) or a plane, a
//! selected plane of the family's own moves by a drag, and Reference Plane (RP) draws one by two clicks: upright in plan,
//! facing across an elevation. Work Plane Set and Show work over the planes. An adaptive family's plans and elevations show
//! its test placement and nothing more: its tools are in {3D}. What each view keeps (how close, where, its work plane) is the
//! session's, never the family's. Each view shows the family as the type previewed flexes it, and its dimensions, Aligned
//! Dimension and Align are F7's (`dimensions.rs`).
use super::dimensions::{self as dims, Clicked};
use super::*;
use bimcraft_core::family::DimRef;
use bimcraft_core::family::views::{self as fv, Side, Staged, Which};
use bimcraft_core::geom::P2;
use bimcraft_core::graphics::Override;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Element;
use bimcraft_core::render::{Drawing, View};

/// What the family document's views keep for the session.
#[derive(Default)]
pub(crate) struct Views {
    /// The view shown; none, {3D}.
    pub(crate) shown: Option<Which>,
    /// How close each view is, times as fitted, and how far it is moved (mm).
    frames: BTreeMap<Which, (f64, (f64, f64))>,
    /// Each view's work plane ({3D}'s under none), once Set; else its own (`default_work_plane`).
    work_planes: BTreeMap<Option<Which>, PlaneRef>,
    /// Work Plane > Show: the work plane tinted.
    pub(crate) show_work_plane: bool,
    /// The Work Plane dialog's Pick a plane or Pick a line (T12): the next click picks.
    pub(crate) picking: Option<super::work_plane::Picking>,
    /// The Work Plane dialog, open.
    pub(crate) dialog: Option<super::work_plane::Dialog>,
    /// Go To View, asked for a plane the view shown doesn't show flat.
    pub(crate) go_to: Option<super::work_plane::GoTo>,
    /// The Reference Plane tool, on, with its first click once made.
    pub(crate) drawing: Option<Option<P2>>,
    /// The plane selected, by GUID.
    pub(crate) plane: Option<Guid>,
    /// The plane being dragged: where the pointer was, in the view's millimetres.
    pub(super) drag: Option<P2>,
    /// A selected extrusion's Start or End (true) being dragged by its handle (F5): where the pointer was, in the view's
    /// millimetres, and the normal it moves along.
    end_drag: Option<(bool, P2, [f64; 3])>,
    /// The picture shown, and what it was drawn of.
    pub(super) cache: Option<Cached>,
    /// The dimension selected, by GUID.
    pub(crate) dim: Option<Guid>,
    /// Aligned Dimension (DI): on, with the references picked so far.
    pub(crate) dimensioning: Option<Vec<DimRef>>,
    /// Align (AL): on, with the reference picked first once it is.
    pub(crate) aligning: Option<Option<DimRef>>,
    /// The padlock Align leaves until the next click: the two references, where it shows (the view's mm), in which view.
    pub(crate) align_lock: Option<(DimRef, DimRef, P2, Which)>,
    /// An Align "Constraints are not satisfied" asked about, which Remove Constraints does again.
    pub(crate) align_pending: Option<(DimRef, DimRef, P2, Which)>,
    /// "Constraints are not satisfied", asked until Remove Constraints or Cancel.
    pub(crate) unsatisfied: Option<super::dimensions::Unsatisfied>,
    /// Create Parameter…, for a dimension's label.
    pub(crate) creating: Option<super::dimensions::Creating>,
    /// What a tool of the views couldn't do, for the status bar.
    pub(crate) status: Option<String>,
    /// Where on screen the view was last shown.
    pane: Option<Rect>,
}

/// A view drawn: what it was drawn of, its scratch model, its frame and drawing (to pick in) and its picture.
pub(super) struct Cached {
    key: u64,
    pub(super) which: Which,
    pub(super) staged: Staged,
    view: View,
    drawing: Drawing,
    texture: TextureHandle,
}

/// The name a family's view goes by, as Revit's browser and title name it.
pub(crate) fn view_title(which: Option<Which>) -> String {
    match which {
        None => tf("3D View: {}", &[document::VIEW]),
        Some(Which::Plan) => tf("Floor Plan: {}", &[t("Ref. Level")]),
        Some(Which::Ceiling) => tf("Ceiling Plan: {}", &[t("Ref. Level")]),
        Some(Which::Elevation(side)) => tf("Elevation: {}", &[t(side.name())]),
    }
}

/// What the view's tab says after the family's name.
pub(crate) fn view_tab(which: Option<Which>) -> String {
    match which {
        None => document::VIEW.to_string(),
        Some(_) => view_title(which),
    }
}

/// The plane a view draws on until another is Set, as Revit's: Ref. Level in plan and in {3D} (none), the Center plane an
/// elevation faces.
fn default_work_plane(which: Option<Which>) -> PlaneRef {
    match which {
        None | Some(Which::Plan | Which::Ceiling) => PlaneRef::Plane(REF_LEVEL),
        Some(Which::Elevation(Side::Front | Side::Back)) => PlaneRef::Plane(CENTER_FRONT_BACK),
        Some(Which::Elevation(Side::Left | Side::Right)) => PlaneRef::Plane(CENTER_LEFT_RIGHT),
    }
}

/// How far beyond what the family takes up {3D} draws its work plane, mm: as far as the plans and elevations frame past it.
const WORK_PLANE_MARGIN: f64 = 300.0;

/// The selection's colour in the family's views.
pub(super) const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
/// Work Plane > Show's tint.
pub(crate) const TINT: Color32 = Color32::from_rgba_premultiplied(30, 70, 140, 60);

impl Views {
    /// The work plane of the view shown.
    pub(crate) fn work_plane(&self) -> PlaneRef {
        self.work_planes.get(&self.shown).copied().unwrap_or_else(|| default_work_plane(self.shown))
    }

    /// The work plane of the view shown made `plane`.
    pub(crate) fn set_work_plane(&mut self, plane: PlaneRef) {
        self.work_planes.insert(self.shown, plane);
    }

    /// Another view shown: the tool and the drag in hand go, the selection stays.
    pub(crate) fn show(&mut self, which: Option<Which>) {
        if self.shown != which {
            self.shown = which;
            self.drawing = None;
            self.picking = None;
            self.drag = None;
            self.dimensioning = None;
            self.aligning = None;
            self.align_lock = None;
        }
    }
}

#[cfg(test)]
impl Views {
    /// Where point `p` of the family is in the picture shown, in its pixels.
    pub(crate) fn pixel_of(&self, p: [f64; 3]) -> Option<(f32, f32)> {
        let c = self.cache.as_ref()?;
        Some(c.view.px(c.staged.to_view(c.which, p)))
    }

    /// Where point `p` of the family is on screen, in the view shown.
    pub(crate) fn screen_of(&self, p: [f64; 3]) -> Option<egui::Pos2> {
        let (x, y) = self.pixel_of(p)?;
        Some(self.pane?.min + vec2(x, y))
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// Type `name` previewed, as Family Types picks it.
    pub(crate) fn preview_type(&mut self, name: &str) {
        if let Some(i) = self.types.iter().position(|(n, _)| n == name) {
            self.picked = i;
        }
    }
}

impl FamilyEditor {
    /// Whether the family's view shown is a plan or an elevation of a family that isn't adaptive: where planes are drawn.
    pub(crate) fn editing_2d(&self) -> bool {
        self.views.shown.is_some() && self.adaptive.is_none()
    }

    /// The plane selected, while the family has it.
    pub(crate) fn selected_plane(&self) -> Option<Guid> {
        self.views.plane.filter(|g| self.planes.iter().any(|p| p.guid == *g))
    }

    /// Reference Plane (RP): the tool, in a plan or an elevation of a family that isn't adaptive; the selection goes.
    pub(crate) fn reference_plane_tool(&mut self) {
        if self.editing_2d() {
            self.views.drawing = Some(None);
            self.views.picking = None;
            self.views.plane = None;
            self.form = None;
        }
    }

    /// Plane `g` of the family's own taken away; refused, saying why, while a form or a line is drawn on it or for a datum.
    pub(crate) fn delete_plane(&mut self, g: Guid) -> Result<(), String> {
        let f = self.family()?;
        if self.planes.iter().any(|p| p.guid == g && p.datum()) {
            return Err(t("a datum stays: it can't be deleted").to_string());
        }
        let on = f.hosted_on(g);
        if !on.is_empty() {
            return Err(tf("the plane can't go while {} is drawn on it", &[&on.join(", ")]));
        }
        self.planes.retain(|p| p.guid != g);
        self.forget_reference(g);
        self.views.plane = None;
        Ok(())
    }

    /// Whether plane `g` is hidden for now (Temporary Hide/Isolate).
    fn plane_hidden(&self, g: Guid) -> bool {
        self.temp_hidden_plane(g)
    }

    /// The scratch model view `which` is drawn of, staged: the family as Preview Visibility and Temporary Hide/Isolate show it;
    /// while Preview Visibility is off, the solid forms the type hides a second component of their own, halftone, so they
    /// can still be seen and picked (not for a door or a window, whose wall holds one opening).
    fn staged(&self, base: &Model) -> Result<Staged, String> {
        let f = self.family()?;
        let values = self.values();
        let shown = temp_hide::without(&f, |i| self.temp_hidden_form(i)).unwrap_or_else(|| f.clone());
        let mut model = self.scratch(base, &shown)?;
        let mut ghost_id = None;
        if !self.document.preview_visibility
            && !f.category.hosted()
            && let Some(g) = ghosts(&f, &values, |i| self.temp_hidden_form(i))
        {
            let mut s = Session::new();
            s.model = Arc::new(model);
            let name = format!("{} {GHOST}", self.name.trim());
            let type_name = self.types.get(self.picked).map(|(n, _)| n.clone()).unwrap_or_default();
            s.model_mut().families.insert(name.clone(), g);
            let made = s.execute("component", &json!({"family": name, "type": type_name, "at": [0, 0], "params": self.preview_params}))?;
            ghost_id = made["id"].as_u64();
            model = Arc::unwrap_or_clone(s.model);
        }
        let ghost_guid = ghost_id.and_then(|id| model.elements.guid(id));
        let host_hidden = self.temp_hidden_host();
        // Its planes where the type's dimensions put them.
        let flexed = f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone());
        let mut st = fv::stage(model, &flexed, &values, &|g| !self.plane_hidden(g));
        let mut ids = vec![st.level];
        ids.extend(Side::ALL.iter().filter_map(|s| st.elevation(*s)));
        for id in ids {
            let graphics = match st.model.elements.get_mut(id) {
                Some(Element::Level(l)) => &mut l.graphics,
                Some(Element::Elevation(s)) => &mut s.graphics,
                _ => continue,
            };
            if let Some(g) = ghost_guid {
                graphics.elements.insert(g, Override { halftone: true, ..Default::default() });
            }
            if host_hidden {
                graphics.categories.insert("wall".into(), Override { hidden: true, ..Default::default() });
            }
        }
        Ok(st)
    }

    /// The view shown, drawn again when what it shows or how it's looked at changes.
    fn refresh_view(&mut self, ctx: &Context, base: &Model, which: Which, size: [u32; 2]) {
        let (zoom, pan) = self.views.frames.get(&which).copied().unwrap_or((1.0, (0.0, 0.0)));
        let key = {
            let mut h = DefaultHasher::new();
            (
                self.family().ok().map(|f| serde_json::to_string(&f).unwrap_or_default()),
                self.picked,
                self.name.trim(),
                which,
                size,
                zoom.to_bits(),
                pan.0.to_bits(),
                pan.1.to_bits(),
                self.document.preview_visibility,
                self.temporary(),
                crate::style::canvas_dark(),
            )
                .hash(&mut h);
            (base.materials.len(), base.images.len()).hash(&mut h);
            h.finish()
        };
        if self.views.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let drawn = self.staged(base).and_then(|mut st| {
            let shown = st.draw(which, size[0], size[1], zoom, P2(pan.0, pan.1))?;
            Ok((st, shown))
        });
        match drawn {
            Ok((staged, shown)) => {
                let pixels = shown.rgba.as_chunks::<4>().0.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
                let picture =
                    ColorImage { size: [shown.view.width as usize, shown.view.height as usize], pixels, source_size: Default::default() };
                let texture = ctx.load_texture("family-view", picture, TextureOptions::LINEAR);
                self.views.cache = Some(Cached { key, which, staged, view: shown.view, drawing: shown.drawing, texture });
                self.problem = None;
            }
            Err(e) => {
                self.views.cache = None;
                self.problem = Some(e);
            }
        }
    }

    /// The plan or elevation shown, filling `pane`: its picture, what is selected over it, the work plane tinted while Show is
    /// on, the plane being drawn; and the clicks and drags in it.
    pub(crate) fn view_2d(&mut self, ui: &mut Ui, base: &Model, which: Which, pane: Rect) -> Option<String> {
        let size = [pane.width().clamp(64.0, 2400.0) as u32, pane.height().clamp(64.0, 1600.0) as u32];
        self.refresh_view(ui.ctx(), base, which, size);
        let response = ui.interact(pane, UiId::new("family-view-2d"), Sense::click_and_drag());
        self.views.pane = Some(pane);
        let c = self.views.cache.as_ref().filter(|c| c.which == which)?;
        let view = c.view;
        let painter = ui.painter_at(pane);
        painter.image(
            c.texture.id(),
            Rect::from_min_size(pane.min, vec2(view.width as f32, view.height as f32)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            pane.min + vec2(x, y)
        };
        let at_view = |p: egui::Pos2| view.mm((p.x - pane.min.x, p.y - pane.min.y));
        let editable = self.adaptive.is_none();
        // A form being sketched (F5): the view draws and picks for the sketch alone.
        if editable && self.sketch.is_some() {
            let cache = self.views.cache.take()?;
            let family = self.family().ok();
            let values = self.values();
            let editing = self.sketch.as_ref().and_then(|s| s.index);
            let planes = self.planes.clone();
            let reach = 6.0 / view.scale.max(1e-9);
            // The Work Plane dialog's Pick a plane or Pick a line, from sketch mode's Set (T12): the click is its.
            if self.views.picking.is_some() {
                let mut problem = None;
                if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let (x, y) = (p.x - pane.min.x, p.y - pane.min.y);
                    let plane = cache.drawing.pick_all((x, y), 6.0).into_iter().find_map(|id| cache.staged.plane_of(id));
                    let line = cache.staged.line_at(which, at_view(p), reach);
                    problem = self.picked(&Arc::new(base.clone()), plane, line);
                }
                self.views.cache = Some(cache);
                self.zoom_and_pan(ui, &response, which, view.scale);
                return problem;
            }
            // Pick Lines: a reference plane's or a reference line's trace, else the nearest edge of another form's outline.
            let pick = |p: egui::Pos2| -> Option<(P2, P2)> {
                let (x, y) = (p.x - pane.min.x, p.y - pane.min.y);
                let picked = cache.drawing.pick_all((x, y), 6.0).into_iter().find_map(|id| cache.staged.plane_of(id));
                if let Some(g) = picked
                    && let Some(plane) = planes.iter().find(|q| q.guid == g)
                {
                    return plane_trace(&cache.staged, which, &plane.frame());
                }
                let q = at_view(p);
                let f = family.as_ref()?;
                let mut best: Option<(f64, (P2, P2))> = None;
                for i in (0..f.forms.len()).filter(|i| Some(*i) != editing) {
                    let ring = fv::outline(f, &values, &cache.staged, which, i);
                    for k in 0..ring.len() {
                        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                        let d = bimcraft_core::geom::dist_to_segment(q, a, b);
                        if d <= reach && best.is_none_or(|(e, _)| d < e) {
                            best = Some((d, (a, b)));
                        }
                    }
                }
                best.map(|(_, s)| s)
            };
            let problem = self.sketch_in_view(ui, &response, &painter, &cache.staged, which, &view, pane, &pick);
            self.views.cache = Some(cache);
            self.zoom_and_pan(ui, &response, which, view.scale);
            return problem;
        }
        let values = self.values();
        let family = self.family().ok();
        // The family as the type previewed flexes it: where its dimensions put its planes and what is drawn on them.
        let family: Option<Family> = family.map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
        let on_screen = dims::screen_of(view, pane.min);
        let pick_plane = |p: egui::Pos2| -> Option<Guid> {
            let (x, y) = (p.x - pane.min.x, p.y - pane.min.y);
            c.drawing.pick_all((x, y), 6.0).into_iter().find_map(|id| c.staged.plane_of(id))
        };
        let reference_at = |p: egui::Pos2| -> Option<DimRef> {
            dims::reference_at(family.as_ref()?, &values, &c.staged, which, pick_plane(p), p, &on_screen)
        };
        // Its dimensions, the one being placed with them, over the picture: the selected one, and those that can't hold, picked.
        let placing = match (&self.views.dimensioning, response.hover_pos(), family.as_ref()) {
            (Some(refs), Some(h), Some(f)) if !refs.is_empty() => {
                let mut refs = refs.clone();
                if let Some(r) = reference_at(h).filter(|r| !refs.contains(r) && dims::parallel(f, &values, &refs[0], r)) {
                    refs.push(r);
                }
                (refs.len() >= 2).then(|| dims::new_dimension(f, &values, &c.staged, which, &refs, at_view(h)))
            }
            _ => None,
        };
        let marks = match family.as_ref() {
            Some(f) if editable => dims::marks_of(f, &values, &c.staged, which, placing),
            _ => Vec::new(),
        };
        let selected_dim = self.selected_dimension();
        let unsatisfied: Vec<Guid> = self.views.unsatisfied.as_ref().map(|u| u.dims.clone()).unwrap_or_default();
        for m in &marks {
            let g = self.dimensions.get(m.index).map(|d| d.guid);
            let picked = g.is_some() && (g == selected_dim || g.is_some_and(|g| unsatisfied.contains(&g)));
            dims::paint(&painter, m, &on_screen, picked, base.units);
        }
        // The references Aligned Dimension and Align have picked.
        let picked_refs: Vec<DimRef> = match (&self.views.dimensioning, &self.views.aligning) {
            (Some(refs), _) => refs.clone(),
            (_, Some(Some(r))) => vec![*r],
            _ => Vec::new(),
        };
        if let Some(f) = family.as_ref() {
            let ends = bimcraft_core::family::constraints::end_faces(f, &values, &c.staged, which);
            for r in &picked_refs {
                let seg = match r {
                    DimRef::Plane(g) => f.planes.iter().find(|p| p.guid == *g).and_then(|p| plane_trace(&c.staged, which, &p.frame())),
                    DimRef::Form(..) => ends.iter().find(|(e, ..)| e == r).map(|(_, a, b)| (*a, *b)),
                };
                if let Some((a, b)) = seg {
                    painter.line_segment([screen(a), screen(b)], Stroke::new(2.5, PICKED));
                }
            }
        }
        // Align's padlock, open, until the next click.
        let align_lock = self.views.align_lock.filter(|(.., w)| *w == which).map(|(_, _, at, _)| screen(at));
        if let Some(at) = align_lock {
            dims::padlock(&painter, at, false, pal().accent);
        }
        // What is selected, over the picture.
        if editable && let (Some(i), Some(f)) = (self.selected_form(), family.as_ref()) {
            let ring: Vec<egui::Pos2> = fv::outline(f, &values, &c.staged, which, i).into_iter().map(screen).collect();
            if ring.len() > 2 {
                painter.add(egui::Shape::closed_line(ring, Stroke::new(2.0, PICKED)));
            }
        }
        let trace = |g: Guid| -> Option<(egui::Pos2, egui::Pos2)> {
            let p =
                family.as_ref().and_then(|f| f.planes.iter().find(|p| p.guid == g)).or_else(|| self.planes.iter().find(|p| p.guid == g))?;
            plane_trace(&c.staged, which, &p.frame()).map(|(a, b)| (screen(a), screen(b)))
        };
        if editable && let Some((a, b)) = self.selected_plane().and_then(trace) {
            painter.line_segment([a, b], Stroke::new(2.5, PICKED));
        }
        // Work Plane > Show: the plane tinted, all over where the view looks straight at it, along its trace where across it.
        if editable
            && self.views.show_work_plane
            && let Ok(f) = self.plane_set().frame(&self.views.work_plane())
        {
            match plane_trace(&c.staged, which, &f) {
                Some((a, b)) => {
                    painter.line_segment([screen(a), screen(b)], Stroke::new(10.0, TINT));
                }
                None => {
                    painter.rect_filled(pane, 0.0, TINT);
                }
            }
        }
        // The plane being drawn, from its first click to the pointer.
        if let Some(Some(first)) = self.views.drawing
            && let Some(hover) = response.hover_pos()
        {
            painter.line_segment([screen(first), hover], Stroke::new(1.5, PICKED));
        }
        // A selected extrusion's Start and End, where the view sees its plane edge-on: shape handles to drag (F5); one tied to
        // a parameter or a formula, greyed.
        let handles = if editable { self.end_handles(&c.staged, which) } else { Vec::new() };
        for (at, end, free, n) in &handles {
            let centre = screen(*at);
            let towards = c.staged.to_view(which, [n[0], n[1], n[2]]) - c.staged.to_view(which, [0.0; 3]);
            let way = egui::vec2(towards.0 as f32, -towards.1 as f32).normalized() * if *end { 1.0 } else { -1.0 };
            let side = egui::vec2(-way.y, way.x);
            let tip = [centre + way * 9.0, centre + side * 6.0, centre - side * 6.0];
            let colour = if *free { PICKED } else { pal().muted };
            painter.add(egui::Shape::convex_polygon(tip.to_vec(), colour, Stroke::new(1.0, Color32::WHITE)));
        }
        let mut problem = None;
        let mut end_moved: Option<(bool, f64)> = None;
        if response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some((_, end, free, n)) = handles.iter().find(|h| screen(h.0).distance(p) <= 10.0)
        {
            if *free {
                self.views.end_drag = Some((*end, at_view(p), *n));
            } else {
                problem = Some(t("This end is tied to a parameter or worked out by a formula: change it in Family Types.").to_string());
            }
        }
        if let Some((end, from, n)) = self.views.end_drag
            && let Some(to) = response.interact_pointer_pos().map(at_view)
            && response.dragged_by(PointerButton::Primary)
        {
            let d = c.staged.vector(which, P2(to.0 - from.0, to.1 - from.1));
            end_moved = Some((end, d[0] * n[0] + d[1] * n[1] + d[2] * n[2]));
            self.views.end_drag = Some((end, to, n));
        }
        let mut select: Option<(Option<usize>, Option<Guid>)> = None;
        let mut moved: Option<(Guid, [f64; 3])> = None;
        let mut new_plane = None;
        let mut picked_work_plane = None;
        let mut new_dim = None;
        let mut align = None;
        let mut on_dims: Option<(Clicked, Vec<f64>)> = None;
        let mut clicked_any = false;
        // A double click on a form sketches it again, as Edit Extrusion or Edit Revolve does (F5).
        let edit = (editable && response.double_clicked() && self.views.drawing.is_none() && self.views.picking.is_none())
            .then(|| response.interact_pointer_pos())
            .flatten()
            .and_then(|p| family.as_ref().and_then(|f| fv::form_at(f, &values, &c.staged, which, at_view(p))));
        // An adaptive family's views only show it: its tools are in {3D}.
        if editable
            && response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let q = at_view(p);
            clicked_any = true;
            if self.views.picking.is_some() {
                // Pick a line: the reference line within the reach a plane's trace has.
                let line = c.staged.line_at(which, q, 6.0 / view.scale.max(1e-9));
                picked_work_plane = Some((pick_plane(p), line));
            } else if let Some(first) = self.views.drawing {
                match first {
                    None => self.views.drawing = Some(Some(q)),
                    Some(a) => {
                        new_plane = fv::new_plane(&c.staged, which, a, q);
                        self.views.drawing = Some(None);
                    }
                }
            } else if align_lock.is_some_and(|at| (at - p).length() < 10.0) {
                // Align's padlock, waiting: it locks what was aligned, whatever tool is on.
                on_dims = Some((Clicked::AlignLock, Vec::new()));
            } else if let Some(refs) = self.views.dimensioning.as_mut() {
                // Aligned Dimension: references one after another, parallel to the first; a click on nothing places it.
                match (reference_at(p), family.as_ref()) {
                    (Some(r), Some(f)) => {
                        if refs.first().is_some_and(|first| !dims::parallel(f, &values, first, &r)) {
                            problem = Some(t("Pick a reference parallel to the first.").to_string());
                        } else if !refs.contains(&r) {
                            refs.push(r);
                        }
                    }
                    (None, Some(f)) if refs.len() >= 2 => {
                        new_dim = Some(dims::new_dimension(f, &values, &c.staged, which, refs, q));
                        refs.clear();
                    }
                    _ => problem = Some(t("Pick two references or more, then click where the dimension goes.").to_string()),
                }
            } else if let Some(first) = self.views.aligning {
                // Align: the reference to align to, then what moves onto it.
                match (reference_at(p), first) {
                    (Some(r), None) => self.views.aligning = Some(Some(r)),
                    (Some(r), Some(a)) if r != a => {
                        align = Some((a, r, q));
                        self.views.aligning = Some(None);
                    }
                    _ => problem = Some(t("Pick a reference plane or an extrusion's end.").to_string()),
                }
            } else if let Some(hit) = dims::clicked(&marks, &self.dimensions, selected_dim, align_lock, p, &on_screen) {
                let lengths = match &hit {
                    Clicked::Lock(g, _) => marks
                        .iter()
                        .find(|m| self.dimensions.get(m.index).is_some_and(|d| d.guid == *g))
                        .map(|m| m.lengths.clone())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                on_dims = Some((hit, lengths));
            } else {
                let plane = pick_plane(p);
                let form = family.as_ref().and_then(|f| fv::form_at(f, &values, &c.staged, which, q));
                select = Some(match (plane, form) {
                    (Some(g), _) => (None, Some(g)),
                    (None, Some(i)) => (Some(i), None),
                    (None, None) => (None, None),
                });
            }
        }
        // A plane of the family's own, selected, dragged square to itself.
        if editable
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.end_drag.is_none()
            && response.dragged_by(PointerButton::Primary)
        {
            let now = response.interact_pointer_pos().map(at_view);
            // From where the button went down: the drag begins only once the hand has moved on.
            if response.drag_started()
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                let own = self.selected_plane().filter(|g| self.planes.iter().any(|q| q.guid == *g && !q.datum()));
                self.views.drag = own.filter(|g| pick_plane(p) == Some(*g)).map(|_| at_view(p));
            }
            if let (Some(from), Some(to), Some(g)) = (self.views.drag, now, self.selected_plane()) {
                moved = Some((g, c.staged.vector(which, P2(to.0 - from.0, to.1 - from.1))));
                self.views.drag = Some(to);
            }
        }
        if response.drag_stopped() {
            self.views.drag = None;
            self.views.end_drag = None;
        }
        self.zoom_and_pan(ui, &response, which, view.scale);
        if let Some((end, d)) = end_moved
            && d != 0.0
            && let Err(e) = self.drag_end(end, d)
        {
            problem = Some(e);
        }
        if let Some(i) = edit
            && let Err(e) = self.edit_form_sketch(i)
        {
            problem = Some(e);
        }
        if let Some((plane, line)) = picked_work_plane {
            let base = Arc::new(base.clone());
            problem = self.picked(&base, plane, line);
        }
        // The next click lets Align's padlock go, unless it locks it.
        if clicked_any && !matches!(on_dims, Some((Clicked::AlignLock, _))) && align.is_none() {
            self.views.align_lock = None;
        }
        if let Some(d) = new_dim {
            self.dimensions.push(d);
        }
        if let Some((a, b, q)) = align {
            self.align_through(a, b, q, which);
        }
        if let Some((hit, lengths)) = on_dims {
            let done = match hit {
                Clicked::Select(g) => {
                    (self.views.dim, self.views.plane, self.form) = (Some(g), None, None);
                    Ok(())
                }
                Clicked::Lock(g, i) => self.toggle_lock(g, i, lengths.get(i).copied().unwrap_or(0.0)),
                Clicked::Equal(g) => self.toggle_equal(g),
                Clicked::Unlock(g) => {
                    self.delete_dimension(g);
                    Ok(())
                }
                Clicked::AlignLock => {
                    self.lock_alignment();
                    Ok(())
                }
            };
            if let Err(e) = done {
                problem = Some(e);
            }
        }
        if let Some((form, plane)) = select {
            self.views.dim = None;
            self.form = form;
            self.views.plane = plane;
            if let Some(i) = form {
                self.tab = Tab::Forms;
                self.document.reveal = Some(i);
            }
        }
        // A plane its dimensions hold is dragged through them: a label it changes takes the new length, what is locked to it
        // follows, and what can't hold says so.
        let held = family.as_ref().is_some_and(|f| {
            f.constrained() && f.dimensions.iter().any(|d| d.refs.contains(&DimRef::Plane(moved.map_or(Guid::default(), |m| m.0))))
        });
        if let Some((g, d)) = moved
            && held
            && let Some(p) = family.as_ref().and_then(|f| f.planes.iter().find(|p| p.guid == g))
        {
            let n = p.frame().z;
            let at = p.start[0] * n[0] + p.start[1] * n[1] + p.start[2] * n[2] + d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            self.drag_through(g, at);
        } else if let Some((g, d)) = moved
            && let Some(p) = self.planes.iter_mut().find(|p| p.guid == g)
        {
            let n = p.frame().z;
            let along = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            for (i, k) in n.iter().enumerate() {
                p.start[i] += k * along;
                p.end[i] += k * along;
            }
        }
        if let Some((start, end, up)) = new_plane {
            let guid = Guid::new();
            self.planes.push(RefPlane {
                id: 0,
                guid,
                name: String::new(),
                start,
                end,
                up,
                reference: Default::default(),
                defines_origin: false,
                subcategory: None,
                level: false,
            });
            self.views.plane = Some(guid);
            self.form = None;
        }
        problem.or_else(|| self.views.status.take())
    }
}

impl FamilyEditor {
    /// How close view `which` is, by the wheel over it, and where, moved by the middle button (its `scale`, pixels a mm).
    fn zoom_and_pan(&mut self, ui: &Ui, response: &egui::Response, which: Which, scale: f64) {
        let mut frame = self.views.frames.get(&which).copied().unwrap_or((1.0, (0.0, 0.0)));
        let before = frame;
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                frame.0 = (frame.0 * f64::from(scroll * 0.003).exp()).clamp(0.05, 50.0);
            }
        }
        if response.dragged_by(PointerButton::Middle) {
            let d = response.drag_delta();
            frame.1 = (frame.1.0 - f64::from(d.x) / scale, frame.1.1 + f64::from(d.y) / scale);
        }
        if frame != before {
            self.views.frames.insert(which, frame);
        }
    }

    /// The work plane {3D} shows, while Work Plane > Show is on in a family that isn't adaptive: its frame.
    pub(crate) fn shown_work_plane_3d(&self) -> Option<bimcraft_core::family::Frame> {
        if self.adaptive.is_some() || !self.views.show_work_plane {
            return None;
        }
        self.plane_set().frame(&self.views.work_plane()).ok()
    }
}

/// Work Plane > Show in the family's {3D}: plane `f` drawn into its picture `image` (taken through `cam`), as the project's 3D
/// view lays out its work plane (`workplane::patch`) over what the family takes up (`extents`) and [`WORK_PLANE_MARGIN`]
/// round, tinted as the plans tint it, with a grid through the family's origin about 5 to 20 lines across
/// (`workplane::grid_step`) and its outline; depth tested, so a form in front of it hides it.
pub(crate) fn draw_work_plane_3d(
    image: &mut bimcraft_core::view3d::Image,
    cam: &bimcraft_core::view3d::Camera,
    f: &bimcraft_core::family::Frame,
    extents: ([f64; 3], [f64; 3]),
) {
    let plane = bimcraft_core::workplane::Frame { origin: f.origin, normal: f.z };
    let Some(p) = crate::workplane::patch(plane, extents, WORK_PLANE_MARGIN, [0.0; 3]) else { return };
    let step = bimcraft_core::workplane::grid_step((p.hi.0 - p.lo.0).max(p.hi.1 - p.lo.1));
    let (grid, edge) = (pal().muted.gamma_multiply(0.5), pal().accent.gamma_multiply(0.6));
    image.draw_plane(cam, p.origin, p.axes, (p.lo, p.hi), step, TINT.to_array(), grid.to_array(), edge.to_array());
}

/// The solid forms the type previewed (`values`) hides, alone: what a view draws halftone while Preview Visibility is off.
/// Every other form, and each `hidden` for now, is left out. None when the type hides none.
fn ghosts(f: &Family, values: &BTreeMap<String, bimcraft_core::family::Val>, hidden: impl Fn(usize) -> bool) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for (i, form) in g.forms.iter_mut().enumerate() {
        let off = !form.void && !hidden(i) && form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0));
        form.visible = if off { None } else { Some(Expr::Number(0.0)) };
        any |= off;
    }
    any.then_some(g)
}

/// What the ghost family is called beside the family in a view's scratch model.
const GHOST: &str = "(hidden in type)";

/// Where a plane of frame `f` crosses view `which`: its trace across the view's room, ends in the view's millimetres. None
/// for a plane the view looks straight at (or askew), which has none.
pub(super) fn plane_trace(st: &Staged, which: Which, f: &bimcraft_core::family::Frame) -> Option<(P2, P2)> {
    let eye = match which {
        Which::Plan | Which::Ceiling => [0.0, 0.0, 1.0],
        Which::Elevation(side) => {
            let l = side.looking();
            [l.0, l.1, 0.0]
        }
    };
    let n = f.z;
    if (n[0] * eye[0] + n[1] * eye[1] + n[2] * eye[2]).abs() > 1e-6 {
        return None;
    }
    // Along it, square to the way the view looks.
    let d = [n[1] * eye[2] - n[2] * eye[1], n[2] * eye[0] - n[0] * eye[2], n[0] * eye[1] - n[1] * eye[0]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    let reach = (0..3).map(|i| (st.room.1[i] - st.room.0[i]).abs()).fold(0.0, f64::max);
    let at = |k: f64| std::array::from_fn(|i| f.origin[i] + d[i] / len * k * reach);
    Some((st.to_view(which, at(-1.0)), st.to_view(which, at(1.0))))
}
