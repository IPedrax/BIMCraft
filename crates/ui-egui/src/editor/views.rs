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
use bimcraft_core::family::views::{self as fv, Side, Staged, Which};
use bimcraft_core::family::{DimRef, FamilyDimension};
use bimcraft_core::geom::P2;
use bimcraft_core::graphics::Override;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Element;
use bimcraft_core::render::{Drawing, Rgb, View};

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
    /// Family Element Visibility Settings, open (F8).
    pub(crate) visibility: Option<super::visibility::Dialog>,
    /// The document's Detail Level, which every one of its views draws at (F8): the session's, Fine when a family opens.
    pub(crate) detail: bimcraft_core::graphics::Detail,
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
    /// Aligned Dimension (DI): on, with the references picked so far; Radial or Diameter Dimension too (`dim_kind`), its
    /// circle picked once it is.
    pub(crate) dimensioning: Option<Vec<DimRef>>,
    /// What the dimension tool on measures: aligned, or a circle's radius or diameter (F7b).
    pub(crate) dim_kind: bimcraft_core::family::DimKind,
    /// Align (AL): on, with the reference picked first once it is.
    pub(crate) aligning: Option<Option<DimRef>>,
    /// The padlock Align leaves until the next click: the two references, where it shows (the view's mm), in which view.
    pub(crate) align_lock: Option<(DimRef, DimRef, P2, Which)>,
    /// Where each Align padlock was last shown on screen, by its dimension's place (the one Align offers, past the last).
    pub(crate) locks_shown: Vec<(usize, egui::Pos2)>,
    /// Where Align's offered padlock was last shown on screen.
    pub(crate) offer_shown: Option<egui::Pos2>,
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
    /// The reference line selected, by GUID (F7d).
    pub(crate) line: Option<Guid>,
    /// Where the pointer was last over {3D} and the camera {3D} was seen through, and the form or nested instance under it
    /// there (T11's hover).
    pub(crate) hover3d: Option<(egui::Pos2, bimcraft_core::view3d::Camera, Option<bimcraft_core::family::views::Part>)>,
    /// Pivot, on: the line or plane the next click pins.
    pub(crate) pivoting: Option<Guid>,
    /// A grip being dragged, and where the pointer was (the view's mm).
    pub(super) grip: Option<(super::pivot::Grip, P2)>,
    /// Where the selected element's grips were last shown on screen.
    pub(crate) grips_shown: Vec<(super::pivot::Grip, egui::Pos2)>,
    /// What is typed in a pivot's rows of Properties: its element, its angle, its minimum and its maximum.
    pub(crate) pivot_text: Option<(Guid, String, String, String)>,
    /// The planes under the pointer, where several lie on one another (a Sill plane on Ref. Level, F10c), and which of them
    /// a click takes: Tab steps through them, as Revit's Tab does.
    pub(crate) under: Vec<Guid>,
    pub(crate) cycle: usize,
}

