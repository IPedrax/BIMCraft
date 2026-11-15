//! A studio office, graph first: a reception, a corridor with the meeting rooms and stores off it, a passage of WCs, and a studio
//! the corridor reaches by a door and the passage by an open link. The graph is sampled by the seed from what the studio-office
//! pack allows (`data/graph-rules/studio-office.json`), then geometry is found for it: the studio is the south band and the core
//! of the other rooms lies against it, full length, flush with one end or inset at both. Each try is built in a scratch project
//! and read back by the topology extractor, so a plan that is written is one whose extracted graph is the generated one.
//! Detail: `docs/areas/graph-generator.md`.

mod emit;
mod place;
mod sample;
#[cfg(test)]
mod tests;

use super::{AreaRequest, Built, Refusal, refuse};

/// The kinds of room the studio generator makes: the pack's six.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Reception,
    Hall,
    Storage,
    Meeting,
    Bath,
    Office,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Reception => "reception",
            Kind::Hall => "hall",
            Kind::Storage => "storage",
            Kind::Meeting => "meeting",
            Kind::Bath => "bath",
            Kind::Office => "office",
        }
    }

    /// The role the pack gives the kind.
    pub fn role(self) -> &'static str {
        match self {
            Kind::Reception => "public",
            Kind::Hall => "circulation",
            Kind::Storage => "support",
            Kind::Meeting => "shared",
            Kind::Bath => "private",
            Kind::Office => "work",
        }
    }
}

/// How two rooms are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Means {
    Swing,
    DoubleSwing,
    Open,
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

/// The graph of a studio office, and what the layout needs to know of how it is made.
#[derive(Clone, Debug)]
pub struct StudioGraph {
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub entry: usize,
    pub reception: usize,
    pub corridor: usize,
    pub passage: usize,
    pub studio: usize,
    pub wcs: Vec<usize>,
    /// The meeting rooms and stores in the order they lie along the corridor.
    pub upper: Vec<usize>,
    /// Whether the studio has a way out of its own.
    pub studio_door: bool,
    /// How wide the corridor is, mm: 2000 and more is wide enough for a double-swing door from the reception.
    pub corridor_width: i64,
}

/// The least area, m2 gross, a studio office is met at.
const LEAST_AREA: f64 = 150.0;

pub fn least_area() -> f64 {
    LEAST_AREA
}

/// A brief's studio office: its files, or why there are none.
pub(super) fn build(seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    if area.m2 < LEAST_AREA {
        return Err(refuse(
            "unsatisfiable_area",
            format!(
                "{:.1} m2 is too small for the studio-office program: it cannot fit a reception, a corridor, a passage of WCs and a studio. The least a studio-office brief is met at is about {} m2.",
                area.m2, LEAST_AREA
            ),
        ));
    }
    let graph = sample::sample(seed, area.m2);
    let mut why = String::from("no try was made");
    for attempt in place::attempts(&graph, seed, area.m2) {
        let mut layout = match attempt {
            Ok(layout) => layout,
            Err(e) => {
                if why.starts_with("no try") {
                    why = e;
                }
                continue;
            }
        };
        if let Err(e) = place::orient(&graph, &mut layout) {
            if why.starts_with("no try") {
                why = e;
            }
            continue;
        }
        match emit::verify(&graph, &layout, seed, area) {
            Ok(done) => return emit::finish(&graph, &layout, done, seed, area),
            Err(e) => why = e,
        }
    }
    Err(refuse(
        "unplaceable_graph",
        format!("no shell fits the sampled studio office at {:.1} m2 (seed {seed}): {why}. Try another seed or area.", area.m2),
    ))
}
