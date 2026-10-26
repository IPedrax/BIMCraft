//! Annotate > Region: filled and masking regions sketched in the plan, their type and boundary in the Options Bar and
//! Properties; Manage > Fill Patterns: the region types and fill patterns, edited as a draft and applied as one step.

use super::*;
use bimcraft_core::fills::{HatchLine, Target};

fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    press(h, "Rectangle");
    click(h, a);
    click(h, b);
}

#[test]
fn a_filled_region_is_sketched_in_the_options_bars_type_and_line_style_any_number_of_loops() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    assert!(h.query_by_label("Modify | Create Filled Region Boundary").is_some());
    choose(&mut h, "Region Type", "Crosshatch");
    choose(&mut h, "Line Style", "<Invisible lines>");
    rectangle(&mut h, P2(1000.0, 1000.0), P2(3000.0, 3000.0));
    rectangle(&mut h, P2(5000.0, 1000.0), P2(7000.0, 2000.0));
    press(&mut h, "Finish Edit Mode");
    let regions = ids(&h, "region");
    assert_eq!(regions.len(), 1, "{}", h.state().status.text);
    let r = regions[0];
    assert_eq!((field(&h, r, "type"), field(&h, r, "line_style")), (json!("Crosshatch"), json!(null)));
    assert_eq!(field(&h, r, "area_m2"), json!(6.0), "two loops, one region");
    assert_eq!(h.state().selection, vec![r]);
    // Its type only in the Type Selector, its lines in Properties; its sketch with Edit Boundary.
    press(&mut h, "Modify | Filled Regions");
    assert!(h.query_by_role_and_label(Role::ComboBox, "Region Type").is_none(), "type is only in the Type Selector");
    press(&mut h, "Type Selector");
    press(&mut h, "Sand");
    choose(&mut h, "Line Style", "Wide Lines");
    assert_eq!((field(&h, r, "type"), field(&h, r, "line_style")), (json!("Sand"), json!("Wide Lines")));
    press(&mut h, "Edit Boundary");
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(8));
    press(&mut h, "Cancel Edit Mode");
}

#[test]
fn a_masking_region_starts_in_masking_with_its_lines_unseen_and_fr_starts_a_filled_one() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Masking Region");
    assert!(h.query_by_label("Modify | Create Masking Region Boundary").is_some());
    rectangle(&mut h, P2(-500.0, 2000.0), P2(1500.0, 4000.0));
    press(&mut h, "Finish Edit Mode");
    let r = ids(&h, "region")[0];
    assert_eq!((field(&h, r, "type"), field(&h, r, "line_style")), (json!("Masking"), json!(null)));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "FR");
    assert!(h.query_by_label("Modify | Create Filled Region Boundary").is_some());
}

#[test]
fn fill_patterns_and_region_types_are_made_and_changed_in_one_step() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Fill Patterns");
    assert!(h.state().fill_patterns_dialog.is_some());
    press(&mut h, "Patterns");
    // A simple pattern of its own: 30°, 3 mm, crosshatched.
    press(&mut h, "New Pattern");
    type_into(&mut h, "Pattern 1: Name", "Lattice");
    type_into(&mut h, "Lattice: Angle", "30");
    type_into(&mut h, "Lattice: Spacing", "3");
    press(&mut h, "Lattice: Crosshatch");
    // A type filling with it, in red.
    press(&mut h, "Region Types");
    press(&mut h, "New Type");
    type_into(&mut h, "Region Type 1: Name", "Lattice Red");
    choose(&mut h, "Lattice Red: Foreground", "Lattice");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!(m.fill_patterns["Lattice"].target, Target::Drafting);
    assert_eq!(m.fill_patterns["Lattice"].lines, vec![HatchLine::simple(30.0, 3.0), HatchLine::simple(120.0, 3.0)]);
    assert_eq!(m.region_types["Lattice Red"].foreground.as_deref(), Some("Lattice"));
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(h.state().session.history().last(), Some(&"Fill Patterns"));
    // Renamed, the type follows; a type a region is of is refused deleted, saying why, the dialog staying open.
    h.state_mut()
        .session
        .execute(
            "filled_region",
            &json!({"sketch": [[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]], "type": "Lattice Red"}),
        )
        .unwrap();
    press(&mut h, "Fill Patterns");
    press(&mut h, "Patterns");
    type_into(&mut h, "Lattice: Name", "Trellis");
    press(&mut h, "Region Types");
    press(&mut h, "Lattice Red: Delete");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some() && h.state().status.error, "{}", h.state().status.text);
    press(&mut h, "Cancel");
    press(&mut h, "Fill Patterns");
    press(&mut h, "Patterns");
    type_into(&mut h, "Lattice: Name", "Trellis");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.region_types["Lattice Red"].foreground.as_deref(), Some("Trellis"));
}

