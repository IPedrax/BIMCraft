//! Visibility/Graphics and Object Styles by the category of a family: a luminaire is Lighting, an outlet Outlets, and so on,
//! where every family component was one `component` row. `component` stays, naming all of them.

use super::testing::*;
use super::{COMPONENT, COMPONENTS, Look, MODEL, ViewKind};
use crate::engine::Session;
use crate::language::Language;
use crate::model::{Element, Model};
use crate::pen::Pen;
use crate::render::Rgb;
use crate::styles::ObjectStyle;
use serde_json::json;

fn place(s: &mut Session, family: &str, at: [f64; 2]) -> u64 {
    run(s, "component", json!({"family": family, "at": at}))["id"].as_u64().unwrap()
}

/// A luminaire hung low enough for a floor plan to see: below the cut plane, where a plan draws a component whole.
fn hang_low(s: &mut Session, family: &str, at: [f64; 2]) -> u64 {
    run(s, "component", json!({"family": family, "at": at, "offset": -2000}))["id"].as_u64().unwrap()
}

/// A device of `family` as its type says, at `at`: an outlet's `Low 300` is the one a floor plan sees.
fn of_type(s: &mut Session, family: &str, ty: &str, at: [f64; 2]) -> u64 {
    run(s, "component", json!({"family": family, "type": ty, "at": at}))["id"].as_u64().unwrap()
}

/// A project with one component of every category a family may be placed in: a family of each, named after its category.
fn every_category() -> (Session, Vec<(&'static str, u64)>, u64) {
    let mut s = Session::new();
    let wall = house(&mut s)[0];
    let mut placed = Vec::new();
    for (i, key) in COMPONENTS.iter().enumerate() {
        let name = format!("Test {key}");
        run(&mut s, "family", json!({"name": name, "category": key, "types": {"A": {}}}));
        placed.push((*key, place(&mut s, &name, [500.0 + 400.0 * i as f64, 1500.0])));
    }
    (s, placed, wall)
}

fn shown(s: &Session, view: u64, id: u64) -> bool {
    Look::of(&s.model, view).shows(id)
}

#[test]
fn model_categories_hold_each_family_category_and_no_component_row() {
    assert!(COMPONENTS.iter().all(|c| MODEL.contains(c)), "every family category is a row");
    assert!(!MODEL.contains(&COMPONENT), "`component` is the name of them all, not a row of its own");
    assert!(super::drawn(COMPONENT) && COMPONENTS.iter().all(|c| super::drawn(c)));
    // The keys are the names a family's category goes by in a file, and every category a component may be of has one.
    for c in crate::family::Category::PLACED.into_iter().chain([crate::family::Category::Casework]) {
        let named = serde_json::to_value(c).unwrap();
        let key = super::component_category(c).unwrap_or_else(|| panic!("{named} has no row"));
        assert_eq!(named, json!(key));
        assert!(COMPONENTS.contains(&key), "{key}");
    }
    // A door's or a window's family is placed in a wall, as a door or a window: it is no component.
    assert!(super::component_category(crate::family::Category::Door).is_none());
    assert!(super::component_category(crate::family::Category::Window).is_none());
    assert_eq!(COMPONENTS.len(), 18);
}

#[test]
fn lighting_off_hides_every_luminaire_and_nothing_else() {
    let mut s = Session::new();
    let walls = house(&mut s);
    let luminaires = [hang_low(&mut s, "Ceiling Luminaire", [2000.0, 2000.0]), hang_low(&mut s, "Recessed Luminaire", [5000.0, 2000.0])];
    let others = [
        of_type(&mut s, "Outlet", "Low 300", [1000.0, 300.0]),
        of_type(&mut s, "Switch", "Low 300", [3000.0, 300.0]),
        run(&mut s, "component", json!({"family": "Distribution Board", "at": [6000.0, 300.0], "offset": -500}))["id"].as_u64().unwrap(),
    ];
    let before = plan(&s, 100000);
    for id in luminaires.into_iter().chain(others) {
        assert!(!owned(&before, id).is_empty(), "{id} is drawn to begin with");
    }
    let r = run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["lighting"]}));
    assert_eq!(r["hidden"], json!(["lighting"]));
    let d = plan(&s, 100000);
    for id in luminaires {
        assert!(owned(&d, id).is_empty() && !shown(&s, 100000, id), "luminaire {id} is gone");
        assert!(shown(&s, 100001, id), "another view still shows it");
    }
    for id in others.into_iter().chain([walls[0]]) {
        assert!(!owned(&d, id).is_empty() && shown(&s, 100000, id), "{id} stays");
    }
    // Shown again, they are back.
    run(&mut s, "graphics", json!({"view": "Level 1", "show": ["lighting"]}));
    assert!(luminaires.iter().all(|id| !owned(&plan(&s, 100000), *id).is_empty()));
}

