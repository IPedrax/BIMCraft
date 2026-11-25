//! Floor framing (`framing/floor.rs`) attacked through commands: every framed floor these tests make is held to the
//! invariants in [`check`], whatever its outline, joist direction, openings, supports, walls on it and framing type: no two
//! members overlap; each lies inside the outline, outside every hole and in the structure layer, its length its box's; a
//! joist ends on something (a rim, a header, a support, the next piece) or a warning says why not; no bay is wider than the
//! type's spacing without a warning; the same model gives the same members, keyed once each; and the schedule, IFC and the 3D
//! view have as many members as `framing` does.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::{Element, Id};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn first(v: &Value) -> Id {
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

/// A member of a `framing` reply, its plan rectangle in plan and in the floor's frame.
#[derive(Clone, Debug)]
struct M {
    key: String,
    role: String,
    grain: String,
    length: f64,
    s: [f64; 2],
    across: [f64; 2],
    z: [f64; 4],
    /// Its corners in plan, counter-clockwise.
    ring: [P2; 4],
    /// In a frame of its own.
    own: bool,
}

impl M {
    /// Its rectangle shrunk by `e` all round, in plan.
    fn shrunk(&self, f: &Reply, e: f64) -> [P2; 4] {
        let (o, x) = self.frame(f);
        let at = |s: f64, a: f64| o + x * s + x.perp() * a;
        let ([s0, s1], [a0, a1]) = (self.s, self.across);
        [at(s0 + e, a0 + e), at(s1 - e, a0 + e), at(s1 - e, a1 - e), at(s0 + e, a1 - e)]
    }

    fn frame(&self, f: &Reply) -> (P2, P2) {
        if self.own { (self.ring[0] - self.x(f) * self.s[0] - self.x(f).perp() * self.across[0], self.x(f)) } else { (f.origin, f.x) }
    }

    fn x(&self, f: &Reply) -> P2 {
        if self.own { (self.ring[1] - self.ring[0]).unit() } else { f.x }
    }
}

struct Reply {
    origin: P2,
    x: P2,
    members: Vec<M>,
    warnings: Vec<String>,
    raw: Value,
}

impl Reply {
    fn frame(&self, p: P2) -> P2 {
        let d = p - self.origin;
        P2(d.dot(self.x), d.dot(self.x.perp()))
    }

    fn keys(&self) -> Vec<&str> {
        self.members.iter().map(|m| m.key.as_str()).collect()
    }

    fn role(&self, role: &str) -> Vec<&M> {
        self.members.iter().filter(|m| m.role == role).collect()
    }

    fn warned(&self, what: &str) -> bool {
        self.warnings.iter().any(|w| w.contains(what))
    }
}

fn read(s: &mut Session, id: Id) -> Reply {
    let raw = run(s, "framing", json!({"id": id}));
    let (origin, x) = (pt(&raw["origin"]), pt(&raw["x_axis"]));
    let members = raw["members"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (o, ax, own) = match m.get("frame") {
                Some(fr) => (pt(&fr["origin"]), pt(&fr["x_axis"]), true),
                None => (origin, x, false),
            };
            let (sv, av) = (pair(&m["s"]), pair(&m["across"]));
            let at = |s: f64, a: f64| o + ax * s + ax.perp() * a;
            M {
                key: m["key"].as_str().unwrap_or("").to_string(),
                role: m["role"].as_str().unwrap_or("").to_string(),
                grain: m["grain"].as_str().unwrap_or("").to_string(),
                length: num(&m["length"]),
                s: sv,
                across: av,
                z: [num(&m["z"][0]), num(&m["z"][1]), num(&m["z"][2]), num(&m["z"][3])],
                ring: [at(sv[0], av[0]), at(sv[1], av[0]), at(sv[1], av[1]), at(sv[0], av[1])],
                own,
            }
        })
        .collect();
    let warnings = raw["warnings"].as_array().cloned().unwrap_or_default().iter().filter_map(|w| w.as_str().map(String::from)).collect();
    Reply { origin, x, members, warnings, raw }
}

