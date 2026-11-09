//! File > Export… (Ctrl+P): the active view, sheet or schedule, or the whole project, to any format the session's exports
//! list, or printed. The page is set up as a print dialog sets it (paper, its own size, orientation, scale, margin,
//! placement, colour, hairlines, resolution, pages), and so are the extents: all the view shows, or a region drawn in it
//! while the dialog steps aside. A PDF has its own settings: an archive standard (PDF/A), its version, lines or pictures,
//! cut marks, and the document's title. The preview lays the drawing on its pages with the core's own [`page::layout`]
//! and [`Paged::place`], so what it shows is what the file holds. Export asks where the file goes, then writes it on a
//! thread of its own ([`worker`]); Print writes the PDF and hands it to the system's spooler ([`print`]). The settings of
//! each format are kept for the next export while the window is open.
//!
//! The preview's drawing, and the list of printers, are made on threads too: a big view takes a while to draw, and the
//! dialog stays live meanwhile.

pub(crate) mod print;
pub(crate) mod worker;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::export::page::{self, Anchor, Colour, Orientation, PageSetup, Pages, Scale};
use bimcraft_core::export::{self, Content, Family, Format, Item, Job, Kind, Options, Paged, Status as Ready, Takes};
use bimcraft_core::geom::P2;
use bimcraft_core::ifc::Origin;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::pdf::{Archive, Version};
use bimcraft_core::render::Drawing;
use bimcraft_i18n::msg;
use egui::{Align, ComboBox, Context, DragValue, Layout, Painter, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::canvas::Place;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool};

/// The papers the dialog lists, by the names `model::paper` knows.
const PAPERS: [&str; 12] = ["A4", "A3", "A2", "A1", "A0", "Letter", "Tabloid", "ARCH A", "ARCH B", "ARCH C", "ARCH D", "ARCH E"];

/// The preview's size on screen.
const PREVIEW: egui::Vec2 = vec2(340.0, 300.0);

/// Where an export goes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Destination {
    /// A file, asked for when Export… is pressed.
    #[default]
    File,
    /// A printer, by name: a PDF made for it and handed to the spooler.
    Printer(String),
}

/// What is exported: what the window shows, every sheet, or a print set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Of {
    #[default]
    Shown,
    /// Every sheet, in one file (a format that combines them: PDF).
    AllSheets,
    /// A print set's views and sheets, by its name.
    Set(String),
}

/// The margin a new page setup starts at, mm.
pub(crate) const MARGIN: f64 = 5.0;

/// The Export dialog, while it is open.
pub(crate) struct Dialog {
    /// The format's id.
    pub(crate) format: String,
    /// What the window shows: the view, sheet or schedule exported, or none for the project.
    pub(crate) source: Option<Id>,
    /// The source, every sheet, or a print set.
    pub(crate) of: Of,
    /// The PDF Settings window is open.
    pub(crate) pdf_open: bool,
    /// Print Sets… was pressed: the window opens on the next frame.
    edit_sets: bool,
    /// The pages typed as a list, kept while another choice is picked.
    page_list: String,
    pub(crate) options: Options,
    /// The scale typed, 1 : n, kept while another scale is picked.
    pub(crate) ratio: f64,
    /// A region of the view rather than all of it: the one drawn, once it is (`options.page.region` while chosen).
    pub(crate) custom: bool,
    region: Option<[f64; 4]>,
    /// Stepped aside while the region is drawn in the view.
    pub(crate) picking: bool,
    pub(crate) destination: Destination,
    pub(crate) copies: u32,
    printers: Printers,
    preview: Preview,
}

/// The printers there are: being looked for, found, or why not.
#[derive(Default)]
struct Printers {
    answer: Option<Receiver<Result<Vec<String>, String>>>,
    found: Option<Result<Vec<String>, String>>,
}

/// What the preview shows: the drawing as the setup has it (its region, colours and lines), made for that setup.
type Shown = Result<(Arc<Paged>, Option<Arc<Drawing>>), String>;

