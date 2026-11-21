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
fn a_curtain_grid_line_is_left_out_not_boxed_whatever_its_box() {
    // A curtain wall's grid line (class CurtainGridLine) is a line its mullions and panels are set out by, which come in on their own:
    // one across a sloping or curved face has a box with volume, a solid block where Revit draws nothing.
    let mut p = base(2024);
    let mut line = Rec::new(300, -2_000_321).at([0.0, 0.0, 0.0, 3.6, 8.0, 50.0], &[]);
    line.class_tag = crate::project_fixture::tag("CurtainGridLine");
    p.recs.push(line);
    let i = import(&p);
    assert!(components(&i).is_empty(), "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("it is a curtain grid line") && s.contains("ids 300)")), "{:?}", i.skipped);
    assert!(i.model.check().is_ok());
}

#[test]
fn a_model_group_and_terrain_are_left_out_not_boxed() {
    // A model group (class ElementGroup) is no body: its members come in on their own. Terrain (class Toposolid, or the older
    // SiteSurface) is a surface the reader does not read, whose box would be a solid block over the whole site.
    use crate::project_fixture::{GROUP, SITE_SURFACE, TOPOSOLID};
    for release in [2019, 2024] {
        let mut p = base(release);
        p.recs.push(Rec::new(300, GENERIC_MODELS).at([8.0, 8.0, 0.0, 9.0, 9.0, 1.0], &[]));
        let mut group = Rec::new(301, -2_000_095).at([0.0, 0.0, 0.0, 20.0, 20.0, 10.0], &[]);
        group.class_tag = GROUP;
        let mut topo = Rec::new(302, -2_001_122).at([-100.0, -100.0, -20.0, 500.0, 500.0, 10.0], &[]);
        topo.class_tag = TOPOSOLID;
        let mut site = Rec::new(303, -2_001_340).at([-100.0, -100.0, -20.0, 500.0, 500.0, 10.0], &[]);
        site.class_tag = SITE_SURFACE;
        p.recs.extend([group, topo, site]);
        let i = import(&p);
        let got: Vec<&str> = components(&i).iter().map(|c| c.family.as_str()).collect();
        assert_eq!(got, ["Generic Models"], "Revit {release}: {:?}", i.skipped);
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(says("it is a model group") && says("ids 301)"), "{:?}", i.skipped);
        assert!(says("it is terrain") && says("ids 302)") && says("ids 303)"), "{:?}", i.skipped);
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
        // A typed, turned one: its symbol, its name and its placement in its data are mutated too.
        let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
        desk(&mut p, 650, "Desk", local);
        placed(&mut p, 304, 650, local, 0.5, [10.0, 5.0, 0.0]);
        // And its graphics: what it draws, and the symbol's solid.
        solid(&mut p, 650, [2.0, 1.0, 2.5]);
        draws(&mut p, 304, 650, 0.5, [10.0, 5.0, 0.0], envelope(local, 0.5, [10.0, 5.0, 0.0]));
        p.members = 3;
        super::sweep(&p, 0x2e3d_4c5b_6a79_8897 ^ u64::from(release), 800, |i, round| {
            assert!(components(i).len() <= 5, "round {round}");
            for c in components(i) {
                assert!(c.angle.is_finite() && c.at.0.is_finite() && c.at.1.is_finite(), "round {round}");
                for v in c.params.values() {
                    assert!(v.as_f64().is_some_and(f64::is_finite), "round {round}: {v}");
                }
            }
        });
    }
}

const FURNITURE: i64 = -2_000_080;

/// The box round `local` (a box in the family's own axes, feet) turned by `angle` radians about up and moved to `origin`: the record
/// box Revit writes for such an instance.
fn envelope(local: [f64; 6], angle: f64, origin: [f64; 3]) -> [f64; 6] {
    let (sin, cos) = angle.sin_cos();
    let mut b = [f64::INFINITY, f64::INFINITY, local[2] + origin[2], f64::NEG_INFINITY, f64::NEG_INFINITY, local[5] + origin[2]];
    for x in [local[0], local[3]] {
        for y in [local[1], local[4]] {
            let (px, py) = (origin[0] + x * cos - y * sin, origin[1] + x * sin + y * cos);
            b[0] = b[0].min(px);
            b[1] = b[1].min(py);
            b[3] = b[3].max(px);
            b[4] = b[4].max(py);
        }
    }
    b
}

/// A furniture symbol `symbol` named `name`, whose box in the family's axes is `local`.
fn desk(p: &mut crate::project_fixture::Project, symbol: u32, name: &str, local: [f64; 6]) {
    use crate::project_fixture::NameEntry;
    p.recs.push(Rec { placed: false, ..Rec::new(symbol, FURNITURE) }.at(local, &[]));
    p.names.push(NameEntry { id: symbol, name: name.into(), category: FURNITURE });
}

/// A furniture instance of `symbol`, turned by `angle` about `origin`, its record box the symbol's box placed so.
fn placed(p: &mut crate::project_fixture::Project, id: u32, symbol: u32, local: [f64; 6], angle: f64, origin: [f64; 3]) {
    use crate::project_fixture::{Block, transform_bytes};
    p.recs.push(Rec::new(id, FURNITURE).at(envelope(local, angle, origin), &[u64::from(symbol), u64::from(id)]));
    p.data.push(Block { id, payload: transform_bytes(angle, origin) });
}

fn param(c: &Component, name: &str) -> f64 {
    c.params.get(name).and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN)
}

