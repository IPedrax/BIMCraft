//! Underlays through commands and their output: what a view saves, the pixels it draws, what snaps to it.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Id, Model};
use crate::render::{self, Rgb, View};
use crate::snap::{self, Kind, Reach, Snaps};
use crate::underlay;

/// Level 1 (100000) with a box of walls, and Level 2 (100001) above with a short wall of its own.
fn floors() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "level": "Level 1"}
wall {"points": [[2000,1000],[6000,1000]], "level": "Level 2"}"#,
    )
    .unwrap();
    s
}

fn run(s: &mut Session, p: Value) -> Value {
    s.execute("underlay", &p).unwrap_or_else(|e| panic!("underlay {p}: {e}"))
}

fn level(s: &Session, name: &str) -> Id {
    s.model.levels().into_iter().find(|(_, l)| l.name == name).map(|(id, _)| id).unwrap()
}

const VIEW: View = View { scale: 0.05, cx: 4000.0, cy: 3000.0, width: 600, height: 400, angle: 0.0 };

/// The colour of the underlay's own PNG at model point `p` of the target's view.
fn under_at(m: &Model, target: Id, p: P2) -> Rgb {
    let d = underlay::draw(m, target, VIEW).expect("an underlay").into_drawing();
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    let (x, y) = VIEW.px(p);
    let c = pm.pixel(x as u32, y as u32).unwrap();
    Rgb(c.red(), c.green(), c.blue())
}

/// In the underlay's orange: its lines, or the wash of it its fills take.
fn orange(c: Rgb) -> bool {
    c != render::PAPER && c.0 > 200 && i32::from(c.0) - i32::from(c.2) > 40
}

#[test]
fn a_view_shows_another_under_it_and_keeps_it_with_the_view() {
    let mut s = floors();
    let (one, two) = (level(&s, "Level 1"), level(&s, "Level 2"));
    let r = run(&mut s, json!({"view": "Level 2", "source": "Level 1"}));
    assert_eq!(r["underlay"]["source"], json!(one));
    assert_eq!(r["underlay"]["color"], json!("#e67e22"));
    assert_eq!(r["underlay"]["hide_annotation"], json!(true));
    let u = underlay::of(&s.model, two).unwrap();
    assert_eq!(u.view, s.model.guid(one), "held by GUID");
    // Saved with the view, and read back.
    let text = serde_json::to_string(&*s.model).unwrap();
    assert!(text.contains("\"underlay\""));
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(underlay::of(&back, two), Some(u));
    // Off keeps the settings; on brings them back; null takes it away.
    run(&mut s, json!({"view": two, "on": false, "color": "#2080ff"}));
    assert!(underlay::shown(&s.model, two).is_none());
    assert_eq!(underlay::of(&s.model, two).unwrap().color, [0x20, 0x80, 0xff]);
    run(&mut s, json!({"view": two, "on": true}));
    assert!(underlay::shown(&s.model, two).is_some());
    run(&mut s, json!({"view": two, "source": null}));
    assert!(underlay::of(&s.model, two).is_none());
    // Undone, it is back.
    s.execute("undo", &json!({})).unwrap();
    assert!(underlay::of(&s.model, two).is_some());
}

#[test]
fn what_an_underlay_cannot_be_is_refused_with_what_to_do() {
    let mut s = floors();
    let e = s.execute("underlay", &json!({"view": "Level 1", "source": "Level 1"})).unwrap_err();
    assert!(e.contains("its own underlay"), "{e}");
    let e = s.execute("underlay", &json!({"view": "Level 1", "color": "#ff0000"})).unwrap_err();
    assert!(e.contains("give it a `source` first"), "{e}");
    let e = s.execute("underlay", &json!({"view": "Level 1", "source": "Level 2", "colour": "red"})).unwrap_err();
    assert!(e.contains("isn't one of its parameters"), "{e}");
    let e = s.execute("underlay", &json!({"view": "Level 1", "source": "Level 2", "color": "red"})).unwrap_err();
    assert!(e.contains("#rrggbb"), "{e}");
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let e = s.execute("underlay", &json!({"view": "Level 1", "source": wall})).unwrap_err();
    assert!(e.contains("not a view") || e.contains("isn't a plan"), "{e}");
    let e = s.execute("underlay", &json!({"view": "Level 1", "source": "Level 2", "rotate": 90, "about": [9e8, 9e8]})).unwrap_err();
    assert!(e.contains("out of reach"), "{e}");
    assert!(underlay::of(&s.model, level(&s, "Level 1")).is_none(), "a refused command leaves the model as it was");
}

