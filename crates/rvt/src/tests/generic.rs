//! The other placed elements: each comes in as a box-shaped component of a family of its category.

use super::walls::{base, import, level_named};
use crate::project_fixture::Rec;
use bimcraft_core::family::Category;
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Component, Element};

const PIPE_FITTINGS: i64 = -2_008_049;
const PLUMBING_FIXTURES: i64 = -2_001_160;
const MECHANICAL_EQUIPMENT: i64 = -2_001_140;
const GENERIC_MODELS: i64 = -2_000_151;

fn components(i: &Imported) -> Vec<&Component> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Component(c) => Some(c),
            _ => None,
        })
        .collect()
}

#[test]
fn a_placed_element_of_another_category_comes_in_as_a_box_of_a_family_of_its_category() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A foot cube on level 2 (10 ft), a 2 by 1 by 3 ft fixture on level 1, and a generic model above level 2.
        p.recs.push(Rec::new(300, PIPE_FITTINGS).at([1.0, 2.0, 10.0, 2.0, 3.0, 11.0], &[]));
        p.recs.push(Rec::new(301, PLUMBING_FIXTURES).at([0.0, 0.0, 0.0, 2.0, 1.0, 3.0], &[]));
        p.recs.push(Rec::new(302, GENERIC_MODELS).at([4.0, 4.0, 15.0, 5.0, 6.0, 16.0], &[]));
        p.recs.push(Rec { placed: false, ..Rec::new(303, GENERIC_MODELS) }); // a symbol is no element
        p.recs.push(Rec::new(304, GENERIC_MODELS).at([0.0, 0.0, 0.0, 1.0, 1.0, 0.0], &[])); // flat: no body to import
        let i = import(&p);
        let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
        let got: Vec<(&str, f64, f64, f64, bimcraft_core::ids::Guid)> =
            components(&i).iter().map(|c| (c.family.as_str(), c.at.0, c.at.1, c.offset, c.level)).collect();
        assert_eq!(
            got,
            [
                ("Pipe Fittings", 457.2, 762.0, 0.0, l2),
                ("Plumbing Fixtures", 304.8, 152.4, 0.0, l1),
                ("Generic Models", 1371.6, 1524.0, 1524.0, l2),
            ],
            "Revit {release}"
        );
        let size = |c: &Component| ["Width", "Depth", "Height"].map(|n| c.params[n].as_f64().unwrap_or(f64::NAN));
        let sizes: Vec<[f64; 3]> = components(&i).iter().map(|c| size(c)).collect();
        assert_eq!(sizes, [[304.8, 304.8, 304.8], [609.6, 304.8, 914.4], [304.8, 609.6, 304.8]], "Revit {release}");
        // A family for each category, of the category's kind of component, and the model checks.
        let kinds: Vec<(&str, Category)> =
            ["Pipe Fittings", "Plumbing Fixtures", "Generic Models"].into_iter().map(|n| (n, i.model.families[n].category)).collect();
        assert_eq!(
            kinds,
            [("Pipe Fittings", Category::PipeFitting), ("Plumbing Fixtures", Category::Plumbing), ("Generic Models", Category::Generic)]
        );
        assert!(i.model.check().is_ok());
        // Each is told as coming in as a box, and none is listed as unread.
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(says("1 pipe fitting: comes in as a box") && says("1 plumbing fixture: comes in as a box"), "{:?}", i.skipped);
        assert!(says("1 generic model left out, it has no volume") && says("ids 304"), "{:?}", i.skipped);
        assert!(!i.skipped.iter().any(|s| s.contains("no reader")), "{:?}", i.skipped);
    }
}

