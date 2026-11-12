//! File > Settings (Ctrl+,): the window's own settings, the user's and never a project's. They live in
//! `settings.json` beside the favorites and the update settings ([`beside`]), which the desktop's `main` hands the app
//! ([`BimApp::use_settings`]); a window that wasn't given the file (every test's) keeps them for as long as it's open.
//! What it keeps: the theme and whether the drawing stays light in a dark one, the user's own accent, panel, canvas and text colours,
//! the profile, the layout, interface language, ribbon, palettes, the
//! window's size, the background autosave interval, the colour High Contrast Point Clouds draws scans in, whether plans show the
//! origin marker and their view's name in the corner, the shortcuts the user made, and the control port
//! for agents (whether the window opens it, on which port, and whether agents may edit the project or only read it), and the
//! assistant's provider, model and, only if the user asks, key (scrambled, or encrypted under a passphrase: never as typed).
//! The pages for what is still to come (performance and graphics) show what they will hold, greyed out.

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::path::{Path, PathBuf};

use egui::{Align, Color32, ComboBox, Context, Id as UiId, Layout, RichText, ScrollArea, ThemePreference, Ui, vec2};
use serde_json::{Value, json};

use bimcraft_assistant::{Provider, model_ok, seal};

use crate::BimApp;
use crate::assistant::Secret;
use crate::dock::{Docking, Palette};
use crate::lang::{Lang, t, tf};
use crate::presets::{Preset, Scale, hidden_from_json, presets_from_json};
use crate::style::{Profile, Scheme, pal};
use crate::updates::failed;
use std::collections::BTreeSet;

