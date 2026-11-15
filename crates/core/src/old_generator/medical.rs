//! A medical day department by the pattern of `specs/medical-day-pattern-generator.md`, graph first like the office pattern: a window
//! band of treatment bays open to a patient corridor, core blocks (a treatment block and an isolation block always, a service block
//! on some seeds) between cross corridors, an office band on some seeds, and a staff corridor, with an end corridor at the far end
//! and, at the start, nothing, a head row or a second end corridor. A department, not a building: there is no door to the outside.
//! Layout only: walls, doors, room separation lines and rooms. Each try is built in a scratch project and read back by the topology
//! extractor, so a plan that is written is one whose extracted graph is the generated one and passes the pack
//! (`data/graph-rules/medical-day.json`). Detail: `docs/areas/graph-generator.md`.

mod emit;
mod place;

use super::{AreaRequest, Built, Refusal, refuse};

/// The kinds of room the pattern makes: the pack's eighteen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    TreatmentBay,
    TreatmentRoom,
    Isolation,
    Anteroom,
    Washroom,
    Medication,
    CleanUtility,
    SoiledUtility,
    StaffWork,
    Office,
    Meeting,
    Storage,
    Alcove,
    Waiting,
    Shell,
    Plant,
    Corridor,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::TreatmentBay => "treatment_bay",
            Kind::TreatmentRoom => "treatment_room",
            Kind::Isolation => "isolation",
            Kind::Anteroom => "anteroom",
            Kind::Washroom => "washroom",
            Kind::Medication => "medication",
            Kind::CleanUtility => "clean_utility",
            Kind::SoiledUtility => "soiled_utility",
            Kind::StaffWork => "staff_work",
            Kind::Office => "office",
            Kind::Meeting => "meeting",
            Kind::Storage => "storage",
            Kind::Alcove => "alcove",
            Kind::Waiting => "waiting",
            Kind::Shell => "shell",
            Kind::Plant => "plant",
            Kind::Corridor => "corridor",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::TreatmentBay => "Treatment Bay",
            Kind::TreatmentRoom => "Treatment Room",
            Kind::Isolation => "Isolation Room",
            Kind::Anteroom => "Anteroom",
            Kind::Washroom => "Washroom",
            Kind::Medication => "Medication Room",
            Kind::CleanUtility => "Clean Utility",
            Kind::SoiledUtility => "Soiled Utility",
            Kind::StaffWork => "Staff Work",
            Kind::Office => "Office",
            Kind::Meeting => "Meeting Room",
            Kind::Storage => "Storage",
            Kind::Alcove => "Alcove",
            Kind::Waiting => "Waiting",
            Kind::Shell => "Shell Space",
            Kind::Plant => "Plant Room",
            Kind::Corridor => "Corridor",
        }
    }

    /// The role the pack gives the kind.
    pub fn role(self) -> &'static str {
        match self {
            Kind::TreatmentBay | Kind::TreatmentRoom | Kind::Isolation => "clinical",
            Kind::Anteroom | Kind::Medication | Kind::CleanUtility | Kind::SoiledUtility => "support",
            Kind::StaffWork | Kind::Storage | Kind::Alcove | Kind::Shell => "support",
            Kind::Washroom | Kind::Plant => "service",
            Kind::Office => "private",
            Kind::Meeting => "shared",
            Kind::Waiting => "public",
            Kind::Corridor => "circulation",
        }
    }
}

/// How two rooms are joined: a room separation line, a single door of a width, or a pair of doors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Means {
    Open,
    Single(i64),
    Pair,
}

#[derive(Clone, Debug)]
pub struct Room {
    /// The node's id, which is also the room's number in the project.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// The room's rectangle in the plan as drawn (before it is turned).
    pub rect: crate::old_generator::house::place::Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    pub means: Means,
}

/// What a plan varies, for the report and the manifest.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    pub service: bool,
    pub office_band: bool,
    /// The core's blocks in order: `T` treatment, `I` isolation, `S` service.
    pub blocks: String,
    pub head_row: bool,
    pub second_end: bool,
}

/// A drawn plan before it is laid out: its rooms with their rectangles, and the links between them.
#[derive(Clone, Debug)]
pub struct Case {
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub width: i64,
    pub depth: i64,
    pub flags: Flags,
}

/// The least area, m2 gross, a medical day department is met at.
const LEAST_AREA: f64 = 400.0;

pub fn least_area() -> f64 {
    LEAST_AREA
}

/// A brief's medical day department: its files, or why there are none.
pub(super) fn build(seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    if area.m2 < LEAST_AREA {
        return Err(refuse(
            "unsatisfiable_area",
            format!(
                "{:.1} m2 is too small for a medical day department: it cannot fit a window band of treatment bays, a patient corridor, a treatment block, an isolation block and a staff corridor. The least a medical-day brief is met at is about {} m2.",
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
        let mut layout = match place::assemble(&case, seed) {
            Ok(layout) => layout,
            Err(e) => {
                remember(&mut why, e);
                continue;
            }
        };
        if let Err(e) = place::orient(&case, &mut layout) {
            remember(&mut why, e);
            continue;
        }
        match emit::verify(&case, &layout, seed, area) {
            Ok(done) => return emit::finish(&case, &layout, done, seed, area),
            Err(e) => why = e,
        }
    }
    Err(refuse(
        "unplaceable_graph",
        format!("no plan fits the medical day pattern at {:.1} m2 (seed {seed}): {why}. Try another seed or area.", area.m2),
    ))
}

/// Keeps the first reason a try failed before any was built.
fn remember(why: &mut String, e: String) {
    if why.starts_with("no try") {
        *why = e;
    }
}
