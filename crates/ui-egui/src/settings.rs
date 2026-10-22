//! File > Settings (Ctrl+,): the window's own settings, the user's and never a project's. They live in
//! `settings.json` beside the favorites and the update settings ([`beside`]), which the desktop's `main` hands the app
//! ([`BimApp::use_settings`]); a window that wasn't given the file (every test's) keeps them for as long as it's open.
//! What it keeps: the theme, interface language, ribbon, palettes, and the background autosave interval.
//! The pages for what is still to come (performance and graphics) show what they will hold, greyed out.

use std::path::{Path, PathBuf};

use bimcraft_core::language::Language;
use egui::{Align, ComboBox, Context, Id as UiId, Layout, Modal, RichText, ScrollArea, ThemePreference, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::dock::{Docking, Palette};
use crate::lang::t;
use crate::style::pal;
use crate::updates::failed;

/// The settings file's name, in the folder of the favorites.
pub const FILE: &str = "settings.json";
/// The most of it that's read: it's a few hundred bytes.
const MAX_FILE: u64 = 64 * 1024;

/// The settings file beside the favorites file at `favorites`.
pub fn beside(favorites: &Path) -> PathBuf {
    favorites.with_file_name(FILE)
}

/// The interface's language the settings file at `path` keeps, if it keeps one: a blank project starts in it.
pub fn language_in(path: &Path) -> Option<Language> {
    read(path)?["language"].as_str().and_then(Language::from_code)
}

/// What the settings file keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemePreference,
    pub language: Language,
    pub ribbon_minimized: bool,
    pub docking: Docking,
    pub autosave_minutes: u32,
}

impl Settings {
    pub fn to_json(&self) -> Value {
        let theme = match self.theme {
            ThemePreference::Dark => "dark",
            ThemePreference::Light => "light",
            ThemePreference::System => "system",
        };
        json!({
            "theme": theme,
            "language": self.language.code(),
            "ribbon_minimized": self.ribbon_minimized,
            "palettes": self.docking.to_json(),
            "autosave_minutes": self.autosave_minutes,
        })
    }

    /// Reads back what [`to_json`](Self::to_json) wrote over `base`: whatever is missing or not understood keeps
    /// `base`'s.
    pub fn from_json(v: &Value, base: Settings) -> Settings {
        let theme = match v["theme"].as_str() {
            Some("dark") => ThemePreference::Dark,
            Some("light") => ThemePreference::Light,
            Some("system") => ThemePreference::System,
            _ => base.theme,
        };
        Settings {
            theme,
            language: v["language"].as_str().and_then(Language::from_code).unwrap_or(base.language),
            ribbon_minimized: v["ribbon_minimized"].as_bool().unwrap_or(base.ribbon_minimized),
            docking: if v["palettes"].is_object() { Docking::from_json(&v["palettes"]) } else { base.docking },
            autosave_minutes: v["autosave_minutes"].as_u64().filter(|n| *n <= 120).map_or(base.autosave_minutes, |n| n as u32),
        }
    }
}

/// The settings file as JSON: none if it isn't there, is too big or isn't JSON.
fn read(path: &Path) -> Option<Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_FILE {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// Writes the settings file, its folder made if need be, through a file beside it so a crash never leaves half of one.
fn write(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&settings.to_json()).map_err(|e| e.to_string())?;
    let staged = path.with_extension("json.new");
    std::fs::write(&staged, text).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, path).map_err(|e| e.to_string())
}

/// A page of the Settings window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Interface,
    Performance,
    Graphics,
    Files,
    Updates,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::Interface, Page::Performance, Page::Graphics, Page::Files, Page::Updates];

    pub fn name(self) -> &'static str {
        match self {
            Page::Interface => "User Interface",
            Page::Performance => "Performance",
            Page::Graphics => "Graphics",
            Page::Files => "Files",
            Page::Updates => "Updates",
        }
    }
}

