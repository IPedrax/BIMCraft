//! The element table, the element inventory, the levels.

use super::*;
use crate::fixture;
use crate::inventory::{Count, OST_LEVELS, category_name};
use crate::project_fixture::{Lev, Project, Rec};

const WALLS: i64 = -2_000_011;
const WINDOWS: i64 = -2_000_014;
const DOORS: i64 = -2_000_023;

fn open(project: &Project) -> Revit {
    Revit::from_bytes(project.file(), Limits::default()).unwrap()
}

/// Walls, doors (one a type's symbol), levels, and from 2024 a window in a wall's own record.
fn project(release: u32) -> Project {
    let mut p = Project::new(release);
    p.recs = vec![Rec::new(100, WALLS), Rec::new(101, WALLS), Rec::new(200, DOORS), Rec { placed: false, ..Rec::new(201, DOORS) }];
    if release >= 2024 {
        p.recs.push(Rec { inside: Some(105), ..Rec::new(300, WINDOWS) });
    }
    p.levels = vec![Lev::new(12, "Sótão ☃", -3.0), Lev::new(10, "Level 1", 0.0), Lev::new(11, "Level 2", 9.84251968503937)];
    p
}

fn counts(p: &Project) -> Vec<(i64, usize, usize)> {
    open(p).inventory().unwrap().counts().into_iter().map(|c| (c.category, c.elements, c.placed)).collect()
}

#[test]
fn the_inventory_counts_elements_by_category_and_lists_the_levels_in_both_record_forms() {
    for release in [2015, 2019, 2023, 2024, 2026, 2027] {
        let p = project(release);
        let inv = open(&p).inventory().unwrap();
        let mut want =
            vec![(OST_LEVELS, 3, 3), (DOORS, 2, 1), (WALLS, if release >= 2024 { 3 } else { 2 }, if release >= 2024 { 3 } else { 2 })];
        if release >= 2024 {
            want.push((WINDOWS, 1, 1));
        }
        want.sort();
        assert_eq!(counts(&p), want, "Revit {release}");
        let names: Vec<(&str, f64)> = inv.levels.iter().map(|l| (l.name.as_str(), l.elevation_feet)).collect();
        assert_eq!(names, [("Sótão ☃", -3.0), ("Level 1", 0.0), ("Level 2", 9.84251968503937)], "by elevation, Revit {release}");
        assert!((inv.levels[0].elevation_mm() + 914.4).abs() < 1e-9, "1 ft is 304.8 mm");
        assert_eq!(inv.unread_levels, 0);
        assert_eq!(inv.declared, crate::elem_table::parse(&p.element_table()).unwrap().len());
        // The class tag of a record is kept, so part 2 can ask the schema what it is.
        let wall = inv.elements.iter().find(|e| e.id == 100).unwrap();
        assert_eq!((wall.class_tag, wall.placed), (100, true));
    }
}

#[test]
fn records_that_run_across_gzip_members_are_found_whole() {
    for release in [2023, 2025] {
        let mut p = project(release);
        let whole = counts(&p);
        for members in [2, 3, 7] {
            p.members = members;
            assert_eq!(counts(&p), whole, "{members} members, Revit {release}");
        }
    }
}

#[test]
fn what_does_not_check_out_is_left_out() {
    for release in [2023, 2024] {
        let mut p = Project::new(release);
        p.recs = vec![
            Rec::new(100, WALLS),
            Rec { declared: false, ..Rec::new(101, WALLS) }, // an id the element table does not declare
            Rec::new(102, -5),                               // a category outside Revit's band
            Rec::new(103, -3_000_000),
        ];
        assert_eq!(counts(&p), [(WALLS, 1, 1)], "Revit {release}");
        // A bounding box that is not finite, or inside out.
        let mut bytes = p.partition_bytes();
        let marker = memchr::memmem::find_iter(&bytes, &[12, 0, 0xff, 0xff, 0xff, 0xff, 13, 0]).next().unwrap();
        bytes[marker + 8..marker + 16].copy_from_slice(&f64::NAN.to_le_bytes());
        let inv = Revit::from_bytes(p.file_from(&bytes), Limits::default()).unwrap().inventory().unwrap();
        assert!(inv.elements.is_empty(), "a NaN box in Revit {release}");
        bytes[marker + 8..marker + 16].copy_from_slice(&5.0f64.to_le_bytes()); // min x 5 over max x 1
        let inv = Revit::from_bytes(p.file_from(&bytes), Limits::default()).unwrap().inventory().unwrap();
        assert!(inv.elements.is_empty(), "an inside-out box in Revit {release}");
    }
}