#[test]
fn each_category_hides_its_own_family_components_alone_and_component_hides_them_all() {
    let (mut s, placed, wall) = every_category();
    assert!(matches!(s.model.elements.get(wall), Some(Element::Wall(_))));
    assert!(placed.iter().all(|(_, id)| shown(&s, 100000, *id)));
    for (key, id) in &placed {
        run(&mut s, "graphics", json!({"view": 100000, "hide": [key]}));
        for (other, o) in &placed {
            assert_eq!(shown(&s, 100000, *o), other != key, "{key} hidden: {other}");
        }
        assert!(shown(&s, 100000, wall));
        run(&mut s, "graphics", json!({"view": 100000, "show": [key]}));
        assert!(shown(&s, 100000, *id));
    }
    // Plural, in any case: what a person types.
    let r = run(&mut s, "graphics", json!({"view": 100000, "hide": ["Switches", "Data Outlets", "Pipe Fittings"]}));
    assert_eq!(r["hidden"], json!(["dataoutlet", "pipefitting", "switch"]));
    run(&mut s, "graphics", json!({"view": 100000, "reset": true}));
    // `component` is all of them, and only them.
    let r = run(&mut s, "graphics", json!({"view": 100000, "hide": ["component"]}));
    assert_eq!(r["hidden"], json!(["component"]));
    assert!(placed.iter().all(|(_, id)| !shown(&s, 100000, *id)));
    assert!(shown(&s, 100000, wall));
    assert!(placed.iter().all(|(_, id)| shown(&s, 100001, *id)), "in its view only");
    // A category shown while `component` is hidden doesn't bring the rest back: the parent hides what is under it.
    let g = s.model.elements.get(100000).and_then(|e| super::graphics_of_element(e)).unwrap().clone();
    assert!(COMPONENTS.iter().all(|c| g.hides(c)) && !g.hides("wall"));
}

#[test]
fn a_family_category_is_painted_by_its_own_override_over_the_component_one() {
    let mut s = Session::new();
    house(&mut s);
    let (lamp, outlet) = (place(&mut s, "Ceiling Luminaire", [2000.0, 2000.0]), place(&mut s, "Outlet", [1000.0, 300.0]));
    let red = json!({"projection": {"color": [200, 0, 0]}});
    let blue = json!({"projection": {"color": [0, 0, 200]}});
    run(&mut s, "graphics", json!({"view": 100000, "categories": {"component": red, "lighting": blue}}));
    let look = Look::of(&s.model, 100000);
    let black = Rgb(0, 0, 0);
    assert_eq!(look.edge(lamp, black), Rgb(0, 0, 200), "its own category prevails");
    assert_eq!(look.edge(outlet, black), Rgb(200, 0, 0), "the rest take the one for all");
    // The same in halftone, which either gives.
    run(&mut s, "graphics", json!({"view": 100000, "reset": true, "categories": {"component": {"halftone": true}}}));
    let look = Look::of(&s.model, 100000);
    assert_eq!(look.edge(lamp, black), look.edge(outlet, black));
    assert_ne!(look.edge(lamp, black), black);
}