impl BimApp {
    /// Keeps the settings in `path`: what it holds now, and from now on what changes.
    pub fn use_settings(&mut self, path: PathBuf) {
        let now = self.settings_now(ThemePreference::System);
        let kept = read(&path).map_or(now.clone(), |v| Settings::from_json(&v, now));
        (self.lang, self.ribbon_min, self.docking) = (kept.language, kept.ribbon_minimized, kept.docking.clone());
        self.theme_wanted = Some(kept.theme);
        self.autosave.interval_minutes = kept.autosave_minutes;
        self.settings_kept = Some(kept);
        self.settings_path = Some(path);
    }

    fn settings_now(&self, theme: ThemePreference) -> Settings {
        Settings {
            theme,
            language: self.lang,
            ribbon_minimized: self.ribbon_min,
            docking: self.docking.clone(),
            autosave_minutes: self.autosave.interval_minutes,
        }
    }

    /// Writes the settings file when what it keeps has changed, between drags: once a frame.
    pub(crate) fn keep_settings(&mut self, ctx: &Context) {
        let Some(path) = self.settings_path.clone() else { return };
        if self.dragging.is_some() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let now = self.settings_now(ctx.options(|o| o.theme_preference));
        if self.settings_kept.as_ref() == Some(&now) {
            return;
        }
        if let Err(e) = write(&path, &now) {
            self.status = failed(crate::lang::tf("Couldn't save the setting: {}", &[&e]));
        }
        self.settings_kept = Some(now);
    }

    /// The Settings window, while it's open: its pages down the left, the one picked beside them.
    pub(crate) fn settings_window(&mut self, ctx: &Context) {
        let Some(mut page) = self.settings_page else { return };
        let mut close = false;
        let r = Modal::new(UiId::new("settings")).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.label(RichText::new(t("Settings")).heading());
            ui.add_space(6.0);
            // A fixed size, so the window doesn't grow or move under the pointer from one frame or page to the next.
            ui.allocate_ui_with_layout(vec2(660.0, 380.0), Layout::left_to_right(Align::Min), |ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    for p in Page::ALL {
                        if ui.selectable_label(page == p, t(p.name())).clicked() {
                            page = p;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ScrollArea::vertical().id_salt("settings-page").auto_shrink([false, false]).show(ui, |ui| match page {
                        Page::Interface => self.interface_page(ui, ctx),
                        Page::Performance => performance_page(ui),
                        Page::Graphics => graphics_page(ui),
                        Page::Files => self.files_page(ui),
                        Page::Updates => self.updates_page(ui),
                    });
                });
            });
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        self.settings_page = if close || r.should_close() { None } else { Some(page) };
    }

    fn interface_page(&mut self, ui: &mut Ui, ctx: &Context) {
        heading(ui, "Theme");
        let mut theme = ctx.options(|o| o.theme_preference);
        let before = theme;
        ui.horizontal(|ui| {
            ui.radio_value(&mut theme, ThemePreference::System, t("As the System"));
            ui.radio_value(&mut theme, ThemePreference::Light, t("Light"));
            ui.radio_value(&mut theme, ThemePreference::Dark, t("Dark"));
        });
        if theme != before {
            ctx.set_theme(theme);
        }
        heading(ui, "Language");
        ComboBox::from_id_salt("settings-language").selected_text(self.lang.label()).width(200.0).show_ui(ui, |ui| {
            for l in Language::ALL {
                ui.selectable_value(&mut self.lang, l, l.label());
            }
        });
        heading(ui, "Ribbon");
        ui.checkbox(&mut self.ribbon_min, t("Minimize the ribbon to its tabs"))
            .on_hover_text(t("Double-clicking a tab's name does the same; a click on a tab then shows its tools until one is picked"));
        heading(ui, "Palettes");
        for p in Palette::ALL {
            let mut on = self.docking.shown(p);
            if ui.checkbox(&mut on, t(p.name())).changed() {
                self.show_palette(p, on);
            }
        }
        if ui.button(t("Reset Palette Layout")).on_hover_text(t("Properties over the Project Browser, on the left, as at first")).clicked()
        {
            self.docking = Docking::default();
        }
        later(ui, |ui| {
            let (mut scale, mut keys, mut reopen, mut views) = (100.0, true, false, false);
            ui.horizontal(|ui| {
                ui.label(t("Interface scale"));
                ui.add(egui::Slider::new(&mut scale, 75.0..=200.0).suffix(" %"));
            });
            ui.checkbox(&mut keys, t("Show shortcut keys in tooltips"));
            ui.checkbox(&mut reopen, t("Open the last project at startup"));
            ui.checkbox(&mut views, t("Reopen the views that were open"));
        });
    }

    fn updates_page(&mut self, ui: &mut Ui) {
        if self.updates.is_some() {
            self.update_menu(ui);
        } else {
            ui.label(RichText::new(t("This copy doesn't look for updates.")).color(pal().muted));
        }
    }
}

