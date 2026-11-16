//! Snapping and alignment guides: what a pick lands on, what a guide says, the references a hand marks, and the
//! two screenshot galleries behind BIMCRAFT_GUIDES_DUMP.

use super::*;

#[test]
fn a_wall_is_drawn_to_a_columns_edge_and_not_only_to_its_centre() {
    use bimcraft_core::snap::Kind;
    // A 400 x 400 column at 4000, 3000: its left edge is at 3800 (issue #88 asked for a wall flush with one).
    let mut h = app(&format!("{BOX}\ncolumn {{\"at\": [4000, 3000], \"type\": \"Rectangular 400 x 400\"}}"));
    // The three hovers below rest long enough for the dwell to mark the column; the test is the wall's edge, not the pins.
    h.state_mut().snaps.mark_on_hover = false;
    tool(&mut h, "Wall");
    let at = |h: &mut Harness<BimApp>, p: P2| hover(h, p).map(|f| (f.kind, f.at));
    assert_eq!(at(&mut h, P2(3790.0, 3020.0)), Some((Kind::Midpoint, P2(3800.0, 3000.0))), "the middle of its left edge");
    assert_eq!(at(&mut h, P2(3790.0, 2790.0)), Some((Kind::Endpoint, P2(3800.0, 2800.0))), "its corner");
    assert_eq!(at(&mut h, P2(4030.0, 3020.0)), Some((Kind::Centre, P2(4000.0, 3000.0))), "and still its centre");
    // A wall from the west ends on the edge.
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3790.0, 3020.0));
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h).last(), Some(&(P2(1000.0, 3000.0), P2(3800.0, 3000.0))));
}

#[test]
fn walls_drawn_near_an_angle_guide_snap_onto_it_and_turn_exact_corners() {
    let mut h = app("");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(3470.0, 2020.0));
    key(&mut h, Key::Enter);
    let (a, b) = walls(&h)[0];
    assert!((degrees(a, b) - 30.0).abs() < 1e-3, "{a:?} {b:?}: 30.2° snaps onto 30°");
    // 20° is no multiple of 15°, so it's free; but 90° from it is exact all the same. Below the first, clear of the Options
    // Bar floating over the view's top.
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, -3000.0));
    click(&mut h, P2(3000.0, -1908.0));
    let (p, q) = match &h.state().tool {
        Tool::Wall(chain) => (chain[0], chain[1]),
        t => panic!("{t:?}"),
    };
    assert!((degrees(p, q) - 20.0).abs() < 0.2, "{p:?} {q:?}");
    let turn = (degrees(p, q) + 90.0).to_radians();
    click(&mut h, q + P2(turn.cos(), turn.sin()) * 2000.0 + P2(25.0, 10.0));
    key(&mut h, Key::Enter);
    let m = walls(&h);
    let (first, second) = (m[1], m[2]);
    assert_eq!(first.1, second.0);
    assert!((degrees(second.0, second.1) - degrees(first.0, first.1) - 90.0).abs() < 1e-2, "{first:?} {second:?}");
}

#[test]
fn the_cursor_snaps_to_ends_middles_and_perpendicular_feet_and_the_pick_says_which() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    tool(&mut h, "Wall");
    let kind = |h: &mut Harness<BimApp>, p: P2| hover(h, p).map(|f| (f.kind, f.at));
    assert_eq!(kind(&mut h, P2(8030.0, 20.0)), Some((Kind::Endpoint, P2(8000.0, 0.0))));
    assert_eq!(kind(&mut h, P2(4030.0, 20.0)), Some((Kind::Midpoint, P2(4000.0, 0.0))));
    assert_eq!(kind(&mut h, P2(2040.0, 30.0)), Some((Kind::Nearest, P2(2040.0, 0.0))));
    assert_eq!(kind(&mut h, P2(4020.0, 3000.0)), Some((Kind::Free, P2(4020.0, 3000.0))));
    // Not over a tool that picks points: nothing to say.
    key(&mut h, Key::Escape);
    // A second wall far off, so the view takes in where the first point goes.
    let mut h = app(r#"wall {"points": [[0,0],[6000,2000]]}
wall {"points": [[3000,5500],[4000,5500]]}"#);
    tool(&mut h, "Wall");
    click(&mut h, P2(1000.0, 4000.0));
    let f = hover(&mut h, P2(2130.0, 680.0)).unwrap();
    assert_eq!(f.kind, Kind::Perpendicular);
    assert!(f.at.dist(P2(2100.0, 700.0)) < 1e-3, "{:?}", f.at);
}

#[test]
fn a_snaps_tag_names_the_element_it_came_from_by_its_id() {
    use bimcraft_core::snap::Kind;
    let mut h = app(r#"wall {"points": [[0,0],[6000,0]]}
wall {"points": [[0,4000],[6000,4000]]}"#);
    let wall = ids(&h, "wall")[0];
    tool(&mut h, "Wall");
    let f = hover(&mut h, P2(6020.0, 10.0)).unwrap();
    assert_eq!((f.kind, f.source), (Kind::Endpoint, Some(wall)));
    let text = painted_text(&h);
    assert!(text.contains(&format!("Endpoint of Wall {wall}")), "{text:?}");
}

#[test]
fn manage_snaps_switches_each_snap_and_the_angle_steps() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    // The window takes the keys while it's open: a shortcut typed does nothing.
    assert!(h.state().show_snaps && h.query_by_label("Object Snaps").is_some());
    shortcut(&mut h, "WA");
    assert_eq!(h.state().tool, Tool::Modify);
    pick(&mut h, Role::CheckBox, "Midpoint");
    press(&mut h, "Close");
    assert!(!h.state().snaps.midpoint && h.state().snaps.endpoint);
    tool(&mut h, "Wall");
    assert_eq!(hover(&mut h, P2(4030.0, 20.0)).map(|f| f.kind), Some(Kind::Nearest), "no middle to stick to now");
    // The angle steps: off, a drawn wall goes where the cursor does.
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    pick(&mut h, Role::ComboBox, "Angle steps");
    press(&mut h, "Off");
    press(&mut h, "Close");
    assert_eq!(h.state().snaps.angle, 0.0);
    key(&mut h, Key::Escape);
    let mut h = app("");
    h.state_mut().snaps.angle = 0.0;
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(3470.0, 2020.0));
    key(&mut h, Key::Enter);
    assert_eq!(walls(&h)[0].1, P2(3470.0, 2020.0));
}

#[test]
fn walls_drawn_near_alignment_guide_snap_to_other_walls_reference_points() {
    let mut h = app("");
    // First wall from (0, 0) to (5000, 0)
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(5000.0, 0.0));
    key(&mut h, Key::Enter);

    // Second wall starting at (0, 3000)
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 3000.0));
    // Click near X = 5000 (e.g. 5005, 3000) within tolerance
    click(&mut h, P2(5005.0, 3000.0));
    key(&mut h, Key::Enter);

    let w = walls(&h);
    assert_eq!(w.len(), 2);
    assert!(w[1].0.dist(P2(0.0, 3000.0)) < 1e-2);
    // The second wall's endpoint snapped to exactly X = 5000.0 aligned with the first wall's endpoint
    assert_eq!(w[1].1.0, 5000.0);
    assert!((w[1].1.1 - 3000.0).abs() < 1e-2);
}

/// A guide says what it is, not which wall it came from, and a crossing names both of them, the way
/// a constraint badge stacks what is locked.
#[test]
fn a_guide_names_the_inference_and_a_crossing_names_both() {
    use bimcraft_core::alignment::{AlignmentAxis, AlignmentResult, GuideLine, Inference, ReferenceType};
    let guide = |axis| AlignmentResult {
        snapped_point: P2(1000.0, 2000.0),
        guide_line: GuideLine { from: P2(0.0, 1000.0), to: P2(1000.0, 2000.0) },
        reference_point: P2(0.0, 1000.0),
        axis,
        kind: ReferenceType::Endpoint,
        wall_id: Some(100231),
    };
    assert_eq!(crate::canvas::alignment_tag(&[guide(AlignmentAxis::Y)]), "Horizontal");
    assert_eq!(crate::canvas::alignment_tag(&[guide(AlignmentAxis::X)]), "Vertical");
    let parallel = AlignmentAxis::Inferred { along: P2(0.5, 0.866), of: Inference::Parallel };
    assert_eq!(crate::canvas::alignment_tag(&[guide(parallel)]), "Parallel");
    assert_eq!(crate::canvas::alignment_tag(&[guide(AlignmentAxis::Y), guide(parallel)]), "Intersection: Horizontal, Parallel");
}

