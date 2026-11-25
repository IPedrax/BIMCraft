//! The bar across the top: Open and Save (the system's file dialogs), Undo and Redo, the zoom buttons, and the project's name. A button
//! runs one method of `App`; Open and Save ask the dialog and run the same method with the path it answers (a headless test answers it
//! too).

use rune::tok::{Theme, type_};
use rune::{El, FileDialogOptions, Tx, el, row, text};

use crate::app::App;
use crate::lang::t;

pub const HEIGHT: f32 = 36.0;

fn button(key: &'static str, label: &str, th: &Theme, on_press: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    labelled(key, label, label, th, on_press)
}

/// A button that shows `shown` and says `label` to a screen reader (the zoom buttons are short on the bar and long to the ear).
fn labelled(key: &'static str, label: &str, shown: &str, th: &Theme, on_press: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    el().key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .px(12.0)
        .h(26.0)
        .center()
        .radius(6.0)
        .fill(th.control_track)
        .child(text(shown.to_string()).font(type_::LABEL).color(th.text))
        .on_press_tx(on_press)
}

fn project_files(title: &str) -> FileDialogOptions {
    FileDialogOptions { title: title.to_string(), ..Default::default() }.filter("BIMCraft", &["bimc", "bimcraft", "bimt", "ifc"])
}

pub fn bar(s: &App, th: &Theme) -> El {
    let name =
        s.session.path.as_ref().and_then(|p| p.file_name()).map_or_else(|| t("Untitled").to_string(), |n| n.to_string_lossy().into_owned());
    // Zoom In Region lit while it waits for its box.
    let armed = if s.region { th.control_off } else { th.control_track };
    row()
        .key("topbar")
        .w_full()
        .h(HEIGHT)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .fill(th.card)
        .child(button("open", t("Open"), th, |_: &mut App, tx: &mut Tx| {
            tx.open_file(project_files(t("Open")), |s: &mut App, answer, _| match answer {
                Ok(Some(path)) => {
                    s.open_path(path);
                }
                Ok(None) => {}
                Err(why) => s.message = Some(why.to_string()),
            });
        }))
        .child(button("save", t("Save"), th, |s: &mut App, tx: &mut Tx| {
            if s.has_file() {
                s.save();
                return;
            }
            let options = FileDialogOptions { file_name: Some("Project.bimc".into()), ..project_files(t("Save")) };
            tx.save_file(options, |s: &mut App, answer, _| match answer {
                Ok(Some(path)) => {
                    s.save_as(path);
                }
                Ok(None) => {}
                Err(why) => s.message = Some(why.to_string()),
            });
        }))
        .child(button("undo", t("Undo"), th, |s: &mut App, _: &mut Tx| {
            s.undo();
        }))
        .child(button("redo", t("Redo"), th, |s: &mut App, _: &mut Tx| {
            s.redo();
        }))
        .child(super::export::button("pdf", th))
        .child(super::export::button("png", th))
        .child(labelled("zoom-fit", t("Zoom to Fit"), t("Fit"), th, |s: &mut App, _: &mut Tx| s.zoom_fit()))
        .child(labelled("zoom-in", t("Zoom In"), "+", th, |s: &mut App, _: &mut Tx| s.zoom_by(1.25)))
        .child(labelled("zoom-out", t("Zoom Out"), "-", th, |s: &mut App, _: &mut Tx| s.zoom_by(0.8)))
        .child(labelled("zoom-region", t("Zoom In Region"), t("Region"), th, |s: &mut App, _: &mut Tx| s.arm_region()).fill(armed))
        .child(labelled("zoom-previous", t("Previous Pan/Zoom"), t("Back"), th, |s: &mut App, _: &mut Tx| {
            s.previous();
        }))
        .child(text(name).key("project-name").font(type_::TITLE).color(th.text_secondary))
}
