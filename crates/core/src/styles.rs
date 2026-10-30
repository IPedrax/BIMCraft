//! Object Styles, as Revit's Manage > Object Styles: the pen each model category draws its lines in, where a view cuts it
//! and where it sees it beyond the cut, in every view of the project. A view's Visibility/Graphics overrides them.
//!
//! A drawing marks the lines that are its category's with [`BY_CATEGORY`], and [`Model::object_style`] gives the pen once
//! the drawing knows who owns each line; the rest of an element's lines (a wall's layers, a door's jambs, a stair's walk
//! line) keep their own.

use serde::{Deserialize, Serialize};

use crate::graphics::MODEL;
use crate::model::Model;
use crate::pen::{MAX_PEN, Pen};

/// The pen a drawing draws a line in that is its category's, cut or in projection as the line's part is: not a pen a file
/// or a command can give (past [`MAX_PEN`]), only a mark for [`Model::object_style`] to replace.
pub(crate) const BY_CATEGORY: Pen = Pen(u8::MAX);

/// A category's pens: its lines in projection, and where a view cuts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectStyle {
    pub projection: Pen,
    pub cut: Pen,
}

/// What a project's categories start with, ours: what's cut, [`Pen::CUT`], and what's seen, [`Pen::PROJECTION`]; a door's
/// and a window's cut, which is their leaf and frame, [`Pen::MEDIUM`], as is a railing seen; a mass, outlined where it's cut
/// as where it's seen, [`Pen::PROJECTION`] both.
pub fn default_of(category: &str) -> ObjectStyle {
    match category {
        "mass" => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::PROJECTION },
        "door" | "window" => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::MEDIUM },
        "railing" => ObjectStyle { projection: Pen::MEDIUM, cut: Pen::CUT },
        _ => ObjectStyle { projection: Pen::PROJECTION, cut: Pen::CUT },
    }
}

impl ObjectStyle {
    /// An error saying what to fix, unless both are pens.
    pub fn check(&self, category: &str) -> Result<(), String> {
        for (what, p) in [("projection", self.projection), ("cut", self.cut)] {
            if !p.is_valid() {
                return Err(format!("object style `{category}` {what}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", p.0));
            }
        }
        Ok(())
    }
}

/// An error saying what to fix, unless `category` is one Object Styles has: a model category views draw.
pub fn known(category: &str) -> Result<(), String> {
    if MODEL.contains(&category) || crate::graphics::ALIAS.contains(&category) {
        Ok(())
    } else {
        Err(format!("`{category}` isn't a model category; Object Styles has: {}", MODEL.join(", ")))
    }
}

