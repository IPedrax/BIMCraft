//! Roof framing (`framing/roof.rs`) attacked through commands: every framed roof these tests make is held to the invariants
//! in [`check`], whatever its footprint, slopes, gables, openings, the walls and beams under it and its framing type: each
//! member lies inside its slope (a ridge board over the two it joins), outside every opening and shaft, and in the structure
//! layer, following the slope; no two members overlap, nor a member and the top plates of the framed walls under it; a
//! rafter ends on something (a rim, a ridge board, a header, the next piece, the roof's edge) or a warning says why not; no
//! bay is wider than the type's spacing without a warning; a piece longer than the longest stock is warned of; the same
//! model gives the same members, keyed once each; and the schedule, IFC and the 3D view have as many members as `framing`.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::{Element, Id};

pub(crate) fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

pub(crate) fn first(v: &Value) -> Id {
    v["ids"][0].as_u64().or(v["id"].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn pair(v: &Value) -> [f64; 2] {
    [num(&v[0]), num(&v[1])]
}

fn pt(v: &Value) -> P2 {
    P2(num(&v[0]), num(&v[1]))
}

/// A member of a `framing` reply: its box in its frame, its corners in plan, and its heights.
#[derive(Clone, Debug)]
pub(crate) struct M {
    pub key: String,
    pub role: String,
    pub grain: String,
    pub length: f64,
    pub stock: Option<f64>,
    pub section: [f64; 2],
    pub s: [f64; 2],
    pub across: [f64; 2],
    /// [bottom at s0, bottom at s1, top at s0, top at s1].
    pub z: [f64; 4],
    /// Its frame: origin and the way `s` runs.
    pub o: P2,
    pub x: P2,
}

impl M {
    pub fn at(&self, s: f64, a: f64) -> P2 {
        self.o + self.x * s + self.x.perp() * a
    }

    /// Its rectangle shrunk by `e` all round, in plan, counter-clockwise.
    pub fn quad(&self, e: f64) -> [P2; 4] {
        let ([s0, s1], [a0, a1]) = (self.s, self.across);
        [self.at(s0 + e, a0 + e), self.at(s1 - e, a0 + e), self.at(s1 - e, a1 - e), self.at(s0 + e, a1 - e)]
    }

    /// Its bottom and top over plan point `p`.
    pub fn heights(&self, p: P2) -> [f64; 2] {
        let s = (p - self.o).dot(self.x);
        let t = if self.s[1] - self.s[0] > 1e-9 { (s - self.s[0]) / (self.s[1] - self.s[0]) } else { 0.0 };
        [self.z[0] + (self.z[1] - self.z[0]) * t, self.z[2] + (self.z[3] - self.z[2]) * t]
    }

    /// Its centre across, in plan, at its middle along.
    pub fn middle(&self) -> P2 {
        self.at((self.s[0] + self.s[1]) / 2.0, (self.across[0] + self.across[1]) / 2.0)
    }

    /// The slope it is in, by its key: `face/<i>/…`.
    pub fn face(&self) -> Option<usize> {
        self.key.strip_prefix("face/")?.split('/').next()?.parse().ok()
    }
}

pub(crate) struct Reply {
    pub members: Vec<M>,
    pub warnings: Vec<String>,
    pub raw: Value,
}

impl Reply {
    pub fn role(&self, role: &str) -> Vec<&M> {
        self.members.iter().filter(|m| m.role == role).collect()
    }

    pub fn warned(&self, what: &str) -> bool {
        self.warnings.iter().any(|w| w.contains(what))
    }
}

/// Host `id`'s `framing`: each member in plan, in its own frame where it has one.
pub(crate) fn read(s: &mut Session, id: Id) -> Reply {
    let raw = run(s, "framing", json!({"id": id}));
    let (origin, x) = (pt(&raw["origin"]), pt(&raw["x_axis"]));
    let members = raw["members"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (o, x) = m.get("frame").map_or((origin, x), |fr| (pt(&fr["origin"]), pt(&fr["x_axis"])));
            M {
                key: m["key"].as_str().unwrap_or("").to_string(),
                role: m["role"].as_str().unwrap_or("").to_string(),
                grain: m["grain"].as_str().unwrap_or("").to_string(),
                length: num(&m["length"]),
                stock: m["stock"].as_f64(),
                section: pair(&m["section"]),
                s: pair(&m["s"]),
                across: pair(&m["across"]),
                z: [num(&m["z"][0]), num(&m["z"][1]), num(&m["z"][2]), num(&m["z"][3])],
                o,
                x,
            }
        })
        .collect();
    let warnings = raw["warnings"].as_array().cloned().unwrap_or_default().iter().filter_map(|w| w.as_str().map(String::from)).collect();
    Reply { members, warnings, raw }
}

/// What two plan quads, counter-clockwise, share.
fn shared(a: &[P2; 4], b: &[P2; 4]) -> Vec<P2> {
    let mut out = a.to_vec();
    for (p, q) in geom::ring_edges(b) {
        out = geom::clip(&out, |r| (q - p).cross(r - p));
        if out.len() < 3 {
            return Vec::new();
        }
    }
    out
}

/// Whether members `a` and `b`, each shrunk a hundredth of a millimetre, overlap in space: where their plans overlap, each's
/// bottom is under the other's top somewhere (their heights are straight over it, so where those differences are both above
/// what rounding leaves is a polygon too).
pub(crate) fn overlap(a: &M, b: &M) -> bool {
    let both = shared(&a.quad(0.01), &b.quad(0.01));
    if both.len() < 3 || geom::area(&both).abs() < 1e-6 {
        return false;
    }
    // What the reply's hundredths of a millimetre leave of their heights on a steep slope.
    let k = |m: &M| ((m.z[1] - m.z[0]) / (m.s[1] - m.s[0]).max(1e-9)).abs();
    let e = 0.02 + 0.01 * (k(a) + k(b));
    let under = geom::clip(&both, |p| a.heights(p)[1] - b.heights(p)[0] - e);
    let under = geom::clip(&under, |p| b.heights(p)[1] - a.heights(p)[0] - e);
    under.len() >= 3 && geom::area(&under).abs() > 1e-6
}

/// The roof's slopes as framing has them (outline and holes in plan, frame), the plane of its structure layer's underside and
/// its plumb depth on each, its openings and shafts, its supports' plan rectangles, and its framing type.
pub(crate) struct Roof {
    pub faces: Vec<crate::framing::roof::Face>,
    /// By slope number: its structure layer's underside, and its depth plumb.
    pub bands: std::collections::BTreeMap<usize, (crate::walls::Plane, f64)>,
    pub holes: Vec<(String, Vec<P2>)>,
    pub outline: Vec<P2>,
    pub courtyards: Vec<Vec<P2>>,
    pub t: crate::framing::FramingType,
    /// Each beam's and wall's plan rectangle.
    pub supports: Vec<[P2; 4]>,
}

pub(crate) fn roof_of(s: &Session, id: Id) -> Roof {
    let m = &s.model;
    let Some(Element::Roof(r)) = m.elements.get(id) else { panic!("{id} isn't a roof") };
    let (_, [f0, f1]) = crate::framing::roof::structure(m, r).expect("a structure layer");
    let t = m.types.framing.get(r.framing.as_deref().unwrap_or("")).cloned().expect("a framing type");
    let faces = crate::framing::roof::faces(m, r).expect("its slopes");
    let bands = faces
        .iter()
        .map(|f| {
            let rise = m.roof_rise(r, &f.plane);
            (f.index, (crate::walls::Plane { z: f.plane.z + rise * f0, g: f.plane.g }, rise * (f1 - f0)))
        })
        .collect();
    let mut holes: Vec<(String, Vec<P2>)> = r.openings.iter().enumerate().map(|(i, o)| (format!("opening {i}"), o.clone())).collect();
    for (sid, e) in m.elements.iter() {
        if let Element::Shaft(sh) = e {
            holes.push((format!("shaft {sid}"), sh.boundary.clone()));
        }
    }
    let mut supports = Vec::new();
    for (_, e) in m.elements.iter() {
        let line = match e {
            Element::Beam(_) => m.run_geom(e).map(|g| (P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1]), g.width / 2.0)),
            Element::Wall(w) if w.arc == 0.0 => {
                let axis = crate::walls::Axis::of(w);
                let c = crate::walls::centre(m, w);
                Some((axis.at(0.0, c), axis.at(axis.length(), c), m.thickness(w) / 2.0))
            }
            _ => None,
        };
        if let Some((a, b, half)) = line {
            let n = (b - a).unit().perp() * half;
            supports.push([a - n, b - n, b + n, a + n]);
        }
    }
    let ccw = |r: &[P2]| if geom::area(r) < 0.0 { r.iter().rev().copied().collect() } else { r.to_vec() };
    Roof { faces, bands, holes, outline: ccw(&r.boundary), courtyards: r.courtyards.clone(), t, supports }
}

