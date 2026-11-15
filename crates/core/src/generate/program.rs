//! The archetypes a brief names, and the roles of spaces the rules read.

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

    pub(crate) fn salt(self) -> u64 {
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
