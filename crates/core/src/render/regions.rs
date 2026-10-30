//! Keep the model's visible ink above non-masking regions. The original model drawing determines
//! occlusion: lifting a low wall's outline must not reveal a portion hidden by a cut wall.
//!
//! The drawing's ink is found once ([`visible`]: the model layer's lines and patterns, and a section's picture as runs of
//! pixels). Each non-masking region then takes the part of it in its area, grown by half the ink's stroke so a line along
//! the region's edge stays whole, and not under a masking region drawn before it. Lines are cut by the area's rings and
//! masks directly, never by boolean operations on polygons, which cost too much for the outlines a sketch can hold; only
//! the model's filled patterns (cut poché) are, and only those that reach the area.

use std::cell::OnceCell;

use super::*;

#[derive(Default)]
pub(super) struct Layers {
    /// Whether the model layer was finished before the regions: a drawing that never finishes one has no ink to keep.
    on: bool,
    end: usize,
    section: Option<Id>,
    /// What a section's picture shows, as runs of its pixels, when whoever draws the section has the picture to hand in, and
    /// whether it showed more than [`MAX_RUNS`] of them.
    runs: Option<(Vec<Prim>, bool)>,
    ink: Option<Vec<Ink>>,
    masks: Vec<Mask>,
}

impl Layers {
    pub(super) fn new(end: usize, section: Option<Id>) -> Self {
        Self { on: true, end, section, ..Self::default() }
    }

    /// The picture this drawing's section is over, drawn through `view` or at a multiple of it (a screen's pixels): its ink.
    pub(super) fn picture(&mut self, image: &Image, view: View) {
        self.runs = Some(runs(image, view));
    }
}

/// Whether view `view` has a region that keeps the model's ink above it, which wants the picture of a section.
pub(super) fn wanted(m: &Model, view: Id) -> bool {
    m.elements.values().any(|e| match e {
        Element::FilledRegion(r) => m.same(r.view, view) && m.region_types.get(&r.type_name).is_some_and(|t| !t.masking),
        Element::DetailGroup(g) => m.same(g.view, view),
        Element::RepeatingDetail(r) => m.same(r.view, view),
        _ => false,
    })
}

#[cfg(test)]
thread_local! {
    /// What drawing regions has cost on this thread, in steps: the points of rings a line was cut by, of polygons a fill was.
    /// The tests assert on it, not on time, which varies with the load of the machine.
    static SPENT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The pictures of sections drawn on this thread.
    static PICTURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The most runs of ink [`runs`] keeps on this thread, which a test sets low to reach the cap with a small picture.
    static MOST: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_RUNS) };
}

#[cfg(test)]
fn spend(n: usize) {
    SPENT.with(|s| s.set(s.get().saturating_add(n)));
}

struct Ink {
    prim: Prim,
    owner: Option<Id>,
    part: Part,
    pen: Option<Pen>,
}

/// A convex piece of a masking region, and the box round it.
struct Mask {
    ring: Vec<P2>,
    lo: P2,
    hi: P2,
}

pub(super) fn mask(d: &mut Drawing, pieces: &[Vec<P2>]) {
    if d.regions.on {
        d.regions.masks.extend(pieces.iter().filter_map(|ring| {
            let (lo, hi) = bounds(ring)?;
            Some(Mask { ring: ring.clone(), lo, hi })
        }));
    }
}

