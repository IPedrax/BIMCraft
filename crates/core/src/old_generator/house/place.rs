//! Geometry for a house graph: a shell of bands the rooms are dealt into.
//!
//! ```text
//!        +-------+--+-------+         the private band (R), north of the shared one:
//!        | bed   |h | bed   |           a hall strip with a column of rooms each side, the bedrooms at the
//!        | clo|b |a |       |           columns' far ends (a closet and an en-suite bath in a strip at the far
//!        +-------+l-+-------+           end of the bedroom), and a closet or laundry at the hall's end
//!   +----+-----+--+--+-------+--+
//!   | living    | kitchen  |dining|   the shared band (P), the hall joined to one room of it
//!   +--------+--+------+----------+
//!        | porch  |                 a porch outside one shared room, closed off by room separation lines
//! ```
//!
//! The shell is where the private band lies against the shared one: flush with an end (`l_shape`, `corner_notch`, `narrow`),
//! centred (`t_shape`), a third along (`side_nub`), stepped past the end (`stepped`), or as wide as it (`bar`). The seed turns
//! and mirrors the whole house last. All in whole millimetres on wall centre lines.

use std::collections::{BTreeMap, BTreeSet};

use super::super::Rng;
use super::{HouseGraph, Kind, LARGE_AREA, Means};

pub const SHELLS: [&str; 7] = ["bar", "l_shape", "corner_notch", "narrow", "t_shape", "side_nub", "stepped"];

const SALT: u64 = 0x4A05_E010;
const HALL_WIDTH: i64 = 1300;
const STRIP_DEPTH: i64 = 1600;
const CLOSET_STRIP: i64 = 1400;
const ENSUITE_WIDTH: i64 = 2200;
/// What a door needs on each side of its leaf in the wall it is in: the jambs, and the walls that cross it.
const DOOR_CLEAR: i64 = 400;
/// Above LARGE_AREA, how far the entry hall's cell reaches past the hall strip over it, west and east.
const ENTRY_PAD_WEST: i64 = 500;
const ENTRY_PAD_EAST: i64 = 600;
/// About what a deck of two runs nets, mm2, which the shared band leaves out of its share of the area.
const DECK_ESTIMATE: f64 = 11.0e6;
/// How deep a deck lies outside its rooms, mm.
const DECK_DEPTH: i64 = 2400;
/// How far the gross area may be from the request.
const AREA_TOLERANCE: f64 = 0.06;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Rect {
    pub fn area(&self) -> i64 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }

    pub fn mid(&self) -> [i64; 2] {
        [(self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2]
    }
}

#[derive(Clone, Debug)]
pub struct DoorAt {
    /// The link of the graph it is the door of; none for the way in from outside.
    pub link: Option<usize>,
    /// The room it opens off, and the one it leads to (none: outside).
    pub rooms: (usize, Option<usize>),
    pub at: [i64; 2],
    pub family: &'static str,
    pub ty: String,
    pub width: i64,
    pub op: &'static str,
    /// A swing door's flips, and what the extractor will say of it: the room it opens into (`exterior` outside) and the jamb.
    pub flip_facing: bool,
    pub flip_hand: bool,
    pub opens_into: Option<String>,
    pub hinge: Option<&'static str>,
    /// The host wall's left unit normal and half thickness, mm, found by `swing::orient` (zero before).
    pub normal: [f64; 2],
    pub half: f64,
}

#[derive(Clone, Debug)]
pub struct WallAt {
    pub exterior: bool,
    pub a: [i64; 2],
    pub b: [i64; 2],
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub shell: &'static str,
    /// A room's rectangle, by its index in the graph.
    pub rects: Vec<Rect>,
    pub doors: Vec<DoorAt>,
    /// Room separation lines: a run of points each, for the open edges and for the porch's three sides.
    pub separators: Vec<Vec<[i64; 2]>>,
    pub walls: Vec<WallAt>,
    /// The outline of the walled house, counter-clockwise, without the porch.
    pub boundary: Vec<[i64; 2]>,
    pub width: i64,
    pub depth: i64,
    pub gross_mm2: i64,
}

/// One way to try: which shell, whether the shared rooms lie in reverse, which side the columns swap to, a proportion.
#[derive(Clone, Copy, Debug)]
struct Variant {
    shell: usize,
    reverse: bool,
    swap: bool,
    deep: bool,
    /// Whether the private band lies against the other end of the shared one.
    right: bool,
}

fn floor100(x: i64) -> i64 {
    x.div_euclid(100) * 100
}

fn ceil100(x: i64) -> i64 {
    -floor100(-x)
}

fn round100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

