//! The Draw panel's tools between one use and the next: what a tool leaves behind (a cloud, a fillet's picks, digits typed), the
//! Options Bar's numbers when they are wrong, and a sketch's undo, closing and plan.

use super::*;
use bimcraft_core::curves::Curve;

const TWO_LINES: &str =
    "detail_line {\"start\": [1000, 1000], \"end\": [4000, 1000]}\ndetail_line {\"start\": [5000, 2000], \"end\": [5000, 5000]}";

/// The plan of the box with the Detail Line tool in hand and `extra` drawn.
fn detailing(extra: &str) -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{extra}"));
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    h
}

fn curve(h: &Harness<BimApp>, id: Id) -> Curve {
    match h.state().session.model.elements.get(id) {
        Some(Element::DetailLine(l)) => l.curve.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

/// The two lines of `TWO_LINES` picked for a fillet.
fn picked_two(h: &mut Harness<BimApp>) {
    press(h, "Fillet Arc");
    click(h, P2(2000.0, 1000.0));
    click(h, P2(5000.0, 4000.0));
}

/// Digits typed with the cursor over the plan.
fn digits(h: &mut Harness<BimApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run();
}

#[test]
fn a_revision_cloud_or_insulation_stays_with_the_detail_line_tool() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Revision Cloud");
    assert!(h.state().drawing.cloud.is_some());
    tool(&mut h, "Wall");
    assert!(h.state().drawing.cloud.is_none(), "the Wall tool has no clouds");
    assert!(!h.state().one_line(), "a wall's Line is not a cloud's");
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(4000.0, 2000.0));
    assert!(!h.state().finishable(), "so there is nothing for Finish Drawing to finish");
    assert_eq!(walls(&h).len(), 5, "the wall was built, one a click");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Insulation");
    assert!(h.state().drawing.insulation.is_some());
    tool(&mut h, "Wall");
    assert!(h.state().drawing.insulation.is_none());
    // The Detail Line tool is the one that keeps them while it is the tool.
    press(&mut h, "Annotate");
    press(&mut h, "Revision Cloud");
    assert!(h.state().drawing.cloud.is_some() && h.state().drawing.insulation.is_none());
}

#[test]
fn the_lines_picked_for_a_fillet_are_dropped_with_the_tool_and_with_the_sketch() {
    let mut h = detailing(TWO_LINES);
    picked_two(&mut h);
    assert_eq!(h.state().drawing.fillet.len(), 2);
    h.state_mut().set_tool(Tool::Modify);
    h.run();
    assert!(h.state().drawing.fillet.is_empty(), "Modify leaves nothing lit as if selected");
    // In a sketch, a pick is a line's place in the sketch: Finish Edit Mode ends it.
    key(&mut h, Key::Escape);
    shortcut(&mut h, "FR");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3000.0, 4000.0));
    press(&mut h, "Fillet Arc");
    click(&mut h, P2(2000.0, 3000.0));
    click(&mut h, P2(3000.0, 3500.0));
    assert_eq!(h.state().drawing.fillet.len(), 2);
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    assert!(h.state().drawing.fillet.is_empty());
    // A new sketch, Fillet Arc still the shape: no radius is waiting for lines that are not there.
    shortcut(&mut h, "FR");
    assert!(!h.state().filleting());
    // Lines that change under a pick (one deleted before it) drop it too.
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3000.0, 4000.0));
    press(&mut h, "Fillet Arc");
    click(&mut h, P2(2000.0, 3000.0));
    assert_eq!(h.state().drawing.fillet.len(), 1);
    h.state_mut().sketch.as_mut().unwrap().lines.remove(0);
    h.run();
    assert!(h.state().drawing.fillet.is_empty(), "its number now means another line");
}