/// The planes the view draws under pixel `(x, y)` of `drawing` (within reach), each once, nearest first.
fn planes_under(drawing: &Drawing, staged: &Staged, at: (f32, f32)) -> Vec<Guid> {
    let mut out: Vec<Guid> = Vec::new();
    for g in drawing.pick_all(at, 6.0).into_iter().filter_map(|id| staged.plane_of(id)) {
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

/// A view drawn: what it was drawn of, its scratch model, its frame and drawing (to pick in) and its picture.
pub(super) struct Cached {
    key: u64,
    pub(super) which: Which,
    pub(super) staged: Staged,
    view: View,
    drawing: Drawing,
    /// An elevation's hidden-line picture, which its drawing is painted over; and the picture itself, kept for a test.
    texture: Option<TextureHandle>,
    #[cfg(test)]
    picture: Option<bimcraft_core::view3d::Image>,
}

#[cfg(test)]
impl FamilyEditor {
    /// The view shown's pixels, as its drawing paints over its picture: what a test reads where the painted vectors are.
    pub(crate) fn view_pixels(&self) -> Option<ColorImage> {
        let c = self.views.cache.as_ref()?;
        let rgba = bimcraft_core::render::rgba(&c.drawing, c.picture.as_ref()).ok()?;
        let pixels = rgba.as_chunks::<4>().0.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
        Some(ColorImage { size: [c.view.width as usize, c.view.height as usize], pixels, source_size: Default::default() })
    }
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
pub(crate) const WORK_PLANE_MARGIN: f64 = 300.0;

/// The selection's colour in the family's views.
pub(super) const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
/// How far a dimension's tick reaches from its point, in points: what a padlock keeps clear of.
const TICK: f32 = 7.0;

/// The plane a click would take where several lie on one another (F10c): lit, lighter than one selected.
const PRESELECT: Color32 = Color32::from_rgb(110, 170, 240);
/// Work Plane > Show's tint, the project's 3D view's.
pub(crate) const TINT: Color32 = crate::workplane::TINT;

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
            self.dim_kind = Default::default();
            self.aligning = None;
            self.align_lock = None;
        }
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// The padlocks shown while a form is sketched, where they are on screen in the view shown (F7b).
    pub(crate) fn sketch_padlocks_shown(&self) -> Vec<(egui::Pos2, super::sketch::Padlock)> {
        let (Some(c), Some(pane), Some(frame)) = (self.views.cache.as_ref(), self.views.pane, self.sketch_frame()) else {
            return Vec::new();
        };
        let on_screen = |q: P2| {
            let (x, y) = c.view.px(super::sketch::in_view(&c.staged, c.which, &frame, q));
            Some(pane.min + vec2(x, y))
        };
        self.sketch_padlocks(&on_screen, &drawn_lines(&c.drawing, pane.min))
    }

    /// The view's drawing's lines on screen.
    pub(crate) fn view_lines_shown(&self) -> Vec<[egui::Pos2; 2]> {
        match (self.views.cache.as_ref(), self.views.pane) {
            (Some(c), Some(pane)) => drawn_lines(&c.drawing, pane.min),
            _ => Vec::new(),
        }
    }

    /// The sketch's lines and the view's drawing on screen, while a form is sketched: what its padlocks keep off.
    pub(crate) fn sketch_lines_shown(&self) -> Vec<[egui::Pos2; 2]> {
        let (Some(c), Some(pane), Some(frame), Some(s)) =
            (self.views.cache.as_ref(), self.views.pane, self.sketch_frame(), self.sketch.as_ref())
        else {
            return Vec::new();
        };
        let on_screen = |q: P2| {
            let (x, y) = c.view.px(super::sketch::in_view(&c.staged, c.which, &frame, q));
            pane.min + vec2(x, y)
        };
        let mut out = drawn_lines(&c.drawing, pane.min);
        for l in &s.sketch.lines {
            let pts: Vec<egui::Pos2> = l.curve.points().into_iter().map(on_screen).collect();
            out.extend(pts.windows(2).map(|w| [w[0], w[1]]));
        }
        out
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
    fn staged(&self, base: &Model, which: Which) -> Result<Staged, String> {
        let f = self.family()?;
        let values = self.values();
        let shown = temp_hide::without(&f, |i| self.temp_hidden_form(i)).unwrap_or_else(|| f.clone());
        let mut model = self.scratch(base, &shown)?;
        let mut ghost_id = None;
        // F8: what the view's Visibility Settings leave out, at the document's Detail Level, drawn halftone too.
        let cut = model.levels().first().map_or(bimcraft_core::model::CUT_HEIGHT, |(_, l)| l.view_range.cut);
        let by_view = fv::hidden_in(&f, &values, Some(which), self.views.detail, cut);
        if !self.document.preview_visibility
            && let Some(g) = ghosts(&f, &values, |i| self.temp_hidden_form(i), &by_view)
        {
            let mut s = Session::new();
            s.model = Arc::new(model);
            let name = format!("{} {GHOST}", self.name.trim());
            let type_name = self.types.get(self.picked).map(|(n, _)| n.clone()).unwrap_or_default();
            // A door's or a window's ghost stands where its family's axes are in the wall: its opening's frame.
            let mut place = json!({"family": name, "type": type_name, "at": [0, 0], "params": self.preview_params});
            let frame = s.model.elements.values().find_map(|e| match e {
                Element::Door(o) | Element::Window(o) => s.model.opening_frame(o),
                _ => None,
            });
            if let Some((p, _, z)) = frame {
                (place["at"], place["offset"]) = (json!([p.0, p.1]), json!(z));
            }
            super::at_origin(&f, &mut place);
            s.model_mut().families.insert(name.clone(), g);
            let made = s.execute("component", &place)?;
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
            graphics.detail = self.views.detail;
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
                self.views.detail,
                crate::style::canvas_dark(),
            )
                .hash(&mut h);
            (base.materials.len(), base.images.len()).hash(&mut h);
            h.finish()
        };
        if self.views.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let drawn = self.staged(base, which).and_then(|mut st| {
            let shown = st.draw_lines(which, size[0], size[1], zoom, P2(pan.0, pan.1))?;
            Ok((st, shown))
        });
        match drawn {
            Ok((staged, shown)) => {
                // The drawing is painted as vectors, as a project's plan is; an elevation's hidden-line picture goes under it, as a
                // section's does, dark on a dark canvas as the drawing over it is.
                let texture = shown.picture.as_ref().map(|image| {
                    let pixels = image.rgba.as_chunks::<4>().0.iter().map(|p| crate::canvas::color(Rgb(p[0], p[1], p[2]))).collect();
                    let picture =
                        ColorImage { size: [image.width as usize, image.height as usize], pixels, source_size: Default::default() };
                    ctx.load_texture("family-view", picture, TextureOptions::NEAREST)
                });
                self.views.cache = Some(Cached {
                    key,
                    which,
                    staged,
                    view: shown.view,
                    drawing: shown.drawing,
                    texture,
                    #[cfg(test)]
                    picture: shown.picture,
                });
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
        painter.rect_filled(pane, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        if let Some(t) = &c.texture {
            let at = Rect::from_min_size(pane.min, vec2(view.width as f32, view.height as f32));
            painter.image(t.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        crate::canvas::paint(&painter, crate::canvas::Place::at(pane.min), &c.drawing, &[], None, false, &[]);
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            pane.min + vec2(x, y)
        };
        let at_view = |p: egui::Pos2| view.mm((p.x - pane.min.x, p.y - pane.min.y));
        let editable = self.adaptive.is_none();
        // The planes under the pointer, and Tab stepping through them where several lie on one another (F10c).
        let under =
            response.hover_pos().map(|p| planes_under(&c.drawing, &c.staged, (p.x - pane.min.x, p.y - pane.min.y))).unwrap_or_default();
        if under != self.views.under {
            (self.views.under, self.views.cycle) = (under, 0);
        }
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Tab)) {
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
            if self.views.under.len() > 1 {
                self.views.cycle = (self.views.cycle + 1) % self.views.under.len();
            }
        }
        let (under, cycle) = (self.views.under.clone(), self.views.cycle);
        // The plane a click at `p` takes: the one Tab has come to, where the pointer is over several; else the nearest.
        let plane_at = |drawing: &Drawing, staged: &Staged, p: egui::Pos2| -> Option<Guid> {
            let here = planes_under(drawing, staged, (p.x - pane.min.x, p.y - pane.min.y));
            if here.len() > 1 && here == under { here.get(cycle % here.len()).copied() } else { here.first().copied() }
        };
        // Where several planes lie on one another, the one a click would take, lit.
        if editable && under.len() > 1 {
            for s in plane_strokes(&c.drawing, &c.staged, under[cycle % under.len()], pane.min) {
                painter.line_segment(s, Stroke::new(2.0, PRESELECT));
            }
        }
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
                    let plane = plane_at(&cache.drawing, &cache.staged, p);
                    let line = cache.staged.line_at(which, at_view(p), reach);
                    problem = self.picked(&Arc::new(base.clone()), plane, line);
                }
                self.views.cache = Some(cache);
                self.zoom_and_pan(ui, &response, which, view.scale);
                return problem;
            }
            // Pick Lines: a reference plane's or a reference line's trace, else the nearest edge of another form's outline. With
            // Lock on (F10a), what Align would take there (a plane, a reference line, another form's line), as its lock waits.
            let locking = self.sketch.as_ref().is_some_and(|s| s.lock && s.options().offset == 0.0);
            let own = self.sketch.as_ref().map(|s| s.guid).unwrap_or_default();
            let pick = |p: egui::Pos2| -> Option<super::sketch::PickedInView> {
                let picked = plane_at(&cache.drawing, &cache.staged, p);
                let lock = |r: DimRef| FamilyDimension {
                    id: 0,
                    guid: Guid::new(),
                    view: bimcraft_core::family::DimView::of(which),
                    refs: vec![r],
                    at: cache.staged.from_view(which, at_view(p)),
                    label: None,
                    locked: vec![bimcraft_core::model::Lock { segment: 0, length: 0.0 }],
                    equal: false,
                    alignment: true,
                    kind: Default::default(),
                };
                // The planes as the type previewed puts them, as the view draws them.
                let flexed = family.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
                let trace = |g: Guid| {
                    let plane = flexed
                        .as_ref()
                        .and_then(|f| f.planes.iter().find(|q| q.guid == g))
                        .or_else(|| planes.iter().find(|q| q.guid == g))?;
                    plane_trace(&cache.staged, which, &plane.frame())
                };
                if locking && let Some(flexed) = flexed.as_ref() {
                    let screen_of = dims::screen_of(view, pane.min);
                    if let Some(r) =
                        dims::reference_at(flexed, &values, &cache.staged, which, picked, p, &screen_of).filter(|r| !r.names(own))
                        && let Some((a, b)) = dims::drawn_as(flexed, &values, &cache.staged, which, &r).filter(|(a, b)| a != b)
                    {
                        // A plane's whole trace, as Pick Lines takes it unlocked; else the line the reference is drawn as.
                        let line = match r {
                            DimRef::Plane(g) => trace(g),
                            _ => None,
                        };
                        return Some((line.unwrap_or((a, b)), Some(lock(r))));
                    }
                }
                if let Some(t) = picked.and_then(trace) {
                    return Some((t, None));
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
                best.map(|(_, s)| (s, None))
            };
            // Align on the sketch's lines (F7b), and the padlocks of its locks: clicked before the sketch takes the click.
            let frame = self.sketch_frame();
            let on_screen = |q: P2| -> Option<egui::Pos2> {
                let f = frame.as_ref()?;
                let (x, y) = view.px(super::sketch::in_view(&cache.staged, which, f, q));
                Some(pane.min + vec2(x, y))
            };
            let lines = drawn_lines(&cache.drawing, pane.min);
            let locks = self.sketch_padlocks(&on_screen, &lines);
            let free = self.sketch.as_ref().is_some_and(|s| s.tool.is_none());
            // A shape of the Draw panel picked puts Align down.
            if !free {
                self.views.aligning = None;
            }
            let aligning = self.views.aligning.is_some();
            let mut aligned = None;
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let near = locks.iter().find(|(at, _)| dims::lock_rect(*at).contains(p)).map(|(_, w)| *w);
                match near {
                    Some(what) if aligning || free => self.sketch_padlock(what),
                    _ if aligning => {
                        // What the view shows there besides the sketch: a plane, a reference line, another form's line.
                        let own = self.sketch.as_ref().map(|s| s.guid).unwrap_or_default();
                        let flexed =
                            family.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
                        let screen_of = dims::screen_of(view, pane.min);
                        let plane = plane_at(&cache.drawing, &cache.staged, p);
                        let outside = flexed.as_ref().and_then(|f| {
                            let r = dims::reference_at(f, &values, &cache.staged, which, plane, p, &screen_of).filter(|r| !r.names(own))?;
                            let (a, b) = dims::drawn_as(f, &values, &cache.staged, which, &r)?;
                            (a != b).then_some((r, (a, b)))
                        });
                        aligned = Some(self.sketch_align(&cache.staged, which, at_view(p), reach, outside));
                    }
                    _ => {}
                }
            }
            let problem = self.sketch_in_view(ui, &response, &painter, &cache.staged, which, &view, pane, &pick);
            // The reference Align picked, and the padlocks, over the lines.
            if let Some((_, (a, b), _)) = self.sketch.as_ref().and_then(|s| s.align_from.as_ref())
                && let (Some(a), Some(b)) = (on_screen(*a), on_screen(*b))
            {
                painter.line_segment([a, b], Stroke::new(2.5, PICKED));
            }
            for (at, what) in self.sketch_padlocks(&on_screen, &lines) {
                let open = what == super::sketch::Padlock::Offer;
                dims::padlock(&painter, at, !open, pal().accent);
            }
            self.views.cache = Some(cache);
            if let Some(Some(e)) = aligned {
                return Some(e);
            }
            self.zoom_and_pan(ui, &response, which, view.scale);
            return problem;
        }
        let values = self.values();
        let drawn = self.family().ok();
        // The family as the type previewed flexes it: where its dimensions put its planes and what is drawn on them.
        let family: Option<Family> =
            drawn.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
        let on_screen = dims::screen_of(view, pane.min);
        let pick_plane = |p: egui::Pos2| -> Option<Guid> { plane_at(&c.drawing, &c.staged, p) };
        let kind = self.views.dim_kind;
        let radial = kind.is_round();
        let angular = kind == bimcraft_core::family::DimKind::Angular;
        let circles = radial && self.views.dimensioning.is_some();
        let crossing = angular && self.views.dimensioning.is_some();
        let reference_at = |p: egui::Pos2| -> Option<DimRef> {
            let f = family.as_ref()?;
            if circles {
                // Radial and Diameter Dimension pick a circle or an arc by its edge.
                return dims::circle_at(f, &values, &c.staged, which, p, &on_screen).map(|c| c.r);
            }
            if crossing {
                // Angular Dimension picks lines.
                return dims::line_at(f, &values, &c.staged, which, pick_plane(p), p, &on_screen);
            }
            dims::reference_at(f, &values, &c.staged, which, pick_plane(p), p, &on_screen)
        };
        // Its dimensions, the one being placed with them, over the picture: the selected one, and those that can't hold, picked.
        let placing = match (&self.views.dimensioning, response.hover_pos(), family.as_ref()) {
            (Some(refs), Some(h), Some(f)) if radial && refs.len() == 1 => {
                Some(dims::new_dimension(f, &values, &c.staged, which, refs, at_view(h), kind))
            }
            (Some(refs), Some(h), Some(f)) if !refs.is_empty() && angular => {
                let mut refs = refs.clone();
                if let Some(r) = reference_at(h).filter(|r| !refs.contains(r) && dims::crosses_last(f, &values, which, &refs, r)) {
                    refs.push(r);
                }
                (refs.len() >= 2).then(|| dims::new_dimension(f, &values, &c.staged, which, &refs, at_view(h), kind))
            }
            (Some(refs), Some(h), Some(f)) if !refs.is_empty() && !radial => {
                let mut refs = refs.clone();
                if let Some(r) = reference_at(h).filter(|r| !refs.contains(r) && dims::parallel(f, &values, &refs, r)) {
                    refs.push(r);
                }
                (refs.len() >= 2 && dims::has_way(f, &values, &refs))
                    .then(|| dims::new_dimension(f, &values, &c.staged, which, &refs, at_view(h), kind))
            }
            _ => None,
        };
        let marks = match (drawn.as_ref(), family.as_ref()) {
            (Some(d), Some(f)) if editable => dims::marks_of(d, f, &values, &c.staged, which, placing),
            _ => Vec::new(),
        };
        // Pivots (F7d): their marks, and the selected element's grips.
        let pivots = match family.as_ref() {
            Some(f) if editable => bimcraft_core::family::constraints::pivot_marks(f, &values, &c.staged, which),
            _ => Vec::new(),
        };
        let grips = match family.as_ref() {
            Some(f) if editable => self.grips(&pivots, f, &c.staged, which, &on_screen),
            _ => Vec::new(),
        };
        let selected_dim = self.selected_dimension();
        let unsatisfied: Vec<Guid> = self.views.unsatisfied.as_ref().map(|u| u.dims.clone()).unwrap_or_default();
        // Where Align's padlocks go: beside their lines, clear of every line drawn and of each other (`padlock_spot`), the
        // one Align offers too (the last).
        let mut lines = drawn_lines(&c.drawing, pane.min);
        for m in marks.iter().filter(|m| !m.alignment) {
            if m.kind == bimcraft_core::family::DimKind::Angular {
                // Where its lines cross out to its arc, along each.
                if let Some(v) = m.points.first() {
                    lines.extend(m.points.iter().skip(1).map(|p| [on_screen(*v), on_screen(*p)]));
                }
            } else if let (Some(a), Some(b)) = (m.points.first(), m.points.last()) {
                lines.push([on_screen(*a), on_screen(*b)]);
                // Its ticks: a small cross at each point, so a padlock keeps off them too.
                for p in &m.points {
                    let c = on_screen(*p);
                    lines.push([c - egui::vec2(TICK, 0.0), c + egui::vec2(TICK, 0.0)]);
                    lines.push([c - egui::vec2(0.0, TICK), c + egui::vec2(0.0, TICK)]);
                }
            }
        }
        let offer = self.views.align_lock.filter(|(.., w)| *w == which).and_then(|(reference, target, at, _)| {
            let (d, f) = (drawn.as_ref()?, family.as_ref()?);
            let lock = FamilyDimension {
                refs: vec![reference, target],
                alignment: true,
                ..dims::new_dimension(f, &values, &c.staged, which, &[], at, Default::default())
            };
            dims::marks_of(d, f, &values, &c.staged, which, Some(lock)).pop()
        });
        let mut pending: Vec<(&dims::DimMark, Vec<DimRef>)> = marks
            .iter()
            .filter(|m| m.alignment)
            .map(|m| (m, self.dimensions.get(m.index).map(|d| d.refs.clone()).unwrap_or_default()))
            .collect();
        if let (Some(o), Some((a, b, ..))) = (offer.as_ref(), self.views.align_lock) {
            pending.push((o, vec![a, b]));
        }
        let mut locks: Vec<(usize, egui::Pos2)> = Vec::new();
        for (m, refs) in pending {
            let (Some(base), Some(f)) = (m.points.first(), family.as_ref()) else { continue };
            let base_at = on_screen(*base);
            let dir = on_screen(*base + m.across) - base_at;
            // A form one of its references belongs to: its outline is the inside the padlock keeps out of. Planes and lines
            // alone: every solid form's, so a plane's padlock isn't left inside a form it runs through.
            let ring = |i: usize| -> Vec<egui::Pos2> { fv::outline(f, &values, &c.staged, which, i).into_iter().map(&on_screen).collect() };
            let own = refs.iter().find_map(|r| match r {
                DimRef::Form(g, _) | DimRef::Sketch(g, _) | DimRef::Point(g, ..) => f.forms.iter().position(|x| x.guid == *g),
                _ => None,
            });
            let rings: Vec<Vec<egui::Pos2>> = match own {
                Some(i) => vec![ring(i)],
                None => (0..f.forms.len()).filter(|i| !f.forms[*i].void).map(ring).collect(),
            };
            let taken: Vec<egui::Pos2> = locks.iter().map(|(_, p)| *p).collect();
            let inside = |p: egui::Pos2| rings.iter().any(|r| r.len() > 2 && dims::within(r, p));
            let at = dims::padlock_spot(base_at, dir, &inside, &lines, &taken);
            locks.push((m.index, at));
        }
        let offered = offer.as_ref().and_then(|o| locks.iter().find(|(i, _)| *i == o.index).map(|(_, p)| *p));
        self.views.locks_shown = locks.clone();
        self.views.offer_shown = offered;
        for m in &marks {
            let g = self.dimensions.get(m.index).map(|d| d.guid);
            let picked = g.is_some() && (g == selected_dim || g.is_some_and(|g| unsatisfied.contains(&g)));
            let lock = locks.iter().find(|(i, _)| *i == m.index).map(|(_, p)| *p);
            dims::paint(&painter, m, &on_screen, picked, base.units, lock);
        }
        // The references Aligned Dimension and Align have picked.
        let picked_refs: Vec<DimRef> = match (&self.views.dimensioning, &self.views.aligning) {
            (Some(refs), _) => refs.clone(),
            (_, Some(Some(r))) => vec![*r],
            _ => Vec::new(),
        };
        if let Some(f) = family.as_ref() {
            for r in &picked_refs {
                // A plane by the strokes its trace is drawn with, short of its name.
                let strokes = match r {
                    DimRef::Plane(g) => plane_strokes(&c.drawing, &c.staged, *g, pane.min),
                    _ => Vec::new(),
                };
                if !strokes.is_empty() {
                    for s in strokes {
                        painter.line_segment(s, Stroke::new(2.5, PICKED));
                    }
                    continue;
                }
                match dims::drawn_as(f, &values, &c.staged, which, r) {
                    // A point (a circle's centre): a ring round it.
                    Some((a, b)) if a == b => {
                        painter.circle_stroke(screen(a), 5.0, Stroke::new(2.0, PICKED));
                    }
                    Some((a, b)) => {
                        painter.line_segment([screen(a), screen(b)], Stroke::new(2.5, PICKED));
                    }
                    None => {}
                }
                // A circle or an arc picked by Radial or Diameter Dimension: the circle or the arc itself.
                if radial
                    && let Some(round) =
                        bimcraft_core::family::constraints::sketch_circles(f, &values, &c.staged, which).into_iter().find(|x| x.r == *r)
                {
                    let (centre, radius) = (round.centre, round.radius);
                    match round.arc {
                        None => {
                            let rim = screen(P2(centre.0 + radius, centre.1));
                            painter.circle_stroke(screen(centre), (rim - screen(centre)).length(), Stroke::new(2.5, PICKED));
                        }
                        Some([a, m, b]) => {
                            let angle = |q: P2| (q.1 - centre.1).atan2(q.0 - centre.0);
                            let tau = std::f64::consts::TAU;
                            let (to_m, to_b) = ((angle(m) - angle(a)).rem_euclid(tau), (angle(b) - angle(a)).rem_euclid(tau));
                            let sweep = if to_m <= to_b { to_b } else { to_b - tau };
                            let pts: Vec<egui::Pos2> = (0..=48)
                                .map(|k| {
                                    let t = angle(a) + sweep * f64::from(k) / 48.0;
                                    screen(P2(centre.0 + radius * t.cos(), centre.1 + radius * t.sin()))
                                })
                                .collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(2.5, PICKED)));
                        }
                    }
                }
            }
        }
        // Align's padlock, open, until the next click.
        let align_lock = offered;
        if let Some(at) = align_lock {
            dims::padlock(&painter, at, false, pal().accent);
        }
        // The connectors (#269), at their places in the family's axes as the view sees them; the one under the pointer lit, as a
        // click would pick it.
        let connector_marks = if editable { self.connector_marks() } else { Vec::new() };
        let project = |q: [f64; 3]| Some(screen(c.staged.to_view(which, q)));
        self.connector_hover = response.hover_pos().and_then(|p| super::connector_marks::pick(&connector_marks, &project, p));
        let lit = (PICKED, crate::style::canvas_pal().hover);
        super::connector_marks::paint(&painter, &connector_marks, &project, self.selected_connector(), self.connector_hover, lit);
        // The insertion point, where the planes that define the origin cross (O1).
        if let Some(at) = self.origin_mark() {
            super::origin_mark::paint(&painter, &project, at);
        }
        // What is selected, over the picture.
        if editable && let (Some(i), Some(f)) = (self.selected_form(), family.as_ref()) {
            let ring: Vec<egui::Pos2> = fv::outline(f, &values, &c.staged, which, i).into_iter().map(screen).collect();
            if ring.len() > 2 {
                painter.add(egui::Shape::closed_line(ring, Stroke::new(2.0, PICKED)));
            }
        }
        if editable && let (Some(i), Some(f)) = (self.selected_nested(), family.as_ref()) {
            let ring: Vec<egui::Pos2> = fv::nested_outline(f, &values, &c.staged, which, i).into_iter().map(screen).collect();
            if ring.len() > 2 {
                painter.add(egui::Shape::closed_line(ring, Stroke::new(2.0, PICKED)));
            }
        }
        let trace = |g: Guid| -> Option<(egui::Pos2, egui::Pos2)> {
            let p =
                family.as_ref().and_then(|f| f.planes.iter().find(|p| p.guid == g)).or_else(|| self.planes.iter().find(|p| p.guid == g))?;
            plane_trace(&c.staged, which, &p.frame()).map(|(a, b)| (screen(a), screen(b)))
        };
        if editable && let Some(g) = self.selected_plane() {
            let strokes = plane_strokes(&c.drawing, &c.staged, g, pane.min);
            if strokes.is_empty() {
                if let Some((a, b)) = trace(g) {
                    painter.line_segment([a, b], Stroke::new(2.5, PICKED));
                }
            } else {
                for s in strokes {
                    painter.line_segment(s, Stroke::new(2.5, PICKED));
                }
            }
        }
        // The reference line selected, its pivots' marks and its grips (F7d).
        if editable
            && let (Some(g), Some(f)) = (self.selected_line(), family.as_ref())
            && let Some(l) = f.lines.iter().find(|l| l.guid == g)
        {
            let (a, b) = (c.staged.to_view(which, l.start), c.staged.to_view(which, l.end));
            painter.line_segment([screen(a), screen(b)], Stroke::new(2.5, PICKED));
        }
        let chosen = self.selected_line().or_else(|| self.selected_plane());
        let turning = match self.views.grip {
            Some((super::pivot::Grip::Rotate(g), _)) => Some(g),
            _ => None,
        };
        super::pivot::paint(&painter, &pivots, &on_screen, chosen, turning);
        let ends: Vec<(super::pivot::Grip, egui::Pos2)> =
            grips.iter().filter(|(g, _)| matches!(g, super::pivot::Grip::End(..))).copied().collect();
        super::pivot::paint_ends(&painter, &ends);
        self.views.grips_shown = grips.clone();
        // Pivot, on: the point the click would pin it to, ringed.
        if self.views.pivoting.is_some()
            && let (Some(h), Some(f)) = (response.hover_pos(), family.as_ref())
        {
            let targets = bimcraft_core::family::constraints::pivot_targets(f, &values, &c.staged, which);
            let (_, p) = super::pivot::target(&c.staged, which, &targets, at_view(h), &on_screen);
            painter.circle_stroke(screen(c.staged.to_view(which, p)), 7.0, Stroke::new(2.0, PICKED));
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
        let mut select_line: Option<Option<Guid>> = None;
        let mut pin: Option<(Guid, (bimcraft_core::family::PivotPoint, bimcraft_core::family::PivotAt))> = None;
        let mut grip_moved: Option<super::pivot::GripMove> = None;
        // A grip pressed (F7d): the rotate grip, or a line's end; the drag is the grip's.
        if editable
            && response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some((grip, _)) = grips.iter().find(|(_, at)| at.distance(p) <= super::pivot::REACH)
        {
            self.views.grip = Some((*grip, at_view(p)));
        }
        if let Some((grip, last)) = self.views.grip
            && response.dragged_by(PointerButton::Primary)
            && let Some(now) = response.interact_pointer_pos().map(at_view)
            && now != last
        {
            grip_moved = Some(match grip {
                super::pivot::Grip::Rotate(g) => super::pivot::GripMove::Rotate(g, c.staged.from_view(which, now)),
                super::pivot::Grip::End(g, spot) => {
                    super::pivot::GripMove::End(g, spot, c.staged.vector(which, P2(now.0 - last.0, now.1 - last.1)))
                }
            });
            self.views.grip = Some((grip, now));
        }
        let mut select_nested: Option<usize> = None;
        let mut select_connector: Option<usize> = None;
        let mut place_at: Option<(PlaneRef, (f64, f64), f64)> = None;
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
            } else if let Some(g) = self.views.pivoting {
                // Pivot: the point the element is pinned to.
                self.views.pivoting = None;
                pin = family
                    .as_ref()
                    .and_then(|f| super::pivot::picked(f, &values, &c.staged, which, g, q, &on_screen, view.scale))
                    .map(|x| (g, x));
            } else if let Some(first) = self.views.drawing {
                match first {
                    None => self.views.drawing = Some(Some(q)),
                    Some(a) => {
                        new_plane = fv::new_plane(&c.staged, which, a, q);
                        self.views.drawing = Some(None);
                    }
                }
            } else if self.placing.is_some() {
                // Component (T6a): an instance on the view's work plane, where the click is on it.
                let host = self.views.work_plane();
                if let Some(f) = family.as_ref()
                    && let Some((at, offset)) = bimcraft_core::family::nested::on_plane(f, &host, c.staged.from_view(which, q))
                {
                    place_at = Some((host, (at.0, at.1), offset));
                }
            } else if align_lock.is_some_and(|at| dims::lock_rect(at).contains(p)) {
                // Align's padlock, waiting: it locks what was aligned, whatever tool is on.
                on_dims = Some((Clicked::AlignLock, Vec::new()));
            } else if let Some(refs) = self.views.dimensioning.as_mut().filter(|_| radial) {
                // Radial or Diameter Dimension: a circle, then a click where it goes.
                match (refs.first(), reference_at(p), family.as_ref()) {
                    (None, Some(r), _) => refs.push(r),
                    (Some(_), _, Some(f)) => {
                        new_dim = Some(dims::new_dimension(f, &values, &c.staged, which, refs, q, kind));
                        refs.clear();
                    }
                    _ => problem = Some(t("Pick a circle or an arc, then click where the dimension goes.").to_string()),
                }
            } else if let Some(refs) = self.views.dimensioning.as_mut().filter(|_| angular) {
                // Angular Dimension: lines one after another, each crossing the one before; a click on nothing places it.
                match (reference_at(p), family.as_ref()) {
                    (Some(r), Some(f)) => {
                        if !dims::crosses_last(f, &values, which, refs, &r) {
                            problem = Some(t("Pick a line that crosses the one before.").to_string());
                        } else if !refs.contains(&r) {
                            refs.push(r);
                        }
                    }
                    (None, Some(f)) if refs.len() >= 2 => {
                        new_dim = Some(dims::new_dimension(f, &values, &c.staged, which, refs, q, kind));
                        refs.clear();
                    }
                    _ => problem = Some(t("Pick lines that cross, then click where the dimension goes.").to_string()),
                }
            } else if let Some(refs) = self.views.dimensioning.as_mut() {
                // Aligned Dimension: references one after another, parallel to the first; a click on nothing places it.
                match (reference_at(p), family.as_ref()) {
                    (Some(r), Some(f)) => {
                        if !dims::parallel(f, &values, refs, &r) {
                            problem = Some(t("Pick a reference parallel to the first.").to_string());
                        } else if !refs.contains(&r) {
                            refs.push(r);
                        }
                    }
                    (None, Some(f)) if refs.len() >= 2 && dims::has_way(f, &values, refs) => {
                        new_dim = Some(dims::new_dimension(f, &values, &c.staged, which, refs, q, kind));
                        refs.clear();
                    }
                    (None, Some(_)) if refs.len() >= 2 => {
                        problem = Some(t("Pick a plane or a line too: points alone face no way to measure along.").to_string())
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
                    (Some(_), Some(_)) => {
                        problem = Some(t("That is the reference: pick what moves onto it.").to_string());
                    }
                    (None, _) => {
                        // Over a form but on none of its faces: say what of a form Align takes.
                        let form = family.as_ref().and_then(|f| fv::form_at(f, &values, &c.staged, which, q).and_then(|i| f.forms.get(i)));
                        problem = Some(match form {
                            Some(f) if !f.mesh.is_empty() => {
                                t("A form made of a mesh has no faces Align can take: pick a reference plane, an extrusion's end or a line of a form.")
                            }
                            Some(_) => t("Align takes a form's straight edge, a side the view sees edge on, or its end: click on its outline."),
                            None => t("Pick a reference plane, an extrusion's end or a line of a form."),
                        }
                        .to_string());
                    }
                }
            } else if let Some(hit) = dims::clicked(&marks, &self.dimensions, selected_dim, align_lock, &locks, p, &on_screen) {
                let lengths = match &hit {
                    Clicked::Lock(g, _) => marks
                        .iter()
                        .find(|m| self.dimensions.get(m.index).is_some_and(|d| d.guid == *g))
                        .map(|m| m.lengths.clone())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                on_dims = Some((hit, lengths));
            } else if let Some(i) = super::connector_marks::pick(&connector_marks, &project, p) {
                select_connector = Some(i);
            } else if let Some(i) = family.as_ref().and_then(|f| fv::nested_at(f, &values, &c.staged, which, q)) {
                select_nested = Some(i);
            } else {
                // A reference line where the view draws it (F7d): over the planes' traces and the forms, as it is drawn.
                let line = c
                    .staged
                    .line_at(which, q, 6.0 / view.scale.max(1e-9))
                    .and_then(|i| family.as_ref().and_then(|f| f.lines.get(i)).map(|l| l.guid));
                let plane = if line.is_some() { None } else { pick_plane(p) };
                let form = family.as_ref().and_then(|f| fv::form_at(f, &values, &c.staged, which, q));
                select = Some(match (plane, line, form) {
                    (Some(g), _, _) => (None, Some(g)),
                    (None, Some(_), _) => (None, None),
                    (None, None, Some(i)) => (Some(i), None),
                    (None, None, None) => (None, None),
                });
                select_line = Some(line);
            }
        }
        // A plane of the family's own, selected, dragged square to itself.
        if editable
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.end_drag.is_none()
            && self.views.grip.is_none()
            && response.dragged_by(PointerButton::Primary)
        {
            let now = response.interact_pointer_pos().map(at_view);
            // From where the button went down: the drag begins only once the hand has moved on.
            if response.drag_started()
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                // A plane its pivot holds turns by its grip, never slides (F7d).
                let own = self.selected_plane().filter(|g| self.planes.iter().any(|q| q.guid == *g && !q.datum() && q.pivot.is_none()));
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
            self.views.grip = None;
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
        if let Some((host, at, offset)) = place_at {
            self.place_nested(host, at, offset);
        }
        if let Some(i) = select_connector {
            self.select_connector(i);
        }
        if let Some(i) = select_nested {
            self.select_nested(Some(i));
        }
        if let Some(m) = grip_moved
            && let Some(e) = self.drag_grip(m)
        {
            problem = Some(e);
        }
        if let Some((g, (to, at))) = pin
            && let Err(e) = self.set_pivot(g, to, at)
        {
            problem = Some(e);
        }
        if let Some((form, plane)) = select {
            self.nested_sel = None;
            self.symbol = None;
            self.connector = None;
            self.views.dim = None;
            self.form = form;
            self.views.plane = plane;
            self.views.line = select_line.flatten();
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
                pivot: None,
                turned: None,
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

/// Work Plane > Show in the family's {3D}: plane `f` as a mark over its picture, as the project's 3D view lays out its work plane
/// (`workplane::patch`) over what the family takes up (`extents`) and [`WORK_PLANE_MARGIN`] round, tinted as the plans tint it,
/// with a grid through the family's origin about 5 to 20 lines across (`workplane::grid_step`) and its outline; depth tested,
/// so a form in front of it hides it.
pub(crate) fn work_plane_mark(f: &bimcraft_core::family::Frame, extents: ([f64; 3], [f64; 3])) -> Option<bimcraft_core::view3d::PlaneMark> {
    let plane = bimcraft_core::workplane::Frame { origin: f.origin, normal: f.z };
    let p = crate::workplane::patch(plane, extents, WORK_PLANE_MARGIN, [0.0; 3])?;
    let step = bimcraft_core::workplane::grid_step((p.hi.0 - p.lo.0).max(p.hi.1 - p.lo.1));
    Some(crate::workplane::plane_mark(p, step))
}

/// The solid forms the type previewed (`values`) hides, and those the view's Visibility Settings leave out (`by_view`, F8),
/// alone: what a view draws halftone while Preview Visibility is off. Every other form, and each `hidden` for now, is left
/// out. A door's or a window's is a generic family of its own, its wall's thickness a parameter, so it stands as a component
/// beside the opening rather than a second opening in the wall. None when nothing is hidden.
fn ghosts(
    f: &Family,
    values: &BTreeMap<String, bimcraft_core::family::Val>,
    hidden: impl Fn(usize) -> bool,
    by_view: &[bool],
) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for (i, form) in g.forms.iter_mut().enumerate() {
        let in_type = form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0));
        let off = !form.void && !hidden(i) && (in_type || by_view.get(i).copied().unwrap_or(false));
        form.visible = if off { None } else { Some(Expr::Number(0.0)) };
        form.visibility = None;
        any |= off;
    }
    if !any {
        return None;
    }
    if g.category.hosted() {
        let wall = match values.get(bimcraft_core::family::WALL) {
            Some(bimcraft_core::family::Val::Number(t)) => *t,
            _ => bimcraft_core::family::DEFAULT_WALL,
        };
        let p = serde_json::from_value(json!({"name": bimcraft_core::family::WALL, "value": wall})).ok()?;
        g.parameters.push(p);
        g.category = Category::Generic;
        g.operation = None;
    }
    // Its symbols and nested instances are the family's, drawn as it draws them.
    g.symbols.clear();
    g.nested_instances.clear();
    Some(g)
}

