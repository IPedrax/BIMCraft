//! File > Settings (Ctrl+,): the window's own settings, the user's and never a project's. They live in
//! `settings.json` beside the favorites and the update settings ([`beside`]), which the desktop's `main` hands the app
//! ([`BimApp::use_settings`]); a window that wasn't given the file (every test's) keeps them for as long as it's open.
//! What it keeps: the theme and whether the drawing stays light in a dark one, the layout, interface language, ribbon, palettes, the
//! window's size, the background autosave interval, and the colour High Contrast Point Clouds draws scans in.
//! The pages for what is still to come (performance and graphics) show what they will hold, greyed out.

use std::path::{Path, PathBuf};

use egui::{Align, ComboBox, Context, Id as UiId, Layout, Modal, RichText, ScrollArea, ThemePreference, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::dock::{Docking, Palette};
use crate::lang::{Lang, t};
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
pub fn language_in(path: &Path) -> Option<Lang> {
    read(path)?["language"].as_str().and_then(Lang::from_code)
}

/// How the window was left: its size in points and whether it was maximized.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
}

impl Window {
    /// The sizes believed: a window is never smaller than this, nor larger than the biggest screens.
    const SIZES: std::ops::RangeInclusive<f32> = 300.0..=16_000.0;

    fn to_json(self) -> Value {
        match self.size {
            Some([width, height]) => json!({"width": width, "height": height, "maximized": self.maximized}),
            None => json!({"maximized": self.maximized}),
        }
    }

    /// Reads back what [`to_json`](Self::to_json) wrote over `base`: whatever is missing, not a number or not a size a window can
    /// be keeps `base`'s.
    fn from_json(v: &Value, base: Window) -> Window {
        let side = |key: &str| v[key].as_f64().map(|n| n as f32).filter(|n| Self::SIZES.contains(n));
        Window {
            size: side("width").zip(side("height")).map(|(w, h)| [w, h]).or(base.size),
            maximized: v["maximized"].as_bool().unwrap_or(base.maximized),
        }
    }
}

/// Whether the settings file at `path` asks for the system's title bar, as `use_settings` reads it: `main` calls that before the
/// window opens, and builds the window from `BimApp::system_title_bar`. This reads the file alone, for the tests.
pub fn system_title_bar_in(path: &Path) -> bool {
    read(path).and_then(|v| v["system_title_bar"].as_bool()).unwrap_or(false)
}

/// The window the settings file at `path` keeps, if it keeps one: the app starts at its size.
pub fn window_in(path: &Path) -> Option<Window> {
    Some(Window::from_json(&read(path)?["window"], Window::default()))
}

/// What the settings file keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemePreference,
    /// Keep the drawing light inside a dark interface. Only the dark theme has the choice: `from_json` drops it from any other.
    pub light_canvas: bool,
    /// The Compact layout: the bars and palettes flush to the window's edges, not floating cards.
    pub compact: bool,
    pub language: Lang,
    pub ribbon_minimized: bool,
    pub docking: Docking,
    pub autosave_minutes: u32,
    /// Draw the 3D view on the graphics card, when there is one.
    pub gpu_3d: bool,
    pub window: Window,
    /// What High Contrast Point Clouds fills a view's scans with.
    pub high_contrast: [u8; 3],
    /// The system's title bar rather than the window's own, where the window can draw its own.
    pub system_title_bar: bool,
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
            "light_canvas": self.light_canvas,
            "layout": if self.compact { "compact" } else { "floating" },
            "language": self.language.code(),
            "ribbon_minimized": self.ribbon_minimized,
            "palettes": self.docking.to_json(),
            "autosave_minutes": self.autosave_minutes,
            "gpu_3d": self.gpu_3d,
            "window": self.window.to_json(),
            "high_contrast": self.high_contrast,
            "system_title_bar": self.system_title_bar,
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
            light_canvas: theme == ThemePreference::Dark && v["light_canvas"].as_bool().unwrap_or(base.light_canvas),
            compact: match v["layout"].as_str() {
                Some("compact") => true,
                Some("floating") => false,
                _ => base.compact,
            },
            language: v["language"].as_str().and_then(Lang::from_code).unwrap_or(base.language),
            ribbon_minimized: v["ribbon_minimized"].as_bool().unwrap_or(base.ribbon_minimized),
            docking: if v["palettes"].is_object() { Docking::from_json(&v["palettes"]) } else { base.docking },
            autosave_minutes: v["autosave_minutes"].as_u64().filter(|n| *n <= 120).map_or(base.autosave_minutes, |n| n as u32),
            gpu_3d: v["gpu_3d"].as_bool().unwrap_or(base.gpu_3d),
            window: Window::from_json(&v["window"], base.window),
            high_contrast: colour(&v["high_contrast"]).unwrap_or(base.high_contrast),
            system_title_bar: v["system_title_bar"].as_bool().unwrap_or(base.system_title_bar),
        }
    }
}

