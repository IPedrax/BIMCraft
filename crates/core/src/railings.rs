//! Railings' parts and hosts: the rails, posts, balusters and panels a railing type stacks along a railing's line
//! (`RailingType`'s `rails`, `posts` and `panel`, or the older kind's one rail and balusters), and the heights a railing
//! with several hosts (a stair and the floor it comes up to) follows along its line.

use crate::geom::{self, P2};
use crate::model::{
    Element, Guid, MAX_BALUSTERS, Model, PanelPart, PartShape, Placing, PostKind, PostPart, RailPart, Railing, RailingType, Transition,
};
use crate::runs::Geom;
use crate::view3d::P3;
use crate::walls::Plane;

pub mod path;

pub use path::sweep;
use path::{circle, rectangle};

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

/// A piece of a railing: a prism (its ring in plan between two planes), a tube along a line, or a section (`width`
/// across, `depth` high) swept along a path, mitred where it bends ([`sweep`]).
#[derive(Clone, Debug)]
pub enum Piece {
    Prism(Vec<P2>, Plane, Plane),
    Tube(Geom),
    Sweep { path: Vec<P3>, profile: Vec<(f64, f64)>, width: f64, depth: f64, round: bool },
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
    /// As prisms: a tube as its square box, a sweep as a box along each stretch of its path.
    pub fn prisms(&self) -> Vec<(Vec<P2>, Plane, Plane)> {
        match self {
            Piece::Prism(ring, bottom, top) => vec![(ring.clone(), *bottom, *top)],
            Piece::Tube(g) => vec![Geom { round: false, ..*g }.prism()],
            Piece::Sweep { path, width, depth, .. } => path
                .windows(2)
                .filter(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) > 1e-6)
                .map(|w| Geom { a: w[0], b: w[1], width: *width, depth: *depth, round: false }.prism())
                .collect(),
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

    /// How far along it the point of it nearest `p` is, where that is at most `reach` from `p`.
    fn along(&self, p: P2, reach: f64) -> Option<f64> {
        let (d, s) = self
            .pts
            .windows(2)
            .zip(&self.at)
            .filter_map(|(w, s0)| {
                let (&(a, _), &(b, _)) = (w.first()?, w.get(1)?);
                let k = geom::param_on_segment(p, a, b);
                Some((p.dist(a + (b - a) * k), s0 + a.dist(b) * k))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))?;
        (d <= reach).then_some(s)
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

/// Where balusters `b` stand in a bay from `a` to `z` (clear of its posts): one only, `shift` from its start, middle or
/// end; or a row of them their spacing apart from its start, round its middle or from its end, moved `shift` along.
fn placing(a: f64, z: f64, b: &PostPart) -> Vec<f64> {
    if z < a {
        return Vec::new();
    }
    let mid = (a + z) / 2.0;
    let first = match b.justify {
        Placing::Start => a + b.shift,
        Placing::End => z - b.shift,
        Placing::Centre | Placing::Spread => mid + b.shift,
    };
    if b.single || b.spacing <= 0.0 {
        return (first >= a - 1e-6 && first <= z + 1e-6).then_some(first).into_iter().collect();
    }
    let step = b.spacing;
    // Back from the first to the bay's start, then on to its end.
    let back = ((first - a) / step).floor().max(0.0);
    let from = first - back * step;
    let n = (((z - from) / step).floor() + 1.0).clamp(0.0, MAX_BALUSTERS) as usize;
    (0..n).map(|k| from + k as f64 * step).filter(|s| *s >= a - 1e-6 && *s <= z + 1e-6).collect()
}

/// A railing's pieces: its rails along its line, its posts and balusters, and its panels.
pub fn parts(m: &Model, rl: &Railing) -> Vec<Part> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    let line = crate::stairs::railing(m, rl);
    let plan: Vec<P2> = line.iter().map(|(p, _)| *p).collect();
    let ground = Ground::of(m, &hosts(rl), &plan, rl.base_offset);
    build(t, line, &ground, rl.flip)
}

/// The steps of a flight, going up: each one's middle in plan; and how far from them a railing beside it may be.
type Flight = (Vec<P2>, f64);

/// What a railing's posts and balusters stand on: the tops of its hosts' steps, landings and surfaces, and the flights of
/// steps its posts may be lined up with. None, they stand on its line.
pub struct Ground {
    hosts: Vec<Host>,
    lift: f64,
}

impl Ground {
    /// Nothing under the line: the posts stand on it.
    pub fn none() -> Ground {
        Ground { hosts: Vec::new(), lift: 0.0 }
    }

    /// The hosts `hosts` under `plan`, `lift` added to their tops.
    pub fn of(m: &Model, hosts: &[Guid], plan: &[P2], lift: f64) -> Ground {
        Ground { hosts: hosts.iter().filter_map(|g| Host::of(m, *g, plan)).collect(), lift }
    }

    /// The top of what is under `p`: a step, a landing or a surface.
    fn at(&self, p: P2) -> Option<f64> {
        self.hosts.iter().find_map(|h| h.ground(p)).map(|z| z + self.lift)
    }

    fn flights(&self) -> Vec<Flight> {
        self.hosts.iter().flat_map(Host::flights).collect()
    }
}

/// The pieces of a railing of type `t` along `line`, each point of it with the height of what it runs along, its posts
/// and balusters standing on `ground`.
/// Flipped, each part's offset goes to the other side of the line.
pub fn build(t: &RailingType, line: Vec<(P2, f64)>, ground: &Ground, flip: bool) -> Vec<Part> {
    let (mut rails, mut posts, mut panel) = stack(t);
    if flip {
        rails.iter_mut().for_each(|r| r.offset = -r.offset);
        posts.iter_mut().for_each(|p| p.offset = -p.offset);
        if let Some(pn) = &mut panel {
            pn.offset = -pn.offset;
        }
    }
    let top = height(t);
    let line = Line::new(line);
    if line.length() <= 1e-6 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (ri, r) in rails.iter().enumerate() {
        let d = rail_depth(r);
        let round = r.shape == PartShape::Circle;
        // The solver makes its centre line from the railing's, moved across; the sweeper its surface.
        let moved = crate::stairs::offset(&line.pts, r.offset);
        let profile = if round { circle(r.width / 2.0) } else { rectangle(r.width, d) };
        // Rails apart where the line steps: each a sweep of its own, the higher running on over the lower.
        for path in path::rails(&moved, r, t.transition == Transition::Separate) {
            out.push(Part {
                piece: Piece::Sweep { path, profile: profile.clone(), width: r.width, depth: d, round },
                material: r.material.clone(),
                which: Which::Rail(ri),
            });
        }
    }
    let rail = |name: &Option<String>| name.as_ref().and_then(|n| rails.iter().find(|r| &r.name == n));
    // Where the posts stand: at the ends and corners, and between at most each one's spacing apart; or on the steps.
    let stations = line.stations();
    let flights: Vec<Vec<f64>> = ground
        .flights()
        .iter()
        .map(|(f, reach)| f.iter().filter_map(|c| line.along(*c, *reach)).collect::<Vec<f64>>())
        .filter(|f| f.len() > 1)
        .collect();
    let mut at_posts: Vec<f64> = Vec::new();
    let mut uprights: Vec<(usize, &PostPart, f64)> = Vec::new();
    for (pi, p) in posts.iter().enumerate().filter(|(_, p)| p.kind == PostKind::Post) {
        let mut here: Vec<f64> = Vec::new();
        // Over the flights, on every so many steps, and the first and last.
        let mut covered: Vec<(f64, f64)> = Vec::new();
        if p.steps > 0 {
            for f in &flights {
                let (lo, hi) = f.iter().fold((f64::MAX, f64::MIN), |(lo, hi), s| (lo.min(*s), hi.max(*s)));
                let half = (hi - lo) / (f.len() - 1) as f64 / 2.0;
                covered.push((lo - half, hi + half));
                let last = f.len() - 1;
                let start = if p.skip_ends { p.steps as usize - 1 } else { 0 };
                for (k, s) in f.iter().enumerate() {
                    let on = k >= start && (k - start) % p.steps as usize == 0;
                    if on || (!p.skip_ends && (k == 0 || k == last)) {
                        here.push(*s);
                    }
                }
            }
        }
        let over = |s: f64| covered.iter().any(|(a, b)| s >= *a && s <= *b);
        here.extend(stations.iter().copied().filter(|s| !over(*s)));
        here.sort_by(f64::total_cmp);
        here.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        let mut filled = here.clone();
        for w in here.windows(2) {
            let &[a, b] = w else { continue };
            if !over((a + b) / 2.0) {
                filled.extend(inside(a, b, p.spacing));
            }
        }
        uprights.extend(filled.into_iter().map(|s| (pi, p, s)));
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
    // How far in from a bay's ends a baluster's middle may be: clear of the post there.
    let post_half = posts.iter().filter(|p| p.kind == PostKind::Post).map(|p| section(p.shape, p.width, p.depth) / 2.0).fold(0.0, f64::max);
    for (bi, b) in posts.iter().enumerate().filter(|(_, p)| p.kind == PostKind::Baluster) {
        let spread = b.justify == Placing::Spread && !b.single;
        if !posted && spread {
            uprights.extend(stations.iter().map(|s| (bi, b, *s)));
        }
        let clear = if posted { post_half + section(b.shape, b.width, b.depth) / 2.0 } else { 0.0 };
        for &(s0, s1) in &bays {
            let placed = if spread { inside(s0, s1, b.spacing) } else { placing(s0 + clear, s1 - clear, b) };
            uprights.extend(placed.into_iter().map(|s| (bi, b, s)));
        }
    }
    uprights.truncate(MAX_BALUSTERS as usize);
    for (pi, p, s) in uprights {
        let Some((at, z, u)) = line.point(s) else { continue };
        let c = at + u.perp() * p.offset;
        // On the step, the landing or the floor under it; on a rail, that rail's top.
        let bottom = match (p.kind, rail(&p.bottom)) {
            (PostKind::Baluster, Some(r)) => z + r.height + p.bottom_offset,
            _ => ground.at(c).unwrap_or(z) + p.bottom_offset,
        };
        let up = match rail(&p.top) {
            Some(r) => z + r.height - rail_depth(r) + p.top_offset,
            None => z + top + p.top_offset,
        };
        if up - bottom < 1.0 {
            continue;
        }
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

/// A host a railing's heights follow: a stair (its walk line and half its width, its flights' runs, and its landings and
/// winders), a ramp (its walk line), or a surface (a floor's, a roof's or a toposolid's top, as the 3D view has it).
enum Host {
    Stair { walk: Vec<(P2, f64)>, half: f64, runs: Vec<crate::stairs::Run>, pads: Vec<crate::stairs::Pad> },
    Ramp(Vec<(P2, f64)>, f64),
    Surface(Vec<crate::view3d::Solid>),
}

/// How near the walk line, past half a stair's width, a point is still over it.
const NEAR: f64 = 1.0;

impl Host {
    /// Host `g`, of what lies near `plan`.
    fn of(m: &Model, g: Guid, plan: &[P2]) -> Option<Host> {
        let e = m.elements.get(g)?;
        match e {
            Element::Stair(_) => {
                let l = crate::stairs::layout(m, e)?;
                Some(Host::Stair { walk: l.walk, half: l.width / 2.0, runs: l.runs, pads: l.pads })
            }
            Element::Ramp(_) => {
                let l = crate::stairs::layout(m, e)?;
                Some(Host::Ramp(l.walk, l.width / 2.0))
            }
            Element::Floor(_) | Element::Roof(_) | Element::Toposolid(_) => {
                let id = m.elements.id(g)?;
                let (lo, hi) = plan.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
                    (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
                });
                Some(Host::Surface(crate::view3d::top_solids(m, id, (lo, hi))))
            }
            _ => None,
        }
    }

    /// Where on walk line `walk` `p` is nearest, how far from it, and its height there.
    fn on_walk(walk: &[(P2, f64)], p: P2) -> Option<(f64, f64)> {
        walk.windows(2)
            .filter_map(|w| {
                let (&(a, za), &(b, zb)) = (w.first()?, w.get(1)?);
                let k = geom::param_on_segment(p, a, b);
                Some((p.dist(a + (b - a) * k), za + (zb - za) * k))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
    }

    /// Where `p` is over a run of a stair's steps: how far along it from its first riser, and the run.
    fn on_run(runs: &[crate::stairs::Run], p: P2) -> Option<(f64, &crate::stairs::Run)> {
        runs.iter().find_map(|r| {
            let (a, c) = ((p - r.start).dot(r.u), (p - r.start).dot(r.u.perp()));
            (a >= -1e-6 && a <= r.length + 1e-6 && c.abs() <= r.width / 2.0 + NEAR && r.risers > 1).then_some((a, r))
        })
    }

    /// The height of its line at `p`, where `p` is over it: a stair's over its treads' nosings, flat on a landing.
    fn top(&self, p: P2) -> Option<f64> {
        match self {
            Host::Stair { walk, half, runs, pads } => {
                if let Some((a, r)) = Host::on_run(runs, p) {
                    let each = (r.z.1 - r.z.0) / f64::from(r.risers);
                    let going = r.going();
                    return Some(if going > 0.0 { (r.z.0 + each * (1.0 + a / going)).min(r.z.1) } else { r.z.1 });
                }
                // A landing flat; winders (a spiral's steps too) along the walk line. Past its flights, landings and winders
                // (before its foot, past its top) it is no host: what is there is.
                let pad = pads.iter().find(|q| geom::contains(&q.ring, p))?;
                if !pad.tread {
                    return Some(pad.z);
                }
                let (d, z) = Host::on_walk(walk, p)?;
                (d <= half + NEAR).then_some(z)
            }
            Host::Ramp(walk, half) => {
                let (d, z) = Host::on_walk(walk, p)?;
                (d <= half + NEAR).then_some(z)
            }
            Host::Surface(solids) => surface(solids, p),
        }
    }

    /// The top of what is under `p`: a stair's tread, landing or winder; a ramp's slope; a surface.
    fn ground(&self, p: P2) -> Option<f64> {
        match self {
            Host::Stair { runs, pads, .. } => {
                if let Some((a, r)) = Host::on_run(runs, p) {
                    let each = (r.z.1 - r.z.0) / f64::from(r.risers);
                    let going = r.going();
                    let k = if going > 0.0 { (a / going).floor() + 1.0 } else { 1.0 };
                    return Some((r.z.0 + each * k.min(f64::from(r.risers - 1))).min(r.z.1));
                }
                pads.iter().find(|q| geom::contains(&q.ring, p)).map(|q| q.z)
            }
            Host::Ramp(..) | Host::Surface(_) => self.top(p),
        }
    }

    /// How steep its steepest flight is, rise over going: 0 but for a stair.
    fn pitch(&self) -> f64 {
        let Host::Stair { runs, .. } = self else { return 0.0 };
        runs.iter()
            .filter(|r| r.risers > 1 && r.going() > 0.0)
            .map(|r| (r.z.1 - r.z.0) / f64::from(r.risers) / r.going())
            .fold(0.0, f64::max)
    }

    /// A stair's flights: each run's treads' middles, going up.
    fn flights(&self) -> Vec<Flight> {
        let Host::Stair { runs, .. } = self else { return Vec::new() };
        runs.iter()
            .filter(|r| r.risers > 1)
            .map(|r| ((1..r.risers).map(|k| r.start + r.u * ((f64::from(k) - 0.5) * r.going())).collect(), r.width / 2.0 + 200.0))
            .collect()
    }
}

/// The highest top of `solids` over `p`, or within 2 mm of it (a railing along a floor's edge).
fn surface(solids: &[crate::view3d::Solid], p: P2) -> Option<f64> {
    let at = |q: P2| solids.iter().filter(|s| s.over(q)).map(|s| s.z_at(q).1).reduce(f64::max);
    at(p).or_else(|| [P2(2.0, 0.0), P2(-2.0, 0.0), P2(0.0, 2.0), P2(0.0, -2.0)].into_iter().find_map(|d| at(p + d)))
}

/// `line` with the corner put in where its heights bend between two of its points (a flight's top, a landing's edge):
/// where the lines through the points either side meet, so that a straight line between the two never cuts under it.
fn corners(line: &[(P2, f64, bool)], top: impl Fn(P2) -> Option<f64>) -> Vec<(P2, f64, bool)> {
    let mut out = Vec::with_capacity(line.len() + line.len() / 8);
    for i in 0..line.len() {
        let &(p, z, own) = &line[i];
        out.push((p, z, own));
        let (Some(&(q, zq, _)), Some(&(o, zo, _)), Some(&(r, zr, _))) =
            (line.get(i + 1), i.checked_sub(1).and_then(|j| line.get(j)), line.get(i + 2))
        else {
            continue;
        };
        let run = p.dist(q);
        let mid = top(p + (q - p) * 0.5);
        if run < 1e-6 || mid.is_none_or(|m| (m - (z + zq) / 2.0).abs() <= 0.25) {
            continue;
        }
        let (a, b) = (o.dist(p), q.dist(r));
        if a < 1e-6 || b < 1e-6 {
            continue;
        }
        // The slopes coming in and going out; where they meet, t along from `p`.
        let (g1, g2) = ((z - zo) / a, (zr - zq) / b);
        if (g1 - g2).abs() < 1e-9 {
            continue;
        }
        let t = (zq - z - g2 * run) / (g1 - g2);
        if t > 1e-6 && t < run - 1e-6 {
            out.push((p + (q - p) * (t / run), z + g1 * t, false));
        }
    }
    out
}

/// A point of a line: where it is in plan, its height, and whether it is a corner of the line's own.
type Point = (P2, f64, bool);

/// A run-on walked into the lower side: the points it raises and to what, and the point where it meets the line there,
/// with the place it goes in.
type Walked = (Vec<(usize, f64)>, Option<(usize, Point)>);

/// `line` with each jump (heights changing faster than twice the steepest flight between two points) joined as a railing
/// is built: the higher side's line run on at its own slope into the lower side until it meets the line there, every
/// point it passes raised to it; where that side does not slope down into the lower one, or the step is higher than
/// `extra`, a joining slope twice as steep as the steepest flight instead.
fn envelope(mut line: Vec<(P2, f64, bool)>, steepest: f64, extra: f64) -> Vec<(P2, f64, bool)> {
    let n = line.len();
    let mut along = vec![0.0; n];
    for i in 1..n {
        along[i] = along[i - 1] + line[i].0.dist(line[i - 1].0);
    }
    let z: Vec<f64> = line.iter().map(|q| q.1).collect();
    // The jumps, found on the heights as the hosts give them: between `i` and `i + 1`.
    let jumps: Vec<usize> = (0..n.saturating_sub(1))
        .filter(|&i| {
            let run = along[i + 1] - along[i];
            run > 1e-6 && (z[i + 1] - z[i]).abs() > 2.0 * steepest * run + 1.0
        })
        .collect();
    let mut inserts: Vec<(usize, (P2, f64, bool))> = Vec::new();
    for i in jumps {
        let up = z[i + 1] > z[i];
        // The higher side's point at the jump, the one past it on that side, and the way into the lower side.
        let (high, beyond) = if up { (i + 1, i + 2) } else { (i, i.wrapping_sub(1)) };
        let (Some(&zh), Some(&sh)) = (z.get(high), along.get(high)) else { continue };
        // How fast the higher line falls going into the lower side.
        let fall = match (z.get(beyond), along.get(beyond)) {
            (Some(&zb), Some(&sb)) if (sb - sh).abs() > 1e-6 => (zb - zh) / (sb - sh).abs(),
            _ => 0.0,
        };
        let step = (z[i + 1] - z[i]).abs();
        let going = step / steepest.max(1e-9);
        let reach = 1.5 * going;
        // Walked from the step into the lower side at `rate`: the points raised and where it meets the line there, if
        // it does within `reach`.
        let order: Vec<usize> = if up { (0..=i).rev().collect() } else { (i + 1..n).collect() };
        let walk = |rate: f64| -> Option<Walked> {
            let mut raised = Vec::new();
            let mut before: Option<usize> = None;
            for &j in &order {
                let away = (along[j] - sh).abs();
                let e = zh - rate * away;
                if e > z[j] + 1e-9 {
                    if away > reach {
                        return None;
                    }
                    raised.push((j, e));
                    before = Some(j);
                    continue;
                }
                let meet = before.map(|k| {
                    let (dj, dk) = (e - z[j], zh - rate * (along[k] - sh).abs() - z[k]);
                    let t = if (dk - dj).abs() > 1e-12 { -dj / (dk - dj) } else { 0.0 };
                    let (pj, pk) = (line[j].0, line[k].0);
                    (j.max(k), (pj + (pk - pj) * t, z[j] + (z[k] - z[j]) * t, false))
                });
                return Some((raised, meet));
            }
            Some((raised, None))
        };
        // The higher line run on at its own slope where the step is at most `extra`; else (or where it never meets the
        // lower line near, an inner corner's flight running beside it) joining slopes steeper and steeper.
        let run_on = (fall > 1e-3 && step <= extra + 1e-6).then_some(fall);
        let found = run_on.into_iter().chain([2.0, 3.0, 5.0, 8.0].map(|k| k * steepest)).find_map(walk);
        if let Some((raised, meet)) = found {
            for (j, e) in raised {
                if let Some(q) = line.get_mut(j) {
                    q.1 = q.1.max(e);
                }
            }
            inserts.extend(meet);
        }
    }
    // The meeting points, each between the two points it lies between.
    inserts.sort_by_key(|q| std::cmp::Reverse(q.0));
    for (at, point) in inserts {
        line.insert(at.min(line.len()), point);
    }
    line
}

/// `line` with each jump (heights changing faster than twice the steepest flight between two points) made upright: the
/// line level to where its hosts change, found by halving, moved `overhang` into the lower side (the higher part runs on
/// over it, never the lower under the higher), and there up or down within a millimetre.
fn upright(hosts: &[Host], line: Vec<(P2, f64, bool)>, steepest: f64, overhang: f64) -> Vec<(P2, f64, bool)> {
    let top = |p: P2| hosts.iter().find_map(|h| h.top(p));
    let total: f64 = line.windows(2).map(|w| w[0].0.dist(w[1].0)).sum();
    // Each sample's distance along, for moving a step back or on.
    let mut along = Vec::with_capacity(line.len());
    let mut s = 0.0;
    for (i, (p, ..)) in line.iter().enumerate() {
        if let Some((q, ..)) = i.checked_sub(1).and_then(|j| line.get(j)) {
            s += p.dist(*q);
        }
        along.push(s);
    }
    // Where each step is, how far along, and its heights before and after.
    let mut steps: Vec<(f64, f64, f64)> = Vec::new();
    for i in 1..line.len() {
        let (Some(&(q, zq, _)), Some(&(p, z, _))) = (line.get(i - 1), line.get(i)) else { continue };
        let run = p.dist(q);
        if run > 1e-6 && (z - zq).abs() > 2.0 * steepest * run + 1.0 {
            let lift = top(p).map_or(0.0, |h| z - h);
            let near = |t: f64| top(q + (p - q) * t).is_none_or(|h| (h + lift - zq).abs() < (h + lift - z).abs());
            let (mut a, mut b) = (0.0, 1.0);
            for _ in 0..20 {
                let mid = (a + b) / 2.0;
                if near(mid) {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            let at = along.get(i - 1).copied().unwrap_or(0.0) + run * a;
            // Into the lower side: back, going up; on, going down.
            let at = if z > zq { at - overhang } else { at + overhang };
            steps.push((at.clamp(0.5, (total - 0.5).max(0.5)), zq, z));
        }
    }
    if steps.is_empty() {
        return line;
    }
    // The heights between steps hold as they were before each step's place and after it, as the higher side asks.
    let mut out: Vec<(P2, f64, bool)> = Vec::with_capacity(line.len() + 2 * steps.len());
    let point = |s: f64| -> (P2, P2) {
        let k = along.partition_point(|a| *a <= s).clamp(1, line.len().saturating_sub(1).max(1));
        let (Some(&(a, ..)), Some(&(b, ..))) = (line.get(k - 1), line.get(k)) else { return (P2(0.0, 0.0), P2(1.0, 0.0)) };
        let (sa, sb) = (along.get(k - 1).copied().unwrap_or(0.0), along.get(k).copied().unwrap_or(0.0));
        let t = if sb > sa { (s - sa) / (sb - sa) } else { 0.0 };
        (a + (b - a) * t, (b - a).unit())
    };
    let mut next = steps.iter().peekable();
    for (i, &(p, z, own)) in line.iter().enumerate() {
        let s = along.get(i).copied().unwrap_or(0.0);
        while let Some(&&(at, before, after)) = next.peek() {
            if at > s {
                break;
            }
            let (q, u) = point(at);
            out.push((q - u * 0.5, before, false));
            out.push((q + u * 0.5, after, false));
            next.next();
        }
        // A sample between a step moved back and where the step was takes the higher side's height.
        let z = steps.iter().fold(z, |z, &(at, before, after)| {
            let high = before.max(after);
            let (lo, hi) = if after > before { (at, at + overhang + 1.0) } else { (at - overhang - 1.0, at) };
            if s > lo && s < hi { z.max(high) } else { z }
        });
        out.push((p, z, own));
    }
    out
}

/// The hosts a railing's heights follow: its stair or ramp first, then its `hosts`.
pub fn hosts(rl: &Railing) -> Vec<Guid> {
    rl.host.into_iter().chain(rl.hosts.iter().copied()).collect()
}

/// `plan`, its points given the heights of `hosts` under them (the first over each), `lift` above, as railing type `t`
/// makes its line: looked up every [`STEP`] along it (where over none, the height it had last, the first it has before
/// it, or `fallback` over none at all); its jumps joined as the type's `transition` says ([`envelope`], [`upright`]); and
/// a point kept only where the heights bend ([`straighten`] within half a millimetre).
pub fn follow(m: &Model, hosts: &[Guid], plan: &[P2], lift: f64, fallback: f64, t: &RailingType) -> Vec<(P2, f64)> {
    let found: Vec<Host> = hosts.iter().filter_map(|g| Host::of(m, *g, plan)).collect();
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
    line = corners(&line, |p| found.iter().find_map(|h| h.top(p)).map(|z| z + lift));
    // Where a flight begins a riser over the landing before it, the heights jump. Joined, the higher line runs on at its
    // own slope over the lower part until it meets that part's line (higher there than it need be, by at most the step);
    // a gooseneck or rails apart, it goes straight up or down.
    let steepest = found.iter().map(Host::pitch).fold(0.0, f64::max);
    if steepest > 0.0 && t.transition != Transition::Continuous {
        line = upright(&found, line, steepest, t.extension.max(0.0));
    } else if steepest > 0.0 {
        line = envelope(line, steepest, t.extra.max(0.0));
    }
    // Each part's line straight as it is: a point kept only where the heights bend.
    straighten(&line, 0.5)
}

/// `line` (each point with the least height it may have, and whether it is a corner of its own) made of straight
/// sections: from where each begins, the longest that stays no lower than the line and at most `extra` higher, as low
/// as it can be. A new one begins at its end, at each corner in plan, and after an upright step (two points within a
/// millimetre in plan), which stays as it is.
pub fn straighten(line: &[(P2, f64, bool)], extra: f64) -> Vec<(P2, f64)> {
    let n = line.len();
    let pts: Vec<(P2, f64)> = line.iter().map(|&(p, z, _)| (p, z)).collect();
    if n < 3 {
        return pts;
    }
    let mut at = vec![0.0; n];
    for i in 1..n {
        at[i] = at[i - 1] + pts[i].0.dist(pts[i - 1].0);
    }
    let upright = |i: usize| pts[i].0.dist(pts[i + 1].0) <= 1.0 + 1e-9 && (pts[i].1 - pts[i + 1].1).abs() > 1.0;
    let turns = |i: usize| {
        let (u, v) = ((pts[i].0 - pts[i - 1].0).unit(), (pts[i + 1].0 - pts[i].0).unit());
        i + 1 < n && u.dot(v) < 0.9998
    };
    let mut out = vec![pts[0]];
    let (mut start, mut zs) = (0usize, pts[0].1);
    while start < n - 1 {
        if upright(start) {
            start += 1;
            zs = pts[start].1;
            out.push(pts[start]);
            continue;
        }
        let (mut lo, mut hi) = (f64::MIN, f64::MAX);
        let (mut end, mut slope) = (start + 1, 0.0);
        for j in start + 1..n {
            if j > start + 1 && upright(j - 1) {
                break;
            }
            let ds = at[j] - at[start];
            if ds <= 1e-9 {
                continue;
            }
            let (nlo, nhi) = (lo.max((pts[j].1 - zs) / ds), hi.min((pts[j].1 + extra - zs) / ds));
            if nlo > nhi + 1e-9 && j > start + 1 {
                break;
            }
            (lo, hi, end, slope) = (nlo, nhi.max(nlo), j, nlo);
            if j + 1 < n && turns(j) {
                break;
            }
        }
        let z = zs + slope * (at[end] - at[start]);
        out.push((pts[end].0, z.max(pts[end].1)));
        start = end;
        zs = z.max(pts[end].1);
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
    for (k, v) in [("extra", t.extra), ("extension", t.extension)] {
        number(&format!("{who} {k}"), v)?;
        if v < 0.0 {
            return Err(format!("{who}: {k} {v} mm; it is 0 or more"));
        }
    }
    if t.posts.iter().filter(|p| p.kind == PostKind::Post).count() > 1 {
        return Err(format!("{who}: it has more than one kind of post; a railing type has one, and balusters as many as you like"));
    }
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
        for (k, v) in [("bottom_offset", p.bottom_offset), ("top_offset", p.top_offset), ("offset", p.offset), ("shift", p.shift)] {
            number(&format!("{what} {k}"), v)?;
        }
        match p.kind {
            PostKind::Post if !(p.spacing == 0.0 || (50.0..=crate::model::MAX_COORD).contains(&p.spacing)) => {
                return Err(format!("{what}: spacing {} mm; posts stand 50 mm apart or more, or 0 for ends and corners only", p.spacing));
            }
            PostKind::Baluster if !p.single && !(50.0..=crate::model::MAX_COORD).contains(&p.spacing) => {
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
        let round = pieces.iter().filter(|p| matches!(p.piece, Piece::Sweep { round: true, .. })).count();
        assert_eq!(round, 1, "the round top rail");
        // A bar stands on Rail1's top (200) and goes up to Top's underside (950).
        let (_, bottom, top) = pieces.last().unwrap().piece.prisms()[0].clone();
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
        let (ring, bottom, top) = pieces.last().unwrap().piece.prisms()[0].clone();
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
        assert!((line.first().unwrap().1 - z_foot - 3000.0 / 17.0).abs() < 1e-6, "over the first nosing, one riser up");
        let end = line.last().unwrap();
        assert_eq!(end.1, s.model.levels()[1].1.elevation, "on the floor's top");
        // At the stair's top, no lower than the floor there and at most the type's extra 100 mm over it.
        let at = |y: f64| {
            let w = line.windows(2).find(|w| (w[0].0.1 - y) * (w[1].0.1 - y) <= 0.0).unwrap();
            let k = if (w[1].0.1 - w[0].0.1).abs() > 1e-9 { (y - w[0].0.1) / (w[1].0.1 - w[0].0.1) } else { 0.0 };
            w[0].1 + (w[1].1 - w[0].1) * k
        };
        let z = at(top.1);
        assert!(z >= z_top - 1e-6 && z <= z_top + 100.0 + 1e-6, "up the stair: {z} at its top, {z_top}: {line:?}");
        // The floor goes: the railing keeps the stair, and the stair's top height past it.
        run(&mut s, "delete", json!({"ids": [floor]}));
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        assert_eq!(hosts(rl).len(), 1);
    }

    #[test]
    fn posts_line_up_with_every_so_many_steps_and_stand_on_the_treads() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1200}))["id"].as_u64().unwrap();
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "Stepped", "rails": [{"name": "Top", "width": 50, "height": 900}],
            "posts": [{"name": "Post", "width": 40, "steps": 3, "top": "Top"}]}),
        );
        let id = run(&mut s, "railing", json!({"host": stair, "side": "left", "type": "Stepped"}))["ids"][0].as_u64().unwrap();
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let posts: Vec<(P2, f64)> = parts(&s.model, rl)
            .iter()
            .filter(|p| p.which == Which::Post(0))
            .map(|p| {
                let (ring, bottom, _) = p.piece.prisms()[0].clone();
                (geom::centroid(&ring), bottom.z)
            })
            .collect();
        // 17 risers, 16 treads 280 deep: on treads 1, 4, 7, 10, 13 and 16 (the last too), each in the middle of its tread.
        let each = 3000.0 / 17.0;
        let steps: Vec<f64> = posts.iter().map(|(c, _)| (c.1 / 280.0 - 0.5).round() + 1.0).collect();
        assert_eq!(steps, vec![1.0, 4.0, 7.0, 10.0, 13.0, 16.0], "{posts:?}");
        for ((c, z), k) in posts.iter().zip(&steps) {
            assert!((c.1 - (k - 0.5) * 280.0).abs() < 1.0);
            assert!((z - k * each).abs() < 1e-6, "on its tread's top: {z} at step {k}");
        }
        // Without the first and last: every third step from the third.
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "Stepped",
            "posts": [{"name": "Post", "width": 40, "steps": 3, "skip_ends": true, "top": "Top"}]}),
        );
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let n = parts(&s.model, rl).iter().filter(|p| p.which == Which::Post(0)).count();
        assert_eq!(n, 5, "steps 3, 6, 9, 12 and 15");
    }