/// The drawing the preview lays out: made on a thread for a source, and kept while the source and the project are the same.
#[derive(Default)]
struct Preview {
    /// The source and the project it was made of.
    of: Option<(Option<Id>, usize)>,
    answer: Option<Receiver<Result<Paged, String>>>,
    made: Option<Result<Arc<Paged>, String>>,
    /// The drawing cut to the region and in the colours and lines of the setup, for that region, colour and hairlines.
    shown: Option<(Option<[f64; 4]>, Colour, bool, Shown)>,
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

/// Whether a region of `kind` can be drawn: a view or sheet drawn flat in the window.
fn has_region(kind: Option<Kind>) -> bool {
    matches!(kind, Some(Kind::Plan | Kind::Section | Kind::Elevation | Kind::Sheet))
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

fn archive_name(a: Archive) -> String {
    match a {
        Archive::None => t("None").to_string(),
        a => a.label().to_string(),
    }
}

/// The rectangle being drawn for an export's region: from its first corner to the cursor, or the cursor's cross before it.
pub(crate) fn region_band(p: &Painter, first: Option<Pos2>, cursor: Pos2) {
    let stroke = Stroke::new(1.5, pal().accent);
    match first {
        Some(a) => {
            let r = Rect::from_two_pos(a, cursor);
            p.rect_filled(r, 0.0, pal().accent.gamma_multiply(0.08));
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            p.extend(egui::Shape::dashed_line(&corners, stroke, 8.0, 4.0));
        }
        None => {
            p.line_segment([cursor - vec2(10.0, 0.0), cursor + vec2(10.0, 0.0)], stroke);
            p.line_segment([cursor - vec2(0.0, 10.0), cursor + vec2(0.0, 10.0)], stroke);
        }
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
        let mut options = self.export_last.get(&format).cloned().unwrap_or_else(|| {
            let mut o = Options::default();
            o.page.margin = MARGIN;
            o
        });
        // A region is of the view it was drawn in.
        options.page.region = None;
        let ratio = match options.page.scale {
            Scale::Custom(n) => n,
            _ => source.map_or(100.0, |id| bimcraft_core::graphics::scale_of(&self.session.model, id)).max(1.0),
        };
        let printers = Printers::ask(Arc::clone(&self.spooler));
        self.export_dialog = Some(Dialog {
            format,
            source,
            of: Of::Shown,
            pdf_open: false,
            edit_sets: false,
            page_list: String::new(),
            options,
            ratio,
            custom: false,
            region: None,
            picking: false,
            destination: Destination::File,
            copies: 1,
            printers,
            preview: Preview::default(),
        });
    }

    /// A click while the export's region is drawn, at `at` in the view's own coordinates: its first corner, then its
    /// opposite one, which brings the dialog back with it.
    pub(crate) fn export_region_at(&mut self, at: P2) {
        let Tool::ExportRegion(first) = self.tool else { return };
        let Some(a) = first else {
            self.tool = Tool::ExportRegion(Some(at));
            return;
        };
        let here = self.export_source();
        if let Some(d) = &mut self.export_dialog {
            if here == d.source {
                let region = [a.0.min(at.0), a.1.min(at.1), a.0.max(at.0), a.1.max(at.1)];
                d.region = Some(region);
                d.custom = true;
                d.options.page.region = Some(region);
            } else {
                self.status = Status { text: t("The region was drawn in another view than the one exported.").into(), error: true };
            }
            d.picking = false;
        }
        self.set_tool(Tool::Modify);
    }

    /// The Export dialog, while it is open.
    pub(crate) fn export_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.export_dialog.take() else { return };
        if d.picking {
            // Stepped aside while the region is drawn: another tool taken (Esc) brings it back as it was.
            if matches!(self.tool, Tool::ExportRegion(_)) {
                self.export_dialog = Some(d);
                return;
            }
            d.picking = false;
        }
        let model = Arc::clone(&self.session.model);
        let registry = self.session.exports.clone();
        let kind = export::kind_of(&model, d.source);
        d.printers.poll(ctx);
        // A printer prints PDF.
        if matches!(d.destination, Destination::Printer(_)) && d.format != "pdf" {
            d.format = "pdf".to_string();
            if let Some(kept) = self.export_last.get("pdf") {
                d.options = kept.clone();
            }
        }
        let Ok(format) = registry.find(&d.format).cloned() else { return };
        let given = source_for(format.as_ref(), kind, d.source).unwrap_or(None);
        let sheets = sheets_by_number(&model);
        // A set gone (deleted, renamed) is no choice.
        if let Of::Set(name) = &d.of
            && !model.print_sets.contains_key(name)
        {
            d.of = Of::Shown;
        }
        // Several sources: every sheet, or a set's views; none, the one shown.
        let many: Option<Vec<Id>> = match &d.of {
            Of::AllSheets if format.combines() && !sheets.is_empty() => Some(sheets.clone()),
            Of::Set(name) => {
                let views = model.print_sets.get(name).map(|s| s.views.iter().filter_map(|g| model.elements.id(*g)).collect());
                let fits = |id: &Id| source_for(format.as_ref(), export::kind_of(&model, Some(*id)), Some(*id)).flatten().is_some();
                views.map(|v: Vec<Id>| v.into_iter().filter(fits).collect())
            }
            _ => None,
        };
        let all_sheets = many.is_some();
        // The region is the view's, not every sheet's.
        d.options.page.region = if d.custom && !all_sheets && has_region(kind) { d.region } else { None };
        // The preview's drawing: the source's, or the first of several.
        let shown = match &many {
            Some(ids) => ids.first().copied(),
            None => given,
        };
        if format.paged() {
            d.preview.ask(ctx, &model, shown);
        }
        let printing = matches!(d.destination, Destination::Printer(_));
        let waiting = d.custom && d.options.page.region.is_none() && has_region(kind) && !all_sheets;
        let empty = many.as_ref().is_some_and(Vec::is_empty);
        let (mut close, mut go) = (false, false);
        let r = Modal::new(egui::Id::new("export-dialog"), t("Export")).width(780.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(400.0);
                        self.export_options(ui, &mut d, &registry, format.as_ref(), kind, given, &model, sheets.len(), many.as_deref());
                    });
                    // A separator here would take all the height the form may scroll to: a gap instead.
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        if format.paged() {
                            Self::export_side(ui, &mut d, format.as_ref());
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
                    let label = if printing { t("Print") } else { t("Export…") };
                    let button = ui.add_enabled(!busy && !waiting && !empty, egui::Button::new(label));
                    let button = if busy {
                        button.on_disabled_hover_text(t("An export is being written"))
                    } else if empty {
                        button.on_disabled_hover_text(t("The print set holds nothing this format writes"))
                    } else {
                        button.on_disabled_hover_text(t("Draw the region first"))
                    };
                    go = button.clicked();
                    close = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if d.pdf_open {
            let mut done = false;
            let r = Modal::new(egui::Id::new("export-pdf-settings"), t("PDF Settings")).width(380.0).show(ctx, |ui| {
                crate::popup::body(ui, |ui| pdf_settings(ui, &mut d.options.pdf));
                crate::popup::footer(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| done = ui.button(t("OK")).clicked());
                });
            });
            if done || r.should_close() {
                d.pdf_open = false;
            }
        }
        if d.picking {
            self.export_dialog = Some(d);
            self.set_tool(Tool::ExportRegion(None));
            return;
        }
        if d.edit_sets {
            d.edit_sets = false;
            let set = match &d.of {
                Of::Set(name) => Some(name.clone()),
                _ => None,
            };
            self.open_print_sets(set);
        }
        if go {
            let items: Vec<Item> = match &many {
                Some(ids) => ids.iter().map(|id| Item { view: Some(model.guid(*id)), format: format.id().to_string() }).collect(),
                None => vec![Item { view: given.map(|id| model.guid(id)), format: format.id().to_string() }],
            };
            let mut kept = d.options.clone();
            kept.page.region = None;
            self.export_last.insert(d.format.clone(), kept);
            self.export_format = Some(d.format.clone());
            let (options, combine) = (d.options.clone(), all_sheets);
            match &d.destination {
                Destination::Printer(printer) => {
                    let title = match given {
                        _ if all_sheets => match &d.of {
                            Of::Set(name) => format!("{} - {name}", model.name),
                            _ => model.name.clone(),
                        },
                        Some(id) => export::title(&model, Some(id), kind.unwrap_or(Kind::Project)),
                        None => model.name.clone(),
                    };
                    self.print(printer.clone(), d.copies, items, options, title);
                }
                Destination::File => {
                    let what = format_name(format.as_ref());
                    // Several sources to a format that doesn't combine them: a file each, named after the one asked for.
                    let combine = combine && format.combines();
                    let each = all_sheets && !combine;
                    self.ask_path(ctx, &what, format.extension(), move |app, path| {
                        app.export_to(path, items, options, combine, each);
                    });
                }
            }
            return;
        }
        if !(close || r.should_close()) {
            self.export_dialog = Some(d);
        }
    }

    /// Writes `items` to the file `path` (a picture of several pages, a file a page beside it).
    /// `each`: a file for each item, named after `path` and the item's view.
    pub(crate) fn export_to(&mut self, path: PathBuf, items: Vec<Item>, options: Options, combine: bool, each: bool) {
        let folder = path.parent().map(PathBuf::from).unwrap_or_default();
        let stem = path.file_stem().map(|s| literal(&s.to_string_lossy())).unwrap_or_default();
        let name = if each { format!("{stem} - {{SheetNumber}} {{ViewName}}") } else { stem.clone() };
        let job = Job {
            items,
            folder,
            name: Some(name),
            combine: combine.then_some(stem),
            // The system's dialog asked before a file was written over.
            overwrite: true,
            options,
            ..Job::default()
        };
        self.start_export(job);
    }

    /// Prints `items` on `printer`: one PDF of them all, in a folder of its own that is removed once the spooler has it.
    fn print(&mut self, printer: String, copies: u32, items: Vec<Item>, options: Options, title: String) {
        static PRINTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = PRINTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let folder = std::env::temp_dir().join(format!("bimcraft-print-{}-{n}", std::process::id()));
        let media = print::media(options.page.paper.as_deref()).filter(|_| options.page.size.is_none());
        let job = Job {
            items,
            folder,
            name: Some("print".to_string()),
            combine: Some("print".to_string()),
            overwrite: true,
            options,
            ..Job::default()
        };
        let to = worker::PrintTo { spooler: Arc::clone(&self.spooler), printer, copies, media, title };
        self.start_print(job, to);
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
        many: Option<&[Id]>,
    ) {
        let printing = matches!(d.destination, Destination::Printer(_));
        egui::Grid::new("export-what").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(t("Destination"));
            ui.horizontal(|ui| {
                let shown = match &d.destination {
                    Destination::File => t("File").to_string(),
                    Destination::Printer(p) => p.clone(),
                };
                ComboBox::from_id_salt("export-destination").width(240.0).selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.destination, Destination::File, t("File"));
                    match &d.printers.found {
                        None => {
                            ui.label(RichText::new(t("Looking for printers…")).color(pal().muted));
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(tf(msg!("No printers: {}"), &[e])).color(pal().muted));
                        }
                        Some(Ok(names)) if names.is_empty() => {
                            ui.label(RichText::new(t("No printers")).color(pal().muted));
                        }
                        Some(Ok(names)) => {
                            for name in names {
                                ui.selectable_value(&mut d.destination, Destination::Printer(name.clone()), name);
                            }
                        }
                    }
                });
                if printing {
                    ui.label(t("Copies"));
                    ui.add(DragValue::new(&mut d.copies).range(1..=99));
                }
            });
            ui.end_row();
            ui.label(t("Format"));
            ui.add_enabled_ui(!printing, |ui| {
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
            });
            ui.end_row();
            ui.label(t("Of"));
            ui.vertical(|ui| {
                let what = match given {
                    Some(id) => export::title(model, Some(id), kind.unwrap_or(Kind::Project)),
                    None => tf(msg!("The whole project: {}"), &[&model.name]),
                };
                ui.radio_value(&mut d.of, Of::Shown, what);
                if format.combines() && sheets > 0 {
                    ui.radio_value(&mut d.of, Of::AllSheets, tf(msg!("All sheets ({}), in one file"), &[&sheets.to_string()]));
                }
                // A print set: its views and sheets, one file of them where the format combines them.
                ui.horizontal(|ui| {
                    let names: Vec<&String> = model.print_sets.keys().collect();
                    let on = matches!(d.of, Of::Set(_));
                    let first = names.first().map(|n| n.to_string());
                    let radio = ui.add_enabled(first.is_some(), egui::RadioButton::new(on, t("Print set")));
                    if radio.on_disabled_hover_text(t("The project has no print sets yet")).clicked()
                        && let Some(first) = first
                        && !on
                    {
                        d.of = Of::Set(first);
                    }
                    if let Of::Set(current) = &mut d.of {
                        ComboBox::from_id_salt("export-print-set").width(150.0).selected_text(current.as_str()).show_ui(ui, |ui| {
                            for name in &names {
                                ui.selectable_value(current, name.to_string(), name.as_str());
                            }
                        });
                    }
                    if ui.button(t("Print Sets…")).on_hover_text(t("Make and change the sets of views and sheets")).clicked() {
                        d.edit_sets = true;
                    }
                });
                if let (Of::Set(_), Some(ids)) = (&d.of, many) {
                    let text = if format.combines() {
                        tf(msg!("{} views and sheets, in one file"), &[&ids.len().to_string()])
                    } else {
                        tf(msg!("{} views and sheets, a file each"), &[&ids.len().to_string()])
                    };
                    ui.label(RichText::new(text).color(pal().muted));
                }
            });
            ui.end_row();
            // A drawing's extents: all of it, or a region drawn in the view.
            let drawing = format.takes().contains(&Takes::Drawing) && given.is_some();
            if drawing && has_region(kind) && many.is_none() {
                ui.label(t("Extents"));
                ui.vertical(|ui| {
                    ui.radio_value(&mut d.custom, false, t("All elements in view"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.custom, true, t("Custom region"));
                        if d.custom {
                            let label = if d.region.is_some() { t("Redraw region…") } else { t("Draw region…") };
                            if ui.button(label).on_hover_text(t("The dialog steps aside: click two corners in the view")).clicked() {
                                d.picking = true;
                            }
                        }
                    });
                    if d.custom {
                        let text = match d.region {
                            Some([x1, y1, x2, y2]) => {
                                let u = model.units;
                                tf(
                                    msg!("{} × {}, from {}, {}"),
                                    &[&u.with_unit(x2 - x1), &u.with_unit(y2 - y1), &u.with_unit(x1), &u.with_unit(y1)],
                                )
                            }
                            None => t("No region drawn yet").to_string(),
                        };
                        ui.label(RichText::new(text).color(pal().muted));
                    }
                });
                ui.end_row();
            }
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
        if format.takes().contains(&Takes::Drawing) && !format.paged() {
            // A drawing of no pages (SVG, DXF, DWG) still takes its colours and lines.
            ui.add_space(8.0);
            egui::Grid::new("export-look").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                colour_rows(ui, &mut o.page);
            });
            return;
        }
        if !format.paged() {
            return;
        }
        let picture = format.family() == Family::Raster || (format.id() == "pdf" && o.pdf.raster);
        let laid = d.preview.layout(&o.page).and_then(Result::ok);
        let own = laid.as_ref().map(|l| l.paper);
        let pages = laid.as_ref().map_or(1, |l| l.cols * l.rows);
        let pdf = (format.id() == "pdf").then_some(&mut o.pdf);
        page_setup(ui, &mut o.page, &mut d.ratio, picture, own, pdf, pages, &mut d.page_list);
    }

    /// Beside the form: the preview, a PDF's own settings under it, and what the pages come to.
    fn export_side(ui: &mut Ui, d: &mut Dialog, format: &dyn Format) {
        d.preview.show(ui, &d.options.page);
        ui.add_space(4.0);
        let laid = d.preview.layout(&d.options.page);
        match (&d.preview.made, laid) {
            (Some(Ok(_)), Some(Ok(l))) => {
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
            (Some(Ok(_)), Some(Err(e))) => {
                ui.label(RichText::new(e).color(pal().error));
            }
            (Some(Err(e)), _) => {
                ui.label(RichText::new(e.as_str()).color(pal().error));
            }
            _ => {}
        }
        if format.id() == "pdf" {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(t("PDF Settings…")).on_hover_text(t("Archive (PDF/A), version, and the document's title")).clicked() {
                    d.pdf_open = true;
                }
                let s = &d.options.pdf;
                let said = format!("{} · PDF {}", archive_name(s.archive), s.version.label());
                ui.label(RichText::new(said).color(pal().muted));
            });
        }
    }
}

