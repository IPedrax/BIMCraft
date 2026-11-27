//! The Options Bar through the app, with no window: what is set joins the click's params, the Type Selector of the tool in hand picks the
//! type, a field that is no height refuses the click, and Chain off ends the chain after each wall.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Location};
use rune::TextEdit;
use serde_json::Value;

use crate::app::App;
use crate::props::panel::Subject;
use crate::select::How;

/// Far from the house, where a point lands on the nearest 10 mm.
const A: P2 = P2(-30000.0, -30000.0);
const B: P2 = P2(-26000.0, -30000.0);

fn click(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.release((x, y), (x, y), How::Replace);
}

fn walls(app: &App) -> Vec<Id> {
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect()
}

/// What the core says of the wall made last.
fn newest(app: &App) -> Value {
    let id = walls(app).into_iter().max().unwrap_or_default();
    bimcraft_core::element_json(&app.session.model, id)
}

fn typed(app: &mut App, key: &str, text: &str) {
    let Some(f) = app.bar.fields.iter_mut().find(|f| f.key == key) else { panic!("the bar has a field {key}") };
    f.edit = TextEdit::new(text);
}

#[test]
fn a_tool_in_hand_makes_the_palette_about_what_it_places_and_the_type_picked_goes_into_the_wall_made() {
    let mut app = App::sample();
    assert!(matches!(app.palette.panel.subject, Subject::Plan(_)));
    app.set_tool("wall");
    assert_eq!(app.palette.panel.subject, Subject::New("wall"), "the palette follows the tool");
    let Some(selector) = app.palette.panel.selector.clone() else { panic!("a wall tool has a Type Selector") };
    let usual = selector.current.clone().unwrap_or_default();
    let Some(other) = selector.types.iter().position(|t| t.label != usual) else { panic!("the house has two wall types") };
    let name = selector.types[other].name.clone();
    assert!(app.pick_type(other), "the pick is the tool's, not a change to any element");
    assert_eq!(app.palette.panel.selector.as_ref().and_then(|s| s.current.clone()), Some(name.clone()));
    let before = walls(&app).len();
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_eq!(walls(&app).len(), before + 1);
    assert_eq!(newest(&app)["type"], serde_json::json!(name), "the wall is of the type the selector picked, not the first there is");
    // A selection brings the palette back to the elements; the tool down, to the plan.
    let id = walls(&app)[0];
    app.select(vec![id]);
    assert_eq!(app.palette.panel.subject, Subject::Element(id));
    app.select(Vec::new());
    assert_eq!(app.palette.panel.subject, Subject::New("wall"));
    assert!(app.escape(), "Escape gives up the chain");
    assert!(app.escape(), "then the tool");
    assert!(app.tool.is_none());
    assert!(matches!(app.palette.panel.subject, Subject::Plan(_)), "{:?}", app.palette.panel.subject);
}

#[test]
fn the_location_line_and_an_unconnected_height_of_the_bar_are_the_walls() {
    let mut app = App::sample();
    app.set_tool("wall");
    assert!(app.bar_choose("wall.location", 3));
    assert_eq!(app.bar.settings.location, Location::ALL[3]);
    let last = crate::options::items("wall", &app.session.model, app.level.unwrap_or_default(), &app.bar.settings)
        .first()
        .map(|i| match &i.editor {
            crate::options::Editor::List { options, .. } => options.len() - 1,
            _ => 0,
        })
        .unwrap_or_default();
    assert!(app.bar_choose("wall.top", last), "Unconnected is the last way");
    typed(&mut app, "wall.height", "2500");
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.message.is_none(), "{:?}", app.message);
    let wall = newest(&app);
    assert_eq!(wall["location"], serde_json::json!(Location::ALL[3].name()));
    assert_eq!(wall["height"], serde_json::json!(2500.0), "unconnected, at the height typed");
}

