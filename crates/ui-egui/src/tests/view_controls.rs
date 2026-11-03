//! The view control bar's view controls, the same in both profiles: the renovation filter, which phase of the work a view
//! shows in plain words, and visibility sets, view templates of what a view shows switched at a click.

use super::*;

/// `item` picked from the bar's menu `menu`: of the widgets so named, the one nearest the menu's button.
fn choose(h: &mut Harness<BimApp>, menu: &str, item: &str) {
    press(h, menu);
    press_near(h, item, menu);
}

/// What the bar's menu `menu` shows, the setting it holds, as a screen reader reads it.
fn shows(h: &Harness<BimApp>, menu: &str) -> String {
    h.get_by_role_and_label(Role::Button, menu).value().unwrap_or_default()
}

/// The phase and the phase filter the plan of Level 1 holds.
fn phasing(h: &Harness<BimApp>) -> (Option<String>, Option<String>) {
    let g = plan_graphics(h, 100000);
    (g.phase, g.phase_filter)
}

#[test]
fn the_renovation_filter_sets_the_phase_and_the_phase_filter_a_view_shows_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        // A view with no phasing of its own shows everything.
        assert_eq!(shows(&h, "Renovation"), "All", "rev {rev}");
        choose(&mut h, "Renovation", "Demolition");
        assert_eq!(phasing(&h), (None, Some("Show Previous + Demolished".into())), "rev {rev}");
        assert_eq!(shows(&h, "Renovation"), "Demolition", "rev {rev}");
        assert_eq!(h.state().session.history().last(), Some(&"Visibility/Graphics"), "one command, one undo step");
        choose(&mut h, "Renovation", "New");
        assert_eq!(phasing(&h), (None, Some("Show Previous + New".into())), "rev {rev}");
        choose(&mut h, "Renovation", "Existing");
        assert_eq!(phasing(&h), (Some("Existing".into()), None), "rev {rev}");
        choose(&mut h, "Renovation", "All");
        assert_eq!(phasing(&h), (None, None), "rev {rev}: the view's own again");
        // Set elsewhere (Properties), a phase and a filter the bar has words for are read back in them.
        h.state_mut().run("graphics", json!({"view": 100000, "phase": "New Construction", "phase_filter": "Show Previous + New"}));
        h.run();
        assert_eq!(shows(&h, "Renovation"), "New", "rev {rev}");
        // A project with no phase filter for one greys it out, and says what it needs.
        h.state_mut().run("phase_filter", json!({"name": "Show Previous + Demolished", "delete": true}));
        h.run();
        press(&mut h, "Renovation");
        assert!(is_disabled(&h, Role::Button, "Demolition"), "rev {rev}");
        key(&mut h, Key::Escape);
    }
}

#[test]
fn a_view_template_that_sets_the_phase_greys_the_renovation_filter_out() {
    let mut h = bimcraft(BOX);
    h.state_mut().run("template", json!({"name": "Phased", "from": 100000, "controls": ["phase"]}));
    h.state_mut().run("graphics", json!({"view": 100000, "template": "Phased"}));
    h.run();
    assert!(is_disabled(&h, Role::Button, "Renovation"));
}

#[test]
fn a_visibility_set_saved_on_the_bar_switches_a_view_at_a_click_in_both_profiles() {
    use bimcraft_core::graphics::Group;
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        let hides_walls = |h: &Harness<BimApp>| plan_graphics(h, 100000).resolved(&h.state().session.model).hides("wall");
        assert_eq!(shows(&h, "Visibility Set"), "<None>", "rev {rev}: the view's own settings");
        // Walls hidden and the demolition shown, saved under a name: a template of what the view shows and its phase, which
        // the view follows, one undo step.
        h.state_mut().run("graphics", json!({"view": 100000, "hide": ["walls"]}));
        assert!(hides_walls(&h), "rev {rev}");
        choose(&mut h, "Renovation", "Demolition");
        press(&mut h, "Visibility Set");
        type_into(&mut h, "Save as", "No walls");
        key(&mut h, Key::Enter);
        let controls = h.state().session.model.view_templates.get("No walls").map(|t| t.controls.clone());
        assert_eq!(controls, Some([Group::Categories, Group::Filters, Group::Phase].into_iter().collect()), "rev {rev}");
        assert_eq!(plan_graphics(&h, 100000).template.as_deref(), Some("No walls"), "rev {rev}");
        assert_eq!(shows(&h, "Visibility Set"), "No walls", "rev {rev}");
        assert_eq!(h.state().session.history().last(), Some(&"Visibility Set"), "rev {rev}");
        // A name taken is said, and makes nothing.
        press(&mut h, "Visibility Set");
        type_into(&mut h, "Save as", "No walls");
        key(&mut h, Key::Enter);
        assert!(h.state().status.error, "rev {rev}");
        // The view's own again, changed: walls shown, every phase.
        choose(&mut h, "Visibility Set", "<None>");
        h.state_mut().run("graphics", json!({"view": 100000, "show": ["walls"], "phase_filter": null}));
        h.run();
        assert!(!hides_walls(&h), "rev {rev}");
        // The set, and back: each as it was saved.
        choose(&mut h, "Visibility Set", "No walls");
        assert!(hides_walls(&h) && shows(&h, "Renovation") == "Demolition", "rev {rev}");
        choose(&mut h, "Visibility Set", "<None>");
        assert!(!hides_walls(&h) && shows(&h, "Renovation") == "All", "rev {rev}");
        // A plan's sets are for plans: the 3D view lists none of them.
        press(&mut h, "Default 3D View");
        press(&mut h, "Visibility Set");
        assert!(h.query_by_label("No walls").is_none(), "rev {rev}");
        key(&mut h, Key::Escape);
    }
}
