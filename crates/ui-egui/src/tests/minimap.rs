//! The Navigator's minimap: the active plan drawn whole at the palette's top, clicked and dragged as a user does, in both
//! profiles.

use super::*;

const PICTURE: &str = "Minimap of the view";

/// The model point in the middle of the active view, mm.
fn centre(h: &Harness<BimApp>) -> P2 {
    let key = match h.state().sheet {
        Sheet::Plan(k) | Sheet::Ceiling(k) | Sheet::Section(k) => k,
        other => panic!("no plan or section open: {other:?}"),
    };
    let v = h.state().canvas.view(key).unwrap();
    P2(v.cx, v.cy)
}

/// Where model point `p` of the active view is on the minimap.
fn on_map(h: &Harness<BimApp>, p: P2) -> Pos2 {
    h.state().minimap.screen(p).unwrap()
}

#[test]
fn the_minimap_shows_the_plan_and_a_click_on_it_centres_the_view_there_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        assert!(h.query_by_label(PICTURE).is_none(), "rev {rev}: it starts folded, so the tree keeps its room");
        press(&mut h, "Minimap");
        assert!(h.query_by_label(PICTURE).is_some(), "rev {rev}: a plan has its minimap");
        let to = P2(1500.0, 4500.0);
        assert!(centre(&h).dist(to) > 1000.0, "rev {rev}: the view starts elsewhere");
        let at = on_map(&h, to);
        click_at(&mut h, at);
        let now = centre(&h);
        assert!(now.dist(to) < 50.0, "rev {rev}: the view's middle is {now:?}, not {to:?}");
        assert!(matches!(h.state().sheet, Sheet::Plan(_)), "rev {rev}: still the plan");
    }
}

#[test]
fn a_drag_on_the_minimap_pans_the_view_as_it_goes_in_both_profiles() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        press(&mut h, "Minimap");
        let start = centre(&h);
        let by = P2(2000.0, -1000.0);
        let (a, b) = (on_map(&h, start), on_map(&h, start + by));
        h.hover_at(a);
        h.run();
        h.event(button(a, true));
        h.run();
        for t in [0.25, 0.5] {
            h.hover_at(a + (b - a) * t);
            h.run();
        }
        let half = centre(&h);
        assert!(half.dist(start + by * 0.5) < 50.0, "rev {rev}: halfway, before the button comes up, the view is at {half:?}");
        for t in [0.75, 1.0] {
            h.hover_at(a + (b - a) * t);
            h.run();
        }
        h.event(button(b, false));
        h.run();
        let end = centre(&h);
        assert!(end.dist(start + by) < 50.0, "rev {rev}: the view went to {end:?}, not {:?}", start + by);
        // Zoomed in, a drag begun beside the outline moves the view to where it began, then with the hand.
        let Sheet::Plan(key) = h.state().sheet else { panic!("rev {rev}: not a plan") };
        h.state_mut().canvas.zoom(key, 8.0);
        h.run();
        let (from, to) = (P2(1000.0, 5000.0), P2(2000.0, 4000.0));
        let (a, b) = (on_map(&h, from), on_map(&h, to));
        drag_screen(&mut h, a, b, PointerButton::Primary);
        let end = centre(&h);
        assert!(end.dist(to) < 50.0, "rev {rev}: from beside the outline, the view went to {end:?}, not {to:?}");
    }
}

#[test]
fn the_minimap_folds_away_maps_a_section_and_shows_nothing_for_a_schedule_or_the_3d_view_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        press(&mut h, "Minimap");
        assert!(h.query_by_label(PICTURE).is_some(), "rev {rev}: unfolded");
        press(&mut h, "Minimap");
        assert!(h.query_by_label(PICTURE).is_none(), "rev {rev}: folded away");
        press(&mut h, "Minimap");
        // A section's minimap, and a click on it, along its line and up.
        let section = ids(&h, "section")[0];
        h.state_mut().open_sheet(Sheet::Section(section));
        h.run();
        assert!(h.query_by_label(PICTURE).is_some(), "rev {rev}: a section has one");
        let to = P2(2000.0, 1000.0);
        let at = on_map(&h, to);
        click_at(&mut h, at);
        let now = centre(&h);
        assert!(now.dist(to) < 50.0, "rev {rev}: the section's middle is {now:?}, not {to:?}");
        for sheet in [Sheet::Schedule("door"), Sheet::View3D] {
            h.state_mut().open_sheet(sheet);
            h.run();
            assert!(h.query_by_label("Minimap").is_none() && h.query_by_label(PICTURE).is_none(), "rev {rev}: none for {sheet:?}");
        }
    }
}
