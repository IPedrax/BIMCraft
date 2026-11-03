//! Look and feel: a lean desktop tool, light or dark (the system's theme until one is picked on the View tab).
//! A dark app bar on top; under it the view, and over it the ribbon, the Options Bar, the palettes and the status bar as
//! cards: rounded, a hairline border and a soft shadow, `GAP` off the edges and each other (the Floating layout), or the
//! same pieces flush and square (Compact). One blue accent for the active tool, selection and focus; IBM Plex for the
//! lettering; the icons are drawn in `icons.rs`.
//! A dark interface may keep the drawing canvas light; canvas painters use `canvas_dark`, chrome uses `dark`.
//! The Rev profile (`Profile`) wears palettes of its own (`REV_LIGHT`, `REV_DARK`: the chrome in the ribbon's greys), square
//! corners (`corner`) and the Compact layout; the drawing's colours (`canvas_pal`) are the same in both profiles.

use std::cell::Cell;
use std::sync::Arc;

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, Ui, vec2,
};

/// A theme's colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub accent: Color32,
    /// The accent where it must be read, as text or an icon or the ring round what has focus, on a card, a tint or a hovered
    /// row: `accent` is for fills, strokes and the drawing, and falls short of 4.5:1 on a tint.
    pub accent_ink: Color32,
    /// Pre-selection: what a click would pick.
    pub hover: Color32,
    pub select_fill: Color32,
    pub hover_fill: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub icon: Color32,
    /// What the cards are: the ribbon, the palettes, the bars.
    pub panel: Color32,
    /// A band inside a card: a header, a group's heading.
    pub bar: Color32,
    /// What a button or a drop-down is: a step past `bar` (darker on light, lighter on dark), so a control never reads as a
    /// heading beside it.
    pub control: Color32,
    pub border: Color32,
    pub error: Color32,
    /// Revit's contextual tabs (Modify | …) stand out in green.
    pub context: Color32,
    pub context_fill: Color32,
    /// Lettering on `context`, the open contextual tab's fill.
    pub on_context: Color32,
    /// What fields and popups sit on.
    pub surface: Color32,
    /// Around a sheet's paper.
    pub desk: Color32,
    /// The band across the top of the window, and its lettering.
    pub app_bar: Color32,
    pub app_bar_text: Color32,
    pub app_bar_muted: Color32,
    pub app_bar_hover: Color32,
    /// What a card's shadow is.
    pub shadow: Color32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub const LIGHT: Palette = Palette {
    accent: rgb(0x2678DA),
    accent_ink: rgb(0x2064B5),
    hover: rgb(0x5B9BE6),
    select_fill: rgb(0xD6E6FA),
    hover_fill: rgb(0xE2EAF6),
    text: rgb(0x262A32),
    muted: rgb(0x636873),
    icon: rgb(0x3A404C),
    panel: rgb(0xFAFBFC),
    bar: rgb(0xECEEF2),
    control: rgb(0xE0E3E9),
    border: rgb(0xD0D4DC),
    error: rgb(0xCC2020),
    context: rgb(0x15803D),
    context_fill: rgb(0xDCFCE7),
    on_context: rgb(0xFFFFFF),
    surface: rgb(0xFFFFFF),
    desk: rgb(0xD4D4D4),
    app_bar: rgb(0x343840),
    app_bar_text: rgb(0xE6EAF2),
    app_bar_muted: rgb(0xBEC3CB),
    app_bar_hover: rgb(0x4A4F59),
    shadow: Color32::from_rgba_premultiplied(9, 10, 12, 46),
};

/// The light palette where the drawing paints on its white paper: a deeper hover stroke (3.3:1 on the paper; the interface's is 2.9:1)
/// and a deeper selection tint, so a glance tells hover from selection. The interface keeps [`LIGHT`]'s tints, the drawing alone has these.
pub const CANVAS_LIGHT: Palette = Palette { hover: rgb(0x4A90E2), select_fill: rgb(0xBFD8F7), ..LIGHT };

