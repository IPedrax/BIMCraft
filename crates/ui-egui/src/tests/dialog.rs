//! The system's file dialogs run on a thread of their own, so the window keeps its event loop while one is open (issue 43,
//! where a drag reaching a window that sat inside a dialog's run loop aborted the app on macOS). No test opens a real
//! dialog: the answer arrives on a channel, as the thread sends it.

use std::path::PathBuf;
use std::sync::mpsc::channel;

use super::*;
use crate::Status;
use crate::dialog::Dialog;

/// A dialog that, once answered, says the path in the status box.
fn open_dialog(h: &mut Harness<BimApp>) -> std::sync::mpsc::Sender<Option<PathBuf>> {
    let (tx, answer) = channel();
    let then = Box::new(|app: &mut BimApp, path: PathBuf| app.status = Status { text: path.display().to_string(), error: false });
    h.state_mut().dialog = Some(Dialog { answer, then });
    tx
}

#[test]
fn a_dialogs_path_is_acted_on_in_a_later_frame_and_the_window_goes_on_meanwhile() {
    let mut h = app(BOX);
    let tx = open_dialog(&mut h);
    h.step();
    h.step();
    assert!(h.state().dialog.is_some() && h.state().status.text.is_empty(), "no answer yet: nothing done, and the frames run");
    tx.send(Some("/projects/house.bimcraft".into())).unwrap();
    h.step();
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().status.text, "/projects/house.bimcraft");
}

#[test]
fn a_cancelled_dialog_does_nothing_and_a_dialog_whose_thread_died_is_dropped() {
    let mut h = app(BOX);
    let tx = open_dialog(&mut h);
    tx.send(None).unwrap();
    h.step();
    assert!(h.state().dialog.is_none() && h.state().status.text.is_empty(), "Cancel is not an answer");
    let tx = open_dialog(&mut h);
    drop(tx);
    h.step();
    assert!(h.state().dialog.is_none(), "so the next dialog can open");
    assert!(h.state().status.text.is_empty());
}
