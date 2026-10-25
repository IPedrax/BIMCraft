//! Walls: their lines and arcs, heights, levels, types, layers and the side their exterior is on.

use crate::project_fixture::{Block, Lev, Mat, Project, Rec, TypeRec, arc_bytes, layers_bytes, line_bytes, name_bytes, orientation_bytes};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Function, Id, Wall};

pub(super) const WALLS: i64 = -2_000_011;

pub(super) fn import(p: &Project) -> Imported {
    crate::import(&p.file(), "house").unwrap()
}

/// The import, and what each element made was in the file.
pub(super) fn import_with_origins(p: &Project) -> (Imported, std::collections::HashMap<Id, String>) {
    let mut r = crate::Revit::from_bytes(p.file(), crate::Limits::default()).unwrap();
    crate::import::build(&mut r, "house").unwrap()
}

pub(super) fn walls(i: &Imported) -> Vec<(Id, &Wall)> {
    i.model
        .elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::Wall(w) => Some((id, w)),
            _ => None,
        })
        .collect()
}

/// The GUID of the level named `name`: what an element's `level` holds.
pub(super) fn level_named(i: &Imported, name: &str) -> bimcraft_core::ids::Guid {
    i.model.levels().into_iter().find(|(_, l)| l.name == name).map(|(id, _)| i.model.guid(id)).unwrap()
}

/// Two levels (0 and 10 ft), two materials, a two-layer wall type (a 0.25 ft brick finish over 0.5 ft of concrete) and no wall.
pub(super) fn base(release: u32) -> Project {
    let mut p = Project::new(release);
    p.levels = vec![Lev::new(10, "Level 1", 0.0), Lev::new(11, "Level 2", 10.0)];
    p.mats = vec![Mat { id: 700, name: "Brick".into() }, Mat { id: 701, name: "Concrete".into() }];
    p.types = vec![TypeRec::new(500, WALLS), TypeRec::new(700, -2_000_700), TypeRec::new(701, -2_000_700)];
    p.types[1].kind = crate::project_fixture::KIND_MATERIAL;
    p.types[2].kind = crate::project_fixture::KIND_MATERIAL;
    let mut payload = name_bytes("Brick wall");
    payload.extend(layers_bytes(&[(0.25, Some(700), 4), (0.5, Some(701), 1)], release));
    p.data = vec![Block { id: 500, payload }];
    p
}

/// A wall 20 ft long from (0, 0) to (20, 0), 10 ft tall from level 1 to level 2, of the wall type.
pub(super) fn straight(p: &mut Project, id: u32, flip: bool) {
    let word = !(2015..=2020).contains(&p.release); // Revit 2015 to 2020 do not write the word
    p.recs.push(Rec::new(id, WALLS).at([0.0, -0.375, 0.0, 20.0, 0.375, 10.0], &[10, 11, 500, u64::from(id)]));
    let mut payload = line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    payload.extend(orientation_bytes(0, 1, flip, word));
    p.data.push(Block { id, payload });
}

