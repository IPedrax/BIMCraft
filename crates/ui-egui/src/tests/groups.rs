//! Detail groups, repeating details and the annotation parameters in Properties: what UG undoes in one step, what a spacing
//! that isn't one makes, which rows a detail line shows, and the fill patterns in the materials window.

use egui_kittest::kittest::NodeT;

use super::*;

fn grouped(h: &mut Harness<BimApp>, at: P2) {
    click(h, at);
    shortcut(h, "GP");
    key(h, Key::Escape);
}

#[test]
fn ug_takes_every_group_selected_apart_in_one_undo_step() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [5000, 3000], \"end\": [7000, 3000]}}"
    ));
    grouped(&mut h, P2(2000.0, 1000.0));
    grouped(&mut h, P2(6000.0, 3000.0));
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (2, 2));
    click(&mut h, P2(2000.0, 1000.0));
    click_with(&mut h, P2(6000.0, 3000.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    let steps = h.state().session.history().len();
    shortcut(&mut h, "UG");
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (0, 2));
    assert_eq!(h.state().session.history().len(), steps + 1, "one step for both");
    assert_eq!(h.state().selection.len(), 2, "their lines, selected");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (2, 2), "one Ctrl+Z puts both back");
}

#[test]
fn a_spacing_that_is_not_a_length_makes_no_repeating_detail_and_says_so() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [1000, 1200]}}"));
    grouped(&mut h, P2(1000.0, 1100.0));
    press(&mut h, "Annotate");
    press(&mut h, "Repeating Detail");
    type_into(&mut h, "Spacing:", "often");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3000.0, 3000.0));
    assert!(ids(&h, "repeatingdetail").is_empty());
    assert!(h.state().status.error && h.state().status.text.contains("often"), "{}", h.state().status.text);
}

fn parameters(h: &mut Harness<BimApp>, id: Id) {
    h.state_mut().selection = vec![id];
    h.run();
}

fn shown(h: &Harness<BimApp>, label: &str) -> bool {
    h.query_all_by_label(label).count() > 0
}

#[test]
fn a_detail_line_shows_the_rows_that_mean_something_for_it() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [1000, 2000], \"end\": [3000, 2000], \"arc\": 90}}\ndetail_line {{\"start\": [1000, 4000], \"end\": [3000, 4000], \"cloud\": 500, \"revision\": \"R1\"}}"
    ));
    let lines = ids(&h, "line");
    // A straight line: a style and a length, no centre mark or revision to switch.
    parameters(&mut h, lines[0]);
    assert!(shown(&h, "Line Style") && shown(&h, "Length"));
    assert!(!shown(&h, "Center Mark Visible") && !shown(&h, "Revision"), "nothing a straight line could use");
    // An arc has a centre to mark.
    parameters(&mut h, lines[1]);
    assert!(shown(&h, "Center Mark Visible") && !shown(&h, "Revision"));
    // A cloud has an arc length and a revision.
    parameters(&mut h, lines[2]);
    assert!(shown(&h, "Revision") && shown(&h, "Arc Length") && !shown(&h, "Center Mark Visible"));
    assert_eq!(field_text(&h, "Revision"), "R1");
}

#[test]
fn a_regions_pattern_angle_a_groups_rotation_and_a_repeats_spacing_show_at_their_defaults() {
    let mut h = app(&format!(
        "{BOX}\nfilled_region {{\"sketch\": [[[1000,1000],[3000,1000]],[[3000,1000],[3000,3000]],[[3000,3000],[1000,3000]],[[1000,3000],[1000,1000]]]}}\ndetail_line {{\"start\": [5000, 1000], \"end\": [5000, 1200]}}"
    ));
    let region = ids(&h, "region")[0];
    parameters(&mut h, region);
    assert_eq!(field_text(&h, "Pattern Angle (°)"), "0", "the file leaves a zero out, the row doesn't");
    click(&mut h, P2(5000.0, 1100.0));
    shortcut(&mut h, "GP");
    key(&mut h, Key::Escape);
    let group = ids(&h, "detailgroup")[0];
    parameters(&mut h, group);
    assert_eq!(field_text(&h, "Rotation (°)"), "0");
    h.state_mut()
        .session
        .execute("repeating_detail", &json!({"type": "Group 1", "start": [1000, 5000], "end": [3000, 5000], "spacing": 500}))
        .unwrap();
    let repeat = ids(&h, "repeatingdetail")[0];
    parameters(&mut h, repeat);
    assert_eq!(field_text(&h, "Spacing"), "500");
    assert!(shown(&h, "Number"));
}

