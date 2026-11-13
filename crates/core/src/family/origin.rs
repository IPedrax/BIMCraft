//! A family's origin (O1): its insertion point, where the planes that define the origin cross ([`Family::origin`]). The
//! family's forms, planes and lines stay where they are in its axes; only the point it is placed by changes. Every way of
//! putting a family's axes in the model goes through [`crate::hosting::Placement::by_origin`], which places the family by
//! this point.

use super::*;

impl Family {
    /// Its insertion point in its own axes: where the planes that define the origin cross, one square to each of x, y and z,
    /// a datum's where no plane of its own takes the origin over in that direction (Ref. Level, Center (Front/Back), Center
    /// (Left/Right)). Those planes stay as a type flexes, so this is the same for every type. An adaptive family has none, its
    /// points placing it: the datums' crossing.
    pub fn origin(&self) -> P3 {
        let mut o = [0.0; 3];
        if self.adaptive.is_some() {
            return o;
        }
        for p in self.planes.iter().filter(|p| p.defines_origin && !p.datum()) {
            let frame = p.frame();
            if let Some(k) = (0..3).find(|&k| frame.z[k].abs() > 1.0 - 1e-9)
                && frame.origin[k].is_finite()
                && frame.origin[k].abs() <= MAX_COORD
            {
                o[k] = frame.origin[k] + 0.0;
            }
        }
        o
    }
}