#[test]
fn a_placed_element_of_a_category_with_no_name_comes_in_as_a_box_of_a_family_named_by_the_categorys_id() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // Two categories the table has no name for: boxes as in the test above, one flat, and a symbol that is no element.
        p.recs.push(Rec::new(300, -2_008_123).at([1.0, 2.0, 10.0, 2.0, 3.0, 11.0], &[]));
        p.recs.push(Rec::new(301, -2_008_123).at([0.0, 0.0, 0.0, 2.0, 1.0, 3.0], &[]));
        p.recs.push(Rec::new(302, -2_002_000).at([4.0, 4.0, 15.0, 5.0, 6.0, 16.0], &[]));
        p.recs.push(Rec::new(303, -2_002_000).at([0.0, 0.0, 0.0, 1.0, 1.0, 0.0], &[]));
        p.recs.push(Rec { placed: false, ..Rec::new(304, -2_002_000) });
        // A category with a name and no reader is left out as ever: only a category the reader cannot name is read by its id.
        p.recs.push(Rec::new(305, -2_009_000).at([0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[]));
        p.recs.push(Rec::new(306, -2_000_051).at([0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[]));
        let i = import(&p);
        let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
        let got: Vec<(&str, f64, f64, f64, bimcraft_core::ids::Guid)> =
            components(&i).iter().map(|c| (c.family.as_str(), c.at.0, c.at.1, c.offset, c.level)).collect();
        assert_eq!(
            got,
            [
                ("Category -2008123", 457.2, 762.0, 0.0, l2),
                ("Category -2008123", 304.8, 152.4, 0.0, l1),
                ("Category -2002000", 1371.6, 1524.0, 1524.0, l2),
            ],
            "Revit {release}"
        );
        let size = |c: &Component| ["Width", "Depth", "Height"].map(|n| c.params[n].as_f64().unwrap_or(f64::NAN));
        let sizes: Vec<[f64; 3]> = components(&i).iter().map(|c| size(c)).collect();
        assert_eq!(sizes, [[304.8, 304.8, 304.8], [609.6, 304.8, 914.4], [304.8, 609.6, 304.8]], "Revit {release}");
        assert_eq!(i.model.families["Category -2008123"].category, Category::Generic);
        assert!(i.model.check().is_ok());
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(
            says("2 category -2008123 elements: comes in as a box") && says("1 category -2002000 element: comes in as a box"),
            "{:?}",
            i.skipped
        );
        assert!(says("its category has no name here"), "{:?}", i.skipped);
        assert!(says("1 category -2002000 element left out, it has no volume") && says("ids 303"), "{:?}", i.skipped);
        let unread: Vec<&str> = i.skipped.iter().map(String::as_str).filter(|s| s.contains("no reader")).collect();
        assert_eq!(unread.len(), 2, "{unread:?}");
        assert!(unread.iter().any(|s| s.contains("Structural Rebar")) && unread.iter().any(|s| s.contains("Lines")), "{unread:?}");
    }
}

#[test]
fn an_element_of_a_category_with_no_name_that_has_the_box_of_a_named_elements_is_its_companion_and_is_left_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        let b = [8.0, 8.0, 0.0, 9.0, 9.0, 1.0];
        p.recs.push(Rec::new(300, GENERIC_MODELS).at(b, &[]));
        // The same box to the micron (the noise of a float) is the same body; a millimetre off is another element.
        p.recs.push(Rec::new(301, -2_008_072).at(b.map(|x| x + 1e-10), &[]));
        p.recs.push(Rec::new(302, -2_008_072).at([8.0, 8.0, 0.0, 9.0, 9.0, 1.0 + 1.0 / 304.8], &[]));
        let i = import(&p);
        let got: Vec<&str> = components(&i).iter().map(|c| c.family.as_str()).collect();
        assert_eq!(got, ["Generic Models", "Category -2008072"], "Revit {release}");
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(
            says("1 category -2008072 element left out, its box is that of an element of a category the reader knows") && says("ids 301)"),
            "{:?}",
            i.skipped
        );
        assert!(says("1 category -2008072 element: comes in as a box") && says("ids 302)"), "{:?}", i.skipped);
        assert!(i.model.check().is_ok());
    }
}

#[test]
fn a_family_of_a_category_is_one_family_for_all_its_elements_and_names_a_single_type() {
    let mut p = base(2024);
    for (k, id) in (300..306).enumerate() {
        let z = k as f64;
        p.recs.push(Rec::new(id, MECHANICAL_EQUIPMENT).at([0.0, 0.0, z, 1.0, 1.0, z + 1.0], &[]));
    }
    let i = import(&p);
    assert_eq!(components(&i).len(), 6);
    assert!(components(&i).iter().all(|c| c.family == "Mechanical Equipment" && c.type_name == "Imported"));
    assert_eq!(i.model.families.keys().filter(|n| n.as_str() == "Mechanical Equipment").count(), 1);
    assert!(i.model.check().is_ok());
}

#[test]
fn a_mutation_sweep_over_a_project_of_boxes_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        p.recs.push(Rec::new(300, PIPE_FITTINGS).at([1.0, 2.0, 10.0, 2.0, 3.0, 11.0], &[]));
        p.recs.push(Rec::new(301, PLUMBING_FIXTURES).at([0.0, 0.0, 0.0, 2.0, 1.0, 3.0], &[]));
        p.recs.push(Rec::new(302, GENERIC_MODELS).at([4.0, 4.0, 15.0, 5.0, 6.0, 16.0], &[]));
        p.recs.push(Rec::new(303, -2_008_123).at([4.0, 4.0, 15.0, 5.0, 6.0, 16.0], &[]));
        p.members = 3;
        super::sweep(&p, 0x2e3d_4c5b_6a79_8897 ^ u64::from(release), 800, |i, round| {
            assert!(components(i).len() <= 4, "round {round}");
        });
    }
}
