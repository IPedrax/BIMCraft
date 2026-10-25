//! A Revit file read into a BIMCraft model.

use super::walls::base;
use crate::project_fixture::{Lev, Project, Rec};
use bimcraft_core::model::Element;

fn import(p: &Project) -> bimcraft_core::import::Imported {
    crate::import(&p.file(), "house").unwrap()
}

fn level_list(i: &bimcraft_core::import::Imported) -> Vec<(String, f64)> {
    i.model.levels().into_iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect()
}

#[test]
fn levels_come_in_lowest_first_in_millimetres_and_the_model_checks_out() {
    for release in [2019, 2023, 2024, 2026] {
        let mut p = Project::new(release);
        p.levels = vec![Lev::new(12, "Roof", 19.68503937007874), Lev::new(10, "Level 1", 0.0), Lev::new(11, "Sótão ☃", -3.0)];
        let i = import(&p);
        assert_eq!(level_list(&i), [("Sótão ☃".to_string(), -914.4), ("Level 1".into(), 0.0), ("Roof".into(), 6000.0)], "Revit {release}");
        assert_eq!(i.schema, format!("Revit {release}"));
        assert!(i.model.check().is_ok());
        assert_eq!(i.moved, None);
        assert_eq!(i.skipped, Vec::<String>::new(), "Revit {release}");
    }
}

#[test]
fn levels_that_share_a_name_are_told_apart_and_a_file_without_levels_gets_one() {
    let mut p = Project::new(2024);
    p.levels = vec![Lev::new(10, "Level", 0.0), Lev::new(11, "Level", 10.0), Lev::new(12, "Level", 20.0)];
    let names: Vec<String> = level_list(&import(&p)).into_iter().map(|l| l.0).collect();
    assert_eq!(names, ["Level", "Level (2)", "Level (3)"]);
    let bare = import(&Project::new(2024));
    assert_eq!(level_list(&bare), [("Level 1".to_string(), 0.0)]);
    assert!(bare.skipped.iter().any(|s| s.contains("no levels")), "{:?}", bare.skipped);
}

