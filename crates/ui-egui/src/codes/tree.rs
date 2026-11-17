//! A rule whose value is more than a number, shown as what it is: an object as a small form, a list of plain values as a row of
//! fields, a list of rows as a table with named columns (an object inside a row flattened into columns of its own), and a list of
//! rows that hold lists of their own as cards, one a row, each shown the same way inside. Long text wraps at the panel's width.
//! The words of a code (`source`, `note`) are kept apart from its numbers: a row's source is its last column, an object's is a
//! line under it.

use egui::{Align, Layout, RichText, ScrollArea, Ui};
use serde_json::{Map, Value};

use super::{as_tsv, cell_text, cell_value, from_tsv};
use crate::lang::t;
use crate::style::pal;

/// What a rule's tables are shown with: the meanings of its columns, by name, where the code explains them.
#[derive(Default)]
pub(crate) struct Hints {
    pub(crate) columns: Map<String, Value>,
}

/// A key as a person reads it: `breaker_a` is "Breaker (A)", `up_to` "Up to".
pub(crate) fn human(key: &str) -> String {
    const UNITS: [(&str, &str); 18] = [
        ("_mm2", "mm²"),
        ("_m2", "m²"),
        ("_kva", "kVA"),
        ("_kw", "kW"),
        ("_kv", "kV"),
        ("_ma", "mA"),
        ("_mm", "mm"),
        ("_dan", "daN"),
        ("_cv", "cv"),
        ("_in", "in"),
        ("_va", "VA"),
        ("_a", "A"),
        ("_v", "V"),
        ("_w", "W"),
        ("_m", "m"),
        ("_ka", "kA"),
        ("_celsius", "°C"),
        ("_percent", "%"),
    ];
    let (stem, unit) = UNITS.iter().find_map(|(end, u)| key.strip_suffix(end).map(|s| (s, Some(*u)))).unwrap_or((key, None));
    let mut words = stem.replace(['_', '.'], " ");
    if let Some(first) = words.get(0..1) {
        words = first.to_uppercase() + &words[1..];
    }
    match unit {
        Some(u) => format!("{words} ({u})"),
        None => words,
    }
}

/// The columns of a table of plain rows (lists of numbers), by the rule's name: what each place is.
fn places(key: &str, width: usize) -> Vec<String> {
    let named: &[&str] = match key {
        "ampacity_pvc" | "ampacity_epr" => &[
            "Section (mm²)",
            "A1, 2 loaded",
            "A1, 3",
            "A2, 2",
            "A2, 3",
            "B1, 2",
            "B1, 3",
            "B2, 2",
            "B2, 3",
            "C, 2",
            "C, 3",
            "D, 2",
            "D, 3",
        ],
        "temperature_air" | "temperature_soil" => &["°C", "PVC factor", "EPR/XLPE factor"],
        "soil_resistivity" => &["K·m/W", "Factor"],
        "grouping_bundle_more" => &["Circuits up to", "Factor"],
        "buried_multi" | "buried_single" => &["2 circuits", "3", "4", "5", "6"],
        "neutral_reduced" => &["Phase (mm²)", "Neutral (mm²)"],
        "resistance_rigid" | "resistance_flexible" => &["Section (mm²)", "Ω/km at 20 °C"],
        "cable_diameters" => &["Section (mm²)", "Outside (mm)"],
        "sizes" => &["Size", "DN (mm)", "Outside (mm)", "Wall (mm)"],
        "least_outlets" => &["Area up to (m²)", "Outlets", "Kitchen outlets"],
        "window_air" => &["BTU/h", "VA", "W"],
        "motors_single_phase" | "motors_three_phase" => &["cv", "kW", "kVA"],
        "tiers" => &["Up to (kW; empty: the rest)", "Factor"],
        "by_area" => &["Area (m²)", "Demand (kVA)"],
        "steps" => &["Dwellings", "Factor"],
        "rows" => &["Up to", "Value"],
        _ => &[],
    };
    (0..width).map(|i| named.get(i).map_or_else(|| (i + 1).to_string(), |s| t(s).to_string())).collect()
}

fn scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

/// A list of a few plain values, shown in one cell.
fn short_list(v: &Value) -> bool {
    v.as_array().is_some_and(|a| a.len() <= 8 && a.iter().all(scalar))
}

/// A value that fits a table's cell, once an object of plain values is flattened.
fn cell_like(v: &Value) -> bool {
    scalar(v) || short_list(v) || v.as_object().is_some_and(|o| o.values().all(|x| scalar(x) || short_list(x)))
}

