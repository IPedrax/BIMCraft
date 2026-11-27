//! Structure > Quantities > Materials (QT), Steel List (BL) and Cut Plan (CU) as one dialog over the `quantities`, `steel_list` and `cut_plan`
//! commands, which read the project and change nothing. It asks which members or bars (a level, or what is selected), shows what the
//! command returned, writes it to a .csv or .xlsx file, or makes the schedules of it (one undo step). A refusal is the command's words,
//! said inside the dialog. Nothing is calculated until Calculate, except the page the dialog opens on: a cut plan is a purchase, so it waits
//! for the stock length the engineer types. egui's `steel.rs` is the model.

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::units::Units;
use rune::TextEdit;
use serde_json::{Value, json};

use super::{Cell, Choice, Discipline, Foot, Host, Outcome, Section, edits, typed};
use crate::lang::{t, tf};

/// The most rows of a list the dialog shows: the rest are in the file.
const SHOWN: usize = 300;
/// A column of a table, points.
const COLUMN: f32 = 86.0;

/// A page of the dialog: the concrete, formwork and steel of the members, the list of the bars, or the plan to cut them.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Page {
    #[default]
    List,
    Plan,
    Materials,
}

impl Page {
    pub fn command(self) -> &'static str {
        match self {
            Page::List => "steel_list",
            Page::Plan => "cut_plan",
            Page::Materials => "quantities",
        }
    }

    /// The tabs, in order.
    const TABS: [Page; 3] = [Page::Materials, Page::List, Page::Plan];

    fn name(self) -> &'static str {
        match self {
            Page::Materials => "Materials",
            Page::List => "Steel List",
            Page::Plan => "Cut Plan",
        }
    }

    fn file(self) -> &'static str {
        match self {
            Page::Plan => "cut-plan.csv",
            Page::List => "steel-list.csv",
            Page::Materials => "quantities.csv",
        }
    }
}

/// The edits: the coil's size, the stock bar's length, the saw's kerf and the least offcut kept.
const COIL: usize = 0;
const STOCK: usize = 1;
const KERF: usize = 2;
const OFFCUT: usize = 3;

/// The dialog: the page it is on, which members or bars, what is typed, and what each page last calculated.
pub struct Quantities {
    page: Page,
    /// A level's name, or none for the whole project.
    level: String,
    /// Only the bars of what is selected, when something is.
    selected: bool,
    selection: Vec<Id>,
    levels: Vec<String>,
    parameters: Vec<String>,
    /// The project parameter the Materials page splits by, or none.
    by: String,
    edits: Vec<TextEdit>,
    units: Units,
    list: Option<Value>,
    plan: Option<Value>,
    quantities: Option<Value>,
    error: Option<String>,
}

/// A length typed in the project's units, in mm; none when it is blank.
fn length(units: Units, text: &str, name: &str) -> Result<Option<f64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    units.parse(text).filter(|x| x.is_finite()).map(Some).ok_or_else(|| tf("{} is not {}: `{}`", &[name, t("a length"), text]))
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
fn table(heads: &[String], rows: Vec<Vec<String>>) -> Vec<Vec<Cell>> {
    let mut out = vec![heads.iter().map(|h| Cell::head(h.clone(), COLUMN)).collect::<Vec<_>>()];
    out.extend(rows.into_iter().map(|r| r.into_iter().map(|c| Cell::text(c, COLUMN)).collect::<Vec<_>>()));
    out
}

fn heads(list: &[&str]) -> Vec<String> {
    list.iter().map(|h| t(h).to_string()).collect()
}

/// What `steel_list` returned: its totals, a row for each host and mark, and the summary by grade and diameter.
fn list_view(r: &Value, units: Units) -> Vec<Section> {
    let totals = &r["totals"];
    let line = tf(
        "{} bars in {} rows: {} of steel, {} kg.",
        &[&whole(totals, "bars"), &whole(totals, "rows"), &units.length(num(totals, "total_length")), &kg(num(totals, "weight_kg"))],
    );
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
    let mut out = vec![Section::new(line, Vec::new())];
    let mut main = table(&heads(&["Host Mark", "Mark", "Diameter", "Grade", "Shape", "Count", "Length", "Total Length", "Weight"]), rows);
    if all.len() > SHOWN {
        main.push(vec![Cell::text(tf("{} more rows: write the file to see them all.", &[&(all.len() - SHOWN).to_string()]), 400.0)]);
    }
    out.push(Section::new("", main));
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
    out.push(Section::new("", table(&heads(&["Grade", "Diameter", "Count", "Total Length", "Weight", "Stock bars"]), summary)));
    out
}