#[test]
fn a_straight_wall_comes_in_with_its_line_height_levels_type_and_layers() {
    for release in [2019, 2023, 2024, 2026] {
        let mut p = base(release);
        straight(&mut p, 100, false);
        let i = import(&p);
        let [(_, w)] = walls(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        assert_eq!((w.start.0, w.start.1, w.end.0, w.end.1), (0.0, 0.0, 6096.0, 0.0), "Revit {release}");
        assert_eq!((w.level, w.top_level), (level_named(&i, "Level 1"), Some(level_named(&i, "Level 2"))));
        assert_eq!((w.height, w.base_offset, w.arc), (3048.0, 0.0, 0.0));
        let t = &i.model.types.wall[&w.type_name];
        assert_eq!(w.type_name, "Brick wall");
        assert_eq!(t.thickness, 228.6);
        // The layers by what they are made of: the library gives each its own id and GUID once the project is made.
        let layers: Vec<(Function, &str, f64)> = t.layers.iter().map(|l| (l.function, l.material.as_str(), l.thickness)).collect();
        assert_eq!(layers, [(Function::Finish, "Brick", 76.2), (Function::Structure, "Concrete", 152.4)]);
        assert!(i.model.materials.contains_key("Brick") && i.model.materials.contains_key("Concrete"));
        assert_eq!(i.skipped, Vec::<String>::new(), "Revit {release}");
    }
}

#[test]
fn a_material_a_32_bit_file_does_not_name_in_its_object_is_named_by_its_data() {
    // Up to 2023 the object may carry no name (here its own name field is a parameter entry); the material's data holds it.
    let mut p = base(2023);
    straight(&mut p, 100, false);
    p.mats[0].name = String::new();
    let mut named = crate::project_fixture::name_bytes("Brick");
    named.extend_from_slice(&[0x12; 4]);
    p.data.push(Block { id: 700, payload: named });
    let i = import(&p);
    let ty = &i.model.types.wall["Brick wall"];
    assert_eq!(ty.layers[0].material, "Brick", "{:?}", i.skipped);
    // From 2024 the object is where the name is, and a name only in the data is not read.
    let mut q = base(2024);
    straight(&mut q, 100, false);
    q.mats[0].name = String::new();
    q.data.push(Block { id: 700, payload: crate::project_fixture::name_bytes("Brick") });
    assert_eq!(import(&q).model.types.wall["Brick wall"].layers[0].material, "");
}

#[test]
fn a_wall_whose_exterior_is_to_the_right_of_its_line_is_turned_round() {
    let mut p = base(2024);
    straight(&mut p, 100, true);
    let i = import(&p);
    let (_, w) = walls(&i)[0];
    assert_eq!((w.start.0, w.end.0), (6096.0, 0.0), "the exterior is to the left of a BIMCraft wall");
    // With no orientation in its data the wall stays as drawn, and the import says so.
    let mut q = base(2024);
    straight(&mut q, 100, false);
    q.data[1].payload.truncate(8 + 4 + 8 * 8); // the line, without the orientation after it
    let i = import(&q);
    let (_, w) = walls(&i)[0];
    assert_eq!((w.start.0, w.end.0), (0.0, 6096.0));
    assert!(
        i.skipped.iter().any(|s| s.contains("1 wall:") && s.contains("which side is the exterior") && s.contains("ids 100")),
        "{:?}",
        i.skipped
    );
}

#[test]
fn a_curved_wall_comes_in_as_the_arc_it_turns() {
    let half_pi = std::f64::consts::FRAC_PI_2;
    // A quarter circle of radius 10 ft about (0, 0), from (10, 0) to (0, 10), counter-clockwise seen from above.
    let ccw = arc_bytes([0.0, half_pi], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, [0.0, 0.0, 0.0]);
    // The same quarter with a flipped frame (its axes cross downward): it runs clockwise from (10, 0) to (0, -10).
    let cw = arc_bytes([0.0, half_pi], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0], 10.0, [0.0, 0.0, 0.0]);
    for (arc_data, flip, ends, turns) in [
        (ccw.clone(), false, ((3048.0, 0.0), (0.0, 3048.0)), 90.0),
        (cw, false, ((3048.0, 0.0), (0.0, -3048.0)), -90.0),
        (ccw, true, ((0.0, 3048.0), (3048.0, 0.0)), -90.0),
    ] {
        let mut p = base(2024);
        p.recs.push(Rec::new(100, WALLS).at([0.0, -10.4, 0.0, 10.4, 10.4, 10.0], &[10, 11, 500, 100]));
        let mut payload = arc_data;
        payload.extend(orientation_bytes(0, 1, flip, true));
        p.data.push(Block { id: 100, payload });
        let i = import(&p);
        let [(_, w)] = walls(&i)[..] else { panic!("{:?}", i.skipped) };
        assert_eq!(((w.start.0, w.start.1), (w.end.0, w.end.1)), ends, "flip {flip}");
        assert!((w.arc - turns).abs() < 1e-6, "turns {} not {turns}", w.arc);
    }
}

#[test]
fn a_walls_levels_come_from_the_two_it_names_and_its_box() {
    let wall = |refs: &[u64], bbox: [f64; 6]| {
        let mut p = base(2024);
        p.levels.push(Lev::new(12, "Level 3", 20.0));
        p.recs.push(Rec::new(100, WALLS).at(bbox, refs));
        let mut payload = line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        payload.extend(orientation_bytes(0, 1, false, true));
        p.data.push(Block { id: 100, payload });
        let i = import(&p);
        let w = walls(&i).first().map(|(_, w)| (*w).clone());
        (i, w)
    };
    let low = [0.0, -0.4, 0.0, 20.0, 0.4];
    let with_top = |top: f64| [low[0], low[1], low[2], low[3], low[4], top];
    // Up to the level named: its box ends at its elevation.
    let (i, w) = wall(&[12, 10, 500], with_top(20.0));
    let w = w.unwrap();
    assert_eq!((w.level, w.top_level, w.height), (level_named(&i, "Level 1"), Some(level_named(&i, "Level 3")), 6096.0));
    // A box that ends short of the level it is constrained to: as tall as its box, unconnected.
    let (i, w) = wall(&[11, 10, 500], with_top(9.0));
    let w = w.unwrap();
    assert_eq!((w.level, w.top_level, w.height), (level_named(&i, "Level 1"), None, 2743.2));
    // The base is the higher level at or below the box's bottom, whichever way the list names them.
    let (i, w) = wall(&[11, 12, 500], [0.0, -0.4, 10.0, 20.0, 0.4, 19.0]);
    let w = w.unwrap();
    assert_eq!((w.level, w.base_offset), (level_named(&i, "Level 2"), 0.0));
    // A wall that starts above its level: an offset.
    let (i, w) = wall(&[10, 500], [0.0, -0.4, 1.0, 20.0, 0.4, 9.0]);
    let w = w.unwrap();
    assert_eq!((w.level, w.base_offset, w.top_level, w.height), (level_named(&i, "Level 1"), 304.8, None, 2438.4));
    // A level it names that is not a project level is not one; none named, the level at or below the box's bottom.
    let (i, w) = wall(&[999, 500], [0.0, -0.4, 10.5, 20.0, 0.4, 19.0]);
    assert_eq!(w.unwrap().level, level_named(&i, "Level 2"));
}

