//! File > Export… (Ctrl+P): the active view, sheet or schedule, or the whole project, to any format the session's exports
//! list, with the page set up as a print dialog sets it (paper, orientation, scale, margin, placement, colour, hairlines,
//! resolution, pages) and a preview that lays the drawing on its pages with the core's own [`page::layout`] and
//! [`Paged::place`], so what it shows is what the file holds. Export asks where the file goes, then writes it on a thread of
//! its own ([`worker`]). The settings of each format are kept for the next export while the window is open.
//!
//! The preview's drawing is made on a thread too: a big view takes a while to draw, and the dialog stays live meanwhile.

pub(crate) mod worker;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::export::page::{self, Anchor, Colour, Orientation, PageSetup, Scale};
use bimcraft_core::export::{self, Content, Family, Format, Item, Job, Kind, Options, Paged, Status as Ready, Takes};
use bimcraft_core::ifc::Origin;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::Drawing;
use bimcraft_i18n::msg;
use egui::{Align, ComboBox, Context, DragValue, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::canvas::Place;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet};

/// The papers the dialog lists, by the names `model::paper` knows.
const PAPERS: [&str; 12] = ["A4", "A3", "A2", "A1", "A0", "Letter", "Tabloid", "ARCH A", "ARCH B", "ARCH C", "ARCH D", "ARCH E"];

/// The preview's size on screen.
const PREVIEW: egui::Vec2 = vec2(340.0, 300.0);

/// The Export dialog, while it is open.
pub(crate) struct Dialog {
    /// The format's id.
    pub(crate) format: String,
    /// What the window shows: the view, sheet or schedule exported, or none for the project.
    pub(crate) source: Option<Id>,
    /// Every sheet, in one file (a format that combines them: PDF), instead of the source.
    pub(crate) all_sheets: bool,
    pub(crate) options: Options,
    /// The scale typed, 1 : n, kept while another scale is picked.
    pub(crate) ratio: f64,
    preview: Preview,
}

/// The drawing the preview lays out: made on a thread for a source, and kept while the source and the project are the same.
#[derive(Default)]
struct Preview {
    /// The source and the project it was made of.
    of: Option<(Option<Id>, usize)>,
    answer: Option<Receiver<Result<Paged, String>>>,
    made: Option<Result<Arc<Paged>, String>>,
    /// The drawing in the colours and lines of the setup, for that colour and hairlines.
    styled: Option<(Colour, bool, Option<Arc<Drawing>>)>,
}

/// What a format can be given of `source` (of `kind`): the source itself, the whole project (a model format, from a view that
/// isn't 3D), or nothing.
pub(crate) fn source_for(format: &dyn Format, kind: Option<Kind>, source: Option<Id>) -> Option<Option<Id>> {
    let kind = kind?;
    if format.takes().iter().any(|t| kind.gives().contains(t)) {
        Some(source.filter(|_| kind != Kind::Project))
    } else if format.takes().contains(&Takes::Model) {
        Some(None)
    } else {
        None
    }
}

/// The sheets of a project, by number.
fn sheets_by_number(m: &Model) -> Vec<Id> {
    let mut sheets = m.sheets();
    sheets.sort_by(|a, b| {
        let number = |id: &Id| m.sheet(*id).map(|s| s.number.clone()).unwrap_or_default();
        number(a).cmp(&number(b))
    });
    sheets
}

/// `{` and `}` doubled, so a file's name as the system's dialog gave it is a template of no tokens.
fn literal(name: &str) -> String {
    name.replace('{', "{{").replace('}', "}}")
}

fn family_name(f: Family) -> &'static str {
    match f {
        Family::Document => msg!("Document"),
        Family::Vector => msg!("Vector"),
        Family::Raster => msg!("Picture"),
        Family::Line => msg!("CAD drawing"),
        Family::Model => msg!("Model"),
        Family::Schedule => msg!("Table"),
        Family::Coordination => msg!("Coordination"),
    }
}

