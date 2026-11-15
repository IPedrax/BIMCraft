//! An office by the pattern of a lobby, a corridor and an open office, graph first like the studio: a lobby at one end with the
//! only way in from outside, a corridor the length of the plan with private offices, conference rooms, restrooms, a pantry (and on
//! some seeds focus rooms) off one side or both, and an open office at the far end, the full depth of the plan. A corner of the
//! lobby and one of the open office may be chamfered, and the seed turns and mirrors the plan. The dimensions are drawn from the
//! seed and scaled by the area, then each try is built in a scratch project and read back by the topology extractor, so a plan
//! that is written is one whose extracted graph is the generated one and passes the pack (`data/graph-rules/office-pattern.json`).
//! Detail: `docs/areas/graph-generator.md`.

mod emit;
mod place;
#[cfg(test)]
mod tests;

use super::{AreaRequest, Built, Refusal, refuse};

/// The kinds of room the pattern makes: the pack's eight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Lobby,
    Hall,
    PrivateOffice,
    Conference,
    Restroom,
    Pantry,
    FocusRoom,
    OpenOffice,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Lobby => "lobby",
            Kind::Hall => "hall",
            Kind::PrivateOffice => "private_office",
            Kind::Conference => "conference",
            Kind::Restroom => "restroom",
            Kind::Pantry => "pantry",
            Kind::FocusRoom => "focus_room",
            Kind::OpenOffice => "open_office",
        }
    }

    /// The role the pack gives the kind.
    pub fn role(self) -> &'static str {
        match self {
            Kind::Lobby => "public",
            Kind::Hall => "circulation",
            Kind::PrivateOffice | Kind::FocusRoom => "private",
            Kind::Conference | Kind::OpenOffice => "shared",
            Kind::Restroom => "service",
            Kind::Pantry => "support",
        }
    }
}

/// How two rooms are joined: by a swing door, or a double-swing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Means {
    Swing,
    DoubleSwing,
}

#[derive(Clone, Debug)]
pub struct Room {
    /// The node's id, which is also the room's number in the project.
    pub id: String,
    pub name: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    pub means: Means,
}

/// The graph of an office pattern: the lobby, the corridor and the open office are the rooms 0, 1 and 2, then the rooms of the
/// south band and the north band.
#[derive(Clone, Debug)]
pub struct PatternGraph {
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub entry: usize,
    pub lobby: usize,
    pub corridor: usize,
    pub open: usize,
}

/// The least area, m2 gross, an office pattern is met at.
const LEAST_AREA: f64 = 170.0;

pub fn least_area() -> f64 {
    LEAST_AREA
}

/// A brief's office pattern: its files, or why there are none.
pub(super) fn build(seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    if area.m2 < LEAST_AREA {
        return Err(refuse(
            "unsatisfiable_area",
            format!(
                "{:.1} m2 is too small for the office pattern: it cannot fit a lobby, a corridor with a private office, a conference room, a restroom and a pantry off it, and an open office. The least an office-pattern brief is met at is about {} m2.",
                area.m2, LEAST_AREA
            ),
        ));
    }
    let mut why = String::from("no try was made");
    for t in 0..place::TRIES {
        let case = match place::draw(seed, area.m2, t) {
            Ok(case) => case,
            Err(e) => {
                remember(&mut why, e);
                continue;
            }
        };
        let graph = place::graph_of(&case);
        let mut layout = match place::assemble(&graph, &case, seed) {
            Ok(layout) => layout,
            Err(e) => {
                remember(&mut why, e);
                continue;
            }
        };
        if let Err(e) = place::orient(&graph, &mut layout) {
            remember(&mut why, e);
            continue;
        }
        match emit::verify(&graph, &layout, seed, area) {
            Ok(done) => return emit::finish(&graph, &layout, done, seed, area),
            Err(e) => why = e,
        }
    }
    Err(refuse(
        "unplaceable_graph",
        format!("no plan fits the office pattern at {:.1} m2 (seed {seed}): {why}. Try another seed or area.", area.m2),
    ))
}

/// Keeps the first reason a try failed before any was built.
fn remember(why: &mut String, e: String) {
    if why.starts_with("no try") {
        *why = e;
    }
}
