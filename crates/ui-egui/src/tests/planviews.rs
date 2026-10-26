//! Plan views besides a level's own, driven as a user drives them: Duplicate View on the View tab, DV, the Project Browser's
//! right-click menu, and annotation made in one.

use super::*;
use egui_kittest::kittest::NodeT;

const HOUSE: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
door {"at": [4000, 0]}
level {"elevation": 3000}"#;

fn views(h: &Harness<BimApp>) -> Vec<(Id, String)> {
    let m = &h.state().session.model;
    let plan_views = m.elements.iter().filter(|(_, e)| matches!(e, Element::PlanView(_)));
    plan_views.map(|(id, _)| (*id, m.view_name(*id).unwrap_or_default().to_string())).collect()
}

/// The first entry named `label` in the Project Browser: a button left of the views' tabs (Properties names a view too, but in a
/// label).
fn in_browser<'a>(h: &'a Harness<BimApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let entries = h.get_all_by_label(label).filter(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button);
    entries.min_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y)).unwrap()
}

#[test]
fn duplicate_view_on_the_view_tab_makes_a_plan_view_and_opens_it() {
    let mut h = app(HOUSE);
    press(&mut h, "View");
    press(&mut h, "Duplicate View");
    press(&mut h, "Duplicate");
    let made = views(&h);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].1, "Level 1 Copy 1");
    assert_eq!(h.state().sheet, Sheet::Plan(made[0].0), "the copy is the view on screen");
    // With detailing, and as a dependent, from the copy now on screen.
    press(&mut h, "Duplicate View");
    press(&mut h, "Duplicate as Dependent");
    let names: Vec<String> = views(&h).into_iter().map(|(_, n)| n).collect();
    assert!(names.contains(&"Level 1 Copy 1 Dependent 1".to_string()), "{names:?}");
    assert_eq!(h.state().sheet, Sheet::Plan(views(&h).iter().find(|(_, n)| n.ends_with("Dependent 1")).unwrap().0), "and it is on screen");
}

#[test]
fn dp_duplicates_the_plan_on_screen_and_a_ceiling_plan_stays_a_ceiling_plan() {
    let mut h = app(HOUSE);
    shortcut(&mut h, "DP");
    assert_eq!(views(&h).len(), 1);
    let copy = views(&h)[0].0;
    h.state_mut().sheet = Sheet::Ceiling(copy);
    h.run();
    shortcut(&mut h, "DP");
    assert_eq!(views(&h).len(), 2);
    assert!(matches!(h.state().sheet, Sheet::Ceiling(id) if id != copy), "{:?}", h.state().sheet);
    // Not on a plan: it says so.
    h.state_mut().sheet = Sheet::View3D;
    h.run();
    shortcut(&mut h, "DP");
    assert_eq!(views(&h).len(), 2);
    assert!(h.state().status.error && h.state().status.text.contains("open a plan"), "{}", h.state().status.text);
}

#[test]
fn tag_all_in_a_duplicate_tags_that_view_and_not_the_levels_plan() {
    let mut h = app(HOUSE);
    shortcut(&mut h, "DP");
    let copy = views(&h)[0].0;
    press(&mut h, "Annotate");
    press(&mut h, "Tag All");
    let guid = h.state().session.model.guid(copy);
    let tags: Vec<Option<_>> = h
        .state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Tag(t) => Some(t.view),
            _ => None,
        })
        .collect();
    assert_eq!(tags, vec![Some(guid)], "the door's tag is in the copy");
}

#[test]
fn the_project_browser_lists_plan_views_and_deletes_one_from_its_right_click_menu() {
    let mut h = app(HOUSE);
    shortcut(&mut h, "DP");
    let (copy, name) = views(&h).remove(0);
    h.state_mut().sheet = Sheet::Plan(h.state().session.model.levels()[0].0);
    h.run();
    // Listed under Floor Plans and under Ceiling Plans, beside its tab.
    assert_eq!(h.get_all_by_label(&name).count(), 3);
    // A right click offers the duplicates, and Delete for a plan view.
    in_browser(&h, &name).click_secondary();
    h.run();
    for item in ["Duplicate", "Duplicate with Detailing", "Duplicate as Dependent", "Delete"] {
        assert!(h.query_by_label(item).is_some(), "{item}");
    }
    press(&mut h, "Delete");
    assert!(views(&h).is_empty() && h.state().session.model.elements.get(copy).is_none());
}