#[test]
fn a_turned_component_comes_in_as_its_types_box_at_its_placement_with_its_types_name() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A desk 2 by 1 ft and 2.5 ft tall whose family's origin is its corner, turned 30 degrees about (10, 5) on level 1.
        let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
        desk(&mut p, 650, "Desk 60x30", local);
        placed(&mut p, 300, 650, local, 30f64.to_radians(), [10.0, 5.0, 0.0]);
        let i = import(&p);
        let [c] = components(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        assert_eq!((c.family.as_str(), c.type_name.as_str()), ("Furniture", "Desk 60x30"), "Revit {release}");
        assert_eq!((c.at.0, c.at.1, c.offset), (3048.0, 1524.0, 0.0), "Revit {release}");
        assert!((c.angle - 30.0).abs() < 1e-3, "Revit {release}: {}", c.angle);
        let got = ["Width", "Depth", "Height", "Offset X", "Offset Y"].map(|n| param(c, n));
        assert_eq!(got, [609.6, 304.8, 762.0, 304.8, 152.4], "Revit {release}");
        assert!(i.model.check().is_ok(), "{:?}", i.model.check());
        // Its body is the desk where Revit has it: the 3D view's box of it is the record's box.
        let ids: std::collections::HashSet<_> =
            i.model.elements.iter().filter(|(_, e)| matches!(e, Element::Component(_))).map(|(id, _)| *id).collect();
        let boxes = bimcraft_core::view3d::Scene::new(&i.model).boxes(&ids);
        let [(lo, hi)] = boxes.values().copied().collect::<Vec<_>>()[..] else { panic!("Revit {release}: {boxes:?}") };
        let want = envelope(local, 30f64.to_radians(), [10.0, 5.0, 0.0]).map(|f| f * 304.8);
        let got = [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.5), "Revit {release}: {got:?} against {want:?}");
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(says("1 furniture item: comes in as a box of its type's box, turned as placed"), "{:?}", i.skipped);
        assert!(!says("comes in as a box of its record's box"), "{:?}", i.skipped);
    }
}

#[test]
fn components_of_two_types_are_two_types_of_their_categorys_family() {
    let mut p = base(2024);
    let (a, b) = ([0.0, 0.0, 0.0, 2.0, 1.0, 2.5], [-1.5, -1.5, 0.0, 1.5, 1.5, 2.4]);
    desk(&mut p, 650, "Desk", a);
    desk(&mut p, 651, "Round Table", b);
    placed(&mut p, 300, 650, a, 0.0, [10.0, 5.0, 0.0]);
    placed(&mut p, 301, 651, b, 0.0, [20.0, 5.0, 0.0]);
    placed(&mut p, 302, 650, a, std::f64::consts::FRAC_PI_2, [30.0, 5.0, 0.0]);
    let i = import(&p);
    let got: Vec<(&str, f64)> = components(&i).iter().map(|c| (c.type_name.as_str(), c.angle)).collect();
    assert_eq!(got, [("Desk", 0.0), ("Round Table", 0.0), ("Desk", 90.0)]);
    let family = &i.model.families["Furniture"];
    assert_eq!(family.types.keys().map(String::as_str).collect::<Vec<_>>(), ["Desk", "Round Table"]);
    assert_eq!(i.model.families.keys().filter(|n| n.starts_with("Furniture")).count(), 1);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
}

#[test]
fn a_component_whose_placement_does_not_give_its_record_box_comes_in_as_that_box_and_says_why() {
    let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
    // The symbol's box placed so is not the record's box: the record wins, with its type's name.
    let mut p = base(2024);
    desk(&mut p, 650, "Desk", local);
    placed(&mut p, 300, 650, local, 30f64.to_radians(), [10.0, 5.0, 0.0]);
    p.recs.iter_mut().filter(|r| r.id == 300).for_each(|r| r.bbox[3] += 1.0);
    // No placement in its data.
    desk(&mut p, 651, "Chair", local);
    p.recs.push(Rec::new(301, FURNITURE).at([20.0, 5.0, 0.0, 22.0, 6.0, 2.5], &[651, 301]));
    // A placement, and a type with no name entry: the reader does not know which type it is.
    p.recs.push(Rec { placed: false, ..Rec::new(652, FURNITURE) }.at(local, &[]));
    placed(&mut p, 302, 652, local, 0.5, [40.0, 5.0, 0.0]);
    let i = import(&p);
    let got: Vec<(&str, f64, f64, f64)> =
        components(&i).iter().map(|c| (c.type_name.as_str(), c.angle, param(c, "Offset X"), param(c, "Offset Y"))).collect();
    assert_eq!(got, [("Desk", 0.0, 0.0, 0.0), ("Chair", 0.0, 0.0, 0.0), ("Imported", 0.0, 0.0, 0.0)], "{:?}", i.skipped);
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    let told = |why: &str, id: &str| {
        i.skipped.iter().any(|s| s.contains("comes in as a box of its record's box") && s.contains(why) && s.contains(id))
    };
    assert!(told("its type's box, placed, is not its record's box", "ids 300)"), "{:?}", i.skipped);
    assert!(told("its placement is not read", "ids 301)"), "{:?}", i.skipped);
    assert!(told("its type is not known", "ids 302)"), "{:?}", i.skipped);
    assert!(!says("turned as placed"), "{:?}", i.skipped);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
}

/// As Revit 2025 writes them: a type with no record of its own, named by a name entry its instances' first lists hold, and a family
/// named by one their second lists hold (where they do).
fn named(p: &mut crate::project_fixture::Project, id: u32, name: &str) {
    p.names.push(crate::project_fixture::NameEntry { id, name: name.into(), category: FURNITURE });
}

