//! The start page, which the window opens on when it is given no project: New and Open, the recent files as tiles with a picture of
//! each (its 3D view, drawn on a thread of its own), and along the bottom the source code's page and whether an update is out. The
//! quick access bar's Home brings it back over a project, which it leaves as it was.

use bimcraft_i18n::msg;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime};

use bimcraft_core::model::Model;
use bimcraft_core::view3d::{AZIMUTH, ELEVATION, Scene};
use bimcraft_update::State;
use egui::epaint::{Mesh, PathShape, Shadow};
use egui::{
    Align, Align2, Color32, ColorImage, Context, CornerRadius, FontId, Frame, Layout, OpenUrl, Panel, Pos2, Rect, Response, RichText,
    ScrollArea, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::lang::{t, tf};
use crate::style::{self, pal};
use crate::{BimApp, Pending};

/// Where the source code is.
pub const SOURCE: &str = "https://github.com/IPedrax/BIMCraft";
/// A tile's picture, px (drawn at twice that, for sharp screens).
const THUMB: [u32; 2] = [232, 136];
/// A project file bigger than this gets no picture: drawing one would hold the thread too long.
const MAX_THUMB_FILE: u64 = 32 << 20;
const TILE: f32 = 232.0;
const GAP: f32 = 18.0;

/// What a tile shows above its name.
enum Thumb {
    Drawing,
    Ready(TextureHandle),
    /// No picture: not a project, not there, or nothing in it to see.
    None,
}

/// The start page's state: whether it's up, and the pictures of the recent files.
#[derive(Default)]
pub struct Home {
    pub shown: bool,
    /// Whether it was brought up over a project, which Back (and Ctrl+D, as in Revit) goes back to.
    pub over: bool,
    thumbs: HashMap<PathBuf, Thumb>,
    drawn: Option<Receiver<(PathBuf, Option<ColorImage>)>>,
}

impl Home {
    /// Whether every tile's picture has been drawn, or found not to be (tests wait for it).
    #[cfg(test)]
    pub fn settled(&self) -> bool {
        self.drawn.is_none()
    }

    /// The page put away, keeping its pictures for when it comes back.
    pub(crate) fn put_away(&mut self) -> Home {
        Home { shown: false, over: false, thumbs: std::mem::take(&mut self.thumbs), drawn: self.drawn.take() }
    }

    /// Whether `path`'s tile has a picture.
    #[cfg(test)]
    pub fn has_picture(&self, path: &Path) -> bool {
        matches!(self.thumbs.get(path), Some(Thumb::Ready(_)))
    }
}

/// The picture of a project file's 3D view, RGBA; none for a file that isn't a project, can't be read, or holds nothing to see.
pub fn picture(path: &Path) -> Option<ColorImage> {
    let is_project = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bimcraft"));
    let meta = std::fs::metadata(path).ok()?;
    if !is_project || !meta.is_file() || meta.len() > MAX_THUMB_FILE {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    std::panic::catch_unwind(|| {
        let m = Model::from_json(&text).ok()?;
        let scene = Scene::new(&m);
        let [w, h] = THUMB.map(|s| s * 2);
        let image = scene.render(&scene.fit(AZIMUTH, ELEVATION, w, h), &[]);
        // Levels alone are nothing to see.
        let seen =
            (0..h).step_by(6).any(|y| (0..w).step_by(6).any(|x| image.pick(x as f64, y as f64).is_some_and(|id| m.level(id).is_none())));
        if !seen {
            return None;
        }
        Some(ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba))
    })
    .ok()
    .flatten()
}

/// The arrow back to the project, at the page's top left.
fn back(ui: &mut Ui) -> Response {
    let label = t("Back");
    let font = FontId::proportional(14.0);
    let text = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(vec2(text.x + 46.0, 32.0), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let fill = if r.hovered() { pal().hover_fill } else { pal().surface };
    ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(ui.painter(), Icon::Back, icon, pal().text);
    ui.painter().text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, label, font, pal().text);
    let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
    r.on_hover_text(tf("Back to the project ({}D)", &[cmd]))
}

/// How long ago `when` was, in words.
fn ago(when: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(when).unwrap_or(Duration::ZERO).as_secs();
    let (minutes, hours, days) = (secs / 60, secs / 3600, secs / 86_400);
    match () {
        _ if minutes < 1 => t("Just now").to_string(),
        _ if hours < 1 => tf("{} min ago", &[&minutes.to_string()]),
        _ if days < 1 => tf("{} h ago", &[&hours.to_string()]),
        _ if days < 2 => t("Yesterday").to_string(),
        _ if days < 31 => tf("{} days ago", &[&days.to_string()]),
        _ if days < 365 => tf("{} months ago", &[&(days / 30).to_string()]),
        _ => tf("{} years ago", &[&(days / 365).to_string()]),
    }
}

/// What a click on the page asks for, done once it's drawn.
enum Click {
    New,
    Open,
    Recent(PathBuf),
    Forget(PathBuf),
    Back,
    Page(String),
    Update,
    Check,
    Restart,
}

impl BimApp {
    /// Open on the start page (the desktop does when it's given no project).
    pub fn show_start_page(&mut self) {
        self.home.shown = true;
    }

    /// Home and Ctrl+D: the start page over the project, or, from it, back to the project (when there is one behind it).
    pub(crate) fn toggle_home(&mut self) {
        if !self.home.shown {
            (self.home.shown, self.home.over) = (true, true);
        } else if self.home.over {
            self.home.shown = false;
        }
    }

    /// Draws the pictures not drawn yet on a thread, one file after another; takes what it has finished.
    fn draw_pictures(&mut self, ctx: &Context) {
        if self.home.drawn.is_none() {
            let todo: Vec<PathBuf> = self.recent.paths().iter().filter(|p| !self.home.thumbs.contains_key(*p)).cloned().collect();
            if todo.is_empty() {
                return;
            }
            let (tx, rx) = channel();
            let wake = ctx.clone();
            for p in &todo {
                self.home.thumbs.insert(p.clone(), Thumb::Drawing);
            }
            let spawned = std::thread::Builder::new().name("start-page-pictures".into()).spawn(move || {
                for p in todo {
                    let image = picture(&p);
                    if tx.send((p, image)).is_err() {
                        return;
                    }
                    wake.request_repaint();
                }
            });
            if spawned.is_err() {
                self.home.thumbs.values_mut().for_each(|t| *t = Thumb::None);
                return;
            }
            self.home.drawn = Some(rx);
        }
        let Some(rx) = &self.home.drawn else { return };
        loop {
            match rx.try_recv() {
                Ok((path, image)) => {
                    let thumb = match image {
                        Some(image) => Thumb::Ready(ctx.load_texture(format!("start-{}", path.display()), image, TextureOptions::LINEAR)),
                        None => Thumb::None,
                    };
                    self.home.thumbs.insert(path, thumb);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.home.drawn = None;
                    // What the thread didn't get to (it stopped) has no picture.
                    for t in self.home.thumbs.values_mut() {
                        if matches!(t, Thumb::Drawing) {
                            *t = Thumb::None;
                        }
                    }
                    break;
                }
            }
        }
    }

    /// The whole window while the start page is up.
    pub(crate) fn start_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.draw_pictures(&ctx);
        let mut click = None;
        let bar = Frame::new().fill(pal().bar).inner_margin(egui::Margin::symmetric(20, 8)).stroke(Stroke::new(1.0, pal().border));
        Panel::bottom("start-footer").frame(bar).show(ui, |ui| self.start_footer(ui, &mut click));
        egui::CentralPanel::default().frame(Frame::NONE.fill(pal().panel)).show(ui, |ui| {
            backdrop(ui);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let full = ui.available_width();
                let width = (full - 64.0).clamp(320.0, 1180.0);
                let left = ((full - width) / 2.0).max(16.0);
                ui.add_space(36.0);
                if self.home.over {
                    ui.horizontal(|ui| {
                        ui.add_space(left);
                        if back(ui).clicked() {
                            click = Some(Click::Back);
                        }
                    });
                    ui.add_space(14.0);
                }
                ui.horizontal(|ui| {
                    ui.add_space(left);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        self.start_hero(ui);
                        ui.add_space(30.0);
                        let wide = width >= 760.0;
                        if wide {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(260.0);
                                    self.start_actions(ui, &mut click);
                                });
                                ui.add_space(36.0);
                                ui.vertical(|ui| self.start_recent(ui, &mut click));
                            });
                        } else {
                            self.start_actions(ui, &mut click);
                            ui.add_space(28.0);
                            self.start_recent(ui, &mut click);
                        }
                        ui.add_space(36.0);
                    });
                });
            });
        });
        let Some(click) = click else { return };
        match click {
            Click::New => self.guard(Pending::New, &ctx),
            Click::Open => self.guard(Pending::Open, &ctx),
            Click::Recent(path) => self.guard(Pending::OpenPath(path), &ctx),
            Click::Forget(path) => {
                self.recent.remove(&path);
                self.home.thumbs.remove(&path);
                self.keep_recent();
            }
            Click::Back => self.home.shown = false,
            Click::Page(url) => ctx.open_url(OpenUrl::new_tab(url)),
            Click::Update => {
                if let Some(u) = self.updates.as_mut() {
                    u.update_now();
                }
            }
            Click::Check => {
                if let Some(u) = self.updates.as_mut() {
                    u.check_now();
                }
            }
            Click::Restart => self.guard(Pending::Update, &ctx),
        }
    }

    /// The logo, the name and what it is.
    fn start_hero(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            logo(ui, rect);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("BIMCraft").font(FontId::proportional(34.0)).strong().color(pal().text));
                ui.label(RichText::new(t("Building information modelling, open and free.")).size(15.0).color(pal().muted));
            });
        });
    }

    /// New, Open, and back to the project open behind the page.
    fn start_actions(&self, ui: &mut Ui, click: &mut Option<Click>) {
        heading(ui, "Start");
        let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        if action(ui, Icon::NewFile, msg!("New Project"), msg!("An empty project to model in."), &format!("{cmd}N")).clicked() {
            *click = Some(Click::New);
        }
        ui.add_space(10.0);
        let what = if self.revit.is_some() {
            msg!("A BIMCraft project, an IFC or a Revit file.")
        } else {
            msg!("A BIMCraft project or an IFC file.")
        };
        if action(ui, Icon::Open, "Open…", what, &format!("{cmd}O")).clicked() {
            *click = Some(Click::Open);
        }
        if self.home.over {
            ui.add_space(10.0);
            let back = tf("Back to {}", &[&self.session.model.name]);
            if action(ui, Icon::Back, &back, msg!("The project stays as you left it."), &format!("{cmd}D")).clicked() {
                *click = Some(Click::Back);
            }
        }
    }

    /// The recent files, as tiles.
    fn start_recent(&self, ui: &mut Ui, click: &mut Option<Click>) {
        heading(ui, msg!("Recent"));
        let files: Vec<(String, PathBuf)> = self.recent.labels().into_iter().zip(self.recent.paths().iter().cloned()).collect();
        if files.is_empty() {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
            dashed(ui, rect.shrink(1.0));
            let inner = Rect::from_center_size(rect.center(), vec2(rect.width(), 48.0));
            ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)), |ui| {
                ui.label(RichText::new(t("No recent projects yet")).size(15.0).color(pal().text));
                ui.label(RichText::new(t("The projects you open or save show here.")).size(13.0).color(pal().muted));
            });
            return;
        }
        let width = ui.available_width();
        let per_row = (((width + GAP) / (TILE + GAP)).floor() as usize).max(1);
        for row in files.chunks(per_row) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (label, path) in row {
                    let r = self.tile(ui, label, path);
                    if r.clicked() {
                        *click = Some(Click::Recent(path.clone()));
                    }
                    r.context_menu(|ui| {
                        if ui.button(t("Open")).clicked() {
                            *click = Some(Click::Recent(path.clone()));
                        }
                        if ui.button(t("Remove from List")).clicked() {
                            *click = Some(Click::Forget(path.clone()));
                        }
                    });
                }
            });
            ui.add_space(GAP);
        }
    }

    /// One recent file: its picture, its name, its folder and when it last changed.
    fn tile(&self, ui: &mut Ui, label: &str, path: &Path) -> Response {
        let thumb_h = THUMB[1] as f32;
        let (rect, r) = ui.allocate_exact_size(vec2(TILE, thumb_h + 62.0), Sense::click());
        let missing = !path.exists();
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        let r = r.on_hover_text(path.to_string_lossy());
        let hot = r.hovered() || r.has_focus();
        let rect = if hot { rect.translate(vec2(0.0, -2.0)) } else { rect };
        let p = ui.painter();
        let round = CornerRadius::same(12);
        let shadow = Shadow { offset: [0, if hot { 8 } else { 3 }], blur: if hot { 22 } else { 10 }, spread: 0, color: shade(hot) };
        p.add(shadow.as_shape(rect, round));
        p.rect(
            rect,
            round,
            pal().surface,
            Stroke::new(if hot { 1.5 } else { 1.0 }, if hot { pal().accent } else { pal().border }),
            StrokeKind::Inside,
        );
        // The picture, its top corners rounded with the card's.
        let pic = Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(TILE - 2.0, thumb_h));
        let pic_round = CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 };
        match self.home.thumbs.get(path) {
            Some(Thumb::Ready(tex)) => {
                p.rect_filled(pic, pic_round, Color32::WHITE);
                let image = egui::Image::new((tex.id(), pic.size())).corner_radius(pic_round);
                image.paint_at(ui, pic);
            }
            other => {
                let tint = if style::dark() { pal().bar } else { pal().hover_fill };
                p.rect_filled(pic, pic_round, tint);
                let icon = Rect::from_center_size(pic.center() - vec2(0.0, 6.0), vec2(40.0, 40.0));
                let ext = path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
                let glyph = if ext == "BIMCRAFT" { Icon::Plan } else { Icon::Open };
                icons::paint(p, glyph, icon, pal().muted);
                let note = if missing {
                    t("Not found").to_string()
                } else if matches!(other, Some(Thumb::Drawing)) {
                    t("Drawing…").to_string()
                } else {
                    ext
                };
                p.text(pic.center() + vec2(0.0, 30.0), Align2::CENTER_CENTER, note, FontId::proportional(12.0), pal().muted);
            }
        }
        p.line_segment([pic.left_bottom(), pic.right_bottom()], Stroke::new(1.0, pal().border));
        // Its name and where it is.
        let text_at = pos2(rect.left() + 14.0, pic.bottom() + 12.0);
        let name = Path::new(label).file_stem().map_or_else(|| label.to_string(), |s| s.to_string_lossy().into_owned());
        let ink = if missing { pal().muted } else { pal().text };
        let clip = Rect::from_min_max(text_at, pos2(rect.right() - 12.0, rect.bottom()));
        let p = p.with_clip_rect(clip);
        p.text(text_at, Align2::LEFT_TOP, name, FontId::proportional(14.5), ink);
        let when = std::fs::metadata(path).and_then(|m| m.modified()).ok().map(ago);
        let folder = path.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let detail = match when {
            Some(w) if !folder.is_empty() => format!("{folder} · {w}"),
            Some(w) => w,
            None => folder,
        };
        p.text(text_at + vec2(0.0, 22.0), Align2::LEFT_TOP, detail, FontId::proportional(12.0), pal().muted);
        r
    }

    /// The footer: the source code, the version, what the status box would say, and the update.
    fn start_footer(&self, ui: &mut Ui, click: &mut Option<Click>) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), Icon::Code, rect, pal().muted);
            if ui.link(t("Source code on GitHub")).on_hover_text(SOURCE).clicked() {
                *click = Some(Click::Page(SOURCE.into()));
            }
            ui.label(RichText::new("·").color(pal().muted));
            ui.label(RichText::new(tf("Version {}", &[env!("CARGO_PKG_VERSION")])).color(pal().muted));
            if !self.status.text.is_empty() {
                ui.label(RichText::new("·").color(pal().muted));
                let ink = if self.status.error { pal().error } else { pal().muted };
                ui.label(RichText::new(&self.status.text).color(ink));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.update_status(ui, click));
        });
    }

    /// Whether an update is out, right to left.
    fn update_status(&self, ui: &mut Ui, click: &mut Option<Click>) {
        let Some(u) = self.updates.as_ref() else {
            ui.label(RichText::new(t("This copy doesn't look for updates.")).color(pal().muted));
            return;
        };
        let current = u.current().to_string();
        match u.state() {
            State::Available(release) => {
                let version = release.version.to_string();
                if release.installer.is_some() {
                    if ui.button(RichText::new(t("Update now")).strong()).clicked() {
                        *click = Some(Click::Update);
                    }
                } else if ui.button(t("Download")).clicked() {
                    *click = Some(Click::Page(release.page.clone()));
                }
                pill(ui, &tf("BIMCraft {} is available", &[&version]), pal().accent);
            }
            State::Downloading { release, .. } => {
                ui.spinner();
                ui.label(tf("Downloading BIMCraft {}…", &[&release.version.to_string()]));
            }
            State::Ready { release, .. } => {
                if ui.button(RichText::new(t("Restart and install")).strong()).clicked() {
                    *click = Some(Click::Restart);
                }
                pill(ui, &tf("BIMCraft {} is ready to install", &[&release.version.to_string()]), pal().accent);
            }
            State::Installing { .. } => {
                ui.spinner();
            }
            State::Idle if u.checking() => {
                ui.spinner();
                ui.label(RichText::new(t("Checking for updates…")).color(pal().muted));
            }
            State::Idle if u.up_to_date() => {
                ui.label(RichText::new(tf("BIMCraft {} is up to date", &[&current])).color(pal().muted));
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), Icon::Check, rect, pal().context);
            }
            State::Idle => {
                if ui.link(t("Check for updates")).clicked() {
                    *click = Some(Click::Check);
                }
            }
        }
    }
}

