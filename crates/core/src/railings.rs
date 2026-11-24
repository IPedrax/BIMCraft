//! Railings' parts and hosts: the rails, posts, balusters and panels a railing type stacks along a railing's line
//! (`RailingType`'s `rails`, `posts` and `panel`, or the older kind's one rail and balusters), and the heights a railing
//! with several hosts (a stair and the floor it comes up to) follows along its line.

use crate::geom::{self, P2};
use crate::model::{Element, Guid, MAX_BALUSTERS, Model, PanelPart, PartShape, PostKind, PostPart, RailPart, Railing, RailingType};
use crate::runs::Geom;
use crate::walls::Plane;

/// The older kind's one rail, which its balusters go up to.
pub const TOP_RAIL: &str = "Top Rail";

/// How far apart the heights of a railing over several hosts are looked up along its line, mm.
const STEP: f64 = 50.0;
/// The most points looked up along one railing.
const MAX_SAMPLES: usize = 4000;

/// A railing type's rails, uprights and panel: its own, or the older kind's one 50 mm square rail `height` up and 20 mm
/// balusters `spacing` apart, up to it.
pub fn stack(t: &RailingType) -> (Vec<RailPart>, Vec<PostPart>, Option<PanelPart>) {
    if !t.rails.is_empty() || !t.posts.is_empty() || t.panel.is_some() {
        return (t.rails.clone(), t.posts.clone(), t.panel.clone());
    }
    let rail = RailPart { name: TOP_RAIL.into(), width: 50.0, depth: 50.0, height: t.height, ..RailPart::default() };
    let balusters = (t.spacing > 0.0).then(|| PostPart {
        name: "Baluster".into(),
        kind: PostKind::Baluster,
        width: 20.0,
        depth: 20.0,
        spacing: t.spacing,
        top: Some(TOP_RAIL.into()),
        ..PostPart::default()
    });
    (vec![rail], balusters.into_iter().collect(), None)
}

/// How high a railing of type `t` stands: the top of its highest rail, or its `height` without rails.
pub fn height(t: &RailingType) -> f64 {
    t.rails.iter().map(|r| r.height).reduce(f64::max).unwrap_or(t.height)
}

/// How high a rail's section is: a circle's its diameter, a rectangle's its depth (its width, given none).
pub fn rail_depth(r: &RailPart) -> f64 {
    section(r.shape, r.width, r.depth)
}

fn section(shape: PartShape, width: f64, depth: f64) -> f64 {
    match shape {
        PartShape::Circle => width,
        PartShape::Rectangle if depth > 0.0 => depth,
        PartShape::Rectangle => width,
    }
}

/// A piece of a railing: a prism (its ring in plan between two planes) or a tube along a line.
#[derive(Clone, Debug)]
pub enum Piece {
    Prism(Vec<P2>, Plane, Plane),
    Tube(Geom),
}

/// Which of its type's parts a piece is of: a rail, a post or baluster (by where it is in the type's lists, as [`stack`]
/// gives them), or the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Rail(usize),
    Post(usize),
    Panel,
}

/// A piece of a railing, what it is made of ("" for the railing's grey) and which part of its type it is.
#[derive(Clone, Debug)]
pub struct Part {
    pub piece: Piece,
    pub material: String,
    pub which: Which,
}

impl Piece {
    /// As a prism: a tube as its square box.
    pub fn prism(&self) -> (Vec<P2>, Plane, Plane) {
        match self {
            Piece::Prism(ring, bottom, top) => (ring.clone(), *bottom, *top),
            Piece::Tube(g) => Geom { round: false, ..*g }.prism(),
        }
    }
}

/// A railing's line, measured along in plan.
struct Line {
    pts: Vec<(P2, f64)>,
    /// How far along each point is.
    at: Vec<f64>,
}

