//! MEP systems: a run, a fitting or an accessory drawn in the colour its system's type gives it.

use super::walls::{base, import};
use crate::project_fixture::{Mat, Objects, Project, Rec, V, box_tree, element_id, graphics_block, object_block, text, tree_bytes};
use bimcraft_core::family::Kind;
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Component, Element, Run};

const DUCTS: i64 = -2_008_000;
const DUCT_FITTINGS: i64 = -2_008_010;
const PIPE_ACCESSORIES: i64 = -2_008_055;
/// A colour a graphic override leaves as it is: the 25th bit set.
const NO_COLOUR: u32 = 1 << 24;

/// A system type `id` named `name`, of class `class`, naming material `material` (none: `u64::MAX`), whose graphic overrides give
/// lines colour `colour` (`0xBBGGRR`; none: no colour overrider at all).
fn system_type(id: u32, class: &'static str, name: &str, material: u64, colour: Option<u32>) -> Vec<u8> {
    let pen = |c: Option<u32>| c.map_or(V::Null, |c| V::Inline("GStyleColorOverrider", vec![V::U32(c)]));
    let settings = V::Inline("OverrideGraphicSettings", vec![V::Byte(0), pen(colour), pen(Some(NO_COLOUR))]);
    let overrides = V::Inline("SystemOverrides", vec![settings, V::Byte(1)]);
    // Its own class's field: an air system's rise and drop settings, a piping system's fluid temperature.
    let last = if class == "RbsPipingSystemType" { V::F64(288.7) } else { V::U32(2) };
    let own = vec![element_id(id.into()), V::Inline("SymbolInfo", vec![text(name)]), element_id(material), text(""), overrides, last];
    object_block(id, class, &Objects::from([(2, (class, own))]))
}

/// A system `id` named `name` of type `type_id`.
fn system(id: u32, class: &'static str, name: &str, type_id: u32) -> Vec<u8> {
    let own = vec![element_id(id.into()), text(name), element_id(type_id.into())];
    object_block(id, class, &Objects::from([(2, (class, own))]))
}

/// An analytical cell naming system `id`.
fn cell(id: u32) -> V {
    V::Inline("MEPAnalyticalModelCell", vec![element_id(u64::MAX), element_id(id.into())])
}

/// An extensible-storage cell: one entity, its blob the inline marker and the GUID of the storage schema it follows.
fn storage() -> V {
    let guid = || V::Seq((0..16).map(|k| V::Byte(0x10 + k)).collect());
    let blob = V::Seq(vec![V::U32(u32::MAX), guid()]);
    V::Inline("ESEntityCell", vec![V::Counted(vec![V::Seq(vec![V::Seq(vec![guid()]), V::Seq(vec![blob])])])])
}

/// A cell that does not read: it owns an object of a class the schema does not have.
fn broken() -> V {
    V::Inline("OverrideGraphicSettings", vec![V::Byte(0), V::Seq(vec![V::U32(u32::MAX), V::U16(9_999)]), V::Null])
}

/// A connector naming system `id`.
fn connector(id: u32) -> V {
    V::Inline("MEPFamilyConnectorCalculationHvac", vec![V::F64(0.5), element_id(id.into())])
}

fn instance(id: u32, cells: Vec<V>, connectors: Vec<V>) -> Vec<u8> {
    let own = vec![element_id(id.into()), V::Counted(cells), V::Counted(connectors)];
    object_block(id, "FamilyInstance", &Objects::from([(2, ("FamilyInstance", own))]))
}

fn components(i: &Imported) -> Vec<&Component> {
    i.model.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect()
}

fn ducts(i: &Imported) -> Vec<&Run> {
    i.model.elements.values().filter(|e| e.category() == "duct").filter_map(Element::run).collect()
}

/// Supply Air (blue lines), Return Air (magenta) and Hydronic Supply (no colour), a system of each; a duct and a duct fitting in
/// the supply system, a valve in the hydronic one, and a fitting between the supply and the return systems.
fn project() -> Project {
    let mut p = base(2024);
    p.graphics.extend(system_type(500, "RbsHvacSystemType", "Supply Air", u64::MAX, Some(0xff_0000)));
    p.graphics.extend(system_type(501, "RbsHvacSystemType", "Return Air", u64::MAX, Some(0xff_00ff)));
    p.graphics.extend(system_type(502, "RbsPipingSystemType", "Hydronic Supply", u64::MAX, Some(NO_COLOUR)));
    p.graphics.extend(system(600, "RbsHvacSystem", "Mechanical Supply Air 1", 500));
    p.graphics.extend(system(601, "RbsHvacSystem", "Mechanical Return Air 1", 501));
    p.graphics.extend(system(602, "RbsPipingSystem", "Hydronic Supply 1", 502));
    // The duct draws a box 10 ft along x, a foot wide and half a foot deep.
    p.recs.push(Rec::new(300, DUCTS).at([0.0, 0.0, 0.0, 10.0, 1.0, 0.5], &[]));
    p.graphics.extend(graphics_block(300, &tree_bytes(&box_tree(300, [10.0, 1.0, 0.5], None))));
    let duct = vec![element_id(300), V::Counted(vec![cell(600)]), V::F64(0.5)];
    p.graphics.extend(object_block(300, "RbsDuctCurve", &Objects::from([(2, ("RbsDuctCurve", duct))])));
    // The fitting holds an extensible-storage entity before the cell that names its system, and a cell that does not read after.
    p.recs.push(Rec::new(301, DUCT_FITTINGS).at([10.0, 0.0, 0.0, 11.0, 1.0, 1.0], &[]));
    p.graphics.extend(instance(301, vec![storage(), cell(600), broken()], vec![connector(600)]));
    p.recs.push(Rec::new(302, PIPE_ACCESSORIES).at([20.0, 0.0, 0.0, 21.0, 1.0, 1.0], &[]));
    p.graphics.extend(instance(302, vec![cell(602)], Vec::new()));
    p.recs.push(Rec::new(303, DUCT_FITTINGS).at([30.0, 0.0, 0.0, 31.0, 1.0, 1.0], &[]));
    p.graphics.extend(instance(303, Vec::new(), vec![connector(600), connector(601)]));
    p
}

