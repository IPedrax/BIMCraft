//! Look and feel: a dense desktop tool, light or dark (the system's theme until one is picked on
//! the View tab). Slate chrome around the drawing, one blue accent for the active tool, selection
//! and focus, an 8 px rhythm, and icons drawn in code as 1.5 px strokes on a 24 px grid.

use std::cell::Cell;
use std::f32::consts::FRAC_PI_2;

use egui::{Color32, Context, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, TextStyle, Theme, ThemePreference, vec2};

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
    pub panel: Color32,
    pub bar: Color32,
    pub border: Color32,
    pub error: Color32,
    /// Revit's contextual tabs (Modify | …) stand out in green.
    pub context: Color32,
    pub context_fill: Color32,
    /// What the ribbon, popups and fields sit on.
    pub surface: Color32,
    /// Around a sheet's paper.
    pub desk: Color32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub const LIGHT: Palette = Palette {
    accent: rgb(0x2563EB),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0xBFDBFE),
    hover_fill: rgb(0xDBEAFE),
    text: rgb(0x0F172A),
    muted: rgb(0x475569),
    icon: rgb(0x334155),
    panel: rgb(0xF8FAFC),
    bar: rgb(0xF1F5F9),
    border: rgb(0xE2E8F0),
    error: rgb(0xDC2626),
    context: rgb(0x15803D),
    context_fill: rgb(0xDCFCE7),
    surface: rgb(0xFFFFFF),
    desk: rgb(0xD4D4D4),
};

pub const DARK: Palette = Palette {
    accent: rgb(0x3B82F6),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0x1E3A5F),
    hover_fill: rgb(0x24334A),
    text: rgb(0xE2E8F0),
    muted: rgb(0x94A3B8),
    icon: rgb(0xCBD5E1),
    panel: rgb(0x1E2228),
    bar: rgb(0x262B33),
    border: rgb(0x3A414B),
    error: rgb(0xF87171),
    context: rgb(0x4ADE80),
    context_fill: rgb(0x14341F),
    surface: rgb(0x171A1F),
    desk: rgb(0x2A2E35),
};

thread_local! {
    /// Whether this frame is drawn dark: each window draws on its own thread.
    static DARK_ON: Cell<bool> = const { Cell::new(false) };
}

/// The colours of the theme this frame is drawn in.
pub fn pal() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
}

pub fn dark() -> bool {
    DARK_ON.with(Cell::get)
}

/// Draw this frame in the theme egui shows: the system's, unless the user picked one.
pub fn follow(ctx: &Context) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
}