impl Line {
    fn new(line: Vec<(P2, f64)>) -> Line {
        let mut pts = line;
        pts.dedup_by(|b, a| a.0.dist(b.0) < 1e-6);
        let mut at = Vec::with_capacity(pts.len());
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if let Some(q) = i.checked_sub(1).and_then(|j| pts.get(j)) {
                s += q.0.dist(p.0);
            }
            at.push(s);
        }
        Line { pts, at }
    }

    fn length(&self) -> f64 {
        self.at.last().copied().unwrap_or(0.0)
    }

    /// The point `s` along, its height, and the way the line goes there.
    fn point(&self, s: f64) -> Option<(P2, f64, P2)> {
        let n = self.pts.len();
        if n < 2 {
            return None;
        }
        let i = self.at.partition_point(|a| *a <= s).clamp(1, n - 1);
        let (&(a, za), &(b, zb)) = (self.pts.get(i - 1)?, self.pts.get(i)?);
        let (sa, sb) = (*self.at.get(i - 1)?, *self.at.get(i)?);
        let k = if sb - sa > 1e-9 { ((s - sa) / (sb - sa)).clamp(0.0, 1.0) } else { 0.0 };
        Some((a + (b - a) * k, za + (zb - za) * k, (b - a).unit()))
    }

    /// How far along its ends and its corners are, where it turns in plan.
    fn stations(&self) -> Vec<f64> {
        let mut out = vec![0.0];
        for (i, w) in self.pts.windows(3).enumerate() {
            let [(a, _), (b, _), (c, _)] = w else { continue };
            if (*b - *a).unit().dot((*c - *b).unit()) < 0.9998 {
                out.extend(self.at.get(i + 1));
            }
        }
        out.push(self.length());
        out
    }

    /// The stretches of it from `s0` to `s1`, each a straight piece with its heights.
    fn between(&self, s0: f64, s1: f64) -> Vec<((P2, f64), (P2, f64))> {
        let mut cuts = vec![s0];
        cuts.extend(self.at.iter().copied().filter(|a| *a > s0 + 1e-6 && *a < s1 - 1e-6));
        cuts.push(s1);
        cuts.windows(2)
            .filter_map(|w| {
                let &[a, b] = w else { return None };
                let (p, zp, _) = self.point(a)?;
                let (q, zq, _) = self.point(b)?;
                (p.dist(q) > 1e-6).then_some(((p, zp), (q, zq)))
            })
            .collect()
    }
}

/// Where along `[a, b]` uprights at most `spacing` apart stand, evenly, its ends left out.
fn inside(a: f64, b: f64, spacing: f64) -> Vec<f64> {
    if spacing <= 0.0 || b - a <= spacing {
        return Vec::new();
    }
    let n = ((b - a) / spacing - 1e-9).ceil().min(MAX_BALUSTERS) as usize;
    (1..n).map(|i| a + (b - a) * i as f64 / n as f64).collect()
}

/// A railing's pieces: its rails along its line, its posts and balusters, and its panels.
pub fn parts(m: &Model, rl: &Railing) -> Vec<Part> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    build(t, crate::stairs::railing(m, rl))
}

