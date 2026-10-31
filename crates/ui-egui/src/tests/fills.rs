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
    // The type is the Type Selector's alone; the Options Bar has the line style.
    assert!(h.query_by_role_and_label(Role::ComboBox, "Region Type").is_none());
    press(&mut h, "Type Selector");
    press(&mut h, "Crosshatch");
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
fn fill_patterns_are_made_and_changed_in_one_step_and_the_region_types_that_fill_with_them_follow() {
    let mut h = app(BOX);
    annotation(&mut h, "Fill Patterns");
    assert!(h.state().fill_patterns_dialog.is_some());
    // A simple pattern of its own: 30°, 3 mm, crosshatched.
    press(&mut h, "New Pattern");
    press(&mut h, "Simple Lines");
    type_into(&mut h, "Pattern 1: Name", "Lattice");
    // Its lines in Edit Lines: 30° and 120°, 3 mm apart; no columns of a simple pattern in the list.
    assert!(h.query_by_label("Lattice: Angle").is_none() && h.query_by_label("Lattice: Crosshatch").is_none());
    press(&mut h, "Lattice: Edit Lines");
    assert!(h.query_by_label("Lattice line 1: Shift").is_none() && h.query_by_label("Lattice line 1: Dashes").is_none(), "whole lines");
    type_into(&mut h, "Lattice line 1: Angle", "30");
    type_into(&mut h, "Lattice line 1: Spacing", "3");
    press(&mut h, "Add Line");
    type_into(&mut h, "Lattice line 2: Angle", "120");
    type_into(&mut h, "Lattice line 2: Spacing", "3");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!(m.fill_patterns["Lattice"].target, Target::Drafting);
    assert_eq!(m.fill_patterns["Lattice"].lines, vec![HatchLine::simple(30.0, 3.0), HatchLine::simple(120.0, 3.0)]);
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(h.state().session.history().last(), Some(&"Fill Patterns"));
    // A type filling with it follows its new name.
    h.state_mut().session.execute("filled_region_type", &json!({"name": "Lattice Red", "foreground": "Lattice"})).unwrap();
    h.run();
    annotation(&mut h, "Fill Patterns");
    type_into(&mut h, "Lattice: Name", "Trellis");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.region_types["Lattice Red"].foreground.as_deref(), Some("Trellis"));
}

#[test]
fn patterns_come_in_from_a_pat_file_go_out_to_one_and_a_custom_one_is_kept_as_it_came() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-pat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("in.pat");
    std::fs::write(
        &file,
        ";%UNITS=MM\n*Herringbone\n;%TYPE=MODEL\n45, 0,0, 0,100, 200,-200\n135, 0,0, 0,100, 200,-200\n*Flat\n0,0,0,0,0\n",
    )
    .unwrap();
    let mut h = app(BOX);
    annotation(&mut h, "Fill Patterns");
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(h.query_by_label_contains("Flat").is_some(), "what was left out is said in the dialog");
    // A custom pattern is kept as it came: it has no Edit Lines.
    assert!(h.query_by_label("Herringbone: Edit Lines").is_none());
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let p = &m.fill_patterns["Herringbone"];
    assert_eq!(p.target, Target::Model);
    assert_eq!(p.lines.len(), 2);
    assert_eq!(p.lines[1].dashes, [200.0, -200.0]);
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
fn align_on_a_pattern_asks_the_core_where_it_starts_and_says_what_the_core_refuses() {
    let square = r#"[[[1000,1000],[4000,1000]],[[4000,1000],[4000,4000]],[[4000,4000],[1000,4000]],[[1000,4000],[1000,1000]]]"#;
    let mut h = app(&format!(
        "{BOX}\nfilled_region_type {{\"name\": \"Tiles\", \"foreground\": \"Tile 300\"}}\nfilled_region {{\"sketch\": {square}, \"type\": \"Tiles\", \"line_style\": null}}"
    ));
    let region = ids(&h, "region")[0];
    // A reference of no length: only `Model::align_pattern` refuses it, in its own words, and the tool stays on it.
    let none = (P2(500.0, 3000.0), P2(500.0, 3000.0));
    h.state_mut().tool = Tool::Align(Some(none));
    h.run();
    click(&mut h, P2(2100.0, 2500.0));
    let s = &h.state().status;
    assert!(s.error && s.text.contains("no length"), "{s:?}");
    assert_eq!(field(&h, region, "pattern_origin"), json!([0.0, 0.0]));
    assert_eq!(h.state().tool, Tool::Align(Some(none)));
}