/// What the ghost family is called beside the family in a view's scratch model.
const GHOST: &str = "(hidden in type)";

/// The lines of drawing `d` (shown from `min` on screen) as segments on screen: what a padlock keeps off.
pub(super) fn drawn_lines(d: &Drawing, min: egui::Pos2) -> Vec<[egui::Pos2; 2]> {
    let at = |p: &(f32, f32)| min + vec2(p.0, p.1);
    let mut out = Vec::new();
    for prim in &d.prims {
        if let bimcraft_core::render::Prim::Line { pts, closed, .. } = prim {
            out.extend(pts.windows(2).map(|w| [at(&w[0]), at(&w[1])]));
            if *closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
                out.push([at(a), at(b)]);
            }
        }
    }
    out
}

/// The strokes the view's drawing draws plane `g`'s trace with, on screen: what highlights it, short of its name as the trace
/// is (none where the drawing hasn't it).
pub(super) fn plane_strokes(d: &Drawing, st: &Staged, g: Guid, min: egui::Pos2) -> Vec<[egui::Pos2; 2]> {
    let at = |p: &(f32, f32)| min + vec2(p.0, p.1);
    let mut out = Vec::new();
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        if owner.and_then(|o| st.plane_of(o)) == Some(g)
            && let bimcraft_core::render::Prim::Line { pts, .. } = prim
        {
            out.extend(pts.windows(2).map(|w| [at(&w[0]), at(&w[1])]));
        }
    }
    out
}

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
