//! Manage > Materials: the project's library in three columns. On the left the materials, the user's favorites starred, with
//! New (Custom or a standard colour), Duplicate and Delete; in the middle how the one picked looks: its colour, the foreground
//! and background patterns of its surfaces and its cut, its texture; on the right its cube and its parameters. Every change is
//! a `material` command, undone like any other.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::favorites::{self, Favorites};
use bimcraft_core::materials::{BUILT_IN, CATEGORY, CUBE_VIEW, cube};
use bimcraft_core::model::{Material, Model, ParameterKind, Pattern, PatternLayer};
use bimcraft_core::render::pattern_swatch;
use bimcraft_core::standard_colors::{self, Collection, Kept};
use bimcraft_core::texture::{FILL_MAX_BYTES, FILL_MAX_SIDE, Kind as TextureKind, Sampler, Texture};
use bimcraft_i18n::msg;
use egui::{
    Align, Button, Color32, ColorImage, ComboBox, Context, Grid, Id as UiId, Layout, Mesh, Painter, Pos2, Rect, Response, RichText,
    ScrollArea, Sense, Shape, Slider, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{self, t, tf};
use crate::popup::Modal;
use crate::properties::{number, parameter_text, parameter_value};
use crate::style::pal;
use crate::{BimApp, Status};

/// The widths of the window and of its outer columns, points.
const WIDTH: f32 = 1080.0;
const LIST: f32 = 240.0;
const SIDE: f32 = 290.0;
/// The cube's side, pixels.
const CUBE: usize = 150;
/// A row of the list of materials, and of a drop-down of patterns.
const ROW: f32 = 26.0;

/// Each pattern as the Materials window names it.
/// How wide the middle column's labels stand, for its fields to line up.
const LABEL: f32 = 128.0;

const PATTERNS: [(Pattern, &str); 10] = [
    (Pattern::None, "None"),
    (Pattern::Solid, msg!("Solid Fill")),
    (Pattern::Diagonal, msg!("Diagonal")),
    (Pattern::Crosshatch, msg!("Crosshatch")),
    (Pattern::Horizontal, msg!("Horizontal")),
    (Pattern::Vertical, msg!("Vertical")),
    (Pattern::Grid, msg!("Square Grid")),
    (Pattern::Brick, "Brick"),
    (Pattern::Concrete, "Concrete"),
    (Pattern::Insulation, "Insulation"),
];

/// A kind of texture, as the Materials window names it.
pub(crate) fn texture_name(k: TextureKind) -> &'static str {
    use TextureKind::*;
    match k {
        Brick => "Brick",
        Block => msg!("Block"),
        Boards => msg!("Boards"),
        Concrete => "Concrete",
        Plaster => "Plaster",
        Tile => msg!("Tile"),
        Roof => "Roof Tiles",
        Fabric => "Fabric",
        Metal => "Metal",
        Insulation => "Insulation",
        Image => msg!("Picture"),
    }
}

fn pattern_name(p: Pattern) -> &'static str {
    PATTERNS.iter().find(|(q, _)| *q == p).map_or("None", |(_, n)| n)
}

/// A material's name, the parameter values it held and those values as typed.
type TypedValues = (String, BTreeMap<String, Value>, BTreeMap<String, String>);

/// The window's state between frames.
pub(crate) struct MaterialsDialog {
    /// Opened from a family document (FE-19): it says the project's materials change, now.
    pub(crate) from_family: bool,
    picked: String,
    /// Its name as typed: a rename once typed in.
    name: String,
    search: String,
    /// Whether the list shows the favorites alone.
    starred_only: bool,
    /// The names the user keeps in their favorites.
    starred: BTreeSet<String>,
    /// The colours as picked, by the command's key: applied once the mouse lets go.
    pending: BTreeMap<&'static str, [u8; 3]>,
    /// Its transparency as set on the slider, percent (applied once the mouse lets go).
    clear: Option<u8>,
    /// The fields of its texture and of its image fills as typed (a tile's width and height, the angle, the offset across and
    /// up), by the command's key, for the material and texture they were made from.
    typed: BTreeMap<&'static str, (String, Texture, [String; 5])>,
    /// The image fills' tiles as drawn, by material and whether it's the cut's, and what each was drawn of.
    fills: HashMap<(String, bool), (String, TextureHandle)>,
    /// The tiles drawn for the list, by material, and what each was drawn of.
    tiles: HashMap<String, (String, TextureHandle)>,
    /// The cube as drawn, and what it was drawn of.
    cube: Option<(String, TextureHandle)>,
    /// How the cube is turned and tilted, radians.
    view: [f64; 2],
    /// The parameters' values as typed, for the material and the values they were made from.
    values: Option<TypedValues>,
    /// A new parameter being named: its name and what it holds.
    adding: Option<(String, ParameterKind)>,
    /// The Standard Colors window, while it is open.
    colors: Option<ColorPicker>,
    /// What the last change could not do.
    error: Option<String>,
}

/// The Standard Colors window: the collections, the one shown, what is searched for, the colour picked, a file being imported.
pub(crate) struct ColorPicker {
    collections: Vec<Collection>,
    shown: usize,
    search: String,
    code: Option<String>,
    /// A CSV file on its way in: the file, the collection's name and where its colours are from, as typed.
    importing: Option<(PathBuf, String, String)>,
    error: Option<String>,
}

impl ColorPicker {
    fn new(path: Option<&Path>) -> ColorPicker {
        let (kept, error) = match path.map(Kept::read).transpose() {
            Ok(kept) => (kept.unwrap_or_default(), None),
            Err(e) => (Kept::default(), Some(e)),
        };
        ColorPicker { collections: standard_colors::all(&kept), shown: 0, search: String::new(), code: None, importing: None, error }
    }
}

/// The names kept in the favorites file at `path`, or at the usual place.
fn starred(path: Option<&PathBuf>) -> BTreeSet<String> {
    let path = path.cloned().or_else(favorites::default_path);
    path.and_then(|p| Favorites::read(&p).ok()).map_or_else(BTreeSet::new, |f| f.materials.into_keys().collect())
}

