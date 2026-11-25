//! The top bar's export buttons: the view shown, as PDF or PNG, to the file the system's dialog answers (`App::export_view`).

use rune::tok::{Theme, type_};
use rune::{El, FileDialogOptions, Tx, el, text};

use crate::app::App;
use crate::lang::tf;

/// A button that exports the view shown as `format` (an extension `export_formats` has).
pub fn button(format: &'static str, th: &Theme) -> El {
    let label = format.to_uppercase();
    el().key(format!("export-{format}"))
        .role("button")
        .label(tf("Export as {}", &[&label]))
        .focusable(true)
        .px(12.0)
        .h(26.0)
        .center()
        .radius(6.0)
        .fill(th.control_track)
        .child(text(label.clone()).font(type_::LABEL).color(th.text))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            let options = FileDialogOptions {
                title: tf("Export as {}", &[&label]),
                file_name: Some(s.export_file_name(format)),
                ..Default::default()
            }
            .filter(label.as_str(), &[format]);
            tx.save_file(options, move |s: &mut App, answer, _| match answer {
                Ok(Some(path)) => {
                    s.export_view(format, path);
                }
                Ok(None) => {}
                Err(why) => s.message = Some(why.to_string()),
            });
        })
}