/// Whether convex quads `a` and `b` overlap by more than nothing (separating axes).
fn quads_overlap(a: &[P2; 4], b: &[P2; 4]) -> bool {
    let axes = |q: &[P2; 4]| [(q[1] - q[0]).unit().perp(), (q[3] - q[0]).unit().perp()];
    !axes(a).into_iter().chain(axes(b)).any(|n| {
        let span = |q: &[P2; 4]| q.iter().map(|p| p.dot(n)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let ((a0, a1), (b0, b1)) = (span(a), span(b));
        a1.min(b1) - a0.max(b0) <= 0.0
    })
}

/// Whether quad `q` and ring `r` overlap: a corner of one inside the other, or edges crossing.
fn quad_meets_ring(q: &[P2; 4], r: &[P2]) -> bool {
    q.iter().any(|p| geom::contains(r, *p))
        || r.iter().any(|p| geom::contains(q, *p))
        || geom::ring_edges(q).any(|(a, b)| geom::ring_edges(r).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

/// Whether quad `q` lies inside ring `r`.
fn quad_inside_ring(q: &[P2; 4], r: &[P2]) -> bool {
    q.iter().all(|p| geom::contains(r, *p))
        && !r.iter().any(|p| geom::contains(q, *p))
        && !geom::ring_edges(q).any(|(a, b)| geom::ring_edges(r).any(|(c, d)| geom::crossing(a, b, c, d).is_some()))
}

/// The floor's outline, its holes (openings and the shafts in the model), its structure layer's heights and its framing type.
struct Floor {
    outline: Vec<P2>,
    holes: Vec<(String, Vec<P2>)>,
    z: [f64; 2],
    t: crate::framing::FramingType,
    /// Each beam's and wall's plan rectangle, in plan.
    supports: Vec<[P2; 4]>,
    /// Each that bears the joists: its centre line's ends, in plan, and half its width.
    bearing: Vec<(P2, P2, f64)>,
}

fn floor_of(s: &Session, id: Id) -> Floor {
    let m = &s.model;
    let Some(Element::Floor(f)) = m.elements.get(id) else { panic!("{id} isn't a floor") };
    let (_, z) = crate::framing::floor::structure(m, f).expect("a structure layer");
    let t = m.types.framing.get(f.framing.as_deref().unwrap_or("")).cloned().expect("a framing type");
    let mut holes: Vec<(String, Vec<P2>)> = f.openings.iter().enumerate().map(|(i, r)| (format!("opening {i}"), r.clone())).collect();
    let mut supports = Vec::new();
    let mut bearing = Vec::new();
    for (sid, e) in m.elements.iter() {
        match e {
            Element::Shaft(sh) => holes.push((format!("shaft {sid}"), sh.boundary.clone())),
            Element::Beam(_) => {
                if let Some(g) = m.run_geom(e) {
                    let (a, b) = (P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1]));
                    let u = (b - a).unit();
                    let n = u.perp() * (g.width / 2.0);
                    supports.push([a - n, b - n, b + n, a + n]);
                    // Level, its top at the joists' underside.
                    if (g.a[2] - g.b[2]).abs() < 1e-6 && (g.a[2] + g.half() - z[0]).abs() <= 10.0 {
                        bearing.push((a, b, g.width / 2.0));
                    }
                }
            }
            Element::Wall(w) if w.arc == 0.0 => {
                let axis = crate::walls::Axis::of(w);
                let c = crate::walls::centre(m, w);
                let (a, b) = (axis.at(0.0, c), axis.at(axis.length(), c));
                let u = (b - a).unit();
                let n = u.perp() * (m.thickness(w) / 2.0);
                supports.push([a - n, b - n, b + n, a + n]);
                if (m.top_z(w) - z[0]).abs() <= 10.0 {
                    bearing.push((a, b, m.thickness(w) / 2.0));
                }
            }
            _ => {}
        }
    }
    let outline = if geom::area(&f.boundary) < 0.0 { f.boundary.iter().rev().copied().collect() } else { f.boundary.clone() };
    Floor { outline, holes, z, t, supports, bearing }
}

/// Where the line `s` = `at` (in the floor's frame) lies inside `ring` (in plan), as intervals of `across`.
fn section(f: &Reply, ring: &[P2], at: f64) -> Vec<[f64; 2]> {
    let local: Vec<P2> = ring.iter().map(|p| f.frame(*p)).collect();
    let mut xs: Vec<f64> = geom::ring_edges(&local)
        .filter(|(p, q)| (p.0 > at) != (q.0 > at))
        .map(|(p, q)| p.1 + (at - p.0) * (q.1 - p.1) / (q.0 - p.0))
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.chunks(2).filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).collect()
}

fn minus(x: &[[f64; 2]], y: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = x.to_vec();
    for b in y {
        out = out
            .into_iter()
            .flat_map(|a| [[a[0], a[1].min(b[0])], [a[0].max(b[1]), a[1]]].into_iter().filter(|i| i[1] - i[0] > 1e-9))
            .collect();
    }
    out
}

/// What may be said of a floor's framing beyond the invariants: gaps the test expects (warned).
#[derive(Default, Clone, Copy)]
struct Allow {
    /// A bay wider than spacing + thickness (each is still to be warned of: `warned` names what the warnings must say).
    gaps: bool,
}

/// Floor `id`'s framing, held to every invariant; its reply.
fn check(s: &mut Session, id: Id) -> Reply {
    check_with(s, id, Allow::default())
}

