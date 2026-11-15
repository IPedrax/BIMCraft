//! A house, graph first: a graph of rooms and the ways between them is sampled from the house rule pack by the seed, and
//! geometry is then found for it, a shell of bands (rectangles) the rooms are dealt into. Every `door` and `open` edge of the
//! graph must land on a wall long enough for it; a try that fails is skipped for the next in a fixed order, and a graph no try
//! fits is refused. Each try is built in a scratch project and read back by the topology extractor, so a house that is
//! written is one whose extracted graph is the generated one. Detail: `docs/areas/graph-generator.md`.

mod emit;
pub(crate) mod place;
mod sample;
pub(crate) mod swing;

use serde_json::Value;

use super::{AreaRequest, Built, Refusal, refuse};
use crate::engine::Session;
pub(crate) use crate::generate::swing::{FOLDING_FAMILY, folding_family};
pub(crate) use emit::{longest, sheet_for};

/// The kinds of room the house generator makes: the house pack's, as far as it uses them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Bedroom,
    Closet,
    Bath,
    Hall,
    Living,
    Kitchen,
    Dining,
    Laundry,
    Porch,
    Garage,
    Deck,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bedroom => "bedroom",
            Kind::Closet => "closet",
            Kind::Bath => "bath",
            Kind::Hall => "hall",
            Kind::Living => "living",
            Kind::Kitchen => "kitchen",
            Kind::Dining => "dining",
            Kind::Laundry => "laundry",
            Kind::Porch => "porch",
            Kind::Garage => "garage",
            Kind::Deck => "deck",
        }
    }

    /// The role the pack gives the kind.
    pub fn role(self) -> &'static str {
        match self {
            Kind::Bedroom | Kind::Bath => "private",
            Kind::Closet => "support",
            Kind::Hall => "circulation",
            Kind::Living | Kind::Kitchen | Kind::Dining => "shared",
            Kind::Laundry | Kind::Garage => "service",
            Kind::Porch | Kind::Deck => "public",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Bedroom => "Bedroom",
            Kind::Closet => "Closet",
            Kind::Bath => "Bath",
            Kind::Hall => "Hall",
            Kind::Living => "Living",
            Kind::Kitchen => "Kitchen",
            Kind::Dining => "Dining",
            Kind::Laundry => "Laundry",
            Kind::Porch => "Porch",
            Kind::Garage => "Garage",
            Kind::Deck => "Deck",
        }
    }
}

/// How two rooms are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Means {
    Swing,
    Sliding,
    Open,
    Folding,
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

/// A bedroom and the closet and the en-suite bath it may have, as indexes of the graph's rooms.
#[derive(Clone, Copy, Debug)]
pub struct Bed {
    pub room: usize,
    pub closet: Option<usize>,
    pub bath: Option<usize>,
}

/// The graph of a house and what the layout needs to know of how it is made.
#[derive(Clone, Debug)]
pub struct HouseGraph {
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub entry: usize,
    /// The shared rooms in the order they lie along the public band.
    pub public: Vec<usize>,
    pub hall: usize,
    /// The public room the hall joins.
    pub hall_to: usize,
    pub beds: Vec<Bed>,
    pub hall_bath: usize,
    /// A closet or laundry at the hall's far end.
    pub cap: Option<usize>,
    pub porch: Option<usize>,
    /// The public room the porch is off.
    pub porch_off: usize,
    /// A deck off one or two shared rooms (`deck_off`). Houses above 160 m2 only, as a garage, a room of kind `Garage`, is.
    pub deck: Option<usize>,
    pub deck_off: Vec<usize>,
}

impl HouseGraph {
    /// The rooms outside the walls, which the shell leaves unwalled and closes off with separation lines.
    pub fn outside(&self) -> impl Iterator<Item = usize> {
        self.porch.into_iter().chain(self.deck)
    }
}

/// Above this area, m2 gross, a house is made on the model of the Unit 1 trace: an entry hall, a garage, a deck and a fourth
/// bedroom in the biggest. At and below it the first trace's family is untouched.
pub const LARGE_AREA: f64 = 160.0;

/// The least area, m2 gross, a house is met at.
const LEAST_AREA: f64 = 45.0;

pub fn least_area() -> f64 {
    LEAST_AREA
}

/// A brief's house: its files, or why there are none.
pub(super) fn build(seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    if area.m2 < LEAST_AREA {
        return Err(refuse(
            "unsatisfiable_area",
            format!(
                "{:.1} m2 is too small for the house program: it cannot fit a living room, a kitchen, a bedroom and a bath. The least a house brief is met at is about {} m2.",
                area.m2, LEAST_AREA
            ),
        ));
    }
    let graph = sample::sample(seed, area.m2);
    let mut why = String::from("no try was made");
    for attempt in place::attempts(&graph, seed, area.m2) {
        let layout = match attempt {
            Ok(mut layout) => match swing::orient(&graph, &mut layout) {
                Ok(()) => layout,
                Err(e) => {
                    if why.starts_with("no try") {
                        why = e;
                    }
                    continue;
                }
            },
            Err(e) => {
                if why.starts_with("no try") {
                    why = e;
                }
                continue;
            }
        };
        match emit::verify(&graph, &layout, seed, area) {
            Ok(done) => return emit::finish(&graph, &layout, done, seed, area),
            Err(e) => why = e,
        }
    }
    Err(refuse(
        "unplaceable_graph",
        format!("no shell fits the sampled house at {:.1} m2 (seed {seed}): {why}. Try another seed or area.", area.m2),
    ))
}

pub(crate) fn extract(session: &Session) -> Value {
    crate::topology::extract(&session.model)
}
