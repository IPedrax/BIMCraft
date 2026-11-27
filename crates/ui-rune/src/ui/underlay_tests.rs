//! The underlay on the plan canvas, driven headless: the layer is there while the plan on screen has one and gone when it is taken off.

use rune::{Env, Runtime};
use serde_json::json;

use super::view;
use crate::app::App;

#[test]
fn the_plan_draws_an_underlay_layer_while_it_has_one() {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(rt.find("plan-underlay").is_none(), "none to draw");
    let level = rt.state.level.expect("a level");
    let copy =
        rt.state.session.execute("duplicate_view", &json!({"view": level, "mode": "duplicate"})).expect("a plan view")["id"].as_u64();
    let copy = copy.expect("its id");
    rt.update(|s, _| {
        s.show_as_underlay(copy);
    });
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(rt.find("plan-underlay").is_some(), "{:?}", rt.state.message);
    rt.update(|s, _| {
        s.remove_underlay();
    });
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(rt.find("plan-underlay").is_none(), "taken off");
}