impl MaterialsDialog {
    pub(crate) fn new(m: &Model, favorites: Option<&PathBuf>) -> MaterialsDialog {
        let picked = m.materials.keys().next().cloned().unwrap_or_default();
        MaterialsDialog {
            from_family: false,
            name: picked.clone(),
            picked,
            search: String::new(),
            starred_only: false,
            starred: starred(favorites),
            pending: BTreeMap::new(),
            clear: None,
            typed: BTreeMap::new(),
            fills: HashMap::new(),
            tiles: HashMap::new(),
            cube: None,
            view: CUBE_VIEW,
            values: None,
            adding: None,
            colors: None,
            error: None,
        }
    }

    /// Material `name` picked, what was being typed for the last one let go.
    fn pick(&mut self, name: String) {
        self.name.clone_from(&name);
        self.picked = name;
        self.pending.clear();
        self.clear = None;
        self.values = None;
        self.adding = None;
        self.error = None;
    }
}

/// What a frame of the window asks for, done once it is drawn.
#[derive(Default)]
struct Asked {
    run: Option<Value>,
    picked: Option<String>,
    /// A material to star (true) or to unstar.
    star: Option<(String, bool)>,
    /// A picture for the material's texture.
    picture: bool,
    /// A picture for an image fill: the cut's (true) or the surfaces'.
    fill_picture: Option<bool>,
    favorites: bool,
    colors: bool,
    parameter: Option<Value>,
    wrong: Option<String>,
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A five-pointed star round `c`, filled or drawn round.
fn paint_star(p: &Painter, c: Pos2, r: f32, filled: bool, colour: Color32) {
    let points: Vec<Pos2> = (0..10)
        .map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
            let radius = if k % 2 == 0 { r } else { r * 0.45 };
            c + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    if filled {
        // A fan from its middle: a star is no convex shape.
        let mut mesh = Mesh::default();
        mesh.colored_vertex(c, colour);
        points.iter().for_each(|q| mesh.colored_vertex(*q, colour));
        for k in 0..10u32 {
            mesh.add_triangle(0, k + 1, (k + 1) % 10 + 1);
        }
        p.add(Shape::mesh(mesh));
    } else {
        p.add(Shape::closed_line(points, Stroke::new(1.0, colour)));
    }
}

/// A pattern over `rect`: one of the project's fill patterns by its name, else one drawn in code.
fn paint_choice(ui: &Ui, rect: Rect, model: &Model, (pattern, fill): (Pattern, Option<&String>), colour: Color32) {
    if let Some(p) = fill.and_then(|n| model.fill_patterns.get(n)) {
        crate::fillpatterns::paint_pattern(ui, rect, Some(p), colour);
        return;
    }
    let swatch = pattern_swatch(pattern, f64::from(rect.width()), f64::from(rect.height()));
    let painter = ui.painter_at(rect);
    if swatch.solid {
        painter.rect_filled(rect, 0.0, colour);
    }
    for [a, b] in swatch.lines {
        let at = |q: bimcraft_core::geom::P2| rect.min + vec2(q.0 as f32, q.1 as f32);
        painter.line_segment([at(a), at(b)], Stroke::new(1.0, colour));
    }
}

/// A pattern on paper, `size` across, with a border.
fn pattern_tile(ui: &mut Ui, size: egui::Vec2, model: &Model, layers: &[((Pattern, Option<&String>), Color32)]) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 2.0, Color32::WHITE);
    for (choice, colour) in layers {
        paint_choice(ui, rect.shrink(1.0), model, (choice.0, choice.1), *colour);
    }
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    response
}

/// A row of a drop-down of patterns: a swatch of it, and its name.
fn pattern_row(ui: &mut Ui, model: &Model, choice: (Pattern, Option<&String>), name: &str, selected: bool, colour: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(210.0), ROW), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, name));
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 2.0, if selected { pal().select_fill } else { pal().hover_fill });
    }
    let tile = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(52.0, ROW - 8.0));
    ui.painter().rect_filled(tile, 1.0, Color32::WHITE);
    paint_choice(ui, tile.shrink(1.0), model, choice, colour);
    ui.painter().rect_stroke(tile, 1.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(pos2(tile.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, name, font, pal().text);
    response
}

/// A row's label, at the left of a column `width` across.
fn row_label(ui: &mut Ui, text: &str, width: f32) -> Response {
    ui.allocate_ui_with_layout(vec2(width, 20.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        ui.label(text)
    })
    .inner
}

/// A heading of a column.
fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong());
    ui.separator();
}

/// One of a material's four patterns: which it is, what its commands call it, what it shows.
struct Row {
    label: &'static str,
    name: &'static str,
    color_name: &'static str,
    key: &'static str,
    color_key: &'static str,
    pattern: Pattern,
    fill: Option<String>,
    color: Option<[u8; 3]>,
}

fn rows(m: &Material, cut: bool) -> [Row; 2] {
    let back = |l: &Option<PatternLayer>| l.clone().unwrap_or_default();
    let (fg, fill, color, bg) = if cut {
        (m.cut, m.cut_pattern.clone(), m.cut_color, back(&m.cut_background))
    } else {
        (m.surface, m.surface_pattern.clone(), m.surface_color, back(&m.surface_background))
    };
    let names = if cut {
        (
            msg!("Cut Foreground Pattern"),
            msg!("Cut Foreground Color"),
            msg!("Cut Background Pattern"),
            msg!("Cut Background Color"),
            ("cut", "cut_color", "cut_background", "cut_background_color"),
        )
    } else {
        (
            msg!("Surface Foreground Pattern"),
            msg!("Surface Foreground Color"),
            msg!("Surface Background Pattern"),
            msg!("Surface Background Color"),
            ("surface", "surface_color", "surface_background", "surface_background_color"),
        )
    };
    [
        Row {
            label: msg!("Foreground"),
            name: names.0,
            color_name: names.1,
            key: names.4.0,
            color_key: names.4.1,
            pattern: fg,
            fill,
            color,
        },
        Row {
            label: msg!("Background"),
            name: names.2,
            color_name: names.3,
            key: names.4.2,
            color_key: names.4.3,
            pattern: bg.pattern,
            fill: bg.fill,
            color: bg.color,
        },
    ]
}