/// The tries for a house, in the fixed order they are made: the seed's shell first, then the next, and for each shell the
/// variants of its sides.
pub fn attempts<'a>(g: &'a HouseGraph, seed: u64, area_m2: f64) -> impl Iterator<Item = Result<Layout, String>> + 'a {
    let start = Rng::new(seed, SALT).below(SHELLS.len() as u64) as usize;
    let mut variants = Vec::new();
    for s in 0..SHELLS.len() {
        for right in [false, true] {
            for (reverse, swap, deep) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, false, true),
                (true, false, true),
                (false, true, true),
                (true, true, true),
            ] {
                variants.push(Variant { shell: (start + s) % SHELLS.len(), reverse, swap, deep, right });
            }
        }
    }
    variants.into_iter().map(move |v| try_variant(g, seed, area_m2, v))
}

#[derive(Clone, Copy)]
enum Cell {
    Bed(usize),
    Bath,
}

fn least_width(kind: Kind) -> i64 {
    match kind {
        Kind::Living => 3000,
        Kind::Kitchen => 2600,
        Kind::Garage => 3000,
        _ => 2400,
    }
}

fn weight(kind: Kind) -> i64 {
    match kind {
        Kind::Living => 10,
        Kind::Kitchen => 6,
        Kind::Garage => 3,
        _ => 4,
    }
}

/// A rect that fills `[x0, x1] x [y0, y1]`, or why it cannot: a room never has less than its two walls' room.
fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Result<Rect, String> {
    if x1 - x0 < 1000 || y1 - y0 < 1000 {
        return Err(format!("a room of {} x {} mm is too small to be one", x1 - x0, y1 - y0));
    }
    Ok(Rect { x0, y0, x1, y1 })
}

/// How a bedroom of column width `cw` is cut: its main depth for about `net_m2`, and the strip's depth when it has one.
fn bed_depth(cw: i64, net_m2: f64, strip: i64, most: i64) -> (i64, i64) {
    let net_w = (cw - 200).max(1000) as f64;
    let main = ceil100((net_m2 * 1.0e6 / net_w + 150.0) as i64).clamp(2600, most);
    (main, strip)
}

/// A layout for a variant: built once at the area asked, and once more with the shared band resized for what the first
/// came to, when it was not close (the porch and the rooms' rounding are known only once they are placed).
fn try_variant(g: &HouseGraph, seed: u64, area_m2: f64, v: Variant) -> Result<Layout, String> {
    let first = build_once(g, seed, area_m2, v, 0.0)?;
    let off = area_m2 * 1.0e6 - first.gross_mm2 as f64;
    let layout = if off.abs() <= 0.02 * area_m2 * 1.0e6 { first } else { build_once(g, seed, area_m2, v, off)? };
    let gross = layout.gross_mm2 as f64 / 1.0e6;
    if (gross - area_m2).abs() > AREA_TOLERANCE * area_m2 {
        return Err(format!("the plan is {gross:.1} m2 for the {area_m2:.1} asked"));
    }
    Ok(layout)
}