/// Whether plan point `p` is on the roof: inside its outline and out of its courtyards.
fn on_roof(rf: &Roof, p: P2) -> bool {
    geom::contains(&rf.outline, p) && !rf.courtyards.iter().any(|c| geom::contains(c, p))
}

/// Whether quad `q` lies inside ring `r` (its corners in it, none of `r`'s in it, no edges crossing).
fn quad_inside(q: &[P2; 4], r: &[P2]) -> bool {
    q.iter().all(|p| geom::contains(r, *p))
        && !r.iter().any(|p| geom::contains(q, *p))
        && !geom::ring_edges(q).any(|(a, b)| geom::ring_edges(r).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

fn quad_meets(q: &[P2; 4], r: &[P2]) -> bool {
    q.iter().any(|p| geom::contains(r, *p))
        || r.iter().any(|p| geom::contains(q, *p))
        || geom::ring_edges(q).any(|(a, b)| geom::ring_edges(r).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

/// What may be said of a roof's framing beyond the invariants.
#[derive(Default, Clone, Copy)]
pub(crate) struct Allow {
    /// A bay wider than spacing + thickness (still to be warned of, as the test says).
    pub gaps: bool,
}

/// Roof `id`'s framing held to every invariant, with the members of the framed walls `walls` under it; its reply.
pub(crate) fn check(s: &mut Session, id: Id, walls: &[Id]) -> Reply {
    check_with(s, id, walls, Allow::default())
}

pub(crate) fn check_with(s: &mut Session, id: Id, walls: &[Id], allow: Allow) -> Reply {
    let r = read(s, id);
    assert_eq!(r.raw, read(s, id).raw, "the same model frames the same");
    let rf = roof_of(s, id);
    let (th, spacing) = (rf.t.thickness, rf.t.spacing);
    let ctx = || format!("warnings: {:?}", r.warnings);
    let mut keys: Vec<&str> = r.members.iter().map(|m| m.key.as_str()).collect();
    keys.sort_unstable();
    for w in keys.windows(2) {
        assert_ne!(w[0], w[1], "a key twice; {}", ctx());
    }
    for w in &r.warnings {
        assert!(!w.trim().is_empty(), "an empty warning");
    }
    let face = |i: usize| rf.faces.iter().find(|f| f.index == i);
    let longest_stock = rf.t.stock.last().copied().unwrap_or(f64::INFINITY);
    for m in &r.members {
        let what = format!("{} {:?} {:?} {:?}", m.key, m.s, m.across, m.z);
        let all = m.s.iter().chain(&m.across).chain(&m.z).chain([&m.length]);
        assert!(all.clone().all(|v| v.is_finite()), "{what}: not a number");
        assert!(m.s[1] - m.s[0] > 0.01 && m.across[1] - m.across[0] > 0.01, "{what}: no size");
        let q = m.quad(0.01);
        let i = m.face().unwrap_or_else(|| panic!("{what}: no slope in its key"));
        let f = face(i).unwrap_or_else(|| panic!("{what}: slope {i} isn't framed"));
        let Some(&(under, dz)) = rf.bands.get(&i) else { panic!("{what}: slope {i} has no band") };
        if m.role == "ridge" {
            assert!(q.iter().all(|p| on_roof(&rf, *p)), "{what}: a ridge board off the roof; {}", ctx());
            // A level box between the layer's underside and its top at its faces (to what the hundredth of a millimetre its
            // corners are taken in by allows on a steep slope).
            for p in q {
                // The band of the slope the corner is over.
                let over = rf.faces.iter().find(|f| f.loops.first().is_some_and(|l| geom::contains(l, p)));
                let (under, dz) = over.and_then(|f| rf.bands.get(&f.index)).copied().unwrap_or((under, dz));
                let near = 0.02 + 0.01 * under.g.length();
                let [b, t] = m.heights(p);
                assert!(b >= under.at(p) - near && t <= under.at(p) + dz + near, "{what}: off its layer at {p:?}");
            }
        } else {
            let outline = f.loops.first().cloned().unwrap_or_default();
            assert!(quad_inside(&q, &outline), "{what} isn't inside slope {i}; {}", ctx());
            for hole in f.loops.iter().skip(1) {
                assert!(!quad_meets(&q, hole), "{what} is in a hole of slope {i}; {}", ctx());
            }
            // In the layer: its bottom on the layer's underside and its top on its top, at every corner (to what the reply's
            // hundredths of a millimetre along a steep slope allow).
            let near = 0.02 + 0.01 * under.g.length();
            for p in q {
                let [b, t] = m.heights(p);
                assert!((b - under.at(p)).abs() < near && (t - under.at(p) - dz).abs() < near, "{what}: not in its layer at {p:?}");
            }
            let run = m.s[1] - m.s[0];
            let boxed = if m.grain == "across" { m.across[1] - m.across[0] } else { run.hypot(m.z[1] - m.z[0]) };
            assert!((m.length - boxed).abs() < 0.02, "{what}: length {} isn't its box's {boxed}", m.length);
        }
        for (name, h) in &rf.holes {
            assert!(!quad_meets(&q, h), "{what} is in {name}; {}", ctx());
        }
        // A rafter's, a ridge board's or a rim's piece from stock, or warned of (and no stock in the cut list).
        if rf.t.stock.is_empty() {
            assert!(m.stock.is_none(), "{what}: a stock, of none");
        } else if m.length > longest_stock + 0.01 {
            let line = ["rafter", "trimmer", "rim", "ridge"].contains(&m.role.as_str());
            assert!(!line || r.warned("longer than any stock"), "{what}: {} long, more than the stock; {}", m.length, ctx());
            assert!(m.stock.is_none(), "{what}: a stock for {}", m.length);
        } else {
            assert!(m.stock.is_some_and(|st| st >= m.length - 0.01), "{what}: no stock for {}", m.length);
        }
    }
    // No two overlap, nor one and a framed wall's member under it.
    let mut wall_members: Vec<M> = Vec::new();
    for w in walls {
        wall_members.extend(read(s, *w).members);
    }
    let all: Vec<&M> = r.members.iter().chain(&wall_members).collect();
    let mut boxes: Vec<(f64, f64, usize)> = all
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let q = m.quad(0.01);
            let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            (lo, hi, i)
        })
        .collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let roofs = r.members.len();
    for (k, (_, hi, i)) in boxes.iter().enumerate() {
        for (lo2, _, j) in boxes.iter().skip(k + 1) {
            if lo2 > hi {
                break;
            }
            if *i >= roofs && *j >= roofs {
                continue;
            }
            let (a, b) = (all[*i], all[*j]);
            assert!(!overlap(a, b), "{} and {} overlap: {a:?} {b:?}; {}", a.key, b.key, ctx());
        }
    }
    // Each rafter's and trimmer's ends on something: another member, past the roof's edge, a warned line or hole.
    for m in r.members.iter().filter(|m| m.grain == "along" && m.role != "rim") {
        for (end, dir) in [(m.s[0], -1.0), (m.s[1], 1.0)] {
            let [a0, a1] = m.across;
            let face = [
                m.at((end + dir * 0.001).min(end + dir * 0.06), a0 + 0.001),
                m.at((end + dir * 0.001).max(end + dir * 0.06), a0 + 0.001),
                m.at((end + dir * 0.001).max(end + dir * 0.06), a1 - 0.001),
                m.at((end + dir * 0.001).min(end + dir * 0.06), a1 - 0.001),
            ];
            let face = if geom::area(&face) < 0.0 { [face[3], face[2], face[1], face[0]] } else { face };
            let probes: Vec<P2> = [a0 + 0.01, (a0 + a1) / 2.0, a1 - 0.01].iter().map(|a| m.at(end + dir * 0.05, *a)).collect();
            let on_member = r.members.iter().any(|o| o.key != m.key && shared(&o.quad(0.0), &face).len() >= 3);
            // Lapped past what bears it, or ended over it.
            let grown = |q: &[P2; 4], g: f64| -> [P2; 4] {
                let (u, n) = ((q[1] - q[0]).unit() * g, (q[3] - q[0]).unit() * g);
                [q[0] - u - n, q[1] + u - n, q[2] + u + n, q[3] - u + n]
            };
            let on_support = probes.iter().any(|p| rf.supports.iter().any(|q| geom::contains(&grown(q, rf.t.lap + 0.1), *p)));
            let off = probes.iter().any(|p| !on_roof(&rf, *p));
            let warned_hole = rf.holes.iter().any(|(name, h)| {
                probes.iter().any(|p| geom::contains(h, *p) || h.iter().any(|q| q.dist(*p) < 3.0 * th + spacing))
                    && r.warned(&format!("{name} isn't trimmed"))
            });
            let warned_line =
                r.warned("isn't framed") || r.warned("runs skewed to the") || r.warnings.iter().any(|w| w.contains("is left out"));
            assert!(on_member || on_support || off || warned_hole || warned_line, "{} ends in mid-air at {end}; {}", m.key, ctx());
        }
    }
    // No bay wider than the spacing without a word, across each slope.
    if !allow.gaps {
        for f in &rf.faces {
            let outline = f.loops.first().cloned().unwrap_or_default();
            let local = |p: P2| {
                let d = p - f.origin;
                P2(d.dot(f.x), d.dot(f.x.perp()))
            };
            let ring: Vec<P2> = outline.iter().map(|p| local(*p)).collect();
            let (lo, hi) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            let section = |ring: &[P2], at: f64| -> Vec<[f64; 2]> {
                let mut xs: Vec<f64> = geom::ring_edges(ring)
                    .filter(|(p, q)| (p.0 > at) != (q.0 > at))
                    .map(|(p, q)| p.1 + (at - p.0) * (q.1 - p.1) / (q.0 - p.0))
                    .collect();
                xs.sort_by(f64::total_cmp);
                xs.chunks(2).filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).collect()
            };
            let minus = |x: &[[f64; 2]], y: &[[f64; 2]]| {
                let mut out = x.to_vec();
                for b in y {
                    out = out
                        .into_iter()
                        .flat_map(|a| [[a[0], a[1].min(b[0])], [a[0].max(b[1]), a[1]]].into_iter().filter(|i| i[1] - i[0] > 1e-9))
                        .collect();
                }
                out
            };
            let rings: Vec<Vec<P2>> = r.members.iter().map(|m| m.quad(0.0).iter().map(|p| local(*p)).collect()).collect();
            let mut at = lo + 13.7;
            while at < hi {
                let mut open = section(&ring, at);
                for hole in f.loops.iter().skip(1).chain(rf.holes.iter().map(|(_, h)| h)) {
                    let local_hole: Vec<P2> = hole.iter().map(|p| local(*p)).collect();
                    let fold = |k: fn(&P2) -> f64| {
                        local_hole.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| [lo.min(v), hi.max(v)])
                    };
                    let (bs, ba) = (fold(|p| p.0), fold(|p| p.1));
                    if at > bs[0] && at < bs[1] {
                        open = minus(&open, &[ba]);
                    }
                }
                let mut covered: Vec<[f64; 2]> = rings.iter().flat_map(|q| section(q, at)).collect();
                covered.sort_by(|a, b| a[0].total_cmp(&b[0]));
                for gap in minus(&open, &covered) {
                    assert!(
                        gap[1] - gap[0] <= spacing + th + 0.5,
                        "slope {} at s {at:.1}: nothing from across {:.1} to {:.1}; {}",
                        f.index,
                        gap[0],
                        gap[1],
                        ctx()
                    );
                }
                at += 97.3;
            }
        }
    }
    r
}