/// The ink of the model that lies in the region whose outline and holes are `rings` (the outline first), above it.
pub(super) fn foreground(d: &mut Drawing, m: &Model, rings: &[Vec<P2>]) {
    if !d.regions.on {
        return;
    }
    if d.regions.ink.is_none() {
        let runs = d.regions.runs.take();
        let (ink, cut) = visible(d, m, runs);
        if cut {
            d.notes.push(format!(
                "this view's picture has more than {MAX_RUNS} runs of ink, more than a region keeps above it: the ink of its last rows is under the region"
            ));
        }
        d.regions.ink = Some(ink);
    }
    let Some(area) = Area::new(rings, &d.regions.masks) else { return };
    let (owner, view) = (d.owner, d.view);
    let restored: Vec<Ink> = d
        .regions
        .ink
        .as_deref()
        .unwrap_or_default()
        .iter()
        .flat_map(|ink| {
            // As wide as it will be drawn: its pen's, which its owner's object style gives while it is not yet set.
            let pen = ink.pen.map(|p| {
                let st = style_of(m, ink.owner);
                if p == BY_CATEGORY { if ink.part == Part::Cut { st.cut } else { st.projection } } else { p }
            });
            let stroke = match (&ink.prim, pen) {
                (Prim::Line { width, .. }, None) => *width,
                (_, Some(pen)) => d.width(pen),
                _ => 0.0,
            };
            // A section's picture is no element's: its ink is the region's now, as its pattern is.
            let (owner, part) = if ink.owner.is_some() { (ink.owner, ink.part) } else { (owner, Part::Pattern) };
            area.clip(&ink.prim, view, stroke).into_iter().map(move |prim| Ink { prim, owner, part, pen: ink.pen })
        })
        .collect();
    for ink in restored {
        keep(d, ink.prim, ink.owner, ink.part, ink.pen);
    }
}

/// The most runs of ink [`runs`] keeps of one picture.
const MAX_RUNS: usize = 250_000;

/// The widest run of ink that [`runs`] joins to the one above it: a line the width of a pen, which a round cap leaves a pixel
/// past the end of at most (a blob of ink is kept a row at a time).
const THIN: u32 = 2;

/// The picture's pixels that aren't paper, as runs in `view`'s pixels (the picture may be drawn at a multiple of `view`, a
/// screen with more pixels to the point): horizontal ones a row high, except that a run of at most [`THIN`] columns that sits on
/// one of the row above, end to end and of its colour, is that run a row taller, a vertical line (a wall's edge 2,000 rows tall
/// is one line, not 2,000). And whether the picture had more runs than [`MAX_RUNS`], the rest of which are not kept.
fn runs(image: &Image, view: View) -> (Vec<Prim>, bool) {
    // Columns from and to, rows from and to, colour.
    type Run = (u32, u32, u32, u32, Rgb);
    let (rx, ry) = (image.width as f32 / view.width.max(1) as f32, image.height as f32 / view.height.max(1) as f32);
    #[cfg(test)]
    let most = MOST.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let most = MAX_RUNS;
    let mut found: Vec<Run> = Vec::new();
    let (mut above, mut cut): (Vec<usize>, bool) = (Vec::new(), false);
    for y in 0..image.height {
        let mut now = Vec::new();
        let mut x = 0;
        while x < image.width {
            let c = image.color(f64::from(x), f64::from(y));
            if c == PAPER {
                x += 1;
                continue;
            }
            let start = x;
            x += 1;
            while x < image.width && image.color(f64::from(x), f64::from(y)) == c {
                x += 1;
            }
            // The run above it with the same columns, if there is one (those above are in order of their columns).
            let on = above
                .binary_search_by_key(&start, |&i| found[i].0)
                .ok()
                .map(|k| above[k])
                .filter(|&i| x - start <= THIN && found[i].1 == x && found[i].4 == c);
            let i = match on {
                Some(i) => {
                    found[i].3 = y + 1;
                    i
                }
                None if found.len() < most => {
                    found.push((start, x, y, y + 1, c));
                    found.len() - 1
                }
                None => {
                    cut = true;
                    continue;
                }
            };
            now.push(i);
        }
        above = now;
    }
    let prims = found
        .into_iter()
        .map(|(x0, x1, y0, y1, color)| {
            let (pts, width) = if y1 - y0 > 1 {
                let at = (x0 + x1) as f32 / 2.0 / rx;
                (vec![(at, y0 as f32 / ry), (at, y1 as f32 / ry)], (x1 - x0) as f32 / rx)
            } else {
                let at = (y0 + y1) as f32 / 2.0 / ry;
                (vec![(x0 as f32 / rx, at), (x1 as f32 / rx, at)], 1.0 / ry)
            };
            Prim::Line { pts, width, color, dash: None, closed: false }
        })
        .collect();
    (prims, cut)
}

