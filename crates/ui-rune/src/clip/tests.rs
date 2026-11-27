use serde_json::json;

use super::*;
use crate::app::App;

#[test]
fn the_view_on_screen_is_a_view_id_or_the_default_3d_by_its_name() {
    assert_eq!(view_id(ViewKind::Plan(7)), Some(7));
    assert_eq!(view_id(ViewKind::Ceiling(8)), Some(8));
    assert_eq!(view_id(ViewKind::Section(9)), Some(9));
    assert_eq!(view_id(ViewKind::Sheet(10)), Some(10));
    assert_eq!(view_id(ViewKind::View3D(Some(4))), Some(4));
    assert_eq!(view_id(ViewKind::View3D(None)), None, "the default 3D view has no element until something makes it");
    assert_eq!(view_value(ViewKind::View3D(None)), Some(json!("{3D}")));
    assert_eq!(view_value(ViewKind::View3D(Some(4))), Some(json!(4)));
    assert_eq!(view_value(ViewKind::Plan(7)), Some(json!(7)));
    assert_eq!(view_value(ViewKind::Nothing), None);
    assert_eq!(view_value(ViewKind::Schedule("wall")), None, "a schedule holds nothing a paste could go into");
}

#[test]
fn ctrl_v_is_by_hand_in_a_plan_and_where_they_were_elsewhere() {
    assert_eq!(ctrl_v(ViewKind::Plan(1)), Mode::Manual);
    assert_eq!(ctrl_v(ViewKind::Ceiling(1)), Mode::Manual);
    assert_eq!(ctrl_v(ViewKind::Section(1)), Mode::CurrentView);
    assert_eq!(ctrl_v(ViewKind::Sheet(1)), Mode::CurrentView);
    assert_eq!(ctrl_v(ViewKind::View3D(None)), Mode::SamePlace);
    assert_eq!(ctrl_v(ViewKind::Nothing), Mode::SamePlace);
    assert!(by_hand(ViewKind::Plan(1)) && !by_hand(ViewKind::Section(1)) && !by_hand(ViewKind::View3D(None)));
}

#[test]
fn the_levels_and_the_frames_come_from_the_model() {
    let app = App::sample();
    let m = &app.session.model;
    let level = app.level.expect("a level");
    let levels = levels_of(m);
    assert!(levels.iter().any(|(id, _)| *id == level), "{levels:?}");
    assert!(frame(m, ViewKind::Plan(level)).is_some());
    assert!(matches!(frame(m, ViewKind::View3D(None)), Some(Frame::ThreeD)));
    assert!(frame(m, ViewKind::Nothing).is_none());
}

#[test]
fn a_view_specific_copy_offers_the_plans_like_the_one_it_came_from_by_name() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let made = app.session.execute("detail_line", &json!({"view": level, "start": [1000, 1000], "end": [3000, 1000]}));
    let line = made.expect("a detail line")["id"].as_u64().expect("its id");
    let m = &app.session.model;
    let copied = bimcraft_core::clipboard::copy(m, &[line], Some(level)).expect("copied");
    let clip = bimcraft_core::clipboard::read(&copied.text).expect("read");
    let like = views_like(m, &clip);
    assert!(like.iter().any(|(id, _)| *id == level), "the plan it was drawn in: {like:?}");
    assert!(like.windows(2).all(|w| w[0].1 <= w[1].1), "by name: {like:?}");
}

#[test]
fn what_is_ticked_is_the_commands_list_in_the_order_offered() {
    let mut c = Choose {
        text: String::new(),
        views: false,
        offered: vec![(5, "A".into()), (6, "B".into()), (7, "C".into())],
        picked: BTreeSet::new(),
    };
    c.toggle(7);
    c.toggle(5);
    c.toggle(99);
    assert_eq!(c.params(), json!({"mode": "levels", "levels": [5, 7]}), "a row that is not offered is not ticked");
    c.toggle(5);
    assert_eq!(c.params(), json!({"mode": "levels", "levels": [7]}), "a second tick takes it off");
    c.views = true;
    assert_eq!(c.params(), json!({"mode": "views", "views": [7]}));
}