/// A wall drawn beside a slanted one comes out parallel to it: the guide runs through the point the
/// wall starts from, so the second click lands on that line however far off it the pointer is.
#[test]
fn a_wall_drawn_beside_a_slanted_one_comes_out_parallel_to_it() {
    let rise = 4000.0 * 20.0_f64.to_radians().tan();
    let mut h = app(&format!("wall {{\"points\": [[0, 0], [4000, {rise}]]}}"));
    // A wall across the plan is a model-wide reference: the borrowing is what this test shows, and the context has its own.
    h.state_mut().snaps.guides_from_whole_model = true;
    tool(&mut h, "Wall");
    // South of the wall beside it, well inside the view it is fitted to, and clear of the Options Bar at the view's top.
    let start = P2(800.0, -200.0);
    click(&mut h, start);
    let level = h.state().session.model.levels()[0].0;
    let scale = h.state().canvas.current_view(level).map_or(0.2, |v| v.scale);
    let turn = 20.0_f64.to_radians();
    let along = P2(turn.cos(), turn.sin());
    // 1500 mm out along the wall's direction, four pixels off the parallel through the start.
    let end = start + along * 1500.0 + P2(-along.1, along.0) * (4.0 / scale);
    let f = hover(&mut h, end).expect("the cursor is over the plan");
    assert!((f.at - start).cross(along).abs() < 0.01, "{:?}: on the borrowed direction, not under the pointer", f.at);
    click(&mut h, end);
    key(&mut h, Key::Enter);

    let made = walls(&h).last().copied().expect("the wall");
    assert!(made.0.dist(start) < 1.0, "{made:?}");
    assert!((made.1 - made.0).cross(along).abs() < 0.01, "{made:?}: parallel to the wall it was drawn beside");
}

#[test]
fn manage_snaps_toggles_alignment_guides() {
    let mut h = app("");
    assert!(h.state().snaps.alignment_guides);
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    assert!(h.query_by_label("Alignment Guides").is_some());
    pick(&mut h, Role::CheckBox, "Alignment Guides");
    press(&mut h, "Close");
    assert!(!h.state().snaps.alignment_guides);
}

/// The refined guide is a hairline: a quiet ray, the stronger relation from its source to the point it locks, and a dot
/// and ring on the source. The classic one keeps the dashed magenta and its blue square.
#[test]
fn a_guide_draws_a_ray_a_relation_and_its_source() {
    use bimcraft_core::alignment::{AlignmentAxis, AlignmentResult, GuideLine, ReferenceType};
    use egui::Shape;
    let guide = AlignmentResult {
        snapped_point: P2(1000.0, 2000.0),
        guide_line: GuideLine { from: P2(0.0, 2000.0), to: P2(1000.0, 2000.0) },
        reference_point: P2(0.0, 2000.0),
        axis: AlignmentAxis::Y,
        kind: ReferenceType::Endpoint,
        wall_id: Some(100231),
    };
    let to = |p: P2| pos2(p.0 as f32, 3000.0 - p.1 as f32);
    let along = |d: P2| vec2(d.0 as f32, -d.1 as f32);
    let shapes = crate::canvas::alignment_shapes(&[guide], to, along, crate::canvas::GuideLook::plain(1.0, 100.0, true));
    let segments: Vec<[Pos2; 2]> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, .. } => Some(*points),
            _ => None,
        })
        .collect();
    // The horizontal guide is put on the physical pixel grid: y = 1000.5, a pixel's centre, so it is one crisp pixel.
    let (snapped, source) = (pos2(1000.0, 1000.5), pos2(0.0, 1000.5));
    assert_eq!(segments.len(), 4, "the relation and its three fading tail pieces: {shapes:?}");
    // The relation runs from its source to the point it locks.
    assert!(segments.iter().any(|[a, b]| a.distance(source) < 0.01 && b.distance(snapped) < 0.01), "{segments:?}");
    // The tail runs on past the point, 100 px in all, and no ray crosses the view.
    let tail: f32 = segments.iter().map(|[a, b]| a.distance(*b)).filter(|len| *len != 0.0).sum();
    assert!((tail - (1000.0 + 100.0)).abs() < 0.1, "the relation and a 100 px tail: {segments:?}");
    assert!(segments.iter().all(|[a, b]| a.distance(*b) <= 1000.0 + 0.01), "{segments:?}");
    let circles: Vec<_> = shapes.iter().filter_map(|s| if let Shape::Circle(c) = s { Some(c) } else { None }).collect();
    assert_eq!(circles.len(), 2, "{shapes:?}");
    assert!(circles.iter().any(|c| (c.radius - 2.5).abs() < 0.01 && c.center.distance(source) < 0.01), "the dot");
    assert!(circles.iter().any(|c| (c.radius - 4.5).abs() < 0.01 && c.center.distance(source) < 0.01), "and its ring");

    let classic = crate::canvas::alignment_shapes(&[guide], to, along, crate::canvas::GuideLook::plain(1.0, 1000.0, false));
    assert!(classic.iter().all(|s| !matches!(s, Shape::Circle(_))), "no dot in the classic guide");
    assert!(
        classic
            .iter()
            .any(|s| matches!(s, Shape::Rect(r) if (r.rect.width() - 7.0).abs() < 0.01 && r.rect.center().distance(source) < 0.01)),
        "{classic:?}"
    );
    // The two looks' inks stay apart: refined has no classic magenta and no classic blue square; classic has both.
    let classic_ink = |shapes: &[Shape]| {
        shapes.iter().any(|s| match s {
            Shape::LineSegment { stroke, .. } => stroke.color == crate::canvas::CLASSIC_GUIDE,
            Shape::Rect(r) => r.stroke.color == crate::canvas::CLASSIC_ANCHOR,
            _ => false,
        })
    };
    assert!(!classic_ink(&shapes), "the refined look has no classic ink: {shapes:?}");
    assert!(classic_ink(&classic), "the classic look draws its magenta and its square");
}

/// A badge sits right and below its glyph when nothing runs through it, turns to the other side rather than cross
/// the view's edge, and keeps off a guide line through its glyph.
#[test]
fn a_badge_stays_inside_the_view_and_off_its_guides() {
    let clip = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let size = vec2(100.0, 20.0);
    let at = pos2(400.0, 300.0);
    assert_eq!(crate::canvas::badge_rect(at, size, clip, &[]).min, pos2(412.0, 312.0));
    let right = crate::canvas::badge_rect(pos2(790.0, 300.0), size, clip, &[]);
    assert!(right.right() <= clip.right() - 6.0 + 0.01, "{right:?}");
    assert!(right.left() < 790.0 - 12.0, "flipped to the left of its glyph: {right:?}");
    let bottom = crate::canvas::badge_rect(pos2(400.0, 590.0), size, clip, &[]);
    assert!(bottom.bottom() <= clip.bottom() - 6.0 + 0.01, "{bottom:?}");
    assert!(bottom.top() < 590.0 - 12.0, "flipped above its glyph: {bottom:?}");
    let corner = crate::canvas::badge_rect(pos2(4.0, 4.0), size, clip, &[]);
    assert!(corner.min.x >= 6.0 - 0.01 && corner.min.y >= 6.0 - 0.01, "{corner:?}");
    assert!(clip.contains_rect(corner), "{corner:?}");

    // A diagonal guide runs through the glyph's right-below corner, where the badge goes by default: it takes the
    // corner across the line instead, so the ray is not cut by the pill.
    let diagonal = std::f32::consts::FRAC_PI_4;
    let rect = crate::canvas::badge_rect(at, size, clip, &[diagonal]);
    let c = rect.center() - at;
    let d = (c.y.atan2(c.x) - diagonal).rem_euclid(std::f32::consts::PI);
    assert!(d.min(std::f32::consts::PI - d) > 0.6, "the badge lies on the guide: {rect:?}");
    assert!(clip.contains_rect(rect), "{rect:?}");
    // Two guides crossing at a right angle leave every corner as far, so the default corner stays.
    let square = crate::canvas::badge_rect(at, size, clip, &[0.0, std::f32::consts::FRAC_PI_2]);
    assert_eq!(square.min, pos2(412.0, 312.0), "{square:?}");
}

/// A point held by two guides is the bullseye even on an angle step, whose own marker is a dot; one guide's glyph is
/// the dot and tick, and a point nothing holds keeps the crosshair, or its angle dot.
#[test]
fn a_point_held_by_guides_says_so_whatever_else_it_snapped_to() {
    use crate::canvas::{Lock, LockGlyph, lock_glyph};
    use bimcraft_core::snap::Kind;
    assert_eq!(lock_glyph(Kind::Angle, Lock::Cross, true), Some(LockGlyph::Bullseye), "the lock wins over the step's dot");
    assert_eq!(lock_glyph(Kind::Free, Lock::Cross, true), Some(LockGlyph::Bullseye));
    assert_eq!(lock_glyph(Kind::Angle, Lock::One(vec2(1.0, 0.0)), true), Some(LockGlyph::Slide(vec2(1.0, 0.0))));
    assert_eq!(lock_glyph(Kind::Free, Lock::Free, true), Some(LockGlyph::Crosshair));
    assert_eq!(lock_glyph(Kind::Angle, Lock::Free, true), None, "an angle step nothing holds keeps its dot");
    assert_eq!(lock_glyph(Kind::Endpoint, Lock::Cross, true), None, "real geometry keeps its own marker");
    assert_eq!(lock_glyph(Kind::Free, Lock::Cross, false), None, "the classic look has its own markers");
}