#[test]
fn an_element_in_several_records_is_one_element_placed_if_any_record_is() {
    for release in [2023, 2024] {
        let mut p = Project::new(release);
        p.recs = vec![Rec { placed: false, ..Rec::new(100, WALLS) }, Rec::new(100, WALLS), Rec { placed: false, ..Rec::new(101, WALLS) }];
        let inv = open(&p).inventory().unwrap();
        assert_eq!(inv.records, 3);
        assert_eq!(inv.counts(), [Count { category: WALLS, elements: 2, placed: 1 }], "Revit {release}");
    }
}

#[test]
fn a_frame_without_an_id_of_its_own_takes_its_hosts_only_when_it_is_placed() {
    let mut p = Project::new(2024);
    // The second is a symbol frame of another category, so that a wrong attribution would show as another element.
    p.recs = vec![Rec { inside: Some(105), ..Rec::new(300, WINDOWS) }, Rec { inside: Some(105), placed: false, ..Rec::new(301, DOORS) }];
    // Only the placed frame is attributed (to its host, which is a wall of its own): the symbol frame has no id.
    let inv = open(&p).inventory().unwrap();
    let got: Vec<(i64, u32)> = inv.elements.iter().map(|e| (e.category, e.id)).collect();
    assert_eq!(got, [(WINDOWS, 105), (WALLS, 105)]);
}

#[test]
fn a_level_without_a_name_block_is_counted_as_unread_and_a_symbol_is_not_a_level() {
    let mut p = Project::new(2024);
    p.levels = vec![
        Lev::new(10, "Level 1", 0.0),
        Lev { named: false, ..Lev::new(11, "x", 3.0) },
        Lev { placed: false, ..Lev::new(12, "A type", 6.0) },
    ];
    for release in [2023, 2024] {
        p.release = release;
        let inv = open(&p).inventory().unwrap();
        assert_eq!(inv.levels.len(), 1, "Revit {release}");
        assert_eq!(inv.unread_levels, 1);
        assert_eq!(inv.counts(), [Count { category: OST_LEVELS, elements: 3, placed: 2 }]);
    }
}

#[test]
fn a_name_block_that_does_not_check_out_names_no_level() {
    for release in [2023, 2024] {
        let mut p = Project::new(release);
        p.levels = vec![Lev::new(10, "Level 1", 0.0)];
        let ok = open(&p).inventory().unwrap();
        assert_eq!((ok.levels.len(), ok.unread_levels), (1, 0), "Revit {release}");
        // The elevation must appear twice.
        p.levels[0].confirmed = false;
        let inv = open(&p).inventory().unwrap();
        assert_eq!((inv.levels.len(), inv.unread_levels), (0, 1), "unconfirmed, Revit {release}");
        // The three bytes after the run of `ff` are 0 or 1 and then 0 and 0.
        p.levels[0].confirmed = true;
        let mut bytes = p.partition_bytes();
        let run = if release >= 2024 { 56 } else { 28 };
        let at = memchr::memmem::find(&bytes, &vec![0xffu8; run]).unwrap() + run;
        for (pad, readable) in [(1u8, true), (2, false)] {
            bytes[at] = pad;
            let inv = Revit::from_bytes(p.file_from(&bytes), Limits::default()).unwrap().inventory().unwrap();
            assert_eq!(inv.levels.len(), usize::from(readable), "padding {pad}, Revit {release}");
        }
    }
}

#[test]
fn a_level_rewritten_in_a_later_partition_has_its_newest_name() {
    let named = |name: &str, feet: f64| {
        let mut p = Project::new(2024);
        p.levels = vec![Lev::new(10, name, feet)];
        p
    };
    let (old, new) = (named("Old name", 0.0), named("New name", 1.0));
    let basic = fixture::basic_file_info(&fixture::Basic { release: 2024, ..Default::default() });
    let schema = fixture::gzip(&Project::schema_bytes());
    let table = fixture::global(&new.element_table());
    let level_in = |at21: &Project, at22: &Project| {
        let (a, b) = (fixture::partition(&[&at21.partition_bytes()]), fixture::partition(&[&at22.partition_bytes()]));
        let streams: [(&str, &[u8]); 5] = [
            ("BasicFileInfo", &basic),
            ("Formats/Latest", &schema),
            ("Global/ElemTable", &table),
            ("Partitions/21", &a),
            ("Partitions/22", &b),
        ];
        let inv = Revit::from_bytes(container(&streams), Limits::default()).unwrap().inventory().unwrap();
        assert_eq!(inv.levels.len(), 1);
        (inv.levels[0].name.clone(), inv.levels[0].elevation_feet)
    };
    // The higher-numbered partition holds the current copy, whichever one the name sorts first in.
    assert_eq!(level_in(&old, &new), ("New name".to_string(), 1.0));
    assert_eq!(level_in(&new, &old), ("Old name".to_string(), 0.0));
}

