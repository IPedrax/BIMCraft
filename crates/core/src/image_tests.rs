//! An image placed in a view, as Revit's Insert > Image and Link Image: a PNG or a JPEG drawn under the model in the plans of
//! its level, its pixels where it stands, listed by Manage Links and the Asset Manager, and read again when it's linked.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::render::{self, Rgb, View};

const LEVEL: u64 = 100000;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A folder of this test's own, empty.
fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-image-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A PNG of `w` by `h` px, its left half `left` and its right half `right`, at `path`.
fn picture(path: &std::path::Path, w: u32, h: u32, left: [u8; 3], right: [u8; 3]) {
    let rgba: Vec<u8> = (0..h)
        .flat_map(|_| {
            (0..w).flat_map(move |x| {
                let [r, g, b] = if x < w / 2 { left } else { right };
                [r, g, b, 255]
            })
        })
        .collect();
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
    std::fs::write(path, png).unwrap();
}

/// The colour the plan of Level 1 draws at `p`, in a view 8 m across about it.
fn colour_at(s: &Session, p: P2) -> Rgb {
    let view = View { angle: 0.0, scale: 0.1, cx: p.0, cy: p.1, width: 800, height: 800 };
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, LEVEL, view)).unwrap()).unwrap();
    let (x, y) = view.px(p);
    let c = pm.pixel(x as u32, y as u32).unwrap();
    Rgb(c.red(), c.green(), c.blue())
}

const RED: [u8; 3] = [220, 20, 20];
const BLUE: [u8; 3] = [20, 20, 220];
const GREEN: [u8; 3] = [20, 200, 20];