/// A furniture instance of type `ty` (and family `family`, where it names one), its box in its own axes `local`, turned by `angle`
/// about `origin`.
fn instance(
    p: &mut crate::project_fixture::Project,
    (id, ty, family): (u32, u64, Option<u64>),
    local: [f64; 6],
    angle: f64,
    origin: [f64; 3],
) {
    use crate::project_fixture::{Block, transform_bytes};
    let mut r = Rec::new(id, FURNITURE).at(envelope(local, angle, origin), &[ty, u64::from(id)]);
    r.refs2 = family.into_iter().collect();
    p.recs.push(r);
    p.data.push(Block { id, payload: transform_bytes(angle, origin) });
}

#[test]
fn a_type_and_a_family_named_only_by_name_entries_make_a_family_of_that_name_with_that_type() {
    let mut p = base(2025);
    named(&mut p, 700, "36\" Diameter");
    named(&mut p, 701, "Table-Dining Round");
    named(&mut p, 710, "Seat");
    named(&mut p, 711, "Chair-Breuer");
    let table = [-1.5, -1.5, 0.0, 1.5, 1.5, 2.5];
    // Square to the axes, naming its family; square, not naming it (its type's other instances say which it is).
    instance(&mut p, (300, 700, Some(701)), table, 0.0, [10.0, 5.0, 0.0]);
    instance(&mut p, (301, 700, None), table, std::f64::consts::PI, [20.0, 5.0, 0.0]);
    // A chair whose family's origin is at its back: square at a quarter turn, and turned by 30 degrees.
    let chair = [-1.0, 0.0, 0.0, 1.0, 1.5, 3.0];
    instance(&mut p, (302, 710, Some(711)), chair, std::f64::consts::FRAC_PI_2, [30.0, 5.0, 0.0]);
    instance(&mut p, (303, 710, Some(711)), chair, 30f64.to_radians(), [40.0, 5.0, 0.0]);
    let i = import(&p);
    let got: Vec<(&str, &str, f64, [f64; 5])> = components(&i)
        .iter()
        .map(|c| {
            let ps = ["Width", "Depth", "Height", "Offset X", "Offset Y"].map(|n| param(c, n));
            (c.family.as_str(), c.type_name.as_str(), c.angle, ps)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("Table-Dining Round", "36\" Diameter", 0.0, [914.4, 914.4, 762.0, 0.0, 0.0]),
            ("Table-Dining Round", "36\" Diameter", 180.0, [914.4, 914.4, 762.0, 0.0, 0.0]),
            ("Chair-Breuer", "Seat", 90.0, [609.6, 457.2, 914.4, 0.0, 228.6]),
            ("Chair-Breuer", "Seat", 30.0, [609.6, 457.2, 914.4, 0.0, 228.6]),
        ],
        "{:?}",
        i.skipped
    );
    let at: Vec<(f64, f64)> = components(&i).iter().map(|c| (c.at.0, c.at.1)).collect();
    assert_eq!(at, [(3048.0, 1524.0), (6096.0, 1524.0), (9144.0, 1524.0), (12192.0, 1524.0)]);
    assert_eq!(i.model.families["Table-Dining Round"].category, Category::Furniture);
    assert!(i.model.families.keys().all(|n| n != "Furniture"), "no element fell back to the category's family");
    // Each solid stands on its record's box.
    let ids: std::collections::HashSet<_> =
        i.model.elements.iter().filter(|(_, e)| matches!(e, Element::Component(_))).map(|(id, _)| *id).collect();
    let boxes = bimcraft_core::view3d::Scene::new(&i.model).boxes(&ids);
    let records: Vec<[f64; 6]> = p.recs.iter().filter(|r| r.category == FURNITURE).map(|r| r.bbox.map(|f| f * 304.8)).collect();
    let solids: Vec<[f64; 6]> = boxes.values().map(|(lo, hi)| [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]]).collect();
    for (s, r) in solids.iter().zip(&records) {
        assert!(s.iter().zip(r).all(|(a, b)| (a - b).abs() < 0.5), "{s:?} against {r:?}");
    }
    assert!(
        i.skipped.iter().any(|s| s.contains("4 furniture items: comes in as a box of its type's box, turned as placed")),
        "{:?}",
        i.skipped
    );
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
}

#[test]
fn a_type_and_a_family_named_only_by_name_entries_are_read_from_2025_on() {
    // The finding was measured on 2025 files alone: before that, a type is a family symbol, and a name entry alone names nothing.
    for (release, want) in [(2024, ("Furniture", "Imported")), (2025, ("Chair-Breuer", "Seat"))] {
        let mut p = base(release);
        named(&mut p, 710, "Seat");
        named(&mut p, 711, "Chair-Breuer");
        instance(&mut p, (302, 710, Some(711)), [-1.0, 0.0, 0.0, 1.0, 1.5, 3.0], 0.0, [30.0, 5.0, 0.0]);
        let i = import(&p);
        let [c] = components(&i)[..] else { panic!("Revit {release}: {:?}", i.skipped) };
        assert_eq!((c.family.as_str(), c.type_name.as_str()), want, "Revit {release}");
        assert!(i.model.check().is_ok(), "Revit {release}: {:?}", i.model.check());
    }
}