/// The live reading sits clear of its own segment and its badge: a wide horizontal pill beside a steep one is pushed
/// out by its box's projection on the normal, where a fixed offset let it cross back over the line.
#[test]
fn a_reading_pill_stays_off_its_segment_and_its_badge() {
    use crate::canvas::reading_spot;
    // 1969 mm at 74.38°: near vertical, so the pill lies almost across the line. Its own width decides the offset.
    let (from, to) = (pos2(400.0, 100.0), pos2(410.0, 400.0));
    let size = vec2(110.0, 22.0);
    let at = reading_spot(from, to, None, size, None);
    let rect = egui::Rect::from_center_size(at, size);
    let d = to - from;
    let n = vec2(d.y, -d.x) / d.length();
    let clearance = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()]
        .iter()
        .map(|q| (*q - from).dot(n).abs())
        .fold(f32::INFINITY, f32::min);
    assert!(clearance >= 5.0, "the pill crosses its line: {clearance} px of clearance, {rect:?}");

    // A badge over where the reading would go steps it farther out, so the two pills never sit on each other.
    let badge = egui::Rect::from_center_size(at, vec2(90.0, 20.0));
    let moved = reading_spot(from, to, None, size, Some(badge));
    assert!(!egui::Rect::from_center_size(moved, size).intersects(badge), "the pills collide at {moved:?}");

    // The pill keeps to the outside of the turn: a path coming from below turns right, so the reading goes below.
    let turned = reading_spot(pos2(0.0, 0.0), pos2(100.0, 0.0), Some(pos2(0.0, 50.0)), size, None);
    assert!(turned.y > 0.0, "the reading took the inside of the corner: {turned:?}");
}

/// The dwell ring waits out a 150 ms dead zone, fills over 300 ms, and is full at 0.45 s: a quick sweep marks nothing.
#[test]
fn the_dwell_ring_waits_150_ms_and_marks_at_450() {
    use crate::guide_refs::{Dwell, PinKey};
    let d = Dwell { key: PinKey::Edge { id: None, face: bimcraft_core::model::Face::Centre }, from: (0.0, 0.0), start: 0.0, marked: false };
    assert!(!d.filling(0.10), "nothing is drawn in the dead zone");
    assert_eq!(d.progress(0.10), 0.0);
    assert!(d.filling(0.20));
    assert!((d.progress(0.30) - 0.5).abs() < 0.01, "halfway through the fill: {}", d.progress(0.30));
    assert_eq!(d.progress(0.45), 1.0, "full at 0.45 s");
}

/// `Q` marks the wall face under the pointer, not the centre line its snap lands on, and does it before Point 1.
#[test]
fn q_marks_a_wall_face_before_the_first_point() {
    use crate::guide_refs::PinTarget;
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let u = P2(s, s);
    let n = P2(-s, s);
    let face = P2(20_000.0, 30_000.0);
    let line = (face - n * 100.0 - u * 4000.0, face - n * 100.0 + u * 4000.0);
    let mut h = app(&format!("wall {{\"points\": [[{}, {}], [{}, {}]]}}", line.0.0, line.0.1, line.1.0, line.1.1));
    h.state_mut().snaps.mark_on_hover = false;
    tool(&mut h, "Wall");
    let at = h.state().screen(face);
    h.hover_at(at);
    h.run();
    key(&mut h, Key::Q);
    let view = match h.state().sheet {
        crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) => id,
        _ => panic!("a plan"),
    };
    let pins = h.state().canvas.pins.borrow();
    let pin = pins.at(view).first().expect("Q marked the face before Point 1");
    let PinTarget::Edge { a, b, face: line, .. } = pin.target else { panic!("an edge, not a point: {:?}", pin.target) };
    assert_eq!(line, bimcraft_core::model::Face::Left, "the exterior face is its left one");
    assert!(bimcraft_core::geom::dist_to_segment(face, a, b) < 0.01, "the face itself, not the centre line");
}

/// `Esc` ends the command, so the references it marked go with it.
#[test]
fn escape_clears_the_marked_references() {
    let mut h = app("wall {\"points\": [[0, 0], [0, 5000]]}");
    let view = h.state().plan_key().expect("a plan");
    h.state_mut().snaps.mark_on_hover = false;
    tool(&mut h, "Wall");
    let at = h.state().screen(P2(6.0, 2500.0));
    h.hover_at(at);
    h.run();
    key(&mut h, Key::Q);
    assert_eq!(h.state().canvas.pins.borrow().at(view).len(), 1, "Q marked the face");
    key(&mut h, Key::Escape);
    assert!(h.state().canvas.pins.borrow().at(view).is_empty(), "Esc takes the mark with the command");
}

/// The 45°-rotated building's shaft: start square off the upper-right wall's face, mark the inner wall's face, and the
/// two cross where the shaft's corner goes. No outer peak or far corner takes part.
#[test]
fn the_rotated_shaft_marks_the_inner_face_and_snaps_to_their_crossing() {
    use crate::guide_refs::PinTarget;
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let u = P2(s, s);
    let n = P2(-s, s);
    let start_face = P2(20_000.0, 30_000.0);
    let q = start_face + n * 4000.0;
    let a = (start_face - n * 100.0 - u * 2000.0, start_face - n * 100.0 + u * 10_000.0);
    let b = (q - n * 100.0 - u * 3000.0, q - n * 100.0 + u * 3000.0);
    let mut h = app(&format!(
        "wall {{\"points\": [[{}, {}], [{}, {}]]}}\nwall {{\"points\": [[{}, {}], [{}, {}]]}}",
        a.0.0, a.0.1, a.1.0, a.1.1, b.0.0, b.0.1, b.1.0, b.1.1
    ));
    h.state_mut().snaps.mark_on_hover = false;
    tool(&mut h, "Wall");
    click(&mut h, start_face);
    let view = match h.state().sheet {
        crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) => id,
        _ => panic!("a plan"),
    };
    // The wall the chain started on: its face, not its centre line, and only it.
    let start_edge = h.state().canvas.start_edge.expect("the starting wall's edge");
    assert!(bimcraft_core::geom::dist_to_segment(start_face, start_edge.a, start_edge.b) < 0.01, "{start_edge:?}");

    // Mark the inner re-entrant wall's face under the pointer.
    let at = h.state().screen(q);
    h.hover_at(at);
    h.run();
    key(&mut h, Key::Q);
    {
        let pins = h.state().canvas.pins.borrow();
        let pin = pins.at(view).first().expect("the inner face is marked");
        let PinTarget::Edge { a, b, face, .. } = pin.target else { panic!("an edge: {:?}", pin.target) };
        assert_eq!(face, bimcraft_core::model::Face::Left, "the exterior face is its left one");
        assert!(bimcraft_core::geom::dist_to_segment(q, a, b) < 0.01, "the face itself, not the centre line");
    }

    // Seven and a half pixels off the face, on the shaft's line: a free point near both guides, not a `Nearest` snap.
    let level = h.state().session.model.levels()[0].0;
    let scale = h.state().canvas.current_view(level).map_or(0.2, |v| v.scale);
    let found = hover(&mut h, q + n * (7.5 / scale)).expect("the cursor is over the plan");
    assert!(found.at.dist(q) < 0.1, "the crossing of the perpendicular and the face's extension: {:?}", found.at);
}

