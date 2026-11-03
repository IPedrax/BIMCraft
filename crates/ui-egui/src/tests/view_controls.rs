//! The view control bar's view controls, the same in both profiles: the renovation filter, which phase of the work a view
//! shows in plain words; visibility sets, view templates of what a view shows switched at a click; and the 3D view linked to
//! the plan, cut to what the plan shows.

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

/// The section box the 3D view is drawn cut to, if it is.
fn drawn_box(h: &mut Harness<BimApp>) -> Option<([f64; 3], [f64; 3])> {
    let app = h.state_mut();
    let was = std::mem::replace(&mut app.sheet, Sheet::View3D);
    let model = app.view_model();
    app.sheet = was;
    bimcraft_core::graphics::Look::of_3d(&model).section_box()
}

#[test]
fn the_linked_3d_view_floats_beside_the_plan_cut_to_what_the_plan_shows_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        let project = Arc::clone(&h.state().session.model);
        assert!(drawn_box(&mut h).is_none(), "rev {rev}");
        press(&mut h, "Linked 3D View");
        // Beside the plan, which stays the view the tools work in.
        assert!(h.state().floats(Sheet::View3D) && h.state().sheet == Sheet::Plan(100000), "rev {rev}");
        // Cut from Level 1 to its view range's top, Level 2, and round what the plan shows: the house, fitted in it.
        let (lo, hi) = drawn_box(&mut h).expect("the 3D view cut to the plan");
        assert_eq!((lo[2], hi[2]), (0.0, 3000.0), "rev {rev}");
        assert!(lo[0] < 0.0 && hi[0] > 8000.0 && lo[1] < 0.0 && hi[1] > 6000.0, "rev {rev}: {lo:?} {hi:?}");
        // The plan panned off the house: the box goes with it.
        h.state_mut().canvas.show(100000, P2(40000.0, 40000.0), P2(50000.0, 46000.0));
        h.run();
        let (lo, hi) = drawn_box(&mut h).expect("still cut");
        assert!(lo[0] > 8000.0 && lo[0] < 45000.0 && hi[0] > 45000.0 && lo[1] > 6000.0, "rev {rev}: {lo:?} {hi:?}");
        // Another level's plan: that level, and up to the model's top over the highest.
        h.state_mut().open_sheet(Sheet::Plan(100001));
        h.run();
        let (lo, hi) = drawn_box(&mut h).expect("still cut");
        assert!(lo[2] == 3000.0 && hi[2] > lo[2], "rev {rev}: {lo:?} {hi:?}");
        // Off: the 3D view as it was, closed again and never cut; the project was never changed.
        press(&mut h, "Linked 3D View");
        assert!(!h.state().floats(Sheet::View3D) && !h.state().views.contains(&Sheet::View3D), "rev {rev}");
        assert!(drawn_box(&mut h).is_none(), "rev {rev}");
        assert!(Arc::ptr_eq(&project, &h.state().session.model), "rev {rev}: no command ran");
    }
}

#[test]
fn the_linked_3d_view_keeps_its_own_section_box_and_its_tab_for_when_the_link_ends() {
    let mut h = app(BOX);
    h.state_mut().run("section_box", json!({"min": [-500, -500, -500], "max": [500, 500, 500]}));
    h.state_mut().open_sheet(Sheet::View3D);
    h.state_mut().open_sheet(Sheet::Plan(100000));
    h.run();
    press(&mut h, "Linked 3D View");
    assert!(h.state().floats(Sheet::View3D));
    assert_eq!(drawn_box(&mut h).map(|b| b.1[2]), Some(3000.0), "cut to the plan, not to its own box");
    // Closed by its window's close button, the link ends with it.
    press(&mut h, "Close {3D}");
    assert!(h.state().linked_3d.is_none());
    // On again from a tab, and off: back on its tab, its own box as it was.
    h.state_mut().open_sheet(Sheet::View3D);
    h.state_mut().open_sheet(Sheet::Plan(100000));
    h.run();
    press(&mut h, "Linked 3D View");
    assert!(h.state().floats(Sheet::View3D) && !h.state().views.contains(&Sheet::View3D));
    press(&mut h, "Linked 3D View");
    assert!(!h.state().floats(Sheet::View3D) && h.state().views.contains(&Sheet::View3D));
    assert_eq!(h.state().sheet, Sheet::Plan(100000), "the plan still the view the tools work in");
    assert_eq!(drawn_box(&mut h), Some(([-500.0; 3], [500.0; 3])));
}