/// A format as the list names it: its label, and how ready it is.
fn format_name(f: &dyn Format) -> String {
    match f.status() {
        Ready::Stable => f.label().to_string(),
        Ready::Experimental => tf(msg!("{} (experimental)"), &[f.label()]),
        Ready::Planned => tf(msg!("{} (planned)"), &[f.label()]),
    }
}

impl BimApp {
    /// What the window shows, as an export's source: the element and its kind (none, the project).
    pub(crate) fn export_source(&self) -> Option<Id> {
        let m = &self.session.model;
        let id = match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => Some(id),
            Sheet::View3D => self.active_3d(),
            Sheet::Table(id) => Some(id),
            Sheet::Schedule(_) => None,
        };
        id.filter(|id| export::kind_of(m, Some(*id)).is_some())
    }

    /// Opens File > Export… on what the window shows, with `format` picked (none, the last one, else PDF).
    pub(crate) fn open_export(&mut self, format: Option<&str>) {
        let source = self.export_source();
        let kind = export::kind_of(&self.session.model, source);
        let registry = &self.session.exports;
        let usable = |f: &&Arc<dyn Format>| f.status() != Ready::Planned && source_for(f.as_ref(), kind, source).is_some();
        let wanted = format.or(self.export_format.as_deref()).unwrap_or("pdf");
        let format = match registry.find(wanted).ok().filter(usable) {
            Some(f) => f.id().to_string(),
            None => match registry.all().iter().find(usable) {
                Some(f) => f.id().to_string(),
                None => return,
            },
        };
        let options = self.export_last.get(&format).cloned().unwrap_or_default();
        let ratio = match options.page.scale {
            Scale::Custom(n) => n,
            _ => source.map_or(100.0, |id| bimcraft_core::graphics::scale_of(&self.session.model, id)).max(1.0),
        };
        self.export_dialog = Some(Dialog { format, source, all_sheets: false, options, ratio, preview: Preview::default() });
    }

    /// The Export dialog, while it is open.
    pub(crate) fn export_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.export_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let registry = self.session.exports.clone();
        let kind = export::kind_of(&model, d.source);
        let Ok(format) = registry.find(&d.format).cloned() else { return };
        let given = source_for(format.as_ref(), kind, d.source).unwrap_or(None);
        let sheets = sheets_by_number(&model);
        let all_sheets = d.all_sheets && format.combines() && !sheets.is_empty();
        // The preview's drawing: the source's, or the first sheet's when they all go.
        let shown = if all_sheets { sheets.first().copied() } else { given };
        if format.paged() {
            d.preview.ask(ctx, &model, shown);
        }
        let (mut close, mut go) = (false, false);
        let r = Modal::new(egui::Id::new("export-dialog"), t("Export")).width(760.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(380.0);
                        self.export_options(ui, &mut d, &registry, format.as_ref(), kind, given, &model, sheets.len());
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        if format.paged() {
                            d.preview.show(ui, &d.options.page);
                        } else {
                            let (rect, _) = ui.allocate_exact_size(PREVIEW, Sense::hover());
                            ui.painter().rect_filled(rect, 4.0, pal().surface);
                            let what = match given {
                                Some(id) => export::title(&model, Some(id), kind.unwrap_or(Kind::Project)),
                                None => tf(msg!("The whole project: {}"), &[&model.name]),
                            };
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                what,
                                egui::FontId::proportional(14.0),
                                pal().muted,
                            );
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let busy = self.exporting();
                    go = ui
                        .add_enabled(!busy, egui::Button::new(t("Export…")))
                        .on_disabled_hover_text(t("An export is being written"))
                        .clicked();
                    close = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if go {
            let items: Vec<Item> = if all_sheets {
                sheets.iter().map(|id| Item { view: Some(model.guid(*id)), format: format.id().to_string() }).collect()
            } else {
                vec![Item { view: given.map(|id| model.guid(id)), format: format.id().to_string() }]
            };
            self.export_last.insert(d.format.clone(), d.options.clone());
            self.export_format = Some(d.format.clone());
            let (options, combine) = (d.options.clone(), all_sheets);
            let what = format_name(format.as_ref());
            self.ask_path(ctx, &what, format.extension(), move |app, path| {
                app.export_to(path, items, options, combine);
            });
            return;
        }
        if !(close || r.should_close()) {
            self.export_dialog = Some(d);
        }
    }

    /// Writes `items` to the file `path` (a picture of several pages, a file a page beside it).
    pub(crate) fn export_to(&mut self, path: PathBuf, items: Vec<Item>, options: Options, combine: bool) {
        let folder = path.parent().map(PathBuf::from).unwrap_or_default();
        let stem = path.file_stem().map(|s| literal(&s.to_string_lossy())).unwrap_or_default();
        let job = Job {
            items,
            folder,
            name: Some(stem.clone()),
            combine: combine.then_some(stem),
            // The system's dialog asked before a file was written over.
            overwrite: true,
            options,
            ..Job::default()
        };
        self.start_export(job);
    }

    #[allow(clippy::too_many_arguments)]
    fn export_options(
        &self,
        ui: &mut Ui,
        d: &mut Dialog,
        registry: &export::Registry,
        format: &dyn Format,
        kind: Option<Kind>,
        given: Option<Id>,
        model: &Model,
        sheets: usize,
    ) {
        egui::Grid::new("export-what").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(t("Format"));
            ComboBox::from_id_salt("export-format").width(240.0).selected_text(format_name(format)).show_ui(ui, |ui| {
                let mut families: BTreeMap<Family, Vec<&Arc<dyn Format>>> = BTreeMap::new();
                for f in registry.all() {
                    families.entry(f.family()).or_default().push(f);
                }
                for (family, formats) in families {
                    ui.label(RichText::new(t(family_name(family))).small().color(pal().muted));
                    for f in formats {
                        let fits = source_for(f.as_ref(), kind, d.source).is_some() && f.status() != Ready::Planned;
                        let picked = f.id() == d.format;
                        if ui.add_enabled(fits, egui::Button::selectable(picked, format_name(f.as_ref()))).clicked() {
                            d.format = f.id().to_string();
                            if let Some(kept) = self.export_last.get(f.id()) {
                                d.options = kept.clone();
                            }
                        }
                    }
                }
            });
            ui.end_row();
            ui.label(t("Of"));
            ui.vertical(|ui| {
                let what = match given {
                    Some(id) => export::title(model, Some(id), kind.unwrap_or(Kind::Project)),
                    None => tf(msg!("The whole project: {}"), &[&model.name]),
                };
                if format.combines() && sheets > 0 {
                    ui.radio_value(&mut d.all_sheets, false, what);
                    ui.radio_value(&mut d.all_sheets, true, tf(msg!("All sheets ({}), in one file"), &[&sheets.to_string()]));
                } else {
                    ui.label(what);
                }
            });
            ui.end_row();
        });
        if format.status() == Ready::Experimental {
            ui.label(
                RichText::new(t("Experimental: its files have not yet been proven in the programs that read them.")).color(pal().muted),
            );
        }
        let o = &mut d.options;
        if format.id().starts_with("ifc") {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t("Coordinates from"));
                let name = |o: Origin| match o {
                    Origin::Internal => t("Internal origin"),
                    Origin::Base => t("Project Base Point"),
                    Origin::Survey => t("Survey Point"),
                    Origin::Shared => t("Shared coordinates"),
                };
                ComboBox::from_id_salt("export-origin").selected_text(name(o.ifc_origin)).show_ui(ui, |ui| {
                    for origin in [Origin::Internal, Origin::Base, Origin::Survey, Origin::Shared] {
                        ui.selectable_value(&mut o.ifc_origin, origin, name(origin));
                    }
                });
            });
        }
        if format.id() == "jpeg" {
            let mut quality = o.quality.unwrap_or(90);
            ui.horizontal(|ui| {
                ui.label(t("Quality"));
                ui.add(egui::Slider::new(&mut quality, 1..=100));
            });
            o.quality = Some(quality);
        }
        if format.id() == "bcf" {
            ui.label(
                RichText::new(tf(msg!("The topic is about the {} elements selected."), &[&o.selected.len().to_string()]))
                    .color(pal().muted),
            );
        }
        if !format.paged() {
            return;
        }
        let picture = format.family() == Family::Raster;
        page_setup(ui, &mut o.page, &mut d.ratio, picture);
        match &d.preview.made {
            Some(Ok(paged)) => match paged.layout(&o.page) {
                Ok(l) => {
                    let pages = l.pages.len();
                    let mut text = if l.cols * l.rows > 1 {
                        tf(
                            msg!("{} of {} pages ({} × {})"),
                            &[&pages.to_string(), &(l.cols * l.rows).to_string(), &l.cols.to_string(), &l.rows.to_string()],
                        )
                    } else {
                        t("1 page").to_string()
                    };
                    text.push_str(&format!(" · {} × {} mm · 1 : {}", l.paper.0.round(), l.paper.1.round(), ratio_text(l.ratio)));
                    ui.label(RichText::new(text).color(pal().muted));
                }
                Err(e) => {
                    ui.label(RichText::new(e).color(pal().error));
                }
            },
            Some(Err(e)) => {
                ui.label(RichText::new(e.as_str()).color(pal().error));
            }
            None => {}
        }
    }
}

