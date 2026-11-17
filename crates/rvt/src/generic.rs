//! The other placed elements (fittings, fixtures, equipment, terminals, generic models): each comes in as a box-shaped component of
//! a family of its category.
//!
//! The file's record says where an element is and the box that holds it, in project axes; its shape and its connectors are not read
//! here. A component is of a family of the category's kind (`Category::PipeFitting`, `Plumbing`, `Generic`...): a box whose width,
//! depth and height, and where its middle sits from the family's origin (`Offset X`, `Offset Y`), are instance parameters. The family
//! is the Revit family the instance's second reference list names (`openings::family_of`), else the one its type's other instances
//! name, else one family of the category; its type is the Revit type its first list names (`openings::symbol_of`), and `Imported`
//! where the reader does not know which. From 2025 a type has no record of its own, only a name entry, which is why both lists are
//! read.
//!
//! A component comes in as **its type's box, turned as placed** where the file says enough: its placement in its data
//! (`placement::transform`), with one axis upright (a light hung upside down, or one on a wall, is turned to stand upright, which
//! leaves its box where it is), and its type's box in the family's own axes. That box is the type's symbol's (`Facts::symbol_boxes`,
//! before 2025); else the instance's own record box taken back through its placement, exact where every axis of the placement lies
//! along the project's; else the box the type's instances that stand so agree on. Placed, it must give the record's box to a
//! millimetre, which checks the lot: then the component stands at the placement's origin, turned by its angle. Otherwise it comes in
//! as **the record's box**, unturned: its middle in plan, its bottom on the level at or below it, and its width, depth and height the
//! box's. A turned element's record box is the one round it, so that box is the element's envelope. The notes say which, and why.
//!
//! A category the table has no name for (`inventory::category_name`) is read the same way, its elements as boxes of a family named
//! after its id (`Category -2008123`): the reader does not guess what Revit calls it, and the note says its category has no name here.
//! Such a category often holds an element's companion, a second element with the same box as the pipe, fitting or duct it goes with;
//! one whose box (to the micron) is the box of an element of a category with a name is that companion, not a second body, and is left
//! out and listed.
//!
//! A box stands for an element as its envelope, which is a fair stand-in for a fitting or a piece of equipment and none for what
//! follows a path or holds other elements, so a category says how its box is taken (`Shape`):
//!
//! - a path (a railing, a mullion, a wall sweep, a slab edge, a conduit) comes in only where its box runs along an axis (across it at
//!   most a third of its length, or standing with its plan at most a quarter of its height): the box of a path that turns a corner
//!   or runs across the axes is the envelope of the path, a block where there is none, and is left out with that;
//! - a whole (a stair, a truss, a beam system) is left out where its box holds the box of one of its parts' categories (a stair's
//!   runs, landings and supports; a truss's or a beam system's framing), which come in on their own; one that holds none comes in
//!   as its box.

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use bimcraft_core::family::{Category, Expr, Family, Form, Kind, MAX_FORMS, Parameter};
use bimcraft_core::model::{Component, Element};

use crate::import::{Facts, Import, clip, fresh, level_at, micron, mm, point};
use crate::inventory::{Gathered, Inventory, Wants, category_name};
use crate::openings::{family_of, symbol_of};
use crate::placement::Transform;
use crate::records::Record;
use crate::shapes::Shape as Solid;

/// How a category's box stands for its elements: as their envelope, as a path's only where it runs along an axis, or as a whole
/// only where it holds no box of its parts' categories.
#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Envelope,
    Path,
    Whole(&'static [i64]),
}

/// A stair's parts' categories: runs, landings and supports.
const STAIR_PARTS: &[i64] = &[-2_000_919, -2_000_920, -2_000_123];
/// A truss's or a beam system's: structural framing.
const FRAMES: &[i64] = &[crate::framing::OST_STRUCTURAL_FRAMING];