/// A scale's n as it reads: whole when it is near enough, else to two places.
fn ratio_text(n: f64) -> String {
    if (n - n.round()).abs() < 0.005 { format!("{}", n.round()) } else { format!("{n:.2}") }
}

/// The colour and hairline rows, in a two-column grid.
fn colour_rows(ui: &mut Ui, p: &mut PageSetup) {
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
}

/// The page setup's controls. `own` is the paper the layout has now: what a paper of its own starts at; `pdf`, a PDF's
/// settings, whose content and cut marks go under the colours; `pages`, how many the drawing takes; `list`, the pages typed.
#[allow(clippy::too_many_arguments)]
fn page_setup(
    ui: &mut Ui,
    p: &mut PageSetup,
    ratio: &mut f64,
    picture: bool,
    own: Option<P2>,
    pdf: Option<&mut bimcraft_core::pdf::Settings>,
    pages: u32,
    list: &mut String,
) {
    ui.add_space(8.0);
    ui.label(RichText::new(t("Page")).strong());
    egui::Grid::new("export-page").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(t("Paper"));
        ui.horizontal(|ui| {
            let own_name = t("The drawing's own").to_string();
            let custom_name = t("Custom size").to_string();
            let shown = match (&p.size, &p.paper) {
                (Some(_), _) => custom_name.clone(),
                (None, Some(name)) => name.clone(),
                (None, None) => own_name.clone(),
            };
            ComboBox::from_id_salt("export-paper").selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(p.size.is_none() && p.paper.is_none(), own_name).clicked() {
                    (p.size, p.paper) = (None, None);
                }
                for name in PAPERS {
                    if ui.selectable_label(p.size.is_none() && p.paper.as_deref() == Some(name), name).clicked() {
                        (p.size, p.paper) = (None, Some(name.to_string()));
                    }
                }
                if ui.selectable_label(p.size.is_some(), custom_name).clicked() && p.size.is_none() {
                    p.size = Some(own.unwrap_or(P2(420.0, 297.0)));
                }
            });
            if let Some(size) = &mut p.size {
                let side = page::MIN_PAPER..=page::MAX_PAPER;
                ui.add(DragValue::new(&mut size.0).range(side.clone()).suffix(" mm"));
                ui.label("×");
                ui.add(DragValue::new(&mut size.1).range(side).suffix(" mm"));
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
        colour_rows(ui, p);
        if let Some(s) = pdf {
            ui.label(t("Content"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.raster, false, t("Vector"));
                ui.radio_value(&mut s.raster, true, t("Raster"));
            });
            ui.end_row();
            ui.label("");
            ui.add_enabled(pages > 1, egui::Checkbox::new(&mut s.cut_marks, t("Draw cut marks")))
                .on_hover_text(t("A hairline in the margin where pages meet, to show where to cut them to put them together"))
                .on_disabled_hover_text(t("For a drawing printed across several pages"));
            ui.end_row();
        }
        if picture {
            ui.label(t("Resolution"));
            let mut dpi = p.dpi();
            ui.add(DragValue::new(&mut dpi).range(page::MIN_DPI..=page::MAX_DPI).suffix(" dpi"));
            p.dpi = Some(dpi);
            ui.end_row();
        }
        ui.label(t("Pages"));
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.radio(p.pages.is_none(), t("All")).clicked() {
                    p.pages = None;
                }
                let range = matches!(p.pages, Some(Pages::Range(_)));
                if ui.radio(range, t("From")).clicked() && !range {
                    p.pages = Some(Pages::Range([1, pages.max(1)]));
                }
                if let Some(Pages::Range([first, last])) = &mut p.pages {
                    ui.add(DragValue::new(first).range(1..=page::MAX_PAGES));
                    ui.label(t("to"));
                    ui.add(DragValue::new(last).range(*first..=page::MAX_PAGES));
                }
            });
            ui.horizontal(|ui| {
                let custom = matches!(p.pages, Some(Pages::List(_)));
                if ui.radio(custom, t("Custom")).clicked() && !custom {
                    p.pages = Some(Pages::List(if list.trim().is_empty() { "1".to_string() } else { list.clone() }));
                }
                if let Some(Pages::List(text)) = &mut p.pages {
                    let edit = egui::TextEdit::singleline(text).hint_text("1, 3, 5-7").char_limit(page::MAX_PAGE_LIST).desired_width(150.0);
                    ui.add(edit)
                        .on_hover_text(t("Page numbers and runs of them, by commas: 1, 3, 5-7"))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, t("Pages to print")));
                    list.clone_from(text);
                }
            });
        });
        ui.end_row();
    });
}

