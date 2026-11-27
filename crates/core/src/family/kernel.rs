//! Family forms and the voids that cut them. A backend receives the original form recipe, including
//! analytic sketch curves, and borrows the family's formula cache. Only the finished solids need to
//! become mesh pieces for existing consumers. Adaptive and project-view booleans have separate builders.

use super::{Family, Form, Node, Piece, Val, heights};
use crate::csg;
use crate::view3d::P3;
use std::collections::{BTreeMap, HashMap};

pub(super) trait FormKernel {
    type Solid: Into<Piece>;
    type Cutter;

    /// Build the visible form in family coordinates. Hidden forms return `None`.
    fn form<'a>(
        &self,
        family: &'a Family,
        form: &'a Form,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Option<Self::Solid>, String>;

    /// Keep just the geometry that subtraction needs.
    fn cutter(&self, solid: Self::Solid) -> Self::Cutter;

    /// Subtract this void using the CSG budget shared by the family build.
    fn cut(&self, solid: &mut Self::Solid, cutter: &Self::Cutter, budget: &mut csg::Meter) -> Result<(), String>;
}

pub(super) struct MeshKernel;

impl FormKernel for MeshKernel {
    type Solid = Piece;
    type Cutter = Vec<[P3; 3]>;

    fn form<'a>(
        &self,
        family: &'a Family,
        form: &'a Form,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Option<Piece>, String> {
        family.mesh_form(form, values, read)
    }

    fn cutter(&self, solid: Piece) -> Self::Cutter {
        solid.tris
    }

    fn cut(&self, solid: &mut Piece, cutter: &Self::Cutter, budget: &mut csg::Meter) -> Result<(), String> {
        let tris = csg::subtract(&solid.tris, cutter, budget)?;
        if tris != solid.tris {
            // The outline seen from above is retained after a cut.
            (solid.bottom, solid.top) = heights(&tris);
            solid.caps = tris.len();
            solid.round = false;
            solid.tris = tris;
        }
        Ok(())
    }
}
