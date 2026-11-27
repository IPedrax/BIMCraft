//! Quantities, with no window: the pages, what each asks of its command, what the fields say, the schedules and the file.

use super::*;
use crate::app::App;

/// Types `text` into box `i`.
fn type_in(d: &mut Quantities, i: usize, text: &str) {
    if let Some(e) = d.edit_mut(i) {
        *e = TextEdit::new(text);
    }
}

fn open(page: Page, app: &mut App, selection: &[Id]) -> Quantities {
    let mut d = Quantities::open(page, &app.session.model, selection);
    d.opened(app);
    d
}

#[test]
fn a_page_other_than_the_cut_plan_is_calculated_when_it_opens_and_the_plan_waits_for_a_stock_length() {
    let mut app = App::sample();
    for page in [Page::Materials, Page::List] {
        let d = open(page, &mut app, &[]);
        assert!(d.error().is_none(), "{page:?}: {:?}", d.error());
        assert!(d.reply(page).is_some(), "{page:?} is calculated at once");
        assert!(!d.sections().is_empty());
    }
    let mut d = open(Page::Plan, &mut app, &[]);
    assert!(d.reply(Page::Plan).is_none() && d.error().is_none(), "a purchase waits");
    assert_eq!(d.press("calculate", 0, &mut app), Outcome::Keep);
    assert!(d.error().is_some_and(|e| e.contains("stock bar")), "{:?}", d.error());
    type_in(&mut d, STOCK, "12000");
    d.press("calculate", 0, &mut app);
    assert!(d.error().is_none(), "{:?}", d.error());
    assert!(d.reply(Page::Plan).is_some(), "the plan is made once there is a stock length");
}

#[test]
fn tabs_change_the_page_and_calculate_what_it_has_not_got() {
    let mut app = App::sample();
    let mut d = open(Page::List, &mut app, &[]);
    assert_eq!((d.tab(), d.tabs().len()), (1, 3));
    d.press("tab", 0, &mut app);
    assert_eq!((d.page(), d.tab()), (Page::Materials, 0));
    assert!(d.reply(Page::Materials).is_some(), "calculated when its tab was chosen");
    d.press("tab", 2, &mut app);
    assert_eq!(d.page(), Page::Plan);
    assert!(d.reply(Page::Plan).is_none(), "the plan is still a purchase");
    d.press("tab", 9, &mut app);
    assert_eq!(d.page(), Page::Plan, "a tab there is not is nobody's");
}

#[test]
fn the_fields_become_the_parameters_the_level_and_the_selection_narrow_the_members() {
    let mut app = App::sample();
    let wall = app
        .session
        .model
        .elements
        .iter()
        .find_map(|(id, e)| matches!(e, bimcraft_core::model::Element::Wall(_)).then_some(*id))
        .expect("a wall");
    let mut d = open(Page::List, &mut app, &[wall]);
    assert_eq!(d.params(Page::List), Ok(json!({"ids": [wall]})), "the selection is the bars asked about");
    d.press("selected", 0, &mut app);
    assert_eq!(d.params(Page::List), Ok(json!({})));
    let levels = d.choice(0).expect("the level list");
    assert_eq!(levels.options.first().map(String::as_str), Some("Whole project"));
    assert!(levels.options.len() > 1 && levels.current == Some(0));
    d.choose(0, 1, &mut app);
    assert_eq!(d.params(Page::List).map(|p| p["level"].clone()), Ok(json!(levels.options[1])));
    d.choose(0, 0, &mut app);
    assert_eq!(d.params(Page::List), Ok(json!({})), "Whole project is no level");
    type_in(&mut d, COIL, "x");
    assert!(d.params(Page::List).err().unwrap_or_default().contains("Coil up to"));
    type_in(&mut d, COIL, "12,5");
    type_in(&mut d, STOCK, "12000");
    let stock = app.session.model.units.parse("12000").expect("a length");
    assert_eq!(d.params(Page::List), Ok(json!({"coil_to": 12.5, "stock_length": stock})));
    type_in(&mut d, KERF, "4");
    let kerf = app.session.model.units.parse("4").expect("a length");
    assert_eq!(d.params(Page::Plan).map(|p| p["kerf"].clone()), Ok(json!(kerf)));
    assert!(d.params(Page::Materials).is_ok_and(|p| p.get("coil_to").is_none()), "the materials page has no coil");
}

#[test]
fn create_schedules_makes_them_in_one_step_and_the_file_is_written_where_the_dialog_asked() {
    let mut app = App::sample();
    let mut d = open(Page::Materials, &mut app, &[]);
    let before = app.session.model.elements.len();
    assert_eq!(d.press("make", 0, &mut app), Outcome::Close, "{:?}", d.error());
    assert!(app.session.model.elements.len() > before, "a schedule was made");
    assert!(app.message.as_deref().is_some_and(|m| m.contains("Materials Summary")), "{:?}", app.message);
    let mut d = open(Page::List, &mut app, &[]);
    assert_eq!(d.press("make", 0, &mut app), Outcome::Close, "{:?}", d.error());
    let mut plan = open(Page::Plan, &mut app, &[]);
    assert_eq!(plan.press("make", 0, &mut app), Outcome::Keep, "a cut plan has no schedules");

    let mut d = open(Page::Materials, &mut app, &[]);
    let path = std::env::temp_dir().join(format!("bimcraft-rune-quantities-{}.csv", std::process::id()));
    assert_eq!(d.saved("export", path.clone(), &mut app), Outcome::Keep);
    assert!(d.error().is_none(), "{:?}", d.error());
    assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 0), "the file was written");
    assert!(std::fs::remove_file(&path).is_ok());
    assert!(d.foot().iter().any(|f| f.action == "export" && f.file == Some("quantities.csv") && f.on));
}
