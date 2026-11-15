//! Steps 3 to 7: the shape as a profile along a bar, the depths, the sharing out of groups, the lengths and the rectangles.

use std::collections::{BTreeMap, BTreeSet};

mod interior;

use super::family::{Family, Shape, Use};
use super::fit::{Cl, Fail, Fitter, Group, LOSS_MAX, LOSS_MIN, Local, R, Range, Rnd, Wall, dn, rnd, share, up};
use super::links::{Linked, Rc, Verified, connect, outline, verify};
use super::plan::{Graph, Room, clusters};

/// Base depths tried.
const DEPTHS: [i64; 19] =
    [2400, 2700, 3000, 3300, 3600, 3900, 4200, 4500, 4800, 5100, 5400, 5700, 6000, 6300, 6600, 6900, 7200, 7500, 7800];

fn deep() -> impl Iterator<Item = i64> {
    (2400..=12_000).step_by(300)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Cells,
    /// The end room alone stands here.
    None,
    /// The spine runs along the outside wall here (an absent side).
    Free,
    /// A branch.
    Sub,
    /// One cell the length of the core: the end room's, which a required link ties to the start end room.
    Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orient {
    A,
    B,
    L,
    R,
}

#[derive(Clone, Debug)]
struct Piece {
    lv: i64,
    mode: Mode,
    groups: Vec<usize>,
    cap: bool,
    own: bool,
    first: Option<usize>,
    last: Option<usize>,
    p: i64,
    q: i64,
    delta: i64,
    len: i64,
    left: Vec<usize>,
    right: Vec<usize>,
}

fn place(rects: &[Local], ox: i64, oy: i64, orient: Orient) -> Vec<Rc> {
    rects
        .iter()
        .map(|&(room, u, v, du, dv)| match orient {
            Orient::A => (room, ox + u, oy + v, du, dv),
            Orient::B => (room, ox + u, oy - v - dv, du, dv),
            Orient::R => (room, ox + v, oy + u, dv, du),
            Orient::L => (room, ox - v - dv, oy + u, dv, du),
        })
        .collect()
}

/// Each piece's share of the bar's length, for side A and for side B.
fn fractions(shape: &str, rng: &mut Rnd) -> (Vec<f64>, Vec<f64>) {
    let mut u = |a: f64, b: f64| rng.uniform(a, b);
    match shape {
        "l_shape" | "corner_notch" => {
            let d = if shape == "l_shape" { u(0.3, 0.55) } else { u(0.62, 0.88) };
            (vec![1.0 - d, d], vec![1.0 - d, d])
        }
        "stepped" => {
            let (x, y) = (u(0.25, 0.4), u(0.25, 0.4));
            (vec![x, y, 1.0 - x - y], vec![x, y, 1.0 - x - y])
        }
        "t_shape" | "side_nub" => {
            let d = if shape == "t_shape" { u(0.36, 0.58) } else { u(0.13, 0.27) };
            let r = u(0.3, 0.7);
            let f = vec![(1.0 - d) * r, d, (1.0 - d) * (1.0 - r)];
            (f.clone(), f)
        }
        "opposite_wings" => {
            let (da, db) = (u(0.35, 0.6), u(0.35, 0.6));
            (vec![da, 1.0 - da], vec![1.0 - db, db])
        }
        "court" => {
            let (l1, l2) = (u(0.25, 0.38), u(0.25, 0.38));
            let f = vec![l1, 1.0 - l1 - l2, l2];
            (f.clone(), f)
        }
        _ => (vec![1.0], vec![1.0]),
    }
}

/// Does the outline keep the catalogue's proportions? `tops` and `bots`: the length and depth of each piece along the bar.
fn shape_ok(shape: &Shape, w: i64, s: i64, tops: &[(i64, i64)], bots: &[(i64, i64)]) -> bool {
    let top: Vec<i64> = tops.iter().map(|(_, d)| s + d).collect();
    let bot: Vec<i64> = bots.iter().map(|(_, d)| *d).collect();
    let (max_top, max_bot) = (top.iter().copied().max().unwrap_or(0), bot.iter().copied().max().unwrap_or(0));
    let d = max_top + max_bot;
    let bx = (w * d) as f64;
    let cut_a: Vec<i64> = tops.iter().zip(&top).map(|((l, _), t)| l * (max_top - t)).collect();
    let cut_b: Vec<i64> = bots.iter().zip(&bot).map(|((l, _), t)| l * (max_bot - t)).collect();
    let steps: Vec<i64> = top.windows(2).chain(bot.windows(2)).map(|p| (p[0] - p[1]).abs()).collect();
    if steps.iter().any(|st| *st > 0 && *st < 600) || tops.iter().chain(bots).any(|(l, _)| *l < 1200) {
        return false;
    }
    if d <= 0 || w <= 0 {
        return false;
    }
    let (lo, hi) = (shape.proportion.range[0], shape.proportion.range[1]);
    let within = |x: f64, lo: f64, hi: f64| x >= lo - 1e-9 && x <= hi + 1e-9;
    let r = w.max(d) as f64 / w.min(d) as f64;
    let sum_a = cut_a.iter().sum::<i64>() as f64 / bx;
    match shape.proportion.measure.as_str() {
        "ratio" => within(r, lo, hi),
        "cut" => within(sum_a, lo, hi) && (shape.id != "stepped" || steps.iter().take(2).all(|st| *st >= 600)),
        "stem" => {
            if top.len() < 2 || tops.len() < 2 {
                return false;
            }
            let deep = tops[1].0 as f64 / w as f64;
            let rise = top[1] - top[0];
            within(deep, lo, hi)
                && if shape.id == "side_nub" { rise >= 600 && rise as f64 <= 0.25 * d as f64 } else { rise as f64 >= 0.25 * d as f64 }
        }
        "wings" => [cut_a.iter().sum::<i64>(), cut_b.iter().sum::<i64>()].iter().all(|c| within(*c as f64 / bx, lo, hi)),
        _ => top.len() == 3 && top[0] == top[2] && within(cut_a[1] as f64 / bx, lo, hi),
    }
}

/// The turns: mirror left to right, then quarter turns anticlockwise, then move the plan so its bounding box starts at 0, 0.
fn turned_by(rects: &[Rc], mirror: bool, turns: usize) -> (Vec<Rc>, (i64, i64)) {
    let mut out: Vec<Rc> = rects
        .iter()
        .map(|&(room, mut x, mut y, mut w, mut h)| {
            if mirror {
                x = -x - w;
            }
            for _ in 0..turns {
                (x, y, w, h) = (-y - h, x, h, w);
            }
            (room, x, y, w, h)
        })
        .collect();
    let (mx, my) = (out.iter().map(|r| r.1).min().unwrap_or(0), out.iter().map(|r| r.2).min().unwrap_or(0));
    for r in &mut out {
        r.1 -= mx;
        r.2 -= my;
    }
    (out, (mx, my))
}

/// Corner points turned the way `turned` turned the rectangles, then moved by what it moved.
fn turned_points(points: &[(i64, i64)], mirror: bool, turns: usize, by: (i64, i64)) -> Vec<(i64, i64)> {
    points
        .iter()
        .map(|&(mut x, mut y)| {
            if mirror {
                x = -x;
            }
            for _ in 0..turns {
                (x, y) = (-y, x);
            }
            (x - by.0, y - by.1)
        })
        .collect()
}

/// A bump, as the manifest lists it: the room whose back wall moved, the side of the plan that wall is on, and how far it moved
/// (positive outward).
#[derive(Clone, Debug)]
pub struct Bump {
    pub room: usize,
    pub side: &'static str,
    pub offset: i64,
}

/// A laid-out plan: rectangles and links.
#[derive(Clone, Debug)]
pub struct Laid {
    pub rects: Vec<Rc>,
    pub linked: Linked,
    pub mirror: bool,
    pub quarter_turns: usize,
    pub verified: Verified,
    /// The interior the plan has: jogged, or aligned.
    pub jogged: bool,
    pub bumps: Vec<Bump>,
    /// The outline before the bumps, in the plan's frame.
    pub base_outline: Vec<(i64, i64)>,
}

/// What a cell is, for the changes made after the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A cell of a row or a branch.
    Band,
    /// An end room across the end of the bar.
    Cap,
    /// A cell a jog or a bump has moved already.
    Changed,
}