#[test]
fn an_image_is_drawn_in_its_levels_plan_where_it_stands_at_its_width() {
    let dir = dir("drawn");
    let path = dir.join("site.png");
    picture(&path, 40, 20, RED, BLUE);
    let mut s = Session::new();
    let v = run(&mut s, "image", json!({"path": path.to_string_lossy(), "at": [0, 0], "width": 4000}));
    assert_eq!((v["size"].clone(), v["height"].clone()), (json!([40, 20]), json!(2000.0)), "{v}");
    // 4 m across, 2 m up about its middle: red on the left, blue on the right.
    assert_eq!(colour_at(&s, P2(-1000.0, 0.0)), Rgb(RED[0], RED[1], RED[2]));
    assert_eq!(colour_at(&s, P2(1000.0, 500.0)), Rgb(BLUE[0], BLUE[1], BLUE[2]));
    assert_eq!(colour_at(&s, P2(3000.0, 0.0)), Rgb(255, 255, 255), "nothing past its edge");
    assert_eq!(colour_at(&s, P2(0.0, 1500.0)), Rgb(255, 255, 255), "nor over its top");
    // It moves as any element does.
    let id = v["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [10_000, 0]}));
    assert_eq!(colour_at(&s, P2(-1000.0, 0.0)), Rgb(255, 255, 255));
    // Saved and opened again, it is there with its picture.
    let file = dir.join("p.bimc").to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    assert_eq!(colour_at(&t, P2(9000.0, 0.0)), Rgb(RED[0], RED[1], RED[2]));
}

#[test]
fn a_linked_image_is_listed_and_reloaded_and_an_imported_one_is_not() {
    let dir = dir("linked");
    let path = dir.join("logo.png");
    picture(&path, 10, 10, RED, RED);
    let mut s = Session::new();
    let a = run(&mut s, "image", json!({"path": path.to_string_lossy(), "width": 1000, "link": true}))["id"].as_u64().unwrap();
    let b =
        run(&mut s, "image", json!({"path": path.to_string_lossy(), "width": 1000, "at": [5000, 0], "link": true}))["id"].as_u64().unwrap();
    assert_eq!(s.model.images.len(), Session::new().model.images.len() + 1, "the same pixels are one picture");
    let rows = run(&mut s, "links", json!({"kind": "images"}))["images"].clone();
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    let ids: Vec<u64> = rows[0]["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!((rows[0]["status"].clone(), ids), (json!("loaded"), vec![a, b]), "{rows}");
    assert_eq!(rows[0]["name"], "logo.png");
    // The file changes; Reload reads it for both.
    picture(&path, 10, 10, GREEN, GREEN);
    let v = run(&mut s, "reload_image", json!({"id": a}));
    assert_eq!(v["images"], json!([a, b]), "{v}");
    assert_eq!(colour_at(&s, P2(5000.0, 0.0)), Rgb(GREEN[0], GREEN[1], GREEN[2]));
    // Reload From another file.
    let other = dir.join("logo2.png");
    picture(&other, 10, 10, BLUE, BLUE);
    run(&mut s, "reload_image", json!({"id": b, "path": other.to_string_lossy()}));
    assert_eq!(colour_at(&s, P2(0.0, 0.0)), Rgb(BLUE[0], BLUE[1], BLUE[2]));
    // The Asset Manager says where the picture is placed.
    let pictures = run(&mut s, "assets", json!({"kind": "pictures"}))["pictures"].clone();
    let used = pictures.as_array().unwrap().iter().find(|p| p["uses"] == 2).unwrap_or_else(|| panic!("{pictures}")).clone();
    assert_eq!(used["used_by"][0], json!({"kind": "element", "id": a, "category": "image"}));
    // One brought in without `link` is imported: there is no file to read again.
    let c = run(&mut s, "image", json!({"path": dir.join("logo.png").to_string_lossy(), "width": 1000}))["id"].as_u64().unwrap();
    let rows = run(&mut s, "links", json!({"kind": "images"}))["images"].clone();
    let row = rows.as_array().unwrap().iter().find(|r| r["ids"] == json!([c])).unwrap().clone();
    assert_eq!((row["status"].clone(), row["path"].clone()), (json!("imported"), Value::Null), "{rows}");
    let e = s.execute("reload_image", &json!({"id": c})).unwrap_err();
    assert!(e.contains("isn't linked"), "{e}");
}

#[test]
fn an_image_in_one_view_only_and_hostile_ones() {
    let dir = dir("view");
    let path = dir.join("a.png");
    picture(&path, 4, 4, RED, RED);
    let mut s = Session::new();
    let plan = run(&mut s, "duplicate_view", json!({"view": "Level 1"}))["id"].as_u64().unwrap();
    run(&mut s, "image", json!({"path": path.to_string_lossy(), "width": 2000, "view": plan}));
    assert_eq!(colour_at(&s, P2(0.0, 0.0)), Rgb(255, 255, 255), "not in the level's own plan");
    for bad in [
        json!({"path": path.to_string_lossy(), "width": -1}),
        json!({"path": path.to_string_lossy(), "width": f64::MAX}),
        json!({"path": path.to_string_lossy(), "at": [1e300, 0]}),
        json!({"path": dir.join("missing.png").to_string_lossy()}),
        json!({"picture": "img-nothing"}),
        json!({}),
    ] {
        let before = s.model.clone();
        assert!(s.execute("image", &bad).is_err(), "{bad}");
        assert_eq!(*s.model, *before);
    }
}

#[test]
fn a_pdfs_page_is_placed_at_its_papers_size_at_the_plans_scale_and_listed_on_the_pdf_tab() {
    let dir = dir("pdf");
    // A sheet of this project, written as a PDF: a page with a title block's lines on it.
    let mut a = Session::new();
    let sheet = run(&mut a, "sheet", json!({}))["id"].as_u64().unwrap();
    let file = dir.join("plans.pdf");
    run(&mut a, "export", json!({"format": "pdf", "view": sheet, "path": file}));
    let mut s = Session::new();
    let v = run(&mut s, "image", json!({"path": file.to_string_lossy(), "link": true}));
    let id = v["id"].as_u64().unwrap();
    let Some(crate::model::Element::Image(im)) = s.model.elements.get(id) else { panic!("{v}") };
    assert_eq!(im.page, Some(1));
    // Its paper at 1 : 100, the plan's scale.
    let paper = crate::graphics::scale_of(&s.model, LEVEL);
    let (w, h) = (v["width"].as_f64().unwrap(), v["height"].as_f64().unwrap());
    assert!(w > 100.0 * paper && w < 1300.0 * paper, "a sheet's width at the plan's scale: {v}");
    let size: Vec<u64> = v["size"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect();
    assert!(size[0].max(size[1]) <= crate::texture::MAX_PLACED_SIDE as u64 && size[0] > 1000, "drawn fine: {v}");
    assert!((w / h - size[0] as f64 / size[1] as f64).abs() < 0.01);
    // Something of the page is drawn, not all white.
    let view = View { angle: 0.0, scale: 800.0 / w, cx: 0.0, cy: 0.0, width: 800, height: 800 };
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&render::draw(&s.model, LEVEL, view)).unwrap()).unwrap();
    assert!(pm.pixels().iter().any(|p| p.red() < 128), "the title block's lines");
    // The PDF tab lists it, not the Images tab, and Reload draws its page again.
    let links = run(&mut s, "links", json!({}));
    assert_eq!((links["pdf"].as_array().unwrap().len(), links["images"].as_array().unwrap().len()), (1, 0), "{links}");
    assert_eq!(links["pdf"][0]["status"], "loaded");
    run(&mut s, "reload_image", json!({"id": id}));
    // Not a PDF, or a page it hasn't, is refused.
    let fake = dir.join("fake.pdf");
    std::fs::write(&fake, b"%PDF-1.7 nothing").unwrap();
    assert!(s.execute("image", &json!({"path": fake.to_string_lossy()})).is_err());
    let e = s.execute("image", &json!({"path": file.to_string_lossy(), "page": 9})).unwrap_err();
    assert!(e.contains("1 pages, not a page 9"), "{e}");
    assert!(s.execute("image", &json!({"path": file.to_string_lossy(), "page": 0})).is_err());
}

#[test]
fn a_jpeg_is_kept_as_the_jpeg_it_came_in_as() {
    let dir = dir("jpeg");
    let mut out = Vec::new();
    let rgb: Vec<u8> = (0..64 * 32).flat_map(|_| [200u8, 40, 40]).collect();
    jpeg_encoder::Encoder::new(&mut out, 80).encode(&rgb, 64, 32, jpeg_encoder::ColorType::Rgb).unwrap();
    let path = dir.join("photo.jpg");
    std::fs::write(&path, &out).unwrap();
    let mut s = Session::new();
    let v = run(&mut s, "image", json!({"path": path.to_string_lossy()}));
    let picture = &s.model.images[v["picture"].as_str().unwrap()];
    assert!(picture.is_jpeg());
    assert_eq!(picture.file(), &out[..], "byte for byte");
    // Through a .bima and back, still the same JPEG.
    let bima = dir.join("a.bima").to_string_lossy().into_owned();
    run(&mut s, "export_assets", json!({"path": bima, "pictures": true}));
    let mut t = Session::new();
    run(&mut t, "import_assets", json!({"path": bima}));
    assert_eq!(t.model.images[v["picture"].as_str().unwrap()].file(), &out[..]);
}

/// A two-sheet PDF of this project's, in `dir`: its path.
fn two_sheets(dir: &std::path::Path) -> std::path::PathBuf {
    let mut a = Session::new();
    let one = run(&mut a, "sheet", json!({}))["id"].as_u64().unwrap();
    let two = run(&mut a, "sheet", json!({}))["id"].as_u64().unwrap();
    let file = dir.join("set.pdf");
    run(&mut a, "export", json!({"format": "pdf", "views": [one, two], "path": file}));
    file
}

#[test]
fn a_pdf_is_drawn_as_its_lines_not_a_picture_and_its_pages_are_listed() {
    let dir = dir("vector");
    let mut a = Session::new();
    let sheet = run(&mut a, "sheet", json!({}))["id"].as_u64().unwrap();
    let file = dir.join("one.pdf");
    run(&mut a, "export", json!({"format": "pdf", "view": sheet, "path": file}));
    let mut s = Session::new();
    let v = run(&mut s, "pdf_pages", json!({"path": file.to_string_lossy()}));
    assert_eq!(v["pages"].as_array().unwrap().len(), 1, "{v}");
    let id = run(&mut s, "image", json!({"path": file.to_string_lossy()}))["id"].as_u64().unwrap();
    let Some(crate::model::Element::Image(im)) = s.model.elements.get(id).cloned() else { panic!() };
    let items = s.model.pdfs[&im.picture].drawing().items.len();
    assert!(items > 4, "the title block's lines, as lines: {items}");
    // In the plan, lines and fills of its own, not one picture.
    let view = View { angle: 0.0, scale: 800.0 / im.width, cx: 0.0, cy: 0.0, width: 800, height: 800 };
    let d = render::draw(&s.model, LEVEL, view);
    let lines = d.prims.iter().filter(|p| matches!(p, render::Prim::Line { .. })).count();
    assert!(lines > 4, "{lines} lines");
    assert!(!d.prims.iter().any(|p| matches!(p, render::Prim::Image { .. })), "no picture of it");
    // Saved and opened, it is still drawn as lines.
    let path = dir.join("p.bimc").to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(t.model.pdfs[&im.picture].drawing().items.len(), items);
}

#[test]
fn a_pdfs_pages_are_counted_and_any_of_them_placed() {
    let dir = dir("pages");
    let file = two_sheets(&dir);
    let mut s = Session::new();
    let v = run(&mut s, "pdf_pages", json!({"path": file.to_string_lossy()}));
    assert_eq!(v["pages"].as_array().unwrap().len(), 2, "{v}");
    let id = run(&mut s, "image", json!({"path": file.to_string_lossy(), "page": 2}))["id"].as_u64().unwrap();
    let Some(crate::model::Element::Image(im)) = s.model.elements.get(id) else { panic!() };
    assert_eq!((im.page, s.model.pdfs[&im.picture].page()), (Some(2), 2));
}

#[test]
fn an_image_drawn_in_front_covers_the_model_and_one_behind_does_not() {
    let dir = dir("front");
    let path = dir.join("a.png");
    picture(&path, 4, 4, RED, RED);
    let mut s = Session::new();
    run(&mut s, "floor", json!({"boundary": [[-3000, -3000], [3000, -3000], [3000, 3000], [-3000, 3000]]}));
    run(&mut s, "wall", json!({"points": [[-2000, 0], [2000, 0]], "thickness": 400}));
    let id = run(&mut s, "image", json!({"path": path.to_string_lossy(), "width": 2000}))["id"].as_u64().unwrap();
    let wall = colour_at(&s, P2(0.0, 0.0));
    assert_ne!(wall, Rgb(RED[0], RED[1], RED[2]), "behind, the wall's poché covers it");
    run(&mut s, "modify", json!({"id": id, "front": true}));
    assert_eq!(colour_at(&s, P2(0.0, 0.0)), Rgb(RED[0], RED[1], RED[2]), "in front, it covers the wall");
}

#[test]
fn an_image_mirrors_about_an_upright_or_level_line_and_resizes_by_its_width() {
    let dir = dir("mirror");
    let path = dir.join("a.png");
    picture(&path, 40, 20, RED, BLUE);
    let mut s = Session::new();
    let id = run(&mut s, "image", json!({"path": path.to_string_lossy(), "width": 4000}))["id"].as_u64().unwrap();
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, -1000], [0, 1000]]}));
    assert_eq!(colour_at(&s, P2(-1000.0, 0.0)), Rgb(BLUE[0], BLUE[1], BLUE[2]), "flipped left to right");
    assert!(s.execute("mirror", &json!({"ids": [id], "axis": [[0, 0], [1000, 1000]]})).is_err(), "not about a slant");
    run(&mut s, "modify", json!({"id": id, "width": 8000}));
    assert_eq!(colour_at(&s, P2(3000.0, 0.0)), Rgb(RED[0], RED[1], RED[2]), "twice as wide, and still flipped");
    assert!(s.execute("modify", &json!({"id": id, "width": -5})).is_err());
}