/// What `cut_plan` returned: each diameter's stock bars and waste, what comes off a coil, and the notes.
fn plan_view(r: &Value, units: Units) -> Vec<Section> {
    let totals = &r["totals"];
    let line = tf(
        "{} stock bars: {} kg bought, {} kg used.",
        &[&whole(totals, "stock_bars"), &kg(num(totals, "bought_kg")), &kg(num(totals, "used_kg"))],
    );
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
    let mut out = vec![
        Section::new(line, Vec::new()),
        Section::new("", table(&heads(&["Grade", "Diameter", "Stock bars", "Pieces", "Used", "Waste", "Bought"]), diameters)),
    ];
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
        out.push(Section::new(t("Off a coil"), table(&heads(&["Grade", "Diameter", "Pieces", "Total Length", "Weight"]), coil)));
    }
    out
}

/// A category as the Materials page names it.
fn category_name(category: &str) -> &str {
    match category {
        "beam" => "Beam",
        "column" => "Column",
        "structuralcolumn" => "Structural Column",
        "footing" => "Isolated Footing",
        "stripfooting" => "Wall Footing",
        "floor" => "Floor",
        "unhosted bars" => "Unhosted bars",
        other => other,
    }
}

/// What `quantities` returned: its total, a row for each level and category (and value of the parameter split by).
fn materials_view(r: &Value, units: Units, by: &str) -> Vec<Section> {
    let total = &r["total"];
    let per = |v: &Value, places: usize| v.as_f64().map_or_else(|| "-".to_string(), |x| format!("{x:.places$}"));
    let (area, unit) = units.area(num(total, "formwork_m2") * 1e6);
    let line = tf(
        "{} members: {} m³ of concrete, {} {} of formwork, {} kg of steel ({} kg/m³).",
        &[
            &whole(total, "count"),
            &per(&total["volume_m3"], 3),
            &format!("{area:.2}"),
            unit,
            &per(&total["rebar_kg"], 1),
            &per(&total["rebar_ratio"], 1),
        ],
    );
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
    let mut head = vec![t("Level").to_string(), t("Category").to_string()];
    if !by.is_empty() {
        head.push(by.to_string());
    }
    head.extend([
        t("Count").to_string(),
        t("Volume (m³)").to_string(),
        tf("{} ({})", &[t("Formwork Area"), unit]),
        tf("{} (kg)", &[t("Rebar Weight")]),
        tf("{} (kg/m³)", &[t("Rebar Ratio")]),
    ]);
    vec![Section::new(line, Vec::new()), Section::new("", table(&head, rows))]
}

impl Quantities {
    /// Opens on `page`; what the page shows at once is asked of the project by `Discipline::opened`.
    pub fn open(page: Page, m: &Model, selection: &[Id]) -> Quantities {
        Quantities {
            page,
            level: String::new(),
            selected: !selection.is_empty(),
            selection: selection.to_vec(),
            levels: m.levels().into_iter().map(|(_, l)| l.name.clone()).collect(),
            parameters: m.parameters.keys().cloned().collect(),
            by: String::new(),
            edits: edits(["", "", "", ""].map(String::from)),
            units: m.units,
            list: None,
            plan: None,
            quantities: None,
            error: None,
        }
    }

    pub fn page(&self) -> Page {
        self.page
    }

    fn typed_text(&self, i: usize) -> String {
        self.edits.get(i).map(typed).unwrap_or_default()
    }