/// A row of plain lists, flattened: `[0.5, [7, 7, …]]` is `0.5, 7, 7, …`.
fn flat(row: &Value) -> Vec<Value> {
    match row {
        Value::Array(a) => a.iter().flat_map(|x| if x.is_array() { flat(x) } else { vec![x.clone()] }).collect(),
        other => vec![other.clone()],
    }
}

/// `row` with its flattened place `i` set.
fn set_flat(row: &mut Value, i: usize, to: Value) -> bool {
    fn walk(v: &mut Value, i: &mut usize, to: &Value) -> bool {
        match v {
            Value::Array(a) => a.iter_mut().any(|x| walk(x, i, to)),
            x => {
                if *i == 0 {
                    *x = to.clone();
                    return true;
                }
                *i -= 1;
                false
            }
        }
    }
    let mut i = i;
    walk(row, &mut i, &to)
}

/// The columns of a list of objects: a key a row has, or `key.inner` for an object of plain values inside it; a row's
/// `source` last.
fn object_columns(rows: &[Value]) -> Vec<String> {
    let mut cols: Vec<String> = Vec::new();
    for r in rows {
        for (k, v) in r.as_object().into_iter().flatten() {
            let names: Vec<String> = match v {
                Value::Object(o) => o.keys().map(|x| format!("{k}.{x}")).collect(),
                _ => vec![k.clone()],
            };
            for n in names {
                if !cols.contains(&n) {
                    cols.push(n);
                }
            }
        }
    }
    cols.sort_by_key(|c| c == "source");
    cols
}

fn get<'a>(row: &'a Value, col: &str) -> Option<&'a Value> {
    match col.split_once('.') {
        Some((a, b)) if row.get(col).is_none() => row.get(a).and_then(|x| x.get(b)),
        _ => row.get(col),
    }
}

fn put(row: &mut Value, col: &str, to: Value) {
    let Value::Object(o) = row else { return };
    match col.split_once('.') {
        Some((a, b)) if !o.contains_key(col) => {
            if let Some(Value::Object(inner)) = o.get_mut(a) {
                inner.insert(b.to_string(), to);
            }
        }
        _ => {
            o.insert(col.to_string(), to);
        }
    }
}

/// A list as a cell shows it, and as it is typed back: values between commas.
fn list_text(v: &Value) -> String {
    v.as_array().map_or_else(|| cell_text(v), |a| a.iter().map(cell_text).collect::<Vec<_>>().join(", "))
}

fn list_value(typed: &str) -> Value {
    Value::Array(typed.split(',').map(str::trim).filter(|x| !x.is_empty()).map(cell_value).collect())
}

/// How wide a column of text is: as its longest value, within reason.
fn width_of(texts: impl Iterator<Item = usize>) -> f32 {
    (texts.max().unwrap_or(4) as f32 * 7.5 + 16.0).clamp(56.0, 320.0)
}

/// A text field as wide as its text, wrapping past the panel's width.
fn text_field(ui: &mut Ui, s: &mut String) -> bool {
    let long = s.chars().count() > 60 || s.contains('\n');
    if long {
        ui.add(egui::TextEdit::multiline(s).desired_width(ui.available_width().max(200.0)).desired_rows(2)).changed()
    } else {
        let w = width_of(std::iter::once(s.chars().count())).max(160.0);
        ui.add(egui::TextEdit::singleline(s).desired_width(w)).changed()
    }
}

/// A plain value's field: a tick, a number, a text.
fn scalar_field(ui: &mut Ui, v: &mut Value) -> bool {
    match v {
        Value::Bool(b) => ui.checkbox(b, "").changed(),
        Value::String(s) => text_field(ui, s),
        other => {
            let mut text = cell_text(other);
            let w = width_of(std::iter::once(text.chars().count())).max(70.0);
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(w));
            if r.changed() {
                *other = cell_value(&text);
            }
            r.changed()
        }
    }
}

/// A rule's value, shown as what it is. `key` is its name, for the columns of its tables.
pub(crate) fn editor(ui: &mut Ui, id: &str, key: &str, v: &mut Value, hints: &Hints, pasted: &mut String) {
    match v {
        Value::Object(o) => object(ui, id, o, hints, pasted),
        Value::Array(rows) if rows.iter().all(scalar) => values(ui, id, rows),
        Value::Array(rows) if rows.iter().all(Value::is_array) => plain_table(ui, id, key, rows, pasted),
        Value::Array(rows) if rows.iter().all(|r| r.as_object().is_some_and(|o| o.values().all(cell_like))) => {
            object_table(ui, id, rows, hints, pasted)
        }
        Value::Array(rows) => cards(ui, id, key, rows, hints, pasted),
        other => {
            scalar_field(ui, other);
        }
    }
}