#[test]
fn the_materials_window_offers_the_projects_fill_patterns_besides_its_own() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Materials");
    press(&mut h, "New");
    press(&mut h, "Custom");
    choose(&mut h, "Cut Foreground Pattern", "Sand");
    let m = &h.state().session.model.materials["Material 1"];
    assert_eq!((m.cut_pattern.as_deref(), m.surface_pattern.as_deref()), (Some("Sand"), None));
    choose(&mut h, "Surface Foreground Pattern", "Tile 300");
    assert_eq!(h.state().session.model.materials["Material 1"].surface_pattern.as_deref(), Some("Tile 300"));
    // And the combo shows the one it has.
    assert_eq!(combo_value(&h, "Cut Foreground Pattern"), "Sand");
}

/// Three walls of a corner, up to Level 2, made a model group.
const CORNER: &str =
    "@w = wall {\"points\": [[0, 3000], [0, 0], [4000, 0], [4000, 3000]], \"top_level\": \"Level 2\"}\ncreate_group {\"ids\": \"@w.ids\"}";

#[test]
fn a_click_picks_the_group_and_tab_its_member() {
    let mut h = app(CORNER);
    let group = ids(&h, "modelgroup");
    assert_eq!(group.len(), 1);
    let walls = ids(&h, "wall");
    h.state_mut().selection.clear();
    h.run();
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, group, "the group, not its wall");
    // Over the wall, Tab steps from the group to the wall in it, and a click takes that.
    h.hover_at(h.state().screen(P2(2000.0, 0.0)));
    h.run();
    key(&mut h, Key::Tab);
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [walls[1]]);
    // A box round it takes the group whole.
    key(&mut h, Key::Escape);
    drag(&mut h, P2(-1000.0, -1000.0), P2(5000.0, 4000.0));
    assert_eq!(h.state().selection, group);
}

#[test]
fn exclude_and_restore_all_excluded_from_the_group_panel() {
    let mut h = wide(CORNER);
    let group = ids(&h, "modelgroup");
    let walls = ids(&h, "wall");
    h.state_mut().selection = vec![walls[0]];
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Exclude");
    assert_eq!(ids(&h, "wall").len(), 2, "the excluded wall goes");
    assert_eq!(ids(&h, "modelgroup"), group, "the group stays");
    h.state_mut().selection = group.clone();
    h.run();
    press(&mut h, "Restore All Excluded");
    assert_eq!(ids(&h, "wall").len(), 3);
}

#[test]
fn exclude_restore_edit_group_and_place_model_group_have_their_shortcuts() {
    let mut h = app(CORNER);
    let group = ids(&h, "modelgroup");
    let walls = ids(&h, "wall");
    // EM excludes the member selected; RA puts back what the group selected has excluded.
    h.state_mut().selection = vec![walls[0]];
    h.run();
    shortcut(&mut h, "EM");
    assert_eq!(ids(&h, "wall").len(), 2, "{}", h.state().status.text);
    h.state_mut().selection = group.clone();
    h.run();
    shortcut(&mut h, "RA");
    assert_eq!(ids(&h, "wall").len(), 3, "{}", h.state().status.text);
    // GE edits the group selected (EG is the Earthing Electrode's), and MG picks Place Model Group.
    h.state_mut().selection = group.clone();
    h.run();
    shortcut(&mut h, "GE");
    assert!(h.state().session.editing_group().is_some(), "{}", h.state().status.text);
    assert!(h.state_mut().run("cancel_group", json!({})).is_some());
    shortcut(&mut h, "MG");
    assert_eq!(h.state().tool, Tool::ModelGroup);
}

#[test]
fn create_group_is_in_bimcrafts_tools_palette_too() {
    let mut h = bimcraft(BOX);
    let walls = ids(&h, "wall");
    h.state_mut().selection = walls.clone();
    h.run();
    press(&mut h, "Create Group");
    assert_eq!(ids(&h, "modelgroup").len(), 1, "{}", h.state().status.text);
    assert_eq!(ids(&h, "wall"), walls, "its walls stay");
}