    #[test]
    fn a_railing_follows_a_sloped_floor_and_a_shaped_one() {
        let mut s = Session::new();
        let floor = run(
            &mut s,
            "floor",
            json!({"boundary": [[0, 0], [6000, 0], [6000, 3000], [0, 3000]],
            "slope": 5, "arrow": [[0, 1500], [6000, 1500]]}),
        )["id"]
            .as_u64()
            .unwrap();
        let id = run(&mut s, "railing", json!({"path": [[0, 100], [6000, 100]], "hosts": [floor]}))["ids"][0].as_u64().unwrap();
        let line = |s: &Session| {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            crate::stairs::railing(&s.model, rl)
        };
        let end = line(&s).last().unwrap().1;
        assert!((end - 6000.0 * 5f64.to_radians().tan()).abs() < 1.0, "up the slope: {end}");
        // A point raised in the middle of a flat floor: the railing goes over it, up and down.
        run(&mut s, "modify", json!({"id": floor, "slope": 0, "arrow": null}));
        run(&mut s, "add_point", json!({"id": floor, "at": [3000, 100], "z": 300}));
        let z: Vec<f64> = line(&s).iter().map(|(_, z)| *z).collect();
        let high = z.iter().copied().fold(f64::MIN, f64::max);
        // Straight up to it and down: never under the floor, at most 100 mm over it (the type's extra).
        assert!((300.0..=400.0).contains(&high) && z[0].abs() < 1.0 && z.last().unwrap().abs() < 1.0, "{z:?}");
    }