fn build_once(g: &HouseGraph, seed: u64, area_m2: f64, v: Variant, adjust: f64) -> Result<Layout, String> {
    let shell = SHELLS[v.shell];
    let large = area_m2 > LARGE_AREA;
    let mut rng = Rng::new(seed, SALT + 1);
    let least_bed = if area_m2 < 100.0 {
        2800
    } else if area_m2 < 135.0 {
        3200
    } else {
        3600
    };
    let widths: Vec<i64> = g
        .beds
        .iter()
        .map(|b| {
            // The bedrooms of a large house are not wider than 4 m: the shorter column's bedrooms grow deeper to meet the
            // taller one, and a wide one would pass a bedroom's most.
            let w = least_bed + 100 * rng.below(if large { 5 } else { 9 }) as i64;
            // A closet beside an en-suite bath in the strip at the bedroom's end needs the room for both.
            if b.closet.is_some() && b.bath.is_some() { w.max(4100) } else { w }
        })
        .collect();
    let nb = g.beds.len();
    let mut widths = widths;
    if nb == 3 {
        // The third bedroom shares its column with the bath, and is the one that grows to meet the other column.
        widths[2] = least_bed.max(if g.beds[2].closet.is_some() && g.beds[2].bath.is_some() { 4100 } else { 0 });
    }
    let (mut west, mut east): (Vec<Cell>, Vec<Cell>) = match nb {
        0 => return Err("a house with no bedroom".into()),
        1 => (vec![Cell::Bath, Cell::Bed(0)], vec![]),
        2 => (vec![Cell::Bath, Cell::Bed(0)], vec![Cell::Bed(1)]),
        3 if large => (vec![Cell::Bath, Cell::Bed(0)], vec![Cell::Bed(1), Cell::Bed(2)]),
        3 => (vec![Cell::Bed(0), Cell::Bed(1)], vec![Cell::Bath, Cell::Bed(2)]),
        _ => (vec![Cell::Bath, Cell::Bed(0), Cell::Bed(1)], vec![Cell::Bed(2), Cell::Bed(3)]),
    };
    if v.swap {
        std::mem::swap(&mut west, &mut east);
    }
    let column_width =
        |col: &[Cell]| col.iter().filter_map(|c| if let Cell::Bed(k) = c { widths.get(*k).copied() } else { None }).max().unwrap_or(0);
    let (cw_w, cw_e) = (column_width(&west), column_width(&east));
    // Above LARGE_AREA the bedrooms grow with the house, as far as the private band's hall may be long.
    let net = 7.5 + area_m2 * 0.018 + if large { 0.05 * (area_m2 - LARGE_AREA) } else { 0.0 };
    let most_main = if large && nb >= 4 { 4000 } else { 4400 };

    // The depth of each cell of a column: a bedroom's main part and its strip, a bath's.
    let depths = |col: &[Cell], cw: i64| -> Vec<(i64, i64)> {
        col.iter()
            .map(|c| match c {
                Cell::Bed(k) => {
                    let bed = &g.beds[*k];
                    let strip = match (bed.closet.is_some(), bed.bath.is_some()) {
                        (false, false) => 0,
                        (true, false) => CLOSET_STRIP,
                        _ => STRIP_DEPTH,
                    };
                    bed_depth(cw, net + if *k == 0 { 1.0 } else { 0.0 }, strip, most_main)
                }
                Cell::Bath => (ceil100((5.0e6 / (cw - 200).max(1000) as f64 + 150.0) as i64).clamp(1900, 3200), 0),
            })
            .collect()
    };
    let (mut dw, mut de) = (depths(&west, cw_w), depths(&east, cw_e));
    let total = |d: &[(i64, i64)]| d.iter().map(|(m, s)| m + s).sum::<i64>();
    let h = total(&dw).max(total(&de));
    for (col, d, cw) in [(&west, &mut dw, cw_w), (&east, &mut de, cw_e)] {
        if col.is_empty() {
            continue;
        }
        let mut short = h - total(d);
        // The shorter column grows a bath first, up to what a bath may be, then its bedrooms alike.
        if let (Some(Cell::Bath), Some(first)) = (col.first(), d.first_mut()) {
            let most = ceil100((9.0e6 / (cw - 200).max(1000) as f64 + 150.0) as i64);
            let grow = short.min((most - first.0).max(0));
            first.0 += grow;
            short -= grow;
        }
        let mut beds_left = col.iter().filter(|c| matches!(c, Cell::Bed(_))).count() as i64;
        for (cell, depth) in col.iter().zip(d.iter_mut()) {
            if matches!(cell, Cell::Bed(_)) && beds_left > 0 {
                let share = if beds_left == 1 { short } else { ceil100(short / beds_left).min(short) };
                beds_left -= 1;
                depth.0 += share;
                short -= share;
                if (cw - 200) * (depth.0 - 100) > 25_000_000 {
                    return Err(format!(
                        "a bedroom would be about {:.1} m2, larger than a bedroom may be ({cw} mm wide, {} mm deep)",
                        (cw - 200) as f64 * (depth.0 - 100) as f64 / 1.0e6,
                        depth.0
                    ));
                }
            }
        }
        if short != 0 {
            return Err("the columns of the private band do not come out level".into());
        }
    }
    let cap_depth = g.cap.map_or(0, |c| if g.rooms[c].kind == Kind::Laundry { 1900 } else { 1500 });
    if h < 2400 {
        return Err("the private band is too short for its hall".into());
    }
    let hall_most = if large { 15_900_000 } else { 11_500_000 };
    if (HALL_WIDTH - 100) * (h - if g.cap.is_some() { 100 } else { 200 }) > hall_most {
        return Err("the hall would be longer than a hall may be".into());
    }
    let wr = cw_w + HALL_WIDTH + cw_e;

    // The shared band, from the area left over.
    let porch_estimate = if g.porch.is_some() { 6.5e6 } else { 0.0 } + if g.deck.is_some() { DECK_ESTIMATE } else { 0.0 };
    let order: Vec<usize> = if v.reverse { g.public.iter().rev().copied().collect() } else { g.public.clone() };
    let least: i64 = order.iter().map(|&r| least_width(g.rooms[r].kind)).sum();
    let cap_area = if g.cap.is_some() { (HALL_WIDTH * cap_depth) as f64 } else { 0.0 };
    let a_p = area_m2 * 1.0e6 - (wr * h) as f64 - porch_estimate - cap_area + adjust;
    let minimum = order
        .iter()
        .map(|&r| match g.rooms[r].kind {
            Kind::Living => 11.0e6,
            Kind::Kitchen => 9.0e6,
            _ => 7.0e6,
        })
        .sum::<f64>();
    if a_p < minimum {
        return Err(format!("the shared rooms would have {:.1} m2, their least being {:.1}", a_p / 1.0e6, minimum / 1.0e6));
    }
    let (wp, dp) = if shell == "bar" {
        let dp = round100(a_p / wr as f64);
        (wr, dp)
    } else {
        let ratio = if v.deep { 2.8 } else { 1.8 };
        let dp = round100((a_p / ratio).sqrt()).clamp(4000, 6500);
        (round100(a_p / dp as f64), dp)
    };
    if !(3600..=6500).contains(&dp) || wp < least {
        return Err(format!("a shared band {wp} mm long and {dp} mm deep does not fit"));
    }
    // The shared rooms before the hall's room and after it, which the hall must leave room for.
    let at = order.iter().position(|&r| r == g.hall_to).ok_or("the hall joins no shared room")?;
    let (before, after) = (&order[..at], &order[at + 1..]);
    let pre_min: i64 = before.iter().map(|&r| least_width(g.rooms[r].kind)).sum();
    let suf_min: i64 = after.iter().map(|&r| least_width(g.rooms[r].kind)).sum();
    // Above LARGE_AREA the hall's room is the entry hall, a cell a little wider than the hall strip over it, and the rooms
    // either side of it take what is left.
    let (pad_w, pad_e) = if large { (ENTRY_PAD_WEST, ENTRY_PAD_EAST) } else { (0, 0) };
    let all_weight: i64 = order.iter().map(|&r| weight(g.rooms[r].kind)).sum();
    let pre_weight: i64 = before.iter().map(|&r| weight(g.rooms[r].kind)).sum();

    // The shell says where along the shared band the private band's west edge may lie, and where it likes to; the hall's
    // place then says where it can.
    let d = wp - wr;
    let (dd, widest) = (d.abs(), wp.max(wr));
    let inset = if d >= 0 { (1200, d - 1200) } else { (d + 1200, -1200) };
    let (r_lo, r_hi, pref) = match shell {
        "bar" => Some((0, 0, 0)),
        "corner_notch" => (dd >= 1500 && dd * 4 < widest).then_some((0, 0, 0)),
        "l_shape" => (dd * 4 >= widest && dd * 2 < widest).then_some((0, 0, 0)),
        "narrow" => (dd * 2 >= widest).then_some((0, 0, 0)),
        "t_shape" => (dd >= 2400).then_some((inset.0, inset.1, d / 2)),
        "side_nub" => (dd >= 4800).then_some((inset.0, inset.1, d / 4)),
        _ => {
            let overhang = (wr / 3).clamp(1500, 2400) / 100 * 100;
            Some(((d + 1500).max(1200), d + 2400, d + overhang))
        }
    }
    .ok_or_else(|| format!("{shell} needs the private band narrower or wider than {wr} mm against {wp} mm"))?;
    let (r_lo, r_hi, pref) = if v.right && shell != "bar" { (d - r_hi, d - r_lo, d - pref) } else { (r_lo, r_hi, pref) };
    let lo = ceil100(r_lo.max(pre_min + pad_w - cw_w).max(-cw_w));
    let hi = floor100(r_hi.min(wp - suf_min - pad_e - HALL_WIDTH - cw_w));
    if lo > hi {
        return Err(format!(
            "{shell} puts the hall where the shared rooms (they need {pre_min} and {suf_min} mm either side of it) have no place"
        ));
    }
    let rx0 = floor100(pref).max(lo).min(hi);
    let (hx0, hx1) = (rx0 + cw_w, rx0 + cw_w + HALL_WIDTH);
    if hx0 < 0 || hx1 > wp {
        return Err(format!("the hall ({hx0} to {hx1}) is not over the shared band (0 to {wp})"));
    }

    let a_hi = hx0;
    let b_lo = hx1;
    if (!before.is_empty() && pre_min > a_hi) || (!after.is_empty() && wp - suf_min < b_lo) {
        return Err(format!(
            "the hall at {hx0} to {hx1} leaves the shared rooms no place: they need {pre_min} and {suf_min} mm either side"
        ));
    }
    let a = if large {
        hx0 - pad_w
    } else if before.is_empty() {
        0
    } else {
        floor100((wp * pre_weight / all_weight).max(pre_min).min(a_hi))
    };
    let b = if large {
        hx1 + pad_e
    } else if after.is_empty() {
        wp
    } else {
        let want = wp - wp * (all_weight - pre_weight - weight(g.rooms[g.hall_to].kind)) / all_weight;
        ceil100(want.max(b_lo).min(wp - suf_min))
    };
    if (!before.is_empty() && a < pre_min) || (!after.is_empty() && b > wp - suf_min) || b - a < least_width(g.rooms[g.hall_to].kind) {
        return Err(format!("the shared rooms do not fit either side of the hall at {hx0} to {hx1}"));
    }
    let mut rects: Vec<Option<Rect>> = vec![None; g.rooms.len()];
    let mut too_big = None;
    let mut deal = |rooms: &[usize], from: i64, to: i64| {
        let min: i64 = rooms.iter().map(|&r| least_width(g.rooms[r].kind)).sum();
        let wt: i64 = rooms.iter().map(|&r| weight(g.rooms[r].kind)).sum::<i64>().max(1);
        let mut x = from;
        for (i, &r) in rooms.iter().enumerate() {
            let w = if i + 1 == rooms.len() {
                to - x
            } else {
                floor100(least_width(g.rooms[r].kind) + (to - from - min) * weight(g.rooms[r].kind) / wt)
            };
            rects[r] = Some(Rect { x0: x, y0: 0, x1: x + w, y1: dp });
            // About what the room nets, to the least and most its kind may be: the project says exactly, after.
            let est = (w - 200) as f64 * (dp - 200) as f64 / 1.0e6;
            let (lo, hi) = match g.rooms[r].kind {
                Kind::Living => (10.5, 43.0),
                Kind::Kitchen => (8.5, 33.5),
                Kind::Garage => (16.5, 44.0),
                Kind::Hall => (3.0, 15.5),
                _ => (6.5, 24.0),
            };
            if !(lo..=hi).contains(&est) && too_big.is_none() {
                too_big = Some(format!("`{}` would be about {est:.1} m2", g.rooms[r].id));
            }
            x += w;
        }
    };
    deal(before, 0, a);
    deal(&[g.hall_to], a, b);
    deal(after, b, wp);
    if let Some(why) = too_big {
        return Err(why);
    }

    // The private band: the hall, its end, and the two columns.
    let top = dp + h;
    rects[g.hall] = Some(rect(hx0, dp, hx1, top)?);
    if let Some(c) = g.cap {
        rects[c] = Some(rect(hx0, top, hx1, top + cap_depth)?);
    }
    for (col, ds, x_in, x_out) in [(&west, &dw, hx0, rx0), (&east, &de, hx1, hx1 + cw_e)] {
        let (lo, hi) = (x_in.min(x_out), x_in.max(x_out));
        let mut y = dp;
        for (cell, &(main, strip)) in col.iter().zip(ds) {
            match cell {
                Cell::Bath => rects[g.hall_bath] = Some(rect(lo, y, hi, y + main)?),
                Cell::Bed(k) => {
                    let bed = &g.beds[*k];
                    rects[bed.room] = Some(rect(lo, y, hi, y + main)?);
                    let (s0, s1) = (y + main, y + main + strip);
                    let outer_is_low = x_out < x_in;
                    // The strip: an en-suite bath on the outer side, the closet on the inner, or either of them across it.
                    match (bed.bath, bed.closet) {
                        (Some(bath), Some(closet)) => {
                            let cut = if outer_is_low { lo + ENSUITE_WIDTH } else { hi - ENSUITE_WIDTH };
                            let (bath_r, closet_r) = if outer_is_low {
                                (rect(lo, s0, cut, s1)?, rect(cut, s0, hi, s1)?)
                            } else {
                                (rect(cut, s0, hi, s1)?, rect(lo, s0, cut, s1)?)
                            };
                            if closet_r.x1 - closet_r.x0 < 1200 {
                                return Err("a bedroom too narrow for an en-suite bath and a closet".into());
                            }
                            rects[bath] = Some(bath_r);
                            rects[closet] = Some(closet_r);
                        }
                        (Some(room), None) | (None, Some(room)) => rects[room] = Some(rect(lo, s0, hi, s1)?),
                        (None, None) => {}
                    }
                }
            }
            y += main + strip;
        }
    }
    // The porch, outside the shared room it is off.
    let mut outside: Vec<Rect> = Vec::new();
    if let (Some(p), Some(off)) = (g.porch, Some(g.porch_off)) {
        let r = rects[off].ok_or("the porch's room has no place")?;
        let room_w = r.x1 - r.x0;
        if room_w < 2400 {
            return Err("the room the porch is off is too narrow for it".into());
        }
        let pw = ((room_w - 400).clamp(2000, 4800)) / 100 * 100;
        let x0 = floor100((r.x0 + r.x1) / 2 - pw / 2);
        let pd = if v.deep { 2400 } else { 2000 };
        let pr = Rect { x0, y0: -pd, x1: x0 + pw, y1: 0 };
        rects[p] = Some(pr);
        outside.push(pr);
    }
    // The deck, outside the one or two shared rooms it is off: across the wall between two, so each has a run of it.
    if let Some(d) = g.deck {
        let dr = match g.deck_off[..] {
            [one] => {
                let r = rects[one].ok_or("the deck's room has no place")?;
                let room_w = r.x1 - r.x0;
                // A deck of at least 6 m2 net, which is as small as the pack has one.
                let dw = ((room_w - 400).min(4800)) / 100 * 100;
                if dw < 3000 {
                    return Err("the room the deck is off is too narrow for it".into());
                }
                let x0 = floor100((r.x0 + r.x1) / 2 - dw / 2);
                Rect { x0, y0: -DECK_DEPTH, x1: x0 + dw, y1: 0 }
            }
            [x, y] => {
                let (ra, rb) = (rects[x].ok_or("the deck's room has no place")?, rects[y].ok_or("the deck's room has no place")?);
                let (left, right) = if ra.x1 == rb.x0 { (ra, rb) } else { (rb, ra) };
                if left.x1 != right.x0 {
                    return Err("the rooms the deck is off are not side by side".into());
                }
                let run = ((left.x1 - left.x0).min(right.x1 - right.x0) - 400).min(3000) / 100 * 100;
                if run < 900 + 2 * DOOR_CLEAR + 100 {
                    return Err("the rooms the deck is off are too narrow for it".into());
                }
                Rect { x0: left.x1 - run, y0: -DECK_DEPTH, x1: left.x1 + run, y1: 0 }
            }
            _ => return Err("the deck is off one or two rooms".into()),
        };
        rects[d] = Some(dr);
        outside.push(dr);
    }
    let rects: Vec<Rect> = rects.into_iter().collect::<Option<Vec<_>>>().ok_or("a room has no place")?;
    assemble(g, seed, shell, rects, outside)
}

