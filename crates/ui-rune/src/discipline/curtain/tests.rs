//! A curtain wall type's Edit Pattern, with no window: the grid's sizes typed, the pieces ticked and given a type, the borders and fits,
//! and the pattern on the type through the `type` command.

use bimcraft_core::model::Element;

use super::*;
use crate::app::App;

/// A curtain wall type of two columns and a row, and a wall of it far from the house; the wall's id.
fn curtain_wall(app: &mut App) -> Id {
    let pattern = json!({
        "columns": {"segments": [1000, 1500]}, "rows": {"segments": [1200]},
        "panels": [["Glazed", "Glazed"]], "vertical": [[{"type": "50 x 150"}, null]], "horizontal": [[null, null]]
    });
    let made = app.session.execute("type", &json!({"category": "wall", "name": "CW", "curtain": pattern}));
    assert!(made.is_ok(), "{made:?}");
    let level = app.level.expect("a level");
    let made = app
        .session
        .execute("wall", &json!({"points": [[-30000.0, -30000.0], [-26000.0, -30000.0]], "type": "CW", "height": 3000.0, "level": level}));
    assert!(made.is_ok(), "{made:?}");
    made.ok().and_then(|m| m["ids"][0].as_u64()).expect("a wall")
}

fn type_in(d: &mut Pattern, i: usize, text: &str) {
    if let Some(e) = d.edit_mut(i) {
        *e = TextEdit::new(text);
    }
}

fn select(d: &mut Pattern, app: &mut App, piece: Piece) {
    d.press("piece", piece.code(), app);
}

#[test]
fn it_opens_on_the_pattern_of_the_curtain_wall_selected_and_says_what_to_select_otherwise() {
    let mut app = App::sample();
    let id = curtain_wall(&mut app);
    let m = &*app.session.model;
    assert!(Pattern::open(m, &[]).err().unwrap_or_default().contains("curtain wall"));
    let plain =
        m.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(w) if w.type_name != "CW").then_some(*id)).expect("a plain wall");
    assert!(Pattern::open(m, &[plain]).is_err(), "a wall that is not a curtain wall has no pattern");
    let d = Pattern::open(m, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!((d.columns.len(), d.rows.len()), (2, 1));
    assert!(d.note.contains("1000") || d.note.contains("Module"), "{}", d.note);
    assert!(!d.sections().is_empty() && d.foot().len() == 3 && d.about().contains("CW"));
}

#[test]
fn the_grid_grows_and_shrinks_with_what_it_holds_and_the_sizes_typed_are_read_or_refused() {
    let mut app = App::sample();
    let id = curtain_wall(&mut app);
    let mut d = Pattern::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    d.press("add_column", 0, &mut app);
    assert_eq!(
        (d.columns.len(), d.l.columns.segments.len(), d.l.panels[0].len(), d.l.vertical[0].len(), d.l.horizontal[0].len()),
        (3, 3, 3, 3, 3)
    );
    assert_eq!(typed(&d.columns[2]), typed(&d.columns[1]), "the new one starts as the one before");
    d.press("add_row", 0, &mut app);
    assert_eq!((d.rows.len(), d.l.panels.len(), d.l.vertical.len(), d.l.horizontal.len()), (2, 2, 2, 2));
    d.press("remove_row", 0, &mut app);
    d.press("remove_row", 0, &mut app);
    assert_eq!(d.rows.len(), 1, "the last row stays");
    d.press("remove_column", 0, &mut app);
    type_in(&mut d, COLUMNS + 1, "2000");
    assert!(d.read().is_ok());
    let two = app.session.model.units.parse("2000").expect("a length");
    assert_eq!(d.l.columns.segments, [app.session.model.units.parse("1000").expect("a length"), two]);
    type_in(&mut d, ROWS, "tall");
    assert!(d.read().err().unwrap_or_default().contains("tall"));
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Keep);
    assert!(d.error().is_some_and(|e| e.contains("not a length")), "{:?}", d.error());
}