#[test]
fn a_duct_and_its_fitting_take_the_colour_their_system_types_overrides_give_lines_and_only_theirs() {
    let i = import(&project());
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    // The duct is in the supply system, and its shape is drawn in Supply Air, Revit's blue.
    let [duct] = ducts(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!(duct.system, "Mechanical Supply Air 1");
    let parts = &duct.imported.as_ref().expect("its shape").parts;
    assert!(!parts.is_empty() && parts.iter().all(|p| p.material == "Supply Air"), "{parts:?}");
    assert_eq!(i.model.materials["Supply Air"].color, [0, 0, 255]);
    // The fitting takes it through the family's material parameter, which the forms with no material of their own name.
    // Each component by where it stands: the fitting at 10 ft along, the valve at 20, the fitting between systems at 30.
    let by_id = |n: u32| {
        let x = f64::from(n - 300) * 10.0 * 304.8;
        components(&i).into_iter().find(|c| c.at.0 >= x - 1.0 && c.at.0 <= x + 304.8 + 1.0).cloned().expect("made")
    };
    let fitting = by_id(301);
    assert_eq!(fitting.params.get("System Material").and_then(|v| v.as_str()), Some("Supply Air"), "{:?}", i.skipped);
    let family = &i.model.families[&fitting.family];
    let p = family.parameters.iter().find(|p| p.name == "System Material").expect("the parameter");
    assert!(p.kind == Kind::Material && p.instance, "{p:?}");
    assert!(
        family.forms.iter().all(|f| f.material == "System Material"),
        "{:?}",
        family.forms.iter().map(|f| &f.material).collect::<Vec<_>>()
    );
    // A system whose type gives no colour, and a fitting in systems of two types, are drawn as their family draws them.
    assert!(!by_id(302).params.contains_key("System Material") && !by_id(303).params.contains_key("System Material"));
    assert!(!i.model.materials.contains_key("Hydronic Supply") && !i.model.materials.contains_key("Return Air"));
    assert!(says("1 duct: it is drawn in the colour its system type's graphic overrides give its lines"), "{:?}", i.skipped);
    assert!(says("1 duct fitting: it is drawn in the colour its system type's graphic overrides give its lines"), "{:?}", i.skipped);
    assert!(says("1 pipe accessory: its system type gives no colour"), "{:?}", i.skipped);
    assert!(says("1 duct fitting: it joins systems of 2 types, so it takes the colour of none"), "{:?}", i.skipped);
    assert_eq!(components(&i).len(), 3);
}

#[test]
fn a_system_type_that_names_a_material_draws_its_elements_in_that_material() {
    let mut p = base(2024);
    p.mats.push(Mat { id: 720, name: "Galvanised".to_string(), look: Some(([200, 200, 210], 0.0)) });
    p.graphics.extend(system_type(500, "RbsHvacSystemType", "Supply Air", 720, Some(0xff_0000)));
    p.graphics.extend(system(600, "RbsHvacSystem", "Mechanical Supply Air 1", 500));
    p.recs.push(Rec::new(301, DUCT_FITTINGS).at([10.0, 0.0, 0.0, 11.0, 1.0, 1.0], &[]));
    p.graphics.extend(instance(301, vec![cell(600)], Vec::new()));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let [fitting] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    assert_eq!(fitting.params.get("System Material").and_then(|v| v.as_str()), Some("Galvanised"));
    assert_eq!(i.model.materials["Galvanised"].color, [200, 200, 210]);
    assert!(!i.model.materials.contains_key("Supply Air"));
}

#[test]
fn an_element_whose_system_is_not_read_says_so_and_keeps_its_familys_colour() {
    let mut p = base(2024);
    p.recs.push(Rec::new(301, DUCT_FITTINGS).at([10.0, 0.0, 0.0, 11.0, 1.0, 1.0], &[]));
    let i = import(&p);
    let [fitting] = components(&i)[..] else { panic!("{:?}", i.skipped) };
    assert!(!fitting.params.contains_key("System Material"));
    assert!(i.skipped.iter().any(|s| s.contains("1 duct fitting: its system is not read")), "{:?}", i.skipped);
}

#[test]
fn a_mutation_sweep_over_a_project_of_systems_never_panics_and_what_comes_in_checks_out() {
    let mut p = project();
    p.members = 3;
    super::sweep(&p, 0x5157_e3a5_c010_0125, 800, |i, round| {
        assert!(components(i).len() <= 3, "round {round}");
    });
}
