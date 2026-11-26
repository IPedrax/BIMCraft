//! A framing type's picture in its Type Properties: a framed wall in elevation, a window and a door in it, each kind of member
//! drawn where it stands (studs, top and bottom plates, kings, jacks, headers, cripples, a sill, blocking), as many and as
//! thick as the type's settings make them, so a change shows as it is typed. A click on a member picks it, and the rows of the
//! type's settings that member is built by light up in the list beside it.

use bimcraft_core::framing::{FramingType, Layout};
use bimcraft_i18n::msg;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::lang::t;
use crate::style::pal;

/// A kind of member the picture shows, or an opening's rough opening.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Stud,
    TopPlate,
    BottomPlate,
    King,
    Jack,
    Header,
    Cripple,
    Sill,
    Blocking,
    Window,
    Door,
}

impl Part {
    /// What it's called, on hover and under the picture.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Part::Stud => msg!("Stud"),
            Part::TopPlate => msg!("Top Plate"),
            Part::BottomPlate => msg!("Bottom Plate"),
            Part::King => msg!("King Stud"),
            Part::Jack => msg!("Jack Stud"),
            Part::Header => msg!("Header"),
            Part::Cripple => msg!("Cripple Stud"),
            Part::Sill => msg!("Sill"),
            Part::Blocking => msg!("Blocking"),
            Part::Window => msg!("Window Rough Opening"),
            Part::Door => msg!("Door Rough Opening"),
        }
    }

    /// The labels of the settings rows it is built by, as the list shows them (English; the caller translates).
    pub(crate) fn rows(self) -> &'static [&'static str] {
        match self {
            Part::Stud => &[msg!("Spacing"), msg!("Thickness"), msg!("Depth"), msg!("Layout"), msg!("Material"), msg!("Stock Lengths")],
            Part::TopPlate => &[msg!("Top Plates"), msg!("Thickness")],
            Part::BottomPlate => &[msg!("Bottom Plates"), msg!("Thickness")],
            Part::King => &[msg!("Kings"), msg!("Thickness")],
            Part::Jack => &[msg!("Jacks"), msg!("Thickness")],
            Part::Header => &[msg!("Headers")],
            Part::Cripple => &[msg!("Spacing"), msg!("Thickness")],
            Part::Sill => &[msg!("Sill Plates"), msg!("Thickness")],
            Part::Blocking => &[msg!("Blocking Heights"), msg!("Thickness")],
            Part::Window => &[msg!("Window Rough Opening: Added Width"), msg!("Window Rough Opening: Added Height")],
            Part::Door => &[msg!("Door Rough Opening: Added Width"), msg!("Door Rough Opening: Added Height")],
        }
    }
}

/// How the picture draws its wall: how long (mm; as tall as a storey, `TALL`), how many times thicker than they are its members
/// are (true to scale a 2x would be a hairline, so a little thicker, but each in proportion: thicknesses, plies, spacing,
/// depths and rough openings are all the type's, so a change to any shows), and where along it the window's and the door's
/// middles are.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scale {
    pub(crate) long: f64,
    pub(crate) thicker: f64,
    pub(crate) window_at: f64,
    pub(crate) door_at: f64,
}

/// What the dialog shows: a wall long enough for a run of studs each side of the window and the door.
pub(crate) const SCALE: Scale = Scale { long: 6000.0, thicker: 1.75, window_at: 2100.0, door_at: 4300.0 };
const TALL: f64 = 2700.0;
/// The window's and the door's sizes, their rough openings adding the type's to them; the window's sill height.
const WINDOW: (f64, f64, f64) = (1200.0, 1200.0, 900.0);
const DOOR: (f64, f64) = (900.0, 2100.0);
/// The most of one kind drawn, past which the type says more than a picture can.
const MOST: u32 = 6;