#[test]
fn a_levels_own_plan_has_no_delete_in_the_browser() {
    let mut h = app(HOUSE);
    in_browser(&h, "Level 1").click_secondary();
    h.run();
    assert!(h.query_by_label("Duplicate").is_some());
    assert!(h.query_by_label("Delete").is_none(), "a level's plan goes with its level");
}

#[test]
fn a_plan_views_properties_name_its_level_and_a_dependents_name_its_parent() {
    let mut h = app(HOUSE);
    shortcut(&mut h, "DP");
    assert!(h.query_by_label("View Name").is_some());
    assert!(h.query_by_label("Cut Plane").is_some(), "an independent view has its own range");
    press(&mut h, "View");
    press(&mut h, "Duplicate View");
    press(&mut h, "Duplicate as Dependent");
    h.run();
    assert!(h.query_by_label("Parent View").is_some());
    assert!(h.query_by_label("Cut Plane").is_none(), "a dependent follows its parent's range");
    // Visibility/Graphics is its parent's to open.
    let vg = h.get_by_label("Visibility/Graphics");
    assert!(vg.accesskit_node().is_disabled());
}

#[test]
fn a_plan_in_the_window_carries_no_title_of_its_own_as_the_tab_names_it() {
    let s = app(HOUSE);
    let model = Arc::clone(&s.state().session.model);
    let level = model.levels()[0].0;
    let view = bimcraft_core::render::plan(&model, level, 800, 600).view;
    let mut cache = crate::canvas::PlanCache::default();
    for ceiling in [false, true] {
        let d = cache.drawing(&model, level, view, ceiling);
        assert_ne!(d.owners.last(), Some(&None), "nothing is lettered at its bottom left (ceiling: {ceiling})");
    }
}

#[test]
fn a_plan_paints_the_scan_it_sees() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("floor.xyz");
    let floor: String =
        (0..=50).flat_map(|i| (0..=50).map(move |k| format!("{} {} 0.5 255 0 0\n", f64::from(i) * 0.04, f64::from(k) * 0.04))).collect();
    std::fs::write(&path, floor).unwrap();
    let mut h = app(&format!("{HOUSE}\npoint_cloud {}", json!({"path": path, "origin": "file"})));
    h.run();
    let loaded = h.state().canvas.drawing.pictures.as_ref().map(|(_, t)| t.len());
    assert_eq!(loaded, Some(1), "the scan's picture is on the card");
}

#[test]
fn a_wall_drawn_over_a_scan_snaps_to_where_the_plan_cuts_it_unless_manage_snaps_says_not() {
    use bimcraft_core::snap::Kind;
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-snap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("face.xyz");
    // A scanned wall face a metre south of the house (clear of the Options Bar floating over the view's top), along x from
    // 2 m to 6 m and 2 m tall.
    let face: String =
        (0..=100).flat_map(|i| (0..=50).map(move |k| format!("{} -1 {}\n", 2.0 + f64::from(i) * 0.04, f64::from(k) * 0.04))).collect();
    std::fs::write(&path, face).unwrap();
    let mut h = app(&format!("{HOUSE}\npoint_cloud {}", json!({"path": path, "origin": "file"})));
    tool(&mut h, "Wall");
    let f = hover(&mut h, P2(4010.0, -970.0)).unwrap();
    assert_eq!((f.kind, f.at), (Kind::Point, P2(4000.0, -1000.0)));
    key(&mut h, egui::Key::Escape);
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    pick(&mut h, Role::CheckBox, "Point Cloud");
    press(&mut h, "Close");
    assert!(!h.state().snaps.cloud);
    tool(&mut h, "Wall");
    assert_ne!(hover(&mut h, P2(4010.0, -970.0)).map(|f| f.kind), Some(Kind::Point));
}