    /// The command's parameters from the fields; the cut plan has no stock length of its own to fall back on.
    pub fn params(&self, page: Page) -> Result<Value, String> {
        let mut p = json!({});
        if !self.level.is_empty() {
            p["level"] = json!(self.level);
        }
        if self.selected && !self.selection.is_empty() {
            p["ids"] = json!(self.selection);
        }
        if page == Page::Materials {
            if !self.by.is_empty() {
                p["by"] = json!(self.by);
            }
            return Ok(p);
        }
        let coil = self.typed_text(COIL).trim().replace(',', ".");
        if !coil.is_empty() {
            let size = coil.parse::<f64>().ok().filter(|x| x.is_finite());
            p["coil_to"] = json!(size.ok_or_else(|| tf("{} is not {}: `{}`", &[t("Coil up to"), t("a size in mm"), &coil]))?);
        }
        let stock = length(self.units, &self.typed_text(STOCK), t("Stock length"))?;
        if page == Page::Plan {
            let Some(stock) = stock else {
                return Err(t("Type the length of a stock bar first: it is a purchase, and yours to say.").to_string());
            };
            p["stock_length"] = json!(stock);
            for (key, i, name) in [("kerf", KERF, t("Kerf")), ("min_offcut", OFFCUT, t("Minimum offcut"))] {
                if let Some(x) = length(self.units, &self.typed_text(i), name)? {
                    p[key] = json!(x);
                }
            }
        } else if let Some(stock) = stock {
            p["stock_length"] = json!(stock);
        }
        Ok(p)
    }

    fn reply(&self, page: Page) -> Option<&Value> {
        match page {
            Page::List => self.list.as_ref(),
            Page::Plan => self.plan.as_ref(),
            Page::Materials => self.quantities.as_ref(),
        }
    }

    /// Runs the page's command with the fields and keeps its reply; a refusal is said in the dialog. `own_page` false is a run the dialog
    /// makes for itself, which is never a cut plan: that waits for a stock length, so it makes the list.
    pub fn calculate(&mut self, own_page: bool, host: &mut dyn Host) {
        let page = if own_page || self.page != Page::Plan { self.page } else { Page::List };
        self.error = None;
        match self.params(page) {
            Err(why) => self.error = Some(why),
            Ok(p) => match host.execute(page.command(), &p) {
                Ok(reply) => match page {
                    Page::Plan => self.plan = Some(reply),
                    Page::List => self.list = Some(reply),
                    Page::Materials => self.quantities = Some(reply),
                },
                Err(why) => self.error = Some(why),
            },
        }
    }

    /// Create Schedules: the page's result as schedules of the project, one undo step.
    fn make(&mut self, host: &mut dyn Host) -> Outcome {
        let with_level = |template: &str| {
            let mut p = json!({"template": template});
            if !self.level.is_empty() {
                p["level"] = json!(self.level);
            }
            p
        };
        let made = match self.page {
            // A schedule of several categories cannot split by a project parameter, so it is made for the level alone.
            Page::Materials => {
                host.batch("Materials Summary", &[("schedule", with_level("materials_summary"))]).map(|_| t("Made the Materials Summary."))
            }
            Page::List => {
                let mut list = with_level("steel_list");
                let mut summary = with_level("steel_summary");
                if self.selected && self.selection.len() == 1 {
                    list["host"] = json!(self.selection[0]);
                    summary["host"] = json!(self.selection[0]);
                }
                host.batch("Steel List", &[("schedule", list), ("schedule", summary)])
                    .map(|_| t("Made the Steel List and the Steel Summary."))
            }
            Page::Plan => return Outcome::Keep,
        };
        match made {
            Ok(words) => {
                host.say(words.to_string());
                Outcome::Close
            }
            Err(why) => {
                self.error = Some(why);
                Outcome::Keep
            }
        }
    }
}

impl Discipline for Quantities {
    fn title(&self) -> String {
        t("Quantities").to_string()
    }

    fn width(&self) -> f32 {
        860.0
    }

    fn tabs(&self) -> Vec<String> {
        Page::TABS.iter().map(|p| t(p.name()).to_string()).collect()
    }

    fn tab(&self) -> usize {
        Page::TABS.iter().position(|p| *p == self.page).unwrap_or(0)
    }

    fn opened(&mut self, host: &mut dyn Host) {
        if self.page != Page::Plan {
            self.calculate(false, host);
        }
    }