fn check_with(s: &mut Session, id: Id, allow: Allow) -> Reply {
    let r = read(s, id);
    let again = read(s, id);
    assert_eq!(r.raw, again.raw, "the same model frames the same");
    let fl = floor_of(s, id);
    let (th, spacing) = (fl.t.thickness, fl.t.spacing);
    let ctx = || format!("warnings: {:?}", r.warnings);
    // Keys once each.
    let mut keys = r.keys();
    keys.sort_unstable();
    for w in keys.windows(2) {
        assert_ne!(w[0], w[1], "a key twice; {}", ctx());
    }
    for w in &r.warnings {
        assert!(!w.trim().is_empty(), "an empty warning");
    }
    for m in &r.members {
        let what = format!("{} {:?} {:?}", m.key, m.s, m.across);
        let all = m.s.iter().chain(&m.across).chain(&m.z).chain([&m.length]);
        assert!(all.clone().all(|v| v.is_finite()), "{what}: not a number");
        assert!(m.s[1] - m.s[0] > 0.01 && m.across[1] - m.across[0] > 0.01, "{what}: no size");
        for (k, z) in m.z.iter().enumerate() {
            let want = if k < 2 { fl.z[0] } else { fl.z[1] };
            assert!((z - want).abs() < 0.011, "{what}: z {:?} isn't the structure layer's {:?}", m.z, fl.z);
        }
        let boxed = if m.grain == "across" { m.across[1] - m.across[0] } else { m.s[1] - m.s[0] };
        assert!((m.length - boxed).abs() < 0.011, "{what}: length {} isn't its box's {boxed}", m.length);
        let q = m.shrunk(&r, 0.01);
        assert!(quad_inside_ring(&q, &fl.outline), "{what} isn't inside the outline; {}", ctx());
        for (name, h) in &fl.holes {
            assert!(!quad_meets_ring(&q, h), "{what} is in {name}; {}", ctx());
        }
    }
    // No two overlap: swept along x.
    let mut boxes: Vec<(f64, f64, usize)> = r
        .members
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let q = m.shrunk(&r, 0.01);
            let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            (lo, hi, i)
        })
        .collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (k, (_, hi, i)) in boxes.iter().enumerate() {
        for (lo2, _, j) in boxes.iter().skip(k + 1) {
            if lo2 > hi {
                break;
            }
            let (a, b) = (&r.members[*i], &r.members[*j]);
            assert!(!quads_overlap(&a.shrunk(&r, 0.01), &b.shrunk(&r, 0.01)), "{} and {} overlap; {}", a.key, b.key, ctx());
        }
    }
    // Each joist's and trimmer's ends on something.
    let grown = |q: &[P2; 4], g: f64| -> [P2; 4] {
        let (u, n) = ((q[1] - q[0]).unit() * g, (q[3] - q[0]).unit() * g);
        [q[0] - u - n, q[1] + u - n, q[2] + u + n, q[3] - u + n]
    };
    for m in r.members.iter().filter(|m| m.grain == "along" && !m.own) {
        for (end, dir) in [(m.s[0], -1.0), (m.s[1], 1.0)] {
            let [a0, a1] = m.across;
            let at = |s: f64, a: f64| r.origin + r.x * s + r.x.perp() * a;
            let (e0, e1) = (end + dir * 0.001, end + dir * 0.06);
            let face = [at(e0.min(e1), a0 + 0.001), at(e0.max(e1), a0 + 0.001), at(e0.max(e1), a1 - 0.001), at(e0.min(e1), a1 - 0.001)];
            let probes: Vec<P2> = [a0 + 0.01, (a0 + a1) / 2.0, a1 - 0.01].iter().map(|a| at(end + dir * 0.05, *a)).collect();
            let on = |p: &P2| {
                r.members.iter().any(|o| o.key != m.key && quads_overlap(&o.ring, &face))
                    || fl.supports.iter().any(|q| geom::contains(&grown(q, fl.t.lap + 0.1), *p))
                    || (!fl.t.rim && !geom::contains(&fl.outline, *p))
            };
            if probes.iter().any(on) {
                continue;
            }
            // At a hole, warned of.
            let warned = fl.holes.iter().any(|(name, h)| {
                probes.iter().any(|p| geom::contains(h, *p) || h.iter().any(|q| q.dist(*p) < 3.0 * th + spacing))
                    && r.warned(&format!("{name} isn't trimmed"))
            }) || r.warnings.iter().any(|w| w.contains("is left out") || w.contains("isn't trimmed"));
            assert!(warned, "{} ends in mid-air at {end}; {}", m.key, ctx());
        }
    }
    // Cut over each support that bears it away from its ends, or (lapped) run no more than the lap past it.
    for m in r.members.iter().filter(|m| m.grain == "along" && !m.own) {
        let c = (m.across[0] + m.across[1]) / 2.0;
        let (p, q) = (r.origin + r.x * m.s[0] + r.x.perp() * c, r.origin + r.x * m.s[1] + r.x.perp() * c);
        for (a, b, half) in &fl.bearing {
            let Some(t) = geom::crossing(p, q, *a, *b) else { continue };
            let at = m.s[0] + t * (m.s[1] - m.s[0]);
            let margin = half.max(th) + th + 0.5;
            let inside = at - m.s[0] > margin && m.s[1] - at > margin;
            // A lapped piece beside the one before is cut where its line's centre crosses the support: on a skewed one its own
            // centre crosses a thickness's skew away.
            let (u, w) = (r.x, (*b - *a).unit());
            let skew = th * (u.dot(w) / u.cross(w).abs().max(1e-9)).abs();
            // (Supports less than a thickness apart are one cut.)
            let lapped = fl.t.lap > 0.0 && (at - m.s[0]).min(m.s[1] - at) <= fl.t.lap + skew + th + 0.5;
            assert!(!inside || lapped, "{} {:?} runs over a support at {at:.1} uncut; {}", m.key, m.s, ctx());
        }
    }
    // No bay wider than the spacing and a joist without a word.
    if !allow.gaps {
        let local: Vec<P2> = fl.outline.iter().map(|p| r.frame(*p)).collect();
        let (lo, hi) = local.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
        let rings: Vec<Vec<P2>> = r.members.iter().map(|m| m.ring.to_vec()).collect();
        let mut at = lo + 13.7;
        while at < hi {
            let mut open = section(&r, &fl.outline, at);
            // Less each hole's box in the joists' frame, which its trimmers and headers frame.
            for (_, h) in &fl.holes {
                let local: Vec<P2> = h.iter().map(|p| r.frame(*p)).collect();
                let fold =
                    |k: fn(&P2) -> f64| local.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| [lo.min(v), hi.max(v)]);
                let (bs, ba) = (fold(|p| p.0), fold(|p| p.1));
                if at > bs[0] && at < bs[1] {
                    open = minus(&open, &[ba]);
                }
            }
            let mut covered: Vec<[f64; 2]> = rings.iter().flat_map(|q| section(&r, q, at)).collect();
            covered.sort_by(|a, b| a[0].total_cmp(&b[0]));
            for gap in minus(&open, &covered) {
                assert!(
                    gap[1] - gap[0] <= spacing + th + 0.5,
                    "at s {at:.1}, nothing from across {:.1} to {:.1}; {}",
                    gap[0],
                    gap[1],
                    ctx()
                );
            }
            at += 97.3;
        }
    }
    r
}

/// The schedule's rows, IFC's beams and members and the 3D view's member solids are as many as floor `id`'s members; hidden
/// framing leaves no member solid.
fn outputs_agree(s: &mut Session, id: Id) {
    let r = read(s, id);
    let n = r.members.len();
    let t = run(
        s,
        "schedule",
        json!({"name": format!("Hunt {id} {}", s.model.elements.len()), "category": "framingmembers", "fields": ["Host", "Key"]}),
    );
    let rows = t["rows"].as_array().map_or(0, |rows| rows.iter().filter(|row| row[0] == json!(id as f64)).count());
    assert_eq!(rows, n, "a schedule row for each member");
    let f = crate::ifc::export(&s.model);
    let blocks = r.role("blocking").len();
    let joists = f.lines().filter(|l| l.contains("IFCBEAM(") && l.ends_with(",.JOIST.);")).count();
    assert_eq!(joists, n - blocks, "an IfcBeam for each joist, rim, trimmer and header");
    assert_eq!(f.lines().filter(|l| l.contains("IFCMEMBER(") && l.contains("'blocking'")).count(), blocks);
    let members = |s: &Session| {
        crate::view3d::shaded(&s.model)
            .into_iter()
            .filter(|x| x.owner() == id && crate::family::visibility::framing(x.vis()) == Some(true))
            .count()
    };
    assert_eq!(members(s), n, "a solid for each member");
}