#[test]
fn a_turned_instance_whose_type_no_square_instance_measures_comes_in_as_its_record_box() {
    let mut p = base(2025);
    named(&mut p, 710, "Seat");
    named(&mut p, 711, "Chair-Breuer");
    let chair = [-1.0, 0.0, 0.0, 1.0, 1.5, 3.0];
    instance(&mut p, (303, 710, Some(711)), chair, 30f64.to_radians(), [40.0, 5.0, 0.0]);
    let i = import(&p);
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!((c.family.as_str(), c.type_name.as_str(), c.angle), ("Chair-Breuer", "Seat", 0.0));
    assert!(
        i.skipped
            .iter()
            .any(|s| s.contains("comes in as a box of its record's box") && s.contains("no instance of its type stands square")),
        "{:?}",
        i.skipped
    );
}

const LIGHTING: i64 = -2_001_120;

/// A placement of axes `axes` (the instance's X, Y and Z in project axes) at `origin`, as its data holds it: the rotation row by row.
fn axes_bytes(axes: [[f64; 3]; 3], origin: [f64; 3]) -> Vec<u8> {
    let rows = (0..3).flat_map(|i| [axes[0][i], axes[1][i], axes[2][i]]);
    rows.chain(origin).flat_map(f64::to_le_bytes).collect()
}

/// The box round `local` placed by `axes` at `origin`.
fn placed_by(local: [f64; 6], axes: [[f64; 3]; 3], origin: [f64; 3]) -> [f64; 6] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for x in [local[0], local[3]] {
        for y in [local[1], local[4]] {
            for z in [local[2], local[5]] {
                for k in 0..3 {
                    let v = origin[k] + x * axes[0][k] + y * axes[1][k] + z * axes[2][k];
                    b[k] = b[k].min(v);
                    b[k + 3] = b[k + 3].max(v);
                }
            }
        }
    }
    b
}

#[test]
fn a_component_hung_upside_down_or_on_a_wall_comes_in_turned_and_one_on_a_slope_as_its_record_box() {
    let mut p = base(2025);
    for (id, name) in [(700, "2x2"), (701, "Troffer"), (710, "Sconce"), (711, "Wall Light")] {
        p.names.push(crate::project_fixture::NameEntry { id, name: name.into(), category: LIGHTING });
    }
    // A troffer hung from a ceiling at 9 ft: its Z down; a sconce on a wall facing east, its Y up; one on a slope.
    let troffer = [-1.0, -1.0, -0.5, 1.0, 1.0, 0.0];
    let down = [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]];
    let sconce = [-0.5, 0.0, 0.0, 0.5, 1.0, 0.75];
    let wall = [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
    let slope = [[0.6, 0.0, 0.8], [0.0, 1.0, 0.0], [-0.8, 0.0, 0.6]];
    let placed = [
        (300u32, 700u64, 701u64, troffer, down, [10.0, 5.0, 9.0]),
        (301, 710, 711, sconce, wall, [20.0, 5.0, 5.0]),
        (302, 710, 711, sconce, slope, [30.0, 5.0, 5.0]),
    ];
    for (id, ty, family, local, axes, origin) in placed {
        let mut r = Rec::new(id, LIGHTING).at(placed_by(local, axes, origin), &[ty, u64::from(id)]);
        r.refs2 = vec![family];
        p.recs.push(r);
        p.data.push(crate::project_fixture::Block { id, payload: axes_bytes(axes, origin) });
    }
    let i = import(&p);
    let got: Vec<(&str, &str, f64)> = components(&i).iter().map(|c| (c.family.as_str(), c.type_name.as_str(), c.angle)).collect();
    assert_eq!(got, [("Troffer", "2x2", 0.0), ("Wall Light", "Sconce", 90.0), ("Wall Light", "Sconce", 0.0)], "{:?}", i.skipped);
    // The two that turn stand on their record boxes; the one on a slope is its record box.
    let ids: std::collections::HashSet<_> =
        i.model.elements.iter().filter(|(_, e)| matches!(e, Element::Component(_))).map(|(id, _)| *id).collect();
    let boxes = bimcraft_core::view3d::Scene::new(&i.model).boxes(&ids);
    let solids: Vec<[f64; 6]> = boxes.values().map(|(lo, hi)| [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]]).collect();
    for (s, (_, _, _, local, axes, origin)) in solids.iter().zip(placed) {
        let r = placed_by(local, axes, origin).map(|f| f * 304.8);
        assert!(s.iter().zip(r).all(|(a, b)| (a - b).abs() < 0.5), "{s:?} against {r:?}");
    }
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    assert!(says("2 lighting fixtures: comes in as a box of its type's box, turned as placed"), "{:?}", i.skipped);
    assert!(says("none of its axes stands upright") && says("ids 302)"), "{:?}", i.skipped);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
}