/// A cell as it was placed, so that it can be fitted again.
#[derive(Clone, Debug)]
struct Cell {
    cl: Cl,
    f: i64,
    d: i64,
    lat: (bool, bool),
    cover: Option<Range>,
    pref: usize,
    ox: i64,
    oy: i64,
    orient: Orient,
    kind: Kind,
    rooms: BTreeSet<usize>,
}

struct Ctx<'a> {
    fam: &'a Family,
    rooms: &'a [Room],
    g: &'a Graph,
    fx: &'a Fitter<'a>,
}

/// What the call asks of the interior: jogs, and bumps.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub jogged: bool,
    pub bumps: bool,
}

/// The groups of a plan: an arena, and the pool still to be dealt out.
struct Pool {
    groups: Vec<Group>,
    pool: Vec<usize>,
}

impl Pool {
    fn remove(&mut self, chosen: &[usize]) {
        self.pool.retain(|g| !chosen.contains(g));
    }
}

fn mid(fx: &Fitter, group: &Group, d: i64) -> R<f64> {
    let iv = fx.grange(group, d).or_else(|| deep().find_map(|x| fx.grange(group, x))).ok_or(Fail("room fits no band"))?;
    Ok((iv.0 + iv.1) as f64 / 2.0)
}

/// The room of the far end room's cluster that a required link ties to the start end room, when there is one: the plan is then
/// steered so that the two ends share a wall.
fn steer_target(fam: &Family, rooms: &[Room], cap0: &Option<Cl>, cap1: &Option<Cl>) -> Option<usize> {
    let (c0, c1) = (cap0.as_ref()?, cap1.as_ref()?);
    for l in fam.links.iter().filter(|l| l.use_ == Use::Required) {
        for (a, b) in [(l.from, l.to), (l.to, l.from)] {
            if rooms[c0.head].ty == a
                && let Some(j) = (0..rooms.len()).find(|j| rooms[*j].ty == b && c1.holds(*j))
            {
                return Some(j);
            }
        }
    }
    None
}

