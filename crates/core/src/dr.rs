//! The circuits NBR 5410:2004 asks additional protection by a high-sensitivity residual-current device (DR) for: a rated residual
//! current of 30 mA at most, whatever the earthing scheme. A pure function of the circuits it is given, with no model behind it:
//! the `dr_check` command is its only caller.
//!
//! A circuit needs it if any point it serves is in one of these cases:
//!
//! - a place with a bathtub or a shower: every point;
//! - an outlet in an external area;
//! - an internal outlet that may feed equipment outdoors;
//! - in a dwelling, a point in a kitchen, pantry-kitchen, laundry, service area, garage or other internal area that is wet in
//!   normal use or may be washed, but for lights 2.5 m up or more;
//! - in a building that is not a dwelling, an outlet in those same areas.
//!
//! An outlet is covered up to a rated current of 32 A. A circuit or sector of an IT scheme that must not be cut, for the safety of
//! persons or to keep lives, is left out. What is not covered, and the caller says so: the further rules for the volumes of a
//! bathroom and for swimming pools, and the other protections against shock the device does not replace.

/// The most the device's rated residual current may be, mA.
pub const MAX_RESIDUAL: f64 = 30.0;
/// The most an outlet's rated current may be for the rule to cover it, A.
pub const MAX_OUTLET: f64 = 32.0;
/// A light at this height (mm) or above, in a dwelling, is left out.
pub const LIGHT_HEIGHT: f64 = 2500.0;

/// Where a point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A place with a bathtub or a shower.
    Bath,
    External,
    Kitchen,
    Laundry,
    Service,
    Garage,
    /// Another internal area that is wet in normal use or may be washed.
    Wet,
    Dry,
}

/// The keys of `Place`, as a command gives them.
pub const PLACES: &[&str] = &["bath", "external", "kitchen", "laundry", "service", "garage", "wet", "dry"];

impl Place {
    pub fn parse(s: &str) -> Option<Place> {
        Some(match s {
            "bath" => Place::Bath,
            "external" => Place::External,
            "kitchen" => Place::Kitchen,
            "laundry" => Place::Laundry,
            "service" => Place::Service,
            "garage" => Place::Garage,
            "wet" => Place::Wet,
            "dry" => Place::Dry,
            _ => return None,
        })
    }

    /// The rooms of a dwelling whose every point needs it, but for high lights.
    fn wet(self) -> bool {
        matches!(self, Place::Kitchen | Place::Laundry | Place::Service | Place::Garage | Place::Wet)
    }
}

/// What a point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Outlet,
    Lighting,
    /// Any other point of use, such as a shower heater connected directly.
    Fixed,
}

/// The keys of `Kind`, as a command gives them.
pub const KINDS: &[&str] = &["outlet", "lighting", "fixed"];

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "outlet" => Kind::Outlet,
            "lighting" => Kind::Lighting,
            "fixed" => Kind::Fixed,
            _ => return None,
        })
    }
}

/// A point a circuit serves.
#[derive(Clone, Debug)]
pub struct Point {
    pub place: Place,
    pub kind: Kind,
    /// An outlet's rated current, A; unknown, it is taken as within the 32 A.
    pub rating: Option<f64>,
    /// A light's height, mm.
    pub height: Option<f64>,
    /// An internal outlet that may feed equipment outdoors.
    pub outdoor: bool,
}

/// A circuit: its points, the rated residual current (mA) of the device it has, if any, and whether it is of an IT scheme that must
/// not be cut.
#[derive(Clone, Debug)]
pub struct Circuit {
    pub points: Vec<Point>,
    pub residual: Option<f64>,
    pub it_continuity: bool,
}

/// What the standard asks of a circuit.
#[derive(Debug)]
pub struct Verdict {
    pub required: bool,
    /// It would need it, but is an IT circuit that must not be cut.
    pub exempt: bool,
    pub reasons: Vec<String>,
    /// Whether what it has is enough; `None` for a circuit that does not need it.
    pub ok: Option<bool>,
}

fn covered(p: &Point) -> bool {
    p.kind != Kind::Outlet || p.rating.is_none_or(|a| a <= MAX_OUTLET)
}

/// Why a point asks for it, if it does.
fn reason(p: &Point, residential: bool) -> Option<&'static str> {
    if p.place == Place::Bath {
        return Some("bath: a point in a place with a bathtub or a shower");
    }
    if !covered(p) {
        return None;
    }
    match (p.place, p.kind) {
        (Place::External, Kind::Outlet) => Some("external: an outlet in an external area"),
        (Place::External, _) => None,
        (_, Kind::Outlet) if p.outdoor => Some("outdoor: an internal outlet that may feed equipment outdoors"),
        (place, kind) if place.wet() => match (residential, kind) {
            (true, Kind::Lighting) if p.height.is_some_and(|h| h >= LIGHT_HEIGHT) => None,
            (true, _) => Some("wet: a point in a kitchen, laundry, service area, garage or other wet internal area of a dwelling"),
            (false, Kind::Outlet) => Some("wet: an outlet in a kitchen, laundry, service area, garage or other wet internal area"),
            (false, _) => None,
        },
        _ => None,
    }
}

/// What a circuit needs and whether it has it; `residential` says the building is a dwelling.
pub fn check(c: &Circuit, residential: bool) -> Verdict {
    let mut reasons: Vec<String> = Vec::new();
    for r in c.points.iter().filter_map(|p| reason(p, residential)) {
        if !reasons.iter().any(|x| x == r) {
            reasons.push(r.to_string());
        }
    }
    let asked = !reasons.is_empty();
    let exempt = asked && c.it_continuity;
    let required = asked && !exempt;
    let ok = required.then(|| c.residual.is_some_and(|r| r <= MAX_RESIDUAL));
    Verdict { required, exempt, reasons, ok }
}

#[cfg(test)]
mod tests;