#[test]
fn object_styles_has_a_row_for_each_family_category_and_component_sets_them_all() {
    let mut s = Session::new();
    let r = s.execute("object_styles", &json!({})).unwrap();
    let rows: Vec<&str> = r["categories"].as_array().unwrap().iter().map(|c| c["category"].as_str().unwrap()).collect();
    assert_eq!(rows, MODEL);
    assert!(COMPONENTS.iter().all(|c| rows.contains(c)) && !rows.contains(&COMPONENT));
    let start = ObjectStyle { projection: Pen::PROJECTION, cut: Pen::CUT };
    assert_eq!(s.model.object_style("lighting"), start);
    // A weight set once, for a category.
    s.execute("object_styles", &json!({"categories": {"lighting": {"projection": 2}}})).unwrap();
    assert_eq!(s.model.object_style("lighting").projection, Pen(2));
    assert_eq!(s.model.object_style("outlet"), start);
    // `component` names every one of them that has none of its own, as it did when it was the only row.
    s.execute("object_styles", &json!({"categories": {"component": {"projection": 6, "cut": 7}}})).unwrap();
    assert_eq!(s.model.object_style("outlet"), ObjectStyle { projection: Pen(6), cut: Pen(7) });
    assert_eq!(s.model.object_style("lighting"), ObjectStyle { projection: Pen(2), cut: Pen::CUT }, "its own row prevails");
    assert_eq!(s.model.object_style("wall"), ObjectStyle { projection: Pen::PROJECTION, cut: Pen::CUT });
    // A category given what it starts with, while `component` says another, keeps saying it.
    s.execute("object_styles", &json!({"categories": {"switch": {"projection": 4, "cut": 10}}})).unwrap();
    assert_eq!(s.model.object_style("switch"), start);
    assert_eq!(s.model.object_style("outlet"), ObjectStyle { projection: Pen(6), cut: Pen(7) });
    let (m, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.object_styles, s.model.object_styles);
    s.execute("object_styles", &json!({"reset": true})).unwrap();
    assert!(s.model.object_styles.is_empty());
    // What is no row is still refused, and the error lists them.
    let e = s.execute("object_styles", &json!({"categories": {"lights": {"cut": 3}}})).unwrap_err();
    assert!(e.contains("isn't a model category") && e.contains("lighting"), "{e}");
}