/// The pieces of a railing of type `t` along `line`, each point of it with the height of what it runs along.
pub fn build(t: &RailingType, line: Vec<(P2, f64)>) -> Vec<Part> {
    let (rails, posts, panel) = stack(t);
    let top = height(t);
    let line = Line::new(line);
    if line.length() <= 1e-6 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (ri, r) in rails.iter().enumerate() {
        let d = rail_depth(r);
        let round = r.shape == PartShape::Circle;
        let moved = crate::stairs::offset(&line.pts, r.offset);
        for w in moved.windows(2) {
            let &[(p, zp), (q, zq)] = w else { continue };
            if p.dist(q) < 1e-6 {
                continue;
            }
            // Its top `height` up, measured straight up: on a slope its section, square to it, reaches higher over its line.
            let k = (p.dist(q).powi(2) + (zq - zp).powi(2)).sqrt() / p.dist(q);
            let up = r.height - d / 2.0 * k;
            let g = Geom { a: [p.0, p.1, zp + up], b: [q.0, q.1, zq + up], width: r.width, depth: d, round };
            let piece = if round {
                Piece::Tube(g)
            } else {
                let (ring, bottom, top) = g.prism();
                Piece::Prism(ring, bottom, top)
            };
            out.push(Part { piece, material: r.material.clone(), which: Which::Rail(ri) });
        }
    }
    let rail = |name: &Option<String>| name.as_ref().and_then(|n| rails.iter().find(|r| &r.name == n));
    // Where the posts stand: at the ends and corners, and between at most each one's spacing apart.
    let stations = line.stations();
    let mut at_posts: Vec<f64> = Vec::new();
    let mut uprights: Vec<(usize, &PostPart, f64)> = Vec::new();
    for (pi, p) in posts.iter().enumerate().filter(|(_, p)| p.kind == PostKind::Post) {
        for w in stations.windows(2) {
            let &[a, b] = w else { continue };
            for s in std::iter::once(a).chain(inside(a, b, p.spacing)) {
                uprights.push((pi, p, s));
            }
        }
        if let Some(&end) = stations.last() {
            uprights.push((pi, p, end));
        }
    }
    at_posts.extend(uprights.iter().map(|(_, _, s)| *s));
    at_posts.sort_by(f64::total_cmp);
    at_posts.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    // The bays between posts; without any, the stretches between the ends and corners.
    let posted = !at_posts.is_empty();
    let bays: Vec<(f64, f64)> = if posted {
        at_posts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
    } else {
        stations.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
    };
    for (bi, b) in posts.iter().enumerate().filter(|(_, p)| p.kind == PostKind::Baluster) {
        if !posted {
            uprights.extend(stations.iter().map(|s| (bi, b, *s)));
        }
        for &(s0, s1) in &bays {
            uprights.extend(inside(s0, s1, b.spacing).into_iter().map(|s| (bi, b, s)));
        }
    }
    uprights.truncate(MAX_BALUSTERS as usize);
    for (pi, p, s) in uprights {
        let Some((at, z, u)) = line.point(s) else { continue };
        let bottom = match (p.kind, rail(&p.bottom)) {
            (PostKind::Baluster, Some(r)) => z + r.height + p.bottom_offset,
            _ => z + p.bottom_offset,
        };
        let up = match rail(&p.top) {
            Some(r) => z + r.height - rail_depth(r) + p.top_offset,
            None => z + top + p.top_offset,
        };
        if up - bottom < 1.0 {
            continue;
        }
        let c = at + u.perp() * p.offset;
        let piece = match p.shape {
            PartShape::Circle => {
                Piece::Tube(Geom { a: [c.0, c.1, bottom], b: [c.0, c.1, up], width: p.width, depth: p.width, round: true })
            }
            PartShape::Rectangle => {
                let (a, n) = (u * (section(p.shape, p.width, p.depth) / 2.0), u.perp() * (p.width / 2.0));
                Piece::Prism(vec![c - a - n, c + a - n, c + a + n, c - a + n], Plane::flat(bottom), Plane::flat(up))
            }
        };
        out.push(Part { piece, material: p.material.clone(), which: Which::Post(pi) });
    }
    if let Some(pn) = &panel {
        // In from each post's face: the deepest post's half along the line.
        let half = posts.iter().filter(|p| p.kind == PostKind::Post).map(|p| section(p.shape, p.width, p.depth) / 2.0).fold(0.0, f64::max);
        let rise = match rail(&pn.top) {
            Some(r) => r.height - rail_depth(r) - pn.top_gap,
            None => top - pn.top_gap,
        };
        for &(s0, s1) in &bays {
            let (a, b) = if posted { (s0 + half + pn.side_gap, s1 - half - pn.side_gap) } else { (s0 + pn.side_gap, s1 - pn.side_gap) };
            if b - a < 1.0 || rise - pn.bottom < 1.0 {
                continue;
            }
            for ((p, zp), (q, zq)) in line.between(a, b) {
                let len = p.dist(q);
                let u = (q - p) * (1.0 / len);
                let (n, off) = (u.perp() * (pn.thickness / 2.0), u.perp() * pn.offset);
                let g = u * ((zq - zp) / len);
                let base = zp - g.dot(p);
                let ring = vec![p + off - n, q + off - n, q + off + n, p + off + n];
                out.push(Part {
                    piece: Piece::Prism(ring, Plane { z: base + pn.bottom, g }, Plane { z: base + rise, g }),
                    material: pn.material.clone(),
                    which: Which::Panel,
                });
            }
        }
    }
    out
}

/// A host a railing's heights follow: a stair's or a ramp's walk line and half its width, or a floor's outline, its holes
/// and its top.
enum Host {
    Walk(Vec<(P2, f64)>, f64),
    Floor(Vec<P2>, Vec<Vec<P2>>, Plane),
}

