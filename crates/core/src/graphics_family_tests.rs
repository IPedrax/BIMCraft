//! A component is on the row of its family's category in Visibility/Graphics and Object Styles, so a plan of power hides the
//! luminaires and one of lighting the outlets; `component` stays as the old row every family's, and the electrical view templates
//! come with a new project.

use serde_json::{Value, json};

use crate::circuit_tests::{fails, place, run};
use crate::engine::Session;
use crate::graphics::{ALIAS, FAMILY, Look, MODEL, drawn};
use crate::model::{Id, Model};
use crate::render::View;

/// A project with two luminaires, a switch, an outlet, a distribution board, a conduit, and a wall: the ids, in that order.
fn project() -> (Session, [Id; 6]) {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let ids = [
        place(&mut s, "Ceiling Luminaire", [1000, 1000]),
        place(&mut s, "Recessed Luminaire", [2000, 1000]),
        place(&mut s, "Switch", [3000, 1000]),
        place(&mut s, "Outlet", [4000, 1000]),
        place(&mut s, "Distribution Board", [5000, 1000]),
        run(&mut s, "conduit", json!({"points": [[0, 2000], [4000, 2000]]}))["ids"][0].as_u64().unwrap(),
    ];
    (s, ids)
}

fn shown(s: &Session, ids: &[Id]) -> Vec<bool> {
    let look = Look::of(&s.model, 100000);
    ids.iter().map(|id| look.shows(*id)).collect()
}

#[test]
fn every_family_category_is_a_row_and_component_is_only_the_old_alias() {
    for name in FAMILY {
        assert!(MODEL.contains(&name) && drawn(name), "{name}");
    }
    assert!(drawn("component") && !MODEL.contains(&"component") && ALIAS == ["component"]);
    assert!(!drawn("bogus"));
    // The rows are the categories a family can have, but for the doors and windows, which have rows of their own.
    use crate::family::Category::*;
    let all = [
        Furniture,
        Casework,
        Plumbing,
        Drain,
        PipeFitting,
        PipeAccessory,
        Equipment,
        Outlet,
        Switch,
        AirTerminal,
        Earthing,
        InspectionBox,
        Lighting,
        Generic,
        DataOutlet,
        VoiceOutlet,
        PatchPanel,
        Rack,
        Door,
        Window,
    ];
    let keys: Vec<&str> = all.iter().map(|c| c.key()).collect();
    for c in &keys[..18] {
        assert!(FAMILY.contains(c), "{c}");
    }
    assert_eq!((FAMILY.len(), keys[18..].to_vec()), (18, vec!["door", "window"]));
    assert!(keys.iter().all(|k| MODEL.contains(k)));
}

#[test]
fn hiding_lighting_hides_every_luminaire_and_nothing_else() {
    let (mut s, ids) = project();
    assert!(shown(&s, &ids).iter().all(|b| *b));
    run(&mut s, "graphics", json!({"hide": ["lighting"]}));
    // Both luminaires, then the switch, the outlet, the board and the conduit (a run, not a component).
    assert_eq!(shown(&s, &ids), [false, false, true, true, true, true]);
    run(&mut s, "graphics", json!({"hide": ["outlet"], "show": ["lighting"]}));
    assert_eq!(shown(&s, &ids), [true, true, true, false, true, true]);
    // `elements` and `tag` take the row as a category too.
    let lit = run(&mut s, "elements", json!({"category": "lighting"}));
    assert_eq!(lit["elements"].as_array().map(Vec::len), Some(2), "{lit}");
    assert!(fails(&mut s, "graphics", json!({"hide": ["lamp"]})).contains("lighting"));
}

#[test]
fn component_still_hides_and_overrides_every_family_and_a_row_of_its_own_goes_over_it() {
    let (mut s, ids) = project();
    run(&mut s, "graphics", json!({"hide": ["component"]}));
    assert_eq!(shown(&s, &ids), [false, false, false, false, false, true]);
    // The same in a file from before the rows: only `component` is there.
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(14);
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty() && m.check().is_ok());
    let look = Look::of(&m, 100000);
    assert!(ids[..5].iter().all(|id| !look.shows(*id)) && look.shows(ids[5]));
    // A half-tone for `component` and a hidden outlet: the outlet is hidden, the luminaires half-tone.
    run(&mut s, "graphics", json!({"reset": true, "categories": {"component": {"halftone": true}, "outlet": {"hidden": true}}}));
    assert_eq!(shown(&s, &ids), [true, true, true, false, true, true]);
    // Half-tone is `component`'s, so the luminaire has it, as the switch does; the hidden outlet isn't there to have any.
    let look = Look::of(&s.model, 100000);
    let black = crate::render::Rgb(0, 0, 0);
    assert!(look.cut_fill(ids[0], black) != black && look.cut_fill(ids[2], black) != black);
}