/// The settings file's name, in the folder of the favorites.
pub const FILE: &str = "settings.json";
/// The most of it that's read: a full set of 4096 bindings still fits.
const MAX_FILE: u64 = 1 << 20;
/// The most shortcuts of the user's own that are read back: the file is the user's, but also a hand's or a damage's.
pub const MAX_SHORTCUTS: usize = 4096;
/// The most actions a settings file may name.
pub const MAX_ACTIONS: usize = 512;

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
    pub profile: Profile,
    /// A preset's colours when dark, and the user's accent.
    pub scheme: Scheme,
    pub accent: Option<[u8; 3]>,
    /// The user's own panel, canvas and text colours, over any scheme.
    pub panel_colour: Option<[u8; 3]>,
    pub canvas_colour: Option<[u8; 3]>,
    pub text_colour: Option<[u8; 3]>,
    /// The project's scale, the tools taken out of the ribbon and the Tools palette, and the presets (`presets.rs`).
    pub scale: Scale,
    pub tools_hidden: BTreeSet<String>,
    pub presets: Vec<Preset>,
    pub language: Lang,
    pub ribbon_minimized: bool,
    pub docking: Docking,
    pub autosave_minutes: u32,
    /// Draw the 3D view on the graphics card, when there is one.
    pub gpu_3d: bool,
    pub window: Window,
    /// What High Contrast Point Clouds fills a view's scans with.
    pub high_contrast: [u8; 3],
    /// Grid snapping applies to every view, using the active view's enabled lattice.
    pub grid_snapping: bool,
    /// The plans' origin marker, and the view's name in their corner: working aids on the screen.
    pub origin_marker: bool,
    pub view_name: bool,
    /// The shortcuts: chords and sequences, many per action, saved as a full effective set.
    pub shortcuts: crate::shortcuts::Config,
    /// The system's title bar rather than the window's own, where the window can draw its own.
    pub system_title_bar: bool,
    /// Agents: the window opens the control port (on `agents_port`, 0 for any free one), and they may edit or only read.
    pub agents_listen: bool,
    pub agents_port: u16,
    pub agents_edit: bool,
    /// The Assistant's provider and model, and the key if the user asked for it to be kept: scrambled or under a passphrase (`seal`), never as typed.
    pub assistant_provider: Provider,
    pub assistant_model: String,
    pub assistant_sealed: Option<String>,
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
            "profile": self.profile.code(),
            "scheme": self.scheme.code(),
            "accent": self.accent,
            "panel_colour": self.panel_colour,
            "canvas_colour": self.canvas_colour,
            "text_colour": self.text_colour,
            "scale": self.scale.code(),
            "tools_hidden": self.tools_hidden,
            "presets": self.presets.iter().map(Preset::to_json).collect::<Vec<_>>(),
            "language": self.language.code(),
            "ribbon_minimized": self.ribbon_minimized,
            "palettes": self.docking.to_json(),
            "autosave_minutes": self.autosave_minutes,
            "gpu_3d": self.gpu_3d,
            "window": self.window.to_json(),
            "high_contrast": self.high_contrast,
            "grid_snapping": self.grid_snapping,
            "origin_marker": self.origin_marker,
            "view_name": self.view_name,
            "shortcuts": self.shortcuts.to_json(),
            "system_title_bar": self.system_title_bar,
            "agents": {"listen": self.agents_listen, "port": self.agents_port, "edit": self.agents_edit},
            // The key, only when the user asked for it to be kept, and then only sealed.
            "assistant": {"provider": self.assistant_provider.key(), "model": self.assistant_model, "key_sealed": self.assistant_sealed},
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
        // A colour of the user's: none (the scheme's) when the file has one this can't read.
        let own = |key: &str, base: Option<[u8; 3]>| if v.get(key).is_some() { colour(&v[key]) } else { base };
        Settings {
            theme,
            light_canvas: theme == ThemePreference::Dark && v["light_canvas"].as_bool().unwrap_or(base.light_canvas),
            compact: match v["layout"].as_str() {
                Some("compact") => true,
                Some("floating") => false,
                _ => base.compact,
            },
            profile: v["profile"].as_str().and_then(Profile::from_code).unwrap_or(base.profile),
            scheme: v["scheme"].as_str().and_then(Scheme::from_code).unwrap_or(base.scheme),
            accent: own("accent", base.accent),
            panel_colour: own("panel_colour", base.panel_colour),
            canvas_colour: own("canvas_colour", base.canvas_colour),
            text_colour: own("text_colour", base.text_colour),
            scale: v["scale"].as_str().and_then(Scale::from_code).unwrap_or(base.scale),
            tools_hidden: if v["tools_hidden"].is_array() { hidden_from_json(&v["tools_hidden"]) } else { base.tools_hidden },
            presets: if v["presets"].is_array() { presets_from_json(&v["presets"]) } else { base.presets },
            language: v["language"].as_str().and_then(Lang::from_code).unwrap_or(base.language),
            ribbon_minimized: v["ribbon_minimized"].as_bool().unwrap_or(base.ribbon_minimized),
            docking: if v["palettes"].is_object() { Docking::from_json(&v["palettes"]) } else { base.docking },
            autosave_minutes: v["autosave_minutes"].as_u64().filter(|n| *n <= 120).map_or(base.autosave_minutes, |n| n as u32),
            gpu_3d: v["gpu_3d"].as_bool().unwrap_or(base.gpu_3d),
            window: Window::from_json(&v["window"], base.window),
            high_contrast: colour(&v["high_contrast"]).unwrap_or(base.high_contrast),
            grid_snapping: v["grid_snapping"].as_bool().unwrap_or(base.grid_snapping),
            origin_marker: v["origin_marker"].as_bool().unwrap_or(base.origin_marker),
            view_name: v["view_name"].as_bool().unwrap_or(base.view_name),
            shortcuts: shortcuts_from(&v["shortcuts"], base.shortcuts),
            system_title_bar: v["system_title_bar"].as_bool().unwrap_or(base.system_title_bar),
            agents_listen: v["agents"]["listen"].as_bool().unwrap_or(base.agents_listen),
            agents_port: v["agents"]["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(base.agents_port),
            agents_edit: v["agents"]["edit"].as_bool().unwrap_or(base.agents_edit),
            assistant_provider: v["assistant"]["provider"].as_str().and_then(Provider::from_key).unwrap_or(base.assistant_provider),
            assistant_model: v["assistant"]["model"].as_str().filter(|m| model_ok(m)).map_or(base.assistant_model, str::to_string),
            // A text that is no sealed key (a damaged file, a hand's edit, the plain key an earlier build kept) is none.
            assistant_sealed: match v["assistant"].get("key_sealed") {
                None => base.assistant_sealed,
                Some(sealed) => sealed.as_str().filter(|s| seal::well_formed(s)).map(str::to_string),
            },
        }
    }
}