pub const DARK: Palette = Palette {
    accent: rgb(0x3A88EE),
    accent_ink: rgb(0x89B8F5),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0x2A466E),
    hover_fill: rgb(0x363D4A),
    text: rgb(0xDEE2E8),
    muted: rgb(0xA2A8B3),
    icon: rgb(0xCFD4DB),
    panel: rgb(0x272B34),
    bar: rgb(0x2F343E),
    control: rgb(0x3A404C),
    border: rgb(0x3F4552),
    error: rgb(0xF87171),
    context: rgb(0x4ADE80),
    context_fill: rgb(0x14341F),
    on_context: rgb(0x0F1F16),
    surface: rgb(0x1E2229),
    desk: rgb(0x2A2E35),
    app_bar: rgb(0x1C1F26),
    app_bar_text: rgb(0xE6EAF2),
    app_bar_muted: rgb(0x9FA5B1),
    app_bar_hover: rgb(0x343A46),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 110),
};

/// Rev's light palette: the chrome (the title bar and the ribbon's tabs) and the cards in the ribbon's own light greys, where
/// BIMCraft's bar is dark. The accent, its tints and the drawing's colours are [`LIGHT`]'s.
pub const REV_LIGHT: Palette = Palette {
    panel: rgb(0xF7F8F9),
    bar: rgb(0xE8EAED),
    control: rgb(0xE0E3E7),
    border: rgb(0xC9CDD2),
    app_bar: rgb(0xE3E6EA),
    app_bar_text: rgb(0x262A32),
    app_bar_muted: rgb(0x555A63),
    app_bar_hover: rgb(0xD3D7DD),
    ..LIGHT
};

/// Rev's dark palette: neutral greys, the chrome a step darker than the cards. The accent and its tints are [`DARK`]'s.
pub const REV_DARK: Palette = Palette {
    panel: rgb(0x2B2F36),
    bar: rgb(0x2F333A),
    control: rgb(0x3D424B),
    border: rgb(0x454B55),
    surface: rgb(0x202328),
    app_bar: rgb(0x1F2227),
    app_bar_hover: rgb(0x30353D),
    ..DARK
};

/// Dracula's colours (the open theme by Zeno Rocha, MIT): its purple for the accent, its ground and lines for the cards.
pub const DRACULA: Palette = Palette {
    accent: rgb(0xBD93F9),
    accent_ink: rgb(0xD4B5FF),
    hover: rgb(0xD6B8FF),
    select_fill: rgb(0x4B3F6E),
    hover_fill: rgb(0x343746),
    text: rgb(0xF8F8F2),
    muted: rgb(0xB0B5CF),
    icon: rgb(0xE0E0E8),
    panel: rgb(0x282A36),
    bar: rgb(0x343746),
    control: rgb(0x3F4256),
    border: rgb(0x4D5068),
    error: rgb(0xFF7B7B),
    context: rgb(0x50FA7B),
    context_fill: rgb(0x1F3A2A),
    on_context: rgb(0x13151C),
    surface: rgb(0x21222C),
    desk: rgb(0x1E1F29),
    app_bar: rgb(0x191A21),
    app_bar_text: rgb(0xF8F8F2),
    app_bar_muted: rgb(0xB8BCD6),
    app_bar_hover: rgb(0x2C2E3B),
    ..DARK
};