/// The 45° building's shaft, for a person to look at: `BIMCRAFT_GUIDES_DUMP=<folder>` writes the ring filling on the
/// inner wall's face, the face marked, and the crossing of the shaft's perpendicular with its extension.
#[test]
fn shaft_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_GUIDES_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let u = P2(s, s);
    let n = P2(-s, s);
    let start_face = P2(20_000.0, 30_000.0);
    let q = start_face + n * 4000.0;
    let a = (start_face - n * 100.0 - u * 2000.0, start_face - n * 100.0 + u * 10_000.0);
    let b = (q - n * 100.0 - u * 3000.0, q - n * 100.0 + u * 3000.0);
    let mut h = app_at(
        &format!(
            "wall {{\"points\": [[{}, {}], [{}, {}]]}}\nwall {{\"points\": [[{}, {}], [{}, {}]]}}",
            a.0.0, a.0.1, a.1.0, a.1.1, b.0.0, b.0.1, b.1.0, b.1.1
        ),
        0.1,
    );
    tool(&mut h, "Wall");
    // The click alone must not mark the wall it starts on: the dwell is for a deliberate rest, not a click.
    h.state_mut().snaps.mark_on_hover = false;
    click(&mut h, start_face);
    h.state_mut().snaps.mark_on_hover = true;
    let view = match h.state().sheet {
        crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) => id,
        _ => panic!("a plan"),
    };
    // The ring filling on the inner face: stage it half full, so the frame shows the ring rather than waiting for the
    // harness's clock to stop in the dead zone.
    let at = h.state().screen(q);
    h.hover_at(at);
    h.run_steps(3);
    let level = h.state().session.model.levels()[0].0;
    let v = h.state().canvas.current_view(level).expect("a view");
    let found = h.state().canvas.found.get().expect("the hover's snap");
    let target = crate::guide_refs::target_at(&h.state().session.model, level, view, &v, v.px(q), &found, &h.state().canvas.under_cursor)
        .expect("the face under the pointer");
    let now = h.state().canvas.now.get();
    h.state_mut().canvas.dwell.set(Some(crate::guide_refs::Dwell {
        key: target.key(),
        from: (at.x, at.y),
        start: now - 0.30,
        marked: false,
    }));
    h.run_steps(1);
    h.render().unwrap().save(std::path::Path::new(&folder).join("shaft-dwell.png")).unwrap();
    // The face marked: the dwell may have closed already; Q does it at once otherwise.
    if h.state().canvas.pins.borrow().at(view).is_empty() {
        key(&mut h, Key::Q);
    }
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("shaft-marked.png")).unwrap();
    // Just off the face and on the shaft's line: the perpendicular and the face's extension cross. The dwell is off,
    // so moving to the crossing does not toggle the face it has just marked.
    h.state_mut().snaps.mark_on_hover = false;
    let level = h.state().session.model.levels()[0].0;
    let scale = h.state().canvas.current_view(level).map_or(0.2, |v| v.scale);
    hover(&mut h, q + n * (7.5 / scale));
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("shaft-intersection.png")).unwrap();
}

/// A guide is entered at 8 px and left at 12: the ray does not blink on and off as the hand shakes at the tolerance's edge.
#[test]
fn a_guide_is_entered_at_8_px_and_left_at_12() {
    use bimcraft_core::alignment::AlignmentAxis;
    let h = app("wall {\"points\": [[0, 0], [0, 5000]]}");
    let m = &h.state().session.model;
    let level = m.levels()[0].0;
    let v = h.state().canvas.current_view(level).expect("a view");
    // The chain's last point is the one being drawn from, so it is not its own reference: the point before it is.
    let chain = [P2(0.0, 0.0), P2(0.0, 2500.0)];
    let at = |px: f64| P2(px / v.scale, 2500.0);
    // Six pixels beside the chain's point: within the 8 the guide enters at.
    let found = h.state().alignments(m, level, &v, at(6.0), &chain);
    assert!(found.iter().any(|a| matches!(a.axis, AlignmentAxis::X)), "the chain's x holds at 6 px: {found:?}");
    // Ten pixels: past the entry, but the guide it held last frame is kept to 12.
    let held = h.state().alignments(m, level, &v, at(10.0), &chain);
    assert!(held.iter().any(|a| matches!(a.axis, AlignmentAxis::X)), "held on to 12 px: {held:?}");
    // Thirteen: gone.
    assert!(h.state().alignments(m, level, &v, at(13.0), &chain).is_empty(), "left past 12 px");
}

/// A structural grid near the cursor lends its own line, unless Manage > Snaps says otherwise; a grid out of reach lends
/// nothing, so a plan is not crossed by every grid in it.
#[test]
fn a_grid_near_the_cursor_lends_a_guide_until_its_switch_is_off() {
    use bimcraft_core::alignment::AlignmentAxis;
    let mut h = app("grid {\"start\": [0, 0], \"end\": [0, 5000]}");
    let (level, scale) = {
        let m = &h.state().session.model;
        let level = m.levels()[0].0;
        (level, h.state().canvas.current_view(level).expect("a view").scale)
    };
    let chain = [P2(4000.0, 2000.0)];
    let six_px = P2(6.0 / scale, 2000.0);
    {
        let m = &h.state().session.model;
        let v = h.state().canvas.current_view(level).expect("a view");
        let found = h.state().alignments(m, level, &v, six_px, &chain);
        // A grid is a line, so it lends a collinear guide along itself, not a bare X.
        let on = found.iter().any(|a| a.snapped_point.0.abs() < 1e-6 && matches!(a.axis, AlignmentAxis::Collinear { .. }));
        assert!(on, "the grid's own line at 6 px: {found:?}");
        // Far from it, the grid casts nothing.
        let far = P2(4000.0, 2000.0);
        assert!(h.state().alignments(m, level, &v, far, &chain).is_empty(), "nothing lines up 4 m away");
    }
    // The same once a view has made the lookup: the guide comes off its grid list, not a walk of every element.
    h.state().session.model.lookup();
    {
        let m = &h.state().session.model;
        assert!(m.derived.made(&m.elements).is_some(), "the lookup is made, so the grid list is the one read");
        let v = h.state().canvas.current_view(level).expect("a view");
        let found = h.state().alignments(m, level, &v, six_px, &chain);
        assert!(
            found.iter().any(|a| a.snapped_point.0.abs() < 1e-6 && matches!(a.axis, AlignmentAxis::Collinear { .. })),
            "the indexed grid list at 6 px: {found:?}"
        );
    }
    h.state_mut().snaps.guides_from_grids = false;
    let m = &h.state().session.model;
    let v = h.state().canvas.current_view(level).expect("a view");
    assert!(h.state().alignments(m, level, &v, six_px, &chain).is_empty(), "the switch turns the grid off");
}

/// A 40 m grid's strong relation starts at the grid's nearer end, not at the cursor: the gallery records the length for a
/// person judging the taper, and this pins the rule on every run.
#[test]
fn a_long_grids_relation_starts_at_the_grid_and_not_the_cursor() {
    let mut h = app("grid {\"start\":[20000,-20000],\"end\":[20000,20000]}");
    let level = h.state().plan_key().expect("a plan");
    h.state_mut().canvas.set_view(level, 20000.0, 0.0, 0.05);
    h.run();
    tool(&mut h, "Wall");
    click(&mut h, P2(19000.0, 2000.0));
    let at = h.state().screen(P2(20000.7, 0.0));
    h.hover_at(at);
    h.run();
    let v = h.state().canvas.current_view(level).expect("a view");
    let res = h.state().alignments(&h.state().session.model, level, &v, P2(20000.7, 0.0), &[P2(19000.0, 2000.0)]);
    let length = res.first().map_or(0.0, |r| r.reference_point.dist(r.snapped_point));
    assert!(length > 10_000.0, "the relation starts at the grid's end, not the cursor: {length} mm");
}

/// A wall is three references, and the snap names the one its pointer is on — not a bare "Nearest of Wall" that could be
/// any of them.
#[test]
fn the_snap_names_the_wall_line_its_pointer_is_on() {
    use bimcraft_core::model::Face;
    let h = app(r#"wall {"points": [[0,0],[6000,0]]}"#);
    let m = &h.state().session.model;
    let id = ids(&h, "wall")[0];
    let (ext, int) = bimcraft_core::walls::faces(m, m.wall(id).expect("the wall"));
    let level = m.levels()[0].0;
    let v = h.state().canvas.current_view(level).expect("a view");
    let near = |y: f64| h.state().snap(m, level, &v, v.px(P2(2000.0, y)), &[], false);
    let f = near(ext - 10.0);
    assert_eq!(f.face, Some(Face::Left), "{f:?}");
    assert_eq!(crate::canvas::snap_name(m, &f), format!("Exterior Face of Wall {id}"));
    let f = near((ext + int) / 2.0 + 10.0);
    assert_eq!(crate::canvas::snap_name(m, &f), format!("Centreline of Wall {id}"), "{f:?}");
    let f = near(int + 10.0);
    assert_eq!(crate::canvas::snap_name(m, &f), format!("Interior Face of Wall {id}"), "{f:?}");
}

/// Tab over a wall steps its pick through the exterior face, the centre line and the interior face, from the line the pick
/// has now; a number already typed keeps Tab for the field it belongs to.
#[test]
fn tab_cycles_a_walls_three_lines_until_a_number_is_typed() {
    use bimcraft_core::model::Face;
    let mut h = app(r#"wall {"points": [[0,0],[6000,0]]}"#);
    let (id, ext) = {
        let m = &h.state().session.model;
        let id = ids(&h, "wall")[0];
        let (ext, _) = bimcraft_core::walls::faces(m, m.wall(id).expect("the wall"));
        (id, ext)
    };
    tool(&mut h, "Wall");
    // A chain started away from the wall, and inside the plan's fit, so its length and angle can be typed.
    click(&mut h, P2(0.0, 1000.0));
    let over = P2(2000.0, ext - 10.0);
    assert!(hover(&mut h, over).is_some(), "the pointer is over the plan");
    assert_eq!(h.state().canvas.found.get().and_then(|f| f.face), Some(Face::Left), "the nearest line first");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().canvas.wall_face, Some((id, Face::Centre)), "the next line along");
    hover(&mut h, over);
    assert_eq!(h.state().canvas.found.get().and_then(|f| f.face), Some(Face::Centre), "and the pick follows it");
    key(&mut h, Key::Tab);
    hover(&mut h, over);
    assert_eq!(h.state().canvas.found.get().and_then(|f| f.face), Some(Face::Right));
    // A number typed keeps Tab: the field swaps, and the wall's line stays where it was.
    h.event(Event::Text("3000".into()));
    h.run();
    assert_eq!(h.state().typed_length, "3000", "the number is being typed");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().canvas.wall_face, Some((id, Face::Right)), "the lines stay where they were");
    assert_eq!(
        (h.state().typed_length.as_str(), h.state().drawing.other.as_str()),
        ("", "3000"),
        "the number moved to the angle field, as Tab does"
    );
}

