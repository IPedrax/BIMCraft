//! Project templates: the `.bimt` files New starts a project from. A template is read and let go (the core's `open` of a `.bimt`),
//! so what it holds comes in as a new, unsaved project, and Save asks where to put it. The templates New lists are kept in a file
//! of their own in the user's configuration folder, beside the recent files, in the order Settings > Templates gives them; New
//! shows the first [`MAX_SHOWN`].

use std::path::{Path, PathBuf};

use bimcraft_core::files;
use bimcraft_i18n::msg;
use egui::{Button, Context, Id as UiId, Layout, RichText, ScrollArea, Ui, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Pending, Status};

/// How many templates New lists. More are kept, and Settings > Templates moves one up to list it.
pub const MAX_SHOWN: usize = 10;
/// How many are kept at all: a list is a few hundred bytes a template, and one past this isn't a person's.
pub const MAX_KEPT: usize = 200;
/// One file over this isn't ours, and is ignored.
const MAX_FILE: u64 = 256 << 10;
/// What the list's file is called.
const FILE: &str = "templates.json";

/// Where the list is kept, given where the favorites are: beside them, in a file of its own.
pub fn beside(favorites: &Path) -> PathBuf {
    favorites.with_file_name(FILE)
}

/// The templates, in the order New lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Templates {
    paths: Vec<PathBuf>,
}

impl Templates {
    /// The list in the file at `path`. Whatever is missing, damaged or not what's expected is left out: a settings file never
    /// stops the app.
    pub fn load(path: &Path) -> Templates {
        let text =
            std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= MAX_FILE).and_then(|_| std::fs::read_to_string(path).ok());
        let j: Value = text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let mut list = Templates::default();
        for p in j.get("files").and_then(Value::as_array).into_iter().flatten() {
            if let Some(p) = p.as_str().filter(|p| !p.is_empty()) {
                list.add(Path::new(p));
            }
        }
        list
    }

    /// Written to `path`, whole or not at all: its folder made if need be.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let files: Vec<_> = self.paths.iter().map(|p| p.to_string_lossy()).collect();
        let text = serde_json::to_string_pretty(&json!({ "files": files })).map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Every template kept, in order.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The ones New lists: the first [`MAX_SHOWN`].
    pub fn shown(&self) -> &[PathBuf] {
        &self.paths[..self.paths.len().min(MAX_SHOWN)]
    }

    /// `path` as it is kept: the same file reached another way (a relative path) is the same entry.
    fn key(path: &Path) -> PathBuf {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// A template just used or saved: where it is if it's known, else first, so New lists it. Whether the list changed.
    pub fn pick(&mut self, path: &Path) -> bool {
        let path = Self::key(path);
        if self.paths.contains(&path) {
            return false;
        }
        self.paths.insert(0, path);
        self.paths.truncate(MAX_KEPT);
        true
    }

    /// A template added in Settings: last, once. Whether it was added (not when known, nor past [`MAX_KEPT`]).
    pub fn add(&mut self, path: &Path) -> bool {
        let path = Self::key(path);
        if self.paths.contains(&path) || self.paths.len() >= MAX_KEPT {
            return false;
        }
        self.paths.push(path);
        true
    }

    pub fn remove(&mut self, path: &Path) {
        let path = Self::key(path);
        self.paths.retain(|p| *p != path);
    }

    /// The template at `index` one place up (`up`) or down. Whether it moved.
    pub fn shift(&mut self, index: usize, up: bool) -> bool {
        let other = if up { index.checked_sub(1) } else { index.checked_add(1) };
        match other.filter(|o| *o < self.paths.len()) {
            Some(o) if index < self.paths.len() => {
                self.paths.swap(index, o);
                true
            }
            _ => false,
        }
    }

    /// What New and Settings show for each: its file name, with its folder for a name two share.
    pub fn labels(&self) -> Vec<String> {
        crate::recent::labels(&self.paths)
    }
}

/// What the New Project popup was asked for, done once it's drawn.
enum Choice {
    Template(PathBuf),
    Empty,
    Browse,
    Cancel,
}

impl BimApp {
    /// Keep the templates in the file at `path`, starting from what it holds. Until the app is told where, it keeps none: a window
    /// made in a test never reads or writes the user's own.
    pub fn keep_templates_in(&mut self, path: PathBuf) {
        self.templates = Templates::load(&path);
        self.templates_path = Some(path);
    }

    /// The templates New lists, in order.
    pub fn templates(&self) -> &Templates {
        &self.templates
    }

    /// `path` is a template just opened or saved: listed from now on.
    pub(crate) fn note_template(&mut self, path: &Path) {
        if self.templates.pick(path) {
            self.keep_templates();
        }
    }

