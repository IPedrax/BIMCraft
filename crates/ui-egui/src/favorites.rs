//! The Favorites window (Manage > Favorites): the materials and families the user keeps for every
//! project. What it shows it reads from the favorites file itself; every change to the project goes
//! through a `favorite` command, so it's checked and undone like any other.

use crate::popup::Modal;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use bimcraft_core::favorites::{self, Favorites};
use bimcraft_core::model::Material;
use bimcraft_core::texture::{Picture, Sampler};
use egui::{
    Align, Color32, ColorImage, Context, Id as UiId, Layout, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, pos2, vec2,
};
use serde_json::json;

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Materials,
    Families,
    TitleBlocks,
}

/// The Favorites window: which kind it lists, what the file held when it was read, and the swatches drawn of it.
pub(crate) struct FavoritesDialog {
    tab: Tab,
    kept: Result<Favorites, String>,
    swatches: HashMap<String, TextureHandle>,
}

/// A family's category, as the window names it.
fn category_name(c: bimcraft_core::family::Category) -> String {
    crate::editor::category_label(c)
}

/// The favorites in the file at `path`, or at the usual place.
fn read(path: Option<&PathBuf>) -> Result<Favorites, String> {
    let path =
        path.cloned().or_else(favorites::default_path).ok_or_else(|| t("there's no folder to keep favorites in here").to_string())?;
    Favorites::read(&path)
}

impl FavoritesDialog {
    pub(crate) fn new(path: Option<&PathBuf>) -> FavoritesDialog {
        FavoritesDialog { tab: Tab::Materials, kept: read(path), swatches: HashMap::new() }
    }
}

/// A tile of `material`'s texture as a picture `side` px across, to draw as its swatch; none for a material with no texture.
pub(crate) fn swatch(
    ctx: &Context,
    id: &str,
    material: &Material,
    images: &BTreeMap<String, Picture>,
    side: usize,
) -> Option<TextureHandle> {
    let sampler = Sampler::new(material.texture.as_ref()?, material.color, images)?;
    Some(ctx.load_texture(id, ColorImage::from_rgb([side, side], &sampler.swatch(side)), TextureOptions::NEAREST))
}

/// A swatch `size` points across: the texture's tile, else the colour.
pub(crate) fn paint_swatch(ui: &mut Ui, size: f32, color: [u8; 3], texture: Option<&TextureHandle>) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    match texture {
        Some(tile) => {
            p.image(tile.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.rect_filled(rect, 2.0, Color32::from_rgb(color[0], color[1], color[2]));
        }
    }
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
}

impl BimApp {
    /// Open the Favorites window.
    pub(crate) fn open_favorites(&mut self) {
        self.favorites_dialog = Some(FavoritesDialog::new(self.favorites_path.as_ref()));
    }

    /// A `favorite` command's parameters: `action` on the favorite `name` of `kind`, in the file the app was told of, if it was.
    pub(crate) fn favorite_params(&self, action: &str, kind: &str, name: &str) -> serde_json::Value {
        let mut p = json!({"action": action, "kind": kind, "name": name});
        if let Some(path) = &self.favorites_path {
            p["path"] = json!(path);
        }
        p
    }

    /// The Favorites window, while it's open.
    pub(crate) fn favorites_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.favorites_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let mut act: Option<(&'static str, &'static str, String)> = None;
        let mut close = false;
        // A swatch of each favorite material with a texture, drawn once.
        if let Ok(f) = &d.kept {
            for (name, e) in &f.materials {
                if !d.swatches.contains_key(name)
                    && let Some(tile) = swatch(ctx, &format!("favorite-{name}"), &e.material, &e.images, 48)
                {
                    d.swatches.insert(name.clone(), tile);
                }
            }
        }
        let r = Modal::new(UiId::new("favorites"), t("Favorites")).width(540.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("Kept for every project: add one to this project with a click.")).color(pal().muted));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (tab, label) in [(Tab::Materials, "Materials"), (Tab::Families, "Families"), (Tab::TitleBlocks, "Title Blocks")] {
                        if ui.selectable_label(d.tab == tab, t(label)).clicked() {
                            d.tab = tab;
                        }
                    }
                });
                ui.separator();
                ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| match (&d.kept, d.tab) {
                    (Err(e), _) => {
                        ui.label(RichText::new(e).color(pal().error));
                    }
                    (Ok(f), Tab::Materials) => {
                        if f.materials.is_empty() {
                            ui.label(RichText::new(t("No favorite materials yet: save one from Manage > Materials.")).color(pal().muted));
                        }
                        for (name, e) in &f.materials {
                            ui.horizontal(|ui| {
                                paint_swatch(ui, 40.0, e.material.color, d.swatches.get(name));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(name).strong());
                                    let what =
                                        e.material.texture.as_ref().map_or(t("No texture"), |x| t(crate::materials::texture_name(x.kind)));
                                    ui.label(RichText::new(what).color(pal().muted));
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(t("Remove")).clicked() {
                                        act = Some(("delete", "material", name.clone()));
                                    }
                                    if ui.button(t("Add to Project")).clicked() {
                                        act = Some(("load", "material", name.clone()));
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        }
                    }
                    (Ok(f), Tab::TitleBlocks) => {
                        if f.title_blocks.is_empty() {
                            ui.label(
                                RichText::new(t("No favorite title blocks yet: save one from View > Title Blocks.")).color(pal().muted),
                            );
                        }
                        for (name, e) in &f.title_blocks {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(name).strong());
                                    let b = &e.title_block;
                                    let mut what = format!("{} × {} mm", b.width, b.height);
                                    if !b.sizes.is_empty() {
                                        what = format!("{what}, {}", b.sizes.join(", "));
                                    }
                                    ui.label(RichText::new(what).color(pal().muted));
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(t("Remove")).clicked() {
                                        act = Some(("delete", "title_block", name.clone()));
                                    }
                                    if ui.button(t("Add to Project")).clicked() {
                                        act = Some(("load", "title_block", name.clone()));
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        }
                    }
                    (Ok(f), Tab::Families) => {
                        if f.families.is_empty() {
                            ui.label(RichText::new(t("No favorite families yet: save one from the Family Editor.")).color(pal().muted));
                        }
                        for (name, e) in &f.families {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(name).strong());
                                    let types = e.family.types.keys().cloned().collect::<Vec<_>>().join(", ");
                                    ui.label(RichText::new(tf("{}: {}", &[&category_name(e.family.category), &types])).color(pal().muted));
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(t("Remove")).clicked() {
                                        act = Some(("delete", "family", name.clone()));
                                    }
                                    if ui.button(t("Add to Project")).clicked() {
                                        act = Some(("load", "family", name.clone()));
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        }
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
            });
        });
        if let Some((action, kind, name)) = act {
            let mut p = self.favorite_params(action, kind, &name);
            // A name the project has already is its own: the favorite comes in as another.
            if action == "load" {
                let taken = |n: &str| match kind {
                    "material" => model.materials.contains_key(n),
                    "title_block" => model.title_blocks.contains_key(n),
                    _ => model.families.contains_key(n),
                };
                if taken(&name) {
                    p["as"] = json!((2..).map(|k| format!("{name} {k}")).find(|n| !taken(n)).unwrap_or_default());
                }
            }
            if self.run("favorite", p).is_some() && action == "load" {
                self.status = Status { text: tf("Added {} to the project", &[&name]), error: false };
            }
            d = FavoritesDialog { tab: d.tab, kept: read(self.favorites_path.as_ref()), swatches: HashMap::new() };
        }
        if !(close || r.should_close()) {
            self.favorites_dialog = Some(d);
        }
    }
}