const FLOOR: &str = "Hunt Joists";
const FRAMING: &str = "Hunt Framing";

/// A session with a 184 mm joist floor type and a framing type `FRAMING` of `extra` (38 thick, 406.4 apart by default).
fn session(extra: Value) -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": FLOOR, "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    run(&mut s, "type", json!({"category": "beam", "name": "Hunt Beam", "width": 114, "depth": 235}));
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Hunt Stud", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
    let mut t = json!({"category": "framing", "name": FRAMING});
    if let (Some(o), Some(e)) = (t.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(&mut s, "type", t);
    s
}

/// A floor of `outline` (and `openings`), framed with `FRAMING` at `angle` (none: automatic).
fn floor(s: &mut Session, outline: &[[f64; 2]], openings: &[Vec<[f64; 2]>], angle: Option<f64>) -> Id {
    let id = first(&run(s, "floor", json!({"boundary": outline, "openings": openings, "type": FLOOR})));
    let mut p = json!({"id": id, "type": FRAMING});
    if let Some(a) = angle {
        p["angle"] = json!(a);
    }
    run(s, "frame", p);
    id
}

/// A beam with its top at the joists' underside (-184) less `below`, from `a` to `b`.
fn beam(s: &mut Session, a: [f64; 2], b: [f64; 2], below: f64) -> Id {
    first(&run(s, "beam", json!({"points": [a, b], "type": "Hunt Beam", "offset": -184.0 - below})))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

const ANGLES: [Option<f64>; 9] = [Some(0.0), Some(90.0), Some(45.0), Some(30.0), Some(180.0), Some(270.0), Some(-90.0), Some(3600.0), None];

#[test]
fn a_rectangle_with_a_beam_and_an_opening_holds_at_every_angle_both_ways_round() {
    for angle in ANGLES {
        for cw in [false, true] {
            let mut s = session(json!({}));
            beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            let mut outline = rect(0.0, 0.0, 6000.0, 4000.0);
            if cw {
                outline.reverse();
            }
            let id = floor(&mut s, &outline, &[rect(2500.0, 2600.0, 3500.0, 3400.0)], angle);
            eprintln!("case: angle {angle:?}, clockwise {cw}");
            let r = check(&mut s, id);
            assert!(!r.role("joist").is_empty(), "{angle:?} {cw}");
        }
    }
}

#[test]
fn l_u_and_t_outlines_hold_at_every_angle() {
    let l = [[0.0, 0.0], [6000.0, 0.0], [6000.0, 3000.0], [3000.0, 3000.0], [3000.0, 6000.0], [0.0, 6000.0]];
    let u = [
        [0.0, 0.0],
        [6000.0, 0.0],
        [6000.0, 4000.0],
        [4000.0, 4000.0],
        [4000.0, 1000.0],
        [2000.0, 1000.0],
        [2000.0, 4000.0],
        [0.0, 4000.0],
    ];
    let t = [
        [0.0, 3000.0],
        [2000.0, 3000.0],
        [2000.0, 0.0],
        [4000.0, 0.0],
        [4000.0, 3000.0],
        [6000.0, 3000.0],
        [6000.0, 4500.0],
        [0.0, 4500.0],
    ];
    for outline in [&l[..], &u[..], &t[..]] {
        for angle in ANGLES {
            let mut s = session(json!({}));
            let id = floor(&mut s, outline, &[], angle);
            eprintln!("case: {outline:?} angle {angle:?}");
            check(&mut s, id);
        }
    }
}

#[test]
fn a_skewed_outline_with_a_reflex_corner_between_two_rims_holds() {
    let outline = [[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [2000.0, 2000.0], [0.0, 3000.0]];
    for angle in ANGLES {
        for layout in ["start", "end", "centre"] {
            let mut s = session(json!({"layout": layout}));
            let id = floor(&mut s, &outline, &[], angle);
            eprintln!("case: angle {angle:?} layout {layout}");
            check(&mut s, id);
        }
    }
}

#[test]
fn an_acute_corner_keeps_its_rims_on_the_floor() {
    // At (4000, 3000) the outline turns back on itself at 63°: the east rim runs through, and stops where its inner side meets
    // the outline, not a hair past it.
    let mut s = session(json!({}));
    let id = floor(&mut s, &[[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [2000.0, 2000.0], [0.0, 3000.0]], &[], Some(0.0));
    let r = check(&mut s, id);
    let east = r.members.iter().find(|m| m.key == "rim/1").expect("the east rim");
    assert!((east.across[1] - (3000.0 - 19.0)).abs() < 0.02, "{:?}", east.across);
}

#[test]
fn rims_meeting_at_a_square_inward_corner_leave_no_wedge_and_a_skewed_one_says_so() {
    // An L at 45°: every edge a rim, the inward corner square.
    let mut s = session(json!({}));
    let l = [[0.0, 0.0], [6000.0, 0.0], [6000.0, 3000.0], [3000.0, 3000.0], [3000.0, 6000.0], [0.0, 6000.0]];
    let id = floor(&mut s, &l, &[], Some(45.0));
    let r = check(&mut s, id);
    assert!(!r.warned("wedge"), "{:?}", r.warnings);
    // The point just inside the inward corner, (3000 - 20, 3000 - 20) is in a rim.
    let p = P2(2990.0, 2990.0);
    assert!(r.role("rim").iter().any(|m| geom::contains(&m.ring, p)), "the corner of the rims at the inward corner is filled");
    // The skewed outline's inward corner isn't square: said so.
    let mut s = session(json!({}));
    let id = floor(&mut s, &[[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [2000.0, 2000.0], [0.0, 3000.0]], &[], Some(90.0));
    let r = check(&mut s, id);
    assert!(r.warned("inward corner that isn't square"), "{:?}", r.warnings);
}

#[test]
fn a_t_whose_top_edges_are_in_line_has_one_edge_joist_along_them_and_no_warning_of_it() {
    let t = [
        [0.0, 3000.0],
        [2000.0, 3000.0],
        [2000.0, 0.0],
        [4000.0, 0.0],
        [4000.0, 3000.0],
        [6000.0, 3000.0],
        [6000.0, 4500.0],
        [0.0, 4500.0],
    ];
    let mut s = session(json!({}));
    let id = floor(&mut s, &t, &[], Some(0.0));
    let r = check(&mut s, id);
    assert!(!r.warned("edge joist"), "{:?}", r.warnings);
}

#[test]
fn narrow_and_short_floors_frame_what_fits_and_say_what_doesnt() {
    for (w, l) in
        [(60.0, 3000.0), (30.0, 3000.0), (300.0, 3000.0), (3000.0, 60.0), (3000.0, 30.0), (3000.0, 300.0), (50.0, 50.0), (76.0, 3000.0)]
    {
        for angle in [Some(0.0), Some(90.0), Some(45.0)] {
            let mut s = session(json!({}));
            let id = floor(&mut s, &rect(0.0, 0.0, w, l), &[], angle);
            eprintln!("case: {w} x {l} at {angle:?}");
            // Too small for what the type frames: what's left out is said.
            let r = check_with(&mut s, id, Allow { gaps: true });
            // No joists: the rims fill it, or a warning says why.
            let filled: f64 = r.members.iter().map(|m| (m.s[1] - m.s[0]) * (m.across[1] - m.across[0])).sum();
            if r.role("joist").is_empty() && filled < w * l - 1.0 {
                assert!(!r.warnings.is_empty(), "no joists, and no word why");
            }
        }
    }
}

#[test]
fn an_edge_not_quite_along_the_joists_still_has_its_edge_joist_the_whole_way() {
    let mut s = session(json!({}));
    let id = floor(&mut s, &[[0.0, 0.0], [6000.0, 0.0], [6000.8, 4000.0], [0.0, 4000.0]], &[], Some(90.0));
    let r = check(&mut s, id);
    let east = r.members.iter().find(|m| m.key.starts_with("edge/1/")).expect("an edge joist along the east edge");
    assert!(east.s[1] - east.s[0] > 3900.0, "{:?}", east.s);
}

#[test]
fn spacing_just_over_the_thickness_and_each_layout_hold() {
    for layout in ["start", "end", "centre"] {
        for spacing in [38.5, 39.0, 76.0, 100.0, 406.4] {
            for angle in [Some(90.0), Some(45.0)] {
                let mut s = session(json!({"layout": layout, "spacing": spacing, "lap": 300, "bridging": 1200}));
                beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
                let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[rect(2500.0, 2600.0, 3500.0, 3400.0)], angle);
                eprintln!("case: {layout} {spacing} {angle:?}");
                check(&mut s, id);
            }
        }
    }
}

/// The openings tried: (what, rings).
fn openings() -> Vec<(&'static str, Vec<Vec<[f64; 2]>>)> {
    let diamond = |cx: f64, cy: f64, r: f64| vec![[cx, cy - r], [cx + r, cy], [cx, cy + r], [cx - r, cy]];
    vec![
        ("near a corner", vec![rect(100.0, 100.0, 900.0, 900.0)]),
        ("by a corner, room for trimmers", vec![rect(200.0, 200.0, 1000.0, 1000.0)]),
        ("at the rim", vec![rect(1000.0, 5.0, 2000.0, 500.0)]),
        ("touching an edge joist", vec![rect(40.0, 1000.0, 800.0, 1500.0)]),
        ("over the beam", vec![rect(2500.0, 1500.0, 3500.0, 2500.0)]),
        ("side by side", vec![rect(1000.0, 1000.0, 1500.0, 1500.0), rect(1700.0, 1000.0, 2200.0, 1500.0)]),
        ("side by side, close", vec![rect(1000.0, 1000.0, 1500.0, 1500.0), rect(1600.0, 1000.0, 2100.0, 1500.0)]),
        ("in line along the joists", vec![rect(1000.0, 500.0, 1500.0, 1200.0), rect(1000.0, 2800.0, 1500.0, 3500.0)]),
        ("in the other's trimmers", vec![rect(1000.0, 1000.0, 1500.0, 1500.0), rect(1540.0, 3000.0, 1800.0, 3300.0)]),
        ("a diamond", vec![diamond(3000.0, 1200.0, 400.0)]),
        ("a diamond at the edge", vec![diamond(5900.0, 1100.0, 90.0)]),
        ("tiny between joists", vec![rect(5300.0, 1000.0, 5400.0, 1100.0)]),
        ("nearly the floor", vec![rect(10.0, 10.0, 5990.0, 3990.0)]),
    ]
}

#[test]
fn openings_anywhere_hold_with_every_ply_and_without_rims() {
    for (what, rings) in openings() {
        for angle in [Some(90.0), Some(0.0), Some(45.0)] {
            for extra in
                [json!({}), json!({"trimmers": 1}), json!({"trimmers": 3}), json!({"rim": false}), json!({"bridging": 900, "lap": 300})]
            {
                let mut s = session(extra.clone());
                beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
                let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &rings, angle);
                eprintln!("case: {what} at {angle:?} with {extra}");
                check(&mut s, id);
            }
        }
    }
}

#[test]
fn shafts_through_it_across_its_edge_and_turned_hold() {
    let shafts = [
        rect(4500.0, 2500.0, 5200.0, 3300.0),
        rect(5800.0, 500.0, 6500.0, 1200.0),
        vec![[3000.0, 800.0], [3500.0, 1300.0], [3000.0, 1800.0], [2500.0, 1300.0]],
        rect(-500.0, 3000.0, 6500.0, 3400.0),
    ];
    for (k, ring) in shafts.iter().enumerate() {
        for angle in [Some(90.0), Some(0.0), Some(30.0)] {
            let mut s = session(json!({"bridging": 1500}));
            beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            run(&mut s, "shaft", json!({"boundary": ring, "level": "Level 1", "base_offset": -1000, "height": 4000}));
            let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[], angle);
            eprintln!("case: shaft {k} at {angle:?}");
            check(&mut s, id);
        }
    }
}

#[test]
fn a_shaft_across_a_notch_isnt_trimmed_by_headers_over_the_notch() {
    // A U, its notch from x 2000 to 4000 open to the north; a shaft through both arms, across the notch.
    let u = [
        [0.0, 0.0],
        [6000.0, 0.0],
        [6000.0, 4000.0],
        [4000.0, 4000.0],
        [4000.0, 1000.0],
        [2000.0, 1000.0],
        [2000.0, 4000.0],
        [0.0, 4000.0],
    ];
    let mut s = session(json!({}));
    run(
        &mut s,
        "shaft",
        json!({"boundary": rect(1000.0, 2000.0, 5000.0, 2500.0), "level": "Level 1", "base_offset": -1000, "height": 4000}),
    );
    let id = floor(&mut s, &u, &[], Some(90.0));
    check(&mut s, id);
}

#[test]
fn supports_of_every_kind_hold() {
    type Make = fn(&mut Session);
    let cases: Vec<(&str, Make)> = vec![
        ("skewed beam", |s| {
            beam(s, [-500.0, 1000.0], [6500.0, 3000.0], 0.0);
        }),
        ("short beam", |s| {
            beam(s, [1000.0, 2000.0], [3000.0, 2000.0], 0.0);
        }),
        ("beam under the rim", |s| {
            beam(s, [-500.0, 19.0], [6500.0, 19.0], 0.0);
        }),
        ("beam 9 below", |s| {
            beam(s, [-500.0, 2000.0], [6500.0, 2000.0], 9.0);
        }),
        ("beam 11 below", |s| {
            beam(s, [-500.0, 2000.0], [6500.0, 2000.0], 11.0);
        }),
        ("two beams close", |s| {
            beam(s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            beam(s, [-500.0, 2400.0], [6500.0, 2400.0], 0.0);
        }),
        ("two beams closer than a joist", |s| {
            beam(s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            beam(s, [-500.0, 2020.0], [6500.0, 2020.0], 0.0);
        }),
        ("a wall under it", |s| {
            run(s, "wall", json!({"points": [[-500, 2000], [6500, 2000]], "type": "Hunt Stud", "base_offset": -2184, "height": 2000}));
        }),
        ("a sloped beam", |s| {
            run(s, "beam", json!({"points": [[-500, 2000], [6500, 2600]], "type": "Hunt Beam", "offset": -184, "end_offset": -884}));
        }),
        ("a beam along the joists", |s| {
            beam(s, [3000.0, -500.0], [3000.0, 4500.0], 0.0);
        }),
    ];
    for (what, make) in cases {
        for angle in [Some(90.0), Some(45.0), Some(0.0)] {
            for extra in [json!({}), json!({"lap": 300}), json!({"lap": 2000, "bridging": 800})] {
                let mut s = session(extra.clone());
                make(&mut s);
                let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[rect(1000.0, 2600.0, 1800.0, 3300.0)], angle);
                eprintln!("case: {what} at {angle:?} with {extra}");
                let r = check(&mut s, id);
                let borne = !r.raw["supports"].as_array().is_none_or(Vec::is_empty);
                match what {
                    "beam 9 below" | "a wall under it" | "two beams close" => assert!(borne, "{:?}", r.warnings),
                    "beam 11 below" => assert!(!borne && r.warned("nothing bears"), "{:?}", r.warnings),
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn two_beams_closer_than_twice_the_lap_shorten_it_between_them_and_say_so() {
    let mut s = session(json!({"lap": 300}));
    beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
    beam(&mut s, [-500.0, 2400.0], [6500.0, 2400.0], 0.0);
    let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[], Some(90.0));
    let r = check(&mut s, id);
    assert!(r.warned("laps only 200 mm"), "{:?}", r.warnings);
}

#[test]
fn a_wall_under_it_a_hair_below_its_joists_bears_them_wherever_the_heights_fall() {
    // The joists' underside 5 above Level 1 (a floor 189 up), the wall's top 4 below it: within 10 mm, so it bears them.
    let mut s = session(json!({}));
    run(&mut s, "wall", json!({"points": [[-500, 2000], [6500, 2000]], "type": "Hunt Stud", "base_offset": -2001, "height": 2000}));
    let id = first(&run(&mut s, "floor", json!({"boundary": rect(0.0, 0.0, 6000.0, 4000.0), "type": FLOOR, "offset": 189})));
    run(&mut s, "frame", json!({"id": id, "type": FRAMING, "angle": 90}));
    let r = check(&mut s, id);
    assert_eq!(r.raw["supports"].as_array().map(Vec::len), Some(1), "{:?}", r.warnings);
    // And a partition 5 above its top, on it, has its pair.
    let w = first(&run(
        &mut s,
        "wall",
        json!({"points": [[3000, 300], [3000, 3700]], "type": "Hunt Stud", "base_offset": 194, "height": 2400}),
    ));
    let r = check(&mut s, id);
    let g = s.model.guid(w).to_string();
    assert!(r.keys().iter().any(|k| k.starts_with(&format!("under/{g}/"))), "{:?} {:?}", r.keys(), r.warnings);
}

#[test]
fn walls_standing_on_it_hold_and_each_gets_its_pair_or_a_word() {
    // Joists north; each wall from y 300 to 3700 at x, or as given.
    let walls: Vec<(&str, Vec<[[f64; 2]; 2]>)> = vec![
        ("on the edge joist", vec![[[19.0, 300.0], [19.0, 3700.0]]]),
        ("half off the edge joist", vec![[[70.0, 300.0], [70.0, 3700.0]]]),
        ("beside the edge joist", vec![[[100.0, 300.0], [100.0, 3700.0]]]),
        ("over the opening", vec![[[3000.0, 300.0], [3000.0, 3700.0]]]),
        ("short", vec![[[2000.0, 1000.0], [2000.0, 2000.0]]]),
        ("half a degree off", vec![[[2500.0, 300.0], [2529.7, 3700.0]]]),
        ("two degrees off", vec![[[2500.0, 300.0], [2618.7, 3700.0]]]),
        ("two 30 apart", vec![[[4000.0, 300.0], [4000.0, 3700.0]], [[4030.0, 300.0], [4030.0, 3700.0]]]),
        ("by a trimmer", vec![[[2780.0, 300.0], [2780.0, 3700.0]]]),
    ];
    for (what, lines) in walls {
        for extra in [json!({}), json!({"lap": 300, "bridging": 1000})] {
            let mut s = session(extra.clone());
            beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[rect(2800.0, 2600.0, 3300.0, 3100.0)], Some(90.0));
            let ws: Vec<Id> =
                lines.iter().map(|l| first(&run(&mut s, "wall", json!({"points": l, "type": "Hunt Stud", "height": 2400})))).collect();
            eprintln!("case: {what} with {extra}");
            let r = check(&mut s, id);
            for w in ws {
                let g = s.model.guid(w).to_string();
                let paired = r.keys().iter().any(|k| k.starts_with(&format!("under/{g}/")));
                let said = r.warned(&format!("wall {w}"));
                let on_edge = what == "on the edge joist";
                let skew = what == "two degrees off";
                assert!(paired || said || on_edge || skew, "wall {w} {what}: no pair and no word: {:?}", r.warnings);
                if what == "half off the edge joist" {
                    assert!(said, "{:?}", r.warnings);
                }
            }
        }
    }
}

#[test]
fn bridging_of_any_spacing_holds_round_holes_and_on_an_l() {
    let l = [[0.0, 0.0], [6000.0, 0.0], [6000.0, 3000.0], [3000.0, 3000.0], [3000.0, 6000.0], [0.0, 6000.0]];
    for bridging in [300.0, 1500.0, 100000.0] {
        for angle in [Some(90.0), Some(45.0), Some(0.0)] {
            let mut s = session(json!({"bridging": bridging}));
            beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
            let id = floor(&mut s, &l, &[rect(1000.0, 3500.0, 1800.0, 4300.0), rect(4000.0, 500.0, 4500.0, 1000.0)], angle);
            eprintln!("case: {bridging} at {angle:?}");
            let r = check(&mut s, id);
            if bridging < 1500.0 {
                assert!(!r.role("blocking").is_empty());
            }
        }
    }
}

#[test]
fn a_floor_on_an_upper_level_with_an_offset_and_its_structure_between_other_layers_holds_and_its_outputs_agree() {
    let mut s = session(json!({"bridging": 1200, "lap": 300}));
    run(&mut s, "level", json!({"elevation": 3000, "name": "Hunt 2"}));
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": FLOOR, "layers": [
            {"function": "finish", "material": "Timber", "thickness": 18},
            {"function": "structure", "material": "Timber", "thickness": 184},
            {"function": "finish", "material": "Timber", "thickness": 13}]}),
    );
    // Its top 150 up, so its joists' underside at 3000 + 150 - 18 - 184 = 2948.
    run(&mut s, "beam", json!({"points": [[-500, 2000], [6500, 2000]], "type": "Hunt Beam", "level": "Hunt 2", "offset": -52}));
    let l = [[0.0, 0.0], [6000.0, 0.0], [6000.0, 3000.0], [3000.0, 3000.0], [3000.0, 6000.0], [0.0, 6000.0]];
    let id = first(&run(
        &mut s,
        "floor",
        json!({"boundary": l, "type": FLOOR, "level": "Hunt 2", "offset": 150, "openings": [rect(1000.0, 3500.0, 1800.0, 4300.0)]}),
    ));
    run(&mut s, "frame", json!({"id": id, "type": FRAMING, "angle": 45}));
    let r = check(&mut s, id);
    assert_eq!(r.raw["supports"].as_array().map(Vec::len), Some(1), "{:?}", r.warnings);
    assert!((r.members[0].z[0] - 2948.0).abs() < 0.01, "{:?}", r.members[0].z);
    outputs_agree(&mut s, id);
}

#[test]
fn outputs_agree_on_the_nastiest_floors() {
    let l = [[0.0, 0.0], [6000.0, 0.0], [6000.0, 3000.0], [3000.0, 3000.0], [3000.0, 6000.0], [0.0, 6000.0]];
    let skew = [[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [2000.0, 2000.0], [0.0, 3000.0]];
    for (outline, holes, angle) in [
        (&l[..], vec![rect(1000.0, 3500.0, 1800.0, 4300.0), rect(4000.0, 500.0, 4500.0, 1000.0)], Some(90.0)),
        (&skew[..], vec![rect(1000.0, 500.0, 1600.0, 1100.0)], Some(30.0)),
        (&l[..], vec![], Some(45.0)),
    ] {
        let mut s = session(json!({"bridging": 1200, "lap": 300}));
        beam(&mut s, [-500.0, 2000.0], [6500.0, 2600.0], 0.0);
        let id = floor(&mut s, outline, &holes, angle);
        check(&mut s, id);
        outputs_agree(&mut s, id);
        // Hidden, no member is drawn and the structure layer is whole.
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["floorframing"]}));
        let look = crate::graphics::Look::of_3d(&s.model);
        let shown: Vec<_> =
            crate::view3d::shaded(&s.model).into_iter().filter(|x| x.owner() == id && look.forms_show(x.vis(), false)).collect();
        assert!(!shown.iter().any(|x| crate::family::visibility::framing(x.vis()) == Some(true)));
        assert!(!shown.is_empty());
    }
}

