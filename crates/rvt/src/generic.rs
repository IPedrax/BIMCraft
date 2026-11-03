//! The other placed elements (fittings, fixtures, equipment, terminals, generic models): each comes in as a box-shaped component of
//! a family of its category.
//!
//! The file's record says where an element is and the box that holds it, in project axes; its shape, its type and its connectors
//! are not read here. So a component comes in as the record's box: its middle in plan, its bottom on the level at or below it, and
//! its width, depth and height the box's. A turned element's box is the one round it, so the box is the element's envelope. One family
//! of the category's kind of component (`Category::PipeFitting`, `Plumbing`, `Generic`...) holds all of a category's, with the sizes
//! as instance parameters, and the notes say each came in as a box.
//!
//! A category the table has no name for (`inventory::category_name`) is read the same way, its elements as boxes of a family named
//! after its id (`Category -2008123`): the reader does not guess what Revit calls it, and the note says its category has no name here.
//! Such a category often holds an element's companion, a second element with the same box as the pipe, fitting or duct it goes with;
//! one whose box (to the micron) is the box of an element of a category with a name is that companion, not a second body, and is left
//! out and listed.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::family::{Category, Expr, Family, Form, Kind, Parameter};
use bimcraft_core::model::{Component, Element};

use crate::import::{Facts, Import, level_at, micron, mm, point};
use crate::inventory::{Inventory, category_name};

/// The categories whose placed elements come in as boxes: Revit's id, the family's name and kind, and what one is called in the notes.
pub(crate) const KINDS: [(i64, &str, Category, &str); 20] = [
    (-2_008_049, "Pipe Fittings", Category::PipeFitting, "pipe fitting"),
    (-2_008_010, "Duct Fittings", Category::Generic, "duct fitting"),
    (-2_008_020, "Flex Ducts", Category::Generic, "flex duct"),
    (-2_008_013, "Air Terminals", Category::Generic, "air terminal"),
    (-2_008_099, "Sprinklers", Category::Generic, "sprinkler"),
    (-2_001_160, "Plumbing Fixtures", Category::Plumbing, "plumbing fixture"),
    (-2_001_140, "Mechanical Equipment", Category::Generic, "mechanical equipment item"),
    (-2_001_040, "Electrical Equipment", Category::Equipment, "electrical equipment item"),
    (-2_001_060, "Electrical Fixtures", Category::Generic, "electrical fixture"),
    (-2_001_120, "Lighting Fixtures", Category::Lighting, "lighting fixture"),
    (-2_008_087, "Lighting Devices", Category::Generic, "lighting device"),
    (-2_000_151, "Generic Models", Category::Generic, "generic model"),
    (-2_000_080, "Furniture", Category::Furniture, "furniture item"),
    (-2_001_000, "Casework", Category::Casework, "casework item"),
    (-2_001_350, "Specialty Equipment", Category::Generic, "specialty equipment item"),
    (-2_008_126, "Cable Tray Fittings", Category::Generic, "cable tray fitting"),
    (-2_008_128, "Conduit Fittings", Category::Generic, "conduit fitting"),
    (-2_001_360, "Planting", Category::Generic, "plant"),
    (-2_001_370, "Entourage", Category::Generic, "entourage item"),
    (-2_001_180, "Parking", Category::Generic, "parking item"),
];

/// A category the table has no name for: its elements come in as boxes named by its id. Every category with a reader is named.
fn by_id(category: i64) -> bool {
    category < 0 && category_name(category).is_none()
}

/// Whether a category's elements come in as boxes.
pub(crate) fn reads(category: i64) -> bool {
    KINDS.iter().any(|(id, ..)| *id == category) || by_id(category)
}

