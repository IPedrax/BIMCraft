use crate::{
    engine::Session,
    geom::P2,
    model::Model,
    render::{self, Prim, Rgb},
};
use serde_json::json;

#[test]
fn free_segments_roundtrip_and_follow_curves_without_moving_the_axis() {
    let mut s = Session::new();
    let segments = json!([
        {"space": 1}, {"offset": {"length": 8, "count": 4, "distance": 0.5}},
        {"wave": {"length": 6, "frequency": 2, "amplitude": 1}},
        {"zigzag": {"length": 6, "frequency": 3, "amplitude": 1}},
        {"text": {"length": 12, "size": 2, "text": "Gas"}}, {"dot_length": 1}, {"dash": 4}
    ]);
    s.execute("line_pattern", &json!({"name":"Utility", "segments":segments})).unwrap();
    s.execute("line_style", &json!({"name":"Utility", "pen":5, "pattern":"Utility"})).unwrap();
    let id = s.execute("detail_line", &json!({"start":[0,0], "end":[4000,0], "style":"Utility"})).unwrap()["id"].as_u64().unwrap();
    let p = &s.model.line_patterns["Utility"];
    let out = p.draw(&[(0.0, 0.0), (500.0, 0.0), (500.0, 500.0)], 5.0, 2.0, Rgb(0, 0, 0));
    assert!(out.iter().any(|p| matches!(p, Prim::Fill { .. })), "text and round dots are filled outlines");
    assert!(out.iter().flat_map(Prim::points).any(|(x, y)| x > 500.0 && y > 0.0), "offsets follow the second leg");
    let (read, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(read.line_patterns, s.model.line_patterns);
    let (a, b) = render::edge_at(&s.model, 100000, id, P2(2000.0, 0.0), 100.0).unwrap();
    assert_eq!((a.1, b.1), (0.0, 0.0), "dimensions and alignment see the centerline");
}

#[test]
fn new_segment_parameters_are_validated_atomically() {
    let mut s = Session::new();
    for segment in [
        json!({"offset":{"length":2,"count":1,"distance":0.5}}),
        json!({"offset":{"length":2,"count":10,"distance":0.5}}),
        json!({"offset":{"length":2,"count":3,"distance":-1}}),
        json!({"wave":{"length":2,"frequency":0,"amplitude":1}}),
        json!({"zigzag":{"length":2,"frequency":2,"amplitude":-1}}),
        json!({"text":{"length":8,"size":0,"text":"Gas"}}),
        json!({"text":{"length":8,"size":2,"text":"a\nb"}}),
        json!({"dot_length":0}),
    ] {
        assert!(s.execute("line_pattern", &json!({"name":"Bad", "segments":[segment]})).is_err());
        assert!(!s.model.line_patterns.contains_key("Bad"));
    }
    s.execute("line_pattern", &json!({"name":"Wave", "segments":[{"wave":{"length":5,"frequency":2,"amplitude":1}}]})).unwrap();
}

#[test]
fn graphic_segments_render_as_pixels_and_keep_their_paper_size() {
    use super::{LinePattern, Segment};
    use crate::render::{Drawing, View};
    let pattern = LinePattern(vec![
        Segment::Space(2.0),
        Segment::Dash(6.0),
        Segment::DotLength(2.0),
        Segment::Offset { length: 8.0, count: 3, distance: 1.0 },
        Segment::Wave { length: 8.0, frequency: 2.0, amplitude: 1.0 },
        Segment::Zigzag { length: 8.0, frequency: 2.0, amplitude: 1.0 },
        Segment::Text { length: 16.0, size: 3.0, text: "Gas 42".into() },
    ]);
    let mut d = Drawing::new(View { angle: 0.0, scale: 1.0, cx: 0.0, cy: 0.0, width: 620, height: 200 });
    d.prims = pattern.draw(&[(10.0, 100.0), (610.0, 100.0)], 10.0, 4.0, Rgb(0, 0, 0));
    let png = render::png(&d).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let black = |x: u32, y: u32| image.pixel(x, y).unwrap().red() < 50;
    assert!(!black(20, 100), "gap is blank");
    assert!(black(40, 100), "straight segment");
    assert!(black(100, 100) && !black(104, 100), "dot is the pen diameter, centered in its segment");
    assert!(black(130, 90) && black(130, 100) && black(130, 110), "three offsets centered on the axis");
    assert!(black(200, 110) && black(220, 90), "wave peaks reach the amplitude");
    assert!(black(280, 110) && black(300, 90), "zigzag peaks reach the amplitude");
    assert!((350..490).any(|x| (82..116).any(|y| black(x, y))), "font outlines produce pixels");
    if let Ok(folder) = std::env::var("BIMCRAFT_PATTERN_PREVIEW") {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(format!("{folder}/segments.png"), png).unwrap();
        d.prims = pattern.draw(
            &(0..=240)
                .map(|i| {
                    let a = i as f32 / 240.0 * std::f32::consts::PI;
                    (310.0 - 260.0 * a.cos(), 165.0 - 130.0 * a.sin())
                })
                .collect::<Vec<_>>(),
            10.0,
            3.0,
            Rgb(0, 0, 0),
        );
        std::fs::write(format!("{folder}/curved.png"), render::png(&d).unwrap()).unwrap();
    }
}

#[test]
fn text_rotates_with_the_curve_and_offset_count_is_bounded_and_symmetric() {
    use super::{LinePattern, Segment};
    let ink = Rgb(0, 0, 0);
    let p = LinePattern(vec![Segment::Text { length: 12.0, size: 2.0, text: "Gas".into() }]);
    let vertical = p.draw(&[(100.0, 0.0), (100.0, 120.0)], 10.0, 2.0, ink);
    let points: Vec<_> = vertical.iter().flat_map(Prim::points).collect();
    let (lo, hi) = points
        .iter()
        .fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), &(x, y)| ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y))));
    assert!(hi.1 - lo.1 > hi.0 - lo.0, "glyphs rotate along a vertical tangent");
    assert!(lo.0 < 100.0 && hi.0 > 100.0, "cap-height midpoint sits on the axis");
    for count in [2, 9] {
        let p = LinePattern(vec![Segment::Offset { length: 10.0, count, distance: 0.5 }]);
        let lines = p.draw(&[(0.0, 100.0), (100.0, 100.0)], 10.0, 2.0, ink);
        assert_eq!(lines.len(), count as usize);
        let ys: Vec<_> = lines.iter().map(|p| p.points()[0].1).collect();
        assert_eq!((ys[0] + ys[ys.len() - 1]) * 0.5, 100.0);
        assert!(ys.windows(2).all(|w| (w[1] - w[0] - 5.0).abs() < 1e-5));
    }
    assert!(LinePattern(vec![Segment::Space(0.1)]).draw(&[(0.0, 0.0), (1e8, 0.0)], 0.01, 2.0, ink).is_empty());
    assert_eq!(p.draw(&[(0.0, 0.0), (1e8, 0.0)], 1.0, 2.0, ink).len(), 1, "extreme density has bounded output");
}