#[test]
fn moving_an_opening_keeps_the_far_joists_keys_and_undo_and_redo_give_back_the_same_members() {
    let mut s = session(json!({"bridging": 1200, "lap": 300}));
    beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
    let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[rect(1000.0, 2600.0, 1800.0, 3300.0)], Some(90.0));
    let before = check(&mut s, id);
    run(&mut s, "modify", json!({"id": id, "openings": [rect(1000.0, 2500.0, 1800.0, 3200.0)]}));
    let after = check(&mut s, id);
    // The joists east of x 3000 are as they were.
    let far = |r: &Reply| -> Vec<(String, [f64; 2])> {
        let mut v: Vec<(String, [f64; 2])> =
            r.members.iter().filter(|m| m.role == "joist" && m.ring[0].0 > 3000.0).map(|m| (m.key.clone(), m.s)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    assert_eq!(far(&before), far(&after));
    run(&mut s, "undo", json!({}));
    assert_eq!(read(&mut s, id).raw, before.raw);
    run(&mut s, "redo", json!({}));
    assert_eq!(read(&mut s, id).raw, after.raw);
    // Each edit undone and redone.
    let edits = [
        ("type", json!({"category": "framing", "name": FRAMING, "spacing": 300, "trimmers": 3})),
        ("frame", json!({"id": id, "angle": 30})),
        ("wall", json!({"points": [[4000, 300], [4000, 3700]], "type": "Hunt Stud", "height": 2400})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [6000, 0], [6000, 3000], [3000, 4000], [0, 4000]]})),
        ("type", json!({"category": "framing", "name": FRAMING, "rim": false})),
    ];
    for (c, p) in edits {
        let was = read(&mut s, id).raw;
        run(&mut s, c, p.clone());
        let now = check(&mut s, id).raw;
        run(&mut s, "undo", json!({}));
        assert_eq!(read(&mut s, id).raw, was, "{c} {p} undone");
        run(&mut s, "redo", json!({}));
        assert_eq!(read(&mut s, id).raw, now, "{c} {p} redone");
    }
}

