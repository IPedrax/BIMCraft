//! Family documents as tabs beside the project's, as Revit opens a family: any number open at once, each with its own draft,
//! history, selection and tool, and each of its views a tab of the window's (`Sheet::Family`), grouped after the project's and
//! titled "{family} - {view}". The project stays the one `Session`; the active tab says which document the ribbon, Properties,
//! the Project Browser, the title, the status bar and Undo are for (`BimApp::active_family`).
//!
//! A frame works on the active document by taking it out of its place and putting it back (`take_family`, `put_family`). What
//! finishes later than it started (a file dialog answered, "Family Already Exists" answered) holds the `DocId` of the document
//! that started it and finishes there (`family_of`, `take_family_of`), whichever tab is active by then; nothing is done to
//! another document when that one was closed meanwhile. A
//! document is closed with its last tab, or by Close Family: one with unsaved changes asks first ("Save changes to {family}?":
//! Load into Project and Close, Close Without Loading, Cancel), and one with a nested family's document open over it closes that
//! one first, asking the same. File > New, Open, Close and quitting ask for each unsaved document before the project's own
//! question. A family is never open twice: Edit Family on one whose document is open, or opening a file that is open, brings
//! that document's last tab forward.
use super::*;
use crate::{Pending, Sheet};
use std::path::PathBuf;

/// Which family document a tab is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub(crate) u32);

/// An open family document.
pub(crate) struct FamilyDoc {
    pub(crate) id: DocId,
    /// The document; none while the frame works on it.
    pub(crate) ed: Option<FamilyEditor>,
    /// The document it was opened from by Edit Family on a nested family, which Load into Family loads into.
    pub(crate) host: Option<DocId>,
    /// The tab it was opened from, which Load into Project and Close goes back to.
    pub(crate) back_to: Sheet,
    /// The file it was opened from, if it was.
    pub(crate) path: Option<PathBuf>,
}

/// A document being closed: asked about first, while it has unsaved changes, and what follows once it is closed.
pub(crate) struct Closing {
    pub(crate) doc: DocId,
    pub(crate) then: Then,
    /// Load into Project and Close was chosen: waiting for the load to close it.
    loading: bool,
}

/// What follows a document closed.
pub(crate) enum Then {
    Nothing,
    /// Another document to close, the one the closed one was nested in.
    Close(DocId, Box<Then>),
    /// File > New, Open, Close or quitting, which then asks about the project.
    Pending(Pending),
}

impl BimApp {
    /// The document the active tab is of.
    pub(crate) fn active_doc(&self) -> Option<DocId> {
        match self.sheet {
            Sheet::Family(id, _) => self.families.iter().any(|d| d.id == id).then_some(id),
            _ => None,
        }
    }

    pub(crate) fn doc(&self, id: DocId) -> Option<&FamilyDoc> {
        self.families.iter().find(|d| d.id == id)
    }

    fn doc_mut(&mut self, id: DocId) -> Option<&mut FamilyDoc> {
        self.families.iter_mut().find(|d| d.id == id)
    }

    /// The family document the active tab is of: what the ribbon, Properties, the Project Browser and Undo are for.
    pub(crate) fn active_family(&self) -> Option<&FamilyEditor> {
        self.doc(self.active_doc()?)?.ed.as_ref()
    }

    pub(crate) fn active_family_mut(&mut self) -> Option<&mut FamilyEditor> {
        let id = self.active_doc()?;
        self.doc_mut(id)?.ed.as_mut()
    }

    /// The document `id`'s draft, whichever tab is active.
    pub(crate) fn family_of(&self, id: DocId) -> Option<&FamilyEditor> {
        self.doc(id)?.ed.as_ref()
    }

    pub(crate) fn family_of_mut(&mut self, id: DocId) -> Option<&mut FamilyEditor> {
        self.doc_mut(id)?.ed.as_mut()
    }

    /// The active document taken out of its place for the frame to work on, as `put_family` puts it back.
    pub(crate) fn take_family(&mut self) -> Option<FamilyEditor> {
        let id = self.active_doc()?;
        self.take_family_of(id)
    }

    /// Document `id` taken out of its place, whichever tab is active, as `put_family_of` puts it back: for what finishes
    /// later than it started (a file dialog answered, a question answered), which finishes in the document that started it.
    /// None when that document was closed meanwhile.
    pub(crate) fn take_family_of(&mut self, id: DocId) -> Option<FamilyEditor> {
        self.doc_mut(id)?.ed.take()
    }

    /// Document `id`, taken out by `take_family_of`, back in its place.
    pub(crate) fn put_family_of(&mut self, id: DocId, ed: FamilyEditor) {
        if let Some(d) = self.doc_mut(id).filter(|d| d.ed.is_none()) {
            d.ed = Some(ed);
        }
    }