/// The shared run of two rectangles' edges: `(vertical, line, lo, hi)`, a wall along `x = line` when vertical.
pub(crate) fn shared(a: &Rect, b: &Rect) -> Option<(bool, i64, i64, i64)> {
    let along = |lo0: i64, hi0: i64, lo1: i64, hi1: i64| {
        let (lo, hi) = (lo0.max(lo1), hi0.min(hi1));
        (hi > lo).then_some((lo, hi))
    };
    for (left, right) in [(a, b), (b, a)] {
        if left.x1 == right.x0
            && let Some((lo, hi)) = along(left.y0, left.y1, right.y0, right.y1)
        {
            return Some((true, left.x1, lo, hi));
        }
        if left.y1 == right.y0
            && let Some((lo, hi)) = along(left.x0, left.x1, right.x0, right.x1)
        {
            return Some((false, left.y1, lo, hi));
        }
    }
    None
}

pub(crate) fn point(vertical: bool, line: i64, along: i64) -> [i64; 2] {
    if vertical { [line, along] } else { [along, line] }
}

/// The door of a link, by its means and how long a run it has: its family, type, width and operation.
fn door_for(means: Means, run: i64, pick: u64, porch: bool) -> Option<(&'static str, String, i64, &'static str)> {
    let fits = |w: i64| run >= w + DOOR_CLEAR;
    match means {
        Means::Swing => {
            let order: [i64; 2] = if pick == 0 { [900, 800] } else { [800, 900] };
            order.into_iter().find(|w| fits(*w)).map(|w| ("Single Door", format!("{w} x 2100"), w, "swing"))
        }
        Means::Sliding => {
            let widths: &[i64] = if porch { &[2000, 1500, 1200] } else { &[1500, 1200] };
            widths.iter().copied().find(|w| fits(*w)).map(|w| ("Sliding Door", format!("{w} x 2100"), w, "sliding"))
        }
        Means::Folding => {
            [900, 800, 600].into_iter().find(|w| fits(*w)).map(|w| (super::FOLDING_FAMILY, format!("{w} x 2100"), w, "folding"))
        }
        Means::Open => None,
    }
}

