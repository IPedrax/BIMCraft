//! The same numbers as Revit: a model described for another tool to build again, our numbers of what that tool reports,
//! and the two compared.
//!
//! [`describe`] writes a model's levels, walls, floors, shafts, doors, windows and rooms as plain numbers (millimetres,
//! GUIDs): what `tools/revit-crosscheck/crosscheck.py` builds the same model from in Revit, with Revit's own system families
//! and nothing of Autodesk's copied here. The script writes what Revit reports of it, one row a quantity
//! (`kind,id,quantity,value`: the element's id, which the same commands give again, where its GUID is new each time), and
//! [`compare`] holds that against [`ours`], the same quantities as this model works them
//! out. Where the two measure a thing differently, the difference is named in `docs/areas/schedules.md`, never let pass.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::geom::{self, P2};
use crate::model::{CUT_HEIGHT, Element, Guid, Id, Model, RoomArea};
use crate::rooms::Rooms;
use crate::walls::{self, Axis};

/// What a description says it is, and its version.
pub const FORMAT: &str = "bimcraft-crosscheck";
pub const VERSION: u32 = 1;
/// The most rows a table of results is read to, and its most bytes.
pub const MAX_ROWS: usize = 1_000_000;
pub const MAX_TEXT: usize = 64 << 20;

/// One quantity of one element: `value` of `quantity` (`area_m2`, `length_m`…) of element `id`, a `kind` (`room`,
/// `wall`…).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: String,
    pub id: Id,
    pub quantity: String,
    pub value: f64,
}

impl Row {
    fn new(kind: &str, id: Id, quantity: &str, value: f64) -> Row {
        Row { kind: kind.into(), id, quantity: quantity.into(), value }
    }
}

/// How far two numbers of a quantity may be apart and be the same: a thousandth of the larger, or what its unit is
/// worked to (a square centimetre, a millimetre, a cubic decimetre, one of a count).
pub fn tolerance(quantity: &str, a: f64, b: f64) -> f64 {
    let unit = match quantity.rsplit('_').next() {
        Some("m2") => 1e-4,
        Some("m") => 1e-3,
        Some("m3") => 1e-3,
        _ => 0.5,
    };
    f64::max(unit, 1e-3 * a.abs().max(b.abs()))
}

/// A wall's centre line: its start, its end and, of an arc, a point halfway along; and its length.
fn centre_line(m: &Model, w: &crate::model::Wall) -> (P2, P2, Option<P2>, f64) {
    let (axis, c) = (Axis::of(w), walls::centre(m, w));
    let len = axis.length();
    let (start, end) = (axis.at(0.0, c), axis.at(len, c));
    match axis.arc() {
        // Turning left, the left is toward the centre: the centre line's radius is less by `c`.
        Some((_, r, _, sweep)) => (start, end, Some(axis.at(len / 2.0, c)), (r - sweep.signum() * c) * sweep.abs()),
        None => (start, end, None, len),
    }
}

fn point(p: P2) -> Value {
    json!([p.0, p.1])
}

fn ring(r: &[P2]) -> Value {
    Value::Array(r.iter().map(|p| point(*p)).collect())
}