/// A colour's button: what is picked is applied once the mouse lets go.
fn colour_button(ui: &mut Ui, d: &mut MaterialsDialog, asked: &mut Asked, (key, name): (&'static str, &str), now: [u8; 3], released: bool) {
    let picked = d.picked.clone();
    let colour = d.pending.entry(key).or_insert(now);
    let r = ui.color_edit_button_srgb(colour);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, name));
    if released && *colour != now {
        asked.run = Some(json!({"name": picked, key: *colour}));
    }
}

/// The left column: what can be done to the library, and its materials.
fn library(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, asked: &mut Asked) {
    ui.horizontal(|ui| {
        ui.menu_button(t("New"), |ui| {
            if ui.button(t("Custom")).clicked() {
                let fresh = (1..).map(|n| format!("{} {n}", t("Material"))).find(|n| !model.materials.contains_key(n)).unwrap_or_default();
                asked.run = Some(json!({"name": fresh}));
                asked.picked = Some(fresh);
                ui.close();
            }
            if ui.button(t("Standard Color…")).clicked() {
                asked.colors = true;
                ui.close();
            }
        });
        let there = model.materials.contains_key(&d.picked);
        if ui.add_enabled(there, Button::new(t("Duplicate"))).clicked() {
            let copy = (2..).map(|n| format!("{} {n}", d.picked)).find(|n| !model.materials.contains_key(n)).unwrap_or_default();
            asked.run = Some(json!({"name": d.picked, "duplicate": copy}));
            asked.picked = Some(copy);
        }
        if ui.add_enabled(there, Button::new(t("Delete"))).clicked() {
            asked.run = Some(json!({"name": d.picked, "delete": true}));
            asked.picked = model.materials.keys().find(|n| **n != d.picked).cloned();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (rect, r) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, d.starred_only, t("Favorites only")));
        if r.clicked() {
            d.starred_only = !d.starred_only;
        }
        let colour = if d.starred_only { pal().accent } else { pal().muted };
        paint_star(ui.painter(), rect.center(), 8.0, d.starred_only, colour);
        r.on_hover_text(t("Favorites only"));
        let hint = t("Search materials");
        ui.add(TextEdit::singleline(&mut d.search).hint_text(hint).desired_width(f32::INFINITY));
    });
    ui.add_space(4.0);
    let query = d.search.trim().to_lowercase();
    let names: Vec<&String> = model
        .materials
        .keys()
        .filter(|n| query.is_empty() || n.to_lowercase().contains(&query))
        .filter(|n| !d.starred_only || d.starred.contains(*n))
        .collect();
    ScrollArea::vertical().id_salt("material-list").auto_shrink([false, false]).show_rows(ui, ROW, names.len(), |ui, range| {
        for name in names.get(range).unwrap_or_default() {
            let Some(m) = model.materials.get(*name) else { continue };
            let selected = **name == d.picked;
            let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            row.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, name.as_str()));
            if selected || row.hovered() {
                ui.painter().rect_filled(rect, 2.0, if selected { pal().select_fill } else { pal().hover_fill });
            }
            if row.clicked() {
                asked.picked = Some((*name).clone());
            }
            // Its star, which a click turns on and off.
            let kept = d.starred.contains(*name);
            let spot = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(20.0, 20.0));
            let star = ui.interact(spot, ui.id().with(("material-star", *name)), Sense::click());
            let label = tf("Favorite {}", &[name.as_str()]);
            star.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, kept, &label));
            if star.clicked() {
                asked.star = Some(((*name).clone(), !kept));
            }
            let colour = if kept {
                pal().accent
            } else if star.hovered() {
                pal().text
            } else {
                pal().muted
            };
            paint_star(ui.painter(), spot.center(), 7.0, kept, colour);
            // A tile of what it looks like.
            let tile = Rect::from_center_size(pos2(rect.left() + 36.0, rect.center().y), vec2(16.0, 16.0));
            if ui.is_rect_visible(tile) {
                let key = format!("{:?}", (m.color, &m.texture));
                if m.texture.is_some() && d.tiles.get(*name).is_none_or(|(k, _)| *k != key) {
                    let id = format!("material-tile-{name}");
                    match crate::favorites::swatch(ui.ctx(), &id, m, &model.images, 32) {
                        Some(h) => drop(d.tiles.insert((*name).clone(), (key, h))),
                        None => drop(d.tiles.remove(*name)),
                    }
                }
                match d.tiles.get(*name).filter(|_| m.texture.is_some()) {
                    Some((_, h)) => {
                        ui.painter().image(h.id(), tile, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(tile, 2.0, rgb(m.color));
                    }
                }
                ui.painter().rect_stroke(tile, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
            }
            let text = Rect::from_min_max(pos2(tile.right() + 6.0, rect.top()), rect.max);
            let font = egui::TextStyle::Body.resolve(ui.style());
            ui.painter_at(text).text(pos2(text.left(), rect.center().y), egui::Align2::LEFT_CENTER, name.as_str(), font, pal().text);
            row.on_hover_text(name.as_str());
        }
    });
}