#[test]
fn an_images_height_and_a_pdfs_scale_set_its_width() {
    let dir = dir("scale");
    let mut a = Session::new();
    let sheet = run(&mut a, "sheet", json!({}))["id"].as_u64().unwrap();
    let file = dir.join("one.pdf");
    run(&mut a, "export", json!({"format": "pdf", "view": sheet, "path": file}));
    let mut s = Session::new();
    let id = run(&mut s, "image", json!({"path": file.to_string_lossy()}))["id"].as_u64().unwrap();
    let shown = run(&mut s, "elements", json!({"category": "image"}))["elements"][0].clone();
    assert!(shown["paper"].as_str().is_some_and(|p| p.ends_with(" mm")), "{shown}");
    let paper = |s: &Session| {
        let Some(crate::model::Element::Image(im)) = s.model.elements.get(id) else { panic!() };
        (im.width, im.paper(&s.model).unwrap()[0])
    };
    run(&mut s, "modify", json!({"id": id, "scale": 50}));
    let (w, p) = paper(&s);
    assert!((w - 50.0 * p).abs() < 1e-6, "1 : 50");
    run(&mut s, "modify", json!({"id": id, "height": 1000}));
    let Some(crate::model::Element::Image(im)) = s.model.elements.get(id) else { panic!() };
    assert!((im.height(&s.model).unwrap() - 1000.0).abs() < 1e-6);
    assert!(s.execute("modify", &json!({"id": id, "scale": -1})).is_err());
}

