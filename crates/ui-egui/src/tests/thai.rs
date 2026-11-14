//! The interface in Thai. Like Chinese it is the window's alone and drawn with a system font: a window is told it has one, or
//! none, except the last test, which draws with the machine's own when it has one.

use super::*;
use crate::lang::{self, Lang};
use crate::settings::language_in;

#[test]
fn the_window_speaks_thai_once_it_is_put_in_it_and_keeps_to_it() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-thai-{}", std::process::id())).join("settings.json");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    lang::assume_cjk_font(&h.ctx);
    let ctx = h.ctx.clone();
    h.state_mut().set_lang(&ctx, Lang::Thai);
    h.run();
    assert_eq!(h.state().lang, Lang::Thai);
    assert!(h.query_by_label("Architecture").is_none(), "the ribbon's tabs are in Thai");
    assert_eq!(language_in(&file), Some(Lang::Thai));
    let mut again = app(BOX);
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().lang, Lang::Thai);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn without_a_font_for_thai_the_window_stays_as_it_was_and_says_why() {
    let mut h = app_of(lang::no_fonts);
    let ctx = h.ctx.clone();
    h.state_mut().set_lang(&ctx, Lang::Thai);
    h.run();
    assert_eq!(h.state().lang, Lang::English);
    let status = h.state().status.clone();
    assert!(status.error && status.text.starts_with("No font with Thai letters"), "{status:?}");
    // A settings file that says Thai on a machine with no font: the window comes up in English, not in boxes.
    let mut start = app_of(lang::no_fonts);
    start.state_mut().lang = Lang::Thai;
    start.run();
    assert_eq!(start.state().lang, Lang::English);
    assert!(start.state().status.error);
}

#[test]
fn thai_in_the_window_s_lettering_are_glyphs_and_not_boxes() {
    // shortcut: needs a Thai font on the machine, as the window does; a machine without one has nothing to draw with.
    if !lang::cjk_fonts().thai {
        return;
    }
    let mut h = app(BOX);
    assert!(lang::missing_font(&h.ctx, lang::cjk_fonts, Lang::Thai).is_none());
    h.run();
    for family in crate::style::families() {
        let drawn = h.ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::new(13.0, family.clone()), "สถาปัตยกรรม ผนัง ประตู หน้าต่าง"));
        assert!(drawn, "{family:?} draws Thai");
    }
}

fn app_of(find: lang::FontFinder) -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let mut app = BimApp::new(s);
    app.font_finder = find;
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .with_step_dt(0.25)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
    h.run();
    h
}

/// What the Thai window looks like, for a person to look at: `BIMCRAFT_THAI_DUMP=<folder>` writes it with the machine's own Thai
/// font, light and dark, its ribbon on Architecture and on Modify with a wall selected.
#[test]
fn thai_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_THAI_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for dark in [false, true] {
        let mut h = app(BOX);
        let ctx = h.ctx.clone();
        assert!(lang::missing_font(&ctx, lang::cjk_fonts, Lang::Thai).is_none(), "needs a Thai font on this machine");
        h.state_mut().set_lang(&ctx, Lang::Thai);
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        h.run();
        let look = if dark { "dark" } else { "light" };
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("thai-{look}.png"))).unwrap();
        click(&mut h, P2(0.0, 2000.0));
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("thai-{look}-wall.png"))).unwrap();
    }
}