#[test]
fn snowdon_2025s_components_come_in_as_their_revit_families_and_types_most_turned_as_placed() {
    // Autodesk's Snowdon Towers architectural sample as Revit 2025 installs it: its types have no records of their own, so each is
    // known by the name entry its instances' first lists hold, and its family by the one their second lists hold.
    for path in super::samples_of("Snowdon Towers Sample Architectural.rvt") {
        let i = crate::import_path(&path, "x").unwrap();
        assert!(i.model.check().is_ok(), "{:?}", i.model.check());
        let count = |what: &str| -> usize {
            i.skipped.iter().filter(|s| s.contains(what)).filter_map(|s| s.split(' ').next()?.parse::<usize>().ok()).sum()
        };
        eprintln!(
            "PINS {} {} {} {}",
            components(&i).len(),
            count("turned as placed"),
            count("comes in with its type's shape"),
            count("comes in with its shape, read from the file where it stands")
        );
        assert_eq!(components(&i).len(), 5654, "{}", path.display());
        assert_eq!(count("turned as placed"), 4709, "{}", path.display());
        // Of those, the ones whose own graphics draw a symbol whose solids read and, placed, stand where the record's box does; and
        // those whose graphics draw solids of their own where they stand, or several symbols (a stair's support, a sweep, a mullion).
        // A symbol's solids may be turned ellipses, and its family may nest others (a railing's balusters, a fixture's parts).
        assert_eq!(count("comes in with its type's shape"), 4614, "{}", path.display());
        assert_eq!(count("comes in with its shape, read from the file where it stands"), 795, "{}", path.display());
        // The doors and windows set in walls whose graphics draw a shape that reads, each in a door's or a window's family of its
        // own; the rest (a schematic opening cut, which draws nothing) the project's own types, the size of their boxes.
        assert_eq!(count("in a family of its own"), 149, "{}", path.display());
        // A family of the model for each Revit family the components are of, and one numbered on from the railings' for the shapes
        // it has no room left for, with the types they are of (numbers, not the file's names: Autodesk's strings stay out of the
        // tests).
        let families: std::collections::BTreeSet<&str> = components(&i).iter().map(|c| c.family.as_str()).collect();
        let types: std::collections::BTreeSet<(&str, &str)> =
            components(&i).iter().map(|c| (c.family.as_str(), c.type_name.as_str())).collect();
        // A Revit type is a type for each shape its instances draw (the symbols Revit makes for their instance parameters), and
        // one more for those that come in as boxes beside them.
        assert_eq!((families.len(), types.len()), (137, 1424), "{}", path.display());
        // Every component of a named category is of a Revit type the file names, but those of the system families (stairs' parts,
        // railings, sweeps, slab edges), which are no family instances.
        let imported = components(&i).iter().filter(|c| c.type_name == "Imported" && !c.family.starts_with("Category ")).count();
        assert!(imported < 700, "{imported}");
    }
}

/// Graphics for instance `id`: it draws `symbol` turned by `angle` about up at `origin`, its record box `whole`.
fn draws(p: &mut crate::project_fixture::Project, id: u32, symbol: u32, angle: f64, origin: [f64; 3], whole: [f64; 6]) {
    use crate::project_fixture::{graphics_block, instance_tree, tree_bytes};
    let (s, c) = angle.sin_cos();
    let axes = [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]];
    p.graphics.extend(graphics_block(id, &tree_bytes(&instance_tree(id, symbol, axes, origin, whole))));
}

/// Graphics for symbol `id`: a box `size` feet across from its origin.
fn solid(p: &mut crate::project_fixture::Project, id: u32, size: [f64; 3]) {
    use crate::project_fixture::{box_tree, graphics_block, tree_bytes};
    p.graphics.extend(graphics_block(id, &tree_bytes(&box_tree(id, size, None))));
    p.extra_ids.push(id);
}

#[test]
fn a_component_whose_graphics_draw_a_symbol_comes_in_with_that_shape_turned_as_placed() {
    let mut p = base(2024);
    let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
    desk(&mut p, 650, "Desk 60x30", local);
    solid(&mut p, 650, [2.0, 1.0, 2.5]);
    // Instance 300 draws its type, turned 30 degrees; 301 draws a symbol Revit made for its own parameters, a foot longer; 302's
    // graphics are not in the file.
    let longer = [0.0, 0.0, 0.0, 3.0, 1.0, 2.5];
    solid(&mut p, 651, [3.0, 1.0, 2.5]);
    for (id, drawn, box_, angle, origin) in [
        (300, Some(650), local, 30f64.to_radians(), [10.0, 5.0, 0.0]),
        (301, Some(651), longer, 0.0, [20.0, 5.0, 0.0]),
        (302, None, local, 0.0, [30.0, 5.0, 0.0]),
    ] {
        placed(&mut p, id, 650, box_, angle, origin);
        if let Some(symbol) = drawn {
            draws(&mut p, id, symbol, angle, origin, envelope(box_, angle, origin));
        }
    }
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let by_at = |x: f64| components(&i).into_iter().find(|c| (c.at.0 - x).abs() < 1.0);
    let (Some(a), Some(b), Some(c)) = (by_at(3048.0), by_at(6096.0), by_at(9144.0)) else { panic!("{:?}", i.skipped) };
    assert_eq!([a.type_name.as_str(), b.type_name.as_str(), c.type_name.as_str()], ["Desk 60x30", "Desk 60x30 (2)", "Desk 60x30 (3)"]);
    assert!((a.angle - 30.0).abs() < 1e-3 && a.offset == 0.0, "{} {}", a.angle, a.offset);
    // The family shows its box for a type with no shape, and each type with one its own form.
    let family = &i.model.families["Furniture"];
    let shows = |t: &str| family.types[t].get("Shape").and_then(serde_json::Value::as_u64);
    assert_eq!([shows("Desk 60x30"), shows("Desk 60x30 (2)"), shows("Desk 60x30 (3)")], [Some(1), Some(2), None]);
    assert_eq!(family.forms.len(), 3);
    assert!(family.forms[0].mesh.is_empty() && family.forms[1].mesh.len() == 12 && family.forms[2].mesh.len() == 12);
    let longest = family.forms[2].mesh.iter().flatten().map(|p| p[0]).fold(0.0, f64::max);
    assert!((longest - 914.4).abs() < 0.02, "{longest}");
    // Placed, the shape stands where its record box says.
    let notes = i.skipped.join("\n");
    assert!(notes.contains("2 furniture items: comes in with its type's shape, read from the file and turned as placed"), "{notes}");
    assert!(notes.contains("the file holds no graphics of it"), "{notes}");
}

