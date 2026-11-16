//! The system's file dialogs. One run inside the window's event handler nests a run loop of its own in it, and on macOS
//! a drag that reaches the window meanwhile re-enters the handler and aborts the app (issue 43). So a dialog is only begun on
//! the window's thread (AppKit wants that, and the sheet it opens does not block) and waited for on a thread of its own: the
//! window keeps its event loop, and the path comes back in a later frame.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::Context;

use crate::BimApp;

/// What is done with the paths a dialog gives.
pub(crate) type Then = Box<dyn FnOnce(&mut BimApp, Vec<PathBuf>)>;

/// A dialog that is open: what it answers, and what to do with its paths.
pub(crate) struct Dialog {
    pub(crate) answer: Receiver<Option<Vec<PathBuf>>>,
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
        let future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>> = if save {
            Box::pin(async move { dialog.save_file().await.map(|f| vec![f]) })
        } else {
            Box::pin(async move { dialog.pick_file().await.map(|f| vec![f]) })
        };
        self.begin(
            ctx,
            future,
            Box::new(|app, paths| {
                if let Some(path) = paths.into_iter().next() {
                    then(app, path);
                }
            }),
        );
    }

    /// Asks for a folder, and calls `then` with its path in a later frame, as [`BimApp::ask_file`] does.
    pub(crate) fn ask_folder(&mut self, ctx: &Context, dialog: rfd::AsyncFileDialog, then: impl FnOnce(&mut BimApp, PathBuf) + 'static) {
        let future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>> =
            Box::pin(async move { dialog.pick_folder().await.map(|f| vec![f]) });
        self.begin(
            ctx,
            future,
            Box::new(|app, paths| {
                if let Some(path) = paths.into_iter().next() {
                    then(app, path);
                }
            }),
        );
    }

    /// Asks for one file or several to open, and calls `then` with their paths in a later frame, as [`BimApp::ask_file`] does.
    pub(crate) fn ask_files(
        &mut self,
        ctx: &Context,
        dialog: rfd::AsyncFileDialog,
        then: impl FnOnce(&mut BimApp, Vec<PathBuf>) + 'static,
    ) {
        self.begin(ctx, Box::pin(dialog.pick_files()), Box::new(then));
    }

    fn begin(&mut self, ctx: &Context, future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>, then: Then) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, answer) = channel();
        wait(ctx, future, tx);
        self.dialog = Some(Dialog { answer, then });
    }

    /// Once a frame: acts on the path an open dialog has given. A thread that died leaves no answer, and the dialog is dropped.
    pub(crate) fn dialog_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        match open.answer.try_recv() {
            Ok(paths) => {
                if let Some(Dialog { then, .. }) = self.dialog.take()
                    && let Some(paths) = paths.filter(|p| !p.is_empty())
                {
                    then(self, paths);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.dialog = None,
        }
    }
}

/// Waits for `future`'s answer on a thread of its own and sends it on `tx`. The future is made by the caller, on the window's
/// thread: on macOS that is where the dialog is begun (its sheet opens at once and does not block), and only waiting for it
/// is done on the thread.
#[cfg(not(test))]
fn wait(ctx: &Context, future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>, tx: Sender<Option<Vec<PathBuf>>>) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let files = pollster::block_on(future);
        let _ = tx.send(files.map(|f| f.iter().map(|f| f.path().to_path_buf()).collect()));
        ctx.request_repaint();
    });
}

/// A test opens no system dialog: the dialog it begins is answered through `BEGUN`'s sender.
#[cfg(test)]
fn wait(_: &Context, future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>, tx: Sender<Option<Vec<PathBuf>>>) {
    drop(future);
    BEGUN.with(|b| b.borrow_mut().push(tx));
}

#[cfg(test)]
thread_local! {
    /// The answer to each dialog begun in the test's thread, oldest first.
    pub(crate) static BEGUN: std::cell::RefCell<Vec<Sender<Option<Vec<PathBuf>>>>> = const { std::cell::RefCell::new(Vec::new()) };
}
