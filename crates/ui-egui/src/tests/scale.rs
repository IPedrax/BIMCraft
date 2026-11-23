//! Modify > Scale (RE), driven as a user drives it: a popup asks how, by a percentage or by a length between two points;
//! then the point that stays, or two points with a line between them and a field for their new length. What keeps its size
//! (text, families, model elements) is left as it is and said; a view with nothing Scale takes says so.

use super::*;
use crate::scale::Step;

/// A line from (1000, 1000) to (3000, 1000) in Level 1's plan, and a note above it.
const LINE: &str = r#"detail_line {"points": [[1000, 1000], [3000, 1000]]}
text {"text": "Kitchen", "at": [1000, 4000]}"#;

fn line(h: &Harness<BimApp>) -> Id {
    find(h, |e| matches!(e, Element::DetailLine(_)))
}

fn ends(h: &Harness<BimApp>, id: Id) -> Vec<P2> {
    let Some(Element::DetailLine(l)) = h.state().session.model.elements.get(id) else { panic!("{id} isn't a line") };
    l.curve.points()
}

#[test]
fn scale_by_a_percentage_grows_the_selection_about_the_point_clicked() {
    let mut h = wide(LINE);
    let id = line(&h);
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(h.state().selection, [id]);
    press_near(&mut h, "Scale", "Rotate");
    assert_eq!(h.state().tool, Tool::Scale(Step::Choose), "the popup asks how first");
    assert!(h.query_by_label("By percentage").is_some() && h.query_by_label("By length between two points").is_some());
    type_into(&mut h, "Percentage:", "50");
    press(&mut h, "OK");
    assert_eq!(h.state().tool, Tool::Scale(Step::Base(0.5)));
    // The point that stays: the line's start, snapped to.
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(ends(&h, id), [P2(1000.0, 1000.0), P2(2000.0, 1000.0)]);
    assert_eq!((h.state().tool.clone(), h.state().selection.clone()), (Tool::Modify, vec![id]), "still selected, back to Modify");
    assert_eq!(h.state().session.history().last(), Some(&"Scale"), "one step to undo");
}