/// The model's visible ink, and whether the picture of a section had more runs of ink than were kept.
fn visible(d: &Drawing, m: &Model, picture: Option<(Vec<Prim>, bool)>) -> (Vec<Ink>, bool) {
    let end = d.regions.end.min(d.prims.len());
    struct Cover {
        index: usize,
        owner: Option<Id>,
        rings: Vec<Vec<P2>>,
        lo: P2,
        hi: P2,
    }
    let fills: Vec<Cover> = d
        .prims
        .iter()
        .take(end)
        .enumerate()
        .filter_map(|(i, p)| {
            let Prim::Fill { rings, .. } = p else { return None };
            let rings: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|p| d.view.mm(*p)).collect()).collect();
            let (lo, hi) = bounds(&rings.iter().flatten().copied().collect::<Vec<_>>())?;
            Some(Cover { index: i, owner: d.owners.get(i).copied().flatten(), rings, lo, hi })
        })
        .collect();
    let extent = |p: &Prim| bounds(&p.points().into_iter().map(|p| d.view.mm(p)).collect::<Vec<_>>());
    let near = |extent: Option<(P2, P2)>, cover: &Cover| extent.is_some_and(|e| overlap(e, (cover.lo, cover.hi)));
    let mut out = Vec::new();
    // A section's projection is a hidden-line image. Its only non-paper pixels are already-visible edges; keep these as
    // pixel runs, without lifting the paper-coloured faces.
    let (picture, cut) = match (picture, d.regions.section) {
        (Some(runs), _) => runs,
        (None, Some(section)) => {
            #[cfg(test)]
            PICTURES.with(|n| n.set(n.get() + 1));
            section_image(m, &Scene::new(m), section, d.view, &[]).map(|image| runs(&image, d.view)).unwrap_or_default()
        }
        (None, None) => (Vec::new(), false),
    };
    for prim in picture {
        let span = extent(&prim);
        let mut parts = vec![prim];
        for cover in fills.iter().filter(|cover| near(span, cover)) {
            parts = parts.into_iter().flat_map(|p| outside(&p, d.view, &cover.rings)).collect();
        }
        out.extend(parts.into_iter().map(|prim| Ink { prim, owner: None, part: Part::Projection, pen: None }));
    }
    for (i, (((prim, owner), part), pen)) in d.prims.iter().zip(&d.owners).zip(&d.parts).zip(&d.pens).take(end).enumerate() {
        let model = owner.and_then(|id| m.elements.get(id)).is_some_and(|e| crate::graphics::MODEL.contains(&m.graphics_category(e)));
        if !model || !matches!(prim, Prim::Line { .. } | Prim::Fill { .. }) || *part == Part::Text {
            continue;
        }
        if matches!(prim, Prim::Fill { .. }) && *part != Part::Pattern {
            continue;
        }
        let span = extent(prim);
        let mut parts = vec![prim.clone()];
        for cover in fills.iter().filter(|c| c.index > i && c.owner != *owner && near(span, c)) {
            parts = parts.into_iter().flat_map(|p| outside(&p, d.view, &cover.rings)).collect();
            if parts.is_empty() {
                break;
            }
        }
        out.extend(parts.into_iter().map(|prim| Ink { prim, owner: *owner, part: *part, pen: *pen }));
    }
    (out, cut)
}

/// A box: its lowest corner and its highest.
type Extent = (P2, P2);

fn overlap((a, b): Extent, (c, d): Extent) -> bool {
    a.0 <= d.0 && b.0 >= c.0 && a.1 <= d.1 && b.1 >= c.1
}

/// Distances under this count as none, mm.
const EPS: f64 = 1e-8;

/// What to keep of a line: the stretches of a segment.
trait Keep {
    /// The stretches of the segment from `a` along unit `u`, `len` long, to keep: distances from `a`, in order and apart.
    fn spans(&self, a: P2, u: P2, len: f64) -> Vec<(f64, f64)>;
    /// Whether to keep a segment with no length, at `a`.
    fn point(&self, a: P2) -> bool;
}

