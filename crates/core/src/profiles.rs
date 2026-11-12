//! Steel sections: the shape a structural column's or a beam's type may have besides a plain rectangle or circle, and a
//! small catalogue of the common European ones by name.
//!
//! A type keeps its `width` (across: an I's flange, a channel's flange, a T's flange, an angle's horizontal leg) and `depth`
//! (an I's, a channel's or a T's height, an angle's vertical leg); a [`Profile`] adds the shape and its plate thicknesses. Its
//! coordinates are those of IFC's parameterized profiles: x across the width, y along the depth, the origin in the middle of
//! the box round it; one `turned` is turned half round in that plane, as IFC turns a profile by its own axes (a T's flange
//! below: an inverted T, a foundation beam's). The outline (a plan's cut of a column) rounds its root fillets; the pieces (what the 3D view and sections build) are
//! rectangles and convex polygons whose union is the section, so a section's cut of a beam is exact.
//!
//! The catalogue's dimensions are those of the public standard tables (EN 10365: IPE, HE A, HE B): depth, width, web and
//! flange thickness and root radius, mm.

use serde::{Deserialize, Serialize};

use crate::geom::P2;

/// Segments a root fillet's quarter circle is drawn with.
const ARC: usize = 4;

/// Sides a circular hollow section's rings are drawn with.
pub const SIDES: usize = 24;

/// Steel's density, kg/m³, which a section's weight per metre is worked out with.
pub const STEEL_DENSITY: f64 = 7850.0;

/// The shape of a steel section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// An I or H: two flanges and a web between their middles (IPE, HE).
    I,
    /// A channel: two flanges and a web along one side (UPE; parallel flanges).
    U,
    /// An angle: two legs at a corner.
    L,
    /// A T: a flange on top and a web down from its middle (turned, an inverted T).
    T,
    /// A rectangular (or square) hollow section, a tube.
    Rhs,
    /// A circular hollow section, a pipe.
    Chs,
}

impl Shape {
    /// The shape a word names: I (or H), U (or C, channel), L (angle), T (tee), RHS (SHS, tube) or CHS (pipe), in any case.
    pub fn parse(s: &str) -> Option<Shape> {
        match s.trim().to_lowercase().as_str() {
            "i" | "h" => Some(Shape::I),
            "u" | "c" | "channel" => Some(Shape::U),
            "l" | "angle" => Some(Shape::L),
            "t" | "tee" => Some(Shape::T),
            "rhs" | "shs" | "tube" => Some(Shape::Rhs),
            "chs" | "pipe" => Some(Shape::Chs),
            _ => None,
        }
    }

    /// Its name, as a file and a schedule write it.
    pub fn word(self) -> &'static str {
        match self {
            Shape::I => "I",
            Shape::U => "U",
            Shape::L => "L",
            Shape::T => "T",
            Shape::Rhs => "RHS",
            Shape::Chs => "CHS",
        }
    }
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// A type's section, besides its width and depth: its shape, the name it has in a catalogue (`IPE 300`), and its plates: an
/// I's, a channel's or a T's `web` and `flange` thickness, an angle's or a hollow section's wall `thickness`, and the `radius`
/// of the fillets where an I's, a channel's, a T's or an angle's plates meet; `turned`, turned half round in its plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub web: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub flange: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub radius: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub turned: bool,
}

/// A piece of a section: a rectangle from its lower left corner to its upper right, or a convex polygon, counter-clockwise.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Rect(P2, P2),
    Convex(Vec<P2>),
}

impl Piece {
    /// Its outline, counter-clockwise.
    pub fn ring(&self) -> Vec<P2> {
        match self {
            Piece::Rect(lo, hi) => vec![*lo, P2(hi.0, lo.1), *hi, P2(lo.0, hi.1)],
            Piece::Convex(pts) => pts.clone(),
        }
    }
}

/// A corner of an outline, and the radius it is rounded inside with (0: sharp).
type Corner = (P2, f64);