/// The schedule's rows, IFC's members and the 3D view's member solids are as many as roof `id`'s members; with Roofs >
/// Framing hidden, no member is drawn and the roof still is.
pub(crate) fn outputs_agree(s: &mut Session, id: Id) {
    let r = read(s, id);
    let n = r.members.len();
    let t = run(
        s,
        "schedule",
        json!({"name": format!("Roof Hunt {id} {}", s.model.elements.len()), "category": "framingmembers", "fields": ["Host", "Key"]}),
    );
    let rows = t["rows"].as_array().map_or(0, |rows| rows.iter().filter(|row| row[0] == json!(id as f64)).count());
    assert_eq!(rows, n, "a schedule row for each member");
    let f = crate::ifc::export(&s.model);
    let rafters = r.members.iter().filter(|m| m.role == "rafter" || m.role == "trimmer").count();
    let tagged = |m: &M| f.lines().any(|l| l.contains("IFCMEMBER(") && l.contains(&format!("'{}'", m.key)));
    assert!(r.members.iter().all(tagged), "an IfcMember for each member");
    assert_eq!(
        f.lines().filter(|l| l.contains("IFCMEMBER(") && l.ends_with(",.RAFTER.);")).count(),
        rafters,
        "a RAFTER for each rafter and trimmer"
    );
    let members = |s: &Session| {
        crate::view3d::shaded(&s.model)
            .into_iter()
            .filter(|x| x.owner() == id && crate::family::visibility::framing(x.vis()) == Some(true))
            .count()
    };
    assert_eq!(members(s), n, "a solid for each member");
}

