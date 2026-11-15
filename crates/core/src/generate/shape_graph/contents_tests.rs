//! Placement of windows and components on synthetic rooms: what the spec asks of them, judged from the points and the footprints.

use super::*;
use crate::generate::shape_graph::family::Conn;

fn content(family: &str, types: &[&str], count: (i64, i64), against: Against) -> Content {
    Content { family: family.into(), types: types.iter().map(|t| (*t).into()).collect(), count, against }
}

/// A room from (0, 0) to (w, h), counter-clockwise, with its four walls 250 thick round it.
fn boxed(w: i64, h: i64) -> (Vec<P>, Vec<WallAt>) {
    let poly = vec![[0.0, 0.0], [w as f64, 0.0], [w as f64, h as f64], [0.0, h as f64]];
    let (x0, y0, x1, y1) = (-125, -125, w + 125, h + 125);
    let wall = |a, b| WallAt { exterior: true, a, b };
    (poly, vec![wall([x0, y0], [x1, y0]), wall([x1, y0], [x1, y1]), wall([x1, y1], [x0, y1]), wall([x0, y1], [x0, y0])])
}

fn door(at: [i64; 2], width: i64) -> DoorAt {
    DoorAt {
        rooms: (0, None),
        conn: Conn::Single,
        at,
        family: "Single Door",
        ty: format!("{width} x 2100"),
        width,
        op: "swing",
        flip_facing: false,
        flip_hand: false,
        opens_into: None,
        hinge: None,
    }
}

fn room<'a>(poly: &[P], window: bool, contents: &'a [Content]) -> RoomIn<'a> {
    RoomIn { poly: poly.to_vec(), window, contents }
}

fn box_of(c: &ComponentAt) -> [P; 4] {
    let fp = footprint(&c.family, &c.ty).expect("a built-in footprint");
    corners(fp, [c.at[0] as f64, c.at[1] as f64], c.angle)
}