/// A PDF's document settings, in their own window: the archive standard, the version, and the document's title.
fn pdf_settings(ui: &mut Ui, s: &mut bimcraft_core::pdf::Settings) {
    egui::Grid::new("export-pdf").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(t("Archive"));
        ComboBox::from_id_salt("export-archive")
            .selected_text(archive_name(s.archive))
            .show_ui(ui, |ui| {
                for a in Archive::ALL {
                    ui.selectable_value(&mut s.archive, a, archive_name(a));
                }
            })
            .response
            .on_hover_text(t("PDF/A, for keeping: PDF/A-2b is the usual one. A file that wouldn't pass isn't written."));
        ui.end_row();
        ui.label(t("Version"));
        ui.add_enabled_ui(s.archive == Archive::None, |ui| {
            ComboBox::from_id_salt("export-pdf-version")
                .selected_text(s.version.label())
                .show_ui(ui, |ui| {
                    for v in Version::ALL {
                        ui.selectable_value(&mut s.version, v, v.label());
                    }
                })
                .response
                .on_disabled_hover_text(t("An archive has its standard's version"));
        });
        ui.end_row();
        let text_row = |ui: &mut Ui, label: &str, value: &mut Option<String>, id: &str| {
            ui.label(t(label));
            let mut text = value.clone().unwrap_or_default();
            if ui.add_sized([220.0, 20.0], egui::TextEdit::singleline(&mut text).id_salt(id)).changed() {
                *value = Some(text).filter(|t| !t.trim().is_empty());
            }
            ui.end_row();
        };
        text_row(ui, msg!("Title"), &mut s.title, "export-pdf-title");
        text_row(ui, msg!("Author"), &mut s.author, "export-pdf-author");
        text_row(ui, msg!("Subject"), &mut s.subject, "export-pdf-subject");
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

    /// Whether the printers have been looked for.
    #[cfg(test)]
    pub(crate) fn printers_found(&self) -> bool {
        self.printers.found.is_some()
    }

    /// The region drawn, in the view's own coordinates.
    #[cfg(test)]
    pub(crate) fn region(&self) -> Option<[f64; 4]> {
        self.region
    }
}

