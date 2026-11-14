//! Spot coordinates in a plan, each as its type says (`crate::spots`): its symbol on the point, its leader from the point
//! to its end with an arrowhead on the point (filled, empty, a dot, an empty dot or none) in its own pens and colour, a
//! shoulder there, and its lines of text over or under the shoulder in their own colour, over the paper where its
//! background is opaque. Sizes are mm on paper, so a spot coordinate reads the same at any scale.

use super::*;
use crate::dims::Background;
use crate::model::Spot;
use crate::spots::{Arrowhead, SpotCoordinateType, Symbol, TextLocation};

/// Where a spot coordinate's leader ends with none given: up and to the right of the point, this many mm on paper each way.
pub const END: f64 = 8.0;

/// How many model mm a mm on paper is in drawing `d`.
fn paper_mm(d: &Drawing) -> f64 {
    f64::from(d.k) / d.view.scale * sheet::DENSITY
}

/// Where spot coordinate `sp`'s leader ends in drawing `d`: its own end, else [`END`] mm on paper up and right of its point.
fn end_in(d: &Drawing, sp: &Spot) -> P2 {
    sp.end.unwrap_or_else(|| sp.at + P2(1.0, 1.0) * (END * paper_mm(d)))
}

/// Where spot coordinate `sp`'s leader ends in its plan as `view` shows it: what a front end puts its grip on.
pub fn leader_end(m: &Model, view: View, sp: &Spot) -> P2 {
    let mm = f64::from(annotation(m, sp.view, view)) / view.scale * sheet::DENSITY;
    sp.end.unwrap_or_else(|| sp.at + P2(1.0, 1.0) * (END * mm))
}

/// The turn of what `sp` reads, degrees, when its type turns its text with it.
fn turn(m: &Model, sp: &Spot, t: &SpotCoordinateType) -> f64 {
    if !t.rotate_with_component {
        return 0.0;
    }
    match sp.host.and_then(|h| m.elements.get(h)) {
        Some(Element::Component(c)) => c.angle,
        Some(Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c)) => c.angle,
        _ => 0.0,
    }
}

/// A spot coordinate, as its type says.
pub(super) fn spot_coordinate(d: &mut Drawing, m: &Model, sp: &Spot) {
    let t = m.spot_coordinate_type(sp);
    let mm = paper_mm(d);
    let ink = Rgb(t.leader_color[0], t.leader_color[1], t.leader_color[2]);
    let p = sp.at;
    let half = t.symbol_size * mm / 2.0;
    match t.symbol {
        Symbol::None => {}
        Symbol::Cross | Symbol::Target => {
            d.line(&[p - P2(half, 0.0), p + P2(half, 0.0)], t.leader_pen, ink, false);
            d.line(&[p - P2(0.0, half), p + P2(0.0, half)], t.leader_pen, ink, false);
            if t.symbol == Symbol::Target {
                d.line(&circle(p, half * 0.7), t.leader_pen, ink, true);
            }
        }
    }
    let text = m.spot_coordinate_text(sp).join("\n");
    let tt = t.lettering();
    let layout = crate::text::layout(&tt, &text, &crate::text::styles(&tt, &text, false, &[]));
    let [l, top, r, b] = layout.frame(0.0);
    let (w, h) = ((r - l) * mm, (b - top) * mm);
    // Level on the sheet however the view is turned, unless its type turns it with its component.
    let a = turn(m, sp, &t).to_radians();
    let model = a - d.view.angle.to_radians();
    let (along, up) = (P2(model.cos(), model.sin()), P2(-model.sin(), model.cos()));
    if !sp.leader {
        // Beside the point, past the symbol, its middle level with it.
        let from = p + along * (half + t.text_offset_symbol * mm);
        letter(d, &t, &text, from + up * (h / 2.0), a);
        return;
    }
    let end = end_in(d, sp);
    d.line(&[p, end], t.leader_pen, ink, false);
    if p.dist(end) > 1e-9 {
        arrowhead(d, &t, p, (p - end).unit(), t.arrowhead_size * mm, ink);
    }
    // The shoulder runs on the side the leader leaves the point to, as long as the text and its offsets.
    let side = if (end - p).dot(along) < 0.0 { -1.0 } else { 1.0 };
    let off = t.text_offset_leader * mm;
    let shoulder = along * (side * (w + 2.0 * off));
    if sp.shoulder {
        d.line(&[end, end + shoulder], t.leader_pen, ink, false);
    }
    // The text's left end: past the leader's end on the shoulder's side, its whole width back where that's the left.
    let left = end + along * (if side > 0.0 { off } else { -(off + w) });
    let corner = match t.text_location {
        TextLocation::AboveLeader => left + up * (off + h),
        TextLocation::BelowLeader => left - up * off,
    };
    letter(d, &t, &text, corner, a);
}

