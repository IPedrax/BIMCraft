//! What a building must hold: the archetypes' programs, as data a layout deals out. A space has a kind, a role the rules
//! read, the least area it needs and the area it would like. The archetypes and roles are the shared ones in
//! `crate::generate::program`.

pub use crate::generate::program::*;

use super::Rng;

#[derive(Clone, Debug)]
pub struct Spec {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub role: Role,
    pub min_m2: u32,
    pub target_m2: u32,
    /// Whether it wants a window in the outside wall.
    pub window: bool,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub lobby: Spec,
    pub corridor: Spec,
    pub cells: Vec<Spec>,
}

impl Program {
    /// Every space, the lobby and the corridor first.
    pub fn all(&self) -> impl Iterator<Item = &Spec> {
        [&self.lobby, &self.corridor].into_iter().chain(&self.cells)
    }
}

fn spec(kind: &'static str, label: &str, role: Role, min_m2: u32, target_m2: u32, window: bool) -> Spec {
    Spec { id: kind.to_string(), kind, label: label.to_string(), role, min_m2, target_m2, window }
}

/// `n` spaces of one kind, numbered when there are several.
fn several(out: &mut Vec<Spec>, n: u64, one: Spec) {
    for i in 1..=n {
        let (id, label) =
            if n == 1 { (one.id.clone(), one.label.clone()) } else { (format!("{}_{i}", one.id), format!("{} {i}", one.label)) };
        out.push(Spec { id, label, ..one.clone() });
    }
}

/// The program of a building of about `area_m2` square metres: more rooms for more floor, a few of them by the seed.
pub fn program(archetype: Archetype, area_m2: f64, seed: u64) -> Program {
    let mut rng = Rng::new(seed, archetype.salt());
    let a = area_m2.clamp(0.0, 1.0e6) as u64;
    let mut cells = Vec::new();
    let lobby = match archetype {
        Archetype::Office => spec("entry_lobby", "Lobby", Role::Public, 16, 40, true),
        Archetype::Clinic => spec("entry_lobby", "Entry Lobby", Role::Public, 16, 30, true),
        Archetype::Hospital => spec("entry_lobby", "Hospital Entry Lobby", Role::Public, 24, 70, true),
        Archetype::House | Archetype::StudioOffice | Archetype::OfficePattern | Archetype::MedicalDay => {
            spec("entry_lobby", "Entry", Role::Public, 4, 6, true)
        }
    };
    let corridor = spec("corridor", "Corridor", Role::Circulation, 0, 0, false);
    match archetype {
        Archetype::Office => {
            several(&mut cells, 1 + a / 1500, spec("open_office", "Open Office", Role::Shared, 36, 150, true));
            several(&mut cells, 1 + a / 600 + rng.below(2), spec("conference", "Conference", Role::Shared, 14, 24, true));
            several(&mut cells, 2 + a / 200 + rng.below(3), spec("private_office", "Private Office", Role::Private, 9, 16, true));
            several(&mut cells, rng.below(2), spec("focus_room", "Focus Room", Role::Private, 5, 8, true));
            several(&mut cells, 1 + a / 700, spec("restroom", "Restroom", Role::Service, 8, 14, false));
            several(&mut cells, 1, spec("it_storage", "IT and Storage", Role::Support, 6, 10, false));
            several(&mut cells, 1 + a / 1500, spec("pantry", "Pantry", Role::Support, 8, 16, true));
        }
        Archetype::Clinic => {
            several(&mut cells, 1 + a / 1200, spec("waiting", "Waiting", Role::Public, 18, 40, true));
            several(&mut cells, 2 + a / 150 + rng.below(3), spec("exam", "Exam Room", Role::Clinical, 10, 14, true));
            several(&mut cells, rng.below(2), spec("procedure", "Procedure Room", Role::Clinical, 14, 20, true));
            several(&mut cells, 1 + a / 500, spec("restroom", "Restroom", Role::Service, 6, 8, false));
            several(&mut cells, 1, spec("staff_room", "Staff Room", Role::Support, 10, 16, true));
            several(&mut cells, 1, spec("nurse_station", "Nurse Station", Role::Support, 8, 12, true));
            several(&mut cells, 1, spec("clean_storage", "Clean Storage", Role::Support, 8, 12, false));
        }
        // A house has no corridor program: `house` samples its graph from the rule pack.
        Archetype::House | Archetype::StudioOffice | Archetype::OfficePattern | Archetype::MedicalDay => {}
        Archetype::Hospital => {
            several(&mut cells, 1 + a / 2500, spec("waiting_reception", "Waiting and Reception", Role::Public, 30, 80, true));
            several(&mut cells, 4 + a / 1200 + rng.below(2), spec("exam", "Exam Room", Role::Clinical, 10, 14, true));
            several(&mut cells, 2 + a / 1800, spec("patient_room", "Patient Room", Role::Clinical, 12, 18, true));
            several(&mut cells, 1 + rng.below(2), spec("treatment_bay", "Treatment Bay", Role::Clinical, 12, 16, true));
            several(&mut cells, 1, spec("nurse_station", "Nurse Station", Role::Support, 12, 18, true));
            several(&mut cells, 1, spec("staff_support", "Staff Support", Role::Support, 12, 18, true));
            several(&mut cells, 1, spec("clean_utility", "Clean Utility", Role::Support, 8, 12, false));
            several(&mut cells, 1, spec("dirty_utility", "Dirty Utility", Role::Service, 8, 12, false));
            several(&mut cells, 1, spec("med_supply", "Medication and Supply", Role::Support, 10, 16, false));
            several(&mut cells, 1, spec("equipment_storage", "Equipment Storage", Role::Support, 10, 16, false));
            several(&mut cells, 2 + a / 2500, spec("restroom", "Restroom", Role::Service, 6, 8, false));
            several(&mut cells, 1, spec("imaging_diagnostic", "Imaging and Diagnostic", Role::Clinical, 18, 30, false));
            several(&mut cells, rng.below(2), spec("procedure_lab", "Procedure Lab", Role::Clinical, 16, 26, false));
        }
    }
    Program { lobby, corridor, cells }
}

/// The least an office needs: one open office, one conference room, two private offices, a restroom and IT storage.
pub fn lean(mut program: Program) -> Program {
    let (mut conference, mut private) = (0, 0);
    program.cells.retain(|c| match c.kind {
        "focus_room" | "pantry" => false,
        "conference" => {
            conference += 1;
            conference <= 1
        }
        "private_office" => {
            private += 1;
            private <= 2
        }
        _ => true,
    });
    program
}