#[test]
fn it_is_drawn_in_its_colour_where_the_internal_origin_puts_it_and_moves_and_turns() {
    let mut s = floors();
    let two = level(&s, "Level 2");
    run(&mut s, json!({"view": two, "source": "Level 1"}));
    // Level 1's wall along y = 0, drawn where it is, in orange; nothing where no wall of it is.
    assert!(orange(under_at(&s.model, two, P2(4000.0, 0.0))), "{:?}", under_at(&s.model, two, P2(4000.0, 0.0)));
    assert_eq!(under_at(&s.model, two, P2(4000.0, 3000.0)), render::PAPER);
    // Nothing of it is picked.
    let u = underlay::draw(&s.model, two, VIEW).unwrap();
    assert!(u.drawing.owners.iter().all(Option::is_none));
    assert!(u.drawing.pick(VIEW.px(P2(4000.0, 0.0)), 6.0).is_none());
    // Moved 1 m north, the wall is at y = 1000.
    let r = run(&mut s, json!({"view": two, "move": [0, 1000]}));
    assert_eq!(r["underlay"]["offset"], json!([0.0, 1000.0]));
    assert!(orange(under_at(&s.model, two, P2(4000.0, 1000.0))));
    assert_eq!(under_at(&s.model, two, P2(4000.0, 0.0)), render::PAPER);
    // Reset, then turned 90° about the origin: the wall along x runs along y, at x = 0 and y = 4000.
    run(&mut s, json!({"view": two, "reset": true}));
    let r = run(&mut s, json!({"view": two, "rotate": 90}));
    assert_eq!(r["underlay"]["angle"], json!(90.0));
    assert!(orange(under_at(&s.model, two, P2(0.0, 4000.0))));
    assert_eq!(under_at(&s.model, two, P2(4000.0, 0.0)), render::PAPER);
    // About another point, it moves too: a quarter turn about (1000, 0) back to where it started, less the turn.
    run(&mut s, json!({"view": two, "reset": true}));
    run(&mut s, json!({"view": two, "rotate": 180, "about": [4000, 3000]}));
    let u = underlay::of(&s.model, two).unwrap();
    assert_eq!((u.offset, u.angle), (P2(8000.0, 6000.0), 180.0));
    // A box turned half round its middle is itself: the wall along y = 0 is there again.
    assert!(orange(under_at(&s.model, two, P2(4000.0, 0.0))));
}

#[test]
fn fills_and_annotation_are_shown_as_it_says() {
    let mut s = floors();
    s.execute("text", &json!({"text": "NOTE", "at": [3000, 3000], "view": "Level 1"})).unwrap();
    let two = level(&s, "Level 2");
    run(&mut s, json!({"view": two, "source": "Level 1"}));
    let lettering =
        |s: &Session| underlay::draw(&s.model, two, VIEW).unwrap().drawing.parts.iter().filter(|p| **p == render::Part::Text).count();
    assert_eq!(lettering(&s), 0, "annotation is hidden unless asked for");
    run(&mut s, json!({"view": two, "hide_annotation": false}));
    assert!(lettering(&s) > 0, "the note's lettering comes");
    // The wall's poché is a wash of the colour, between its faces; without fills, paper.
    let mid = P2(4000.0, 0.0);
    let filled = |s: &Session| {
        let d = underlay::draw(&s.model, two, VIEW).unwrap().drawing;
        d.prims.iter().zip(&d.parts).any(|(p, part)| matches!(p, render::Prim::Fill { .. }) && *part != render::Part::Text)
    };
    assert!(filled(&s));
    let c = under_at(&s.model, two, mid);
    assert!(c != render::PAPER, "{c:?}");
    run(&mut s, json!({"view": two, "fills": false}));
    assert!(!filled(&s));
}

#[test]
fn a_cropped_underlay_shows_only_its_crop() {
    let mut s = floors();
    s.execute("crop", &json!({"view": "Level 1", "region": [[-500, -500], [4000, 6500]]})).unwrap();
    let two = level(&s, "Level 2");
    run(&mut s, json!({"view": two, "source": "Level 1"}));
    assert!(orange(under_at(&s.model, two, P2(2000.0, 0.0))));
    assert_eq!(under_at(&s.model, two, P2(6000.0, 0.0)), render::PAPER, "past its crop");
}