#[test]
fn a_typed_fillet_radius_that_does_not_fit_keeps_the_lines_and_one_that_does_clears_its_digits() {
    let mut h = detailing(TWO_LINES);
    let lines = ids(&h, "line");
    picked_two(&mut h);
    hover(&mut h, P2(4500.0, 1500.0));
    // Too big: the lines stay picked and the digits are spent, so the next number typed is a new radius.
    digits(&mut h, "99999");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!((h.state().drawing.fillet.len(), h.state().typed_length.as_str()), (2, ""));
    assert_eq!(ids(&h, "line"), lines, "nothing was made");
    digits(&mut h, "500");
    // The arc shown is the radius typed, wherever the cursor is.
    let cursor = P2(4900.0, 1100.0);
    let shown = h.state().shape_preview(cursor);
    let radius = shown.get(1).and_then(|c| c.segments().first().and_then(|s| s.radius()));
    assert!(radius.is_some_and(|r| (r - 500.0).abs() < 1e-6), "{radius:?}");
    assert!(h.state().fillet_radius_at(cursor).is_some_and(|r| (r - 500.0).abs() > 1.0), "the cursor's own radius is another");
    key(&mut h, Key::Enter);
    assert_eq!(ids(&h, "line").len(), 3);
    assert!(curve(&h, lines[0]).end().dist(P2(4500.0, 1000.0)) < 1e-3);
    // Enter again: nothing is left to fillet, nothing happens, and nothing is said.
    key(&mut h, Key::Enter);
    assert_eq!((ids(&h, "line").len(), h.state().status.error), (3, false));
}

#[test]
fn digits_typed_for_a_fillet_that_a_click_sized_do_not_stay_for_the_next() {
    let mut h = detailing(&format!(
        "{TWO_LINES}\ndetail_line {{\"start\": [1000, 6000], \"end\": [4000, 6000]}}\ndetail_line {{\"start\": [5000, 7000], \"end\": [5000, 9000]}}"
    ));
    let lines = ids(&h, "line");
    picked_two(&mut h);
    hover(&mut h, P2(4500.0, 1500.0));
    digits(&mut h, "500");
    // Sized by a click instead.
    click(&mut h, P2(4500.0, 1500.0));
    assert_eq!(ids(&h, "line").len(), 5, "the arc was made");
    assert!(h.state().typed_length.is_empty(), "'500' is spent: {}", h.state().typed_length);
    click(&mut h, P2(2000.0, 6000.0));
    click(&mut h, P2(5000.0, 8000.0));
    digits(&mut h, "300");
    assert_eq!(h.state().typed_length, "300", "not 500300");
    key(&mut h, Key::Enter);
    assert!(curve(&h, lines[2]).end().dist(P2(4700.0, 6000.0)) < 1e-3, "{:?}", curve(&h, lines[2]));
}

#[test]
fn what_is_typed_for_one_shape_is_not_taken_for_another_and_tab_stays_among_its_fields() {
    let mut h = detailing("");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    hover(&mut h, P2(2000.0, 1500.0));
    digits(&mut h, "2400");
    key(&mut h, Key::Tab);
    digits(&mut h, "300");
    press(&mut h, "Circle");
    assert_eq!((h.state().typed_length.as_str(), h.state().drawing.other.as_str(), h.state().drawing.field), ("", "", 0));
    // A circle's one number is the field Tab was on, whatever it was last: its label says what Enter does.
    click(&mut h, P2(3000.0, 4000.0));
    hover(&mut h, P2(4000.0, 4000.0));
    h.state_mut().drawing.field = 1;
    h.state_mut().typed_length = "300".into();
    let chain = h.state().points().cloned().unwrap_or_default();
    let m = h.state().session.model.clone();
    let texts = h.state().shape_texts(&m, &chain, P2(4000.0, 4000.0));
    assert_eq!(texts.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["300 (Enter)"]);
    key(&mut h, Key::Enter);
    assert_eq!(curve(&h, *ids(&h, "line").last().unwrap()), Curve::Circle { centre: P2(3000.0, 4000.0), radius: 300.0 });
}

#[test]
fn an_arcs_radius_is_read_by_the_cursor_and_the_wall_tool_shows_it_too() {
    let mut h = app("");
    tool(&mut h, "Wall");
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(4000.0, 0.0));
    let chain = h.state().points().cloned().unwrap_or_default();
    let m = h.state().session.model.clone();
    let texts = h.state().shape_texts(&m, &chain, P2(2000.0, -2000.0));
    assert_eq!(texts.len(), 1);
    assert!(texts[0].1.starts_with("R "), "{}", texts[0].1);
}

