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

    const CORNER: &str = include_str!("../../../samples/compound-corner.txt");

    fn plan() -> (Drawing, Id, Id) {
        let mut session = Session::new();
        let results = session.script(CORNER).unwrap();
        let mut walls = results.iter().filter_map(|r| r["ids"].as_array()).flatten().map(|id| id.as_u64().unwrap());
        let (h, v) = (walls.next().unwrap(), walls.next().unwrap());
        assert!(walls.next().is_none());
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
    #[ignore = "MOD-01: wall layers still end at the envelope miter; run explicitly to compare with the measured Butt order"]
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
}
