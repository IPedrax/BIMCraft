//! The symbols a building's doors and windows are drawn with. A seeded profile says which door and window symbols a building
//! uses (a double main entry or a single one, a back door or a service exit, storefront or ribbon or slot windows), and each
//! opening takes a symbol by the profile and the room it is in. The symbol fixes what BIMCraft's `type` command is given (a
//! width, a height, a sill and how the leaves operate), so the script draws it, and the graphs export it as a label.
//!
//! Everything is a function of the seed and the plan, in whole millimetres, so the same brief gives the same openings.

use std::collections::BTreeMap;

use super::Rng;
use super::layout::{Cell, Opening};
use super::program::{Archetype, Role};

/// One window for each this much of outside wall, for the symbols that repeat along it.
const WINDOW_SPACING: i64 = 4000;
/// A double leaf wants a room this wide on its corridor wall, clear of its walls.
const DOUBLE_LEAF_ROOM: i64 = 3600;

/// Which door and window symbols a building draws, chosen by the seed and archetype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolProfile {
    /// A double main entry and a back door, single leaves between rooms, punched windows and slots in the support rooms.
    ClassicPunched,
    /// A tall double storefront entry, storefront glass in public rooms and ribbon windows in shared ones.
    StorefrontRibbon,
    /// A single entry, a back door and a service exit, secure and service doors, clerestories in the back of house.
    ServiceSecure,
    /// A double entry and a service exit, narrow slots in private and clinical rooms, clerestories and ribbons elsewhere.
    SlotClerestory,
}

impl SymbolProfile {
    pub const ALL: [SymbolProfile; 4] =
        [SymbolProfile::ClassicPunched, SymbolProfile::StorefrontRibbon, SymbolProfile::ServiceSecure, SymbolProfile::SlotClerestory];

    pub fn name(self) -> &'static str {
        match self {
            SymbolProfile::ClassicPunched => "classic_punched",
            SymbolProfile::StorefrontRibbon => "storefront_ribbon",
            SymbolProfile::ServiceSecure => "service_secure",
            SymbolProfile::SlotClerestory => "slot_clerestory",
        }
    }
}

fn ordinal(archetype: Archetype) -> u64 {
    match archetype {
        Archetype::Office => 0,
        Archetype::Clinic => 1,
        Archetype::Hospital => 2,
        Archetype::House => 3,
        Archetype::StudioOffice => 4,
        Archetype::OfficePattern => 5,
        Archetype::MedicalDay => 6,
    }
}

/// The profile of a brief: the same one for the same archetype and seed.
pub fn profile(archetype: Archetype, seed: u64) -> SymbolProfile {
    let mut rng = Rng::new(seed.wrapping_add(ordinal(archetype)), 0x0DE5_0001);
    SymbolProfile::ALL[rng.below(SymbolProfile::ALL.len() as u64) as usize]
}

/// What a symbol is as BIMCraft draws it.
#[derive(Clone, Copy, Debug)]
struct Shape {
    symbol: &'static str,
    width: i64,
    height: i64,
    sill: i64,
    operation: &'static str,
}

const fn shape(symbol: &'static str, width: i64, height: i64, sill: i64, operation: &'static str) -> Shape {
    Shape { symbol, width, height, sill, operation }
}

const SINGLE_SWING: Shape = shape("single_swing", 900, 2100, 0, "swing");
const DOUBLE_SWING: Shape = shape("double_swing", 1600, 2100, 0, "double-swing");
const SERVICE: Shape = shape("service", 1100, 2100, 0, "swing");
const SECURE: Shape = shape("secure", 1000, 2100, 0, "swing");
const BACK_DOOR: Shape = shape("back_door", 1000, 2100, 0, "swing");
const SERVICE_EXIT: Shape = shape("service_exit", 1200, 2100, 0, "swing");