#[test]
fn a_number_the_options_bar_cannot_read_is_said_and_makes_nothing() {
    // A revision cloud's arc length.
    let mut h = detailing("");
    press(&mut h, "Annotate");
    press(&mut h, "Revision Cloud");
    type_into(&mut h, "Arc Length:", "abc");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 2000.0));
    assert!(ids(&h, "line").is_empty(), "not a cloud of 500 mm arcs");
    assert!(h.state().status.error && h.state().status.text.contains("Arc Length") && h.state().status.text.contains("abc"));
    // A polygon's sides.
    let mut h = detailing("");
    press(&mut h, "Inscribed Polygon");
    type_into(&mut h, "Sides:", "six");
    click(&mut h, P2(2000.0, 2000.0));
    assert!(h.state().status.error && h.state().status.text.contains("six"), "{}", h.state().status.text);
    click(&mut h, P2(3000.0, 2000.0));
    assert!(ids(&h, "line").is_empty(), "not a hexagon");
    // A radius that is switched on.
    let mut h = detailing("");
    press(&mut h, "Circle");
    press(&mut h, "Radius:");
    type_role(&mut h, Role::TextInput, "Radius", "big");
    click(&mut h, P2(2000.0, 2000.0));
    assert!(ids(&h, "line").is_empty() && h.state().status.error && h.state().status.text.contains("big"));
    // An offset, for a line and for a rectangle.
    let mut h = detailing("");
    type_into(&mut h, "Offset:", "far");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 1000.0));
    assert!(ids(&h, "line").is_empty() && h.state().status.text.contains("far"), "{}", h.state().status.text);
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(2000.0, 4000.0));
    assert!(ids(&h, "line").is_empty());
    // Read again, it draws.
    type_into(&mut h, "Offset:", "0");
    click(&mut h, P2(2000.0, 4000.0));
    click(&mut h, P2(1000.0, 3000.0));
    assert_eq!(ids(&h, "line").len(), 4);
    // Insulation's width.
    let mut h = detailing("");
    press(&mut h, "Annotate");
    press(&mut h, "Insulation");
    type_into(&mut h, "Width:", "wide");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 1000.0));
    assert!(ids(&h, "line").is_empty() && h.state().status.text.contains("wide"), "{}", h.state().status.text);
}

#[test]
fn a_dot_of_a_hundredth_of_a_millimetre_still_draws_about_a_pixel_on_the_canvas() {
    // The core gives a Dot that length, and the canvas hands egui a dash of it. egui's lines have no butt caps to lose it: its
    // feathering draws a dash of a hundredth of a pixel as long as one of a pixel and a half, so a dot shows (a round marker is not needed).
    // If egui changes that, this fails and the canvas draws its dots itself.
    let covered = |on: f32| {
        let mut mesh = egui::Mesh::default();
        let mut tessellator = egui::epaint::Tessellator::new(1.0, Default::default(), [64, 64], Vec::new());
        let line = [pos2(0.0, 5.0), pos2(30.0, 5.0)];
        for shape in egui::Shape::dashed_line_with_offset(&line, egui::Stroke::new(1.5, egui::Color32::BLACK), &[on], &[100.0], 0.0) {
            tessellator.tessellate_shape(shape, &mut mesh);
        }
        let corner = |i: u32| mesh.vertices[i as usize].pos;
        let alpha = |t: &[u32]| t.iter().map(|i| f32::from(mesh.vertices[*i as usize].color.a())).sum::<f32>() / 3.0 / 255.0;
        mesh.indices
            .chunks(3)
            .map(|t| {
                let (a, b) = (corner(t[1]) - corner(t[0]), corner(t[2]) - corner(t[0]));
                alpha(t) * (a.x * b.y - a.y * b.x).abs() / 2.0
            })
            .sum::<f32>()
    };
    let (dot, pixel) = (covered(0.01 * 3.78), covered(1.5));
    assert!(dot > 1.0 && (dot - pixel).abs() < pixel * 0.2, "a dot covers {dot} px², a dash 1.5 px long {pixel}");
}

fn sketched(h: &Harness<BimApp>) -> Vec<Curve> {
    h.state().sketch.as_ref().map(|s| s.lines.iter().map(|l| l.curve.clone()).collect()).unwrap_or_default()
}