impl Printers {
    /// Looks for the printers on a thread: the spooler may be slow to answer.
    fn ask(spooler: Arc<dyn print::Spooler>) -> Printers {
        let (send, answer) = channel();
        let spawned = std::thread::Builder::new().name("printers".into()).spawn(move || {
            let _ = send.send(spooler.printers());
        });
        match spawned {
            Ok(_) => Printers { answer: Some(answer), found: None },
            Err(e) => Printers { answer: None, found: Some(Err(e.to_string())) },
        }
    }

    fn poll(&mut self, ctx: &Context) {
        let Some(answer) = &self.answer else { return };
        match answer.try_recv() {
            Ok(found) => {
                self.found = Some(found);
                self.answer = None;
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => {
                self.found = Some(Err("the spooler didn't answer".to_string()));
                self.answer = None;
            }
        }
    }
}

impl Preview {
    /// Asks for the drawing of `source` of the project `m`, when it isn't the one made or being made.
    fn ask(&mut self, ctx: &Context, m: &Arc<Model>, source: Option<Id>) {
        let of = (source, Arc::as_ptr(m) as usize);
        if self.of != Some(of) {
            self.of = Some(of);
            self.made = None;
            self.shown = None;
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

    /// The drawing as `setup` has it: cut to its region, in its colours and lines. Made once for each.
    fn shown(&mut self, setup: &PageSetup) -> Option<Shown> {
        let made = match &self.made {
            Some(Ok(paged)) => Arc::clone(paged),
            _ => return None,
        };
        let want = (setup.region, setup.colour, setup.hairlines);
        match &self.shown {
            Some((r, c, h, shown)) if (*r, *c, *h) == want => Some(shown.clone()),
            _ => {
                let paged = match setup.region {
                    Some(r) => made.cropped(r).map(Arc::new),
                    None => Ok(made),
                };
                let shown = paged.map(|paged| {
                    let styled = export::formats::styled(&paged.drawing, setup).map(Arc::new);
                    (paged, styled)
                });
                self.shown = Some((want.0, want.1, want.2, shown.clone()));
                Some(shown)
            }
        }
    }

    /// How the drawing as `setup` has it lies on its pages; none before it is drawn.
    fn layout(&mut self, setup: &PageSetup) -> Option<Result<page::Layout, String>> {
        match self.shown(setup)? {
            Ok((paged, _)) => Some(paged.layout(setup)),
            Err(e) => Some(Err(e)),
        }
    }

    /// The pages, the margins, and the drawing on them as the file will hold it.
    fn show(&mut self, ui: &mut Ui, setup: &PageSetup) {
        let (rect, _) = ui.allocate_exact_size(PREVIEW, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, pal().surface);
        if self.made.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                t("Drawing the preview…"),
                egui::FontId::proportional(14.0),
                pal().muted,
            );
            return;
        }
        let Some(Ok((paged, styled))) = self.shown(setup) else { return };
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
        // Each page's number, light over it, to tell them apart (and to type in Pages).
        if layout.cols * layout.rows > 1 {
            let size = (w.min(h) * s * 0.4).clamp(9.0, 48.0);
            for r in 0..layout.rows {
                for c in 0..layout.cols {
                    let page = Rect::from_min_size(page_at(c, r), vec2(w * s, h * s));
                    let n = r * layout.cols + c + 1;
                    let ink = egui::Color32::from_rgba_unmultiplied(120, 120, 120, 110);
                    painter.text(page.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(size), ink);
                }
            }
        }
    }
}