const PUNCHED: Shape = shape("punched", 1200, 1500, 900, "swing");
const NARROW_SLOT: Shape = shape("narrow_slot", 600, 1800, 600, "fixed");
const CLERESTORY: Shape = shape("clerestory", 1500, 600, 2100, "fixed");
const RIBBON: Shape = shape("ribbon", 1200, 1200, 900, "sliding");
const STOREFRONT: Shape = shape("storefront", 1600, 2400, 0, "fixed");

/// The door symbols and window symbols the generator can give an opening.
pub const DOOR_SYMBOLS: [&str; 7] = ["main_entry", "back_door", "service_exit", "single_swing", "double_swing", "service", "secure"];
pub const WINDOW_SYMBOLS: [&str; 5] = ["punched", "ribbon", "storefront", "clerestory", "narrow_slot"];

fn shaped(kind: &'static str, role: &'static str, space: &str, at: [i64; 2], s: Shape, flip_hand: bool) -> Opening {
    Opening {
        kind,
        space: space.to_string(),
        at,
        entry: role == "main_entry",
        role,
        symbol: s.symbol,
        width_mm: s.width,
        height_mm: s.height,
        sill_mm: s.sill,
        operation: s.operation,
        flip_hand,
    }
}

/// The way in: wide as the profile wants, and clear of the corridor's ends.
fn main_entry(profile: SymbolProfile, space: &str, at: [i64; 2], corridor: i64) -> Opening {
    let (width, height, operation) = match profile {
        SymbolProfile::ClassicPunched => (1600, 2100, "double-swing"),
        SymbolProfile::StorefrontRibbon => (1800, 2400, "double-swing"),
        SymbolProfile::ServiceSecure => (1000, 2100, "swing"),
        SymbolProfile::SlotClerestory => (1400, 2100, "double-swing"),
    };
    let width = width.min((corridor - 400) / 100 * 100).max(1000);
    shaped("door", "main_entry", space, at, shape("main_entry", width, height, 0, operation), false)
}

/// A room's door to the corridor, by what the room is and how wide it is.
fn room_door(profile: SymbolProfile, kind: &str, role: Role, width: i64) -> Shape {
    let wide = width >= DOUBLE_LEAF_ROOM;
    let double = match role {
        Role::Public => true,
        Role::Shared => kind == "open_office" || (profile == SymbolProfile::StorefrontRibbon && kind == "conference"),
        // A bed goes through a patient room, a treatment bay or a procedure room; an exam room has an ordinary door.
        Role::Clinical => {
            matches!(kind, "patient_room" | "treatment_bay" | "procedure")
                && matches!(profile, SymbolProfile::StorefrontRibbon | SymbolProfile::SlotClerestory)
        }
        _ => false,
    };
    match kind {
        "it_storage" | "med_supply" | "equipment_storage" | "imaging_diagnostic" | "procedure_lab" => SECURE,
        "restroom" | "clean_storage" | "clean_utility" | "dirty_utility" | "pantry" | "staff_room" | "staff_support" | "nurse_station" => {
            SERVICE
        }
        _ if double && wide => DOUBLE_SWING,
        _ => SINGLE_SWING,
    }
}

/// A room's window symbol, by the profile and the room's role.
fn window_shape(profile: SymbolProfile, role: Role) -> Shape {
    match (profile, role) {
        (SymbolProfile::ClassicPunched, Role::Support | Role::Service) => NARROW_SLOT,
        (SymbolProfile::ClassicPunched, _) => PUNCHED,
        (SymbolProfile::StorefrontRibbon, Role::Public) => STOREFRONT,
        (SymbolProfile::StorefrontRibbon, Role::Shared) => RIBBON,
        (SymbolProfile::StorefrontRibbon, _) => PUNCHED,
        (SymbolProfile::ServiceSecure, Role::Public) => RIBBON,
        (SymbolProfile::ServiceSecure, Role::Support | Role::Service) => CLERESTORY,
        (SymbolProfile::ServiceSecure, _) => PUNCHED,
        (SymbolProfile::SlotClerestory, Role::Public) => RIBBON,
        (SymbolProfile::SlotClerestory, Role::Shared | Role::Support | Role::Service) => CLERESTORY,
        (SymbolProfile::SlotClerestory, _) => NARROW_SLOT,
    }
}

