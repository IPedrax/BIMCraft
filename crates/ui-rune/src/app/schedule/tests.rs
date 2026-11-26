use super::*;

fn open_room_schedule(app: &mut App) {
    assert!(app.open_view(ViewKind::Schedule("room")));
}

#[test]
fn opening_a_schedule_makes_its_grid_and_leaving_it_lets_it_go() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    assert!(app.grid.shown.rows.is_empty());
    open_room_schedule(&mut app);
    assert!(app.grid.shown.rows.len() >= 4 && app.grid.shown.note.is_none());
    assert!(app.drawing.is_none() && !app.views_busy(), "a schedule needs no drawing and no thread");
    assert!(app.status().starts_with("Schedule: Room Schedule"), "{}", app.status());
    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(app.grid.shown.rows.is_empty() && app.grid.shown.name.is_empty(), "a plan lets the grid go");
}

#[test]
fn a_click_on_a_row_picks_what_it_lists_and_the_row_is_lit_while_it_is_selected() {
    let mut app = App::sample();
    open_room_schedule(&mut app);
    let ids = app.grid.shown.ids[0].clone();
    assert!(!app.row_lit(0));
    app.pick_row(0, How::Replace);
    assert_eq!(app.selection, ids, "the row's elements are the selection");
    assert!(app.row_lit(0) && !app.row_lit(1));
    app.pick_row(1, How::Add);
    assert!(app.row_lit(0) && app.row_lit(1), "Ctrl adds the next row's");
    app.pick_row(0, How::Remove);
    assert!(!app.row_lit(0) && app.row_lit(1), "Shift takes a row's away");
    app.pick_row(999, How::Replace);
    assert!(app.row_lit(1), "a row that is not there changes nothing");

    let selected = app.selection.clone();
    let level = app.level.expect("a level");
    assert!(app.open_view(ViewKind::Plan(level)));
    assert_eq!(app.selection, selected, "the plan has the selection the schedule made");
}

#[test]
fn a_command_makes_the_grid_again() {
    let mut app = App::sample();
    assert!(app.open_view(ViewKind::Schedule("door")));
    let rows = app.grid.shown.rows.len();
    assert!(app.run("door", &serde_json::json!({"at": [1500, 4000], "type": "Single 800 x 2100"})), "refused: {:?}", app.message);
    assert_eq!(app.grid.shown.rows.len(), rows + 1, "a new door is a new row at once");
    assert!(app.undo());
    assert_eq!(app.grid.shown.rows.len(), rows);
}