/// A section's name, small and spaced, over it.
fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(t(text).to_uppercase()).size(12.0).strong().color(pal().muted).extra_letter_spacing(1.2));
    ui.add_space(10.0);
}

/// A big button: an icon in a disc, a title, what it does and its shortcut.
fn action(ui: &mut Ui, icon: Icon, title: &str, hint: &str, keys: &str) -> Response {
    let title = t(title).to_string();
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &title));
    let hot = r.hovered() || r.has_focus();
    let p = ui.painter();
    let round = CornerRadius::same(12);
    let shadow = Shadow { offset: [0, if hot { 6 } else { 2 }], blur: if hot { 18 } else { 8 }, spread: 0, color: shade(hot) };
    p.add(shadow.as_shape(rect, round));
    let fill = if hot { pal().hover_fill } else { pal().surface };
    p.rect(rect, round, fill, Stroke::new(if hot { 1.5 } else { 1.0 }, if hot { pal().accent } else { pal().border }), StrokeKind::Inside);
    let disc = pos2(rect.left() + 36.0, rect.center().y);
    p.circle_filled(disc, 21.0, pal().accent.gamma_multiply(if hot { 1.0 } else { 0.14 }));
    icons::paint(p, icon, Rect::from_center_size(disc, vec2(22.0, 22.0)), if hot { Color32::WHITE } else { pal().accent });
    let at = pos2(rect.left() + 70.0, rect.center().y);
    let clip = Rect::from_min_max(pos2(at.x, rect.top()), pos2(rect.right() - 8.0, rect.bottom()));
    let p = p.with_clip_rect(clip);
    p.text(at - vec2(0.0, 10.0), Align2::LEFT_CENTER, &title, FontId::proportional(15.5), pal().text);
    p.text(at + vec2(0.0, 11.0), Align2::LEFT_CENTER, t(hint), FontId::proportional(12.0), pal().muted);
    if !keys.is_empty() {
        p.text(pos2(rect.right() - 14.0, rect.top() + 16.0), Align2::RIGHT_CENTER, keys, FontId::monospace(11.0), pal().muted);
    }
    r.on_hover_text(t(hint))
}