impl Host {
    fn of(m: &Model, g: Guid) -> Option<Host> {
        let e = m.elements.get(g)?;
        match e {
            Element::Stair(_) | Element::Ramp(_) => {
                let l = crate::stairs::layout(m, e)?;
                Some(Host::Walk(l.walk, l.width / 2.0))
            }
            Element::Floor(f) => Some(Host::Floor(f.boundary.clone(), f.openings.clone(), m.floor_top(f))),
            _ => None,
        }
    }

    /// Its top at `p`, where `p` is over it.
    fn top(&self, p: P2) -> Option<f64> {
        match self {
            Host::Walk(walk, half) => {
                let (d, z) = walk
                    .windows(2)
                    .filter_map(|w| {
                        let (&(a, za), &(b, zb)) = (w.first()?, w.get(1)?);
                        let k = geom::param_on_segment(p, a, b);
                        Some((p.dist(a + (b - a) * k), za + (zb - za) * k))
                    })
                    .min_by(|x, y| x.0.total_cmp(&y.0))?;
                (d <= half + 1.0).then_some(z)
            }
            Host::Floor(ring, holes, top) => {
                let on_edge = geom::ring_edges(ring).any(|(a, b)| geom::dist_to_segment(p, a, b) < 2.0);
                let over = (on_edge || geom::contains(ring, p)) && !holes.iter().any(|h| geom::contains(h, p));
                over.then(|| top.at(p))
            }
        }
    }
}

/// The hosts a railing's heights follow: its stair or ramp first, then its `hosts`.
pub fn hosts(rl: &Railing) -> Vec<Guid> {
    rl.host.into_iter().chain(rl.hosts.iter().copied()).collect()
}

/// `plan`, its points given the heights of `hosts` under them (the first over each), `lift` above; looked up every
/// [`STEP`] along it, and a point kept where the heights stop going straight. Where over none, the height it had last
/// (the first it has, before it), or `fallback` over none at all.
pub fn follow(m: &Model, hosts: &[Guid], plan: &[P2], lift: f64, fallback: f64) -> Vec<(P2, f64)> {
    let found: Vec<Host> = hosts.iter().filter_map(|g| Host::of(m, *g)).collect();
    let total: f64 = plan.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum();
    let step = STEP.max(total / MAX_SAMPLES as f64);
    // Each point looked up, and whether it is one of the line's own.
    let mut samples: Vec<(P2, bool)> = Vec::new();
    for w in plan.windows(2) {
        let (&a, &b) = (w.first().unwrap_or(&P2(0.0, 0.0)), w.get(1).unwrap_or(&P2(0.0, 0.0)));
        let n = (a.dist(b) / step).ceil().max(1.0) as usize;
        for k in 0..n {
            samples.push((a + (b - a) * (k as f64 / n as f64), k == 0));
        }
    }
    if let Some(&last) = plan.last() {
        samples.push((last, true));
    }
    let tops: Vec<Option<f64>> = samples.iter().map(|(p, _)| found.iter().find_map(|h| h.top(*p))).collect();
    let first = tops.iter().flatten().next().copied().unwrap_or(fallback - lift);
    let mut last = first;
    let mut line: Vec<(P2, f64, bool)> = Vec::with_capacity(samples.len());
    for ((p, own), z) in samples.into_iter().zip(tops) {
        let z = z.unwrap_or(last);
        last = z;
        line.push((p, z + lift, own));
    }
    // Keep the line's own points and those where the heights bend.
    let mut out: Vec<(P2, f64)> = Vec::new();
    for (i, &(p, z, own)) in line.iter().enumerate() {
        let next = line.get(i + 1);
        let keep = own
            || next.is_none()
            || out.last().zip(next).is_none_or(|(&(a, za), &(b, zb, _))| {
                let (ab, ap) = (a.dist(b), a.dist(p));
                ab < 1e-9 || (za + (zb - za) * ap / ab - z).abs() > 0.5
            });
        if keep {
            out.push((p, z));
        }
    }
    out
}

fn number(what: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v.abs() <= crate::model::MAX_COORD { Ok(()) } else { Err(format!("{what}: {v} is not a usable number")) }
}

fn above_zero(what: &str, v: f64) -> Result<(), String> {
    number(what, v)?;
    if v > 0.0 { Ok(()) } else { Err(format!("{what} must be above zero, got {v}")) }
}

