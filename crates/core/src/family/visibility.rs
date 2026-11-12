//! F8: where a family's part is drawn, as Revit's Family Element Visibility Settings say: in which views by the way they look
//! at it (Plan/RCP, Front/Back, Left/Right; only where cut) and at which detail levels (Coarse, Medium, Fine). It is
//! drawing only: a part hidden in a view is still in the model, its IFC and its schedules, which read [`super::Family::pieces`]
//! as before. A part with no settings draws as a file of 17 did (`None`, below).
//!
//! The views ask through [`FormView`], which a [`crate::graphics::Look`] holds: a plan, a view along a direction in plan (a
//! section or an elevation), or 3D, and its detail level. A built form carries its settings as a [`Piece`](super::Piece)'s
//! `vis`, and the 3D scene's triangles carry them packed ([`packed`]) with the way the component's x axis runs in plan.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::graphics::Detail;

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

/// Revit's Family Element Visibility Settings of a form, a void, a nested instance or (its detail levels only) a symbol.
/// Every key may be left out, and is then Revit's default: shown everywhere, not only when cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visibility {
    /// Plan/RCP: in floor plans and ceiling plans, cut or not.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub plan: bool,
    /// Front/Back: in sections and elevations that look along the family's y axis, as its Front and Back elevations do.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub front_back: bool,
    /// Left/Right: in sections and elevations that look along its x axis.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub left_right: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub coarse: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub medium: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub fine: bool,
    /// Only shown when cut: in a plan whose cut plane passes through it, and as a section's poché; never in projection, in an
    /// elevation or in 3D.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub only_when_cut: bool,
}

impl Default for Visibility {
    fn default() -> Visibility {
        Visibility { plan: true, front_back: true, left_right: true, coarse: true, medium: true, fine: true, only_when_cut: false }
    }
}

/// What a symbol with no settings is: drawn at Medium and Fine, as a plan's symbols always were (Coarse left them out).
pub const SYMBOL_DEFAULT: Visibility =
    Visibility { plan: true, front_back: true, left_right: true, coarse: false, medium: true, fine: true, only_when_cut: false };

impl Visibility {
    /// Whether it shows at detail level `d`.
    pub fn at(self, d: Detail) -> bool {
        match d {
            Detail::Coarse => self.coarse,
            Detail::Medium => self.medium,
            Detail::Fine => self.fine,
        }
    }

    /// Both: shown only where each is (a nested instance's settings over its family's form's).
    pub fn and(self, o: Visibility) -> Visibility {
        Visibility {
            plan: self.plan && o.plan,
            front_back: self.front_back && o.front_back,
            left_right: self.left_right && o.left_right,
            coarse: self.coarse && o.coarse,
            medium: self.medium && o.medium,
            fine: self.fine && o.fine,
            only_when_cut: self.only_when_cut || o.only_when_cut,
        }
    }

    /// Settings `a` and `b`, either of which may be none.
    pub fn both(a: Option<Visibility>, b: Option<Visibility>) -> Option<Visibility> {
        match (a, b) {
            (Some(a), Some(b)) => Some(a.and(b)),
            (a, b) => a.or(b),
        }
    }

    /// Why a symbol can't hold these, if it can't: a symbol is drawn only in plan, so only its detail levels may change.
    pub fn check_symbol(self) -> Result<(), String> {
        let only_details = Visibility { coarse: self.coarse, medium: self.medium, fine: self.fine, ..Visibility::default() };
        if self == only_details {
            Ok(())
        } else {
            Err("a symbol is drawn only in plan: its `visibility` takes `coarse`, `medium` and `fine`, not `plan`, \
                 `front_back`, `left_right` or `only_when_cut`"
                .into())
        }
    }

    /// Whether a plan (or a ceiling plan) at `detail` draws it, `cut` saying whether its cut plane passes through it.
    pub fn in_plan(self, detail: Detail, cut: bool) -> bool {
        self.plan && self.at(detail) && (cut || !self.only_when_cut)
    }