/// The categories whose placed elements come in as boxes: Revit's id, the family's name and kind, what one is called in the notes, and
/// how its box stands for it.
pub(crate) const KINDS: [(i64, &str, Category, &str, Shape); 37] = [
    (-2_008_049, "Pipe Fittings", Category::PipeFitting, "pipe fitting", Shape::Envelope),
    (-2_008_010, "Duct Fittings", Category::Generic, "duct fitting", Shape::Envelope),
    (-2_008_020, "Flex Ducts", Category::Generic, "flex duct", Shape::Envelope),
    (-2_008_013, "Air Terminals", Category::Generic, "air terminal", Shape::Envelope),
    (-2_008_099, "Sprinklers", Category::Generic, "sprinkler", Shape::Envelope),
    (-2_001_160, "Plumbing Fixtures", Category::Plumbing, "plumbing fixture", Shape::Envelope),
    (-2_001_140, "Mechanical Equipment", Category::Generic, "mechanical equipment item", Shape::Envelope),
    (-2_001_040, "Electrical Equipment", Category::Equipment, "electrical equipment item", Shape::Envelope),
    (-2_001_060, "Electrical Fixtures", Category::Generic, "electrical fixture", Shape::Envelope),
    (-2_001_120, "Lighting Fixtures", Category::Lighting, "lighting fixture", Shape::Envelope),
    (-2_008_087, "Lighting Devices", Category::Generic, "lighting device", Shape::Envelope),
    (-2_000_151, "Generic Models", Category::Generic, "generic model", Shape::Envelope),
    (-2_000_080, "Furniture", Category::Furniture, "furniture item", Shape::Envelope),
    (-2_001_000, "Casework", Category::Casework, "casework item", Shape::Envelope),
    (-2_001_350, "Specialty Equipment", Category::Generic, "specialty equipment item", Shape::Envelope),
    (-2_008_126, "Cable Tray Fittings", Category::Generic, "cable tray fitting", Shape::Envelope),
    (-2_008_128, "Conduit Fittings", Category::Generic, "conduit fitting", Shape::Envelope),
    (-2_001_360, "Planting", Category::Generic, "plant", Shape::Envelope),
    (-2_001_370, "Entourage", Category::Generic, "entourage item", Shape::Envelope),
    (-2_001_180, "Parking", Category::Generic, "parking item", Shape::Envelope),
    // The parts first, so a whole is told after what came in of them.
    (-2_000_919, "Stair Runs", Category::Generic, "stair run", Shape::Envelope),
    (-2_000_920, "Stair Landings", Category::Generic, "stair landing", Shape::Envelope),
    (-2_000_123, "Stair Supports", Category::Generic, "stair support", Shape::Envelope),
    (-2_000_120, "Stairs", Category::Generic, "stair", Shape::Whole(STAIR_PARTS)),
    (-2_000_126, "Railings", Category::Generic, "railing", Shape::Path),
    (-2_000_170, "Curtain Panels", Category::Generic, "curtain panel", Shape::Path),
    (-2_000_171, "Curtain Wall Mullions", Category::Generic, "mullion", Shape::Path),
    (-2_000_181, "Wall Sweeps", Category::Generic, "wall sweep", Shape::Path),
    (-2_001_392, "Slab Edges", Category::Generic, "slab edge", Shape::Path),
    (-2_008_132, "Conduits", Category::Generic, "conduit", Shape::Path),
    (-2_001_100, "Furniture Systems", Category::Furniture, "furniture system", Shape::Envelope),
    (-2_001_263, "Site Pads", Category::Generic, "site pad", Shape::Envelope),
    (-2_003_400, "Mass", Category::Generic, "mass", Shape::Envelope),
    (-2_001_336, "Structural Trusses", Category::Generic, "truss", Shape::Whole(FRAMES)),
    (-2_001_327, "Structural Beam Systems", Category::Generic, "beam system", Shape::Whole(FRAMES)),
    (-2_001_354, "Structural Stiffeners", Category::Generic, "stiffener", Shape::Envelope),
    (-2_009_030, "Structural Connections", Category::Generic, "structural connection", Shape::Envelope),
];