/// One edge pin per wall: marking another of its faces moves the pin rather than spending a second of the three, and the
/// face marked again unmarks it.
#[test]
fn one_pin_per_wall_moves_to_the_face_marked_last() {
    use bimcraft_core::model::Face;
    let mut h = app(r#"wall {"points": [[0,0],[6000,0]]}"#);
    let (ext, int) = {
        let m = &h.state().session.model;
        let id = ids(&h, "wall")[0];
        bimcraft_core::walls::faces(m, m.wall(id).expect("the wall"))
    };
    tool(&mut h, "Wall");
    // The rest marks nothing here: this test is what Q does, once, per wall.
    h.state_mut().snaps.mark_on_hover = false;
    let view = match h.state().sheet {
        crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) => id,
        _ => panic!("a plan"),
    };
    hover(&mut h, P2(2000.0, ext - 10.0));
    key(&mut h, Key::Q);
    assert_eq!(h.state().canvas.pins.borrow().at(view).len(), 1, "the exterior face marked");
    hover(&mut h, P2(2000.0, int + 10.0));
    key(&mut h, Key::Q);
    {
        let pins = h.state().canvas.pins.borrow();
        assert_eq!(pins.at(view).len(), 1, "one reference per wall, however many of its lines are marked");
        let crate::guide_refs::PinTarget::Edge { face, .. } = pins.at(view)[0].target else { panic!("an edge") };
        assert_eq!(face, Face::Right, "it moved to the face marked last");
    }
    assert!(h.state().status.text.contains("moved"), "{}", h.state().status.text);
    hover(&mut h, P2(2000.0, int + 10.0));
    key(&mut h, Key::Q);
    assert!(h.state().canvas.pins.borrow().at(view).is_empty(), "the same face again unmarks it");
}

/// Resting on one wall marks it once, even when the hand drifts across its lines: the element, not the line, is what stays
/// quiet after a mark.
#[test]
fn a_wall_swept_over_is_marked_once() {
    let mut h = app_at(r#"wall {"points": [[0,0],[6000,0]]}"#, 0.1);
    let (ext, int) = {
        let m = &h.state().session.model;
        let id = ids(&h, "wall")[0];
        bimcraft_core::walls::faces(m, m.wall(id).expect("the wall"))
    };
    tool(&mut h, "Wall");
    let view = match h.state().sheet {
        crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) => id,
        _ => panic!("a plan"),
    };
    h.hover_at(h.state().screen(P2(2000.0, ext - 10.0)));
    h.run_steps(7);
    assert_eq!(h.state().canvas.pins.borrow().at(view).len(), 1, "half a second closes the ring");
    h.hover_at(h.state().screen(P2(2000.0, int + 10.0)));
    h.run_steps(8);
    assert_eq!(h.state().canvas.pins.borrow().at(view).len(), 1, "one wall, one mark");
}

/// A pin is its line or its dot and ring, never the classic guide's filled square.
#[test]
fn a_pin_is_a_line_or_a_dot_and_never_a_square() {
    use crate::guide_refs::{Pin, PinTarget};
    use bimcraft_core::model::Face;
    use egui::Shape;
    let edge = PinTarget::Edge { a: P2(0.0, 0.0), b: P2(1000.0, 0.0), id: Some(100002), face: Face::Left, near: None };
    let point = PinTarget::Point { at: P2(0.0, 500.0), kind: bimcraft_core::snap::Kind::Endpoint, id: Some(100002) };
    let pins = vec![Pin { target: edge, lines: Vec::new() }, Pin { target: point, lines: Vec::new() }];
    let shapes = crate::canvas::pin_shapes(&pins, |p| pos2(p.0 as f32, p.1 as f32), 1.0);
    assert!(shapes.iter().any(|s| matches!(s, Shape::LineSegment { .. })), "the face is its line");
    assert_eq!(shapes.iter().filter(|s| matches!(s, Shape::Circle(_))).count(), 2, "a dot and its ring");
    assert!(!shapes.iter().any(|s| matches!(s, Shape::Rect(_))), "never a square: {shapes:?}");
}

/// A view keeps three references, a fourth letting the oldest go, and a point marked at a wall's end is a reference of
/// its own beside that wall's edge pin.
#[test]
fn a_view_keeps_three_references_and_a_point_is_one_of_its_own() {
    use crate::guide_refs::{Marked, PinTarget, Pins};
    use bimcraft_core::model::Face;
    let edge = |id: Id| PinTarget::Edge { a: P2(0.0, 0.0), b: P2(1000.0, 0.0), id: Some(id), face: Face::Left, near: None };
    let mut pins = Pins::default();
    for id in [1, 2, 3] {
        assert_eq!(pins.toggle(9, edge(id), Vec::new()), Marked::New);
    }
    assert_eq!(pins.toggle(9, edge(4), Vec::new()), Marked::New, "the fourth comes in");
    assert_eq!(pins.at(9).len(), 3, "and three is what a view keeps");
    assert!(pins.at(9).iter().all(|p| p.target.id() != Some(1)), "the oldest went");
    // A point marked at one wall's end sits beside that wall's edge pin rather than moving it.
    let point = PinTarget::Point { at: P2(0.0, 0.0), kind: bimcraft_core::snap::Kind::Endpoint, id: Some(2) };
    let mut pins = Pins::default();
    assert_eq!(pins.toggle(9, edge(2), Vec::new()), Marked::New);
    assert_eq!(pins.toggle(9, point, Vec::new()), Marked::New, "a point is a reference of its own");
    assert_eq!(pins.at(9).len(), 2);
    assert_eq!(pins.toggle(9, point, Vec::new()), Marked::Cleared, "and Q on it again clears it");
    assert_eq!(pins.toggle(9, edge(2), Vec::new()), Marked::Cleared, "the wall's own line goes too");
}

#[test]
fn manage_snaps_picks_the_guide_style() {
    use crate::style::GuideStyle;
    let mut h = app("");
    assert_eq!(h.state().guide_style, GuideStyle::Refined, "the refined look is the one to see");
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    assert!(h.query_by_label("Guide style").is_some());
    choose(&mut h, "Guide style", "Classic");
    assert_eq!(h.state().guide_style, GuideStyle::Classic);
    press(&mut h, "Close");
    assert_eq!(h.state().guide_style, GuideStyle::Classic, "kept when the window closes");
}

/// The look changes only what is drawn: a click takes the guide's point in either style.
#[test]
fn a_guide_locks_the_point_in_both_guide_styles() {
    use crate::style::GuideStyle;
    for style in GuideStyle::ALL {
        let mut h = app("wall {\"points\": [[0, 0], [5000, 0]]}");
        h.state_mut().guide_style = style;
        // The wall's end is a model-wide reference: this test is the look, and the whole-model fallback gives it one.
        h.state_mut().snaps.guides_from_whole_model = true;
        tool(&mut h, "Wall");
        click(&mut h, P2(1000.0, -1000.0));
        let level = h.state().session.model.levels()[0].0;
        let scale = h.state().canvas.current_view(level).map_or(0.2, |v| v.scale);
        let f = hover(&mut h, P2(5000.0 + 4.0 / scale, -1000.0)).expect("the cursor is over the plan");
        assert!((f.at.0 - 5000.0).abs() < 0.01, "{style:?}: on the guide, not under the pointer");
    }
}

