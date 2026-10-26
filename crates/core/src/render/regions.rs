//! Keep the model's visible ink above non-masking regions. The original model drawing determines
//! occlusion: lifting a low wall's outline must not reveal a portion hidden by a cut wall.

use super::*;

#[derive(Default)]
pub(super) struct Layers {
    end: usize,
    section: Option<Id>,
    ink: Option<Vec<Ink>>,
    masks: Vec<Vec<P2>>,
}

impl Layers {
    pub(super) fn new(end: usize, section: Option<Id>) -> Self {
        Self { end, section, ..Self::default() }
    }
}

struct Ink {
    prim: Prim,
    owner: Option<Id>,
    part: Part,
    pen: Option<Pen>,
}

pub(super) fn mask(d: &mut Drawing, pieces: &[Vec<P2>]) {
    if d.regions.end != 0 {
        d.regions.masks.extend(pieces.iter().cloned());
    }
}

pub(super) fn foreground(d: &mut Drawing, m: &Model, pieces: &[Vec<P2>]) {
    if d.regions.end == 0 {
        return;
    }
    let area = d.regions.masks.iter().fold(pieces.to_vec(), |parts, mask| walls::minus(&parts, mask));
    if area.is_empty() {
        return;
    }
    if d.regions.ink.is_none() {
        d.regions.ink = Some(visible(d, m));
    }
    let restored: Vec<Ink> = d
        .regions
        .ink
        .as_ref()
        .into_iter()
        .flatten()
        .flat_map(|ink| {
            clipped(&ink.prim, d.view, &area, true).into_iter().map(|prim| Ink { prim, owner: ink.owner, part: ink.part, pen: ink.pen })
        })
        .collect();
    for ink in restored {
        keep(d, ink.prim, ink.owner, ink.part, ink.pen);
    }
}

fn visible(d: &Drawing, m: &Model) -> Vec<Ink> {
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
    let near = |extent: Option<(P2, P2)>, cover: &Cover| {
        extent.is_some_and(|(lo, hi)| lo.0 <= cover.hi.0 && hi.0 >= cover.lo.0 && lo.1 <= cover.hi.1 && hi.1 >= cover.lo.1)
    };
    let mut out = Vec::new();
    // Section projection is a hidden-line image. Its only non-paper pixels are already-visible
    // edges; retain these as horizontal pixel runs, without lifting the paper-coloured faces.
    if let Some(section) = d.regions.section
        && let Some(image) = section_image(m, &Scene::new(m), section, d.view, &[])
    {
        for y in 0..image.height {
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
                let prim = Prim::Line {
                    pts: vec![(start as f32, y as f32 + 0.5), (x as f32, y as f32 + 0.5)],
                    width: 1.0,
                    color: c,
                    dash: None,
                    closed: false,
                };
                let span = extent(&prim);
                let covers: Vec<_> = fills.iter().filter(|cover| near(span, cover)).collect();
                let mut parts = vec![prim];
                for cover in covers {
                    parts = parts.into_iter().flat_map(|p| clipped(&p, d.view, &cover.rings, false)).collect();
                }
                out.extend(parts.into_iter().map(|prim| Ink { prim, owner: None, part: Part::Projection, pen: None }));
            }
        }
    }
    for (i, (((prim, owner), part), pen)) in d.prims.iter().zip(&d.owners).zip(&d.parts).zip(&d.pens).take(end).enumerate() {
        let model = owner.and_then(|id| m.elements.get(id)).is_some_and(|e| crate::graphics::MODEL.contains(&e.category()));
        if !model || !matches!(prim, Prim::Line { .. } | Prim::Fill { .. }) || *part == Part::Text {
            continue;
        }
        if matches!(prim, Prim::Fill { .. }) && *part != Part::Pattern {
            continue;
        }
        let span = extent(prim);
        let mut parts = vec![prim.clone()];
        for cover in fills.iter().filter(|c| c.index > i && c.owner != *owner && near(span, c)) {
            parts = parts.into_iter().flat_map(|p| clipped(&p, d.view, &cover.rings, false)).collect();
            if parts.is_empty() {
                break;
            }
        }
        out.extend(parts.into_iter().map(|prim| Ink { prim, owner: *owner, part: *part, pen: *pen }));
    }
    out
}