pub fn layout(fam: &Family, rooms: &[Room], g: &Graph, fx: &Fitter, shape: &Shape, opts: Options, rng: &mut Rnd) -> R<Laid> {
    let cx = Ctx { fam, rooms, g, fx };
    let (mut cap0, mut cap1, groups) = clusters(g);
    let steer = steer_target(fam, rooms, &cap0, &cap1);
    let mut pool = Pool { pool: (0..groups.len()).collect(), groups };
    rng.shuffle(&mut pool.pool);
    let sp = &fam.types[rooms[g.spines[0]].ty];
    let s = up((sp.min_side + LOSS_MAX) as f64) + rng.pick(&[0, 0, 100, 200])?;
    let sp_len: Range =
        (up(sp.area.0 * 1e6 / (s - LOSS_MAX) as f64 + LOSS_MAX as f64), dn(sp.area.1 * 1e6 / (s - LOSS_MIN) as f64 + LOSS_MIN as f64));
    let mut spare = g.spines.len() as i64 - 1;

    // Base depths: ones most groups can stand in.
    let score = |d: i64| pool.pool.iter().filter(|gr| fx.grange(&pool.groups[**gr], d).is_some()).count();
    let n_pool = pool.pool.len() as f64;
    let mut good: Vec<i64> = DEPTHS.iter().copied().filter(|d| score(*d) as f64 >= 0.75 * n_pool).collect();
    if good.is_empty() {
        good = DEPTHS.iter().copied().filter(|d| score(*d) as f64 >= 0.5 * n_pool).collect();
    }
    if good.is_empty() {
        return Err(Fail("no depth"));
    }
    let deepest = deep().filter(|d| pool.pool.iter().any(|gr| fx.grange(&pool.groups[*gr], *d).is_some())).max().unwrap_or(0);
    let (mut lv_a, mut lv_b) = (shape.levels.a.clone(), shape.levels.b.clone());
    let (mut fr_a, mut fr_b) = fractions(&shape.id, rng);
    if rng.random() < 0.5 {
        lv_a.reverse();
        lv_b.reverse();
        fr_a.reverse();
        fr_b.reverse();
    }
    let lv = [lv_a, lv_b];
    let fr = [fr_a, fr_b];
    let k = lv[0].len();
    // Steered: one side holds the far end room's cell from the start end room to the end wall, the other side everything else.
    let span_group: Option<Group> = steer.and_then(|room| {
        cap1.as_ref().map(|c| vec![(Cl { goal: Some((room, Wall::Left)), ..c.clone() }, false, false)])
    });
    let span_side = if span_group.is_some() {
        let ok: Vec<usize> = (0..2)
            .filter(|sd| lv[*sd][k - 1] == 0 && lv[*sd][..k - 1].iter().enumerate().all(|(j, l)| *l == 0 || j == 0))
            .collect();
        Some(rng.pick(&ok).map_err(|_| Fail("no side for the end room"))?)
    } else {
        None
    };
    let single = span_side.is_none()
        && lv[1].iter().copied().max().unwrap_or(0) == 0
        && rng.random() < if shape.id == "narrow_bar" { 0.6 } else { 0.08 };
    // A side with a raised piece starts shallower, so that some rooms can still stand in the raised depth.
    let mut base = [0i64; 2];
    for side in 0..2 {
        base[side] = if side == 1 && single {
            0
        } else if lv[side].iter().copied().max().unwrap_or(0) > 0 {
            rng.pick(&good)?.min(rng.pick(&good)?)
        } else {
            rng.pick(&good)?
        };
    }
    if let (Some(x), Some(gr)) = (span_side, &span_group) {
        // A depth the end room can stand in, and the start end room can still stand across the whole width.
        let fits: Vec<i64> = DEPTHS
            .iter()
            .copied()
            .filter(|d| {
                let (b0, b1) = if x == 0 { (*d, base[1]) } else { (base[0], *d) };
                fx.grange(gr, *d).is_some() && cap0.as_ref().is_some_and(|c| fx.drange(c, b0 + s + b1, (b1, b1 + s)).is_some())
            })
            .collect();
        let a = rng.pick(&fits).map_err(|_| Fail("no depth for the end room"))?;
        base[x] = if lv[x].iter().copied().max().unwrap_or(0) > 0 { a.min(rng.pick(&fits)?) } else { a };
    }
    let d0 = base[0] + s + base[1];
    // An end room stands across the end of the bar when it can be that long. Otherwise, and now and then for variety, it is a
    // corner room: the first or the last cell of one row, with the spine running on to the end wall.
    let mut corner: Vec<(bool, usize, usize)> = Vec::new(); // (is the first, group, side)
    for (first, cl, i) in [(true, &cap0, 0), (false, &cap1, k - 1)] {
        let Some(cl) = cl else { continue };
        if span_side.is_some() {
            if first && fx.drange(cl, d0, (base[1], base[1] + s)).is_none() {
                return Err(Fail("end room"));
            }
            continue;
        }
        let gr: Group = vec![(cl.clone(), false, false)];
        let homes: Vec<usize> = (0..2).filter(|sd| base[*sd] > 0 && lv[*sd][i] == 0 && fx.grange(&gr, base[*sd]).is_some()).collect();
        let across = fx.drange(cl, d0, (base[1], base[1] + s)).is_some();
        if homes.is_empty() && !across {
            return Err(Fail("end room"));
        }
        if !homes.is_empty() && (!across || rng.random() < 0.2) {
            let side = rng.pick(&homes)?;
            pool.groups.push(gr);
            corner.push((first, pool.groups.len() - 1, side));
        }
    }
    let span = span_side.zip(span_group).map(|(side, gr)| {
        pool.groups.push(gr);
        cap1 = None;
        (side, pool.groups.len() - 1)
    });
    if corner.iter().any(|c| c.0) {
        cap0 = None;
    }
    if corner.iter().any(|c| !c.0) {
        cap1 = None;
    }
    let (lo_r, hi_r) = shape.raise.map_or((0.0, 0.0), |r| (r[0], r[1]));
    let e_lo = up((600.0f64).max(lo_r * d0 as f64));
    let e_hi = dn(hi_r * d0 as f64);

    // Each side's pieces. A raised piece is a cap alone (none), deeper cells, or a branch (sub).
    let mut sides: Vec<Vec<Piece>> = Vec::new();
    for side in 0..2 {
        let mut pieces = Vec::new();
        for (i, &level) in lv[side].iter().enumerate() {
            let has_cap = (i == 0 && cap0.is_some()) || (i == k - 1 && cap1.is_some());
            let mut own = false;
            let mode = if span_side == Some(side) {
                if level > 0 { Mode::None } else { Mode::Span }
            } else if level > 0 {
                let turn = sp_len.1 / 2 >= base[side] + e_lo;
                let mut opts: Vec<Mode> = Vec::new();
                if deepest >= base[side] + e_lo {
                    opts.push(Mode::Cells);
                }
                if has_cap {
                    opts.extend([Mode::None; 2]);
                }
                let subs = if spare > 0 {
                    3
                } else if turn {
                    1
                } else {
                    0
                };
                opts.extend(std::iter::repeat_n(Mode::Sub, subs));
                if opts.is_empty() {
                    return Err(Fail("nothing can be raised"));
                }
                let mode = rng.pick(&opts)?;
                own = mode == Mode::Sub && spare > 0 && rng.random() < 0.8;
                spare -= i64::from(own);
                mode
            } else if base[side] == 0 {
                Mode::Free
            } else {
                let pinned = corner.iter().any(|(first, _, sd)| *sd == side && i == if *first { 0 } else { k - 1 });
                if has_cap && k > 1 && !pinned && rng.random() < 0.25 { Mode::None } else { Mode::Cells }
            };
            pieces.push(Piece {
                lv: level,
                mode,
                groups: Vec::new(),
                cap: has_cap,
                own: level > 0 && own,
                first: None,
                last: None,
                p: 0,
                q: 0,
                delta: 0,
                len: 0,
                left: Vec::new(),
                right: Vec::new(),
            });
        }
        sides.push(pieces);
    }
    for &(first, gr, side) in &corner {
        let p = &mut sides[side][if first { 0 } else { k - 1 }];
        if first {
            p.first = Some(gr);
        } else {
            p.last = Some(gr);
        }
        p.groups.push(gr);
    }
    let mut raise: [BTreeMap<i64, i64>; 2] = [BTreeMap::from([(0, 0)]), BTreeMap::from([(0, 0)])];

    // Branches first: a branch's length is the depth of its level.
    for side in 0..2 {
        for pi in 0..sides[side].len() {
            if sides[side][pi].mode != Mode::Sub {
                continue;
            }
            let lvl = sides[side][pi].lv;
            let own = sides[side][pi].own;
            let lim: Range = if own { sp_len } else { (0, sp_len.1 / 2) };
            let (mut lo, hi) = match raise[side].get(&lvl) {
                Some(known) => (base[side] + known, base[side] + known),
                None => ((base[side] + e_lo).max(lim.0), (base[side] + e_hi).min(lim.1)),
            };
            if lvl == 2 {
                lo = lo.max(base[side] + raise[side].get(&1).copied().unwrap_or(e_lo) + 600);
            }
            if lo > hi {
                return Err(Fail("branch length"));
            }
            let (p, q) = (rng.pick(&good)?, rng.pick(&good)?);
            let (left, l0, l1) = fill(&cx, &mut pool, rng, p, lo, hi)?;
            let (right, r0, r1) = fill(&cx, &mut pool, rng, q, lo.max(l0), hi.min(l1))?;
            let (a, b) = (lo.max(l0).max(r0), hi.min(l1).min(r1));
            let delta = rnd(rng.uniform(up(a as f64) as f64, dn(b as f64) as f64));
            if !(a <= delta && delta <= b) {
                return Err(Fail("branch length"));
            }
            let piece = &mut sides[side][pi];
            (piece.p, piece.q, piece.left, piece.right, piece.delta, piece.len) = (p, q, left, right, delta, p + s + q);
            raise[side].insert(lvl, delta - base[side]);
        }
    }

    // The other raises, and the caps: a cap is the full depth of the bar where it stands, the spine meeting its front.
    let cap_range = |cl: &Option<Cl>, i: usize, depth: &[Vec<i64>; 2]| -> (Option<Range>, Option<Range>) {
        let Some(cl) = cl else { return (Some((0, 0)), None) };
        let cover = (depth[1][i], depth[1][i] + s);
        (fx.drange(cl, depth[1][i] + s + depth[0][i], cover), Some(cover))
    };
    let mut chosen = None;
    for _ in 0..8 {
        let mut rs = raise.clone();
        let mut ok = true;
        'sides: for side in 0..2 {
            let mut levels: Vec<i64> = sides[side].iter().map(|p| p.lv).filter(|l| !rs[side].contains_key(l)).collect();
            levels.sort_unstable();
            levels.dedup();
            for level in levels {
                let lo = if level == 2 { e_lo.max(rs[side].get(&(level - 1)).copied().unwrap_or(0) + 600) } else { e_lo };
                let cands: Vec<i64> = (lo..=e_hi)
                    .step_by(300)
                    .filter(|e| {
                        sides[side].iter().filter(|p| p.lv == level).all(|p| {
                            p.mode != Mode::Cells || pool.pool.iter().any(|gr| fx.grange(&pool.groups[*gr], base[side] + e).is_some())
                        })
                    })
                    .collect();
                if cands.is_empty() {
                    ok = false;
                    break 'sides;
                }
                rs[side].insert(level, rng.pick(&cands)?);
            }
            if let (Some(a), Some(b)) = (rs[side].get(&2), rs[side].get(&1))
                && *a < *b + 600
            {
                ok = false;
            }
        }
        if !ok {
            continue;
        }
        let depth: [Vec<i64>; 2] =
            [0, 1].map(|side| sides[side].iter().map(|p| base[side] + rs[side].get(&p.lv).copied().unwrap_or(0)).collect());
        let (rc0, cover0) = cap_range(&cap0, 0, &depth);
        let (rc1, cover1) = cap_range(&cap1, k - 1, &depth);
        if let (Some(a), Some(b)) = (rc0, rc1) {
            chosen = Some((depth, a, b, cover0, cover1));
            break;
        }
    }
    let Some((depth, rc0, rc1, cover0, cover1)) = chosen else { return Err(Fail("cap")) };

    let take = |pool: &mut Pool, d: i64, target: i64| -> Vec<usize> {
        let (mut lo, mut hi) = (0, 0);
        let mut got = Vec::new();
        for &gr in &pool.pool {
            if let Some(iv) = fx.grange(&pool.groups[gr], d)
                && ((lo + hi) as f64 / 2.0) < target as f64
            {
                got.push(gr);
                lo += iv.0;
                hi += iv.1;
            }
        }
        pool.remove(&got);
        got
    };
    let mut subs = 0;
    for side in &sides {
        subs += side.iter().filter(|p| p.mode == Mode::Sub).map(|p| p.len).sum::<i64>();
    }
    let mut pool_mid = 0.0;
    for &gr in &pool.pool {
        pool_mid += mid(fx, &pool.groups[gr], base[0])?;
    }
    let core_est = (pool_mid + subs as f64) / if single || span.is_some() { 1.0 } else { 2.0 };
    let cap_est = [(rc0.0 + rc0.1) as f64 / 2.0, (rc1.0 + rc1.1) as f64 / 2.0];
    let w_est = cap_est[0] + core_est + cap_est[1];
    let want: Vec<Vec<f64>> = (0..2)
        .map(|side| {
            (0..k)
                .map(|i| fr[side][i] * w_est - if i == 0 { cap_est[0] } else { 0.0 } - if i == k - 1 { cap_est[1] } else { 0.0 })
                .collect()
        })
        .collect();
    // Raised cells first: few groups can stand that deep.
    for side in 0..2 {
        for i in 0..k {
            if sides[side][i].mode == Mode::Cells && sides[side][i].lv > 0 {
                let target = want[side][i].max(1500.0) as i64;
                let got = take(&mut pool, depth[side][i], target);
                if got.is_empty() {
                    return Err(Fail("empty piece"));
                }
                sides[side][i].groups = got;
            }
        }
    }
    // The rest, widest first, each to the side that is shorter so far, and there to the piece furthest from its share.
    let mut total = [0.0f64; 2];
    for side in 0..2 {
        for (i, p) in sides[side].iter().enumerate() {
            total[side] += p.len as f64;
            for &gr in &p.groups {
                total[side] += mid(fx, &pool.groups[gr], depth[side][i])?;
            }
        }
    }
    let mut order: Vec<(usize, f64)> = Vec::new();
    for &gr in &pool.pool {
        order.push((gr, mid(fx, &pool.groups[gr], base[0])?));
    }
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (gr, _) in order {
        let mut homes: Vec<(usize, usize)> = Vec::new();
        for side in 0..2 {
            for (i, p) in sides[side].iter().enumerate() {
                if p.mode == Mode::Cells && p.lv == 0 && fx.grange(&pool.groups[gr], depth[side][i]).is_some() {
                    homes.push((side, i));
                }
            }
        }
        if homes.is_empty() {
            return Err(Fail("left over"));
        }
        let side = if homes.iter().any(|h| h.0 == 0) && (!homes.iter().any(|h| h.0 == 1) || total[0] <= total[1]) { 0 } else { 1 };
        let mut best: Option<(usize, f64)> = None;
        for &(_, i) in homes.iter().filter(|h| h.0 == side) {
            let mut have = 0.0;
            for &x in &sides[side][i].groups {
                have += mid(fx, &pool.groups[x], depth[side][i])?;
            }
            let score = have - want[side][i];
            if best.is_none_or(|b| score < b.1) {
                best = Some((i, score));
            }
        }
        let Some((i, _)) = best else { return Err(Fail("left over")) };
        sides[side][i].groups.push(gr);
        total[side] += mid(fx, &pool.groups[gr], depth[side][i])?;
    }
    for pieces in &mut sides {
        for p in pieces {
            if p.mode == Mode::Cells && p.groups.is_empty() {
                if !p.cap {
                    return Err(Fail("empty piece"));
                }
                p.mode = Mode::None;
            }
            let mut free: Vec<usize> = p.groups.iter().copied().filter(|g| Some(*g) != p.first && Some(*g) != p.last).collect();
            rng.shuffle(&mut free);
            p.groups = p.first.into_iter().chain(free).chain(p.last).collect();
        }
    }

    // Lengths. Each piece has a range; the two sides and the spine must agree on the core's length.
    let mut rg: Vec<Vec<Range>> = Vec::new();
    for side in 0..2 {
        let mut ranges = Vec::new();
        for (i, p) in sides[side].iter().enumerate() {
            ranges.push(match p.mode {
                Mode::Sub => (p.len, p.len),
                Mode::Free | Mode::Span => (0, 1_000_000_000),
                _ => {
                    let (mut lo, mut hi) = (0, 0);
                    for &gr in &p.groups {
                        let iv = fx.grange(&pool.groups[gr], depth[side][i]).ok_or(Fail("a group has no range"))?;
                        lo += iv.0;
                        hi += iv.1;
                    }
                    (lo, hi)
                }
            });
        }
        rg.push(ranges);
    }
    let n_main = (spare + 1).max(1) as usize;
    let sum = |side: usize, hi: bool| -> i64 { rg[side].iter().map(|r| if hi { r.1 } else { r.0 }).sum() };
    let mut lo = up(sum(0, false).max(sum(1, false)).max(n_main as i64 * sp_len.0) as f64);
    let mut hi = dn(sum(0, true).min(sum(1, true)).min(n_main as i64 * sp_len.1) as f64);
    if let Some((x, gr)) = span {
        let first = (0..k).find(|i| sides[x][*i].mode == Mode::Span).ok_or(Fail("no span"))?;
        let iv = fx.grange(&pool.groups[gr], depth[x][first]).ok_or(Fail("the end room fits no depth"))?;
        (lo, hi) = (lo.max(iv.0), hi.min(iv.1));
    }
    if lo > hi {
        return Err(Fail("sides disagree"));
    }
    for _ in 0..24 {
        let core = rnd(rng.uniform(lo as f64, hi as f64));
        let mut ls: Vec<Vec<i64>> = Vec::new();
        for side in 0..2 {
            let free: Vec<usize> = (0..k).filter(|i| sides[side][*i].mode == Mode::Free).collect();
            let got = if span.is_some_and(|(x, _)| x == side) {
                // The span's pieces share the core as the outline's fractions say; the pieces the end room alone stands in have none.
                let idx: Vec<usize> = (0..k).filter(|i| sides[side][*i].mode == Mode::Span).collect();
                let whole: f64 = idx.iter().map(|i| fr[side][*i]).sum();
                let mut v = vec![0; k];
                let mut used = 0;
                for (n, &i) in idx.iter().enumerate() {
                    v[i] = if n + 1 < idx.len() { rnd(core as f64 * fr[side][i] / whole) } else { core - used };
                    used += v[i];
                }
                Some(v)
            } else if free.is_empty() {
                share(core, &rg[side])
            } else {
                let fixed: Vec<Range> = (0..k).map(|i| if free.contains(&i) { (0, 0) } else { rg[side][i] }).collect();
                let (flo, fhi) = (fixed.iter().map(|r| r.0).sum::<i64>(), fixed.iter().map(|r| r.1).sum::<i64>());
                share(core.min(flo.max(core.min(fhi))), &fixed).map(|mut got| {
                    let rest = core - got.iter().sum::<i64>();
                    for (n, &i) in free.iter().enumerate() {
                        got[i] = if n + 1 < free.len() {
                            rnd(rest as f64 / free.len() as f64)
                        } else {
                            rest - free[..n].iter().map(|j| got[*j]).sum::<i64>()
                        };
                    }
                    got
                })
            };
            match got {
                Some(v) => ls.push(v),
                None => break,
            }
        }
        if ls.len() < 2 {
            continue;
        }
        let (c0, c1) = (rnd(rng.uniform(rc0.0 as f64, rc0.1 as f64)), rnd(rng.uniform(rc1.0 as f64, rc1.1 as f64)));
        let ext: Vec<Vec<(i64, i64)>> = (0..2)
            .map(|side| {
                (0..k).map(|i| (ls[side][i] + if i == 0 { c0 } else { 0 } + if i == k - 1 { c1 } else { 0 }, depth[side][i])).collect()
            })
            .collect();
        if !shape_ok(shape, c0 + core + c1, s, &ext[0], &ext[1]) {
            continue;
        }
        let built = Build {
            cx: &cx,
            pool: &pool,
            shape,
            s,
            c0,
            c1,
            core,
            sides: &sides,
            ls: &ls,
            depth: &depth,
            cap0: &cap0,
            cap1: &cap1,
            cover0,
            cover1,
            n_main,
            opts,
            span,
        };
        if let Ok(laid) = built.run(rng) {
            return Ok(laid);
        }
    }
    Err(Fail("shape"))
}