/// A rounded label, tinted.
fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| ui.label(RichText::new(text).color(color).strong()));
}

/// The colour of a card's shadow.
fn shade(hot: bool) -> Color32 {
    let a = match (style::dark(), hot) {
        (true, true) => 140,
        (true, false) => 90,
        (false, true) => 46,
        (false, false) => 18,
    };
    Color32::from_black_alpha(a)
}

/// A soft wash of the accent over the top of the page, and a faint grid, as drawing paper.
fn backdrop(ui: &Ui) {
    let r = ui.max_rect();
    let p = ui.painter();
    let grid = if style::dark() { Color32::from_white_alpha(5) } else { Color32::from_black_alpha(6) };
    let step = 28.0;
    let mut x = r.left();
    while x < r.right() {
        p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.0, grid));
        x += step;
    }
    let mut y = r.top();
    while y < r.bottom() {
        p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, grid));
        y += step;
    }
    let wash = Rect::from_min_size(r.min, vec2(r.width(), 320.0_f32.min(r.height())));
    let top = pal().accent.gamma_multiply(if style::dark() { 0.22 } else { 0.12 });
    let mut mesh = Mesh::default();
    mesh.colored_vertex(wash.left_top(), top);
    mesh.colored_vertex(wash.right_top(), top);
    mesh.colored_vertex(wash.right_bottom(), Color32::TRANSPARENT);
    mesh.colored_vertex(wash.left_bottom(), Color32::TRANSPARENT);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
}