    fn sections(&self) -> Vec<Section> {
        let hint = self.units.name();
        let mut out = vec![Section::new(
            "",
            vec![vec![
                Cell::text(t("Level:"), 60.0),
                Cell::Pick { pick: 0, width: 180.0 },
                Cell::Check {
                    action: "selected",
                    label: t("Only the selected elements").to_string(),
                    on: self.selected,
                    enabled: !self.selection.is_empty(),
                },
            ]],
        )];
        out.push(Section::new(
            "",
            vec![if self.page == Page::Materials {
                vec![Cell::text(t("Split by:"), 80.0), Cell::Pick { pick: 1, width: 180.0 }]
            } else {
                let mut row = vec![
                    Cell::text(tf("{}:", &[t("Coil up to")]), 90.0),
                    Cell::field(COIL, t("Coil up to"), "mm", 56.0),
                    Cell::text(tf("{}:", &[t("Stock length")]), 100.0),
                    Cell::field(STOCK, t("Stock length"), hint, 72.0),
                ];
                if self.page == Page::Plan {
                    row.extend([
                        Cell::text(tf("{}:", &[t("Kerf")]), 50.0),
                        Cell::field(KERF, t("Kerf"), hint, 48.0),
                        Cell::text(tf("{}:", &[t("Minimum offcut")]), 120.0),
                        Cell::field(OFFCUT, t("Minimum offcut"), hint, 56.0),
                    ]);
                }
                row
            }],
        ));
        match (self.reply(self.page), self.page) {
            (Some(r), Page::Plan) => out.extend(plan_view(r, self.units)),
            (Some(r), Page::List) => out.extend(list_view(r, self.units)),
            (Some(r), Page::Materials) => out.extend(materials_view(r, self.units, &self.by)),
            (None, Page::Plan) => out.push(Section::new(t("Type the length of a stock bar, then Calculate."), Vec::new())),
            (None, _) => {}
        }
        out
    }

    fn edit(&self, i: usize) -> Option<&TextEdit> {
        self.edits.get(i)
    }

    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit> {
        self.edits.get_mut(i)
    }

    fn choice(&self, pick: usize) -> Option<Choice> {
        let (name, none, names, now) = match pick {
            0 => (t("Level:"), t("Whole project"), &self.levels, &self.level),
            1 => (t("Split by:"), t("None"), &self.parameters, &self.by),
            _ => return None,
        };
        let options: Vec<String> = std::iter::once(none.to_string()).chain(names.iter().cloned()).collect();
        let current = if now.is_empty() { Some(0) } else { names.iter().position(|n| n == now).map(|i| i + 1) };
        Some(Choice { name: name.to_string(), options, current })
    }

    fn choose(&mut self, pick: usize, option: usize, _host: &mut dyn Host) {
        let (names, now) = if pick == 0 { (&self.levels, &mut self.level) } else { (&self.parameters, &mut self.by) };
        *now = option.checked_sub(1).and_then(|i| names.get(i)).cloned().unwrap_or_default();
    }

    fn press(&mut self, action: &str, arg: usize, host: &mut dyn Host) -> Outcome {
        match action {
            "tab" => {
                if let Some(page) = Page::TABS.get(arg) {
                    self.page = *page;
                    self.error = None;
                    if *page != Page::Plan && self.reply(*page).is_none() {
                        self.calculate(false, host);
                    }
                }
            }
            "selected" => self.selected = !self.selected,
            "calculate" => self.calculate(true, host),
            "make" => return self.make(host),
            "close" => return Outcome::Close,
            _ => {}
        }
        Outcome::Keep
    }

    fn saved(&mut self, action: &str, path: std::path::PathBuf, host: &mut dyn Host) -> Outcome {
        if action != "export" {
            return Outcome::Keep;
        }
        // The parameters as they are now, so the file is what the fields say; the name decides .csv or .xlsx.
        match self.params(self.page) {
            Err(why) => self.error = Some(why),
            Ok(mut p) => {
                p["path"] = json!(path.to_string_lossy());
                match host.execute(self.page.command(), &p) {
                    Ok(_) => host.say(tf("Written to {}", &[&path.display().to_string()])),
                    Err(why) => self.error = Some(why),
                }
            }
        }
        Outcome::Keep
    }

    fn foot(&self) -> Vec<Foot> {
        let mut out = vec![Foot::new("calculate", t("Calculate"), false)];
        out.push(Foot { on: self.reply(self.page).is_some(), file: Some(self.page.file()), ..Foot::new("export", t("Export…"), false) });
        if self.page != Page::Plan {
            out.push(Foot::new("make", t("Create Schedules"), false));
        }
        out.push(Foot::new("close", t("Close"), true));
        out
    }

    fn notes(&self) -> Vec<String> {
        self.reply(self.page)
            .map(|r| r["notes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default()
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests;