/// `spans` sorted, and joined where they overlap or touch.
fn joined(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match out.last_mut() {
            Some((_, end)) if lo <= *end + EPS => *end = end.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// What `0..len` has left of it outside the (joined) `spans`: the gaps between them.
fn gaps(spans: Vec<(f64, f64)>, len: f64) -> Vec<(f64, f64)> {
    let (mut gaps, mut at) = (Vec::new(), 0.0);
    for (lo, hi) in spans {
        if lo > at + EPS {
            gaps.push((at, lo));
        }
        at = at.max(hi);
    }
    if at < len - EPS {
        gaps.push((at, len));
    }
    gaps
}

/// The (joined) `spans` less the (joined) `holes`.
fn less(spans: Vec<(f64, f64)>, holes: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (mut lo, hi) in spans {
        for &(a, b) in holes {
            if b <= lo || a >= hi {
                continue;
            }
            if a > lo + EPS {
                out.push((lo, a));
            }
            lo = lo.max(b);
        }
        if hi > lo + EPS {
            out.push((lo, hi));
        }
    }
    out
}

/// The part of `prim` outside `rings` (what a later fill leaves of it), as pieces.
fn outside(prim: &Prim, view: View, rings: &[Vec<P2>]) -> Vec<Prim> {
    struct Outside<'a>(&'a [Vec<P2>]);
    impl Keep for Outside<'_> {
        fn spans(&self, a: P2, u: P2, len: f64) -> Vec<(f64, f64)> {
            #[cfg(test)]
            spend(self.0.iter().map(Vec::len).sum());
            let over = self.0.iter().flat_map(|r| view3d::crossings(r, a, u)).map(|(lo, hi)| (lo.max(0.0), hi.min(len)));
            gaps(joined(over.filter(|(lo, hi)| hi - lo > EPS).collect()), len)
        }
        fn point(&self, a: P2) -> bool {
            !self.0.iter().any(|r| geom::contains(r, a))
        }
    }
    // Reject disjoint extents before polygon work; most model primitives do not overlap a fill.
    let points: Vec<P2> = prim.points().into_iter().map(|p| view.mm(p)).collect();
    let overlaps = bounds(&points).is_some_and(|b| rings.iter().filter_map(|r| bounds(r)).any(|c| overlap(b, c)));
    if !overlaps {
        return vec![prim.clone()];
    }
    match prim {
        Prim::Line { .. } => cut(prim, view, &Outside(rings)),
        Prim::Fill { rings: source, color } => {
            let source: Vec<Vec<P2>> = source.iter().map(|r| r.iter().map(|p| view.mm(*p)).collect()).collect();
            fill(
                rings.iter().fold(source, |pieces, r| {
                    #[cfg(test)]
                    spend(pieces.iter().map(Vec::len).sum::<usize>() + r.len());
                    walls::minus(&pieces, r)
                }),
                view,
                *color,
            )
        }
        Prim::Image { .. } => Vec::new(),
    }
}

fn fill(pieces: Vec<Vec<P2>>, view: View, color: Rgb) -> Vec<Prim> {
    if pieces.is_empty() {
        Vec::new()
    } else {
        vec![Prim::Fill { rings: pieces.iter().map(|r| r.iter().map(|p| view.px(*p)).collect()).collect(), color }]
    }
}

/// The pieces of line `prim` that `keep` keeps. A polyline's pieces that meet stay one line, so its corners keep their joins;
/// a dashed line's dashes stay where they were ([`phased`]); and a line left whole is the line.
fn cut(prim: &Prim, view: View, keep: &impl Keep) -> Vec<Prim> {
    let Prim::Line { pts, width, color, dash, closed } = prim else { return Vec::new() };
    let segs: Vec<(P2, P2)> = segments(pts, *closed).into_iter().map(|(a, b)| (view.mm(a), view.mm(b))).collect();
    if segs.iter().all(|(a, b)| a.dist(*b) <= EPS) {
        return if segs.first().is_none_or(|(a, _)| keep.point(*a)) { vec![prim.clone()] } else { Vec::new() };
    }
    // Each run's points, and how far along the line it begins, mm.
    let mut runs: Vec<(Vec<P2>, f64)> = Vec::new();
    // Whether the last run reaches the end of the last segment, and the first begins at the line's start.
    let (mut reaches, mut from_start, mut seen, mut whole, mut gone) = (false, false, 0, true, 0.0);
    for (a, b) in segs {
        let len = a.dist(b);
        if len <= EPS {
            continue;
        }
        let u = (b - a) * (1.0 / len);
        let spans = keep.spans(a, u, len);
        whole &= matches!(spans.as_slice(), [(lo, hi)] if *lo <= EPS && *hi >= len - EPS);
        for (n, (lo, hi)) in spans.iter().enumerate() {
            let (p, q) = (a + u * *lo, a + u * *hi);
            match runs.last_mut() {
                Some((run, _)) if n == 0 && reaches && *lo <= EPS => run.push(q),
                _ => runs.push((vec![p, q], gone + *lo)),
            }
            if seen == 0 && n == 0 {
                from_start = *lo <= EPS;
            }
        }
        reaches = spans.last().is_some_and(|(_, hi)| *hi >= len - EPS);
        seen += 1;
        gone += len;
    }
    if whole {
        return vec![prim.clone()];
    }
    // A ring cut where it began has its last run and its first as one.
    if *closed && reaches && from_start && runs.len() > 1 {
        let (head, _) = runs.remove(0);
        if let Some((last, _)) = runs.last_mut() {
            last.extend(head.into_iter().skip(1));
        }
    }
    runs.into_iter()
        .flat_map(|(run, at)| {
            let pts = run.into_iter().map(|p| view.px(p)).collect();
            phased(pts, at * view.scale, (*width, *color), dash.as_deref())
        })
        .collect()
}