#[test]
fn patterns_come_in_from_a_pat_file_go_out_to_one_and_any_patterns_lines_are_edited_family_by_family() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-pat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("in.pat");
    std::fs::write(
        &file,
        ";%UNITS=MM\n*Herringbone\n;%TYPE=MODEL\n45, 0,0, 0,100, 200,-200\n135, 0,0, 0,100, 200,-200\n*Flat\n0,0,0,0,0\n",
    )
    .unwrap();
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Fill Patterns");
    press(&mut h, "Patterns");
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(h.state().status.text.contains("Flat"), "what was left out is said: {}", h.state().status.text);
    // Its families, edited: the second's dashes typed anew, a third added.
    press(&mut h, "Herringbone: Edit Lines");
    type_into(&mut h, "Herringbone family 2: Dashes", "300, -100");
    press(&mut h, "Add Family");
    type_into(&mut h, "Herringbone family 3: Spacing", "50");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let p = &m.fill_patterns["Herringbone"];
    assert_eq!(p.target, Target::Model);
    assert_eq!(p.lines.len(), 3);
    assert_eq!(p.lines[1].dashes, [300.0, -100.0]);
    assert_eq!(p.lines[2].spacing, 50.0);
    // And out to a file, read back the same.
    let out = dir.join("out");
    let drafted: Vec<(String, bimcraft_core::fills::FillPattern)> = m.fill_patterns.iter().map(|(n, p)| (n.clone(), p.clone())).collect();
    h.state_mut().export_patterns(&out, &drafted);
    let back = bimcraft_core::pat::read_file(&dir.join("out.pat")).unwrap();
    assert!(back.patterns.iter().any(|(n, q)| n == "Herringbone" && q == &h.state().session.model.fill_patterns["Herringbone"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn align_moves_a_regions_model_pattern_onto_the_line_picked_first() {
    let square = r#"[[[1000,1000],[4000,1000]],[[4000,1000],[4000,4000]],[[4000,4000],[1000,4000]],[[1000,4000],[1000,1000]]]"#;
    let mut h = app(&format!(
        "{BOX}\nfilled_region_type {{\"name\": \"Tiles\", \"foreground\": \"Tile 300\"}}\nfilled_region {{\"sketch\": {square}, \"type\": \"Tiles\", \"line_style\": null}}\ndetail_line {{\"start\": [500, 1000], \"end\": [500, 5000]}}"
    ));
    let region = ids(&h, "region")[0];
    shortcut(&mut h, "AL");
    click(&mut h, P2(500.0, 3000.0));
    // A tile line, 2100 mm east: the pattern moves 1600 mm west.
    click(&mut h, P2(2100.0, 2500.0));
    assert_eq!(field(&h, region, "pattern_origin"), json!([-1600.0, 0.0]));
    assert_eq!(h.state().tool, Tool::Align(None));
}

#[test]
fn edit_type_during_a_region_sketch_creates_a_type_with_independent_fill_layers() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    press(&mut h, "Edit Type");
    assert!(h.query_by_label("Type Properties").is_some());
    press(&mut h, "Duplicate…");
    type_into(&mut h, "Type Name", "Earth on gray");
    choose(&mut h, "Foreground Pattern", "Diagonal Up");
    choose(&mut h, "Background Pattern", "Solid fill");
    press(&mut h, "Masking");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    let ty = &h.state().session.model.region_types["Earth on gray"];
    assert_eq!(ty.foreground.as_deref(), Some("Diagonal Up"));
    assert_eq!(ty.background.as_deref(), Some("Solid fill"));
    assert!(ty.masking);
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(h.state().sketch.as_ref().unwrap().region.0, "Earth on gray");
    rectangle(&mut h, P2(1000.0, 1000.0), P2(3000.0, 3000.0));
    press(&mut h, "Finish Edit Mode");
    assert_eq!(field(&h, ids(&h, "region")[0], "type"), json!("Earth on gray"));
}

#[test]
fn region_edit_type_changes_all_instances_renames_and_cancels_a_duplicate() {
    let mut h = app(&format!(
        r#"{BOX}
filled_region {{"sketch": [[[1000,1000],[3000,1000]],[[3000,1000],[3000,3000]],[[3000,3000],[1000,3000]],[[1000,3000],[1000,1000]]]}}
filled_region {{"sketch": [[[4000,1000],[5000,1000]],[[5000,1000],[5000,2000]],[[5000,2000],[4000,2000]],[[4000,2000],[4000,1000]]]}}"#
    ));
    let regions = ids(&h, "region");
    h.state_mut().selection = vec![regions[0]];
    h.run();
    press(&mut h, "Edit Type");
    type_into(&mut h, "Type Name", "Shared fill");
    choose(&mut h, "Foreground Pattern", "<None>");
    choose(&mut h, "Background Pattern", "Crosshatch");
    h.get_all_by_label("Apply").max_by(|a, b| a.rect().left().total_cmp(&b.rect().left())).unwrap().click_accesskit();
    h.run();
    assert!(h.query_by_label("Type Properties").is_some());
    for r in &regions {
        assert_eq!(field(&h, *r, "type"), json!("Shared fill"));
    }
    assert_eq!(h.state().session.model.region_types["Shared fill"].background.as_deref(), Some("Crosshatch"));
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "Duplicate…");
    type_into(&mut h, "Type Name", "Discard me");
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    key(&mut h, Key::Escape);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(!h.state().session.model.region_types.contains_key("Shared fill"));
}

#[test]
fn region_sketch_snaps_to_detail_lines_and_existing_region_boundaries() {
    use bimcraft_core::snap::Kind;
    let mut h = app(&format!(
        r#"{BOX}
detail_line {{"points": [[1000,1000],[5000,1000]]}}
filled_region {{"sketch": [[[2000,3000],[6000,3000]],[[6000,3000],[6000,5000]],[[6000,5000],[2000,5000]],[[2000,5000],[2000,3000]]]}}"#
    ));
    shortcut(&mut h, "FR");
    let f = hover(&mut h, P2(1030.0, 1020.0)).unwrap();
    assert_eq!((f.kind, f.at), (Kind::Endpoint, P2(1000.0, 1000.0)));
    click(&mut h, P2(1030.0, 1020.0));
    let f = hover(&mut h, P2(4020.0, 3030.0)).unwrap();
    assert_eq!((f.kind, f.at), (Kind::Midpoint, P2(4000.0, 3000.0)));
    click(&mut h, P2(4020.0, 3030.0));
    let line = &h.state().sketch.as_ref().unwrap().lines[0];
    assert_eq!((line.start(), line.end()), (P2(1000.0, 1000.0), P2(4000.0, 3000.0)));
}

#[test]
fn new_region_types_reject_duplicate_names_without_overwriting_existing_types() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    press(&mut h, "Edit Type");
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "New Type");
    type_into(&mut h, "Type Name", "Crosshatch");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some());
    assert!(h.state().status.text.contains("already in use"));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    type_into(&mut h, "Type Name", "My new region");
    choose(&mut h, "Foreground Pattern", "Sand");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    assert_eq!(h.state().session.model.region_types["My new region"].foreground.as_deref(), Some("Sand"));
}

