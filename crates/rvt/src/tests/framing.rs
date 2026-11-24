//! Structural framing along the lines their graphics draw, framing and foundations from their records' boxes, and the categories that come in as boxes besides: stairs' runs,
//! landings and supports, railings, curtain panels and mullions, wall sweeps, slab edges, furniture systems, site pads, masses,
//! trusses, beam systems, stiffeners, connections and conduits. What a box would invent (a beam across the axes, a path that
//! turns, a whole whose parts come in) is left out and said.

use super::walls::{base, import, level_named};
use crate::project_fixture::Rec;
use bimcraft_core::family::Category;
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Column, Component, Element, Justify, Run};

const FRAMING: i64 = -2_001_320;
const FOUNDATIONS: i64 = -2_001_300;
const STAIRS: i64 = -2_000_120;
const STAIR_RUNS: i64 = -2_000_919;
const STAIR_LANDINGS: i64 = -2_000_920;
const RAILINGS: i64 = -2_000_126;
const MULLIONS: i64 = -2_000_171;
const SLAB_EDGES: i64 = -2_001_392;
const TRUSSES: i64 = -2_001_336;
const CONDUITS: i64 = -2_008_132;

/// A beam's ends in plan, the heights of its ends over its level (the second where it differs), its justification and its level.
type Line = ([f64; 2], [f64; 2], f64, Option<f64>, Justify, bimcraft_core::ids::Guid);

fn beams(i: &Imported) -> Vec<&Run> {
    i.model.elements.values().filter_map(|e| if let Element::Beam(r) = e { Some(r) } else { None }).collect()
}

fn footings(i: &Imported) -> Vec<&Column> {
    i.model.elements.values().filter_map(|e| if let Element::Footing(c) = e { Some(c) } else { None }).collect()
}

fn components(i: &Imported) -> Vec<&Component> {
    i.model.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect()
}

#[test]
fn structural_framing_comes_in_as_beams_along_its_boxs_longer_side_and_one_no_box_can_give_is_left_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A beam 20 ft east, 1 ft wide and 2 ft deep, its top at level 2 (10 ft); one 12 ft north, half a foot by 1 ft, its top
        // 1 ft under level 2, so on level 1 (the level at or below its top, as every reader takes it).
        p.recs.push(Rec::new(300, FRAMING).at([0.0, -0.5, 8.0, 20.0, 0.5, 10.0], &[]));
        p.recs.push(Rec::new(301, FRAMING).at([5.0, 0.0, 8.0, 5.5, 12.0, 9.0], &[]));
        // Across the axes (its box nearly square in plan), and standing (deeper than it is long): no beam a box gives.
        p.recs.push(Rec::new(302, FRAMING).at([0.0, 0.0, 9.0, 10.0, 10.0, 10.0], &[]));
        p.recs.push(Rec::new(303, FRAMING).at([0.0, 0.0, 0.0, 1.0, 0.25, 9.0], &[]));
        let i = import(&p);
        let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
        let got: Vec<Line> =
            beams(&i).iter().map(|r| ([r.start.0, r.start.1], [r.end.0, r.end.1], r.offset, r.end_offset, r.justify, r.level)).collect();
        assert_eq!(
            got,
            [([0.0, 0.0], [6096.0, 0.0], 0.0, None, Justify::Top, l2), ([1600.2, 0.0], [1600.2, 3657.6], 2743.2, None, Justify::Top, l1),],
            "Revit {release}"
        );
        let sections: Vec<(f64, f64)> = beams(&i)
            .iter()
            .map(|r| {
                let t = &i.model.types.runs["beam"][&r.type_name];
                (t.width, t.depth)
            })
            .collect();
        assert_eq!(sections, [(304.8, 609.6), (152.4, 304.8)], "Revit {release}");
        assert!(i.model.check().is_ok());
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(says("2 structural framing members: comes in as a beam along its box's longer side"), "{:?}", i.skipped);
        assert!(says("2 structural framing members left out, its box is no beam's") && says("ids 302, 303"), "{:?}", i.skipped);
        assert!(!says("Structural Framing left out, the importer has no reader"), "{:?}", i.skipped);
    }
}

