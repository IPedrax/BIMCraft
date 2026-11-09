//! The family document's {3D}, drawn by the project's 3D view (`orbit.rs`): the family staged in a scratch model, each of its
//! parts a family of its own, so the project's tints colour each and its pick finds each. Its picture is the card's or the
//! processor's, as the project's is, and the hand, the wheel, the 3D mouse, the triad and the ViewCube move its camera.
//!
//! What it highlights (T11), as the project's 3D view does: the form, void or nested instance selected, in the selection's
//! colour, and the form or nested instance under the pointer, which a click would select, in the hover colour, each tinted
//! halfway with its edges whole while what stands in front of it still hides it. A void, never seen, is drawn see-through
//! while it is selected. A reference plane or line selected in a plan or an elevation is laid over the picture in the
//! selection's colour, depth tested as the work plane is (`view3d::PlaneMark`).
use super::*;
use bimcraft_core::family::views::Part;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Element, Id, View3D};
use bimcraft_core::render::Rgb;
use bimcraft_core::view3d::{Camera, PlaneMark};

/// What {3D} draws highlighted: a part of the family (`Part`), a reference plane or a reference line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Lit {
    Part(Part),
    Plane(Guid),
    Line(Guid),
}

/// The material a selected void is drawn in: see-through, the selection's tint over it. The scratch project's own.
const VOID: &str = "(selected void)";

/// The family's {3D} staged: the scratch model its scene is made of, each part of the family placed in it as a family of its
/// own, and which part each element is; built again only when what it shows changes (`key`).
pub(crate) struct FamilyScene {
    pub(crate) model: Arc<Model>,
    pub(crate) part: BTreeMap<Id, Part>,
    key: u64,
}

impl FamilyScene {
    /// The tints of what is `lit`: each part's elements in its colour.
    pub(crate) fn tints(&self, lit: &[(Lit, Rgb)]) -> Vec<(Id, Rgb)> {
        let mut out = Vec::new();
        for (l, c) in lit {
            if let Lit::Part(p) = l {
                out.extend(self.part.iter().filter(|(_, q)| *q == p).map(|(id, _)| (*id, *c)));
            }
        }
        out
    }
}

impl FamilyEditor {
    /// What is selected, as {3D} shows it: a form, a nested instance, a reference plane or a reference line.
    pub(crate) fn selected_3d(&self) -> Option<Lit> {
        if self.adaptive.is_some() {
            return None;
        }
        let part = self.selected_form().map(Part::Form).or_else(|| self.selected_nested().map(Part::Nested));
        part.map(Lit::Part).or_else(|| self.selected_line().map(Lit::Line)).or_else(|| self.selected_plane().map(Lit::Plane))
    }

    /// What {3D} highlights, and in which colour: what is selected, in the selection's, and the part under the pointer (when
    /// it isn't the one selected), in the hover colour, as the project's 3D view tints its elements. None while sketching.
    pub(crate) fn lit_3d(&self) -> Vec<(Lit, Rgb)> {
        if self.adaptive.is_some() || self.sketching() {
            return Vec::new();
        }
        let selected = self.selected_3d();
        let hovered = self.views.hover3d.and_then(|(_, _, p)| p).map(Lit::Part).filter(|h| Some(*h) != selected);
        let colour = |c: Color32| Rgb(c.r(), c.g(), c.b());
        selected
            .map(|s| (s, colour(crate::style::canvas_pal().accent)))
            .into_iter()
            .chain(hovered.map(|h| (h, colour(crate::style::canvas_pal().hover))))
            .collect()
    }

    /// A click in {3D} on `part` (none: on nothing): the form or nested instance selected, as a click selects it in a plan; on
    /// nothing, the selection let go.
    pub(crate) fn click_3d(&mut self, part: Option<Part>) {
        self.views.dim = None;
        self.views.plane = None;
        self.views.line = None;
        match part {
            Some(Part::Form(i)) => {
                self.select_only_form(i);
            }
            Some(Part::Nested(i)) => self.select_nested(Some(i)),
            None => self.let_everything_go(),
        }
    }

    /// The family's {3D} staged anew where what it shows has changed: the draft, the type and its values, Preview Visibility,
    /// the Detail Level, Temporary Hide/Isolate, or a void selected. Why it can't be, in `problem`.
    pub(crate) fn stage_3d(&mut self, base: &Model) {
        let placed_only = self.document.preview_visibility;
        let void = self.selected_form().filter(|i| self.forms.get(*i).is_some_and(|f| f.void));
        let result = self.family().and_then(|f| {
            let key = {
                let mut h = DefaultHasher::new();
                (
                    serde_json::to_string(&f).unwrap_or_default(),
                    serde_json::to_string(&self.preview_params).unwrap_or_default(),
                    self.picked,
                    self.name.trim(),
                    placed_only,
                    self.temporary(),
                    self.views.detail,
                    void,
                )
                    .hash(&mut h);
                (base.materials.len(), base.images.len()).hash(&mut h);
                h.finish()
            };
            if self.scene3d.as_ref().is_some_and(|s| s.key == key) {
                return Ok(None);
            }
            self.staged_3d(base, &f, void, key).map(Some)
        });
        match result {
            Ok(Some(scene)) => (self.scene3d, self.problem) = (Some(scene), None),
            Ok(None) => self.problem = None,
            Err(e) => self.problem = Some(e),
        }
    }

