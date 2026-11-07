//! Dimensions in a plan, each as its type says (`crate::dims`): its lines and witness lines in its pens and colour, its
//! ticks (diagonal, arrows, dots), its string (continuous, baseline or ordinate), and its numbers lettered in its font,
//! over the paper where its background is opaque, with the dimension's own text above and below them. Sizes are mm on
//! paper, so a dimension reads the same at any scale.

use super::*;
use crate::dims::{Background, DimString, DimensionType, Read, Tick, Witness};

/// A dimension, of its kind.
pub(super) fn dimension(d: &mut Drawing, m: &Model, dim: &Dimension) {
    // Align's lock is shown by a front end, never drawn.
    if dim.alignment {
        return;
    }
    let t = m.dimension_type(dim);
    let ink = Ink { mm: f64::from(d.k) / d.view.scale * sheet::DENSITY, color: Rgb(t.color[0], t.color[1], t.color[2]), t };
    match dim.kind {
        DimKind::Aligned | DimKind::Linear => aligned(d, m, dim, &ink),
        DimKind::Angular => angular(d, m, dim, &ink),
        DimKind::Radial => radial(d, m, dim, &ink, false),
        DimKind::Diameter => radial(d, m, dim, &ink, true),
        DimKind::ArcLength => arc_length(d, m, dim, &ink),
    }
}

/// What a dimension draws with: its type, its colour, and how many model mm a mm of paper is in this view.
struct Ink {
    t: DimensionType,
    color: Rgb,
    mm: f64,
}

impl Ink {
    fn line(&self, d: &mut Drawing, pts: &[P2]) {
        d.line(pts, self.t.line_pen, self.color, false);
    }

    /// Where a witness line along `u` meets the dimension line at `p`, the mark there: a diagonal stroke across, or an
    /// arrowhead pointing at `p` along each of `toward` (the way the dimension line comes to it, from each side it does).
    fn tick(&self, d: &mut Drawing, p: P2, u: P2, toward: &[P2]) {
        let size = self.t.tick_size * self.mm;
        match self.t.tick {
            Tick::None => {}
            Tick::Diagonal => {
                let Some(&along) = toward.first() else { return };
                let k = (u + along).unit() * (size / 2.0);
                d.line(&[p - k, p + k], self.t.tick_pen, self.color, false);
            }
            Tick::Dot => {
                let r = size / 4.0;
                let ring: Vec<P2> = (0..16).map(|i| p + P2((f64::from(i) * PI / 8.0).cos(), (f64::from(i) * PI / 8.0).sin()) * r).collect();
                d.fill(&[ring], self.color);
            }
            Tick::Arrow | Tick::OpenArrow => {
                for &dir in toward {
                    // 15° either side of the line.
                    let (back, wide) = (p - dir * size, dir.perp() * (size * 0.268));
                    if self.t.tick == Tick::Arrow {
                        d.fill(&[vec![p, back + wide, back - wide]], self.color);
                    } else {
                        d.line(&[back + wide, p, back - wide], self.t.tick_pen, self.color, false);
                    }
                }
            }
        }
    }

    /// A witness line along `u` from what it measures, which spans `ta` to `tb` along `u` from the dimension line's point
    /// `c`, to the dimension line and past it; across it when what it measures crosses the dimension line.
    fn witness(&self, d: &mut Drawing, c: P2, u: P2, ta: f64, tb: f64) {
        let (gap, ext) = (self.t.witness_gap * self.mm, self.t.witness_extension * self.mm);
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        let (from, to) = match (lo > 0.0, hi < 0.0, self.t.witness) {
            (false, false, _) => (-ext, ext),
            (true, _, Witness::GapToElement) => (lo - gap, -ext),
            (_, true, Witness::GapToElement) => (hi + gap, ext),
            (true, _, Witness::Fixed) => (self.t.witness_length * self.mm - ext, -ext),
            (_, true, Witness::Fixed) => (ext - self.t.witness_length * self.mm, ext),
        };
        if (to - from).abs() > 1e-9 {
            self.line(d, &[c + u * from, c + u * to]);
        }
    }