#[test]
fn edit_type_keeps_annotation_boundary_styles_and_pattern_placement_and_imports_pat_patterns() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    choose(&mut h, "Line Style", "Wide Lines");
    rectangle(&mut h, P2(1000.0, 1000.0), P2(4000.0, 3000.0));
    key(&mut h, Key::Escape);
    click(&mut h, P2(2000.0, 1000.0));
    choose(&mut h, "Line Style", "<Invisible lines>");
    press(&mut h, "Finish Edit Mode");
    let id = ids(&h, "region")[0];
    h.state_mut().session.execute("move", &json!({"ids": [id], "by": [150, 75]})).unwrap();
    h.state_mut().session.execute("rotate", &json!({"ids": [id], "about": [0, 0], "angle": 30})).unwrap();
    h.run();
    let Some(Element::FilledRegion(before)) = h.state().session.model.elements.get(id).cloned() else { panic!("region") };
    assert!(before.sketch.iter().any(|l| l.invisible));
    assert!(before.sketch.iter().any(|l| l.style.as_deref() == Some("Wide Lines")));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "Type Name", "Annotation Hatch");
    press(&mut h, "Patterns");
    let dir = tempfile::tempdir().unwrap();
    let pat = dir.path().join("annotation.pat");
    std::fs::write(&pat, ";%UNITS=MM\n*Annotation Pattern\n;%TYPE=MODEL\n45,0,0,0,100\n").unwrap();
    h.state_mut().import_patterns(&pat);
    h.run();
    press(&mut h, "Annotation Pattern: Edit Lines");
    press(&mut h, "Add Family");
    type_into(&mut h, "Annotation Pattern family 2: Spacing", "200");
    press(&mut h, "Region Types");
    choose(&mut h, "Foreground Pattern", "Annotation Pattern");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let Some(Element::FilledRegion(after)) = h.state().session.model.elements.get(id) else { panic!("region") };
    assert_eq!(after.type_name, "Annotation Hatch");
    assert_eq!(
        (&after.sketch, after.pattern_origin, after.pattern_angle, &after.line_style),
        (&before.sketch, before.pattern_origin, before.pattern_angle, &before.line_style)
    );
    let p = &h.state().session.model.fill_patterns["Annotation Pattern"];
    assert_eq!((p.target, p.lines.len()), (Target::Model, 2));
    assert_eq!(p.lines[1].spacing, 200.0);
}
