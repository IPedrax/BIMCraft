//! Railings' parts and hosts: the rails, posts, balusters and panels a railing type stacks along a railing's line
//! (`RailingType`'s `rails`, `posts` and `panel`, or the older kind's one rail and balusters), and the heights a railing
//! with several hosts (a stair and the floor it comes up to) follows along its line.

use crate::geom::{self, P2};
use crate::model::{
    Element, Guid, MAX_BALUSTERS, Model, PanelPart, PartAt, PartShape, Placing, PostKind, PostPart, RailPart, Railing, RailingPartEdit,
    RailingPartKind, RailingType, Transition,
};
use crate::runs::Geom;
use crate::view3d::P3;
use crate::walls::Plane;

pub mod path;

pub use path::sweep;
use path::{circle, rectangle};

/// The older kind's one rail, which its balusters go up to.
pub const TOP_RAIL: &str = "Top Rail";

/// The most points looked up along one railing.
const MAX_SAMPLES: usize = 4000;
/// How far a railing's line turns, in degrees, at a corner a post stands on: less, it is a curve drawn in pieces.
const CORNER: f64 = 30.0;

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

/// A piece of a railing, what it is made of ("" for the railing's grey), which part of its type it is and which of the
/// railing's.
#[derive(Clone, Debug)]
pub struct Part {
    pub piece: Piece,
    pub material: String,
    pub which: Which,
    /// Which it is of the railing's: what its own changes name it by.
    pub at: PartAt,
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

    /// How far along its ends and its corners are, where it turns in plan by [`CORNER`] or more (less, it curves).
    fn stations(&self) -> Vec<f64> {
        let mut out = vec![0.0];
        for (i, w) in self.pts.windows(3).enumerate() {
            let [(a, _), (b, _), (c, _)] = w else { continue };
            if (*b - *a).unit().dot((*c - *b).unit()) < CORNER.to_radians().cos() {
                out.extend(self.at.get(i + 1));
            }
        }
        out.push(self.length());
        out
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

/// A railing's pieces: its rails along its line, its posts and balusters, and its panels, as its type makes them and its
/// own changes leave them.
pub fn parts(m: &Model, rl: &Railing) -> Vec<Part> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    let line = crate::stairs::railing(m, rl);
    let plan: Vec<P2> = line.iter().map(|(p, _)| *p).collect();
    let ground = Ground::of(m, &hosts(rl), &plan, rl.base_offset);
    let mut out = build(t, line, &ground, rl.flip);
    edited(&mut out, &rl.parts);
    out
}

/// `parts` as `edits` change them: one left out gone, another material, a post's or a baluster's height from its bottom,
/// a rail's ends run on.
pub fn edited(parts: &mut Vec<Part>, edits: &[RailingPartEdit]) {
    if edits.is_empty() {
        return;
    }
    parts.retain(|p| !edits.iter().any(|e| e.excluded && e.at == p.at));
    for p in parts.iter_mut() {
        let Some(e) = edits.iter().find(|e| e.at == p.at) else { continue };
        if let Some(mat) = &e.material {
            p.material.clone_from(mat);
        }
        match &mut p.piece {
            Piece::Prism(_, bottom, top) => {
                if let Some(h) = e.height.filter(|h| *h > 0.0 && p.at.kind != RailingPartKind::Panel) {
                    *top = Plane::flat(bottom.z + h);
                }
            }
            Piece::Tube(g) => {
                if let Some(h) = e.height.filter(|h| *h > 0.0) {
                    g.b[2] = g.a[2] + h;
                }
            }
            Piece::Sweep { path, .. } => {
                *path = path::extended(path, e.extend_start, e.extend_end);
            }
        }
    }
}

/// The steps of a flight, going up: each one's middle in plan; and how far from them a railing beside it may be.
/// What is under a point of a railing's line: a step (a run's tread, by run and number, or a winder's, by its landing
/// pad's place after the runs), a landing, or the ground (a ramp's slope, a floor's top), of which of its hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Under {
    Step(usize, usize, usize),
    Landing(usize, usize),
    Ground(usize),
}

/// A stretch of a railing's line over one thing: from and to how far along, what (none: nothing), how high its top is, and
/// for a step how high one riser is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stretch {
    pub from: f64,
    pub to: f64,
    pub under: Option<Under>,
    pub z: f64,
    pub rise: f64,
}

impl Stretch {
    fn step(&self) -> bool {
        matches!(self.under, Some(Under::Step(..)))
    }

    fn landing(&self) -> bool {
        matches!(self.under, Some(Under::Landing(..)))
    }
}

/// What a railing's posts and balusters stand on: the tops of its hosts' steps, landings and surfaces, and the flights of
/// steps its posts may be lined up with. None, they stand on its line.
pub struct Ground {
    hosts: Vec<Host>,
    lift: f64,
}

/// What is under a point, how high its top is and a step's riser, if anything is.
type Found = Option<(Under, f64, f64)>;

/// How far apart a line's heights are looked at, at most, to find where what is under it changes.
const FINE: f64 = 20.0;

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
        self.under(p).map(|(_, z, _)| z)
    }

    /// What is under `p` (the first host with anything there), the height of its top and a step's riser.
    fn under(&self, p: P2) -> Option<(Under, f64, f64)> {
        self.hosts.iter().enumerate().find_map(|(i, h)| h.under(i, p)).map(|(u, z, r)| (u, z + self.lift, r))
    }

    /// How steep the steepest flight is, rise over going.
    fn pitch(&self) -> f64 {
        self.hosts.iter().map(Host::pitch).fold(0.0, f64::max)
    }

    /// `plan` cut where what is under it changes: each stretch over one step, landing, ground or nothing, its ends found
    /// within a hundredth of a millimetre.
    pub fn stretches(&self, plan: &[P2]) -> Vec<Stretch> {
        if self.hosts.is_empty() {
            return Vec::new();
        }
        let total: f64 = plan.windows(2).map(|w| w[0].dist(w[1])).sum();
        let step = FINE.max(total / MAX_SAMPLES as f64);
        let key = |u: &Option<(Under, f64, f64)>| u.map(|(k, ..)| k);
        let mut out: Vec<Stretch> = Vec::new();
        let mut open: Option<(f64, Found)> = None;
        let mut s0 = 0.0;
        for w in plan.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.dist(b);
            if len < 1e-9 {
                continue;
            }
            let n = (len / step).ceil().max(1.0) as usize;
            let mut was: Option<(f64, Found)> = None;
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let here = self.under(a + (b - a) * t);
                let s = s0 + len * t;
                match open {
                    None => open = Some((s, here)),
                    Some((from, u)) if key(&u) != key(&here) => {
                        // Where it changes: halved between the last point looked at on this stretch and this one.
                        let (mut lo, mut hi) = (was.map_or(t, |(tw, _)| tw), t);
                        for _ in 0..32 {
                            let mid = (lo + hi) / 2.0;
                            if key(&self.under(a + (b - a) * mid)) == key(&u) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        let at = s0 + len * (lo + hi) / 2.0;
                        let (under, z, rise) = u.map_or((None, 0.0, 0.0), |(k, z, r)| (Some(k), z, r));
                        out.push(Stretch { from, to: at, under, z, rise });
                        open = Some((at, here));
                    }
                    _ => {}
                }
                was = Some((t, here));
            }
            s0 += len;
        }
        if let Some((from, u)) = open {
            let (under, z, rise) = u.map_or((None, 0.0, 0.0), |(k, z, r)| (Some(k), z, r));
            out.push(Stretch { from, to: s0, under, z, rise });
        }
        out.retain(|x| x.to - x.from > 1e-6);
        out
    }
}

