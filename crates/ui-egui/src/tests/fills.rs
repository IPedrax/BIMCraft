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
    // Its type and lines in Properties; its sketch with Edit Boundary.
    press(&mut h, "Modify | Filled Regions");
    choose(&mut h, "Region Type", "Sand");
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
