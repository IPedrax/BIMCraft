//! The Options Bar driven as a person drives it, headless: the tool picked, the bar over the plan, a tap on a list and on one of its options.

use bimcraft_core::model::Location;
use rune::{Env, Runtime};

use crate::app::App;
use crate::ui::view;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
}

fn tap(rt: &mut Runtime<App>, at: (f32, f32)) {
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    settle(rt);
}

#[test]
fn the_bar_is_over_the_plan_only_while_a_tool_with_settings_is_in_hand_and_a_list_picks_through_the_app() {
    let mut rt = window();
    assert!(rt.find("options-bar").is_none(), "no tool, no bar");
    rt.update(|s, _| s.set_tool("wall"));
    settle(&mut rt);
    assert!(rt.find("options-bar").is_some(), "the Wall tool has one");
    assert!(rt.find("bar-opt-wall.location-1").is_none(), "its lists are closed");
    let list = rt.find("bar-wall.location").expect("the Location Line list");
    tap(&mut rt, list);
    assert_eq!(rt.state.bar.open, Some("wall.location"));
    let option = rt.find("bar-opt-wall.location-1").expect("it opens under the bar");
    tap(&mut rt, option);
    assert_eq!(rt.state.bar.settings.location, Location::ALL[1]);
    assert_eq!(rt.state.bar.open, None);
    rt.update(|s, _| s.set_tool("wall"));
    settle(&mut rt);
    assert!(rt.find("options-bar").is_none(), "the tool put down, the bar goes");
    rt.update(|s, _| s.set_tool("move"));
    settle(&mut rt);
    assert!(rt.find("options-bar").is_none(), "a tool with nothing to set has none");
}