/// The height of a stair's nosing line at each end of stretch `i`, a step: where it meets a higher step or landing, that
/// one's top; a lower one, its own; nothing (or the ground), its own top on its lower side, a riser over it on its higher.
fn nosed(all: &[Stretch], i: usize) -> (f64, f64) {
    let Some(x) = all.get(i) else { return (0.0, 0.0) };
    let side = |j: Option<usize>| -> Option<f64> {
        let y = all.get(j?)?;
        y.under.is_some().then_some(y.z.max(x.z))
    };
    // The slope of the step next to it on `j`'s side, where that one's ends are both known: an end open to nothing carries
    // it on, so a line starting short of its first riser (a curved one's) climbs as one slope.
    let next = |j: usize, k: usize| -> Option<f64> {
        let (y, z) = (all.get(j)?, all.get(k)?);
        let w = all.get(i)?;
        (y.step() && z.under.is_some() && y.to - y.from > 1.0).then(|| {
            let far = z.z.max(y.z);
            let near = w.z.max(y.z);
            (far - near) / (y.to - y.from)
        })
    };
    let len = x.to - x.from;
    let (a, b) = (side(i.checked_sub(1)), side(Some(i + 1)));
    match (a, b) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => {
            let k = i.checked_sub(2).and_then(|k| next(i - 1, k)).map(|g| -g);
            (a, k.map_or(if a > x.z + 0.5 { x.z } else { x.z + x.rise }, |g| a + g * len))
        }
        (None, Some(b)) => {
            let k = next(i + 1, i + 2);
            (k.map_or(if b > x.z + 0.5 { x.z } else { x.z + x.rise }, |g| b - g * len), b)
        }
        (None, None) => (x.z, x.z),
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
        let pieces = path::rails(&moved, r, t.transition == Transition::Separate);
        let last = pieces.len().saturating_sub(1);
        for (k, path) in pieces.into_iter().enumerate() {
            let path = path::extended(&path, if k == 0 { r.extend_start } else { 0.0 }, if k == last { r.extend_end } else { 0.0 });
            out.push(Part {
                piece: Piece::Sweep { path, profile: profile.clone(), width: r.width, depth: d, round },
                material: r.material.clone(),
                which: Which::Rail(ri),
                at: PartAt { kind: RailingPartKind::Rail, name: r.name.clone(), index: k },
            });
        }
    }
    let rail = |name: &Option<String>| name.as_ref().and_then(|n| rails.iter().find(|r| &r.name == n));
    let plan: Vec<P2> = line.pts.iter().map(|(p, _)| *p).collect();
    let stretches = ground.stretches(&plan);
    let stations = line.stations();
    let mut uprights: Vec<(usize, &PostPart, f64)> = Vec::new();
    for (pi, p) in posts.iter().enumerate().filter(|(_, p)| p.kind == PostKind::Post) {
        let depth = section(p.shape, p.width, p.depth);
        uprights.extend(post_places(p, depth, &line, &stations, &stretches).into_iter().map(|s| (pi, p, s)));
    }
    let mut at_posts: Vec<f64> = uprights.iter().map(|(_, _, s)| *s).collect();
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
    // Each named the how-manyth of its part it is along the line.
    uprights.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.total_cmp(&b.2)));
    let mut counted: Vec<usize> = vec![0; posts.len()];
    for (pi, p, s) in uprights {
        let Some((at, z, u)) = line.point(s) else { continue };
        let index = counted.get(pi).copied().unwrap_or(0);
        if let Some(c) = counted.get_mut(pi) {
            *c += 1;
        }
        let c = at + u.perp() * p.offset;
        // On the step, the landing or the floor under it (moved past the stair's side, on the tread the railing's line is
        // over there); on a rail, that rail's top.
        let bottom = match (p.kind, rail(&p.bottom)) {
            (PostKind::Baluster, Some(r)) => z + r.height + p.bottom_offset,
            _ => ground.at(c).or_else(|| ground.at(at)).unwrap_or(z) + p.bottom_offset,
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
        let kind = if p.kind == PostKind::Post { RailingPartKind::Post } else { RailingPartKind::Baluster };
        out.push(Part { piece, material: p.material.clone(), which: Which::Post(pi), at: PartAt { kind, name: p.name.clone(), index } });
    }
    if let Some(pn) = &panel {
        // In from each post's face: the deepest post's half along the line.
        let half = post_half;
        let rise = match rail(&pn.top) {
            Some(r) => r.height - rail_depth(r) - pn.top_gap,
            None => top - pn.top_gap,
        };
        // Its bottom over what the railing stands on (a stair's nosings, a landing), not over the rail's line, which may
        // run higher where it joins two heights.
        let floor = nosing_line(&line, &stretches);
        for (bay, &(s0, s1)) in bays.iter().enumerate() {
            let (a, b) = if posted { (s0 + half + pn.side_gap, s1 - half - pn.side_gap) } else { (s0 + pn.side_gap, s1 - pn.side_gap) };
            if b - a < 1.0 || rise - pn.bottom < 1.0 {
                continue;
            }
            // Cut where the rail's line bends and where the floor's does.
            let mut cuts: Vec<f64> = vec![a, b];
            cuts.extend(line.at.iter().copied().filter(|s| *s > a + 1e-6 && *s < b - 1e-6));
            cuts.extend(stretches.iter().flat_map(|x| [x.from, x.to]).filter(|s| *s > a + 1e-6 && *s < b - 1e-6));
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-3);
            for w in cuts.windows(2) {
                let &[sp, sq] = w else { continue };
                let (Some((p, zp, _)), Some((q, zq, _))) = (line.point(sp), line.point(sq)) else { continue };
                let len = p.dist(q);
                if len < 1e-6 {
                    continue;
                }
                let u = (q - p) * (1.0 / len);
                let (n, off) = (u.perp() * (pn.thickness / 2.0), u.perp() * pn.offset);
                let g = u * ((zq - zp) / len);
                let base = zp - g.dot(p);
                let ring = vec![p + off - n, q + off - n, q + off + n, p + off + n];
                // Each end as the stretch between them has it: where a landing meets a step, the step's height on the step's side.
                let near = ((sq - sp) / 2.0).min(0.01);
                let (fp, fq) = (floor(sp + near).unwrap_or(zp), floor(sq - near).unwrap_or(zq));
                let fg = u * ((fq - fp) / len);
                let bottom = Plane { z: fp - fg.dot(p) + pn.bottom, g: fg };
                out.push(Part {
                    piece: Piece::Prism(ring, bottom, Plane { z: base + rise, g }),
                    material: pn.material.clone(),
                    which: Which::Panel,
                    at: PartAt { kind: RailingPartKind::Panel, name: String::new(), index: bay },
                });
            }
        }
    }
    out
}