/// Three levels: a storey, a level that is not one, and one whose block holds no setting.
fn storeys(release: u32) -> Project {
    let mut p = Project::new(release);
    p.levels = vec![
        Lev { story: Some(true), ..Lev::new(10, "Level 1", 0.0) },
        Lev { story: Some(false), ..Lev::new(11, "Top of Footing", -3.0) },
        Lev::new(12, "Level 2", 10.0),
    ];
    p
}

#[test]
fn a_levels_building_story_setting_is_read_in_both_record_forms() {
    for release in [2019, 2023, 2024, 2026] {
        let inv = open(&storeys(release)).inventory().unwrap();
        let seen: Vec<(&str, Option<bool>)> = inv.levels.iter().map(|l| (l.name.as_str(), l.building_story)).collect();
        assert_eq!(seen, [("Top of Footing", Some(false)), ("Level 1", Some(true)), ("Level 2", None)], "Revit {release}");
    }
}

#[test]
fn a_building_story_setting_that_is_not_a_bool_is_no_setting() {
    for release in [2023, 2024] {
        let p = storeys(release);
        let mut bytes = p.partition_bytes();
        // The setting follows an unset id, `00 01`, a number and the level's id; the first is Level 1's. Make it 2.
        let (unset, id_len) = if release >= 2024 { (8, 8) } else { (4, 4) };
        let anchor = [vec![0xff; unset], vec![0, 1]].concat();
        let at = memchr::memmem::find(&bytes, &anchor).unwrap() + anchor.len() + 8 + id_len;
        assert_eq!(bytes[at], 1, "Level 1 is a storey");
        bytes[at] = 2;
        let inv = Revit::from_bytes(p.file_from(&bytes), Limits::default()).unwrap().inventory().unwrap();
        let seen: Vec<Option<bool>> = inv.levels.iter().map(|l| l.building_story).collect();
        assert_eq!(seen, [Some(false), None, None], "Revit {release}");
    }
}

#[test]
fn a_level_inside_another_element_is_no_project_level_and_is_not_unread() {
    let mut p = storeys(2024);
    p.levels.push(Lev { owner: Some(4321), ..Lev::new(13, "Inside a family", 5.0) });
    let inv = open(&p).inventory().unwrap();
    let names: Vec<&str> = inv.levels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Top of Footing", "Level 1", "Level 2"]);
    assert_eq!(inv.owned_levels, [13]);
    assert_eq!(inv.unread_levels, 0, "an owned level is neither a level nor an unread one");
    // It is still an element of the project's levels category.
    assert_eq!(inv.counts().iter().find(|c| c.category == OST_LEVELS).map(|c| c.elements), Some(4));
    // A level whose block simply is not there is unread, not owned.
    let mut q = storeys(2024);
    q.levels.push(Lev { named: false, ..Lev::new(14, "Nameless", 5.0) });
    let inv = open(&q).inventory().unwrap();
    assert_eq!((inv.owned_levels.len(), inv.unread_levels), (0, 1));
    // Up to 2023 no level is measured inside another element: the shape is not looked for.
    assert!(open(&storeys(2023)).inventory().unwrap().owned_levels.is_empty());
}

