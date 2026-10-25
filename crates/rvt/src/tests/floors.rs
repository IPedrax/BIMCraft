//! Floors and ceilings: sketched boundaries, types, levels and offsets.

use super::walls::{base, import, level_named};
use crate::project_fixture::{Block, Project, Rec, TypeRec, layers_bytes, line_bytes, name_bytes};
use bimcraft_core::geom::area;
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Ceiling, Element, Floor, Id};

const FLOORS: i64 = -2_000_032;
const CEILINGS: i64 = -2_000_038;
const SKETCH: i64 = -2_000_045;

fn floors(i: &Imported) -> Vec<(Id, &Floor)> {
    i.model
        .elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::Floor(f) => Some((id, f)),
            _ => None,
        })
        .collect()
}

fn ceilings(i: &Imported) -> Vec<&Ceiling> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Ceiling(c) => Some(c),
            _ => None,
        })
        .collect()
}

/// A project with a floor type (a 0.5 ft slab: 152.4 mm) and a ceiling type (0.25 ft).
pub(super) fn project(release: u32) -> Project {
    let mut p = base(release);
    p.types.push(TypeRec::new(510, FLOORS));
    let mut payload = name_bytes("Slab");
    payload.extend(layers_bytes(&[(0.5, Some(701), 1)], release));
    p.data.push(Block { id: 510, payload });
    p.types.push(TypeRec::new(520, CEILINGS));
    let mut payload = name_bytes("Tiles");
    payload.extend(layers_bytes(&[(0.25, Some(700), 4)], release));
    p.data.push(Block { id: 520, payload });
    p
}

/// A sketch line of `owner` from `a` to `b` (feet) at height `z`, with its curve in its data when `data`.
fn line(p: &mut Project, owner: u32, id: u32, a: (f64, f64), b: (f64, f64), z: f64, data: bool) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let bbox = [a.0.min(b.0), a.1.min(b.1), z, a.0.max(b.0), a.1.max(b.1), z];
    let mut r = Rec::new(id, SKETCH).at(bbox, &[1]);
    r.refs2 = vec![30, u64::from(owner)];
    p.recs.push(r);
    if data {
        p.data.push(Block { id, payload: line_bytes(0.0, len, [a.0, a.1, z], [dx / len, dy / len, 0.0]) });
    }
}

/// The sketch of a rectangle `[x0, y0, x1, y1]` as four lines with ids from `first`.
pub(super) fn rectangle(p: &mut Project, owner: u32, first: u32, r: [f64; 4], z: f64, data: bool) {
    let [x0, y0, x1, y1] = r;
    for (k, (a, b)) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))].into_iter().enumerate() {
        line(p, owner, first + k as u32, a, b, z, data);
    }
}

/// A floor on level 1 whose top is at the level, 20 by 10 ft, with a 4 by 4 ft hole in it.
fn floor_with_hole(release: u32, data: bool) -> Project {
    let mut p = project(release);
    p.recs.push(Rec::new(100, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 510, 100]));
    rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 0.0, data);
    rectangle(&mut p, 100, 210, [5.0, 3.0, 9.0, 7.0], 0.0, data);
    p
}

#[test]
fn a_sketch_line_names_its_floor_wherever_in_its_second_list_the_id_sits() {
    // A real file names the owner among the ids of the second list: first, in the middle, last, and sometimes with what else the
    // line touches after it. The Snowdon sample's floors were left out because only the last slot was looked at.
    for slots in [vec![100u64], vec![30, 100], vec![30, 100, 999], vec![100, 30], vec![999, 100, 30]] {
        let mut p = floor_with_hole(2024, true);
        for r in p.recs.iter_mut().filter(|r| r.category == SKETCH) {
            r.refs2 = slots.clone();
        }
        let i = import(&p);
        assert_eq!(floors(&i).len(), 1, "{slots:?}: {:?}", i.skipped);
    }
}