/// Midnight: deep blues, the accent a brighter one.
pub const MIDNIGHT: Palette = Palette {
    accent: rgb(0x4C8DFF),
    accent_ink: rgb(0x8DB6FF),
    hover: rgb(0x6FA2FF),
    select_fill: rgb(0x1F3B6B),
    hover_fill: rgb(0x1A2944),
    text: rgb(0xE3E9F5),
    muted: rgb(0x9FB0CE),
    icon: rgb(0xCFD9EC),
    panel: rgb(0x0F1A2E),
    bar: rgb(0x16233B),
    control: rgb(0x1E2D48),
    border: rgb(0x2A3B5C),
    error: rgb(0xFF7A7A),
    context_fill: rgb(0x12301F),
    on_context: rgb(0x0B1A12),
    surface: rgb(0x0B1424),
    desk: rgb(0x0A1220),
    app_bar: rgb(0x08101E),
    app_bar_text: rgb(0xE3E9F5),
    app_bar_muted: rgb(0x9FB0CE),
    app_bar_hover: rgb(0x18263F),
    ..DARK
};

/// AMOLED: black, so an OLED screen's pixels are off round the drawing.
pub const AMOLED: Palette = Palette {
    select_fill: rgb(0x12305A),
    hover_fill: rgb(0x1A1A1A),
    text: rgb(0xE6E6E6),
    muted: rgb(0xA3A3A3),
    icon: rgb(0xD0D0D0),
    panel: rgb(0x000000),
    bar: rgb(0x0D0D0D),
    control: rgb(0x1C1C1C),
    border: rgb(0x2E2E2E),
    context_fill: rgb(0x0E2416),
    on_context: rgb(0x000000),
    surface: rgb(0x000000),
    desk: rgb(0x000000),
    app_bar: rgb(0x000000),
    app_bar_text: rgb(0xE6E6E6),
    app_bar_muted: rgb(0xA3A3A3),
    app_bar_hover: rgb(0x1F1F1F),
    ..DARK
};

/// The colours a user picks over the profile's own: one of the presets (dark ones), or the profile's light and dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Own,
    Dracula,
    Midnight,
    Amoled,
}

impl Scheme {
    pub const PRESETS: [Scheme; 3] = [Scheme::Dracula, Scheme::Midnight, Scheme::Amoled];

    /// Its name, which is its label too: names, so never translated.
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Own => "Own",
            Scheme::Dracula => "Dracula",
            Scheme::Midnight => "Midnight",
            Scheme::Amoled => "AMOLED",
        }
    }

    /// What the settings file keeps.
    pub fn code(self) -> &'static str {
        match self {
            Scheme::Own => "own",
            Scheme::Dracula => "dracula",
            Scheme::Midnight => "midnight",
            Scheme::Amoled => "amoled",
        }
    }

    pub fn from_code(code: &str) -> Option<Scheme> {
        [Scheme::Own].into_iter().chain(Scheme::PRESETS).find(|s| s.code() == code)
    }
}

/// The colours a frame is drawn in: the profile's own, light or dark, or the scheme picked when dark (the presets are dark ones),
/// with the user's accent over them when there is one.
pub fn palette(dark: bool, rev: bool, scheme: Scheme, accent: Option<[u8; 3]>) -> Palette {
    let base = match (dark, scheme, rev) {
        (true, Scheme::Dracula, _) => DRACULA,
        (true, Scheme::Midnight, _) => MIDNIGHT,
        (true, Scheme::Amoled, _) => AMOLED,
        (false, _, false) => LIGHT,
        (true, _, false) => DARK,
        (false, _, true) => REV_LIGHT,
        (true, _, true) => REV_DARK,
    };
    accent.map_or(base, |[r, g, b]| with_accent(base, Color32::from_rgb(r, g, b)))
}

/// `a` and `b` mixed, `t` of the way to `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// `p` with `accent` for its fills and strokes, a tint of it for what is picked, and its ink: the accent stepped toward the
/// text's colour until it holds 4.5:1 on every ground it is read on (WCAG 1.4.3), whatever the accent is.
fn with_accent(p: Palette, accent: Color32) -> Palette {
    let select_fill = mix(p.panel, accent, 0.25);
    let grounds = [p.panel, p.bar, p.surface, p.hover_fill, select_fill, p.control];
    let ink = (0..=20).map(|i| mix(accent, p.text, i as f32 / 20.0)).find(|ink| grounds.iter().all(|g| contrast(*ink, *g) >= 4.5));
    Palette { accent, accent_ink: ink.unwrap_or(p.text), hover: mix(accent, p.text, 0.3), select_fill, ..p }
}