#[test]
fn edit_type_during_a_region_sketch_creates_a_type_with_independent_fill_layers() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    press(&mut h, "Edit Type");
    assert!(h.query_by_label("Type Properties").is_some());
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Earth on gray");
    press(&mut h, "Create");
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
    press(&mut h, "Rename…");
    type_into(&mut h, "New name", "Shared fill");
    press(&mut h, "Rename");
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
    type_into(&mut h, "New type", "Discard me");
    press(&mut h, "Create");
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
    // A copy under a name another type has is refused at once, in the dialog, with nothing done.
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Crosshatch");
    press(&mut h, "Create");
    assert!(h.query_by_label_contains("already in use").is_some());
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none());
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "My new region");
    press(&mut h, "Create");
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
    type_into(&mut h, "New type", "Annotation Hatch");
    press(&mut h, "Create");
    press(&mut h, "Foreground Pattern: Edit Patterns");
    let dir = tempfile::tempdir().unwrap();
    let pat = dir.path().join("annotation.pat");
    std::fs::write(&pat, ";%UNITS=MM\n*Annotation Pattern\n;%TYPE=MODEL\n45,0,0,0,100\n").unwrap();
    h.state_mut().import_patterns(&pat);
    h.run();
    press(&mut h, "Annotation Pattern: Edit Lines");
    press(&mut h, "Add Line");
    type_into(&mut h, "Annotation Pattern line 2: Spacing", "200");
    press(&mut h, "‹ Back");
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