#[test]
fn pattern_dots_and_lettering_take_line_overrides_and_exports_keep_outlines() {
    let mut s = Session::new();
    s.execute(
        "line_pattern",
        &json!({"name":"Gas", "segments":[
            {"dot_length":2},{"text":{"length":12,"size":2,"text":"Gas"}},{"dash":6}
        ]}),
    )
    .unwrap();
    s.execute("line_style", &json!({"name":"Gas", "pen":3,"pattern":"Gas"})).unwrap();
    let id = s.execute("detail_line", &json!({"start":[0,0],"end":[4000,0],"style":"Gas"})).unwrap()["id"].as_u64().unwrap();
    s.execute("graphics", &json!({"view":100000,"elements":{id.to_string():{"projection":{"color":[200,0,0],"weight":12}}}})).unwrap();
    let d = render::draw(&s.model, 100000, render::View { angle: 0.0, scale: 0.2, cx: 2000.0, cy: 0.0, width: 1000, height: 300 });
    let mut dot = None;
    let mut stroke = None;
    for ((prim, owner), pen) in d.prims.iter().zip(&d.owners).zip(&d.pens) {
        if *owner != Some(id) {
            continue;
        }
        match prim {
            Prim::Fill { rings, color } => {
                assert_eq!(*color, Rgb(200, 0, 0), "both text and dots take line colour");
                if pen.is_some() {
                    let (lo, hi) = rings.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
                    dot = Some(hi - lo);
                }
            }
            Prim::Line { width, .. } => stroke = Some(*width),
            _ => {}
        }
    }
    assert!((dot.unwrap() - stroke.unwrap()).abs() < 1e-3, "dot diameter follows the overridden pen");
    assert!(render::svg(&d).contains("#c80000"));
    let pdf = crate::pdf::pdf(&[(&d, P2(250.0, 75.0))]);
    assert!(pdf.starts_with(b"%PDF"));
    assert!(!pdf.windows(5).any(|w| w == b"/Font"), "glyph outlines export without system font dependencies");
}

#[test]
fn offsets_stay_parallel_on_both_sides_of_a_polyline_corner() {
    use super::{LinePattern, Segment};
    let p = LinePattern(vec![Segment::Offset { length: 20.0, count: 2, distance: 1.0 }]);
    let out = p.draw(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)], 10.0, 2.0, Rgb(0, 0, 0));
    assert_eq!(out.len(), 2);
    for prim in out {
        let points = prim.points();
        assert_eq!(points.len(), 3);
        assert!((points[0].1 - points[1].1).abs() < 1e-5, "first side stays horizontal");
        assert!((points[1].0 - points[2].0).abs() < 1e-5, "second side stays vertical");
    }
    let p = LinePattern(vec![Segment::Offset { length: 40.0, count: 2, distance: 1.0 }]);
    let out = p.draw(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0), (0.0, 0.0)], 10.0, 2.0, Rgb(0, 0, 0));
    for prim in out {
        let points = prim.points();
        assert_eq!(points.first(), points.last(), "closed offsets meet at their seam");
    }
}