/// A category the table has no name for: its elements come in as boxes named by its id. Every category with a reader is named.
fn by_id(category: i64) -> bool {
    category < 0 && category_name(category).is_none()
}

/// Whether a category's elements come in as boxes.
pub(crate) fn reads(category: i64) -> bool {
    KINDS.iter().any(|(id, ..)| *id == category) || by_id(category)
}

/// Whether a path's box runs along an axis: across it at most a third of its length, or standing, its plan at most a quarter of
/// its height (a mullion).
fn runs_along(b: &[f64; 6]) -> bool {
    let (dx, dy, dz) = (b[3] - b[0], b[4] - b[1], b[5] - b[2]);
    let (across, length) = (dx.min(dy), dx.max(dy));
    across <= length / 3.0 || length <= dz / 4.0
}

/// Whether box `part` lies within box `whole`, a millimetre of slack.
fn holds(whole: &[f64; 6], part: &[f64; 6]) -> bool {
    let slack = 1.0 / 304.8;
    (0..3).all(|k| part[k] >= whole[k] - slack && part[k + 3] <= whole[k + 3] + slack)
}

/// The type of a component whose type the reader does not know.
const IMPORTED: &str = "Imported";

/// What is said of a component that came in as its type's box, turned as placed.
const TURNED: &str = "comes in as a box of its type's box, turned as placed: its shape and its connectors are not read";

/// What is said of a component that came in with its type's shape.
const SHAPED: &str = "comes in with its type's shape, read from the file and turned as placed: its connectors are not read";

/// The type parameter that says which of a family's forms its type shows: 0 the box, `k` its `k`th form.
const SHOWS: &str = "Shape";

/// A family of a box whose width, depth and height, and where its middle sits from the origin, are instance parameters; its types
/// are added as its components name them.
fn box_family(category: Category) -> Family {
    let formula = |s: &str| Expr::Formula(s.to_string());
    let length = |name: &str, value: f64| Parameter {
        name: name.to_string(),
        kind: Kind::Length,
        instance: true,
        value: value.into(),
        ..Parameter::default()
    };
    Family {
        category,
        parameters: vec![
            length("Width", 100.0),
            length("Depth", 100.0),
            length("Height", 100.0),
            length("Offset X", 0.0),
            length("Offset Y", 0.0),
        ],
        forms: vec![{
            let mut form = Form { from: Expr::Number(0.0), to: formula("Height"), ..Default::default() };
            form.set_corners(&[
                [formula("Offset X - Width / 2"), formula("Offset Y - Depth / 2")],
                [formula("Offset X + Width / 2"), formula("Offset Y - Depth / 2")],
                [formula("Offset X + Width / 2"), formula("Offset Y + Depth / 2")],
                [formula("Offset X - Width / 2"), formula("Offset Y + Depth / 2")],
            ]);
            form
        }],
        ..Default::default()
    }
}

/// How a component stands, in feet: where its family's origin is in plan, its angle (degrees), the bottom of its box, the box's
/// width, depth and height, and where the box's middle sits from the origin in the family's axes.
struct Stand {
    at: [f64; 2],
    angle: f64,
    bottom: f64,
    size: [f64; 3],
    middle: [f64; 2],
}

impl Stand {
    /// The record's own box, unturned.
    fn of_box(b: &[f64; 6]) -> Stand {
        Stand {
            at: [(b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0],
            angle: 0.0,
            bottom: b[2],
            size: [b[3] - b[0], b[4] - b[1], b[5] - b[2]],
            middle: [0.0, 0.0],
        }
    }
}

/// The box round `local`, a box in an instance's own axes, placed by `t`.
fn placed_box(local: &[f64; 6], t: &Transform) -> [f64; 6] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for x in [local[0], local[3]] {
        for y in [local[1], local[4]] {
            for z in [local[2], local[5]] {
                for k in 0..3 {
                    let v = t.origin[k] + x * t.axes[0][k] + y * t.axes[1][k] + z * t.axes[2][k];
                    b[k] = b[k].min(v);
                    b[k + 3] = b[k + 3].max(v);
                }
            }
        }
    }
    b
}

