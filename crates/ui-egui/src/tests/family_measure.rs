//! Measure and Create Similar in a family's plans and elevations, run as Find runs them. Measure lays a ruler by two clicks
//! and reads it, changing nothing; Create Similar starts the tool that makes what is selected, as it is.
use super::family_drag_move::{CENTER_LR, DIM, EDGE, LINE, family, g, pick, plan, status, untouched};
use super::family_selection::command;
use super::family_views::{click_family, editor};
use super::*;
use crate::editor::select_box::Picked;
use bimcraft_core::family::DimKind;
use bimcraft_core::family::forms::FormKind;

#[test]
fn measure_lays_a_ruler_by_two_clicks_reads_it_and_changes_nothing() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Measure");
    assert!(editor(&h).measuring(), "Measure");
    assert!(h.state().prompt().contains("Measure"), "{}", h.state().prompt());
    assert!(editor(&h).selection.items().is_empty(), "the selection goes, as with any tool");
    click_family(&mut h, [0.0, 0.0, 0.0]);
    click_family(&mut h, [300.0, -400.0, 0.0]);
    let r = editor(&h).measured().expect("a ruler laid");
    assert!((r.length - 500.0).abs() < 1e-6 && (r.dx - 300.0).abs() < 1e-6 && (r.dy + 400.0).abs() < 1e-6, "{r:?}");
    assert!(status(&h).contains("500"), "the status bar reads it: {}", status(&h));
    assert!(untouched(&h), "it changes nothing");
    // Another first click lays another; Escape puts the tool down.
    click_family(&mut h, [0.0, 0.0, 0.0]);
    assert!(editor(&h).measured().is_none());
    // Escape lets the first click go, then puts the tool down.
    key(&mut h, Key::Escape);
    assert!(editor(&h).measuring());
    key(&mut h, Key::Escape);
    assert!(!editor(&h).measuring());
    // The Measure panel's button picks it up again (wide enough for the ribbon to show it).
    h.set_size(egui::vec2(3200.0, 1000.0));
    pick(&mut h, &[Picked::Form(0)]);
    press(&mut h, "Measure");
    assert!(editor(&h).measuring());
}

#[test]
fn create_similar_starts_the_tool_that_makes_what_is_selected() {
    let mut h = plan();
    // Nothing selected: nothing.
    command(&mut h, "Create Similar");
    assert!(editor(&h).sketch.is_none() && editor(&h).views.drawing.is_none());
    // A solid extrusion: a new one sketched, solid.
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Create Similar");
    let s = editor(&h).sketch.as_ref().expect("a sketch");
    assert_eq!((s.kind, s.void), (FormKind::Extrusion, false));
    h.state_mut().active_family_mut().unwrap().cancel_form_sketch();
    h.run();
    // A plane: the Reference Plane tool.
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Create Similar");
    assert_eq!(editor(&h).views.drawing, Some(None), "{}", status(&h));
    key(&mut h, Key::Escape);
    // A datum too.
    pick(&mut h, &[Picked::Plane(g(CENTER_LR))]);
    command(&mut h, "Create Similar");
    assert_eq!(editor(&h).views.drawing, Some(None));
    key(&mut h, Key::Escape);
    // An aligned dimension: Aligned Dimension.
    pick(&mut h, &[Picked::Dimension(g(DIM))]);
    command(&mut h, "Create Similar");
    assert!(editor(&h).views.dimensioning.is_some() && editor(&h).views.dim_kind == DimKind::Aligned);
    key(&mut h, Key::Escape);
    // A symbolic line: another, as the ribbon's Symbolic Line adds one.
    pick(&mut h, &[Picked::Symbol(0)]);
    command(&mut h, "Create Similar");
    assert_eq!(family(&h).symbols.len(), 2, "{}", status(&h));
    // Two things selected: Create Similar takes one, so it does nothing.
    let n = family(&h).symbols.len();
    pick(&mut h, &[Picked::Form(0), Picked::Form(1)]);
    command(&mut h, "Create Similar");
    assert!(editor(&h).sketch.is_none() && family(&h).symbols.len() == n);
    // A reference line: the Reference Line tool.
    pick(&mut h, &[Picked::Line(g(LINE))]);
    command(&mut h, "Create Similar");
    assert!(editor(&h).views.drawing.is_some() && editor(&h).views.drawing_line, "{}", status(&h));
}

/// A ruler laid in plan, read over the view and in the status bar, saved where `BIMCRAFT_FAMILY_MEASURE_DUMP` says (only
/// then), in light and dark.
#[test]
fn family_measure_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_MEASURE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for dark in [false, true] {
        let mut h = app(&super::family_drag_move::family_with(""));
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        h.set_size(vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Movers");
        h.run();
        super::family_views::view(&mut h, Some(bimcraft_core::family::views::Which::Plan));
        command(&mut h, "Measure");
        click_family(&mut h, [100.0, 500.0, 0.0]);
        click_family(&mut h, [1000.0, 900.0, 0.0]);
        // The pointer off the ruler, so its reading shows whole.
        let away = editor(&h).views.screen_of([1300.0, -600.0, 0.0]).unwrap();
        h.hover_at(away);
        h.run();
        h.run();
        let name = if dark { "measure-dark.png" } else { "measure-light.png" };
        h.render().unwrap().save(std::path::Path::new(&folder).join(name)).unwrap();
    }
}

#[test]
fn measure_snaps_to_a_planes_trace_and_another_tool_puts_it_down() {
    let mut h = plan();
    command(&mut h, "Measure");
    // A few millimetres off Edge (x 1000): the ruler's end is on it.
    click_family(&mut h, [0.0, 200.0, 0.0]);
    click_family(&mut h, [1004.0, 200.0, 0.0]);
    let r = editor(&h).measured().expect("a ruler laid");
    assert!((r.dx - 1000.0).abs() < 1e-6, "{r:?}");
    // Reference Plane picked: Measure is put down, so the next clicks draw the plane.
    h.state_mut().active_family_mut().unwrap().reference_plane_tool();
    h.run();
    assert!(!editor(&h).measuring());
    assert_eq!(editor(&h).views.drawing, Some(None));
    // In {3D}, Create Similar on a plane says where it works.
    super::family_views::view(&mut h, None);
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Create Similar");
    assert!(status(&h).contains("plan or an elevation") && h.state().status.error, "{}", status(&h));
}
