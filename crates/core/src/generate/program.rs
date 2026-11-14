//! What a building must hold: the archetypes' programs, as data a layout deals out. A space has a kind, a role the rules
//! read, the least area it needs and the area it would like.

use super::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archetype {
    Office,
    Clinic,
    Hospital,
    /// Graph first: a house is not laid out by `layout` and `program` but by `house`.
    House,
    /// Graph first as well: a reception, a corridor and a passage of WCs, and a studio, by `studio`.
    StudioOffice,
    /// Graph first too: a lobby, a corridor with rooms off it and an open office at the far end, by `pattern`.
    OfficePattern,
    /// Graph first too: a medical day department with no entry, by `medical`.
    MedicalDay,
}

impl Archetype {
    /// Whether the archetype's plan draws gridlines unless told otherwise: all but the medical-day department.
    pub fn draws_grid(self) -> bool {
        self != Archetype::MedicalDay
    }

    pub const ALL: [Archetype; 7] = [
        Archetype::Office,
        Archetype::Clinic,
        Archetype::Hospital,
        Archetype::House,
        Archetype::StudioOffice,
        Archetype::OfficePattern,
        Archetype::MedicalDay,
    ];

    pub fn parse(name: &str) -> Option<Archetype> {
        Archetype::ALL.into_iter().find(|a| a.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Archetype::Office => "office",
            Archetype::Clinic => "clinic",
            Archetype::Hospital => "hospital",
            Archetype::House => "house",
            Archetype::StudioOffice => "studio-office",
            Archetype::OfficePattern => "office-pattern",
            Archetype::MedicalDay => "medical-day",
        }
    }

    pub fn names() -> String {
        Archetype::ALL.map(Archetype::name).join(", ")
    }

    /// The kind of building a plan of this archetype is, as its script's `building_type` drawing property says it.
    pub fn building_type(self) -> &'static str {
        match self {
            Archetype::Office | Archetype::StudioOffice | Archetype::OfficePattern => "office",
            Archetype::Clinic => "clinic",
            Archetype::Hospital | Archetype::MedicalDay => "hospital",
            Archetype::House => "residential",
        }
    }

    fn salt(self) -> u64 {
        match self {
            Archetype::Office => 0x000F_F1CE,
            Archetype::Clinic => 0x00C1_191C,
            Archetype::Hospital => 0x0000_0A51_17A1,
            Archetype::House => 0x0000_0A05_E000,
            Archetype::StudioOffice => 0x0057_0D10_0FF1,
            Archetype::OfficePattern => 0x0000_0FF1_CE9A,
            Archetype::MedicalDay => 0x0000_0ED1_DA7E,
        }
    }
}

/// What a space is to the rules: who may door into whom is a matter of roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Private,
    Circulation,
    Shared,
    Public,
    Service,
    Clinical,
    Support,
}

impl Role {
    pub const ALL: [Role; 7] = [Role::Private, Role::Circulation, Role::Shared, Role::Public, Role::Service, Role::Clinical, Role::Support];

    pub fn parse(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == name)
    }

    pub fn names() -> String {
        Role::ALL.map(Role::name).join(", ")
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Private => "private",
            Role::Circulation => "circulation",
            Role::Shared => "shared",
            Role::Public => "public",
            Role::Service => "service",
            Role::Clinical => "clinical",
            Role::Support => "support",
        }
    }
}

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