/// A guide is found from where the pointer is, not from the 10 mm point the snap rounded it to: at 2 px a millimetre the
/// guide's tolerance is 4 mm while the rounding can be 7 mm, so a guide 3.6 mm off used to be missed and a point riding
/// one used to be quantised to the grid.
#[test]
fn a_guide_at_two_px_a_mm_is_taken_from_the_pointer_not_the_ten_mm_point() {
    use bimcraft_core::render::View;
    use bimcraft_core::snap::Kind;
    let h = app("grid {\"start\":[0,3],\"end\":[8000,3]}");
    let level = h.state().plan_key().expect("a plan");
    let v = View { angle: 0.0, scale: 2.0, cx: 4000.0, cy: 0.0, width: 800, height: 600 };
    let raw = P2(4123.7, 6.6);
    let m = &h.state().session.model;
    let f = h.state().snap(m, level, &v, v.px(raw), &[], false);
    assert_eq!(f.kind, Kind::Free, "3.6 mm off the grid, which is only a guide: {f:?}");
    let p = h.state().snap_point(m, level, &v, v.px(raw), &[], false);
    assert!(p.dist(P2(4123.7, 3.0)) < 0.01, "the guide takes the pointer's own place: {p:?}");
}

/// A crossing of two guides is exact, and may take a point off a wall's face; one guide alone leaves the face its own
/// 10 mm point, because the face under the pointer is the more exact reference then.
#[test]
fn a_guide_crossing_beats_a_wall_face_and_one_guide_alone_does_not() {
    use crate::guide_refs::PinTarget;
    use bimcraft_core::model::{Face, Reference};
    use bimcraft_core::render::View;
    use bimcraft_core::snap::Kind;
    let mut h = app("wall {\"points\":[[0,0],[8000,0]]}\ngrid {\"start\":[2234.7,0],\"end\":[2234.7,8000]}");
    let level = h.state().plan_key().expect("a plan");
    let (wall, ext, face) = {
        let m = &h.state().session.model;
        let wall = ids(&h, "wall")[0];
        let (ext, _) = bimcraft_core::walls::faces(m, m.wall(wall).expect("the wall"));
        (wall, ext, m.reference_line(&Reference::line(m.guid(wall), Face::Left)).expect("the face"))
    };
    let grid = ids(&h, "grid")[0];
    mark(&mut h, PinTarget::Edge { a: face.0, b: face.1, id: Some(wall), face: Face::Left, near: None });
    mark(&mut h, PinTarget::Edge { a: P2(2234.7, 0.0), b: P2(2234.7, 8000.0), id: Some(grid), face: Face::Centre, near: None });
    let v = View { angle: 0.0, scale: 0.1, cx: 4000.0, cy: 0.0, width: 1280, height: 800 };
    let m = &h.state().session.model;
    let at = P2(2234.7, ext);
    let f = h.state().snap(m, level, &v, v.px(at), &[], false);
    // The grid crosses the wall's face, which the plan's outline of the wall gives as a line (#386): a crossing of the two.
    assert_eq!(f.kind, Kind::Intersection, "the grid crosses the wall's face here: {f:?}");
    let p = h.state().snap_point(m, level, &v, v.px(at), &[], false);
    assert!(p.dist(at) < 0.01, "the crossing is exact, not the face's 10 mm point: {p:?}");
    // Out of reach of the grid's crossing with the face, which is a snap of its own (#386's outline).
    let one = P2(1734.7, ext);
    let p = h.state().snap_point(m, level, &v, v.px(one), &[], false);
    assert!((p.0 - 1730.0).abs() < 0.01 && (p.1 - ext).abs() < 0.01, "one guide does not take the face's point: {p:?}");
}

/// A passive 15° step does not take a pick off the wall face under the pointer; Shift's step, which was asked for, does.
#[test]
fn a_passive_angle_step_does_not_take_a_pick_off_a_wall_face() {
    use bimcraft_core::render::View;
    use bimcraft_core::snap::Kind;
    let h = app("wall {\"points\":[[0,0],[12000,0]]}");
    let level = h.state().plan_key().expect("a plan");
    let v = View { angle: 0.0, scale: 0.05, cx: 6000.0, cy: 0.0, width: 1280, height: 800 };
    let from = P2(0.0, -40.0);
    let at = P2(8000.0, -100.0);
    let m = &h.state().session.model;
    let f = h.state().snap(m, level, &v, v.px(at), &[from], false);
    assert_eq!(f.kind, Kind::Nearest, "the face under the pointer: {f:?}");
    let p = h.state().snap_point(m, level, &v, v.px(at), &[from], false);
    assert!((p.1 + 100.0).abs() < 0.01, "the point stays on the face, not 60 mm above it: {p:?}");
    let f = h.state().snap(m, level, &v, v.px(at), &[from], true);
    assert_eq!(f.kind, Kind::Angle, "Shift asks for the angle, and keeps it: {f:?}");
}

/// A background grid point is not a crossing of two model lines: it is a free place a guide can still take.
#[test]
fn a_background_grid_point_lets_a_guide_through() {
    use bimcraft_core::snap::Kind;
    let mut h = app("grid {\"start\":[-5000,-1030],\"end\":[5000,-1030]}");
    h.state_mut().grid_snapping = true;
    h.state_mut().run("graphics", json!({"view": "Level 1", "background_grid": {"main": {"enabled": true}}}));
    let level = h.state().plan_key().expect("a plan");
    let v = h.state().canvas.current_view(level).expect("a view");
    let m = &h.state().session.model;
    let at = P2(-1000.0, -1000.0);
    let f = h.state().snap(m, level, &v, v.px(at), &[], false);
    assert_eq!(f.kind, Kind::Free, "the background grid is a free point: {f:?}");
    let p = h.state().snap_point(m, level, &v, v.px(at), &[], false);
    assert!((p.1 + 1030.0).abs() < 0.01, "the structural grid's line takes it: {p:?}");
}

