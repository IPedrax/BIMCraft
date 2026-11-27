//! The Work Plane button and dialog driven as a person drives them, headless: the top bar names the plane in a 3D view only, a press opens
//! the dialog, and OK sets what it names.

use rune::{Env, Runtime};

use super::view;
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

/// A dialog opens on a spring and takes presses once it has come to rest.
fn tap(rt: &mut Runtime<App>, key: &str) {
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let at = rt.find(key).unwrap_or_else(|| panic!("an element keyed {key}"));
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

#[test]
fn the_top_bar_names_the_work_plane_in_a_3d_view_only_and_its_button_opens_the_dialog_that_ok_closes() {
    let mut rt = window();
    assert!(rt.find("work-plane").is_none(), "a plan has no work plane button");
    rt.update(|s, _| {
        s.open_named("3d");
    });
    for _ in 0..3 {
        rt.frame(0.0);
    }
    tap(&mut rt, "work-plane");
    assert!(rt.state.dialogs.workplane.is_some(), "the button opens the dialog");
    for key in ["work-plane-current", "work-plane-offset", "work-plane-dissociate", "work-plane-ok", "work-plane-close"] {
        assert!(rt.find(key).is_some(), "{key} is in the dialog");
    }
    tap(&mut rt, "work-plane-ok");
    assert!(rt.state.dialogs.workplane.is_none(), "OK sets the plane it opened on and closes: {:?}", rt.state.message);
    tap(&mut rt, "work-plane");
    tap(&mut rt, "work-plane-close");
    assert!(rt.state.dialogs.workplane.is_none() && rt.find("work-plane-ok").is_none());
}