#[test]
fn sections_show_plans_and_each_other() {
    let mut s = floors();
    let a = s.execute("section", &json!({"start": [-1000, 3000], "end": [9000, 3000]})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("section", &json!({"start": [-1000, 2000], "end": [9000, 2000]})).unwrap()["id"].as_u64().unwrap();
    run(&mut s, json!({"view": b, "source": a}));
    // Both along x from x = -1000: the internal origin falls at 1000 along each, so walls line up, its picture in colour.
    let view = render::section_fit(&s.model, b, 600, 400);
    let u = underlay::draw(&s.model, b, view).unwrap();
    let (_, picture) = u.picture.as_ref().expect("a section's picture");
    assert!(picture.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0), "it shows something");
    assert!(picture.rgba.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).all(|px| px[..3] == underlay::COLOR), "all in its colour");
    // A plan under a section, and a section under a plan.
    run(&mut s, json!({"view": b, "source": "Level 1"}));
    assert!(underlay::draw(&s.model, b, view).is_some_and(|u| !u.drawing.prims.is_empty()));
    run(&mut s, json!({"view": "Level 1", "source": a}));
    assert!(underlay::draw(&s.model, level(&s, "Level 1"), VIEW).is_some());
    assert_eq!(underlay::origin(&s.model, a), Some(P2(1000.0, 0.0)));
}

#[test]
fn snaps_land_on_its_corners() {
    let mut s = floors();
    let two = level(&s, "Level 2");
    run(&mut s, json!({"view": two, "source": "Level 1", "move": [37, 0]}));
    let u = underlay::draw(&s.model, two, VIEW).unwrap();
    let reach = Reach { point: 400.0, line: 120.0 };
    let near = P2(8037.0 + 150.0, 6000.0 - 120.0);
    let plain = snap::snap(&s.model, two, near, reach, &[], false, &Snaps::default());
    assert_ne!(plain.at, P2(8037.0, 6000.0), "Level 2 has nothing there");
    let found = snap::snap_with(&s.model, two, near, reach, &[], false, &Snaps::default(), &u.segments());
    // The wall's outer corner (it is 200 thick, centred on the line), or its centre line's, moved 37 east.
    assert!(matches!(found.kind, Kind::Endpoint | Kind::Intersection), "{found:?}");
    assert!(found.at.0 > 8000.0 && (found.at.0 - 37.0).fract().abs() < 1e-6, "{found:?}");
    assert_eq!(found.source, None, "an underlay is no element's");
}

#[test]
fn a_duplicate_has_it_and_a_deleted_source_leaves_none() {
    let mut s = floors();
    let two = level(&s, "Level 2");
    let plan = s.execute("duplicate_view", &json!({"view": "Level 1"})).unwrap()["id"].as_u64().unwrap();
    run(&mut s, json!({"view": two, "source": plan, "color": [10, 20, 30]}));
    let copy = s.execute("duplicate_view", &json!({"view": "Level 1"})).unwrap()["id"].as_u64().unwrap();
    assert!(underlay::of(&s.model, copy).is_none());
    run(&mut s, json!({"view": plan, "source": "Level 2"}));
    let again = s.execute("duplicate_view", &json!({"view": plan})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(underlay::of(&s.model, again), underlay::of(&s.model, plan), "a duplicate shows what its source shows");
    // Its view deleted, the underlay goes with it.
    s.execute("delete", &json!({"ids": [plan]})).unwrap();
    assert!(underlay::of(&s.model, two).is_none());
    assert!(underlay::draw(&s.model, two, VIEW).is_none());
    run(&mut s, json!({"view": two, "source": "Level 1"}));
    assert!(underlay::shown(&s.model, two).is_some());
}

#[test]
fn a_plan_starts_with_the_level_below() {
    let s = floors();
    let (one, two) = (level(&s, "Level 1"), level(&s, "Level 2"));
    assert_eq!(underlay::default_source(&s.model, two), Some(one));
    assert_eq!(underlay::default_source(&s.model, one), Some(two));
    assert_eq!(underlay::candidates(&s.model, two), vec![one]);
}
