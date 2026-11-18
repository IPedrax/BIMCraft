use serde_json::{Value, json};

use crate::{engine::Session, model::Element};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

#[test]
fn vertical_tools_dimensions_follow_levels_and_view_owned_lines_through_history() {
    let mut s = Session::new();
    let lower = s.model.levels()[0].0;
    let upper = run(&mut s, "level", json!({"name":"Upper", "elevation":3000}))["id"].clone();
    let view = run(&mut s, "section", json!({"name":"Rotated", "start":[1000,2000], "end":[4000,6000]}))["id"].clone();
    let dim = run(&mut s, "dimension", json!({"view":view,"refs":[{"id":lower},{"id":upper}],"at":[1000,1500]}));
    assert_eq!(dim["values"], json!([3000.0]));
    let a = run(&mut s, "detail_line", json!({"view":view,"start":[0,0],"end":[1000,0]}))["id"].clone();
    let b = run(&mut s, "detail_line", json!({"view":view,"start":[0,500],"end":[1000,500]}))["id"].clone();
    let detail = run(&mut s, "dimension", json!({"view":view,"refs":[{"id":a},{"id":b}],"at":[500,250]}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"view":view,"ids":[b],"by":[0,250]}));
    let values = |s: &Session| match s.model.elements.get(detail).unwrap() {
        Element::Dimension(d) => s.model.dimension_values(d),
        _ => panic!("dimension missing"),
    };
    assert_eq!(values(&s), vec![750.0]);
    run(&mut s, "undo", json!({}));
    assert_eq!(values(&s), vec![500.0]);
    run(&mut s, "redo", json!({}));
    assert_eq!(values(&s), vec![750.0]);
}

#[test]
fn vertical_tools_move_refuses_model_xy_and_foreign_annotations_atomically() {
    let mut s = Session::new();
    let view = run(&mut s, "section", json!({"start":[1000,2000],"end":[4000,6000]}))["id"].clone();
    let line = run(&mut s, "detail_line", json!({"start":[0,0],"end":[1000,0]}))["id"].clone();
    let before = serde_json::to_value(&*s.model).unwrap();
    assert!(s.execute("move", &json!({"view":view,"ids":[line],"by":[0,500]})).is_err());
    assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
}

#[test]
fn vertical_tools_rotated_elevation_moves_openings_in_host_coordinates_and_rolls_back() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[1000,2000],[7000,10000]]}))["ids"][0].clone();
    let window = run(&mut s, "window", json!({"wall":wall,"offset":3000,"sill":900}))["id"].as_u64().unwrap();
    let view = run(&mut s, "elevation", json!({"name":"Oblique","start":[1800,1400],"end":[7800,9400]}))["id"].clone();
    let plane = crate::drawing_plane::DrawingPlane::of(&s.model, view.as_u64().unwrap()).unwrap();
    assert_eq!(plane.vector(crate::geom::P2(500.0, 200.0)), [300.0, 400.0, 200.0]);
    let start = plane.point(crate::geom::P2(123.0, 456.0));
    let projected = plane.project(start);
    assert!((projected.0 - 123.0).abs() < 1e-8 && (projected.1 - 456.0).abs() < 1e-8);
    run(&mut s, "move", json!({"view":view,"ids":[window],"by":[500,200]}));
    let get = |s: &Session| match s.model.elements.get(window).unwrap() {
        Element::Window(o) => (o.offset, o.sill, o.host),
        _ => panic!(),
    };
    assert_eq!((get(&s).0, get(&s).1), (3500.0, 1100.0));
    let moved = get(&s);
    run(&mut s, "undo", json!({}));
    assert_eq!((get(&s).0, get(&s).1), (3000.0, 900.0));
    run(&mut s, "redo", json!({}));
    assert_eq!(get(&s), moved);
    let before = serde_json::to_value(&*s.model).unwrap();
    assert!(s.execute("move", &json!({"view":view,"ids":[window,wall],"by":[100,100]})).is_err());
    assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
    let cross = run(&mut s, "section", json!({"start":[0,0],"end":[10000,0]}))["id"].clone();
    assert!(s.execute("move", &json!({"view":cross,"ids":[window],"by":[500,0]})).is_err());
}