    /// A document taken out, back in its place.
    pub(crate) fn put_family(&mut self, ed: FamilyEditor) {
        if let Some(d) = self.families.iter_mut().find(|d| d.ed.is_none()) {
            d.ed = Some(ed);
        }
    }

    /// The document's tabs, in order.
    pub(crate) fn doc_tabs(&self, id: DocId) -> Vec<Sheet> {
        self.views.iter().copied().filter(|s| matches!(s, Sheet::Family(d, _) if *d == id)).collect()
    }

    /// `ed` open as a document of its own, opened from file `path` (none: from the project, or new), its view's tab brought
    /// forward after the project's tabs and the other documents'.
    pub(crate) fn open_family_document(&mut self, ed: FamilyEditor, path: Option<PathBuf>) -> DocId {
        let id = DocId(self.next_doc);
        self.next_doc = self.next_doc.saturating_add(1);
        let back_to = match self.sheet {
            Sheet::Family(d, _) => self.doc(d).map_or(self.front, |d| d.back_to),
            s => s,
        };
        let view = ed.views.shown;
        self.families.push(FamilyDoc { id, ed: Some(ed), host: None, back_to, path });
        self.open_sheet(Sheet::Family(id, view));
        id
    }

    /// Document `id`'s last tab brought forward.
    pub(crate) fn activate_doc(&mut self, id: DocId) {
        let last = self.doc_tabs(id).last().copied();
        let shown = self.family_of(id).map(|ed| ed.views.shown);
        if let Some(sheet) = last.or_else(|| shown.map(|v| Sheet::Family(id, v))) {
            self.open_sheet(sheet);
        }
    }

    /// The document whose family is the project's `name`, opened from it; none open.
    pub(crate) fn doc_of_family(&self, name: &str) -> Option<DocId> {
        self.families
            .iter()
            .find(|d| d.host.is_none() && d.ed.as_ref().is_some_and(|ed| ed.original.as_deref() == Some(name)))
            .map(|d| d.id)
    }

    /// The document opened from file `path`; none open.
    pub(crate) fn doc_of_file(&self, path: &std::path::Path) -> Option<DocId> {
        self.families.iter().find(|d| d.path.as_deref() == Some(path)).map(|d| d.id)
    }

    /// The name a new blank family takes: "Family1", "Family2" and so on, the first that neither an open document nor the
    /// project has.
    pub(crate) fn next_family_name(&self) -> String {
        let taken = |n: &str| {
            self.session.model.families.contains_key(n) || self.families.iter().any(|d| d.ed.as_ref().is_some_and(|ed| ed.title() == n))
        };
        (1..).map(|k| format!("Family{k}")).find(|n| !taken(n)).unwrap_or_else(|| "Family".into())
    }

    /// The view the active family tab shows, made the document's own, and the other way round: a view the document went to
    /// (its Project Browser, Go To View) opens its tab.
    pub(crate) fn sync_family_tab(&mut self) {
        let Sheet::Family(id, view) = self.sheet else { return };
        let Some(shown) = self.family_of(id).map(|ed| ed.views.shown) else { return };
        if shown != view {
            self.open_sheet(Sheet::Family(id, shown));
        }
    }

    /// The tab of family view `sheet` closed: the document's last asks first if it has unsaved changes, and closes the document.
    pub(crate) fn close_family_tab(&mut self, sheet: Sheet) {
        let Sheet::Family(id, _) = sheet else { return };
        if self.doc_tabs(id).len() > 1 {
            self.views.retain(|v| *v != sheet);
            if self.sheet == sheet
                && let Some(last) = self.doc_tabs(id).last().copied()
            {
                self.open_sheet(last);
            }
            return;
        }
        self.ask_close_family(id, Then::Nothing);
    }

    /// Document `id` closed, asking first if it has unsaved changes; a nested family's document open over it closed first. Then
    /// `then`.
    pub(crate) fn ask_close_family(&mut self, id: DocId, then: Then) {
        if let Some(child) = self.families.iter().find(|d| d.host == Some(id)).map(|d| d.id) {
            self.ask_close_family(child, Then::Close(id, Box::new(then)));
            return;
        }
        if self.family_of(id).is_some_and(FamilyEditor::unsaved) {
            self.activate_doc(id);
            self.doc_closing = Some(Closing { doc: id, then, loading: false });
        } else {
            self.close_family_document(id);
            self.after_close(then);
        }
    }

    fn after_close(&mut self, then: Then) {
        match then {
            Then::Nothing => {}
            Then::Close(id, then) => self.ask_close_family(id, *then),
            Then::Pending(p) => self.pending_after_documents(p),
        }
    }

