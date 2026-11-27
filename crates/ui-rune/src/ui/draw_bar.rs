//! The Draw panel's shapes for the Wall tool, as a row of buttons over the plan's top left: Line, a rectangle, polygons, a circle
//! and arcs, each making walls along what its clicks draw (`tooling.rs`, `draw::make`). It is there only while the tool in hand takes
//! a shape, so the plan has no element for it otherwise.

use bimcraft_core::draw::Shape;
use rune::tok::{self, Theme, type_};
use rune::{El, el, row, text};

use crate::app::App;
use crate::lang::t;
use crate::tooling;

/// The shapes a wall is drawn with by clicks, and the two that pick what is there (Pick Lines, Pick Faces). Fillet Arc needs two lines and a
/// radius, and Tangent End Arc the wall before it: neither is here yet.
pub const SHAPES: [Shape; 9] = [
    Shape::Line,
    Shape::Rectangle,
    Shape::Inscribed,
    Shape::Circumscribed,
    Shape::Circle,
    Shape::Arc,
    Shape::CentreArc,
    Shape::PickLines,
    Shape::PickFaces,
];

/// Whether the tool in hand takes one of [`SHAPES`].
pub fn takes_shape(s: &App) -> bool {
    s.tool.and_then(tooling::spec_of).is_some_and(tooling::Spec::takes_shape)
}

fn button(shape: Shape, active: bool, th: &Theme) -> El {
    let label = t(shape.label());
    let name = format!("{shape:?}").to_lowercase();
    el().key(format!("shape-{name}"))
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .px(8.0)
        .h(24.0)
        .center()
        .radius(6.0)
        .fill(if active { th.control_off } else { th.control_track })
        .child(text(label.to_string()).font(type_::LABEL).color(th.text))
        .on_press(move |s: &mut App| s.set_shape(shape))
}

/// The row of shapes, with the one in hand lit; none while the tool takes no shape.
pub fn bar(s: &App) -> Option<El> {
    if !takes_shape(s) {
        return None;
    }
    let th = tok::theme(s.dark());
    let now = s.tooling.shape;
    let buttons: Vec<El> = SHAPES.iter().map(|&shape| button(shape, shape == now, &th)).collect();
    Some(row().key("draw-bar").gap(4.0).padding(4.0).radius(8.0).fill(th.card).pin(Some(8.0), Some(8.0), None, None).children(buttons))
}