/// Whether two boxes are the same to a millimetre.
fn same_box(a: &[f64; 6], b: &[f64; 6]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1.0 / 304.8)
}

/// How closely a placement's axis must lie along a project axis to be taken as lying along it.
const ALONG: f64 = 1e-9;

/// Whether a placement stands its family upright as drawn: its z the project's, so its shape turns only in plan.
fn standing(t: &Transform) -> bool {
    let [x, _, z] = t.axes;
    z[0].abs() <= ALONG && z[1].abs() <= ALONG && z[2] > 0.0 && x[2].abs() <= ALONG
}

/// Whether the symbol of shape `shape`, placed by `t`, stands where instance box `b` says: the box its graphics give round all
/// they draw, placed, is the instance's record box to a millimetre; or why not.
fn fits(shape: &Solid, t: &Transform, b: &[f64; 6]) -> Result<(), String> {
    if !standing(t) {
        return Err("it stands on its side, upside down or mirrored, and its shape is drawn upright".to_string());
    }
    if !same_box(&placed_box(&shape.whole, t), b) {
        return Err("the symbol it draws, placed, is not its record's box".to_string());
    }
    Ok(())
}

/// The box of instance `r` in its own axes, where each axis of its placement `t` lies along one of the project's (upright, upside
/// down or on its side, turned by a multiple of a right angle): its record box taken back through the placement, exactly, since such
/// a turn takes a box to a box.
fn square_box(r: &Record, t: &Transform) -> Option<[f64; 6]> {
    let square = t.axes.iter().all(|a| a.iter().filter(|v| v.abs() > ALONG).count() == 1);
    if !square {
        return None;
    }
    let b = &r.bbox;
    let mut local = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for x in [b[0], b[3]] {
        for y in [b[1], b[4]] {
            for z in [b[2], b[5]] {
                let d = [x - t.origin[0], y - t.origin[1], z - t.origin[2]];
                for k in 0..3 {
                    let v = d[0] * t.axes[k][0] + d[1] * t.axes[k][1] + d[2] * t.axes[k][2];
                    local[k] = local[k].min(v);
                    local[k + 3] = local[k + 3].max(v);
                }
            }
        }
    }
    local.iter().all(|v| v.is_finite()).then_some(local)
}

/// What the file says of an instance's type: its symbol's box (a file before 2025, where a type has a record), and the box its
/// instances that stand square agree on.
struct TypeBox<'a> {
    known: bool,
    symbol: Option<&'a [f64; 6]>,
    learned: Option<&'a [f64; 6]>,
}

/// The box `local`, in the axes of placement `t`, in upright axes that put it in the same place: X the first of `t`'s axes that lies
/// level, turned in plan, and Z up. None where none of `t`'s axes stands upright (one on a slope), since a component turns only in
/// plan. Where one does, the others lie level, and the box in the upright axes is a box, exactly.
fn upright(local: &[f64; 6], t: &Transform) -> Option<(f64, [f64; 6])> {
    let vertical = t.axes.iter().position(|a| a[0].abs() <= ALONG && a[1].abs() <= ALONG)?;
    let level = t.axes.iter().enumerate().find(|(k, _)| *k != vertical).map(|(_, a)| a)?;
    let length = level[0].hypot(level[1]);
    if length.is_nan() || length <= 0.5 {
        return None;
    }
    let x = [level[0] / length, level[1] / length];
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for cx in [local[0], local[3]] {
        for cy in [local[1], local[4]] {
            for cz in [local[2], local[5]] {
                let p: [f64; 3] = std::array::from_fn(|k| cx * t.axes[0][k] + cy * t.axes[1][k] + cz * t.axes[2][k]);
                let q = [p[0] * x[0] + p[1] * x[1], -p[0] * x[1] + p[1] * x[0], p[2]];
                for k in 0..3 {
                    b[k] = b[k].min(q[k]);
                    b[k + 3] = b[k + 3].max(q[k]);
                }
            }
        }
    }
    Some((x[1].atan2(x[0]), b))
}