#[test]
fn vertical_tools_grid_projection_details_trim_persistence_and_drawing() {
    use crate::{model::Model, render};
    let mut s = Session::new();
    let view = run(&mut s, "section", json!({"name":"Rotated", "start":[1000,2000],"end":[4000,6000]}))["id"].as_u64().unwrap();
    let a = run(&mut s, "grid", json!({"start":[200,2600],"end":[1800,1400]}))["id"].clone();
    let b = run(&mut s, "grid", json!({"start":[2000,5000],"end":[3600,3800]}))["id"].clone();
    let dim = run(&mut s, "dimension", json!({"view":view,"refs":[{"id":a},{"id":b}],"at":[1500,1000]}));
    assert_eq!(dim["values"], json!([3000.0]));
    let id = dim["id"].as_u64().unwrap();
    assert!(s.execute("dimension_value", &json!({"id":id,"value":4000})).unwrap_err().contains("measure only"));
    assert!(s.execute("dimension_constraint", &json!({"id":id,"equal":true})).unwrap_err().contains("not supported yet"));
    let x = run(&mut s, "detail_line", json!({"view":view,"start":[0,500],"end":[3000,500]}))["id"].clone();
    let y = run(&mut s, "detail_line", json!({"view":view,"start":[2000,0],"end":[2000,2000]}))["id"].clone();
    run(&mut s, "trim_extend", json!({"boundary":y,"ids":[x],"keep":[[500,500]]}));
    if let Element::DetailLine(l) = s.model.elements.get(x.as_u64().unwrap()).unwrap() {
        assert_eq!(l.curve.end(), crate::geom::P2(2000.0, 500.0));
    } else {
        panic!();
    }
    let text = serde_json::to_string(&*s.model).unwrap();
    let (reopened, left) = Model::read(&text).unwrap();
    assert!(left.is_empty());
    assert_eq!(reopened, s.model.as_ref().clone());
    let frame = render::section_fit(&reopened, view, 1000, 700);
    let drawing = render::section(&reopened, view, frame);
    assert!(drawing.owners.contains(&Some(id)), "dimension must be drawn in its owner view");
    let plan = render::draw(&reopened, reopened.levels()[0].0, frame);
    assert!(!plan.owners.contains(&Some(id)), "dimension must not leak into plan");
    let png = render::section_png(&reopened, view, frame).unwrap();
    assert!(png.len() > 1000);
    if let Ok(dir) = std::env::var("BIMCRAFT_VERTICAL_DUMP") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/rotated-section.png"), png).unwrap();
        std::fs::write(format!("{dir}/rotated-section.bimc"), text).unwrap();
    }
}

#[test]
fn vertical_tools_component_motion_preserves_depth_and_pinned_selection() {
    let mut s = Session::new();
    let view = run(&mut s, "elevation", json!({"start":[1000,2000],"end":[4000,6000]}))["id"].clone();
    let component = run(&mut s, "component", json!({"family":"Table","at":[3000,4000],"offset":100}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"view":view,"ids":[component],"by":[500,200]}));
    let at = match s.model.elements.get(component).unwrap() {
        Element::Component(c) => (c.at, c.offset),
        _ => panic!(),
    };
    assert_eq!(at, (crate::geom::P2(3300.0, 4400.0), 300.0));
    run(&mut s, "pin", json!({"ids":[component]}));
    let before = serde_json::to_value(&*s.model).unwrap();
    assert!(s.execute("move", &json!({"view":view,"ids":[component],"by":[100,100]})).is_err());
    assert_eq!(serde_json::to_value(&*s.model).unwrap(), before);
}
