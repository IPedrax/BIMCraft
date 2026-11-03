//! The interface in Simplified Chinese (issue 38). No test draws a glyph: a window is told it has a font, or that it has none.

use super::*;
use crate::lang::{self, Lang, Script};
use crate::settings::language_in;

fn settings_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-ui-chinese-{}-{name}", std::process::id())).join("settings.json")
}

#[test]
fn the_window_speaks_chinese_once_it_is_put_in_it_and_keeps_to_it() {
    let file = settings_file("kept");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    lang::assume_cjk_font(&h.ctx);
    let ctx = h.ctx.clone();
    h.state_mut().set_lang(&ctx, Lang::Chinese);
    h.run();
    assert_eq!(h.state().lang, Lang::Chinese);
    for word in ["文件", "建筑", "注释", "体量建模", "视图", "管理", "修改"] {
        assert!(h.query_all_by_label(word).next().is_some(), "the ribbon's tab `{word}`");
    }
    assert!(h.query_by_label("Architecture").is_none());
    // New projects are named in the project's language, which is never Chinese.
    press(&mut h, "文件");
    h.get_by_label_contains("新建").click();
    h.run();
    let levels: Vec<String> = h.state().session.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert_eq!(levels, ["Level 1", "Level 2"]);
    // The settings file has it, and a window started on the file is in it.
    assert_eq!(language_in(&file), Some(Lang::Chinese));
    let mut again = app(BOX);
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().lang, Lang::Chinese);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn without_a_font_for_hanzi_the_window_stays_as_it_was_and_says_why() {
    let mut h = app_of(lang::no_fonts);
    let ctx = h.ctx.clone();
    h.state_mut().set_lang(&ctx, Lang::Chinese);
    h.run();
    assert_eq!(h.state().lang, Lang::English);
    let status = h.state().status.clone();
    assert!(status.error && status.text.starts_with("No font with Chinese characters"), "{status:?}");
    // A settings file that says Chinese on a machine with no font: the window comes up in English, not in boxes.
    let mut start = app_of(lang::no_fonts);
    start.state_mut().lang = Lang::Chinese;
    start.run();
    assert_eq!(start.state().lang, Lang::English);
    assert!(start.state().status.error);
    // In Portuguese, the reason is said in it.
    let mut pt = app_of(lang::no_fonts);
    pt.state_mut().lang = Lang::Portuguese;
    pt.run();
    let ctx = pt.ctx.clone();
    pt.state_mut().set_lang(&ctx, Lang::Chinese);
    assert!(pt.state().status.text.starts_with("Nenhuma fonte com caracteres chineses"), "{:?}", pt.state().status);
}

#[test]
fn the_language_list_offers_chinese_and_picking_it_puts_the_window_in_it() {
    let mut h = app(BOX);
    lang::assume_cjk_font(&h.ctx);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_all_by_role(Role::ComboBox).find(|n| n.value().as_deref() == Some("English")).expect("the language list").click();
    h.run();
    h.get_by_label("Chinese (Simplified)").click();
    h.run();
    assert_eq!(h.state().lang, Lang::Chinese);
    // The list shows its name in itself once the window can draw it.
    assert!(h.get_all_by_role(Role::ComboBox).any(|n| n.value().as_deref() == Some("简体中文")));
}

/// A window that looks for its fonts in `find` from its first frame on: the lookup is the window's, once, and the harness's
/// first frame is in the build, so the finder goes in the app before it.
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

/// Any font will do for the first test below: it only looks for where the font with the hanzi was put.
fn some_font() -> lang::Fonts {
    lang::Fonts {
        fonts: vec![(
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf")).to_vec(),
            0,
            Script::Han,
        )],
        hanzi: true,
    }
}

#[test]
fn the_font_with_the_hanzi_joins_every_family_the_window_sets_text_in() {
    let mut h = app_of(some_font);
    assert!(lang::use_cjk_font(&h.ctx, some_font));
    h.run();
    for family in crate::style::families() {
        let has = h.ctx.fonts(|f| f.definitions().families.get(&family).is_some_and(|l| l.iter().any(|n| n == "cjk-han")));
        assert!(has, "{family:?} must hold the font with the hanzi: egui does not fall back across families");
    }
}

#[test]
fn hanzi_in_the_semibold_lettering_are_glyphs_and_not_boxes() {
    // ponytail: needs a CJK font on the machine, as the window does; a machine without one has nothing to draw with.
    if lang::cjk_fonts().fonts.is_empty() {
        return;
    }
    let mut h = app(BOX);
    assert!(lang::use_cjk_font(&h.ctx, lang::cjk_fonts));
    h.run();
    for family in crate::style::families() {
        let drawn = h.ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::new(13.0, family.clone()), "文件建筑注释视图管理修改"));
        assert!(drawn, "{family:?} draws hanzi");
    }
}