/// How instance `r`, placed by `t`, stands as its type's box turned; or why the file does not say. The box is its type's symbol's,
/// else its own where it stands square, else the one its type's square instances agree on; placed by `t`, it must give the record's
/// box to a millimetre.
fn turned(r: &Record, t: Option<&Transform>, ty: TypeBox) -> Result<Stand, &'static str> {
    let t = t.ok_or("its placement is not read")?;
    if !t.axes.iter().any(|a| a[0].abs() <= ALONG && a[1].abs() <= ALONG) {
        return Err("it is tilted out of the plan: none of its axes stands upright");
    }
    let local = match (ty.symbol, square_box(r, t)) {
        (Some(b), _) => *b,
        (None, Some(b)) => b,
        (None, None) if !ty.known => return Err("its type is not known"),
        (None, None) => *ty.learned.ok_or("its type's box is not known: no instance of its type stands square to the axes")?,
    };
    let placed = placed_box(&local, t);
    if !same_box(&placed, &r.bbox) {
        return Err("its type's box, placed, is not its record's box");
    }
    let (angle, local) = upright(&local, t).ok_or("it is tilted out of the plan: none of its axes stands upright")?;
    Ok(Stand {
        at: [t.origin[0], t.origin[1]],
        angle: angle.to_degrees(),
        bottom: placed[2],
        size: [local[3] - local[0], local[4] - local[1], local[5] - local[2]],
        middle: [(local[0] + local[3]) / 2.0, (local[1] + local[4]) / 2.0],
    })
}

/// The placed elements of the categories read here: their placements are read from their data, and what they draw from their own
/// graphics, with the shapes of the symbols drawn.
pub(crate) fn wanted(f: &Facts, out: &mut Wants) {
    let mine: Vec<u32> = f.current.values().filter(|r| reads(r.category)).map(|r| r.id).collect();
    out.placements.extend(&mine);
    out.drawings.extend(mine);
}

/// A box to the micron, the way two reads of one body compare equal.
type Boxed = [i64; 6];

fn boxed(b: &[f64; 6]) -> Boxed {
    b.map(|x| (x * 304.8 * 1000.0).round() as i64)
}

/// The model's name of each family the components are made in, by category and Revit family (none: the category's own family), and
/// the names taken.
struct Families {
    taken: BTreeSet<String>,
    made: BTreeMap<(i64, Option<u32>), String>,
}

impl Families {
    /// The name of the family of `category` named `revit` in the file (none: the category's, `fallback`), a name no other family of
    /// the model has.
    fn name(&mut self, category: i64, revit: Option<&(u32, String)>, fallback: &str) -> String {
        let key = (category, revit.map(|(id, _)| *id));
        if let Some(name) = self.made.get(&key) {
            return name.clone();
        }
        let base = revit.map(|(_, n)| clip(n)).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| fallback.to_string());
        let name = fresh(&mut self.taken, &base);
        self.made.insert(key, name.clone());
        name
    }
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered, inv: &Inventory) {
    let mut families = Families { taken: cx.model.families.keys().cloned().collect(), made: BTreeMap::new() };
    let read = Read { f, g, inv };
    for (category, family, kind, label, shape) in KINDS {
        boxes(cx, &read, &mut families, None, category, (family, kind, label), shape);
    }
    let unnamed: BTreeSet<i64> = inv.elements.iter().filter(|e| e.placed && by_id(e.category)).map(|e| e.category).collect();
    if unnamed.is_empty() {
        return;
    }
    let known: BTreeSet<Boxed> = f.current.values().filter(|r| !by_id(r.category)).map(|r| boxed(&r.bbox)).collect();
    for category in unnamed {
        let label = format!("category {category} element");
        boxes(
            cx,
            &read,
            &mut families,
            Some(&known),
            category,
            (&format!("Category {category}"), Category::Generic, &label),
            Shape::Envelope,
        );
    }
}