/// The picture's members as framing type `t` lays them out: each a kind and its rectangle `[x0, z0, x1, z1]`, mm along the wall
/// and up from its base. The rough openings first, so a member over one is what a click there takes.
#[cfg(test)]
pub(crate) fn layout(t: &FramingType) -> Vec<(Part, [f64; 4])> {
    layout_in(t, SCALE)
}

pub(crate) fn layout_in(t: &FramingType, sc: Scale) -> Vec<(Part, [f64; 4])> {
    let long = sc.long;
    let th = t.thickness.clamp(10.0, 120.0) * sc.thicker;
    let count = |n: u32| f64::from(n.min(MOST));
    let bottom = count(t.bottom_plates) * th;
    let top = TALL - count(t.top_plates) * th;
    let (kings, jacks) = (count(t.kings), count(t.jacks));
    // A header for a rough opening `wide`: the first whose Up To holds it, else the last; on edge it shows its depth, flat its
    // plies stacked.
    let header = |wide: f64| {
        let h = t.headers.iter().find(|h| h.up_to.is_none_or(|u| wide <= u)).or(t.headers.last());
        h.map_or((th, 1), |h| if h.flat { (th, h.plies.clamp(1, MOST)) } else { (h.depth.clamp(th, 600.0), 1) })
    };
    let mut out = Vec::new();
    // The rough openings: the window's over its sill, the door's from the floor; neither up into its header's room.
    let rough = |(w, h): (f64, f64), add: [f64; 2], z0: f64, mid: f64| {
        let w = (w + add[0]).clamp(300.0, 1600.0);
        let (depth, plies) = header(w);
        let z1 = (z0 + h + add[1]).min(top - depth * f64::from(plies) - th);
        [mid - w / 2.0, z0, mid + w / 2.0, z1.max(z0 + 300.0)]
    };
    let window = rough((WINDOW.0, WINDOW.1), t.window_rough, WINDOW.2, sc.window_at);
    let door = rough(DOOR, t.door_rough, 0.0, sc.door_at);
    out.push((Part::Window, window));
    out.push((Part::Door, door));
    // The plates: the bottom ones cut across the door's rough opening, under its jacks either side.
    for i in 0..t.bottom_plates.min(MOST) {
        let z = f64::from(i) * th;
        out.push((Part::BottomPlate, [0.0, z, door[0], z + th]));
        out.push((Part::BottomPlate, [door[2], z, long, z + th]));
    }
    for i in 0..t.top_plates.min(MOST) {
        let z = top + f64::from(i) * th;
        out.push((Part::TopPlate, [0.0, z, long, z + th]));
    }
    // Each opening's kings outside its jacks, its header on them, cripples over it on the grid, and the window's sill and the
    // cripples under it.
    let grid = grid(t, th, long);
    let mut zones = Vec::new();
    for (ro, sill) in [(window, true), (door, false)] {
        let (x0, x1) = (ro[0] - jacks * th, ro[2] + jacks * th);
        for i in 0..t.jacks.min(MOST) {
            let k = f64::from(i);
            out.push((Part::Jack, [ro[0] - (k + 1.0) * th, bottom, ro[0] - k * th, ro[3]]));
            out.push((Part::Jack, [ro[2] + k * th, bottom, ro[2] + (k + 1.0) * th, ro[3]]));
        }
        for i in 0..t.kings.min(MOST) {
            let k = f64::from(i);
            out.push((Part::King, [x0 - (k + 1.0) * th, bottom, x0 - k * th, top]));
            out.push((Part::King, [x1 + k * th, bottom, x1 + (k + 1.0) * th, top]));
        }
        let (depth, plies) = header(ro[2] - ro[0]);
        for i in 0..plies {
            let z = ro[3] + depth * f64::from(i);
            out.push((Part::Header, [x0, z, x1, z + depth]));
        }
        let over = ro[3] + depth * f64::from(plies);
        let sills = if sill { count(t.sill_plates) } else { 0.0 };
        for i in 0..t.sill_plates.min(MOST).min(if sill { MOST } else { 0 }) {
            let z = ro[1] - f64::from(i + 1) * th;
            out.push((Part::Sill, [ro[0], z, ro[2], z + th]));
        }
        for &x in grid.iter().filter(|x| **x - th / 2.0 > ro[0] && **x + th / 2.0 < ro[2]) {
            if top - over > th {
                out.push((Part::Cripple, [x - th / 2.0, over, x + th / 2.0, top]));
            }
            if sill && ro[1] - sills * th - bottom > th {
                out.push((Part::Cripple, [x - th / 2.0, bottom, x + th / 2.0, ro[1] - sills * th]));
            }
        }
        zones.push((x0 - kings * th, x1 + kings * th));
    }
    // The studs on the grid, but where an opening's framing stands, and one at each end.
    let clear = |x: f64| zones.iter().all(|(a, b)| x + th / 2.0 <= *a || x - th / 2.0 >= *b);
    let mut studs: Vec<f64> = grid.into_iter().filter(|x| clear(*x)).collect();
    studs.extend([th / 2.0, long - th / 2.0]);
    studs.sort_by(f64::total_cmp);
    studs.dedup_by(|a, b| (*a - *b).abs() < th);
    for &x in &studs {
        out.push((Part::Stud, [x - th / 2.0, bottom, x + th / 2.0, top]));
    }
    // A row of blocking at each height, flat between each two full-height members not an opening apart.
    let mut uprights: Vec<[f64; 2]> = studs.iter().map(|x| [x - th / 2.0, x + th / 2.0]).collect();
    uprights.extend(out.iter().filter(|(p, _)| *p == Part::King).map(|(_, r)| [r[0], r[2]]));
    uprights.sort_by(|a, b| a[0].total_cmp(&b[0]));
    for &z in t.blocking.iter().take(MOST as usize).filter(|z| **z > bottom + th && **z < top - th) {
        for pair in uprights.windows(2) {
            let (a, b) = (pair[0][1], pair[1][0]);
            if b - a > th && zones.iter().all(|(z0, z1)| b <= *z0 || a >= *z1) {
                out.push((Part::Blocking, [a, z - th / 2.0, b, z + th / 2.0]));
            }
        }
    }
    out
}