fn assemble(g: &HouseGraph, seed: u64, shell: &'static str, rects: Vec<Rect>, outside: Vec<Rect>) -> Result<Layout, String> {
    let out_rooms: Vec<usize> = g.outside().collect();
    let walled: Vec<(usize, Rect)> = rects.iter().enumerate().filter(|(i, _)| !out_rooms.contains(i)).map(|(i, r)| (i, *r)).collect();
    let gross: i64 = rects.iter().map(Rect::area).sum();
    let open: BTreeSet<(usize, usize)> = g.links.iter().filter(|l| l.means == Means::Open).map(|l| (l.a.min(l.b), l.a.max(l.b))).collect();

    // Every edge of every room, cut where another room's corner lies on it: one piece for each stretch with a wall to it.
    type Edges = Vec<(i64, i64, usize)>;
    let mut lines: BTreeMap<(bool, i64), Edges> = BTreeMap::new();
    for (i, r) in &walled {
        for (vertical, line, lo, hi) in
            [(true, r.x0, r.y0, r.y1), (true, r.x1, r.y0, r.y1), (false, r.y0, r.x0, r.x1), (false, r.y1, r.x0, r.x1)]
        {
            lines.entry((vertical, line)).or_default().push((lo, hi, *i));
        }
    }
    let mut exterior: Vec<(bool, i64, i64, i64)> = Vec::new();
    let mut interior: Vec<(bool, i64, i64, i64)> = Vec::new();
    let mut opened: Vec<(bool, i64, i64, i64)> = Vec::new();
    for ((vertical, line), edges) in &lines {
        let cuts: BTreeSet<i64> = edges.iter().flat_map(|(lo, hi, _)| [*lo, *hi]).collect();
        let cuts: Vec<i64> = cuts.into_iter().collect();
        for pair in cuts.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let rooms: Vec<usize> = edges.iter().filter(|(lo, hi, _)| *lo <= p && q <= *hi).map(|e| e.2).collect();
            match rooms[..] {
                [] => {}
                [_] => exterior.push((*vertical, *line, p, q)),
                [x, y] if open.contains(&(x.min(y), x.max(y))) => opened.push((*vertical, *line, p, q)),
                [_, _] => interior.push((*vertical, *line, p, q)),
                _ => return Err("three rooms share a wall".into()),
            }
        }
    }
    let merge = |mut pieces: Vec<(bool, i64, i64, i64)>| -> Vec<(bool, i64, i64, i64)> {
        pieces.sort();
        let mut out: Vec<(bool, i64, i64, i64)> = Vec::new();
        for p in pieces {
            match out.last_mut() {
                Some(last) if last.0 == p.0 && last.1 == p.1 && last.3 == p.2 => last.3 = p.3,
                _ => out.push(p),
            }
        }
        out
    };
    let corner = |s: &(bool, i64, i64, i64)| (point(s.0, s.1, s.2), point(s.0, s.1, s.3));
    let boundary = trace(&exterior)?;

    // Doors: the link's, at the middle of the run the two rooms share; and the way in, in the south wall of the entry room.
    let mut doors = Vec::new();
    let mut separators: Vec<Vec<[i64; 2]>> = Vec::new();
    for (k, l) in g.links.iter().enumerate() {
        let Some((vertical, line, lo, hi)) = shared(&rects[l.a], &rects[l.b]) else {
            return Err(format!("`{}` and `{}` have no wall in common for their link", g.rooms[l.a].id, g.rooms[l.b].id));
        };
        if l.means == Means::Open {
            continue;
        }
        let pick = Rng::new(seed, SALT + 100 + k as u64).below(2);
        let porch_door = out_rooms.contains(&l.a) || out_rooms.contains(&l.b);
        let Some((family, ty, width, op)) = door_for(l.means, hi - lo, pick, porch_door) else {
            return Err(format!(
                "the wall of {} mm between `{}` and `{}` is too short for its door",
                hi - lo,
                g.rooms[l.a].id,
                g.rooms[l.b].id
            ));
        };
        doors.push(DoorAt {
            link: Some(k),
            rooms: (l.a, Some(l.b)),
            at: point(vertical, line, (lo + hi) / 2),
            family,
            ty,
            width,
            op,
            flip_facing: false,
            flip_hand: false,
            opens_into: None,
            hinge: None,
            normal: [0.0, 0.0],
            half: 0.0,
        });
    }
    let e = &rects[g.entry];
    let run = e.x1 - e.x0;
    if run < 900 + DOOR_CLEAR {
        return Err("the entry room is too narrow for the way in".into());
    }
    doors.push(DoorAt {
        link: None,
        rooms: (g.entry, None),
        at: [(e.x0 + e.x1) / 2, e.y0],
        family: "Single Door",
        ty: "900 x 2100".into(),
        width: 900,
        op: "swing",
        flip_facing: false,
        flip_hand: false,
        opens_into: None,
        hinge: None,
        normal: [0.0, 0.0],
        half: 0.0,
    });
    for piece in merge(opened) {
        let (a, b) = corner(&piece);
        separators.push(vec![a, b]);
    }
    for p in &outside {
        separators.push(vec![[p.x0, 0], [p.x0, p.y0], [p.x1, p.y0], [p.x1, 0]]);
    }
    let walls_out: Vec<WallAt> = merge(exterior)
        .iter()
        .map(|s| (true, s))
        .chain(merge(interior).iter().map(|s| (false, s)))
        .map(|(exterior, s)| {
            let (a, b) = corner(s);
            WallAt { exterior, a, b }
        })
        .collect();

    // Turned and mirrored by the seed, then moved so that the house's south-west corner is the origin.
    let mut turn = Rng::new(seed, SALT + 2);
    let (quarters, mirror) = (turn.below(4), turn.below(2) == 1);
    let tf = |p: [i64; 2]| -> [i64; 2] {
        let [x, y] = if mirror { [-p[0], p[1]] } else { p };
        match quarters {
            0 => [x, y],
            1 => [-y, x],
            2 => [-x, -y],
            _ => [y, -x],
        }
    };
    let mut every: Vec<[i64; 2]> = Vec::new();
    for r in &rects {
        every.extend([[r.x0, r.y0], [r.x1, r.y1]].map(tf));
    }
    let (min_x, min_y) = (every.iter().map(|p| p[0]).min().unwrap_or(0), every.iter().map(|p| p[1]).min().unwrap_or(0));
    let (max_x, max_y) = (every.iter().map(|p| p[0]).max().unwrap_or(0), every.iter().map(|p| p[1]).max().unwrap_or(0));
    let mv = |p: [i64; 2]| -> [i64; 2] {
        let q = tf(p);
        [q[0] - min_x, q[1] - min_y]
    };
    let ordered = |a: [i64; 2], b: [i64; 2]| if a <= b { (a, b) } else { (b, a) };
    let rects: Vec<Rect> = rects
        .iter()
        .map(|r| {
            let (a, b) = (mv([r.x0, r.y0]), mv([r.x1, r.y1]));
            Rect { x0: a[0].min(b[0]), y0: a[1].min(b[1]), x1: a[0].max(b[0]), y1: a[1].max(b[1]) }
        })
        .collect();
    for d in &mut doors {
        d.at = mv(d.at);
    }
    let separators = separators.into_iter().map(|run| run.into_iter().map(mv).collect()).collect();
    let walls = walls_out
        .into_iter()
        .map(|w| {
            let (a, b) = ordered(mv(w.a), mv(w.b));
            WallAt { exterior: w.exterior, a, b }
        })
        .collect();
    let boundary: Vec<[i64; 2]> = boundary.into_iter().map(mv).collect();
    let boundary = counter_clockwise(boundary);
    Ok(Layout { shell, rects, doors, separators, walls, boundary, width: max_x - min_x, depth: max_y - min_y, gross_mm2: gross })
}

