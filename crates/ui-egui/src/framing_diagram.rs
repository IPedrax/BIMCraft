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

/// The wall the picture shows, mm: as long and as tall as a storey's, a window and a door in it.
const LONG: f64 = 4800.0;
const TALL: f64 = 2700.0;
/// How many times thicker than they are its members are drawn: true to scale, a 2x would be a hairline. Thicknesses, plies,
/// spacing, depths and rough openings are all the type's, so a change to any shows.
const THICKER: f64 = 4.0;
/// The window's and the door's sizes (their rough openings add the type's to them), and where their middles are along the wall.
const WINDOW: (f64, f64, f64, f64) = (1200.0, 1200.0, 900.0, 1500.0);
const DOOR: (f64, f64, f64) = (900.0, 2100.0, 3650.0);
/// The most of one kind drawn, past which the type says more than a picture can.
const MOST: u32 = 6;

/// The picture's members as framing type `t` lays them out: each a kind and its rectangle `[x0, z0, x1, z1]`, mm along the wall
/// and up from its base. The rough openings first, so a member over one is what a click there takes.
pub(crate) fn layout(t: &FramingType) -> Vec<(Part, [f64; 4])> {
    let th = t.thickness.clamp(10.0, 120.0) * THICKER;
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
    let window = rough((WINDOW.0, WINDOW.1), t.window_rough, WINDOW.2, WINDOW.3);
    let door = rough((DOOR.0, DOOR.1), t.door_rough, 0.0, DOOR.2);
    out.push((Part::Window, window));
    out.push((Part::Door, door));
    // The plates: the bottom ones cut across the door.
    for i in 0..t.bottom_plates.min(MOST) {
        let z = f64::from(i) * th;
        out.push((Part::BottomPlate, [0.0, z, door[0] - jacks * th, z + th]));
        out.push((Part::BottomPlate, [door[2] + jacks * th, z, LONG, z + th]));
    }
    for i in 0..t.top_plates.min(MOST) {
        let z = top + f64::from(i) * th;
        out.push((Part::TopPlate, [0.0, z, LONG, z + th]));
    }
    // Each opening's kings outside its jacks, its header on them, cripples over it on the grid, and the window's sill and the
    // cripples under it.
    let grid = grid(t, th);
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
    studs.extend([th / 2.0, LONG - th / 2.0]);
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

/// The stud grid's centres along the picture's wall, as the type lays it out: from its start, its end or its middle, `spacing`
/// apart (no closer than two members, so a picture's grid never runs solid).
fn grid(t: &FramingType, th: f64) -> Vec<f64> {
    let step = t.spacing.max(2.0 * th).max(100.0);
    let from = match t.layout {
        Layout::Start => 0.0,
        Layout::End => LONG,
        Layout::Centre => LONG / 2.0,
    };
    let back = (from / step).floor();
    (0..=((LONG / step).ceil() as i64 + 1)).map(|i| from - back * step + step * i as f64).filter(|x| *x > th && *x < LONG - th).collect()
}

/// The picture in `ui`, `width` wide, of `framing`, a framing type as typed so far; a click picks a member (or the window or door's rough
/// opening), another on nothing lets it go. Whether `picked` changed.
pub(crate) fn show(ui: &mut Ui, width: f32, framing: &FramingType, picked: &mut Option<Part>) -> bool {
    let pad = 4.0;
    let k = (width - 2.0 * pad) / LONG as f32;
    let height = TALL as f32 * k + 2.0 * pad;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let at = |p: [f64; 4]| {
        let x = |v: f64| rect.left() + pad + v as f32 * k;
        let y = |v: f64| rect.bottom() - pad - v as f32 * k;
        Rect::from_min_max(pos2(x(p[0]), y(p[3])), pos2(x(p[2]), y(p[1])))
    };
    let parts = layout(framing);
    let under = |q: Pos2| parts.iter().rev().find(|(_, r)| at(*r).expand(1.0).contains(q)).map(|(p, _)| *p);
    let hovered = response.hover_pos().and_then(under);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, pal().surface);
    let wood = Color32::from_rgb(196, 122, 58);
    let ink = Color32::from_rgb(70, 40, 18);
    for (part, r) in &parts {
        let r = at(*r);
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
                painter.rect_filled(r, 0.0, fill);
                if lit || over {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.5, pal().accent), StrokeKind::Inside);
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
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, ink), StrokeKind::Inside);
            }
        }
    }
    // Each kind's name on the first of it with room for it, along it: up an upright one.
    let font = egui::FontId::proportional(8.0);
    for kind in
        [Part::Header, Part::TopPlate, Part::BottomPlate, Part::Sill, Part::Blocking, Part::Stud, Part::King, Part::Jack, Part::Cripple]
    {
        let galley = painter.layout_no_wrap(t_short(kind).to_string(), font.clone(), Color32::WHITE);
        let size = galley.size();
        let fits = parts.iter().filter(|(p, _)| *p == kind).map(|(_, r)| at(*r)).find_map(|r| {
            if r.width() >= size.x + 2.0 && r.height() >= size.y {
                Some((r, false))
            } else if r.height() >= size.x + 2.0 && r.width() >= size.y {
                Some((r, true))
            } else {
                None
            }
        });
        match fits {
            Some((r, false)) => {
                painter.galley(r.center() - size / 2.0, galley, Color32::WHITE);
            }
            Some((r, true)) => {
                let at = pos2(r.center().x - size.y / 2.0, r.center().y + size.x / 2.0);
                painter.add(egui::epaint::TextShape::new(at, galley, Color32::WHITE).with_angle(-std::f32::consts::FRAC_PI_2));
            }
            None => {}
        }
    }
    let response = match hovered {
        Some(p) => response.on_hover_text(t_name(p)),
        None => response,
    };
    let was = *picked;
    if response.clicked() {
        *picked = response.interact_pointer_pos().and_then(under);
    }
    let shown = picked.map_or_else(|| t("Click a member to see what sets it").to_string(), |p| t_name(p).to_string());
    ui.label(egui::RichText::new(shown).small().color(pal().muted));
    was != *picked
}

/// A kind's name as the picture's hover and caption give it.
fn t_name(p: Part) -> &'static str {
    t(p.name())
}

/// A kind's name, short, as its members carry it.
fn t_short(p: Part) -> &'static str {
    t(match p {
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
    })
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

    #[test]
    fn every_member_lies_within_the_wall_and_none_has_no_size() {
        for t in [FramingType::american(None), FramingType::european(None), FramingType::exterior(None)] {
            for (p, r) in layout(&t) {
                assert!(r[2] > r[0] && r[3] > r[1], "{p:?} {r:?}");
                assert!(r[0] >= -1e-6 && r[2] <= LONG + 1e-6 && r[1] >= -1e-6 && r[3] <= TALL + 1e-6, "{p:?} {r:?}");
            }
        }
    }
}