/// A scale's n as it reads: whole when it is near enough, else to two places.
fn ratio_text(n: f64) -> String {
    if (n - n.round()).abs() < 0.005 { format!("{}", n.round()) } else { format!("{n:.2}") }
}

/// The page setup's controls.
fn page_setup(ui: &mut Ui, p: &mut PageSetup, ratio: &mut f64, picture: bool) {
    ui.add_space(8.0);
    ui.label(RichText::new(t("Page")).strong());
    egui::Grid::new("export-page").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(t("Paper"));
        let own = t("The drawing's own").to_string();
        ComboBox::from_id_salt("export-paper").selected_text(p.paper.clone().unwrap_or_else(|| own.clone())).show_ui(ui, |ui| {
            ui.selectable_value(&mut p.paper, None, own);
            for name in PAPERS {
                ui.selectable_value(&mut p.paper, Some(name.to_string()), name);
            }
        });
        ui.end_row();
        ui.label(t("Orientation"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut p.orientation, Orientation::Auto, t("Auto"));
            ui.radio_value(&mut p.orientation, Orientation::Portrait, t("Portrait"));
            ui.radio_value(&mut p.orientation, Orientation::Landscape, t("Landscape"));
        });
        ui.end_row();
        ui.label(t("Scale"));
        ui.horizontal(|ui| {
            let custom = matches!(p.scale, Scale::Custom(_));
            if ui.radio(p.scale == Scale::Original, t("Original")).clicked() {
                p.scale = Scale::Original;
            }
            if ui.radio(p.scale == Scale::Fit, t("Fit to page")).clicked() {
                p.scale = Scale::Fit;
            }
            if ui.radio(custom, t("1 :")).clicked() {
                p.scale = Scale::Custom(*ratio);
            }
            let typed = ui.add(DragValue::new(ratio).range(page::MIN_RATIO..=page::MAX_RATIO).speed(1.0));
            if typed.changed() {
                p.scale = Scale::Custom(*ratio);
            }
        });
        ui.end_row();
        ui.label(t("Margin"));
        ui.add(DragValue::new(&mut p.margin).range(0.0..=page::MAX_MARGIN).suffix(" mm"));
        ui.end_row();
        ui.label(t("Placement"));
        anchor_picker(ui, &mut p.anchor);
        ui.end_row();
        ui.label(t("Colour"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut p.colour, Colour::Colour, t("Colour"));
            ui.radio_value(&mut p.colour, Colour::Grey, t("Greyscale"));
            ui.radio_value(&mut p.colour, Colour::Black, t("Black and white"));
        });
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut p.hairlines, t("Hairlines"));
        ui.end_row();
        if picture {
            ui.label(t("Resolution"));
            let mut dpi = p.dpi();
            ui.add(DragValue::new(&mut dpi).range(page::MIN_DPI..=page::MAX_DPI).suffix(" dpi"));
            p.dpi = Some(dpi);
            ui.end_row();
        }
        ui.label(t("Pages"));
        ui.horizontal(|ui| {
            let mut all = p.pages.is_none();
            if ui.checkbox(&mut all, t("All")).changed() {
                p.pages = if all { None } else { Some([1, 1]) };
            }
            if let Some([first, last]) = &mut p.pages {
                ui.add(DragValue::new(first).range(1..=page::MAX_PAGES));
                ui.label(t("to"));
                ui.add(DragValue::new(last).range(*first..=page::MAX_PAGES));
            }
        });
        ui.end_row();
    });
}