#[test]
fn the_newest_copy_of_a_walls_data_and_record_is_the_current_one() {
    use crate::fixture::{self, container};
    let mut older = base(2024);
    straight(&mut older, 100, false);
    let mut newer = older.clone();
    newer.recs.clear();
    newer.data[1].payload = {
        let mut b = line_bytes(0.0, 10.0, [0.0, 5.0, 0.0], [1.0, 0.0, 0.0]);
        b.extend(orientation_bytes(0, 1, false, true));
        b
    };
    newer.recs.push(Rec::new(100, WALLS).at([0.0, 4.6, 0.0, 10.0, 5.4, 10.0], &[10, 11, 500, 100]));
    let basic = fixture::basic_file_info(&fixture::Basic { release: 2024, ..Default::default() });
    let schema = fixture::gzip(&Project::schema_bytes());
    let table = fixture::global(&newer.element_table());
    let (a, b) = (fixture::partition(&[&older.partition_bytes()]), fixture::partition(&[&newer.partition_bytes()]));
    let wall_in = |at21: &[u8], at22: &[u8]| {
        let streams: [(&str, &[u8]); 5] = [
            ("BasicFileInfo", &basic),
            ("Formats/Latest", &schema),
            ("Global/ElemTable", &table),
            ("Partitions/21", at21),
            ("Partitions/22", at22),
        ];
        let i = crate::import(&container(&streams), "x").unwrap();
        let (_, w) = walls(&i)[0];
        (w.start.1, w.end.0)
    };
    // The higher-numbered partition is the newer one, whichever stream the name sorts first in.
    assert_eq!(wall_in(&a, &b), (1524.0, 3048.0));
    assert_eq!(wall_in(&b, &a), (0.0, 6096.0));
}

#[test]
fn a_wall_the_reader_cannot_describe_is_left_out_with_the_reason_and_the_others_stay() {
    let mut p = base(2024);
    straight(&mut p, 100, false);
    // No data at all.
    p.recs.push(Rec::new(101, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 500, 101]));
    // A line that is not level.
    p.recs.push(Rec::new(102, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 500, 102]));
    p.data.push(Block { id: 102, payload: line_bytes(0.0, 5.0, [0.0, 0.0, 0.0], [0.6, 0.0, 0.8]) });
    // A record that names no wall type, and one that names two.
    p.recs.push(Rec::new(103, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 103]));
    p.types.push(TypeRec::new(501, WALLS));
    p.recs.push(Rec::new(104, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 500, 501, 104]));
    for id in [103, 104] {
        p.data.push(Block { id, payload: line_bytes(0.0, 5.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]) });
    }
    // A type with no layers.
    p.types.push(TypeRec::new(502, WALLS));
    p.data.push(Block { id: 502, payload: name_bytes("Curtain") });
    p.recs.push(Rec::new(105, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 502, 105]));
    p.data.push(Block { id: 105, payload: line_bytes(0.0, 5.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]) });
    // A symbol with no volume, and one that is not placed.
    p.recs.push(Rec::new(106, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 0.0], &[10, 11, 500, 106]));
    p.recs.push(Rec { placed: false, ..Rec::new(107, WALLS).at([0.0, 0.0, 0.0, 5.0, 0.3, 10.0], &[10, 11, 500, 107]) });
    let i = import(&p);
    assert_eq!(walls(&i).len(), 5, "{:?}", i.skipped);
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    // A wall with no location line comes in along its own box; one whose line is not level is left out.
    assert!(said("no location line").contains("101") && said("long side of its own box").contains("101"), "{:?}", i.skipped);
    assert!(said("not level").contains("102"));
    // Its type holds a name and no layers: a curtain wall or a storefront, which comes in as the plane its own box gives.
    assert!(said("its type has no layers").contains("103, 104"), "{:?}", i.skipped);
    // A type with no layers at all is a curtain wall or a storefront: it comes in as the plane its own box gives.
    assert!(said("comes in as the plane").contains("105"), "{:?}", i.skipped);
    assert!(
        !i.skipped.iter().any(|s| s.contains("106") || s.contains("107")),
        "a symbol is not a wall and an unplaced record is not an instance"
    );
}