#[test]
fn a_floor_comes_in_with_its_boundary_its_hole_its_type_level_and_offset() {
    // From the curves in the sketch lines' data (2024 and later) and from their boxes alone, as a file of 2023 or before has.
    for (release, data) in [(2024, true), (2026, true), (2024, false), (2023, false), (2019, true)] {
        let i = import(&floor_with_hole(release, data));
        let [(_, fl)] = floors(&i)[..] else { panic!("Revit {release}, data {data}: {:?}", i.skipped) };
        assert_eq!(fl.level, level_named(&i, "Level 1"));
        assert_eq!(fl.offset, 0.0);
        assert_eq!(fl.type_name, "Slab");
        assert_eq!(i.model.types.floor["Slab"].thickness, 152.4);
        let outer = area(&fl.boundary);
        assert!((outer - 6096.0 * 3048.0).abs() < 1.0 && fl.boundary.len() == 4, "{outer} {:?}", fl.boundary);
        assert_eq!(fl.openings.len(), 1);
        assert!((area(&fl.openings[0]).abs() - 1219.2 * 1219.2).abs() < 1.0);
        assert!(!fl.structural && fl.slope == 0.0 && fl.arrow.is_none());
        assert_eq!(i.skipped, Vec::<String>::new(), "Revit {release}, data {data}");
    }
}

#[test]
fn a_floors_top_is_its_offset_from_the_level_it_names() {
    let mut p = floor_with_hole(2024, true);
    // Its top 2 ft under level 1, on the level it names; and one that names none, on the level below its top.
    p.recs[0].bbox = [0.0, 0.0, -2.5, 20.0, 10.0, -2.0];
    for r in p.recs.iter_mut().filter(|r| r.category == SKETCH) {
        r.bbox[2] = -2.0;
        r.bbox[5] = -2.0;
    }
    let i = import(&p);
    let (_, fl) = floors(&i)[0];
    assert_eq!((fl.level, fl.offset), (level_named(&i, "Level 1"), -609.6));
    let mut q = floor_with_hole(2024, true);
    q.recs[0].refs = vec![510, 100];
    q.recs[0].bbox = [0.0, 0.0, 9.5, 20.0, 10.0, 10.0];
    for r in q.recs.iter_mut().filter(|r| r.category == SKETCH) {
        r.bbox[2] = 10.0;
        r.bbox[5] = 10.0;
    }
    let i = import(&q);
    let (_, fl) = floors(&i)[0];
    assert_eq!((fl.level, fl.offset), (level_named(&i, "Level 2"), 0.0), "{:?}", i.skipped);
}

#[test]
fn a_curved_edge_of_a_sketch_comes_in_as_chords_within_half_a_millimetre() {
    // A 10 ft square with its top-right corner cut by a quarter circle of radius 4 ft about (6, 6)... as an arc edge.
    let mut p = project(2024);
    p.recs.push(Rec::new(100, FLOORS).at([0.0, 0.0, -0.5, 10.0, 10.0, 0.0], &[10, 510, 100]));
    line(&mut p, 100, 200, (0.0, 0.0), (10.0, 0.0), 0.0, true);
    line(&mut p, 100, 201, (10.0, 0.0), (10.0, 6.0), 0.0, true);
    line(&mut p, 100, 203, (6.0, 10.0), (0.0, 10.0), 0.0, true);
    line(&mut p, 100, 204, (0.0, 10.0), (0.0, 0.0), 0.0, true);
    let half_pi = std::f64::consts::FRAC_PI_2;
    let mut r = Rec::new(202, SKETCH).at([6.0, 6.0, 0.0, 10.0, 10.0, 0.0], &[1]);
    r.refs2 = vec![30, 100];
    p.recs.push(r);
    p.data.push(Block {
        id: 202,
        payload: crate::project_fixture::arc_bytes([0.0, half_pi], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, [6.0, 6.0, 0.0]),
    });
    let i = import(&p);
    let [(_, fl)] = floors(&i)[..] else { panic!("{:?}", i.skipped) };
    // The square less a quarter-disc corner: the exact area is 100 − (16 − 4π)... the corner outside the arc, 16 − 4π ft2.
    let exact = (100.0 - (16.0 - 4.0 * std::f64::consts::PI)) * 304.8 * 304.8;
    assert!((area(&fl.boundary).abs() - exact).abs() / exact < 1e-4, "{} against {exact}", area(&fl.boundary));
    assert!(fl.boundary.len() > 12, "the arc is drawn as chords: {}", fl.boundary.len());
}

#[test]
fn a_diagonal_sketch_line_known_only_by_its_box_is_placed_where_one_pair_fits() {
    // A triangle: the diagonal's box is the whole 10 by 10 ft, and only one pair of open vertices is its corners.
    let mut p = project(2023);
    p.recs.push(Rec::new(100, FLOORS).at([0.0, 0.0, -0.5, 10.0, 10.0, 0.0], &[10, 510, 100]));
    line(&mut p, 100, 200, (0.0, 0.0), (10.0, 0.0), 0.0, false);
    line(&mut p, 100, 201, (10.0, 0.0), (10.0, 10.0), 0.0, false);
    line(&mut p, 100, 202, (10.0, 10.0), (0.0, 0.0), 0.0, false);
    let i = import(&p);
    let [(_, fl)] = floors(&i)[..] else { panic!("{:?}", i.skipped) };
    assert!((area(&fl.boundary).abs() - 50.0 * 304.8 * 304.8).abs() < 1.0);
}