/// The stud grid's centres along a picture's wall `long` mm, as the type lays it out: from its start, its end or its middle,
/// `spacing` apart (no closer than two members, so a picture's grid never runs solid).
fn grid(t: &FramingType, th: f64, long: f64) -> Vec<f64> {
    let step = t.spacing.max(2.0 * th).max(100.0);
    let from = match t.layout {
        Layout::Start => 0.0,
        Layout::End => long,
        Layout::Centre => long / 2.0,
    };
    let back = (from / step).floor();
    (0..=((long / step).ceil() as i64 + 1)).map(|i| from - back * step + step * i as f64).filter(|x| *x > th && *x < long - th).collect()
}

/// A kind's name, short, as the picture labels its members.
fn short(p: Part) -> &'static str {
    match p {
        Part::Stud => msg!("Stud"),
        Part::TopPlate => msg!("Top Plate"),
        Part::BottomPlate => msg!("Bottom Plate"),
        Part::King => msg!("King"),
        Part::Jack => msg!("Jack"),
        Part::Header => msg!("Header"),
        Part::Cripple => msg!("Cripple"),
        Part::Sill => msg!("Sill"),
        Part::Blocking => msg!("Blocking"),
        Part::Window | Part::Door => "",
    }
}

/// Where each kind's label goes, in the margin over the wall (`true`) or under it, and the point of its member its leader
/// starts from, `parts` in the picture's pixels: each kind's member picked where the labels spread along the wall (the bottom
/// plate at its start, the window's king left of it and its jack right of it, a stud towards the wall's end, the door's
/// header), so few leaders cross. A kind none of is laid out has no label.
fn labels(parts: &[(Part, Rect)]) -> Vec<(Part, bool, Pos2)> {
    let all = |kind: Part| parts.iter().filter(move |(p, _)| *p == kind).map(|(_, r)| *r);
    let wall = parts.iter().fold(Rect::NOTHING, |b, (_, r)| b.union(*r));
    // The one of a kind nearest a fraction of the way along the wall.
    let near = |kind: Part, along: f32| {
        let x = wall.left() + wall.width() * along;
        all(kind).min_by(|a, b| (a.center().x - x).abs().total_cmp(&(b.center().x - x).abs()))
    };
    let window = parts.iter().find(|(p, _)| *p == Part::Window).map(|(_, r)| *r);
    let door = parts.iter().find(|(p, _)| *p == Part::Door).map(|(_, r)| *r);
    let beside = |kind: Part, of: Option<Rect>, left: bool| {
        let of = of?;
        let side = |r: &Rect| if left { r.center().x < of.left() } else { r.center().x > of.right() };
        all(kind).filter(side).min_by(|a, b| (a.center().x - of.center().x).abs().total_cmp(&(b.center().x - of.center().x).abs()))
    };
    let over = |kind: Part, of: Option<Rect>| {
        let of = of?;
        all(kind).find(|r| r.center().x > of.left() && r.center().x < of.right())
    };
    let up = |r: Rect, k: f32| pos2(r.center().x, r.bottom() - r.height() * k);
    let mut out = Vec::new();
    let mut put = |kind: Part, top: bool, at: Option<Pos2>| {
        if let Some(at) = at {
            out.push((kind, top, at));
        }
    };
    put(Part::TopPlate, true, all(Part::TopPlate).next().map(|r| pos2(r.left() + r.width() * 0.06, r.top())));
    put(Part::Cripple, true, over(Part::Cripple, window).filter(|r| window.is_some_and(|w| r.bottom() <= w.top())).map(|r| r.center()));
    put(Part::Header, true, over(Part::Header, door).or_else(|| all(Part::Header).next()).map(|r| r.center()));
    put(Part::Blocking, true, near(Part::Blocking, 0.9).map(|r| r.center()));
    put(Part::BottomPlate, false, all(Part::BottomPlate).next().map(|r| pos2(r.left() + r.width() * 0.06, r.bottom())));
    put(Part::King, false, beside(Part::King, window, true).map(|r| up(r, 0.25)));
    put(Part::Sill, false, all(Part::Sill).next().map(|r| r.center()));
    put(Part::Jack, false, beside(Part::Jack, window, false).map(|r| up(r, 0.25)));
    put(Part::Stud, false, near(Part::Stud, 0.92).map(|r| up(r, 0.25)));
    out
}