/// Undo and redo take back or move the elements a mark was made on: the mark goes with them rather than becoming a ghost.
#[test]
fn undo_clears_the_marked_references() {
    use crate::guide_refs::PinTarget;
    use bimcraft_core::model::{Face, Reference};
    let mut h = app("wall {\"points\":[[0,0],[8000,0]]}");
    tool(&mut h, "Wall");
    click(&mut h, P2(0.0, 3000.0));
    click(&mut h, P2(5000.0, 3000.0));
    key(&mut h, Key::Enter);
    let level = h.state().plan_key().expect("a plan");
    let wall = *ids(&h, "wall").last().expect("the wall just made");
    let face = {
        let m = &h.state().session.model;
        m.reference_line(&Reference::line(m.guid(wall), Face::Left)).expect("its face")
    };
    mark(&mut h, PinTarget::Edge { a: face.0, b: face.1, id: Some(wall), face: Face::Left, near: None });
    assert_eq!(h.state().canvas.pins.borrow().at(level).len(), 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    h.run();
    assert!(h.state().canvas.pins.borrow().at(level).is_empty(), "undo takes the mark with the wall");
    assert_eq!(walls(&h).len(), 1, "the wall the mark named is gone");
}

/// The Split tool's crosshair and its cut are the same landing: a guide crossing, not the face's rounded point.
#[test]
fn split_takes_the_point_its_preview_shows() {
    use crate::guide_refs::PinTarget;
    use bimcraft_core::model::{Face, Reference};
    let mut h = app("wall {\"points\":[[0,0],[8000,0]]}\ngrid {\"start\":[2234.7,0],\"end\":[2234.7,8000]}");
    let (wall, face) = {
        let m = &h.state().session.model;
        let wall = ids(&h, "wall")[0];
        (wall, m.reference_line(&Reference::line(m.guid(wall), Face::Centre)).expect("its centre line"))
    };
    let grid = ids(&h, "grid")[0];
    mark(&mut h, PinTarget::Edge { a: face.0, b: face.1, id: Some(wall), face: Face::Centre, near: None });
    mark(&mut h, PinTarget::Edge { a: P2(2234.7, 0.0), b: P2(2234.7, 8000.0), id: Some(grid), face: Face::Centre, near: None });
    h.state_mut().set_tool(Tool::Split);
    let at = P2(2234.7, 0.0);
    let shown = hover(&mut h, at).expect("the hover's snap").at;
    assert!(shown.dist(at) < 0.01, "the preview shows the crossing: {shown:?}");
    click(&mut h, at);
    let cut = walls(&h).iter().flat_map(|(a, b)| [a.0, b.0]).any(|x| (x - 2234.7).abs() < 0.01);
    assert!(cut, "the split is where the preview said: {:?}", walls(&h));
}

/// The Spot Coordinate tool's preview and its first click are the same landing too.
#[test]
fn spot_coordinate_takes_the_point_its_preview_shows() {
    use crate::guide_refs::PinTarget;
    use bimcraft_core::model::{Face, Reference};
    let mut h = app("wall {\"points\":[[0,0],[8000,0]]}\ngrid {\"start\":[2234.7,0],\"end\":[2234.7,8000]}");
    let (wall, face) = {
        let m = &h.state().session.model;
        let wall = ids(&h, "wall")[0];
        (wall, m.reference_line(&Reference::line(m.guid(wall), Face::Centre)).expect("its centre line"))
    };
    let grid = ids(&h, "grid")[0];
    mark(&mut h, PinTarget::Edge { a: face.0, b: face.1, id: Some(wall), face: Face::Centre, near: None });
    mark(&mut h, PinTarget::Edge { a: P2(2234.7, 0.0), b: P2(2234.7, 8000.0), id: Some(grid), face: Face::Centre, near: None });
    h.state_mut().set_tool(Tool::SpotCoordinate(None));
    let at = P2(2234.7, 0.0);
    let shown = hover(&mut h, at).expect("the hover's snap").at;
    assert!(shown.dist(at) < 0.01, "the preview shows the crossing: {shown:?}");
    click(&mut h, at);
    match h.state().tool {
        Tool::SpotCoordinate(Some(p)) => assert!(p.dist(shown) < 0.01, "the click takes the previewed point: {p:?}, {shown:?}"),
        ref other => panic!("the spot's first point: {other:?}"),
    }
}

/// `Tab` steps a wall's three lines for a tool that takes a wall face, not only for a chain: Move's base is one.
#[test]
fn tab_cycles_a_walls_lines_while_a_move_waits_for_its_base() {
    let mut h = app("wall {\"points\":[[0,0],[8000,0]]}");
    click(&mut h, P2(4000.0, 0.0));
    h.state_mut().set_tool(Tool::Move(None));
    hover(&mut h, P2(2000.0, -100.0));
    key(&mut h, Key::Tab);
    assert!(h.state().canvas.wall_face.is_some(), "Tab steps the wall's lines under the pointer");
}

/// `Q` reads the tool's own chain, not five tools' chains: a beam's start point is a mark like a wall's.
#[test]
fn q_marks_the_beam_s_own_start_point() {
    use bimcraft_core::snap::Kind;
    let mut h = app(BOX);
    // The click's own rest would fill the ring and mark the start; this test is `Q`'s mark.
    h.state_mut().snaps.mark_on_hover = false;
    h.state_mut().set_tool(Tool::Run("beam", None));
    click(&mut h, P2(4000.0, 3000.0));
    assert!(matches!(&h.state().tool, Tool::Run("beam", Some(p)) if p.dist(P2(4000.0, 3000.0)) < 0.01), "{:?}", h.state().tool);
    hover(&mut h, P2(4004.0, 3004.0));
    key(&mut h, Key::Q);
    let level = h.state().plan_key().expect("a plan");
    let pins = h.state().canvas.pins.borrow();
    let [pin] = pins.at(level) else { panic!("one mark: {:?}", pins.at(level)) };
    match pin.target {
        crate::guide_refs::PinTarget::Point { at, kind, .. } => {
            assert!(at.dist(P2(4000.0, 3000.0)) < 0.01 && kind == Kind::Endpoint, "{:?}", pin.target);
        }
        other => panic!("a point: {other:?}"),
    }
}

/// The audit gallery: twelve scenes a person can look at, each one of the cases the guide and snap audit named. Set
/// `BIMCRAFT_GUIDES_DUMP=<folder>` to have them written; nothing is committed, and without the variable the test does
/// nothing. Run it alone or with `--test-threads=1`: the screenshot tests share the frame's ink and are not parallel-safe.
#[test]
fn guides_audit_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_GUIDES_DUMP") else { return };
    std::fs::create_dir_all(&folder).expect("the dump folder");
    fn zoom(h: &mut Harness<BimApp>, cx: f64, cy: f64, scale: f64) {
        let level = h.state().plan_key().expect("a plan");
        h.state_mut().canvas.set_view(level, cx, cy, scale);
        h.run();
    }
    fn shot(h: &mut Harness<BimApp>, folder: &str, name: &str) {
        h.run();
        h.render().expect("a rendered frame").save(std::path::Path::new(folder).join(name)).expect("the screenshot");
    }
    fn hover_at(h: &mut Harness<BimApp>, p: P2) {
        let at = h.state().screen(p);
        h.hover_at(at);
        h.run();
    }

    // 1. A 25 px segment: the reading pill's minimum length, the corner arc and the badge in one small place.
    {
        let mut h = app("");
        zoom(&mut h, 0.0, 0.0, 0.1);
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 0.0));
        hover_at(&mut h, P2(250.0, 0.0));
        shot(&mut h, &folder, "01-short-25px.png");
    }

    // 2. A steep 75° segment: the pill is placed by the turn's outside, where a wide box can cross back.
    {
        let mut h = app("");
        zoom(&mut h, 0.0, 0.0, 0.2);
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 0.0));
        let a = 75f64.to_radians();
        hover_at(&mut h, P2(500.0 * a.cos(), 500.0 * a.sin()));
        shot(&mut h, &folder, "02-steep-75.png");
    }

    // 3. A dense hatch under the badge and the reading pill: the opaque pill against hairlines over 20 px lines.
    {
        let mut script = String::new();
        for i in 0..12 {
            let y = f64::from(i) * 200.0;
            script.push_str(&format!("detail_line {{\"start\":[0,{y}],\"end\":[4000,{y}]}}\n"));
        }
        script.push_str("grid {\"start\":[2000,0],\"end\":[2000,2400]}\n");
        let mut h = app(&script);
        zoom(&mut h, 2000.0, 1200.0, 0.1);
        tool(&mut h, "Wall");
        click(&mut h, P2(1000.0, 3000.0));
        hover_at(&mut h, P2(2001.0, 1100.0));
        shot(&mut h, &folder, "03-dense-hatch.png");
    }

    // 4. Tab's face pre-highlight at a fitted-plan zoom: a 1.5 px hover line against the wall's own ink.
    {
        let mut h = app("wall {\"points\":[[0,0],[8000,0]]}");
        zoom(&mut h, 4000.0, 0.0, 0.096);
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 2000.0));
        hover_at(&mut h, P2(4000.0, -100.0));
        key(&mut h, Key::Tab);
        shot(&mut h, &folder, "04-tab-at-1-150.png");
    }

    // 5. The dwell ring half full, on a wall's face.
    {
        let mut h = app("wall {\"points\":[[0,0],[8000,0]]}");
        zoom(&mut h, 4000.0, 0.0, 0.1);
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 2000.0));
        let q = P2(3000.0, -100.0);
        let at = h.state().screen(q);
        h.hover_at(at);
        h.run_steps(3);
        let level = h.state().session.model.levels()[0].0;
        let view = h.state().plan_key().expect("a plan");
        let v = h.state().canvas.current_view(level).expect("a view");
        let found = h.state().canvas.found.get().expect("the hover's snap");
        let target =
            crate::guide_refs::target_at(&h.state().session.model, level, view, &v, v.px(q), &found, &h.state().canvas.under_cursor)
                .expect("the face under the pointer");
        let now = h.state().canvas.now.get();
        h.state_mut().canvas.dwell.set(Some(crate::guide_refs::Dwell {
            key: target.key(),
            from: (at.x, at.y),
            start: now - 0.30,
            marked: false,
        }));
        h.run_steps(1);
        shot(&mut h, &folder, "05-dwell-50.png");
    }

    // 6. One Collinear guide: the Slide glyph, the source dot and ring, and the fading tail.
    {
        let mut h = app("grid {\"start\":[2000,0],\"end\":[2000,5000]}");
        zoom(&mut h, 1500.0, 1500.0, 0.2);
        tool(&mut h, "Wall");
        click(&mut h, P2(1000.0, 1000.0));
        hover_at(&mut h, P2(2000.7, 2300.0));
        shot(&mut h, &folder, "06-one-guide-tick.png");
    }

    // 7. Shift's 0° ray and a 45° marked detail line meeting: the badge says both, and the point is the crossing.
    {
        use crate::guide_refs::PinTarget;
        use bimcraft_core::model::Face;
        let mut h = app("detail_line {\"start\":[1000,0],\"end\":[4000,3000]}");
        let line = h
            .state()
            .session
            .model
            .elements
            .iter()
            .find_map(|(id, e)| matches!(e, Element::DetailLine(_)).then_some(*id))
            .expect("the detail line");
        mark(&mut h, PinTarget::Edge { a: P2(1000.0, 0.0), b: P2(4000.0, 3000.0), id: Some(line), face: Face::Centre, near: None });
        zoom(&mut h, 1000.0, 1000.0, 0.2);
        tool(&mut h, "Wall");
        click(&mut h, P2(0.0, 1000.0));
        h.event(Event::ModifiersChanged(Modifiers::SHIFT));
        let at = h.state().screen(P2(2000.0, 1003.0));
        h.hover_at(at);
        h.run();
        // The frame that is seen must be a frame with Shift still held.
        shot(&mut h, &folder, "07-shift-cross.png");
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.run();
    }

    // 8. Three marked references and the status readout: two wall faces and a point, the cap and the pin shapes.
    {
        let mut h = app(concat!(
            "wall {\"points\":[[0,0],[6000,0]]}\n",
            "wall {\"points\":[[0,3000],[6000,3000]]}\n",
            "wall {\"points\":[[0,6000],[3000,6000]]}"
        ));
        h.state_mut().snaps.mark_on_hover = false;
        tool(&mut h, "Wall");
        click(&mut h, P2(1000.0, 1500.0));
        hover_at(&mut h, P2(3000.0, -100.0));
        key(&mut h, Key::Q);
        hover_at(&mut h, P2(3000.0, 3100.0));
        key(&mut h, Key::Q);
        hover_at(&mut h, P2(3000.0, 6000.0));
        key(&mut h, Key::Q);
        shot(&mut h, &folder, "08-three-pins.png");
    }

    // 9. The same crossing on the dark canvas: lavender, the badge's ground and its text.
    {
        let mut h = app(concat!(
            "wall {\"points\": [[0, 0], [5000, 0]]}\n",
            "wall {\"points\": [[0, 5000], [2500, 6000]]}\n",
            "wall {\"points\": [[3000, 4500], [3000, 6000]]}\n",
            "wall {\"points\": [[0, 3000], [1500, 3000]]}"
        ));
        h.state_mut().snaps.guides_from_whole_model = true;
        h.ctx.set_theme(egui::ThemePreference::Dark);
        h.run();
        tool(&mut h, "Wall");
        click(&mut h, P2(500.0, 1200.0));
        hover_at(&mut h, P2(3006.0, 3006.0));
        shot(&mut h, &folder, "09-dark-canvas.png");
    }

    // 10. A mark at the view's top: the badge flips and clamps, the reading pill keeps off it.
    {
        let mut h = app("wall {\"points\":[[8000,6900],[10000,6900]]}");
        let level = h.state().plan_key().expect("a plan");
        let half = {
            let v = h.state().canvas.current_view(level).expect("a view");
            f64::from(v.height) / 2.0
        };
        // The wall sits ten pixels under the view's top, whatever the plan area's own height is.
        zoom(&mut h, 9000.0, 6900.0 - (half - 10.0) / 0.1, 0.1);
        h.state_mut().snaps.mark_on_hover = false;
        tool(&mut h, "Wall");
        click(&mut h, P2(8000.0, 5000.0));
        hover_at(&mut h, P2(9000.0, 6900.0));
        key(&mut h, Key::Q);
        hover_at(&mut h, P2(9000.0, 6900.0));
        shot(&mut h, &folder, "10-pin-at-the-edge.png");
    }

    // 11. A guide at 2 px/mm: the guide must be taken from the pointer, not from the snap's 10 mm point. This is the
    //     scene the fix is about, and the assertion is the audit's `11-guide-at-8x` case.
    {
        let mut h = app("grid {\"start\":[0,3],\"end\":[8000,3]}");
        zoom(&mut h, 4123.7, 3.0, 2.0);
        tool(&mut h, "Wall");
        click(&mut h, P2(4123.7, -40.0));
        hover_at(&mut h, P2(4123.7, 6.6));
        let f = h.state().canvas.found.get().expect("the hover's snap");
        assert!(f.at.dist(P2(4123.7, 3.0)) < 0.01, "the guide is taken at 2 px/mm: {f:?}");
        shot(&mut h, &folder, "11-guide-at-8x.png");
    }

    // 12. A 40 m grid's relation: the strong segment starts at the grid's nearer end, twenty metres away, not at the
    //     cursor. The screenshot is for judging whether it should taper instead; the assertion records the length.
    {
        let mut h = app("grid {\"start\":[20000,-20000],\"end\":[20000,20000]}");
        zoom(&mut h, 20000.0, 0.0, 0.05);
        tool(&mut h, "Wall");
        click(&mut h, P2(19000.0, 2000.0));
        hover_at(&mut h, P2(20000.7, 0.0));
        let level = h.state().plan_key().expect("a plan");
        let v = h.state().canvas.current_view(level).expect("a view");
        let res = h.state().alignments(&h.state().session.model, level, &v, P2(20000.7, 0.0), &[P2(19000.0, 2000.0)]);
        let length = res.first().map_or(0.0, |r| r.reference_point.dist(r.snapped_point));
        eprintln!("12-grid-anchor: the strong relation runs {length} mm to its grid end");
        assert!(length > 10_000.0, "the relation starts at the grid's end, not the cursor: {length} mm");
        shot(&mut h, &folder, "12-grid-anchor.png");
    }
}

