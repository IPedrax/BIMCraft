//! MOD-01: two 200 mm walls, drawn east then south, with different cores and matching finishes.
//! Run the fixture with `bimcraft-cli run --script samples/compound-corner.txt --save corner.svg`.
//! Revit 2027.3 (27.3.0.28), Fine plan: in the default Butt order the masonry extends through,
//! giving a 120 mm horizontal seam. Next Butt order puts the concrete through instead, giving a
//! 160 mm vertical seam at x = -60. The command fixture exercises the automatic order only.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::geom::{self, P2};
    use bimcraft_core::model::Id;
    use bimcraft_core::render::{self, Drawing, PAPER, Prim, View};
    use bimcraft_core::walls;

    const CORNER: &str = include_str!("../../../samples/compound-corner.txt");

    fn fixture(script: &str) -> (Session, Id, Id) {
        let mut session = Session::new();
        let results = session.script(script).unwrap();
        let mut walls = results.iter().filter_map(|r| r["ids"].as_array()).flatten().map(|id| id.as_u64().unwrap());
        let (h, v) = (walls.next().unwrap(), walls.next().unwrap());
        assert!(walls.next().is_none());
        (session, h, v)
    }

    fn plan() -> (Drawing, Id, Id) {
        let (session, h, v) = fixture(CORNER);
        // Include both free ends. At this scale the Fine plan draws both core hatches.
        let view = View { scale: 1.0, cx: -1950.0, cy: -1950.0, width: 4300, height: 4300 };
        (render::draw(&session.model, session.model.levels()[0].0, view), h, v)
    }

    // Only the two cores have a cut pattern. Read their filled regions in model millimetres from the
    // paper backgrounds of their two hatches in the plan used for SVG and picking.
    fn core_rings(plan: &Drawing, wall: Id) -> Vec<Vec<P2>> {
        let mut result = Vec::new();
        for (prim, owner) in plan.prims.iter().zip(&plan.owners) {
            if *owner == Some(wall)
                && let Prim::Fill { rings, color: PAPER } = prim
            {
                result.extend(rings.iter().map(|ring| ring.iter().map(|&p| plan.view.mm(p)).collect()));
            }
        }
        assert!(!result.is_empty(), "missing core hatch background for wall {wall}");
        result
    }

    #[test]
    fn free_ends_show_the_two_core_widths() {
        let (plan, h, v) = plan();
        for (id, inside, outside) in [
            (h, P2(-3999.0, 79.0), P2(-3999.0, 81.0)),
            (h, P2(-3999.0, -79.0), P2(-3999.0, -81.0)),
            (v, P2(59.0, -3999.0), P2(61.0, -3999.0)),
            (v, P2(-59.0, -3999.0), P2(-61.0, -3999.0)),
        ] {
            let rings = core_rings(&plan, id);
            assert!(rings.iter().any(|ring| geom::contains(ring, inside)), "missing core at {inside:?}");
            assert!(!rings.iter().any(|ring| geom::contains(ring, outside)), "unexpected core at {outside:?}");
        }
    }

    #[test]
    fn horizontal_masonry_meets_the_return_at_a_120_mm_butt() {
        let (plan, h, v) = plan();
        // These rectangles touch along (-60,-80)..(60,-80). Bound and measure the fill so a
        // shortened wall, a gap, or a diagonal with the right extreme corner cannot pass alone.
        for (id, min, max) in [(h, P2(-4000.0, -80.0), P2(60.0, 80.0)), (v, P2(-60.0, -4000.0), P2(60.0, -80.0))] {
            let rings = core_rings(&plan, id);
            for p in rings.iter().flatten() {
                assert!((min.0..=max.0).contains(&p.0) && (min.1..=max.1).contains(&p.1), "wall {id}: {p:?} outside {min:?}..{max:?}");
            }
            let filled: f64 = rings.iter().map(|r| geom::area(r).abs()).sum();
            let expected = (max.0 - min.0) * (max.1 - min.1);
            assert!((filled - expected).abs() < 0.01, "wall {id}: expected {expected} mm² of core, got {filled}");
        }
    }

    fn segment_count(d: &Drawing, wall: Id, a: P2, b: P2) -> usize {
        let near = |p, q: P2| d.view.mm(p).dist(q) < 0.01;
        d.prims
            .iter()
            .zip(&d.owners)
            .filter_map(|(prim, owner)| match prim {
                Prim::Line { pts, .. } if *owner == Some(wall) => Some(pts),
                _ => None,
            })
            .flat_map(|pts| pts.windows(2))
            .filter(|p| near(p[0], a) && near(p[1], b) || near(p[1], a) && near(p[0], b))
            .count()
    }

    #[test]
    fn only_different_core_materials_draw_a_seam() {
        for (material, seams) in [("Concrete", 1), ("Masonry", 0)] {
            let (s, h, v) = fixture(&CORNER.replace(r#""material":"Concrete""#, &format!(r#""material":"{material}""#)));
            let d = render::draw(&s.model, s.model.levels()[0].0, View { scale: 1.0, cx: 0.0, cy: 0.0, width: 400, height: 400 });
            let count = |a, b| segment_count(&d, h, a, b) + segment_count(&d, v, a, b);
            assert_eq!(count(P2(-60.0, -80.0), P2(60.0, -80.0)), seams);
            assert_eq!(count(P2(100.0, 100.0), P2(60.0, 80.0)), 0);
            assert_eq!(count(P2(-100.0, -100.0), P2(-60.0, -80.0)), 0);
        }
    }

    #[test]
    fn both_bodies_share_the_stepped_cap_after_rigid_transforms() {
        // Non-cardinal rotation, translation, and reversal of handedness. The cut is derived
        // in each wall's frame; the exterior finish can turn either left or right.
        for mirror in [1.0, -1.0] {
            let map = |x: f64, y: f64| P2(3200.0 + x * 0.6 - mirror * y * 0.8, -8100.0 + x * 0.8 + mirror * y * 0.6);
            let (types, _) = CORNER.split_once("wall {").unwrap();
            let mut s = Session::new();
            s.script(types).unwrap();
            let mut place = |name, a, b| {
                s.execute("wall", &serde_json::json!({"points":[a,b], "location":"centre", "type":name, "height":3000})).unwrap()["ids"][0]
                    .as_u64()
                    .unwrap()
            };
            let h = place("H", map(-4000.0, 0.0), map(0.0, 0.0));
            let v = place("V", map(0.0, 0.0), map(0.0, -4000.0));
            for (id, end) in [(h, true), (v, false)] {
                let cap = walls::cap(&s.model, id, end);
                assert_eq!(cap.len(), 5);
                for (x, y) in [(100., 100.), (60., 80.), (60., -80.), (-60., -80.), (-100., -100.)] {
                    assert!(cap.iter().any(|p| p.dist(map(x, y)) < 1e-6), "{cap:?}");
                }
                let prisms = walls::prisms(&s.model, id, true);
                assert_eq!(prisms.len(), 1);
                let p = &prisms[0];
                let outline = walls::outline(&s.model, id);
                assert_eq!(outline, p.ring);
                assert_eq!((p.bottom.at(map(0., 0.)), p.top.at(map(0., 0.))), (0., 3000.));
                for (x, y, owner) in [(40., 0., h), (70., 75., v), (-70., -75., h), (0., -81., v)] {
                    assert_eq!(geom::contains(&p.ring, map(x, y)), id == owner);
                }
            }
        }
    }

    #[test]
    fn type_widths_and_wall_order_determine_the_butt() {
        let wider = CORNER.replace(r#""thickness":120"#, r#""thickness":180"#).replace(r#""thickness":20"#, r#""thickness":30"#);
        let (s, h, _) = fixture(&wider);
        assert_eq!(walls::cap(&s.model, h, true), [P2(130., 110.), P2(90., 80.), P2(90., -80.), P2(-90., -80.), P2(-130., -110.)]);
        let mut lines: Vec<_> = CORNER.lines().collect();
        let n = lines.len();
        lines.swap(n - 1, n - 2);
        let (s, v, h) = fixture(&lines.join("\n"));
        let expected = [P2(100., 100.), P2(60., 80.), P2(-60., 80.), P2(-60., -80.), P2(-100., -100.)];
        assert_eq!(walls::cap(&s.model, h, true), expected);
        assert_eq!(walls::cap(&s.model, v, false), expected.into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn other_junctions_keep_the_ordinary_cap() {
        for script in [
            CORNER.replace("[0,-4000]", "[800,-4000]"),
            CORNER.replacen(r#""height":3000"#, r#""height":2900"#, 1),
            CORNER.replacen(r#""material":"Inner plaster""#, r#""material":"Outer render""#, 1),
            CORNER.replace(r#""function":"structure""#, r#""function":"substrate""#),
        ] {
            let (s, h, v) = fixture(&script);
            assert_eq!(walls::cap(&s.model, h, true).len(), 3);
            assert_eq!(walls::cap(&s.model, v, false).len(), 3);
        }
    }

    #[test]
    fn reversing_the_return_with_its_layers_preserves_the_physical_join() {
        let reversed = CORNER
            .lines()
            .map(|line| {
                if line.contains(r#""name":"V""#) {
                    line.replace("Outer render", "_swap_").replace("Inner plaster", "Outer render").replace("_swap_", "Inner plaster")
                } else {
                    line.replace("[[0,0],[0,-4000]]", "[[0,-4000],[0,0]]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (s, h, v) = fixture(&reversed);
        let cap = walls::cap(&s.model, h, true);
        assert_eq!(cap, [P2(100., 100.), P2(60., 80.), P2(60., -80.), P2(-60., -80.), P2(-100., -100.)]);
        assert_eq!(walls::cap(&s.model, v, true), cap.into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn opening_near_the_join_restores_the_miter_on_both_walls() {
        let (mut s, h, v) = fixture(CORNER);
        s.execute("window", &serde_json::json!({"at":[0,-660]})).unwrap();
        assert_eq!(walls::cap(&s.model, h, true).len(), 3);
        assert_eq!(walls::cap(&s.model, v, false).len(), 3);
        s.execute("undo", &serde_json::json!({})).unwrap();
        assert_eq!(walls::cap(&s.model, h, true).len(), 5);
        assert_eq!(walls::cap(&s.model, v, false).len(), 5);
    }

    #[test]
    fn a_section_along_the_return_cuts_both_cores_at_the_same_station() {
        let (mut s, h, v) = fixture(CORNER);
        let id = s.execute("section", &serde_json::json!({"start":[0,300],"end":[0,-4200],"depth":1})).unwrap()["id"].as_u64().unwrap();
        let d = render::section(&s.model, id, View { scale: 1.0, cx: 2150.0, cy: 1500.0, width: 4500, height: 3100 });
        // The section starts at y=300 and looks along the return: y=-80 is station 380.
        for (id, lo, hi) in [(h, 220., 380.), (v, 380., 4300.)] {
            let rings = core_rings(&d, id);
            let area: f64 = rings.iter().map(|r| geom::area(r).abs()).sum();
            assert!((area - (hi - lo) * 3000.).abs() < 0.01, "owner {id}: {area}");
            assert!(rings.iter().flatten().all(|p| (lo..=hi).contains(&p.0) && (0.0..=3000.).contains(&p.1)));
        }
        assert_eq!(segment_count(&d, v, P2(380., 0.), P2(380., 3000.)), 1);
    }
}
