//! A plan's display list as Rune shapes. `render::draw` makes the primitives (lines, fills, lettering's fallback) in the canvas's
//! own pixels, one point to a pixel, so painting is a translation of each: a line is a stroke, a ring a polygon, lettering and
//! other non-zero fills the triangles the core makes of them (`text::triangles`), a dash split into its strokes. What Rune's
//! shapes cannot say yet is counted in [`Painted::skipped`], never drawn wrong: pictures, for one (docs/areas/rune-ui.md).

use std::collections::HashSet;

use bimcraft_core::ids::Id;
use bimcraft_core::render::{Drawing, Part, Prim, Rgb};
use rune::{Color, LineCap, Shape};

/// The most strokes one dashed line may split into: a hostile pattern (a dash of a millionth of a point) draws a line, not a hang.
const MAX_DASHES: usize = 20_000;
/// The thinnest a stroke is drawn, points: a hairline that would fall between pixels still shows.
const HAIRLINE: f32 = 0.6;

/// The canvas's colours that aren't the drawing's own.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub paper: Color,
    pub accent: Color,
    pub accent_fill: Color,
    pub hover: Color,
    pub hover_fill: Color,
}

impl Palette {
    pub fn dark() -> Palette {
        Palette {
            paper: rgb8(30, 32, 36),
            accent: rgb8(255, 170, 40),
            accent_fill: rgba8(255, 170, 40, 90),
            hover: rgb8(120, 200, 255),
            hover_fill: rgba8(120, 200, 255, 60),
        }
    }
}

fn rgb8(r: u8, g: u8, b: u8) -> Color {
    rgba8(r, g, b, 255)
}

fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: f32::from(a) / 255.0 }
}

/// A drawing's colour, as it is on paper, which a dark canvas shows lightened so black ink stays visible on it.
pub fn ink(c: Rgb, dark: bool) -> Color {
    let Rgb(r, g, b) = c;
    if dark && u16::from(r) + u16::from(g) + u16::from(b) < 90 {
        return rgb8(225, 225, 225);
    }
    rgb8(r, g, b)
}

/// What painting a drawing made.
pub struct Painted {
    pub shape: Shape,
    /// How many strokes and fills it holds.
    pub pieces: usize,
    /// How many primitives it left out because a shape cannot draw them (pictures).
    pub skipped: usize,
}

/// Whether the box of `pts` can reach the canvas `size` (a margin for the stroke's width): what lies far outside is never sent.
fn reaches(pts: &[(f32, f32)], size: (f32, f32), margin: f32) -> bool {
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for &(x, y) in pts {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        (lo, hi) = ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)));
    }
    !pts.is_empty() && hi.0 >= -margin && hi.1 >= -margin && lo.0 <= size.0 + margin && lo.1 <= size.1 + margin
}

fn points(pts: &[(f32, f32)]) -> Vec<[f32; 2]> {
    pts.iter().map(|&(x, y)| [x, y]).collect()
}