#[test]
fn a_framing_member_comes_in_along_the_line_its_graphics_draw_with_their_section_and_is_drawn_as_they_draw_it() {
    use crate::project_fixture::{box_tree, graphics_block, instance_tree, tree_bytes};
    let mut p = base(2024);
    // Two symbols: a member 20 ft long, 1 ft wide and 2 ft deep, and one 10 by 1 by 1 ft, each its corner at the origin.
    p.graphics.extend(graphics_block(660, &tree_bytes(&box_tree(660, [20.0, 1.0, 2.0], None))));
    p.graphics.extend(graphics_block(661, &tree_bytes(&box_tree(661, [10.0, 1.0, 1.0], None))));
    p.extra_ids.extend([660, 661]);
    // Member 300 runs across the axes in plan (3, 4, 5), its top at level 2; member 301 rises 3 in 5 to the east from level 1. Each
    // record's box is the box round what it draws.
    let (across, whole_300) = ([[0.8, 0.6, 0.0], [-0.6, 0.8, 0.0], [0.0, 0.0, 1.0]], [9.4, 10.0, 8.0, 26.0, 22.8, 10.0]);
    let (rising, whole_301) = ([[0.8, 0.0, 0.6], [0.0, 1.0, 0.0], [-0.6, 0.0, 0.8]], [-0.6, 30.0, 0.0, 8.0, 31.0, 6.8]);
    p.recs.push(Rec::new(300, FRAMING).at(whole_300, &[]));
    p.recs.push(Rec::new(301, FRAMING).at(whole_301, &[]));
    p.graphics.extend(graphics_block(300, &tree_bytes(&instance_tree(300, 660, across, [10.0, 10.0, 8.0], whole_300))));
    p.graphics.extend(graphics_block(301, &tree_bytes(&instance_tree(301, 661, rising, [0.0, 30.0, 0.0], whole_301))));
    // Member 302's box runs across the axes too, and the file draws nothing of it: no line, so no beam.
    p.recs.push(Rec::new(302, FRAMING).at([0.0, 40.0, 9.0, 10.0, 50.0, 10.0], &[]));
    // Member 303 draws a solid of its own, 20 by 1 by 2 ft at the origin, and a stick line half a foot past each of its ends; its
    // record's box holds its location line, which runs 0.2 ft past the solid: between the two.
    p.recs.push(Rec::new(303, FRAMING).at([-0.2, 0.0, 0.0, 20.2, 1.0, 2.0], &[]));
    p.graphics.extend(graphics_block(303, &tree_bytes(&box_tree(303, [20.0, 1.0, 2.0], Some([-0.5, 0.0, 0.0, 20.5, 1.0, 2.0])))));
    let i = import(&p);
    assert!(i.model.check().is_ok(), "{:?}", i.model.check());
    let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
    let got: Vec<Line> =
        beams(&i).iter().map(|r| ([r.start.0, r.start.1], [r.end.0, r.end.1], r.offset, r.end_offset, r.justify, r.level)).collect();
    assert_eq!(
        got,
        [
            // The middle of its top, from end to end: from (9.7, 10.4) to (25.7, 22.4) ft, at level 2.
            ([2956.56, 3169.92], [7833.36, 6827.52], 0.0, None, Justify::Top, l2),
            // From (-0.6, 30.5) to (7.4, 30.5) ft, its top 0.8 ft over level 1 there and 6.8 ft at its east end.
            ([-182.88, 9296.4], [2255.52, 9296.4], 243.84, Some(2072.64), Justify::Top, l1),
            // Its solid's line, not its record's: the top of the solid, 2 ft over level 1.
            ([0.0, 152.4], [6096.0, 152.4], 609.6, None, Justify::Top, l1),
        ],
        "{:?}",
        i.skipped
    );
    let sections: Vec<(f64, f64)> = beams(&i)
        .iter()
        .map(|r| {
            let t = &i.model.types.runs["beam"][&r.type_name];
            (t.width, t.depth)
        })
        .collect();
    assert_eq!(sections, [(304.8, 609.6), (304.8, 304.8), (304.8, 609.6)]);
    assert!(beams(&i).iter().all(|r| r.imported.is_some()), "{:?}", i.skipped);
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    assert!(says("3 structural framing members: comes in as a beam along the line its graphics draw"), "{:?}", i.skipped);
    assert!(says("3 structural framing members: it is drawn in 3D as its own graphics draw it"), "{:?}", i.skipped);
    assert!(says("1 structural framing member left out, its box is no beam's") && says("ids 302"), "{:?}", i.skipped);
}

