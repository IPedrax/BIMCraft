//! A family's plan, elevation and {3D} are drawn below the view tabs that float over the view's top, and fitted to what
//! they draw, lettering included: an elevation's plane names along its top, a plan's elevation marks' names at its sides, and
//! {3D}'s box all show, in the BIMCraft profile's narrower view as in the Rev profile's.
use super::family_views::{editor, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};

const BOX: &str = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "to": 2000}]}"#;

/// The foot of the lowest view tab.
fn tabs_foot(h: &Harness<BimApp>) -> f32 {
    h.state().tab_rects.iter().map(|(_, r)| r.bottom()).fold(f32::NEG_INFINITY, f32::max)
}

#[test]
fn family_views_are_drawn_below_the_tabs_and_show_all_they_draw() {
    for own in [true, false] {
        let mut h = if own { bimcraft(BOX) } else { app(BOX) };
        h.set_size(egui::vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Box");
        h.run();
        for which in [Some(Which::Plan), Some(Which::Elevation(Side::Front)), Some(Which::Elevation(Side::Left)), None] {
            view(&mut h, which);
            for _ in 0..3 {
                h.run();
            }
            let foot = tabs_foot(&h);
            assert!(foot.is_finite(), "tabs drawn");
            let drawn = editor(&h).views.drawn.expect("a view drawn");
            assert!(drawn.top() >= foot - 0.5, "{own} {which:?}: drawn from {} under the tabs to {foot}", drawn.top());
            if let Some(which) = which {
                let (marks, rest) = editor(&h).views.ink_on_screen();
                let inside = drawn.expand(0.5);
                // No elevation mark, its name with it, cut at the view's edge: wholly in, or (in a plan) wholly out.
                for m in &marks {
                    assert!(
                        m.iter().all(|p| inside.contains(*p)) || !m.iter().any(|p| inside.contains(*p)),
                        "{own} {which:?}: a mark cut at {drawn:?}"
                    );
                }
                // An elevation draws all it draws inside, plane names along its top under the tabs no more.
                if let Which::Elevation(_) = which {
                    assert!(!rest.is_empty());
                    let out: Vec<_> = rest.iter().filter(|p| !inside.contains(**p)).collect();
                    assert!(out.is_empty(), "{own} {which:?}: {} points outside {drawn:?}, as {:?}", out.len(), out.first());
                }
            }
        }
    }
}
