//! What the family document's {3D} highlights (T11), as the project's 3D view does (`orbit.rs`): the form, void or nested
//! instance selected, in the selection's colour, and the form or nested instance under the pointer, which a click would
//! select, in the hover colour. Each is drawn through the project's own tints (`Scene::render_in`): the part is a family of its
//! own in the scratch model, placed where the family is, the rest of the family without it, so it is tinted halfway with its
//! edges whole while what stands in front of it still hides it. A void, never seen, is drawn see-through while it is
//! selected. A reference plane or line selected in a plan or an elevation is drawn into the picture in the selection's
//! colour, depth tested as the work plane is (`view3d::PlaneMark`, `Image::draw_line`).
use super::*;
use bimcraft_core::family::views::Part;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Element, Id};
use bimcraft_core::render::Rgb;
use bimcraft_core::view3d::{Camera, Image, PlaneMark};

/// What {3D} draws highlighted: a part of the family (`Part`), a reference plane or a reference line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Lit {
    Part(Part),
    Plane(Guid),
    Line(Guid),
}

/// The material a selected void is drawn in: see-through, the selection's tint over it. The scratch project's own.
const VOID: &str = "(selected void)";

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

    /// A click in {3D} at `px` of its picture, taken through `cam`: the form or nested instance there selected, as a click
    /// selects it in a plan; on nothing, the selection let go.
    pub(crate) fn click_3d(&mut self, cam: &Camera, px: (f64, f64)) {
        let part = self.family().ok().and_then(|f| bimcraft_core::family::views::part_at_3d(&f, &self.values(), cam, px));
        self.views.dim = None;
        self.views.plane = None;
        self.views.line = None;
        match part {
            Some(Part::Form(i)) => {
                self.nested_sel = None;
                self.select_form_at(i);
                self.tab = Tab::Forms;
                self.document.reveal = Some(i);
            }
            Some(Part::Nested(i)) => self.select_nested(Some(i)),
            None => {
                self.nested_sel = None;
                self.form = None;
            }
        }
    }

    /// The pointer at `at` on screen, `px` of {3D}'s picture taken through `cam` (none: off the view, or the view is being
    /// turned): the part under it, found again only when it has moved.
    pub(crate) fn hover_3d(&mut self, at: Option<(egui::Pos2, (f64, f64))>, cam: &Camera) {
        let Some((at, px)) = at else {
            self.views.hover3d = None;
            return;
        };
        if self.views.hover3d.is_some_and(|(was, through, _)| was == at && through == *cam) {
            return;
        }
        let part = self.family().ok().and_then(|f| bimcraft_core::family::views::part_at_3d(&f, &self.values(), cam, px));
        self.views.hover3d = Some((at, *cam, part));
    }
}

/// Family `f` as {3D} draws it with `lit` highlighted: without the parts lit (a void stays, to cut as it does), and each part
/// lit alone, its tint beside it: a form alone with the voids that cut it (a void lit, made a see-through solid), a nested
/// instance alone.
pub(super) fn split(f: &Family, lit: &[(Lit, Rgb)]) -> (Family, Vec<(Family, Rgb)>) {
    let parts: Vec<(Part, Rgb)> = lit.iter().filter_map(|(l, c)| if let Lit::Part(p) = l { Some((*p, *c)) } else { None }).collect();
    let off = || Some(Expr::Number(0.0));
    let mut rest = f.clone();
    for (p, _) in &parts {
        match *p {
            Part::Form(i) => {
                if let Some(form) = rest.forms.get_mut(i).filter(|form| !form.void) {
                    form.visible = off();
                }
            }
            Part::Nested(i) => {
                if let Some(n) = rest.nested_instances.get_mut(i) {
                    n.visible = off();
                }
            }
        }
    }
    let alone = |p: Part| {
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
        g
    };
    let lit = parts
        .into_iter()
        .filter(|(p, _)| matches!(p, Part::Nested(i) if *i < f.nested_instances.len()) || matches!(p, Part::Form(i) if *i < f.forms.len()));
    (rest, lit.map(|(p, c)| (alone(p), c)).collect())
}

/// Scratch model `m`, family `name` placed in it once, with each of `parts` placed where it is, as a family of its own: the
/// tints that colour them. A door's or a window's part is another opening in the same wall, at the same place.
pub(super) fn place(m: &mut Model, name: &str, parts: Vec<(Family, Rgb)>) -> Vec<(Id, Rgb)> {
    let placed = m.elements.iter().find_map(|(_, e)| match e {
        Element::Component(c) if c.family == name => Some(e.clone()),
        Element::Door(o) | Element::Window(o) if o.family.as_deref() == Some(name) => Some(e.clone()),
        _ => None,
    });
    let Some(placed) = placed else { return Vec::new() };
    m.materials.insert(VOID.into(), bimcraft_core::model::Material { color: [40, 120, 230], transparency: 55, ..Default::default() });
    let mut tints = Vec::new();
    for (k, (family, tint)) in parts.into_iter().enumerate() {
        let own = format!("{name} (lit {})", k + 1);
        m.families.insert(own.clone(), family);
        let mut e = placed.clone();
        match &mut e {
            Element::Component(c) => c.family = own,
            Element::Door(o) | Element::Window(o) => o.family = Some(own),
            _ => continue,
        }
        tints.push((m.insert(e), tint));
    }
    tints
}

/// The reference plane or line `lit` names drawn into {3D}'s picture `image` (taken through `cam`) in colour `c`: the plane as
/// much of it as the family takes up (`extents`) and a margin round, as Work Plane > Show lays one; the line a little wide.
/// `f` is the family as flexed, with its planes and lines where the type puts them.
pub(super) fn draw_datum(image: &mut Image, cam: &Camera, f: &Family, lit: Lit, c: Color32, extents: ([f64; 3], [f64; 3])) {
    match lit {
        Lit::Plane(g) => {
            let Ok(frame) = f.frame(&PlaneRef::Plane(g)) else { return };
            let plane = bimcraft_core::workplane::Frame { origin: frame.origin, normal: frame.z };
            let Some(p) = crate::workplane::patch(plane, extents, super::views::WORK_PLANE_MARGIN, [0.0; 3]) else { return };
            let mark = PlaneMark {
                origin: p.origin,
                axes: p.axes,
                lo: p.lo,
                hi: p.hi,
                step: 0.0,
                fill: c.gamma_multiply(0.25).to_array(),
                grid: [0; 4],
                edge: c.to_array(),
                edge_width: 2.0,
            };
            image.draw_marks(cam, &[mark]);
        }
        Lit::Line(g) => {
            if let Some(l) = f.lines.iter().find(|l| l.guid == g) {
                image.draw_line(cam, [l.start, l.end], 3.0, c.to_array());
            }
        }
        Lit::Part(_) => {}
    }
}