#[test]
fn a_pan_or_a_zoom_of_a_section_with_a_region_over_it_draws_the_picture_once_a_frame() {
    let square = r#"[[[1000,1000],[4000,1000]],[[4000,1000],[4000,4000]],[[4000,4000],[1000,4000]],[[1000,4000],[1000,1000]]]"#;
    let mut h = app(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nfilled_region_type {{\"name\": \"Tiles\", \"foreground\": \"Tile 300\"}}"
    ));
    let section = ids(&h, "section")[0];
    let sketch: Value = serde_json::from_str(square).unwrap();
    h.state_mut()
        .session
        .execute("filled_region", &json!({"view": section, "type": "Tiles", "line_style": null, "sketch": sketch}))
        .unwrap();
    browse(&mut h, "Section 1");
    h.run();
    let drawn = |h: &Harness<BimApp>| h.state().cut.pictures;
    let still = drawn(&h);
    assert!(still >= 1, "the section's picture was drawn");
    h.run();
    assert_eq!(drawn(&h), still, "a still window draws nothing again");
    // The frames after a wheel notch, which egui spreads over several: a picture each, and one only.
    let centre = pos2(640.0, 400.0);
    h.hover_at(centre);
    h.run();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    let mut zoom = Vec::new();
    for _ in 0..10 {
        let before = drawn(&h);
        h.step();
        zoom.push(drawn(&h) - before);
    }
    assert!(zoom.iter().filter(|n| **n > 0).count() >= 2, "the wheel zoomed over a few frames: {zoom:?}");
    assert!(zoom.iter().all(|n| *n <= 1), "a frame of the zoom drew the picture {zoom:?} times");
    // A pan, by the middle button, frame by frame.
    h.run();
    let to = centre + vec2(80.0, 40.0);
    h.event(Event::PointerButton { pos: centre, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    let mut pan = Vec::new();
    for at in [centre + vec2(20.0, 10.0), centre + vec2(50.0, 25.0), to] {
        let before = drawn(&h);
        h.hover_at(at);
        h.step();
        pan.push(drawn(&h) - before);
    }
    assert!(pan.iter().all(|n| *n == 1), "each frame of the pan drew the picture once: {pan:?}");
}

/// Detail lines drawn in the plan: a brick's bed joint and its head joint, which runs on past the tile.
fn brick_lines(h: &mut Harness<BimApp>) -> Vec<Id> {
    let s = &mut h.state_mut().session;
    let mut made = Vec::new();
    for (a, b) in [([1000, 1000], [3000, 1000]), ([1000, 1000], [1000, 3000])] {
        let r = s.execute("detail_line", &json!({"start": a, "end": b})).unwrap();
        made.push(r["id"].as_u64().unwrap());
    }
    h.run();
    made
}

#[test]
fn a_custom_pattern_is_made_of_the_lines_picked_cut_to_two_corners_previewed_and_kept_on_ok() {
    let mut h = app(BOX);
    let lines = brick_lines(&mut h);
    annotation(&mut h, "Fill Patterns");
    // The Manage dialog is the patterns alone: region types are edited where a region is drawn, with Edit Type.
    assert!(h.query_by_label("Region Types").is_none());
    press(&mut h, "New Pattern");
    pick(&mut h, Role::Button, "Custom");
    // The dialog steps aside for the plan, which says what to do.
    assert!(h.query_by_label("New Pattern").is_none(), "the dialog is hidden while lines are picked");
    assert_eq!(h.state().prompt(), "Select fill pattern lines then press Enter");
    click(&mut h, P2(2000.0, 1000.0));
    click_with(&mut h, P2(1000.0, 2500.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    key(&mut h, Key::Enter);
    assert_eq!(h.state().prompt(), "Click on Bottom Left Corner");
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(h.state().prompt(), "Click on Top Right Corner");
    click(&mut h, P2(3000.0, 2000.0));
    // Back in the dialog, the pattern previewed: kept with OK, picked again, or dropped.
    assert!(h.query_by_label("Pick Again").is_some(), "{}", h.state().status.text);
    press(&mut h, "Pick Again");
    assert_eq!(h.state().prompt(), "Select fill pattern lines then press Enter");
    assert_eq!(h.state().selection, lines, "the lines picked are still picked");
    key(&mut h, Key::Enter);
    click(&mut h, P2(3000.0, 2000.0));
    click(&mut h, P2(1000.0, 1000.0));
    press(&mut h, "OK");
    // A row of the list now: named, model or drafting, deleted as any; a custom one has no Edit Lines.
    type_into(&mut h, "Pattern 1: Name", "Drawn Brick");
    assert!(h.query_by_label("Drawn Brick: Edit Lines").is_none());
    assert!(h.query_by_label("Sand: Edit Lines").is_none(), "nor an imported or a starter custom one");
    assert!(h.query_by_label("Crosshatch: Edit Lines").is_some(), "a simple one keeps it");
    assert_eq!(h.state().session.model.fill_patterns.get("Drawn Brick"), None, "nothing is the project's until OK");
    // Drafting, it is the drawing on paper at the plan's scale; Model again, the drawing.
    choose(&mut h, "Drawn Brick: Type", "Drafting");
    choose(&mut h, "Drawn Brick: Type", "Model");
    press(&mut h, "OK");
    let p = h.state().session.model.fill_patterns["Drawn Brick"].clone();
    assert_eq!(p.target, Target::Model);
    // Both joints cross the 2000 x 1000 tile whole: courses 1000 apart, heads 2000 apart.
    let mut spacings: Vec<f64> = p.lines.iter().map(|l| l.spacing).collect();
    spacings.sort_by(f64::total_cmp);
    assert_eq!(spacings, [1000.0, 2000.0]);
    // Made again and kept as a drafting pattern.
    annotation(&mut h, "Fill Patterns");
    press(&mut h, "New Pattern");
    pick(&mut h, Role::Button, "Custom");
    click(&mut h, P2(2000.0, 1000.0));
    click_with(&mut h, P2(1000.0, 2500.0), Modifiers::COMMAND);
    key(&mut h, Key::Enter);
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 2000.0));
    press(&mut h, "OK");
    choose(&mut h, "Pattern 1: Type", "Drafting");
    press(&mut h, "OK");
    let p = &h.state().session.model.fill_patterns["Pattern 1"];
    let scale = bimcraft_core::graphics::scale_of(&h.state().session.model, h.state().plan_level().unwrap());
    assert_eq!(p.target, Target::Drafting);
    assert!(p.lines.iter().any(|l| (l.spacing - 1000.0 / scale).abs() < 1e-9), "{p:?}");
}

#[test]
fn escape_while_picking_brings_the_dialog_back_and_nothing_picked_says_so() {
    let mut h = app(BOX);
    brick_lines(&mut h);
    annotation(&mut h, "Fill Patterns");
    press(&mut h, "New Pattern");
    pick(&mut h, Role::Button, "Custom");
    // Enter with nothing picked: said, and still picking.
    key(&mut h, Key::Enter);
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().prompt(), "Select fill pattern lines then press Enter");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("New Pattern").is_some(), "the dialog is back");
    // Corners around nothing: said, and the corners asked again.
    press(&mut h, "New Pattern");
    pick(&mut h, Role::Button, "Custom");
    click(&mut h, P2(2000.0, 1000.0));
    key(&mut h, Key::Enter);
    click(&mut h, P2(5000.0, 4000.0));
    click(&mut h, P2(6000.0, 5000.0));
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!(h.state().prompt(), "Click on Bottom Left Corner");
    // The right-click menu's Finish does what Enter does.
    key(&mut h, Key::Escape);
    press(&mut h, "New Pattern");
    pick(&mut h, Role::Button, "Custom");
    click(&mut h, P2(2000.0, 1000.0));
    h.state_mut().act(crate::context::Action::FinishPick);
    assert_eq!(h.state().prompt(), "Click on Bottom Left Corner");
    assert!(h.state().context_items()[0].iter().all(|i| i.action != crate::context::Action::FinishPick), "only while lines are picked");
}

#[test]
fn a_pattern_made_of_lines_is_redefined_from_lines_picked_again_keeping_its_name_and_type() {
    let mut h = app(BOX);
    let lines = brick_lines(&mut h);
    h.state_mut()
        .session
        .execute(
            "make_fill_pattern",
            &json!({"ids": lines, "from": [1000, 1000], "to": [3000, 2000], "name": "Bond", "target": "drafting"}),
        )
        .unwrap();
    h.run();
    let count = h.state().session.model.fill_patterns.len();
    annotation(&mut h, "Fill Patterns");
    // Made of lines, saved and opened again: Redefine where a simple one has Edit Lines, as wide.
    assert!(h.query_by_label("Bond: Edit Lines").is_none());
    let (redefine, edit) = (h.get_by_label("Bond: Redefine").rect(), h.get_by_label("Crosshatch: Edit Lines").rect());
    assert!((redefine.width() - edit.width()).abs() < 0.5 && (redefine.left() - edit.left()).abs() < 0.5, "{redefine:?} {edit:?}");
    press(&mut h, "Bond: Redefine");
    assert_eq!(h.state().prompt(), "Select fill pattern lines then press Enter");
    h.state_mut().selection = lines.clone();
    key(&mut h, Key::Enter);
    // A tile half as wide: the head joints 1000 apart.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(2000.0, 2000.0));
    assert!(h.query_by_label_contains("Bond redefined").is_some(), "the preview says what it redefines");
    press(&mut h, "OK");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.fill_patterns.len(), count, "no new pattern");
    let p = &m.fill_patterns["Bond"];
    let scale = p.drawn.unwrap();
    assert_eq!(p.target, Target::Drafting, "drafting still");
    let mut spacings: Vec<f64> = p.lines.iter().map(|l| (l.spacing * scale).round()).collect();
    spacings.sort_by(f64::total_cmp);
    assert_eq!(spacings, [1000.0, 1000.0]);
}

#[test]
fn the_annotation_settings_list_and_the_fill_patterns_window_are_no_wider_than_what_they_hold() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Annotation Settings");
    assert!(h.get_by_label("Fill Patterns").rect().width() <= 220.0, "{:?}", h.get_by_label("Fill Patterns").rect());
    press(&mut h, "Fill Patterns");
    let r = h.ctx.memory(|m| m.area_rect(egui::Id::new("fill-patterns"))).unwrap();
    assert!(r.width() <= 760.0, "{r:?}");
    // Import and Export at the right, with what an import does with a name the list has.
    let (import, new) = (h.get_by_label("Import...").rect(), h.get_by_label("New Pattern").rect());
    assert!(import.left() > r.center().x && new.right() < r.center().x);
    assert!(h.query_by_label("Replace imported by same name").is_some());
}

#[test]
fn the_line_editor_stands_twenty_pixels_clear_of_the_row_of_buttons_under_it() {
    let mut h = app(BOX);
    annotation(&mut h, "Fill Patterns");
    press(&mut h, "Crosshatch: Edit Lines");
    let (done, new) = (h.get_by_label("Done").rect(), h.get_by_label("New Pattern").rect());
    assert!(new.top() - done.bottom() >= 20.0, "{done:?} {new:?}");
}