    /// Family `f` staged for {3D}, `void` the void selected: a form the type hides (or its Visibility Settings leave out of {3D}
    /// at the document's Detail Level) drawn ghosted, as Revit's family editor draws it, unless Preview Visibility is on, and
    /// what Temporary Hide/Isolate hides left out. The family is placed once with its solids hidden (a door's or a window's
    /// opening still cuts its wall), and each solid form and nested instance beside it as a family of its own, where the family
    /// is (a door's or a window's at its opening's frame, a generic family with its wall's thickness, as the views' ghosts are).
    fn staged_3d(&self, base: &Model, f: &Family, void: Option<usize>, key: u64) -> Result<FamilyScene, String> {
        let values = self.values();
        let by_view = bimcraft_core::family::views::hidden_in(f, &values, None, self.views.detail, 0.0);
        let shown = if self.document.preview_visibility { f.clone() } else { ghosted(f, &values, &by_view).unwrap_or_else(|| f.clone()) };
        let shown = temp_hide::without(&shown, |i| self.temp_hidden_form(i)).unwrap_or(shown);
        let off = || Some(Expr::Number(0.0));
        let hides = |e: &Option<Expr>| e.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, &values) == Some(0.0));
        let mut host = shown.clone();
        for form in host.forms.iter_mut().filter(|form| !form.void) {
            form.visible = off();
        }
        for n in &mut host.nested_instances {
            n.visible = off();
        }
        let mut s = Session::new();
        s.model = Arc::new(self.scratch(base, &host)?);
        let name = self.name.trim();
        let type_name = self.types.get(self.picked).map(|(n, _)| n.clone()).unwrap_or_default();
        let mut place = json!({"type": type_name, "at": [0, 0], "params": self.preview_params});
        let hosted = shown.category.hosted();
        if hosted {
            let frame = s.model.elements.values().find_map(|e| match e {
                Element::Door(o) | Element::Window(o) => s.model.opening_frame(o),
                _ => None,
            });
            if let Some((p, _, z)) = frame {
                (place["at"], place["offset"], place["params"]) = (json!([p.0, p.1]), json!(z), json!({}));
            }
        }
        let parts = (0..shown.forms.len())
            .filter(|i| shown.forms.get(*i).is_some_and(|form| (!form.void || void == Some(*i)) && !hides(&form.visible)))
            .map(Part::Form)
            .chain((0..shown.nested_instances.len()).filter(|k| !hides(&shown.nested_instances[*k].visible)).map(Part::Nested));
        let mut part = BTreeMap::new();
        s.model_mut()
            .materials
            .insert(VOID.into(), bimcraft_core::model::Material { color: [40, 120, 230], transparency: 55, ..Default::default() });
        for (k, p) in parts.enumerate() {
            let mut g = alone(&shown, p);
            if hosted {
                generic(&mut g, &values);
            }
            let own = format!("{name} (part {})", k + 1);
            s.model_mut().families.insert(own.clone(), g);
            let mut at = place.clone();
            at["family"] = json!(own);
            let made = s.execute("component", &at)?;
            if let Some(id) = made["id"].as_u64() {
                part.insert(id, p);
            }
        }
        let m = s.model_mut();
        // The view it is seen in: the document's Detail Level, the scratch project's levels hidden, and the door's or the window's
        // wall while Temporary Hide/Isolate hides it.
        let mut graphics = bimcraft_core::graphics::Graphics { detail: self.views.detail, ..Default::default() };
        let hidden = bimcraft_core::graphics::Override { hidden: true, ..Default::default() };
        graphics.categories.insert("level".into(), hidden.clone());
        if self.temp_hidden_host() {
            graphics.categories.insert("wall".into(), hidden);
        }
        m.insert(Element::View3D(View3D {
            name: "{3D}".into(),
            azimuth: AZIMUTH,
            elevation: ELEVATION,
            graphics,
            section_box: None,
            perspective: None,
            camera: None,
            display: Default::default(),
            work_plane: None,
        }));
        Ok(FamilyScene { model: Arc::clone(&s.model), part, key })
    }

    /// What {3D} lays over its picture through `cam`: Work Plane > Show's plane, and the reference plane or line selected.
    pub(crate) fn marks_3d(&self, cam: &Camera) -> Vec<PlaneMark> {
        let Ok(f) = self.family() else { return Vec::new() };
        let values = self.values();
        let extents = bimcraft_core::family::views::extents(&f, &values);
        let mut marks: Vec<PlaneMark> =
            self.shown_work_plane_3d().and_then(|shown| super::views::work_plane_mark(&shown, extents)).into_iter().collect();
        if let Some((datum, c)) = self.lit_3d().into_iter().find(|(l, _)| !matches!(l, Lit::Part(_))) {
            let flexed = f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone());
            marks.extend(datum_mark(cam, &flexed, datum, Color32::from_rgb(c.0, c.1, c.2), extents));
        }
        marks
    }
}