#[test]
fn a_field_that_is_no_height_refuses_the_click_and_gives_the_point_back() {
    let mut app = App::sample();
    app.set_tool("wall");
    assert!(!app.bar_choose("wall.top", 99), "a way there is not");
    let last = crate::options::items("wall", &app.session.model, app.level.unwrap_or_default(), &app.bar.settings)
        .first()
        .map(|i| match &i.editor {
            crate::options::Editor::List { options, .. } => options.len() - 1,
            _ => 0,
        })
        .unwrap_or_default();
    assert!(app.bar_choose("wall.top", last));
    typed(&mut app, "wall.height", "tall");
    let before = walls(&app).len();
    click(&mut app, A);
    click(&mut app, B);
    assert_eq!(walls(&app).len(), before, "nothing was asked of the core");
    assert!(app.message.as_deref().is_some_and(|m| m.contains("tall") && m.contains("height")), "{:?}", app.message);
    assert_eq!(app.picked.len(), 1, "the click that was refused is given back");
}

#[test]
fn chain_off_ends_the_chain_after_each_wall_and_a_beam_takes_its_justification_and_offset() {
    let mut app = App::sample();
    app.set_tool("wall");
    assert!(app.bar_check("chain", false));
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.picked.is_empty(), "each wall is a pair of clicks");

    app.set_tool("beam");
    assert!(app.tool == Some("beam"));
    assert!(app.bar_choose("run.justify", 2), "Bottom");
    typed(&mut app, "run.offset", "150");
    let before = app.session.model.elements.len();
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_eq!(app.session.model.elements.len(), before + 1);
    let beam = app.session.model.elements.iter().find(|(_, e)| e.category() == "beam").map(|(id, _)| *id).unwrap_or_default();
    let json = bimcraft_core::element_json(&app.session.model, beam);
    assert_eq!((json["justify"].clone(), json["offset"].clone()), (serde_json::json!("bottom"), serde_json::json!(150.0)));
    assert!(app.picked.is_empty(), "Chain is off for the beam too");
}

#[test]
fn what_was_typed_is_kept_when_the_tool_changes_and_a_list_closes_on_a_pick() {
    let mut app = App::sample();
    app.set_tool("wall");
    typed(&mut app, "wall.height", "2700");
    app.bar_open("wall.location");
    assert_eq!(app.bar.open, Some("wall.location"));
    app.bar_open("wall.location");
    assert_eq!(app.bar.open, None);
    app.bar_open("wall.location");
    assert!(app.bar_choose("wall.location", 1));
    assert_eq!(app.bar.open, None, "picking closes it");
    app.set_tool("ceiling");
    assert_eq!(app.bar.fields.iter().map(|f| f.key).collect::<Vec<_>>(), ["ceiling.height"]);
    app.set_tool("wall");
    let height = app.bar.fields.iter().find(|f| f.key == "wall.height").map(|f| f.edit.text.clone());
    assert_eq!(height.as_deref(), Some("2700"), "what was typed comes back with the tool");
    assert!(!app.bar_choose("nope", 0) && !app.bar_check("nope", true));
}

#[test]
fn space_turns_the_door_about_to_be_placed_and_a_placed_one_starts_the_next_as_the_bar_has_it() {
    let mut app = App::sample();
    assert!(!app.flip_opening(), "no door or window in hand");
    app.set_tool("door");
    assert!(app.flip_opening() && app.bar.settings.flip);
    assert!(app.flip_opening() && !app.bar.settings.flip, "Space again turns it back");
    assert!(app.flip_opening());
    let before = app.session.model.elements.len();
    click(&mut app, P2(3000.0, 0.0));
    if app.message.is_none() {
        assert_eq!(app.session.model.elements.len(), before + 1, "a door on the wall");
        assert!(!app.bar.settings.flip, "the flip went with the door placed");
    }
    app.set_tool("door");
    app.set_tool("window");
    assert!(app.flip_opening());
}

#[test]
fn a_roof_or_floor_made_is_what_is_selected_and_the_tool_goes_back_to_modify() {
    let mut app = App::sample();
    app.set_tool("floor");
    click(&mut app, A);
    if app.message.is_none() {
        assert!(app.tool.is_none(), "the tool is put down");
        assert_eq!(app.selection.len(), 1, "what the click made is selected");
        let made = app.selection[0];
        assert_eq!(app.session.model.elements.get(made).map(|e| e.category()), Some("floor"));
        assert_eq!(app.palette.panel.subject, Subject::Element(made));
    } else {
        assert_eq!(app.tool, Some("floor"), "a refusal leaves the tool in hand");
    }
}