/// An object: its plain values as a form, then each of its tables under its name; its source as a line under it.
fn object(ui: &mut Ui, id: &str, o: &mut Map<String, Value>, hints: &Hints, pasted: &mut String) {
    // The explanations of a utility's columns are the hints of the tables beside them.
    let mut hints_here = Hints { columns: hints.columns.clone() };
    if let Some(Value::Object(c)) = o.get("columns")
        && c.values().all(Value::is_string)
    {
        hints_here.columns.extend(c.clone());
    }
    let plain: Vec<String> = o.iter().filter(|(k, v)| scalar(v) && *k != "source").map(|(k, _)| k.clone()).collect();
    if !plain.is_empty() {
        egui::Grid::new((id, "form")).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            for k in &plain {
                ui.label(RichText::new(human(k)).color(pal().muted));
                if let Some(v) = o.get_mut(k) {
                    scalar_field(ui, v);
                }
                ui.end_row();
            }
        });
    }
    let nested: Vec<String> = o.iter().filter(|(k, v)| !scalar(v) && k.as_str() != "source").map(|(k, _)| k.clone()).collect();
    for k in nested {
        let Some(v) = o.get_mut(&k) else { continue };
        let what = match &*v {
            Value::Array(a) => format!("{} · {}", human(&k), crate::lang::tf("{} rows", &[&a.len().to_string()])),
            _ => human(&k),
        };
        egui::CollapsingHeader::new(RichText::new(what).strong()).id_salt((id, k.as_str())).default_open(true).show(ui, |ui| {
            editor(ui, &format!("{id}.{k}"), &k, v, &hints_here, pasted);
        });
    }
    if let Some(Value::String(s)) = o.get_mut("source") {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(t("Source:")).color(pal().muted).small());
            text_field(ui, s);
        });
    }
}

/// A list of plain values: a field each, taken away by its ×, and one added.
fn values(ui: &mut Ui, id: &str, rows: &mut Vec<Value>) {
    let mut gone = None;
    ui.horizontal_wrapped(|ui| {
        for (i, v) in rows.iter_mut().enumerate() {
            ui.push_id((id, i), |ui| {
                scalar_field(ui, v);
                if ui.small_button("×").on_hover_text(t("Delete this row")).clicked() {
                    gone = Some(i);
                }
            });
            ui.add_space(6.0);
        }
        if ui.small_button("+").on_hover_text(t("Add Row")).clicked() {
            let blank = match rows.last() {
                Some(Value::Number(_)) => Value::from(0),
                Some(Value::Bool(_)) => Value::Bool(false),
                _ => Value::String(String::new()),
            };
            rows.push(blank);
        }
    });
    if let Some(i) = gone {
        rows.remove(i);
    }
}

/// The buttons under a table: a row added, the table copied for a spreadsheet, rows pasted from one.
fn table_tools(ui: &mut Ui, rows: &mut Vec<Value>, pasted: &mut String) {
    ui.horizontal(|ui| {
        if ui.small_button(t("Add Row")).clicked() {
            let blank = match rows.last() {
                Some(Value::Object(o)) => {
                    Value::Object(o.iter().map(|(k, v)| (k.clone(), if v.is_object() { blank_of(v) } else { Value::Null })).collect())
                }
                Some(Value::Array(_)) => rows.last().map(blank_of).unwrap_or(Value::Null),
                _ => Value::Null,
            };
            rows.push(blank);
        }
        if ui.small_button(t("Copy")).on_hover_text(t("Copy the table to paste it into a spreadsheet")).clicked() {
            ui.ctx().copy_text(as_tsv(&Value::Array(rows.clone())));
        }
    });
    // Rows pasted replace the table, where its rows are plain.
    if rows.iter().all(|r| r.as_array().is_some_and(|a| a.iter().all(scalar)) || r.as_object().is_some_and(|o| o.values().all(scalar))) {
        let mut take = false;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::multiline(pasted).desired_rows(1).desired_width(360.0).hint_text(t("Paste rows here")));
            take = ui.add_enabled(!pasted.trim().is_empty(), egui::Button::new(t("Use Pasted Rows"))).clicked();
        });
        if take && let Some(Value::Array(new)) = from_tsv(pasted, &Value::Array(rows.clone())) {
            *rows = new;
            pasted.clear();
        }
    }
}

/// The same shape, emptied.
fn blank_of(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), blank_of(x))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(blank_of).collect()),
        _ => Value::Null,
    }
}

