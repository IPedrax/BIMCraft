//! Doors and windows: the wall that holds them, where along it, how wide and how high.

use super::walls::{WALLS, base, import, straight, wall_of, walls};
use crate::project_fixture::{Block, NameEntry, Project, Rec, line_bytes, orientation_bytes};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Id, Opening};

const DOORS: i64 = -2_000_023;
const WINDOWS: i64 = -2_000_014;

fn openings(i: &Imported) -> Vec<(Id, bool, &Opening)> {
    i.model
        .elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::Door(o) => Some((id, true, o)),
            Element::Window(o) => Some((id, false, o)),
            _ => None,
        })
        .collect()
}

/// A project with the wall of `straight` (id 100, east along y = 0 from 0 to 20 ft, 10 ft tall) and a door symbol 600 and a
/// window symbol 601, each named.
fn project(release: u32) -> Project {
    let mut p = base(release);
    straight(&mut p, 100, false);
    p.recs.push(Rec { placed: false, ..Rec::new(600, DOORS) });
    p.recs.push(Rec { placed: false, ..Rec::new(601, WINDOWS) });
    p.names = vec![
        NameEntry { id: 600, name: "Single-Flush".into(), category: DOORS },
        NameEntry { id: 601, name: "Fixed".into(), category: WINDOWS },
    ];
    p
}

/// A door (3 ft wide, 7 ft tall, from the floor) centred `at` ft along the wall, listing `refs` besides its symbol.
fn door(p: &mut Project, id: u32, at: f64, refs: &[u64]) {
    let mut all = refs.to_vec();
    all.extend([600, u64::from(id)]);
    p.recs.push(Rec::new(id, DOORS).at([at - 1.5, -0.4, 0.0, at + 1.5, 1.2, 7.0], &all));
}

#[test]
fn a_door_and_a_window_come_in_set_in_their_wall_with_the_size_and_height_of_their_boxes() {
    for release in [2019, 2023, 2024, 2026] {
        let mut p = project(release);
        door(&mut p, 200, 5.0, &[100]);
        p.recs.push(Rec::new(201, WINDOWS).at([11.0, -0.4, 3.0, 15.0, 0.4, 7.0], &[100, 601, 201]));
        let i = import(&p);
        let ops = openings(&i);
        assert_eq!(ops.len(), 2, "Revit {release}: {:?}", i.skipped);
        let (_, is_door, d) = ops.iter().find(|(_, door, _)| *door).copied().unwrap();
        assert!(is_door);
        assert_eq!((d.offset, d.sill, d.type_name.as_str()), (1524.0, 0.0, "Single-Flush"), "Revit {release}");
        assert_eq!((i.model.types.door["Single-Flush"].width, i.model.types.door["Single-Flush"].height), (914.4, 2133.6));
        let (_, _, w) = ops.iter().find(|(_, door, _)| !*door).copied().unwrap();
        assert_eq!((w.offset, w.sill, w.type_name.as_str()), (3962.4, 914.4, "Fixed"));
        assert_eq!((i.model.types.window["Fixed"].width, i.model.types.window["Fixed"].height), (1219.2, 1219.2));
        assert!(w.host == d.host && !d.flip_facing && !d.flip_hand && d.family.is_none());
        assert_eq!(i.skipped, Vec::<String>::new(), "Revit {release}");
    }
}

