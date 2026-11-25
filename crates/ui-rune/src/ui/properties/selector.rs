//! The Type Selector: a card naming the category (or the family) and the type the selected elements are, which opens, in place, the
//! types they can be; and Edit Type under it, which asks for the Type Properties dialog (`App::edit_type`, a seam until the dialogs
//! session's window has it). Picking a type is a `modify` of the shown elements (`App::pick_type`).

use rune::tok::{Theme, type_};
use rune::{El, ScrollSpec, col, row, text};

use super::editors::button;
use crate::app::App;
use crate::lang::t;

/// The key the open list is kept under (`Palette::open`).
pub const KEY: &str = "type";
/// How tall the open list grows before it scrolls, points.
const LIST: f32 = 260.0;

/// The selector for what the palette shows, if it shows elements of one category.
pub fn selector(s: &App, th: &Theme) -> Option<El> {
    let sel = s.palette.panel.selector.as_ref()?;
    let open = sel.enabled && s.palette.open == Some(KEY);
    let current = sel.current.clone().unwrap_or_default();
    let mut card = row()
        .key("type-selector")
        .role("button")
        .label(t("Type Selector"))
        .expanded(open)
        .focusable(sel.enabled)
        .w_full()
        .h(46.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .fill(th.control_track)
        .child(
            col()
                .grow(1.0)
                .child(text(sel.heading.clone()).font(type_::LABEL).color(th.text).truncate())
                .child(text(current.clone()).key("type-current").font(type_::BODY).color(th.text_secondary).truncate()),
        );
    if sel.enabled {
        card = card
            .child(text(if open { "▴" } else { "▾" }).font(type_::LABEL).color(th.text_secondary))
            .on_press(|s: &mut App| s.open_list(KEY));
    }
    let mut c = col().key("type-box").w_full().gap(4.0).child(card);
    if open {
        let mut items: Vec<El> = Vec::new();
        let mut heading = None;
        for (i, ty) in sel.types.iter().enumerate() {
            if ty.heading != heading {
                heading = ty.heading;
                if let Some(h) = heading {
                    items.push(text(t(h)).font(type_::SECTION).color(th.text_secondary));
                }
            }
            let item = row()
                .key(format!("opt-{KEY}-{i}"))
                .role("button")
                .label(ty.label.clone())
                .focusable(true)
                .w_full()
                .h(24.0)
                .px(8.0)
                .center_y()
                .radius(4.0)
                .child(text(ty.label.clone()).font(type_::BODY).color(th.text).truncate())
                .on_press(move |s: &mut App| {
                    s.pick_type(i);
                });
            items.push(if current == ty.label { item.fill(th.control_off) } else { item });
        }
        c = c.child(
            col()
                .key("type-list")
                .w_full()
                .gap(1.0)
                .padding(4.0)
                .radius(6.0)
                .fill(th.card)
                .border(1.0, th.card_border)
                .max_h(LIST)
                .children(items)
                .scroll(ScrollSpec::default()),
        );
    }
    if sel.editable()
        && let Some(&id) = s.palette.panel.ids.first()
    {
        c = c.child(button("edit-type", t("Edit Type"), th, move |s: &mut App| s.edit_type(id)));
    }
    Some(c)
}