/// WCAG 2.x's contrast ratio of two opaque colours: (L1 + 0.05) / (L2 + 0.05), L the relative luminance.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let (a, b) = (lum(a), lum(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Rev's File tab, white on it in both themes.
pub const FILE_TAB: Color32 = rgb(0x2064B5);

/// How far the cards stand off the window's edges and each other, in the Floating layout.
pub const GAP: i8 = 7;
/// A card's corners, and a menu's or a dialog's.
pub const CARD_RADIUS: u8 = 8;
pub const WINDOW_RADIUS: u8 = 10;

thread_local! {
    /// Whether this frame is drawn dark: each window draws on its own thread.
    static DARK_ON: Cell<bool> = const { Cell::new(false) };
    /// Canvas darkness is independent of the surrounding interface.
    static CANVAS_DARK: Cell<bool> = const { Cell::new(false) };
    /// Whether this frame lays the window out flush (Compact) rather than in cards (Floating).
    static COMPACT: Cell<bool> = const { Cell::new(false) };
    /// Whether this frame wears the Rev profile.
    static REV: Cell<bool> = const { Cell::new(false) };
    /// The colours this frame is drawn in (`palette`), set by `follow`.
    static CURRENT: Cell<Palette> = const { Cell::new(LIGHT) };
    /// Whether what's drawn now is on the dark app bar, where icons are light.
    static ON_APP_BAR: Cell<bool> = const { Cell::new(false) };
    /// Whether the window has its semibold lettering yet: fonts set take effect a frame later.
    static SEMIBOLD_ON: Cell<bool> = const { Cell::new(false) };
}

/// Draw what `add` draws on the app bar: icons and their hover light on dark.
pub fn on_app_bar<R>(add: impl FnOnce() -> R) -> R {
    ON_APP_BAR.with(|b| b.set(true));
    let r = add();
    ON_APP_BAR.with(|b| b.set(false));
    r
}

/// The ring round what has keyboard focus: the accent's ink, which holds on a card, a tint and a hovered row, and on the app bar
/// the lettering's colour, since the accent is dim on the light theme's dark bar.
pub fn focus_ring() -> Stroke {
    Stroke::new(2.0, if ON_APP_BAR.with(Cell::get) { pal().app_bar_text } else { pal().accent_ink })
}

/// An icon's ink and its hover's fill where it's drawn: on the app bar or on a card.
pub fn icon_inks() -> (Color32, Color32) {
    if ON_APP_BAR.with(Cell::get) { (pal().app_bar_text, pal().app_bar_hover) } else { (pal().icon, pal().hover_fill) }
}

/// The colours this frame is drawn in: the profile's, light or dark, or a preset, with the user's accent (`palette`).
pub fn pal() -> Palette {
    CURRENT.with(Cell::get)
}

/// A button's or a tab's corners, `r` in BIMCraft's look: Rev's are all but square.
pub fn corner(r: f32) -> f32 {
    if rev() { r.min(2.0) } else { r }
}

/// Whether the interface is dark this frame (the chrome around the drawing; the drawing's own is [`canvas_dark`]).
pub fn dark() -> bool {
    DARK_ON.with(Cell::get)
}

/// Whether the drawing (a plan, a section, the 3D view) is dark this frame: it is where the interface is, unless the user kept
/// the canvas light inside a dark interface.
pub fn canvas_dark() -> bool {
    CANVAS_DARK.with(Cell::get)
}

/// The colours the drawing paints with where they must read on its own ground (the fills of what is selected or hovered, a crop
/// region's outline): the dark palette on a dark canvas, [`CANVAS_LIGHT`] on a light canvas, whatever the interface around it is.
pub fn canvas_pal() -> &'static Palette {
    if canvas_dark() { &DARK } else { &CANVAS_LIGHT }
}