/// Groups for one band of a branch: together they can be `reach` long, and need not be longer than `cap`.
fn fill(cx: &Ctx, pool: &mut Pool, rng: &mut Rnd, depth: i64, reach: i64, cap: i64) -> R<(Vec<usize>, i64, i64)> {
    let (mut chosen, mut lo, mut hi) = (Vec::new(), 0, 0);
    for &gr in &pool.pool {
        let Some(iv) = cx.fx.grange(&pool.groups[gr], depth) else { continue };
        if lo + iv.0 > cap {
            continue;
        }
        if hi >= reach && rng.random() < 0.5 {
            break;
        }
        chosen.push(gr);
        lo += iv.0;
        hi += iv.1;
    }
    if hi < reach || chosen.is_empty() {
        return Err(Fail("branch rooms"));
    }
    pool.remove(&chosen);
    Ok((chosen, lo, hi))
}

struct Build<'a> {
    cx: &'a Ctx<'a>,
    pool: &'a Pool,
    shape: &'a Shape,
    s: i64,
    c0: i64,
    c1: i64,
    core: i64,
    sides: &'a [Vec<Piece>],
    ls: &'a [Vec<i64>],
    depth: &'a [Vec<i64>; 2],
    cap0: &'a Option<Cl>,
    cap1: &'a Option<Cl>,
    cover0: Option<Range>,
    cover1: Option<Range>,
    n_main: usize,
    opts: Options,
    /// The side the end room's span is on, and its group.
    span: Option<(usize, usize)>,
}