fn circle(c: P2, r: f64) -> Vec<P2> {
    (0..32).map(|i| c + P2((f64::from(i) * PI / 16.0).cos(), (f64::from(i) * PI / 16.0).sin()) * r).collect()
}

/// An arrowhead `size` long on point `p`, the leader coming to it along `dir`: a triangle 15° either side of it, filled or
/// outlined, or a dot `size` / 2 across, filled or outlined.
fn arrowhead(d: &mut Drawing, t: &SpotCoordinateType, p: P2, dir: P2, size: f64, ink: Rgb) {
    let (back, wide) = (p - dir * size, dir.perp() * (size * 0.268));
    match t.leader_arrowhead {
        Arrowhead::None => {}
        Arrowhead::Filled => d.fill(&[vec![p, back + wide, back - wide]], ink),
        Arrowhead::Empty => {
            // The paper inside, so the leader doesn't show through it.
            d.fill(&[vec![p, back + wide, back - wide]], PAPER);
            d.line(&[p, back + wide, back - wide], t.arrowhead_pen, ink, true);
        }
        Arrowhead::Dot => d.fill(&[circle(p, size / 4.0)], ink),
        Arrowhead::EmptyDot => {
            d.fill(&[circle(p, size / 4.0)], PAPER);
            d.line(&circle(p, size / 4.0), t.arrowhead_pen, ink, true);
        }
    }
}

/// `text`, in the type's lettering and colour, its top left corner at model point `corner`, turned `angle` radians on the
/// sheet.
fn letter(d: &mut Drawing, t: &SpotCoordinateType, text: &str, corner: P2, angle: f64) {
    if text.is_empty() {
        return;
    }
    let tt = t.lettering();
    let color = Rgb(t.text_color[0], t.text_color[1], t.text_color[2]);
    let layout = crate::text::layout(&tt, text, &crate::text::styles(&tt, text, false, &[]));
    let k = d.px(sheet::DENSITY as f32);
    let [l, top, r, b] = layout.frame(0.0);
    // Clockwise on screen, y down.
    let angle = -(angle as f32);
    let origin = NoteFrame { at: (0.0, 0.0), k, angle }.px((l, top));
    let c = d.view.px(corner);
    let frame = NoteFrame { at: (c.0 - origin.0, c.1 - origin.1), k, angle };
    if t.background == Background::Opaque {
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
    for glyph in &drawn.fill {
        d.push(Prim::Fill { rings: glyph.iter().map(|r| r.iter().copied().map(f).collect()).collect(), color });
    }
    for (pts, width) in drawn.strokes {
        d.push(Prim::Line { pts: pts.into_iter().map(f).collect(), width: (width * k).max(1.0), color, dash: None, closed: false });
    }
    for (a, b) in drawn.underlines {
        let width = (drawn.underline_width * k).max(1.0);
        d.push(Prim::Line { pts: vec![frame.px(a), frame.px(b)], width, color, dash: None, closed: false });
    }
    d.part = part;
}