    #[test]
    fn a_rail_is_one_sweep_mitred_at_its_corners() {
        let (tris, caps) = sweep(&[[0.0, 0.0, 1000.0], [1000.0, 0.0, 1000.0], [1000.0, 1000.0, 1000.0]], &rectangle(40.0, 40.0));
        assert_eq!((tris.len(), caps), (2 * 4 + 2 * 4 * 2, 8));
        // The outer corner of the mitre: 20 out from each stretch.
        let corner = tris.iter().flatten().any(|p| (p[0] - 1020.0).abs() < 1e-9 && (p[1] + 20.0).abs() < 1e-9);
        assert!(corner, "mitred, not overlapped");
    }

    #[test]
    fn a_path_is_its_lines_joined_end_to_end_whichever_way_they_were_drawn() {
        use crate::sketch::{SketchLine, open_path};
        let lines = [
            SketchLine::new(P2(1000.0, 0.0), P2(1000.0, 1000.0)),
            SketchLine::new(P2(1000.0, 0.0), P2(0.0, 0.0)),
            SketchLine::new(P2(2000.0, 1000.0), P2(1000.0, 1000.0)),
        ];
        let path = open_path(&lines).unwrap();
        assert_eq!(path, vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0), P2(2000.0, 1000.0)]);
        let apart = [SketchLine::new(P2(0.0, 0.0), P2(1.0, 0.0)), SketchLine::new(P2(5.0, 0.0), P2(9.0, 0.0))];
        assert!(open_path(&apart).unwrap_err().contains("join end to end"));
    }

    #[test]
    fn a_gooseneck_goes_level_to_the_flight_then_straight_up() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 8, "turn": -90}]}))["id"]
            .as_u64()
            .unwrap();
        let id = run(&mut s, "railing", json!({"host": stair, "side": "left"}))["ids"][0].as_u64().unwrap();
        let line = |s: &Session| {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            crate::stairs::railing(&s.model, rl)
        };
        let upright = |l: &[(P2, f64)]| l.windows(2).any(|w| w[0].0.dist(w[1].0) <= 1.0 + 1e-9 && (w[1].1 - w[0].1) > 100.0);
        assert!(!upright(&line(&s)), "climbing on a slope, by default");
        let name = s.model.types.railing.keys().next().cloned().unwrap();
        run(&mut s, "type", json!({"category": "railing", "name": name, "transition": "gooseneck"}));
        let l = line(&s);
        assert!(upright(&l), "straight up where the next flight starts: {l:?}");
    }

    #[test]
    fn a_line_goes_straight_while_it_may_be_higher_and_breaks_past_that() {
        // Flat for a metre, then up 50 mm on a gentle wave, then flat: sampled every 100 mm.
        let line: Vec<(P2, f64, bool)> = (0..=30)
            .map(|i| {
                let x = f64::from(i) * 100.0;
                let z = if x < 1000.0 {
                    0.0
                } else if x < 2000.0 {
                    50.0 * ((x - 1000.0) / 1000.0)
                } else {
                    50.0
                };
                (P2(x, 0.0), z, i == 0 || i == 30)
            })
            .collect();
        let check = |out: &[(P2, f64)], extra: f64| {
            for (p, z, _) in &line {
                let w = out.windows(2).find(|w| w[0].0.0 <= p.0 + 1e-9 && w[1].0.0 >= p.0 - 1e-9).unwrap();
                let k = (p.0 - w[0].0.0) / (w[1].0.0 - w[0].0.0);
                let on = w[0].1 + (w[1].1 - w[0].1) * k;
                assert!(on >= z - 1e-6 && on <= z + extra + 1e-6, "{on} over {z} at {}", p.0);
            }
        };
        // 100 mm allowed: one straight section, end to end.
        let wide = straighten(&line, 100.0);
        assert_eq!(wide.len(), 2, "{wide:?}");
        check(&wide, 100.0);
        // 5 mm: it breaks where it would rise further over the line.
        let tight = straighten(&line, 5.0);
        assert!(tight.len() > 2, "{tight:?}");
        check(&tight, 5.0);
    }

    #[test]
    fn balusters_stand_from_the_start_round_the_middle_or_alone_and_a_railing_flips() {
        let mut s = Session::new();
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "B", "rails": [{"name": "Top", "width": 50, "height": 1000, "offset": 100}],
            "posts": [{"name": "Post", "width": 40, "spacing": 1000},
                      {"name": "Bar", "kind": "baluster", "width": 20, "spacing": 300, "justify": "start", "shift": 50, "top": "Top"}]}),
        );
        let id = run(&mut s, "railing", json!({"path": [[0, 0], [1000, 0]], "type": "B"}))["ids"][0].as_u64().unwrap();
        let bars = |s: &Session| -> Vec<f64> {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            parts(&s.model, rl)
                .iter()
                .filter(|p| p.which == Which::Post(1))
                .map(|p| (geom::centroid(&p.piece.prisms()[0].0).0 * 1000.0).round() / 1000.0)
                .collect()
        };
        // Clear of the posts (20 + 10 in from each end), from the start, 50 on, 300 apart.
        assert_eq!(bars(&s), vec![80.0, 380.0, 680.0]);
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "B", "posts": [{"name": "Post", "width": 40, "spacing": 1000},
            {"name": "Bar", "kind": "baluster", "width": 20, "spacing": 300, "justify": "centre", "top": "Top"}]}),
        );
        assert_eq!(bars(&s), vec![200.0, 500.0, 800.0]);
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "B", "posts": [{"name": "Post", "width": 40, "spacing": 1000},
            {"name": "Bar", "kind": "baluster", "width": 20, "single": true, "justify": "end", "shift": 100, "top": "Top"}]}),
        );
        assert_eq!(bars(&s), vec![870.0]);
        // Flipped, the rail 100 to the left goes 100 to the right.
        let rail_y = |s: &Session| {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            let Piece::Sweep { path, .. } = &parts(&s.model, rl)[0].piece else { panic!() };
            path[0][1]
        };
        assert_eq!(rail_y(&s), 100.0);
        run(&mut s, "modify", json!({"id": id, "flip": true}));
        assert_eq!(rail_y(&s), -100.0);
        // One kind of post only.
        let two = json!({"category": "railing", "name": "B", "posts": [{"name": "A", "width": 40}, {"name": "C", "width": 40}]});
        assert!(s.execute("type", &two).unwrap_err().contains("more than one kind of post"));
    }

    #[test]
    fn rails_apart_end_at_a_step_the_higher_running_on() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 8, "turn": -90}]}))["id"]
            .as_u64()
            .unwrap();
        let name = s.model.types.railing.keys().next().cloned().unwrap();
        run(&mut s, "type", json!({"category": "railing", "name": name, "transition": "separate"}));
        let id = run(&mut s, "railing", json!({"host": stair, "side": "left"}))["ids"][0].as_u64().unwrap();
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let rails = parts(&s.model, rl).iter().filter(|p| p.which == Which::Rail(0)).count();
        assert_eq!(rails, 2, "the landing's rail and the flight's, apart");
    }

    /// A straight stair with a landing half way, and its left railing's line: each point's distance up the stair (y)
    /// and height; the landing's two ends, its height, a riser and a going.
    /// The line up the stair (y, height), the landing's ends, its height, a riser and a going.
    type Landing = (Vec<(f64, f64)>, (f64, f64), f64, f64, f64);

    fn landing_line(transition: &str) -> Landing {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1200, "landings": [{"at": 8, "turn": 0}]}))["id"]
            .as_u64()
            .unwrap();
        let name = s.model.types.railing.keys().next().cloned().unwrap();
        run(&mut s, "type", json!({"category": "railing", "name": name, "transition": transition}));
        let id = run(&mut s, "railing", json!({"host": stair, "side": "left"}))["ids"][0].as_u64().unwrap();
        let l = crate::stairs::layout(&s.model, s.model.elements.get(stair).unwrap()).unwrap();
        let (a, b) = (&l.runs[0], &l.runs[1]);
        let each = (a.z.1 - a.z.0) / f64::from(a.risers);
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        let line = crate::stairs::railing(&s.model, rl).into_iter().map(|(p, z)| (p.1, z)).collect();
        (line, (a.end().1, b.start.1), a.z.1, each, a.going())
    }

    /// The line's height `y` up the stair.
    fn height_at(line: &[(f64, f64)], y: f64) -> f64 {
        let w = line.windows(2).find(|w| w[0].0 <= y + 1e-9 && w[1].0 >= y - 1e-9).unwrap();
        if w[1].0 - w[0].0 > 1e-9 { w[0].1 + (w[1].1 - w[0].1) * (y - w[0].0) / (w[1].0 - w[0].0) } else { w[1].1 }
    }

    #[test]
    fn over_a_landing_the_line_is_level_and_the_next_flight_runs_on_over_it() {
        let (line, (from, to), landing, each, going) = landing_line("continuous");
        // The line is the top of what the rails stand over: the landing's top, the nosings.
        let h = 0.0;
        // Level over the landing from where the flight below arrives, up to a going before the next flight.
        for y in [from + 1.0, (from + to) / 2.0, to - going - 1.0] {
            assert!((height_at(&line, y) - (landing + h)).abs() < 0.6, "level over the landing at {y}: {line:?}");
        }
        // Over the last going of the landing, the next flight's line run on at its slope: half a riser up half way.
        assert!((height_at(&line, to - going / 2.0) - (landing + h + each / 2.0)).abs() < 1.0, "{line:?}");
        // And on up the flight over its nosings, a riser up at its first.
        assert!((height_at(&line, to) - (landing + h + each)).abs() < 1.0);
        // Straight pieces only: up the flight, level, the run-on, up the next flight.
        assert!(line.len() <= 6, "{line:?}");
    }

    #[test]
    fn a_gooseneck_runs_level_past_the_step_and_goes_straight_up() {
        let (line, (_, to), landing, each, _) = landing_line("gooseneck");
        let h = 0.0;
        // Up straight 50 mm (the extension) before the flight, the higher side level over that 50.
        let rise = line.windows(2).find(|w| (w[1].0 - w[0].0).abs() <= 1.0 + 1e-9 && w[1].1 - w[0].1 > 100.0).unwrap();
        assert!((rise[0].0 - (to - 50.0)).abs() < 1.0, "{line:?}");
        assert!((rise[0].1 - (landing + h)).abs() < 0.6 && (rise[1].1 - (landing + h + each)).abs() < 0.6);
        assert!((height_at(&line, to - 20.0) - (landing + h + each)).abs() < 0.6, "level over the extension");
    }
}