/// The height of what a railing stands on `s` along `line`: a stair's nosing line, a landing's top; the railing's line
/// where it stands on nothing it knows.
fn nosing_line<'a>(line: &'a Line, stretches: &'a [Stretch]) -> impl Fn(f64) -> Option<f64> + 'a {
    move |s: f64| {
        let i = stretches.iter().position(|x| s >= x.from - 1e-6 && s <= x.to + 1e-6)?;
        let x = stretches.get(i)?;
        match x.under? {
            Under::Step(..) => {
                let (za, zb) = nosed(stretches, i);
                let k = if x.to - x.from > 1e-9 { ((s - x.from) / (x.to - x.from)).clamp(0.0, 1.0) } else { 0.0 };
                Some(za + (zb - za) * k)
            }
            Under::Landing(..) => Some(x.z),
            Under::Ground(_) => line.point(s).map(|(_, z, _)| z),
        }
    }
}

/// Where along `line` post `p` (`depth` along it) stands: at the line's ends; at its corners; where each landing starts,
/// turns and ends; on the first and last step of each flight; on every so many steps, counted again from each post; and
/// between, where the line is not on steps lined up with, at most its spacing apart. Two closer than the post is deep
/// are one.
fn post_places(p: &PostPart, depth: f64, line: &Line, stations: &[f64], stretches: &[Stretch]) -> Vec<f64> {
    let len = line.length();
    let half = depth / 2.0;
    let over = |s: f64| stretches.iter().find(|x| s >= x.from - 1e-6 && s <= x.to + 1e-6);
    let mut fixed: Vec<f64> = Vec::new();
    if !p.skip_ends {
        fixed.extend([0.0, len]);
    }
    for &c in stations.iter().filter(|c| **c > 1e-6 && **c < len - 1e-6) {
        match over(c) {
            Some(x) if x.landing() && p.skip_landings => {}
            Some(x) if x.step() => {}
            _ => fixed.push(c),
        }
    }
    if !p.skip_landings {
        for x in stretches.iter().filter(|x| x.landing()) {
            if x.to - x.from <= 2.0 * depth + 20.0 {
                // Too short for one at each end: one, at its corner if it turns there (an inside corner), else its middle.
                let corner = stations.iter().copied().find(|c| *c > x.from && *c < x.to);
                fixed.push(corner.unwrap_or((x.from + x.to) / 2.0));
            } else {
                fixed.extend([x.from + half, x.to - half]);
            }
        }
    }
    // The flights: their steps' middles, in order along the line, each flight's apart.
    let mut flights: Vec<Vec<f64>> = Vec::new();
    for (i, x) in stretches.iter().enumerate() {
        if !x.step() {
            continue;
        }
        let mid = (x.from + x.to) / 2.0;
        let joined = i.checked_sub(1).and_then(|j| stretches.get(j)).is_some_and(|y| y.step() && (y.to - x.from).abs() < 1e-3);
        match flights.last_mut() {
            Some(f) if joined => f.push(mid),
            _ => flights.push(vec![mid]),
        }
    }
    if p.flight_ends {
        for f in &flights {
            fixed.extend(f.first());
            fixed.extend(f.last());
        }
    }
    fixed.sort_by(f64::total_cmp);
    fixed.dedup_by(|a, b| (*a - *b).abs() < depth.max(1.0));
    let mut here = fixed.clone();
    if p.steps > 0 {
        // Counted from each post: a post on the step that is the so-manyth since the last.
        let mut k = 0;
        let mut count = 0;
        for &s in flights.iter().flatten() {
            let mut on_post = false;
            while let Some(&f) = fixed.get(k) {
                if f > s + half.max(1.0) {
                    break;
                }
                on_post |= (f - s).abs() <= half.max(1.0);
                count = 0;
                k += 1;
            }
            if on_post {
                continue;
            }
            count += 1;
            if count == p.steps as usize {
                here.push(s);
                count = 0;
            }
        }
    }
    here.sort_by(f64::total_cmp);
    here.dedup_by(|a, b| (*a - *b).abs() < depth.max(1.0));
    // Between, at most their spacing apart: off steps the posts are lined up with.
    let mut filled = here.clone();
    for w in here.windows(2) {
        let &[a, b] = w else { continue };
        if p.steps > 0 && over((a + b) / 2.0).is_some_and(Stretch::step) {
            continue;
        }
        filled.extend(inside(a, b, p.spacing));
    }
    filled.sort_by(f64::total_cmp);
    filled.into_iter().filter(|s| *s >= -1e-6 && *s <= len + 1e-6).collect()
}

/// A host a railing's heights follow: a stair (its flights' runs, its landings and winders, and how high a riser is), a
/// ramp (its walk line and half its width), or a surface (a floor's, a roof's or a toposolid's top, as the 3D view has
/// it).
enum Host {
    Stair { runs: Vec<crate::stairs::Run>, pads: Vec<crate::stairs::Pad>, rise: f64 },
    Ramp(Vec<(P2, f64)>, f64),
    Surface(Vec<crate::view3d::Solid>),
}

/// How near the walk line, past half a stair's width, a point is still over it.
const NEAR: f64 = 1.0;
/// How far outside a landing or a winder a point is still taken as on it: a curved flight's railing, whose line cuts
/// each tread's arc short.
const NEAR_PAD: f64 = 60.0;

