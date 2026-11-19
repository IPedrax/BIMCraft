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
    assert_eq!(s.model.images.len(), 1, "the same pixels are one picture");
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
    assert_eq!(t.model.images.values().next().unwrap().file(), &out[..]);
}