/// The shortcuts a settings file holds: version 2 keeps its full set; an old object of key to
/// action is migrated over the base profile.
fn shortcuts_from(v: &Value, base: crate::shortcuts::Config) -> crate::shortcuts::Config {
    if v.is_object() { crate::shortcuts::Config::from_json(v, &base) } else { base }
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
    Agents,
    Assistant,
    Updates,
}

impl Page {
    pub const ALL: [Page; 7] =
        [Page::Interface, Page::Performance, Page::Graphics, Page::Files, Page::Agents, Page::Assistant, Page::Updates];

    pub fn name(self) -> &'static str {
        match self {
            Page::Interface => "User Interface",
            Page::Performance => msg!("Performance"),
            Page::Graphics => "Graphics",
            Page::Files => msg!("Files"),
            Page::Agents => msg!("Agents"),
            Page::Assistant => msg!("Assistant"),
            Page::Updates => msg!("Updates"),
        }
    }
}

impl BimApp {
    /// Puts the interface in `lang`. The fonts with the hanzi and the Hangul are already behind the window's own, whatever it
    /// speaks. Chinese needs its glyphs: where the system has none the language stays as it was, and the status says why.
    pub(crate) fn set_lang(&mut self, ctx: &Context, lang: Lang) {
        let hanzi = crate::lang::use_cjk_font(ctx, self.font_finder);
        if lang.is_chinese() && !hanzi {
            self.status = failed(t(crate::lang::NO_CHINESE_FONT).to_string());
        } else {
            self.lang = lang;
        }
    }