#[test]
fn an_owner_slot_that_is_not_an_element_id_or_not_filled_is_no_owner() {
    let mut p = Project::new(2024);
    p.levels = vec![Lev { owner: Some(4321), ..Lev::new(13, "Inside", 5.0) }];
    let bytes = p.partition_bytes();
    let at = memchr::memmem::find(&bytes, &[1, 0, 0, 0, 13, 0, 0, 0]).unwrap() + 4;
    // The owner's id as 0, one of the eight `ff` gone, the owner's id past a `u32`, and a name of no length.
    for (range, value, says) in
        [(16..24, 0u8, "owner 0"), (8..9, 0, "a gap in the sentinel"), (20..21, 1, "owner past u32"), (0x43..0x47, 0, "no name")]
    {
        let mut bad = bytes.clone();
        bad[at + range.start..at + range.end].fill(value);
        let inv = Revit::from_bytes(p.file_from(&bad), Limits::default()).unwrap().inventory().unwrap();
        assert!(inv.owned_levels.is_empty(), "{says}");
    }
    let inv = open(&p).inventory().unwrap();
    assert_eq!(inv.owned_levels, [13], "the unmutated block is owned");
}

#[test]
fn an_inventory_that_cannot_be_read_is_an_error_that_says_why() {
    let basic = |release| fixture::basic_file_info(&fixture::Basic { release, ..Default::default() });
    let p = project(2024);
    let schema = fixture::gzip(&Project::schema_bytes());
    let table = fixture::global(&p.element_table());
    let part = fixture::partition(&[&p.partition_bytes()]);
    let streams = |release, schema: &[u8], table: &[u8], part: Option<&[u8]>| {
        let basic = basic(release);
        let mut all: Vec<(&str, &[u8])> = vec![("BasicFileInfo", &basic), ("Formats/Latest", schema), ("Global/ElemTable", table)];
        if let Some(part) = part {
            all.push(("Partitions/21", part));
        }
        Revit::from_bytes(container(&all), Limits::default()).unwrap().inventory().unwrap_err().to_string()
    };
    assert!(streams(2030, &schema, &table, Some(&part)).contains("does not guess"));
    assert!(streams(2010, &schema, &table, Some(&part)).contains("does not guess"));
    assert!(streams(2024, &schema, &table, None).contains("no Partitions"));
    let no_plane = fixture::gzip(&fixture::schema(&[
        fixture::Class { name: "Outline", fields: &[], base: None },
        fixture::Class { name: "ElementParents", fields: &[], base: None },
        fixture::Class { name: "ElementHeader", fields: &[], base: None },
    ]));
    assert!(streams(2024, &no_plane, &table, Some(&part)).contains("Plane"));
    assert!(streams(2024, &schema, &global(b"garbage"), Some(&part)).contains("ElemTable"));
}

