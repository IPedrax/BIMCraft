//! A camera view's own Visibility/Graphics: what it hides is hidden in its picture, and only in its picture. Driven through the
//! commands every front end drives, and tested through the PNG `render` returns.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The PNG returned to a CLI or MCP caller.
fn picture(reply: &Value) -> tiny_skia::Pixmap {
    let bytes = crate::texture::unbase64(reply["pngBase64"].as_str().unwrap()).unwrap();
    tiny_skia::Pixmap::decode_png(&bytes).unwrap()
}

/// How many pixels are something other than the white background.
fn drawn(pm: &tiny_skia::Pixmap) -> usize {
    pm.pixels().iter().filter(|p| (p.red(), p.green(), p.blue()) != (255, 255, 255)).count()
}

fn render(s: &mut Session, view: &str) -> tiny_skia::Pixmap {
    picture(&run(s, "render", json!({"view": view, "width": 400, "height": 300})))
}

/// Four walls, the default 3D view made, and a camera looking at them.
fn project() -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    // The default 3D view, made by giving it a camera too, so its graphics are a view's of their own.
    run(&mut s, "camera", json!({"view": "{3D}", "eye": [-6000, -9000, 6000], "target": [4000, 3000, 1500]}));
    run(&mut s, "camera", json!({"name": "Walk", "eye": [-5000, -4000, 1600], "target": [4000, 3000, 1200]}));
    s
}

#[test]
fn render_draws_a_camera_view_as_its_own_graphics_say() {
    let mut s = project();
    let before = render(&mut s, "Walk");
    let default_before = render(&mut s, "{3D}");
    assert!(drawn(&before) > 1000, "the camera sees the walls: {} pixels", drawn(&before));
    assert!(drawn(&default_before) > 1000, "the default 3D view sees the walls: {} pixels", drawn(&default_before));

    run(&mut s, "graphics", json!({"view": "Walk", "hide": ["walls"]}));
    let after = render(&mut s, "Walk");
    assert!(
        drawn(&after) * 10 < drawn(&before),
        "walls hidden in the camera view leave it empty: {} pixels against {} before",
        drawn(&after),
        drawn(&before)
    );
    // The default 3D view still shows them: what one view hides is that view's.
    let default_after = render(&mut s, "{3D}");
    assert_eq!(default_after.data(), default_before.data(), "the default 3D view is drawn as it was");
}

#[test]
fn render_leaves_a_camera_view_alone_when_the_default_3d_view_hides_something() {
    let mut s = project();
    let before = render(&mut s, "Walk");
    let default_before = render(&mut s, "{3D}");
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls"]}));
    let default_after = render(&mut s, "{3D}");
    assert!(
        drawn(&default_after) * 10 < drawn(&default_before),
        "the default 3D view hides its walls: {} pixels against {} before",
        drawn(&default_after),
        drawn(&default_before)
    );
    let after = render(&mut s, "Walk");
    assert_eq!(after.data(), before.data(), "the camera view still sees the walls");
}

/// The id of the 3D view called `name`.
fn view(s: &Session, name: &str) -> crate::model::Id {
    s.model.elements.iter().find_map(|(id, e)| matches!(e, crate::model::Element::View3D(v) if v.name == name).then_some(*id)).unwrap()
}

#[test]
fn the_look_of_a_3d_view_is_its_own() {
    use crate::graphics::Look;
    let mut s = project();
    let (walk, default) = (view(&s, "Walk"), s.model.default_3d());
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    run(&mut s, "graphics", json!({"view": "Walk", "hide": ["walls"]}));
    assert!(!Look::of_3d_view(&s.model, Some(walk)).shows(wall), "the camera view hides its walls");
    assert!(Look::of_3d_view(&s.model, default).shows(wall), "the default 3D view still shows them");
    // No view, or one that isn't a 3D view: as the default 3D view looks.
    assert!(Look::of_3d_view(&s.model, None).shows(wall));
    assert!(Look::of_3d_view(&s.model, Some(wall)).shows(wall));
}

#[test]
fn a_camera_view_s_scene_is_kept_only_while_it_looks_as_it_did() {
    use crate::view3d::{scene_kept, scene_kept_in};
    let mut s = project();
    let walk = view(&s, "Walk");
    let before = std::sync::Arc::clone(&s.model);
    run(&mut s, "graphics", json!({"view": "Walk", "hide": ["walls"]}));
    assert!(!scene_kept_in(&before, &s.model, Some(walk)), "the camera view hides walls now: its mesh is made again");
    assert!(scene_kept(&before, &s.model), "the default 3D view looks as it did");
    let before = std::sync::Arc::clone(&s.model);
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls"]}));
    assert!(scene_kept_in(&before, &s.model, Some(walk)), "the camera view looks as it did");
    assert!(!scene_kept(&before, &s.model), "the default 3D view hides walls now");
}