    /// `text` centred on `at`, its lines running along `along`, a model direction; over the paper when the type's
    /// background is opaque.
    fn letter(&self, d: &mut Drawing, text: &str, at: P2, along: P2) {
        if text.is_empty() {
            return;
        }
        let tt = self.t.lettering();
        let layout = crate::text::layout(&tt, text, &crate::text::styles(&tt, text, false, &[]));
        // Clockwise on screen, y down.
        let angle = -(along.1.atan2(along.0) as f32);
        let k = d.px(sheet::DENSITY as f32);
        let [l, top, r, b] = layout.frame(0.0);
        let centre = ((l + r) / 2.0, (top + b) / 2.0);
        let origin = NoteFrame { at: (0.0, 0.0), k, angle }.px(centre);
        let c = d.view.px(at);
        let frame = NoteFrame { at: (c.0 - origin.0, c.1 - origin.1), k, angle };
        if self.t.background == Background::Opaque {
            let pad = 0.3;
            let ring: Vec<P2> = [(l - pad, top - pad), (r + pad, top - pad), (r + pad, b + pad), (l - pad, b + pad)]
                .into_iter()
                .map(|p| d.view.mm(frame.px(p)))
                .collect();
            d.fill(&[ring], PAPER);
        }
        let f = |(x, y): (f32, f32)| frame.px((f64::from(x), f64::from(y)));
        let part = std::mem::replace(&mut d.part, Part::Text);
        let drawn = layout.drawn();
        for letter in &drawn.fill {
            d.push(Prim::Fill { rings: letter.iter().map(|r| r.iter().copied().map(f).collect()).collect(), color: self.color });
        }
        for (pts, width) in drawn.strokes {
            d.push(Prim::Line {
                pts: pts.into_iter().map(f).collect(),
                width: (width * k).max(1.0),
                color: self.color,
                dash: None,
                closed: false,
            });
        }
        for (a, b) in drawn.underlines {
            let width = (drawn.underline_width * k).max(1.0);
            d.push(Prim::Line { pts: vec![frame.px(a), frame.px(b)], width, color: self.color, dash: None, closed: false });
        }
        d.part = part;
    }

    /// A number `label` beside the line at `at` running along `along`, on its `up` side, with the dimension's text above
    /// and below it: what reads the way the type's read convention says.
    fn label(&self, d: &mut Drawing, (above, below): &(String, String), label: &str, at: P2, along: P2, up: P2) {
        let (size, off) = (self.t.text_size * self.mm, self.t.text_offset * self.mm);
        // A line running up the page reads from its foot, or with up then right from its head.
        let along = if self.t.read == Read::UpThenRight && along.1.abs() > along.0.abs() { along * -1.0 } else { along };
        let mid = at + up * (off + size / 2.0);
        self.letter(d, label, mid, along);
        let step = up * (size * 1.5);
        self.letter(d, above, mid + step, along);
        self.letter(d, below, at - up * (off + size / 2.0), along);
    }
}