/// A family of one type, `Imported`, of a box whose width, depth and height are instance parameters.
fn box_family(category: Category) -> Family {
    let formula = |s: &str| Expr::Formula(s.to_string());
    Family {
        category,
        parameters: ["Width", "Depth", "Height"]
            .into_iter()
            .map(|name| Parameter { name: name.to_string(), kind: Kind::Length, instance: true, value: 100.0.into(), formula: None })
            .collect(),
        types: BTreeMap::from([("Imported".to_string(), BTreeMap::new())]),
        forms: vec![Form {
            profile: vec![
                [formula("-(Width) / 2"), formula("-(Depth) / 2")],
                [formula("Width / 2"), formula("-(Depth) / 2")],
                [formula("Width / 2"), formula("Depth / 2")],
                [formula("-(Width) / 2"), formula("Depth / 2")],
            ],
            from: Expr::Number(0.0),
            to: formula("Height"),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// A box to the micron, the way two reads of one body compare equal.
type Boxed = [i64; 6];

fn boxed(b: &[f64; 6]) -> Boxed {
    b.map(|x| (x * 304.8 * 1000.0).round() as i64)
}

pub(crate) fn import(cx: &mut Import, f: &Facts, inv: &Inventory) {
    for (category, family, kind, label) in KINDS {
        boxes(cx, f, inv, None, category, (family, kind, label));
    }
    let unnamed: BTreeSet<i64> = inv.elements.iter().filter(|e| e.placed && by_id(e.category)).map(|e| e.category).collect();
    if unnamed.is_empty() {
        return;
    }
    let known: BTreeSet<Boxed> = f.current.values().filter(|r| !by_id(r.category)).map(|r| boxed(&r.bbox)).collect();
    for category in unnamed {
        let label = format!("category {category} element");
        boxes(cx, f, inv, Some(&known), category, (&format!("Category {category}"), Category::Generic, &label));
    }
}

/// The placed elements of `category` as components of the family `family` (made with the first), told as `label`; those of a category
/// with no name (`known` is the boxes of the elements of the others) whose box is one of `known` are left out.
fn boxes(
    cx: &mut Import,
    f: &Facts,
    inv: &Inventory,
    known: Option<&BTreeSet<Boxed>>,
    category: i64,
    (family, kind, label): (&str, Category, &str),
) {
    let (shape, flat) = if known.is_some() {
        (
            "comes in as a box of its record's box, in a family named by its category's id: its category has no name here, and its \
             shape, its type and its connectors are not read",
            "it has no volume, so it has no body to import",
        )
    } else {
        (
            "comes in as a box of its record's box: its shape, its type and its connectors are not read",
            "it has no volume: a flat symbol has no body to import",
        )
    };
    for r in f.of_category(category) {
        if known.is_some_and(|k| k.contains(&boxed(&r.bbox))) {
            let why =
                "its box is that of an element of a category the reader knows: it is taken as that element's companion, not a second body";
            cx.left_out(label, r.id, why);
            continue;
        }
        let made = (|| -> Result<Component, String> {
            let (level, elevation) = level_at(cx, f, r.bbox[2])?;
            let size = |k: usize| micron(mm(r.bbox[k + 3] - r.bbox[k]));
            let mut params = BTreeMap::new();
            for (k, name) in ["Width", "Depth", "Height"].into_iter().enumerate() {
                params.insert(name.to_string(), size(k).into());
            }
            Ok(Component {
                family: family.to_string(),
                type_name: "Imported".to_string(),
                level,
                at: point((r.bbox[0] + r.bbox[3]) / 2.0, (r.bbox[1] + r.bbox[4]) / 2.0),
                angle: 0.0,
                offset: micron(mm(r.bbox[2]) - mm(elevation)),
                mirrored: false,
                params,
            })
        })();
        match made {
            Ok(component) => {
                cx.model.families.entry(family.to_string()).or_insert_with(|| box_family(kind));
                let id = cx.model.insert(Element::Component(component));
                cx.from.insert(id, format!("{label} {}", r.id));
                cx.note(label, r.id, shape);
            }
            Err(why) => cx.left_out(label, r.id, &why),
        }
    }
    // A placed element with no volume (a flat symbol drawn in plan) has no body to import.
    for e in inv.elements.iter().filter(|e| e.placed && e.category == category && !f.current.contains_key(&e.id)) {
        cx.left_out(label, e.id, flat);
    }
}