/// A one-page PDF, 600 by 800 points, drawing `content` with fonts F1 (Verdana) and F2 (Helvetica), neither held in it.
fn hand_made_pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Verdana >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        at.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for a in at {
        pdf.extend(format!("{a:010} 00000 n \n").bytes());
    }
    pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    pdf
}

#[test]
fn a_pdfs_frame_filled_even_odd_keeps_its_opening_and_a_font_it_doesnt_hold_is_named() {
    let dir = dir("holes");
    let file = dir.join("frame.pdf");
    // Two squares wound alike, filled even-odd: a frame. Text in Verdana, which isn't held and isn't a standard family, and in
    // Helvetica, which is.
    let content = "0 0 0 rg 100 100 400 400 re 200 200 200 200 re f* BT /F1 24 Tf 100 600 Td (Hi) Tj ET BT /F2 24 Tf 100 650 Td (Hi) Tj ET";
    std::fs::write(&file, hand_made_pdf(content)).unwrap();
    let mut s = Session::new();
    // 600 points across as 6 m: 10 mm a point, the page's top left at (-3000, 4000).
    let v = run(&mut s, "image", json!({"path": file.to_string_lossy(), "width": 6000}));
    assert_eq!(v["missing_fonts"], json!(["Verdana"]), "{v}");
    let id = v["id"].as_u64().unwrap();
    let e = run(&mut s, "elements", json!({"category": "image"}));
    let row = e["elements"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).cloned().unwrap();
    assert_eq!(row["missing_fonts"], json!("Verdana"), "{row}");
    // The opening, (300, 300) on the page, is the paper's; the frame round it, (150, 300), is ink.
    let white = colour_at(&s, P2(0.0, -1000.0));
    assert!(white.0 > 240 && white.1 > 240 && white.2 > 240, "the opening stays open: {white:?}");
    let ink = colour_at(&s, P2(-1500.0, -1000.0));
    assert!(ink.0 < 60 && ink.1 < 60 && ink.2 < 60, "the frame is filled: {ink:?}");
    // In the plan, the frame is one fill of two rings, what the window fills by the non-zero rule.
    let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 800 };
    let d = render::draw(&s.model, LEVEL, view);
    let mine: Vec<_> = d.prims.iter().zip(&d.owners).zip(&d.parts).filter(|((_, o), _)| **o == Some(id)).collect();
    assert!(!mine.is_empty() && mine.iter().all(|(_, part)| **part == render::Part::Picture), "a picture's own part");
    assert!(mine.iter().any(|((p, _), _)| matches!(p, render::Prim::Fill { rings, .. } if rings.len() == 2)), "the frame");
}