#[test]
fn edit_group_finish_changes_every_group_of_the_type() {
    let mut h = app(&format!("{CORNER}\ncopy {{\"ids\": [\"$last\"], \"by\": [10000, 0]}}"));
    let groups = ids(&h, "modelgroup");
    assert_eq!(groups.len(), 2, "two of one type");
    h.state_mut().selection = vec![groups[0]];
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Edit Group");
    assert!(h.state().session.editing_group().is_some());
    // In Edit Group a click on a member takes the member.
    click(&mut h, P2(2000.0, 0.0));
    let wall = h.state().selection.clone();
    assert_eq!(wall.len(), 1);
    assert!(h.state().session.model.wall(wall[0]).is_some());
    let r = h.state_mut().run("move", serde_json::json!({"ids": wall, "by": [0, -500]}));
    assert!(r.is_some(), "{}", h.state().status.text);
    press(&mut h, "Finish Edit Group");
    assert!(h.state().session.editing_group().is_none());
    // The copy's wall moved too.
    let m = &h.state().session.model;
    let walls: Vec<(P2, P2)> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((w.start, w.end)),
            _ => None,
        })
        .collect();
    assert!(walls.iter().any(|(a, _)| *a == P2(10000.0, -500.0)), "the other group follows: {walls:?} {}", h.state().status.text);
}

#[test]
fn a_member_changed_on_its_own_asks_ungroup_create_new_type_or_cancel() {
    let mut h = app(CORNER);
    let walls = ids(&h, "wall");
    let r = h.state_mut().run("move", serde_json::json!({"ids": [walls[0]], "by": [0, 500]}));
    assert!(r.is_none());
    h.run();
    assert!(h.state().group_conflict.is_some());
    assert!(h.query_by_label("Create New Type").is_some());
    press(&mut h, "Ungroup");
    assert!(h.state().group_conflict.is_none());
    assert!(ids(&h, "modelgroup").is_empty(), "ungrouped");
    assert_eq!(field(&h, walls[0], "start"), json!([0.0, 3500.0]), "the change kept");
    // Undone, the group is back; Create New Type makes it a type of its own.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(ids(&h, "modelgroup").len(), 1);
    h.state_mut().run("move", serde_json::json!({"ids": [walls[0]], "by": [0, 500]}));
    h.run();
    press(&mut h, "Create New Type");
    assert_eq!(h.state().session.model.model_groups.len(), 2);
    // Cancel leaves everything as it was.
    h.state_mut().run("move", serde_json::json!({"ids": [walls[1]], "by": [0, 500]}));
    h.run();
    press(&mut h, "Cancel");
    assert!(h.state().group_conflict.is_none());
    assert_eq!(field(&h, walls[1], "start"), json!([0.0, 0.0]));
}

#[test]
fn create_group_is_offered_for_a_selection_of_one_kind_only() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}"));
    let walls = ids(&h, "wall");
    let line = ids(&h, "line");
    h.state_mut().selection = vec![walls[0], line[0]];
    h.run();
    assert_eq!(h.state().groupable(), None, "a wall and a line are two kinds");
    h.state_mut().selection = walls.clone();
    h.run();
    assert_eq!(h.state().groupable(), Some(bimcraft_core::groups::Kind::Model));
    shortcut(&mut h, "GP");
    assert_eq!(ids(&h, "modelgroup").len(), 1);
    assert_eq!(ids(&h, "wall"), walls, "its walls stay");
    h.state_mut().selection = line.clone();
    h.run();
    assert_eq!(h.state().groupable(), Some(bimcraft_core::groups::Kind::Detail));
}

