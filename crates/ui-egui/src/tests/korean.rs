//! Korean content in a window of whatever language (issue 156): the fonts go behind its own whether it speaks English or Chinese,
//! for a floor's name from a Korean file is not the interface's language.

use super::*;
use crate::lang::{self, Script};

#[test]
fn hangul_in_an_english_window_are_glyphs_and_not_boxes() {
    // ponytail: needs a font with the Hangul on the machine, as the window does; a machine without one has nothing to draw with.
    if !lang::cjk_fonts().fonts.iter().any(|(_, _, s)| *s != Script::Han) {
        return;
    }
    let mut h = app(BOX);
    h.run();
    for family in crate::style::families() {
        let drawn = h.ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::new(13.0, family.clone()), "제1층 지하 주차층"));
        assert!(drawn, "{family:?} draws Hangul in an English window");
    }
}
