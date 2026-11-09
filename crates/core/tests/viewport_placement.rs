//! Automatic placement is exercised through the commands used by CLI and MCP clients.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Id};
    use bimcraft_core::render::{self, Drawing, Prim};
    use bimcraft_core::sheet;
    use serde_json::json;

    fn project(size: &str) -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points":[[0,0],[1000,0],[1000,1000],[0,1000]],"closed":true}
            crop {"view":"Level 1","region":[[0,0],[1000,1000]]}"#,
        )
        .unwrap();
        s.execute("sheet", &json!({"number":"A101","name":"Placement","size":size})).unwrap();
        s
    }

    fn place(s: &mut Session, name: &str, scale: f64) -> Id {
        s.execute("duplicate_view", &json!({"view":"Level 1","name":name})).unwrap();
        s.execute("place", &json!({"sheet":"A101","view":name,"scale":scale})).unwrap()["id"].as_u64().unwrap()
    }

    // Measure the output, including all lettering/rules, rather than assuming titles are frame-wide.
    fn output_bounds(d: &Drawing, owner: Id) -> [f32; 4] {
        let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for (p, o) in d.prims.iter().zip(&d.owners) {
            if *o != Some(owner) {
                continue;
            }
            let pad = match p {
                Prim::Line { width, .. } => *width / 2.0,
                _ => 0.0,
            };
            for (x, y) in p.points() {
                b[0] = b[0].min(x - pad);
                b[1] = b[1].min(y - pad);
                b[2] = b[2].max(x + pad);
                b[3] = b[3].max(y + pad);
            }
        }
        assert!(b.iter().all(|n| n.is_finite()), "viewport must produce a drawing");
        b
    }

    fn apart(a: [f32; 4], b: [f32; 4]) -> bool {
        a[2] < b[0] || b[2] < a[0] || a[3] < b[1] || b[3] < a[1]
    }

    fn drawing(s: &Session) -> Drawing {
        sheet::draw(&s.model, s.model.sheet_numbered("A101").unwrap()).unwrap()
    }

    fn assert_inside(d: &Drawing, b: [f32; 4]) {
        assert!(
            b[0] > 20.0 * 5.0 && b[1] > 10.0 * 5.0 && b[2] < d.view.width as f32 - 10.0 * 5.0 && b[3] < d.view.height as f32 - 10.0 * 5.0,
            "complete viewport must be inside the sheet border: {b:?}"
        );
    }

    #[test]
    fn long_titles_remain_readable_after_automatic_placement_and_native_reopen() {
        let mut s = project("A1");
        let a = place(&mut s, &"A".repeat(80), 100.0);
        let b = place(&mut s, &"B".repeat(80), 100.0);
        let d = drawing(&s);
        let (a_bounds, b_bounds) = (output_bounds(&d, a), output_bounds(&d, b));
        assert!(apart(a_bounds, b_bounds), "rendered titles must not overprint: {a_bounds:?}, {b_bounds:?}");
        assert_inside(&d, a_bounds);
        assert_inside(&d, b_bounds);

        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("placement.bimcraft");
        let png = dir.path().join("placement.png");
        s.execute("save", &json!({"path":native})).unwrap();
        s.execute("render", &json!({"view":"A101","path":png})).unwrap();
        let before = std::fs::read(&png).unwrap();
        let pixels = tiny_skia::Pixmap::decode_png(&before).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (4205, 2970));
        // Both title rules must also reach the final raster output, not just the display list.
        for id in [a, b] {
            let (x, y) = d
                .prims
                .iter()
                .zip(&d.owners)
                .find_map(|(p, owner)| {
                    if *owner == Some(id)
                        && let Prim::Line { pts, .. } = p
                        && let [(x1, y1), (x2, y2)] = pts.as_slice()
                        && (y1 - y2).abs() < 0.01
                        && (x2 - x1).abs() > 500.0
                    {
                        return Some((((x1 + x2) / 2.0) as u32, *y1 as u32));
                    }
                    None
                })
                .unwrap();
            assert!(
                (y.saturating_sub(3)..=y + 3).any(|y| (x.saturating_sub(3)..=x + 3)
                    // Thin rules can share their coverage between two antialiased rows.
                    .any(|x| { pixels.pixel(x, y).is_some_and(|c| c.red() < 200 && c.green() < 200 && c.blue() < 200) })),
                "a placed title rule must be visible in the PNG"
            );
        }
        let mut reopened = Session::new();
        let opened = reopened.execute("open", &json!({"path":native})).unwrap();
        assert!(opened.get("left_out").is_none(), "native reopen must preserve all fields: {opened}");
        reopened.execute("render", &json!({"view":"A101","path":png})).unwrap();
        assert_eq!(before, std::fs::read(&png).unwrap(), "native reopening must preserve the printed sheet");
    }

    #[test]
    fn a_full_sheet_keeps_later_viewports_separate_instead_of_stacking_them() {
        let mut s = project("A3");
        let ids: Vec<_> = (1..=8).map(|n| place(&mut s, &format!("Plan {n}"), 10.0)).collect();
        let d = drawing(&s);
        for (i, id) in ids.iter().enumerate() {
            let a = output_bounds(&d, *id);
            for other in &ids[..i] {
                let b = output_bounds(&d, *other);
                assert!(apart(a, b), "automatic viewports must stay separate, even after the sheet fills: {a:?}, {b:?}");
            }
        }
        let png = render::png(&d).unwrap();
        assert!(!png.is_empty());
    }

    #[test]
    fn overflowing_views_preserve_the_printed_border_and_title_block() {
        let mut s = project("A3");
        place(&mut s, "Plan 1", 10.0);
        let before = tiny_skia::Pixmap::decode_png(&render::png(&drawing(&s)).unwrap()).unwrap();
        // The same four-plan A3 journey as the public CLI reproduction.
        for n in 2..=4 {
            place(&mut s, &format!("Plan {n}"), 10.0);
        }
        let after = tiny_skia::Pixmap::decode_png(&render::png(&drawing(&s)).unwrap()).unwrap();
        // ISO A3 at 5 px/mm: borders x=20/410, y=10/287; the title block is x=230..410, y=10..50.
        // Include the stroke's antialiased edges in the comparison, with raster y increasing downwards.
        for [left, top, right, bottom] in
            [[98, 48, 103, 1438], [2048, 48, 2053, 1438], [98, 48, 2053, 53], [98, 1433, 2053, 1438], [1148, 1233, 2053, 1438]]
        {
            for y in top..bottom {
                for x in left..right {
                    assert_eq!(after.pixel(x, y), before.pixel(x, y), "overflow must preserve sheet graphics at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn a_first_long_title_fits_and_manual_positions_are_preserved() {
        let mut s = project("A3");
        let first = place(&mut s, &"W".repeat(100), 100.0);
        let d = drawing(&s);
        assert_inside(&d, output_bounds(&d, first));
        s.execute("place", &json!({"sheet":"A101","view":"Level 1","at":[50,60],"scale":100})).unwrap();
        let manual = s
            .model
            .elements
            .values()
            .find_map(|e| match e {
                Element::Viewport(v) if v.at.0 == 50.0 && v.at.1 == 60.0 => Some(v),
                _ => None,
            })
            .unwrap();
        assert_eq!(manual.scale, 100.0);
    }

    #[test]
    fn automatic_views_clear_each_standards_border_and_title_block() {
        for standard in ["nbr", "iso", "ansi", "eu", "jis"] {
            let mut s = project("A3");
            s.execute("standard", &json!({"set":standard,"adapt_all":false})).unwrap();
            let ids: Vec<_> = (1..=6).map(|n| place(&mut s, &format!("Drawing {n}"), 50.0)).collect();
            let id = s.model.sheet_numbered("A101").unwrap();
            let margins = s.model.standard().sheet_margins("A3");
            let d = drawing(&s);
            let title = [
                ((420.0 - margins.right - margins.title_width) * 5.0) as f32,
                ((297.0 - margins.bottom - margins.title_height) * 5.0) as f32,
                ((420.0 - margins.right) * 5.0) as f32,
                ((297.0 - margins.bottom) * 5.0) as f32,
            ];
            assert!(d.owners.contains(&Some(id)), "the sheet must retain its title block");
            for viewport in ids {
                let b = output_bounds(&d, viewport);
                assert!(
                    b[0] > (margins.left * 5.0) as f32
                        && b[1] > (margins.top * 5.0) as f32
                        && b[2] < ((420.0 - margins.right) * 5.0) as f32
                        && b[3] < ((297.0 - margins.bottom) * 5.0) as f32,
                    "{standard}: the viewport must clear the border: {b:?}"
                );
                assert!(apart(b, title), "{standard}: the viewport must clear the title block: {b:?}");
            }
        }
    }

    #[test]
    fn automatic_placement_keeps_schedules_sections_and_3d_views_separate() {
        let mut s = project("A3");
        s.execute("section", &json!({"name":"Section A","start":[-100,500],"end":[1100,500],"depth":2000})).unwrap();
        let mut ids = vec![place(&mut s, "Floor plan", 100.0)];
        for view in ["Room Schedule", "Section A", "{3D}"] {
            ids.push(s.execute("place", &json!({"sheet":"A101","view":view,"scale":100})).unwrap()["id"].as_u64().unwrap());
        }
        let d = drawing(&s);
        for (i, id) in ids.iter().enumerate() {
            let a = output_bounds(&d, *id);
            for other in &ids[..i] {
                assert!(apart(a, output_bounds(&d, *other)), "mixed view types must remain separate");
            }
        }
        assert!(!render::png(&d).unwrap().is_empty());
    }
}