#[test]
fn structural_foundations_come_in_as_isolated_footings_of_their_box() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A pad 4 by 3 ft and 2 ft thick, its top at level 1; a long one under a wall.
        p.recs.push(Rec::new(300, FOUNDATIONS).at([-2.0, -1.5, -2.0, 2.0, 1.5, 0.0], &[]));
        p.recs.push(Rec::new(301, FOUNDATIONS).at([0.0, -1.0, -1.0, 20.0, 1.0, 0.0], &[]));
        let i = import(&p);
        let l1 = level_named(&i, "Level 1");
        let got: Vec<([f64; 2], f64, f64, f64, bimcraft_core::ids::Guid)> =
            footings(&i).iter().map(|c| ([c.at.0, c.at.1], c.angle, c.height, c.base_offset, c.level)).collect();
        assert_eq!(got, [([0.0, 0.0], 0.0, 609.6, -609.6, l1), ([3048.0, 0.0], 0.0, 304.8, -304.8, l1)], "Revit {release}");
        let sizes: Vec<(f64, f64)> = footings(&i)
            .iter()
            .map(|c| {
                let t = &i.model.types.footing[&c.type_name];
                (t.width, t.depth)
            })
            .collect();
        assert_eq!(sizes, [(1219.2, 914.4), (6096.0, 609.6)], "Revit {release}");
        assert!(i.model.check().is_ok());
        assert!(
            i.skipped.iter().any(|s| s.contains("2 structural foundations: comes in as an isolated footing of its box")),
            "{:?}",
            i.skipped
        );
    }
}