    /// Whether a view along `class` at `detail` draws it, in projection (`cut` false) or as its poché (`cut` true).
    pub fn along(self, class: Class, detail: Detail, cut: bool) -> bool {
        let way = match class {
            Class::FrontBack => self.front_back,
            Class::LeftRight => self.left_right,
        };
        way && self.at(detail) && (cut || !self.only_when_cut)
    }

    /// Whether a 3D view at `detail` draws it: whichever way it looks, never what is only shown when cut.
    pub fn in_3d(self, detail: Detail) -> bool {
        self.at(detail) && !self.only_when_cut
    }
}

/// Which of a family's settings a view looking along a direction in plan reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    FrontBack,
    LeftRight,
}

/// Which settings a view looking along `looking` (in plan) reads of a component whose x axis runs along `x` (in plan): Left/Right
/// when it looks nearer its x axis, else Front/Back (a tie too).
pub fn class(looking: P2, x: P2) -> Class {
    let along = looking.dot(x).abs();
    let across = looking.dot(x.perp()).abs();
    if along > across + 1e-9 { Class::LeftRight } else { Class::FrontBack }
}

/// How a view looks at forms: from above or below (a plan), along a direction in plan (a section or an elevation), or in 3D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seen {
    Plan,
    Along(P2),
    ThreeD,
}

/// What a view asks of a part's settings: how it looks and at what detail. The default is a 3D view at Fine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormView {
    pub seen: Seen,
    pub detail: Detail,
}

impl Default for FormView {
    fn default() -> FormView {
        FormView { seen: Seen::ThreeD, detail: Detail::Fine }
    }
}

/// The bit saying a packed value holds settings; without it, the part has none and is drawn everywhere.
const SET: u32 = 1 << 31;
const PLAN: u32 = 1;
const FRONT_BACK: u32 = 1 << 1;
const LEFT_RIGHT: u32 = 1 << 2;
const COARSE: u32 = 1 << 3;
const MEDIUM: u32 = 1 << 4;
const FINE: u32 = 1 << 5;
const ONLY_CUT: u32 = 1 << 6;
/// Where the angle of the component's x axis in plan sits, as a fraction of a half turn in 16 bits.
const ANGLE_SHIFT: u32 = 8;

/// Settings `v` of a part whose component's x axis runs along `x` in plan, packed for a triangle: 0 for none. The flags say
/// what hides it; the axis is kept to a half turn, which is all [`class`] reads.
pub fn packed(v: Option<Visibility>, x: P2) -> u32 {
    let Some(v) = v else { return 0 };
    let mut bits = SET;
    for (shown, bit) in [
        (v.plan, PLAN),
        (v.front_back, FRONT_BACK),
        (v.left_right, LEFT_RIGHT),
        (v.coarse, COARSE),
        (v.medium, MEDIUM),
        (v.fine, FINE),
        (!v.only_when_cut, ONLY_CUT),
    ] {
        if !shown {
            bits |= bit;
        }
    }
    let turn = x.1.atan2(x.0).rem_euclid(std::f64::consts::PI);
    let turn = if turn.is_finite() { turn } else { 0.0 };
    let q = ((turn / std::f64::consts::PI) * 65535.0).round().clamp(0.0, 65535.0) as u32;
    bits | (q << ANGLE_SHIFT)
}

/// The settings and the component's x axis a packed value holds; none for 0.
pub fn unpacked(bits: u32) -> Option<(Visibility, P2)> {
    if bits & SET == 0 {
        return None;
    }
    let off = |bit: u32| bits & bit != 0;
    let v = Visibility {
        plan: !off(PLAN),
        front_back: !off(FRONT_BACK),
        left_right: !off(LEFT_RIGHT),
        coarse: !off(COARSE),
        medium: !off(MEDIUM),
        fine: !off(FINE),
        only_when_cut: off(ONLY_CUT),
    };
    let turn = f64::from((bits >> ANGLE_SHIFT) & 0xffff) / 65535.0 * std::f64::consts::PI;
    Some((v, P2(turn.cos(), turn.sin())))
}