/// Family `f` with only part `p` of it seen: a form alone with the voids that cut it (a void, made a see-through solid), a
/// nested instance alone.
fn alone(f: &Family, p: Part) -> Family {
    let off = || Some(Expr::Number(0.0));
    let mut g = f.clone();
    for (k, form) in g.forms.iter_mut().enumerate() {
        if p == Part::Form(k) {
            if form.void {
                form.void = false;
                form.material = VOID.into();
            }
        } else if !form.void {
            form.visible = off();
        }
    }
    for (k, n) in g.nested_instances.iter_mut().enumerate() {
        if p != Part::Nested(k) {
            n.visible = off();
        }
    }
    // Its symbols are drawn in plans only.
    g.symbols.clear();
    g
}

/// A door's or a window's part made a generic family, its wall's thickness a parameter, so it stands as a component at the
/// opening's frame beside the opening rather than a second opening in the wall.
fn generic(g: &mut Family, values: &BTreeMap<String, bimcraft_core::family::Val>) {
    let wall = match values.get(bimcraft_core::family::WALL) {
        Some(bimcraft_core::family::Val::Number(t)) => *t,
        _ => bimcraft_core::family::DEFAULT_WALL,
    };
    if !g.parameters.iter().any(|p| p.name == bimcraft_core::family::WALL)
        && let Ok(p) = serde_json::from_value(json!({"name": bimcraft_core::family::WALL, "value": wall}))
    {
        g.parameters.push(p);
    }
    g.category = Category::Generic;
    g.operation = None;
}

/// The reference plane or line `lit` names as a mark over {3D}'s picture, seen through `cam`, in colour `c`: the plane as much
/// of it as the family takes up (`extents`) and a margin round, as Work Plane > Show lays one; the line a strip a few pixels
/// wide, turned to the eye. `f` is the family as flexed, with its planes and lines where the type puts them.
fn datum_mark(cam: &Camera, f: &Family, lit: Lit, c: Color32, extents: ([f64; 3], [f64; 3])) -> Option<PlaneMark> {
    match lit {
        Lit::Plane(g) => {
            let frame = f.frame(&PlaneRef::Plane(g)).ok()?;
            let plane = bimcraft_core::workplane::Frame { origin: frame.origin, normal: frame.z };
            let p = crate::workplane::patch(plane, extents, super::views::WORK_PLANE_MARGIN, [0.0; 3])?;
            Some(PlaneMark {
                origin: p.origin,
                axes: p.axes,
                lo: p.lo,
                hi: p.hi,
                step: 0.0,
                fill: c.gamma_multiply(0.25).to_array(),
                grid: [0; 4],
                edge: c.to_array(),
                edge_width: 2.0,
            })
        }
        Lit::Line(g) => {
            let l = f.lines.iter().find(|l| l.guid == g)?;
            let d: [f64; 3] = std::array::from_fn(|i| l.end[i] - l.start[i]);
            let long = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if !long.is_finite() || long < 1e-9 {
                return None;
            }
            let u = d.map(|v| v / long);
            // Across the line and square to the eye, as wide as 3 pixels.
            let (a, e) = (cam.azimuth.to_radians(), cam.elevation.to_radians());
            let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
            let across = [u[1] * back[2] - u[2] * back[1], u[2] * back[0] - u[0] * back[2], u[0] * back[1] - u[1] * back[0]];
            let n = (across[0] * across[0] + across[1] * across[1] + across[2] * across[2]).sqrt();
            if !n.is_finite() || n < 1e-9 {
                return None;
            }
            let half = 1.5 / cam.scale.max(1e-9);
            Some(PlaneMark {
                origin: l.start,
                axes: (u, across.map(|v| v / n)),
                lo: (0.0, -half),
                hi: (long, half),
                step: 0.0,
                fill: c.to_array(),
                grid: [0; 4],
                edge: c.to_array(),
                edge_width: 0.0,
            })
        }
        Lit::Part(_) => None,
    }
}
