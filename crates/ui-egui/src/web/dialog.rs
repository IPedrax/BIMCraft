//! The file dialogs in a browser, which has no file system. Opening picks a file with the browser's own chooser and puts its
//! bytes in the page (`bimcraft_core::disk::put`) under its name, so the command that reads it finds it by that path as on a
//! computer; once the command has read it, it is taken out again, so the page holds no more than it needs. Saving asks nothing:
//! the path is the name the window offers, the command writes the file into the page, and each file commands wrote is handed
//! to the browser as a download (the page wakes the window when one is written). Same names as the desktop's `dialog.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;

use crate::lang::tf;
use crate::{BimApp, Status};

/// The largest file the page takes in: what the core opens.
const MAX_PICK: f64 = (256u64 << 20) as f64;
/// How long a download's address is kept before it is let go, ms: some browsers start the download only after the click returns.
const KEEP_DOWNLOAD_MS: i32 = 60_000;

/// What is done with the paths a dialog gives.
pub(crate) type Then = Box<dyn FnOnce(&mut BimApp, Vec<PathBuf>)>;

/// A chooser that is open: the files it put in the page (or why it couldn't), and what to do with their paths.
pub(crate) struct Dialog {
    pub(crate) answer: Receiver<Result<Vec<PathBuf>, String>>,
    pub(crate) then: Then,
    /// Whether its paths are files put in the page, let go once `then` has read them (a save's path is to be written).
    picked: bool,
}

/// `then` for the first path of those given.
fn first(then: impl FnOnce(&mut BimApp, PathBuf) + 'static) -> Then {
    Box::new(|app, paths| {
        if let Some(path) = paths.into_iter().next() {
            then(app, path);
        }
    })
}

/// Where a file named `name` is held in the page.
fn in_page(name: &str) -> PathBuf {
    let name = name.replace(['/', '\\'], "-");
    Path::new("/").join(if name.is_empty() { "file".into() } else { name })
}

impl BimApp {
    /// Asks for a file to open, or names one to save (`save`), and calls `then` with its path, in a later frame for an open.
    pub(crate) fn ask_file(
        &mut self,
        ctx: &Context,
        dialog: rfd::AsyncFileDialog,
        save: bool,
        then: impl FnOnce(&mut BimApp, PathBuf) + 'static,
    ) {
        if save {
            if self.dialog.is_some() {
                return;
            }
            // The browser's handle to save to comes at once, carrying the name the window offered; the browser asks where
            // when the file is downloaded.
            let (tx, answer) = channel();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let path = dialog.save_file().await.map(|f| in_page(&f.file_name()));
                let _ = tx.send(Ok(path.into_iter().collect()));
                ctx.request_repaint();
            });
            self.dialog = Some(Dialog { answer, then: first(then), picked: false });
            return;
        }
        self.begin(ctx, dialog, false, first(then));
    }

    /// Asks for one file or several to open, and calls `then` with their paths in a later frame.
    pub(crate) fn ask_files(
        &mut self,
        ctx: &Context,
        dialog: rfd::AsyncFileDialog,
        then: impl FnOnce(&mut BimApp, Vec<PathBuf>) + 'static,
    ) {
        self.begin(ctx, dialog, true, Box::new(then));
    }

    fn begin(&mut self, ctx: &Context, dialog: rfd::AsyncFileDialog, several: bool, then: Then) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, answer) = channel();
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let picked = if several { dialog.pick_files().await } else { dialog.pick_file().await.map(|f| vec![f]) };
            let mut paths = Vec::new();
            for file in picked.unwrap_or_default() {
                let name = file.file_name();
                if file.inner().size() > MAX_PICK {
                    for path in paths {
                        bimcraft_core::disk::take(path);
                    }
                    let _ = tx.send(Err(tf("{} is too large to open here", &[&name])));
                    ctx.request_repaint();
                    return;
                }
                let path = in_page(&name);
                let put = read(file.inner()).await.and_then(|bytes| bimcraft_core::disk::put(&path, bytes).map_err(|e| e.to_string()));
                if let Err(e) = put {
                    // What was put in before this one won't be read: it goes.
                    for path in paths {
                        bimcraft_core::disk::take(path);
                    }
                    let _ = tx.send(Err(tf("Couldn't open {}: {}", &[&name, &e])));
                    ctx.request_repaint();
                    return;
                }
                paths.push(path);
            }
            let _ = tx.send(Ok(paths));
            ctx.request_repaint();
        });
        self.dialog = Some(Dialog { answer, then, picked: true });
    }

    /// Once a frame: acts on the files a chooser put in the page, then hands the browser what commands wrote.
    pub(crate) fn dialog_answered(&mut self) {
        self.chooser_answered();
        for path in bimcraft_core::disk::written() {
            if let Some(bytes) = bimcraft_core::disk::take(&path) {
                let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
                if let Err(e) = download(&name, &bytes) {
                    self.status = Status { text: tf("Couldn't download {}: {}", &[&name, &e]), error: true };
                }
            }
        }
    }

    fn chooser_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        let answer = match open.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.dialog = None;
                return;
            }
        };
        let Some(Dialog { then, picked, .. }) = self.dialog.take() else { return };
        match answer {
            Ok(paths) if !paths.is_empty() => {
                then(self, paths.clone());
                // The command has read what was picked by now: the page lets it go.
                if picked {
                    for path in paths {
                        bimcraft_core::disk::take(path);
                    }
                }
            }
            Ok(_) => {}
            Err(text) => self.status = Status { text, error: true },
        }
    }
}

/// Hands `bytes` to the browser as a file named `name` to download.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let link: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not a link")?;
    link.set_href(&url);
    link.set_download(name);
    // In the page while it is clicked, which some browsers want of a link they download from.
    let body = document.body().ok_or("no body")?;
    body.append_child(&link).map_err(js)?;
    link.click();
    link.remove();
    let let_go = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(let_go.unchecked_ref(), KEEP_DOWNLOAD_MS).map_err(js)?;
    Ok(())
}

/// The bytes of a file the user picked. The browser's promise fails if it can't be read (a file in the cloud not on this
/// computer, one taken away meanwhile), where rfd's own read would wait for ever.
async fn read(file: &web_sys::File) -> Result<Vec<u8>, String> {
    let buffer = JsFuture::from(file.array_buffer()).await.map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
