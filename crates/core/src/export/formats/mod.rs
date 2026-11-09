//! The built-in formats, a file each for each kind of writer, and what they share: a drawing in the page setup's colours and
//! lines. A format not written yet is [`Planned`], listed so the dialog can show it and an export to it says so.

mod coordination;
mod document;
mod line;
mod mesh;
mod model;
mod picture;
mod table;

use std::sync::Arc;

use super::page::{Colour, PageSetup};
use super::{Family, Format, Options, Out, Prepared, Status, Takes};
use crate::render::{Drawing, Prim, Rgb};

pub use coordination::Bcf;
pub use document::Pdf;
pub use line::Dxf;
pub use mesh::{Obj, Stl};
pub use model::{Glb, Ifc};
pub use picture::{Jpeg, Jxl, Png, Svg};
pub use table::{Csv, Ods, Xlsx};

/// Every format the core writes or plans to, in the order the dialog lists them.
pub fn builtin() -> Vec<Arc<dyn Format>> {
    vec![
        Arc::new(Pdf),
        Arc::new(Svg),
        Arc::new(Png),
        Arc::new(Jpeg),
        Arc::new(Jxl),
        Arc::new(Dxf),
        // Written by an app that has the DWG bridge; the core can't.
        Arc::new(Planned { id: "dwg", label: "DWG", extension: "dwg", family: Family::Line, takes: &[Takes::Drawing] }),
        Arc::new(Planned { id: "ifc2x3", label: "IFC2x3", extension: "ifc", family: Family::Model, takes: &[Takes::Model] }),
        Arc::new(Ifc(crate::ifc::Schema::Ifc4)),
        Arc::new(Ifc(crate::ifc::Schema::Ifc4x3)),
        Arc::new(Glb),
        Arc::new(Obj),
        Arc::new(Stl),
        Arc::new(Planned { id: "u3d", label: "U3D", extension: "u3d", family: Family::Model, takes: &[Takes::Model] }),
        Arc::new(Csv),
        Arc::new(Xlsx),
        Arc::new(Ods),
        Arc::new(Bcf),
    ]
}

/// A format listed but not written yet.
pub struct Planned {
    pub id: &'static str,
    pub label: &'static str,
    pub extension: &'static str,
    pub family: Family,
    pub takes: &'static [Takes],
}

impl Format for Planned {
    fn id(&self) -> &'static str {
        self.id
    }
    fn label(&self) -> &'static str {
        self.label
    }
    fn extension(&self) -> &'static str {
        self.extension
    }
    fn family(&self) -> Family {
        self.family
    }
    fn status(&self) -> Status {
        Status::Planned
    }
    fn takes(&self) -> &'static [Takes] {
        self.takes
    }
    fn write(&self, _: &[Prepared], _: &Options, _: &super::Ctx) -> Result<Vec<Out>, String> {
        Err(format!("{} export isn't written yet", self.label))
    }
}

/// How bright a colour looks, 0 to 255.
fn luma(c: Rgb) -> u8 {
    (0.299 * f64::from(c.0) + 0.587 * f64::from(c.1) + 0.114 * f64::from(c.2)).round().clamp(0.0, 255.0) as u8
}

/// `d` in the colours and lines `setup` asks for; none when it asks for the drawing as it is.
pub fn styled(d: &Drawing, setup: &PageSetup) -> Option<Drawing> {
    if setup.colour == Colour::Colour && !setup.hairlines {
        return None;
    }
    let (colour, hairlines) = (setup.colour, setup.hairlines);
    let ink = move |c: Rgb, fill: bool| match colour {
        Colour::Colour => c,
        Colour::Grey => {
            let l = luma(c);
            Rgb(l, l, l)
        }
        // A fill stays white unless it is dark; a line is black unless it is white.
        Colour::Black if fill => {
            if luma(c) < 128 {
                Rgb(0, 0, 0)
            } else {
                Rgb(255, 255, 255)
            }
        }
        Colour::Black => {
            if luma(c) > 250 {
                c
            } else {
                Rgb(0, 0, 0)
            }
        }
    };
    Some(d.restyled(|p| {
        match p {
            Prim::Fill { rings, color } => Prim::Fill { rings: rings.clone(), color: ink(*color, true) },
            Prim::Line { pts, width, color, dash, closed } => Prim::Line {
                pts: pts.clone(),
                width: if hairlines { 0.0 } else { *width },
                color: ink(*color, false),
                dash: dash.clone(),
                closed: *closed,
            },
            Prim::Image { at, width, height, rgba, ink: is_ink } => Prim::Image {
                at: *at,
                width: *width,
                height: *height,
                rgba: match colour {
                    Colour::Colour => rgba.clone(),
                    Colour::Grey | Colour::Black => rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|px| {
                            let l = luma(Rgb(px[0], px[1], px[2]));
                            [l, l, l, px[3]]
                        })
                        .collect(),
                },
                ink: *is_ink,
            },
        }
    }))
}
