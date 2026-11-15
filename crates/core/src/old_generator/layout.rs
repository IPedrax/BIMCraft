//! Where the program goes: one rectangle, a corridor strategy, and rooms dealt into bands off that corridor. All in whole
//! millimetres, on wall centre lines, so the same brief gives the same plan on any machine.
//!
//! ```text
//!   +------------------------------+   y = depth
//!   |  north band: rooms, each doored to the corridor
//!   +------------------------------+   y = corridor_top
//!   |  corridor, the whole width   |
//!   +------------------------------+   y = corridor_bottom
//!   |  lobby | south band: rooms   |   (the entry is in the lobby's west wall)
//!   +------------------------------+   y = 0
//! ```

use super::Rng;
use super::openings::{self, SymbolProfile};
use super::program::{Archetype, Program, Spec, program};

pub const EXTERIOR: &str = "Exterior - 300mm";
pub const INTERIOR: &str = "Interior - 100mm";
/// The narrowest room, so a door and a window sit in it clear of its walls.
pub const MIN_CELL_WIDTH: i64 = 2400;
const MIN_DEPTH: i64 = 12_000;
const MAX_DEPTH: i64 = 24_000;
const MAX_OPEN_DEPTH: i64 = 60_000;
const MIN_WIDTH: i64 = 6_000;
const CORRIDOR_WIDTHS: [i64; 4] = [1800, 2000, 2200, 2400];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    North,
    South,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    DoubleLoaded,
    SingleLoaded,
    Offset,
}

impl Strategy {
    pub const ALL: [Strategy; 3] = [Strategy::DoubleLoaded, Strategy::SingleLoaded, Strategy::Offset];