#[test]
fn a_floor_the_reader_cannot_describe_comes_in_flat_with_the_reason_or_is_left_out_with_it() {
    let mut p = floor_with_hole(2024, true);
    // Sloped or stepped: its box is taller than its type is thick.
    p.recs.push(Rec::new(101, FLOORS).at([0.0, 0.0, -1.5, 20.0, 10.0, 0.0], &[10, 510, 101]));
    rectangle(&mut p, 101, 300, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    // A sketch that does not close.
    p.recs.push(Rec::new(102, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 510, 102]));
    rectangle(&mut p, 102, 310, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    p.recs.retain(|r| r.id != 313);
    // No sketch, no type, a type with no layers.
    p.recs.push(Rec::new(103, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 510, 103]));
    p.recs.push(Rec::new(104, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 104]));
    rectangle(&mut p, 104, 320, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    p.types.push(TypeRec::new(511, FLOORS));
    p.data.push(Block { id: 511, payload: name_bytes("Bare") });
    p.recs.push(Rec::new(105, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 511, 105]));
    rectangle(&mut p, 105, 330, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    let i = import(&p);
    // A floor the reader cannot describe still comes in where the file says enough: level, sloped or stepped, and as thick as its
    // own box; what it cannot read at all is left out with the reason.
    assert_eq!(floors(&i).len(), 4, "{:?}", i.skipped);
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    assert!(said("sloped or stepped").contains("101") && said("comes in level").contains("101"), "{:?}", i.skipped);
    assert!(said("do not close").contains("102") && said("joined by a straight line").contains("102"));
    assert!(said("no sketch lines").contains("103"));
    assert!(said("does not name one floor type").contains("104"));
    assert!(said("no layers").contains("105") && said("as thick as its own box").contains("105"));
}