/// The rectangles laid so far, and the cells they came from.
#[derive(Default)]
struct Acc {
    rects: Vec<Rc>,
    cells: Vec<Cell>,
}

impl Build<'_> {
    #[allow(clippy::too_many_arguments)]
    fn cell(
        &self,
        rng: &mut Rnd,
        acc: &mut Acc,
        cl: &Cl,
        f: i64,
        d: i64,
        lat: (bool, bool),
        cover: Option<Range>,
        at: (i64, i64),
        orient: Orient,
        kind: Kind,
    ) -> R<()> {
        let fx = self.cx.fx;
        let pref = rng.below(3);
        let got = fx.fit(cl, f, d, lat, cover, pref).or_else(|| fx.fit(cl, f, d, lat, cover, 0)).ok_or(Fail("fit"))?;
        acc.rects.extend(place(&got, at.0, at.1, orient));
        acc.cells.push(Cell {
            cl: cl.clone(),
            f,
            d,
            lat,
            cover,
            pref,
            ox: at.0,
            oy: at.1,
            orient,
            kind,
            rooms: got.iter().map(|r| r.0).collect(),
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn cells(&self, rng: &mut Rnd, acc: &mut Acc, groups: &[usize], length: i64, d: i64, ox: i64, oy: i64, orient: Orient) -> R<()> {
        let fx = self.cx.fx;
        let ranges: Option<Vec<Range>> = groups.iter().map(|g| fx.grange(&self.pool.groups[*g], d)).collect();
        let gl = share(length, &ranges.ok_or(Fail("share"))?).ok_or(Fail("share"))?;
        let mut pos = 0;
        for (gr, glen) in groups.iter().zip(gl) {
            let members = &self.pool.groups[*gr];
            let ranges: Option<Vec<Range>> = members.iter().map(|(cl, l, r)| fx.frange(cl, d, (*l, *r))).collect();
            let lens = share(glen, &ranges.ok_or(Fail("share"))?).ok_or(Fail("share"))?;
            for ((cl, l, r), f) in members.iter().zip(lens) {
                let at = if matches!(orient, Orient::A | Orient::B) { (ox + pos, oy) } else { (ox, oy + pos) };
                self.cell(rng, acc, cl, f, d, (*l, *r), None, at, orient, Kind::Band)?;
                pos += f;
            }
        }
        Ok(())
    }

    fn run(&self, rng: &mut Rnd) -> R<Laid> {
        let cx = self.cx;
        let (s, c0, core) = (self.s, self.c0, self.core);
        let mut acc = Acc::default();
        let mut branches: Vec<(bool, Local)> = Vec::new();
        for side in 0..2 {
            let mut x = c0;
            for (i, p) in self.sides[side].iter().enumerate() {
                match p.mode {
                    Mode::Cells => self.cells(
                        rng,
                        &mut acc,
                        &p.groups,
                        self.ls[side][i],
                        self.depth[side][i],
                        x,
                        if side == 0 { s } else { 0 },
                        if side == 0 { Orient::A } else { Orient::B },
                    )?,
                    Mode::Sub => {
                        let y0 = if side == 0 { s } else { -p.delta };
                        self.cells(rng, &mut acc, &p.left, p.delta, p.p, x + p.p, y0, Orient::L)?;
                        self.cells(rng, &mut acc, &p.right, p.delta, p.q, x + p.p + s, y0, Orient::R)?;
                        branches.push((p.own, (0, x + p.p, y0, s, p.delta)));
                    }
                    Mode::Span => {
                        if let Some((_, gr)) = self.span
                            && !self.sides[side][..i].iter().any(|q| q.mode == Mode::Span)
                        {
                            let len: i64 = (i..self.sides[side].len())
                                .filter(|j| self.sides[side][*j].mode == Mode::Span)
                                .map(|j| self.ls[side][j])
                                .sum();
                            self.cells(
                                rng,
                                &mut acc,
                                &[gr],
                                len,
                                self.depth[side][i],
                                x,
                                if side == 0 { s } else { 0 },
                                if side == 0 { Orient::A } else { Orient::B },
                            )?;
                        }
                    }
                    _ => {}
                }
                x += self.ls[side][i];
            }
        }
        let k = self.depth[0].len();
        for (cl, d, ox, i, orient, cover) in
            [(self.cap0, c0, c0, 0, Orient::L, self.cover0), (self.cap1, self.c1, c0 + core, k - 1, Orient::R, self.cover1)]
        {
            let Some(cl) = cl else { continue };
            let f = self.depth[1][i] + s + self.depth[0][i];
            self.cell(rng, &mut acc, cl, f, d, (false, false), cover, (ox, -self.depth[1][i]), orient, Kind::Cap)?;
        }
        // The spine: `n_main` rooms end to end along the core, then one room for each branch.
        let spines = &cx.g.spines;
        let n_main = self.n_main;
        let cuts: Vec<i64> = (0..n_main).map(|i| c0 + rnd(core as f64 * i as f64 / n_main as f64)).chain([c0 + core]).collect();
        for i in 0..n_main {
            acc.rects.push((spines[i], cuts[i], 0, cuts[i + 1] - cuts[i], s));
        }
        let mut own = 0;
        for (is_own, br) in &branches {
            if *is_own {
                let room = *spines.get(n_main + own).ok_or(Fail("no spare spine room"))?;
                acc.rects.push((room, br.1, br.2, br.3, br.4));
                own += 1;
            } else {
                let room = (0..n_main).filter(|i| 2 * cuts[*i] <= 2 * br.1 + s).max().ok_or(Fail("no spine room"))?;
                acc.rects.push((spines[room], br.1, br.2, br.3, br.4));
            }
        }
        let mut lost: Vec<usize> = acc.rects.iter().map(|r| r.0).collect();
        lost.sort_unstable();
        lost.dedup();
        if lost.len() != cx.rooms.len() {
            return Err(Fail("rooms lost"));
        }
        // An aligned plan's spine room is a rectangle, or turns one corner into a branch: at most six corners.
        if !self.opts.jogged {
            for &room in spines {
                let own: Vec<Rc> = acc.rects.iter().filter(|r| r.0 == room).copied().collect();
                if own.len() > 1 && !outline(&own).is_ok_and(|o| o.len() <= 6) {
                    return Err(Fail("spine room has more than six corners"));
                }
            }
        }
        // Would this plan pass? Links are tried with a throwaway random source, so the real one is not used up.
        let mut trial = Rnd::new(0, 0);
        let linked = connect(cx.fam, cx.rooms, cx.g, &acc.rects, n_main, &mut trial)?;
        verify(cx.fam, cx.rooms, &acc.rects, &linked, Some(self.shape.corners))?;
        // Step 9: the outline as it stands is the base; jogs and bumps change one cell each.
        let base = outline(&acc.rects)?;
        let step = interior::Step { cx, n_main, corners: self.shape.corners };
        if self.opts.jogged {
            interior::jog(&step, &mut acc, rng)?;
        }
        let bumps = if self.opts.bumps { interior::bump(&step, &mut acc, &base, rng)? } else { Vec::new() };
        let mirror = rng.random() < 0.5;
        let quarter_turns = rng.below(4);
        let (rects, by) = turned_by(&acc.rects, mirror, quarter_turns);
        let base_outline = turned_points(&base, mirror, quarter_turns, by);
        let bumps = bumps.into_iter().map(|b| b.turned(mirror, quarter_turns)).collect();
        let linked = connect(cx.fam, cx.rooms, cx.g, &rects, n_main, rng)?;
        let verified = verify(cx.fam, cx.rooms, &rects, &linked, None)?;
        outline(&rects)?;
        Ok(Laid { rects, linked, mirror, quarter_turns, verified, jogged: self.opts.jogged, bumps, base_outline })
    }
}