/// Which interface the window wears: BIMCraft's own (the default), or Rev, laid out as the ribbon tool its users know: flat and
/// square, the chrome in the ribbon's own greys, the File tab leading the ribbon's tabs and the Options Bar docked under the ribbon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    #[default]
    BimCraft,
    Rev,
}

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::BimCraft, Profile::Rev];

    /// Its name, which is its label too: a name, so never translated.
    pub fn name(self) -> &'static str {
        match self {
            Profile::BimCraft => "BIMCraft",
            Profile::Rev => "Rev",
        }
    }

    /// What the settings file keeps.
    pub fn code(self) -> &'static str {
        match self {
            Profile::BimCraft => "bimcraft",
            Profile::Rev => "rev",
        }
    }

    pub fn from_code(code: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.code() == code)
    }
}

/// How the window is drawn beyond the theme: what the user kept on the View tab and in Settings.
#[derive(Clone, Copy, Default)]
pub struct Look {
    /// The drawing stays light in a dark interface.
    pub light_canvas: bool,
    /// The cards sit flush and square (Compact) rather than floating.
    pub compact: bool,
    pub profile: Profile,
    /// A preset's colours when dark, and the user's accent.
    pub scheme: Scheme,
    pub accent: Option<[u8; 3]>,
}

/// Draw this frame in the theme egui shows: the system's, unless the user picked one; in the look the user kept.
pub fn follow(ctx: &Context, look: Look) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
    CANVAS_DARK.with(|d| d.set(dark() && !look.light_canvas));
    COMPACT.with(|c| c.set(look.compact));
    REV.with(|r| r.set(look.profile == Profile::Rev));
    CURRENT.with(|c| c.set(palette(dark(), rev(), look.scheme, look.accent)));
    // The widgets' look is the context's, set for both themes: again only when the profile or the colours change.
    let (styled, now) = (egui::Id::new("styled-look"), (look.profile, look.scheme, look.accent));
    if ctx.data(|d| d.get_temp::<(Profile, Scheme, Option<[u8; 3]>)>(styled)) != Some(now) {
        visuals(ctx, look);
        ctx.data_mut(|d| d.insert_temp(styled, now));
    }
    let semibold = ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(SEMIBOLD.into())));
    SEMIBOLD_ON.with(|s| s.set(semibold));
}

/// Whether this frame is laid out flush: the Compact layout, and Rev, which has no other.
pub fn compact() -> bool {
    COMPACT.with(Cell::get) || rev()
}

/// Whether this frame wears the Rev profile.
pub fn rev() -> bool {
    REV.with(Cell::get)
}

/// The gap round the cards: none in the Compact layout.
pub fn gap() -> i8 {
    if compact() { 0 } else { GAP }
}

/// A card standing off what's round it on the sides `outer` names (left, right, top, bottom): the ribbon, a palette, a bar.
/// Flush and square in the Compact layout, where a hairline parts it from its neighbours.
pub fn card(outer: [bool; 4]) -> Frame {
    let [left, right, top, bottom] = outer.map(|on| if on { gap() } else { 0 });
    let frame = Frame::new().fill(pal().panel).stroke(Stroke::new(1.0, pal().border)).outer_margin(Margin { left, right, top, bottom });
    if compact() { frame } else { frame.corner_radius(CARD_RADIUS).shadow(card_shadow()) }
}

/// A card's shadow: close and soft.
pub fn card_shadow() -> Shadow {
    Shadow { offset: [0, 2], blur: 8, spread: 0, color: pal().shadow }
}

/// What a dialog leaves of the window's height round its frame, so it never touches the window's edges.
const DIALOG_MARGIN: f32 = 48.0;

/// The tallest what a dialog says about why it was refused is, in the box that scrolls ([`boxed_text`]).
pub const ERROR_HEIGHT: f32 = 40.0;

