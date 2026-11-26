use bimcraft_core::engine::Session;

use super::*;
use crate::app::App;

fn room_schedule(app: &App) -> Shown {
    make(&app.session.model, ViewKind::Schedule("room"))
}

#[test]
fn the_room_schedule_is_a_grid_of_text_with_the_elements_each_row_lists() {
    let app = App::sample();
    let s = room_schedule(&app);
    assert!(s.note.is_none(), "{:?}", s.note);
    assert_eq!(s.name, "Room Schedule");
    assert!(s.columns.len() >= 2 && s.rows.len() >= 4, "the four rooms: {} rows", s.rows.len());
    assert!(s.rows.iter().all(|r| r.len() == s.columns.len()), "a cell for every column of every row");
    assert!(s.rows.iter().flatten().any(|c| c == "Living"), "the room's name is a cell");
    assert_eq!(s.ids.len(), s.rows.len());
    assert!(
        s.ids.iter().all(|ids| !ids.is_empty() && ids.iter().all(|id| app.session.model.elements.get(*id).is_some())),
        "each row lists elements the model has"
    );
    assert!(s.columns.iter().any(|h| h.numeric) && s.columns.iter().any(|h| !h.numeric));
    assert!(!s.cut);
}

#[test]
fn a_row_is_lit_while_what_it_lists_is_selected() {
    let app = App::sample();
    let s = room_schedule(&app);
    let id = s.ids[1][0];
    assert!(lists_selected(&s, 1, &[id]));
    assert_eq!(lists_selected(&s, 0, &[id]), s.ids[0].contains(&id), "another row only when it lists it too");
    assert!(!lists_selected(&s, 1, &[]) && !lists_selected(&s, 999, &[id]), "nothing selected, or no such row");
}

#[test]
fn a_schedule_with_no_rows_or_no_such_view_says_so_and_has_no_grid() {
    let empty = Session::new();
    let s = make(&empty.model, ViewKind::Schedule("room"));
    assert!(s.rows.is_empty() && s.columns.is_empty());
    assert!(s.note.as_deref().is_some_and(|n| n.starts_with("No rows yet")), "{:?}", s.note);

    let app = App::sample();
    let level = app.levels[0].0;
    for other in [ViewKind::Table(level), ViewKind::Table(7), ViewKind::Sheet(level), ViewKind::Nothing] {
        let s = make(&app.session.model, other);
        assert!(s.rows.is_empty() && s.note.is_some(), "{other:?} is no schedule this grid draws");
    }
}

#[test]
fn the_grid_is_made_again_for_another_model_or_view_and_kept_for_the_same() {
    let mut app = App::sample();
    let mut grid = Grid::default();
    grid.rebuild(&app.session.model, ViewKind::Schedule("door"));
    let first = grid.shown.rows.len();
    assert!(first >= 2, "the sample house has doors");
    grid.table.select("1");
    grid.rebuild(&app.session.model, ViewKind::Schedule("door"));
    assert!(grid.table.is_selected("1"), "the same schedule of the same model is kept as it is");

    app.session.execute("door", &serde_json::json!({"at": [1500, 4000], "type": "Single 800 x 2100"})).expect("a door");
    grid.rebuild(&app.session.model, ViewKind::Schedule("door"));
    assert_eq!(grid.shown.rows.len(), first + 1, "a new door is a new row");
    assert!(grid.table.is_selected("1"), "and the state stays for the same schedule");

    grid.rebuild(&app.session.model, ViewKind::Schedule("window"));
    assert!(!grid.table.is_selected("1"), "another schedule starts with a clean state");
    assert_eq!(grid.shown.name, "Window Schedule");
    grid.clear();
    assert!(grid.shown.rows.is_empty() && grid.shown.name.is_empty());
}
