//! Structural framing and foundations from their records' boxes, and the categories that come in as boxes besides: stairs' runs,
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