/// The union of a set of filled rings along a finite segment, as distances from its start.
fn spans(a: P2, b: P2, rings: &[Vec<P2>]) -> Vec<(f64, f64)> {
    let len = a.dist(b);
    if len <= 1e-9 {
        return Vec::new();
    }
    let u = (b - a) * (1.0 / len);
    let mut spans: Vec<_> = rings
        .iter()
        .flat_map(|r| view3d::crossings(r, a, u))
        .map(|(lo, hi)| (lo.max(0.0), hi.min(len)))
        .filter(|(lo, hi)| hi - lo > 1e-8)
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut joined: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match joined.last_mut() {
            Some((_, end)) if lo <= *end + 1e-8 => *end = end.max(hi),
            _ => joined.push((lo, hi)),
        }
    }
    joined
}

fn clipped(prim: &Prim, view: View, rings: &[Vec<P2>], inside: bool) -> Vec<Prim> {
    // Reject disjoint extents before polygon work; most model primitives do not overlap a fill.
    let points: Vec<P2> = prim.points().into_iter().map(|p| view.mm(p)).collect();
    let overlaps = bounds(&points)
        .is_some_and(|(a, b)| rings.iter().filter_map(|r| bounds(r)).any(|(c, d)| a.0 <= d.0 && b.0 >= c.0 && a.1 <= d.1 && b.1 >= c.1));
    if !overlaps {
        return if inside { Vec::new() } else { vec![prim.clone()] };
    }
    match prim {
        Prim::Line { pts, width, color, dash, closed } => {
            let mut out = Vec::new();
            let mut whole = true;
            for (a, b) in segments(pts, *closed) {
                let (a, b) = (view.mm(a), view.mm(b));
                let len = a.dist(b);
                if len <= 1e-9 {
                    let hit = rings.iter().any(|r| geom::contains(r, a));
                    if hit == inside {
                        out.push(prim.clone());
                    } else {
                        whole = false;
                    }
                    continue;
                }
                let u = (b - a) * (1.0 / len);
                let found = spans(a, b, rings);
                let kept = if inside {
                    found
                } else {
                    let (mut gaps, mut at) = (Vec::new(), 0.0);
                    for (lo, hi) in found {
                        if lo > at + 1e-8 {
                            gaps.push((at, lo));
                        }
                        at = hi;
                    }
                    if at < len - 1e-8 {
                        gaps.push((at, len));
                    }
                    gaps
                };
                whole &= kept.len() == 1 && kept[0].0 <= 1e-8 && kept[0].1 >= len - 1e-8;
                for (lo, hi) in kept {
                    out.push(Prim::Line {
                        pts: vec![view.px(a + u * lo), view.px(a + u * hi)],
                        width: *width,
                        color: *color,
                        dash: dash.clone(),
                        closed: false,
                    });
                }
            }
            if whole { vec![prim.clone()] } else { out }
        }
        Prim::Fill { rings: source, color } => {
            let source: Vec<Vec<P2>> = source.iter().map(|r| r.iter().map(|p| view.mm(*p)).collect()).collect();
            let pieces = if inside {
                let clips: Vec<_> = rings.iter().flat_map(|r| walls::convex_pieces(r)).collect();
                source
                    .iter()
                    .flat_map(|r| walls::convex_pieces(r))
                    .flat_map(|r| clips.iter().filter_map(move |c| walls::split(r.clone(), c).0))
                    .collect()
            } else {
                rings.iter().fold(source, |pieces, r| walls::minus(&pieces, r))
            };
            if pieces.is_empty() {
                Vec::new()
            } else {
                vec![Prim::Fill { rings: pieces.iter().map(|r| r.iter().map(|p| view.px(*p)).collect()).collect(), color: *color }]
            }
        }
        Prim::Image { .. } => Vec::new(),
    }
}