/// A run of a line, `at` px along the line it was cut from, drawn as the line's dashes (`dash`: on, off, on, ...) have it there.
/// A dashed line's dashes start with it, so the run's own would start where it does: the dashes of the cycle it begins in are
/// drawn as lines of their own, solid, and the rest of it is a dashed line that begins a cycle.
fn phased(pts: Vec<(f32, f32)>, at: f64, (width, color): (f32, Rgb), dash: Option<&[f32]>) -> Vec<Prim> {
    let line = |pts, dash: Option<&[f32]>| Prim::Line { pts, width, color, dash: dash.map(<[f32]>::to_vec), closed: false };
    let Some(base) = dash else { return vec![line(pts, None)] };
    // As SVG has it, an odd list goes round twice.
    let d: Vec<f32> = if base.len() % 2 == 1 { base.iter().chain(base).copied().collect() } else { base.to_vec() };
    let cycle: f32 = d.iter().sum();
    if !(cycle > 0.0 && cycle.is_finite() && d.iter().all(|x| x.is_finite() && *x >= 0.0)) {
        return vec![line(pts, dash)];
    }
    let phase = (at as f32).rem_euclid(cycle);
    if phase < 1e-3 || cycle - phase < 1e-3 {
        return vec![line(pts, dash)];
    }
    let total: f32 = pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    let head = (cycle - phase).min(total);
    let mut out = Vec::new();
    let mut pos = -phase;
    for (i, len) in d.iter().enumerate() {
        let (from, to) = (pos, pos + len);
        pos = to;
        if to < 0.0 || (to == 0.0 && *len > 0.0) {
            continue;
        }
        if from >= head {
            break;
        }
        if i % 2 == 0 {
            out.push(line(along(&pts, from.max(0.0), to.min(head)), None));
        }
    }
    if total > head + 1e-3 {
        out.push(line(along(&pts, head, total), dash));
    }
    out
}

/// The part of the polyline `pts` from `from` to `to` px along it.
fn along(pts: &[(f32, f32)], from: f32, to: f32) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut at = 0.0;
    for w in pts.windows(2) {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let len = (bx - ax).hypot(by - ay);
        if len > 0.0 && at + len >= from && at <= to {
            let at_t = |t: f32| (ax + (bx - ax) * t, ay + (by - ay) * t);
            let (t0, t1) = (((from - at) / len).clamp(0.0, 1.0), ((to - at) / len).clamp(0.0, 1.0));
            if out.is_empty() {
                out.push(at_t(t0));
            }
            out.push(at_t(t1));
        }
        at += len;
    }
    if out.len() < 2 {
        let p = out.first().copied().or_else(|| pts.first().copied()).unwrap_or_default();
        out = vec![p, p];
    }
    out
}

/// The area a region keeps ink in: its rings (the outline and its holes: inside an odd number of them), less the masks
/// drawn before it.
struct Area<'a> {
    rings: &'a [Vec<P2>],
    boxes: Vec<(P2, P2)>,
    masks: &'a [Mask],
    lo: P2,
    hi: P2,
    /// The rings as convex pieces, the holes cut out, each with the box round it: found when a filled pattern first reaches it.
    pieces: OnceCell<Vec<(Vec<P2>, Extent)>>,
}