/// An aligned or linear dimension: its line through `at` (a baseline string's lines stacked out from it), a witness line
/// from each reference, ticks where they meet, and the numbers.
fn aligned(d: &mut Drawing, m: &Model, dim: &Dimension, ink: &Ink) {
    let (n, cross) = m.crossings(dim);
    if cross.is_empty() {
        return;
    }
    let labels = m.dimension_labels(dim);
    // Along the references, the side the text goes; a baseline string's lines step away from what it measures.
    let u = n.perp();
    let on = |s: f64, w: f64| dim.at + n * s + u * w;
    let mean = cross.iter().map(|(s, (a, b))| ((*a + *b) * 0.5 - on(*s, 0.0)).dot(u)).sum::<f64>() / cross.len() as f64;
    let away = if mean > 0.0 { -1.0 } else { 1.0 };
    let ext = ink.t.line_extension * ink.mm;
    let spacing = ink.t.baseline_spacing * ink.mm * away;
    // Each line: how far out it is, and the crossings it runs between.
    let lines: Vec<(f64, usize, usize)> = match ink.t.string {
        DimString::Baseline => (1..cross.len()).map(|i| ((i - 1) as f64 * spacing, 0, i)).collect(),
        DimString::Continuous | DimString::Ordinate => vec![(0.0, 0, cross.len() - 1)],
    };
    for &(w, i, j) in &lines {
        let (Some(&(s0, _)), Some(&(s1, _))) = (cross.get(i), cross.get(j)) else { continue };
        ink.line(d, &[on(s0, w) - n * ext, on(s1, w) + n * ext]);
    }
    for (k, &(s, (a, b))) in cross.iter().enumerate() {
        // The farthest line it reaches: a baseline's first reference all of them, each other its own.
        let w = match ink.t.string {
            DimString::Baseline if k == 0 => lines.last().map_or(0.0, |l| l.0),
            DimString::Baseline => lines.get(k.saturating_sub(1)).map_or(0.0, |l| l.0),
            _ => 0.0,
        };
        let c = on(s, w);
        ink.witness(d, c, u, (a - c).dot(u), (b - c).dot(u));
        for &(lw, i, j) in &lines {
            if k < i || k > j {
                continue;
            }
            let toward: Vec<P2> = match (k == i, k == j) {
                (true, true) => vec![n * -1.0, n],
                (true, false) => vec![n * -1.0],
                (false, true) => vec![n],
                (false, false) => vec![n, n * -1.0],
            };
            ink.tick(d, on(s, lw), u, &toward);
        }
    }
    match ink.t.string {
        DimString::Continuous => {
            for (i, (pair, label)) in cross.windows(2).zip(&labels).enumerate() {
                let [(s0, _), (s1, _)] = pair else { continue };
                ink.label(d, &m.dimension_extra(dim, i), label, on((s0 + s1) / 2.0, 0.0), n, u);
            }
        }
        DimString::Baseline => {
            for (k, (&(w, i, j), label)) in lines.iter().zip(&labels).enumerate() {
                let (Some(&(s0, _)), Some(&(s1, _))) = (cross.get(i), cross.get(j)) else { continue };
                let prev = cross.get(j.saturating_sub(1)).map_or(s0, |c| c.0);
                ink.label(d, &m.dimension_extra(dim, k), label, on((prev + s1) / 2.0, w), n, u);
            }
        }
        DimString::Ordinate => {
            for (i, (&(s, _), label)) in cross.iter().zip(&labels).enumerate() {
                ink.label(d, &m.dimension_extra(dim, i), label, on(s, 0.0), n, u);
            }
        }
    }
}

/// An angular dimension: an arc through `at` between its two lines, witness lines out along them to it, ticks at its
/// ends, and the angle outside its middle.
fn angular(d: &mut Drawing, m: &Model, dim: &Dimension, ink: &Ink) {
    let Some((o, from, sweep)) = m.angle_sector(dim) else { return };
    let r = dim.at.dist(o);
    if r <= 0.0 {
        return;
    }
    let ray = |t: f64| P2(t.cos(), t.sin());
    let arc: Vec<P2> = (0..=48).map(|i| o + ray(from + sweep * f64::from(i) / 48.0) * r).collect();
    ink.line(d, &arc);
    let lines: Vec<(P2, P2)> = dim.refs.iter().filter_map(|x| m.reference_line(x).ok()).collect();
    for (t, toward) in [(from, -1.0), (from + sweep, 1.0)] {
        let u = ray(t);
        // How far out along this ray its line reaches: the witness line runs from there to the arc.
        let reach = lines
            .iter()
            .filter(|(a, b)| geom::parallel(*a, *b, o, o + u))
            .flat_map(|(a, b)| [(*a - o).dot(u), (*b - o).dot(u)])
            .fold(0.0, f64::max);
        ink.witness(d, o + u * r, u, -r, reach - r);
        ink.tick(d, o + u * r, u, &[u.perp() * toward]);
    }
    let mid = ray(from + sweep / 2.0);
    if let Some(label) = m.dimension_labels(dim).first() {
        // Read along the arc's middle, outside it.
        let along = if mid.perp().0 < 0.0 { mid.perp() * -1.0 } else { mid.perp() };
        let up = if mid.dot(along.perp()) >= 0.0 { along.perp() } else { along.perp() * -1.0 };
        ink.label(d, &m.dimension_extra(dim, 0), label, o + mid * r, along, up);
    }
}