#[test]
fn ticked_pieces_are_given_a_panel_or_a_mullion_and_a_join_and_the_lists_say_what_they_hold() {
    let mut app = App::sample();
    let id = curtain_wall(&mut app);
    let mut d = Pattern::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    assert!(d.choice(PANEL).is_some_and(|c| c.current.is_none()), "nothing ticked, nothing held");
    select(&mut d, &mut app, Piece::Cell(0, 0));
    let list = d.choice(PANEL).expect("the panel list");
    assert_eq!(
        (list.options.first().map(String::as_str), list.current),
        (Some("None"), Some(1 + d.panels.iter().position(|p| p == "Glazed").expect("Glazed")))
    );
    select(&mut d, &mut app, Piece::Cell(0, 1));
    d.choose(PANEL, 1 + d.panels.iter().position(|p| p == "Spandrel").expect("Spandrel"), &mut app);
    assert_eq!(d.l.panels[0], [Some("Spandrel".to_string()), Some("Spandrel".to_string())], "both ticked cells take it");
    select(&mut d, &mut app, Piece::Cell(0, 1));
    d.choose(PANEL, 0, &mut app);
    assert_eq!(d.l.panels[0], [None, Some("Spandrel".to_string())], "None takes the panel out");
    select(&mut d, &mut app, Piece::Cell(0, 1));
    assert!(d.choice(PANEL).is_some_and(|c| c.current.is_none()), "a cell with none and one with a panel is a mix");
    select(&mut d, &mut app, Piece::Cell(0, 0));
    select(&mut d, &mut app, Piece::Cell(0, 1));
    select(&mut d, &mut app, Piece::Vertical(0, 1));
    assert!(
        d.sections().iter().any(|s| s.rows.iter().flatten().any(|c| matches!(c, Cell::Pick { pick, .. } if *pick == MULLION))),
        "a mullion ticked shows the mullion list"
    );
    d.choose(MULLION, 1, &mut app);
    d.choose(JOIN, 1, &mut app);
    assert_eq!(d.l.vertical[0][1], Some(MullionPick { type_name: d.mullions[0].clone(), join: Join::Mitre }));
    assert_eq!(d.choice(JOIN).and_then(|c| c.current), Some(1));
    select(&mut d, &mut app, Piece::Vertical(0, 1));
    assert!(d.edges().is_empty());
}

#[test]
fn the_borders_the_fits_and_the_rest_are_chosen_and_ok_puts_the_pattern_on_the_type() {
    let mut app = App::sample();
    let id = curtain_wall(&mut app);
    let mut d = Pattern::open(&app.session.model, &[id]).unwrap_or_else(|why| panic!("{why}"));
    let before = d.l.borders.clone();
    d.choose(BORDER, 1, &mut app);
    d.choose(BORDER + 3, 1, &mut app);
    d.choose(COLUMN_FIT, 1, &mut app);
    d.choose(ROW_JUSTIFY, 3, &mut app);
    d.choose(CONTINUOUS, 1, &mut app);
    d.press("anchor", 0, &mut app);
    let b = &d.l.borders;
    assert!(b.left.is_some() && b.top.is_some(), "the ones chosen: {b:?}");
    assert!(b.right == before.right && b.bottom == before.bottom, "and no other: {b:?} {before:?}");
    assert_eq!(
        (d.l.columns.fit, d.l.rows.justify, d.l.continuous, d.l.anchor_rows),
        (Fit::Fixed, Justify::End, Continuous::Horizontal, true)
    );
    assert_eq!(d.choice(COLUMN_FIT).and_then(|c| c.current), Some(1));
    assert_eq!(d.choice(BORDER).and_then(|c| c.current), Some(1));
    select(&mut d, &mut app, Piece::Cell(0, 0));
    d.choose(PANEL, 2, &mut app);
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Close, "{:?}", d.error());
    let made = app.session.model.types.wall.get("CW").and_then(|w| w.curtain.clone()).expect("the pattern");
    assert_eq!(
        (made.columns.fit, made.continuous, made.anchor_rows, made.borders.left.is_some()),
        (Fit::Fixed, Continuous::Horizontal, true, true)
    );
    assert!(made.panels[0][0].is_some(), "the panel picked");
    assert!(app.undo(), "one undo step");
    assert_eq!(app.session.model.types.wall.get("CW").and_then(|w| w.curtain.as_ref()).map(|l| l.columns.fit), Some(Fit::Flexible));
}

#[test]
fn pieces_are_named_by_column_letter_and_row_number_and_a_place_is_its_piece() {
    assert_eq!((letter(0), letter(25), letter(26), letter(27)), ("A".to_string(), "Z".to_string(), "AA".to_string(), "AB".to_string()));
    for p in [Piece::Cell(0, 0), Piece::Cell(3, 17), Piece::Vertical(2, 5), Piece::Horizontal(63, 63)] {
        assert_eq!(Piece::of(p.code()), p);
    }
}
