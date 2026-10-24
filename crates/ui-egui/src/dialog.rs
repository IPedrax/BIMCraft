//! The system's file dialogs. One run inside the window's event handler nests a run loop of its own in it, and on macOS
//! a drag that reaches the window meanwhile re-enters the handler and aborts the app (issue 43). So a dialog is only begun on
//! the window's thread (AppKit wants that, and the sheet it opens does not block) and waited for on a thread of its own: the
//! window keeps its event loop, and the path comes back in a later frame.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;

use crate::BimApp;

/// What is done with the path a dialog gives.
pub(crate) type Then = Box<dyn FnOnce(&mut BimApp, PathBuf)>;

/// A dialog that is open: what it answers, and what to do with a path.
pub(crate) struct Dialog {
    pub(crate) answer: Receiver<Option<PathBuf>>,
    pub(crate) then: Then,
}

impl BimApp {
    /// Asks for a file to open, or for where to save one (`save`), and calls `then` with the path in a later frame. Nothing
    /// happens if the dialog is cancelled, and a second dialog is not opened while one is.
    pub(crate) fn ask_file(
        &mut self,
        ctx: &Context,
        dialog: rfd::AsyncFileDialog,
        save: bool,
        then: impl FnOnce(&mut BimApp, PathBuf) + 'static,
    ) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, answer) = channel();
        let ctx = ctx.clone();
        // Made here, on the window's thread: on macOS that is where the dialog is begun (its sheet opens at once and does not
        // block), and only waiting for it is done on the thread below.
        let future: Pin<Box<dyn Future<Output = Option<rfd::FileHandle>> + Send>> =
            if save { Box::pin(dialog.save_file()) } else { Box::pin(dialog.pick_file()) };
        std::thread::spawn(move || {
            let file = pollster::block_on(future);
            let _ = tx.send(file.map(|f| f.path().to_path_buf()));
            ctx.request_repaint();
        });
        self.dialog = Some(Dialog { answer, then: Box::new(then) });
    }

    /// Once a frame: acts on the path an open dialog has given. A thread that died leaves no answer, and the dialog is dropped.
    pub(crate) fn dialog_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        match open.answer.try_recv() {
            Ok(path) => {
                if let Some(Dialog { then, .. }) = self.dialog.take()
                    && let Some(path) = path
                {
                    then(self, path);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.dialog = None,
        }
    }
}
