//! Look and feel: a lean desktop tool, light or dark (the system's theme until one is picked on the View tab).
//! A dark app bar on top; under it the view, and over it the ribbon, the Options Bar, the palettes and the status bar as
//! cards: rounded, a hairline border and a soft shadow, `GAP` off the edges and each other (the Floating layout), or the
//! same pieces flush and square (Compact). One blue accent for the active tool, selection and focus; IBM Plex for the
//! lettering; the icons are drawn in `icons.rs`.
//! A dark interface may keep the drawing canvas light; canvas painters use `canvas_dark`, chrome uses `dark`.

use std::cell::Cell;
use std::sync::Arc;

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, Ui, vec2,
};

/// A theme's colours.
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

/// The colours of the theme this frame is drawn in.
pub fn pal() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
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

/// How the window is drawn beyond the theme: what the user kept on the View tab and in Settings.
#[derive(Clone, Copy, Default)]
pub struct Look {
    /// The drawing stays light in a dark interface.
    pub light_canvas: bool,
    /// The cards sit flush and square (Compact) rather than floating.
    pub compact: bool,
}

/// Draw this frame in the theme egui shows: the system's, unless the user picked one; in the look the user kept.
pub fn follow(ctx: &Context, look: Look) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
    CANVAS_DARK.with(|d| d.set(dark() && !look.light_canvas));
    COMPACT.with(|c| c.set(look.compact));
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

    /// WCAG 2.x's contrast ratio of two opaque colours: (L1 + 0.05) / (L2 + 0.05), L the relative luminance.
    fn contrast(a: Color32, b: Color32) -> f32 {
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
        for (theme, p) in [("light", &LIGHT), ("dark", &DARK)] {
            DARK_ON.with(|d| d.set(theme == "dark"));
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
            for (what, ink, ground, least) in rows {
                let c = contrast(ink, ground);
                if c < least {
                    fails.push(format!("{theme}: {what} on {ground:?} is {c:.2}:1, under {least}:1"));
                }
            }
        }
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