/// The height a dialog gives a part of it that scrolls: the window's, less the dialog's frame, what `ui` has drawn so far, and
/// `below`, what the dialog draws under that part (its notes, its row of buttons: [`dialog_row`] is one). Never under a few rows, so
/// a window too small for a dialog scrolls it, and its buttons stay in reach.
pub fn dialog_room(ui: &Ui, below: f32) -> f32 {
    let used = ui.cursor().top() - ui.min_rect().top();
    (ui.ctx().content_rect().height() - DIALOG_MARGIN - used - below).min(ui.available_height() - below).max(80.0)
}

/// The height of a row of buttons or fields in a dialog, with the gap above it.
pub fn dialog_row(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y + ui.spacing().item_spacing.y
}

/// `text` in `colour` in a box at most `height` tall that scrolls, for what a dialog says in itself and whose length it cannot
/// know (a core message, a long list of what an import left out).
pub fn boxed_text(ui: &mut Ui, id: &str, colour: Color32, text: &str, height: f32) {
    egui::ScrollArea::vertical().id_salt(id).max_height(height).show(ui, |ui| {
        ui.colored_label(colour, text);
    });
}

/// The lettering at a weight heavier than the body's: a title, a tab. The body's until the window has it.
pub fn semibold(size: f32) -> FontId {
    match SEMIBOLD_ON.with(Cell::get) {
        true => FontId::new(size, FontFamily::Name(SEMIBOLD.into())),
        false => FontId::proportional(size),
    }
}

const SEMIBOLD: &str = "plex-semibold";

/// Every family the window's text is set in. A font meant for all the text (the Chinese one) joins each: egui does not fall back
/// from one family to another.
pub fn families() -> [FontFamily; 3] {
    [FontFamily::Proportional, FontFamily::Monospace, FontFamily::Name(SEMIBOLD.into())]
}
// The core's, which text types letter notes in: the window holds one copy.
use bimcraft_core::text::{PLEX_MONO, PLEX_SANS};

/// IBM Plex Sans (regular, and semibold as a family of its own) and Plex Mono first, egui's own faces behind them for the
/// symbols Plex hasn't.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let behind = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, weight, family) in [("plex-sans", 400.0, FontFamily::Proportional), (SEMIBOLD, 600.0, FontFamily::Name(SEMIBOLD.into()))] {
        let mut data = FontData::from_static(PLEX_SANS);
        data.tweak.coords = VariationCoords::new([(b"wght", weight)]);
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
        fonts.families.insert(family, std::iter::once(name.to_owned()).chain(behind.iter().cloned()).collect());
    }
    fonts.font_data.insert("plex-mono".to_owned(), Arc::new(FontData::from_static(PLEX_MONO)));
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".to_owned());
    fonts
}

/// The fonts and the theme's preference, once: the widgets' look follows the profile ([`follow`]).
pub fn apply(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| {
        o.theme_preference = ThemePreference::System;
        o.fallback_theme = Theme::Light;
    });
}