impl FormView {
    /// Whether this view draws a triangle (or a solid) whose settings are packed as `bits`, in projection (`cut` false) or as
    /// a section's poché (`cut` true). A plan draws no triangles: what it asks is [`FormView::piece_in_plan`].
    pub fn shows(&self, bits: u32, cut: bool) -> bool {
        let Some((v, x)) = unpacked(bits) else { return true };
        match self.seen {
            Seen::ThreeD => v.in_3d(self.detail),
            Seen::Along(looking) => v.along(class(looking, x), self.detail, cut),
            Seen::Plan => v.in_plan(self.detail, cut),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_keeps_the_settings_and_the_axis_to_a_half_turn() {
        let v = Visibility { plan: false, coarse: false, only_when_cut: true, ..Visibility::default() };
        let (back, x) = unpacked(packed(Some(v), P2(0.0, -1.0))).unwrap();
        assert_eq!(back, v);
        assert!(x.0.abs() < 1e-4 && (x.1.abs() - 1.0).abs() < 1e-4, "{x:?}");
        assert_eq!(packed(None, P2(1.0, 0.0)), 0);
        assert!(FormView::default().shows(0, false));
    }

    #[test]
    fn a_view_reads_front_back_or_left_right_by_the_nearer_axis() {
        let x = P2(1.0, 0.0);
        assert_eq!(class(P2(0.0, 1.0), x), Class::FrontBack);
        assert_eq!(class(P2(-1.0, 0.0), x), Class::LeftRight);
        // At 45 degrees, a tie: Front/Back.
        let d = P2(1.0, 1.0).unit();
        assert_eq!(class(d, x), Class::FrontBack);
        // A component turned a quarter: a view looking north sees its side.
        assert_eq!(class(P2(0.0, 1.0), P2(0.0, 1.0)), Class::LeftRight);
    }

    #[test]
    fn plans_sections_and_3d_read_their_own_settings() {
        let leaf = Visibility { plan: false, coarse: false, ..Visibility::default() };
        let bits = packed(Some(leaf), P2(1.0, 0.0));
        let at = |seen, detail| FormView { seen, detail };
        assert!(!leaf.in_plan(Detail::Fine, true));
        assert!(at(Seen::Along(P2(0.0, 1.0)), Detail::Fine).shows(bits, false));
        assert!(!at(Seen::Along(P2(0.0, 1.0)), Detail::Coarse).shows(bits, false));
        assert!(at(Seen::ThreeD, Detail::Medium).shows(bits, false));
        assert!(!at(Seen::ThreeD, Detail::Coarse).shows(bits, false));
        let cut_only = packed(Some(Visibility { only_when_cut: true, ..Visibility::default() }), P2(1.0, 0.0));
        assert!(!at(Seen::ThreeD, Detail::Fine).shows(cut_only, false));
        assert!(!at(Seen::Along(P2(0.0, 1.0)), Detail::Fine).shows(cut_only, false));
        assert!(at(Seen::Along(P2(0.0, 1.0)), Detail::Fine).shows(cut_only, true));
    }

    #[test]
    fn settings_read_and_write_only_what_differs_from_revits_defaults() {
        let v: Visibility = serde_json::from_value(serde_json::json!({"plan": false})).unwrap();
        assert_eq!(v, Visibility { plan: false, ..Visibility::default() });
        assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!({"plan": false}));
        assert!(serde_json::from_value::<Visibility>(serde_json::json!({"plna": false})).is_err());
        assert!(SYMBOL_DEFAULT.check_symbol().is_ok());
        assert!(Visibility { plan: false, ..Visibility::default() }.check_symbol().is_err());
    }
}