#[test]
fn an_object_style_draws_a_luminaire_in_its_family_categorys_pens() {
    let mut s = Session::new();
    house(&mut s);
    let (lamp, outlet) = (hang_low(&mut s, "Ceiling Luminaire", [2000.0, 2000.0]), of_type(&mut s, "Outlet", "Low 300", [1000.0, 300.0]));
    let pens = |s: &Session, id: u64| -> Vec<u8> {
        let d = plan(s, 100000);
        let mut v: Vec<u8> = lines(&d, id).into_iter().map(|(pen, _, _)| pen).filter(|p| *p != u8::MAX).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let (lamp_pens, outlet_pens) = (pens(&s, lamp), pens(&s, outlet));
    run(&mut s, "object_styles", json!({"categories": {"lighting": {"projection": 1, "cut": 1}}}));
    assert_eq!(pens(&s, lamp), vec![1], "was {lamp_pens:?}");
    assert_eq!(pens(&s, outlet), outlet_pens, "an outlet keeps its own");
    run(&mut s, "object_styles", json!({"categories": {"component": {"projection": 3, "cut": 3}}}));
    assert_eq!(pens(&s, lamp), vec![1], "its own row prevails over the one for all");
    assert_eq!(pens(&s, outlet), vec![3]);
}

/// A project as `main` wrote one before the rows were split: format 12, a `component` row in an object style, a view's graphics,
/// a view template and a view filter, and no starter templates.
fn saved_before_the_split() -> (String, [u64; 3]) {
    let mut s = Session::new();
    house(&mut s);
    let ids = [
        place(&mut s, "Ceiling Luminaire", [2000.0, 2000.0]),
        place(&mut s, "Outlet", [1000.0, 300.0]),
        place(&mut s, "Distribution Board", [6000.0, 300.0]),
    ];
    for starter in ["Electrical Power", "Electrical Lighting", "Electrical Ceiling"] {
        run(&mut s, "template", json!({"name": starter, "delete": true}));
    }
    run(&mut s, "object_styles", json!({"categories": {"component": {"projection": 2, "cut": 3}}}));
    run(&mut s, "template", json!({"name": "Old", "kind": "plan", "controls": ["categories"], "hide": ["component"]}));
    run(&mut s, "filter", json!({"name": "All Fixtures", "categories": ["component"]}));
    run(&mut s, "graphics", json!({"view": 100000, "categories": {"component": {"projection": {"color": [200, 0, 0]}}}}));
    run(&mut s, "graphics", json!({"view": 100001, "template": "Old"}));
    let mut v: serde_json::Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(12);
    (v.to_string(), ids)
}

#[test]
fn a_project_saved_before_the_split_opens_and_draws_unchanged() {
    let (text, ids) = saved_before_the_split();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.check(), Ok(()));
    // Through `open`, as a front end reads it.
    let dir = std::env::temp_dir().join(format!("bimcraft-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("before.bimcraft");
    std::fs::write(&path, &text).unwrap();
    let mut s = Session::new();
    run(&mut s, "open", json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(s.model.object_styles.keys().collect::<Vec<_>>(), ["component"]);
    let black = Rgb(0, 0, 0);
    // The category override for all of them paints every one.
    let look = Look::of(&s.model, 100000);
    for id in ids {
        assert_eq!(look.edge(id, black), Rgb(200, 0, 0), "{id}");
        assert_eq!(s.model.object_style_of(s.model.elements.get(id).unwrap()), ObjectStyle { projection: Pen(2), cut: Pen(3) });
    }
    // The template that hides `component` hides every one in the view that follows it.
    assert!(ids.iter().all(|id| !shown(&s, 100001, *id)));
    assert!(ids.iter().all(|id| shown(&s, 100000, *id)));
    // The filter that names `component` still picks them out.
    assert_eq!(s.model.view_filters["All Fixtures"].matching(&s.model).len(), 3);
    // Saved again it keeps the row it had, and reads back as it was.
    let (again, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(again.object_styles, s.model.object_styles);
    assert_eq!(again.view_templates["Old"], s.model.view_templates["Old"]);
}

#[test]
fn a_new_project_starts_with_the_electrical_templates_named_in_its_language() {
    for l in Language::ALL {
        let m = Model::new_in("p", l);
        for name in ["Electrical Power", "Electrical Lighting", "Electrical Ceiling"] {
            let t = m.view_templates.get(l.word(name)).unwrap_or_else(|| panic!("{l:?}: no {name}"));
            assert_eq!(t.kind, Some(ViewKind::Plan), "{name}");
            assert_eq!(m.check(), Ok(()));
        }
        assert_eq!(m.view_templates.len(), 3);
    }
}

#[test]
fn the_electrical_templates_show_power_lighting_and_the_ceiling_each_its_own_fixtures() {
    let mut s = Session::new();
    house(&mut s);
    let tray = run(&mut s, "cable_tray", json!({"points": [[500, 4000], [7000, 4000]], "offset": 2800}))["ids"][0].as_u64().unwrap();
    let lamp = place(&mut s, "Ceiling Luminaire", [2000.0, 2000.0]);
    let (outlet, switch, board) = (
        place(&mut s, "Outlet", [1000.0, 300.0]),
        place(&mut s, "Switch", [3000.0, 300.0]),
        place(&mut s, "Distribution Board", [6000.0, 300.0]),
    );
    let black = Rgb(0, 0, 0);
    let follow = |s: &mut Session, template: &str| {
        run(s, "graphics", json!({"view": 100000, "template": template}));
        Look::of(&s.model, 100000)
    };
    // Power: outlets and equipment as they are, luminaires lightened.
    let look = follow(&mut s, "Electrical Power");
    assert!([lamp, outlet, switch, board, tray].iter().all(|id| look.shows(*id)));
    assert_ne!(look.edge(lamp, black), black, "luminaires in halftone");
    assert_eq!([outlet, board].map(|id| look.edge(id, black)), [black; 2]);
    // Lighting: luminaires and switches shown, outlets hidden.
    let look = follow(&mut s, "Electrical Lighting");
    assert!(look.shows(lamp) && look.shows(switch));
    assert!(!look.shows(outlet));
    // Ceiling: luminaires and cable trays, none of what stands on the floor or the wall.
    let look = follow(&mut s, "Electrical Ceiling");
    assert!(look.shows(lamp) && look.shows(tray));
    assert!(!look.shows(outlet) && !look.shows(switch) && !look.shows(board));
    // A template controls its categories only: the view keeps its own scale.
    assert!(s.model.view_templates["Electrical Ceiling"].controls.len() == 1);
}