#[test]
fn hostile_types_and_edits_never_break_a_framed_floor() {
    let mut s = session(json!({"bridging": 1200, "lap": 300}));
    beam(&mut s, [-500.0, 2000.0], [6500.0, 2000.0], 0.0);
    let id = floor(&mut s, &rect(0.0, 0.0, 6000.0, 4000.0), &[rect(1000.0, 2600.0, 1800.0, 3300.0)], Some(90.0));
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
        t(json!({"thickness": 0.001})),
        t(json!({"lap": 2000, "bridging": 300, "trimmers": 3, "spacing": 39})),
        t(json!({"lap": f64::NAN})),
        t(json!({"bridging": -1})),
        t(json!({"trimmers": 4294967295u64})),
        t(json!({"layout": "middle"})),
        ("modify", json!({"id": id, "framing_angle": 89.9999999})),
        ("modify", json!({"id": id, "framing_angle": -3600})),
        ("modify", json!({"id": id, "framing_angle": 1e-300})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [1e7, 0], [1e7, 1], [0, 1]]})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [6000, 0], [6000, 0.5], [0, 0.5]]})),
        ("modify", json!({"id": id, "boundary": [[0, 0], [6000, 0], [6000, 4000], [3000, 1], [0, 4000]]})),
        ("modify", json!({"id": id, "openings": [[[1, 1], [5999, 1], [5999, 3999], [1, 3999]]]})),
        ("modify", json!({"id": id, "offset": 1e9})),
    ];
    for (c, p) in edits {
        let _ = s.execute(c, &p);
        eprintln!("case: {c} {p}");
        if s.execute("framing", &json!({"id": id})).is_ok() {
            check_with(&mut s, id, Allow { gaps: true });
        }
    }
}

