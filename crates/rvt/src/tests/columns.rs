//! Columns: their place, size, levels, type, and what is not read.

use super::walls::{base, import, level_named};
use crate::project_fixture::{Block, NameEntry, Project, Rec, transform_bytes};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Column, Element};

const COLUMNS: i64 = -2_000_100;
const STRUCTURAL: i64 = -2_001_330;

fn columns(i: &Imported) -> Vec<(bool, &Column)> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Column(c) => Some((false, c)),
            Element::StructuralColumn(c) => Some((true, c)),
            _ => None,
        })
        .collect()
}

/// Levels 0 and 10 ft, a column symbol 700 named "Concrete" in both categories' sets.
fn project(release: u32, category: i64) -> Project {
    let mut p = base(release);
    p.recs.push(Rec { placed: false, ..Rec::new(650, category) });
    p.names.push(NameEntry { id: 650, name: "Concrete".into(), category });
    p
}

/// A column 2 ft by 1 ft in plan at (10, 5) ft, from level 1 to level 2.
fn column(p: &mut Project, category: i64, id: u32) {
    p.recs.push(Rec::new(id, category).at([9.0, 4.5, 0.0, 11.0, 5.5, 10.0], &[10, 11, 650, u64::from(id)]));
}

#[test]
fn a_column_comes_in_at_the_middle_of_its_box_with_its_size_levels_and_height() {
    for release in [2019, 2024] {
        for (category, structural) in [(COLUMNS, false), (STRUCTURAL, true)] {
            let mut p = project(release, category);
            column(&mut p, category, 100);
            let i = import(&p);
            let [(is_structural, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
            assert_eq!(is_structural, structural);
            assert_eq!((c.at.0, c.at.1, c.angle), (3048.0, 1524.0, 0.0));
            assert_eq!(
                (c.level, c.top_level, c.height, c.base_offset),
                (level_named(&i, "Level 1"), Some(level_named(&i, "Level 2")), 3048.0, 0.0)
            );
            let types = if structural { &i.model.types.structural_column } else { &i.model.types.column };
            let t = &types["Concrete"];
            assert_eq!((c.type_name.as_str(), t.width, t.depth, t.round), ("Concrete", 609.6, 304.8, false));
            assert!(
                i.skipped.iter().any(|s| s.contains("1 column") && s.contains("rectangle of its box") && s.contains("100")),
                "{:?}",
                i.skipped
            );
        }
    }
}

#[test]
fn a_column_that_stands_short_of_its_top_level_is_as_tall_as_its_box() {
    let mut p = project(2024, STRUCTURAL);
    p.recs.push(Rec::new(100, STRUCTURAL).at([9.0, 4.5, 1.0, 11.0, 5.5, 9.0], &[10, 11, 650, 100]));
    let i = import(&p);
    let [(_, c)] = columns(&i)[..] else { panic!() };
    assert_eq!((c.top_level, c.height, c.base_offset), (None, 2438.4, 304.8));
}

#[test]
fn a_column_turned_off_the_axes_takes_its_symbols_size_and_its_placements_angle_and_origin() {
    let mut p = project(2024, STRUCTURAL);
    // The symbol's envelope in the family's own axes: 2 ft by 1 ft. The instance is turned 30 degrees about (10, 5).
    p.recs.iter_mut().find(|r| r.id == 650).unwrap().bbox = [-1.0, -0.5, 0.0, 1.0, 0.5, 10.0];
    column(&mut p, STRUCTURAL, 100);
    p.recs.iter_mut().find(|r| r.id == 100).unwrap().bbox = [8.8, 3.9, 0.0, 11.2, 6.1, 10.0];
    p.data.push(Block { id: 100, payload: transform_bytes(30f64.to_radians(), [10.0, 5.0, 0.0]) });
    let i = import(&p);
    let [(_, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((c.at.0, c.at.1), (3048.0, 1524.0));
    assert!((c.angle - 30.0).abs() < 1e-3, "{}", c.angle);
    let t = &i.model.types.structural_column["Concrete"];
    assert_eq!((t.width, t.depth), (609.6, 304.8));
    // A quarter turn leaves the box what it is: no envelope is needed.
    let mut q = project(2024, STRUCTURAL);
    column(&mut q, STRUCTURAL, 100);
    q.data.push(Block { id: 100, payload: transform_bytes(std::f64::consts::FRAC_PI_2, [10.0, 5.0, 0.0]) });
    let i = import(&q);
    let [(_, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!(c.angle, 0.0);
    assert_eq!(i.model.types.structural_column["Concrete"].width, 609.6);
    // Turned, with a symbol whose size is not known: it comes in as its box, turned, and says so.
    let mut r = project(2024, STRUCTURAL);
    r.names.clear();
    column(&mut r, STRUCTURAL, 100);
    r.data.push(Block { id: 100, payload: transform_bytes(0.5, [10.0, 5.0, 0.0]) });
    let i = import(&r);
    let [(_, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
    assert!(c.angle != 0.0 && i.skipped.iter().any(|s| s.contains("family symbol's size is not known")), "{:?}", i.skipped);
}

#[test]
fn a_boxs_bottom_at_the_ground_is_corrected_from_the_columns_geometry_node_in_2024_and_later() {
    let mut p = project(2024, STRUCTURAL);
    column(&mut p, STRUCTURAL, 100);
    let r = p.recs.iter_mut().find(|r| r.id == 100).unwrap();
    r.bbox = [9.0, 4.5, 0.0, 11.0, 5.5, 20.0];
    // The node says it starts at 2 ft, with the record's plan and top.
    p.nodes.push((100, [9.0, 4.5, 2.0, 11.0, 5.5, 20.0], [9.0, 4.5, 2.0, 11.0, 5.5, 20.0]));
    p.levels.push(crate::project_fixture::Lev::new(12, "Level 3", 20.0));
    let i = import(&p);
    let [(_, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((c.base_offset, c.height), (609.6, 5486.4));
    // A node of another plan, or the same bottom, or two that disagree, moves nothing.
    for nodes in [
        vec![(100, [9.0, 4.5, 2.0, 11.0, 5.5, 20.0], [9.0, 4.5, 2.0, 13.0, 5.5, 20.0])],
        vec![(100, [9.0, 4.5, 0.0, 11.0, 5.5, 20.0], [9.0, 4.5, 0.0, 11.0, 5.5, 20.0])],
        vec![
            (100, [9.0, 4.5, 2.0, 11.0, 5.5, 20.0], [9.0, 4.5, 2.0, 11.0, 5.5, 20.0]),
            (100, [9.0, 4.5, 3.0, 11.0, 5.5, 20.0], [9.0, 4.5, 3.0, 11.0, 5.5, 20.0]),
        ],
        vec![(100, [9.0, 4.5, 2.0, 11.0, 5.5, 20.0], [9.0, 4.5, 2.0, 11.0, 5.5, 15.0])],
    ] {
        let mut q = p.clone();
        q.nodes = nodes;
        let i = import(&q);
        assert_eq!(columns(&i)[0].1.base_offset, 0.0);
    }
}

#[test]
fn a_column_that_names_no_level_stands_on_the_one_below_its_bottom_and_an_unnamed_type_gets_a_name() {
    let mut p = project(2024, COLUMNS);
    p.names.clear();
    p.recs.push(Rec::new(100, COLUMNS).at([9.0, 4.5, 10.5, 11.0, 5.5, 14.0], &[650, 100]));
    let i = import(&p);
    let [(_, c)] = columns(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((c.level, c.base_offset, c.type_name.as_str()), (level_named(&i, "Level 2"), 152.4, "Imported column"));
}

#[test]
fn the_columns_of_the_samples_are_those_of_rvt_rss_export() {
    use super::rvt_rs::{RAC_COLUMNS, RST_COLUMNS};
    let check = |path: &std::path::Path, theirs: &[(u32, [f64; 6])]| {
        let (i, from) = crate::import_path_with_origins(path, "x").unwrap();
        if !super::pinned(&i) {
            return;
        }
        assert_eq!(columns(&i).len(), theirs.len(), "{}: {:?}", path.display(), i.skipped);
        for (id, b) in theirs {
            let (_, c) = i
                .model
                .elements
                .iter()
                .find(|(m, _)| from[m] == format!("Structural column {id}"))
                .and_then(|(_, e)| columns_of(e))
                .unwrap_or_else(|| panic!("column {id} is missing"));
            let t = &i.model.types.structural_column[&c.type_name];
            let z0 = i.model.elevation(c.level) + c.base_offset;
            // The body of rvt-rs's export is the record box cut by the walls it joins: equal in plan and bottom here, and the top where
            // no slab was cut from it.
            let want =
                [(c.at.0, (b[0] + b[3]) / 2.0), (c.at.1, (b[1] + b[4]) / 2.0), (t.width, b[3] - b[0]), (t.depth, b[4] - b[1]), (z0, b[2])];
            for (k, (mine, theirs)) in want.into_iter().enumerate() {
                assert!((mine - theirs).abs() < 0.2, "column {id}, value {k}: {mine} against {theirs}");
            }
            assert!((z0 + c.height - b[5]).abs() < 0.2, "column {id}: top {} against {}", z0 + c.height, b[5]);
        }
    };
    for path in super::samples_of("racbasicsampleproject.rvt") {
        check(&path, &RAC_COLUMNS);
    }
    // From 2024: the 2023 file does not correct the box's bottom (its geometry node is laid out otherwise).
    for path in super::samples().into_iter().filter(|p| !p.to_string_lossy().contains("2023")) {
        check(&path, &RST_COLUMNS);
    }
}

fn columns_of(e: &Element) -> Option<((), &Column)> {
    match e {
        Element::Column(c) | Element::StructuralColumn(c) => Some(((), c)),
        _ => None,
    }
}

#[test]
fn a_mutation_sweep_over_a_project_of_columns_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = project(release, STRUCTURAL);
        column(&mut p, STRUCTURAL, 100);
        column(&mut p, STRUCTURAL, 101);
        p.data.push(Block { id: 101, payload: transform_bytes(0.5, [10.0, 5.0, 0.0]) });
        p.nodes.push((100, [9.0, 4.5, 2.0, 11.0, 5.5, 10.0], [9.0, 4.5, 2.0, 11.0, 5.5, 10.0]));
        p.members = 3;
        let bytes = p.partition_bytes();
        let mut seed = 0x0f0f_1e1e_2d2d_3c3cu64 ^ u64::from(release);
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
                assert!(columns(&i).len() <= 2, "round {round}");
            }
        }
    }
}

#[test]
fn column_types_of_one_name_are_one_type_when_their_size_matches_and_two_when_it_does_not() {
    let mut p = project(2024, STRUCTURAL);
    // A second symbol of the same name; columns of the first at 2 by 1 ft and 3 by 1 ft, and of the second at 2 by 1 ft.
    p.recs.push(Rec { placed: false, ..Rec::new(651, STRUCTURAL) });
    p.names.push(NameEntry { id: 651, name: "Concrete".into(), category: STRUCTURAL });
    for (id, symbol, x1) in [(100, 650u64, 11.0), (101, 651, 41.0), (102, 650, 71.5)] {
        let x0 = x1 - if id == 102 { 3.0 } else { 2.0 };
        p.recs.push(Rec::new(id, STRUCTURAL).at([x0, 4.5, 0.0, x1, 5.5, 10.0], &[10, 11, symbol, u64::from(id)]));
    }
    let i = import(&p);
    let names: Vec<&str> = columns(&i).iter().map(|(_, c)| c.type_name.as_str()).collect();
    assert_eq!(names, ["Concrete", "Concrete", "Concrete (2)"], "{:?}", i.skipped);
}