/// The quarter circle that rounds corner `i` of `corners` (none for a sharp one): from where it leaves the edge before the
/// corner to where it meets the one after.
fn fillet(corners: &[Corner], i: usize) -> Option<(P2, Vec<P2>)> {
    let n = corners.len();
    let (v, r) = *corners.get(i)?;
    if r <= 0.0 || n < 3 {
        return None;
    }
    let (prev, next) = (corners.get((i + n - 1) % n)?.0, corners.get((i + 1) % n)?.0);
    let (e1, e2) = ((prev - v).unit(), (next - v).unit());
    let o = v + (e1 + e2) * r;
    let (t1, t2) = (v + e1 * r, v + e2 * r);
    let a1 = (t1.1 - o.1).atan2(t1.0 - o.0);
    let a2 = (t2.1 - o.1).atan2(t2.0 - o.0);
    let mut da = a2 - a1;
    while da > std::f64::consts::PI {
        da -= std::f64::consts::TAU;
    }
    while da < -std::f64::consts::PI {
        da += std::f64::consts::TAU;
    }
    let arc = (0..=ARC)
        .map(|k| {
            let a = a1 + da * k as f64 / ARC as f64;
            o + P2(a.cos(), a.sin()) * r
        })
        .collect();
    Some((v, arc))
}

/// The outline through `corners`, each rounded by its radius.
fn rounded(corners: &[Corner]) -> Vec<P2> {
    (0..corners.len())
        .flat_map(|i| match fillet(corners, i) {
            Some((_, arc)) => arc,
            None => corners.get(i).map(|c| c.0).into_iter().collect(),
        })
        .collect()
}

/// The fillets of `corners` as pieces: each the triangles from its corner to its arc.
fn fans(corners: &[Corner]) -> Vec<Piece> {
    (0..corners.len())
        .filter_map(|i| fillet(corners, i))
        .flat_map(|(v, arc)| arc.windows(2).filter_map(|w| Some(Piece::Convex(vec![v, *w.first()?, *w.get(1)?]))).collect::<Vec<_>>())
        .collect()
}

