//! Structure > Quantities > Materials (QT), Steel List (BL) and Cut Plan (CU): one dialog over the `quantities`, `steel_list` and
//! `cut_plan` commands, which read the project and change nothing. It asks which members or bars (a level, or what is selected),
//! shows what the command returned, writes it to a .csv or .xlsx file, or makes the schedules of it (one undo step). A refusal is the
//! command's words, said inside the dialog. Nothing is calculated until Calculate, except the page the dialog opens on: a cut plan is
//! a purchase, so it waits for the stock length the engineer types.

use bimcraft_core::model::Id;
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Align, Checkbox, ComboBox, Context, Grid, Id as UiId, Layout, RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::{self, Modal};
use crate::reinforce::labelled;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// The most rows of the list the dialog draws: the rest are in the file.
const SHOWN: usize = 300;

/// A page of the dialog: the concrete, formwork and steel of the members, the list of the bars, or the plan to cut them.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    List,
    Plan,
    Materials,
}

impl Page {
    fn command(self) -> &'static str {
        match self {
            Page::List => "steel_list",
            Page::Plan => "cut_plan",
            Page::Materials => "quantities",
        }
    }
}

/// The dialog: the page it is on, which members or bars, what is typed, and what each page last calculated.
#[derive(Default)]
pub(crate) struct SteelDialog {
    pub(crate) page: Page,
    /// A level's name, or none for the whole project.
    pub(crate) level: String,
    /// Only the bars of what is selected, when something is.
    pub(crate) selected: bool,
    pub(crate) coil_to: String,
    pub(crate) stock_length: String,
    kerf: String,
    min_offcut: String,
    pub(crate) list: Option<Value>,
    pub(crate) plan: Option<Value>,
    /// The project parameter the Materials page splits by, or none.
    pub(crate) by: String,
    pub(crate) quantities: Option<Value>,
    pub(crate) error: String,
}

/// A length typed in the project's units, in mm; none when it is blank.
fn length(units: Units, text: &str, name: &str) -> Result<Option<f64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    units.parse(text).map(Some).ok_or_else(|| tf("{} is not {}: `{}`", &[name, t("a length"), text]))
}

impl SteelDialog {
    /// The command's parameters from the fields; the cut plan has no stock length of its own to fall back on.
    pub(crate) fn params(&self, page: Page, units: Units, selection: &[Id]) -> Result<Value, String> {
        let mut p = json!({});
        if !self.level.is_empty() {
            p["level"] = json!(self.level);
        }
        if self.selected && !selection.is_empty() {
            p["ids"] = json!(selection);
        }
        if page == Page::Materials {
            if !self.by.is_empty() {
                p["by"] = json!(self.by);
            }
            return Ok(p);
        }
        let coil = self.coil_to.trim().replace(',', ".");
        if !coil.is_empty() {
            let size = coil.parse::<f64>().ok().filter(|x| x.is_finite());
            p["coil_to"] = json!(size.ok_or_else(|| tf("{} is not {}: `{}`", &[t("Coil up to"), t("a size in mm"), &coil]))?);
        }
        let stock = length(units, &self.stock_length, t("Stock length"))?;
        if page == Page::Plan {
            let Some(stock) = stock else {
                return Err(t("Type the length of a stock bar first: it is a purchase, and yours to say.").into());
            };
            p["stock_length"] = json!(stock);
            for (key, typed, name) in [("kerf", &self.kerf, t("Kerf")), ("min_offcut", &self.min_offcut, t("Minimum offcut"))] {
                if let Some(x) = length(units, typed, name)? {
                    p[key] = json!(x);
                }
            }
        } else if let Some(stock) = stock {
            p["stock_length"] = json!(stock);
        }
        Ok(p)
    }

    fn reply(&self, page: Page) -> &Option<Value> {
        match page {
            Page::List => &self.list,
            Page::Plan => &self.plan,
            Page::Materials => &self.quantities,
        }
    }
}

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn whole(v: &Value, key: &str) -> String {
    v[key].as_u64().map_or_else(String::new, |n| n.to_string())
}