#[test]
fn types_with_one_name_and_different_layers_are_told_apart() {
    let mut p = base(2024);
    straight(&mut p, 100, false);
    p.types.push(TypeRec::new(503, WALLS));
    let mut payload = name_bytes("Brick wall");
    payload.extend(layers_bytes(&[(0.5, Some(701), 1)], 2024));
    p.data.push(Block { id: 503, payload });
    p.recs.push(Rec::new(101, WALLS).at([0.0, 4.6, 0.0, 10.0, 5.4, 10.0], &[10, 11, 503, 101]));
    p.data.push(Block { id: 101, payload: line_bytes(0.0, 10.0, [0.0, 5.0, 0.0], [1.0, 0.0, 0.0]) });
    // A third wall of the first type shares its model type.
    p.recs.push(Rec::new(102, WALLS).at([0.0, 9.6, 0.0, 10.0, 10.4, 10.0], &[10, 11, 500, 102]));
    p.data.push(Block { id: 102, payload: line_bytes(0.0, 10.0, [0.0, 10.0, 0.0], [1.0, 0.0, 0.0]) });
    let i = import(&p);
    let mut names: Vec<&str> = walls(&i).iter().map(|(_, w)| w.type_name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Brick wall", "Brick wall", "Brick wall (2)"]);
    assert_eq!(i.model.types.wall["Brick wall (2)"].thickness, 152.4);
}

#[test]
fn walls_whose_types_have_no_layers_come_in_as_the_plane_their_own_boxes_give() {
    let mut p = base(2024);
    straight(&mut p, 100, false);
    // A type that holds a name and no layers (a storefront), under two walls, and one that holds nothing at all.
    p.types.push(TypeRec::new(503, WALLS));
    p.types.push(TypeRec::new(504, WALLS));
    p.data.push(Block { id: 503, payload: name_bytes("Storefront") });
    for (id, ty, y) in [(101, 503, 5.0), (102, 503, 8.0), (103, 504, 10.0)] {
        p.recs.push(Rec::new(id, WALLS).at([0.0, y - 0.4, 0.0, 10.0, y + 0.4, 10.0], &[10, 11, ty, u64::from(id)]));
        p.data.push(Block { id, payload: line_bytes(0.0, 10.0, [0.0, y, 0.0], [1.0, 0.0, 0.0]) });
    }
    let i = import(&p);
    // A type with no compound structure is a curtain wall or a storefront: each of its walls comes in as the plane its own box
    // gives, and the note says what of it is not read. A type whose data says nothing at all is named by its thickness, as a
    // floor's is.
    assert_eq!(walls(&i).len(), 4, "{:?}", i.skipped);
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    assert!(said("comes in as the plane").contains("3 walls"), "{:?}", i.skipped);
    for id in ["101", "102", "103"] {
        assert!(said("comes in as the plane").contains(id), "{id}: {:?}", i.skipped);
    }
    assert!(i.model.types.wall["Storefront"].layers.len() == 1, "one layer, of the box's thickness");
}

#[test]
fn the_walls_of_the_architectural_sample_are_those_of_rvt_rss_export_of_the_same_file() {
    use super::rvt_rs::RAC_WALLS;
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        let at = |id: u32| walls(&i).into_iter().find(|(m, _)| from[m] == format!("Wall {id}"));
        // All 56: the nine curtain walls come in as the plane their own boxes give, and the note says what of them is not read.
        assert_eq!(walls(&i).len(), 56, "{}: {:?}", path.display(), i.skipped);
        assert!(i.skipped.iter().any(|s| s.contains("9 walls") && s.contains("curtain wall")), "{:?}", i.skipped);
        for theirs in &RAC_WALLS {
            let id = theirs.id;
            let (m, w) = at(id).unwrap_or_else(|| panic!("{}: wall {id} is missing", path.display()));
            let elevation = i.model.elevation(w.level);
            let z0 = elevation + w.base_offset;
            assert!(
                (elevation - theirs.storey).abs() < 0.11 && (z0 - theirs.bottom).abs() < 0.11,
                "wall {id}: bottom {z0} on {elevation}, rvt-rs {} on {}",
                theirs.bottom,
                theirs.storey
            );
            assert!((z0 + w.height - theirs.top).abs() < 0.11, "wall {id}: top {} not {}", z0 + w.height, theirs.top);
            // Its layers: how many, how thick, each named (a layer the file gives no material of its own takes its category's).
            let ty = &i.model.types.wall[&w.type_name];
            let mine: Vec<f64> = ty.layers.iter().filter(|l| l.thickness > 0.0).map(|l| l.thickness).collect();
            assert_eq!(mine.len(), theirs.layers.len(), "wall {id}");
            assert!(
                mine.iter().zip(theirs.layers).all(|(a, b)| (a - b).abs() < 0.11),
                "wall {id}: layers {mine:?} against {:?}",
                theirs.layers
            );
            assert!(
                ty.layers.iter().filter(|l| l.thickness > 0.0).all(|l| !l.material.is_empty()),
                "wall {id}: a layer with no material name"
            );
            // The exterior is to the left of the line, and the line runs down the middle of rvt-rs's body.
            let (dx, dy) = (w.end.0 - w.start.0, w.end.1 - w.start.1);
            let left = (-dy / dx.hypot(dy), dx / dx.hypot(dy));
            assert!(
                left.0 * f64::from(theirs.exterior[0]) + left.1 * f64::from(theirs.exterior[1]) > 0.5,
                "wall {id}: its exterior is on the other side"
            );
            let [x0, y0, x1, y1] = theirs.plan;
            if dx.abs().min(dy.abs()) < 0.01 {
                let (thin, centre, here) =
                    if dx.abs() < dy.abs() { (x1 - x0, (x0 + x1) / 2.0, w.start.0) } else { (y1 - y0, (y0 + y1) / 2.0, w.start.1) };
                assert!(
                    (thin - ty.thickness).abs() < 0.2 && (centre - here).abs() < 0.2,
                    "wall {id}: {thin} thick at {centre}, here {} at {here}",
                    ty.thickness
                );
            }
            // Along it, within the thickest wall that could cut it back (500 mm covers the sample's).
            let ring = bimcraft_core::walls::outline(&i.model, m);
            let (a, b) = ring.iter().fold(((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)), |(lo, hi), p| {
                ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1)))
            });
            for (mine, theirs) in [(a.0, x0), (a.1, y0), (b.0, x1), (b.1, y1)] {
                assert!((mine - theirs).abs() <= 520.0, "wall {id}: plan box {mine} against rvt-rs's {theirs}");
            }
        }
    }
}