pub(crate) fn counter_clockwise(mut points: Vec<[i64; 2]>) -> Vec<[i64; 2]> {
    let twice: i64 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum();
    if twice < 0 {
        points.reverse();
    }
    points
}

/// The outline the outside walls make, as the corners of its one loop.
pub(crate) fn trace(exterior: &[(bool, i64, i64, i64)]) -> Result<Vec<[i64; 2]>, String> {
    let mut next: BTreeMap<[i64; 2], Vec<[i64; 2]>> = BTreeMap::new();
    for &(vertical, line, lo, hi) in exterior {
        let (a, b) = (point(vertical, line, lo), point(vertical, line, hi));
        next.entry(a).or_default().push(b);
        next.entry(b).or_default().push(a);
    }
    if next.values().any(|v| v.len() != 2) {
        return Err("the outside walls do not make one simple loop".into());
    }
    let Some((&start, _)) = next.iter().next() else { return Err("no outside walls".into()) };
    let mut loop_points = vec![start];
    let (mut prev, mut here) = (start, next[&start][0]);
    while here != start {
        loop_points.push(here);
        let step = &next[&here];
        let there = if step[0] == prev { step[1] } else { step[0] };
        prev = here;
        here = there;
        if loop_points.len() > next.len() {
            return Err("the outside walls do not close".into());
        }
    }
    if loop_points.len() != next.len() {
        return Err("the outside walls make more than one loop".into());
    }
    let n = loop_points.len();
    let corners: Vec<[i64; 2]> = (0..n)
        .filter(|&i| {
            let (a, b, c) = (loop_points[(i + n - 1) % n], loop_points[i], loop_points[(i + 1) % n]);
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) != 0
        })
        .map(|i| loop_points[i])
        .collect();
    Ok(corners)
}