/// A radial or a diameter dimension: a line from a round element's centre toward `at` (a diameter's from the edge
/// opposite, through the centre), its ticks where it meets the edge, a cross at the centre, and the number past its end.
fn radial(d: &mut Drawing, m: &Model, dim: &Dimension, ink: &Ink, across: bool) {
    let Some(Ok((c, r))) = dim.refs.first().map(|x| m.reference_arc(x)) else { return };
    let u = if dim.at.dist(c) > 0.0 { (dim.at - c).unit() } else { P2(1.0, 0.0) };
    let (edge, end) = (c + u * r, if dim.at.dist(c) > r { dim.at } else { c + u * r });
    let start = if across { c - u * r } else { c };
    ink.line(d, &[start, end]);
    // Inside a circle too small for it, the arrow comes in from outside.
    let small = r < 2.0 * ink.t.tick_size * ink.mm && end != edge;
    ink.tick(d, edge, u.perp(), &[if small { u * -1.0 } else { u }]);
    if across {
        ink.tick(d, c - u * r, u.perp(), &[u * -1.0]);
    }
    if ink.t.centre_mark {
        let arm = 0.8 * ink.mm;
        for k in [P2(arm, 0.0), P2(0.0, arm)] {
            ink.line(d, &[c - k, c + k]);
        }
    }
    if let Some(label) = m.dimension_labels(dim).first() {
        let along = if u.0 < -1e-9 { u * -1.0 } else { u };
        let up = along.perp();
        // Past its end, along it.
        let tt = ink.t.lettering();
        let wide = crate::text::layout(&tt, label, &crate::text::styles(&tt, label, false, &[])).width * ink.mm;
        let at = end + u * (wide / 2.0 + ink.t.text_offset * ink.mm) - up * (ink.t.text_offset * ink.mm + ink.t.text_size * ink.mm / 2.0);
        ink.label(d, &m.dimension_extra(dim, 0), label, at, along, up);
    }
}

/// An arc length dimension: an arc through `at` round the arc's centre, as far round as it sweeps, witness lines out
/// along the radii from its ends, ticks where they cross, and its length outside its middle.
fn arc_length(d: &mut Drawing, m: &Model, dim: &Dimension, ink: &Ink) {
    let Ok((o, radius, from, sweep)) = m.arc_span(dim) else { return };
    let r = dim.at.dist(o);
    if r <= 0.0 {
        return;
    }
    let ray = |t: f64| P2(t.cos(), t.sin());
    let n = ((sweep.abs() / 0.05).ceil() as u32).clamp(8, 720);
    let line: Vec<P2> = (0..=n).map(|i| o + ray(from + sweep * f64::from(i) / f64::from(n)) * r).collect();
    ink.line(d, &line);
    let turn = sweep.signum();
    for (t, toward) in [(from, -turn), (from + sweep, turn)] {
        let u = ray(t);
        ink.witness(d, o + u * r, u, radius - r, radius - r);
        ink.tick(d, o + u * r, u, &[u.perp() * toward]);
    }
    let mid = ray(from + sweep / 2.0);
    if let Some(label) = m.dimension_labels(dim).first() {
        let along = if mid.perp().0 < 0.0 { mid.perp() * -1.0 } else { mid.perp() };
        let up = if mid.dot(along.perp()) >= 0.0 { along.perp() } else { along.perp() * -1.0 };
        ink.label(d, &m.dimension_extra(dim, 0), label, o + mid * r, along, up);
    }
}