#[test]
fn create_group_of_a_mixed_selection_asks_which_kind_and_leaves_the_rest_out() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}"));
    let walls = ids(&h, "wall");
    let line = ids(&h, "line");
    // Both kinds and a level: Create Group is still offered, and asks.
    let mixed: Vec<Id> = walls.iter().copied().chain(line.iter().copied()).chain([100000]).collect();
    h.state_mut().selection = mixed.clone();
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Create Group");
    let split = h.state().group_split.clone().unwrap_or_else(|| panic!("no question: {}", h.state().status.text));
    assert_eq!((split.model.clone(), split.detail.clone()), (walls.clone(), line.clone()));
    assert!(ids(&h, "modelgroup").is_empty(), "nothing made before the answer");
    press(&mut h, &format!("Model elements only ({})", walls.len()));
    assert!(h.state().group_split.is_none());
    assert_eq!(ids(&h, "modelgroup").len(), 1);
    assert_eq!(bimcraft_core::groups::group_of(&h.state().session.model, line[0]), None, "the line left out");
    // Undone, GP asks the same, and the detail elements make a detail group.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    h.state_mut().selection = mixed;
    h.run();
    shortcut(&mut h, "GP");
    press(&mut h, "Detail elements only (1)");
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "modelgroup").len()), (1, 0));
    // Cancel makes nothing.
    h.state_mut().selection = walls.iter().copied().chain([100000]).collect();
    h.run();
    shortcut(&mut h, "GP");
    assert!(h.state().group_split.is_some());
    press(&mut h, "Cancel");
    assert!(h.state().group_split.is_none() && ids(&h, "modelgroup").is_empty());
}

#[test]
fn in_edit_group_only_members_pick_and_add_and_remove_are_picks() {
    let mut h = app(&format!("{CORNER}\nwall {{\"points\": [[0, -3000], [4000, -3000]]}}"));
    let group = ids(&h, "modelgroup");
    let walls = ids(&h, "wall");
    let loose = *walls.last().unwrap_or_else(|| panic!("no wall"));
    h.state_mut().selection = group.clone();
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Edit Group");
    // The wall outside the group doesn't pick; a member does, as itself.
    click(&mut h, P2(2000.0, -3000.0));
    assert!(h.state().selection.is_empty(), "the loose wall is greyed: {:?}", h.state().selection);
    drag(&mut h, P2(-1000.0, -4000.0), P2(5000.0, -2000.0));
    assert!(h.state().selection.is_empty(), "a box round it takes nothing");
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [walls[1]]);
    // Whatever selects it elsewhere, the selection keeps only the group's.
    h.state_mut().selection = vec![loose, walls[1]];
    h.run();
    assert_eq!(h.state().selection, [walls[1]]);
    // Add picks the loose wall into the group.
    press(&mut h, "Add");
    assert_eq!(h.state().group_pick, Some(crate::groups::GroupPick::Add));
    assert!(h.state().selection.is_empty());
    click(&mut h, P2(2000.0, -3000.0));
    assert!(h.state().session.group_scope().is_some_and(|s| s.editable(loose)), "{}", h.state().status.text);
    assert!(h.state().selection.is_empty(), "added, not selected");
    // Esc stops adding, the edit going on; Remove picks a member out.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().group_pick, None);
    assert!(h.state().session.editing_group().is_some());
    press(&mut h, "Remove");
    click(&mut h, P2(0.0, 1500.0));
    assert!(h.state().session.group_scope().is_some_and(|s| !s.editable(walls[0])));
    press(&mut h, "Finish Edit Group");
    assert_eq!(h.state().group_pick, None);
    let members = bimcraft_core::groups::members(&h.state().session.model, group[0]);
    assert!(members.contains(&loose) && !members.contains(&walls[0]), "{members:?}");
}

