//! Dimensions in 3D views (`Element::Dimension3D`), as Revit's aligned dimension in a 3D view: how far apart two points of
//! the model are, or a point and an edge, or two parallel edges (square to them). It draws as its dimension type says: a
//! witness line from each end of what it measures to its line, which runs through a point picked off it, a tick at each end
//! and its number lettered along it, over the picture (nothing hides it). The core works out what it measures and draws it,
//! in pixels through a camera, as [`Mark`]s: painted into the processor's picture ([`paint`]), lines on a sheet, and what a
//! front end draws over the graphics card's picture; a front end only picks ([`pick`], [`line_through`]).

use crate::dims::Tick;
use crate::font;
use crate::graphics::Look;
use crate::ids::Key;
use crate::model::{Dimension3D, Element, Id, Model, Ref3};
use crate::render::Rgb;
use crate::view3d::{Camera, Image, P3, add, dot, mul, sub, unit};

/// Pixels a millimetre of paper takes on a screen of 96 dots an inch: how big a dimension's ticks and number are on screen.
pub const SCREEN: f64 = 96.0 / 25.4;

/// A line a dimension draws, in pixels of the picture (y down): its points, how wide and in what colour, and whose it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub owner: Id,
    pub pts: Vec<(f32, f32)>,
    pub width: f32,
    pub color: Rgb,
}

/// The foot of the square from `p` to the line through edge `e`.
fn foot(p: P3, [a, b]: [P3; 2]) -> P3 {
    let d = sub(b, a);
    let l = dot(d, d);
    if l <= 1e-12 { a } else { add(a, mul(d, dot(sub(p, a), d) / l)) }
}

fn dist(a: P3, b: P3) -> f64 {
    let d = sub(b, a);
    dot(d, d).sqrt()
}

/// The two points `d` measures between: its first reference's and its second's; to an edge, the foot of the square to it.
pub fn measured(d: &Dimension3D) -> (P3, P3) {
    ends(d.refs)
}

/// The two points a dimension of references `a` and `b` measures between, as [`measured`] says.
pub fn ends([a, b]: [Ref3; 2]) -> (P3, P3) {
    match (a.edge, b.edge) {
        (_, Some(e)) => (a.at, foot(a.at, e)),
        (Some(e), None) => (foot(b.at, e), b.at),
        (None, None) => (a.at, b.at),
    }
}

/// How long `d` is, mm.
pub fn length(d: &Dimension3D) -> f64 {
    let (p, q) = measured(d);
    dist(p, q)
}

/// Where its line runs: from over its first point to over its second, moved square off them to pass through `d.at`.
pub fn line(d: &Dimension3D) -> (P3, P3) {
    let (p, q) = measured(d);
    let u = unit(sub(q, p));
    let off = sub(d.at, p);
    let off = sub(off, mul(u, dot(off, u)));
    (add(p, off), add(q, off))
}

/// Where a pick at pixel `at` through `cam` puts the line of a dimension from `p` to `q`: on the plane through them that
/// faces the camera most. None where the view runs along it.
pub fn line_through(cam: &Camera, p: P3, q: P3, at: (f64, f64)) -> Option<P3> {
    let u = unit(sub(q, p));
    let to = cam.toward(p);
    let n = unit(sub(to, mul(u, dot(to, u))));
    let on = cam.unproject(at);
    let (from, along) = match cam.eye() {
        Some(eye) => (eye, sub(on, eye)),
        None => (on, cam.toward(on)),
    };
    let k = dot(along, n);
    let t = dot(sub(p, from), n) / k;
    (k.abs() > 1e-9 && t.is_finite()).then(|| add(from, mul(along, t))).filter(|x| x.iter().all(|v| v.is_finite()))
}

/// What the 3D dimensions of `view` that `look` shows draw through `cam`, `mm` pixels to a millimetre of paper.
pub fn marks(m: &Model, view: impl Key, cam: &Camera, look: &Look, mm: f64) -> Vec<Mark> {
    let Some(g) = m.elements.guid(view) else { return Vec::new() };
    m.elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Dimension3D(d) if d.view == g && look.shows(*id) => Some((*id, d)),
            _ => None,
        })
        .flat_map(|(id, d)| marks_of(m, id, d, cam, mm))
        .collect()
}