/// What the guides look like in both styles, for a person to look at: `BIMCRAFT_GUIDES_DUMP=<folder>` writes a wall being
/// drawn to the crossing of two guides, once with each.
#[test]
fn guides_screenshots() {
    use crate::style::GuideStyle;
    let Ok(folder) = std::env::var("BIMCRAFT_GUIDES_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for style in GuideStyle::ALL {
        // A crossing of two guides in open space: x from the end of the wall above, y from the end of the wall to the
        // left, and the cursor clear of every wall, so the snap stays `Free` (a snap to real geometry suppresses guides).
        let mut h = app(concat!(
            "wall {\"points\": [[0, 0], [5000, 0]]}\n",
            "wall {\"points\": [[0, 5000], [2500, 6000]]}\n",
            "wall {\"points\": [[3000, 4500], [3000, 6000]]}\n",
            "wall {\"points\": [[0, 3000], [1500, 3000]]}"
        ));
        h.state_mut().guide_style = style;
        // This A/B shows the whole-model look; the local context has its own screenshots.
        h.state_mut().snaps.guides_from_whole_model = true;
        tool(&mut h, "Wall");
        click(&mut h, P2(500.0, 1200.0));
        let found = hover(&mut h, P2(3006.0, 3006.0)).expect("the cursor is over the plan");
        assert!(
            (found.at.0 - 3000.0).abs() < 0.01 && (found.at.1 - 3000.0).abs() < 0.01,
            "{style:?}: the crossing, not the pointer: {:?}",
            found.at
        );
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("guides-{}.png", style.code()))).unwrap();
    }
}

#[test]
fn grids_drawn_near_alignment_guide_snap_to_other_grids_reference_points() {
    use bimcraft_core::snap::Kind;
    let mut h = app("");
    // The first grid's end is marked by hand: a rest would mark whichever end the hand last stood on, and the test is the
    // guide that mark lends, not the dwell.
    h.state_mut().snaps.mark_on_hover = false;
    // First grid from (0, 0) to (0, 5000)
    tool(&mut h, "Grid");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(0.0, 5000.0));
    let first = ids(&h, "grid")[0];
    mark(&mut h, crate::guide_refs::PinTarget::Point { at: P2(0.0, 5000.0), kind: Kind::Endpoint, id: Some(first) });

    // Second grid starting at (3000, 0)
    tool(&mut h, "Grid");
    click(&mut h, P2(3000.0, 0.0));
    // Second point near (3000, 5003) aligned with Y=5000
    click(&mut h, P2(3000.0, 5003.0));

    let grid = ids(&h, "grid");
    assert_eq!(grid.len(), 2);
    let point = |key: &str| -> [f64; 2] {
        let v = field(&h, grid[1], key);
        [v[0].as_f64().expect("x"), v[1].as_f64().expect("y")]
    };
    // A guide keeps the pointer's own x, so 1/1000 mm of the view's arithmetic is allowed: the alignment is the point.
    let (start, end) = (point("start"), point("end"));
    assert!((start[0] - 3000.0).abs() < 0.01 && start[1].abs() < 0.01, "{start:?}");
    assert!((end[0] - 3000.0).abs() < 0.01 && (end[1] - 5000.0).abs() < 0.01, "{end:?}");
}

#[test]
fn shortcut_toggles_alignment_guides() {
    let mut h = app("");
    assert!(h.state().snaps.alignment_guides);
    // F11 is free now: only F8, and the legacy two-letter sequences, move the guides.
    key(&mut h, Key::F11);
    assert!(h.state().snaps.alignment_guides, "F11 no longer toggles the guides");
    key(&mut h, Key::F8);
    assert!(!h.state().snaps.alignment_guides);
    key(&mut h, Key::F8);
    assert!(h.state().snaps.alignment_guides);
    shortcut(&mut h, "AG");
    assert!(!h.state().snaps.alignment_guides);
    shortcut(&mut h, "OG");
    assert!(h.state().snaps.alignment_guides);
}
