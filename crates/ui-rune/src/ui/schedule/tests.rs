//! A schedule opened in the window, driven headless: the grid's rows, the row a click picks, and the row lit while it is selected.

use rune::{Env, PhysicalInput, Mods, Runtime};

use crate::app::App;
use crate::ui::view;
use crate::views::ViewKind;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt.update(|s, _| {
        s.open_view(ViewKind::Schedule("room"));
    });
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

/// The middle of row `i` of the grid: its element is keyed by its index, and so are its cells, so it is found by its role.
fn row(rt: &Runtime<App>, i: usize) -> (f32, f32) {
    let e = rt.elements().into_iter().find(|e| e.role == "row" && e.key.as_deref() == Some(i.to_string().as_str())).expect("the row");
    (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
}

#[test]
fn a_schedule_is_a_grid_with_a_row_for_each_of_its_rows() {
    let rt = window();
    assert!(rt.find("schedule").is_some() && rt.find("plan").is_none() && rt.find("view-pending").is_none());
    let rows = rt.elements().into_iter().filter(|e| e.role == "row").count();
    assert!(rows >= 4, "the four rooms are rows: {rows}");
    assert!(rt.elements().iter().any(|e| e.text.as_deref() == Some("Living")), "a room's name is a cell");
    assert!(rt.elements().iter().any(|e| e.text.as_deref() == Some("Room Schedule")), "the schedule's name heads the page");
}

#[test]
fn a_click_on_a_row_selects_what_it_lists_and_the_plan_has_it_selected_too() {
    let mut rt = window();
    let first = row(&rt, 0);
    tap(&mut rt, first);
    assert!(!rt.state.selection.is_empty() && rt.state.row_lit(0), "the row's rooms are selected: {:?}", rt.state.selection);
    let ids = rt.state.grid.shown.ids[0].clone();
    assert_eq!(rt.state.selection, ids);

    let second = row(&rt, 1);
    rt.physical_input(PhysicalInput::Modifiers(Mods { cmd: true, ..Mods::default() }));
    tap(&mut rt, second);
    assert!(rt.state.row_lit(0) && rt.state.row_lit(1), "Ctrl adds the next row's");
}