#[test]
fn the_other_known_categories_come_in_as_boxes_of_a_family_of_their_category() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A flight's run and a landing; a straight railing; a mullion; a straight slab edge; a conduit; a furniture system.
        p.recs.push(Rec::new(300, STAIR_RUNS).at([0.0, 0.0, 0.0, 10.0, 3.0, 5.0], &[]));
        p.recs.push(Rec::new(301, STAIR_LANDINGS).at([10.0, 0.0, 4.5, 14.0, 3.0, 5.0], &[]));
        p.recs.push(Rec::new(302, RAILINGS).at([0.0, 3.0, 0.0, 10.0, 3.2, 8.0], &[]));
        p.recs.push(Rec::new(303, MULLIONS).at([20.0, 0.0, 0.0, 20.2, 0.5, 10.0], &[]));
        p.recs.push(Rec::new(304, SLAB_EDGES).at([0.0, -1.0, 9.0, 20.0, 0.0, 10.0], &[]));
        p.recs.push(Rec::new(305, CONDUITS).at([0.0, 5.0, 9.0, 15.0, 5.1, 9.1], &[]));
        p.recs.push(Rec::new(306, -2_001_100).at([3.0, 6.0, 0.0, 6.0, 9.0, 2.5], &[]));
        let i = import(&p);
        let mut got: Vec<(&str, Category)> =
            components(&i).iter().map(|c| (c.family.as_str(), i.model.families[&c.family].category)).collect();
        got.sort_by_key(|(f, _)| *f);
        assert_eq!(
            got,
            [
                ("Conduits", Category::Generic),
                ("Curtain Wall Mullions", Category::Generic),
                ("Furniture Systems", Category::Furniture),
                ("Railings", Category::Generic),
                ("Slab Edges", Category::Generic),
                ("Stair Landings", Category::Generic),
                ("Stair Runs", Category::Generic),
            ],
            "Revit {release}"
        );
        // The landing: its box's middle in plan, on level 1 at 4.5 ft, its box's size.
        let landing = components(&i)
            .into_iter()
            .find(|c| c.family == "Stair Landings")
            .map(|c| (c.at, c.offset, ["Width", "Depth", "Height"].map(|n| c.params[n].as_f64().unwrap_or(f64::NAN))));
        assert_eq!(landing, Some((bimcraft_core::geom::P2(3657.6, 457.2), 1371.6, [1219.2, 914.4, 152.4])));
        assert!(i.model.check().is_ok());
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        for kind in ["stair run", "stair landing", "railing", "mullion", "slab edge", "conduit", "furniture system"] {
            assert!(says(&format!("1 {kind}: comes in as a box")), "{kind}: {:?}", i.skipped);
        }
        assert!(!says("no reader"), "{:?}", i.skipped);
    }
}

#[test]
fn a_box_that_would_invent_a_body_is_left_out_saying_why() {
    for release in [2019, 2024] {
        let mut p = base(release);
        // A railing that turns a corner (its box the envelope of an L), a slab edge round a whole slab.
        p.recs.push(Rec::new(300, RAILINGS).at([0.0, 0.0, 0.0, 10.0, 8.0, 3.0], &[]));
        p.recs.push(Rec::new(301, SLAB_EDGES).at([0.0, 0.0, 9.0, 20.0, 15.0, 10.0], &[]));
        // A stair whose box holds its run: the run comes in, the stair is their whole. A stair holding none comes in as a box.
        p.recs.push(Rec::new(302, STAIRS).at([0.0, 0.0, 0.0, 10.0, 3.0, 10.0], &[]));
        p.recs.push(Rec::new(303, STAIR_RUNS).at([0.0, 0.0, 0.0, 10.0, 3.0, 10.0], &[]));
        p.recs.push(Rec::new(304, STAIRS).at([30.0, 0.0, 0.0, 34.0, 3.0, 10.0], &[]));
        // A truss whose box holds a framing member: the member is a beam, the truss their whole.
        p.recs.push(Rec::new(305, TRUSSES).at([0.0, 20.0, 10.0, 30.0, 20.5, 15.0], &[]));
        p.recs.push(Rec::new(306, FRAMING).at([0.0, 20.0, 10.0, 30.0, 20.5, 11.0], &[]));
        let i = import(&p);
        let mut got: Vec<&str> = components(&i).iter().map(|c| c.family.as_str()).collect();
        got.sort();
        assert_eq!(got, ["Stair Runs", "Stairs"], "Revit {release}");
        assert_eq!(beams(&i).len(), 1, "Revit {release}");
        let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
        assert!(
            says("1 railing left out, its box is the envelope of a path that turns or runs across the axes") && says("ids 300"),
            "{:?}",
            i.skipped
        );
        assert!(says("1 slab edge left out, its box is the envelope of a path"), "{:?}", i.skipped);
        assert!(
            says("1 stair left out, its box holds those of its parts, which come in on their own") && says("ids 302"),
            "{:?}",
            i.skipped
        );
        assert!(says("1 truss left out, its box holds those of its parts"), "{:?}", i.skipped);
        assert!(says("1 stair: comes in as a box") && says("ids 304"), "{:?}", i.skipped);
        assert!(i.model.check().is_ok());
    }
}

