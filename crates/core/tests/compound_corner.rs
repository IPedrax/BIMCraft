//! MOD-01: two 200 mm walls, drawn east then south, with different cores and matching finishes.
//! Run the fixture with `bimcraft-cli run --script samples/compound-corner.txt --save corner.svg`.
//! Revit 2027.3 (27.3.0.28), Fine plan: in the default Butt order the masonry extends through,
//! giving a 120 mm horizontal seam. Next Butt order puts the concrete through instead, giving a
//! 160 mm vertical seam at x = -60. The command fixture exercises the automatic order only.
//! Here that order is the walls' GUIDs': the lower runs through. The fixture gives the wall
//! placed first the lower one, so H runs through, as above.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::geom::{self, P2};
    use bimcraft_core::model::{Id, Location, Model};
    use bimcraft_core::render::{self, Drawing, PAPER, Prim, View};
    use bimcraft_core::sketch::{self, SketchLine};
    use bimcraft_core::walls;

    const CORNER: &str = include_str!("../../../samples/compound-corner.txt");

    fn fixture(script: &str) -> (Session, Id, Id) {
        let mut session = Session::new();
        let results = session.script(script).unwrap();
        let mut walls = results.iter().filter_map(|r| r["ids"].as_array()).flatten().map(|id| id.as_u64().unwrap());
        let (h, v) = (walls.next().unwrap(), walls.next().unwrap());
        assert!(walls.next().is_none());
        lower_guid_first(&mut session, h, v);
        (session, h, v)
    }

    /// Walls `a` and `b` with each other's GUID, as a project could have drawn them.
    fn swap_guids(s: &mut Session, a: Id, b: Id) {
        let m = s.model_mut();
        let (ga, gb) = (m.guid(a), m.guid(b));
        let (ea, eb) = (m.elements.get(a).cloned().unwrap(), m.elements.get(b).cloned().unwrap());
        let spare = m.elements.fresh();
        m.elements.insert_as(a, spare, ea.clone()).unwrap();
        m.elements.insert_as(b, ga, eb).unwrap();
        m.elements.insert_as(a, gb, ea).unwrap();
    }

    /// Wall `a` given the lower GUID of the two, so it runs through.
    fn lower_guid_first(s: &mut Session, a: Id, b: Id) {
        if s.model.guid(b) < s.model.guid(a) {
            swap_guids(s, a, b);
        }
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
            lower_guid_first(&mut s, h, v);
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
    fn type_widths_and_guid_order_determine_the_butt() {
        let wider = CORNER.replace(r#""thickness":120"#, r#""thickness":180"#).replace(r#""thickness":20"#, r#""thickness":30"#);
        let (s, h, _) = fixture(&wider);
        assert_eq!(walls::cap(&s.model, h, true), [P2(130., 110.), P2(90., 80.), P2(90., -80.), P2(-90., -80.), P2(-130., -110.)]);
        // V placed first, with the lower GUID: the concrete runs through. Neither the order the walls were
        // placed in nor their ids count, only their GUIDs: given V's, H runs through.
        let mut lines: Vec<_> = CORNER.lines().collect();
        let n = lines.len();
        lines.swap(n - 1, n - 2);
        let (mut s, v, h) = fixture(&lines.join("\n"));
        assert!(v < h);
        let concrete = [P2(100., 100.), P2(60., 80.), P2(-60., 80.), P2(-60., -80.), P2(-100., -100.)];
        assert_eq!(walls::cap(&s.model, h, true), concrete);
        assert_eq!(walls::cap(&s.model, v, false), concrete.into_iter().rev().collect::<Vec<_>>());
        swap_guids(&mut s, v, h);
        let masonry = [P2(100., 100.), P2(60., 80.), P2(60., -80.), P2(-60., -80.), P2(-100., -100.)];
        assert_eq!(walls::cap(&s.model, h, true), masonry);
        assert_eq!(walls::cap(&s.model, v, false), masonry.into_iter().rev().collect::<Vec<_>>());
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
        // The same floor corner now uses the opposite named face on V, at its end.
        let horizontal = walls::layer_lines(&s.model, h);
        let reversed = walls::layer_lines(&s.model, v);
        for (hi, vi, turn) in [(1, 2, P2(60., 80.)), (2, 1, P2(-60., -80.))] {
            assert_eq!((horizontal[hi][1], reversed[vi][1]), (turn, turn));
        }
    }

    // A straight reference measured from the chosen face, still ending at the wall's authored stations.
    // The additional offset is signed towards the wall's left, in mm.
    fn reference(m: &Model, id: Id, face: Location, offset: f64) -> (P2, P2) {
        let wall = m.wall(id).unwrap();
        let ty = &m.types.wall[&wall.type_name];
        let from_location = walls::faces(m, wall).0 - walls::depth(ty.thickness, &ty.layers, face);
        let axis = walls::Axis::of(wall);
        (axis.at(0.0, from_location + offset), axis.at(axis.length(), from_location + offset))
    }

    #[test]
    fn sketch_returns_the_picked_wall_face_without_editing_the_model() {
        use serde_json::json;
        let (mut s, h, _) = fixture(CORNER);
        let steps = s.history().len();
        for (face, y) in [("exterior-core", 80.), ("interior-core", -80.)] {
            let reply = s.execute("sketch", &json!({"wall":h,"face":face})).unwrap();
            let line: SketchLine = serde_json::from_value(reply["line"].clone()).unwrap();
            assert_eq!((line.start(), line.end()), (P2(-4000., y), P2(0., y)));
        }
        assert_eq!(s.history().len(), steps);
        s.execute("flip", &json!({"id":h})).unwrap();
        let reply = s.execute("sketch", &json!({"wall":h,"face":"exterior-core"})).unwrap();
        let line: SketchLine = serde_json::from_value(reply["line"].clone()).unwrap();
        assert_eq!((line.start(), line.end()), (P2(0., -80.), P2(-4000., -80.)));
        assert!(s.execute("sketch", &json!({"wall":h,"face":"west"})).is_err());
        assert!(s.execute("sketch", &json!({"wall":h,"lines":[]})).is_err());
        assert!(s.execute("sketch", &json!({"face":"exterior-core"})).is_err());
        let generic = s.execute("wall", &json!({"points":[[0,2000],[4000,2000]]})).unwrap()["ids"][0].clone();
        assert!(s.execute("sketch", &json!({"wall":generic,"face":"interior-core"})).is_err());
        assert!(s.execute("sketch", &json!({"wall":generic})).is_ok());
        let curved = s.execute("wall", &json!({"type":"H","points":[[0,4000],[4000,4000]],"arc":30})).unwrap()["ids"][0].clone();
        assert!(s.execute("sketch", &json!({"wall":curved,"face":"exterior-core"})).is_err());
        let arc: SketchLine = serde_json::from_value(s.execute("sketch", &json!({"wall":curved})).unwrap()["line"].clone()).unwrap();
        assert_eq!(arc.curve.kind(), "arc");
        let at_finish =
            s.execute("wall", &json!({"type":"H","points":[[0,8000],[4000,8000]],"location":"exterior-finish"})).unwrap()["ids"][0].clone();
        let reply = s.execute("sketch", &json!({"wall":at_finish,"face":"exterior-core"})).unwrap();
        let line: SketchLine = serde_json::from_value(reply["line"].clone()).unwrap();
        assert_eq!((line.start(), line.end()), (P2(0., 7980.), P2(4000., 7980.)));
    }

    #[test]
    fn floor_core_turns_on_the_compound_l() {
        // Change only a finish material to make the same geometry take the ordinary cap.
        let ordinary = CORNER.replacen(r#""material":"Inner plaster""#, r#""material":"Outer render""#, 1);
        for (script, butt) in [(CORNER, true), (ordinary.as_str(), false)] {
            let (s, h, v) = fixture(script);
            let m = &s.model;
            assert_eq!(walls::cap(m, h, true).len(), if butt { 5 } else { 3 });
            for (face, layer, sign) in [(Location::ExteriorCore, 1, 1.), (Location::InteriorCore, 2, -1.)] {
                let turn = P2(sign * 60., sign * 80.);
                let h_end = walls::layer_lines(m, h)[layer][1];
                let v_start = walls::layer_lines(m, v)[layer][0];
                if butt {
                    assert_eq!((h_end, v_start), (turn, turn));
                } else {
                    assert_eq!((h_end, v_start), (P2(sign * 80., sign * 80.), P2(sign * 60., sign * 60.)));
                }
                for offset in [0., sign * 25.] {
                    let (a, b) = reference(m, h, face, offset);
                    let (c, d) = reference(m, v, face, offset);
                    assert_eq!((b, c), (P2(0., turn.1 + offset), P2(turn.0 + offset, 0.)));
                    let corner = geom::line_intersection(a, b, c, d).unwrap();
                    assert!(corner.dist(turn + P2(offset, offset)) < 1e-6, "{corner:?}");
                }
            }
        }
    }

    #[test]
    fn floor_core_faces_close_the_asymmetric_rectangle() {
        let mut s = Session::new();
        let result = s.script(include_str!("../../../samples/floor-pick-walls.txt")).unwrap();
        let ids = result.last().unwrap()["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 4);
        // Clockwise axes: exterior is left. The 30/160/10 stack puts the core at +70 and -90 mm.
        for (face, layer, inset, area_m2) in [(Location::ExteriorCore, 1, -70., 49.98), (Location::InteriorCore, 2, 90., 45.51)] {
            let corners = [P2(inset, inset), P2(inset, 6000. - inset), P2(8000. - inset, 6000. - inset), P2(8000. - inset, inset)];
            let mut lines = Vec::new();
            let mut untrimmed = Vec::new();
            for (i, id) in ids.iter().enumerate() {
                let id = id.as_u64().unwrap();
                let (a, b) = reference(&s.model, id, face, 0.);
                untrimmed.push(SketchLine::new(a, b));
                let joined = &walls::layer_lines(&s.model, id)[layer];
                assert_eq!(joined, &[corners[i], corners[(i + 1) % 4]]);
                lines.push(SketchLine::new(joined[0], joined[1]));
            }
            assert!(sketch::solve_one(&untrimmed).is_err());
            let region = sketch::solve_one(&lines).unwrap();
            assert_eq!(region.area(), (8000. - 2. * inset) * (6000. - 2. * inset));
            let floor = s.execute("floor", &serde_json::json!({ "sketch": lines })).unwrap();
            assert_eq!(floor["area_m2"], area_m2);
        }
        // Flipping just the west wall turns its asymmetric stack around in the model.
        let west = ids[0].as_u64().unwrap();
        s.execute("flip", &serde_json::json!({ "id": west })).unwrap();
        assert_eq!(reference(&s.model, west, Location::ExteriorCore, 0.), (P2(70., 6000.), P2(70., 0.)));
        assert_eq!(reference(&s.model, west, Location::InteriorCore, 0.), (P2(-90., 6000.), P2(-90., 0.)));
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
    fn an_inserted_corner_keeps_the_core_that_ran_through() {
        // Insert from File gives an element whose id this project has the next one free and leaves its GUID as it was.
        // Here H's id is taken and V's is free, so the corner comes in with its ids the other way round.
        for swap in [false, true] {
            let (mut s, h, v) = fixture(CORNER);
            if swap {
                swap_guids(&mut s, h, v);
            }
            let caps = |s: &Session, h, v| (walls::cap(&s.model, h, true), walls::cap(&s.model, v, false));
            let before = caps(&s, h, v);
            assert_eq!(before.0.len(), 5);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("corner.bimcraft");
            s.execute("save", &serde_json::json!({"path": path})).unwrap();
            let mut t = Session::new();
            let mut far =
                |y: i32| t.execute("wall", &serde_json::json!({"points": [[0, y], [4000, y]]})).unwrap()["ids"][0].as_u64().unwrap();
            let (a, b) = (far(20000), far(30000));
            assert_eq!((a, b), (h, v));
            t.execute("delete", &serde_json::json!({"ids": [b]})).unwrap();
            let ids = t.execute("insert", &serde_json::json!({"path": path})).unwrap()["ids"].clone();
            let here = |id: Id| ids[id.to_string()].as_u64().unwrap();
            let (h2, v2) = (here(h), here(v));
            assert!(v2 < h2, "V came in as {v2}, H as {h2}");
            assert_eq!((t.model.guid(h2), t.model.guid(v2)), (s.model.guid(h), s.model.guid(v)));
            assert_eq!(caps(&t, h2, v2), before, "the other core runs through once H is {h2} and V {v2}");
        }
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
