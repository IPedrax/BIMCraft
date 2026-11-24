//! Rev's ribbon: a strip of tabs, one a discipline of `tools::TOOLS`, and under it the tab's tools and the Modify group as buttons with
//! their icons over their names. The same table the BIMCraft profile's palette draws, laid out the other way; the only difference
//! between the two profiles is how it looks. Kept as one element while the tab and the tool in hand are the same.

use rune::tok::{Theme, type_};
use rune::{Color, El, col, el, memo, row, text};

use crate::app::App;
use crate::lang::t;
use crate::tools::{MODIFY, TOOLS};

pub const HEIGHT: f32 = 122.0;

fn tab(i: usize, name: &str, active: bool, th: &Theme) -> El {
    el().key(format!("tab-{i}"))
        .role("tab")
        .label(name.to_string())
        .focusable(true)
        .h(28.0)
        .px(14.0)
        .center()
        .radius(6.0)
        .fill(if active { th.control_off } else { Color::TRANSPARENT })
        .child(text(name.to_string()).font(type_::LABEL).color(th.text))
        .on_press(move |s: &mut App| s.set_tab(i))
}

fn button(id: &'static str, label: &str, active: bool, th: &Theme) -> El {
    col()
        .key(format!("tool-{id}"))
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .w(76.0)
        .h(72.0)
        .gap(4.0)
        .center()
        .radius(6.0)
        .fill(if active { th.control_off } else { Color::TRANSPARENT })
        .child(el().size(32.0, 32.0).radius(6.0).fill(th.indicator))
        .child(text(label.to_string()).font(type_::LABEL).color(th.text))
        .on_press(move |s: &mut App| s.set_tool(id))
}

fn group(title: &str, tools: &[(&'static str, &'static str)], picked: Option<&str>, th: &Theme) -> El {
    col()
        .gap(4.0)
        .child(row().gap(2.0).children(tools.iter().map(|&(id, label)| button(id, t(label), picked == Some(id), th))))
        .child(text(t(title).to_string()).font(type_::SECTION).color(th.text_secondary))
}

pub fn ribbon(s: &App, th: &Theme) -> El {
    let (open, picked) = (s.tab.min(TOOLS.len().saturating_sub(1)), s.tool);
    let body = memo("ribbon", (open, picked), || {
        let tabs = row().gap(2.0).children(TOOLS.iter().enumerate().map(|(i, (name, _))| tab(i, t(name), i == open, th)));
        let panels = row().gap(18.0).child(group(TOOLS[open].0, TOOLS[open].1, picked, th)).child(group("Modify", MODIFY, picked, th));
        col().gap(6.0).child(tabs).child(panels)
    });
    col().key("ribbon").w_full().h(HEIGHT).fill(th.card).padding(8.0).child(body)
}
