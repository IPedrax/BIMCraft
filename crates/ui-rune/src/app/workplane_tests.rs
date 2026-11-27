//! The Work Plane dialog's state through the app, with no window: the engine keeps the plane, the dialog names it, sets another and
//! dissociates it; and the Reference Plane tool makes the planes it can be set on.

use bimcraft_core::geom::P2;
use rune::TextEdit;
use serde_json::json;

use crate::app::App;
use crate::select::How;

fn click(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.release((x, y), (x, y), How::Replace);
}

/// The house with a reference plane named `Ramp` in it.
fn with_a_reference_plane() -> App {
    let mut app = App::sample();
    let made = app.session.execute("reference_plane", &json!({"start": [0, -5000], "end": [8000, -5000], "name": "Ramp"}));
    assert!(made.is_ok(), "{made:?}");
    app
}

#[test]
fn the_work_plane_is_named_as_the_button_says_and_the_dialog_opens_on_it() {
    let mut app = App::sample();
    let name = app.work_plane_name();
    assert!(name.starts_with("Level : "), "{name}");
    assert!(!app.work_plane_follows(), "a level follows nothing");
    app.open_work_plane();
    let d = app.dialogs.workplane.as_ref().expect("the dialog");
    assert!(!d.choices.is_empty() && d.choices.iter().all(|c| c.starts_with("Level : ")), "the house has levels: {:?}", d.choices);
    assert_eq!(d.name, name, "it opens on the plane that is the work plane now");
    assert!(app.dialogs.any());
    app.close_dialogs();
    assert!(!app.dialogs.any());
}

#[test]
fn an_offset_typed_moves_the_plane_through_the_engine_and_the_dialog_closes() {
    let mut app = App::sample();
    app.open_work_plane();
    app.dialogs.workplane.as_mut().expect("the dialog").offset = TextEdit::new("500");
    assert!(app.apply_work_plane(), "{:?}", app.dialogs.workplane.as_ref().map(|d| d.error.clone()));
    assert!(app.dialogs.workplane.is_none(), "taken: it closes");
    let m = &app.session.model;
    assert_eq!(m.work_plane(m.default_3d()).offset, 500.0);
    assert!(app.work_plane_name().contains(" + "), "the button says the offset: {}", app.work_plane_name());
}

#[test]
fn what_is_wrong_is_said_in_the_dialog_and_it_stays_open() {
    let mut app = App::sample();
    app.open_work_plane();
    app.dialogs.workplane.as_mut().expect("the dialog").offset = TextEdit::new("not a length");
    assert!(!app.apply_work_plane());
    let d = app.dialogs.workplane.as_ref().expect("still open");
    assert!(d.error.as_deref().is_some_and(|e| e.contains("not a length")), "{:?}", d.error);
    assert!(!app.dissociate_work_plane(), "a level follows nothing");
    assert!(app.dialogs.workplane.as_ref().is_some_and(|d| d.error.as_deref().is_some_and(|e| e.starts_with("Dissociate"))));
    app.dialogs.workplane.as_mut().expect("the dialog").offset = TextEdit::new("f".repeat(5_000));
    assert!(!app.apply_work_plane(), "a hostile offset is refused");
    assert!(app.dialogs.workplane.as_ref().is_some_and(|d| d.error.as_ref().is_some_and(|e| e.chars().count() <= 2_000)));
}

#[test]
fn a_reference_plane_is_offered_by_name_set_followed_and_dissociated() {
    let mut app = with_a_reference_plane();
    app.open_work_plane();
    let choices = app.dialogs.workplane.as_ref().expect("the dialog").choices.clone();
    assert!(choices.iter().any(|c| c == "Reference Plane : Ramp"), "{choices:?}");
    app.choose_work_plane("Reference Plane : Ramp");
    assert_eq!(app.dialogs.workplane.as_ref().map(|d| d.name.as_str()), Some("Reference Plane : Ramp"));
    assert!(app.apply_work_plane(), "{:?}", app.dialogs.workplane.as_ref().map(|d| d.error.clone()));
    assert_eq!(app.work_plane_name(), "Reference Plane : Ramp");
    assert!(app.work_plane_follows(), "on a reference plane it follows it");
    assert!(app.dissociate_work_plane(), "{:?}", app.message);
    assert!(
        !app.work_plane_follows() && !app.work_plane_name().starts_with("Reference Plane"),
        "left where it was: {}",
        app.work_plane_name()
    );
}

#[test]
fn the_reference_plane_tool_takes_two_clicks_and_makes_a_plane_the_work_plane_can_be_set_on() {
    let mut app = App::sample();
    assert!(app.session.model.rare("refplane").is_empty());
    app.set_tool("reference_plane");
    assert_eq!(app.tool, Some("reference_plane"), "the palette has it");
    click(&mut app, P2(-30000.0, -30000.0));
    assert!(app.session.model.rare("refplane").is_empty(), "one click is a start, not a plane");
    click(&mut app, P2(-26000.0, -30000.0));
    assert_eq!(app.session.model.rare("refplane").len(), 1, "{:?}", app.message);
}