#[test]
fn a_mutation_sweep_over_framing_foundations_and_boxes_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        p.recs.push(Rec::new(300, FRAMING).at([0.0, -0.5, 8.0, 20.0, 0.5, 10.0], &[]));
        p.recs.push(Rec::new(301, FOUNDATIONS).at([-2.0, -1.5, -2.0, 2.0, 1.5, 0.0], &[]));
        p.recs.push(Rec::new(302, STAIRS).at([0.0, 0.0, 0.0, 10.0, 3.0, 10.0], &[]));
        p.recs.push(Rec::new(303, STAIR_RUNS).at([0.0, 0.0, 0.0, 10.0, 3.0, 10.0], &[]));
        p.recs.push(Rec::new(304, RAILINGS).at([0.0, 3.0, 0.0, 10.0, 3.2, 8.0], &[]));
        p.members = 3;
        super::sweep(&p, 0x51ab_7c3d_0e9f_2a46 ^ u64::from(release), 800, |i, round| {
            assert!(beams(i).len() + footings(i).len() + components(i).len() <= 5, "round {round}");
        });
    }
}

/// A prism of `profile` (points across and up, millimetres) from `from` `length` along `axis`, the profile at its far end `grow`
/// times as deep; its sides as two triangles each, its ends as fans.
fn prism(profile: &[[f64; 2]], from: [f64; 3], axis: [f64; 3], length: f64, grow: f64) -> Vec<[[f64; 3]; 3]> {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let a = axis.map(|v| v / n);
    let flat = (a[0] * a[0] + a[1] * a[1]).sqrt();
    let across = if flat > 1e-9 { [-a[1] / flat, a[0] / flat, 0.0] } else { [1.0, 0.0, 0.0] };
    let up = [a[1] * across[2] - a[2] * across[1], a[2] * across[0] - a[0] * across[2], a[0] * across[1] - a[1] * across[0]];
    let at = |s: f64, [w, u]: [f64; 2], k: f64| std::array::from_fn(|i| from[i] + a[i] * s + across[i] * w + up[i] * u * k);
    let (near, far): (Vec<[f64; 3]>, Vec<[f64; 3]>) = profile.iter().map(|p| (at(0.0, *p, 1.0), at(length, *p, grow))).unzip();
    let mut out = Vec::new();
    for i in 0..profile.len() {
        let j = (i + 1) % profile.len();
        out.push([near[i], near[j], far[j]]);
        out.push([near[i], far[j], far[i]]);
    }
    for i in 1..profile.len() - 1 {
        out.push([near[0], near[i], near[i + 1]]);
        out.push([far[0], far[i + 1], far[i]]);
    }
    out
}

/// An I section 200 mm wide and 400 deep, its top's middle at the origin: 15 mm flanges, a 10 mm web.
const I_SECTION: [[f64; 2]; 12] = [
    [-100.0, 0.0],
    [100.0, 0.0],
    [100.0, -15.0],
    [5.0, -15.0],
    [5.0, -385.0],
    [100.0, -385.0],
    [100.0, -400.0],
    [-100.0, -400.0],
    [-100.0, -385.0],
    [-5.0, -385.0],
    [-5.0, -15.0],
    [-100.0, -15.0],
];