#[test]
fn a_files_component_object_style_is_every_family_rows_until_it_has_its_own() {
    let mut s = Session::new();
    run(&mut s, "object_styles", json!({"categories": {"component": {"projection": 2, "cut": 5}}}));
    assert_eq!((s.model.object_style("lighting").projection.0, s.model.object_style("outlet").cut.0), (2, 5));
    run(&mut s, "object_styles", json!({"categories": {"lighting": {"projection": 9}}}));
    assert_eq!((s.model.object_style("lighting").projection.0, s.model.object_style("outlet").projection.0), (9, 2));
    // A wall is not a family: the alias doesn't reach it. Every row is listed, `component` is not.
    assert_eq!(s.model.object_style("wall").projection.0, 4);
    let r = run(&mut s, "object_styles", json!({}));
    let names: Vec<&str> = r["categories"].as_array().unwrap().iter().filter_map(|c| c["category"].as_str()).collect();
    assert!(FAMILY.iter().all(|f| names.contains(f)) && !names.contains(&"component"), "{names:?}");
    // Drawn in those pens: the luminaire's lines by its row.
    assert!(fails(&mut s, "object_styles", json!({"categories": {"lamp": {"cut": 3}}})).contains("isn't a model category"));
}

#[test]
fn a_new_project_has_the_electrical_templates_and_an_old_one_has_none() {
    let s = Session::new();
    let names: Vec<&String> = s.model.view_templates.keys().collect();
    assert_eq!(names, ["Electrical – Ceiling", "Electrical – Lighting", "Electrical – Power"]);
    let pt = Model::new_in("Casa", crate::language::Language::Portuguese);
    assert!(pt.view_templates.len() == 3 && pt.check().is_ok());
    // A file of an earlier format that has no templates opens with none: they are not put in.
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(14);
    file.as_object_mut().unwrap().remove("view_templates");
    let (m, _) = Model::read(&file.to_string()).unwrap();
    assert!(m.view_templates.is_empty());
}

#[test]
fn the_power_template_halftones_the_luminaires_and_the_lighting_one_hides_the_receptacles() {
    let (mut s, ids) = project();
    let data = place(&mut s, "Data Outlet", [1000, 3000]);
    let all = [ids[0], ids[1], ids[2], ids[3], ids[4], data];
    run(&mut s, "graphics", json!({"template": "Electrical – Power"}));
    assert!(shown(&s, &all).iter().all(|b| *b), "nothing is hidden in the power plan");
    let (look, black) = (Look::of(&s.model, 100000), crate::render::Rgb(0, 0, 0));
    assert!(look.cut_fill(ids[0], black) != black && look.cut_fill(ids[1], black) != black, "the luminaires are in half-tone");
    assert!(look.cut_fill(ids[3], black) == black && look.cut_fill(ids[4], black) == black, "the outlet and the board are not");
    run(&mut s, "graphics", json!({"template": "Electrical – Lighting"}));
    // The luminaires, the switch and the board stay; the outlet and the data outlet go.
    assert_eq!(shown(&s, &all), [true, true, true, false, true, false]);
    // A view that follows a template can't change what it controls, and says to change the template.
    assert!(fails(&mut s, "graphics", json!({"hide": ["lighting"]})).contains("Electrical – Lighting"));
    run(&mut s, "graphics", json!({"template": null}));
    assert!(shown(&s, &all).iter().all(|b| *b));
}

#[test]
fn a_ceiling_plan_draws_luminaires_and_trays_only_where_the_view_says() {
    let (mut s, ids) = project();
    let tray = run(&mut s, "cable_tray", json!({"points": [[0, 3000], [4000, 3000]]}))["ids"][0].as_u64().unwrap();
    let view = View { scale: 0.1, cx: 3000.0, cy: 1000.0, width: 800, height: 400 };
    let before = crate::render::draw_ceiling(&s.model, 100000, view);
    assert!(!before.owners.contains(&Some(ids[0])) && !before.owners.contains(&Some(tray)), "as always, none of them");
    // Opted in, by the command: the luminaires and the tray, not the outlet or the conduit.
    run(&mut s, "graphics", json!({"ceiling": ["lighting", "cabletray"]}));
    let d = crate::render::draw_ceiling(&s.model, 100000, view);
    assert!(d.owners.contains(&Some(ids[0])) && d.owners.contains(&Some(ids[1])) && d.owners.contains(&Some(tray)));
    assert!(!d.owners.contains(&Some(ids[3])) && !d.owners.contains(&Some(ids[5])));
    // Hidden, they are not drawn, ceiling plan or not.
    run(&mut s, "graphics", json!({"hide": ["lighting"]}));
    assert!(!crate::render::draw_ceiling(&s.model, 100000, view).owners.contains(&Some(ids[0])));
    // The template does it, and a view with an empty set is as it was.
    run(&mut s, "graphics", json!({"reset": true}));
    assert_eq!(crate::render::draw_ceiling(&s.model, 100000, view).prims.len(), before.prims.len());
    run(&mut s, "graphics", json!({"template": "Electrical – Ceiling"}));
    assert!(crate::render::draw_ceiling(&s.model, 100000, view).owners.contains(&Some(ids[0])));
    // What isn't a model category is refused, and so is a list that isn't a list.
    run(&mut s, "graphics", json!({"template": null}));
    assert!(fails(&mut s, "graphics", json!({"ceiling": ["grid"]})).contains("ceiling plan"));
    assert!(fails(&mut s, "graphics", json!({"ceiling": "lighting"})).contains("list"));
}

#[test]
fn tag_takes_the_row_of_the_plan() {
    let (mut s, ids) = project();
    let r = run(&mut s, "tag", json!({"view": 100000, "category": "lighting"}));
    let made = r["ids"].as_array().map_or(0, Vec::len);
    assert_eq!(made, 2, "a tag for each luminaire and no other: {r} {ids:?}");
}