#[test]
fn a_component_whose_symbol_placed_is_not_its_record_box_comes_in_as_a_box() {
    let mut p = base(2024);
    let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
    desk(&mut p, 650, "Desk", local);
    solid(&mut p, 650, [2.0, 1.0, 2.5]);
    placed(&mut p, 300, 650, local, 0.0, [10.0, 5.0, 0.0]);
    // Its graphics draw the symbol a foot east of where its record box stands.
    draws(&mut p, 300, 650, 0.0, [11.0, 5.0, 0.0], envelope(local, 0.0, [11.0, 5.0, 0.0]));
    let i = import(&p);
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    assert!(!i.model.families["Furniture"].types[&c.type_name].contains_key("Shape"));
    let notes = i.skipped.join("\n");
    assert!(notes.contains("the symbol it draws, placed, is not its record's box"), "{notes}");
}

#[test]
fn a_component_drawn_mirrored_on_its_side_or_on_a_slope_comes_in_with_its_shape_placed_so() {
    let mut p = base(2024);
    let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
    desk(&mut p, 650, "Desk", local);
    solid(&mut p, 650, [2.0, 1.0, 2.5]);
    // Mirrored (its Y flipped), lying on its side (its Z level, facing south) and tilted on a slope.
    let mirrored = [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]];
    let side = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
    let slope = [[0.6, 0.0, 0.8], [0.0, 1.0, 0.0], [-0.8, 0.0, 0.6]];
    let placed = [(300u32, mirrored, [10.0, 5.0, 0.0]), (301, side, [20.0, 5.0, 1.0]), (302, slope, [30.0, 5.0, 0.0])];
    for (id, axes, origin) in placed {
        let whole = placed_by(local, axes, origin);
        p.recs.push(Rec::new(id, FURNITURE).at(whole, &[650, u64::from(id)]));
        p.data.push(crate::project_fixture::Block { id, payload: axes_bytes(axes, origin) });
        use crate::project_fixture::{graphics_block, instance_tree, tree_bytes};
        p.graphics.extend(graphics_block(id, &tree_bytes(&instance_tree(id, 650, axes, origin, whole))));
    }
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let notes = i.skipped.join("\n");
    assert!(notes.contains("3 furniture items: comes in with its type's shape"), "{notes}");
    let by_at = |x: f64| components(&i).into_iter().find(|c| (c.at.0 - x).abs() < 1.0);
    let (Some(a), Some(b), Some(c)) = (by_at(3048.0), by_at(6096.0), by_at(9144.0)) else { panic!("{notes}") };
    assert!(a.mirrored && a.tilt.is_none() && a.angle == 0.0, "{a:?}");
    assert!(!b.mirrored && b.tilt.is_some() && !c.mirrored && c.tilt.is_some(), "{b:?} {c:?}");
    // Each stands, in the model's 3D scene, where its record box says.
    for (c, (_, axes, origin)) in [a, b, c].into_iter().zip(placed) {
        let id = i.model.elements.iter().find(|(_, e)| matches!(e, Element::Component(k) if k == c)).map(|(id, _)| *id);
        let ids: std::collections::HashSet<_> = id.into_iter().collect();
        let boxes = bimcraft_core::view3d::Scene::new(&i.model).boxes(&ids);
        let [(lo, hi)] = boxes.values().copied().collect::<Vec<_>>()[..] else { panic!("{boxes:?}") };
        let want = placed_by(local, axes, origin).map(|f| f * 304.8);
        let got = [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.5), "{got:?} against {want:?}");
    }
}