/// A diameter in mm, without a trailing zero.
fn size(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0)
}

fn kg(x: f64) -> String {
    format!("{x:.1}")
}

/// A table of text: bold headings, then a row each.
fn grid(ui: &mut Ui, id: &str, heads: &[&str], rows: &[Vec<String>]) {
    Grid::new(id).striped(true).spacing(vec2(20.0, 4.0)).show(ui, |ui| {
        for h in heads {
            ui.label(RichText::new(t(h)).strong());
        }
        ui.end_row();
        for row in rows {
            for cell in row {
                ui.label(cell);
            }
            ui.end_row();
        }
    });
}

fn notes(ui: &mut Ui, r: &Value) {
    for n in r["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        ui.label(format!("\u{2022} {n}"));
    }
}

/// What `steel_list` returned: its totals, a row for each host and mark, and the summary by grade and diameter.
fn list_view(ui: &mut Ui, r: &Value, units: Units) {
    let totals = &r["totals"];
    ui.label(tf(
        "{} bars in {} rows: {} of steel, {} kg.",
        &[&whole(totals, "bars"), &whole(totals, "rows"), &units.length(num(totals, "total_length")), &kg(num(totals, "weight_kg"))],
    ));
    ui.add_space(4.0);
    let all = r["rows"].as_array().map_or(&[][..], Vec::as_slice);
    let rows: Vec<Vec<String>> = all
        .iter()
        .take(SHOWN)
        .map(|row| {
            vec![
                text(row, "host_mark"),
                text(row, "mark"),
                size(num(row, "diameter")),
                text(row, "grade"),
                text(row, "shape"),
                whole(row, "count"),
                units.length(num(row, "length")),
                units.length(num(row, "total_length")),
                kg(num(row, "weight_kg")),
            ]
        })
        .collect();
    let heads = [
        msg!("Host Mark"),
        msg!("Mark"),
        msg!("Diameter"),
        msg!("Grade"),
        msg!("Shape"),
        msg!("Count"),
        msg!("Length"),
        msg!("Total Length"),
        msg!("Weight"),
    ];
    grid(ui, "steel-rows", &heads, &rows);
    if all.len() > SHOWN {
        ui.label(tf("{} more rows: write the file to see them all.", &[&(all.len() - SHOWN).to_string()]));
    }
    ui.add_space(8.0);
    let summary: Vec<Vec<String>> = r["summary"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| {
            let last = if line["form"] == "coil" { t("Coil").to_string() } else { whole(line, "stock_bars") };
            vec![
                text(line, "grade"),
                size(num(line, "diameter")),
                whole(line, "count"),
                units.length(num(line, "total_length")),
                kg(num(line, "weight_kg")),
                last,
            ]
        })
        .collect();
    grid(
        ui,
        "steel-summary",
        &[msg!("Grade"), msg!("Diameter"), msg!("Count"), msg!("Total Length"), msg!("Weight"), msg!("Stock bars")],
        &summary,
    );
    notes(ui, r);
}

