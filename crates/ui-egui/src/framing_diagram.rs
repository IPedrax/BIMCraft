//! A framing type's picture in its Type Properties: a framed wall in elevation, a window and a door in it, each kind of member
//! drawn where it stands (studs, top and bottom plates, kings, jacks, headers, cripples, a sill, blocking). A click on a member
//! picks it, and the rows of the type's settings that member is built by light up in the list beside it.

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

/// The picture's members: each a kind and its rectangle, in a frame 100 wide and 64 tall, y down. The openings first, so a
/// member over one is what a click there takes.
fn layout() -> Vec<(Part, [f32; 4])> {
    let mut out = vec![
        (Part::Window, [34.0, 27.0, 54.0, 46.0]),
        (Part::Door, [72.0, 23.0, 88.0, 58.0]),
        (Part::BottomPlate, [2.0, 58.0, 98.0, 61.0]),
        (Part::TopPlate, [2.0, 3.0, 98.0, 6.0]),
        (Part::TopPlate, [2.0, 6.0, 98.0, 9.0]),
    ];
    // Studs on the grid, and at each end.
    for x in [2.0, 15.0, 62.0, 96.0] {
        out.push((Part::Stud, [x, 9.0, x + 2.0, 58.0]));
    }
    // A row of blocking between the first studs.
    out.push((Part::Blocking, [4.0, 33.0, 15.0, 35.0]));
    out.push((Part::Blocking, [17.0, 33.0, 30.0, 35.0]));
    // The window: kings outside jacks, a header on the jacks, a sill, cripples over the header and under the sill.
    for (king, jack) in [(30.0, 32.0), (56.0, 54.0)] {
        out.push((Part::King, [king, 9.0, king + 2.0, 58.0]));
        out.push((Part::Jack, [jack, 27.0, jack + 2.0, 58.0]));
    }
    out.push((Part::Header, [32.0, 21.0, 56.0, 27.0]));
    out.push((Part::Sill, [34.0, 46.0, 54.0, 48.0]));
    for x in [39.0, 48.0] {
        out.push((Part::Cripple, [x, 9.0, x + 2.0, 21.0]));
        out.push((Part::Cripple, [x, 48.0, x + 2.0, 58.0]));
    }
    // The door: the same, its bottom plate cut across it, no sill.
    for (king, jack) in [(68.0, 70.0), (90.0, 88.0)] {
        out.push((Part::King, [king, 9.0, king + 2.0, 58.0]));
        out.push((Part::Jack, [jack, 23.0, jack + 2.0, 58.0]));
    }
    out.push((Part::Header, [70.0, 17.0, 90.0, 23.0]));
    out.push((Part::Cripple, [79.0, 9.0, 81.0, 17.0]));
    out
}

/// The picture in `ui`, `width` wide; a click picks a member (or the window or door's rough opening), another on nothing lets
/// it go. Whether `picked` changed.
pub(crate) fn show(ui: &mut Ui, width: f32, picked: &mut Option<Part>) -> bool {
    let height = width * 0.64;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let at = |p: [f32; 4]| {
        let k = rect.width() / 100.0;
        Rect::from_min_max(pos2(rect.left() + p[0] * k, rect.top() + p[1] * k), pos2(rect.left() + p[2] * k, rect.top() + p[3] * k))
    };
    let parts = layout();
    let under = |q: Pos2| parts.iter().rev().find(|(_, r)| at(*r).contains(q)).map(|(p, _)| *p);
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
    // The members' names where there's room: the headers, and the top and bottom plates.
    let label = |text: &str, r: [f32; 4]| {
        let r = at(r);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(10.0), Color32::WHITE);
    };
    label(t("Header"), [32.0, 21.0, 56.0, 27.0]);
    label(t("Header"), [70.0, 17.0, 90.0, 23.0]);
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
