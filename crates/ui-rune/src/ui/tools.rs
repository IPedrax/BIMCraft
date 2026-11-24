//! The Tools palette: a column of the tools by discipline (`tools::TOOLS`) at the window's left. A press picks a tool (a second press
//! puts it down); what a tool does on the plan is stage 3 (docs/areas/rune-ui.md). The palette never changes while the pointer
//! moves, so it is one element kept by `memo` while the picked tool is the same.

use rune::tok::{Theme, type_};
use rune::{Color, El, ScrollSpec, col, el, memo, row, text};

use crate::app::App;
use crate::lang::t;
use crate::tools::TOOLS;

pub const WIDTH: f32 = 200.0;

fn tool_row(id: &'static str, label: &str, active: bool, th: &Theme) -> El {
    row()
        .key(format!("tool-{id}"))
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(26.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .fill(if active { th.control_off } else { Color::TRANSPARENT })
        .child(el().size(14.0, 14.0).radius(3.0).fill(th.indicator))
        .child(text(label.to_string()).font(type_::BODY).color(th.text))
        .on_press(move |s: &mut App| s.set_tool(id))
}

pub fn palette(s: &App, th: &Theme) -> El {
    let picked = s.tool;
    let list = memo("tools", picked, || {
        let mut rows = col().gap(2.0);
        for &(section, tools) in TOOLS {
            rows = rows.child(text(t(section).to_string()).font(type_::SECTION).color(th.text_secondary).padding(6.0));
            for &(id, label) in tools {
                rows = rows.child(tool_row(id, t(label), picked == Some(id), th));
            }
        }
        rows.scroll(ScrollSpec::default()).grow(1.0).min_h(0.0)
    });
    col().key("tools").w(WIDTH).h_full().fill(th.card).padding(8.0).gap(6.0).child(list)
}