#[test]
fn a_path_whose_own_graphics_draw_its_body_comes_in_with_that_body_whatever_its_box() {
    // A wall sweep running across the axes: its box is no body, but its graphics draw its solid where it stands (here a box, turned
    // 30 degrees, as Revit writes a sweep's solid in the project's axes).
    let mut p = base(2024);
    let local = [0.0, 0.0, 0.0, 6.0, 0.5, 0.5];
    let (a, origin) = (30f64.to_radians(), [10.0, 5.0, 3.0]);
    let whole = envelope(local, a, origin);
    p.recs.push(Rec::new(300, -2_000_181).at(whole, &[]));
    use crate::project_fixture::{box_tree, graphics_block, root, tree_bytes};
    // The sweep's solid: a box tree whose points are turned and moved where it stands.
    let mut tree = box_tree(300, [6.0, 0.5, 0.5], None);
    let (s, c) = a.sin_cos();
    let place = |x: f64, y: f64, z: f64| [origin[0] + x * c - y * s, origin[1] + x * s + y * c, origin[2] + z];
    for (_, fields) in tree.values_mut() {
        for v in fields.iter_mut() {
            turn(v, &place);
        }
    }
    tree.insert(2, root(300, vec![crate::project_fixture::V::Own(3, "Geometry")], whole, whole));
    p.graphics.extend(graphics_block(300, &tree_bytes(&tree)));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    let family = &i.model.families[&c.family];
    let shows = family.types[&c.type_name].get("Shape").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
    assert_eq!(family.forms[shows].mesh.len(), 12, "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("1 wall sweep: comes in with its shape")), "{:?}", i.skipped);
}

#[test]
fn a_component_drawing_one_symbol_and_marks_past_it_comes_in_with_its_shape_where_its_own_graphics_give_its_box() {
    // A stair run: its graphics place its steps (a symbol a foot high) and draw its walk line's point three feet up, which its record
    // box takes in. The steps, placed, are not its record box, but all it draws is.
    use crate::project_fixture::{V, ginfo, graphics_block, instance_tree, root, tree_bytes};
    let mut p = base(2024);
    let whole = [10.0, 5.0, 0.0, 11.0, 7.0, 3.0];
    p.recs.push(Rec::new(300, -2_000_919).at(whole, &[]));
    solid(&mut p, 650, [1.0, 2.0, 1.0]);
    let square = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut tree = instance_tree(300, 650, square, [10.0, 5.0, 0.0], whole);
    let up = [V::F64(10.5), V::F64(6.0), V::F64(3.0)];
    tree.insert(1000, ("GPoint", vec![ginfo(0), V::Seq(up.to_vec()), V::U32(6), V::U32(2), V::U32(0)]));
    tree.insert(2, root(300, vec![V::Own(3, "GInstance"), V::Own(1000, "GPoint")], whole, whole));
    p.graphics.extend(graphics_block(300, &tree_bytes(&tree)));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    let family = &i.model.families[&c.family];
    let shows = family.types[&c.type_name].get("Shape").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
    let mesh = &family.forms[shows].mesh;
    assert_eq!(mesh.len(), 12, "{:?}", i.skipped);
    let top = mesh.iter().flatten().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((top - 304.8).abs() < 0.01, "{top}");
    assert!(i.skipped.iter().any(|s| s.contains("1 stair run: comes in with its shape")), "{:?}", i.skipped);
}

#[test]
fn a_component_drawing_its_own_solid_and_a_symbol_with_none_comes_in_with_its_solid() {
    // A stair run whose graphics hold its steps' solid where it stands, and place a symbol that draws no solid at all.
    use crate::project_fixture::{V, box_tree, graphics_block, instance_tree, root, tree_bytes};
    let mut p = base(2024);
    let whole = [0.0, 0.0, 0.0, 1.0, 2.0, 1.0];
    p.recs.push(Rec::new(300, -2_000_919).at(whole, &[]));
    p.graphics.extend(graphics_block(651, &tree_bytes(&[(2, root(651, vec![], whole, whole))].into())));
    p.extra_ids.push(651);
    let square = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut tree = box_tree(300, [1.0, 2.0, 1.0], None);
    let instance = instance_tree(300, 651, square, [0.0; 3], whole).remove(&3).unwrap_or_else(|| panic!("no instance"));
    tree.insert(5000, instance);
    tree.insert(2, root(300, vec![V::Own(3, "Geometry"), V::Own(5000, "GInstance")], whole, whole));
    p.graphics.extend(graphics_block(300, &tree_bytes(&tree)));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    let family = &i.model.families[&c.family];
    let shows = family.types[&c.type_name].get("Shape").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
    assert_eq!(family.forms[shows].mesh.len(), 12, "{:?}", i.skipped);
    assert!(i.skipped.iter().any(|s| s.contains("1 stair run: comes in with its shape")), "{:?}", i.skipped);
}

#[test]
fn a_shape_its_family_has_no_room_for_comes_in_in_a_family_of_the_same_name_numbered_on() {
    // Railings that turn (their boxes as wide as long), each drawing a shape of its own, more than one family holds forms for.
    use crate::project_fixture::{box_tree, graphics_block, tree_bytes};
    let mut p = base(2024);
    for k in 0..260u32 {
        let size = [2.0 + f64::from(k) * 0.001, 2.0, 3.0];
        p.recs.push(Rec::new(300 + k, -2_000_126).at([0.0, 0.0, 0.0, size[0], size[1], size[2]], &[]));
        p.graphics.extend(graphics_block(300 + k, &tree_bytes(&box_tree(300 + k, size, None))));
    }
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    assert_eq!(components(&i).len(), 260, "{:?}", i.skipped);
    let shown = |c: &&Component| {
        i.model.families[&c.family].types[&c.type_name].get("Shape").and_then(serde_json::Value::as_u64).is_some_and(|k| k > 0)
    };
    assert!(components(&i).iter().all(shown), "{:?}", i.skipped);
    let families: std::collections::BTreeSet<&str> = components(&i).iter().map(|c| c.family.as_str()).collect();
    assert_eq!(families, ["Railings", "Railings (2)"].into());
    // No family holds a type none of them is of.
    let types: usize = families.iter().map(|f| i.model.families[*f].types.len()).sum();
    assert_eq!(types, 260);
}

#[test]
fn the_families_numbered_on_take_the_shapes_one_has_no_room_for_until_they_too_are_full() {
    // Shapes of a hundred thousand triangles: three to a family, then the next numbered on, until the last is full.
    let mut model = std::collections::BTreeMap::new();
    let mut families = crate::generic::Families::new(["Railings".to_string(), "Railings (3)".to_string()].into());
    let one = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let solid = crate::shapes::Shape {
        triangles: vec![one; bimcraft_core::family::MAX_TRIANGLES],
        materials: vec![None; bimcraft_core::family::MAX_TRIANGLES],
        ..crate::shapes::Shape::default()
    };
    let names = std::collections::BTreeMap::new();
    let mut got = Vec::new();
    for k in 0..3 * (crate::generic::MAX_OVERFLOW + 1) + 2 {
        got.push(crate::generic::room(&mut model, &mut families, ("Railings", Category::Generic), &format!("T{k}"), &solid, &names));
    }
    let named = |n: &str| Some(n.to_string());
    // A name another family has ("Railings (3)", a Revit family's) is passed over.
    assert_eq!(
        got[..9],
        [
            named("Railings"),
            named("Railings"),
            named("Railings"),
            named("Railings (2)"),
            named("Railings (2)"),
            named("Railings (2)"),
            named("Railings (4)"),
            named("Railings (4)"),
            named("Railings (4)")
        ]
    );
    assert!(got[..3 * (crate::generic::MAX_OVERFLOW + 1)].iter().all(Option::is_some), "{got:?}");
    assert_eq!(got[3 * (crate::generic::MAX_OVERFLOW + 1)..], [None, None]);
    // A family that refused a shape keeps no type for it.
    let types: usize = model.values().map(|f| f.types.len()).sum();
    assert_eq!(types, 3 * (crate::generic::MAX_OVERFLOW + 1));
    assert_eq!(model.len(), crate::generic::MAX_OVERFLOW + 1);
}

#[test]
fn a_ramp_comes_in_as_a_component_of_the_family_ramps() {
    let mut p = base(2024);
    p.recs.push(Rec::new(300, -2_000_180).at([0.0, 0.0, 0.0, 3.0, 12.0, 1.0], &[]));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!(c.family, "Ramps");
    assert!(i.skipped.iter().any(|s| s.starts_with("1 ramp: comes in as a box")), "{:?}", i.skipped);
}

/// Moves the planes of a box tree's faces by `place` (their origins, and their directions turned by it about its origin).
fn turn(v: &mut crate::project_fixture::V, place: &dyn Fn(f64, f64, f64) -> [f64; 3]) {
    use crate::project_fixture::V;
    match v {
        V::Inline("Plane", fields) => {
            let p = |v: &V| match v {
                V::Seq(xs) => match xs[..] {
                    [V::F64(x), V::F64(y), V::F64(z)] => Some([x, y, z]),
                    _ => None,
                },
                _ => None,
            };
            let (Some(o), Some(x), Some(y)) = (p(&fields[1]), p(&fields[2]), p(&fields[3])) else { return };
            let zero = place(0.0, 0.0, 0.0);
            let dir = |d: [f64; 3]| {
                let q = place(d[0], d[1], d[2]);
                [q[0] - zero[0], q[1] - zero[1], q[2] - zero[2]]
            };
            fields[1] = crate::project_fixture::p3(place(o[0], o[1], o[2]));
            fields[2] = crate::project_fixture::p3(dir(x));
            fields[3] = crate::project_fixture::p3(dir(y));
        }
        V::Seq(vs) | V::Counted(vs) => vs.iter_mut().for_each(|v| turn(v, place)),
        _ => {}
    }
}

#[test]
fn a_shapes_faces_come_in_in_the_materials_they_are_drawn_in() {
    // A desk whose top is oak (its face names the material) and the rest by its category (no material named).
    let mut p = base(2024);
    p.mats.push(crate::project_fixture::Mat { id: 720, name: "Oak".into(), look: Some(([104, 79, 49], 0.0)) });
    let mut oak = crate::project_fixture::TypeRec::new(720, -2_000_700);
    oak.kind = crate::project_fixture::KIND_MATERIAL;
    p.types.push(oak);
    let local = [0.0, 0.0, 0.0, 2.0, 1.0, 2.5];
    desk(&mut p, 650, "Desk", local);
    use crate::project_fixture::{box_tree, element_id, graphics_block, tree_bytes};
    let mut tree = box_tree(650, [2.0, 1.0, 2.5], None);
    let top = tree.values_mut().filter(|(class, _)| *class == "Face").nth(1).map(|(_, fields)| fields);
    if let Some(fields) = top {
        fields[3] = element_id(720);
    }
    p.graphics.extend(graphics_block(650, &tree_bytes(&tree)));
    p.extra_ids.push(650);
    placed(&mut p, 300, 650, local, 0.0, [10.0, 5.0, 0.0]);
    draws(&mut p, 300, 650, 0.0, [10.0, 5.0, 0.0], envelope(local, 0.0, [10.0, 5.0, 0.0]));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [c] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    let family = &i.model.families[&c.family];
    let k = family.types[&c.type_name].get("Shape").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let shown: Vec<(&str, usize)> = family
        .forms
        .iter()
        .filter(|f| f.visible == Some(bimcraft_core::family::Expr::Formula(format!("Shape = {k}"))))
        .map(|f| (f.material.as_str(), f.mesh.len()))
        .collect();
    assert_eq!(shown, [("", 10), ("Oak", 2)], "{:?}", i.skipped);
    assert_eq!(i.model.materials.get("Oak").map(|m| m.color), Some([104, 79, 49]));
}

#[test]
fn a_family_whose_types_would_build_more_than_one_family_may_keeps_the_box_for_the_type_past_it() {
    // Types of a hundred thousand triangles each: the family takes them while its types build no more than the core allows.
    let mut family = bimcraft_core::family::Family::default();
    let one = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let solid = crate::shapes::Shape {
        triangles: vec![one; bimcraft_core::family::MAX_TRIANGLES],
        materials: vec![None; bimcraft_core::family::MAX_TRIANGLES],
        ..crate::shapes::Shape::default()
    };
    let shown: Vec<bool> =
        (0..5).map(|k| crate::generic::show(&mut family, &format!("T{k}"), &solid, &std::collections::BTreeMap::new())).collect();
    assert_eq!(shown, [true, true, true, false, false]);
    let built: usize = family.forms.iter().map(|f| f.mesh.len()).sum();
    assert!(built + 12 * family.types.len() <= bimcraft_core::family::MAX_BUILT, "{built}");
}