/// The lettering's sizes, the spacing and the widgets' colours and corners, in both themes, for `profile`.
fn visuals(ctx: &Context, look: Look) {
    let rev = look.profile == Profile::Rev;
    let palettes = [false, true].map(|dark| palette(dark, rev, look.scheme, look.accent));
    let (widget, window) = if rev { (2, 3) } else { (4, WINDOW_RADIUS) };
    for (theme, pal) in [Theme::Light, Theme::Dark].into_iter().zip(palettes) {
        ctx.style_mut_of(theme, |s| {
            for (style, size) in [(TextStyle::Heading, 15.0), (TextStyle::Body, 13.0), (TextStyle::Button, 13.0), (TextStyle::Small, 11.0)]
            {
                s.text_styles.insert(style, FontId::proportional(size));
            }
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
            s.spacing.item_spacing = vec2(6.0, 4.0);
            s.spacing.button_padding = vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
            s.spacing.menu_margin = Margin::same(6);
            s.spacing.window_margin = Margin::same(10);
            let v = &mut s.visuals;
            v.panel_fill = pal.panel;
            v.window_fill = pal.panel;
            v.extreme_bg_color = pal.surface;
            v.faint_bg_color = pal.bar;
            v.window_stroke = Stroke::new(1.0, pal.border);
            v.window_corner_radius = CornerRadius::same(window);
            v.menu_corner_radius = CornerRadius::same(window);
            v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: pal.shadow };
            v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: pal.shadow };
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.select_fill;
            v.selection.stroke = Stroke::new(1.0, pal.accent);
            v.text_cursor.stroke = Stroke::new(2.0, pal.accent);
            let radius = CornerRadius::same(widget);
            for w in
                [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
            {
                w.corner_radius = radius;
                w.expansion = 0.0;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.weak_bg_fill = pal.control;
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.hovered.weak_bg_fill = pal.hover_fill;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.hover);
            v.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.active.weak_bg_fill = pal.select_fill;
            v.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
            v.widgets.active.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.open.weak_bg_fill = pal.select_fill;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_is_wcags_ratio() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
        // What the Dark theme's open contextual tab once was, white on that green.
        assert!((contrast(Color32::WHITE, rgb(0x4ADE80)) - 1.74).abs() < 0.01);
    }

    /// What the window draws one colour over another, in both themes. Text holds 4.5:1 (WCAG 1.4.3); an icon, a focus ring and
    /// the like hold 3:1 (1.4.11).
    #[test]
    fn what_is_read_holds_its_contrast_on_what_it_sits_on_in_both_themes() {
        let mut fails = Vec::new();
        let mut looks: Vec<(String, Palette, bool, bool)> = vec![
            ("light".into(), LIGHT, false, false),
            ("dark".into(), DARK, true, false),
            ("Rev light".into(), REV_LIGHT, false, true),
            ("Rev dark".into(), REV_DARK, true, true),
        ];
        for s in Scheme::PRESETS {
            looks.push((s.name().into(), palette(true, false, s, None), true, false));
        }
        // An accent of every kind on every ground: a pale yellow, a dark navy, a mid grey, pure red.
        let bases = [(false, false, Scheme::Own), (true, false, Scheme::Own), (false, true, Scheme::Own), (true, true, Scheme::Own)];
        for accent in [[255, 255, 0], [0, 17, 51], [128, 128, 128], [255, 0, 0]] {
            for (dark, rev, scheme) in bases.into_iter().chain(Scheme::PRESETS.map(|s| (true, false, s))) {
                let name = format!("{accent:?} over {scheme:?}, dark {dark}, Rev {rev}");
                looks.push((name, palette(dark, rev, scheme, Some(accent)), dark, rev));
            }
        }
        for (theme, p, dark, rev) in &looks {
            let (dark, rev) = (*dark, *rev);
            DARK_ON.with(|d| d.set(dark));
            REV.with(|r| r.set(rev));
            CURRENT.with(|c| c.set(*p));
            let (ring, ring_on_bar) = (focus_ring().color, on_app_bar(|| focus_ring().color));
            // Text, on a card, a band, a field and a hovered row; some also on the open tab's tint.
            let on_cards = [p.panel, p.bar, p.surface, p.hover_fill];
            let on_tints = [p.panel, p.bar, p.surface, p.hover_fill, p.select_fill, p.control];
            let on_bar = [p.app_bar, p.app_bar_hover];
            let mut rows: Vec<(&str, Color32, Color32, f32)> = Vec::new();
            for (what, ink, grounds, least) in [
                ("text", p.text, &on_tints[..], 4.5),
                ("muted text", p.muted, &on_cards[..], 4.5),
                ("the accent as text and icons", p.accent_ink, &on_tints[..], 4.5),
                ("lettering on the app bar", p.app_bar_text, &on_bar[..], 4.5),
                ("dimmer lettering on the app bar", p.app_bar_muted, &on_bar[..], 4.5),
                ("the focus ring", ring, &on_tints[..], 3.0),
                ("the focus ring on the app bar", ring_on_bar, &on_bar[..], 3.0),
                ("icons", p.icon, &on_tints[..3], 3.0),
                ("error text", p.error, &on_tints[..3], 4.5),
            ] {
                rows.extend(grounds.iter().map(|ground| (what, ink, *ground, least)));
            }
            rows.extend([
                ("lettering on the open contextual tab", p.on_context, p.context, 4.5),
                ("an inactive contextual tab's lettering", p.context, p.context_fill, 4.5),
                ("a caption button's focus ring and glyph", p.app_bar_text, p.app_bar_hover, 3.0),
                ("Close's focus ring and glyph", Color32::WHITE, crate::titlebar::CLOSE_HOVER, 3.0),
            ]);
            // Rev's ribbon tabs and the button that folds the ribbon sit on the chrome, and File is white on its blue.
            if rev {
                rows.extend([
                    ("a ribbon tab's lettering on the chrome", p.text, p.app_bar, 4.5),
                    ("a hovered ribbon tab's lettering", p.text, p.hover_fill, 4.5),
                    ("an icon on the chrome", p.icon, p.app_bar, 3.0),
                    ("the File tab's lettering", Color32::WHITE, FILE_TAB, 4.5),
                ]);
            }
            for (what, ink, ground, least) in rows {
                let c = contrast(ink, ground);
                if c < least {
                    fails.push(format!("{theme}: {what} on {ground:?} is {c:.2}:1, under {least}:1"));
                }
            }
        }
        REV.with(|r| r.set(false));
        assert!(fails.is_empty(), "{fails:#?}");
    }

    /// The plain Light interface keeps the tints it has always had: the ribbon's pressed buttons, the open view tab and the selected
    /// rows draw `select_fill`, `hover` and `accent_ink`. Only the drawing asked for deeper ones, and those are `CANVAS_LIGHT`'s.
    #[test]
    fn the_light_interface_keeps_its_tints() {
        assert_eq!((LIGHT.accent_ink, LIGHT.hover, LIGHT.select_fill), (rgb(0x2064B5), rgb(0x5B9BE6), rgb(0xD6E6FA)));
    }

    /// What the drawing paints on its paper, from the canvas's palette: what a click would pick (hover) and what is picked are
    /// two colours a glance tells apart, in the fill and in the stroke (at least 35 apart in red, green and blue: the light
    /// palette's were 13 apart in the fill), and each stroke holds 3:1 on the paper (WCAG 1.4.11).
    #[test]
    fn on_the_canvas_hover_and_selection_are_told_apart_and_their_strokes_hold_3_to_1_on_the_paper() {
        let apart = |a: Color32, b: Color32| {
            let d = |x: u8, y: u8| (f32::from(x) - f32::from(y)).powi(2);
            (d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())).sqrt()
        };
        let mut fails = Vec::new();
        for canvas in ["light", "dark"] {
            CANVAS_DARK.with(|d| d.set(canvas == "dark"));
            let p = canvas_pal();
            let paper = crate::canvas::color(bimcraft_core::render::PAPER);
            for (what, a, b) in [("fills", p.hover_fill, p.select_fill), ("strokes", p.hover, p.accent)] {
                if apart(a, b) < 35.0 {
                    fails.push(format!("{canvas} canvas: hover and selection {what} are {:.0} apart", apart(a, b)));
                }
            }
            for (what, stroke) in [("hover", p.hover), ("selection", p.accent)] {
                let c = contrast(stroke, paper);
                if c < 3.0 {
                    fails.push(format!("{canvas} canvas: the {what} stroke is {c:.2}:1 on the paper, under 3:1"));
                }
            }
        }
        CANVAS_DARK.with(|d| d.set(false));
        assert!(fails.is_empty(), "{fails:#?}");
    }
}
