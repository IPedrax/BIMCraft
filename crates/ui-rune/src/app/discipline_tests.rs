//! The discipline dialogs through the app, with no window: opening, the boxes typed in, a press, and the dialog closing.

use bimcraft_core::model::Element;

use crate::app::App;
use crate::discipline::{Cell, Kind};

/// The place of the box named `name` on the open dialog.
fn edit_named(app: &App, name: &str) -> usize {
    let open = app.dialogs.discipline.as_ref().expect("a dialog open");
    let cells = open.dialog.sections().into_iter().flat_map(|s| s.rows).flatten();
    cells
        .find_map(|c| if let Cell::Field { edit, name: n, .. } = c { (n == name).then_some(edit) } else { None })
        .expect("a box with that name")
}

#[test]
fn a_dialog_that_cannot_open_says_why_in_the_status_bar_and_one_that_opens_is_a_dialog_open() {
    let mut app = App::sample();
    assert!(!app.open_discipline(Kind::Reinforce), "nothing selected to reinforce");
    assert!(app.message.as_deref().is_some_and(|m| m.contains("Select")) && !app.dialogs.any(), "{:?}", app.message);
    assert!(app.open_discipline(Kind::Materials));
    assert!(app.dialogs.any() && app.dialogs.discipline.is_some());
    assert!(!app.discipline_press("close", 0), "Close closes it");
    assert!(app.dialogs.discipline.is_none() && !app.dialogs.any());
}

#[test]
fn what_is_typed_in_a_box_is_read_by_the_dialog_and_a_press_runs_the_command() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let made = app.session.execute("beam", &serde_json::json!({"points": [[-30000.0, -30000.0], [-26000.0, -30000.0]], "level": level}));
    assert!(made.is_ok(), "{made:?}");
    let beam = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Beam(_))).map(|(id, _)| *id).max().expect("a beam");
    app.select(vec![beam]);
    assert!(app.open_discipline(Kind::Reinforce), "{:?}", app.message);
    for (name, text) in [
        ("Bottom bars: Count", "2"),
        ("Bottom bars: Diameter", "16"),
        ("Top bars: Count", "2"),
        ("Top bars: Diameter", "12.5"),
        ("Stirrups: Diameter", "8"),
        ("Stirrups: Spacing", "200"),
    ] {
        let i = edit_named(&app, name);
        app.dialogs.discipline_edit(i).text = text.to_string();
    }
    let rebars = |app: &App| app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Rebar(_))).count();
    assert_eq!(rebars(&app), 0);
    assert!(
        !app.discipline_press("ok", 0),
        "OK closes it once the bars are made: {:?}",
        app.dialogs.discipline.as_ref().and_then(|o| o.dialog.error().map(String::from))
    );
    assert!(rebars(&app) > 0 && app.dialogs.discipline.is_none());
    assert!(app.undo(), "the bars are one undo step");
    assert_eq!(rebars(&app), 0);
}

#[test]
fn find_lists_the_dialogs_and_a_row_opens_one() {
    let hits = crate::finder::find("steel list", &[]);
    assert!(hits.iter().any(|h| h.found == crate::finder::Found::Discipline(Kind::SteelList)), "{hits:?}");
    let mut app = App::sample();
    app.toggle_find();
    app.dialogs.find_edit().text = "cut plan".to_string();
    app.pick_found(0);
    assert!(
        app.dialogs.find.is_none() && app.dialogs.discipline.as_ref().is_some_and(|o| o.dialog.tab() == 2),
        "Find closed and the Cut Plan is open"
    );
}