    pub fn name(self) -> &'static str {
        match self {
            Strategy::DoubleLoaded => "double_loaded_corridor",
            Strategy::SingleLoaded => "single_loaded_corridor",
            Strategy::Offset => "offset_corridor",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFamily {
    CorridorBar,
    CentralCommons,
    ClusteredCommons,
    AtriumCommons,
}

impl LayoutFamily {
    pub const ALL: [LayoutFamily; 4] =
        [LayoutFamily::CorridorBar, LayoutFamily::CentralCommons, LayoutFamily::ClusteredCommons, LayoutFamily::AtriumCommons];

    pub fn name(self) -> &'static str {
        match self {
            LayoutFamily::CorridorBar => "corridor_bar",
            LayoutFamily::CentralCommons => "central_commons",
            LayoutFamily::ClusteredCommons => "clustered_commons",
            LayoutFamily::AtriumCommons => "atrium_commons",
        }
    }

    fn target_aspect(self) -> f64 {
        match self {
            LayoutFamily::CorridorBar => 3.0,
            LayoutFamily::CentralCommons => 1.45,
            LayoutFamily::ClusteredCommons => 1.8,
            LayoutFamily::AtriumCommons => 1.25,
        }
    }

    fn max_depth(self) -> i64 {
        match self {
            LayoutFamily::CorridorBar => MAX_DEPTH,
            LayoutFamily::CentralCommons | LayoutFamily::ClusteredCommons | LayoutFamily::AtriumCommons => MAX_OPEN_DEPTH,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryShape {
    Rectangle,
    LShape,
    SteppedBar,
    TShape,
    BarWithWings,
}

impl BoundaryShape {
    pub fn name(self) -> &'static str {
        match self {
            BoundaryShape::Rectangle => "rectangle",
            BoundaryShape::LShape => "l_shape",
            BoundaryShape::SteppedBar => "stepped_bar",
            BoundaryShape::TShape => "t_shape",
            BoundaryShape::BarWithWings => "bar_with_wings",
        }
    }
}

impl Band {
    pub fn name(self) -> &'static str {
        match self {
            Band::North => "north",
            Band::South => "south",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub x0: i64,
    pub x1: i64,
}

impl Span {
    fn width(self) -> i64 {
        self.x1 - self.x0
    }
}

/// A room's rectangle, on wall centre lines.
#[derive(Clone, Debug)]
pub struct Cell {
    pub spec: Spec,
    pub band: Band,
    pub x0: i64,
    pub x1: i64,
    pub y0: i64,
    pub y1: i64,
    /// The y of the wall to the corridor, where its door is, and of the outside wall, where its windows are.
    pub inside_y: i64,
    pub outside_y: i64,
}

impl Cell {
    pub fn middle(&self) -> [i64; 2] {
        [(self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2]
    }

    pub fn area_mm2(&self) -> i64 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub kind: &'static str,
    pub points: Vec<[i64; 2]>,
    pub closed: bool,
}

#[derive(Clone, Debug)]
pub struct Opening {
    pub kind: &'static str,
    pub space: String,
    pub at: [i64; 2],
    /// Where the building is entered from outside.
    pub entry: bool,
    /// What the opening is for: `main_entry`, `back_door`, `service_exit`, `room_access` or `window`.
    pub role: &'static str,
    /// The symbol it is drawn with, one of `openings::DOOR_SYMBOLS` or `openings::WINDOW_SYMBOLS`.
    pub symbol: &'static str,
    pub width_mm: i64,
    pub height_mm: i64,
    pub sill_mm: i64,
    /// How its leaves open, as BIMCraft's `type` command spells it.
    pub operation: &'static str,
    pub flip_hand: bool,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub archetype: Archetype,
    pub seed: u64,
    pub layout_family: LayoutFamily,
    pub layout_strategy: Strategy,
    pub boundary_shape: BoundaryShape,
    pub opening_symbol_profile: SymbolProfile,
    pub requested_m2: f64,
    pub width: i64,
    pub depth: i64,
    pub corridor_bottom: i64,
    pub corridor_top: i64,
    pub boundary: Vec<[i64; 2]>,
    pub program: Program,
    /// The lobby first, then the south band west to east, then the north band.
    pub cells: Vec<Cell>,
    pub walls: Vec<Wall>,
    pub openings: Vec<Opening>,
}

impl Plan {
    /// Every room's `(id, label, point)`, the corridor first, in the order rooms are numbered.
    pub fn room_points(&self) -> Vec<(String, String, [i64; 2])> {
        let corridor = &self.program.corridor;
        let mut rooms = vec![(corridor.id.clone(), corridor.label.clone(), [self.width / 2, self.corridor_mid()])];
        rooms.extend(self.cells.iter().map(|c| (c.spec.id.clone(), c.spec.label.clone(), c.middle())));
        rooms
    }

    fn corridor_mid(&self) -> i64 {
        (self.corridor_bottom + self.corridor_top) / 2
    }

    /// The y of the dimension along the plan's width: the corridor's middle, a little off it.
    pub fn width_dimension_y(&self) -> i64 {
        self.corridor_mid() + 137
    }

    /// The ys a grid line is drawn at beside the plan's edges and its rooms'.
    pub fn guide_ys(&self) -> [i64; 3] {
        [self.corridor_bottom, self.corridor_top, self.corridor_mid()]
    }

    /// Where the building is entered from outside, and the clear width of the wall there.
    pub fn entry_wall(&self) -> ([i64; 2], i64) {
        ([0, self.corridor_mid()], self.corridor_width())
    }

    /// The box of a room by its id, on wall centre lines.
    pub fn rect_of(&self, id: &str) -> Option<[i64; 4]> {
        if id == self.program.corridor.id {
            Some([0, self.corridor_bottom, self.width, self.corridor_top])
        } else {
            self.cells.iter().find(|c| c.spec.id == id).map(|c| [c.x0, c.y0, c.x1, c.y1])
        }
    }

    /// The id of the wall of a given kind that runs from `a` to `b`, when the layout names it (an exterior wall is named by lift).
    pub fn inner_wall_id(&self, kind: &str, a: [i64; 2], b: [i64; 2]) -> Option<String> {
        match kind {
            "corridor" if a[1] == b[1] && a[1] == self.corridor_bottom => Some("wall_corr_south".into()),
            "corridor" if a[1] == b[1] && a[1] == self.corridor_top => Some("wall_corr_north".into()),
            "partition" => self
                .cells
                .iter()
                .find(|c| c.x1 == a[0] && a[0] == b[0] && c.y0 == a[1].min(b[1]) && c.y1 == a[1].max(b[1]))
                .map(|c| format!("wall_part_{}", c.spec.id)),
            _ => None,
        }
    }

    /// The wall a room's door to the corridor is in.
    pub fn corridor_wall_of(&self, space: &str) -> &'static str {
        match self.cells.iter().find(|c| c.spec.id == space).map(|c| c.band) {
            Some(Band::North) => "wall_corr_north",
            _ => "wall_corr_south",
        }
    }

    pub fn corridor_width(&self) -> i64 {
        self.corridor_top - self.corridor_bottom
    }

    pub fn gross_m2(&self) -> f64 {
        mm2_to_m2(self.width * self.depth)
    }

    pub fn corridor_area_mm2(&self) -> i64 {
        self.width * self.corridor_width()
    }

    /// The cells of one band, west to east.
    pub fn band(&self, band: Band) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(move |c| c.band == band)
    }
}

pub fn mm2_to_m2(mm2: i64) -> f64 {
    ((mm2 as f64) / 10_000.0).round() / 100.0
}

fn round_to_100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

/// A plan for the brief, or why none fits.
pub fn plan(archetype: Archetype, area_m2: f64, seed: u64) -> Result<Plan, String> {
    if archetype == Archetype::House {
        return Err("a house is laid out graph first, by `house`, not by a corridor plan".into());
    }
    if archetype == Archetype::StudioOffice {
        return Err("a studio office is laid out graph first, by `studio`, not by a corridor plan".into());
    }
    if archetype == Archetype::OfficePattern {
        return Err("an office pattern is laid out graph first, by `pattern`, not by a corridor plan".into());
    }
    if archetype == Archetype::MedicalDay {
        return Err("a medical day department is laid out graph first, by `medical`, not by a corridor plan".into());
    }
    // The seed's own family and strategy first; a brief they can't fit is tried with the others, in a fixed order, so a brief
    // that is met today keeps its plan and one that is not may still find a layout that fits it.
    let own = (layout_family(seed), strategy(seed));
    let first = plan_with(archetype, area_m2, seed, own.0, own.1);
    if first.is_ok() {
        return first;
    }
    for family in LayoutFamily::ALL {
        for strat in Strategy::ALL {
            if (family, strat) != own
                && let Ok(found) = plan_with(archetype, area_m2, seed, family, strat)
            {
                return Ok(found);
            }
        }
    }
    // Nothing fits the full program: the lean one, with only the rooms an office needs, is tried the same way.
    if archetype == Archetype::Office {
        for family in LayoutFamily::ALL {
            for strat in Strategy::ALL {
                if let Ok(found) = plan_program(archetype, area_m2, seed, family, strat, true) {
                    return Ok(found);
                }
            }
        }
    }
    first
}

fn plan_with(
    archetype: Archetype,
    area_m2: f64,
    seed: u64,
    layout_family: LayoutFamily,
    layout_strategy: Strategy,
) -> Result<Plan, String> {
    plan_program(archetype, area_m2, seed, layout_family, layout_strategy, false)
}

fn plan_program(
    archetype: Archetype,
    area_m2: f64,
    seed: u64,
    layout_family: LayoutFamily,
    layout_strategy: Strategy,
    lean: bool,
) -> Result<Plan, String> {
    let area_mm2 = area_m2 * 1.0e6;
    let depth = round_to_100((area_mm2 / layout_family.target_aspect()).sqrt()).clamp(MIN_DEPTH, layout_family.max_depth());
    let width = round_to_100(area_mm2 / depth as f64);
    if width < MIN_WIDTH {
        return Err(format!("a plan {depth} mm deep and {width} mm long is narrower than the {MIN_WIDTH} mm a corridor and a room need"));
    }
    let mut program = program(archetype, area_m2, seed);
    if lean {
        program = super::program::lean(program);
    }
    let mut rng = Rng::new(seed, 0x1A7_0001);
    let boundary_shape = boundary_shape(seed);
    let corridor_width = corridor_width(layout_family, depth, &mut rng);
    let corridor_bottom = corridor_bottom(layout_family, layout_strategy, depth, corridor_width);
    let corridor_top = corridor_bottom + corridor_width;

    // The rooms in an order of the seed's, each to the band with less of the program in it so far; the lobby is the south band's first.
    let mut rooms = program.cells.clone();
    rng.shuffle(&mut rooms);
    let (mut south, mut north) = (vec![program.lobby.clone()], Vec::new());
    let south_depth = corridor_bottom.max(1);
    let north_depth = (depth - corridor_top).max(1);
    let (mut south_target, mut north_target) = (least_cell_width(&program.lobby, south_depth), 0_i64);
    for room in rooms {
        if north_target <= south_target {
            north_target += least_cell_width(&room, north_depth);
            north.push(room);
        } else {
            south_target += least_cell_width(&room, south_depth);
            south.push(room);
        }
    }

    let (south_span, north_span) = spans(boundary_shape, width, corridor_bottom, depth - corridor_top, &south, &north);
    let mut cells = Vec::new();
    if layout_family == LayoutFamily::CorridorBar && layout_strategy == Strategy::SingleLoaded {
        south.extend(north);
        let (_, north_span) = spans(boundary_shape, width, 0, depth - corridor_top, &[], &south);
        cells.extend(deal(Band::North, south, depth - corridor_top, north_span, corridor_top, depth)?);
    } else {
        for (band, specs, depth_of_band, span, y0, y1) in [
            (Band::South, south, corridor_bottom, south_span, 0, corridor_bottom),
            (Band::North, north, depth - corridor_top, north_span, corridor_top, depth),
        ] {
            cells.extend(deal(band, specs, depth_of_band, span, y0, y1)?);
        }
    }
    let boundary = boundary(width, depth, corridor_bottom, corridor_top, &cells);
    let walls = walls(width, corridor_bottom, corridor_top, &boundary, &cells);
    let opening_symbol_profile = openings::profile(archetype, seed);
    let openings =
        openings::place(opening_symbol_profile, seed, &cells, [0, (corridor_bottom + corridor_top) / 2], corridor_top - corridor_bottom);
    Ok(Plan {
        archetype,
        seed,
        layout_family,
        layout_strategy,
        boundary_shape,
        opening_symbol_profile,
        requested_m2: area_m2,
        width,
        depth,
        corridor_bottom,
        corridor_top,
        boundary,
        program,
        cells,
        walls,
        openings,
    })
}

fn layout_family(seed: u64) -> LayoutFamily {
    match (seed / 3) % 4 {
        0 => LayoutFamily::CorridorBar,
        1 => LayoutFamily::CentralCommons,
        2 => LayoutFamily::ClusteredCommons,
        _ => LayoutFamily::AtriumCommons,
    }
}

fn strategy(seed: u64) -> Strategy {
    match seed % 3 {
        0 => Strategy::DoubleLoaded,
        1 => Strategy::SingleLoaded,
        _ => Strategy::Offset,
    }
}

fn corridor_width(family: LayoutFamily, depth: i64, rng: &mut Rng) -> i64 {
    match family {
        LayoutFamily::CorridorBar => CORRIDOR_WIDTHS[rng.below(CORRIDOR_WIDTHS.len() as u64) as usize],
        LayoutFamily::CentralCommons => round_to_100(depth as f64 * 0.35).clamp(4_800, 14_000),
        LayoutFamily::ClusteredCommons => round_to_100(depth as f64 * 0.28).clamp(4_200, 12_000),
        LayoutFamily::AtriumCommons => round_to_100(depth as f64 * 0.45).clamp(6_000, 18_000),
    }
}

fn corridor_bottom(family: LayoutFamily, strategy: Strategy, depth: i64, corridor_width: i64) -> i64 {
    match family {
        LayoutFamily::CentralCommons => (depth - corridor_width) / 2 / 100 * 100,
        LayoutFamily::ClusteredCommons => (depth * 42 / 100 - corridor_width / 2) / 100 * 100,
        LayoutFamily::AtriumCommons => (depth - corridor_width) / 2 / 100 * 100,
        LayoutFamily::CorridorBar => match strategy {
            Strategy::DoubleLoaded => (depth - corridor_width) / 2 / 100 * 100,
            Strategy::SingleLoaded => 0,
            Strategy::Offset => (depth * 35 / 100 - corridor_width / 2) / 100 * 100,
        },
    }
}

fn boundary_shape(seed: u64) -> BoundaryShape {
    match seed % 5 {
        0 => BoundaryShape::Rectangle,
        1 => BoundaryShape::LShape,
        2 => BoundaryShape::SteppedBar,
        3 => BoundaryShape::TShape,
        _ => BoundaryShape::BarWithWings,
    }
}

fn least_cell_width(spec: &Spec, band_depth: i64) -> i64 {
    let for_area = (i64::from(spec.min_m2) * 1_000_000 + band_depth - 1) / band_depth.max(1);
    MIN_CELL_WIDTH.max((for_area + 99) / 100 * 100)
}

fn least_width(specs: &[Spec], band_depth: i64) -> i64 {
    specs.iter().map(|s| least_cell_width(s, band_depth)).sum()
}

fn inset(width: i64, band_depth: i64, specs: &[Spec]) -> i64 {
    let spare = (width - least_width(specs, band_depth)).max(0);
    if spare < MIN_CELL_WIDTH * 2 { 0 } else { (width / 5).clamp(MIN_CELL_WIDTH, spare / 2) / 100 * 100 }
}

fn spans(shape: BoundaryShape, width: i64, south_depth: i64, north_depth: i64, south: &[Spec], north: &[Spec]) -> (Span, Span) {
    let full = Span { x0: 0, x1: width };
    let south_inset = inset(width, south_depth, south);
    let north_inset = inset(width, north_depth, north);
    match shape {
        BoundaryShape::Rectangle => (full, full),
        BoundaryShape::LShape => (full, Span { x0: 0, x1: width - north_inset }),
        BoundaryShape::SteppedBar => (Span { x0: 0, x1: width - south_inset }, Span { x0: north_inset, x1: width }),
        BoundaryShape::TShape => {
            (Span { x0: south_inset / 2, x1: width - south_inset / 2 }, Span { x0: north_inset / 2, x1: width - north_inset / 2 })
        }
        BoundaryShape::BarWithWings => (Span { x0: south_inset, x1: width }, Span { x0: 0, x1: width - north_inset }),
    }
}

/// A band's rooms along its length: each as wide as its least area asks, the rest shared out by what they would like.
fn deal(band: Band, specs: Vec<Spec>, band_depth: i64, span: Span, y0: i64, y1: i64) -> Result<Vec<Cell>, String> {
    let length = span.width();
    let least: Vec<i64> = specs.iter().map(|s| least_cell_width(s, band_depth)).collect();
    let needed: i64 = least.iter().sum();
    if needed > length {
        return Err(format!("the {} band needs {needed} mm of rooms for its minimum program and has {length} mm", band.name()));
    }
    let (spare, wanted) = (length - needed, specs.iter().map(|s| i64::from(s.target_m2)).sum::<i64>().max(1));
    let mut widths: Vec<i64> = specs.iter().zip(&least).map(|(s, l)| l + spare * i64::from(s.target_m2) / wanted / 100 * 100).collect();
    let left = length - widths.iter().sum::<i64>();
    let widest = specs.iter().enumerate().fold(0, |best, (i, s)| if s.target_m2 > specs[best].target_m2 { i } else { best });
    if let Some(w) = widths.get_mut(widest) {
        *w += left;
    }
    let mut x = span.x0;
    Ok(specs
        .into_iter()
        .zip(widths)
        .map(|(spec, w)| {
            let (inside_y, outside_y) = if band == Band::North { (y0, y1) } else { (y1, y0) };
            let cell = Cell { spec, band, x0: x, x1: x + w, y0, y1, inside_y, outside_y };
            x += w;
            cell
        })
        .collect())
}

fn layer(cells: &[Cell], band: Band, y0: i64, y1: i64) -> Option<[i64; 4]> {
    if y1 <= y0 {
        return None;
    }
    let mut xs = cells.iter().filter(|c| c.band == band).flat_map(|c| [c.x0, c.x1]);
    let first = xs.next()?;
    let (mut x0, mut x1) = (first, first);
    for x in xs {
        x0 = x0.min(x);
        x1 = x1.max(x);
    }
    Some([x0, y0, x1, y1])
}

fn boundary(width: i64, depth: i64, bottom: i64, top: i64, cells: &[Cell]) -> Vec<[i64; 2]> {
    let mut rects = Vec::new();
    if let Some(south) = layer(cells, Band::South, 0, bottom) {
        rects.push(south);
    }
    rects.push([0, bottom, width, top]);
    if let Some(north) = layer(cells, Band::North, top, depth) {
        rects.push(north);
    }
    let mut points = Vec::new();
    if let Some(first) = rects.first() {
        points.push([first[0], first[1]]);
        points.push([first[2], first[1]]);
    }
    for (i, rect) in rects.iter().enumerate() {
        points.push([rect[2], rect[3]]);
        if let Some(next) = rects.get(i + 1) {
            points.push([next[2], rect[3]]);
        }
    }
    if let Some(last) = rects.last() {
        points.push([last[0], last[3]]);
    }
    for (i, rect) in rects.iter().enumerate().rev() {
        points.push([rect[0], rect[1]]);
        if i > 0 {
            points.push([rects[i - 1][0], rect[1]]);
        }
    }
    let mut simple = Vec::new();
    for point in points {
        if simple.last().is_none_or(|last| *last != point) {
            simple.push(point);
        }
    }
    while simple.len() >= 3 {
        let n = simple.len();
        let (a, b, c) = (simple[n - 2], simple[n - 1], simple[0]);
        if (a[0] == b[0] && b[0] == c[0]) || (a[1] == b[1] && b[1] == c[1]) {
            simple.pop();
        } else {
            break;
        }
    }
    simplify(simple)
}

fn simplify(mut points: Vec<[i64; 2]>) -> Vec<[i64; 2]> {
    loop {
        let mut changed = false;
        let mut out = Vec::new();
        for i in 0..points.len() {
            let a = points[(i + points.len() - 1) % points.len()];
            let b = points[i];
            let c = points[(i + 1) % points.len()];
            if (a[0] == b[0] && b[0] == c[0]) || (a[1] == b[1] && b[1] == c[1]) {
                changed = true;
            } else {
                out.push(b);
            }
        }
        points = out;
        if !changed {
            return points;
        }
    }
}

fn walls(width: i64, bottom: i64, top: i64, boundary: &[[i64; 2]], cells: &[Cell]) -> Vec<Wall> {
    let mut walls = vec![
        Wall { kind: "exterior", points: boundary.to_vec(), closed: true },
        Wall { kind: "corridor", points: vec![[0, bottom], [width, bottom]], closed: false },
        Wall { kind: "corridor", points: vec![[0, top], [width, top]], closed: false },
    ];
    for (i, c) in cells.iter().enumerate() {
        let same_band_after = cells.iter().skip(i + 1).any(|n| n.band == c.band && n.x0 == c.x1 && n.y0 == c.y0 && n.y1 == c.y1);
        if !same_band_after {
            continue;
        }
        walls.push(Wall { kind: "partition", points: vec![[c.x1, c.y0], [c.x1, c.y1]], closed: false });
    }
    walls
}