/// How an element is to come in: its family, its Revit type's name, its stand as a box, why it is no turned box (if so), and what
/// it draws, or why its shape is not used.
struct Plan<'a> {
    r: &'a Record,
    family: String,
    type_name: String,
    stand: Stand,
    why: Option<&'static str>,
    draws: Result<Drawn<'a>, String>,
}

/// What an instance draws: the symbol, the transform it is drawn by, and its shape.
struct Drawn<'a> {
    symbol: u32,
    at: &'a Transform,
    shape: &'a Solid,
}

/// A reason with the offset in the file it was found at left out, so the notes of many elements read alike.
fn short(why: &str) -> &str {
    why.split(" at 0x").next().unwrap_or(why)
}

/// What instance `r` draws, where its own graphics name a symbol whose shape is read and, placed, gives its record's box.
fn drawing<'a>(g: &'a Gathered, r: &Record) -> Result<Drawn<'a>, String> {
    let (symbol, at) = match g.drawn.get(&r.id) {
        None => return Err("the file holds no graphics of it".to_string()),
        Some(Err(why)) => return Err(format!("its graphics are not read: {}", short(why))),
        Some(Ok((symbol, at))) => (*symbol, at),
    };
    let shape = match g.shapes.get(&symbol) {
        None => return Err("the file holds no graphics of the symbol it draws".to_string()),
        Some(Err(why)) => return Err(format!("the shape it draws is not read: {}", short(why))),
        Some(Ok(shape)) => shape,
    };
    fits(shape, at, &r.bbox)?;
    Ok(Drawn { symbol, at, shape })
}

/// The box round a shape, millimetres in its family's axes.
fn extent(shape: &Solid) -> [f64; 6] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in shape.triangles.iter().flatten() {
        for k in 0..3 {
            b[k] = b[k].min(p[k]);
            b[k + 3] = b[k + 3].max(p[k]);
        }
    }
    b
}

/// Gives type `type_name` of `family` its shape `solid`, as a form only it shows (the family's box shows for its types without
/// one); whether it is drawn so. A family past [`MAX_FORMS`] keeps the box for its type.
fn show(family: &mut Family, type_name: &str, solid: &Solid) -> bool {
    if let Some(k) = family.types.get(type_name).and_then(|t| t.get(SHOWS)) {
        return k.as_u64().is_some_and(|k| k > 0);
    }
    if family.forms.len() >= MAX_FORMS {
        family.types.entry(type_name.to_string()).or_default();
        return false;
    }
    if !family.parameters.iter().any(|p| p.name == SHOWS) {
        family.parameters.push(Parameter { name: SHOWS.to_string(), kind: Kind::Number, value: 0.into(), ..Parameter::default() });
        for form in &mut family.forms {
            form.visible = Some(Expr::Formula(format!("{SHOWS} = 0")));
        }
    }
    let k = family.forms.len();
    let visible = Some(Expr::Formula(format!("{SHOWS} = {k}")));
    family.forms.push(Form { mesh: Arc::new(solid.triangles.clone()), visible, ..Form::default() });
    family.types.entry(type_name.to_string()).or_default().insert(SHOWS.to_string(), k.into());
    true
}

/// What the survey and the second pass found, which the components are made from.
struct Read<'a> {
    f: &'a Facts,
    g: &'a Gathered,
    inv: &'a Inventory,
}

