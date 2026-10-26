//! Look and feel: a lean desktop tool, light or dark (the system's theme until one is picked on the View tab).
//! A dark app bar on top; under it the view, and over it the ribbon, the Options Bar, the palettes and the status bar as
//! cards: rounded, a hairline border and a soft shadow, `GAP` off the edges and each other (the Floating layout), or the
//! same pieces flush and square (Compact). One blue accent for the active tool, selection and focus; IBM Plex for the
//! lettering; the icons are drawn in `icons.rs`.

use std::cell::Cell;
use std::sync::Arc;

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, vec2,
};

/// A theme's colours.
pub struct Palette {
    pub accent: Color32,
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
    pub border: Color32,
    pub error: Color32,
    /// Revit's contextual tabs (Modify | …) stand out in green.
    pub context: Color32,
    pub context_fill: Color32,
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
    hover: rgb(0x5B9BE6),
    select_fill: rgb(0xD6E6FA),
    hover_fill: rgb(0xE2EAF6),
    text: rgb(0x262A32),
    muted: rgb(0x6E7480),
    icon: rgb(0x3A404C),
    panel: rgb(0xFAFBFC),
    bar: rgb(0xECEEF2),
    border: rgb(0xD0D4DC),
    error: rgb(0xDC2626),
    context: rgb(0x15803D),
    context_fill: rgb(0xDCFCE7),
    surface: rgb(0xFFFFFF),
    desk: rgb(0xD4D4D4),
    app_bar: rgb(0x343840),
    app_bar_text: rgb(0xE6EAF2),
    app_bar_muted: rgb(0xA4AAB6),
    app_bar_hover: rgb(0x4A4F59),
    shadow: Color32::from_rgba_premultiplied(9, 10, 12, 46),
};

pub const DARK: Palette = Palette {
    accent: rgb(0x3A88EE),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0x2A466E),
    hover_fill: rgb(0x363D4A),
    text: rgb(0xDEE2E8),
    muted: rgb(0x949BA8),
    icon: rgb(0xCFD4DB),
    panel: rgb(0x272B34),
    bar: rgb(0x2F343E),
    border: rgb(0x3F4552),
    error: rgb(0xF87171),
    context: rgb(0x4ADE80),
    context_fill: rgb(0x14341F),
    surface: rgb(0x1E2229),
    desk: rgb(0x2A2E35),
    app_bar: rgb(0x1C1F26),
    app_bar_text: rgb(0xE6EAF2),
    app_bar_muted: rgb(0x949BA8),
    app_bar_hover: rgb(0x343A46),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 110),
};

/// How far the cards stand off the window's edges and each other, in the Floating layout.
pub const GAP: i8 = 7;
/// A card's corners, and a menu's or a dialog's.
pub const CARD_RADIUS: u8 = 8;
pub const WINDOW_RADIUS: u8 = 10;

thread_local! {
    /// Whether this frame is drawn dark: each window draws on its own thread.
    static DARK_ON: Cell<bool> = const { Cell::new(false) };
    /// Whether this frame lays the window out flush (Compact) rather than in cards (Floating).
    static COMPACT: Cell<bool> = const { Cell::new(false) };
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

/// An icon's ink and its hover's fill where it's drawn: on the app bar or on a card.
pub fn icon_inks() -> (Color32, Color32) {
    if ON_APP_BAR.with(Cell::get) { (pal().app_bar_text, pal().app_bar_hover) } else { (pal().icon, pal().hover_fill) }
}

/// The colours of the theme this frame is drawn in.
pub fn pal() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
}

pub fn dark() -> bool {
    DARK_ON.with(Cell::get)
}

/// Draw this frame in the theme egui shows: the system's, unless the user picked one; in the layout the user picked.
pub fn follow(ctx: &Context, compact: bool) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
    COMPACT.with(|c| c.set(compact));
    let semibold = ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(SEMIBOLD.into())));
    SEMIBOLD_ON.with(|s| s.set(semibold));
}

pub fn compact() -> bool {
    COMPACT.with(Cell::get)
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

/// The lettering at a weight heavier than the body's: a title, a tab. The body's until the window has it.
pub fn semibold(size: f32) -> FontId {
    match SEMIBOLD_ON.with(Cell::get) {
        true => FontId::new(size, FontFamily::Name(SEMIBOLD.into())),
        false => FontId::proportional(size),
    }
}

const SEMIBOLD: &str = "plex-semibold";
const PLEX_SANS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf"));
const PLEX_MONO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"));

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

pub fn apply(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| {
        o.theme_preference = ThemePreference::System;
        o.fallback_theme = Theme::Light;
    });
    for (theme, pal) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
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
            v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
            v.menu_corner_radius = CornerRadius::same(WINDOW_RADIUS);
            v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: pal.shadow };
            v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: pal.shadow };
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.select_fill;
            v.selection.stroke = Stroke::new(1.0, pal.accent);
            v.text_cursor.stroke = Stroke::new(2.0, pal.accent);
            let radius = CornerRadius::same(4);
            for w in
                [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
            {
                w.corner_radius = radius;
                w.expansion = 0.0;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.weak_bg_fill = pal.bar;
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