#[test]
fn a_sketch_of_separate_loops_makes_a_floor_for_each() {
    let mut p = floor_with_hole(2024, true);
    rectangle(&mut p, 100, 220, [30.0, 0.0, 40.0, 10.0], 0.0, true);
    p.recs[0].bbox = [0.0, 0.0, -0.5, 40.0, 10.0, 0.0];
    let i = import(&p);
    assert_eq!(floors(&i).len(), 2, "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("separate loops") && s.contains("100")));
    // A loop that crosses another is no region the reader can follow: the floor comes in as the footprint of its box, and says so.
    let mut q = floor_with_hole(2024, true);
    rectangle(&mut q, 100, 220, [8.0, 5.0, 12.0, 8.0], 0.0, true);
    let i = import(&q);
    assert_eq!(floors(&i).len(), 1, "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("footprint of its own box") && s.contains("100")), "{:?}", i.skipped);
}

#[test]
fn a_ceiling_is_its_undersides_level_and_offset() {
    let mut p = project(2024);
    p.recs.push(Rec::new(100, CEILINGS).at([0.0, 0.0, 8.0, 20.0, 10.0, 8.25], &[10, 520, 100]));
    rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 8.25, true);
    let i = import(&p);
    let [c] = ceilings(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((c.level, c.offset, c.type_name.as_str()), (level_named(&i, "Level 1"), 2438.4, "Tiles"));
    assert_eq!(i.model.types.ceiling["Tiles"].thickness, 76.2);
    assert!((area(&c.boundary) - 6096.0 * 3048.0).abs() < 1.0);
}

#[test]
fn the_floors_and_ceilings_of_the_architectural_sample_are_those_of_rvt_rss_export() {
    use super::rvt_rs::{RAC_CEILINGS, RAC_FLOORS};
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        let find = |label: &str, id: u32| -> Vec<(Id, &Element)> {
            i.model.elements.iter().filter(|(m, _)| from[*m] == format!("{label} {id}")).map(|(m, e)| (*m, e)).collect()
        };
        for theirs in &RAC_FLOORS {
            let found = find("Floor", theirs.id);
            let [(_, Element::Floor(fl))] = found[..] else { panic!("{}: floor {} is missing", path.display(), theirs.id) };
            let elevation = i.model.elevation(fl.level);
            let top = elevation + fl.offset;
            let bottom = top - i.model.types.floor[&fl.type_name].thickness;
            assert!(
                (elevation - theirs.storey).abs() < 0.11 && (top - theirs.top).abs() < 0.11 && (bottom - theirs.bottom).abs() < 0.11,
                "floor {}",
                theirs.id
            );
            let net = area(&fl.boundary).abs() - fl.openings.iter().map(|o| area(o).abs()).sum::<f64>();
            // A floor whose sketch does not close comes in as the footprint of its box, which is not the area another reader has
            // of it: it is compared to within its own outline's error, and the report says what of it is not read.
            let boxed = i.skipped.iter().any(|s| s.contains("footprint of its own box") && s.contains(&theirs.id.to_string()));
            let slack = if boxed { 0.1 * theirs.area } else { 0.005 };
            assert!((net / 1e6 - theirs.area).abs() < slack, "floor {}: {} m2 against {}", theirs.id, net / 1e6, theirs.area);
        }
        for theirs in &RAC_CEILINGS {
            let found = find("Ceiling", theirs.id);
            let [(_, Element::Ceiling(c))] = found[..] else { panic!("{}: ceiling {} is missing", path.display(), theirs.id) };
            let elevation = i.model.elevation(c.level);
            let under = elevation + c.offset;
            assert!((elevation - theirs.storey).abs() < 0.11 && (under - theirs.bottom).abs() < 0.11, "ceiling {}", theirs.id);
            assert!((under + i.model.types.ceiling[&c.type_name].thickness - theirs.top).abs() < 0.11);
            let net = area(&c.boundary).abs() - c.openings.iter().map(|o| area(o).abs()).sum::<f64>();
            assert!((net / 1e6 - theirs.area).abs() < 0.005, "ceiling {}: {} m2 against {}", theirs.id, net / 1e6, theirs.area);
        }
        assert_eq!(floors(&i).len(), 8, "{}: {:?}", path.display(), i.skipped);
        assert_eq!(ceilings(&i).len(), 2);
    }
}

#[test]
fn the_structural_samples_five_floors_come_in_at_every_release() {
    use super::rvt_rs::RST_FLOORS;
    for path in super::samples() {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        // Five floors of the file are the ones a second reader writes (rvt-rs); a floor whose sketch does not close comes in too,
        // as the footprint of its box, and the report says so, so the count is five or more.
        assert!(floors(&i).len() >= 5, "{}: {:?}", path.display(), i.skipped);
        for theirs in &RST_FLOORS {
            let (m, fl) = floors(&i)
                .into_iter()
                .find(|(m, _)| from[m] == format!("Floor {}", theirs.id))
                .unwrap_or_else(|| panic!("floor {}", theirs.id));
            let _ = m;
            let top = i.model.elevation(fl.level) + fl.offset;
            let bottom = top - i.model.types.floor[&fl.type_name].thickness;
            assert!((top - theirs.top).abs() < 0.11 && (bottom - theirs.bottom).abs() < 0.11, "floor {} of {}", theirs.id, path.display());
            let net = area(&fl.boundary).abs() - fl.openings.iter().map(|o| area(o).abs()).sum::<f64>();
            // A floor whose sketch does not close comes in as the footprint of its box, which is not the area another reader has
            // of it: it is compared to within its own outline's error, and the report says what of it is not read.
            let boxed = i.skipped.iter().any(|s| s.contains("footprint of its own box") && s.contains(&theirs.id.to_string()));
            let slack = if boxed { 0.1 * theirs.area } else { 0.005 };
            assert!((net / 1e6 - theirs.area).abs() < slack, "floor {}: {} m2 against {}", theirs.id, net / 1e6, theirs.area);
        }
    }
}

#[test]
fn a_mutation_sweep_over_a_project_of_floors_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = floor_with_hole(release, release >= 2024);
        p.recs.push(Rec::new(101, CEILINGS).at([0.0, 0.0, 8.0, 20.0, 10.0, 8.25], &[10, 520, 101]));
        rectangle(&mut p, 101, 300, [0.0, 0.0, 20.0, 10.0], 8.25, release >= 2024);
        p.members = 3;
        let clean = import(&p);
        assert_eq!((floors(&clean).len(), ceilings(&clean).len()), (1, 1), "{:?}", clean.skipped);
        let bytes = p.partition_bytes();
        let mut seed = 0x1357_9bdf_2468_ace0u64 ^ u64::from(release);
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..800 {
            let mut bad = bytes.clone();
            for _ in 0..1 + next() % 8 {
                let at = (next() % bad.len() as u64) as usize;
                bad[at] = match next() % 4 {
                    0 => 0xff,
                    1 => 0,
                    _ => next() as u8,
                };
            }
            if round % 7 == 0 {
                bad.truncate(bad.len() - (next() % 400) as usize);
            }
            if let Ok(i) = crate::import(&p.file_from(&bad), "x") {
                assert!(i.model.check().is_ok(), "round {round}");
                assert!(floors(&i).len() <= 3 && ceilings(&i).len() <= 3, "round {round}");
            }
        }
    }
}