#[test]
fn scale_by_a_length_draws_a_line_between_two_points_and_takes_the_length_typed() {
    let mut h = wide(LINE);
    let id = line(&h);
    click(&mut h, P2(2000.0, 1000.0));
    shortcut(&mut h, "RE");
    assert_eq!(h.state().tool, Tool::Scale(Step::Choose), "RE is Scale's shortcut");
    press(&mut h, "By length between two points");
    press(&mut h, "OK");
    assert_eq!(h.state().tool, Tool::Scale(Step::First));
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(h.state().tool, Tool::Scale(Step::Second(P2(1000.0, 1000.0))));
    // A line follows the cursor from the first point, its length beside it.
    h.hover_at(h.state().screen(P2(2600.0, 1000.0)));
    h.run();
    click(&mut h, P2(3000.0, 1000.0));
    assert_eq!(h.state().tool, Tool::Scale(Step::Length(P2(1000.0, 1000.0), P2(3000.0, 1000.0))));
    assert_eq!(h.state().scaling.length, "2000", "the field starts at the length between them");
    type_into(&mut h, "New length:", "5000");
    key(&mut h, Key::Enter);
    assert_eq!(ends(&h, id), [P2(1000.0, 1000.0), P2(6000.0, 1000.0)], "the first point stays, the second goes to the length");
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn what_scale_cant_take_is_said_and_a_bad_size_is_asked_again() {
    let mut h = wide(&format!("{BOX}\n{LINE}"));
    let id = line(&h);
    let note = find(&h, |e| matches!(e, Element::Text(_)));
    // Nothing selected.
    shortcut(&mut h, "RE");
    assert!(h.state().status.error && h.state().status.text.contains("select the annotation"), "{:?}", h.state().status);
    assert_eq!(h.state().tool, Tool::Modify);
    // A wall alone: model elements keep their size.
    click(&mut h, P2(4000.0, 0.0));
    press_near(&mut h, "Scale", "Rotate");
    assert!(h.state().status.error && h.state().status.text.contains("model elements keep their size"), "{:?}", h.state().status);
    assert_eq!(h.state().tool, Tool::Modify);
    // The line and the note: a percentage that isn't one, then none at all, is asked again.
    h.state_mut().selection = vec![id, note];
    h.run();
    press_near(&mut h, "Scale", "Rotate");
    type_into(&mut h, "Percentage:", "big");
    press(&mut h, "OK");
    assert_eq!(h.state().tool, Tool::Scale(Step::Choose));
    assert!(h.query_by_label("`big` is not a percentage").is_some());
    type_into(&mut h, "Percentage:", "0");
    press(&mut h, "OK");
    assert!(h.query_by_label("Enter a size above 0.").is_some() && h.state().tool == Tool::Scale(Step::Choose));
    type_into(&mut h, "Percentage:", "200 %");
    press(&mut h, "OK");
    let before = h.state().session.model.elements.get(note).cloned();
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(ends(&h, id), [P2(1000.0, 1000.0), P2(5000.0, 1000.0)]);
    assert_eq!(h.state().session.model.elements.get(note).cloned(), before, "the note keeps its size and place");
    let s = &h.state().status;
    assert!(s.error && s.text.contains("Scaled 1") && s.text.contains("text keeps its size"), "said: {s:?}");
    // By length: a length that isn't one, or none, is asked again; Esc leaves it all as it was.
    h.state_mut().selection = vec![id];
    h.run();
    press_near(&mut h, "Scale", "Rotate");
    assert_eq!(h.state().tool, Tool::Scale(Step::Choose), "{:?}", h.state().status);
    press(&mut h, "By length between two points");
    press(&mut h, "OK");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 1000.0));
    assert!(h.state().status.error && matches!(h.state().tool, Tool::Scale(Step::Second(_))), "a second point on the first");
    click(&mut h, P2(5000.0, 1000.0));
    for (typed, said) in [("long", "`long` is not a length"), ("0", "Enter a length above 0.")] {
        type_into(&mut h, "New length:", typed);
        key(&mut h, Key::Enter);
        assert!(h.query_by_label(said).is_some(), "{typed}");
        assert!(matches!(h.state().tool, Tool::Scale(Step::Length(..))));
    }
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!((h.state().tool.clone(), ends(&h, id)), (Tool::Modify, vec![P2(1000.0, 1000.0), P2(5000.0, 1000.0)]));
    // The 3D view shows nothing Scale takes.
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().selection = vec![id];
    shortcut(&mut h, "RE");
    assert!(h.state().status.error && h.state().status.text.contains("plans, sections and elevations"), "{:?}", h.state().status);
}

#[test]
fn a_sections_detail_line_is_scaled_about_a_point_clicked_in_the_section() {
    let mut h = wide(&format!(
        "{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\ndetail_line {{\"view\": \"Section 1\", \"points\": [[2000, 1000], [4000, 1000]]}}"
    ));
    let id = line(&h);
    browse(&mut h, "Section 1");
    h.state_mut().selection = vec![id];
    h.run();
    shortcut(&mut h, "RE");
    type_into(&mut h, "Percentage:", "150");
    press(&mut h, "OK");
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(ends(&h, id), [P2(2000.0, 1000.0), P2(5000.0, 1000.0)]);
}

/// What Scale looks like, for a person to look at: `BIMCRAFT_SCALE_DUMP=<folder>` writes its popup, the selection grown about
/// the cursor, the line between the two points and the length's field, light and dark.
#[test]
fn scale_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_SCALE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let mut h = wide(&format!("{LINE}\ndetail_line {{\"centre\": [2000, 2500], \"radius\": 600}}"));
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        let save = |h: &mut Harness<BimApp>, name: &str| {
            h.run_steps(4);
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("scale-{name}-{look}.png"))).unwrap();
        };
        h.state_mut().selection = ids(&h, "line");
        h.run();
        shortcut(&mut h, "RE");
        save(&mut h, "popup");
        type_into(&mut h, "Percentage:", "150");
        press(&mut h, "OK");
        h.hover_at(h.state().screen(P2(1000.0, 1000.0)));
        save(&mut h, "percent");
        key(&mut h, Key::Escape);
        shortcut(&mut h, "RE");
        press(&mut h, "By length between two points");
        press(&mut h, "OK");
        click(&mut h, P2(1000.0, 1000.0));
        h.hover_at(h.state().screen(P2(2400.0, 1600.0)));
        save(&mut h, "line");
        click(&mut h, P2(3000.0, 1000.0));
        type_into(&mut h, "New length:", "3000");
        save(&mut h, "length");
    }
}