/// Model `m` as another tool builds it again: its levels, the types its walls and floors are of (each by its thickness),
/// its walls by their centre lines, floors, shafts, doors and windows (by host, the point they're centred on, and their
/// size) and rooms (by the point they're found from), and how rooms are measured. What else it holds is counted in
/// `left_out`.
pub fn describe(m: &Model) -> Value {
    let (mut levels, mut walls_out, mut floors, mut shafts, mut openings, mut rooms) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut left_out: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut wall_types, mut floor_types) = (BTreeMap::new(), BTreeMap::new());
    for (id, g, e) in m.elements.with_guids() {
        match e {
            Element::Level(l) => levels.push(json!({
                "guid": g, "name": l.name, "elevation": l.elevation, "computation_height": l.computation_height,
            })),
            Element::Wall(w) => {
                let (start, end, mid, _) = centre_line(m, w);
                wall_types.insert(w.type_name.clone(), m.thickness(w));
                walls_out.push(json!({
                    "guid": g, "id": id, "type": w.type_name, "level": w.level, "top_level": w.top_level,
                    "height": w.height, "base_offset": w.base_offset,
                    "start": point(start), "end": point(end), "mid": mid.map(point),
                }));
            }
            Element::Floor(f) => {
                floor_types.insert(f.type_name.clone(), m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness));
                floors.push(json!({
                    "guid": g, "id": id, "type": f.type_name, "level": f.level, "offset": f.offset, "slope": f.slope,
                    "boundary": ring(&f.boundary), "openings": f.openings.iter().map(|o| ring(o)).collect::<Vec<_>>(),
                }));
            }
            Element::Shaft(sh) => shafts.push(json!({
                "guid": g, "id": id, "level": sh.level, "top_level": sh.top_level, "height": sh.height,
                "base_offset": sh.base_offset, "boundary": ring(&sh.boundary),
            })),
            Element::Door(o) | Element::Window(o) => {
                let Some(host) = m.wall(o.host) else {
                    *left_out.entry(e.category()).or_default() += 1;
                    continue;
                };
                let (dims, c) = (m.opening_dims(e).ok(), walls::centre(m, host));
                openings.push(json!({
                    "guid": g, "id": id, "kind": e.category(), "host": o.host,
                    "at": point(Axis::of(host).at(o.offset, c)),
                    "width": dims.as_ref().map(|d| d.width), "height": dims.as_ref().map(|d| d.height), "sill": o.sill,
                }));
            }
            Element::Room(r) => rooms.push(json!({
                "guid": g, "id": id, "level": r.level, "point": point(r.point), "name": r.name, "number": r.number,
                "top_level": r.top_level, "base_offset": r.base_offset, "limit_offset": r.limit_offset,
            })),
            _ => *left_out.entry(e.category()).or_default() += 1,
        }
    }
    let types = |t: BTreeMap<String, f64>| -> Vec<Value> {
        t.into_iter().map(|(name, thickness)| json!({"name": name, "thickness": thickness})).collect()
    };
    json!({
        "format": FORMAT,
        "version": VERSION,
        "units": "mm",
        "project": m.name,
        "room_area": match m.room_area { RoomArea::Centre => "centre", RoomArea::Finish => "finish" },
        "computation_height": CUT_HEIGHT,
        "levels": levels,
        "wall_types": types(wall_types),
        "floor_types": types(floor_types),
        "walls": walls_out,
        "floors": floors,
        "shafts": shafts,
        "openings": openings,
        "rooms": rooms,
        "left_out": left_out,
    })
}

/// What this model works out of the quantities [`describe`]'s tool reports: each room's area, perimeter and volume (0 where
/// it isn't enclosed), whether it is, and its unbounded height (its upper limit and offsets'), each wall's length along its centre line and how many doors and windows it holds,
/// each floor's area (less its openings and the shafts through it) and volume.
pub fn ours(m: &Model) -> Vec<Row> {
    let rooms = Rooms::new(m);
    let mut hosted: BTreeMap<Guid, usize> = BTreeMap::new();
    for e in m.elements.values() {
        if let Element::Door(o) | Element::Window(o) = e {
            *hosted.entry(o.host).or_default() += 1;
        }
    }
    let mut rows = Vec::new();
    for (id, g, e) in m.elements.with_guids() {
        match e {
            Element::Room(r) => {
                let region = rooms.region(r);
                rows.push(Row::new("room", id, "enclosed", if region.is_some() { 1.0 } else { 0.0 }));
                rows.push(Row::new("room", id, "area_m2", region.as_ref().map_or(0.0, |r| r.area() / 1e6)));
                rows.push(Row::new("room", id, "perimeter_m", region.as_ref().map_or(0.0, |r| r.perimeter() / 1e3)));
                // Its volume as Revit's: its area up its unbounded height, while nothing lower (a ceiling) bounds it.
                let height = m.room_height(r);
                rows.push(Row::new("room", id, "height_m", height / 1e3));
                rows.push(Row::new("room", id, "volume_m3", region.as_ref().map_or(0.0, |r| r.area() * height / 1e9)));
            }
            Element::Wall(w) => {
                rows.push(Row::new("wall", id, "length_m", centre_line(m, w).3 / 1e3));
                rows.push(Row::new("wall", id, "openings", hosted.get(&g).copied().unwrap_or(0) as f64));
            }
            Element::Floor(f) => {
                let thickness = m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness);
                let area: f64 =
                    m.floor_slabs(f).iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).0).map(|r| geom::area(&r).abs()).sum();
                rows.push(Row::new("floor", id, "area_m2", area / 1e6));
                rows.push(Row::new("floor", id, "volume_m3", area * thickness / 1e9));
            }
            _ => {}
        }
    }
    rows
}

