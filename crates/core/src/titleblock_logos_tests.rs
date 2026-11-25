//! Logos in title blocks: a PNG or a JPEG of the project's, placed in a title block and printed with it.

use std::path::Path;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::{Prim, png};
use crate::sheet;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn sheet_id(s: &Session, number: &str) -> Id {
    s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Sheet(sh) if sh.number == number).then_some(*id)).unwrap()
}

/// A PNG `w` by `h` px of one colour, written to `dir` as `name`.
fn write_png(dir: &Path, name: &str, (w, h): (u32, u32), rgb: [u8; 3]) -> String {
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255));
    let path = dir.join(name);
    std::fs::write(&path, pm.encode_png().unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}

fn colour_at(png: &[u8], (x, y): (f32, f32)) -> [u8; 3] {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    [c.red(), c.green(), c.blue()]
}

/// A title block 100 by 30 mm with one logo, a red picture twice as wide as it is tall, 20 mm wide at (2, 2).
fn with_logo(s: &mut Session, dir: &Path) -> Value {
    let path = write_png(dir, "logo.png", (40, 20), [255, 0, 0]);
    run(
        s,
        "title_block",
        json!({
            "name": "ESCRITORIO", "width": 100, "height": 30,
            "lines": [{"points": [[0, 0], [100, 0], [100, 30], [0, 30]], "closed": true, "pen": 8}],
            "logos": [{"path": path, "at": [2, 2], "width": 20}]
        }),
    )
}

/// The pictures a new project holds: the starter materials' (#536).
fn starter() -> usize {
    Session::new().model.images.len()
}

#[test]
fn a_logo_is_read_from_a_png_kept_in_the_project_and_printed_where_it_is_placed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    let made = with_logo(&mut s, dir.path());
    let logo = &made["title_block"]["logos"][0];
    assert_eq!(
        (logo["at"].clone(), logo["width"].as_f64(), logo["height"].as_f64()),
        (json!([2.0, 2.0]), Some(20.0), Some(10.0)),
        "{made}"
    );
    assert_eq!(s.model.images.len(), starter() + 1, "its picture is the project's");
    assert!(logo["image"].as_str().is_some_and(|k| s.model.images.contains_key(k)));
    run(&mut s, "sheet", json!({"number": "A101", "title_block": "ESCRITORIO"}));
    run(&mut s, "sheet", json!({"number": "A102"}));
    // A3: the title block stands in the border's bottom right corner; the logo's middle is 12 mm along and 7 up in it.
    let margins = s.model.standard().sheet_margins("A3");
    let middle = P2(420.0 - margins.right - 100.0, margins.bottom) + P2(12.0, 7.0);
    let d = sheet::draw(&s.model, sheet_id(&s, "A101")).unwrap();
    assert_eq!(colour_at(&png(&d).unwrap(), d.view.px(middle)), [255, 0, 0], "the picture, at its size");
    let outside = P2(420.0 - margins.right - 100.0, margins.bottom) + P2(30.0, 7.0);
    assert_ne!(colour_at(&png(&d).unwrap(), d.view.px(outside)), [255, 0, 0], "and only there");
    let d = sheet::draw(&s.model, sheet_id(&s, "A102")).unwrap();
    assert_ne!(colour_at(&png(&d).unwrap(), d.view.px(middle)), [255, 0, 0], "a sheet without it has no logo");
    // The title block's own preview, as the desktop shows it, has the picture too.
    let preview = crate::titleblock::preview(&s.model, &s.model.title_blocks["ESCRITORIO"], &Default::default());
    assert_eq!(preview.prims.iter().filter(|p| matches!(p, Prim::Image { .. })).count(), 1);
}