/// A rectangle's outline, dashed and rounded at its corners.
fn dashed(ui: &Ui, r: Rect) {
    let stroke = Stroke::new(1.5, pal().border);
    let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in corners.windows(2) {
        ui.painter().extend(Shape::dashed_line(&[w[0], w[1]], stroke, 8.0, 6.0));
    }
}

/// BIMCraft's logo (`assets/app-icon/bimcraft.svg`), drawn into `r`.
fn logo(ui: &Ui, r: Rect) {
    let k = r.width() / 256.0;
    let at = |x: f32, y: f32| r.min + vec2(x, y) * k;
    let p = ui.painter();
    let tile = Rect::from_min_max(at(8.0, 8.0), at(248.0, 248.0));
    p.rect_filled(tile, CornerRadius::same((52.0 * k).round() as u8), Color32::from_rgb(0x1F, 0x9D, 0x7F));
    let lines = p.with_clip_rect(tile.shrink(6.0 * k));
    let faint = Stroke::new((2.5 * k).max(0.6), Color32::from_white_alpha(80));
    for i in 0..10 {
        let v = 20.0 + 24.0 * i as f32;
        lines.line_segment([at(v, 0.0), at(v, 256.0)], faint);
        lines.line_segment([at(0.0, v), at(256.0, v)], faint);
    }
    let ink = Color32::from_rgb(0x17, 0x19, 0x1C);
    let (orange, cream, teal) =
        (Color32::from_rgb(0xF0, 0x7A, 0x3A), Color32::from_rgb(0xF4, 0xEC, 0xD8), Color32::from_rgb(0x1F, 0x9D, 0x7F));
    let poly = |pts: &[(f32, f32)], fill: Color32, w: f32| {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(PathShape::convex_polygon(pts, fill, Stroke::new(w * k, ink)));
    };
    p.line_segment([at(138.0, 62.0), at(200.0, 62.0)], Stroke::new(7.0 * k, ink));
    poly(&[(187.0, 37.0), (213.0, 37.0), (200.0, 59.0)], orange, 6.0);
    poly(&[(63.0, 71.3), (157.0, 125.6), (190.0, 137.0), (96.0, 82.7)], orange, 7.0);
    poly(&[(123.3, 228.7), (180.1, 195.9), (180.1, 132.4), (151.7, 122.5), (123.3, 165.2)], cream, 7.0);
    poly(&[(39.9, 180.5), (123.3, 228.7), (123.3, 165.2), (39.9, 117.1)], cream, 7.0);
    poly(&[(63.0, 71.3), (157.0, 125.6), (124.0, 175.1), (30.0, 120.8)], orange, 7.0);
    poly(&[(52.2, 187.7), (69.2, 197.5), (69.2, 159.2), (52.2, 149.3)], teal, 6.0);
    poly(&[(83.5, 187.1), (108.1, 201.3), (108.1, 181.6), (83.5, 167.4)], teal, 6.0);
    poly(&[(162.1, 187.7), (141.3, 199.7), (141.3, 180.0), (162.1, 167.9)], teal, 6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_since_reads_as_words() {
        let now = SystemTime::now();
        assert_eq!(ago(now), "Just now");
        assert_eq!(ago(now - Duration::from_secs(5 * 60)), "5 min ago");
        assert_eq!(ago(now - Duration::from_secs(3 * 3600)), "3 h ago");
        assert_eq!(ago(now - Duration::from_secs(30 * 3600)), "Yesterday");
        assert_eq!(ago(now - Duration::from_secs(9 * 86_400)), "9 days ago");
        assert_eq!(ago(now - Duration::from_secs(70 * 86_400)), "2 months ago");
        assert_eq!(ago(now - Duration::from_secs(800 * 86_400)), "2 years ago");
        assert_eq!(ago(now + Duration::from_secs(60)), "Just now", "a clock that went back");
    }

    #[test]
    fn a_picture_is_drawn_of_a_project_and_of_nothing_else() {
        let dir = std::env::temp_dir().join(format!("bimcraft-start-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = bimcraft_core::engine::Session::new();
        s.execute("wall", &serde_json::json!({"points": [[0, 0], [5000, 0], [5000, 4000]]})).unwrap();
        let project = dir.join("walls.bimcraft");
        s.execute("save", &serde_json::json!({"path": project})).unwrap();
        let image = picture(&project).unwrap();
        assert_eq!(image.size, [THUMB[0] as usize * 2, THUMB[1] as usize * 2]);
        let empty = dir.join("empty.bimcraft");
        bimcraft_core::engine::Session::new().execute("save", &serde_json::json!({"path": empty})).unwrap();
        assert!(picture(&empty).is_none(), "nothing to see");
        let junk = dir.join("junk.bimcraft");
        std::fs::write(&junk, "{ not json").unwrap();
        assert!(picture(&junk).is_none());
        assert!(picture(&dir.join("gone.bimcraft")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