/// A table of results, `kind,id,quantity,value` with that header first: each row's id an element id, its value a finite
/// number.
pub fn read(text: &str) -> Result<Vec<Row>, String> {
    if text.len() > MAX_TEXT {
        return Err(format!("the table is over {} MB", MAX_TEXT >> 20));
    }
    let mut lines = text.lines().map(str::trim).enumerate().filter(|(_, l)| !l.is_empty());
    match lines.next() {
        Some((_, h)) if h.trim_start_matches('\u{feff}').replace(' ', "") == "kind,id,quantity,value" => {}
        _ => return Err("the table's first line must be `kind,id,quantity,value`".into()),
    }
    let mut rows = Vec::new();
    for (n, line) in lines {
        if rows.len() >= MAX_ROWS {
            return Err(format!("the table has over {MAX_ROWS} rows"));
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let [kind, id, quantity, value] = cells.as_slice() else {
            return Err(format!("line {}: a row is `kind,id,quantity,value`", n + 1));
        };
        let id: Id = id.parse().map_err(|_| format!("line {}: `{id}` isn't an element id", n + 1))?;
        let value: f64 = value.parse().ok().filter(|v: &f64| v.is_finite()).ok_or(format!("line {}: `{value}` isn't a number", n + 1))?;
        if [kind, quantity].iter().any(|s| s.is_empty() || s.len() > 64) {
            return Err(format!("line {}: a kind and a quantity, each of up to 64 characters", n + 1));
        }
        rows.push(Row { kind: kind.to_string(), id, quantity: quantity.to_string(), value });
    }
    Ok(rows)
}

/// What [`compare`] found.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// The quantities both have.
    pub compared: usize,
    /// Those of them that differ by more than [`tolerance`]: ours, then theirs.
    pub differ: Vec<(Row, f64)>,
    /// Those only we have, and those only they have.
    pub only_ours: Vec<Row>,
    pub only_theirs: Vec<Row>,
}

/// `ours` held against `theirs`, quantity by quantity of each element.
pub fn compare(ours: &[Row], theirs: &[Row]) -> Report {
    let key = |r: &Row| (r.kind.clone(), r.id, r.quantity.clone());
    let mut them: BTreeMap<_, &Row> = theirs.iter().map(|r| (key(r), r)).collect();
    let mut report = Report::default();
    for r in ours {
        match them.remove(&key(r)) {
            Some(t) => {
                report.compared += 1;
                if (r.value - t.value).abs() > tolerance(&r.quantity, r.value, t.value) {
                    report.differ.push((r.clone(), t.value));
                }
            }
            None => report.only_ours.push(r.clone()),
        }
    }
    report.only_theirs = them.into_values().cloned().collect();
    report
}