#[test]
fn a_straight_members_line_is_where_its_faces_run_and_one_that_tapers_has_none() {
    use crate::framing::line_of;
    let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 0.01);
    // Sloped and across the axes in plan, drawn either way: the same line, from its west end.
    let axis = [3.0, 4.0, 1.0];
    let n = 26f64.sqrt();
    let end = [1000.0 + 6000.0 * 3.0 / n, 2000.0 + 6000.0 * 4.0 / n, 3000.0 + 6000.0 / n];
    for (from, way) in [([1000.0, 2000.0, 3000.0], axis), (end, axis.map(|v| -v))] {
        let line = line_of(&prism(&I_SECTION, from, way, 6000.0, 1.0)).unwrap_or_else(|| panic!("{from:?}"));
        assert!(near(line.start, [1000.0, 2000.0, 3000.0]) && near(line.end, end), "{line:?}");
        assert!((line.width - 200.0).abs() < 0.01 && (line.depth - 400.0).abs() < 0.01, "{line:?}");
    }
    // A post: it stands, east across it.
    let post = line_of(&prism(&I_SECTION, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 3000.0, 1.0)).unwrap_or_else(|| panic!("a post"));
    assert!((post.start[0] - post.end[0]).abs() < 0.01 && (post.start[1] - post.end[1]).abs() < 0.01, "{post:?}");
    assert!((post.start[2] - post.end[2]).abs() > 2999.0, "{post:?}");
    // A web's openings, a square hole every metre through its middle, are holes through it: its line and section stay.
    let mut holed = prism(&I_SECTION, [0.0; 3], [1.0, 0.0, 0.0], 6000.0, 1.0);
    for k in 1..6 {
        let x = 1000.0 * f64::from(k);
        for (a, b) in [([x - 50.0, -5.0, -150.0], [x + 50.0, -5.0, -150.0]), ([x - 50.0, -5.0, -250.0], [x - 50.0, -5.0, -150.0])] {
            holed.push([a, b, [b[0], 5.0, b[2]]]);
        }
    }
    let line = line_of(&holed).unwrap_or_else(|| panic!("a holed web"));
    assert!(near(line.start, [0.0, 0.0, 0.0]) && near(line.end, [6000.0, 0.0, 0.0]) && (line.depth - 400.0).abs() < 0.01, "{line:?}");
    // A member whose far end is twice as deep is no prism of one section; nor is one shorter than it is deep.
    assert_eq!(line_of(&prism(&I_SECTION, [0.0; 3], [1.0, 0.0, 0.0], 6000.0, 2.0)), None);
    assert_eq!(line_of(&prism(&I_SECTION, [0.0; 3], [1.0, 0.0, 0.0], 300.0, 1.0)), None);
    assert_eq!(line_of(&[]), None);
}

#[test]
fn a_rafters_line_is_where_its_sides_run_though_its_end_is_cut_plumb_and_level() {
    // A rafter 80 by 300 mm rising 0.777 in 0.629 to the north, 5 m long, its top's middle at the origin; at its far end a plumb
    // cut and a level seat, faces of their own that run along no side.
    let n = (0.629f64 * 0.629 + 0.777 * 0.777).sqrt();
    let axis = [0.0, 0.629 / n, 0.777 / n];
    let rect = [[-40.0, 0.0], [40.0, 0.0], [40.0, -300.0], [-40.0, -300.0]];
    let mut tris = prism(&rect, [0.0; 3], axis, 5000.0, 1.0);
    let up = [0.0, -axis[2], axis[1]];
    let far = |w: f64, u: f64| [-w, 5000.0 * axis[1] + u * up[1], 5000.0 * axis[2] + u * up[2]];
    let (a, b) = (far(-40.0, 0.0), far(40.0, 0.0));
    tris.push([a, b, [a[0], a[1], a[2] - 250.0]]);
    let (c, d) = (far(-40.0, -300.0), far(40.0, -300.0));
    tris.push([c, d, [c[0], c[1] - 200.0, c[2]]]);
    let line = crate::framing::line_of(&tris).unwrap_or_else(|| panic!("no line"));
    let near = |p: [f64; 3], q: [f64; 3]| p.iter().zip(&q).all(|(x, y)| (x - y).abs() < 0.01);
    assert!(near(line.start, [0.0; 3]) && near(line.end, axis.map(|v| v * 5000.0)), "{line:?}");
    assert!((line.width - 80.0).abs() < 0.01 && (line.depth - 300.0).abs() < 0.01, "{line:?}");
}