#[test]
fn an_opening_is_measured_from_the_start_of_a_wall_whichever_way_it_runs() {
    // The wall turned round (its exterior on the right), then standing north: the offset is from where its line starts.
    let mut p = base(2024);
    p.recs.push(Rec::new(100, WALLS).at([-0.4, 0.0, 0.0, 0.4, 20.0, 10.0], &[10, 11, 500, 100]));
    let mut payload = line_bytes(0.0, 20.0, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    payload.extend(orientation_bytes(0, 1, true, true));
    p.data.push(Block { id: 100, payload });
    p.recs.push(Rec { placed: false, ..Rec::new(600, DOORS) });
    p.recs.push(Rec::new(200, DOORS).at([-0.4, 3.5, 0.0, 1.2, 6.5, 7.0], &[100, 600, 200]));
    let i = import(&p);
    let [(_, w)] = walls(&i)[..] else { panic!() };
    assert_eq!((w.start.1, w.end.1), (6096.0, 0.0), "the wall runs south: its exterior is on the right");
    let [(_, _, d)] = openings(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!(d.offset, 6096.0 - 1524.0, "5 ft from where the line starts, which is the north end");
}

#[test]
fn the_host_is_the_nearest_wall_below_an_openings_id_then_the_nearest_above_and_a_storefront_is_a_wall_too() {
    let wall_at = |p: &mut Project, id: u32, y: f64, type_id: u64| {
        p.recs.push(Rec::new(id, WALLS).at([0.0, y - 0.4, 0.0, 20.0, y + 0.4, 10.0], &[10, 11, type_id, u64::from(id)]));
        let mut b = line_bytes(0.0, 20.0, [0.0, y, 0.0], [1.0, 0.0, 0.0]);
        b.extend(orientation_bytes(0, 1, false, true));
        p.data.push(Block { id, payload: b });
    };
    let mut p = project(2024);
    wall_at(&mut p, 80, 20.0, 500);
    wall_at(&mut p, 300, 30.0, 500);
    // A curtain wall: its type has no layers, and the name it has is the one it comes in under.
    p.types.push(crate::project_fixture::TypeRec::new(502, WALLS));
    p.data.push(Block { id: 502, payload: crate::project_fixture::name_bytes("Storefront") });
    wall_at(&mut p, 90, 40.0, 502);
    // Door 200 names walls 80 and 100, both below it: the nearest below is 100. Door 250 names only wall 300, above it.
    door(&mut p, 200, 5.0, &[80, 100]);
    p.recs.push(Rec::new(250, DOORS).at([3.5, 29.6, 0.0, 6.5, 31.2, 7.0], &[300, 600, 250]));
    // Door 95 names the curtain wall 90 (below it) and wall 100 (above it): the nearest below it is the curtain wall, which comes in
    // as the plane its box gives, so it is the host. Door 410 names only it.
    door(&mut p, 95, 9.0, &[90, 100]);
    p.recs.push(Rec::new(410, DOORS).at([3.5, 40.0, 0.0, 6.5, 41.2, 7.0], &[90, 600, 410]));
    let (i, from) = super::walls::import_with_origins(&p);
    let host = |door: u32| {
        openings(&i)
            .into_iter()
            .find(|(m, _, _)| from[m] == format!("Door {door}"))
            .map(|(_, _, o)| from[&i.model.elements.id(o.host).unwrap()].clone())
    };
    assert_eq!(host(200).as_deref(), Some("Wall 100"), "{:?}", i.skipped);
    assert_eq!(host(250).as_deref(), Some("Wall 300"));
    assert_eq!(host(95).as_deref(), Some("Wall 90"));
    assert_eq!(host(410).as_deref(), Some("Wall 90"), "{:?}", i.skipped);
}

/// A wall at an angle: from (0, 5) ft, 15 ft long along `direction`, 10 ft tall.
fn angled_wall(p: &mut Project, id: u32, direction: [f64; 3]) {
    let end = [15.0 * direction[0], 5.0 + 15.0 * direction[1]];
    let (x, y) = ((0.0_f64.min(end[0]), 0.0_f64.max(end[0])), (5.0_f64.min(end[1]), 5.0_f64.max(end[1])));
    p.recs.push(Rec::new(id, WALLS).at([x.0, y.0, 0.0, x.1, y.1, 10.0], &[10, 11, 500, u64::from(id)]));
    p.data.push(Block {
        id,
        payload: {
            let mut b = line_bytes(0.0, 15.0, [0.0, 5.0, 0.0], direction);
            b.extend(orientation_bytes(0, 1, false, true));
            b
        },
    });
}

#[test]
fn an_opening_in_a_wall_at_an_angle_has_its_width_solved_from_its_box() {
    // The 3-4-5 wall runs (0.8, 0.6). A window 4 ft wide and 1 ft deep with its middle 6 ft along it lies at (4.8, 8.6), and the
    // box round it is 4·0.8 + 1·0.6 = 3.8 ft in x and 4·0.6 + 1·0.8 = 3.2 ft in y.
    let mut p = project(2024);
    angled_wall(&mut p, 150, [0.8, 0.6, 0.0]);
    p.recs.push(Rec::new(210, WINDOWS).at([4.8 - 1.9, 8.6 - 1.6, 3.0, 4.8 + 1.9, 8.6 + 1.6, 7.0], &[150, 601, 210]));
    let i = import(&p);
    let [(_, false, w)] = openings(&i)[..] else { panic!("{:?}", i.skipped) };
    let width = i.model.types.window[&w.type_name].width;
    assert!((width - 4.0 * 304.8).abs() < 0.01 && (w.offset - 6.0 * 304.8).abs() < 0.01, "{width} wide at {}", w.offset);
    // The same wall turned the other way round the compass, running (-0.6, 0.8) from the same start: the box is as wide the other way.
    let mut q = project(2024);
    angled_wall(&mut q, 150, [-0.6, 0.8, 0.0]);
    // 5 ft along it: the middle is at (-3, 9), and a 3 ft wide, 0.5 ft deep door has extents 3·0.6 + 0.5·0.8 = 2.2 and 3·0.8 + 0.5·0.6 = 2.7.
    q.recs.push(Rec::new(211, DOORS).at([-3.0 - 1.1, 9.0 - 1.35, 0.0, -3.0 + 1.1, 9.0 + 1.35, 7.0], &[150, 600, 211]));
    let i = import(&q);
    let [(_, true, d)] = openings(&i)[..] else { panic!("{:?}", i.skipped) };
    let width = i.model.types.door[&d.type_name].width;
    assert!((width - 3.0 * 304.8).abs() < 0.01 && (d.offset - 5.0 * 304.8).abs() < 0.01, "{width} wide at {}", d.offset);
    // At 45° the two extents say the same thing and the width is lost: the opening is left out, and the import says why.
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let mut r = project(2024);
    angled_wall(&mut r, 150, [half, half, 0.0]);
    r.recs.push(Rec::new(212, WINDOWS).at([2.0, 7.0, 3.0, 5.0, 10.0, 7.0], &[150, 601, 212]));
    let i = import(&r);
    assert!(openings(&i).is_empty() && i.skipped.iter().any(|s| s.contains("runs at an angle") && s.contains("212")), "{:?}", i.skipped);
}

#[test]
fn an_opening_in_a_wall_at_an_angle_or_a_curved_one_or_with_no_wall_is_left_out_with_the_reason() {
    let mut p = project(2024);
    // A wall at an angle (3-4-5) and a door in it whose box is no rectangle turned to the wall.
    angled_wall(&mut p, 150, [0.8, 0.6, 0.0]);
    p.recs.push(Rec::new(210, DOORS).at([4.0, 8.0, 0.0, 7.0, 10.0, 7.0], &[150, 600, 210]));
    // A door that names no wall, and one whose wall has no location line but a box (a family instance): that wall comes in along its box.
    p.recs.push(Rec::new(220, DOORS).at([4.0, -0.4, 0.0, 7.0, 0.4, 7.0], &[600, 220]));
    p.recs.push(Rec::new(130, WALLS).at([0.0, 50.0, 0.0, 20.0, 51.0, 10.0], &[10, 11, 500, 130]));
    p.recs.push(Rec::new(230, DOORS).at([4.0, 50.0, 0.0, 7.0, 51.0, 7.0], &[130, 600, 230]));
    let i = import(&p);
    let said = |what: &str| i.skipped.iter().find(|s| s.contains(what)).cloned().unwrap_or_default();
    assert!(said("runs at an angle").contains("210"), "{:?}", i.skipped);
    assert!(said("names no wall").contains("220"));
    assert_eq!(openings(&i).len(), 1, "a door in a wall read from its box comes in: {:?}", i.skipped);
    // A curved wall.
    let mut q = project(2024);
    q.recs[0].bbox = [0.0, -10.4, 0.0, 10.4, 10.4, 10.0];
    q.data[1].payload = {
        let mut b = crate::project_fixture::arc_bytes([0.0, 1.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, [0.0; 3]);
        b.extend(orientation_bytes(0, 1, false, true));
        b
    };
    door(&mut q, 200, 5.0, &[100]);
    let i = import(&q);
    assert!(openings(&i).is_empty() && i.skipped.iter().any(|s| s.contains("curved")), "{:?}", i.skipped);
}

#[test]
fn neighbours_that_overlap_a_little_are_narrowed_to_fit_and_ones_that_overlap_much_are_not() {
    let mut p = project(2024);
    // Two 4 ft windows overlapping by 0.2 ft (6.1 mm of 1219.2: a few percent).
    for (id, at) in [(201, 5.0), (202, 8.8)] {
        p.recs.push(Rec::new(id, WINDOWS).at([at - 2.0, -0.4, 3.0, at + 2.0, 0.4, 7.0], &[100, 601, u64::from(id)]));
    }
    let i = import(&p);
    let ops = openings(&i);
    assert_eq!(ops.len(), 2, "{:?}", i.skipped);
    let spans: Vec<(f64, f64)> = {
        let mut v: Vec<(f64, f64)> = ops
            .iter()
            .map(|(_, _, o)| {
                let w = i.model.types.window[&o.type_name].width;
                (o.offset - w / 2.0, o.offset + w / 2.0)
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    };
    assert!((spans[0].1 - spans[1].0).abs() < 1e-6, "they meet: {spans:?}");
    assert!((spans[0].0 - (5.0 - 2.0) * 304.8).abs() < 1e-6 && (spans[1].1 - (8.8 + 2.0) * 304.8).abs() < 1e-6, "the outer edges stay");
    assert!(i.skipped.iter().any(|s| s.contains("2 openings") && s.contains("narrowed to fit")), "{:?}", i.skipped);
    // Overlapping by 2 ft is not a few percent: one of them goes, and the import says why.
    let mut q = project(2024);
    for (id, at) in [(201, 5.0), (202, 7.0)] {
        q.recs.push(Rec::new(id, WINDOWS).at([at - 2.0, -0.4, 3.0, at + 2.0, 0.4, 7.0], &[100, 601, u64::from(id)]));
    }
    let i = import(&q);
    assert_eq!(openings(&i).len(), 1);
    assert!(i.skipped.iter().any(|s| s.contains("overlap in wall")), "{:?}", i.skipped);
}

#[test]
fn an_opening_whose_box_overruns_its_wall_comes_in_as_far_as_the_wall_goes() {
    let mut p = project(2024);
    door(&mut p, 200, 5.0, &[100]);
    door(&mut p, 201, -0.5, &[100]); // half of it before the wall starts
    p.recs.push(Rec::new(202, WINDOWS).at([10.0, -0.4, 9.0, 12.0, 0.4, 12.0], &[100, 601, 202])); // above its wall's top
    let i = import(&p);
    // Revit draws these with their frames a little past the wall's box: each comes in as far as the wall goes, and says so.
    assert_eq!(openings(&i).len(), 3, "{:?}", i.skipped);
    let said = i.skipped.iter().find(|s| s.contains("overruns its wall")).cloned().unwrap_or_default();
    assert!(said.contains("2 openings") && said.contains("ids 201, 202"), "{:?}", i.skipped);
    let window = openings(&i).into_iter().find(|(_, door, _)| !door).map(|(_, _, o)| o).expect("the window");
    assert!(i.model.types.window[&window.type_name].height <= 3048.0, "no taller than its wall");
}

#[test]
fn what_the_reader_cuts_down_to_fit_is_told_as_a_count_of_its_kind_with_the_ids() {
    // Two doors whose boxes overrun their wall the same way: one note, both ids.
    let mut p = project(2024);
    door(&mut p, 200, 5.0, &[100]);
    door(&mut p, 201, -0.5, &[100]);
    p.recs.push(Rec::new(203, WINDOWS).at([10.0, -0.4, 9.0, 12.0, 0.4, 12.0], &[100, 601, 203]));
    let i = import(&p);
    let said: Vec<&String> = i.skipped.iter().filter(|s| s.contains("overruns its wall")).collect();
    assert_eq!(said.len(), 1, "{:?}", i.skipped);
    assert!(said[0].starts_with("2 openings: ") && said[0].ends_with("(ids 201, 203)"), "{said:?}");
    assert_eq!(openings(&i).len(), 3);
}

#[test]
fn types_are_named_by_their_name_entries_and_told_apart_by_size() {
    for release in [2019, 2023, 2024] {
        let mut p = project(release);
        door(&mut p, 200, 5.0, &[100]);
        // The same type, a wider door: BIMCraft's type has one size.
        p.recs.push(Rec::new(201, DOORS).at([9.0, -0.4, 0.0, 13.0, 1.2, 7.0], &[100, 600, 201]));
        // No name entry for its symbol: a name of its own.
        p.recs.push(Rec { placed: false, ..Rec::new(602, DOORS) });
        p.recs.push(Rec::new(202, DOORS).at([15.0, -0.4, 0.0, 18.0, 1.2, 7.0], &[100, 602, 202]));
        // A name entry of another category does not name a door's type (2024 and later keep the category).
        let i = import(&p);
        // (A new project has door types of its own; the import leaves them be, as the IFC import does.)
        let mut names: Vec<&str> =
            i.model.types.door.keys().map(String::as_str).filter(|n| n.starts_with("Imported") || n.starts_with("Single-Flush")).collect();
        names.sort();
        assert_eq!(names, ["Imported door", "Single-Flush", "Single-Flush (2)"], "Revit {release}: {:?}", i.skipped);
        assert_eq!(i.model.types.door["Single-Flush (2)"].width, 1219.2);
    }
    let mut p = project(2024);
    p.names[0].category = WINDOWS;
    door(&mut p, 200, 5.0, &[100]);
    assert!(import(&p).model.types.door.contains_key("Imported door"));
}

#[test]
fn the_doors_and_windows_of_the_architectural_sample_are_those_of_rvt_rss_export_with_a_host() {
    use super::rvt_rs::RAC_OPENINGS;
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let (i, from) = crate::import_path_with_origins(&path, "x").unwrap();
        if !super::pinned(&i) {
            continue;
        }
        let mut missing = Vec::new();
        let mut fitted = 0;
        let mut differed = 0;
        for theirs in &RAC_OPENINGS {
            let label = if theirs.door { "Door" } else { "Window" };
            let Some((_, _, o)) = openings(&i).into_iter().find(|(m, _, _)| from[m] == format!("{label} {}", theirs.id)) else {
                missing.push(theirs.id);
                continue;
            };
            assert!(!o.type_name.starts_with("Imported"), "{label} {}: its type is named in the file", theirs.id);
            let host = i.model.wall(o.host).unwrap();
            let ours = from[&i.model.elements.id(o.host).unwrap()].clone();
            if ours != format!("Wall {}", theirs.host) {
                // The one opening of the sample where the two readers differ: its box overruns the end of the wall rvt-rs names (a
                // door Revit draws with its frame past the wall's box), so this reader sets it in the next wall the record names
                // that holds it whole. The wall rvt-rs names is shorter than the opening is wide.
                assert!(from.values().any(|n| n == &format!("Wall {}", theirs.host)), "the wall rvt-rs names is in the model");
                // The hole is the box's, whatever wall holds it: its width is the box's extent along the wall it is set in.
                let width = if theirs.door { i.model.types.door[&o.type_name].width } else { i.model.types.window[&o.type_name].width };
                assert!(width > 0.0, "{label} {}: a hole", theirs.id);
                differed += 1;
                continue;
            }
            let t = if theirs.door { i.model.types.door[&o.type_name] } else { i.model.types.window[&o.type_name] };
            let along_x = (host.end.1 - host.start.1).abs() < 0.01;
            let (lo, hi) = if along_x { (theirs.cut[0], theirs.cut[3]) } else { (theirs.cut[1], theirs.cut[4]) };
            let (from_start, sign) = if along_x {
                (host.start.0, (host.end.0 - host.start.0).signum())
            } else {
                (host.start.1, (host.end.1 - host.start.1).signum())
            };
            let mine = (from_start + sign * (o.offset - t.width / 2.0), from_start + sign * (o.offset + t.width / 2.0));
            let (mine_lo, mine_hi) = (mine.0.min(mine.1), mine.0.max(mine.1));
            // Within what fitting a neighbour may take (a tenth of the width) and, for the rest, to the hole's edge.
            assert!(
                (mine_lo - lo).abs() <= 0.1 * (hi - lo) && (mine_hi - hi).abs() <= 0.1 * (hi - lo),
                "{label} {}: {mine_lo}..{mine_hi} against {lo}..{hi}",
                theirs.id
            );
            fitted += usize::from((mine_lo - lo).abs() > 0.2 || (mine_hi - hi).abs() > 0.2);
            let base = i.model.own_base(host);
            assert!(
                (base + o.sill - theirs.cut[2]).abs() < 0.2 && (base + o.sill + t.height - theirs.cut[5]).abs() < 0.2,
                "{label} {}: z",
                theirs.id
            );
        }
        // 26 with a host: every one comes in now. 25 are set in the wall rvt-rs names; the one that is not (906937) hangs 1 m out
        // of the end of that wall, so it is set in the next wall its record names that holds it whole.
        assert!(missing.is_empty(), "{}: {:?}", path.display(), i.skipped);
        assert_eq!(differed, 1, "one opening of the sample is set in another wall than rvt-rs sets it in");
        assert_eq!(fitted, 10, "ten windows are narrowed to fit their neighbours");
        assert_eq!(openings(&i).len(), 31, "26 rvt-rs sets in a wall, and the 5 others the file holds none of");
    }
}

#[test]
fn a_mutation_sweep_over_a_project_with_openings_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = project(release);
        door(&mut p, 200, 5.0, &[100]);
        p.recs.push(Rec::new(201, WINDOWS).at([11.0, -0.4, 3.0, 15.0, 0.4, 7.0], &[100, 601, 201]));
        p.members = 3;
        let bytes = p.partition_bytes();
        let mut seed = 0x2468_ace0_1357_9bdfu64 ^ u64::from(release);
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
                assert!(openings(&i).len() <= 2, "round {round}");
            }
        }
    }
}

