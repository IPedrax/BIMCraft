//! The status bar: the line `App::status` says, along the window's foot.

use rune::tok::{Theme, type_};
use rune::{El, row, text};

use crate::app::App;

pub const HEIGHT: f32 = 24.0;

pub fn bar(s: &App, th: &Theme) -> El {
    row()
        .key("status")
        .w_full()
        .h(HEIGHT)
        .px(10.0)
        .center_y()
        .fill(th.card)
        .child(text(s.status()).key("status-line").font(type_::LABEL).color(th.text_secondary).truncate())
}