/// What dimension `d` (element `owner`, or one being placed) draws through `cam`, `mm` pixels to a millimetre of paper: its
/// witness lines, its line, its ticks and its number, as its type says. Nothing where an end of it is behind the eye.
pub fn marks_of(m: &Model, owner: Id, d: &Dimension3D, cam: &Camera, mm: f64) -> Vec<Mark> {
    let t = m.dimension_types.get(&d.type_name).cloned().unwrap_or_default();
    let color = Rgb(t.color[0], t.color[1], t.color[2]);
    let weights = m.line_weights().at(m.units.default_scale());
    let pen = |p| (weights.mm(p, true) * mm).max(1.0) as f32;
    let (p, q) = measured(d);
    let (a, b) = line(d);
    let at = |x: P3| -> Option<(f64, f64)> {
        cam.in_front(x, x)?;
        let (x, y, _) = cam.project(x);
        (x.is_finite() && y.is_finite()).then_some((x, y))
    };
    let (Some(sp), Some(sq), Some(sa), Some(sb)) = (at(p), at(q), at(a), at(b)) else { return Vec::new() };
    let v2 = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0, b.1 - a.1);
    let len = |v: (f64, f64)| v.0.hypot(v.1);
    let norm = |v: (f64, f64), or: (f64, f64)| {
        let l = len(v);
        if l > 1e-6 { (v.0 / l, v.1 / l) } else { or }
    };
    // Along the line on screen, and square to it.
    let along = norm(v2(sa, sb), (1.0, 0.0));
    let across = (-along.1, along.0);
    let mut out = Vec::new();
    let mut put = |pts: Vec<(f64, f64)>, width: f32| {
        let pts: Vec<(f32, f32)> = pts.into_iter().map(|(x, y)| (x as f32, y as f32)).collect();
        if pts.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
            out.push(Mark { owner, pts, width, color });
        }
    };
    let plus = |a: (f64, f64), v: (f64, f64), k: f64| (a.0 + v.0 * k, a.1 + v.1 * k);
    // Witness lines, from a gap off what it measures to past its line; across it where the line runs over what it measures.
    let (gap, ext) = (t.witness_gap * mm, t.witness_extension * mm);
    for (from, to) in [(sp, sa), (sq, sb)] {
        let off = v2(from, to);
        if len(off) > gap + 1.0 {
            let w = norm(off, across);
            put(vec![plus(from, w, gap), plus(to, w, ext)], pen(t.line_pen));
        } else {
            put(vec![plus(to, across, -ext), plus(to, across, ext)], pen(t.line_pen));
        }
    }
    let line_ext = t.line_extension * mm;
    put(vec![plus(sa, along, -line_ext), plus(sb, along, line_ext)], pen(t.line_pen));
    // A tick at each end, as the type's.
    let size = t.tick_size * mm;
    for (end, inward) in [(sa, along), (sb, (-along.0, -along.1))] {
        match t.tick {
            Tick::None => {}
            Tick::Diagonal => {
                let k = norm((along.0 + across.0, along.1 + across.1), along);
                put(vec![plus(end, k, -size / 2.0), plus(end, k, size / 2.0)], pen(t.tick_pen));
            }
            Tick::Dot => {
                let r = size / 4.0;
                let ring = (0..=16).map(|i| f64::from(i) * std::f64::consts::PI / 8.0).map(|a| (end.0 + r * a.cos(), end.1 + r * a.sin()));
                put(ring.collect(), pen(t.tick_pen));
            }
            Tick::Arrow | Tick::OpenArrow => {
                let back = plus(end, inward, size);
                let wide = size * 0.268;
                put(vec![plus(back, across, wide), end, plus(back, across, -wide)], pen(t.tick_pen));
            }
        }
    }
    // Its number, along the line, on the side of it away from what it measures, reading from the left.
    let text = t.length(m.units, dist(p, q));
    let h = t.text_size * mm;
    let read = if along.0 < -1e-9 || (along.0.abs() <= 1e-9 && along.1 > 0.0) { (-along.0, -along.1) } else { along };
    let up = (read.1, -read.0);
    let away = norm(v2(sp, sa), (0.0, -1.0));
    let middle = ((sa.0 + sb.0) / 2.0, (sa.1 + sb.1) / 2.0);
    let off = t.text_offset * mm;
    let base = if away.0 * up.0 + away.1 * up.1 >= 0.0 { plus(middle, up, off) } else { plus(middle, up, -(off + h)) };
    let w = f64::from(font::width(&text, h as f32));
    for stroke in font::strokes(&text, (-w / 2.0) as f32, 0.0, h as f32) {
        let pts = stroke.into_iter().map(|(x, y)| {
            let (x, y) = (f64::from(x), f64::from(y));
            (base.0 + read.0 * x - up.0 * y, base.1 + read.1 * x - up.1 * y)
        });
        put(pts.collect(), pen(t.tick_pen).min(pen(t.line_pen) + 1.0));
    }
    out
}

