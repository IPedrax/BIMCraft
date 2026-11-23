//! Scale (RE): annotation, CAD drawings, images and PDFs made `k` times their size about a point that stays, as Revit's
//! Scale makes them. What's drawn by a type or a family keeps its size: text, tags, spot dimensions, groups and every
//! model element. The `scale_elements` command (`commands.rs`) takes what this scales and lists what it leaves out, and why.

use crate::commands::micron;
use crate::geom::P2;
use crate::model::Element;

/// How many times bigger or smaller a scale may make what it takes: a thousandth to a thousand.
pub const FACTORS: std::ops::RangeInclusive<f64> = 1e-3..=1e3;

/// Element `e` `k` times its size about `about`, in the terms it's drawn in (a plan's x and y; a section's, an elevation's
/// or a sheet's own); or why it keeps its size, in a few words an agent and a person read.
pub fn scaled(e: &Element, about: P2, k: f64) -> Result<Element, &'static str> {
    let f = |p: P2| micron(about + (p - about) * k);
    let mut e = e.clone();
    match &mut e {
        Element::DetailLine(l) => {
            l.curve = l.curve.scaled(about, k).map(micron);
            // Its insulation's width and its cloud's scallops are lengths in the view, as its points are.
            l.insulation = l.insulation.map(|w| w * k);
            l.cloud = l.cloud.map(|s| s * k);
        }
        Element::FilledRegion(r) => {
            r.sketch.iter_mut().for_each(|line| line.curve = line.curve.scaled(about, k).map(micron));
            // Its pattern is its type's, the same size; it starts where its origin goes.
            r.pattern_origin = f(r.pattern_origin);
        }
        // Its path; the detail repeated along it is a group's, the same size and as far apart.
        Element::RepeatingDetail(r) => (r.start, r.end) = (f(r.start), f(r.end)),
        // Its line goes where its point goes; what it measures says what it reads.
        Element::Dimension(d) => d.at = f(d.at),
        // Each of its units `k` times as many mm, its origin where `at` goes.
        Element::CadImport(ci) => {
            ci.at = f(ci.at);
            ci.scale *= k;
        }
        // As wide `k` times, its middle where `at` goes; its height follows its paper's or its pixels' shape.
        Element::Image(im) => {
            im.at = f(im.at);
            im.width *= k;
        }
        Element::Text(_) => return Err("text keeps its size: its text type says how big it's lettered"),
        Element::Tag(_) => return Err("a tag keeps its size: its type draws it, where what it tags is"),
        Element::Spot(_) => return Err("a spot dimension keeps its size: its type draws its symbol"),
        Element::Dimension3D(_) => return Err("a 3D view's dimension isn't scaled: Scale works in plans, sections and elevations"),
        Element::DetailGroup(_) | Element::ModelGroup(_) => {
            return Err("a group keeps its type's size: scale what's in it in Edit Group");
        }
        Element::Component(_) => return Err("a family keeps its size: its type says how big it is"),
        Element::Link(_) => return Err("a linked model is the size its file says"),
        Element::PointCloud(_) => return Err("a point cloud is the size it was measured"),
        Element::Section(_) | Element::Elevation(_) | Element::Viewport(_) => {
            return Err("a view's mark keeps its size: it's drawn as big on every sheet");
        }
        _ => return Err("a model element keeps its size: Scale takes annotation, CAD drawings, images and PDFs"),
    }
    Ok(e)
}

/// Whether Scale takes `e`, and so whether a selection holding it is one to scale.
pub fn scalable(e: &Element) -> bool {
    matches!(
        e,
        Element::DetailLine(_)
            | Element::FilledRegion(_)
            | Element::RepeatingDetail(_)
            | Element::Dimension(_)
            | Element::CadImport(_)
            | Element::Image(_)
    )
}