/// The placed elements of `category` as components of the family `family` (made with the first), told as `label`, where their box
/// stands for them as `taken` says; those of a category with no name (`known` is the boxes of the elements of the others) whose box is
/// one of `known` are left out.
fn boxes(
    cx: &mut Import,
    &Read { f, g, inv }: &Read,
    families: &mut Families,
    known: Option<&BTreeSet<Boxed>>,
    category: i64,
    (family, kind, label): (&str, Category, &str),
    taken: Shape,
) {
    let (box_so, turned_so, flat) = if known.is_some() {
        (
            "comes in as a box of its record's box: its category has no name here, and its shape and its connectors are not read",
            "comes in as a box of its type's box, turned as placed: its category has no name here, and its shape and its connectors are \
             not read",
            "it has no volume, so it has no body to import",
        )
    } else {
        (
            "comes in as a box of its record's box: its shape and its connectors are not read",
            TURNED,
            "it has no volume: a flat symbol has no body to import",
        )
    };
    let symbols = f.symbols.get(&category).cloned().unwrap_or_default();
    // Each type's family, from the instances that name both (one that names two is none's), and each type's box in its own axes,
    // from the instances that stand square (where they disagree, none).
    let mut type_family: BTreeMap<u32, Option<(u32, String)>> = BTreeMap::new();
    let mut type_box: BTreeMap<u32, Option<[f64; 6]>> = BTreeMap::new();
    for r in f.of_category(category) {
        let Some((ty, _)) = symbol_of(r, &symbols, g, category) else { continue };
        if let Some(fam) = family_of(r, g, category) {
            let held = type_family.entry(ty).or_insert_with(|| Some(fam.clone()));
            if held.as_ref().is_some_and(|h| h.0 != fam.0) {
                *held = None;
            }
        }
        if let Some(b) = g.placements.get(&r.id).and_then(|t| square_box(r, t)) {
            let held = type_box.entry(ty).or_insert(Some(b));
            if held.is_some_and(|h| !same_box(&h, &b)) {
                *held = None;
            }
        }
    }
    // What comes in, and how: each element's family, type and stand, and what it draws where its shape is used.
    let mut plans: Vec<Plan> = Vec::new();
    for r in f.of_category(category) {
        // A model group is no body, and terrain's box is a solid block over the site: neither is a box of anything.
        if f.tags.group == Some(r.class_tag) {
            cx.left_out(label, r.id, "it is a model group (class ElementGroup): its members come in on their own, and its box is no body");
            continue;
        }
        if f.tags.terrain.contains(&Some(r.class_tag)) {
            let why =
                "it is terrain (class Toposolid or SiteSurface): its surface is not read, and its box would be a solid block over the site";
            cx.left_out(label, r.id, why);
            continue;
        }
        if known.is_some_and(|k| k.contains(&boxed(&r.bbox))) {
            let why =
                "its box is that of an element of a category the reader knows: it is taken as that element's companion, not a second body";
            cx.left_out(label, r.id, why);
            continue;
        }
        match taken {
            Shape::Envelope => {}
            Shape::Path if runs_along(&r.bbox) => {}
            Shape::Path => {
                let why = "its box is the envelope of a path that turns or runs across the axes, not its body, and the path is not read";
                cx.left_out(label, r.id, why);
                continue;
            }
            Shape::Whole(parts) => {
                if parts.iter().flat_map(|p| f.of_category(*p)).any(|p| holds(&r.bbox, &p.bbox)) {
                    cx.left_out(label, r.id, "its box holds those of its parts, which come in on their own");
                    continue;
                }
            }
        }
        let symbol = symbol_of(r, &symbols, g, category);
        let type_name = symbol.as_ref().map(|(_, n)| clip(n)).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| IMPORTED.to_string());
        let ty = symbol.as_ref().map(|(id, _)| *id);
        let revit_family = family_of(r, g, category).or_else(|| ty.and_then(|t| type_family.get(&t).cloned().flatten()));
        let family = families.name(category, revit_family.as_ref(), family);
        let type_box = TypeBox {
            known: ty.is_some(),
            symbol: ty.and_then(|t| f.symbol_boxes.get(&t)),
            learned: ty.and_then(|t| type_box.get(&t)).and_then(Option::as_ref),
        };
        let (stand, why) = match turned(r, g.placements.get(&r.id), type_box) {
            Ok(stand) => (stand, None),
            Err(why) => (Stand::of_box(&r.bbox), Some(why)),
        };
        plans.push(Plan { r, family, type_name, stand, why, draws: drawing(g, r) });
    }
    // A type is each shape its instances draw (symbols drawn alike are one): the first its Revit type's name, the next numbered
    // on from 2. Instances that come in as boxes are a type of their own beside them, so a box never shows a shape it was not
    // checked against.
    let mut alike: Vec<(u32, &Solid)> = Vec::new();
    let mut same_as: BTreeMap<u32, u32> = BTreeMap::new();
    for d in plans.iter().filter_map(|p| p.draws.as_ref().ok()) {
        if same_as.contains_key(&d.symbol) {
            continue;
        }
        let first = alike.iter().find(|(_, s)| s.triangles == d.shape.triangles).map_or(d.symbol, |(k, _)| *k);
        if first == d.symbol {
            alike.push((d.symbol, d.shape));
        }
        same_as.insert(d.symbol, first);
    }
    let variant = |p: &Plan| p.draws.as_ref().ok().map(|d| same_as.get(&d.symbol).copied().unwrap_or(d.symbol));
    let mut variants: BTreeMap<(String, String), Vec<Option<u32>>> = BTreeMap::new();
    for p in &plans {
        let held = variants.entry((p.family.clone(), p.type_name.clone())).or_default();
        let v = variant(p);
        if !held.contains(&v) {
            held.push(v);
        }
    }
    for p in plans {
        let r = p.r;
        let v = variant(&p);
        let n = variants.get(&(p.family.clone(), p.type_name.clone())).and_then(|held| held.iter().position(|h| *h == v)).unwrap_or(0);
        let type_name = if n == 0 { p.type_name.clone() } else { clip(&format!("{} ({})", p.type_name, n + 1)) };
        let made = cx.model.families.entry(p.family.clone()).or_insert_with(|| box_family(kind));
        let drawn = match &p.draws {
            Ok(d) if show(made, &type_name, d.shape) => Some(d),
            _ => {
                made.types.entry(type_name.clone()).or_default();
                None
            }
        };
        let built = (|| -> Result<Component, String> {
            let (level, elevation) = level_at(cx, f, r.bbox[2].min(p.stand.bottom))?;
            let mut params = BTreeMap::new();
            let (at, angle, base, lengths) = match drawn {
                Some(d) => {
                    let b = extent(d.shape);
                    let sizes = [b[3] - b[0], b[4] - b[1], b[5] - b[2], (b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0];
                    let angle = d.at.axes[0][1].atan2(d.at.axes[0][0]).to_degrees();
                    ([d.at.origin[0], d.at.origin[1]], angle, d.at.origin[2], sizes)
                }
                None => {
                    let s = &p.stand;
                    let feet = [s.size[0], s.size[1], s.size[2], s.middle[0], s.middle[1]];
                    (s.at, s.angle, s.bottom, feet.map(mm))
                }
            };
            for (name, length) in ["Width", "Depth", "Height", "Offset X", "Offset Y"].into_iter().zip(lengths) {
                params.insert(name.to_string(), micron(length).into());
            }
            Ok(Component {
                family: p.family.clone(),
                type_name: type_name.clone(),
                level,
                at: point(at[0], at[1]),
                angle: micron(angle),
                offset: micron(mm(base) - mm(elevation)),
                mirrored: false,
                params,
                adaptive: None,
                host: None,
                panel: None,
            })
        })();
        match built {
            Ok(component) => {
                let id = cx.model.insert(Element::Component(component));
                cx.from.insert(id, format!("{label} {}", r.id));
                match (drawn, p.why, &p.draws) {
                    (Some(_), ..) => cx.note(label, r.id, SHAPED),
                    (None, None, Err(not)) => cx.note(label, r.id, &format!("{turned_so}; {not}")),
                    (None, None, Ok(_)) => cx.note(label, r.id, &format!("{turned_so}; its family holds as many shapes as one family may")),
                    (None, Some(why), _) => cx.note(label, r.id, &format!("{box_so}; {why}")),
                }
            }
            Err(why) => cx.left_out(label, r.id, &why),
        }
    }
    // A placed element with no volume (a flat symbol drawn in plan) has no body to import.
    for e in inv.elements.iter().filter(|e| e.placed && e.category == category && !f.current.contains_key(&e.id)) {
        cx.left_out(label, e.id, flat);
    }
}
