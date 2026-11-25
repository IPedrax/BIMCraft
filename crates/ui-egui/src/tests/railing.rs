//! Railings in the desktop: Edit Railing (its parts, its fixed size) and the screenshots of it.

use super::*;

/// Edit Railing for the project's railing type, open over its Type Properties.
fn edit_railing(h: &mut Harness<BimApp>) {
    let name = h.state().session.model.types.railing.keys().next().cloned().unwrap();
    h.state_mut().open_railing_editor(&name);
    h.set_size(egui::vec2(1400.0, 900.0));
    h.run();
}

/// The window's rectangle, by its title.
fn window(h: &Harness<BimApp>) -> egui::Rect {
    h.get_by_label_contains("Edit Railing:").rect()
}

#[test]
fn edit_railing_adds_parts_in_three_columns_and_keeps_its_size() {
    let mut h = app(BOX);
    edit_railing(&mut h);
    let before = window(&h);
    // The older type opens as a top rail and balusters; a post added is picked, its parameters shown.
    assert!(h.query_by_label("Top Rail").is_some() && h.query_by_label("Baluster").is_some());
    press(&mut h, "+ Post");
    assert!(h.query_by_label("Line up with steps").is_some(), "a post's parameters");
    press(&mut h, "+ Panel");
    assert!(h.query_by_label("Gap at the posts").is_some(), "the panel's parameters");
    press(&mut h, "Top Rail");
    assert!(h.query_by_label("Height of its top").is_some(), "a rail's parameters");
    assert_eq!(window(&h), before, "the window does not move or grow as parts are picked");
    press(&mut h, "OK");
    let t = h.state().session.model.types.railing.values().next().cloned().unwrap();
    assert_eq!((t.rails.len(), t.posts.len(), t.panel.is_some()), (1, 2, true));
}

/// What Edit Railing looks like: `BIMCRAFT_RAILING_DUMP=<folder>` writes it with a rail, a post and a baluster picked, in
/// the three looks.
#[test]
fn edit_railing_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_RAILING_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let mut h = app(BOX);
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        edit_railing(&mut h);
        press(&mut h, "+ Post");
        for (pick, name) in [("Top Rail", "rail"), ("Post", "post"), ("Baluster", "baluster")] {
            press(&mut h, pick);
            h.run_steps(6);
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("railing-{name}-{look}.png"))).unwrap();
        }
    }
}

#[test]
fn a_railing_selected_offers_edit_path_and_edit_hosts() {
    let mut h = app(&format!("{BOX}\nstair {{\"start\": [2000, 1000], \"angle\": 90}}\nrailing {{\"path\": [[1000, 500], [1000, 5500]]}}"));
    let (stair, railing) = {
        let m = &h.state().session.model;
        let find = |c: &str| m.elements.iter().find(|(_, e)| e.category() == c).map(|(id, _)| *id).unwrap();
        (find("stair"), find("railing"))
    };
    h.state_mut().selection = vec![railing];
    h.state_mut().raise_context();
    h.run();
    // Edit Path: its line, redrawn as lines, and Finish keeps it.
    press(&mut h, "Edit Path");
    assert_eq!(h.state().sketch.as_ref().map(|sk| sk.lines.len()), Some(1));
    h.state_mut().finish_sketch();
    h.run();
    assert!(h.state().sketch.is_none() && h.state().selection == vec![railing]);
    // Edit Hosts: a click on the stair adds it.
    press(&mut h, "Edit Hosts");
    assert!(h.state_mut().railing_host_pick_up(&[stair]));
    let Some(Element::Railing(rl)) = h.state().session.model.elements.get(railing) else { panic!() };
    assert_eq!(rl.hosts.len(), 1);
    press(&mut h, "Finish");
    assert!(h.state().railing_hosts.is_none());
}

#[test]
fn tab_picks_a_part_of_the_selected_railing_and_delete_leaves_it_out_of_the_pattern() {
    let script = format!(
        "{BOX}\n{}\n{}",
        r#"type {"category": "railing", "name": "P", "rails": [{"name": "Top", "width": 50, "height": 1000}], "posts": [{"name": "Post", "width": 50, "spacing": 1000}]}"#,
        r#"railing {"path": [[1000, 500], [4000, 500]], "type": "P"}"#
    );
    let mut h = app(&script);
    let railing = h.state().session.model.elements.iter().find(|(_, e)| e.category() == "railing").map(|(id, _)| *id).unwrap();
    h.state_mut().selection = vec![railing];
    h.run();
    // Tab over the railing's start: its first post, then the rail over it, then the whole railing again.
    let list = {
        let m = &h.state().session.model;
        let Some(Element::Railing(rl)) = m.elements.get(railing) else { panic!() };
        bimcraft_core::railings::parts_near(m, rl, P2(1000.0, 500.0), 30.0)
    };
    assert_eq!(list.first().map(|p| p.kind), Some(bimcraft_core::model::RailingPartKind::Post), "{list:?}");
    assert!(h.state_mut().railing_tab(railing, &list));
    h.run();
    assert!(h.query_by_label("Post: Post 1").is_some(), "the post's own Properties");
    assert!(h.query_by_label("Delete from Pattern").is_some());
    // Delete from Pattern: the post left out, the railing kept and still selected.
    press(&mut h, "Delete from Pattern");
    let m = &h.state().session.model;
    let Some(Element::Railing(rl)) = m.elements.get(railing) else { panic!("the railing stays") };
    assert!(rl.parts.iter().any(|e| e.excluded && e.at.index == 0));
    assert_eq!(
        bimcraft_core::railings::parts(m, rl).iter().filter(|p| p.at.kind == bimcraft_core::model::RailingPartKind::Post).count(),
        3
    );
    // With the railing alone selected, its deleted parts can be put back.
    h.run();
    press(&mut h, "Put Back Deleted Parts (1)");
    let Some(Element::Railing(rl)) = h.state().session.model.elements.get(railing) else { panic!() };
    assert!(rl.parts.is_empty());
}

#[test]
fn undo_in_edit_path_takes_back_the_last_change_to_the_path_not_the_model() {
    let mut h = app(&format!("{BOX}\nrailing {{\"path\": [[1000, 500], [1000, 5500]]}}"));
    let railing = ids(&h, "railing")[0];
    h.state_mut().selection = vec![railing];
    h.state_mut().raise_context();
    h.run();
    let steps = h.state().session.history().len();
    press(&mut h, "Edit Path");
    let lines = |h: &Harness<BimApp>| h.state().sketch.as_ref().map(|sk| sk.lines.len());
    assert_eq!(lines(&h), Some(1));
    // A line on from its end.
    click(&mut h, P2(1000.0, 5500.0));
    click(&mut h, P2(3000.0, 5500.0));
    key(&mut h, Key::Escape);
    assert_eq!(lines(&h), Some(2));
    // Ctrl+Z takes the line back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(lines(&h), Some(1));
    // Again, and Undo on the toolbar: the sketch's, never the model's behind it.
    click(&mut h, P2(1000.0, 5500.0));
    click(&mut h, P2(3000.0, 5500.0));
    key(&mut h, Key::Escape);
    assert_eq!(lines(&h), Some(2));
    press(&mut h, "Undo");
    assert_eq!(lines(&h), Some(1), "the toolbar's Undo takes back the line");
    assert_eq!(h.state().session.history().len(), steps, "and the model is as it was");
    assert!(h.state().sketch.is_some(), "still editing the path");
}