/// The middle column: how the material looks.
fn settings(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, m: &Material, asked: &mut Asked, released: bool) {
    heading(ui, t("Identity"));
    Grid::new("material").num_columns(2).spacing(vec2(12.0, 6.0)).min_col_width(LABEL).show(ui, |ui| {
        let l = ui.label(t("Name"));
        let r = ui.add(TextEdit::singleline(&mut d.name).desired_width(240.0)).labelled_by(l.id);
        if r.lost_focus() && !d.name.trim().is_empty() && d.name.trim() != d.picked {
            asked.run = Some(json!({"name": d.picked, "rename": d.name.trim()}));
            asked.picked = Some(d.name.trim().to_string());
        }
        ui.end_row();
    });
    heading(ui, t("Shading"));
    Grid::new("material-shading").num_columns(2).spacing(vec2(12.0, 6.0)).min_col_width(LABEL).show(ui, |ui| {
        ui.label(t("Color"));
        colour_button(ui, d, asked, ("color", t("Color")), m.color, released);
        ui.end_row();
        // How see-through the 3D view draws what is made of it, percent.
        let l = ui.label(t("Transparency"));
        let clear = d.clear.get_or_insert(m.transparency);
        ui.add(Slider::new(clear, 0..=100).suffix("%")).labelled_by(l.id);
        if released && *clear != m.transparency {
            asked.run = Some(json!({"name": d.picked, "transparency": *clear}));
        }
        ui.end_row();
    });
    for (cut, title) in [(false, msg!("Surface Pattern")), (true, msg!("Cut Pattern"))] {
        heading(ui, t(title));
        let image = if cut { &m.cut_image } else { &m.surface_image };
        fill_switch(ui, d, asked, cut, image.is_some());
        if let Some(x) = image {
            image_fill(ui, d, model, m, x, cut, asked);
            continue;
        }
        Grid::new(("material-patterns", cut)).num_columns(4).spacing(vec2(8.0, 6.0)).min_col_width(0.0).show(ui, |ui| {
            for row in rows(m, cut) {
                row_label(ui, t(row.label), 86.0);
                let ink = row.color.map_or(Color32::BLACK, rgb);
                pattern_tile(ui, vec2(52.0, 22.0), model, &[((row.pattern, row.fill.as_ref()), ink)]);
                let shown = row.fill.clone().unwrap_or_else(|| t(pattern_name(row.pattern)).to_string());
                let c = ComboBox::from_id_salt(("material-pattern", row.key)).selected_text(shown).width(170.0).height(320.0).show_ui(
                    ui,
                    |ui| {
                        ui.set_min_width(230.0);
                        for (p, name) in PATTERNS {
                            let on = row.fill.is_none() && p == row.pattern;
                            if pattern_row(ui, model, (p, None), t(name), on, ink).clicked() {
                                asked.run = Some(json!({"name": d.picked, row.key: p}));
                            }
                        }
                        ui.separator();
                        for n in model.fill_patterns.keys() {
                            if pattern_row(ui, model, (Pattern::None, Some(n)), n, row.fill.as_ref() == Some(n), ink).clicked() {
                                asked.run = Some(json!({"name": d.picked, row.key: n}));
                            }
                        }
                    },
                );
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t(row.name)));
                colour_button(ui, d, asked, (row.color_key, t(row.color_name)), row.color.unwrap_or([0, 0, 0]), released);
                ui.end_row();
            }
        });
    }
    heading(ui, t("Texture"));
    texture(ui, d, model, m, asked);
}

/// Its texture: a pattern drawn in code or a picture, over its colour in the 3D view; a tile of it this big, turned and moved.
fn texture(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, m: &Material, asked: &mut Asked) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let l = row_label(ui, t("Texture"), LABEL);
        let now = m.texture.as_ref().map(|x| x.kind);
        let shown = now.map_or(t("None"), |k| t(texture_name(k)));
        let c = ComboBox::from_id_salt("material-texture").selected_text(shown).width(150.0).show_ui(ui, |ui| {
            if ui.selectable_label(now.is_none(), t("None")).clicked() {
                asked.run = Some(json!({"name": d.picked, "texture": null}));
            }
            for k in TextureKind::ALL.into_iter().filter(|k| *k != TextureKind::Image) {
                if ui.selectable_label(now == Some(k), t(texture_name(k))).clicked() {
                    asked.run = Some(json!({"name": d.picked, "texture": {"kind": k}}));
                }
            }
        });
        c.response.labelled_by(l.id);
        if ui.button(t("Load Image…")).on_hover_text(t("A JPG or PNG picture, taken into the project")).clicked() {
            asked.picture = true;
        }
    });
    let Some(x) = &m.texture else { return };
    if let Some(p) = model.images.get(&x.image).filter(|_| x.kind == TextureKind::Image) {
        let (w, h) = p.size();
        ui.label(RichText::new(format!("{} ({w} × {h} px)", p.name())).color(pal().muted));
    }
    let labels = [msg!("Tile Width"), msg!("Tile Height"), msg!("Texture Angle"), msg!("Texture Offset Across"), msg!("Texture Offset Up")];
    tile_fields(ui, d, model, x, "texture", labels, asked);
}

/// Whether a material's surfaces (or its cut) show patterns or a picture: the picture, once a file is picked for it.
fn fill_switch(ui: &mut Ui, d: &MaterialsDialog, asked: &mut Asked, cut: bool, image: bool) {
    let (key, names) = if cut {
        ("cut_image", [msg!("Cut Pattern Fill"), msg!("Cut Image Fill")])
    } else {
        ("surface_image", [msg!("Surface Pattern Fill"), msg!("Surface Image Fill")])
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        row_label(ui, t("Fill"), 86.0);
        for (on, text, label, hover) in [
            (!image, msg!("Pattern"), names[0], msg!("A foreground and a background pattern")),
            (image, msg!("Image"), names[1], msg!("A JPG or PNG picture in place of the patterns")),
        ] {
            let r = ui.selectable_label(on, t(text));
            r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, t(label)));
            if r.on_hover_text(t(hover)).clicked() && !on {
                if image {
                    asked.run = Some(json!({"name": d.picked, key: null}));
                } else {
                    asked.fill_picture = Some(cut);
                }
            }
        }
    });
}