impl<'a> Area<'a> {
    fn new(rings: &'a [Vec<P2>], masks: &'a [Mask]) -> Option<Self> {
        let boxes: Vec<(P2, P2)> = rings.iter().filter_map(|r| bounds(r)).collect();
        let first = *boxes.first()?;
        let (lo, hi) = boxes.iter().fold(first, |(lo, hi), (a, b)| (P2(lo.0.min(a.0), lo.1.min(a.1)), P2(hi.0.max(b.0), hi.1.max(b.1))));
        Some(Area { rings, boxes, masks, lo, hi, pieces: OnceCell::new() })
    }

    /// The part of `prim` in the area, its lines `stroke` px wide (a hairline is a pixel) and so reaching half that far out.
    fn clip(&self, prim: &Prim, view: View, stroke: f32) -> Vec<Prim> {
        match prim {
            Prim::Line { .. } => {
                let grow = f64::from(stroke.max(1.0)) / 2.0 / view.scale;
                let points: Vec<P2> = prim.points().into_iter().map(|p| view.mm(p)).collect();
                match bounds(&points) {
                    Some((lo, hi)) if overlap((P2(lo.0 - grow, lo.1 - grow), P2(hi.0 + grow, hi.1 + grow)), (self.lo, self.hi)) => {
                        cut(prim, view, &Within { area: self, grow })
                    }
                    _ => Vec::new(),
                }
            }
            Prim::Fill { rings: source, color } => {
                let source: Vec<Vec<P2>> = source.iter().map(|r| r.iter().map(|p| view.mm(*p)).collect()).collect();
                let all: Vec<P2> = source.iter().flatten().copied().collect();
                if !bounds(&all).is_some_and(|b| overlap(b, (self.lo, self.hi))) {
                    return Vec::new();
                }
                let clips = self.pieces();
                let mut parts: Vec<Vec<P2>> = Vec::new();
                for r in source.iter().flat_map(|r| walls::convex_pieces(r)) {
                    let Some(rb) = bounds(&r) else { continue };
                    parts.extend(clips.iter().filter(|(_, cb)| overlap(rb, *cb)).filter_map(|(c, _)| {
                        #[cfg(test)]
                        spend(r.len() + c.len());
                        walls::split(r.clone(), c).0
                    }));
                }
                for mask in self.masks {
                    parts = parts
                        .into_iter()
                        .flat_map(|p| match bounds(&p) {
                            Some(b) if overlap(b, (mask.lo, mask.hi)) => {
                                #[cfg(test)]
                                spend(p.len() + mask.ring.len());
                                walls::split(p, &mask.ring).1
                            }
                            _ => vec![p],
                        })
                        .collect();
                }
                fill(parts, view, *color)
            }
            Prim::Image { .. } => Vec::new(),
        }
    }

    /// The rings as convex pieces, the holes cut out.
    fn pieces(&self) -> &[(Vec<P2>, Extent)] {
        self.pieces.get_or_init(|| {
            let (outline, holes) = self.rings.split_first().map_or((&[][..], &[][..]), |(o, h)| (o.as_slice(), h));
            #[cfg(test)]
            spend(self.rings.iter().map(Vec::len).sum());
            let cut = holes.iter().fold(walls::convex_pieces(outline), |ps, hole| walls::minus(&ps, hole));
            cut.into_iter().filter_map(|p| Some((bounds(&p)?, p))).map(|(b, p)| (p, b)).collect()
        })
    }
}

/// Where a line that reaches `grow` mm either side of its centre touches an area: inside it, or within `grow` of it.
struct Within<'a> {
    area: &'a Area<'a>,
    grow: f64,
}