/// Where the drawing sits on its pages: nine squares, as a print dialog's.
fn anchor_picker(ui: &mut Ui, anchor: &mut Anchor) {
    let size = 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(size * 3.0 + 4.0, size * 3.0 + 4.0), Sense::hover());
    for a in Anchor::ALL {
        let (c, r) = a.cell();
        let min = rect.min + vec2(f32::from(c) * (size + 2.0), f32::from(r) * (size + 2.0));
        let cell = Rect::from_min_size(min, vec2(size, size));
        let id = ui.id().with(("anchor", c, r));
        let response = ui.interact(cell, id, Sense::click());
        let response = response.on_hover_text(t(anchor_name(a)));
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, *anchor == a, t(anchor_name(a))));
        if response.clicked() {
            *anchor = a;
        }
        let fill = if *anchor == a { pal().accent } else { pal().surface };
        ui.painter().rect(cell, 2.0, fill, Stroke::new(1.0, pal().muted), StrokeKind::Inside);
    }
}

fn anchor_name(a: Anchor) -> &'static str {
    match a {
        Anchor::TopLeft => msg!("Top left"),
        Anchor::Top => msg!("Top"),
        Anchor::TopRight => msg!("Top right"),
        Anchor::Left => msg!("Left"),
        Anchor::Centre => msg!("Centre"),
        Anchor::Right => msg!("Right"),
        Anchor::BottomLeft => msg!("Bottom left"),
        Anchor::Bottom => msg!("Bottom"),
        Anchor::BottomRight => msg!("Bottom right"),
    }
}