    /// Document `id` closed without a question: its tabs gone, and the tab it was opened from (its host's, for a nested
    /// family's) forward if it was the active one, else the project's last.
    pub(crate) fn close_family_document(&mut self, id: DocId) {
        let Some(at) = self.families.iter().position(|d| d.id == id) else { return };
        let doc = self.families.remove(at);
        self.views.retain(|v| !matches!(v, Sheet::Family(d, _) if *d == id));
        if matches!(self.sheet, Sheet::Family(d, _) if d == id) {
            let host = doc.host.filter(|h| self.doc(*h).is_some());
            match host {
                Some(h) => self.activate_doc(h),
                None => {
                    let back =
                        Some(doc.back_to).filter(|s| self.views.contains(s) || matches!(s, Sheet::Family(d, _) if self.doc(*d).is_some()));
                    let last = self.views.iter().rev().copied().find(|s| !matches!(s, Sheet::Family(..)));
                    if let Some(sheet) = back.or(last).or_else(|| self.views.last().copied()) {
                        self.open_sheet(sheet);
                    } else {
                        self.sheet = self.front;
                    }
                }
            }
        }
        if self.families.is_empty() {
            self.family_closed_from_home();
        }
    }

    /// File > New, Open, Close or quitting asked for: each family document is closed first, those with unsaved changes asked
    /// about, then the project is (`after_documents`).
    pub(crate) fn pending_after_documents(&mut self, p: Pending) {
        // The last opened first: a nested family's document before the one it was opened from.
        match self.families.last().map(|d| d.id) {
            Some(id) => self.ask_close_family(id, Then::Pending(p)),
            None => self.after_documents = Some(p),
        }
    }

    /// "Save changes to {family}?", while a document with unsaved changes is being closed: Load into Project (or Family) and
    /// Close, Close Without Loading, or Cancel.
    pub(crate) fn closing_window(&mut self, ctx: &Context) {
        let Some(closing) = self.doc_closing.as_mut() else { return };
        let id = closing.doc;
        // Load into Project and Close chosen: closed once the load went in; still open when it was refused (it says why).
        if closing.loading {
            let waiting = self.family_loading.is_some() || self.family_of(id).is_some_and(|ed| ed.ask.ok);
            if self.doc(id).is_none() {
                if let Some(closing) = self.doc_closing.take() {
                    self.after_close(closing.then);
                }
            } else if !waiting {
                self.doc_closing = None;
            }
            return;
        }
        let Some(ed) = self.family_of(id) else {
            self.doc_closing = None;
            return;
        };
        let name = ed.title().to_string();
        let nested = ed.into_host;
        #[derive(PartialEq)]
        enum Choice {
            Load,
            Discard,
            Cancel,
        }
        let mut choice = None;
        let load = if nested { msg!("Load into Family and Close") } else { msg!("Load into Project and Close") };
        let r = Modal::new(UiId::new("family-unsaved"), tf("Save changes to {}?", &[&name])).width(620.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t("The family has changes that are not loaded. If you close it without loading, they are lost."));
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, c) in [(load, Choice::Load), (msg!("Close Without Loading"), Choice::Discard), ("Cancel", Choice::Cancel)] {
                        if ui.button(t(label)).clicked() {
                            choice = Some(c);
                        }
                    }
                });
            });
        });
        match choice {
            Some(Choice::Load) => {
                self.activate_doc(id);
                if let Some(ed) = self.family_of_mut(id) {
                    ed.ask.ok = true;
                }
                if let Some(c) = self.doc_closing.as_mut() {
                    c.loading = true;
                }
            }
            Some(Choice::Discard) => {
                if let Some(closing) = self.doc_closing.take() {
                    self.close_family_document(id);
                    self.after_close(closing.then);
                }
            }
            Some(Choice::Cancel) => self.doc_closing = None,
            None if r.should_close() => self.doc_closing = None,
            None => {}
        }
    }

    /// The tab's title of family view `sheet`: "{family} - {view}".
    pub(crate) fn family_tab_name(&self, sheet: Sheet) -> String {
        let Sheet::Family(id, view) = sheet else { return String::new() };
        let family = self.family_of(id).map(|ed| ed.title().to_string()).unwrap_or_default();
        format!("{family} - {}", super::views::view_tab(view))
    }
}

/// The active tab's document among `families`, `sheet` the active tab: for a frame that borrows other fields of the window
/// meanwhile.
pub(crate) fn active_in(families: &mut [FamilyDoc], sheet: Sheet) -> Option<&mut FamilyEditor> {
    let Sheet::Family(id, _) = sheet else { return None };
    families.iter_mut().find(|d| d.id == id)?.ed.as_mut()
}