impl Report {
    /// As JSON: the counts, and the first `limit` of each list.
    pub fn to_json(&self, limit: usize) -> Value {
        let row = |r: &Row| json!({"kind": r.kind, "id": r.id, "quantity": r.quantity, "value": r.value});
        json!({
            "compared": self.compared,
            "differ_count": self.differ.len(),
            "differ": self.differ.iter().take(limit).map(|(r, theirs)| {
                json!({"kind": r.kind, "id": r.id, "quantity": r.quantity, "ours": r.value, "theirs": theirs})
            }).collect::<Vec<_>>(),
            "only_ours_count": self.only_ours.len(),
            "only_ours": self.only_ours.iter().take(limit).map(row).collect::<Vec<_>>(),
            "only_theirs_count": self.only_theirs.len(),
            "only_theirs": self.only_theirs.iter().take(limit).map(row).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::tower::{Plate, tower_on};

    fn rows(m: &Model, kind: &str) -> usize {
        m.elements.values().filter(|e| e.category() == kind).count()
    }

    #[test]
    fn a_description_holds_what_another_tool_builds_the_model_again_from() {
        let m = tower_on(Plate::SMALL, 1).unwrap();
        let d = describe(&m);
        assert_eq!((d["format"].as_str(), d["version"].as_u64(), d["units"].as_str()), (Some(FORMAT), Some(1), Some("mm")));
        let len = |k: &str| d[k].as_array().map_or(0, Vec::len);
        assert_eq!(len("walls"), rows(&m, "wall"));
        assert_eq!(len("floors"), rows(&m, "floor"));
        assert_eq!(len("rooms"), rows(&m, "room"));
        assert_eq!(len("openings"), rows(&m, "door") + rows(&m, "window"));
        assert!(len("rooms") > 50 && len("openings") > 100, "{} rooms, {} openings", len("rooms"), len("openings"));
        // Every wall's type, level and every opening's host are in it.
        let guids = |k: &str| -> Vec<String> { d[k].as_array().unwrap().iter().map(|v| v["guid"].as_str().unwrap().into()).collect() };
        let (levels, walls) = (guids("levels"), guids("walls"));
        let types: Vec<&str> = d["wall_types"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for w in d["walls"].as_array().unwrap() {
            assert!(levels.contains(&w["level"].as_str().unwrap().to_string()));
            assert!(types.contains(&w["type"].as_str().unwrap()));
        }
        for o in d["openings"].as_array().unwrap() {
            assert!(walls.contains(&o["host"].as_str().unwrap().to_string()));
            assert!(o["width"].as_f64().unwrap() > 0.0);
        }
        // A wall's centre line is its location line moved to its middle: as long, and its type's half thickness from
        // each face.
        let (_, g, e) = m.elements.with_guids().find(|(_, _, e)| matches!(e, Element::Wall(_))).unwrap();
        let Element::Wall(w) = e else { panic!("a wall") };
        let dw = d["walls"].as_array().unwrap().iter().find(|v| v["guid"] == json!(g)).unwrap();
        let p = |v: &Value| P2(v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
        let (s, t) = (p(&dw["start"]), p(&dw["end"]));
        assert!((s.dist(t) - w.start.dist(w.end)).abs() < 1e-6);
        let (ext, int) = walls::faces(&m, w);
        let off = Axis::of(w).project(s).1;
        assert!((off - (ext + int) / 2.0).abs() < 1e-6, "{off} {ext} {int}");
        assert!(d["left_out"]["tag"].as_u64().unwrap() > 0, "{}", d["left_out"]);
    }

    #[test]
    fn our_numbers_are_those_the_model_shows() {
        let m = tower_on(Plate::SMALL, 1).unwrap();
        let rows = ours(&m);
        let rooms = Rooms::new(&m);
        let mut n = 0;
        for (id, _, e) in m.elements.with_guids() {
            let shown = crate::commands::element_json_with(&m, id, &rooms);
            let of = |q: &str| rows.iter().find(|r| r.id == id && r.quantity == q).map(|r| r.value);
            match e {
                Element::Room(_) => {
                    assert!((of("area_m2").unwrap() - shown["area_m2"].as_f64().unwrap()).abs() < 0.006);
                    assert!((of("perimeter_m").unwrap() - shown["perimeter_m"].as_f64().unwrap()).abs() < 0.006);
                    assert_eq!(of("enclosed"), Some(if shown["enclosed"] == json!(true) { 1.0 } else { 0.0 }));
                    assert!((of("volume_m3").unwrap() - shown["volume_m3"].as_f64().unwrap()).abs() < 0.006);
                    assert!((of("height_m").unwrap() * 1e3 - shown["unbounded_height"].as_f64().unwrap()).abs() < 1e-6);
                    n += 1;
                }
                Element::Floor(_) => {
                    assert!((of("area_m2").unwrap() - shown["area_m2"].as_f64().unwrap()).abs() < 0.006);
                    n += 1;
                }
                Element::Wall(w) => assert!((of("length_m").unwrap() * 1e3 - w.start.dist(w.end)).abs() < 1e-6),
                _ => {}
            }
        }
        assert!(n > 50);
        // Every room of the tower bounds.
        assert!(rows.iter().filter(|r| r.quantity == "enclosed").all(|r| r.value == 1.0));
    }

    #[test]
    fn a_comparison_names_each_difference_and_what_only_one_side_has() {
        let r = |kind: &str, id: Id, q: &str, v: f64| Row { kind: kind.into(), id, quantity: q.into(), value: v };
        let ours = [r("room", 1, "area_m2", 20.0), r("room", 1, "perimeter_m", 18.0), r("wall", 2, "length_m", 6.0)];
        let theirs = [
            r("room", 1, "area_m2", 20.00004),
            r("room", 1, "perimeter_m", 18.2),
            r("wall", 3, "length_m", 6.0),
            r("room", 1, "volume_m3", 50.0),
        ];
        let report = compare(&ours, &theirs);
        assert_eq!(report.compared, 2);
        assert_eq!(report.differ, vec![(ours[1].clone(), 18.2)]);
        assert_eq!(report.only_ours, vec![ours[2].clone()]);
        assert_eq!(report.only_theirs.len(), 2);
        assert_eq!(report.to_json(10)["differ"][0]["theirs"], 18.2);
    }

    #[test]
    fn a_table_of_results_is_read_strictly() {
        let rows = read("\u{feff}kind, id, quantity, value\nroom,100001,area_m2,20.5\n\nwall,100002,length_m,6\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, 20.5);
        let head = "kind,id,quantity,value\n";
        for bad in [
            String::new(),
            "kind,id\nroom,1".into(),
            format!("{head}room,1,area_m2,NaN"),
            format!("{head}room,1,area_m2"),
            format!("{head},1,area_m2,1"),
            format!("{head}room,a,area_m2,1"),
            format!("{head}room,-1,area_m2,1"),
        ] {
            assert!(read(&bad).is_err(), "{bad:?}");
        }
    }

    /// The small tower, two storeys, as Revit reports it (`tools/revit-crosscheck`): every quantity both have the same,
    /// within [`tolerance`].
    #[test]
    #[ignore = "needs Revit's numbers: run tools/revit-crosscheck and commit its table as tests/revit/small-tower-2.csv"]
    fn the_small_tower_has_the_numbers_revit_gives_it() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/revit/small-tower-2.csv");
        let theirs = read(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let m = tower_on(Plate::SMALL, 2).unwrap();
        let report = compare(&ours(&m), &theirs);
        assert!(report.compared > 1000, "{}", report.to_json(5));
        assert!(report.differ.is_empty(), "{}", report.to_json(20));
    }
}