#[test]
fn the_structural_samples_nine_walls_come_in_at_every_release() {
    // rvt-rs's 2024 export writes nine IfcWall for it too. (The union of their footprints, 31.077 m2, against the import's
    // 30.915 m2, was compared once with a scratch script; the 2023 export of rvt-rs draws walls as boxes and cannot be compared.)
    let mut shape = Vec::new();
    for path in super::samples() {
        let i = crate::import_path(&path, "x").unwrap();
        let ws = walls(&i);
        assert_eq!(ws.len(), 9, "{}: {:?}", path.display(), i.skipped);
        let mut s: Vec<(String, i64, i64)> =
            ws.iter().map(|(_, w)| (w.type_name.clone(), (w.start.0 * 10.0).round() as i64, (w.height * 10.0).round() as i64)).collect();
        s.sort();
        shape.push(s);
    }
    // The same model saved by four releases: the same walls.
    assert!(shape.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn a_mutation_sweep_over_a_project_of_walls_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        straight(&mut p, 100, false);
        straight(&mut p, 101, true);
        let half_pi = std::f64::consts::FRAC_PI_2;
        p.recs.push(Rec::new(102, WALLS).at([0.0, -10.4, 0.0, 10.4, 10.4, 10.0], &[10, 11, 500, 102]));
        let mut arc = arc_bytes([0.0, half_pi], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, [0.0, 0.0, 0.0]);
        arc.extend(orientation_bytes(0, 1, false, !(2015..=2020).contains(&release)));
        p.data.push(Block { id: 102, payload: arc });
        p.members = 3;
        let clean = import(&p);
        assert_eq!(walls(&clean).len(), 3, "{:?}", clean.skipped);
        let bytes = p.partition_bytes();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ u64::from(release);
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let (mut ok, mut failed) = (0, 0);
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
            match crate::import(&p.file_from(&bad), "x") {
                Ok(i) => {
                    ok += 1;
                    assert!(i.model.check().is_ok(), "round {round}");
                    assert!(walls(&i).len() <= 3, "round {round}: a wall that is not in the file");
                }
                Err(_) => failed += 1,
            }
        }
        assert!(ok > 300 && failed < 500, "Revit {release}: {ok} imported, {failed} refused");
    }
}