    /// Write the list down. A failure is said, unless the status already says something.
    pub(crate) fn keep_templates(&mut self) {
        let Some(path) = &self.templates_path else { return };
        if let Err(e) = self.templates.save(path)
            && self.status.text.is_empty()
        {
            self.status = Status { text: tf("Couldn't save the setting: {}", &[&e]), error: true };
        }
    }

    /// New Project (File > New > Project, Ctrl+N, the start page): the popup that asks what to start from.
    pub fn new_project(&mut self) {
        self.new_project_popup = true;
    }

    /// A new project from the template at `path`: its content, with no file. One that isn't there any more is said and taken off
    /// the list.
    pub(crate) fn open_template(&mut self, path: &Path) -> bool {
        if !path.is_file() {
            let name = path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
            self.templates.remove(path);
            self.keep_templates();
            self.status = Status { text: tf("The template {} isn't there any more; it was taken off the list.", &[&name]), error: true };
            return false;
        }
        self.open_project_file(path, true)
    }

    /// Save As > Template…: the project as a `.bimt`, listed from then on. The project keeps its own file.
    pub(crate) fn save_as_template(&mut self, ctx: &Context) {
        self.ask_path(ctx, msg!("BIMCraft template"), &[files::TEMPLATE], |app, p| {
            app.save_to(&p);
        });
    }