fn performance_page(ui: &mut Ui) {
    later(ui, |ui| {
        let (mut undo, mut quality, mut smooth, mut background, mut gpu) = (100, 0, true, false, false);
        ui.horizontal(|ui| {
            ui.label(t("Undo steps kept"));
            ui.add(egui::DragValue::new(&mut undo).range(10..=1000));
        });
        ui.horizontal(|ui| {
            ui.label(t("3D view resolution"));
            ComboBox::from_id_salt("settings-3d")
                .selected_text(t("Full"))
                .show_index(ui, &mut quality, 2, |i| if i == 0 { t("Full") } else { t("Half") }.to_string());
        });
        ui.checkbox(&mut smooth, t("Smooth lines (anti-aliasing)"));
        ui.checkbox(&mut background, t("Redraw views in the background"));
        ui.checkbox(&mut gpu, t("Draw the 3D view on the graphics card"));
    });
}

fn graphics_page(ui: &mut Ui) {
    later(ui, |ui| {
        let (mut color, mut hover, mut ground) = (pal().accent, true, 0);
        ui.horizontal(|ui| {
            ui.label(t("Selection colour"));
            ui.color_edit_button_srgba(&mut color);
        });
        ui.checkbox(&mut hover, t("Highlight what's under the cursor"));
        ui.horizontal(|ui| {
            ui.label(t("Background of the 3D view"));
            ComboBox::from_id_salt("settings-ground")
                .selected_text(t("Plain"))
                .show_index(ui, &mut ground, 2, |i| if i == 0 { t("Plain") } else { t("Sky") }.to_string());
        });
    });
}

impl BimApp {
    fn files_page(&mut self, ui: &mut Ui) {
        heading(ui, "Autosave");
        let mut enabled = self.autosave.interval_minutes > 0;
        if ui.checkbox(&mut enabled, t("Autosave in the background")).changed() {
            self.autosave.interval_minutes = if enabled { 2 } else { 0 };
        }
        if enabled {
            ui.horizontal(|ui| {
                ui.label(t("Save a recovery copy every"));
                if ui.add(egui::DragValue::new(&mut self.autosave.interval_minutes).range(1..=120).suffix(" min")).changed() {
                    self.autosave.next_due =
                        std::time::Instant::now() + std::time::Duration::from_secs(u64::from(self.autosave.interval_minutes) * 60);
                }
            });
        }
        ui.label(t("Recovery copies are kept beside saved projects; untitled projects use the temporary folder."));
        later(ui, |ui| {
            let mut folder = String::new();
            ui.horizontal(|ui| {
                ui.label(t("Folder for new projects"));
                ui.add(egui::TextEdit::singleline(&mut folder).desired_width(220.0));
            });
        });
    }
}

/// A group's heading on a page.
fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(t(text)).strong());
}

/// Settings still to come: shown as they will be, greyed out.
fn later(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    ui.label(RichText::new(t("Coming in a later version")).color(pal().muted).italics());
    ui.add_space(2.0);
    ui.add_enabled_ui(false, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        add(ui);
    });
}