/// An image fill's tile, drawn once for what it is of.
fn fill_tile(ctx: &Context, d: &mut MaterialsDialog, model: &Model, m: &Material, x: &Texture, cut: bool) -> Option<egui::TextureId> {
    let (id, key) = ((d.picked.clone(), cut), format!("{x:?}"));
    if d.fills.get(&id).is_none_or(|(k, _)| *k != key) {
        let side = 64;
        let rgb = Sampler::new(x, m.color, &model.images)?.swatch(side);
        let image = ColorImage::from_rgb([side, side], &rgb);
        d.fills.insert(id.clone(), (key, ctx.load_texture("material-image-fill", image, TextureOptions::LINEAR_REPEAT)));
    }
    d.fills.get(&id).map(|(_, h)| h.id())
}

/// A picture `rect` big, or the border of one.
fn paint_tile(ui: &Ui, rect: Rect, tile: Option<egui::TextureId>) {
    if let Some(id) = tile {
        // Repeated at its own proportions, however wide the box.
        let across = (rect.width() / rect.height().max(1.0)).max(1.0);
        let up = (rect.height() / rect.width().max(1.0)).max(1.0);
        ui.painter().image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(across, up)), Color32::WHITE);
    }
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
}

/// An image fill: its picture, another to load in its place, and how big a tile of it is, how turned and moved.
fn image_fill(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, m: &Material, x: &Texture, cut: bool, asked: &mut Asked) {
    let tile = fill_tile(ui.ctx(), d, model, m, x, cut);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        row_label(ui, t("Image"), 86.0);
        let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
        paint_tile(ui, rect, tile);
        ui.vertical(|ui| {
            if let Some(p) = model.images.get(&x.image) {
                let (w, h) = p.size();
                ui.label(RichText::new(format!("{} ({w} × {h} px)", p.name())).color(pal().muted));
            }
            let label = if cut { msg!("Replace Cut Image") } else { msg!("Replace Surface Image") };
            let r = ui.button(t("Replace Image…"));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, t(label)));
            let limits = tf(
                "A JPG or PNG of up to {} MB, kept at up to {} px a side",
                &[&(FILL_MAX_BYTES >> 20).to_string(), &FILL_MAX_SIDE.to_string()],
            );
            if r.on_hover_text(limits).clicked() {
                asked.fill_picture = Some(cut);
            }
        });
    });
    let (key, labels) = if cut {
        (
            "cut_image",
            [
                msg!("Cut Image Width"),
                msg!("Cut Image Height"),
                msg!("Cut Image Angle"),
                msg!("Cut Image Offset Across"),
                msg!("Cut Image Offset Up"),
            ],
        )
    } else {
        (
            "surface_image",
            [
                msg!("Surface Image Width"),
                msg!("Surface Image Height"),
                msg!("Surface Image Angle"),
                msg!("Surface Image Offset Across"),
                msg!("Surface Image Offset Up"),
            ],
        )
    };
    tile_fields(ui, d, model, x, key, labels, asked);
}

/// How big a tile of a texture or of an image fill is, how turned and moved: five fields, `labels`, a change made by the
/// `material` command's `key` once one is typed in.
fn tile_fields(
    ui: &mut Ui,
    d: &mut MaterialsDialog,
    model: &Model,
    x: &Texture,
    key: &'static str,
    labels: [&'static str; 5],
    asked: &mut Asked,
) {
    let units = model.units;
    if !d.typed.get(key).is_some_and(|(n, old, _)| *n == d.picked && old == x) {
        let typed = [
            units.length(x.size[0]),
            units.length(x.size[1]),
            format!("{}", x.angle),
            units.length(x.offset[0]),
            units.length(x.offset[1]),
        ];
        d.typed.insert(key, (d.picked.clone(), x.clone(), typed));
    }
    let picked = d.picked.clone();
    let Some((_, _, typed)) = d.typed.get_mut(key) else { return };
    Grid::new(("material-tile", key)).num_columns(2).spacing(vec2(12.0, 6.0)).min_col_width(LABEL).show(ui, |ui| {
        for (k, label) in labels.into_iter().enumerate() {
            let l = ui.label(t(label));
            let hint = if k == 2 { "°" } else { units.name() };
            let r = ui.add(TextEdit::singleline(&mut typed[k]).desired_width(120.0).hint_text(hint)).labelled_by(l.id);
            if r.lost_focus() {
                let length = |i: usize| units.parse(&typed[i]);
                match (length(0), length(1), number(&typed[2]), length(3), length(4)) {
                    (Some(w), Some(h), Some(a), Some(ox), Some(oy)) => {
                        if [w, h, a, ox, oy] != [x.size[0], x.size[1], x.angle, x.offset[0], x.offset[1]] {
                            asked.run = Some(json!({"name": picked, key: {"size": [w, h], "angle": a, "offset": [ox, oy]}}));
                        }
                    }
                    _ => asked.wrong = Some(tf("`{}` is not a size, an angle or an offset", &[&typed[k]])),
                }
            }
            ui.end_row();
        }
    });
}

/// The right column's top: the cube, which a drag turns and a double click sets straight, and its patterns side by side.
fn preview(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, m: &Material) {
    heading(ui, t("Preview"));
    let width = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(vec2(width, CUBE as f32), Sense::click_and_drag());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, t("Material preview")));
    let drag = r.drag_delta();
    if drag != egui::Vec2::ZERO {
        d.view[0] += f64::from(drag.x) * 0.01;
        d.view[1] = (d.view[1] + f64::from(drag.y) * 0.01).clamp(-1.5, 1.5);
    }
    if r.double_clicked() {
        d.view = CUBE_VIEW;
    }
    // A checked ground, for what is seen through it to show.
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_gray(232));
    let square = 14.0;
    for i in 0..(width / square) as usize + 1 {
        for j in 0..(CUBE as f32 / square) as usize + 1 {
            if (i + j) % 2 == 0 {
                let at = rect.min + vec2(i as f32 * square, j as f32 * square);
                painter.rect_filled(Rect::from_min_size(at, vec2(square, square)), 0.0, Color32::from_gray(214));
            }
        }
    }
    let key = format!("{:?}", (m.color, &m.texture, m.transparency, d.view));
    if d.cube.as_ref().is_none_or(|(k, _)| *k != key) {
        let pixels = cube(m, &model.images, CUBE, d.view);
        let image = ColorImage::from_rgba_unmultiplied([CUBE, CUBE], &pixels);
        d.cube = Some((key, ui.ctx().load_texture("material-cube", image, TextureOptions::LINEAR)));
    }
    if let Some((_, h)) = &d.cube {
        let at = Rect::from_center_size(rect.center(), vec2(CUBE as f32, CUBE as f32));
        painter.image(h.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    r.on_hover_text(t("Drag to turn it; double-click to set it straight"));
    ui.add_space(6.0);
    let tile = vec2((width - 8.0) / 2.0, 34.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (cut, label) in [(false, msg!("Surface")), (true, msg!("Cut"))] {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                match if cut { &m.cut_image } else { &m.surface_image } {
                    Some(x) => {
                        let picture = fill_tile(ui.ctx(), d, model, m, x, cut);
                        let (rect, _) = ui.allocate_exact_size(tile, Sense::hover());
                        paint_tile(ui, rect, picture);
                    }
                    None => {
                        let [fg, bg] = rows(m, cut);
                        let ink = |r: &Row| r.color.map_or(Color32::BLACK, rgb);
                        let layers = [((bg.pattern, bg.fill.as_ref()), ink(&bg)), ((fg.pattern, fg.fill.as_ref()), ink(&fg))];
                        pattern_tile(ui, tile, model, &layers);
                    }
                }
                ui.label(RichText::new(t(label)).small().color(pal().muted));
            });
        }
    });
}