/// Whether railing type `n` holds together: its parts' sizes usable numbers, its rails named once each, what its posts,
/// balusters and panel stand on or go up to among its rails, and its materials the project's.
pub fn check_type(m: &Model, n: &str, t: &RailingType) -> Result<(), String> {
    let who = format!("railing type `{n}`");
    above_zero(&format!("{who} height"), t.height)?;
    if !(t.spacing == 0.0 || (50.0..=crate::model::MAX_COORD).contains(&t.spacing)) {
        return Err(format!("{who}: spacing {} mm; balusters stand 50 mm apart or more, or 0 for none", t.spacing));
    }
    let material = |what: &str, name: &str| {
        if name.is_empty() || m.materials.contains_key(name) {
            Ok(())
        } else {
            Err(format!("{who}: {what}'s material `{name}` isn't one of the project's"))
        }
    };
    let mut names = std::collections::BTreeSet::new();
    for r in &t.rails {
        let what = format!("{who} rail `{}`", r.name);
        if r.name.trim().is_empty() || r.name.len() > crate::model::MAX_TEXT {
            return Err(format!("{who}: each rail has a name, up to {} characters", crate::model::MAX_TEXT));
        }
        if !names.insert(r.name.as_str()) {
            return Err(format!("{who}: two rails are named `{}`; name each once, as posts go up to them by name", r.name));
        }
        above_zero(&format!("{what} width"), r.width)?;
        number(&format!("{what} depth"), r.depth)?;
        if r.depth < 0.0 {
            return Err(format!("{what}: depth {} mm; 0 makes it as deep as it is wide", r.depth));
        }
        above_zero(&format!("{what} height"), r.height)?;
        number(&format!("{what} offset"), r.offset)?;
        material(&what, &r.material)?;
    }
    let rail = |what: &str, name: &Option<String>| match name {
        Some(x) if !names.contains(x.as_str()) => Err(format!("{what}: there is no rail `{x}` in the type")),
        _ => Ok(()),
    };
    for p in &t.posts {
        let what = format!("{who} {} `{}`", if p.kind == PostKind::Post { "post" } else { "baluster" }, p.name);
        above_zero(&format!("{what} width"), p.width)?;
        number(&format!("{what} depth"), p.depth)?;
        for (k, v) in [("bottom_offset", p.bottom_offset), ("top_offset", p.top_offset), ("offset", p.offset)] {
            number(&format!("{what} {k}"), v)?;
        }
        match p.kind {
            PostKind::Post if !(p.spacing == 0.0 || (50.0..=crate::model::MAX_COORD).contains(&p.spacing)) => {
                return Err(format!("{what}: spacing {} mm; posts stand 50 mm apart or more, or 0 for ends and corners only", p.spacing));
            }
            PostKind::Baluster if !(50.0..=crate::model::MAX_COORD).contains(&p.spacing) => {
                return Err(format!("{what}: spacing {} mm; balusters stand 50 mm apart or more", p.spacing));
            }
            PostKind::Post if p.bottom.is_some() => {
                return Err(format!("{what}: a post stands on the host; make it a baluster to stand it on a rail"));
            }
            _ => {}
        }
        rail(&what, &p.bottom)?;
        rail(&what, &p.top)?;
        material(&what, &p.material)?;
    }
    if let Some(pn) = &t.panel {
        let what = format!("{who} panel");
        above_zero(&format!("{what} thickness"), pn.thickness)?;
        for (k, v) in [("offset", pn.offset), ("bottom", pn.bottom), ("top_gap", pn.top_gap), ("side_gap", pn.side_gap)] {
            number(&format!("{what} {k}"), v)?;
        }
        rail(&what, &pn.top)?;
        material(&what, &pn.material)?;
    }
    Ok(())
}

