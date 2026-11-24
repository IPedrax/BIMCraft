//! The Project Browser's first list: the model's levels, one a row, the one on screen lit. A press shows that level's plan. The views
//! (ceiling plans, 3D, sections, sheets, schedules) join it as the browser grows; it is one element kept by `memo` while the levels
//! and the level shown are the same.

use rune::tok::{Theme, type_};
use rune::{Color, El, ScrollSpec, col, memo, row, text};

use crate::app::App;
use crate::lang::t;

pub const HEIGHT: f32 = 220.0;

pub fn panel(s: &App, th: &Theme) -> El {
    let shown = s.level;
    let list = memo("browser-levels", (shown, &s.levels), || {
        col().gap(2.0).children(s.levels.iter().map(|(id, name)| {
            let id = *id;
            row()
                .key(format!("level-{id}"))
                .role("button")
                .label(name.clone())
                .focusable(true)
                .h(24.0)
                .px(8.0)
                .center_y()
                .radius(6.0)
                .fill(if shown == Some(id) { th.control_off } else { Color::TRANSPARENT })
                .child(text(name.clone()).font(type_::BODY).color(th.text))
                .on_press(move |s: &mut App| s.set_level(id))
        }))
    });
    col()
        .key("browser")
        .w_full()
        .h(HEIGHT)
        .fill(th.card)
        .padding(12.0)
        .gap(8.0)
        .child(text(t("Levels")).font(type_::TITLE).color(th.text))
        .child(list.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0))
}