impl Keep for Within<'_> {
    fn spans(&self, a: P2, u: P2, len: f64) -> Vec<(f64, f64)> {
        let (area, grow) = (self.area, self.grow);
        let b = a + u * len;
        let seg = (P2(a.0.min(b.0), a.1.min(b.1)), P2(a.0.max(b.0), a.1.max(b.1)));
        let reach = (P2(seg.0.0 - grow, seg.0.1 - grow), P2(seg.1.0 + grow, seg.1.1 + grow));
        if !overlap(reach, (area.lo, area.hi)) {
            return Vec::new();
        }
        let near: Vec<&Vec<P2>> = area.rings.iter().zip(&area.boxes).filter(|(_, bx)| overlap(reach, **bx)).map(|(r, _)| r).collect();
        #[cfg(test)]
        spend(near.iter().map(|r| r.len()).sum());
        // Inside an odd number of rings: between the crossings of their edges, paired off; a vertex on the line counts as
        // just under it.
        let n = u.perp();
        let mut cross: Vec<f64> = Vec::new();
        for (p, q) in near.iter().flat_map(|r| geom::ring_edges(r)) {
            let (sp, sq) = ((p - a).dot(n), (q - a).dot(n));
            if (sp > 0.0) != (sq > 0.0) {
                cross.push(((p + (q - p) * (sp / (sp - sq))) - a).dot(u));
            }
        }
        cross.sort_by(f64::total_cmp);
        let mut found: Vec<(f64, f64)> = cross.as_chunks::<2>().0.iter().map(|[x, y]| (x.max(0.0), y.min(len))).collect();
        // Or within half the stroke of an edge.
        if grow > 0.0 {
            found.extend(near.iter().flat_map(|r| geom::ring_edges(r)).filter_map(|(p, q)| capsule(p, q, grow, a, u)));
        }
        let kept = joined(found.into_iter().map(|(x, y)| (x.max(0.0), y.min(len))).filter(|(x, y)| y - x > EPS).collect());
        if kept.is_empty() {
            return kept;
        }
        // Not under a mask drawn before the region.
        let under: Vec<(f64, f64)> = area
            .masks
            .iter()
            .filter(|mk| overlap(seg, (mk.lo, mk.hi)))
            .flat_map(|mk| {
                #[cfg(test)]
                spend(mk.ring.len());
                view3d::crossings(&mk.ring, a, u)
            })
            .map(|(x, y)| (x.max(0.0), y.min(len)))
            .filter(|(x, y)| y - x > EPS)
            .collect();
        less(kept, &joined(under))
    }

    fn point(&self, a: P2) -> bool {
        let area = self.area;
        area.rings.iter().filter(|r| geom::contains(r, a)).count() % 2 == 1 && !area.masks.iter().any(|mk| geom::contains(&mk.ring, a))
    }
}

/// The stretch of the line from `a` along unit `u` within `d` of the segment `p`→`q`, as distances from `a`: where it meets the
/// segment's capsule, which is convex, so one stretch.
fn capsule(p: P2, q: P2, d: f64, a: P2, u: P2) -> Option<(f64, f64)> {
    let mut hull: Option<(f64, f64)> = None;
    let mut add = |r: Option<(f64, f64)>| {
        if let Some((x, y)) = r {
            hull = Some(hull.map_or((x, y), |(lo, hi)| (lo.min(x), hi.max(y))));
        }
    };
    // Its ends' discs.
    for c in [p, q] {
        let w = c - a;
        let (along, across) = (w.dot(u), w.cross(u).abs());
        if across <= d {
            let h = (d * d - across * across).sqrt();
            add(Some((along - h, along + h)));
        }
    }
    // And the rectangle between them, as four half-planes.
    let e = q - p;
    let length = e.length();
    if length > EPS {
        let (s, n) = (e * (1.0 / length), e.perp() * (1.0 / length));
        let w = a - p;
        let band = |c0: f64, c1: f64, lo: f64, hi: f64| -> Option<(f64, f64)> {
            if c1.abs() < 1e-12 {
                (lo..=hi).contains(&c0).then_some((f64::NEG_INFINITY, f64::INFINITY))
            } else {
                let (x, y) = ((lo - c0) / c1, (hi - c0) / c1);
                Some((x.min(y), x.max(y)))
            }
        };
        let both = |x: Option<(f64, f64)>, y: Option<(f64, f64)>| -> Option<(f64, f64)> {
            let ((a, b), (c, d)) = (x?, y?);
            (a.max(c) <= b.min(d)).then_some((a.max(c), b.min(d)))
        };
        add(both(band(w.dot(s), u.dot(s), 0.0, length), band(w.dot(n), u.dot(n), -d, d)));
    }
    hull
}

#[cfg(test)]
mod tests;