    /// The New Project popup, while it's up: the templates, an empty project, or another template from the system's dialog.
    pub(crate) fn new_project_window(&mut self, ctx: &Context) {
        if !self.new_project_popup {
            return;
        }
        let mut choice = None;
        let shown: Vec<(String, PathBuf)> =
            self.templates.labels().into_iter().zip(self.templates.paths().iter().cloned()).take(MAX_SHOWN).collect();
        let more = self.templates.paths().len().saturating_sub(MAX_SHOWN);
        let r = Modal::new(UiId::new("new-project"), t("New Project")).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("Start from a template")).strong());
                ui.add_space(4.0);
                if shown.is_empty() {
                    ui.label(RichText::new(t("No templates yet: Browse… picks one, and New lists it from then on.")).color(pal().muted));
                }
                ScrollArea::vertical().id_salt("new-project-templates").max_height(280.0).show(ui, |ui| {
                    for (label, path) in &shown {
                        let button = Button::new(label.as_str()).min_size(vec2(ui.available_width(), 0.0));
                        if ui.add(button).on_hover_text(path.to_string_lossy()).clicked() {
                            choice = Some(Choice::Template(path.clone()));
                        }
                    }
                });
                if more > 0 {
                    let text = tf("{} more in Settings > Templates; move one up to list it here.", &[&more.to_string()]);
                    ui.label(RichText::new(text).small().color(pal().muted));
                }
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("Empty Project")).on_hover_text(t("An empty project to model in.")).clicked() {
                        choice = Some(Choice::Empty);
                    }
                    if ui.button(t("Browse…")).on_hover_text(t("Another template, which New lists from then on.")).clicked() {
                        choice = Some(Choice::Browse);
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.button(t("Cancel")).clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        });
        if choice.is_none() && r.should_close() {
            choice = Some(Choice::Cancel);
        }
        let Some(choice) = choice else { return };
        self.new_project_popup = false;
        match choice {
            Choice::Template(path) => self.guard(Pending::Template(path), ctx),
            Choice::Empty => self.guard(Pending::New, ctx),
            Choice::Browse => {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft template"), &[files::TEMPLATE]);
                let later = ctx.clone();
                self.ask_file(ctx, dialog, false, move |app, path| app.guard(Pending::Template(path), &later));
            }
            Choice::Cancel => {}
        }
    }

    /// Settings > Templates: every template kept, in the order New lists them, to add to, take from and put in order.
    pub(crate) fn templates_page(&mut self, ui: &mut Ui, ctx: &Context) {
        crate::settings::heading(ui, msg!("Project Templates"));
        ui.label(
            RichText::new(tf(
                "New lists the first {} of these; move one up to list it. A template is a .bimt file: Save As > Template… makes one.",
                &[&MAX_SHOWN.to_string()],
            ))
            .color(pal().muted),
        );
        ui.add_space(6.0);
        let labels = self.templates.labels();
        let paths = self.templates.paths().to_vec();
        let picked = self.template_picked.filter(|i| *i < paths.len());
        if paths.is_empty() {
            ui.label(RichText::new(t("No templates yet.")).color(pal().muted));
        }
        let mut pick = picked;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ScrollArea::vertical().id_salt("templates-list").max_height(250.0).show(ui, |ui| {
                for (i, (label, path)) in labels.iter().zip(&paths).enumerate() {
                    if i == MAX_SHOWN {
                        ui.separator();
                        ui.label(RichText::new(t("Not listed by New:")).small().color(pal().muted));
                    }
                    let gone = !path.is_file();
                    let text = if gone { tf("{} (not found)", &[label]) } else { label.clone() };
                    if ui.selectable_label(picked == Some(i), text).on_hover_text(path.to_string_lossy()).clicked() {
                        pick = Some(i);
                    }
                }
            });
        });
        self.template_picked = pick;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button(t("Add…")).clicked() {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft template"), &[files::TEMPLATE]);
                self.ask_file(ctx, dialog, false, |app, path| {
                    if app.templates.add(&path) {
                        app.template_picked = Some(app.templates.paths().len().saturating_sub(1));
                        app.keep_templates();
                    }
                });
            }
            let at = self.template_picked;
            if ui.add_enabled(at.is_some(), Button::new(t("Remove"))).clicked()
                && let Some(path) = at.and_then(|i| self.templates.paths().get(i).cloned())
            {
                self.templates.remove(&path);
                self.template_picked = None;
                self.keep_templates();
            }
            let last = self.templates.paths().len().saturating_sub(1);
            for (label, up, can) in
                [(msg!("Move Up"), true, at.is_some_and(|i| i > 0)), (msg!("Move Down"), false, at.is_some_and(|i| i < last))]
            {
                if ui.add_enabled(can, Button::new(t(label))).clicked()
                    && let Some(i) = at
                    && self.templates.shift(i, up)
                {
                    self.template_picked = Some(if up { i - 1 } else { i + 1 });
                    self.keep_templates();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(path: &str) -> PathBuf {
        std::path::absolute(path).unwrap()
    }

    #[test]
    fn a_template_picked_comes_first_once_and_one_added_goes_last() {
        let mut list = Templates::default();
        assert!(list.add(Path::new("/t/a.bimt")) && list.add(Path::new("/t/b.bimt")));
        assert!(!list.add(Path::new("/t/a.bimt")), "once");
        assert!(list.pick(Path::new("/t/c.bimt")));
        assert_eq!(list.paths(), [abs("/t/c.bimt"), abs("/t/a.bimt"), abs("/t/b.bimt")]);
        // A template already listed stays where it was put.
        assert!(!list.pick(Path::new("/t/b.bimt")));
        assert_eq!(list.paths()[2], abs("/t/b.bimt"));
        assert!(list.shift(2, true));
        assert_eq!(list.paths(), [abs("/t/c.bimt"), abs("/t/b.bimt"), abs("/t/a.bimt")]);
        assert!(list.shift(0, false));
        assert_eq!(list.paths(), [abs("/t/b.bimt"), abs("/t/c.bimt"), abs("/t/a.bimt")]);
        assert!(!list.shift(0, true) && !list.shift(2, false) && !list.shift(7, true), "no further than the ends");
        list.remove(Path::new("/t/c.bimt"));
        assert_eq!(list.paths(), [abs("/t/b.bimt"), abs("/t/a.bimt")]);
    }

    #[test]
    fn new_lists_the_first_ten_and_more_are_kept() {
        let mut list = Templates::default();
        for n in 0..MAX_SHOWN + 2 {
            list.add(Path::new(&format!("/t/t{n}.bimt")));
        }
        assert_eq!(list.paths().len(), MAX_SHOWN + 2);
        assert_eq!(list.shown().len(), MAX_SHOWN);
        assert_eq!(list.shown()[0], abs("/t/t0.bimt"));
        for n in 0..MAX_KEPT {
            list.add(Path::new(&format!("/u/u{n}.bimt")));
        }
        assert_eq!(list.paths().len(), MAX_KEPT, "kept to so many");
        list.pick(Path::new("/v/new.bimt"));
        assert_eq!((list.paths().len(), list.paths()[0].clone()), (MAX_KEPT, abs("/v/new.bimt")), "a pick still comes in first");
    }

    #[test]
    fn the_list_is_kept_in_a_file_in_order_and_a_bad_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let file = beside(&dir.path().join("favorites.json"));
        assert_eq!(file, dir.path().join("templates.json"));
        let mut list = Templates::default();
        for p in ["/t/b.bimt", "/t/a.bimt", "/s/a.bimt"] {
            list.add(Path::new(p));
        }
        list.save(&file).unwrap();
        let read = Templates::load(&file);
        assert_eq!(read, list);
        assert_eq!(read.labels(), ["b.bimt", "a.bimt (t)", "a.bimt (s)"]);
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(Templates::load(&file).paths().is_empty());
        std::fs::write(&file, r#"{"files": ["", 7, "/t/a.bimt", "/t/a.bimt"]}"#).unwrap();
        assert_eq!(Templates::load(&file).paths(), [abs("/t/a.bimt")], "odd entries and repeats are dropped");
        std::fs::write(&file, vec![b' '; MAX_FILE as usize + 1]).unwrap();
        assert!(Templates::load(&file).paths().is_empty(), "too large to be ours");
        assert!(Templates::load(&dir.path().join("missing.json")).paths().is_empty());
    }
}