/// How many uprights railing `rl` of type `t` has about, along `length` mm: an error past [`MAX_BALUSTERS`].
pub fn check_count(who: &str, t: &RailingType, length: f64) -> Result<(), String> {
    let (_, posts, _) = stack(t);
    let n: f64 = posts.iter().filter(|p| p.spacing > 0.0).map(|p| length / p.spacing).sum();
    if n > MAX_BALUSTERS {
        return Err(format!("{who}: {n:.0} posts and balusters, more than {MAX_BALUSTERS}; space them wider"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::engine::Session;

    fn run(s: &mut Session, id: &str, p: serde_json::Value) -> serde_json::Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    #[test]
    fn a_type_stacks_rails_posts_between_balusters_and_panels() {
        let mut s = Session::new();
        let t = run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "Bars", "rails": [
                {"name": "Top", "shape": "circle", "width": 50, "height": 1000},
                {"name": "Rail1", "width": 40, "depth": 40, "height": 200}],
              "posts": [
                {"name": "Post", "width": 60, "spacing": 1200},
                {"name": "Bar", "kind": "baluster", "width": 15, "spacing": 120, "bottom": "Rail1", "top": "Top"}]}),
        );
        assert_eq!(s.model.types.railing["Bars"].height, 1000.0, "the top of its highest rail: {t}");
        let id = run(&mut s, "railing", json!({"path": [[0, 0], [3000, 0]], "type": "Bars"}))["ids"][0].as_u64().unwrap();
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let pieces = parts(&s.model, rl);
        // Two rails; posts at 0, 1000, 2000 and 3000 (1200 apart at most); 8 bars in each of the 3 bays.
        assert_eq!(pieces.len(), 2 + 4 + 24);
        let tubes = pieces.iter().filter(|p| matches!(p.piece, Piece::Tube(_))).count();
        assert_eq!(tubes, 1, "the round top rail");
        // A bar stands on Rail1's top (200) and goes up to Top's underside (950).
        let (_, bottom, top) = pieces.last().unwrap().piece.prism();
        assert_eq!((bottom.z, top.z), (200.0, 950.0));
        // A panel in each bay instead of the bars, 10 in from each post's face.
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "Bars", "posts": [{"name": "Post", "width": 60, "spacing": 1200}],
            "panel": {"thickness": 12, "bottom": 100, "top": "Top", "side_gap": 10}}),
        );
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let pieces = parts(&s.model, rl);
        assert_eq!(pieces.len(), 2 + 4 + 3);
        let (ring, bottom, top) = pieces.last().unwrap().piece.prism();
        assert_eq!((bottom.z, top.z), (100.0, 950.0));
        assert!((ring[0].0 - 2040.0).abs() < 1e-6, "from 2000 + 30 + 10: {ring:?}");
        // A post can't stand on a rail, and a baluster goes up to a rail the type has.
        let bad = json!({"category": "railing", "name": "Bars", "posts": [{"name": "B", "kind": "baluster", "width": 15, "spacing": 120, "top": "Nope"}]});
        assert!(s.execute("type", &bad).unwrap_err().contains("no rail `Nope`"));
    }

    #[test]
    fn a_railing_over_a_stair_and_the_floor_it_comes_up_to_follows_both() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000}))["id"].as_u64().unwrap();
        let l = crate::stairs::layout(&s.model, s.model.elements.get(stair).unwrap()).unwrap();
        let &(top, z_top) = l.walk.last().unwrap();
        let (_, z_foot) = l.walk[0];
        let floor = run(
            &mut s,
            "floor",
            json!({"level": "Level 2", "boundary": [[-2000, top.1], [2000, top.1], [2000, top.1 + 3000.0], [-2000, top.1 + 3000.0]]}),
        )["id"]
            .as_u64()
            .unwrap();
        let id = run(&mut s, "railing", json!({"path": [[-450, 0], [-450, top.1 + 2000.0]]}))["ids"][0].as_u64().unwrap();
        let flat = |s: &Session| {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            crate::stairs::railing(&s.model, rl)
        };
        assert!(flat(&s).iter().all(|(_, z)| *z == 0.0), "on its level, without hosts");
        assert_eq!(run(&mut s, "railing_hosts", json!({"id": id, "add": [stair, floor]}))["hosts"], json!([stair, floor]));
        let line = flat(&s);
        assert_eq!(line.first().unwrap().1, z_foot, "at the stair's foot");
        let end = line.last().unwrap();
        assert_eq!(end.1, s.model.levels()[1].1.elevation, "on the floor's top");
        assert!((line.iter().find(|(p, _)| (p.1 - top.1).abs() < 30.0).unwrap().1 - z_top).abs() < 60.0, "up the stair: {line:?}");
        // The floor goes: the railing keeps the stair, and the stair's top height past it.
        run(&mut s, "delete", json!({"ids": [floor]}));
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        assert_eq!(hosts(rl).len(), 1);
    }
}