#[test]
fn rings_filled_even_odd_are_wound_for_the_non_zero_rule() {
    let square = |a: f32, b: f32| vec![[a, a], [b, a], [b, b], [a, b]];
    let area = |r: &[[f32; 2]]| (0..r.len()).map(|i| r[i][0] * r[(i + 1) % r.len()][1] - r[(i + 1) % r.len()][0] * r[i][1]).sum::<f32>();
    // A square, a square inside it and one inside that, all wound alike.
    let mut rings = vec![square(0.0, 30.0), square(5.0, 25.0), square(10.0, 20.0)];
    crate::pdf_in::wind_even_odd(&mut rings);
    let signs: Vec<bool> = rings.iter().map(|r| area(r) > 0.0).collect();
    assert_eq!(signs, [true, false, true], "out, opening, island");
}

#[test]
fn a_fonts_family_is_read_from_its_postscript_name_and_found_on_the_computer() {
    use crate::pdf_in::{family_of, system_font};
    use hayro::hayro_interpret::font::{FallbackFontQuery, FontQuery, StandardFont};
    assert_eq!(family_of("ArialMT"), "Arial");
    assert_eq!(family_of("ABCDEF+TimesNewRomanPS-BoldMT"), "Times New Roman");
    assert_eq!(family_of("Calibri,Bold"), "Calibri");
    assert_eq!(family_of("DejaVuSans-Oblique"), "Deja Vu Sans");
    // A computer with IBM Plex Mono alone: a CID font named for it is drawn in it, one named for a font it hasn't isn't, and
    // nor is Courier, whose stand-ins it isn't.
    let dir = dir("fonts");
    std::fs::write(dir.join("Mono.ttf"), crate::text::PLEX_MONO).unwrap();
    let index = crate::text::system::scan(&[dir]);
    let asked = |name: &str| FontQuery::Fallback(FallbackFontQuery { post_script_name: Some(name.into()), ..Default::default() });
    assert!(system_font(&index, &asked("IBMPlexMono-Bold")).is_some(), "found by its name, spaced or not");
    assert!(system_font(&index, &asked("NoSuchFont")).is_none());
    assert!(system_font(&index, &FontQuery::Standard(StandardFont::Courier)).is_none());
}

