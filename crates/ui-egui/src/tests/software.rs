//! The window drawn on the processor (`software::raster`) against egui-wgpu's drawing of the same frames: the start page, the
//! sample's plan, its 3D view and a sheet, side by side. A runner with no graphics adapter has nothing to compare with, and says
//! so. The two differ where egui-wgpu's test target blends in linear light (anti-aliased edges, text) and where the card's
//! texture filtering rounds otherwise; what is filled, where, and in what colour, must match.

use super::*;
use crate::software::raster::Raster;
use egui::TexturesDelta;
use egui_kittest::TestRenderer;

/// kittest's renderer, drawing with the processor's raster.
struct Processor(Raster);

impl TestRenderer for Processor {
    fn handle_delta(&mut self, delta: &mut TexturesDelta) {
        self.0.set_textures(delta);
        self.0.free_textures(delta);
    }

    fn render(&mut self, ctx: &egui::Context, output: &egui::FullOutput) -> Result<image::RgbaImage, String> {
        let ppp = ctx.pixels_per_point();
        let size = ctx.content_rect().size() * ppp;
        let (w, h) = (size.x.round() as usize, size.y.round() as usize);
        let primitives = ctx.tessellate(output.shapes.clone(), ppp);
        let mut px = vec![0u32; w * h];
        self.0.draw(&mut px, w, h, ppp, egui::Color32::from_rgb(12, 12, 12), &primitives);
        let bytes = px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]).collect();
        image::RgbaImage::from_raw(w as u32, h as u32, bytes).ok_or_else(|| "the picture's size".to_string())
    }
}

/// The app on `script`, at 1280 × 800, drawn by `renderer`, after `then`.
fn window(script: &str, renderer: Option<Processor>, then: &dyn Fn(&mut Harness<BimApp>)) -> image::RgbaImage {
    let mut s = Session::new();
    s.script(script).unwrap();
    let builder = Harness::builder().with_size([1280.0, 800.0]).with_max_steps(64).with_step_dt(0.25);
    let mut h = match renderer {
        Some(r) => builder.renderer(r).build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s)),
        None => builder.build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s)),
    };
    h.run();
    then(&mut h);
    h.run();
    h.render().unwrap()
}

/// The pixels where any channel differs by more than `by`, and the share of the picture they are.
fn differing(a: &image::RgbaImage, b: &image::RgbaImage, by: u8) -> (usize, f64) {
    assert_eq!(a.dimensions(), b.dimensions());
    let n = a.pixels().zip(b.pixels()).filter(|(p, q)| (0..3).any(|i| p.0[i].abs_diff(q.0[i]) > by)).count();
    (n, n as f64 / f64::from(a.width() * a.height()))
}

fn compare(name: &str, script: &str, then: &dyn Fn(&mut Harness<BimApp>)) {
    let card = window(script, None, then);
    let cpu = window(script, Some(Processor(Raster::new(2))), then);
    if let Ok(folder) = std::env::var("BIMCRAFT_SHOTS") {
        let _ = card.save(std::path::Path::new(&folder).join(format!("software-{name}-card.png")));
        let _ = cpu.save(std::path::Path::new(&folder).join(format!("software-{name}-cpu.png")));
    }
    // Far apart (a fill missing, a wrong colour) is rare; a little apart (edges, text, filtering) is allowed more.
    let (far, far_share) = differing(&card, &cpu, 64);
    let (near, near_share) = differing(&card, &cpu, 16);
    eprintln!(
        "{name}: {far} pixels differ by more than 64 ({:.3} %), {near} by more than 16 ({:.3} %)",
        far_share * 100.0,
        near_share * 100.0
    );
    assert!(far_share < 0.002, "{name}: {far} pixels far apart");
    assert!(near_share < 0.02, "{name}: {near} pixels apart");
}

#[test]
fn the_processor_draws_the_window_as_the_graphics_card_does() {
    if crate::gpu::headless().is_none() {
        eprintln!("no graphics adapter here: nothing to compare the processor's drawing with");
        return;
    }
    compare("start", "", &|h| h.state_mut().show_start_page());
    compare("plan", SAMPLE, &|_| {});
    compare("3d", SAMPLE, &|h| press(h, "Default 3D View"));
    compare("sheet", SAMPLE, &|h| {
        let sheet = ids(h, "sheet")[0];
        h.state_mut().open_sheet(Sheet::Paper(sheet));
    });
}

#[test]
fn on_the_processor_a_view_moved_shows_the_last_drawing_until_a_worker_makes_the_next() {
    let mut s = Session::new();
    s.script(SAMPLE).unwrap();
    let mut app = BimApp::new(s);
    app.drawn_on_processor = Some("a test".into());
    let Sheet::Plan(level) = app.sheet else { panic!("the sample opens on a plan") };
    let ctx = egui::Context::default();
    let mut i = 0u32;
    let mut step = |app: &mut BimApp| {
        let side = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let mut raw = egui::RawInput { screen_rect: Some(side), time: Some(f64::from(i) * 10.0), ..Default::default() };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
        i += 1;
        // What the frame changed of the textures goes nowhere here (egui checks, in a debug build, that it isn't dropped unapplied).
        ctx.run_ui(raw, |ui| app.show(ui)).textures_delta.clear();
    };
    let settled = |app: &mut BimApp, step: &mut dyn FnMut(&mut BimApp)| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.canvas.drawing.shown_drawing().map(|d| d.view) != app.canvas.current_view(level) {
            assert!(std::time::Instant::now() < deadline, "the worker published nothing");
            std::thread::sleep(std::time::Duration::from_millis(5));
            step(app);
        }
    };
    for _ in 0..4 {
        step(&mut app);
    }
    settled(&mut app, &mut step);
    let first = app.canvas.drawing.shown_drawing().unwrap();
    app.canvas.show(level, P2(1000.0, 1000.0), P2(6000.0, 5000.0));
    step(&mut app);
    assert!(Arc::ptr_eq(&first, &app.canvas.drawing.shown_drawing().unwrap()), "the frame that moved doesn't wait for a drawing");
    settled(&mut app, &mut step);
    assert!(!Arc::ptr_eq(&first, &app.canvas.drawing.shown_drawing().unwrap()), "the worker's drawing replaced it");
}