/// The colours of the fills the last frame painted.
fn fills(h: &Harness<BimApp>) -> Vec<egui::Color32> {
    fn walk(s: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match s {
            egui::Shape::Path(p) => out.push(p.fill),
            egui::Shape::Mesh(m) => out.extend(m.vertices.iter().map(|v| v.color)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn a_group_under_the_pointer_lights_its_members_before_tab_steps_into_it() {
    let mut h = app(CORNER);
    let group = ids(&h, "modelgroup");
    let walls = ids(&h, "wall");
    h.state_mut().selection.clear();
    h.run();
    let lit = crate::style::canvas_pal().hover_fill;
    h.hover_at(h.state().screen(P2(2000.0, 4500.0)));
    h.run();
    assert!(!fills(&h).contains(&lit), "nothing under the pointer, nothing lit");
    h.hover_at(h.state().screen(P2(2000.0, 0.0)));
    h.run();
    h.run();
    assert_eq!(h.state().canvas.under.first(), group.first(), "the group is what a click takes");
    assert!(fills(&h).contains(&lit), "its walls light in the hover colour");
    key(&mut h, Key::Tab);
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [walls[1]]);
}

#[test]
fn in_3d_a_click_takes_a_group_lit_whole_and_edit_group_greys_and_blocks_the_rest() {
    let mut h = app(&format!("{CORNER}\ncopy {{\"ids\": [\"$last\"], \"by\": [10000, 0]}}"));
    let groups = ids(&h, "modelgroup");
    h.state_mut().selection.clear();
    press(&mut h, "Default 3D View");
    let mine = bimcraft_core::groups::members(&h.state().session.model, groups[0]);
    let other = bimcraft_core::groups::members(&h.state().session.model, groups[1]);
    let at = h.state().screen3([2000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [groups[0]], "the group, not its wall");
    h.hover_at(h.state().orbit.rect.min + vec2(4.0, 4.0));
    h.run();
    h.run();
    let accent = crate::orbit::rgb(crate::style::canvas_pal().accent);
    let tints = h.state().orbit.tints_shown();
    for id in &mine {
        assert!(tints.contains(&(*id, accent)), "member {id} lit: {tints:?}");
    }
    // In Edit Group the other group's walls are greyed, and a click on one takes nothing.
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Edit Group");
    h.run();
    h.run();
    let tints = h.state().orbit.tints_shown();
    let grey = crate::groups::grey();
    for id in &other {
        assert!(tints.contains(&(*id, grey)), "the other group's {id} greyed: {tints:?}");
    }
    assert!(mine.iter().all(|id| !tints.iter().any(|(t, c)| t == id && *c == grey)), "the group's own not greyed");
    let at = h.state().screen3([12000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert!(h.state().selection.is_empty(), "the other group doesn't pick: {:?}", h.state().selection);
    let at = h.state().screen3([2000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection.len(), 1);
    assert!(mine.contains(&h.state().selection[0]), "a member picks as itself");
}

#[test]
fn place_model_group_puts_one_where_clicked() {
    let mut h = app(CORNER);
    h.state_mut().selection.clear();
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Model Group");
    assert_eq!(h.state().tool, Tool::ModelGroup);
    click(&mut h, P2(6000.0, 1000.0));
    assert_eq!(ids(&h, "modelgroup").len(), 2);
    assert_eq!(ids(&h, "wall").len(), 6);
}

/// What groups look like, for a person to look at: `BIMCRAFT_GROUPS_DUMP=<folder>` writes a group selected with the Group panel,
/// Edit Group's panel and the conflict dialog, light and dark.
#[test]
fn group_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_GROUPS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap_or_default();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            if let Ok(image) = h.render() {
                let _ = image.save(std::path::Path::new(&folder).join(format!("groups-{name}-{look}.png")));
            }
        };
        let mut h = app(&format!("{CORNER}\ncopy {{\"ids\": [\"$last\"], \"by\": [6000, 0]}}"));
        let groups = ids(&h, "modelgroup");
        h.state_mut().selection = vec![groups[0]];
        h.state_mut().tab = crate::Tab::Modify;
        save(&mut h, "selected");
        // A member Tab-selected: only what varies by group instance is set in Properties.
        h.state_mut().run("parameter", json!({"name": "Fire Rating", "kind": "text", "categories": ["wall"]}));
        h.state_mut().run("parameter", json!({"name": "Comments", "kind": "text", "categories": ["wall"], "vary_by_group": true}));
        h.state_mut().selection = vec![ids(&h, "wall")[1]];
        let headings = ["Constraints", "Construction", "Structural", "Dimensions", "Identity Data", "Graphics", "Phasing", "Other"];
        h.state_mut().collapsed.extend(headings);
        if let Some(close) = h.query_by_label("Close Project Browser") {
            close.click();
        }
        save(&mut h, "member");
        super::press(&mut h, "Manage");
        h.state_mut().collapsed.clear();
        h.state_mut().selection.clear();
        h.run();
        super::press(&mut h, "Project Parameters");
        save(&mut h, "parameters");
        let closes: Vec<_> = h.query_all_by_label("Close").collect();
        if let Some(close) = closes.last() {
            close.click();
        }
        h.state_mut().tab = crate::Tab::Modify;
        h.state_mut().collapsed.clear();
        h.state_mut().selection = vec![groups[0]];
        h.state_mut().edit_group();
        h.state_mut().selection = vec![ids(&h, "wall")[1]];
        save(&mut h, "editing");
        h.state_mut().selection.clear();
        h.state_mut().group_pick = Some(crate::groups::GroupPick::Add);
        save(&mut h, "adding");
        h.state_mut().group_pick = None;
        // The 3D view in Edit Group: the rest greyed.
        h.state_mut().open_3d(None);
        save(&mut h, "editing-3d");
        h.state_mut().sheet = crate::Sheet::Plan(h.state().session.model.levels()[0].0);
        // A mixed selection: Create Group asks which kind.
        h.state_mut().run("cancel_group", json!({}));
        h.state_mut().run("detail_line", json!({"start": [1000, -2000], "end": [3000, -2000]}));
        h.state_mut().selection = ids(&h, "wall").into_iter().chain(ids(&h, "line")).collect();
        h.state_mut().create_group();
        save(&mut h, "mixed");
        h.state_mut().group_split = None;
        h.state_mut().selection.clear();
        let walls = ids(&h, "wall");
        h.state_mut().run("move", json!({"ids": [walls[0]], "by": [0, 500]}));
        save(&mut h, "conflict");
    }
}

#[test]
fn a_tab_selected_member_sets_only_what_varies_by_group_instance_in_properties() {
    let mut h = app(&format!(
        "{CORNER}\nparameter {{\"name\": \"Fire Rating\", \"kind\": \"text\", \"categories\": [\"wall\"]}}\nparameter {{\"name\": \"Comments\", \"kind\": \"text\", \"categories\": [\"wall\"]}}"
    ));
    let group = ids(&h, "modelgroup")[0];
    let walls = ids(&h, "wall");
    // Project Parameters says, for each, whether a grouped element's value is its own or its group type's.
    super::press(&mut h, "Manage");
    super::press(&mut h, "Project Parameters");
    h.get_by_role_and_label(Role::ComboBox, "Group values of Comments").click();
    h.run();
    let options: Vec<_> = h.query_all_by_label("Vary by group instance").collect();
    options.last().unwrap_or_else(|| panic!("no option")).click();
    h.run();
    let presses: Vec<_> = h.query_all_by_label("Apply").collect();
    presses.last().unwrap_or_else(|| panic!("no Apply")).click();
    h.run();
    assert!(h.state().session.model.parameters["Comments"].vary_by_group);
    assert!(!h.state().session.model.parameters["Fire Rating"].vary_by_group, "aligned per group type unless said");
    let closes: Vec<_> = h.query_all_by_label("Close").collect();
    closes.last().unwrap_or_else(|| panic!("no Close")).click();
    h.run();
    // Tab-selected, a member's Properties set what varies by group instance; the rest is its type's, greyed.
    h.state_mut().selection = vec![walls[1]];
    h.run();
    let name = h.state().session.model.model_groups.keys().next().cloned().unwrap_or_default();
    let note =
        format!("A member of group {name}: only the parameters that vary by group instance change here. The rest change in Edit Group.");
    assert!(h.query_by_label(&note).is_some(), "says why the rest is greyed");
    assert!(!h.get_by_role_and_label(Role::TextInput, "Comments").accesskit_node().is_disabled());
    assert!(h.query_by_role_and_label(Role::TextInput, "Fire Rating").is_none(), "its type's, shown and not set here");
    assert!(h.get_by_role_and_label(Role::TextInput, "Top Offset").accesskit_node().is_disabled(), "its own fields are its type's too");
    type_into(&mut h, "Comments", "east corner");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, walls[1], "params"), json!({"Comments": "east corner"}));
    assert!(h.state().group_conflict.is_none(), "no change to the group");
    assert_eq!(ids(&h, "modelgroup"), [group]);
    // In Edit Group, its members are free.
    h.state_mut().run("edit_group", json!({"id": group}));
    h.state_mut().selection = vec![walls[1]];
    h.run();
    assert!(h.query_by_label(&note).is_none());
    assert!(!h.get_by_role_and_label(Role::TextInput, "Fire Rating").accesskit_node().is_disabled());
    assert!(!h.get_by_role_and_label(Role::TextInput, "Top Offset").accesskit_node().is_disabled());
}