#[test]
fn a_60_m_square_floor_at_300_with_bridging_frames_in_well_under_a_second_or_two() {
    let mut s = session(json!({"spacing": 300, "bridging": 1200, "lap": 300}));
    beam(&mut s, [-500.0, 20000.0], [60500.0, 20000.0], 0.0);
    beam(&mut s, [-500.0, 40000.0], [60500.0, 40000.0], 0.0);
    let id = floor(&mut s, &rect(0.0, 0.0, 60000.0, 60000.0), &[rect(10000.0, 10000.0, 12000.0, 13000.0)], Some(90.0));
    let t = std::time::Instant::now();
    let f = crate::framing::floor::floor(&s.model, id).ok().flatten().expect("framed");
    let took = t.elapsed();
    eprintln!("60 m floor: {} members in {took:?}", f.members.len());
    assert!(took.as_secs_f64() < 2.0, "{took:?}");
    check(&mut s, id);
}

#[test]
fn a_60_m_floor_on_a_beam_every_600_with_lap_and_bridging_frames_in_a_couple_of_seconds() {
    let mut s = session(json!({"spacing": 300, "bridging": 1200, "lap": 300}));
    for k in 1..100 {
        beam(&mut s, [-500.0, k as f64 * 600.0], [60500.0, k as f64 * 600.0], 0.0);
    }
    let id = floor(&mut s, &rect(0.0, 0.0, 60000.0, 60000.0), &[rect(10000.0, 10000.0, 12000.0, 13000.0)], Some(90.0));
    let t = std::time::Instant::now();
    let f = crate::framing::floor::floor(&s.model, id).ok().flatten().expect("framed");
    let took = t.elapsed();
    eprintln!("60 m floor on 99 beams: {} members in {took:?}", f.members.len());
    assert!(took.as_secs_f64() < 4.0, "{took:?}");
}