impl Model {
    /// The pens `category` draws in: the project's own, or what it starts with. A family's category with none of its own has
    /// the `component` ones a file from before the families' rows may hold.
    pub fn object_style(&self, category: &str) -> ObjectStyle {
        let old = crate::graphics::FAMILY.contains(&category).then(|| self.object_styles.get("component").copied()).flatten();
        self.object_styles.get(category).copied().or(old).unwrap_or_else(|| default_of(category))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::render::{Drawing, Part, Prim, View};
    use serde_json::json;

    /// The widest solid line `id` draws of its `part` in `d`.
    fn widest(d: &Drawing, id: crate::model::Id, part: Part) -> f32 {
        (d.prims.iter().zip(&d.owners).zip(&d.parts))
            .filter_map(|((p, o), q)| match p {
                Prim::Line { width, dash: None, .. } if *o == Some(id) && *q == part => Some(*width),
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn object_styles_are_the_projects_pens_by_category_saved_and_undone() {
        let mut s = Session::new();
        // Asking says each model category's, and changes nothing.
        let r = s.execute("object_styles", &json!({})).unwrap();
        assert_eq!(r["categories"].as_array().map(Vec::len), Some(MODEL.len()));
        assert_eq!(r["categories"][0], json!({"category": "wall", "projection": 4, "cut": 10, "own": false}));
        assert!(s.history().is_empty());
        // Given, by category, a pen or both, in one undo step.
        s.execute("object_styles", &json!({"categories": {"wall": {"cut": 12}, "door": {"projection": 2, "cut": 5}}})).unwrap();
        assert_eq!(s.history(), ["Object Styles"]);
        assert_eq!(s.model.object_style("wall"), ObjectStyle { projection: Pen(4), cut: Pen(12) });
        assert_eq!(s.model.object_style("door"), ObjectStyle { projection: Pen(2), cut: Pen(5) });
        // Saved with the project and read back.
        let (m, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(m.object_styles, s.model.object_styles);
        // Given back what it starts with, a category is the project's start again: nothing of its own to keep.
        s.execute("object_styles", &json!({"categories": {"wall": {"cut": 10}}})).unwrap();
        assert_eq!(s.model.object_styles.keys().collect::<Vec<_>>(), ["door"]);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.object_style("wall").cut, Pen(12));
        s.execute("object_styles", &json!({"reset": true})).unwrap();
        assert!(s.model.object_styles.is_empty());
        // What isn't an object style is refused, saying why, and changes nothing.
        s.execute("object_styles", &json!({"categories": {"floor": {"cut": 9}}})).unwrap();
        let before = s.model.clone();
        for (p, says) in [
            (json!({"categories": {"grid": {"cut": 3}}}), "isn't a model category"),
            (json!({"categories": {"walls": {"cut": 3}}}), "Object Styles has: wall"),
            (json!({"categories": {"wall": {"cut": 17}}}), "0 (a hairline) to 16"),
            (json!({"categories": {"wall": {"cut": 2.5}}}), "whole number"),
            (json!({"categories": {"wall": {"colour": 3}}}), "colour"),
            (json!({"categories": ["wall"]}), "categories"),
            (json!({"reset": "yes"}), "reset"),
        ] {
            let e = s.execute("object_styles", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        }
        assert_eq!(*s.model, *before);
    }

    #[test]
    fn a_category_draws_its_object_styles_pens_cut_and_seen_in_plan_and_section_under_the_views_overrides() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,5000],[0,5000]], "closed": true}
               floor {}
               section {"start": [-1000, 2500], "end": [9000, 2500]}"#,
        )
        .unwrap();
        let (west, floor, section) = (100005, 100006, 100007);
        assert!(matches!(s.model.elements.get(floor), Some(crate::model::Element::Floor(_))));
        let plan =
            |s: &Session| crate::render::draw(&s.model, 100000, View { scale: 0.1, cx: 4000.0, cy: 2500.0, width: 1000, height: 800 });
        let cut = |s: &Session| {
            let view = crate::render::section_fit(&s.model, section, 800, 600);
            crate::render::section(&s.model, section, view)
        };
        // As a project starts: cut pen 10, the floor seen in pen 4.
        let d = plan(&s);
        assert!((widest(&d, west, Part::Cut) - d.width(Pen::CUT)).abs() < 1e-4);
        assert!((widest(&d, floor, Part::Projection) - d.width(Pen::PROJECTION)).abs() < 1e-4);
        // Walls cut in pen 13 and floors seen in pen 1: in plan, and in section where it cuts them.
        s.execute("object_styles", &json!({"categories": {"wall": {"cut": 13}, "floor": {"projection": 1}}})).unwrap();
        let d = plan(&s);
        assert!((widest(&d, west, Part::Cut) - d.width(Pen(13))).abs() < 1e-4, "{}", widest(&d, west, Part::Cut));
        assert!((widest(&d, floor, Part::Projection) - d.width(Pen(1))).abs() < 1e-4);
        let d = cut(&s);
        assert!((widest(&d, west, Part::Cut) - d.width(Pen(13))).abs() < 1e-4);
        // A wall's layers keep their own pen: the cut's weight is its outline's.
        assert!(d.pens.iter().all(|p| *p != Some(BY_CATEGORY)), "every mark replaced");
        // The view's Visibility/Graphics prevails.
        s.execute("graphics", &json!({"view": 100000, "categories": {"wall": {"cut": {"weight": 2}}}})).unwrap();
        let d = plan(&s);
        assert!((widest(&d, west, Part::Cut) - d.width(Pen(2))).abs() < 1e-4);
    }
}