#[test]
fn a_real_architectural_file_damaged_in_its_partition_is_an_error_or_fewer_walls_never_a_panic() {
    let Some(path) = super::samples_of("racbasicsampleproject.rvt").into_iter().next() else { return };
    let original = std::fs::read(&path).unwrap();
    let whole = crate::import(&original, "x").unwrap();
    let limits = crate::Limits { max_inflated: 256 << 20, ..crate::Limits::default() };
    let mut seed = 0xfeed_beef_1234_5678u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..6 {
        let mut bytes = original.clone();
        for _ in 0..1 + next() % 30 {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        if round % 3 == 2 {
            bytes.truncate(bytes.len() - (next() % 300_000) as usize);
        }
        let got = crate::Revit::from_bytes(bytes, limits)
            .map_err(|e| e.to_string())
            .and_then(|mut r| crate::import::build(&mut r, "x").map(|(i, _)| i));
        if let Ok(i) = got {
            assert!(i.model.check().is_ok());
            assert!(walls(&i).len() <= walls(&whole).len() + 3, "round {round}");
        }
    }
}

#[test]
fn a_large_real_architectural_model_imports_without_trouble_and_what_comes_in_checks_out() {
    // Snowdon Towers (architectural, 2024): about 1,100 walls, 29,957 records and 442 MB of partition.
    for path in super::samples_of("snowdon-towers-sample-architectural.rvt") {
        let i = crate::import_path(&path, "x").unwrap();
        assert!(i.model.check().is_ok());
        let count = |category: &str| i.model.elements.values().filter(|e| e.category() == category).count();
        assert_eq!(count("level"), 18);
        assert!(count("wall") > 1000 && count("floor") > 100 && count("door") > 80 && count("column") > 100, "{:?}", i.skipped);
        // The second-prologue types: nearly every wall names a type.
        assert!(
            !i.skipped.iter().any(|s| s.contains("does not name one wall type") && !s.starts_with("1 wall ") && !s.starts_with("2 walls ")),
            "{:?}",
            i.skipped
        );
    }
}

/// A wall record and its data, `id` of type `tid`, from `line` (a line or arc record) with the box `bbox` and the refs `refs`.
pub(super) fn wall_of(p: &mut Project, id: u32, bbox: [f64; 6], refs: &[u64], line: Vec<u8>) {
    p.recs.push(Rec::new(id, WALLS).at(bbox, refs));
    let mut payload = line;
    payload.extend(orientation_bytes(0, 1, false, true));
    p.data.push(Block { id, payload });
}

#[test]
fn two_types_of_one_name_are_one_type_when_their_layers_match_and_two_when_they_do_not() {
    let mut p = base(2024);
    // 501 has the same name and layers as 500; 502 the same name and thickness, with the brick and the concrete the other way round.
    for (tid, outer) in [(501, 700), (502, 701)] {
        p.types.push(TypeRec::new(tid, WALLS));
        let mut payload = name_bytes("Brick wall");
        payload.extend(layers_bytes(&[(0.25, Some(outer), 4), (0.5, Some(701), 1)], 2024));
        p.data.push(Block { id: tid, payload });
    }
    for (k, tid) in [500u64, 501, 502].into_iter().enumerate() {
        let (id, y) = (100 + k as u32, 5.0 * k as f64);
        let line = line_bytes(0.0, 20.0, [0.0, y, 0.0], [1.0, 0.0, 0.0]);
        wall_of(&mut p, id, [0.0, y - 0.4, 0.0, 20.0, y + 0.4, 10.0], &[10, 11, tid, u64::from(id)], line);
    }
    let i = import(&p);
    let names: Vec<&str> = walls(&i).iter().map(|(_, w)| w.type_name.as_str()).collect();
    assert_eq!(names, ["Brick wall", "Brick wall", "Brick wall (2)"], "{:?}", i.skipped);
}

#[test]
fn a_wall_on_an_upper_level_has_its_line_at_that_height_and_a_climbing_line_is_not_a_wall() {
    let mut p = base(2024);
    // On level 2 (10 ft up), naming both levels: its base is level 2, which its box starts at.
    wall_of(&mut p, 100, [0.0, -0.4, 10.0, 20.0, 0.4, 20.0], &[10, 11, 500, 100], line_bytes(0.0, 20.0, [0.0, 0.0, 10.0], [1.0, 0.0, 0.0]));
    // A line that climbs 16 ft over its 12 ft.
    wall_of(&mut p, 101, [0.0, -0.4, 0.0, 12.0, 0.4, 10.0], &[10, 11, 500, 101], line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [0.6, 0.0, 0.8]));
    let i = import(&p);
    let [(_, w)] = walls(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((w.level, w.base_offset, w.height), (level_named(&i, "Level 2"), 0.0, 3048.0));
    assert!(i.skipped.iter().any(|s| s.contains("not level") && s.contains("101")), "{:?}", i.skipped);
}

#[test]
fn an_arc_is_read_by_its_angles_and_its_axes_wherever_it_starts_and_whichever_way_its_frame_is_turned() {
    let half_pi = std::f64::consts::FRAC_PI_2;
    // The frame turned a quarter (x north, y west), counter-clockwise, from 0.5 rad on: from r (-sin a, cos a) to r (-cos a, -sin a).
    let mut p = base(2024);
    let arc = arc_bytes([0.5, 0.5 + half_pi], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], 10.0, [0.0, 0.0, 0.0]);
    wall_of(&mut p, 100, [-10.4, -10.4, 0.0, 10.4, 10.4, 10.0], &[10, 11, 500, 100], arc);
    // Not in plan: the x axis stands up, or only the y axis does.
    let up_x = arc_bytes([0.0, half_pi], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], 10.0, [0.0, 0.0, 0.0]);
    let up_y = arc_bytes([0.0, half_pi], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, [0.0, 0.0, 0.0]);
    wall_of(&mut p, 101, [-10.4, -10.4, 0.0, 10.4, 10.4, 10.0], &[10, 11, 500, 101], up_x);
    wall_of(&mut p, 102, [-10.4, -10.4, 0.0, 10.4, 10.4, 10.0], &[10, 11, 500, 102], up_y);
    let i = import(&p);
    let [(_, w)] = walls(&i)[..] else { panic!("{:?}", i.skipped) };
    let (sin, cos) = 0.5f64.sin_cos();
    let mm = |v: f64| (v * 3048.0 * 1000.0).round() / 1000.0;
    assert_eq!(((w.start.0, w.start.1), (w.end.0, w.end.1)), ((mm(-sin), mm(cos)), (mm(-cos), mm(-sin))));
    assert!((w.arc - 90.0).abs() < 1e-6, "{}", w.arc);
    assert!(i.skipped.iter().any(|s| s.contains("not in plan") && s.contains("101") && s.contains("102")), "{:?}", i.skipped);
}