/// The picture in `ui`, `width` wide, of `framing`, a framing type as typed so far; a click picks a member, its label or the
/// window or door's rough opening, another on nothing lets it go. Whether `picked` changed.
pub(crate) fn show(ui: &mut Ui, width: f32, framing: &FramingType, picked: &mut Option<Part>) -> bool {
    show_in(ui, width, framing, picked, SCALE)
}

pub(crate) fn show_in(ui: &mut Ui, width: f32, framing: &FramingType, picked: &mut Option<Part>, sc: Scale) -> bool {
    let (pad, row) = (4.0, 13.0);
    let k = (width - 2.0 * pad) / sc.long as f32;
    // A row of labels over the wall, two under it.
    let (over, under_rows) = (row + 6.0, 2.0 * row + 6.0);
    let height = TALL as f32 * k + 2.0 * pad + over + under_rows;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let wall =
        Rect::from_min_max(pos2(rect.left() + pad, rect.top() + pad + over), pos2(rect.right() - pad, rect.bottom() - pad - under_rows));
    let at = |p: [f64; 4]| {
        let x = |v: f64| wall.left() + v as f32 * k;
        let y = |v: f64| wall.bottom() - v as f32 * k;
        Rect::from_min_max(pos2(x(p[0]), y(p[3])), pos2(x(p[2]), y(p[1])))
    };
    let parts: Vec<(Part, Rect)> = layout_in(framing, sc).into_iter().map(|(p, r)| (p, at(r))).collect();
    let painter = ui.painter_at(rect);
    let font = egui::FontId::proportional(10.0);
    let ink = pal().text;
    // The labels laid out first, so a click or a hover on one is its kind's: left to right in their rows, none over another.
    let mut placed: Vec<(Part, Rect, Pos2)> = Vec::new();
    let mut ends = [wall.left(); 3];
    let mut wanted = labels(&parts);
    wanted.sort_by(|a, b| a.2.x.total_cmp(&b.2.x));
    for (kind, top, anchor) in wanted {
        let size = painter.layout_no_wrap(t(short(kind)).to_string(), font.clone(), ink).size();
        let rows: &[usize] = if top { &[0] } else { &[1, 2] };
        let x = (anchor.x - size.x / 2.0).clamp(rect.left() + 1.0, rect.right() - size.x - 1.0);
        // The first row it fits in where it wants to be, else the one it is least pushed along in.
        let Some(&r) = rows.iter().find(|r| ends[**r] + 6.0 <= x).or_else(|| rows.iter().min_by(|a, b| ends[**a].total_cmp(&ends[**b])))
        else {
            continue;
        };
        let x = x.max(ends[r] + 6.0).min(rect.right() - size.x - 1.0);
        let y = match r {
            0 => rect.top() + pad,
            1 => wall.bottom() + 5.0,
            _ => wall.bottom() + 5.0 + row,
        };
        let label = Rect::from_min_size(pos2(x, y), size);
        ends[r] = label.right();
        placed.push((kind, label, anchor));
    }
    let under = |q: Pos2| {
        placed
            .iter()
            .find(|(_, r, _)| r.expand(2.0).contains(q))
            .map(|(p, ..)| *p)
            .or_else(|| parts.iter().rev().find(|(_, r)| r.expand(1.0).contains(q)).map(|(p, _)| *p))
    };
    let hovered = response.hover_pos().and_then(under);
    painter.rect_filled(rect, 4.0, pal().surface);
    let wood = Color32::from_rgb(214, 160, 96);
    let edge = Color32::from_rgb(110, 70, 34);
    for (part, r) in &parts {
        let lit = *picked == Some(*part);
        let over = hovered == Some(*part);
        match part {
            Part::Window | Part::Door => {
                let fill = if lit {
                    pal().accent.gamma_multiply(0.25)
                } else if over {
                    pal().hover.gamma_multiply(0.15)
                } else {
                    pal().panel
                };
                painter.rect_filled(*r, 0.0, fill);
                if lit || over {
                    painter.rect_stroke(*r, 0.0, Stroke::new(1.5, pal().accent), StrokeKind::Inside);
                }
            }
            _ => {
                let fill = if lit {
                    pal().accent
                } else if over {
                    wood.gamma_multiply(0.7)
                } else {
                    wood
                };
                painter.rect_filled(*r, 0.0, fill);
                painter.rect_stroke(*r, 0.0, Stroke::new(0.75, edge), StrokeKind::Inside);
            }
        }
    }
    // Each label, small, outside the members, and a thin leader from it to the member it names.
    for (kind, label, anchor) in &placed {
        let lit = *picked == Some(*kind) || hovered == Some(*kind);
        let colour = if lit { pal().accent } else { ink };
        let foot = if label.bottom() <= wall.top() {
            pos2(label.center().x, label.bottom() + 1.0)
        } else {
            pos2(label.center().x, label.top() - 1.0)
        };
        painter.line_segment([foot, *anchor], Stroke::new(0.75, colour.gamma_multiply(0.7)));
        painter.circle_filled(*anchor, 1.5, colour);
        painter.text(label.left_top(), egui::Align2::LEFT_TOP, t(short(*kind)), font.clone(), colour);
    }
    let response = match hovered {
        Some(p) => response.on_hover_text(t(p.name())),
        None => response,
    };
    let was = *picked;
    if response.clicked() {
        *picked = response.interact_pointer_pos().and_then(under);
    }
    let shown = picked.map_or_else(|| t("Click a member to see what sets it").to_string(), |p| t(p.name()).to_string());
    ui.label(egui::RichText::new(shown).small().color(pal().muted));
    was != *picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::framing::Header;

    fn count(t: &FramingType, kind: Part) -> usize {
        layout(t).iter().filter(|(p, _)| *p == kind).count()
    }

    #[test]
    fn the_picture_follows_the_types_spacing_plates_kings_jacks_headers_and_thickness() {
        let us = FramingType::american(None);
        let studs = count(&us, Part::Stud);
        let wide = FramingType { spacing: 609.6, ..us.clone() };
        assert!(count(&wide, Part::Stud) < studs, "24 in apart: fewer studs than at 16");
        assert_eq!(count(&FramingType { top_plates: 1, ..us.clone() }, Part::TopPlate), 1);
        assert_eq!(count(&FramingType { top_plates: 3, ..us.clone() }, Part::TopPlate), 3);
        assert_eq!(count(&FramingType { kings: 2, jacks: 2, ..us.clone() }, Part::King), 8, "two each side of two openings");
        assert_eq!(count(&FramingType { jacks: 2, ..us.clone() }, Part::Jack), 8);
        assert_eq!(count(&FramingType { sill_plates: 2, ..us.clone() }, Part::Sill), 2);
        let width = |t: &FramingType| layout(t).iter().find(|(p, _)| *p == Part::Stud).map_or(0.0, |(_, r)| r[2] - r[0]);
        let kvh = FramingType::european(None);
        assert!(width(&kvh) > width(&us), "60 mm studs drawn thicker than 38s");
        let header = |t: &FramingType| layout(t).iter().filter(|(p, _)| *p == Part::Header).map(|(_, r)| r[3] - r[1]).fold(0.0, f64::max);
        let deep = FramingType { headers: vec![Header { up_to: None, depth: 300.0, plies: 2, flat: false }], ..us.clone() };
        let shallow = FramingType { headers: vec![Header { up_to: None, depth: 140.0, plies: 2, flat: false }], ..us.clone() };
        assert!(header(&deep) > header(&shallow), "a deeper header drawn deeper");
        let rows = FramingType { blocking: vec![1200.0, 1800.0], ..us.clone() };
        assert!(count(&rows, Part::Blocking) > count(&FramingType { blocking: vec![1200.0], ..us }, Part::Blocking));
    }

    /// With BIMCRAFT_FRAMING_DUMP a folder, the picture of each starter type, to look at; another `Scale` tried here.
    #[test]
    fn framing_picture_options() {
        let Ok(folder) = std::env::var("BIMCRAFT_FRAMING_DUMP") else { return };
        std::fs::create_dir_all(&folder).unwrap();
        let options = [("shown", SCALE)];
        for (name, sc) in options {
            for (kind, t) in
                [("us", FramingType::american(None)), ("kvh", FramingType::european(None)), ("exterior", FramingType::exterior(None))]
            {
                let mut h = egui_kittest::Harness::builder().with_size([360.0, 300.0]).build_ui(move |ui| {
                    let mut picked = None;
                    show_in(ui, 340.0, &t, &mut picked, sc);
                });
                h.run();
                h.render().unwrap().save(std::path::Path::new(&folder).join(format!("picture_{name}_{kind}.png"))).unwrap();
            }
        }
    }

    #[test]
    fn every_member_lies_within_the_wall_and_none_has_no_size() {
        for t in [FramingType::american(None), FramingType::european(None), FramingType::exterior(None)] {
            for (p, r) in layout(&t) {
                assert!(r[2] > r[0] && r[3] > r[1], "{p:?} {r:?}");
                assert!(r[0] >= -1e-6 && r[2] <= SCALE.long + 1e-6 && r[1] >= -1e-6 && r[3] <= TALL + 1e-6, "{p:?} {r:?}");
            }
        }
    }
}