/// A circle's points, counter-clockwise from half a side past east: no corner on its axes, so a section through its middle
/// crosses its sides, not its corners.
fn circle(r: f64) -> Vec<P2> {
    (0..SIDES)
        .map(|k| {
            let a = std::f64::consts::TAU * (k as f64 + 0.5) / SIDES as f64;
            P2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Compact, allocation-free shape identity used by derived geometry caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GeometryKey {
    shape: Shape,
    plates: [u64; 4],
    turned: bool,
}

impl GeometryKey {
    pub(crate) fn profile(self) -> Profile {
        let [web, flange, thickness, radius] = self.plates.map(f64::from_bits);
        Profile { shape: self.shape, name: String::new(), web, flange, thickness, radius, turned: self.turned }
    }
}

impl Profile {
    pub(crate) fn geometry_key(&self) -> GeometryKey {
        GeometryKey {
            shape: self.shape,
            plates: [self.web, self.flange, self.thickness, self.radius].map(f64::to_bits),
            turned: self.turned,
        }
    }

    /// Lateral perimeter, mm, including the inner surface of hollow sections.
    pub fn perimeter(&self, width: f64, depth: f64) -> f64 {
        let r = self.radius;
        let rounded_corner = (2.0 - std::f64::consts::FRAC_PI_2) * r;
        match self.shape {
            Shape::I => 4.0 * width + 2.0 * depth - 2.0 * self.web - 4.0 * rounded_corner,
            Shape::U => 4.0 * width + 2.0 * depth - 2.0 * self.web - 2.0 * rounded_corner,
            Shape::L => 2.0 * (width + depth) - rounded_corner,
            Shape::T => 2.0 * (width + depth) - 2.0 * rounded_corner,
            Shape::Rhs => 4.0 * (width + depth - 2.0 * self.thickness),
            Shape::Chs => std::f64::consts::PI * (2.0 * width - 2.0 * self.thickness),
        }
    }

    /// The corners of its outline, counter-clockwise, with the radius each is rounded by; none for a hollow section.
    fn corners(&self, width: f64, depth: f64) -> Vec<Corner> {
        let (hw, hd, r) = (width / 2.0, depth / 2.0, self.radius);
        let p = |x: f64, y: f64, r: f64| (P2(x, y), r);
        match self.shape {
            Shape::I => {
                let (t, f) = (self.web / 2.0, self.flange);
                vec![
                    p(-hw, -hd, 0.0),
                    p(hw, -hd, 0.0),
                    p(hw, -hd + f, 0.0),
                    p(t, -hd + f, r),
                    p(t, hd - f, r),
                    p(hw, hd - f, 0.0),
                    p(hw, hd, 0.0),
                    p(-hw, hd, 0.0),
                    p(-hw, hd - f, 0.0),
                    p(-t, hd - f, r),
                    p(-t, -hd + f, r),
                    p(-hw, -hd + f, 0.0),
                ]
            }
            // The web along the west side, the flanges reaching east from it.
            Shape::U => {
                let (w, f) = (self.web, self.flange);
                vec![
                    p(-hw, -hd, 0.0),
                    p(hw, -hd, 0.0),
                    p(hw, -hd + f, 0.0),
                    p(-hw + w, -hd + f, r),
                    p(-hw + w, hd - f, r),
                    p(hw, hd - f, 0.0),
                    p(hw, hd, 0.0),
                    p(-hw, hd, 0.0),
                ]
            }
            // One leg up the west side, the other along the bottom.
            Shape::L => {
                let t = self.thickness;
                vec![p(-hw, -hd, 0.0), p(hw, -hd, 0.0), p(hw, -hd + t, 0.0), p(-hw + t, -hd + t, r), p(-hw + t, hd, 0.0), p(-hw, hd, 0.0)]
            }
            // The flange along the top, the web down from its middle.
            Shape::T => {
                let (t, f) = (self.web / 2.0, self.flange);
                vec![
                    p(-t, -hd, 0.0),
                    p(t, -hd, 0.0),
                    p(t, hd - f, r),
                    p(hw, hd - f, 0.0),
                    p(hw, hd, 0.0),
                    p(-hw, hd, 0.0),
                    p(-hw, hd - f, 0.0),
                    p(-t, hd - f, r),
                ]
            }
            Shape::Rhs | Shape::Chs => Vec::new(),
        }
    }

    /// Its outline: the outer ring, counter-clockwise, and a hollow section's inner one, clockwise.
    pub fn rings(&self, width: f64, depth: f64) -> Vec<Vec<P2>> {
        let (hw, hd, t) = (width / 2.0, depth / 2.0, self.thickness);
        let rect = |x: f64, y: f64| vec![P2(-x, -y), P2(x, -y), P2(x, y), P2(-x, y)];
        let rings = match self.shape {
            Shape::Rhs => {
                let mut inner = rect(hw - t, hd - t);
                inner.reverse();
                vec![rect(hw, hd), inner]
            }
            Shape::Chs => {
                let mut inner = circle(hw - t);
                inner.reverse();
                vec![circle(hw), inner]
            }
            _ => vec![rounded(&self.corners(width, depth))],
        };
        rings.into_iter().map(|r| r.into_iter().map(|q| self.turn(q)).collect()).collect()
    }

    /// Point `q` of the section as it stands: turned half round about its middle when it is `turned`.
    fn turn(&self, q: P2) -> P2 {
        if self.turned { P2(-q.0, -q.1) } else { q }
    }

    /// Whether it has a hole: a tube or a pipe.
    pub fn hollow(&self) -> bool {
        matches!(self.shape, Shape::Rhs | Shape::Chs)
    }

    /// Rectangles and convex polygons whose union is the section.
    pub fn pieces(&self, width: f64, depth: f64) -> Vec<Piece> {
        let (hw, hd) = (width / 2.0, depth / 2.0);
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Piece::Rect(P2(x0, y0), P2(x1, y1));
        let mut out = match self.shape {
            Shape::I => {
                let (t, f) = (self.web / 2.0, self.flange);
                vec![rect(-hw, -hd, hw, -hd + f), rect(-t, -hd + f, t, hd - f), rect(-hw, hd - f, hw, hd)]
            }
            Shape::U => {
                let (w, f) = (self.web, self.flange);
                vec![rect(-hw, -hd, hw, -hd + f), rect(-hw, -hd + f, -hw + w, hd - f), rect(-hw, hd - f, hw, hd)]
            }
            Shape::L => {
                let t = self.thickness;
                vec![rect(-hw, -hd, -hw + t, hd), rect(-hw + t, -hd, hw, -hd + t)]
            }
            Shape::T => {
                let (t, f) = (self.web / 2.0, self.flange);
                vec![rect(-hw, hd - f, hw, hd), rect(-t, -hd, t, hd - f)]
            }
            Shape::Rhs => {
                let t = self.thickness;
                vec![
                    rect(-hw, -hd, hw, -hd + t),
                    rect(-hw, hd - t, hw, hd),
                    rect(-hw, -hd + t, -hw + t, hd - t),
                    rect(hw - t, -hd + t, hw, hd - t),
                ]
            }
            Shape::Chs => {
                let (outer, inner) = (circle(hw), circle(hw - self.thickness));
                (0..SIDES)
                    .filter_map(|k| {
                        let j = (k + 1) % SIDES;
                        Some(Piece::Convex(vec![*inner.get(k)?, *outer.get(k)?, *outer.get(j)?, *inner.get(j)?]))
                    })
                    .collect()
            }
        };
        out.extend(fans(&self.corners(width, depth)));
        if !self.turned {
            return out;
        }
        out.into_iter()
            .map(|piece| match piece {
                // Turned half round, a rectangle's lower left corner is its upper right one, turned.
                Piece::Rect(lo, hi) => Piece::Rect(self.turn(hi), self.turn(lo)),
                Piece::Convex(pts) => Piece::Convex(pts.into_iter().map(|q| self.turn(q)).collect()),
            })
            .collect()
    }

    /// Its cross-section's area, mm², with its fillets.
    pub fn area(&self, width: f64, depth: f64) -> f64 {
        let (b, h, r) = (width, depth, self.radius);
        let fillet = (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
        match self.shape {
            Shape::I => 2.0 * b * self.flange + (h - 2.0 * self.flange) * self.web + 4.0 * fillet,
            Shape::U => 2.0 * b * self.flange + (h - 2.0 * self.flange) * self.web + 2.0 * fillet,
            Shape::L => self.thickness * (b + h - self.thickness) + fillet,
            Shape::T => b * self.flange + (h - self.flange) * self.web + 2.0 * fillet,
            Shape::Rhs => b * h - (b - 2.0 * self.thickness) * (h - 2.0 * self.thickness),
            Shape::Chs => std::f64::consts::FRAC_PI_4 * (b * b - (b - 2.0 * self.thickness).powi(2)),
        }
    }

    /// Why it isn't a section `width` by `depth` (and `round` or not) can be, if it isn't: its plates must fit.
    pub fn check(&self, who: &str, width: f64, depth: f64, round: bool) -> Result<(), String> {
        for (what, x) in [("web", self.web), ("flange", self.flange), ("thickness", self.thickness), ("radius", self.radius)] {
            if !x.is_finite() || x < 0.0 {
                return Err(format!("{who}: its profile's {what} must be a length of 0 or more, got {x}"));
            }
        }
        if self.name.chars().count() > crate::model::MAX_TEXT {
            return Err(format!("{who}: its profile's name is longer than {} characters", crate::model::MAX_TEXT));
        }
        let need = |what: &str, x: f64| -> Result<(), String> {
            if x > 0.0 { Ok(()) } else { Err(format!("{who}: a {} section needs its `{what}`", self.shape.word())) }
        };
        let fits = |ok: bool, why: String| if ok { Ok(()) } else { Err(format!("{who}: {why}")) };
        let (r, small) = (self.radius, width.min(depth));
        if (self.shape == Shape::Chs) != round {
            return Err(format!("{who}: a circular hollow section is round, and only it: give it `width` (its diameter), not `round`"));
        }
        match self.shape {
            Shape::I | Shape::U => {
                need("web", self.web)?;
                need("flange", self.flange)?;
                let sides = if self.shape == Shape::I { self.web + 2.0 * r } else { self.web + r };
                fits(sides < width, format!("its web ({} mm) and fillets don't fit in its width ({width} mm)", self.web))?;
                fits(
                    2.0 * (self.flange + r) < depth,
                    format!("its flanges ({} mm each) and fillets don't fit in its depth ({depth} mm)", self.flange),
                )
            }
            Shape::L => {
                need("thickness", self.thickness)?;
                fits(self.thickness + r < small, format!("its legs ({} mm thick) and fillet don't fit in it", self.thickness))
            }
            Shape::T => {
                need("web", self.web)?;
                need("flange", self.flange)?;
                fits(self.web + 2.0 * r < width, format!("its web ({} mm) and fillets don't fit in its width ({width} mm)", self.web))?;
                fits(self.flange + r < depth, format!("its flange ({} mm) and fillets don't fit in its depth ({depth} mm)", self.flange))
            }
            Shape::Rhs | Shape::Chs => {
                need("thickness", self.thickness)?;
                fits(2.0 * self.thickness < small, format!("its walls ({} mm thick) leave no hole", self.thickness))
            }
        }
    }
}

/// The series of the catalogue: their prefix, and each size's depth, width, web, flange and root radius (mm), from the
/// EN 10365 tables.
type Row = (u32, f64, f64, f64, f64, f64);

const IPE: &[Row] = &[
    (80, 80.0, 46.0, 3.8, 5.2, 5.0),
    (100, 100.0, 55.0, 4.1, 5.7, 7.0),
    (120, 120.0, 64.0, 4.4, 6.3, 7.0),
    (140, 140.0, 73.0, 4.7, 6.9, 7.0),
    (160, 160.0, 82.0, 5.0, 7.4, 9.0),
    (180, 180.0, 91.0, 5.3, 8.0, 9.0),
    (200, 200.0, 100.0, 5.6, 8.5, 12.0),
    (220, 220.0, 110.0, 5.9, 9.2, 12.0),
    (240, 240.0, 120.0, 6.2, 9.8, 15.0),
    (270, 270.0, 135.0, 6.6, 10.2, 15.0),
    (300, 300.0, 150.0, 7.1, 10.7, 15.0),
    (330, 330.0, 160.0, 7.5, 11.5, 18.0),
    (360, 360.0, 170.0, 8.0, 12.7, 18.0),
    (400, 400.0, 180.0, 8.6, 13.5, 21.0),
    (450, 450.0, 190.0, 9.4, 14.6, 21.0),
    (500, 500.0, 200.0, 10.2, 16.0, 21.0),
    (550, 550.0, 210.0, 11.1, 17.2, 24.0),
    (600, 600.0, 220.0, 12.0, 19.0, 24.0),
];

const HEA: &[Row] = &[
    (100, 96.0, 100.0, 5.0, 8.0, 12.0),
    (120, 114.0, 120.0, 5.0, 8.0, 12.0),
    (140, 133.0, 140.0, 5.5, 8.5, 12.0),
    (160, 152.0, 160.0, 6.0, 9.0, 15.0),
    (180, 171.0, 180.0, 6.0, 9.5, 15.0),
    (200, 190.0, 200.0, 6.5, 10.0, 18.0),
    (220, 210.0, 220.0, 7.0, 11.0, 18.0),
    (240, 230.0, 240.0, 7.5, 12.0, 21.0),
    (260, 250.0, 260.0, 7.5, 12.5, 24.0),
    (280, 270.0, 280.0, 8.0, 13.0, 24.0),
    (300, 290.0, 300.0, 8.5, 14.0, 27.0),
    (320, 310.0, 300.0, 9.0, 15.5, 27.0),
    (340, 330.0, 300.0, 9.5, 16.5, 27.0),
    (360, 350.0, 300.0, 10.0, 17.5, 27.0),
    (400, 390.0, 300.0, 11.0, 19.0, 27.0),
    (450, 440.0, 300.0, 11.5, 21.0, 27.0),
    (500, 490.0, 300.0, 12.0, 23.0, 27.0),
    (550, 540.0, 300.0, 12.5, 24.0, 27.0),
    (600, 590.0, 300.0, 13.0, 25.0, 27.0),
];

const HEB: &[Row] = &[
    (100, 100.0, 100.0, 6.0, 10.0, 12.0),
    (120, 120.0, 120.0, 6.5, 11.0, 12.0),
    (140, 140.0, 140.0, 7.0, 12.0, 12.0),
    (160, 160.0, 160.0, 8.0, 13.0, 15.0),
    (180, 180.0, 180.0, 8.5, 14.0, 15.0),
    (200, 200.0, 200.0, 9.0, 15.0, 18.0),
    (220, 220.0, 220.0, 9.5, 16.0, 18.0),
    (240, 240.0, 240.0, 10.0, 17.0, 21.0),
    (260, 260.0, 260.0, 10.0, 17.5, 24.0),
    (280, 280.0, 280.0, 10.5, 18.0, 24.0),
    (300, 300.0, 300.0, 11.0, 19.0, 27.0),
    (320, 320.0, 300.0, 11.5, 20.5, 27.0),
    (340, 340.0, 300.0, 12.0, 21.5, 27.0),
    (360, 360.0, 300.0, 12.5, 22.5, 27.0),
    (400, 400.0, 300.0, 13.5, 24.0, 27.0),
    (450, 450.0, 300.0, 14.0, 26.0, 27.0),
    (500, 500.0, 300.0, 14.5, 28.0, 27.0),
    (550, 550.0, 300.0, 15.0, 29.0, 27.0),
    (600, 600.0, 300.0, 15.5, 30.0, 27.0),
];

const SERIES: [(&str, &[Row]); 3] = [("IPE", IPE), ("HEA", HEA), ("HEB", HEB)];

/// A section of the catalogue by its name, in any case and spacing (`IPE 300`, `ipe300`, `HEA 200`, `HE 200 A`): its
/// width, its depth and its profile, named as the catalogue names it.
pub fn catalogue(name: &str) -> Option<(f64, f64, Profile)> {
    let key: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    // HE 200 A is HEA 200.
    let key = match key.strip_prefix("HE") {
        Some(rest) if rest.ends_with('A') || rest.ends_with('B') => {
            let (size, series) = rest.split_at(rest.len() - 1);
            format!("HE{series}{size}")
        }
        _ => key,
    };
    SERIES.iter().find_map(|(prefix, rows)| {
        let size: u32 = key.strip_prefix(prefix)?.parse().ok()?;
        let &(_, h, b, web, flange, radius) = rows.iter().find(|r| r.0 == size)?;
        let profile = Profile { shape: Shape::I, name: format!("{prefix} {size}"), web, flange, thickness: 0.0, radius, turned: false };
        Some((b, h, profile))
    })
}

/// Every name of the catalogue, by series and size.
pub fn catalogue_names() -> Vec<String> {
    SERIES.iter().flat_map(|(prefix, rows)| rows.iter().map(move |r| format!("{prefix} {}", r.0))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_of(rings: &[Vec<P2>]) -> f64 {
        rings.iter().map(|r| crate::geom::area(r)).sum()
    }

    #[test]
    fn the_catalogue_reads_its_names_in_any_spelling_and_has_the_standards_weights() {
        for name in ["IPE 300", "ipe300", " IPE  300 "] {
            let (b, h, p) = catalogue(name).unwrap();
            assert_eq!((b, h, p.web, p.flange, p.radius, p.name.as_str()), (150.0, 300.0, 7.1, 10.7, 15.0, "IPE 300"));
        }
        assert_eq!(catalogue("HE 300 A"), catalogue("HEA 300"));
        assert!(catalogue("IPE 301").is_none() && catalogue("XYZ 300").is_none() && catalogue("").is_none());
        // The tables' masses per metre: IPE 300 42.2 kg/m, HEA 300 88.3, HEB 300 117, IPE 600 122.
        for (name, kg) in [("IPE 300", 42.2), ("HEA 300", 88.3), ("HEB 300", 117.0), ("IPE 600", 122.0), ("HEB 100", 20.4)] {
            let (b, h, p) = catalogue(name).unwrap();
            let per_metre = p.area(b, h) * 1e-6 * STEEL_DENSITY;
            assert!((per_metre - kg).abs() / kg < 0.01, "{name}: {per_metre} kg/m, not {kg}");
        }
        assert_eq!(catalogue_names().len(), IPE.len() + HEA.len() + HEB.len());
        for n in catalogue_names() {
            let (b, h, p) = catalogue(&n).unwrap();
            p.check(&n, b, h, false).unwrap();
        }
    }

    #[test]
    fn every_shapes_outline_and_pieces_cover_its_area() {
        let shapes = [
            (
                Profile { shape: Shape::I, name: String::new(), web: 7.1, flange: 10.7, thickness: 0.0, radius: 15.0, turned: false },
                150.0,
                300.0,
            ),
            (
                Profile { shape: Shape::U, name: String::new(), web: 7.0, flange: 11.0, thickness: 0.0, radius: 13.0, turned: false },
                90.0,
                200.0,
            ),
            (
                Profile { shape: Shape::L, name: String::new(), web: 0.0, flange: 0.0, thickness: 10.0, radius: 12.0, turned: false },
                100.0,
                150.0,
            ),
            (
                Profile { shape: Shape::Rhs, name: String::new(), web: 0.0, flange: 0.0, thickness: 8.0, radius: 0.0, turned: false },
                200.0,
                100.0,
            ),
            (
                Profile { shape: Shape::Chs, name: String::new(), web: 0.0, flange: 0.0, thickness: 6.0, radius: 0.0, turned: false },
                168.3,
                168.3,
            ),
            (
                Profile { shape: Shape::T, name: String::new(), web: 400.0, flange: 500.0, thickness: 0.0, radius: 50.0, turned: false },
                1500.0,
                1000.0,
            ),
            (
                Profile { shape: Shape::T, name: String::new(), web: 400.0, flange: 500.0, thickness: 0.0, radius: 50.0, turned: true },
                1500.0,
                1000.0,
            ),
        ];
        for (p, w, d) in shapes {
            let exact = p.area(w, d);
            let outline = area_of(&p.rings(w, d));
            let pieces: f64 = p.pieces(w, d).iter().map(|q| crate::geom::area(&q.ring())).sum();
            // Polygons a little inside the arcs: within two percent of the area.
            assert!((outline - exact).abs() / exact < 0.02, "{:?}: outline {outline} of {exact}", p.shape);
            assert!((pieces - outline).abs() / exact < 1e-6, "{:?}: pieces {pieces} of {outline}", p.shape);
            assert!(p.pieces(w, d).iter().all(|q| crate::geom::area(&q.ring()) > 0.0), "{:?}: counter-clockwise", p.shape);
            // Within the box round it.
            for q in p.rings(w, d).iter().flatten() {
                assert!(q.0.abs() <= w / 2.0 + 1e-9 && q.1.abs() <= d / 2.0 + 1e-9, "{:?}", p.shape);
            }
            // A T's flange is on top, a turned one's below.
            if p.shape == Shape::T {
                let flange = p.pieces(w, d).into_iter().find(|q| matches!(q, Piece::Rect(lo, hi) if hi.0 - lo.0 == w));
                let Some(Piece::Rect(lo, hi)) = flange else { panic!("no flange") };
                assert_eq!(if p.turned { lo.1 } else { hi.1 }, if p.turned { -d / 2.0 } else { d / 2.0 }, "{lo:?} {hi:?}");
            }
        }
    }

    #[test]
    fn a_profile_whose_plates_dont_fit_is_refused() {
        let i = |web, flange, radius| Profile { shape: Shape::I, name: String::new(), web, flange, thickness: 0.0, radius, turned: false };
        assert!(i(7.0, 10.0, 15.0).check("t", 150.0, 300.0, false).is_ok());
        assert!(i(0.0, 10.0, 0.0).check("t", 150.0, 300.0, false).unwrap_err().contains("web"));
        assert!(i(160.0, 10.0, 0.0).check("t", 150.0, 300.0, false).unwrap_err().contains("width"));
        assert!(i(7.0, 150.0, 0.0).check("t", 150.0, 300.0, false).unwrap_err().contains("depth"));
        assert!(i(7.0, 10.0, f64::NAN).check("t", 150.0, 300.0, false).is_err());
        assert!(i(7.0, 10.0, 0.0).check("t", 150.0, 300.0, true).unwrap_err().contains("round"));
        let chs = Profile { shape: Shape::Chs, name: String::new(), web: 0.0, flange: 0.0, thickness: 90.0, radius: 0.0, turned: false };
        assert!(chs.check("t", 168.3, 168.3, true).unwrap_err().contains("hole"));
    }
}
