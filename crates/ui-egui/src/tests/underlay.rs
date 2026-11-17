use super::*;

/// Walls on Level 1, and others on Level 2 that Level 1 lies under.
const FLOORS: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "level": "Level 1"}
wall {"points": [[2000,1000],[6000,1000],[6000,5000]], "level": "Level 2"}"#;

/// What underlays look like, for a person to look at: `BIMCRAFT_UNDERLAY_DUMP=<folder>` writes Level 2 over Level 1, light
/// and dark, its Properties and Underlay Settings.
#[test]
fn underlay_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_UNDERLAY_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let mut h = app(FLOORS);
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        topmost(&mut h, "Level 2");
        let view = h.state().plan_key().unwrap();
        h.state_mut().run("underlay", serde_json::json!({"view": view, "source": "Level 1"})).unwrap();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("underlay-plan-{look}.png"))).unwrap();
        h.state_mut().underlay_dialog = Some(view);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("underlay-settings-{look}.png"))).unwrap();
    }
}

fn level_two(h: &mut Harness<BimApp>) -> Id {
    topmost(h, "Level 2");
    h.state().plan_key().unwrap()
}

fn underlay(h: &Harness<BimApp>, view: Id) -> Option<bimcraft_core::underlay::Underlay> {
    bimcraft_core::underlay::of(&h.state().session.model, view).cloned()
}

#[test]
fn show_underlay_in_properties_starts_with_the_level_below() {
    let mut h = app(FLOORS);
    let view = level_two(&mut h);
    // Properties is a short palette: scroll it down to the Underlay group.
    h.hover_at(pos2(180.0, 420.0));
    h.event(Event::MouseWheel {
        phase: TouchPhase::Move,
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -3000.0),
        modifiers: Modifiers::NONE,
    });
    h.run();
    pick(&mut h, Role::CheckBox, "Show Underlay");
    let u = underlay(&h, view).expect("an underlay");
    let m = &h.state().session.model;
    assert_eq!(m.view_name(u.view), Some("Level 1"));
    assert!(u.on);
    pick(&mut h, Role::CheckBox, "Show Underlay");
    assert!(!underlay(&h, view).unwrap().on, "off, its settings kept");
}

#[test]
fn an_underlay_is_drawn_and_snapped_to_but_never_picked() {
    let mut h = app(FLOORS);
    let view = level_two(&mut h);
    let one = h.state().session.model.view_named("Level 1").unwrap();
    h.state_mut().show_as_underlay(one);
    h.run();
    assert!(underlay(&h, view).is_some_and(|u| u.on));
    assert!(h.state().underlay.segments(view).len() >= 4, "its lines are there to snap to");
    // On Level 1's wall, where Level 2 has nothing: a click picks nothing.
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().selection.is_empty(), "{:?}", h.state().selection);
    // A point near the box's corner snaps to it.
    let app = h.state();
    let v = app.canvas.current_view(view).unwrap();
    let found = app.snap(&app.session.model, app.session.model.plan_level(view).unwrap(), &v, v.px(P2(8030.0, 6040.0)), &[], false);
    assert!(found.at.dist(P2(8000.0, 6000.0)) < 150.0, "{found:?}");
    assert_eq!(found.source, None);
}

#[test]
fn move_and_rotate_pick_in_the_view_while_the_dialog_stands_aside() {
    let mut h = app(FLOORS);
    let view = level_two(&mut h);
    h.state_mut().run("underlay", serde_json::json!({"view": view, "source": "Level 1"})).unwrap();
    h.state_mut().underlay_dialog = Some(view);
    h.run();
    assert!(h.query_by_label("Underlay Settings: Level 2").is_some());
    press_near(&mut h, "Move", "Reset Position");
    assert!(h.state().underlay_edit.is_some());
    assert!(h.query_by_label("Underlay Settings: Level 2").is_none(), "the dialog stands aside");
    click(&mut h, P2(3000.0, 3000.0));
    click(&mut h, P2(3500.0, 3000.0));
    let u = underlay(&h, view).unwrap();
    assert_eq!(u.offset, P2(500.0, 0.0));
    assert!(h.state().underlay_edit.is_none());
    assert!(h.state().selection.is_empty(), "the clicks were the underlay's");
    assert!(h.query_by_label("Underlay Settings: Level 2").is_some(), "and it comes back");
    // Esc stops one part way.
    press_near(&mut h, "Rotate", "Reset Position");
    click(&mut h, P2(3000.0, 3000.0));
    key(&mut h, Key::Escape);
    assert!(h.state().underlay_edit.is_none());
    assert_eq!(underlay(&h, view).unwrap().angle, 0.0);
    // A quarter turn about a point.
    h.state_mut().underlay_dialog = Some(view);
    h.run();
    press_near(&mut h, "Rotate", "Reset Position");
    click(&mut h, P2(3000.0, 3000.0));
    click(&mut h, P2(5000.0, 3000.0));
    click(&mut h, P2(3000.0, 5000.0));
    let u = underlay(&h, view).unwrap();
    assert!((u.angle - 90.0).abs() < 1e-6, "{u:?}");
    press_near(&mut h, "Reset Position", "Rotate");
    assert!(underlay(&h, view).unwrap().placed());
}

#[test]
fn the_browser_shows_a_view_as_the_underlay_of_the_view_on_screen() {
    let mut h = app(FLOORS);
    let view = level_two(&mut h);
    assert_eq!(h.state().underlay_target(), Some(view));
    let one = h.state().session.model.view_named("Level 1").unwrap();
    h.state_mut().show_as_underlay(one);
    assert_eq!(underlay(&h, view).map(|u| h.state().session.model.elements.id(u.view)), Some(Some(one)));
}

#[test]
fn ud_opens_the_underlay_settings_of_the_view_on_screen_and_says_why_not_in_3d() {
    let mut h = app(FLOORS);
    let view = level_two(&mut h);
    shortcut(&mut h, "UD");
    assert_eq!(h.state().underlay_dialog, Some(view));
    assert!(h.query_by_label("Underlay Settings: Level 2").is_some(), "with no underlay yet, it is where one is picked");
    assert!(h.query_all_by_label_contains("not printed or exported").next().is_some(), "and it says an underlay is a working aid");
    press(&mut h, "Close");
    assert_eq!(h.state().underlay_dialog, None);
    shortcut(&mut h, "3D");
    shortcut(&mut h, "UD");
    assert!(h.state().underlay_dialog.is_none() && h.state().status.error, "{}", h.state().status.text);
}