#[test]
fn a_pdf_that_snaps_is_snapped_to_at_its_lines_ends_and_middles_but_not_its_letters() {
    use crate::snap::{self, Kind, Reach, Snaps};
    let dir = dir("snaps");
    let file = dir.join("line.pdf");
    // A line across the page, 700 points up, and a word in Helvetica under it.
    std::fs::write(&file, hand_made_pdf("0 0 0 RG 1 w 100 700 m 400 700 l S BT /F2 24 Tf 100 600 Td (Hi) Tj ET")).unwrap();
    let mut s = Session::new();
    // 10 mm a point, the page's top left at (-3000, 4000): the line runs from (-2000, 3000) to (1000, 3000).
    let id = run(&mut s, "image", json!({"path": file.to_string_lossy(), "width": 6000}))["id"].as_u64().unwrap();
    let reach = Reach { point: 400.0, line: 120.0 };
    let at = |s: &Session, q: P2| snap::snap(&s.model, LEVEL, q, reach, &[], false, &Snaps::default());
    assert_ne!(at(&s, P2(-1950.0, 2960.0)).source, Some(id), "it doesn't snap until asked");
    run(&mut s, "modify", json!({"id": id, "snaps": true}));
    let end = at(&s, P2(-1950.0, 2960.0));
    assert_eq!((end.at, end.kind, end.source), (P2(-2000.0, 3000.0), Kind::Endpoint, Some(id)), "{end:?}");
    let middle = at(&s, P2(-480.0, 2970.0));
    assert_eq!((middle.at, middle.kind), (P2(-500.0, 3000.0), Kind::Midpoint), "{middle:?}");
    // The word's letters, 2000 to 2170 up and 900 below the line: nothing of them is snapped to.
    assert_ne!(at(&s, P2(-1990.0, 2100.0)).source, Some(id), "a letter isn't snapped to");
    // Placed snapping from the start.
    let two = run(&mut s, "image", json!({"path": file.to_string_lossy(), "width": 6000, "at": [20000, 0], "snaps": true}));
    let Some(crate::model::Element::Image(im)) = s.model.elements.get(two["id"].as_u64().unwrap()) else { panic!() };
    assert!(im.snaps);
}

#[test]
fn a_pdfs_pages_are_drawn_small_one_by_one_until_asked_to_stop() {
    let dir = dir("thumbs");
    let bytes = std::fs::read(two_sheets(&dir)).unwrap();
    let mut got = Vec::new();
    crate::pdf_in::thumbnails(bytes.clone(), 64, |n, w, h, rgba| {
        got.push((n, w.max(h), rgba.len() == (w * h * 4) as usize));
        true
    })
    .unwrap();
    assert_eq!(got, [(1, 64, true), (2, 64, true)], "each page, 64 px on its longer side");
    let mut first = 0;
    crate::pdf_in::thumbnails(bytes, 64, |_, _, _, _| {
        first += 1;
        false
    })
    .unwrap();
    assert_eq!(first, 1, "stopped after the first");
    assert!(crate::pdf_in::thumbnails(b"not a pdf".to_vec(), 64, |_, _, _, _| true).is_err());
}