/// `marks` painted into `image`, each as wide as it says.
pub fn paint(image: &mut Image, marks: &[Mark]) {
    let (w, h) = (image.width as i64, image.height as i64);
    for mk in marks {
        let r = (f64::from(mk.width) / 2.0).max(0.5);
        let reach = r.ceil() as i64;
        for seg in mk.pts.windows(2) {
            let [(x0, y0), (x1, y1)] = [seg[0], seg[1]].map(|(x, y)| (f64::from(x), f64::from(y)));
            // A quarter of a pixel apart, so a slanting line misses none of the pixels it crosses.
            let steps = (((x1 - x0).hypot(y1 - y0) * 4.0).ceil() as usize).clamp(1, 1 << 18);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        let (px, py) = (x.floor() as i64 + dx, y.floor() as i64 + dy);
                        // The pixel the point is in, and those whose middles are within the line's half width.
                        let near = (dx, dy) == (0, 0) || ((px as f64 + 0.5 - x).powi(2) + (py as f64 + 0.5 - y).powi(2)).sqrt() <= r;
                        if near && (0..w).contains(&px) && (0..h).contains(&py) {
                            let at = 4 * (py as usize * w as usize + px as usize);
                            if let Some(c) = image.rgba.get_mut(at..at + 3) {
                                c.copy_from_slice(&[mk.color.0, mk.color.1, mk.color.2]);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The dimension whose marks pass nearest pixel `at`, within `reach` pixels.
pub fn pick(marks: &[Mark], at: (f64, f64), reach: f64) -> Option<Id> {
    let off = |a: (f32, f32), b: (f32, f32)| {
        let (a, b) = ((f64::from(a.0), f64::from(a.1)), (f64::from(b.0), f64::from(b.1)));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx * dx + dy * dy;
        let t = if l > 0.0 { (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
        (a.0 + dx * t - at.0).hypot(a.1 + dy * t - at.1)
    };
    marks
        .iter()
        .flat_map(|mk| mk.pts.windows(2).map(move |s| (mk.owner, off(s[0], s[1]))))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::engine::Session;
    use crate::view3d::Scene;

    fn boxed() -> Session {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        s
    }

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
    }

    /// The south wall's outer top corners, its line 1500 mm south of them at the ground.
    fn across_the_south_wall(s: &mut Session) -> Id {
        let d = run(s, "dimension_3d", json!({"refs": [{"at": [-100, -100, 3000]}, {"at": [8100, -100, 3000]}], "at": [4000, -1600, 0]}));
        assert_eq!(d["length"], 8200.0);
        d["id"].as_u64().unwrap()
    }

    fn the(s: &Session, id: Id) -> &Dimension3D {
        match s.model.elements.get(id) {
            Some(Element::Dimension3D(d)) => d,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_dimension_in_a_3d_view_measures_between_points_or_square_to_an_edge_and_reports_its_length() {
        let mut s = boxed();
        let id = across_the_south_wall(&mut s);
        let e = crate::element_json(&s.model, id);
        assert_eq!((e["category"].as_str(), e["length"].as_f64()), (Some("dimension3d"), Some(8200.0)));
        assert_eq!(s.model.elements.id(the(&s, id).view), s.model.default_3d(), "kept by the default 3D view, made for it");
        // A point to an edge: square to the edge's line, the north wall's top.
        let d = run(
            &mut s,
            "dimension_3d",
            json!({"refs": [{"at": [0, 0, 0]}, {"at": [3000, 6000, 3000], "edge": [[0, 6000, 3000], [8000, 6000, 3000]]}], "at": [0, 3000, 4000]}),
        );
        assert_eq!(d["length"], 6708.2, "the hypotenuse of 6000 north and 3000 up");
        // Two parallel edges: how far apart they are.
        let d = run(
            &mut s,
            "dimension_3d",
            json!({"refs": [{"at": [1000, 0, 3000], "edge": [[0, 0, 3000], [8000, 0, 3000]]}, {"at": [5000, 6000, 3000], "edge": [[0, 6000, 3000], [8000, 6000, 3000]]}],
                   "at": [-500, 3000, 3000]}),
        );
        assert_eq!(d["length"], 6000.0);
        // Refused: nothing between them, a plan as its view, a point out of the project.
        let refused = [
            json!({"refs": [{"at": [0, 0, 0]}, {"at": [0, 0, 0]}], "at": [0, 0, 100]}),
            json!({"refs": [{"at": [0, 0, 0]}, {"at": [1000, 0, 0]}], "at": [0, 0, 100], "view": "Level 1"}),
            json!({"refs": [{"at": [0, 0, 0]}, {"at": [1e12, 0, 0]}], "at": [0, 0, 100]}),
            json!({"refs": [{"at": [0, 0, 0]}], "at": [0, 0, 100]}),
        ];
        let before = s.model.elements.len();
        for p in refused {
            assert!(s.execute("dimension_3d", &p).is_err(), "{p}");
        }
        assert_eq!(s.model.elements.len(), before);
        // A move takes it along; its view deleted takes it too.
        run(&mut s, "move", json!({"ids": [id], "by": [0, 1000]}));
        assert_eq!(the(&s, id).refs[0].at, [-100.0, 900.0, 3000.0]);
        let view = s.model.default_3d().unwrap();
        run(&mut s, "delete", json!({"ids": [view]}));
        assert!(s.model.elements.values().all(|e| !matches!(e, Element::Dimension3D(_))));
    }

    #[test]
    fn its_line_and_number_are_painted_into_the_processors_picture_where_the_camera_sees_them() {
        let mut s = boxed();
        let id = across_the_south_wall(&mut s);
        let m = &s.model;
        let view = m.default_3d().unwrap();
        let (scene, look) = (Scene::new(m), Look::of_3d(m));
        let cam = scene.fit_in(crate::view3d::AZIMUTH, crate::view3d::ELEVATION, 800, 600, &look);
        let (a, b) = line(the(&s, id));
        // Its line, a third of the way along: off the model, where the picture is paper.
        let along = add(a, mul(sub(b, a), 1.0 / 3.0));
        for cam in [cam, cam.orbit(40.0, 10.0)] {
            let mut image = scene.render_in(&cam, &[], &look);
            let (x, y, _) = cam.project(along);
            assert_eq!(image.color(x, y), Rgb(255, 255, 255), "nothing there before");
            let marks = marks(m, view, &cam, &look, SCREEN);
            assert!(marks.len() > 6, "witness lines, its line, ticks and letters: {}", marks.len());
            paint(&mut image, &marks);
            assert_eq!(image.color(x, y), Rgb(0, 0, 0), "its line, in its type's colour, where the camera sees it");
            assert_eq!(pick(&marks, (x + 1.0, y - 1.0), 6.0), Some(id), "picked where it's drawn");
        }
        // The render of the view has it.
        let with = run(&mut s, "render", json!({"view": "{3D}"}))["pngBase64"].as_str().map(String::from);
        run(&mut s, "delete", json!({"ids": [id]}));
        let without = run(&mut s, "render", json!({"view": "{3D}"}))["pngBase64"].as_str().map(String::from);
        assert_ne!(with, without, "the 3D view's PNG draws it");
        // Hidden with the Dimensions category in the view's Visibility/Graphics.
        let id = across_the_south_wall(&mut s);
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["dimension"]}));
        let m = &s.model;
        assert!(marks(m, view, &cam, &Look::of_3d(m), SCREEN).iter().all(|mk| mk.owner != id));
    }

    #[test]
    fn a_3d_views_dimension_prints_on_the_sheet_that_holds_the_view() {
        let mut s = boxed();
        let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
        let id = across_the_south_wall(&mut s);
        let view = s.model.default_3d().unwrap();
        run(&mut s, "place", json!({"sheet": sheet, "view": view}));
        let paper = crate::model::paper(&s.model.sheet(sheet).unwrap().size).unwrap();
        // The sheet's lines, and the strokes of its PDF page.
        let strokes = |s: &Session| {
            let d = crate::sheet::draw(&s.model, sheet).unwrap();
            let lines = d.prims.iter().filter(|p| matches!(p, crate::render::Prim::Line { .. })).count();
            (lines, crate::pdf::contents(&d, paper).0.matches(" S\n").count())
        };
        let (lines, with) = strokes(&s);
        run(&mut s, "delete", json!({"ids": [id]}));
        let (fewer, without) = strokes(&s);
        assert!(lines - fewer > 6, "its witness lines, line, ticks and letters on the sheet: {}", lines - fewer);
        assert_eq!(with - without, lines - fewer, "each a stroke of the sheet's PDF");
    }
}