const ROOF: &str = "Hunt Rafters";
const FRAMING: &str = "Hunt Roof Framing";

/// A session with a roof type of 11 mm sheathing over a 235 mm structure layer, a stud wall type, a beam type and a framing
/// type `FRAMING` of `extra`.
fn session(extra: Value) -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "roof", "name": ROOF, "layers": [
            {"function": "substrate", "material": "Timber", "thickness": 11},
            {"function": "structure", "material": "Timber", "thickness": 235}]}),
    );
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Hunt Stud", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
    run(&mut s, "type", json!({"category": "beam", "name": "Hunt Beam", "width": 89, "depth": 302}));
    let mut t = json!({"category": "framing", "name": FRAMING});
    if let (Some(o), Some(e)) = (t.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(&mut s, "type", t);
    s
}

/// `pts` turned `angle` degrees about the origin.
fn turned(pts: &[[f64; 2]], angle: f64) -> Vec<[f64; 2]> {
    let (c, sn) = (angle.to_radians().cos(), angle.to_radians().sin());
    pts.iter().map(|[x, y]| [x * c - y * sn, x * sn + y * c]).collect()
}

/// A roof over `outline` at `slope`, `gables` upright, its eave 2400 up, framed with `FRAMING`; and framed walls under it
/// along `walls` (a closed loop of points) with their tops attached to it.
fn roof(
    s: &mut Session,
    outline: &[[f64; 2]],
    slope: f64,
    gables: &[usize],
    walls: &[[f64; 2]],
    openings: &[Vec<[f64; 2]>],
) -> (Id, Vec<Id>) {
    let id = first(&run(
        s,
        "roof",
        json!({"boundary": outline, "slope": slope, "gables": gables, "type": ROOF, "level": "Level 1", "offset": 2400,
               "overhang": 0, "openings": openings}),
    ));
    let mut ws = Vec::new();
    for k in 0..walls.len() {
        let (a, b) = (walls[k], walls[(k + 1) % walls.len()]);
        let w = first(&run(s, "wall", json!({"points": [a, b], "type": "Hunt Stud", "height": 1000})));
        run(s, "modify", json!({"id": w, "attach_top": id}));
        ws.push(w);
    }
    if !ws.is_empty() {
        run(s, "frame", json!({"ids": ws, "type": FRAMING}));
    }
    let r = run(s, "frame", json!({"id": id, "type": FRAMING}));
    assert!(r["warnings"].is_array(), "{r}");
    (id, ws)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// The footprints tried: what, its outline, the gables of it as given and as reversed, and the walls under it.
/// A footprint tried: what, its outline, its gables as given and as reversed, and the walls under it.
type Footprint = (&'static str, Vec<[f64; 2]>, Vec<usize>, Vec<usize>, Vec<[f64; 2]>);

fn footprints() -> Vec<Footprint> {
    let l = vec![[0.0, 0.0], [8000.0, 0.0], [8000.0, 4000.0], [4000.0, 4000.0], [4000.0, 7000.0], [0.0, 7000.0]];
    let l_walls = vec![[500.0, 500.0], [7500.0, 500.0], [7500.0, 3500.0], [3500.0, 3500.0], [3500.0, 6500.0], [500.0, 6500.0]];
    vec![
        // A shed falling south: every edge but the south one upright.
        ("shed", rect(0.0, 0.0, 8000.0, 5000.0), vec![1, 2, 3], vec![0, 1, 3], rect(500.0, 500.0, 7500.0, 4500.0)),
        ("gable", rect(0.0, 0.0, 8000.0, 5000.0), vec![1, 3], vec![1, 3], rect(500.0, 500.0, 7500.0, 4500.0)),
        ("hip", rect(0.0, 0.0, 8000.0, 5000.0), vec![], vec![], rect(500.0, 500.0, 7500.0, 4500.0)),
        ("hipped L", l.clone(), vec![], vec![], l_walls.clone()),
    ]
}

#[test]
fn shed_gable_hip_and_l_roofs_hold_at_every_orientation_slope_and_winding() {
    for (what, outline, gables, reversed, walls) in footprints() {
        for angle in [0.0, 90.0, 37.0] {
            for slope in [0.05f64.atan().to_degrees(), 30.0, 60.0] {
                for cw in [false, true] {
                    let mut s = session(json!({"spacing": 600, "lap": 230}));
                    let mut ring = turned(&outline, angle);
                    let mut g = gables.clone();
                    if cw {
                        ring.reverse();
                        g = reversed.clone();
                    }
                    eprintln!("case: {what} at {angle} degrees, slope {slope}, clockwise {cw}");
                    let (id, ws) = roof(&mut s, &ring, slope, &g, &turned(&walls, angle), &[]);
                    let r = check(&mut s, id, &ws);
                    assert!(!r.role("rafter").is_empty(), "{what}: no rafters");
                    let hips = r.warnings.iter().filter(|w| w.contains("hip from") || w.contains("valley from")).count();
                    match what {
                        "shed" => {
                            assert_eq!(hips, 0, "{:?}", r.warnings);
                            assert!(r.role("ridge").is_empty());
                        }
                        "gable" => {
                            assert_eq!(hips, 0, "{:?}", r.warnings);
                            // One board, spliced where it is longer than the stock (`face/<i>/ridge/<edge>/<n>`).
                            let boards: std::collections::BTreeSet<String> =
                                r.role("ridge").iter().map(|m| m.key.split('/').take(4).collect::<Vec<_>>().join("/")).collect();
                            assert_eq!(boards.len(), 1, "one ridge board: {:?}", r.warnings);
                        }
                        "hip" => assert_eq!(hips, 4, "one warning a hip: {:?}", r.warnings),
                        _ => assert!(hips >= 5, "the L's hips and its valley: {:?}", r.warnings),
                    }
                }
            }
        }
    }
}

#[test]
fn rims_ridges_laps_and_bridging_hold_on_every_footprint() {
    for (what, outline, gables, _, walls) in footprints() {
        for extra in [
            json!({"rim": true}),
            json!({"ridge": false, "rim": false}),
            json!({"lap": 0, "bridging": 1200}),
            json!({"lap": 600, "stock": [2440, 3050]}),
            json!({"layout": "centre", "spacing": 406.4}),
        ] {
            let mut s = session(extra.clone());
            eprintln!("case: {what} with {extra}");
            let (id, ws) = roof(&mut s, &outline, 30.0, &gables, &walls, &[]);
            let r = check(&mut s, id, &ws);
            if extra["ridge"] == json!(false) {
                assert!(r.role("ridge").is_empty());
            }
            if extra["rim"] == json!(true) && what != "hipped L" {
                assert!(!r.role("rim").is_empty(), "{:?}", r.warnings);
            }
        }
    }
}

#[test]
fn openings_through_a_slope_are_trimmed_or_said_why_not() {
    let openings = [
        ("a skylight", vec![rect(2000.0, 1000.0, 3000.0, 1800.0)]),
        ("across the ridge", vec![rect(5000.0, 2000.0, 5600.0, 3000.0)]),
        ("at the eave", vec![rect(1000.0, 10.0, 1800.0, 400.0)]),
        ("a diamond", vec![vec![[4000.0, 900.0], [4400.0, 1300.0], [4000.0, 1700.0], [3600.0, 1300.0]]]),
        ("two side by side", vec![rect(1000.0, 1000.0, 1500.0, 1500.0), rect(1700.0, 1000.0, 2200.0, 1500.0)]),
    ];
    for (what, rings) in openings {
        for (kind, gables) in [("gable", vec![1, 3]), ("hip", vec![])] {
            let mut s = session(json!({"bridging": 1000}));
            eprintln!("case: {what} in a {kind} roof");
            let (id, ws) = roof(&mut s, &rect(0.0, 0.0, 8000.0, 5000.0), 30.0, &gables, &rect(500.0, 500.0, 7500.0, 4500.0), &rings);
            let r = check(&mut s, id, &ws);
            if what == "a skylight" {
                assert!(r.members.iter().any(|m| m.key.contains("opening/0/trimmer/")), "{:?}", r.warnings);
                assert!(r.members.iter().any(|m| m.key.contains("opening/0/header/")), "{:?}", r.warnings);
            }
        }
    }
}

#[test]
fn a_flat_roof_frames_as_a_floor_square_to_its_longest_edge() {
    let mut s = session(json!({}));
    let id = first(&run(
        &mut s,
        "roof",
        json!({"boundary": rect(0.0, 0.0, 8000.0, 5000.0), "slope": 0, "type": ROOF, "level": "Level 1", "offset": 2400}),
    ));
    let w = first(&run(&mut s, "wall", json!({"points": [[0.0, 2500.0], [8000.0, 2500.0]], "type": "Hunt Stud", "height": 1000})));
    run(&mut s, "modify", json!({"id": w, "attach_top": id}));
    run(&mut s, "frame", json!({"id": id, "type": FRAMING}));
    let r = check(&mut s, id, &[]);
    let rafters = r.role("rafter");
    assert!(rafters.iter().all(|m| m.x.0.abs() < 1e-9), "square to the long edges: running north");
    assert!(!r.role("rim").is_empty());
    assert_eq!(r.raw["supports"].as_array().map(Vec::len), Some(1), "{:?}", r.warnings);
}

#[test]
fn outputs_agree_on_the_nastiest_roofs() {
    for (what, outline, gables, _, walls) in footprints() {
        let mut s = session(json!({"bridging": 1200, "lap": 300, "rim": true}));
        eprintln!("case: {what}");
        let (id, ws) = roof(&mut s, &turned(&outline, 37.0), 30.0, &gables, &turned(&walls, 37.0), &[rect(2000.0, 1000.0, 3000.0, 1800.0)]);
        check(&mut s, id, &ws);
        outputs_agree(&mut s, id);
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["roofframing"]}));
        let look = crate::graphics::Look::of_3d(&s.model);
        let shown: Vec<_> =
            crate::view3d::shaded(&s.model).into_iter().filter(|x| x.owner() == id && look.forms_show(x.vis(), false)).collect();
        assert!(!shown.iter().any(|x| crate::family::visibility::framing(x.vis()) == Some(true)));
        assert!(!shown.is_empty());
    }
}