/// What `cut_plan` returned: each diameter's stock bars and waste, what comes off a coil, and the notes.
fn plan_view(ui: &mut Ui, r: &Value, units: Units) {
    let totals = &r["totals"];
    ui.label(tf(
        "{} stock bars: {} kg bought, {} kg used.",
        &[&whole(totals, "stock_bars"), &kg(num(totals, "bought_kg")), &kg(num(totals, "used_kg"))],
    ));
    ui.add_space(4.0);
    let diameters: Vec<Vec<String>> = r["diameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            vec![
                text(d, "grade"),
                size(num(d, "diameter")),
                whole(d, "stock_bars"),
                whole(d, "pieces"),
                units.length(num(d, "used_mm")),
                format!("{} ({:.1} %)", units.length(num(d, "waste_mm")), num(d, "waste_percent")),
                kg(num(d, "bought_kg")),
            ]
        })
        .collect();
    grid(
        ui,
        "steel-plan",
        &[msg!("Grade"), msg!("Diameter"), msg!("Stock bars"), msg!("Pieces"), msg!("Used"), msg!("Waste"), msg!("Bought")],
        &diameters,
    );
    let coil: Vec<Vec<String>> = r["coil"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            vec![
                text(c, "grade"),
                size(num(c, "diameter")),
                whole(c, "pieces"),
                units.length(num(c, "total_length")),
                kg(num(c, "weight_kg")),
            ]
        })
        .collect();
    if !coil.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(t("Off a coil")).strong());
        grid(ui, "steel-coil", &[msg!("Grade"), msg!("Diameter"), msg!("Pieces"), msg!("Total Length"), msg!("Weight")], &coil);
    }
    notes(ui, r);
}

/// A category as the Materials page names it.
fn category_name(category: &str) -> &str {
    match category {
        "beam" => msg!("Beam"),
        "column" => msg!("Column"),
        "structuralcolumn" => msg!("Structural Column"),
        "footing" => msg!("Isolated Footing"),
        "stripfooting" => msg!("Wall Footing"),
        "floor" => msg!("Floor"),
        "unhosted bars" => msg!("Unhosted bars"),
        other => other,
    }
}