fn bounds(b: &[P; 4]) -> (f64, f64, f64, f64) {
    let xs = b.iter().map(|p| p[0]);
    let ys = b.iter().map(|p| p[1]);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

/// A window's span along its wall's centre line, and its distance along the room's face from the face's start, and the face's length.
fn window_span(win: &WindowAt, w: i64, h: i64) -> ((P, P), f64, f64) {
    let at = [win.at[0] as f64, win.at[1] as f64];
    let half = win.width as f64 / 2.0;
    let (wf, hf) = (w as f64, h as f64);
    if (at[1] + 125.0).abs() < 1.0 || (at[1] - hf - 125.0).abs() < 1.0 {
        (([at[0] - half, at[1]], [at[0] + half, at[1]]), at[0], wf)
    } else {
        assert!((at[0] + 125.0).abs() < 1.0 || (at[0] - wf - 125.0).abs() < 1.0, "window at {at:?} is on no wall");
        (([at[0], at[1] - half], [at[0], at[1] + half]), at[1], hf)
    }
}

#[test]
fn windows_come_one_to_each_three_to_five_metres_and_keep_clear() {
    for (w, h) in [(4000, 3000), (6000, 5000), (9000, 4000), (3200, 3200)] {
        let (poly, walls) = boxed(w, h);
        let doors = vec![door([w / 2, -125], 900)];
        let p = place(&[room(&poly, true, &[])], &walls, &doors, &[]);
        let total = (2 * (w + h)) as f64;
        let (lo, hi) = ((total / 5000.0).floor() as usize, (total / 3000.0).ceil() as usize);
        assert!((lo..=hi).contains(&p.windows.len()), "{w}x{h}: {} windows, want {lo}..{hi}", p.windows.len());
        let door_span = ([w as f64 / 2.0 - 450.0, -125.0], [w as f64 / 2.0 + 450.0, -125.0]);
        for (i, a) in p.windows.iter().enumerate() {
            let (mine, along, len) = window_span(a, w, h);
            let half = a.width as f64 / 2.0;
            assert!(along - half >= 299.0 && len - along - half >= 299.0, "{w}x{h}: window {:?} near a corner", a.at);
            assert!(seg_seg(mine, door_span) >= 299.0, "{w}x{h}: window {:?} near the door", a.at);
            for b in &p.windows[i + 1..] {
                assert!(seg_seg(mine, window_span(b, w, h).0) >= 299.0, "{w}x{h}: windows {:?} and {:?} too close", a.at, b.at);
            }
        }
    }
}

#[test]
fn a_room_without_window_true_and_a_face_too_short_get_none() {
    let (poly, walls) = boxed(6000, 4000);
    assert!(place(&[room(&poly, false, &[])], &walls, &[], &[]).windows.is_empty());
    // A face of 1100 mm cannot hold the narrowest window and its two clearances.
    let (poly, walls) = boxed(1100, 1100);
    assert!(place(&[room(&poly, true, &[])], &walls, &[], &[]).windows.is_empty());
    // A room whose walls are all interior has no window.
    let (poly, mut walls) = boxed(6000, 4000);
    for w in &mut walls {
        w.exterior = false;
    }
    assert!(place(&[room(&poly, true, &[])], &walls, &[], &[]).windows.is_empty());
}

#[test]
fn each_against_puts_the_box_where_it_says() {
    let (poly, walls) = boxed(5000, 4000);
    let list = [
        content("Bed", &["1600 x 2000"], (1, 1), Against::Wall),
        content("WC", &["400 x 650"], (1, 1), Against::Corner),
        content("Table", &["1200 x 800"], (1, 1), Against::Centre),
    ];
    let p = place(&[room(&poly, false, &list)], &walls, &[], &[]);
    assert_eq!(p.components.len(), 3, "{:?}", p.left_out);
    let sides_on_wall = |b: &[P; 4]| {
        let (x0, y0, x1, y1) = bounds(b);
        let near = |v: f64, w: f64| (v - w).abs() <= 5.0;
        [near(x0, 0.0), near(x1, 5000.0), near(y0, 0.0), near(y1, 4000.0)].iter().filter(|x| **x).count()
    };
    for c in &p.components {
        let b = box_of(c);
        let (x0, y0, x1, y1) = bounds(&b);
        assert!(x0 >= 0.0 && y0 >= 0.0 && x1 <= 5000.0 && y1 <= 4000.0, "{c:?} leaves the room");
        match c.family.as_str() {
            "Bed" => assert!(sides_on_wall(&b) >= 1, "{c:?}"),
            "WC" => assert!(sides_on_wall(&b) >= 2, "{c:?}"),
            _ => assert_eq!(sides_on_wall(&b), 0, "{c:?} is on a wall"),
        }
    }
}

#[test]
fn a_bed_has_its_back_on_the_wall() {
    let (poly, walls) = boxed(4000, 4000);
    let list = [content("Bed", &["1600 x 2000"], (1, 1), Against::Wall)];
    let p = place(&[room(&poly, false, &list)], &walls, &[], &[]);
    let c = &p.components[0];
    // The back is at local y 0; turned by the angle, the box's side at the back is the one nearest the wall.
    let (s, k) = (c.angle.to_radians().sin().round(), c.angle.to_radians().cos().round());
    let back = [-s, k];
    let (x0, y0, x1, y1) = bounds(&box_of(c));
    let gap = if back[0] > 0.5 {
        4000.0 - x1
    } else if back[0] < -0.5 {
        x0
    } else if back[1] > 0.5 {
        4000.0 - y1
    } else {
        y0
    };
    assert!(gap.abs() <= 5.0, "{c:?}: back {back:?} is {gap} mm from its wall");
}

#[test]
fn doors_swings_and_openings_keep_their_clear_space() {
    let (poly, walls) = boxed(5000, 4000);
    let doors = vec![door([2500, -125], 900), door([5125, 2000], 900)];
    let seps = vec![vec![[0, 3000], [0, 3900]]];
    let list = [
        content("Bed", &["1600 x 2000", "900 x 2000"], (1, 1), Against::Wall),
        content("Sofa", &["2000 x 900"], (1, 1), Against::Wall),
        content("Table", &["1200 x 800"], (1, 1), Against::Centre),
    ];
    let p = place(&[room(&poly, false, &list)], &walls, &doors, &seps);
    assert!(!p.components.is_empty());
    let zones: Vec<[P; 4]> = vec![
        rect([2500.0, -125.0], [1.0, 0.0], 450.0, 125.0 + 900.0),
        rect([5125.0, 2000.0], [0.0, 1.0], 450.0, 125.0 + 900.0),
        rect([0.0, 3450.0], [0.0, 1.0], 450.0, 600.0),
    ];
    for c in &p.components {
        for z in &zones {
            assert!(!near(&box_of(c), z, 0.0), "{c:?} stands in the clear space {z:?}");
        }
    }
}

#[test]
fn components_overlap_nothing_and_a_room_too_small_leaves_them_out() {
    let (poly, walls) = boxed(6000, 5000);
    let list = [
        content("Table", &["1200 x 800"], (3, 3), Against::Centre),
        content("Bed", &["1600 x 2000"], (2, 2), Against::Wall),
        content("Sofa", &["2000 x 900"], (2, 2), Against::Wall),
    ];
    let p = place(&[room(&poly, false, &list)], &walls, &[], &[]);
    assert!(p.components.len() >= 4, "{:?}", p.components);
    for (i, a) in p.components.iter().enumerate() {
        for b in &p.components[i + 1..] {
            assert!(!near(&box_of(a), &box_of(b), 0.0), "{a:?} and {b:?} overlap");
        }
    }
    let (small, walls) = boxed(1500, 1500);
    let list = [content("Bed", &["1600 x 2000"], (1, 1), Against::Wall)];
    let p = place(&[room(&small, false, &list)], &walls, &[], &[]);
    assert!(p.components.is_empty());
    assert_eq!(p.left_out, vec![LeftOut { room: 0, family: "Bed".into() }]);
}

#[test]
fn placement_is_the_same_each_time() {
    let (poly, walls) = boxed(5200, 4300);
    let list =
        [content("Sofa", &["2000 x 900", "1600 x 900"], (1, 1), Against::Wall), content("Table", &["1200 x 800"], (0, 1), Against::Centre)];
    let doors = vec![door([1800, -125], 900)];
    let a = place(&[room(&poly, true, &list)], &walls, &doors, &[]);
    let b = place(&[room(&poly, true, &list)], &walls, &doors, &[]);
    assert_eq!((a.windows, a.components), (b.windows, b.components));
}