#[test]
fn an_opening_is_measured_along_a_wall_that_starts_away_from_the_origin_in_either_axis() {
    let mut p = project(2024);
    // East from (5, 3) to (25, 3), and north from (40, 3) to (40, 23).
    wall_of(&mut p, 110, [5.0, 2.6, 0.0, 25.0, 3.4, 10.0], &[10, 11, 500, 110], line_bytes(0.0, 20.0, [5.0, 3.0, 0.0], [1.0, 0.0, 0.0]));
    wall_of(&mut p, 111, [39.6, 3.0, 0.0, 40.4, 23.0, 10.0], &[10, 11, 500, 111], line_bytes(0.0, 20.0, [40.0, 3.0, 0.0], [0.0, 1.0, 0.0]));
    p.recs.push(Rec::new(210, DOORS).at([10.0, 2.6, 0.0, 13.0, 3.4, 7.0], &[110, 600, 210]));
    p.recs.push(Rec::new(211, WINDOWS).at([39.6, 8.0, 3.0, 40.4, 12.0, 7.0], &[111, 601, 211]));
    let i = import(&p);
    let find = |door: bool| openings(&i).into_iter().find(|(_, d, _)| *d == door).map(|(_, _, o)| o.clone());
    let (d, w) = (find(true).expect("the door"), find(false).expect("the window"));
    assert_eq!((d.offset, w.offset), (6.5 * 304.8, 7.0 * 304.8), "{:?}", i.skipped);
}