#[test]
fn what_the_inventory_cannot_make_a_level_of_is_listed() {
    let mut p = Project::new(2024);
    p.levels = vec![
        Lev::new(10, "Level 1", 0.0),
        Lev { owner: Some(4321), ..Lev::new(11, "Inside a family", 5.0) },
        Lev { named: false, ..Lev::new(12, "Nameless", 10.0) },
        Lev::new(13, "Far away", 1.0e7),
    ];
    let i = import(&p);
    assert_eq!(level_list(&i).iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Level 1"]);
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    assert!(says("it sits inside another element") && says("ids 11"), "{:?}", i.skipped);
    assert!(says("1 level") && says("no name"), "{:?}", i.skipped);
    assert!(says("Far away"), "a level past the model's limits is left out with the rule: {:?}", i.skipped);
    assert!(i.model.check().is_ok());
    // Each is a note that counts: `N kinds left out, why`, which the desktop adds up for its status line.
    let counted = |kind: &str, what: &str| i.skipped.iter().any(|s| s.starts_with(&format!("1 {kind} left out, ")) && s.contains(what));
    assert!(says("ids 11") && counted("level", "no name") && counted("level", "Far away"), "{:?}", i.skipped);
}

#[test]
fn a_file_that_is_not_a_project_is_refused_with_the_reason() {
    let e = crate::import(b"not a revit file", "x").err().unwrap();
    assert!(e.contains("Compound File"), "{e}");
    let basic = crate::fixture::basic_file_info(&crate::fixture::Basic::default());
    let family = crate::fixture::container(&[("BasicFileInfo", &basic)]);
    let e = crate::import(&family, "x").err().unwrap();
    assert!(e.contains("Formats/Latest"), "{e}");
    // The elements the project holds are not levels.
    let mut p = Project::new(2024);
    p.recs = vec![Rec::new(100, -2_000_011)];
    p.levels = vec![Lev::new(10, "Level 1", 0.0)];
    let i = import(&p);
    // A wall record with no data comes in along its own box: it is a wall of the file, and the report says what was not read of it.
    assert!(i.model.elements.values().any(|e| matches!(e, Element::Wall(_))), "a wall record with no data is still a wall");
}

#[test]
fn the_levels_of_real_files_come_in_as_the_storeys_of_rvt_rss_ifc_export() {
    // Names and elevations (mm) of the IfcBuildingStorey rvt-rs writes for the architectural basic sample, 2024 and 2025.
    let want = [
        ("Foundation", -800.0),
        ("Level 1 Living Rm.", -550.0),
        ("Level 1", 0.0),
        ("Ceiling", 2700.0),
        ("Level 2", 3000.0),
        ("Roof Line", 6000.0),
    ];
    for path in super::samples_of("racbasicsampleproject.rvt") {
        let i = crate::import_path(&path, "x").unwrap();
        let got: Vec<(&str, f64)> = i.model.levels().into_iter().map(|(_, l)| (l.name.as_str(), l.elevation)).collect();
        assert_eq!(got, want, "{}", path.display());
        assert_eq!(i.model.name, "racbasicsampleproject");
    }
    // The structural basic sample has nine levels, none a building story, and rvt-rs's export invents two storeys for it
    // from the elements' heights; the nine levels all come in here, since walls and beams stand on them.
    for path in super::samples() {
        let i = crate::import_path(&path, "x").unwrap();
        assert_eq!(i.model.levels().len(), 9, "{}", path.display());
        assert!(!i.skipped.iter().any(|s| s.contains("level")), "{:?}", i.skipped);
    }
    // Snowdon Towers (architectural): 18 levels and a note for the two inside other elements.
    for path in super::samples_of("snowdon-towers-sample-architectural.rvt") {
        let i = crate::import_path(&path, "x").unwrap();
        assert_eq!(i.model.levels().len(), 18);
        assert!(i.skipped.iter().any(|s| s.contains("2 levels left out") && s.contains("inside other elements")), "{:?}", i.skipped);
    }
}

#[test]
fn a_note_lists_the_first_eight_ids_and_says_there_are_more_only_when_there_are() {
    let said = |count: u32| {
        let mut p = base(2024);
        p.recs.push(Rec { placed: false, ..Rec::new(650, -2_000_100) });
        for id in 100..100 + count {
            p.recs.push(Rec::new(id, -2_000_100).at([0.0, 0.0, 0.0, 1.0, 1.0, 10.0], &[10, 11, 650, u64::from(id)]));
        }
        import(&p).skipped.into_iter().find(|s| s.contains("columns")).unwrap_or_default()
    };
    assert!(said(8).ends_with("(ids 100, 101, 102, 103, 104, 105, 106, 107)"), "{}", said(8));
    assert!(said(9).ends_with("(ids 100, 101, 102, 103, 104, 105, 106, 107, …)"), "{}", said(9));
}

#[test]
fn the_levels_that_have_no_name_are_counted_in_the_singular_or_the_plural() {
    let said = |unnamed: usize| {
        let mut p = base(2024);
        for k in 0..unnamed {
            p.levels.push(crate::project_fixture::Lev { named: false, ..crate::project_fixture::Lev::new(20 + k as u32, "x", 30.0) });
        }
        import(&p).skipped.into_iter().find(|s| s.contains("no name or elevation")).unwrap_or_default()
    };
    assert!(said(1).starts_with("1 level left out, no name"), "{}", said(1));
    assert!(said(2).starts_with("2 levels left out, no name"), "{}", said(2));
}

#[test]
fn what_an_element_was_in_the_file_is_told_as_its_kind_and_its_id() {
    use crate::import::origin;
    assert_eq!(origin("Wall 423099"), Some(("wall".to_string(), Ok(423099))));
    assert_eq!(origin("Structural column 12"), Some(("structural column".to_string(), Ok(12))), "a kind of two words");
    assert_eq!(origin("Level 'Far away'"), Some(("level".to_string(), Err("'Far away'"))));
    assert_eq!(origin("Level 'Roof 2'"), Some(("level".to_string(), Err("'Roof 2'"))), "a level's name may end in a number");
    assert_eq!(origin("Wall"), None);
    assert_eq!(origin(""), None);
}