/// A table of lists: a column for each place, a list inside a row spread over columns of its own.
fn plain_table(ui: &mut Ui, id: &str, key: &str, rows: &mut Vec<Value>, pasted: &mut String) {
    let width = rows.iter().map(|r| flat(r).len()).max().unwrap_or(0);
    let heads = places(key, width);
    let mut gone = None;
    ScrollArea::horizontal().id_salt((id, "scroll")).show(ui, |ui| {
        egui::Grid::new((id, "plain")).striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
            for h in &heads {
                ui.label(RichText::new(h).color(pal().muted).small());
            }
            ui.label("");
            ui.end_row();
            for (i, row) in rows.iter_mut().enumerate() {
                let cells = flat(row);
                for (j, c) in cells.iter().enumerate() {
                    let mut text = cell_text(c);
                    let w = if c.is_string() { width_of(std::iter::once(text.chars().count())) } else { 64.0 };
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(w)).changed() {
                        set_flat(row, j, cell_value(&text));
                    }
                }
                for _ in cells.len()..width {
                    ui.label("");
                }
                if ui.small_button("×").on_hover_text(t("Delete this row")).clicked() {
                    gone = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = gone {
        rows.remove(i);
    }
    table_tools(ui, rows, pasted);
}

/// A table of objects: a column for each key, an object inside a row spread over columns of its own, its source last; a
/// column the code explains says so when pointed at.
fn object_table(ui: &mut Ui, id: &str, rows: &mut Vec<Value>, hints: &Hints, pasted: &mut String) {
    let cols = object_columns(rows);
    let widths: Vec<f32> = cols
        .iter()
        .map(|c| {
            let texts = rows.iter().filter_map(|r| get(r, c)).map(|v| list_text(v).chars().count());
            let head = column_name(c.strip_prefix("columns.").unwrap_or(c)).chars().count();
            width_of(texts.chain(std::iter::once(head.min(14))))
        })
        .collect();
    let mut gone = None;
    ScrollArea::horizontal().id_salt((id, "scroll")).show(ui, |ui| {
        egui::Grid::new((id, "objects")).striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
            for c in &cols {
                let leaf = c.strip_prefix("columns.").unwrap_or(c);
                let head = ui.label(RichText::new(column_name(leaf)).color(pal().muted).small());
                if let Some(Value::String(meaning)) = hints.columns.get(leaf) {
                    head.on_hover_text(meaning);
                }
            }
            ui.label("");
            ui.end_row();
            for (i, row) in rows.iter_mut().enumerate() {
                for (c, w) in cols.iter().zip(&widths) {
                    let now = get(row, c).cloned().unwrap_or(Value::Null);
                    let mut text = list_text(&now);
                    let r = if c == "source" || *w >= 300.0 {
                        ui.add(egui::TextEdit::multiline(&mut text).desired_width(*w).desired_rows(1))
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut text).desired_width(*w))
                    };
                    if r.changed() {
                        put(row, c, if now.is_array() { list_value(&text) } else { cell_value(&text) });
                    }
                }
                if ui.small_button("×").on_hover_text(t("Delete this row")).clicked() {
                    gone = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = gone {
        rows.remove(i);
    }
    table_tools(ui, rows, pasted);
}

/// A table's column in words: `demand_kva.over` is "Demand (kVA) over".
fn column_name(col: &str) -> String {
    match col.split_once('.') {
        Some((a, b)) => format!("{} {}", human(a), human(b).to_lowercase()),
        None => human(col),
    }
}

/// What a row is called on its card: its name, or what tells it from the others.
fn title_of(row: &Value, i: usize) -> String {
    let said = |k: &str| row.get(k).map(list_text).filter(|s| !s.is_empty());
    let mut parts: Vec<String> = ["name", "kind", "class", "system", "use", "network", "annex", "id", "utility"]
        .iter()
        .filter_map(|k| said(k).map(|v| if *k == "annex" { format!("{} {v}", human(k)) } else { v }))
        .collect();
    parts.dedup();
    if parts.is_empty() { format!("#{}", i + 1) } else { parts.join(" · ") }
}

/// Rows that hold tables of their own: a card each, shown as an object inside.
fn cards(ui: &mut Ui, id: &str, key: &str, rows: &mut Vec<Value>, hints: &Hints, pasted: &mut String) {
    let mut gone = None;
    for (i, row) in rows.iter_mut().enumerate() {
        let title = title_of(row, i);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&title).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text(t("Delete this row")).clicked() {
                        gone = Some(i);
                    }
                });
            });
            ui.push_id((id, i), |ui| editor(ui, &format!("{id}[{i}]"), key, row, hints, pasted));
        });
    }
    if let Some(i) = gone {
        rows.remove(i);
    }
    if ui.small_button(t("Add Row")).clicked()
        && let Some(last) = rows.last()
    {
        rows.push(blank_of(last));
    }
}