impl Dialog {
    /// Whether the preview's drawing has come, or said why not.
    #[cfg(test)]
    pub(crate) fn preview_ready(&self) -> bool {
        self.preview.made.is_some()
    }
}

impl Preview {
    /// Asks for the drawing of `source` of the project `m`, when it isn't the one made or being made.
    fn ask(&mut self, ctx: &Context, m: &Arc<Model>, source: Option<Id>) {
        let of = (source, Arc::as_ptr(m) as usize);
        if self.of != Some(of) {
            self.of = Some(of);
            self.made = None;
            self.styled = None;
            let (send, answer) = channel();
            let (m, repaint) = (Arc::clone(m), ctx.clone());
            let spawned = std::thread::Builder::new().name("export-preview".into()).spawn(move || {
                let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let seen = bimcraft_core::link::seen(&m);
                    match export::prepare(&m, &seen, source, Takes::Drawing)?.content {
                        Content::Drawing(paged) => Ok(*paged),
                        _ => Err("nothing to draw".to_string()),
                    }
                }))
                .unwrap_or_else(|_| Err("the preview stopped".to_string()));
                let _ = send.send(made);
                repaint.request_repaint();
            });
            self.answer = spawned.ok().map(|_| answer);
            if self.answer.is_none() {
                self.made = Some(Err("could not start the preview".to_string()));
            }
        }
        if let Some(answer) = &self.answer {
            match answer.try_recv() {
                Ok(made) => {
                    self.made = Some(made.map(Arc::new));
                    self.answer = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    self.made = Some(Err("the preview stopped".to_string()));
                    self.answer = None;
                }
            }
        }
    }

    /// The pages, the margins, and the drawing on them as the file will hold it.
    fn show(&mut self, ui: &mut Ui, setup: &PageSetup) {
        let (rect, _) = ui.allocate_exact_size(PREVIEW, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, pal().surface);
        let paged = match &self.made {
            Some(Ok(paged)) => Arc::clone(paged),
            Some(Err(_)) => return,
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("Drawing the preview…"),
                    egui::FontId::proportional(14.0),
                    pal().muted,
                );
                return;
            }
        };
        let Ok(layout) = paged.layout(setup) else { return };
        let gap = 6.0;
        let (cols, rows) = (layout.cols.max(1) as f32, layout.rows.max(1) as f32);
        let (w, h) = (layout.paper.0 as f32, layout.paper.1 as f32);
        let room = rect.shrink(12.0);
        let s = ((room.width() - gap * (cols - 1.0)) / (w * cols)).min((room.height() - gap * (rows - 1.0)) / (h * rows));
        if !(s.is_finite() && s > 0.0) {
            return;
        }
        let whole = vec2(w * s * cols + gap * (cols - 1.0), h * s * rows + gap * (rows - 1.0));
        let top_left = room.center() - whole / 2.0;
        let styled = self.drawing(&paged.drawing, setup);
        let drawing = styled.as_deref().unwrap_or(&paged.drawing);
        let page_at = |c: u32, r: u32| -> Pos2 { top_left + vec2(c as f32 * (w * s + gap), r as f32 * (h * s + gap)) };
        for r in 0..layout.rows {
            for c in 0..layout.cols {
                let page = Rect::from_min_size(page_at(c, r), vec2(w * s, h * s));
                let printed = layout.pages.iter().any(|p| p.col == c && p.row == r);
                painter.rect_filled(page, 0.0, if printed { egui::Color32::WHITE } else { egui::Color32::from_gray(200) });
                painter.rect_stroke(page, 0.0, Stroke::new(1.0, pal().muted), StrokeKind::Outside);
            }
        }
        for tile in &layout.pages {
            let corner = page_at(tile.col, tile.row);
            let place = paged.place(&layout, tile);
            let clip = place.clip.map_or(Rect::NOTHING, |[x, y, cw, ch]| {
                Rect::from_min_size(corner + vec2(x as f32, y as f32) * s, vec2(cw as f32, ch as f32) * s)
            });
            let origin = corner + vec2(place.at.0 as f32, place.at.1 as f32) * s;
            let at = Place { origin, k: place.k as f32 * s, paper: true, hide: None };
            crate::canvas::paint(&painter.with_clip_rect(clip), at, drawing, &[], None, setup.hairlines, &[]);
            if layout.margin > 0.0 {
                painter.rect_stroke(clip, 0.0, Stroke::new(0.5, pal().accent.gamma_multiply(0.5)), StrokeKind::Outside);
            }
        }
    }

    /// The drawing in the setup's colours and lines, made once for each; none, the drawing as it is.
    fn drawing(&mut self, d: &Drawing, setup: &PageSetup) -> Option<Arc<Drawing>> {
        let want = (setup.colour, setup.hairlines);
        match &self.styled {
            Some((c, h, made)) if (*c, *h) == want => made.clone(),
            _ => {
                let made = export::formats::styled(d, setup).map(Arc::new);
                self.styled = Some((want.0, want.1, made.clone()));
                made
            }
        }
    }
}
