//! The Renovation palette: the status of the work new elements are drawn in (existing, to demolish, new, or as the view says),
//! that status given to the selection in one undo step, and which phase of the work the view shows, in both profiles.

use super::*;

use crate::dock::Palette;

/// The phases element `id` is made and taken down in, as it holds them: none for the project's first.
fn phases(h: &Harness<BimApp>, id: Id) -> (Option<Value>, Option<Value>) {
    (param(h, id, "Phase Created"), param(h, id, "Phase Demolished"))
}

fn palette(script: &str, rev: bool) -> Harness<'static, BimApp> {
    let mut h = if rev { wide(script) } else { bimcraft(script) };
    h.state_mut().show_palette(Palette::Renovation, true);
    h.run();
    h
}

/// The palette's button `label` pressed, scrolled into sight first: the palette shares its side with others.
fn push(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

/// Whether the palette's button `label` is the one chosen.
fn chosen(h: &Harness<BimApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

const NEW: &str = "New Construction";

#[test]
fn a_wall_drawn_to_demolish_stands_before_the_work_and_comes_down_in_it_in_both_profiles() {
    for rev in [false, true] {
        let mut h = palette(BOX, rev);
        // At first it follows the view, as the tools always have: a view of no phase of its own makes nothing phased.
        assert!(chosen(&h, "Follow the View"), "rev {rev}");
        push(&mut h, "Draw To Demolish");
        assert_eq!(h.state().drawn_as, Some(bimcraft_core::phasing::Status::Demolished), "rev {rev}");
        let down = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
        assert_eq!(phases(&h, down), (None, Some(json!(NEW))), "rev {rev}: the first phase, taken down in the work");
        // It stays chosen in any view, until another is.
        push(&mut h, "Draw New");
        let new = draw_wall(&mut h, P2(2000.0, 3000.0), P2(6000.0, 3000.0));
        assert_eq!(phases(&h, new), (Some(json!(NEW)), None), "rev {rev}");
        push(&mut h, "Draw Existing");
        let old = draw_wall(&mut h, P2(2000.0, 4500.0), P2(6000.0, 4500.0));
        assert_eq!(phases(&h, old), (None, None), "rev {rev}");
        // Following the view again: the view's phase, never a demolition.
        push(&mut h, "Draw To Demolish");
        push(&mut h, "Follow the View");
        let plain = draw_wall(&mut h, P2(1000.0, 1000.0), P2(1000.0, 5000.0));
        assert_eq!(phases(&h, plain), (None, None), "rev {rev}");
        assert!(!h.state().session.history().contains(&"Renovation"), "rev {rev}: choosing how to draw is no undo step");
    }
}

#[test]
fn a_status_drawn_in_is_read_against_the_phase_the_view_shows() {
    let mut h = palette(&format!("{BOX}\nphase {{\"name\": \"Phase 3\"}}"), false);
    h.state_mut().run("graphics", json!({"view": 100000, "phase": NEW}));
    h.run();
    push(&mut h, "Draw To Demolish");
    let down = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
    assert_eq!(phases(&h, down), (None, Some(json!(NEW))), "the view's phase is the work, the one before it what stands");
    push(&mut h, "Draw New");
    let new = draw_wall(&mut h, P2(2000.0, 3000.0), P2(6000.0, 3000.0));
    assert_eq!(phases(&h, new), (Some(json!(NEW)), None));
}

#[test]
fn the_selection_is_given_a_status_in_one_undo_step_and_says_the_one_it_has_in_both_profiles() {
    for rev in [false, true] {
        let mut h = palette(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"), rev);
        let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
        let door = find(&h, |e| matches!(e, Element::Door(_)));
        // Nothing selected: nothing to give.
        assert!(is_disabled(&h, Role::Button, "Set New"), "rev {rev}");
        h.state_mut().selection = vec![wall, door];
        h.run();
        assert!(h.query_by_label("Selection: Existing").is_some(), "rev {rev}");
        let steps = h.state().session.history().len();
        push(&mut h, "Set New");
        assert_eq!(phases(&h, wall), (Some(json!(NEW)), None), "rev {rev}");
        assert_eq!(phases(&h, door), (Some(json!(NEW)), None), "rev {rev}: the door its own phase");
        assert_eq!(h.state().session.history().len(), steps + 1, "rev {rev}");
        assert_eq!(h.state().session.history().last(), Some(&"Renovation"), "rev {rev}");
        assert!(h.query_by_label("Selection: New").is_some(), "rev {rev}");
        // The door alone, to come down: the wall stays new, and the two are mixed.
        h.state_mut().selection = vec![door];
        h.run();
        push(&mut h, "Set To Demolish");
        assert_eq!(phases(&h, door).1, Some(json!(NEW)), "rev {rev}");
        h.state_mut().selection = vec![wall, door];
        h.run();
        assert!(h.query_by_label("Selection: Mixed").is_some(), "rev {rev}");
        // One undo takes the door back up; another, both back to what stood.
        h.state_mut().run("undo", json!({"count": 2}));
        h.run();
        assert_eq!((phases(&h, wall), phases(&h, door)), ((None, None), (None, None)), "rev {rev}");
    }
}

#[test]
fn the_palette_sets_which_phase_of_the_work_the_view_shows() {
    let mut h = palette(BOX, false);
    let phasing = |h: &Harness<BimApp>| {
        let g = plan_graphics(h, 100000);
        (g.phase, g.phase_filter)
    };
    assert!(chosen(&h, "Show All"));
    push(&mut h, "Show Demolition");
    assert_eq!(phasing(&h), (None, Some("Show Previous + Demolished".into())));
    assert_eq!(h.state().session.history().last(), Some(&"Visibility/Graphics"));
    push(&mut h, "Show Existing");
    assert_eq!(phasing(&h), (Some("Existing".into()), None));
    push(&mut h, "Show New");
    assert_eq!(phasing(&h), (None, Some("Show Previous + New".into())));
    push(&mut h, "Show All");
    assert_eq!(phasing(&h), (None, None));
}

#[test]
fn a_project_of_one_phase_greys_the_palette_out_and_says_what_it_needs() {
    let mut h = palette(&format!("phase {{\"name\": \"Existing\", \"delete\": true}}\n{BOX}"), false);
    for b in ["Draw Existing", "Draw To Demolish", "Draw New"] {
        assert!(is_disabled(&h, Role::Button, b), "{b}");
    }
    assert!(h.query_by_label_contains("one phase").is_some());
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.run();
    assert!(is_disabled(&h, Role::Button, "Set New"));
}

#[test]
fn the_ribbon_and_the_user_interface_menu_open_the_palette() {
    let mut h = wide(BOX);
    assert!(!h.state().docking.shown(Palette::Renovation), "closed in a new window");
    topmost(&mut h, "Modify");
    push(&mut h, "Renovation Palette");
    assert!(h.state().docking.shown(Palette::Renovation));
    assert!(h.query_by_label("Draw To Demolish").is_some());
    push(&mut h, "Renovation Palette");
    assert!(!h.state().docking.shown(Palette::Renovation));
}

#[test]
fn a_status_set_through_the_control_port_is_the_one_the_palette_shows() {
    let mut h = palette(BOX, false);
    h.state_mut().run("renovation", json!({"status": "new"}));
    h.run();
    assert_eq!(h.state().drawn_as, Some(bimcraft_core::phasing::Status::New));
    let new = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
    assert_eq!(phases(&h, new), (Some(json!(NEW)), None));
}

/// What the palette looks like, for a person to look at: `BIMCRAFT_RENOVATION_DUMP=<folder>` writes the window with the palette
/// open, a wall drawn to demolish and selected, in both profiles, light and dark, in English and Italian.
#[test]
fn renovation_palette_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_RENOVATION_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for rev in [false, true] {
        for (dark, look) in [(false, "light"), (true, "dark")] {
            for lang in [crate::lang::Lang::English, crate::lang::Lang::Italian] {
                let mut h = palette(BOX, rev);
                push(&mut h, "Draw To Demolish");
                let wall = draw_wall(&mut h, P2(2000.0, 1500.0), P2(6000.0, 1500.0));
                h.state_mut().selection = vec![wall];
                let ctx = h.ctx.clone();
                h.state_mut().set_lang(&ctx, lang);
                if dark {
                    h.ctx.set_theme(egui::Theme::Dark);
                }
                h.run();
                let name = format!("renovation_{}_{look}_{lang:?}.png", if rev { "rev" } else { "bimcraft" });
                h.render().unwrap().save(std::path::Path::new(&folder).join(name)).unwrap();
            }
        }
    }
}

/// What the view control bar's Renovation menu shows, the setting it holds.
fn menu_shows(h: &Harness<BimApp>) -> String {
    h.get_by_role_and_label(Role::Button, "Renovation").value().unwrap_or_default()
}

#[test]
fn the_comparative_shows_every_status_overridden_and_makes_its_filter_once() {
    use bimcraft_core::phasing::{PhaseFilter, Presentation::Overridden as O};
    let all = PhaseFilter { new: O, existing: O, demolished: O, temporary: O };
    let mut h = palette(BOX, false);
    let filters = |h: &Harness<BimApp>| h.state().session.model.phase_filters.len();
    let before = filters(&h);
    // The project starts with no filter that shows it: the first pick makes one, in the same undo step.
    let steps = h.state().session.history().len();
    push(&mut h, "Show Comparative");
    let g = plan_graphics(&h, 100000);
    assert_eq!((g.phase, g.phase_filter.as_deref()), (None, Some("Comparative")));
    assert_eq!(h.state().session.model.phase_filters.get("Comparative"), Some(&all));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert!(chosen(&h, "Show Comparative"));
    assert_eq!(menu_shows(&h), "Comparative");
    // Picked again, it finds the filter it made.
    push(&mut h, "Show All");
    push(&mut h, "Show Comparative");
    assert_eq!(filters(&h), before + 1);
    // Undone, the view and the filter go together.
    h.state_mut().run("undo", json!({"count": 3}));
    h.run();
    assert_eq!((plan_graphics(&h, 100000).phase_filter, filters(&h)), (None, before));
}

#[test]
fn the_comparative_finds_a_projects_filter_by_what_it_shows_whatever_its_name() {
    let script = format!(
        "{BOX}\nphase_filter {{\"name\": \"Comparativo\", \"new\": \"overridden\", \"existing\": \"overridden\", \"demolished\": \"overridden\", \"temporary\": \"overridden\"}}"
    );
    let mut h = app(&script);
    let filters = h.state().session.model.phase_filters.len();
    // From the view control bar's menu, in Rev.
    press(&mut h, "Renovation");
    press(&mut h, "Comparative");
    assert_eq!(plan_graphics(&h, 100000).phase_filter.as_deref(), Some("Comparativo"));
    assert_eq!(h.state().session.model.phase_filters.len(), filters, "no filter made");
    assert_eq!(menu_shows(&h), "Comparative");
    assert_eq!(h.state().session.history().last(), Some(&"Visibility/Graphics"));
}