#[test]
fn hostile_types_and_edits_never_break_a_framed_roof() {
    let mut s = session(json!({"bridging": 1200, "lap": 300}));
    let (id, ws) = roof(&mut s, &rect(0.0, 0.0, 8000.0, 5000.0), 30.0, &[1, 3], &rect(500.0, 500.0, 7500.0, 4500.0), &[]);
    let t = |v: Value| {
        let mut t = json!({"category": "framing", "name": FRAMING});
        if let (Some(o), Some(e)) = (t.as_object_mut(), v.as_object()) {
            o.extend(e.clone());
        }
        ("type", t)
    };
    let edits = [
        t(json!({"spacing": 38.0001})),
        t(json!({"spacing": 3000, "thickness": 2999})),
        t(json!({"lap": 2000, "bridging": 300, "trimmers": 3, "spacing": 39})),
        t(json!({"stock": [10.0]})),
        t(json!({"stock": []})),
        t(json!({"ridge": "yes"})),
        ("modify", json!({"id": id, "slope": 75})),
        ("modify", json!({"id": id, "slope": 0.001})),
        ("modify", json!({"id": id, "slope": 0})),
        ("modify", json!({"id": id, "gables": [0, 1, 2, 3]})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [1e7, 0], [1e7, 1], [0, 1]]})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [6000, 0], [6000, 0.5], [0, 0.5]]})),
        ("modify", json!({"id": id, "openings": [[[1, 1], [7999, 1], [7999, 4999], [1, 4999]]]})),
        ("modify", json!({"id": id, "offset": 1e9})),
        ("modify", json!({"id": id, "extrusion": {"line": [[0, 0], [8000, 0]], "profile": [[0, 0], [8000, 1000]], "to": 5000}})),
    ];
    for (c, p) in edits {
        let _ = s.execute(c, &p);
        eprintln!("case: {c} {p}");
        if s.execute("framing", &json!({"id": id})).is_ok() {
            check_with(&mut s, id, &ws, Allow { gaps: true });
        }
    }
}