pub fn apply(ctx: &Context) {
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
            s.spacing.item_spacing = vec2(8.0, 4.0);
            s.spacing.button_padding = vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
            let v = &mut s.visuals;
            v.panel_fill = pal.panel;
            v.window_fill = pal.surface;
            v.extreme_bg_color = pal.surface;
            v.faint_bg_color = pal.bar;
            v.window_stroke = Stroke::new(1.0, pal.border);
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.select_fill;
            v.selection.stroke = Stroke::new(1.0, pal.accent);
            let radius = CornerRadius::same(4);
            for w in
                [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
            {
                w.corner_radius = radius;
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
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Modify,
    Wall,
    Door,
    Window,
    Room,
    Level,
    Grid,
    Undo,
    Redo,
    Open,
    Save,
    Fit,
    ZoomIn,
    ZoomOut,
    ThinLines,
    Schedule,
    CloseInactive,
    Interface,
    Properties,
    TypeProperties,
    Delete,
    Filter,
    Close,
    Plan,
    Move,
    Copy,
    Rotate,
    MirrorPick,
    MirrorDraw,
    Align,
    Offset,
    Trim,
    Split,
    Dimension,
    Tag,
    TagAll,
    View3D,
    Section,
    Elevation,
    Sheet,
    PlaceView,
    Column,
    Beam,
    Footing,
    WallFooting,
    Pipe,
    Fixture,
    Fitting,
    Accessory,
    Conduit,
    CableTray,
    Wire,
    Equipment,
    Device,
    Luminaire,
    AirTerminal,
    DownConductor,
    Electrode,
    InspectionBox,
    GroundConductor,
    DataOutlet,
    VoiceOutlet,
    Rack,
    PatchPanel,
    Cabling,
    Stair,
    Roof,
    Theme,
    Text,
    Spot,
    Angular,
    Radial,
    Crop,
    Units,
    Line,
    Arc,
    Flip,
    Attach,
    Detach,
    Floor,
    Ceiling,
    Shaft,
    Mass,
    SlopeArrow,
    Ramp,
    Railing,
    ExtrusionRoof,
    Dormer,
    GableEdge,
    Materials,
    Component,
    LoadFamily,
    Parameters,
    Measure,
    Snaps,
    Visibility,
    Template,
    Phasing,
    Options,
    SelectById,
    IdsOfSelection,
    Demolish,
    Paint,
    Unpaint,
    Favorite,
    FamilyEditor,
    TempHide,
    Reveal,
}

/// Points around `c` at radius `r` from angle `a0` to `a1` (radians, y down).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=12).map(|i| a0 + (a1 - a0) * i as f32 / 12.0).map(|a| c + vec2(a.cos(), a.sin()) * r).collect()
}

/// Draws `icon` into `r`, designed on a 24 x 24 grid.
pub fn paint(p: &Painter, icon: Icon, r: Rect, color: Color32) {
    let k = r.width() / 24.0;
    let mirror = matches!(icon, Icon::Redo);
    let at = |x: f32, y: f32| r.min + vec2(if mirror { 24.0 - x } else { x }, y) * k;
    let s = Stroke::new(1.5, color);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s);
    let ring = |pts: &[(f32, f32)]| Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s);
    let shapes = match icon {
        Icon::Modify => vec![ring(&[(6.0, 3.0), (6.0, 19.0), (10.0, 15.0), (13.0, 21.0), (15.5, 20.0), (12.5, 14.0), (18.0, 14.0)])],
        Icon::Wall => vec![
            ring(&[(2.0, 8.0), (22.0, 8.0), (22.0, 16.0), (2.0, 16.0)]),
            line(&[(5.0, 16.0), (9.0, 8.0)]),
            line(&[(10.0, 16.0), (14.0, 8.0)]),
            line(&[(15.0, 16.0), (19.0, 8.0)]),
        ],
        Icon::Door => vec![
            line(&[(2.0, 20.0), (6.0, 20.0)]),
            line(&[(18.0, 20.0), (22.0, 20.0)]),
            line(&[(6.0, 20.0), (6.0, 8.0)]),
            Shape::line(arc(at(6.0, 20.0), 12.0 * k, -FRAC_PI_2, 0.0), s),
        ],
        Icon::Window => vec![
            ring(&[(2.0, 9.0), (22.0, 9.0), (22.0, 15.0), (2.0, 15.0)]),
            line(&[(2.0, 12.0), (22.0, 12.0)]),
            line(&[(6.0, 9.0), (6.0, 15.0)]),
            line(&[(18.0, 9.0), (18.0, 15.0)]),
        ],
        Icon::Room => vec![
            ring(&[(3.0, 3.0), (21.0, 3.0), (21.0, 21.0), (3.0, 21.0)]),
            line(&[(8.0, 11.0), (16.0, 11.0)]),
            line(&[(9.5, 14.5), (14.5, 14.5)]),
        ],
        // A level line with its head: a triangle standing on the line's end.
        Icon::Level => vec![line(&[(2.0, 16.0), (22.0, 16.0)]), ring(&[(13.0, 8.0), (21.0, 8.0), (17.0, 16.0)])],
        Icon::Grid => vec![Shape::circle_stroke(at(12.0, 6.0), 4.0 * k, s), line(&[(12.0, 10.0), (12.0, 22.0)])],
        Icon::Undo | Icon::Redo => {
            let mut curve = vec![at(7.0, 9.0), at(15.0, 9.0)];
            curve.extend(
                arc(at(15.0, 14.0), 5.0 * k, -FRAC_PI_2, FRAC_PI_2)
                    .into_iter()
                    .map(|q| if mirror { Pos2::new(2.0 * at(15.0, 14.0).x - q.x, q.y) } else { q }),
            );
            curve.push(at(9.0, 19.0));
            vec![Shape::line(curve, s), line(&[(10.5, 5.5), (7.0, 9.0), (10.5, 12.5)])]
        }
        Icon::Open => vec![ring(&[(3.0, 6.0), (9.0, 6.0), (11.0, 8.5), (21.0, 8.5), (21.0, 19.0), (3.0, 19.0)])],
        Icon::Save => vec![
            line(&[(12.0, 4.0), (12.0, 15.0)]),
            line(&[(8.0, 11.0), (12.0, 15.0), (16.0, 11.0)]),
            line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]),
        ],
        Icon::Fit => vec![
            line(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]),
            line(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]),
            line(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]),
            line(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]),
        ],
        Icon::ZoomIn | Icon::ZoomOut => {
            let mut v = vec![
                Shape::circle_stroke(at(10.0, 10.0), 6.5 * k, s),
                line(&[(14.8, 14.8), (20.5, 20.5)]),
                line(&[(7.0, 10.0), (13.0, 10.0)]),
            ];
            if matches!(icon, Icon::ZoomIn) {
                v.push(line(&[(10.0, 7.0), (10.0, 13.0)]));
            }
            v
        }
        Icon::ThinLines => vec![line(&[(3.0, 7.0), (21.0, 7.0)]), line(&[(3.0, 12.0), (21.0, 12.0)]), line(&[(3.0, 17.0), (21.0, 17.0)])],
        Icon::Schedule => vec![
            ring(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]),
            line(&[(3.0, 9.0), (21.0, 9.0)]),
            line(&[(3.0, 14.5), (21.0, 14.5)]),
            line(&[(10.0, 4.0), (10.0, 20.0)]),
        ],
        Icon::CloseInactive => {
            vec![ring(&[(3.0, 4.0), (15.0, 4.0), (15.0, 14.0), (3.0, 14.0)]), ring(&[(9.0, 10.0), (21.0, 10.0), (21.0, 20.0), (9.0, 20.0)])]
        }
        Icon::Interface => vec![
            ring(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]),
            line(&[(3.0, 8.0), (21.0, 8.0)]),
            line(&[(9.0, 8.0), (9.0, 20.0)]),
        ],
        Icon::Properties => vec![
            ring(&[(4.0, 3.0), (20.0, 3.0), (20.0, 21.0), (4.0, 21.0)]),
            line(&[(7.0, 8.0), (17.0, 8.0)]),
            line(&[(7.0, 12.0), (17.0, 12.0)]),
            line(&[(7.0, 16.0), (13.0, 16.0)]),
        ],
        Icon::TypeProperties => vec![
            ring(&[(4.0, 3.0), (20.0, 3.0), (20.0, 21.0), (4.0, 21.0)]),
            line(&[(8.0, 8.0), (16.0, 8.0)]),
            line(&[(12.0, 8.0), (12.0, 17.0)]),
        ],
        Icon::Delete | Icon::Close => vec![line(&[(6.0, 6.0), (18.0, 18.0)]), line(&[(18.0, 6.0), (6.0, 18.0)])],
        Icon::Filter => vec![ring(&[(3.0, 4.0), (21.0, 4.0), (14.0, 12.0), (14.0, 19.0), (10.0, 21.0), (10.0, 12.0)])],
        Icon::Move => vec![
            line(&[(12.0, 3.0), (12.0, 21.0)]),
            line(&[(3.0, 12.0), (21.0, 12.0)]),
            line(&[(9.0, 6.0), (12.0, 3.0), (15.0, 6.0)]),
            line(&[(9.0, 18.0), (12.0, 21.0), (15.0, 18.0)]),
            line(&[(6.0, 9.0), (3.0, 12.0), (6.0, 15.0)]),
            line(&[(18.0, 9.0), (21.0, 12.0), (18.0, 15.0)]),
        ],
        Icon::Copy => vec![
            ring(&[(8.0, 3.0), (20.0, 3.0), (20.0, 15.0), (8.0, 15.0)]),
            line(&[(8.0, 9.0), (4.0, 9.0), (4.0, 21.0), (16.0, 21.0), (16.0, 15.0)]),
        ],
        Icon::Rotate => {
            let mut v = vec![Shape::line(arc(at(12.0, 12.0), 7.5 * k, -2.6, 2.0), s)];
            let tip = at(12.0, 12.0) + vec2(2.0f32.cos(), 2.0f32.sin()) * 7.5 * k;
            v.push(Shape::line(vec![tip + vec2(-4.0, -1.0) * k, tip, tip + vec2(1.0, -4.0) * k], s));
            v
        }
        Icon::MirrorPick | Icon::MirrorDraw => {
            let left = ring(&[(3.0, 18.0), (9.0, 6.0), (9.0, 18.0)]);
            let right = ring(&[(21.0, 18.0), (15.0, 6.0), (15.0, 18.0)]);
            let axis = if matches!(icon, Icon::MirrorPick) {
                Shape::dashed_line(&[at(12.0, 2.0), at(12.0, 22.0)], s, 2.5 * k, 2.0 * k)
            } else {
                vec![line(&[(12.0, 2.0), (12.0, 22.0)]), line(&[(10.0, 2.0), (14.0, 2.0)]), line(&[(10.0, 22.0), (14.0, 22.0)])]
            };
            let mut v = vec![left, right];
            v.extend(axis);
            v
        }
        Icon::Align => vec![
            line(&[(5.0, 3.0), (5.0, 21.0)]),
            line(&[(18.0, 3.0), (18.0, 21.0)]),
            line(&[(15.0, 12.0), (8.0, 12.0)]),
            line(&[(11.0, 9.0), (8.0, 12.0), (11.0, 15.0)]),
        ],
        Icon::Offset => vec![
            line(&[(3.0, 15.0), (13.0, 5.0)]),
            line(&[(9.0, 21.0), (21.0, 9.0)]),
            line(&[(9.0, 10.0), (14.0, 15.0)]),
            line(&[(14.0, 11.5), (14.0, 15.0), (10.5, 15.0)]),
        ],
        Icon::Trim => vec![line(&[(6.0, 21.0), (6.0, 6.0), (21.0, 6.0)]), line(&[(6.0, 6.0), (3.0, 3.0)])],
        Icon::Split => vec![
            ring(&[(2.0, 9.0), (10.0, 9.0), (10.0, 15.0), (2.0, 15.0)]),
            ring(&[(14.0, 9.0), (22.0, 9.0), (22.0, 15.0), (14.0, 15.0)]),
            line(&[(11.0, 20.0), (13.0, 4.0)]),
        ],
        Icon::Dimension => vec![
            line(&[(4.0, 7.0), (4.0, 20.0)]),
            line(&[(20.0, 7.0), (20.0, 20.0)]),
            line(&[(2.0, 16.0), (22.0, 16.0)]),
            line(&[(2.5, 18.0), (5.5, 14.0)]),
            line(&[(18.5, 18.0), (21.5, 14.0)]),
            line(&[(9.0, 11.0), (15.0, 11.0)]),
        ],
        Icon::Tag => vec![
            ring(&[(9.0, 4.0), (21.0, 4.0), (21.0, 12.0), (9.0, 12.0)]),
            line(&[(13.0, 8.0), (17.0, 8.0)]),
            line(&[(9.0, 12.0), (3.0, 20.0)]),
        ],
        Icon::TagAll => vec![
            ring(&[(2.0, 3.0), (11.0, 3.0), (11.0, 9.0), (2.0, 9.0)]),
            ring(&[(8.0, 10.0), (17.0, 10.0), (17.0, 16.0), (8.0, 16.0)]),
            ring(&[(13.0, 17.0), (22.0, 17.0), (22.0, 23.0), (13.0, 23.0)]),
        ],
        Icon::View3D => vec![
            ring(&[(12.0, 2.5), (20.5, 7.25), (20.5, 16.75), (12.0, 21.5), (3.5, 16.75), (3.5, 7.25)]),
            line(&[(3.5, 7.25), (12.0, 12.0), (20.5, 7.25)]),
            line(&[(12.0, 12.0), (12.0, 21.5)]),
        ],
        Icon::Section => vec![
            ring(&[(3.0, 14.0), (9.0, 14.0), (3.0, 7.0)]),
            line(&[(9.0, 14.0), (13.0, 14.0)]),
            line(&[(15.0, 14.0), (19.0, 14.0)]),
            line(&[(21.0, 11.0), (21.0, 17.0)]),
        ],
        Icon::Elevation => vec![Shape::circle_stroke(at(12.0, 15.0), 5.0 * k, s), ring(&[(8.0, 10.5), (16.0, 10.5), (12.0, 4.0)])],
        Icon::Sheet => vec![
            ring(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)]),
            ring(&[(13.0, 15.0), (21.0, 15.0), (21.0, 19.0), (13.0, 19.0)]),
        ],
        Icon::PlaceView => vec![
            ring(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)]),
            ring(&[(6.0, 8.0), (13.0, 8.0), (13.0, 14.0), (6.0, 14.0)]),
            line(&[(17.0, 9.0), (17.0, 15.0)]),
            line(&[(14.0, 12.0), (20.0, 12.0)]),
        ],
        // A column in elevation: its shaft between a base and a capital.
        Icon::Column => vec![
            ring(&[(9.0, 5.0), (15.0, 5.0), (15.0, 19.0), (9.0, 19.0)]),
            line(&[(6.0, 4.0), (18.0, 4.0)]),
            line(&[(6.0, 20.0), (18.0, 20.0)]),
        ],
        // A beam in section: an I, its flanges top and bottom.
        Icon::Beam => vec![
            line(&[(3.0, 5.0), (21.0, 5.0)]),
            line(&[(3.0, 19.0), (21.0, 19.0)]),
            line(&[(12.0, 5.0), (12.0, 19.0)]),
            line(&[(3.0, 5.0), (3.0, 8.0)]),
            line(&[(21.0, 5.0), (21.0, 8.0)]),
            line(&[(3.0, 19.0), (3.0, 16.0)]),
            line(&[(21.0, 19.0), (21.0, 16.0)]),
        ],
        // A column on its footing, which is wider and sits on the ground.
        Icon::Footing => vec![
            ring(&[(10.0, 3.0), (14.0, 3.0), (14.0, 12.0), (10.0, 12.0)]),
            ring(&[(4.0, 12.0), (20.0, 12.0), (20.0, 18.0), (4.0, 18.0)]),
            line(&[(2.0, 20.0), (22.0, 20.0)]),
        ],
        // A wall on a strip footing running along it.
        Icon::WallFooting => vec![
            ring(&[(9.0, 3.0), (15.0, 3.0), (15.0, 13.0), (9.0, 13.0)]),
            ring(&[(2.0, 13.0), (22.0, 13.0), (22.0, 18.0), (2.0, 18.0)]),
            line(&[(2.0, 20.0), (22.0, 20.0)]),
        ],
        // A pipe running up to the right, its ends open.
        Icon::Pipe => vec![
            line(&[(3.0, 17.0), (17.0, 3.0)]),
            line(&[(7.0, 21.0), (21.0, 7.0)]),
            line(&[(3.0, 17.0), (7.0, 21.0)]),
            line(&[(17.0, 3.0), (21.0, 7.0)]),
        ],
        // A conduit bending: a tube between two lines, turned up at its end.
        Icon::Conduit => vec![
            line(&[(3.0, 15.0), (14.0, 15.0), (14.0, 4.0)]),
            line(&[(3.0, 20.0), (19.0, 20.0), (19.0, 4.0)]),
            line(&[(14.0, 4.0), (19.0, 4.0)]),
        ],
        // A tray seen from the side: two rails and its rungs.
        Icon::CableTray => vec![
            line(&[(2.0, 8.0), (22.0, 8.0)]),
            line(&[(2.0, 16.0), (22.0, 16.0)]),
            line(&[(6.0, 8.0), (6.0, 16.0)]),
            line(&[(12.0, 8.0), (12.0, 16.0)]),
            line(&[(18.0, 8.0), (18.0, 16.0)]),
        ],
        // A wire, running in a gentle wave.
        Icon::Wire => vec![line(&[(2.0, 14.0), (6.0, 8.0), (10.0, 14.0), (14.0, 8.0), (18.0, 14.0), (22.0, 8.0)])],
        // An air terminal: a rod with a pointed tip, on its base.
        Icon::AirTerminal => vec![
            line(&[(12.0, 6.0), (12.0, 17.0)]),
            ring(&[(10.5, 6.0), (12.0, 2.0), (13.5, 6.0)]),
            ring(&[(8.0, 17.0), (16.0, 17.0), (19.0, 21.0), (5.0, 21.0)]),
        ],
        // A down conductor: a wire down a wall, held by its clips.
        Icon::DownConductor => vec![
            line(&[(12.0, 2.0), (12.0, 22.0)]),
            line(&[(9.0, 6.0), (15.0, 6.0)]),
            line(&[(9.0, 12.0), (15.0, 12.0)]),
            line(&[(9.0, 18.0), (15.0, 18.0)]),
        ],
        // The earth symbol, for an electrode: a lead and three bars, each shorter than the last.
        Icon::Electrode => vec![
            line(&[(12.0, 3.0), (12.0, 10.0)]),
            line(&[(6.0, 10.0), (18.0, 10.0)]),
            line(&[(8.5, 14.0), (15.5, 14.0)]),
            line(&[(11.0, 18.0), (13.0, 18.0)]),
        ],
        // An inspection box in the ground: its rim at the surface, its chamber below.
        Icon::InspectionBox => vec![
            ring(&[(5.0, 8.0), (19.0, 8.0), (19.0, 21.0), (5.0, 21.0)]),
            line(&[(2.0, 8.0), (22.0, 8.0)]),
            ring(&[(9.0, 12.0), (15.0, 12.0), (15.0, 17.0), (9.0, 17.0)]),
        ],
        // An earthing mesh: conductors crossing.
        Icon::GroundConductor => vec![
            line(&[(3.0, 8.0), (21.0, 8.0)]),
            line(&[(3.0, 16.0), (21.0, 16.0)]),
            line(&[(8.0, 3.0), (8.0, 21.0)]),
            line(&[(16.0, 3.0), (16.0, 21.0)]),
        ],
        // A data outlet: a plate with a jack, its contacts in a row.
        Icon::DataOutlet => vec![
            ring(&[(4.0, 5.0), (20.0, 5.0), (20.0, 19.0), (4.0, 19.0)]),
            ring(&[(8.0, 9.0), (16.0, 9.0), (16.0, 15.0), (8.0, 15.0)]),
            line(&[(9.5, 15.0), (9.5, 12.0)]),
            line(&[(12.0, 15.0), (12.0, 12.0)]),
            line(&[(14.5, 15.0), (14.5, 12.0)]),
        ],
        // A voice outlet: a plate with a handset.
        Icon::VoiceOutlet => vec![
            ring(&[(4.0, 5.0), (20.0, 5.0), (20.0, 19.0), (4.0, 19.0)]),
            line(&[(8.0, 10.0), (11.0, 8.0), (13.0, 8.0), (16.0, 10.0)]),
            line(&[(8.0, 10.0), (8.0, 12.0)]),
            line(&[(16.0, 10.0), (16.0, 12.0)]),
            line(&[(9.0, 15.0), (15.0, 15.0)]),
        ],
        // A rack: its frame, its units in rows.
        Icon::Rack => vec![
            ring(&[(6.0, 2.0), (18.0, 2.0), (18.0, 22.0), (6.0, 22.0)]),
            line(&[(6.0, 7.0), (18.0, 7.0)]),
            line(&[(6.0, 12.0), (18.0, 12.0)]),
            line(&[(6.0, 17.0), (18.0, 17.0)]),
        ],
        // A patch panel: a strip of ports.
        Icon::PatchPanel => vec![
            ring(&[(2.0, 8.0), (22.0, 8.0), (22.0, 16.0), (2.0, 16.0)]),
            line(&[(6.0, 10.0), (6.0, 14.0)]),
            line(&[(10.0, 10.0), (10.0, 14.0)]),
            line(&[(14.0, 10.0), (14.0, 14.0)]),
            line(&[(18.0, 10.0), (18.0, 14.0)]),
        ],
        // Cabling: three cables running together.
        Icon::Cabling => vec![
            line(&[(2.0, 7.0), (8.0, 7.0), (11.0, 12.0), (22.0, 12.0)]),
            line(&[(2.0, 12.0), (8.0, 12.0), (11.0, 7.0), (22.0, 7.0)]),
            line(&[(2.0, 17.0), (22.0, 17.0)]),
        ],
        // A distribution board: a box, its door and its breakers in rows.
        Icon::Equipment => vec![
            ring(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]),
            ring(&[(8.0, 6.0), (16.0, 6.0), (16.0, 10.0), (8.0, 10.0)]),
            line(&[(8.0, 13.0), (16.0, 13.0)]),
            line(&[(8.0, 16.0), (16.0, 16.0)]),
            line(&[(8.0, 19.0), (16.0, 19.0)]),
        ],
        // An outlet's plate, its two slots.
        Icon::Device => vec![
            ring(&[(5.0, 4.0), (19.0, 4.0), (19.0, 20.0), (5.0, 20.0)]),
            line(&[(9.0, 9.0), (9.0, 12.0)]),
            line(&[(15.0, 9.0), (15.0, 12.0)]),
            line(&[(12.0, 15.0), (12.0, 17.0)]),
        ],
        // A luminaire hung from the ceiling: its cord, its shade, its rays.
        Icon::Luminaire => vec![
            line(&[(12.0, 2.0), (12.0, 8.0)]),
            ring(&[(5.0, 15.0), (8.0, 8.0), (16.0, 8.0), (19.0, 15.0)]),
            line(&[(8.0, 19.0), (6.0, 22.0)]),
            line(&[(12.0, 19.0), (12.0, 22.0)]),
            line(&[(16.0, 19.0), (18.0, 22.0)]),
        ],
        // A washbasin with its tap, on its pedestal.
        Icon::Fixture => vec![
            ring(&[(4.0, 10.0), (20.0, 10.0), (17.0, 16.0), (7.0, 16.0)]),
            line(&[(12.0, 10.0), (12.0, 5.0), (16.0, 5.0)]),
            line(&[(12.0, 16.0), (12.0, 21.0)]),
            line(&[(8.0, 21.0), (16.0, 21.0)]),
        ],
        // An elbow: two pipes meeting at a bend, a flange on each end.
        Icon::Fitting => vec![
            line(&[(4.0, 8.0), (15.0, 8.0), (15.0, 19.0)]),
            line(&[(4.0, 14.0), (9.0, 14.0), (9.0, 19.0)]),
            line(&[(4.0, 6.0), (4.0, 16.0)]),
            line(&[(7.0, 19.0), (17.0, 19.0)]),
        ],
        // A gate valve: two cones meeting, a stem and its wheel.
        Icon::Accessory => vec![
            ring(&[(3.0, 12.0), (3.0, 20.0), (12.0, 16.0)]),
            ring(&[(21.0, 12.0), (21.0, 20.0), (12.0, 16.0)]),
            line(&[(12.0, 16.0), (12.0, 6.0)]),
            line(&[(8.0, 6.0), (16.0, 6.0)]),
        ],
        Icon::Stair => vec![ring(&[
            (3.0, 20.0),
            (3.0, 16.0),
            (8.0, 16.0),
            (8.0, 12.0),
            (13.0, 12.0),
            (13.0, 8.0),
            (18.0, 8.0),
            (18.0, 4.0),
            (21.0, 4.0),
            (21.0, 20.0),
        ])],
        Icon::Roof => vec![
            ring(&[(2.0, 14.0), (12.0, 5.0), (22.0, 14.0), (19.0, 14.0), (12.0, 8.5), (5.0, 14.0)]),
            line(&[(6.0, 14.0), (6.0, 20.0), (18.0, 20.0), (18.0, 14.0)]),
        ],
        // A capital A.
        Icon::Text => vec![line(&[(5.0, 20.0), (12.0, 4.0), (19.0, 20.0)]), line(&[(8.0, 14.0), (16.0, 14.0)])],
        // A triangle standing on a surface, its height written over a line beside it.
        Icon::Spot => vec![
            ring(&[(5.0, 9.0), (13.0, 9.0), (9.0, 16.0)]),
            line(&[(2.0, 16.0), (22.0, 16.0)]),
            line(&[(13.0, 9.0), (21.0, 9.0)]),
            line(&[(15.0, 5.0), (21.0, 5.0)]),
        ],
        // Two lines from a corner, and the arc between them.
        Icon::Angular => {
            let mut v = vec![line(&[(4.0, 20.0), (21.0, 20.0)]), line(&[(4.0, 20.0), (16.0, 5.0)])];
            v.push(Shape::line(arc(at(4.0, 20.0), 11.0 * k, -0.9, 0.0), s));
            v
        }
        // A circle, its centre, and a radius out to it.
        Icon::Radial => vec![
            Shape::circle_stroke(at(12.0, 12.0), 8.0 * k, s),
            Shape::circle_filled(at(12.0, 12.0), 1.5 * k, color),
            line(&[(12.0, 12.0), (17.7, 6.3)]),
            line(&[(14.5, 6.5), (17.7, 6.3), (17.5, 9.5)]),
        ],
        // A rule with its marks.
        Icon::Units => vec![
            ring(&[(2.0, 8.0), (22.0, 8.0), (22.0, 16.0), (2.0, 16.0)]),
            line(&[(6.0, 8.0), (6.0, 12.0)]),
            line(&[(10.0, 8.0), (10.0, 11.0)]),
            line(&[(14.0, 8.0), (14.0, 12.0)]),
            line(&[(18.0, 8.0), (18.0, 11.0)]),
        ],
        // A line between two points.
        Icon::Line => vec![
            line(&[(5.0, 19.0), (19.0, 5.0)]),
            ring(&[(2.5, 16.5), (7.5, 16.5), (7.5, 21.5), (2.5, 21.5)]),
            ring(&[(16.5, 2.5), (21.5, 2.5), (21.5, 7.5), (16.5, 7.5)]),
        ],
        // An arc between two points, through a third.
        Icon::Arc => vec![
            Shape::line(arc(at(12.0, 22.0), 10.0 * k, -2.8, -0.34), s),
            ring(&[(0.5, 15.0), (5.0, 15.0), (5.0, 19.5), (0.5, 19.5)]),
            ring(&[(19.0, 15.0), (23.5, 15.0), (23.5, 19.5), (19.0, 19.5)]),
            Shape::circle_filled(at(12.0, 12.0), 1.6 * k, color),
        ],
        // A wall, and the arrows that turn it round.
        Icon::Flip => vec![
            ring(&[(2.0, 10.0), (22.0, 10.0), (22.0, 14.0), (2.0, 14.0)]),
            line(&[(12.0, 2.0), (12.0, 8.0)]),
            line(&[(9.0, 5.0), (12.0, 2.0), (15.0, 5.0)]),
            line(&[(12.0, 16.0), (12.0, 22.0)]),
            line(&[(9.0, 19.0), (12.0, 22.0), (15.0, 19.0)]),
        ],
        // A wall reaching up to the roof over it.
        Icon::Attach => vec![
            line(&[(2.0, 9.0), (12.0, 3.0), (22.0, 9.0)]),
            ring(&[(8.0, 12.0), (16.0, 12.0), (16.0, 22.0), (8.0, 22.0)]),
            line(&[(12.0, 12.0), (12.0, 6.5)]),
            line(&[(10.0, 8.5), (12.0, 6.5), (14.0, 8.5)]),
        ],
        // A wall come loose from the roof.
        Icon::Detach => vec![
            line(&[(2.0, 9.0), (12.0, 3.0), (22.0, 9.0)]),
            ring(&[(8.0, 14.0), (16.0, 14.0), (16.0, 22.0), (8.0, 22.0)]),
            line(&[(12.0, 7.0), (12.0, 12.0)]),
            line(&[(10.0, 10.0), (12.0, 12.0), (14.0, 10.0)]),
        ],
        // A slab, from above and to one side: its top and its edges.
        Icon::Floor => vec![
            ring(&[(2.0, 12.0), (13.0, 7.0), (22.0, 11.0), (11.0, 16.0)]),
            line(&[(2.0, 12.0), (2.0, 15.0), (11.0, 19.0), (22.0, 14.0), (22.0, 11.0)]),
            line(&[(11.0, 16.0), (11.0, 19.0)]),
        ],
        // A tiled ceiling hung over a room's floor.
        Icon::Ceiling => vec![
            ring(&[(2.0, 5.0), (22.0, 5.0), (22.0, 9.0), (2.0, 9.0)]),
            line(&[(8.7, 5.0), (8.7, 9.0)]),
            line(&[(15.3, 5.0), (15.3, 9.0)]),
            line(&[(6.0, 2.0), (6.0, 5.0)]),
            line(&[(18.0, 2.0), (18.0, 5.0)]),
            line(&[(2.0, 21.0), (22.0, 21.0)]),
        ],
        // A shaft in plan: its outline, crossed.
        Icon::Shaft => vec![
            ring(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]),
            line(&[(4.0, 4.0), (20.0, 20.0)]),
            line(&[(20.0, 4.0), (4.0, 20.0)]),
        ],
        // A mass: a block seen from above and to one side.
        Icon::Mass => vec![
            ring(&[(3.0, 9.0), (15.0, 9.0), (15.0, 21.0), (3.0, 21.0)]),
            line(&[(3.0, 9.0), (8.0, 4.0), (20.0, 4.0), (15.0, 9.0)]),
            line(&[(20.0, 4.0), (20.0, 16.0), (15.0, 21.0)]),
        ],
        // A sloped slab, and the arrow up it.
        Icon::SlopeArrow => vec![
            ring(&[(2.0, 18.0), (22.0, 9.0), (22.0, 12.0), (2.0, 21.0)]),
            line(&[(5.0, 12.5), (18.0, 6.5)]),
            line(&[(14.2, 5.6), (18.0, 6.5), (16.0, 9.8)]),
        ],
        // A sphere, lit from the top left: a material swatch.
        Icon::Materials => {
            let mut v = vec![Shape::circle_stroke(at(12.0, 12.0), 9.0 * k, s)];
            v.push(Shape::line(arc(at(12.0, 12.0), 5.5 * k, 3.4, 4.6), s));
            v.push(line(&[(3.0, 13.0), (21.0, 13.0)]));
            v
        }
        // A chair from its side: its back, its seat, its legs.
        Icon::Component => {
            vec![line(&[(7.0, 3.0), (7.0, 21.0)]), line(&[(7.0, 13.0), (18.0, 13.0), (18.0, 21.0)]), line(&[(4.0, 21.0), (21.0, 21.0)])]
        }
        // A ruler: a bar with its graduations, longer every other one.
        Icon::Measure => vec![
            ring(&[(2.0, 8.0), (22.0, 8.0), (22.0, 16.0), (2.0, 16.0)]),
            line(&[(6.0, 8.0), (6.0, 12.0)]),
            line(&[(10.0, 8.0), (10.0, 14.0)]),
            line(&[(14.0, 8.0), (14.0, 12.0)]),
            line(&[(18.0, 8.0), (18.0, 14.0)]),
        ],
        // A point snapped to: a square on a cross-hair.
        Icon::Snaps => vec![
            ring(&[(7.0, 7.0), (17.0, 7.0), (17.0, 17.0), (7.0, 17.0)]),
            line(&[(12.0, 2.0), (12.0, 22.0)]),
            line(&[(2.0, 12.0), (22.0, 12.0)]),
        ],
        // An eye: what a view shows.
        Icon::Visibility => vec![
            ring(&[(2.0, 12.0), (7.0, 7.0), (12.0, 5.5), (17.0, 7.0), (22.0, 12.0), (17.0, 17.0), (12.0, 18.5), (7.0, 17.0)]),
            Shape::circle_stroke(at(12.0, 12.0), 3.0 * k, s),
        ],
        // A page, a corner turned down, and its lines of settings.
        Icon::Template => vec![
            ring(&[(5.0, 2.0), (15.0, 2.0), (19.0, 6.0), (19.0, 22.0), (5.0, 22.0)]),
            line(&[(15.0, 2.0), (15.0, 6.0), (19.0, 6.0)]),
            line(&[(8.0, 11.0), (16.0, 11.0)]),
            line(&[(8.0, 15.0), (16.0, 15.0)]),
        ],
        // Steps in time: a stair climbing to the right.
        Icon::Phasing => vec![
            line(&[(2.0, 20.0), (8.0, 20.0), (8.0, 14.0), (14.0, 14.0), (14.0, 8.0), (20.0, 8.0), (20.0, 3.0)]),
            line(&[(2.0, 22.0), (22.0, 22.0)]),
        ],
        // A hammer, its head across the end of its handle, and the rubble it leaves.
        Icon::Demolish => vec![
            ring(&[(11.2, 2.5), (22.0, 11.3), (18.8, 15.1), (8.0, 6.3)]),
            line(&[(5.0, 21.0), (13.4, 10.7)]),
            line(&[(15.5, 20.0), (19.0, 20.0)]),
            line(&[(18.0, 17.5), (21.5, 17.5)]),
        ],
        // A bucket tipped over, with what it pours; the same with a cross through it.
        Icon::Paint | Icon::Unpaint => {
            let mut bucket = vec![
                ring(&[(4.0, 9.0), (13.0, 4.0), (18.0, 12.0), (9.0, 17.0)]),
                line(&[(8.0, 6.5), (6.0, 3.0), (11.0, 1.5)]),
                line(&[(18.0, 12.0), (20.5, 17.0)]),
                Shape::circle_stroke(at(20.5, 19.5), 1.5 * k, s),
            ];
            if matches!(icon, Icon::Unpaint) {
                bucket.extend([line(&[(2.0, 22.0), (10.0, 14.0)]), line(&[(2.0, 14.0), (10.0, 22.0)])]);
            }
            bucket
        }
        // A star.
        Icon::Favorite => vec![ring(&[
            (12.0, 3.0),
            (14.2, 8.9),
            (20.6, 9.2),
            (15.6, 13.2),
            (17.3, 19.3),
            (12.0, 15.8),
            (6.7, 19.3),
            (8.4, 13.2),
            (3.4, 9.2),
            (9.8, 8.9),
        ])],
        // A box seen corner on: a family is a thing of three dimensions.
        Icon::FamilyEditor => vec![
            ring(&[(12.0, 2.0), (21.0, 7.0), (21.0, 17.0), (12.0, 22.0), (3.0, 17.0), (3.0, 7.0)]),
            line(&[(3.0, 7.0), (12.0, 12.0), (21.0, 7.0)]),
            line(&[(12.0, 12.0), (12.0, 22.0)]),
        ],
        // A fork: one design branching into its options.
        // A magnifier over a number sign: an element found by what names it.
        Icon::SelectById => vec![
            Shape::circle_stroke(at(10.0, 10.0), 7.0 * k, s),
            line(&[(15.0, 15.0), (22.0, 22.0)]),
            line(&[(8.5, 6.5), (7.5, 13.5)]),
            line(&[(12.5, 6.5), (11.5, 13.5)]),
            line(&[(6.0, 8.5), (14.0, 8.5)]),
            line(&[(6.0, 11.5), (14.0, 11.5)]),
        ],
        // A label on a string, its lines what it says: what names the selection.
        Icon::IdsOfSelection => vec![
            ring(&[(9.0, 4.0), (21.0, 4.0), (21.0, 20.0), (9.0, 20.0), (3.0, 12.0)]),
            Shape::circle_stroke(at(8.0, 12.0), 1.5 * k, s),
            line(&[(12.0, 9.0), (18.0, 9.0)]),
            line(&[(12.0, 12.0), (18.0, 12.0)]),
            line(&[(12.0, 15.0), (16.0, 15.0)]),
        ],
        Icon::Options => vec![
            line(&[(3.0, 12.0), (10.0, 12.0)]),
            line(&[(10.0, 12.0), (16.0, 6.0)]),
            line(&[(10.0, 12.0), (16.0, 18.0)]),
            Shape::circle_stroke(at(19.0, 6.0), 2.5 * k, s),
            Shape::circle_stroke(at(19.0, 18.0), 2.5 * k, s),
        ],
        // A tag, as a value hangs on what it's of.
        Icon::Parameters => {
            vec![ring(&[(3.0, 7.0), (14.0, 7.0), (21.0, 12.0), (14.0, 17.0), (3.0, 17.0)]), Shape::circle_stroke(at(7.5, 12.0), 1.8 * k, s)]
        }
        // A box, and an arrow down into it.
        Icon::LoadFamily => vec![
            ring(&[(4.0, 12.0), (20.0, 12.0), (20.0, 21.0), (4.0, 21.0)]),
            line(&[(12.0, 2.0), (12.0, 16.0)]),
            line(&[(8.0, 12.0), (12.0, 16.0), (16.0, 12.0)]),
        ],
        // A gable's profile, swept back.
        Icon::ExtrusionRoof => {
            vec![line(&[(2.0, 16.0), (8.0, 9.0), (14.0, 16.0)]), line(&[(8.0, 9.0), (16.0, 4.0), (22.0, 11.0), (14.0, 16.0)])]
        }
        // A little gabled front, window and all, standing out of a roof's slope.
        Icon::Dormer => vec![
            line(&[(2.0, 21.0), (22.0, 7.0)]),
            line(&[(8.0, 16.8), (8.0, 11.0), (11.5, 7.5), (15.0, 11.0), (15.0, 11.9)]),
            ring(&[(10.0, 11.5), (13.0, 11.5), (13.0, 14.6), (10.0, 14.6)]),
        ],
        // A roof in plan: hipped at its left end, a gable at its right.
        Icon::GableEdge => vec![
            ring(&[(2.0, 6.0), (22.0, 6.0), (22.0, 18.0), (2.0, 18.0)]),
            line(&[(2.0, 6.0), (8.0, 12.0), (2.0, 18.0)]),
            line(&[(8.0, 12.0), (22.0, 12.0)]),
        ],
        // A wedge climbing to the right, an arrow up it.
        Icon::Ramp => vec![
            ring(&[(2.0, 20.0), (22.0, 20.0), (22.0, 10.0)]),
            line(&[(6.0, 14.5), (16.0, 9.5)]),
            line(&[(12.6, 8.9), (16.0, 9.5), (14.4, 12.6)]),
        ],
        // A rail over balusters, on the floor.
        Icon::Railing => vec![
            line(&[(2.0, 7.0), (22.0, 7.0)]),
            line(&[(4.0, 7.0), (4.0, 20.0)]),
            line(&[(9.3, 7.0), (9.3, 20.0)]),
            line(&[(14.7, 7.0), (14.7, 20.0)]),
            line(&[(20.0, 7.0), (20.0, 20.0)]),
            line(&[(2.0, 20.0), (22.0, 20.0)]),
        ],
        // Two crop marks, at opposite corners.
        Icon::Crop => vec![line(&[(6.0, 2.0), (6.0, 18.0), (22.0, 18.0)]), line(&[(2.0, 6.0), (18.0, 6.0), (18.0, 22.0)])],
        // Sunglasses: what a view hides or isolates for now.
        Icon::TempHide => vec![
            Shape::circle_stroke(at(7.0, 14.0), 4.5 * k, s),
            Shape::circle_stroke(at(17.0, 14.0), 4.5 * k, s),
            line(&[(11.5, 13.0), (12.0, 12.0), (12.5, 13.0)]),
            line(&[(2.5, 14.0), (3.0, 7.0)]),
            line(&[(21.5, 14.0), (21.0, 7.0)]),
        ],
        // A light bulb: what a view hides, lit up.
        Icon::Reveal => vec![
            Shape::circle_stroke(at(12.0, 10.0), 6.0 * k, s),
            line(&[(9.5, 15.0), (9.5, 19.0), (14.5, 19.0), (14.5, 15.0)]),
            line(&[(10.0, 21.5), (14.0, 21.5)]),
        ],
        // Half light, half dark.
        Icon::Theme => {
            let half: Vec<Pos2> = arc(at(12.0, 12.0), 8.0 * k, FRAC_PI_2, 3.0 * FRAC_PI_2);
            vec![Shape::circle_stroke(at(12.0, 12.0), 8.0 * k, s), Shape::convex_polygon(half, color, Stroke::NONE)]
        }
        Icon::Plan => vec![
            ring(&[(3.0, 3.0), (21.0, 3.0), (21.0, 21.0), (3.0, 21.0)]),
            line(&[(3.0, 12.0), (12.0, 12.0), (12.0, 21.0)]),
            line(&[(15.0, 3.0), (15.0, 8.0)]),
        ],
    };
    p.extend(shapes);
}