#[test]
fn the_element_table_is_read_in_both_layouts_and_what_does_not_fit_is_refused() {
    for release in [2023, 2024] {
        let mut p = project(release);
        p.extra_ids = vec![7, 5000, 5001];
        let ids = crate::elem_table::parse(&p.element_table()).unwrap();
        assert!(ids.windows(2).all(|w| w[0] < w[1]) && ids.contains(&5001) && ids.contains(&7), "Revit {release}");
        assert_eq!(open(&p).element_ids().unwrap(), ids);
    }
    let table = project(2024).element_table();
    let mut falling = table.clone();
    falling[6 + 20..6 + 24].copy_from_slice(&u32::MAX.to_le_bytes()); // the first record's ids above the second's, in both columns
    falling[6..6 + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut huge = table.clone();
    huge[2..6].copy_from_slice(&u32::MAX.to_le_bytes()); // a count the table cannot hold
    let mut zero = table.clone();
    zero[2..6].copy_from_slice(&0u32.to_le_bytes());
    for (bad, says) in
        [(falling, "rise"), (huge, "rise"), (zero, "no elements"), (table[..4].to_vec(), "too short"), (Vec::new(), "too short")]
    {
        assert!(crate::elem_table::parse(&bad).unwrap_err().to_string().contains(says), "{says}");
    }
}

#[test]
fn a_workshared_table_whose_second_ids_do_not_rise_is_read_by_its_first_ids() {
    // A workshared table: its second ids repeat and fall in places, its first ids rise all the way.
    for release in [2023, 2024] {
        let table = project(release).element_table();
        let (size, id_at) = if release >= 2024 { (40, 20) } else { (28, 4) };
        let expected = crate::elem_table::parse(&table).unwrap();
        let mut worked = table.clone();
        let set = |bytes: &mut Vec<u8>, record: usize, at: usize, id: u32| {
            bytes[6 + record * size + at..6 + record * size + at + 4].copy_from_slice(&id.to_le_bytes());
        };
        set(&mut worked, 1, id_at, expected[0]); // a repeat
        set(&mut worked, 3, id_at, 1); // a fall
        assert_eq!(crate::elem_table::parse(&worked).unwrap(), expected, "Revit {release}");
        // Where both columns rise the second is the ElementId, as before.
        let mut apart = table.clone();
        set(&mut apart, 0, 0, 0);
        assert_eq!(crate::elem_table::parse(&apart).unwrap(), expected, "Revit {release}");
    }
}

#[test]
fn category_names_are_sorted_for_the_search_and_known_ones_resolve() {
    assert_eq!(category_name(WALLS), Some("Walls"));
    assert_eq!(category_name(OST_LEVELS), Some("Levels"));
    assert_eq!(category_name(-2_001_320), Some("Structural Framing"));
    assert_eq!(category_name(-2_009_664), None);
    assert_eq!(category_name(0), None);
    for id in [-2_009_030, -2_000_011, -2_001_000, -2_008_000, -2_000_240] {
        assert!(category_name(id).is_some(), "{id}");
    }
}

#[test]
fn a_mutation_sweep_over_a_project_never_panics_and_never_invents_elements() {
    for release in [2023, 2024] {
        let mut p = project(release);
        p.members = 3;
        let bytes = p.partition_bytes();
        let table = p.element_table();
        let clean = open(&p).inventory().unwrap();
        let limits = Limits { max_file: 1 << 20, max_streams: 64, max_stream: 1 << 20, max_inflated: 1 << 20 };
        let mut seed = 0x0123_4567_89ab_cdefu64 ^ u64::from(release);
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut read = 0;
        for round in 0..2500 {
            let mut part = bytes.clone();
            for _ in 0..1 + next() % 6 {
                let at = (next() % part.len() as u64) as usize;
                part[at] = match next() % 4 {
                    0 => 0xff,
                    1 => 0,
                    2 => part[(next() % part.len() as u64) as usize],
                    _ => next() as u8,
                };
            }
            if round % 11 == 0 {
                part.truncate((next() % part.len() as u64) as usize);
            }
            let mut file = Revit::from_bytes(p.file_from(&part), limits).unwrap();
            if round % 5 == 0 {
                // Break the element table too.
                let at = (next() % table.len() as u64) as usize;
                let mut broken = table.clone();
                broken[at] = next() as u8;
                let _ = crate::elem_table::parse(&broken);
            }
            if let Ok(inv) = file.inventory() {
                read += 1;
                // No damage can make an element the clean file does not hold appear with an undeclared id.
                let declared = file.element_ids().unwrap();
                assert!(inv.elements.iter().all(|e| declared.binary_search(&e.id).is_ok()), "round {round}");
                assert!(inv.levels.len() <= clean.levels.len() + 3);
            }
        }
        assert!(read > 500, "the sweep must reach the scanner: {read}");
    }
}

// ---------- real files ----------

#[test]
fn every_samples_inventory_matches_what_rvt_rs_finds_on_the_same_file() {
    // rvt-rs at b6086d4, on rstbasicsampleproject of 2023, 2024, 2025 and 2026: the same model saved by four releases.
    let declared = |year| match year {
        2023 => Some(13_821),
        2024 => Some(13_819),
        2025 => Some(13_861),
        2026 => Some(13_936),
        _ => None,
    };
    let levels = [
        ("FOUNDATION-2700", -8.858267716535433),
        ("Foundation", -3.937007874015748),
        ("Level Lower", -1.804461942257218),
        ("Level 1", 0.0),
        ("Ref Beam system", 8.001968503937007),
        ("Ceiling", 8.858267716536028),
        ("Level 2", 9.84251968503937),
        ("Top of Parapet", 19.68503937007874),
        ("Roof Level", 29.52755905511811),
    ];
    for path in samples() {
        let r = &mut Revit::open(&path, Limits::default()).unwrap();
        let inv = r.inventory().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let counts = inv.counts();
        eprintln!("{}: {} declared, {} records, {} categories", path.display(), inv.declared, inv.records, counts.len());
        if let Some(n) = metadata::folder_year(&path).and_then(declared) {
            assert_eq!(inv.declared, n);
        }
        assert_eq!(inv.records, 5851);
        assert_eq!(counts.len(), 46, "45 categories of element records and the levels");
        assert_eq!(counts.iter().map(|c| c.elements).sum::<usize>(), 5861);
        assert_eq!(counts.iter().map(|c| c.placed).sum::<usize>(), 5591);
        let of = |category| counts.iter().find(|c| c.category == category).map(|c| (c.elements, c.placed));
        assert_eq!(of(WALLS), Some((9, 9)));
        assert_eq!(of(-2_000_032), Some((5, 5)), "floors");
        assert_eq!(of(DOORS), Some((3, 2)), "three door elements, one of them a type's symbol");
        assert_eq!(of(-2_001_320), Some((425, 370)), "structural framing");
        assert_eq!(of(-2_001_330), Some((40, 30)), "structural columns");
        assert_eq!(of(-2_001_300), Some((43, 36)), "structural foundations");
        assert_eq!(of(-2_000_051), Some((3956, 3956)), "lines");
        assert_eq!(of(OST_LEVELS), Some((10, 9)));
        assert_eq!(inv.unread_levels, 0);
        let got: Vec<(&str, f64)> = inv.levels.iter().map(|l| (l.name.as_str(), l.elevation_feet)).collect();
        assert_eq!(got, levels, "the nine levels, by elevation");
        let schema = r.schema().unwrap();
        let wall = inv.elements.iter().find(|e| e.category == WALLS).unwrap();
        eprintln!("a wall's class: {:?}", schema.by_tag(wall.class_tag).map(|c| &c.name));
        // Every element's class tag names a class of the file's own schema.
        let unknown = inv.elements.iter().filter(|e| e.category != OST_LEVELS && schema.by_tag(e.class_tag).is_none()).count();
        assert_eq!(unknown, 0, "{}", path.display());
    }
}

/// A level as (name, elevation in mm, building story setting).
type Seen = (String, f64, Option<bool>);

/// The levels of a real file, and the ids of those inside other elements.
fn real_levels(path: &Path) -> (Vec<Seen>, Vec<u32>) {
    let inv = Revit::open(path, Limits::default()).unwrap().inventory().unwrap();
    let levels = inv.levels.iter().map(|l| (l.name.clone(), (l.elevation_mm() * 10.0).round() / 10.0, l.building_story)).collect();
    (levels, inv.owned_levels)
}

#[test]
fn the_building_story_settings_and_owned_levels_of_real_files_are_what_rvt_rs_reads() {
    // Autodesk's architectural basic sample, 2024 and 2025: six levels, every one a storey of rvt-rs's IFC export (names and
    // elevations equal to that export's IfcBuildingStorey, in metres there).
    let rac = [
        ("Foundation", -800.0),
        ("Level 1 Living Rm.", -550.0),
        ("Level 1", 0.0),
        ("Ceiling", 2700.0),
        ("Level 2", 3000.0),
        ("Roof Line", 6000.0),
    ];
    for path in samples_of("racbasicsampleproject.rvt") {
        let (levels, owned) = real_levels(&path);
        let got: Vec<(&str, f64)> = levels.iter().map(|(n, e, _)| (n.as_str(), *e)).collect();
        assert_eq!(got, rac, "{}", path.display());
        assert!(levels.iter().all(|l| l.2 == Some(true)) && owned.is_empty(), "{}", path.display());
    }
    // The structural basic sample sets Building Story off on all nine levels, in 2023 and 2024 alike.
    for path in samples() {
        let (levels, owned) = real_levels(&path);
        assert!(levels.len() == 9 && levels.iter().all(|l| l.2 == Some(false)) && owned.is_empty(), "{}", path.display());
    }
    // Snowdon Towers (2024), where rvt-rs measured it against Revit's own export (RE-51, RE-118): the architectural model has
    // 18 storeys and two levels inside other elements; the structural one 19 levels, of which seven (Top of Footing and the six
    // "TOS" ones) are not building stories.
    for path in samples_of("snowdon-towers-sample-architectural.rvt") {
        let (levels, owned) = real_levels(&path);
        assert_eq!((levels.len(), owned), (18, vec![1_981_744, 1_982_117]), "{}", path.display());
        assert!(levels.iter().all(|l| l.2 == Some(true)));
        assert_eq!(levels[0].0, "Parking");
        assert_eq!(levels[17].0, "Parapet 2");
    }
    for path in samples_of("snowdon-towers-sample-structural.rvt") {
        let (levels, owned) = real_levels(&path);
        assert_eq!((levels.len(), owned.len()), (19, 0), "{}", path.display());
        let off: Vec<&str> = levels.iter().filter(|l| l.2 == Some(false)).map(|l| l.0.as_str()).collect();
        assert_eq!(off, ["Top of Footing", "M1 TOS", "L2 TOS", "L3 TOS", "L4 TOS", "L5 TOS", "R2 TOS"], "{}", path.display());
        assert_eq!(levels.iter().filter(|l| l.2 == Some(true)).count(), 12);
    }
}

#[test]
fn the_record_chain_of_a_real_partition_has_a_record_for_every_declared_element() {
    for path in samples() {
        let r = &mut Revit::open(&path, Limits::default()).unwrap();
        if r.basic_file_info().unwrap().release < 2024 {
            continue; // up to 2023 the records have no chain
        }
        let schema = r.schema().unwrap();
        let name = r.stream_names().into_iter().find(|n| n.starts_with("Partitions/")).unwrap().to_string();
        let buf = r.inflate_all(&name).unwrap().bytes;
        let (records, end) = crate::records::chain_len(&buf, &schema).unwrap();
        // One record for each ElementId the element table declares, and then the rest of the partition (families' own documents).
        assert_eq!(records, r.element_ids().unwrap().len(), "{}", path.display());
        assert!(end < buf.len(), "{}", path.display());
    }
}

#[test]
fn a_real_file_damaged_in_its_partition_is_an_error_or_a_smaller_inventory_never_a_panic() {
    let Some(path) = samples().into_iter().next() else { return };
    let original = std::fs::read(&path).unwrap();
    let whole = Revit::from_bytes(original.clone(), Limits::default()).unwrap().inventory().unwrap();
    let limits = Limits { max_inflated: 64 << 20, ..Limits::default() };
    let mut seed = 0xdead_beef_1234_5678u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let (mut ok, mut failed) = (0, 0);
    for round in 0..12 {
        let mut bytes = original.clone();
        // The file is mostly its partition: flip bytes anywhere in it, and cut it short now and then.
        for _ in 0..1 + next() % 20 {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        if round % 4 == 3 {
            bytes.truncate(bytes.len() - (next() % 200_000) as usize);
        }
        match Revit::from_bytes(bytes, limits).and_then(|mut r| r.inventory()) {
            Ok(inv) => {
                ok += 1;
                assert!(inv.records <= whole.records && inv.elements.len() <= whole.elements.len() + 20, "round {round}");
            }
            Err(_) => failed += 1,
        }
    }
    eprintln!("{ok} read, {failed} refused");
}

#[test]
fn a_level_record_whose_flag_marker_or_header_is_damaged_is_no_record() {
    for release in [2023, 2024] {
        let mut p = Project::new(release);
        p.levels = vec![Lev::new(10, "Level 1", 0.0)];
        let bytes = p.partition_bytes();
        let records = |bytes: &[u8]| {
            let inv = Revit::from_bytes(p.file_from(bytes), Limits::default()).unwrap().inventory().unwrap();
            inv.counts().iter().find(|c| c.category == OST_LEVELS).map_or(0, |c| c.elements)
        };
        assert_eq!(records(&bytes), 1, "Revit {release}");
        let hit = memchr::memmem::find(&bytes, &OST_LEVELS.to_le_bytes()).unwrap();
        // From 2024 the flag is a `u16` 2 bytes before the category and the marker 0x3e after it; before, the header's tag is 6 bytes
        // before it and the marker 38 after.
        let damaged = if release >= 2024 { [hit - 2, hit + 0x3e] } else { [hit - 6, hit + 38] };
        for at in damaged {
            let mut bad = bytes.clone();
            bad[at] ^= 0x55;
            assert_eq!(records(&bad), 0, "Revit {release}, byte {at}");
        }
    }
}

#[test]
fn a_building_story_setting_may_come_some_bytes_after_the_levels_name() {
    for release in [2023, 2024] {
        let p = storeys(release);
        let mut bytes = p.partition_bytes();
        let (unset, _) = if release >= 2024 { (8, 8) } else { (4, 4) };
        let anchor = [vec![0xff; unset], vec![0, 1]].concat();
        let at = memchr::memmem::find(&bytes, &anchor).unwrap();
        bytes.splice(at..at, [0x44u8; 7]);
        let inv = Revit::from_bytes(p.file_from(&bytes), Limits::default()).unwrap().inventory().unwrap();
        let seen: Vec<Option<bool>> = inv.levels.iter().map(|l| l.building_story).collect();
        assert_eq!(seen, [Some(false), Some(true), None], "Revit {release}");
    }
}