#[test]
fn undo_and_redo_give_back_the_same_members_and_far_keys_stay() {
    let mut s = session(json!({"bridging": 1200, "lap": 300}));
    let (id, ws) = roof(&mut s, &rect(0.0, 0.0, 8000.0, 5000.0), 30.0, &[1, 3], &rect(500.0, 500.0, 7500.0, 4500.0), &[]);
    check(&mut s, id, &ws);
    let edits = [
        ("modify", json!({"id": id, "openings": [rect(2000.0, 1000.0, 3000.0, 1800.0)]})),
        ("type", json!({"category": "framing", "name": FRAMING, "spacing": 300, "rim": true})),
        ("modify", json!({"id": id, "slope": 40})),
        ("type", json!({"category": "framing", "name": FRAMING, "ridge": false})),
    ];
    for (c, p) in edits {
        let was = read(&mut s, id).raw;
        run(&mut s, c, p.clone());
        let now = check(&mut s, id, &ws).raw;
        run(&mut s, "undo", json!({}));
        assert_eq!(read(&mut s, id).raw, was, "{c} {p} undone");
        run(&mut s, "redo", json!({}));
        assert_eq!(read(&mut s, id).raw, now, "{c} {p} redone");
    }
    // An opening on the south slope leaves the north slope's keys as they were.
    let mut s = session(json!({}));
    let (id, ws) = roof(&mut s, &rect(0.0, 0.0, 8000.0, 5000.0), 30.0, &[1, 3], &rect(500.0, 500.0, 7500.0, 4500.0), &[]);
    let north = |r: &Reply| -> Vec<(String, [f64; 2])> {
        let mut v: Vec<(String, [f64; 2])> =
            r.members.iter().filter(|m| m.role == "rafter" && m.middle().1 > 2500.0).map(|m| (m.key.clone(), m.s)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let was = north(&check(&mut s, id, &ws));
    run(&mut s, "modify", json!({"id": id, "openings": [rect(2000.0, 1000.0, 3000.0, 1800.0)]}));
    assert_eq!(north(&check(&mut s, id, &ws)), was);
}