/// What `quantities` returned: its total, a row for each level and category (and value of the parameter split by), and the notes.
fn materials_view(ui: &mut Ui, r: &Value, units: Units, by: &str) {
    let total = &r["total"];
    let per = |v: &Value, places: usize| v.as_f64().map_or_else(|| "-".to_string(), |x| format!("{x:.places$}"));
    let (area, unit) = units.area(num(total, "formwork_m2") * 1e6);
    ui.label(tf(
        "{} members: {} m³ of concrete, {} {} of formwork, {} kg of steel ({} kg/m³).",
        &[
            &whole(total, "count"),
            &per(&total["volume_m3"], 3),
            &format!("{area:.2}"),
            unit,
            &per(&total["rebar_kg"], 1),
            &per(&total["rebar_ratio"], 1),
        ],
    ));
    ui.add_space(4.0);
    let rows: Vec<Vec<String>> = r["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .take(SHOWN)
        .map(|row| {
            let mut cells = vec![text(row, "level"), t(category_name(&text(row, "category"))).to_string()];
            if !by.is_empty() {
                cells.push(text(row, "value"));
            }
            let area = units.area(num(row, "formwork_m2") * 1e6).0;
            cells.extend([
                whole(row, "count"),
                per(&row["volume_m3"], 3),
                format!("{area:.2}"),
                per(&row["rebar_kg"], 1),
                per(&row["rebar_ratio"], 1),
            ]);
            cells
        })
        .collect();
    let mut heads = vec![t("Level").to_string(), t("Category").to_string()];
    if !by.is_empty() {
        heads.push(by.to_string());
    }
    heads.extend([
        t("Count").to_string(),
        t("Volume (m³)").to_string(),
        tf("{} ({})", &[t("Formwork Area"), unit]),
        tf("{} (kg)", &[t("Rebar Weight")]),
        tf("{} (kg/m³)", &[t("Rebar Ratio")]),
    ]);
    grid(ui, "materials-rows", &heads.iter().map(String::as_str).collect::<Vec<_>>(), &rows);
    notes(ui, r);
}

impl BimApp {
    /// Materials (QT), Steel List (BL) or Cut Plan (CU): the dialog, on that page. What is not yet calculated for the page, except a
    /// cut plan, is made at once.
    pub(crate) fn open_steel(&mut self, page: Page) {
        let first = self.steel_dialog.is_none();
        let mut d =
            self.steel_dialog.take().unwrap_or_else(|| SteelDialog { selected: !self.selection.is_empty(), ..SteelDialog::default() });
        d.page = page;
        if first || (page != Page::Plan && d.reply(page).is_none()) {
            self.steel_calculate(&mut d, false);
        }
        self.steel_dialog = Some(d);
    }

    /// Runs the page's command with the fields and keeps its reply; a refusal is said in the dialog. `own_page` false is a run
    /// the dialog makes for itself, which is never a cut plan: that waits for a stock length, so it makes the list.
    fn steel_calculate(&mut self, d: &mut SteelDialog, own_page: bool) {
        let page = if own_page || d.page != Page::Plan { d.page } else { Page::List };
        d.error.clear();
        match d.params(page, self.session.model.units, &self.selection) {
            Err(e) => d.error = e,
            Ok(p) => match self.run(page.command(), p) {
                Some(reply) => match page {
                    Page::Plan => d.plan = Some(reply),
                    Page::List => d.list = Some(reply),
                    Page::Materials => d.quantities = Some(reply),
                },
                None => {
                    d.error = std::mem::take(&mut self.status.text);
                    self.status = Status::default();
                }
            },
        }
    }

    /// The dialog, while it is open.
    pub(crate) fn steel_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.steel_dialog.take() else { return };
        let units = self.session.model.units;
        let hint = units.name();
        let levels: Vec<String> = self.session.model.levels().into_iter().map(|(_, l)| l.name.clone()).collect();
        let parameters: Vec<String> = self.session.model.parameters.keys().cloned().collect();
        let has_selection = !self.selection.is_empty();
        let (mut calculate, mut export, mut make, mut close) = (false, false, false, false);
        let r = Modal::new(UiId::new("steel"), t("Quantities")).width(780.0).min_height(420.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (page, label) in [(Page::Materials, t("Materials")), (Page::List, t("Steel List")), (Page::Plan, t("Cut Plan"))] {
                        if ui.selectable_label(d.page == page, label).clicked() {
                            d.page = page;
                            d.error.clear();
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    let l = ui.label(t("Level:"));
                    let shown = if d.level.is_empty() { t("Whole project").to_string() } else { d.level.clone() };
                    let c = ComboBox::from_id_salt("steel-level").selected_text(shown).width(160.0).show_ui(ui, |ui| {
                        if ui.selectable_label(d.level.is_empty(), t("Whole project")).clicked() {
                            d.level.clear();
                        }
                        for n in &levels {
                            if ui.selectable_label(*n == d.level, n).clicked() {
                                d.level.clone_from(n);
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.add_enabled(has_selection, Checkbox::new(&mut d.selected, t("Only the selected elements")));
                });
                ui.horizontal_wrapped(|ui| {
                    if d.page == Page::Materials {
                        let l = ui.label(t("Split by:"));
                        let shown = if d.by.is_empty() { t("None").to_string() } else { d.by.clone() };
                        let c = ComboBox::from_id_salt("steel-by").selected_text(shown).width(160.0).show_ui(ui, |ui| {
                            if ui.selectable_label(d.by.is_empty(), t("None")).clicked() {
                                d.by.clear();
                            }
                            for n in &parameters {
                                if ui.selectable_label(*n == d.by, n).clicked() {
                                    d.by.clone_from(n);
                                }
                            }
                        });
                        c.response.labelled_by(l.id);
                        return;
                    }
                    let on = |label: &str| tf("{}:", &[t(label)]);
                    labelled(ui, &on("Coil up to"), &mut d.coil_to, t("Coil up to").to_string(), 56.0, "mm");
                    labelled(ui, &on("Stock length"), &mut d.stock_length, t("Stock length").to_string(), 72.0, hint);
                    if d.page == Page::Plan {
                        labelled(ui, &on("Kerf"), &mut d.kerf, t("Kerf").to_string(), 48.0, hint);
                        labelled(ui, &on("Minimum offcut"), &mut d.min_offcut, t("Minimum offcut").to_string(), 56.0, hint);
                    }
                });
                ui.separator();
                match (d.reply(d.page), d.page) {
                    (Some(reply), Page::Plan) => plan_view(ui, reply, units),
                    (Some(reply), Page::List) => list_view(ui, reply, units),
                    (Some(reply), Page::Materials) => materials_view(ui, reply, units, &d.by),
                    (None, Page::Plan) => {
                        ui.label(RichText::new(t("Type the length of a stock bar, then Calculate.")).color(pal().muted));
                    }
                    (None, _) => {}
                }
                if !d.error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&d.error).color(pal().error));
                }
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    calculate = ui.button(t("Calculate")).clicked();
                    export = ui.add_enabled(d.reply(d.page).is_some(), egui::Button::new(t("Export…"))).clicked();
                    match d.page {
                        Page::Materials => {
                            make = ui
                                .button(t("Create Schedules"))
                                .on_hover_text(t("Make the Materials Summary as a schedule in the project"))
                                .clicked();
                        }
                        Page::List => {
                            make = ui
                                .button(t("Create Schedules"))
                                .on_hover_text(t("Make the Steel List and the Steel Summary as schedules in the project"))
                                .clicked();
                        }
                        Page::Plan => {}
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        close = ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        let mut keep = !(close || r.should_close());
        if calculate || (d.page != Page::Plan && d.reply(d.page).is_none() && d.error.is_empty()) {
            self.steel_calculate(&mut d, calculate);
        }
        if export {
            // The parameters as they are now, so the file is what the fields say; the name decides .csv or .xlsx.
            match d.params(d.page, units, &self.selection) {
                Err(e) => d.error = e,
                Ok(p) => {
                    let cmd = d.page.command();
                    let file = match d.page {
                        Page::Plan => "cut-plan.csv",
                        Page::List => "steel-list.csv",
                        Page::Materials => "quantities.csv",
                    };
                    let dialog = rfd::AsyncFileDialog::new().add_filter("CSV", &["csv"]).add_filter("Excel", &["xlsx"]).set_file_name(file);
                    self.ask_file(ctx, dialog, true, move |app, path| {
                        let mut p = p;
                        p["path"] = json!(path.to_string_lossy());
                        if app.run(cmd, p).is_some() {
                            app.status = Status { text: tf("Written to {}", &[&path.display().to_string()]), error: false };
                        }
                    });
                }
            }
        }
        if make && d.page == Page::Materials {
            // A schedule of several categories cannot split by a project parameter, so it is made for the level alone.
            let mut p = json!({"template": "materials_summary"});
            if !d.level.is_empty() {
                p["level"] = json!(d.level);
            }
            match self.exec_batch("Materials Summary", &[("schedule", p)]) {
                Ok(made) => {
                    if let Some(id) = made.first().and_then(|m| m["id"].as_u64()) {
                        self.open_sheet(Sheet::Table(id));
                    }
                    self.status = Status { text: t("Made the Materials Summary.").into(), error: false };
                    keep = false;
                }
                Err(e) => d.error = e,
            }
        } else if make {
            let mut of = json!({});
            if !d.level.is_empty() {
                of["level"] = json!(d.level);
            }
            if d.selected && self.selection.len() == 1 {
                of["host"] = json!(self.selection[0]);
            }
            let with = |template: &str| {
                let mut p = of.clone();
                p["template"] = json!(template);
                p
            };
            match self.exec_batch("Steel List", &[("schedule", with("steel_list")), ("schedule", with("steel_summary"))]) {
                Ok(made) => {
                    if let Some(id) = made.first().and_then(|m| m["id"].as_u64()) {
                        self.open_sheet(Sheet::Table(id));
                    }
                    self.status = Status { text: t("Made the Steel List and the Steel Summary.").into(), error: false };
                    keep = false;
                }
                Err(e) => d.error = e,
            }
        }
        if keep {
            self.steel_dialog = Some(d);
        }
    }
}