/// How many windows of this symbol a room of `width` takes, and how wide each is.
fn window_run(s: Shape, width: i64) -> (i64, i64) {
    match s.symbol {
        "ribbon" => {
            let n = 1 + i64::from(width >= 9000);
            (n, (width / n - 1200).clamp(1200, 4800) / 100 * 100)
        }
        "storefront" => {
            let n = 1 + i64::from(width >= 9000);
            (n, (width / n - 800).clamp(1200, 3600) / 100 * 100)
        }
        "narrow_slot" => ((width / 3000).max(1), s.width),
        _ => ((width / WINDOW_SPACING).max(1), s.width),
    }
}

/// The exits to outside from back of house: how many the profile wants, and of what symbol.
fn exits(profile: SymbolProfile) -> &'static [(&'static str, Shape)] {
    match profile {
        SymbolProfile::ClassicPunched | SymbolProfile::StorefrontRibbon => &[("back_door", BACK_DOOR)],
        SymbolProfile::ServiceSecure => &[("back_door", BACK_DOOR), ("service_exit", SERVICE_EXIT)],
        SymbolProfile::SlotClerestory => &[("service_exit", SERVICE_EXIT)],
    }
}

/// The way in, a door from every room to the corridor, an exit or two from back of house, and windows for the rooms that
/// want them.
pub fn place(profile: SymbolProfile, seed: u64, cells: &[Cell], entry: [i64; 2], entry_clear: i64) -> Vec<Opening> {
    let mut out = Vec::new();
    if let Some(lobby) = cells.first() {
        out.push(main_entry(profile, &lobby.spec.id, entry, entry_clear));
    }
    for (i, c) in cells.iter().enumerate() {
        let (inside, outside) = (c.inside_y, c.outside_y);
        let door = room_door(profile, c.spec.kind, c.spec.role, c.x1 - c.x0);
        out.push(shaped("door", "room_access", &c.spec.id, [c.middle()[0], inside], door, i % 2 == 1));
        if c.spec.window {
            let width = c.x1 - c.x0;
            let s = window_shape(profile, c.spec.role);
            let (n, each) = window_run(s, width);
            for k in 0..n {
                let at = [c.x0 + width * (2 * k + 1) / (2 * n), outside];
                out.push(shaped("window", "window", &c.spec.id, at, Shape { width: each, ..s }, false));
            }
        }
    }
    // Back of house: a service or support room with no window of its own, in the order the seed gives.
    let mut rng = Rng::new(seed, 0x0DE5_0002);
    let mut back: Vec<&Cell> =
        cells.iter().skip(1).filter(|c| !c.spec.window && matches!(c.spec.role, Role::Service | Role::Support)).collect();
    rng.shuffle(&mut back);
    for ((role, s), c) in exits(profile).iter().zip(back) {
        out.push(shaped("door", role, &c.spec.id, [c.middle()[0], c.outside_y], *s, true));
    }
    out
}

/// A symbol's words as a type's name starts: `double_swing` is `Double Swing`.
fn title(symbol: &str) -> String {
    symbol
        .split('_')
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Opening {
    /// The name of the BIMCraft type the script makes for this opening: its symbol, width and height.
    pub fn type_name(&self) -> String {
        format!("{} {} x {}", title(self.symbol), self.width_mm, self.height_mm)
    }
}

/// How many of each symbol, by kind, in a list of openings: door symbols and window symbols, in name order.
pub fn symbol_counts<'a>(openings: impl IntoIterator<Item = (&'a str, &'a str)>) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let (mut doors, mut windows) = (BTreeMap::new(), BTreeMap::new());
    for (kind, symbol) in openings {
        *(if kind == "door" { &mut doors } else { &mut windows }).entry(symbol.to_string()).or_insert(0) += 1;
    }
    (doors, windows)
}