fn undo(h: &mut Harness<BimApp>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

#[test]
fn a_loop_that_closes_after_one_line_makes_no_second_line_on_the_first() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    press(&mut h, "Line");
    let (a, b, c) = (P2(1000.0, 1000.0), P2(3000.0, 1000.0), P2(3000.0, 3000.0));
    click(&mut h, a);
    click(&mut h, b);
    click(&mut h, a);
    assert_eq!(sketched(&h), [Curve::line(a, b)], "one line is no loop");
    assert!(h.state().status.error, "and it says so");
    assert_eq!(h.state().tool, Tool::SketchLine(vec![a, b]), "the chain goes on");
    click(&mut h, c);
    click(&mut h, a);
    assert_eq!(sketched(&h).len(), 3, "a triangle closes");
    assert_eq!(h.state().tool, Tool::SketchLine(Vec::new()));
    press(&mut h, "Finish Edit Mode");
    assert_eq!(ids(&h, "region").len(), 1, "{}", h.state().status.text);
}

#[test]
fn ctrl_z_while_sketching_takes_back_one_change_whole_not_one_line() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    press(&mut h, "Line");
    for p in [P2(1000.0, 1000.0), P2(4000.0, 1000.0), P2(4000.0, 4000.0)] {
        click(&mut h, p);
    }
    key(&mut h, Key::Escape);
    let before = sketched(&h);
    assert_eq!(before.len(), 2);
    // A fillet cuts both lines back and adds the arc: one change.
    press(&mut h, "Fillet Arc");
    press(&mut h, "Radius:");
    type_role(&mut h, Role::TextInput, "Radius", "500");
    click(&mut h, P2(2000.0, 1000.0));
    click(&mut h, P2(4000.0, 2500.0));
    assert_eq!(sketched(&h).len(), 3, "{}", h.state().status.text);
    undo(&mut h);
    assert_eq!(sketched(&h), before, "the lines are as they were, uncut, and the arc is gone");
    // A trim, a grip moved: each a step, back through them in turn.
    press(&mut h, "Line");
    click(&mut h, P2(5000.0, 5000.0));
    click(&mut h, P2(6000.0, 5000.0));
    assert_eq!(sketched(&h).len(), 3);
    undo(&mut h);
    assert_eq!(sketched(&h), before);
    undo(&mut h);
    assert_eq!(sketched(&h).len(), 1);
    undo(&mut h);
    undo(&mut h);
    assert!(sketched(&h).is_empty(), "nothing more to take back, and no harm in asking");
}

#[test]
fn a_sketch_stays_in_the_plan_it_began_in() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    let first = h.state().sheet;
    let levels = h.state().session.model.levels();
    let second = levels.iter().map(|(id, _)| *id).find(|id| Sheet::Plan(*id) != first).expect("a second level");
    h.state_mut().open_sheet(Sheet::Plan(second));
    h.run();
    assert_eq!(h.state().sheet, first, "back in the plan the sketch belongs to");
    assert!(h.state().status.error && h.state().sketch.is_some());
    // Finished or cancelled, the plan can be left.
    press(&mut h, "Cancel Edit Mode");
    h.state_mut().open_sheet(Sheet::Plan(second));
    h.run();
    assert_eq!(h.state().sheet, Sheet::Plan(second));
}

#[test]
fn edit_type_from_a_regions_edit_boundary_opens_the_type_the_region_has() {
    let mut h = app(&format!(
        "{BOX}\nfilled_region {{\"sketch\": [[[1000,1000],[3000,1000]],[[3000,1000],[3000,3000]],[[3000,3000],[1000,3000]],[[1000,3000],[1000,1000]]], \"type\": \"Sand\"}}"
    ));
    let region = ids(&h, "region")[0];
    h.state_mut().selection = vec![region];
    h.run();
    press(&mut h, "Modify | Filled Regions");
    press(&mut h, "Edit Boundary");
    assert!(matches!(h.state().sketch.as_ref().map(|s| s.of), Some(crate::sketch::Of::Edit(id)) if id == region));
    press(&mut h, "Edit Type");
    let shown = h.get_by_role_and_label(Role::ComboBox, "Type:").value();
    assert_eq!(shown.as_deref(), Some("Sand"));
    assert!(h.query_by_label("Type Properties").is_some());
}