/// The runs of a dashed line that are drawn: `pattern` is its lengths on, off, on, off, again and again. A pattern that isn't made
/// of finite lengths above zero draws the line whole.
pub fn dashes(pts: &[(f32, f32)], closed: bool, pattern: &[f32]) -> Vec<Vec<(f32, f32)>> {
    let mut path = pts.to_vec();
    if closed && let Some(&first) = pts.first() {
        path.push(first);
    }
    if path.len() < 2 {
        return Vec::new();
    }
    let sound = !pattern.is_empty() && pattern.iter().all(|d| d.is_finite() && *d > 0.0);
    if !sound {
        return vec![path];
    }
    let (mut runs, mut run): (Vec<Vec<(f32, f32)>>, Vec<(f32, f32)>) = (Vec::new(), Vec::new());
    let (mut index, mut left) = (0usize, pattern[0]);
    for w in path.windows(2) {
        let [(ax, ay), (bx, by)] = [w[0], w[1]];
        let length = (bx - ax).hypot(by - ay);
        if !length.is_finite() || length == 0.0 {
            continue;
        }
        let mut at = 0.0f32;
        if index % 2 == 0 && run.is_empty() {
            run.push((ax, ay));
        }
        while length - at > left {
            at += left;
            let p = (ax + (bx - ax) * at / length, ay + (by - ay) * at / length);
            if index % 2 == 0 {
                run.push(p);
                runs.push(std::mem::take(&mut run));
            } else {
                run.push(p);
            }
            index += 1;
            left = pattern[index % pattern.len()];
            if runs.len() >= MAX_DASHES {
                return runs;
            }
        }
        left -= length - at;
        if index % 2 == 0 {
            run.push((bx, by));
        }
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs.retain(|r| r.len() >= 2);
    runs
}

/// Paints the drawing's primitives that can reach a canvas of `size` points, in the order they were made.
pub fn base(d: &Drawing, size: (f32, f32), dark: bool) -> Painted {
    paint(d, size, |_, _| true, |c, _| ink(c, dark), HAIRLINE)
}

/// Paints again, in `color` and `fill`, the primitives of `owners`: a selection's or a hover's outline over the plan.
pub fn lit(d: &Drawing, size: (f32, f32), owners: &HashSet<Id>, color: Color, fill: Color) -> Painted {
    if owners.is_empty() {
        return Painted { shape: Shape::new(), pieces: 0, skipped: 0 };
    }
    paint(d, size, |owner, _| owner.is_some_and(|o| owners.contains(&o)), |_, filled| if filled { fill } else { color }, HAIRLINE * 2.5)
}

fn paint(
    d: &Drawing,
    size: (f32, f32),
    wanted: impl Fn(Option<Id>, Part) -> bool,
    color: impl Fn(Rgb, bool) -> Color,
    least: f32,
) -> Painted {
    let (mut shape, mut pieces, mut skipped) = (Shape::new(), 0usize, 0usize);
    for ((prim, owner), part) in d.prims.iter().zip(&d.owners).zip(&d.parts) {
        if !wanted(*owner, *part) {
            continue;
        }
        for p in prim.paint() {
            match p {
                Prim::Text { .. } => {}
                Prim::Image { .. } => skipped += 1,
                Prim::Line { pts, width, color: c, dash, closed } => {
                    let width = if width.is_finite() { width.max(least) } else { least };
                    if !reaches(pts, size, width) {
                        continue;
                    }
                    let c = color(*c, false);
                    match dash {
                        Some(pattern) => {
                            for run in dashes(pts, *closed, pattern) {
                                shape = shape.stroke_with(points(&run), false, width, LineCap::Butt, c);
                                pieces += 1;
                            }
                        }
                        None => {
                            shape = shape.stroke_with(points(pts), *closed, width, LineCap::Round, c);
                            pieces += 1;
                        }
                    }
                }
                Prim::Fill { rings, color: c } => {
                    let flat: Vec<(f32, f32)> = rings.iter().flatten().copied().collect();
                    if !reaches(&flat, size, 1.0) {
                        continue;
                    }
                    let c = color(*c, true);
                    // Ordinary fills are independent polygons; lettering and compound patterns are non-zero fills, with counters.
                    if matches!(part, Part::Text | Part::Picture) || (*part == Part::LinePattern && rings.len() > 1) {
                        for tri in bimcraft_core::text::triangles(rings) {
                            shape = shape.fill(points(&tri), c);
                            pieces += 1;
                        }
                    } else {
                        for ring in rings.iter().filter(|r| r.len() >= 3) {
                            shape = shape.fill(points(ring), c);
                            pieces += 1;
                        }
                    }
                }
            }
        }
    }
    Painted { shape, pieces, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(run: &[(f32, f32)]) -> f32 {
        run.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
    }

    #[test]
    fn a_dash_splits_a_line_into_the_lengths_it_says() {
        let runs = dashes(&[(0.0, 0.0), (100.0, 0.0)], false, &[10.0, 5.0]);
        assert_eq!(runs.len(), 7, "10 on, 5 off, up to 100: seven dashes");
        assert!(runs.iter().all(|r| (length(r) - 10.0).abs() < 1e-3));
        assert_eq!(runs[6], vec![(90.0, 0.0), (100.0, 0.0)], "the seventh starts at 90 and ends at 100");
        assert_eq!(runs[1][0], (15.0, 0.0), "the second starts where the first gap ends");
    }

    #[test]
    fn a_dash_goes_round_a_corner_and_a_ring_closes() {
        let runs = dashes(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false, &[15.0, 100.0]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)], "one dash bends with the line");
        let ring = dashes(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true, &[1000.0, 1.0]);
        assert_eq!(ring[0].last(), Some(&(0.0, 0.0)), "a closed line's last side is dashed too");
    }

    #[test]
    fn a_pattern_that_is_not_one_draws_the_line_whole_and_a_hostile_one_is_capped() {
        let line = [(0.0, 0.0), (10.0, 0.0)];
        for bad in [&[][..], &[0.0, 1.0][..], &[f32::NAN, 1.0][..], &[-1.0, 1.0][..]] {
            assert_eq!(dashes(&line, false, bad), vec![line.to_vec()]);
        }
        assert!(dashes(&[(0.0, 0.0), (1e9, 0.0)], false, &[1e-3, 1e-3]).len() <= MAX_DASHES);
        assert!(dashes(&[], false, &[1.0, 1.0]).is_empty());
        assert!(dashes(&[(1.0, 1.0)], false, &[1.0, 1.0]).is_empty());
    }

    #[test]
    fn what_cannot_reach_the_canvas_is_left_out() {
        let size = (100.0, 100.0);
        assert!(reaches(&[(10.0, 10.0), (20.0, 20.0)], size, 1.0));
        assert!(reaches(&[(-50.0, 50.0), (50.0, 50.0)], size, 1.0), "a line that crosses it");
        assert!(!reaches(&[(200.0, 10.0), (300.0, 20.0)], size, 1.0));
        assert!(!reaches(&[(f32::NAN, 0.0), (1.0, 1.0)], size, 1.0));
        assert!(!reaches(&[], size, 1.0));
    }

    #[test]
    fn dark_ink_is_lightened_on_a_dark_canvas_and_colour_is_kept() {
        assert_eq!(ink(Rgb(0, 0, 0), true), rgb8(225, 225, 225));
        assert_eq!(ink(Rgb(0, 0, 0), false), rgb8(0, 0, 0));
        assert_eq!(ink(Rgb(200, 40, 40), true), rgb8(200, 40, 40));
    }
}