#[test]
fn a_wall_that_names_no_level_stands_on_the_one_below_its_bottom_and_on_none_where_two_tie() {
    let mut p = base(2024);
    wall_of(&mut p, 100, [0.0, -0.4, 0.0, 20.0, 0.4, 9.0], &[500, 100], line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    let i = import(&p);
    assert_eq!(walls(&i)[0].1.level, level_named(&i, "Level 1"), "{:?}", i.skipped);
    // A second level at the same height: which one is it on?
    p.levels.push(Lev::new(12, "Level 1b", 0.0));
    let i = import(&p);
    assert!(walls(&i).is_empty() && i.skipped.iter().any(|s| s.contains("no level") && s.contains("100")), "{:?}", i.skipped);
}

#[test]
fn a_type_whose_layers_have_no_width_is_no_wall_type_and_a_layer_whose_material_is_not_an_id_is_not_a_layer() {
    for release in [2024, 2026] {
        let mut p = base(release);
        // A membrane has no width: a type of nothing else is no wall.
        p.types.push(TypeRec::new(501, WALLS));
        let mut payload = name_bytes("Membrane");
        payload.extend(layers_bytes(&[(0.0, None, 100)], release));
        p.data.push(Block { id: 501, payload });
        // A material whose upper half is not zero is no ElementId.
        p.types.push(TypeRec::new(502, WALLS));
        let mut payload = name_bytes("Damaged");
        let mut layers = layers_bytes(&[(0.25, Some(700), 4)], release);
        layers[22] = 1;
        payload.extend(layers);
        p.data.push(Block { id: 502, payload });
        for (id, tid) in [(100, 501), (101, 502)] {
            let line = line_bytes(0.0, 20.0, [0.0, 5.0 * f64::from(id - 100), 0.0], [1.0, 0.0, 0.0]);
            wall_of(&mut p, id, [0.0, -0.4, 0.0, 20.0, 0.4, 10.0], &[10, 11, tid, u64::from(id)], line);
        }
        let i = import(&p);
        // A membrane has no width, so its type is no wall and the wall is left out; a layer list that is not framed reads as no
        // list, so its type is read as a curtain wall's — the plane its box gives — and the note says so.
        assert_eq!(walls(&i).len(), 1, "Revit {release}: {:?}", i.skipped);
        assert!(i.skipped.iter().any(|s| s.contains("no layers") && s.contains("Membrane") && s.contains("ids 100")), "{:?}", i.skipped);
        assert!(i.skipped.iter().any(|s| s.contains("comes in as the plane") && s.contains("101")), "{:?}", i.skipped);
    }
}

#[test]
fn a_structural_deck_with_no_width_of_its_own_is_left_out_of_the_layers_it_carries_nothing_of() {
    // Revit lets a structural deck take its thickness from the layer above it and write none of its own: a floor of the
    // structural sample is NW Concrete on Metal Deck with such a layer, and the model refuses a layer of no thickness that is
    // not a membrane, so the import refused the whole file.
    for release in [2024, 2026] {
        let mut p = base(release);
        p.types.push(TypeRec::new(503, WALLS));
        let mut payload = name_bytes("Deck");
        payload.extend(layers_bytes(&[(0.5, Some(701), 1), (0.0, None, 200)], release));
        p.data.push(Block { id: 503, payload });
        let line = line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        wall_of(&mut p, 100, [0.0, -0.4, 0.0, 20.0, 0.4, 10.0], &[10, 11, 503, 100], line);
        let i = import(&p);
        let [(_, w)] = walls(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        let t = &i.model.types.wall[&w.type_name];
        assert_eq!(t.layers.len(), 1, "{:?}", t.layers);
        assert_eq!(t.thickness, 152.4);
    }
}

#[test]
fn a_layer_list_is_a_types_only_when_it_is_framed() {
    // `ff ff ff ff` and another tag, or the right tag with no `ff ff ff ff` before it, frame nothing.
    for damage in [|l: &mut Vec<u8>| l[4] ^= 0x55, |l: &mut Vec<u8>| l[0..4].fill(0x11)] {
        let mut p = base(2024);
        let mut layers = layers_bytes(&[(0.25, Some(700), 4), (0.5, Some(701), 1)], 2024);
        damage(&mut layers);
        let mut payload = name_bytes("Brick wall");
        payload.extend(layers);
        p.data[0].payload = payload;
        straight(&mut p, 100, false);
        let i = import(&p);
        // A layer list that is not framed is no layer list: the type reads as a curtain wall's, and its wall comes in as the plane
        // its own box gives, which is what the note says.
        assert_eq!(walls(&i).len(), 1, "{:?}", i.skipped);
        assert!(i.skipped.iter().any(|s| s.contains("no layers") && s.contains("comes in as the plane")), "{:?}", i.skipped);
    }
}

#[test]
fn a_materials_name_is_read_by_each_of_the_rules_and_a_rule_only_where_its_release_writes_it() {
    use crate::project_fixture::NameStyle;
    let layers = |release: u32, style: NameStyle| {
        let mut p = base(release);
        p.name_style = style;
        straight(&mut p, 100, false);
        let i = import(&p);
        let t = &i.model.types.wall["Brick wall"];
        (t.layers[0].material.clone(), t.layers[1].material.clone())
    };
    let named = ("Brick".to_string(), "Concrete".to_string());
    let none = (String::new(), String::new());
    for (release, style, want) in [
        (2024, NameStyle::PatternFramed, &named),
        (2024, NameStyle::ParamEntry, &named),
        (2024, NameStyle::Closed, &named),
        (2023, NameStyle::ParamEntry, &named),
        (2019, NameStyle::ParamEntry, &named),
        (2023, NameStyle::PatternFramed, &none),
        (2023, NameStyle::Closed, &none),
    ] {
        assert_eq!(&layers(release, style), want, "Revit {release}, {style:?}");
    }
}

#[test]
fn a_layer_with_no_material_takes_its_categorys_from_the_object_styles_up_to_2024_when_one_value_is_written() {
    let layer_material = |release: u32, written: &[(i64, u32)]| {
        let mut p = base(release);
        let mut payload = name_bytes("Brick wall");
        payload.extend(layers_bytes(&[(0.25, None, 4), (0.5, Some(701), 1)], release));
        p.data[0].payload = payload;
        p.category_materials = written.to_vec();
        straight(&mut p, 100, false);
        import(&p).model.types.wall["Brick wall"].layers[0].material.clone()
    };
    for (release, want) in [(2019, "Brick"), (2023, "Brick"), (2024, "Brick"), (2025, ""), (2026, "")] {
        assert_eq!(layer_material(release, &[(-2_000_011, 700)]), want, "Revit {release}");
    }
    // Two values for the category, or one for another: none.
    assert_eq!(layer_material(2024, &[(-2_000_011, 700), (-2_000_011, 701)]), "");
    assert_eq!(layer_material(2024, &[(-2_000_032, 700)]), "");
    assert_eq!(layer_material(2023, &[(-2_000_011, 700), (-2_000_011, 701)]), "");
}

#[test]
fn a_material_found_twice_is_named_only_where_its_copies_agree() {
    let layer = |extra: Option<(u32, &str)>| {
        let mut p = base(2024);
        if let Some((id, name)) = extra {
            p.mats.push(Mat { id, name: name.into() });
        }
        straight(&mut p, 100, false);
        import(&p).model.types.wall["Brick wall"].layers[0].material.clone()
    };
    assert_eq!(layer(None), "Brick");
    assert_eq!(layer(Some((700, "Brick"))), "Brick", "the same name twice");
    assert_eq!(layer(Some((700, "Clinker"))), "", "two names for one material");
}
