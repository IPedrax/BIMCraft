//! The loads a dwelling's rooms must have as NBR 5410:2004 item 9.5.2 asks (residential): the fixed lighting point of each room
//! (9.5.2.1) and its general-use outlets, TUG (9.5.2.2.1 and 9.5.2.2.2). Pure functions of their input: the `load_points`
//! command is their caller, reading the rooms' areas and perimeters from the model or from its params.
//!
//! - Lighting: at least one point in the ceiling of each room, 100 VA up to 6 m² and 100 VA + 60 VA for each whole 4 m² above 6.
//! - Outlets, by the kind of room (known by its name, or given): a bathroom 1; kitchens, pantries, service areas and laundries
//!   one for each 3.5 m of perimeter (a whole number up, 1 at least); rooms and bedrooms one for each 5 m; a veranda 1; the others
//!   1 up to 6 m² and one for each 5 m of perimeter above.
//! - Power: bathrooms, kitchens, pantries, service areas and laundries 600 VA for each of the first 3 points of a room and 100 VA
//!   for each other; where those rooms together have more than 6 points the standard lets it be 600 VA for the first 2 of each room
//!   (the alternative, taken only when asked). Rooms, bedrooms, verandas and the others 100 VA a point. Power is paid for the points
//!   a room has, not for the least it needs.
//!
//! What is not applied: the exceptions the standard allows to the lighting point and to a veranda's outlet, and the placement of
//! the outlets (near the lavatory; above a counter; for a room of 2.25 m² or less, up to 0.80 m outside the door). The TUE, the
//! outlets for specific use, are not here.

/// The kinds of room the standard tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bath,
    Kitchen,
    Service,
    Dry,
    Veranda,
    Other,
}

/// The keys of `Kind`, as a command gives them.
pub const KINDS: &[&str] = &["bath", "kitchen", "service", "dry", "veranda", "other"];

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "bath" => Kind::Bath,
            "kitchen" => Kind::Kitchen,
            "service" => Kind::Service,
            "dry" => Kind::Dry,
            "veranda" => Kind::Veranda,
            "other" => Kind::Other,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        KINDS[self as usize]
    }

    /// Whether the room is one whose outlets are paid at 600 VA (bathrooms, kitchens, pantries, service areas, laundries).
    pub fn wet(self) -> bool {
        matches!(self, Kind::Bath | Kind::Kitchen | Kind::Service)
    }
}

/// A name in lower case without accents.
fn plain(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// The kind of room a name says (Portuguese or English), `Other` when it says none.
pub fn kind_of(name: &str) -> Kind {
    let n = plain(name);
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let word = |w: &str| n.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
    if has(&["servico", "lavanderia", "laundry", "utility"]) {
        Kind::Service
    } else if has(&["banheiro", "lavabo", "sanitario", "bathroom", "toilet", "restroom"]) || word("wc") || word("bwc") || word("bath") {
        Kind::Bath
    } else if has(&["cozinha", "copa", "kitchen", "pantry"]) {
        Kind::Kitchen
    } else if has(&["varanda", "sacada", "balcony", "veranda"]) {
        Kind::Veranda
    } else if has(&["sala", "dormitorio", "quarto", "suite", "living", "bedroom"]) {
        Kind::Dry
    } else {
        Kind::Other
    }
}

/// The lighting of a room (VA) for its area (m²).
pub fn lighting_va(area: f64) -> f64 {
    if area > 6.0 { 100.0 + ((area - 6.0) / 4.0 + 1e-9).floor() * 60.0 } else { 100.0 }
}

/// How many times `step` goes into `length`, a whole number up and 1 at least.
fn per(length: f64, step: f64) -> u32 {
    (length / step - 1e-9).ceil().max(1.0) as u32
}

/// The fewest outlets a room needs, for its area (m²) and perimeter (m).
pub fn min_points(kind: Kind, area: f64, perimeter: f64) -> u32 {
    match kind {
        Kind::Bath | Kind::Veranda => 1,
        Kind::Kitchen | Kind::Service => per(perimeter, 3.5),
        Kind::Dry => per(perimeter, 5.0),
        Kind::Other if area > 6.0 => per(perimeter, 5.0),
        Kind::Other => 1,
    }
}

/// The power (VA) of `points` outlets in a room; `alternative` is the 600 VA for the first 2 points, not the first 3.
pub fn power_va(kind: Kind, points: u32, alternative: bool) -> f64 {
    let n = f64::from(points);
    if kind.wet() {
        let full = if alternative { 2.0 } else { 3.0 };
        600.0 * n.min(full) + 100.0 * (n - full).max(0.0)
    } else {
        100.0 * n
    }
}

/// The outlets a kitchen or pantry needs over the counter of its sink, at the same point or at others.
pub fn counter_outlets(kind: Kind, name: &str) -> u32 {
    let n = plain(name);
    if kind == Kind::Kitchen && ["cozinha", "kitchen", "copa"].iter().any(|w| n.contains(w)) { 2 } else { 0 }
}

/// A room as the sum takes it: the area in m², the perimeter in m, and the outlets it has (`None`: just the least).
pub struct Room<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub area: f64,
    pub perimeter: f64,
    pub provided: Option<u32>,
}

/// One room's share.
#[derive(Debug)]
pub struct Share {
    pub lighting_va: f64,
    pub minimum: u32,
    pub points: u32,
    pub tug_va: f64,
    pub counter_outlets: u32,
}

/// The shares of all the rooms and their sums.
#[derive(Debug)]
pub struct Total {
    pub rooms: Vec<Share>,
    pub lighting_va: f64,
    pub tug_va: f64,
    /// The outlets of the rooms paid at 600 VA.
    pub wet_points: u32,
    /// Whether those are above 6, so that the alternative may be taken.
    pub alternative_possible: bool,
    pub alternative_applied: bool,
}

/// Adds up the rooms; `alternative` is taken only where the standard allows it.
pub fn total(rooms: &[Room], alternative: bool) -> Total {
    let counts: Vec<(u32, u32)> = rooms
        .iter()
        .map(|r| {
            let least = min_points(r.kind, r.area, r.perimeter);
            (least, r.provided.unwrap_or(least))
        })
        .collect();
    let wet_points: u32 = rooms.iter().zip(&counts).filter(|(r, _)| r.kind.wet()).map(|(_, c)| c.1).sum();
    let possible = wet_points > 6;
    let applied = alternative && possible;
    let shares: Vec<Share> = rooms
        .iter()
        .zip(&counts)
        .map(|(r, &(minimum, points))| Share {
            lighting_va: lighting_va(r.area),
            minimum,
            points,
            tug_va: power_va(r.kind, points, applied),
            counter_outlets: counter_outlets(r.kind, r.name),
        })
        .collect();
    Total {
        lighting_va: shares.iter().map(|s| s.lighting_va).sum(),
        tug_va: shares.iter().map(|s| s.tug_va).sum(),
        rooms: shares,
        wet_points,
        alternative_possible: possible,
        alternative_applied: applied,
    }
}

#[cfg(test)]
mod tests;