impl Host {
    /// Host `g`, of what lies near `plan`.
    fn of(m: &Model, g: Guid, plan: &[P2]) -> Option<Host> {
        let e = m.elements.get(g)?;
        match e {
            Element::Stair(st) => {
                let l = crate::stairs::layout(m, e)?;
                Some(Host::Stair { runs: l.runs, pads: l.pads, rise: m.flight(st).1 })
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

    /// Where `p` is over a run of a stair's steps: how far along it from its first riser, which run it is, and the run.
    fn on_run(runs: &[crate::stairs::Run], p: P2) -> Option<(f64, usize, &crate::stairs::Run)> {
        runs.iter().enumerate().find_map(|(i, r)| {
            let (a, c) = ((p - r.start).dot(r.u), (p - r.start).dot(r.u.perp()));
            (a >= -1e-6 && a <= r.length + 1e-6 && c.abs() <= r.width / 2.0 + NEAR && r.risers > 1).then_some((a, i, r))
        })
    }

    /// What of it is under `p` (it being host `i`), how high its top is there, and for a step how high a riser is: a
    /// run's tread, a winder, a landing; a ramp's slope; a surface's top.
    fn under(&self, i: usize, p: P2) -> Option<(Under, f64, f64)> {
        match self {
            Host::Stair { runs, pads, rise } => {
                if let Some((a, ri, r)) = Host::on_run(runs, p) {
                    let each = (r.z.1 - r.z.0) / f64::from(r.risers);
                    let going = r.going();
                    let k = if going > 0.0 { (a / going).floor().clamp(0.0, f64::from(r.risers - 2)) } else { 0.0 };
                    return Some((Under::Step(i, ri, k as usize), (r.z.0 + each * (k + 1.0)).min(r.z.1), each));
                }
                // A landing flat; a winder (a spiral's step, a curved flight's) a step of its own. Past its flights,
                // landings and winders (before its foot, past its top) it is no host: what is there is.
                let near = pads.iter().position(|q| geom::contains(&q.ring, p)).or_else(|| {
                    pads.iter()
                        .enumerate()
                        .map(|(j, q)| (j, edge_distance(&q.ring, p)))
                        .filter(|(_, d)| *d <= NEAR_PAD)
                        .min_by(|x, y| x.1.total_cmp(&y.1))
                        .map(|(j, _)| j)
                })?;
                let pad = pads.get(near)?;
                Some(if pad.tread { (Under::Step(i, runs.len() + near, 0), pad.z, *rise) } else { (Under::Landing(i, near), pad.z, 0.0) })
            }
            Host::Ramp(walk, half) => {
                let (d, z) = Host::on_walk(walk, p)?;
                (d <= half + NEAR).then_some((Under::Ground(i), z, 0.0))
            }
            Host::Surface(solids) => surface(solids, p).map(|z| (Under::Ground(i), z, 0.0)),
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
}

/// How far `p` is from the edge of `ring`.
fn edge_distance(ring: &[P2], p: P2) -> f64 {
    (0..ring.len()).filter_map(|k| Some(geom::dist_to_segment(p, *ring.get(k)?, *ring.get((k + 1) % ring.len())?))).fold(f64::MAX, f64::min)
}

/// The highest top of `solids` over `p`, or within 2 mm of it (a railing along a floor's edge).
fn surface(solids: &[crate::view3d::Solid], p: P2) -> Option<f64> {
    let at = |q: P2| solids.iter().filter(|s| s.over(q)).map(|s| s.z_at(q).1).reduce(f64::max);
    at(p).or_else(|| [P2(2.0, 0.0), P2(-2.0, 0.0), P2(0.0, 2.0), P2(0.0, -2.0)].into_iter().find_map(|d| at(p + d)))
}

/// A point of a line: where it is in plan, its height, and whether it is a corner of the line's own.
type Point = (P2, f64, bool);

/// A run-on walked into the lower side: the points it raises and to what, and the point where it meets the line there,
/// with the place it goes in.
type Walked = (Vec<(usize, f64)>, Option<(usize, Point)>);

/// How far apart in plan the two points of a jump in a line's heights are: one each side of where it steps.
const JUMP: f64 = 0.5;

/// `line` with each jump (heights changing faster than twice the steepest flight between two points) joined as a railing
/// is built: the higher side's line run on at its own slope into the lower side until it meets the line there, every
/// point it passes raised to it, as long as it stays within `extra` over it; where that side does not slope down into the
/// lower one, or it would rise more than `extra` over the line, a joining slope twice as steep as the steepest flight
/// instead.
fn envelope(mut line: Vec<Point>, steepest: f64, extra: f64) -> Vec<Point> {
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
    let mut inserts: Vec<(usize, Point)> = Vec::new();
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
        // Walked from the step into the lower side at `rate`: the points raised and where it meets the line there. The
        // higher line run on goes as far as it stays within `extra` of the line under it (over a flight beside it at an
        // inner corner, all the way, one straight line); a joining slope, within `reach`.
        let order: Vec<usize> = if up { (0..=i).rev().collect() } else { (i + 1..n).collect() };
        let walk = |(rate, own): (f64, bool)| -> Option<Walked> {
            let mut raised = Vec::new();
            let mut before: Option<usize> = None;
            for &j in &order {
                let away = (along[j] - sh).abs();
                let e = zh - rate * away;
                if e > z[j] + 1e-9 {
                    if if own { e - z[j] > extra + 1e-6 } else { away > reach } {
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
        let run_on = (fall > 1e-3 && step <= extra + 1e-6).then_some((fall, true));
        let found = run_on.into_iter().chain([2.0, 3.0, 5.0, 8.0].map(|k| (k * steepest, false))).find_map(walk);
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

/// `line` with each jump (two points [`JUMP`] apart in plan, at different heights) moved `overhang` into the lower side,
/// the higher side's height held over that stretch: the higher part runs on over the lower, never the lower under the
/// higher.
fn upright(line: Vec<Point>, overhang: f64) -> Vec<Point> {
    let n = line.len();
    let mut along = vec![0.0; n];
    for i in 1..n {
        along[i] = along[i - 1] + line[i].0.dist(line[i - 1].0);
    }
    let total = along.last().copied().unwrap_or(0.0);
    // Each jump: where it moves to, the stretch the higher height holds over, and the two heights.
    let mut jumps: Vec<(f64, (f64, f64), f64, f64)> = Vec::new();
    for i in 1..n {
        let (q, p) = (&line[i - 1], &line[i]);
        if p.0.dist(q.0) <= 1.0 + 1e-9 && (p.1 - q.1).abs() > 1.0 {
            let mid = (along[i - 1] + along[i]) / 2.0;
            let at = if p.1 > q.1 { mid - overhang } else { mid + overhang };
            let at = at.clamp(JUMP, (total - JUMP).max(JUMP));
            let held = if at < mid { (at, mid) } else { (mid, at) };
            jumps.push((at, held, q.1, p.1));
        }
    }
    if jumps.is_empty() || overhang <= 0.0 {
        return line;
    }
    let point = |s: f64| -> (P2, P2) {
        let k = along.partition_point(|a| *a <= s).clamp(1, n.saturating_sub(1).max(1));
        let (Some(&(a, ..)), Some(&(b, ..))) = (line.get(k - 1), line.get(k)) else { return (P2(0.0, 0.0), P2(1.0, 0.0)) };
        let (sa, sb) = (along.get(k - 1).copied().unwrap_or(0.0), along.get(k).copied().unwrap_or(0.0));
        let t = if sb > sa { (s - sa) / (sb - sa) } else { 0.0 };
        (a + (b - a) * t, (b - a).unit())
    };
    let mut out: Vec<Point> = Vec::with_capacity(n + 2 * jumps.len());
    let mut next = jumps.iter().peekable();
    for (i, &(p, z, own)) in line.iter().enumerate() {
        let s = along[i];
        while let Some(&&(at, _, before, after)) = next.peek() {
            if at > s {
                break;
            }
            let (q, u) = point(at);
            out.push((q - u * (JUMP / 2.0), before, false));
            out.push((q + u * (JUMP / 2.0), after, false));
            next.next();
        }
        // Over the stretch the step moved across, the higher side's height.
        let z = jumps
            .iter()
            .fold(z, |z, &(_, (lo, hi), before, after)| if s > lo - 1e-9 && s < hi + 1e-9 { z.max(before.max(after)) } else { z });
        out.push((p, z, own));
    }
    out
}

/// The hosts a railing's heights follow: its stair or ramp first, then its `hosts`.
pub fn hosts(rl: &Railing) -> Vec<Guid> {
    rl.host.into_iter().chain(rl.hosts.iter().copied()).collect()
}

/// `plan`, its points given the heights of `hosts` under them (the first over each), `lift` above, as railing type `t`
/// makes its line: a stair's steps by their nosing line, each landing level, the ground's own heights looked up every
/// [`FINE`] along it (where over none, the height it had last, the first it has, or `fallback` over none at all); where
/// the heights jump (a flight a riser over the landing before it), joined as the type's `transition` says
/// ([`envelope`], [`upright`]); and a point kept only where the heights bend ([`straighten`] within half a millimetre).
pub fn follow(m: &Model, hosts: &[Guid], plan: &[P2], lift: f64, fallback: f64, t: &RailingType) -> Vec<(P2, f64)> {
    let ground = Ground::of(m, hosts, plan, lift);
    let stretches = ground.stretches(plan);
    // How far along each of the line's own points is.
    let mut own: Vec<(f64, P2)> = Vec::with_capacity(plan.len());
    let mut s = 0.0;
    for (i, p) in plan.iter().enumerate() {
        if let Some(q) = i.checked_sub(1).and_then(|j| plan.get(j)) {
            s += q.dist(*p);
        }
        own.push((s, *p));
    }
    let total = s;
    let at = |s: f64| -> P2 {
        let k = own.partition_point(|(a, _)| *a <= s).clamp(1, own.len().saturating_sub(1).max(1));
        match (own.get(k - 1), own.get(k)) {
            (Some(&(sa, a)), Some(&(sb, b))) if sb - sa > 1e-12 => a + (b - a) * ((s - sa) / (sb - sa)).clamp(0.0, 1.0),
            (Some(&(_, a)), _) => a,
            _ => P2(0.0, 0.0),
        }
    };
    let step = FINE.max(total / MAX_SAMPLES as f64);
    // Each stretch's points: its ends, the line's own corners on it, and on the ground, every so far.
    let first = stretches.iter().find(|x| x.under.is_some()).map_or(fallback, |x| x.z);
    let mut line: Vec<Point> = Vec::with_capacity(plan.len() + 4 * stretches.len());
    let mut held = first;
    let push = |line: &mut Vec<Point>, s: f64, z: f64, corner: bool| {
        let p = at(s);
        if let Some(last) = line.last_mut()
            && last.0.dist(p) < 1e-6
            && (last.1 - z).abs() < 1e-6
        {
            last.2 |= corner;
            return;
        }
        line.push((p, z, corner));
    };
    if stretches.is_empty() {
        return plan.iter().map(|p| (*p, fallback)).collect();
    }
    for (i, x) in stretches.iter().enumerate() {
        let corners: Vec<f64> = own.iter().map(|(s, _)| *s).filter(|s| *s > x.from + 1e-6 && *s < x.to - 1e-6).collect();
        // Each side of where two stretches meet at different heights, a point JUMP apart.
        let (from, to) =
            (if i == 0 { x.from } else { x.from + JUMP / 2.0 }, if i + 1 == stretches.len() { x.to } else { x.to - JUMP / 2.0 });
        let z_at: Box<dyn Fn(f64) -> f64> = match x.under {
            Some(Under::Step(..)) => {
                let (za, zb) = nosed(&stretches, i);
                let (a, b) = (x.from, x.to);
                Box::new(move |s: f64| if b - a > 1e-9 { za + (zb - za) * ((s - a) / (b - a)).clamp(0.0, 1.0) } else { za })
            }
            Some(Under::Landing(..)) => {
                let z = x.z;
                Box::new(move |_| z)
            }
            Some(Under::Ground(_)) => {
                let (h, g) = (held, &ground);
                Box::new(move |s: f64| g.under(at(s)).map_or(h, |(_, z, _)| z))
            }
            None => {
                let z = held;
                Box::new(move |_| z)
            }
        };
        let mut points: Vec<(f64, bool)> = vec![(from, own.iter().any(|(s, _)| (*s - x.from).abs() < 1e-6))];
        if matches!(x.under, Some(Under::Ground(_))) {
            let n = ((to - from) / step).ceil().max(1.0) as usize;
            points.extend((1..n).map(|k| (from + (to - from) * k as f64 / n as f64, false)));
        }
        points.extend(corners.into_iter().map(|s| (s, true)));
        points.push((to, own.iter().any(|(s, _)| (*s - x.to).abs() < 1e-6)));
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (s, corner) in points {
            let z = z_at(s);
            push(&mut line, s, z, corner);
        }
        if x.under.is_some() {
            held = z_at(to);
        }
    }
    // Where a flight begins a riser over the landing before it, the heights jump. Joined, the higher line runs on at its
    // own slope over the lower part until it meets that part's line (higher there than it need be, by at most the step);
    // a gooseneck or rails apart, it goes straight up or down.
    let steepest = match ground.pitch() {
        p if p > 0.0 => p,
        _ => stretches.iter().filter(|x| x.step() && x.to - x.from > 1.0).map(|x| x.rise / (x.to - x.from)).fold(0.0, f64::max),
    };
    if steepest > 0.0 && t.transition != Transition::Continuous {
        line = upright(line, t.extension.max(0.0));
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
        for (k, v) in
            [("offset", r.offset), ("bend", r.bend), ("corner", r.corner), ("extend_start", r.extend_start), ("extend_end", r.extend_end)]
        {
            number(&format!("{what} {k}"), v)?;
        }
        if r.extend_start < 0.0 || r.extend_end < 0.0 {
            return Err(format!("{what}: a rail runs on 0 mm or more past its ends"));
        }
        material(&what, &r.material)?;
        check_values(m, &what, RailingPartKind::Rail.category(), &r.params, crate::model::Scope::Type)?;
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
        let category = if p.kind == PostKind::Post { RailingPartKind::Post } else { RailingPartKind::Baluster }.category();
        check_values(m, &what, category, &p.params, crate::model::Scope::Type)?;
    }
    if let Some(pn) = &t.panel {
        let what = format!("{who} panel");
        above_zero(&format!("{what} thickness"), pn.thickness)?;
        for (k, v) in [("offset", pn.offset), ("bottom", pn.bottom), ("top_gap", pn.top_gap), ("side_gap", pn.side_gap)] {
            number(&format!("{what} {k}"), v)?;
        }
        rail(&what, &pn.top)?;
        material(&what, &pn.material)?;
        check_values(m, &what, RailingPartKind::Panel.category(), &pn.params, crate::model::Scope::Type)?;
    }
    Ok(())
}

/// The most parts of one railing changed by hand.
pub const MAX_EDITS: usize = 4096;

/// Every part of railing `rl` as its type makes it, before its own changes.
pub fn made(m: &Model, rl: &Railing) -> Vec<Part> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    let line = crate::stairs::railing(m, rl);
    let plan: Vec<P2> = line.iter().map(|(p, _)| *p).collect();
    build(t, line, &Ground::of(m, &hosts(rl), &plan, rl.base_offset), rl.flip)
}

/// Part `at` of railing `rl` as it stands: its pieces' edges, each a closed or open line of points, for a view to mark it.
pub fn part_edges(m: &Model, rl: &Railing, at: &PartAt) -> Vec<Vec<P3>> {
    let mut out = Vec::new();
    for part in parts(m, rl).iter().filter(|p| p.at == *at) {
        match &part.piece {
            Piece::Sweep { path, width, depth, .. } => {
                // Its centre line, and the line along each of its top corners.
                let (hw, hd) = (width / 2.0, depth / 2.0);
                for (across, up) in [(0.0, 0.0), (hw, hd), (-hw, hd), (hw, -hd), (-hw, -hd)] {
                    let line: Vec<P3> = path
                        .iter()
                        .enumerate()
                        .map(|(i, q)| {
                            let (a, b) = (path.get(i.saturating_sub(1)).unwrap_or(q), path.get(i + 1).unwrap_or(q));
                            let u = P2(b[0] - a[0], b[1] - a[1]).unit();
                            let n = u.perp() * across;
                            [q[0] + n.0, q[1] + n.1, q[2] + up]
                        })
                        .collect();
                    out.push(line);
                }
            }
            piece => {
                for (ring, bottom, top) in piece.prisms() {
                    let lo: Vec<P3> = ring.iter().map(|q| [q.0, q.1, bottom.at(*q)]).collect();
                    let hi: Vec<P3> = ring.iter().map(|q| [q.0, q.1, top.at(*q)]).collect();
                    for (a, b) in lo.iter().zip(&hi) {
                        out.push(vec![*a, *b]);
                    }
                    out.push(lo.iter().chain(lo.first()).copied().collect());
                    out.push(hi.iter().chain(hi.first()).copied().collect());
                }
            }
        }
    }
    out
}

/// The parts of railing `rl` a ray from `a` through `b` passes through, nearest first, each once.
pub fn parts_on_ray(m: &Model, rl: &Railing, a: P3, b: P3) -> Vec<PartAt> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let mut hits: Vec<(f64, PartAt)> = Vec::new();
    for part in parts(m, rl) {
        let tris: Vec<[P3; 3]> = match &part.piece {
            Piece::Sweep { path, profile, .. } => sweep(path, profile).0,
            piece => piece.prisms().iter().flat_map(|(ring, bottom, top)| prism_tris(ring, bottom, top)).collect(),
        };
        if let Some(t) = tris.iter().filter_map(|t| ray_hit(a, d, t)).reduce(f64::min) {
            hits.push((t, part.at));
        }
    }
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<PartAt> = Vec::new();
    for (_, at) in hits {
        if !out.contains(&at) {
            out.push(at);
        }
    }
    out
}

/// The parts of railing `rl` over `p` in plan, or within `reach` of it: posts and balusters first, then rails, then
/// panels, nearest first among each.
pub fn parts_near(m: &Model, rl: &Railing, p: P2, reach: f64) -> Vec<PartAt> {
    let mut hits: Vec<(u8, f64, PartAt)> = Vec::new();
    for part in parts(m, rl) {
        let d = part
            .piece
            .prisms()
            .iter()
            .map(|(ring, ..)| if geom::contains(ring, p) { 0.0 } else { edge_distance(ring, p) })
            .fold(f64::MAX, f64::min);
        if d <= reach {
            let rank = match part.at.kind {
                RailingPartKind::Post | RailingPartKind::Baluster => 0,
                RailingPartKind::Rail => 1,
                RailingPartKind::Panel => 2,
            };
            hits.push((rank, d, part.at));
        }
    }
    hits.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
    let mut out: Vec<PartAt> = Vec::new();
    for (.., at) in hits {
        if !out.contains(&at) {
            out.push(at);
        }
    }
    out
}

/// A prism's skin as triangles: its sides and its two ends, fanned.
fn prism_tris(ring: &[P2], bottom: &Plane, top: &Plane) -> Vec<[P3; 3]> {
    let lo: Vec<P3> = ring.iter().map(|q| [q.0, q.1, bottom.at(*q)]).collect();
    let hi: Vec<P3> = ring.iter().map(|q| [q.0, q.1, top.at(*q)]).collect();
    let n = ring.len();
    let mut out = Vec::with_capacity(4 * n);
    for k in 0..n {
        let j = (k + 1) % n;
        out.push([lo[k], lo[j], hi[j]]);
        out.push([lo[k], hi[j], hi[k]]);
    }
    for k in 1..n.saturating_sub(1) {
        out.push([lo[0], lo[k], lo[k + 1]]);
        out.push([hi[0], hi[k], hi[k + 1]]);
    }
    out
}

/// How far along ray `a` + t·`d` it meets triangle `t`, if it does ahead of `a`.
fn ray_hit(a: P3, d: P3, t: &[P3; 3]) -> Option<f64> {
    let sub = |x: P3, y: P3| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let cross = |x: P3, y: P3| [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let dot = |x: P3, y: P3| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-12 {
        return None;
    }
    let f = 1.0 / det;
    let s = sub(a, t[0]);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let k = f * dot(e2, q);
    (k > 1e-9).then_some(k)
}

/// How IFC has a part of a railing: its class and its kind, and a user-defined kind's name.
pub fn ifc_class(kind: RailingPartKind) -> (&'static str, &'static str, Option<&'static str>) {
    match kind {
        RailingPartKind::Rail => ("IFCRAILING", "HANDRAIL", None),
        RailingPartKind::Post => ("IFCMEMBER", "POST", None),
        RailingPartKind::Baluster => ("IFCMEMBER", "USERDEFINED", Some("BALUSTER")),
        RailingPartKind::Panel => ("IFCPLATE", "USERDEFINED", Some("INFILLPANEL")),
    }
}

/// Whether `values` are values of the project's parameters for `category` of `scope`, each of its kind.
pub fn check_values(
    m: &Model,
    who: &str,
    category: &str,
    values: &std::collections::BTreeMap<String, serde_json::Value>,
    scope: crate::model::Scope,
) -> Result<(), String> {
    for (k, v) in values {
        let p = m
            .parameters
            .get(k)
            .filter(|p| p.categories.iter().any(|c| c == category))
            .ok_or_else(|| format!("{who}: `{k}` isn't a parameter of {category}s; add it with `parameter`"))?;
        if p.scope != scope {
            let (is, set) = if p.scope == crate::model::Scope::Type {
                ("type", "its railing type's part")
            } else {
                ("instance", "the part, with `railing_part`")
            };
            return Err(format!("{who}: `{k}` is an {is} parameter; set it on {set}"));
        }
        if !p.holds(v) {
            return Err(format!("{who}: `{k}` holds {}, not {v}", crate::parameters::kind_word(p.kind)));
        }
    }
    Ok(())
}

/// Whether railing `rl`'s parts changed by hand hold together: each named once, a height for a post or a baluster only and
/// above zero, a run-on for a rail only and not below zero, a material of the project's, and values of the project's
/// instance parameters for its category.
pub fn check_edits(m: &Model, who: &str, rl: &Railing) -> Result<(), String> {
    if rl.parts.len() > MAX_EDITS {
        return Err(format!("{who}: {} parts changed by hand; a railing has up to {MAX_EDITS}", rl.parts.len()));
    }
    for (i, e) in rl.parts.iter().enumerate() {
        let what = format!("{who} {}", named(&e.at));
        if rl.parts.get(..i).is_some_and(|before| before.iter().any(|x| x.at == e.at)) {
            return Err(format!("{what}: it is changed twice; change it once"));
        }
        if e.at.name.len() > crate::model::MAX_TEXT {
            return Err(format!("{what}: its name is longer than {} characters", crate::model::MAX_TEXT));
        }
        if let Some(h) = e.height {
            above_zero(&format!("{what} height"), h)?;
            if !matches!(e.at.kind, RailingPartKind::Post | RailingPartKind::Baluster) {
                return Err(format!("{what}: only a post or a baluster has a height of its own"));
            }
        }
        for (k, v) in [("extend_start", e.extend_start), ("extend_end", e.extend_end)] {
            number(&format!("{what} {k}"), v)?;
            if v < 0.0 {
                return Err(format!("{what}: {k} {v} mm; it is 0 or more"));
            }
            if v > 0.0 && e.at.kind != RailingPartKind::Rail {
                return Err(format!("{what}: only a rail runs on past its ends"));
            }
        }
        if let Some(mat) = e.material.as_ref().filter(|x| !x.is_empty() && !m.materials.contains_key(*x)) {
            return Err(format!("{what}: material `{mat}` isn't one of the project's"));
        }
        check_values(m, &what, e.at.kind.category(), &e.params, crate::model::Scope::Instance)?;
    }
    Ok(())
}

/// A part of a railing as a message names it: "post `Post` 3".
pub fn named(at: &PartAt) -> String {
    let kind = match at.kind {
        RailingPartKind::Rail => "rail",
        RailingPartKind::Post => "post",
        RailingPartKind::Baluster => "baluster",
        RailingPartKind::Panel => "panel",
    };
    if at.name.is_empty() { format!("{kind} {}", at.index) } else { format!("{kind} `{}` {}", at.name, at.index) }
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

    /// Where railing `id`'s posts stand, their middles in plan and their bottoms.
    fn post_spots(s: &Session, id: u64) -> Vec<(P2, f64)> {
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        parts(&s.model, rl)
            .iter()
            .filter(|p| p.at.kind == RailingPartKind::Post)
            .map(|p| {
                let (ring, bottom, _) = p.piece.prisms()[0].clone();
                (geom::centroid(&ring), bottom.z)
            })
            .collect()
    }

    #[test]
    fn posts_line_up_with_every_so_many_steps_counted_from_each_post_and_stand_on_the_treads() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1200}))["id"].as_u64().unwrap();
        let typed = |s: &mut Session, post: serde_json::Value| {
            run(
                s,
                "type",
                json!({"category": "railing", "name": "Stepped", "rails": [{"name": "Top", "width": 50, "height": 900}],
                "posts": [post]}),
            );
        };
        typed(&mut s, json!({"name": "Post", "width": 40, "steps": 3, "top": "Top"}));
        let id = run(&mut s, "railing", json!({"host": stair, "side": "left", "type": "Stepped"}))["ids"][0].as_u64().unwrap();
        // 17 risers, 16 treads 280 deep. A post at the railing's start and end; between, on every third tread counted from
        // the start's: treads 3, 6, 9, 12 and 15, each in the middle of its tread and on its top.
        let each = 3000.0 / 17.0;
        let tread = |y: f64| (y / 280.0 - 0.5).round() + 1.0;
        let posts = post_spots(&s, id);
        let ys: Vec<f64> = posts.iter().map(|(c, _)| c.1.round()).collect();
        assert_eq!(ys, vec![0.0, 700.0, 1540.0, 2380.0, 3220.0, 4060.0, 4480.0], "{posts:?}");
        for (c, z) in &posts[1..6] {
            assert!((z - tread(c.1) * each).abs() < 1e-6, "on its tread's top: {z} at {}", c.1);
        }
        // With each flight's first and last step: the count starts again from the first.
        typed(&mut s, json!({"name": "Post", "width": 40, "steps": 3, "flight_ends": true, "top": "Top"}));
        let steps: Vec<f64> = post_spots(&s, id).iter().map(|(c, _)| c.1).filter(|y| *y > 1.0 && *y < 4479.0).map(tread).collect();
        assert_eq!(steps, vec![1.0, 4.0, 7.0, 10.0, 13.0, 16.0]);
        // Without the ends: every third step from the stair's foot.
        typed(&mut s, json!({"name": "Post", "width": 40, "steps": 3, "skip_ends": true, "top": "Top"}));
        let steps: Vec<f64> = post_spots(&s, id).iter().map(|(c, _)| tread(c.1)).collect();
        assert_eq!(steps, vec![3.0, 6.0, 9.0, 12.0, 15.0]);
    }

    /// A stair going up north six risers, a landing turning left, six more west, a landing turning right, and the rest
    /// north: a Z.
    fn z_stair(s: &mut Session) -> u64 {
        run(s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1200, "landings": [{"at": 6, "turn": 90}, {"at": 12, "turn": -90}]}))
            ["id"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn on_a_z_stair_posts_stand_where_each_landing_starts_turns_and_ends_and_the_count_starts_again_after_them() {
        let mut s = Session::new();
        let stair = z_stair(&mut s);
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "Z", "rails": [{"name": "Top", "width": 50, "height": 900}],
            "posts": [{"name": "Post", "width": 40, "steps": 2, "spacing": 1000, "top": "Top"}],
            "panel": {"thickness": 10, "bottom": 100, "top": "Top"}}),
        );
        let l = crate::stairs::layout(&s.model, s.model.elements.get(stair).unwrap()).unwrap();
        for side in ["left", "right"] {
            let id = run(&mut s, "railing", json!({"host": stair, "side": side, "type": "Z"}))["ids"][0].as_u64().unwrap();
            let posts = post_spots(&s, id);
            // Each landing has a post on it: at its start, its corner and its end on the outer side; at its one corner
            // on the inner side, where the landing is only as long as the railing is in from the stair's edge.
            for pad in l.pads.iter().filter(|p| !p.tread) {
                let on: Vec<&(P2, f64)> = posts.iter().filter(|(c, _)| geom::contains(&pad.ring, *c)).collect();
                assert!(!on.is_empty(), "{side}: a post on the landing at {}: {posts:?}", pad.z);
                assert!(on.iter().all(|(_, z)| (*z - pad.z).abs() < 1e-6), "{side}: standing on it");
            }
            // No bay of panel is longer than three treads on a flight (the posts are on every second step after each), or
            // than the spacing on a landing.
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            let mut bays: std::collections::BTreeMap<usize, f64> = Default::default();
            for p in parts(&s.model, rl).iter().filter(|p| p.at.kind == RailingPartKind::Panel) {
                for (ring, ..) in p.piece.prisms() {
                    *bays.entry(p.at.index).or_default() += ring[0].dist(ring[1]);
                }
            }
            assert!(bays.values().all(|len| *len < 1000.0), "{side}: {bays:?}");
        }
    }

    #[test]
    fn a_curved_stair_s_railing_climbs_smoothly_over_its_nosings_and_its_posts_line_up_with_its_steps() {
        let mut s = Session::new();
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "C", "rails": [{"name": "Top", "width": 50, "height": 900}],
            "posts": [{"name": "Post", "width": 40, "steps": 3, "top": "Top"}]}),
        );
        let spiral =
            run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000, "spiral": {"radius": 1200}}))["id"].as_u64().unwrap();
        let arc = run(&mut s, "stair", json!({"start": [8000, 0], "path": [{"to": [8000, 3000], "arc": 120}]}))["id"].as_u64().unwrap();
        for st in [spiral, arc] {
            let treads =
                crate::stairs::layout(&s.model, s.model.elements.get(st).unwrap()).unwrap().pads.iter().filter(|p| p.tread).count();
            for side in ["left", "right"] {
                let id = run(&mut s, "railing", json!({"host": st, "side": side, "type": "C"}))["ids"][0].as_u64().unwrap();
                let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
                let line = crate::stairs::railing(&s.model, rl);
                // Going up all the way, never down, and with no step in it: each stretch's slope near the next one's.
                let slopes: Vec<f64> =
                    line.windows(2).filter(|w| w[0].0.dist(w[1].0) > 1.0).map(|w| (w[1].1 - w[0].1) / w[0].0.dist(w[1].0)).collect();
                assert!(slopes.iter().all(|g| *g > -1e-6), "{side}: never down: {line:?}");
                let (lo, hi) = slopes.iter().fold((f64::MAX, f64::MIN), |(lo, hi), g| (lo.min(*g), hi.max(*g)));
                assert!(hi - lo < 0.05, "{side}: one slope all the way, {lo} to {hi}: {line:?}");
                // Posts on every third step, and at the ends: not one at each corner the curve is drawn in.
                let posts = post_spots(&s, id).len();
                assert!(posts <= treads / 3 + 2 && posts >= treads / 3, "{side}: {posts} posts for {treads} steps");
            }
        }
    }

    #[test]
    fn a_part_of_a_railing_is_deleted_its_height_changed_its_ends_run_on_and_its_material_its_own() {
        let mut s = Session::new();
        run(&mut s, "material", json!({"name": "Oak", "color": [150, 100, 50]}));
        run(
            &mut s,
            "type",
            json!({"category": "railing", "name": "B", "rails": [{"name": "Top", "width": 50, "height": 1000, "extend_end": 300}],
            "posts": [{"name": "Post", "width": 40, "spacing": 1000}, {"name": "Bar", "kind": "baluster", "width": 20, "spacing": 200, "top": "Top"}]}),
        );
        let id = run(&mut s, "railing", json!({"path": [[0, 0], [3000, 0]], "type": "B"}))["ids"][0].as_u64().unwrap();
        let got = |s: &Session| {
            let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
            parts(&s.model, rl)
        };
        // Its type runs the rail on 300 past its end, level.
        let rail_end = |s: &Session| {
            let all = got(s);
            let Piece::Sweep { path, .. } = &all.iter().find(|p| p.at.kind == RailingPartKind::Rail).unwrap().piece else { panic!() };
            path.last().unwrap()[0]
        };
        assert!((rail_end(&s) - 3300.0).abs() < 1e-6);
        let posts = got(&s).iter().filter(|p| p.at.kind == RailingPartKind::Post).count();
        assert_eq!(posts, 4);
        let listed = run(&mut s, "railing_part", json!({"id": id}));
        let names: Vec<String> = listed["parts"].as_array().unwrap().iter().map(|p| format!("{}/{}", p["category"], p["ifc"])).collect();
        assert!(names.contains(&"\"railingpost\"/\"IFCMEMBER.POST\"".to_string()), "{names:?}");
        run(&mut s, "railing_part", json!({"id": id, "part": {"kind": "post", "name": "Post", "index": 1}, "exclude": true}));
        assert_eq!(got(&s).iter().filter(|p| p.at.kind == RailingPartKind::Post).count(), 3, "one left out");
        run(
            &mut s,
            "railing_part",
            json!({"id": id, "part": {"kind": "post", "name": "Post", "index": 2}, "height": 1500, "material": "Oak"}),
        );
        let tall = got(&s).into_iter().find(|p| p.at == PartAt { kind: RailingPartKind::Post, name: "Post".into(), index: 2 }).unwrap();
        let (_, bottom, top) = tall.piece.prisms()[0].clone();
        assert_eq!((top.z - bottom.z, tall.material.as_str()), (1500.0, "Oak"));
        run(
            &mut s,
            "railing_part",
            json!({"id": id, "part": {"kind": "rail", "name": "Top", "index": 0}, "extend_start": 200, "extend_end": 100}),
        );
        assert!((rail_end(&s) - 3400.0).abs() < 1e-6, "the railing's 100 on top of its type's 300");
        // A rail has no height of its own, and a part the railing doesn't have is named.
        let bad = s.execute("railing_part", &json!({"id": id, "part": {"kind": "rail", "name": "Top", "index": 0}, "height": 5}));
        assert!(bad.unwrap_err().contains("only a post or a baluster"));
        let none = s.execute("railing_part", &json!({"id": id, "part": {"kind": "post", "name": "Post", "index": 40}, "exclude": true}));
        assert!(none.unwrap_err().contains("has no post `Post` 40"));
        // A parameter of posts, each its own.
        run(&mut s, "parameter", json!({"name": "Finish", "kind": "text", "categories": ["railingpost"]}));
        run(
            &mut s,
            "railing_part",
            json!({"id": id, "part": {"kind": "post", "name": "Post", "index": 0}, "params": {"Finish": "Galvanised"}}),
        );
        let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
        assert_eq!(
            rl.parts.iter().find(|e| e.at.index == 0 && e.at.kind == RailingPartKind::Post).unwrap().params["Finish"],
            json!("Galvanised")
        );
        assert!(rl.parts.iter().all(|e| e.id != 0), "each change a part with an id");
        // Put back as the type makes it.
        run(
            &mut s,
            "railing_part",
            json!({"id": id, "parts": [{"kind": "post", "name": "Post", "index": 1}, {"kind": "post", "name": "Post", "index": 2}], "reset": true}),
        );
        assert_eq!(got(&s).iter().filter(|p| p.at.kind == RailingPartKind::Post).count(), 4);
        // In IFC: the railing made of its parts, each of its own class.
        let file = crate::ifc::export(&s.model);
        for class in ["IFCRAILING(", "IFCMEMBER(", ".POST.", "'BALUSTER'", ".HANDRAIL."] {
            assert!(file.contains(class), "{class}");
        }
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

    #[test]
    fn every_baluster_stands_on_the_tread_under_it_its_bottom_offset_over_that_wherever_across_the_railing_it_is() {
        let mut s = Session::new();
        let stair = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1200}))["id"].as_u64().unwrap();
        let each = 3000.0 / 17.0;
        let tread = |y: f64| (y / 280.0 - 0.5).round() + 1.0;
        for (offset, lift) in [(0.0, 0.0), (40.0, 0.0), (-40.0, 0.0), (40.0, 30.0), (0.0, -20.0), (80.0, 0.0), (-80.0, 25.0)] {
            run(
                &mut s,
                "type",
                json!({"category": "railing", "name": "Bars", "rails": [{"name": "Top", "width": 50, "height": 900}],
                "posts": [{"name": "Bar", "kind": "baluster", "width": 15, "spacing": 110, "offset": offset,
                    "bottom_offset": lift, "top": "Top"}]}),
            );
            for side in ["left", "right"] {
                let id = run(&mut s, "railing", json!({"host": stair, "side": side, "type": "Bars"}))["ids"][0].as_u64().unwrap();
                let Some(Element::Railing(rl)) = s.model.elements.get(id) else { panic!() };
                let bars: Vec<(P2, f64)> = parts(&s.model, rl)
                    .iter()
                    .filter(|p| p.at.kind == RailingPartKind::Baluster)
                    .map(|p| {
                        let (ring, bottom, _) = p.piece.prisms()[0].clone();
                        (geom::centroid(&ring), bottom.z)
                    })
                    .collect();
                // Those wholly over a tread, clear of its nosing by more than half a bar.
                let over: Vec<&(P2, f64)> = bars
                    .iter()
                    .filter(|(c, _)| c.1 > 10.0 && c.1 < 4470.0 && (c.1 / 280.0 - (c.1 / 280.0).round()).abs() * 280.0 > 10.0)
                    .collect();
                assert!(over.len() > 20, "{bars:?}");
                for (c, z) in over {
                    let want = tread(c.1) * each + lift;
                    assert!((z - want).abs() < 1e-6, "{side}, offset {offset}, {lift} up: {z} at {c:?}, the tread's {want}");
                }
                run(&mut s, "delete", json!({"ids": [id]}));
            }
        }
    }
}
