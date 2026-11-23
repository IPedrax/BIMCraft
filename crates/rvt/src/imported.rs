//! Each wall, floor, roof, ceiling and column made, drawn in 3D as its own graphics draw it ([`bimcraft_core::model::ImportedShape`]):
//! Revit's corners as its joins cut them, a wall's lean, an outline as it was edited, the holes its doors and windows cut, a roof's
//! and a floor's slopes, a column's section. The element stays BIMCraft's, made by its own module from its record, which plans,
//! schedules and IFC read; the model draws its shape in 3D while it is as it came in (its key is set once the model is checked,
//! `import::finish`), and its own geometry once it is edited.
//!
//! The graphics are read with the components' reading and checks ([`crate::generic::drawn_where_it_stands`]): an element whose
//! graphics do not read, or are not the box they give round their solids, keeps its own geometry, with a note why; one the file
//! holds no graphics of, with none. A floor or a ceiling whose sketch came in as several elements, one for each loop, keeps them:
//! the one shape is all of them.

use std::collections::{BTreeMap, HashMap};

use bimcraft_core::model::{Id, ImportedPart, ImportedShape, MAX_IMPORTED_PARTS, MAX_IMPORTED_TRIANGLES};

use crate::import::{Facts, Import, origin};
use crate::inventory::Gathered;
use crate::records::Record;
use crate::{columns, floors, roofs, walls};

/// The categories whose elements are drawn as their graphics draw them.
const CATEGORIES: [i64; 6] =
    [walls::OST_WALLS, floors::OST_FLOORS, floors::OST_CEILINGS, roofs::OST_ROOFS, columns::OST_COLUMNS, columns::OST_STRUCTURAL_COLUMNS];

/// The elements whose graphics are read for the shape each is drawn in: those of [`CATEGORIES`], but for the walls a curtain wall's
/// parts name as their host (a curtain wall draws nothing of its own).
pub(crate) fn wanted(f: &Facts) -> Vec<u32> {
    let hosts = walls::curtain_hosts(f);
    f.current.values().filter(|r| CATEGORIES.contains(&r.category) && !hosts.contains(&r.id)).map(|r| r.id).collect()
}

/// The shape record `r`'s graphics draw, its triangles by the material each is drawn in, as `g` names the file's materials; or
/// why not.
fn shape_of(g: &Gathered, r: &Record) -> Result<ImportedShape, String> {
    let drawn = crate::generic::drawn_where_it_stands(g, r)?;
    if drawn.triangles.len() > MAX_IMPORTED_TRIANGLES {
        return Err(format!("it draws {} triangles, past the {MAX_IMPORTED_TRIANGLES} one may hold", drawn.triangles.len()));
    }
    let mut by: BTreeMap<&str, Vec<[[f64; 3]; 3]>> = BTreeMap::new();
    for (k, t) in drawn.triangles.iter().enumerate() {
        let name = drawn.materials.get(k).copied().flatten().and_then(|m| g.materials.get(&m)).map_or("", String::as_str);
        by.entry(name).or_default().push(*t);
    }
    if by.len() > MAX_IMPORTED_PARTS {
        return Err(format!("it is drawn in {} materials, past the {MAX_IMPORTED_PARTS} one may hold", by.len()));
    }
    let parts = by.into_iter().map(|(m, tris)| ImportedPart { material: m.to_string(), tris: std::sync::Arc::new(tris) }).collect();
    Ok(ImportedShape { key: 0, parts })
}

/// Gives each element made of [`CATEGORIES`] the shape its graphics draw, or notes why it keeps its own geometry.
pub(crate) fn shaped(cx: &mut Import, f: &Facts, g: &Gathered) {
    // What each Revit element came in as: its kind, and the model's elements made of it.
    let mut made: HashMap<u32, (String, Vec<Id>)> = HashMap::new();
    for (id, from) in &cx.from {
        if let Some((kind, Ok(n))) = origin(from) {
            let entry = made.entry(n).or_insert_with(|| (kind, Vec::new()));
            entry.1.push(*id);
        }
    }
    let hosts = walls::curtain_hosts(f);
    for r in f.current.values().filter(|r| CATEGORIES.contains(&r.category) && !hosts.contains(&r.id)) {
        let Some((kind, ids)) = made.get(&r.id) else { continue };
        let takes = |id: &Id| {
            cx.model.elements.get(*id).is_some_and(|e| {
                let curtain = matches!(e, bimcraft_core::model::Element::Wall(w) if w.curtain.is_some());
                !curtain && e.clone().imported_mut().is_some()
            })
        };
        let [id] = ids[..] else {
            if ids.len() > 1 && ids.iter().all(takes) && g.drawn.contains_key(&r.id) {
                cx.note(kind, r.id, "it came in as several elements, one for each loop of its sketch: each is drawn from its own geometry");
            }
            continue;
        };
        if !takes(&id) {
            continue;
        }
        match shape_of(g, r) {
            Ok(shape) => {
                if let Some(slot) = cx.model.elements.get_mut(id).and_then(|e| e.imported_mut()) {
                    *slot = Some(shape);
                }
                cx.note(kind, r.id, "it is drawn in 3D as its own graphics draw it, until it is edited");
            }
            // An element the file holds no graphics of says nothing of its shape: its own geometry is all there is.
            Err(why) if why == crate::generic::NO_GRAPHICS => {}
            Err(why) => cx.note(kind, r.id, &format!("it is drawn in 3D from its own geometry: {why}")),
        }
    }
}