/// `[r, g, b]`, each 0 to 255: a colour; anything else, none.
fn colour(v: &Value) -> Option<[u8; 3]> {
    let parts: Vec<u8> = v.as_array()?.iter().map(|c| c.as_u64().and_then(|c| u8::try_from(c).ok())).collect::<Option<_>>()?;
    parts.try_into().ok()
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
    /// Puts the interface in `lang`. Chinese needs a font with its glyphs: where the system has none the language stays as it was, and
    /// the status says why.
    pub(crate) fn set_lang(&mut self, ctx: &Context, lang: Lang) {
        if lang == Lang::Chinese && !crate::lang::use_chinese_font(ctx, self.font_finder) {
            self.status = failed(t(crate::lang::NO_CHINESE_FONT).to_string());
        } else {
            self.lang = lang;
        }
    }

    /// Keeps the settings in `path`: what it holds now, and from now on what changes.
    pub fn use_settings(&mut self, path: PathBuf) {
        let now = self.settings_now(ThemePreference::System);
        let kept = read(&path).map_or(now.clone(), |v| Settings::from_json(&v, now));
        (self.lang, self.ribbon_min, self.docking) = (kept.language, kept.ribbon_minimized, kept.docking.clone());
        self.compact = kept.compact;
        self.theme_wanted = Some(kept.theme);
        self.light_canvas = kept.light_canvas;
        self.autosave.interval_minutes = kept.autosave_minutes;
        self.card.wanted = kept.gpu_3d;
        self.window.kept = kept.window;
        self.scan_contrast = kept.high_contrast;
        self.system_title_bar = kept.system_title_bar;
        self.settings_kept = Some(kept);
        self.settings_path = Some(path);
    }

    /// Lets the 3D view be drawn on the card `state` is the window's, when the user asks (Settings > Performance).
    pub fn use_graphics_card(&mut self, state: &eframe::egui_wgpu::RenderState) {
        self.card.state = Some(state.clone());
    }

    fn settings_now(&self, theme: ThemePreference) -> Settings {
        Settings {
            theme,
            light_canvas: self.light_canvas,
            compact: self.compact,
            language: self.lang,
            ribbon_minimized: self.ribbon_min,
            docking: self.docking.clone(),
            autosave_minutes: self.autosave.interval_minutes,
            gpu_3d: self.card.wanted,
            window: self.window.kept,
            high_contrast: self.scan_contrast,
            system_title_bar: self.system_title_bar,
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
                        Page::Performance => self.performance_page(ui),
                        Page::Graphics => self.graphics_page(ui),
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

    /// The same choices in the ribbon's popup and File > Settings.
    pub(crate) fn theme_choices(&mut self, ui: &mut Ui, menu: bool) {
        let current = (ui.ctx().options(|o| o.theme_preference), self.light_canvas);
        for (label, theme, light_canvas) in [
            ("As the System", ThemePreference::System, false),
            ("Light", ThemePreference::Light, false),
            ("Dark", ThemePreference::Dark, false),
            ("Dark UI, Light Canvas", ThemePreference::Dark, true),
        ] {
            if ui.radio(current == (theme, light_canvas), t(label)).clicked() {
                self.light_canvas = light_canvas;
                ui.ctx().set_theme(theme);
                ui.ctx().request_repaint();
                if menu {
                    ui.close();
                }
            }
        }
    }

    fn interface_page(&mut self, ui: &mut Ui, ctx: &Context) {
        heading(ui, "Theme");
        self.theme_choices(ui, false);
        heading(ui, "Layout");
        ui.horizontal(|ui| {
            let tip = "The ribbon, the palettes and the bars as cards floating over the view";
            ui.radio_value(&mut self.compact, false, t("Floating")).on_hover_text(t(tip));
            let tip = "The ribbon, the palettes and the bars flush to the window's edges";
            ui.radio_value(&mut self.compact, true, t("Compact")).on_hover_text(t(tip));
        });
        heading(ui, "Language");
        ComboBox::from_id_salt("settings-language").selected_text(self.lang.label()).width(200.0).show_ui(ui, |ui| {
            for l in Lang::ALL {
                if ui.selectable_label(self.lang == l, l.label()).clicked() {
                    self.set_lang(ctx, l);
                }
            }
        });
        heading(ui, "Ribbon");
        ui.checkbox(&mut self.ribbon_min, t("Minimize the ribbon to its tabs"))
            .on_hover_text(t("Double-clicking a tab's name does the same; a click on a tab then shows its tools until one is picked"));
        if self.own_title_bar {
            heading(ui, "Title bar");
            let mut system = self.system_title_bar;
            if ui
                .checkbox(&mut system, t("Use the system's title bar"))
                .on_hover_text(t("The system's own title bar and frame, rather than the window's: the app bar then sits under it"))
                .changed()
            {
                self.use_system_title_bar(ctx, system);
            }
        }
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

impl BimApp {
    fn performance_page(&mut self, ui: &mut Ui) {
        heading(ui, "Graphics card");
        let found = self.card.available();
        ui.add_enabled(found, egui::Checkbox::new(&mut self.card.wanted, t("Draw the 3D view on the graphics card"))).on_hover_text(t(
            "Faster for large models. Hidden line and wireframe are still drawn on the processor, and so is everything if the card fails",
        ));
        let muted = |ui: &mut Ui, text: String| ui.label(RichText::new(text).color(pal().muted));
        let text = match (self.card.available(), &self.card.note) {
            (_, Some(why)) => crate::lang::tf("The 3D view is drawn on the processor: {}", &[why]),
            (false, None) => t("This copy has no graphics card it can draw the 3D view on").to_string(),
            (true, None) => crate::lang::tf("Card: {}", &[&self.card.adapter().unwrap_or_default()]),
        };
        muted(ui, text);
        later(ui, |ui| {
            let (mut undo, mut quality, mut smooth, mut background) = (100, 0, true, false);
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
        });
    }
}

impl BimApp {
    fn graphics_page(&mut self, ui: &mut Ui) {
        heading(ui, "Point clouds");
        ui.horizontal(|ui| {
            let l = ui.label(t("High contrast colour"));
            ui.color_edit_button_srgb(&mut self.scan_contrast)
                .labelled_by(l.id)
                .on_hover_text(t("What High Contrast Point Clouds, on the view control bar, draws a view's scans in"));
            if ui.button(t("Reset Colour")).clicked() {
                self.scan_contrast = crate::footer::HIGH_CONTRAST;
            }
        });
        later(ui, graphics_later);
    }
}

/// The Graphics page's settings still to come.
fn graphics_later(ui: &mut Ui) {
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
                        web_time::Instant::now() + web_time::Duration::from_secs(u64::from(self.autosave.interval_minutes) * 60);
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
