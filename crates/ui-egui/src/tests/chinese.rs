//! The interface in Simplified Chinese (issue 38). No test draws a glyph: a window is told it has a font, or that it has none.

use super::*;
use crate::lang::{self, Lang};
use crate::settings::language_in;

fn settings_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-ui-chinese-{}-{name}", std::process::id())).join("settings.json")
}

#[test]
fn the_window_speaks_chinese_once_it_is_put_in_it_and_keeps_to_it() {
    let file = settings_file("kept");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    lang::assume_chinese_font(&h.ctx);
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
    let mut h = app(BOX);
    h.state_mut().font_finder = || None;
    let ctx = h.ctx.clone();
    h.state_mut().set_lang(&ctx, Lang::Chinese);
    h.run();
    assert_eq!(h.state().lang, Lang::English);
    let status = h.state().status.clone();
    assert!(status.error && status.text.starts_with("No font with Chinese characters"), "{status:?}");
    // A settings file that says Chinese on a machine with no font: the window comes up in English, not in boxes.
    let mut start = app(BOX);
    start.state_mut().font_finder = || None;
    start.state_mut().lang = Lang::Chinese;
    start.run();
    assert_eq!(start.state().lang, Lang::English);
    assert!(start.state().status.error);
    // In Portuguese, the reason is said in it.
    let mut pt = app(BOX);
    pt.state_mut().font_finder = || None;
    pt.state_mut().lang = Lang::Portuguese;
    pt.run();
    let ctx = pt.ctx.clone();
    pt.state_mut().set_lang(&ctx, Lang::Chinese);
    assert!(pt.state().status.text.starts_with("Nenhuma fonte com caracteres chineses"), "{:?}", pt.state().status);
}

#[test]
fn the_language_list_offers_chinese_and_picking_it_puts_the_window_in_it() {
    let mut h = app(BOX);
    lang::assume_chinese_font(&h.ctx);
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