#[test]
fn a_logo_stays_with_its_project_file_and_its_picture_goes_when_it_does() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    with_logo(&mut s, dir.path());
    // Changing a material, which prunes the pictures nothing uses, leaves the logo's.
    run(&mut s, "material", json!({"name": "Plaster", "color": "#ffffff"}));
    assert_eq!(s.model.images.len(), starter() + 1);
    let file = dir.path().join("logo.bimc");
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    let opened = run(&mut t, "open", json!({"path": file}));
    assert!(opened.get("left_out").is_none(), "{opened}");
    assert_eq!(t.model.title_blocks, s.model.title_blocks);
    assert_eq!(t.model.images.len(), starter() + 1);
    // Taken out of the title block, its picture is taken out of the project; the other edits leave a logo alone.
    run(&mut s, "title_block", json!({"name": "ESCRITORIO", "width": 110}));
    assert_eq!(s.model.title_blocks["ESCRITORIO"].logos.len(), 1);
    run(&mut s, "title_block", json!({"name": "ESCRITORIO", "logos": []}));
    assert!(s.model.title_blocks["ESCRITORIO"].logos.is_empty());
    assert_eq!(s.model.images.len(), starter());
    // A picture of the project's is placed by its id, and a copy of the block carries the logo.
    let path = write_png(dir.path(), "again.png", (10, 10), [0, 0, 255]);
    let made = run(&mut s, "title_block", json!({"name": "A", "logos": [{"path": path, "at": [0, 0], "height": 8}]}));
    let id = made["title_block"]["logos"][0]["image"].as_str().unwrap().to_string();
    assert_eq!(made["title_block"]["logos"][0]["width"], 8.0, "a square one, 8 mm tall, is 8 mm wide");
    let copy = run(&mut s, "title_block", json!({"name": "B", "like": "A"}));
    assert_eq!(copy["title_block"]["logos"][0]["image"], id.as_str());
    run(&mut s, "title_block", json!({"name": "C", "logos": [{"image": id, "at": [1, 1], "width": 5}]}));
    assert_eq!(s.model.images.len(), starter() + 1, "one picture, three users");
    run(&mut s, "title_block", json!({"name": "A", "delete": true}));
    run(&mut s, "title_block", json!({"name": "B", "delete": true}));
    assert_eq!(s.model.images.len(), starter() + 1, "still C's");
}

#[test]
fn a_logo_that_is_not_one_is_refused_and_says_what_to_fix() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    let path = write_png(dir.path(), "logo.png", (40, 20), [255, 0, 0]);
    let mut refused = |logos: Value, wants: &str| {
        let e = s.execute("title_block", &json!({"name": "T", "logos": logos})).unwrap_err();
        assert!(e.contains(wants), "{wants}: {e}");
    };
    refused(json!([{"path": path, "at": [0, 0]}]), "width");
    refused(json!([{"path": path, "at": [0, 0], "width": 0}]), "width");
    refused(json!([{"path": path, "at": [0, 0], "width": 5000}]), "width");
    refused(json!([{"path": "/no/such/logo.png", "at": [0, 0], "width": 10}]), "logo.png");
    refused(json!([{"image": "img-nothing", "at": [0, 0], "width": 10}]), "img-nothing");
    refused(json!([{"path": path, "image": "img-x", "at": [0, 0], "width": 10}]), "one");
    refused(json!([{"at": [0, 0], "width": 10}]), "path");
    refused(json!([{"path": path, "at": [0, 0], "width": 10, "colour": "red"}]), "colour");
    refused(json!("logo.png"), "logos");
    refused(json!((0..9).map(|_| json!({"path": path, "at": [0, 0], "width": 10})).collect::<Vec<_>>()), "logos");
    assert!(s.model.title_blocks.is_empty() && s.model.images.len() == starter(), "nothing was kept");
    // A file that is neither a PNG nor a JPEG.
    let text = dir.path().join("logo.txt");
    std::fs::write(&text, "not a picture").unwrap();
    let e = s.execute("title_block", &json!({"name": "T", "logos": [{"path": text, "at": [0, 0], "width": 10}]})).unwrap_err();
    assert!(e.contains("neither a PNG nor a JPEG"), "{e}");
}