#[test]
fn neighbours_that_overlap_by_more_than_a_tenth_of_the_narrower_are_not_fitted_whatever_the_wider_is() {
    // A 4 ft window from 10 ft and a 2 ft one from 13.7: 0.3 ft (91 mm) overlap, more than a tenth of the narrower (61 mm) and less
    // than a tenth of the wider (122 mm). BIMCraft refuses overlapping openings, so one of them is dropped, none narrowed.
    let mut p = project(2024);
    p.recs.push(Rec::new(200, WINDOWS).at([10.0, -0.4, 3.0, 14.0, 0.4, 7.0], &[100, 601, 200]));
    p.recs.push(Rec::new(201, WINDOWS).at([13.7, -0.4, 3.0, 15.7, 0.4, 7.0], &[100, 601, 201]));
    let i = import(&p);
    assert_eq!(openings(&i).len(), 1, "{:?}", i.skipped);
    assert!(!i.skipped.iter().any(|s| s.contains("narrowed")), "{:?}", i.skipped);
    // The narrower first: a 2 ft window from 10 ft and a 4 ft one from 11.7, overlapping by the same 0.3 ft.
    let mut r = project(2024);
    r.recs.push(Rec::new(200, WINDOWS).at([10.0, -0.4, 3.0, 12.0, 0.4, 7.0], &[100, 601, 200]));
    r.recs.push(Rec::new(201, WINDOWS).at([11.7, -0.4, 3.0, 15.7, 0.4, 7.0], &[100, 601, 201]));
    let i = import(&r);
    assert_eq!(openings(&i).len(), 1, "{:?}", i.skipped);
    assert!(!i.skipped.iter().any(|s| s.contains("narrowed")), "{:?}", i.skipped);
    // Half a tenth of the narrower is fitted: each gives up half.
    let mut q = project(2024);
    q.recs.push(Rec::new(200, WINDOWS).at([10.0, -0.4, 3.0, 14.0, 0.4, 7.0], &[100, 601, 200]));
    q.recs.push(Rec::new(201, WINDOWS).at([13.97, -0.4, 3.0, 15.97, 0.4, 7.0], &[100, 601, 201]));
    let i = import(&q);
    assert_eq!(openings(&i).len(), 2, "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("narrowed") && s.contains("200") && s.contains("201")), "{:?}", i.skipped);
}

#[test]
fn only_a_family_symbol_a_record_names_gives_its_type_and_one_type_serves_every_opening_of_its_size() {
    let mut p = project(2024);
    // 777 has a name entry of the doors' category and is no symbol.
    p.extra_ids.push(777);
    p.names.push(NameEntry { id: 777, name: "Not a symbol".into(), category: DOORS });
    door(&mut p, 200, 3.0, &[100, 777]);
    door(&mut p, 201, 8.0, &[100]);
    // The same symbol, 2 ft wide: a type of its own.
    p.recs.push(Rec::new(202, DOORS).at([13.0, -0.4, 0.0, 15.0, 1.2, 7.0], &[100, 600, 202]));
    let i = import(&p);
    let mut names: Vec<&str> = openings(&i).iter().map(|(_, _, o)| o.type_name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Single-Flush", "Single-Flush", "Single-Flush (2)"], "{:?}", i.skipped);
}