/// The right column's rest: what the material holds of the parameters every material has, and of the project's.
fn parameters(ui: &mut Ui, d: &mut MaterialsDialog, model: &Model, m: &Material, asked: &mut Asked) {
    let units = model.units;
    let own: Vec<(&String, ParameterKind)> =
        model.parameters.iter().filter(|(_, p)| p.categories.iter().any(|c| c == CATEGORY)).map(|(n, p)| (n, p.kind)).collect();
    let kinds = BUILT_IN.iter().map(|(n, k, _)| ((*n).to_string(), *k)).chain(own.iter().map(|(n, k)| ((*n).clone(), *k)));
    if !d.values.as_ref().is_some_and(|(n, v, _)| *n == d.picked && *v == m.parameters) {
        let typed = kinds.map(|(n, k)| {
            let text = m.parameters.get(&n).map_or_else(String::new, |v| parameter_text(k, v, units));
            (n, text)
        });
        d.values = Some((d.picked.clone(), m.parameters.clone(), typed.collect()));
    }
    let picked = d.picked.clone();
    let Some((_, _, typed)) = &mut d.values else { return };
    let mut field = |ui: &mut Ui, name: &str, shown: &str, kind: ParameterKind| {
        let l = ui.label(shown);
        if kind == ParameterKind::YesNo {
            let mut on = m.parameters.get(name).and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut on, "").labelled_by(l.id).changed() {
                asked.run = Some(json!({"name": picked, "parameters": {name: on}}));
            }
        } else if let Some(text) = typed.get_mut(name) {
            let r = ui.add(TextEdit::singleline(text).desired_width(150.0)).labelled_by(l.id);
            let was = m.parameters.get(name).map_or_else(String::new, |v| parameter_text(kind, v, units));
            if r.lost_focus() && *text != was {
                match parameter_value(kind, text, units) {
                    Ok(v) => asked.run = Some(json!({"name": picked, "parameters": {name: v}})),
                    Err(e) => asked.wrong = Some(format!("{shown}: {e}")),
                }
            }
        }
        ui.end_row();
    };
    for group in [msg!("Identity Data"), msg!("Product Information")] {
        ui.add_space(4.0);
        ui.label(RichText::new(t(group)).color(pal().muted));
        Grid::new(("material-parameters", group)).num_columns(2).spacing(vec2(8.0, 4.0)).min_col_width(90.0).show(ui, |ui| {
            for (n, k, _) in BUILT_IN.iter().filter(|(.., g)| *g == group) {
                field(ui, n, t(n), *k);
            }
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new(t("Project Parameters")).color(pal().muted));
    Grid::new("material-project-parameters").num_columns(2).spacing(vec2(8.0, 4.0)).min_col_width(90.0).show(ui, |ui| {
        for (n, k) in &own {
            field(ui, n, n, *k);
        }
    });
    ui.add_space(4.0);
    match &mut d.adding {
        None => {
            if ui.button(t("Add Parameter")).clicked() {
                d.adding = Some((String::new(), ParameterKind::Text));
            }
        }
        Some((name, kind)) => {
            let l = ui.label(t("Parameter Name"));
            ui.add(TextEdit::singleline(name).desired_width(f32::INFINITY)).labelled_by(l.id);
            ui.horizontal(|ui| {
                let kinds = [
                    (ParameterKind::Text, msg!("Text")),
                    (ParameterKind::Number, msg!("Number")),
                    (ParameterKind::Integer, msg!("Integer")),
                    (ParameterKind::Length, msg!("Length")),
                    (ParameterKind::YesNo, msg!("Yes/No")),
                ];
                let shown = kinds.iter().find(|(k, _)| k == kind).map_or("", |(_, n)| t(n));
                ComboBox::from_id_salt("material-parameter-kind").selected_text(shown).width(90.0).show_ui(ui, |ui| {
                    for (k, n) in kinds {
                        ui.selectable_value(kind, k, t(n));
                    }
                });
                let name = name.trim().to_string();
                if ui.add_enabled(!name.is_empty(), Button::new(t("Add"))).clicked() {
                    // One the project has for other categories comes to materials too.
                    let mut categories = model.parameters.get(&name).map_or_else(Vec::new, |p| p.categories.clone());
                    categories.push(CATEGORY.to_string());
                    asked.parameter = Some(json!({"name": name, "kind": *kind, "categories": categories}));
                }
                if ui.button(t("Cancel")).clicked() {
                    asked.parameter = Some(Value::Null);
                }
            });
        }
    }
}

impl BimApp {
    /// The file the user's collections of colours are kept in.
    fn colors_path(&self) -> Option<PathBuf> {
        match &self.favorites_path {
            Some(p) => Some(p.with_file_name("standard_colors.json")),
            None => standard_colors::default_path(),
        }
    }

    /// A `standard_color` command's parameters, for the file the app was told of, if it was.
    fn color_params(&self, mut p: Value) -> Value {
        if let Some(path) = self.favorites_path.as_ref().map(|f| f.with_file_name("standard_colors.json")) {
            p["path"] = json!(path);
        }
        p
    }

    /// CSV file `path` on its way in: its collection named as the file is, until the user says otherwise.
    pub(crate) fn import_colors(&mut self, path: PathBuf) {
        if let Some(c) = self.materials_dialog.as_mut().and_then(|d| d.colors.as_mut()) {
            let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            c.importing = Some((path, stem, String::new()));
            c.error = None;
        }
    }

    /// The Standard Colors window, over the Materials window.
    fn colors_window(&mut self, ctx: &Context, d: &mut MaterialsDialog) {
        let Some(mut c) = d.colors.take() else { return };
        let (mut close, mut make, mut ask_file, mut import, mut remove) = (false, None, false, None, None);
        let r = Modal::new(UiId::new("standard-colors"), t("Standard Colors")).width(520.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let l = ui.label(t("Collection"));
                let shown = c.collections.get(c.shown).map_or("", |x| x.id.as_str()).to_string();
                let combo = ComboBox::from_id_salt("standard-collection").selected_text(shown).width(170.0).show_ui(ui, |ui| {
                    for (k, x) in c.collections.iter().enumerate() {
                        if ui.selectable_label(k == c.shown, format!("{} ({})", x.id, x.colors.len())).clicked() {
                            (c.shown, c.code) = (k, None);
                        }
                    }
                });
                combo.response.labelled_by(l.id);
                if ui.button(t("Import CSV…")).on_hover_text(t("A table of your own: code, name, #rrggbb")).clicked() {
                    ask_file = true;
                }
                let imported = c.collections.get(c.shown).filter(|x| x.imported).map(|x| x.id.clone());
                if ui.add_enabled(imported.is_some(), Button::new(t("Remove"))).clicked() {
                    remove = imported;
                }
            });
            if let Some((file, name, source)) = &mut c.importing {
                ui.add_space(6.0);
                ui.label(RichText::new(file.display().to_string()).small().color(pal().muted));
                Grid::new("standard-import").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                    let l = ui.label(t("Collection Name"));
                    ui.add(TextEdit::singleline(name).desired_width(320.0)).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t("Source"));
                    ui.add(TextEdit::singleline(source).desired_width(320.0)).labelled_by(l.id);
                    ui.end_row();
                });
                if ui.add_enabled(!name.trim().is_empty(), Button::new(t("Import"))).clicked() {
                    import = Some((file.clone(), name.trim().to_string(), source.trim().to_string()));
                }
            }
            ui.add_space(6.0);
            let hint = t("Search by code or name");
            let search = ui.add(TextEdit::singleline(&mut c.search).hint_text(hint).desired_width(f32::INFINITY));
            search.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, hint));
            ui.add_space(4.0);
            let Some(x) = c.collections.get(c.shown) else { return };
            let found = x.search(c.search.trim());
            let height = (ctx.content_rect().height() - 340.0).clamp(160.0, 380.0);
            ScrollArea::vertical().id_salt("standard-list").max_height(height).auto_shrink([false, false]).show_rows(
                ui,
                ROW,
                found.len(),
                |ui, range| {
                    for color in found.get(range).unwrap_or_default() {
                        let label = color.material_name(&x.id);
                        let on = c.code.as_ref() == Some(&color.code);
                        let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        row.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, &label));
                        if on || row.hovered() {
                            ui.painter().rect_filled(rect, 2.0, if on { pal().select_fill } else { pal().hover_fill });
                        }
                        let tile = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(36.0, ROW - 8.0));
                        ui.painter().rect_filled(tile, 2.0, rgb(color.rgb));
                        ui.painter().rect_stroke(tile, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        ui.painter().text(pos2(tile.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, &label, font, pal().text);
                        if row.clicked() {
                            c.code = Some(color.code.clone());
                        }
                        if row.double_clicked() {
                            make = Some((x.id.clone(), color.code.clone()));
                        }
                    }
                },
            );
            ui.add_space(6.0);
            ui.label(RichText::new(t("Colors are RGB approximations of standard colors.")).small().color(pal().muted));
            if !x.source.is_empty() {
                ui.label(RichText::new(tf("Source: {}", &[lang::t(&x.source)])).small().color(pal().muted));
            }
            if let Some(e) = &c.error {
                ui.colored_label(pal().error, e);
            }
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    if ui.add_enabled(c.code.is_some(), Button::new(t("Create Material"))).clicked() {
                        make = c.code.clone().map(|code| (x.id.clone(), code));
                    }
                });
            });
        });
        if ask_file {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("CSV Files"), &["csv", "txt"]);
            self.ask_file(ctx, dialog, false, |app, path| app.import_colors(path));
        }
        if let Some((file, name, source)) = import {
            let p = self.color_params(json!({"action": "import", "collection": name, "file": file, "source": source, "replace": true}));
            // The user's own file, not the project: done here, on a Co-Work guest's machine too.
            match Session::new().execute("standard_color", &p) {
                Ok(_) => {
                    c = ColorPicker::new(self.colors_path().as_deref());
                    c.shown = c.collections.iter().position(|x| x.id == name).unwrap_or(0);
                }
                Err(e) => c.error = Some(e),
            }
        }
        if let Some(name) = remove {
            let p = self.color_params(json!({"action": "remove", "collection": name}));
            match Session::new().execute("standard_color", &p) {
                Ok(_) => c = ColorPicker::new(self.colors_path().as_deref()),
                Err(e) => c.error = Some(e),
            }
        }
        if let Some((collection, code)) = make {
            let x = c.collections.iter().find(|x| x.id == collection);
            let color = x.and_then(|x| x.colors.iter().find(|k| k.code == code).map(|k| (k.material_name(&x.id), k.rgb)));
            if let Some((name, [r, g, b])) = color {
                // A plain `material`, which a Co-Work guest's window sends to the host.
                let hex = format!("#{r:02x}{g:02x}{b:02x}");
                let p = json!({"name": name, "color": hex, "surface": "solid", "surface_color": hex, "cut": "solid", "cut_color": hex});
                if self.session.model.materials.contains_key(&name) || self.run("material", p).is_some() {
                    d.pick(name);
                    close = true;
                }
            }
        }
        if !(close || r.should_close()) {
            d.colors = Some(c);
        }
    }

    /// The picture at `path` as material `material`'s image fill, of its cut or of its surfaces; what goes wrong said in the
    /// window.
    pub(crate) fn load_fill_image(&mut self, material: &str, cut: bool, path: &Path) {
        let key = if cut { "cut_image" } else { "surface_image" };
        let ok = self.run("material", json!({"name": material, key: {"path": path.to_string_lossy()}})).is_some();
        if let Some(d) = self.materials_dialog.as_mut() {
            d.error = (!ok).then(|| self.status.text.clone());
        }
    }

    /// The Materials window: Manage > Materials.
    pub(crate) fn materials_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.materials_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let mut asked = Asked::default();
        let mut close = false;
        let released = ctx.input(|i| !i.pointer.any_down());
        let r = Modal::new(UiId::new("materials"), t("Materials")).width(WIDTH).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                if d.from_family {
                    ui.label(RichText::new(t(crate::editor::PROJECT_NOTE_MATERIALS)).strong());
                    ui.add_space(4.0);
                }
                // As tall as the window lets the dialog be, so the columns fill it; on a short screen, tall enough to use.
                let height = (crate::popup::body_room(ui) - 2.0).max(300.0);
                let total = ui.available_width();
                let gaps = 2.0 * (ui.spacing().item_spacing.x * 2.0 + 6.0);
                let (list, side) = (LIST.min(total * 0.26), SIDE.min(total * 0.3));
                let middle = (total - list - side - gaps).max(220.0);
                let column = |ui: &mut Ui, width: f32, add: &mut dyn FnMut(&mut Ui)| {
                    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        add(ui);
                    });
                };
                // The row as tall as its columns, and no taller: a separator stands as tall as the row it is in.
                ui.allocate_ui_with_layout(vec2(total, height), Layout::left_to_right(Align::Min), |ui| {
                    ui.set_height(height);
                    column(ui, list, &mut |ui| library(ui, &mut d, &model, &mut asked));
                    ui.separator();
                    let picked = model.materials.get(&d.picked);
                    column(ui, middle, &mut |ui| {
                        ScrollArea::vertical().id_salt("material-settings").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width((middle - 14.0).max(120.0));
                            if let Some(m) = picked {
                                settings(ui, &mut d, &model, m, &mut asked, released);
                            }
                        });
                    });
                    ui.separator();
                    column(ui, side, &mut |ui| {
                        if let Some(m) = picked {
                            preview(ui, &mut d, &model, m);
                            heading(ui, t("Parameters"));
                            ScrollArea::vertical().id_salt("material-values").auto_shrink([false, false]).show(ui, |ui| {
                                parameters(ui, &mut d, &model, m, &mut asked);
                            });
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    asked.favorites = ui.button(t("Favorites…")).clicked();
                    if let Some(e) = &d.error {
                        ui.colored_label(pal().error, e);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| close = ui.button(t("Close")).clicked());
                });
            });
        });
        self.colors_window(ctx, &mut d);
        if asked.colors {
            d.colors = Some(ColorPicker::new(self.colors_path().as_deref()));
        }
        if let Some(text) = asked.wrong {
            d.error = Some(text.clone());
            self.status = Status { text, error: true };
        }
        if let Some((name, on)) = asked.star {
            let p = self.favorite_params(if on { "save" } else { "delete" }, "material", &name);
            if self.run("favorite", p).is_some() {
                if on {
                    d.starred.insert(name);
                } else {
                    d.starred.remove(&name);
                }
            }
        }
        if asked.favorites {
            self.open_favorites();
        }
        match asked.parameter {
            Some(Value::Null) => d.adding = None,
            Some(p) => {
                if self.run("parameter", p).is_some() {
                    d.adding = None;
                    d.values = None;
                } else {
                    d.error = Some(self.status.text.clone());
                }
            }
            None => {}
        }
        let ran = asked.run.is_some();
        if let Some(p) = asked.run {
            if self.run("material", p).is_some() {
                d.error = None;
            } else {
                d.error = Some(self.status.text.clone());
                asked.picked = None;
            }
        }
        if asked.picture {
            let material = d.picked.clone();
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Pictures"), &["jpg", "jpeg", "png"]);
            self.ask_file(ctx, dialog, false, move |app, path| {
                app.run("material", json!({"name": material, "texture": {"kind": "image", "path": path.to_string_lossy()}}));
            });
        }
        if let Some(cut) = asked.fill_picture {
            let material = d.picked.clone();
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Pictures"), &["jpg", "jpeg", "png"]);
            self.ask_file(ctx, dialog, false, move |app, path| app.load_fill_image(&material, cut, &path));
        }
        if let Some(n) = asked.picked {
            d.pick(n);
        } else if !self.session.model.materials.contains_key(&d.picked) {
            let colors = d.colors.take();
            d = MaterialsDialog::new(&self.session.model, self.favorites_path.as_ref());
            d.colors = colors;
        } else if released && !ran {
            d.pending.clear();
            d.clear = None;
        }
        if !(close || r.should_close()) {
            self.materials_dialog = Some(d);
        }
    }
}