#[test]
fn floor_and_ceiling_types_of_one_name_are_one_type_when_their_layers_match_and_two_when_they_do_not() {
    let mut p = project(2024);
    // 511 and 521 repeat 510 and 520; 512 and 522 are of the other material.
    for (tid, category, name, layers) in [
        (511, FLOORS, "Slab", (0.5, 701)),
        (512, FLOORS, "Slab", (0.5, 700)),
        (521, CEILINGS, "Tiles", (0.25, 700)),
        (522, CEILINGS, "Tiles", (0.25, 701)),
    ] {
        p.types.push(TypeRec::new(tid, category));
        let function = if category == FLOORS { 1 } else { 4 };
        let mut payload = name_bytes(name);
        payload.extend(layers_bytes(&[(layers.0, Some(layers.1), function)], 2024));
        p.data.push(Block { id: tid, payload });
    }
    for (k, (category, tid)) in
        [(FLOORS, 510), (FLOORS, 511), (FLOORS, 512), (CEILINGS, 520), (CEILINGS, 521), (CEILINGS, 522)].into_iter().enumerate()
    {
        let (id, x) = (100 + k as u32, 30.0 * k as f64);
        let (z0, z1) = if category == FLOORS { (-0.5, 0.0) } else { (8.0, 8.25) };
        let z = if category == FLOORS { 0.0 } else { 8.25 };
        p.recs.push(Rec::new(id, category).at([x, 0.0, z0, x + 20.0, 10.0, z1], &[10, tid, u64::from(id)]));
        rectangle(&mut p, id, 200 + 10 * k as u32, [x, 0.0, x + 20.0, 10.0], z, true);
    }
    let i = import(&p);
    let floor_names: Vec<&str> = floors(&i).iter().map(|(_, f)| f.type_name.as_str()).collect();
    let ceiling_names: Vec<&str> = ceilings(&i).iter().map(|c| c.type_name.as_str()).collect();
    assert_eq!(floor_names, ["Slab", "Slab", "Slab (2)"], "{:?}", i.skipped);
    assert_eq!(ceiling_names, ["Tiles", "Tiles", "Tiles (2)"], "{:?}", i.skipped);
    assert!(i.skipped.is_empty(), "a sketch of one loop needs no note: {:?}", i.skipped);
}

#[test]
fn a_floor_is_as_thick_as_its_type_to_within_a_millimetre_and_a_type_of_no_width_is_read_as_its_box() {
    let mut p = project(2024);
    // Thicker than 152.4 mm by 0.88 mm: a floor; by 1.13 mm: a sloped or stepped one.
    p.recs.push(Rec::new(100, FLOORS).at([0.0, 0.0, -0.5029, 20.0, 10.0, 0.0], &[10, 510, 100]));
    rectangle(&mut p, 100, 200, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    p.recs.push(Rec::new(101, FLOORS).at([0.0, 0.0, -0.5037, 20.0, 10.0, 0.0], &[10, 510, 101]));
    rectangle(&mut p, 101, 210, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    // A type that is only a membrane.
    p.types.push(TypeRec::new(511, FLOORS));
    let mut payload = name_bytes("Film");
    payload.extend(layers_bytes(&[(0.0, None, 100)], 2024));
    p.data.push(Block { id: 511, payload });
    p.recs.push(Rec::new(102, FLOORS).at([0.0, 0.0, -0.5, 20.0, 10.0, 0.0], &[10, 511, 102]));
    rectangle(&mut p, 102, 220, [0.0, 0.0, 20.0, 10.0], 0.0, true);
    let i = import(&p);
    assert_eq!(floors(&i).len(), 3, "{:?}", i.skipped);
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    assert!(said("sloped or stepped").contains("101") && !said("sloped or stepped").contains("100"), "{:?}", i.skipped);
    assert!(said("no layers").contains("102"), "{:?}", i.skipped);
    assert_eq!(i.model.types.floor.values().find(|t| t.thickness == 152.4).map(|t| t.thickness), Some(152.4));
}