    /// Keeps the settings in `path`: what it holds now, and from now on what changes.
    pub fn use_settings(&mut self, path: PathBuf) {
        let now = self.settings_now(ThemePreference::System);
        let raw = read(&path);
        // An old key-to-action object is migrated to version 2, and the file is written back.
        let migrating = raw
            .as_ref()
            .and_then(|v| v.get("shortcuts"))
            .and_then(Value::as_object)
            .is_some_and(|o| o.get("version").and_then(Value::as_u64) != Some(2));
        let kept = match raw {
            Some(v) => Settings::from_json(&v, now),
            None => now,
        };
        (self.lang, self.ribbon_min, self.docking) = (kept.language, kept.ribbon_minimized, kept.docking.clone());
        (self.compact, self.profile, self.scale) = (kept.compact, kept.profile, kept.scale);
        (self.scheme, self.accent) = (kept.scheme, kept.accent);
        (self.panel_colour, self.canvas_colour, self.text_colour) = (kept.panel_colour, kept.canvas_colour, kept.text_colour);
        (self.tools_hidden, self.presets) = (kept.tools_hidden.clone(), kept.presets.clone());
        // BIMCraft has no ribbon: it starts with its tools out, whatever a session in Rev left.
        if self.profile == Profile::BimCraft {
            self.docking.show(Palette::Tools, true);
        }
        self.theme_wanted = Some(kept.theme);
        self.light_canvas = kept.light_canvas;
        self.autosave.interval_minutes = kept.autosave_minutes;
        self.card.wanted = kept.gpu_3d;
        self.window.kept = kept.window;
        self.scan_contrast = kept.high_contrast;
        self.grid_snapping = kept.grid_snapping;
        (self.show_origin, self.show_view_name) = (kept.origin_marker, kept.view_name);
        self.shortcuts = crate::shortcuts::Shortcuts::from_config(&kept.shortcuts);
        crate::shortcuts::publish(&self.shortcuts);
        self.system_title_bar = kept.system_title_bar;
        (self.agents.listen, self.agents.port, self.agents.edit) = (kept.agents_listen, kept.agents_port, kept.agents_edit);
        let a = &mut self.assistant;
        (a.provider, a.model, a.sealed) = (kept.assistant_provider, kept.assistant_model.clone(), kept.assistant_sealed.clone());
        // A saved key is opened for the session, unless it is under a passphrase: then it stays locked until the user types it. One this build
        // cannot open (another kind of build saved it, or the file was changed) is forgotten, and the user types it again.
        (a.protect, a.key_saved) = (false, false);
        match a.sealed.as_deref() {
            Some(sealed) if seal::needs_passphrase(sealed) => (a.remember, a.protect) = (true, true),
            Some(sealed) => match seal::open(sealed) {
                Ok(key) => (a.key, a.remember, a.key_saved) = (Secret(key), true, true),
                Err(_) => (a.sealed, a.remember) = (None, false),
            },
            None => a.remember = false,
        }
        self.settings_kept = if migrating { None } else { Some(kept) };
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
            profile: self.profile,
            scheme: self.scheme,
            accent: self.accent,
            panel_colour: self.panel_colour,
            canvas_colour: self.canvas_colour,
            text_colour: self.text_colour,
            scale: self.scale,
            tools_hidden: self.tools_hidden.clone(),
            presets: self.presets.clone(),
            language: self.lang,
            ribbon_minimized: self.ribbon_min,
            docking: self.docking.clone(),
            autosave_minutes: self.autosave.interval_minutes,
            gpu_3d: self.card.wanted,
            window: self.window.kept,
            high_contrast: self.scan_contrast,
            grid_snapping: self.grid_snapping,
            origin_marker: self.show_origin,
            view_name: self.show_view_name,
            shortcuts: self.shortcuts.to_config(),
            system_title_bar: self.system_title_bar,
            agents_listen: self.agents.listen,
            agents_port: self.agents.port,
            agents_edit: self.agents.edit,
            assistant_provider: self.assistant.provider,
            assistant_model: self.assistant.model.clone(),
            assistant_sealed: self.assistant.sealed.clone(),
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
        let r = Modal::new(UiId::new("settings"), t("Settings")).width(660.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
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
                            Page::Agents => self.agents_page(ui),
                            Page::Assistant => self.assistant_page(ui),
                            Page::Updates => self.updates_page(ui),
                        });
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
            });
        });
        self.settings_page = if close || r.should_close() { None } else { Some(page) };
    }

    /// The same choices in the ribbon's popup and File > Settings.
    pub(crate) fn theme_choices(&mut self, ui: &mut Ui, menu: bool) {
        let current = (ui.ctx().options(|o| o.theme_preference), self.light_canvas);
        let own = self.scheme == Scheme::Own;
        for (label, theme, light_canvas) in [
            (msg!("As the System"), ThemePreference::System, false),
            (msg!("Light"), ThemePreference::Light, false),
            (msg!("Dark"), ThemePreference::Dark, false),
            (msg!("Dark UI, Light Canvas"), ThemePreference::Dark, true),
        ] {
            if ui.radio(own && current == (theme, light_canvas), t(label)).clicked() {
                (self.light_canvas, self.scheme) = (light_canvas, Scheme::Own);
                ui.ctx().set_theme(theme);
                ui.ctx().request_repaint();
                if menu {
                    ui.close();
                }
            }
        }
        // The presets, dark ones: one picked puts the window dark in its colours.
        for scheme in Scheme::PRESETS {
            if ui.radio(self.scheme == scheme, scheme.name()).clicked() {
                (self.light_canvas, self.scheme) = (false, scheme);
                ui.ctx().set_theme(ThemePreference::Dark);
                ui.ctx().request_repaint();
                if menu {
                    ui.close();
                }
            }
        }
    }

    /// A box for each palette, shown or closed, and the layout the profile starts in: Settings > User Interface's, and the
    /// app bar's Palettes menu, which is the way back for the Tools palette, as it has no shortcut.
    pub(crate) fn palette_switches(&mut self, ui: &mut Ui) {
        for p in Palette::ALL {
            let mut on = self.docking.shown(p);
            if ui.checkbox(&mut on, t(p.name())).changed() {
                self.show_palette(p, on);
            }
        }
        let (tip, layout) = match self.profile {
            Profile::BimCraft => {
                (msg!("The tools on the left, the Inspector over the Navigator on the right, as at first"), Docking::default())
            }
            Profile::Rev => (msg!("Properties over the Project Browser, on the left, as at first"), Docking::revit()),
        };
        if ui.button(t("Reset Palette Layout")).on_hover_text(t(tip)).clicked() {
            self.docking = layout;
        }
    }

    fn interface_page(&mut self, ui: &mut Ui, ctx: &Context) {
        heading(ui, msg!("Profile"));
        let before = self.profile;
        ui.horizontal(|ui| {
            for (profile, tip) in [
                (Profile::BimCraft, msg!("BIMCraft's own: the tools in a palette, the Inspector and the Navigator on the right")),
                (Profile::Rev, msg!("The ribbon over the view, Properties and the Project Browser on the left")),
            ] {
                ui.radio_value(&mut self.profile, profile, profile.name()).on_hover_text(t(tip));
            }
        });
        // Back in BIMCraft, its tools come back where they were: Rev, which has the ribbon, put them away.
        if before != self.profile && self.profile == Profile::BimCraft {
            self.docking.show(Palette::Tools, true);
        }
        heading(ui, msg!("Presets"));
        self.presets_section(ui);
        heading(ui, msg!("Theme"));
        self.theme_choices(ui, false);
        // Colours of the user's own over any theme, each showing what is drawn now until one is picked.
        let (now, ground) = (pal(), crate::canvas::color(bimcraft_core::render::PAPER));
        let rgb = |c: Color32| [c.r(), c.g(), c.b()];
        for (label, tip, reset, kept, shown) in [
            (
                msg!("Accent colour"),
                msg!(
                    "What is picked and what has focus are drawn in it, in every theme; its lettering is made dark or light enough to read"
                ),
                msg!("Reset Accent"),
                &mut self.accent,
                now.accent,
            ),
            (
                msg!("Panel colour"),
                msg!("The ground of the ribbon, the palettes, the bars and the dialogs; their bands and buttons are a step off it"),
                msg!("Reset Panels"),
                &mut self.panel_colour,
                now.panel,
            ),
            (
                msg!("Canvas colour"),
                msg!("What the views are drawn on; the drawing's lines turn light on a dark one and dark on a light one"),
                msg!("Reset Canvas"),
                &mut self.canvas_colour,
                ground,
            ),
            (
                msg!("Text colour"),
                msg!("The interface's lettering, made darker or lighter where it would not read at 4.5:1 on the panels"),
                msg!("Reset Text"),
                &mut self.text_colour,
                now.text,
            ),
        ] {
            ui.horizontal(|ui| {
                let label = ui.label(t(label));
                let mut picked = kept.unwrap_or(rgb(shown));
                if ui.color_edit_button_srgb(&mut picked).labelled_by(label.id).on_hover_text(t(tip)).changed() {
                    *kept = Some(picked);
                }
                if kept.is_some() && ui.button(t(reset)).clicked() {
                    *kept = None;
                }
            });
        }
        if self.text_colour.is_some_and(|c| c != rgb(now.text)) {
            let why = t("Your text colour was too close to the panels: it is made darker or lighter to read at 4.5:1");
            ui.label(RichText::new(why).color(now.muted));
        }
        heading(ui, msg!("Layout"));
        ui.horizontal(|ui| {
            let tip = msg!("The ribbon, the palettes and the bars as cards floating over the view");
            ui.radio_value(&mut self.compact, false, t("Floating")).on_hover_text(t(tip));
            let tip = msg!("The ribbon, the palettes and the bars flush to the window's edges");
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
        heading(ui, msg!("Ribbon"));
        ui.checkbox(&mut self.ribbon_min, t("Minimize the ribbon to its tabs"))
            .on_hover_text(t("Double-clicking a tab's name does the same; a click on a tab then shows its tools until one is picked"));
        if self.own_title_bar {
            heading(ui, msg!("Title bar"));
            let mut system = self.system_title_bar;
            if ui
                .checkbox(&mut system, t("Use the system's title bar"))
                .on_hover_text(t("The system's own title bar and frame, rather than the window's: the app bar then sits under it"))
                .changed()
            {
                self.use_system_title_bar(ctx, system);
            }
        }
        heading(ui, msg!("Palettes"));
        self.palette_switches(ui);
        heading(ui, msg!("Plans"));
        ui.checkbox(&mut self.show_origin, t("Show the origin marker in plans")).on_hover_text(t(
            "A small marker at the project's 0, 0 with the directions of x (east) and y (north). It is drawn on the screen only: \
             never in a printed drawing, a sheet or an export",
        ));
        ui.checkbox(&mut self.show_view_name, t("Show the view's name in the corner of plans")).on_hover_text(t(
            "The name of the floor plan or ceiling plan in its bottom left corner. It is drawn on the screen only, and the view's tab \
             names it too",
        ));
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
        heading(ui, msg!("Graphics card"));
        let found = self.card.available();
        ui.add_enabled(found, egui::Checkbox::new(&mut self.card.wanted, t("Draw the 3D view on the graphics card")))
            .on_hover_text(t("Faster for large models. If the card fails, the 3D view is drawn on the processor"));
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

impl BimApp {
    /// Agents: the control port, through which an agent works on this project (`bimcraft-cli mcp --connect`), opened and closed
    /// here as `--control PORT` opens it at the start, and whether agents may change the project or only read it.
    fn agents_page(&mut self, ui: &mut Ui) {
        let muted = |ui: &mut Ui, text: &str| ui.label(RichText::new(text).color(pal().muted));
        heading(ui, msg!("Control port"));
        let mut on = self.control.is_some();
        let tip =
            t("Opens the control port now and whenever BIMCraft starts: an agent connects with bimcraft-cli mcp --connect. Only this \
             machine can reach it, and only with the token in bridge.json beside your favorites");
        if ui.checkbox(&mut on, t("Let agents work on this project")).on_hover_text(tip).changed() {
            self.agents.listen = on;
            if on {
                self.open_control();
            } else {
                self.close_control();
            }
        }
        ui.horizontal(|ui| {
            let l = ui.label(t("Port"));
            let r = ui
                .add(egui::DragValue::new(&mut self.agents.port).range(0..=u16::MAX).update_while_editing(false))
                .labelled_by(l.id)
                .on_hover_text(t("0 for any port that is free; an open port moves to a new one"));
            if (r.changed() && !r.dragged()) || r.drag_stopped() {
                self.control_port_changed();
            }
            muted(ui, t("0: any free port"));
        });
        let state = match (&self.control, &self.agents.failed) {
            (Some(c), _) => {
                let port = c.entry.port.to_string();
                tf("Listening on 127.0.0.1, port {}: agents connect with bimcraft-cli mcp --connect {}", &[&port, &port])
            }
            (None, Some(why)) => why.clone(),
            (None, None) => t("Off: no agent can reach this window").to_string(),
        };
        muted(ui, &state);
        if let Some(c) = &self.control {
            muted(ui, &tf("Agents connected: {}", &[&c.agents().to_string()]));
        }
        heading(ui, msg!("Editing"));
        ui.checkbox(&mut self.agents.edit, t("Allow agents to edit this project")).on_hover_text(t(
            "Off, agents only read the project: they inspect it, read its schedules, render its views and export it to new files \
             (its sheets to PDF among them), and whatever they ask that would change it, undo it, replace it or save over its file \
             is refused",
        ));
        if !self.agents.edit {
            muted(ui, t("Agents may only read this project."));
        }
    }
}

/// A group's heading on a page.
pub(crate) fn heading(ui: &mut Ui, text: &str) {
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
