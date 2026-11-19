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

/// A value that fits a table's cell, once an object of plain values (and of such objects) is flattened.
fn cell_like(v: &Value) -> bool {
    scalar(v) || short_list(v) || v.as_object().is_some_and(|o| o.values().all(cell_like))
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

/// The columns of a list of objects: a key a row has, or `key.inner` (`key.inner.deeper`, …) for an object inside it; a
/// row's `name` first and its `source` last.
fn object_columns(rows: &[Value]) -> Vec<String> {
    fn names(prefix: &str, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    names(&format!("{prefix}.{k}"), x, out);
                }
            }
            _ => {
                if !out.iter().any(|n| n == prefix) {
                    out.push(prefix.to_string());
                }
            }
        }
    }
    let mut cols: Vec<String> = Vec::new();
    for r in rows {
        for (k, v) in r.as_object().into_iter().flatten() {
            names(k, v, &mut cols);
        }
    }
    cols.sort_by_key(|c| (c != "name", c == "source"));
    cols
}

fn get<'a>(row: &'a Value, col: &str) -> Option<&'a Value> {
    match col.split_once('.') {
        Some((a, b)) if row.get(col).is_none() => row.get(a).and_then(|x| get(x, b)),
        _ => row.get(col),
    }
}

/// Whether a row can hold a value in a column: a column inside an object the row doesn't have yet can (the object is made
/// as it is typed), one inside a value that isn't an object can't (it is not turned into one).
fn reachable(row: &Value, col: &str) -> bool {
    let Value::Object(o) = row else { return false };
    match col.split_once('.') {
        Some((a, b)) if !o.contains_key(col) => o.get(a).is_none_or(|inner| reachable(inner, b)),
        _ => true,
    }
}

/// The columns that are a key of their own with a dot in it (`"a.b"`), not a path into an object: a row without it gets that
/// key, never an object made from the column's name.
fn literal_columns(rows: &[Value]) -> std::collections::BTreeMap<String, Vec<String>> {
    type Out = std::collections::BTreeMap<String, Vec<String>>;
    fn walk(path: &mut Vec<String>, v: &Value, out: &mut Out) {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    path.push(k.clone());
                    walk(path, x, out);
                    path.pop();
                }
            }
            _ if path.iter().any(|k| k.contains('.')) => {
                out.entry(path.join(".")).or_insert_with(|| path.clone());
            }
            _ => {}
        }
    }
    let mut out = Out::new();
    for r in rows {
        walk(&mut Vec::new(), r, &mut out);
    }
    out
}

/// Whether a row can take a value at a literal column's keys: each object on the way is there or can be made.
fn reachable_at(row: &Value, keys: &[String]) -> bool {
    match (row, keys.split_first()) {
        (Value::Object(o), Some((k, rest))) => rest.is_empty() || o.get(k).is_none_or(|x| reachable_at(x, rest)),
        _ => false,
    }
}

/// What a cell typed in writes: `put`, but a cell of a literal column the row doesn't have takes that key whole.
fn write(row: &mut Value, col: &str, to: Value, literal: Option<&[String]>) {
    if let Some(keys) = literal
        && get(row, col).is_none()
    {
        if to.is_null() || !reachable_at(row, keys) {
            return;
        }
        let mut at = row;
        for k in keys {
            let Value::Object(o) = at else { return };
            at = o.entry(k.clone()).or_insert_with(|| Value::Object(Map::new()));
        }
        *at = to;
        return;
    }
    put(row, col, to);
}

/// Writes a cell: where the row has no object for it yet, one is made (another row has that column).
fn put(row: &mut Value, col: &str, to: Value) {
    if !reachable(row, col) {
        return;
    }
    let Value::Object(o) = row else { return };
    match col.split_once('.') {
        Some((a, b)) if !o.contains_key(col) => put(o.entry(a).or_insert_with(|| Value::Object(Map::new())), b, to),
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
    let literal = literal_columns(rows);
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
                let explained = leaf.split('.').next().unwrap_or(leaf);
                if let Some(Value::String(meaning)) = hints.columns.get(explained) {
                    head.on_hover_text(meaning);
                }
            }
            ui.label("");
            ui.end_row();
            for (i, row) in rows.iter_mut().enumerate() {
                for (c, w) in cols.iter().zip(&widths) {
                    let now = get(row, c).cloned().unwrap_or(Value::Null);
                    // An empty cell is empty to type in: its dash is a hint, not text typed after.
                    let mut text = if now.is_null() { String::new() } else { list_text(&now) };
                    // A column inside a value this row gives whole is not one it can be typed in.
                    let lit = literal.get(c).map(Vec::as_slice);
                    let open = lit.map_or_else(|| reachable(row, c), |keys| get(row, c).is_some() || reachable_at(row, keys));
                    let r = if c == "source" || *w >= 300.0 {
                        ui.add_enabled(open, egui::TextEdit::multiline(&mut text).desired_width(*w).desired_rows(1).hint_text("—"))
                    } else {
                        ui.add_enabled(open, egui::TextEdit::singleline(&mut text).desired_width(*w).hint_text("—"))
                    };
                    let r = r.on_disabled_hover_text(t("This row gives one value for these columns"));
                    if r.changed() {
                        write(row, c, if now.is_array() { list_value(&text) } else { cell_value(&text) }, lit);
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

/// A table's column in words: `demand_kva.over` is "Demand (kVA) over", `connection_mm2.copper` "Connection (mm²) copper".
fn column_name(col: &str) -> String {
    match col.split_once('.') {
        Some((a, b)) => format!("{} {}", human(a), column_name(b).to_lowercase()),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every list of objects in every electrical code BIMCraft has, by where it is.
    fn tables() -> Vec<(String, Vec<Value>)> {
        fn walk(at: &str, v: &Value, out: &mut Vec<(String, Vec<Value>)>) {
            match v {
                Value::Array(a) => {
                    if !a.is_empty() && a.iter().all(Value::is_object) {
                        out.push((at.to_string(), a.clone()));
                    }
                    a.iter().enumerate().for_each(|(i, x)| walk(&format!("{at}[{i}]"), x, out));
                }
                Value::Object(o) => o.iter().for_each(|(k, x)| walk(&format!("{at}.{k}"), x, out)),
                _ => {}
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/data/codes/electrical");
        let mut out = Vec::new();
        for f in std::fs::read_dir(dir).unwrap() {
            let path = f.unwrap().path();
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            walk(&path.file_stem().unwrap().to_string_lossy(), &v, &mut out);
        }
        out
    }

    #[test]
    fn a_cell_typed_in_is_kept_or_cannot_be_typed_in() {
        // Each cell of each table's columns: typed in, the row holds what was typed, there and nowhere else; or the cell is
        // not open, because the row holds a whole value where the column looks inside one (G7).
        let (mut kept, mut closed) = (0, 0);
        for (at, rows) in tables() {
            let cols = object_columns(&rows);
            for (i, row) in rows.iter().enumerate() {
                for c in &cols {
                    let mut typed = row.clone();
                    if reachable(row, c) {
                        put(&mut typed, c, serde_json::json!(25));
                        assert_eq!(get(&typed, c), Some(&serde_json::json!(25)), "{at} row {i} column {c}: typed and lost");
                        for other in cols.iter().filter(|o| *o != c && !o.starts_with(&format!("{c}.")) && !c.starts_with(&format!("{o}.")))
                        {
                            assert_eq!(get(&typed, other), get(row, other), "{at} row {i}: typing in {c} changed {other}");
                        }
                        kept += 1;
                    } else {
                        put(&mut typed, c, serde_json::json!(25));
                        assert_eq!(&typed, row, "{at} row {i} column {c}: closed, and nothing changes");
                        closed += 1;
                    }
                }
            }
        }
        assert!(kept > 1000 && closed > 0, "both kinds of cell asked ({kept} kept, {closed} closed)");
    }

    #[test]
    fn the_cells_the_review_found_keep_what_is_typed() {
        // Rows shaped as Energisa's T4, Hidropan's C7 and FECOERGS's C5, where the review typed.
        let row = serde_json::json!({"category": "T4", "entry_pvc_mm2": {"neutral": 16}});
        let mut typed = row.clone();
        put(&mut typed, "entry_pvc_mm2.phase", serde_json::json!(25));
        assert_eq!(get(&typed, "entry_pvc_mm2.phase"), Some(&serde_json::json!(25)));
        let row = serde_json::json!({"category": "C7"});
        let mut typed = row.clone();
        put(&mut typed, "motor_cv.fff", serde_json::json!(25));
        assert_eq!(get(&typed, "motor_cv.fff"), Some(&serde_json::json!(25)));
        let row = serde_json::json!({"category": "C5", "resistive_kw": "limited by the breaker"});
        assert!(!reachable(&row, "resistive_kw.ff"), "a whole value is not turned into an object");
    }

    // The review's cases (G7), as it wrote them.
    use bimcraft_core::codes::Discipline;
    use bimcraft_core::engine::Session;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    fn g7_table(rows: Value) -> Harness<'static, (Value, String)> {
        Harness::builder()
            .with_size([1500.0, 550.0])
            .with_step_dt(0.25)
            .build_ui_state(|ui, (rows, pasted)| editor(ui, "g7", "rows", rows, &Hints::default(), pasted), (rows, String::new()))
    }

    fn g7_cell<'a, S>(h: &'a Harness<'_, S>, row: &str, header: &str) -> egui_kittest::Node<'a> {
        let row_y = h
            .get_all_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some(row))
            .next()
            .unwrap_or_else(|| panic!("missing row {row}"))
            .rect()
            .center()
            .y;
        let header_x = h
            .get_all_by_label(header)
            .filter(|n| n.rect().center().y < row_y)
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap_or_else(|| panic!("missing header {header}"))
            .rect()
            .left();
        h.get_all_by(|n| matches!(n.role(), Role::TextInput | Role::MultilineTextInput))
            .filter(|n| (n.rect().center().y - row_y).abs() < 8.0)
            .min_by(|a, b| (a.rect().left() - header_x).abs().total_cmp(&(b.rect().left() - header_x).abs()))
            .filter(|n| (n.rect().left() - header_x).abs() < 12.0)
            .unwrap_or_else(|| panic!("no text input for {row} / {header} at {header_x}, {row_y}"))
    }

    fn g7_type<S>(h: &mut Harness<'_, S>, row: &str, header: &str, value: &str) {
        g7_cell(h, row, header).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        if value.is_empty() {
            h.key_press(egui::Key::Backspace);
        } else {
            h.event(egui::Event::Text(value.into()));
        }
        h.run();
    }

    #[test]
    fn g7_three_deep_open_and_closed_cells_focus_and_tooltip() {
        let mut h = g7_table(json!([
            {"name":"example","columns":{"a":{"b":12}},"source":"keep"},
            {"name":"missing"},
            {"name":"empty","columns":{}},
            {"name":"null","columns":{"a":null}},
            {"name":"list","columns":{"a":[1,2]}},
            {"name":"number","columns":{"a":7}},
            {"name":"string","columns":{"a":"intact"}}
        ]));
        let original = h.state().0.clone();
        let header = "A b";
        assert_eq!(object_columns(original.as_array().unwrap()), ["name", "columns.a.b", "columns.a", "source"]);
        for (row, i) in [("missing", 1), ("empty", 2)] {
            let id = g7_cell(&h, row, header).accesskit_node().locate().0;
            g7_type(&mut h, row, header, "25");
            let cell = g7_cell(&h, row, header);
            assert_eq!(cell.accesskit_node().locate().0, id);
            assert!(cell.is_focused());
            assert_eq!(h.state().0[i], json!({"name":row,"columns":{"a":{"b":25}}}));
        }
        for row in ["null", "list", "number", "string"] {
            let cell = g7_cell(&h, row, header);
            cell.focus();
            h.run();
            assert!(!g7_cell(&h, row, header).is_focused(), "{row}");
        }
        assert_eq!(h.state().0.as_array().unwrap()[3..], original.as_array().unwrap()[3..]);
        g7_cell(&h, "string", header).hover();
        h.run_steps(6);
        assert!(h.query_by_label("This row gives one value for these columns").is_some());
    }

    #[test]
    fn g7_dot_keys_and_object_paths_are_distinct() {
        let mut rows = json!([{"a.b":7,"a":{"b":11}},{"a.b":13},{"a":{"b":17}}]);
        let rows = rows.as_array_mut().unwrap();
        assert_eq!(object_columns(rows), ["a.b"]);
        put(&mut rows[0], "a.b", json!(23));
        put(&mut rows[1], "a.b", json!(25));
        put(&mut rows[2], "a.b", json!(31));
        assert_eq!(rows.as_slice(), [json!({"a.b":23,"a":{"b":11}}), json!({"a.b":25}), json!({"a":{"b":31}})]);
        // A literal column a row lacks takes that key whole, never an object made from its name.
        let literal_rows = json!([{"a.b":7},{"name":"missing"}]);
        let lit = literal_columns(literal_rows.as_array().unwrap());
        let mut unfilled = literal_rows[1].clone();
        write(&mut unfilled, "a.b", json!(25), lit.get("a.b").map(Vec::as_slice));
        assert_eq!(unfilled, json!({"name":"missing","a.b":25}));
    }

    #[test]
    fn g7_add_delete_and_lists() {
        let mut h = g7_table(json!([{"name":"seed","a":{"b":12},"list":["old"],"source":"end"},{"name":"last","list":["new"]}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        assert_eq!(h.state().0[2], json!({"name":null,"list":null}));
        let name = h
            .get_all_by(|n| n.role() == Role::TextInput && matches!(n.value().as_deref(), None | Some("")))
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()).then_with(|| b.rect().left().total_cmp(&a.rect().left())))
            .unwrap();
        name.focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("added".into()));
        h.run();
        g7_type(&mut h, "added", "A b", "25");
        g7_type(&mut h, "seed", "List", "a, b");
        assert_eq!(h.state().0[0]["list"], json!(["a", "b"]));
        let staged = h.state().0[2].clone();
        h.get_all_by_label("×").last().unwrap().click_accesskit();
        h.run();
        assert_eq!(h.state().0.as_array().unwrap().len(), 2);
        assert_eq!(staged, json!({"name":"added","a":{"b":25},"list":null}));
    }

    #[test]
    fn g7_paste_flat_and_with_a_missing_parent() {
        let mut h = g7_table(json!([{"name":"first","a":1},{"name":"last"}]));
        h.state_mut().1 = "name\ta\nfirst\t11\nlast\t12".into();
        h.run();
        h.get_by_label("Use Pasted Rows").click_accesskit();
        h.run();
        assert_eq!(h.state().0, json!([{"name":"first","a":11},{"name":"last","a":12}]));
        let nested = json!([{"name":"first","a":{"b":11}},{"name":"last"}]);
        let h = g7_table(nested.clone());
        assert!(h.query_by_label("Use Pasted Rows").is_none());
        let tsv = as_tsv(&nested);
        assert_eq!(tsv, "a\tname\n{\"b\":11}\tfirst\n\tlast\n");
        assert_eq!(from_tsv(&tsv, &nested).unwrap(), json!([{"name":"first","a":{"b":11}},{"name":"last","a":null}]));
        assert_eq!(from_tsv("name\ta.b\nlast\t25", &nested).unwrap(), json!([{"name":"last","a.b":25}]));
    }

    fn g7_dialog(utility: &str) -> Harness<'static, crate::BimApp> {
        let mut s = Session::new();
        s.execute("building_code", &json!({"discipline":"electrical","set":"nbr-5410","amendments":[utility]})).unwrap();
        let mut app = crate::BimApp::new(s);
        let mut d = crate::codes::CodesDialog::new(&app.session.model);
        d.tab = 3;
        d.tab_mut(Discipline::Electrical).section = "entrance".into();
        d.table = Some(("entrance".into(), "tables".into()));
        app.codes_dialog = Some(d);
        Harness::builder().with_size([1800.0, 1200.0]).with_step_dt(0.25).build_ui_state(|ui, app| app.codes_window(ui.ctx()), app)
    }

    fn g7_ok(h: &mut Harness<'_, crate::BimApp>) {
        h.get_by_label("OK").click_accesskit();
        h.run();
    }

    #[test]
    fn g7_dialog_energisa_created_column_reaches_session() {
        let mut h = g7_dialog("energisa");
        let before = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = before["tables"][0]["rows"].as_array().unwrap().iter().position(|r| r["name"] == "T4").unwrap();
        g7_type(&mut h, "T4", "Entry pvc (mm²) phase", "25");
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let mut expected = json!(before);
        expected["tables"][0]["rows"][row]["columns"]["entry_pvc_mm2"] = json!({"phase":25});
        assert!(Value::Object(after) == expected, "the edit should be the only difference in the resolved section");
    }

    #[test]
    fn g7_tooltip_translation() {
        // In Portuguese, the language of the utilities' codes it closes cells of; the others fall back to English.
        let tip = "This row gives one value for these columns";
        let pt = crate::lang::Lang::ALL.into_iter().find(|l| l.code() == "pt").unwrap();
        assert!(crate::lang::translated_in(pt, tip).is_some(), "missing translation for pt");
    }

    fn g7_paths(v: &Value, prefix: &mut Vec<String>, paths: &mut std::collections::BTreeSet<Vec<String>>) {
        if let Some(object) = v.as_object() {
            for (k, value) in object {
                prefix.push(k.clone());
                g7_paths(value, prefix, paths);
                prefix.pop();
            }
        } else {
            paths.insert(prefix.clone());
        }
    }

    fn g7_expected(row: &Value, path: &[String], new: Value) -> Option<Value> {
        let mut row = row.clone();
        let mut current = &mut row;
        for k in &path[..path.len() - 1] {
            current = current.as_object_mut()?.entry(k).or_insert_with(|| json!({}));
        }
        current.as_object_mut()?.insert(path.last().unwrap().clone(), new);
        Some(row)
    }

    fn g7_tables<'a>(v: &'a Value, tables: &mut Vec<&'a [Value]>) {
        match v {
            Value::Object(o) => {
                for v in o.values() {
                    g7_tables(v, tables);
                }
            }
            Value::Array(r) if !r.is_empty() && r.iter().all(|v| v.as_object().is_some_and(|o| o.values().all(cell_like))) => {
                tables.push(r);
            }
            Value::Array(r) => {
                for v in r {
                    g7_tables(v, tables);
                }
            }
            _ => {}
        }
    }

    fn g7_electrical_files() -> Vec<(String, Value)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/data/codes/electrical");
        let mut files: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.file_stem().unwrap().to_str().unwrap().to_owned(), serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    #[test]
    fn g7_every_electrical_object_table_whole_row_oracle() {
        let mut missing = 0;
        for (name, code) in g7_electrical_files() {
            let mut tables = Vec::new();
            g7_tables(&code["sections"], &mut tables);
            let mut mismatches = 0;
            let mut count = 0;
            let mut blocked = 0;
            for rows in &tables {
                let mut paths = std::collections::BTreeSet::new();
                for row in *rows {
                    g7_paths(row, &mut Vec::new(), &mut paths);
                }
                let cols = object_columns(rows);
                assert_eq!(cols.len(), paths.len(), "{name}");
                for row in *rows {
                    for path in &paths {
                        let col = path.join(".");
                        assert!(cols.contains(&col));
                        let mut observed = row.clone();
                        let input = if get(row, &col).is_some_and(Value::is_array) { list_value("a, b") } else { cell_value("999") };
                        let expected = g7_expected(row, path, input.clone());
                        put(&mut observed, &col, input);
                        count += 1;
                        match expected {
                            Some(expected) => {
                                mismatches += usize::from(observed != expected);
                            }
                            None => {
                                blocked += 1;
                                assert_eq!(&observed, row);
                            }
                        }
                    }
                }
            }
            assert!(blocked <= count, "{name}: cells asked");
            missing += mismatches;
        }
        assert_eq!(missing, 0);
    }

    #[test]
    fn g7_clear_optional_and_required_nested_fields_for_every_utility() {
        for (utility, doc) in g7_electrical_files() {
            if doc["amends"] != "nbr-5410" || !doc["sections"]["entrance"].is_object() {
                continue;
            }
            let mut s = Session::new();
            s.execute("building_code", &json!({"discipline":"electrical","set":"nbr-5410","amendments":[utility]})).unwrap();
            let origin = s.model.resolve_code(Discipline::Electrical, "entrance").value;
            let initial = crate::codes::Tab::new(&s.model, Discipline::Electrical);
            let mut cases = [0; 5];
            for (table, t) in origin["tables"].as_array().unwrap().iter().enumerate() {
                let rows = t["rows"].as_array().unwrap();
                for (i, row) in rows.iter().enumerate() {
                    for col in object_columns(rows) {
                        let Some((outer, leaf)) = col.rsplit_once('.') else {
                            continue;
                        };
                        let path: Vec<_> = col.split('.').map(str::to_owned).collect();
                        if get(row, &col).is_some() || g7_expected(row, &path, json!(25)).is_none() {
                            continue;
                        }
                        let mut typed = row.clone();
                        put(&mut typed, &col, cell_value("25"));
                        let populated = get(&typed, &col).cloned();
                        put(&mut typed, &col, cell_value(""));
                        if populated.is_none() {
                            cases[0] += 1;
                            continue;
                        }
                        assert_eq!(get(&typed, &col), Some(&Value::Null));
                        assert!(get(&typed, outer).unwrap().as_object().unwrap().contains_key(leaf));
                        let mut tab = initial.clone();
                        tab.rules.get_mut("entrance").unwrap().get_mut("tables").unwrap()[table]["rows"][i] = typed;
                        let mut trial = Session::new();
                        trial.model = s.model.clone();
                        match trial.execute("building_code", &tab.params(&s.model)) {
                            Ok(_) => {
                                let after = trial.model.resolve_code(Discipline::Electrical, "entrance").value;
                                assert_eq!(get(&after["tables"][table]["rows"][i], &col), Some(&Value::Null));
                                cases[if col.starts_with("columns.") { 1 } else { 2 }] += 1;
                            }
                            Err(_) => {
                                assert_eq!(trial.model.resolve_code(Discipline::Electrical, "entrance").value, origin);
                                let n = if get(row, outer).is_none() { 3 } else { 4 };
                                cases[n] += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn g7_typing_after_the_empty_cells_dash() {
        let mut h = g7_table(json!([{"name":"nested","a":{"b":12}},{"name":"missing"}]));
        let cell = g7_cell(&h, "missing", "A b");
        let rect = cell.rect();
        let pos = egui::pos2(rect.left() + 22.0, rect.center().y);
        h.hover_at(pos);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run();
        h.key_press(egui::Key::End);
        h.event(egui::Event::Text("25".into()));
        h.run();
        assert_eq!(h.state().0[1]["a"]["b"], json!(25));
    }

    #[test]
    fn g7_dialog_typed_after_an_empty_cell_is_the_number_typed() {
        let mut h = g7_dialog("energisa");
        let header = "Entry pvc (mm²) phase";
        g7_cell(&h, "T4", header).focus();
        h.run();
        h.key_press(egui::Key::End);
        h.event(egui::Event::Text("25".into()));
        h.run();
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = after["tables"][0]["rows"].as_array().unwrap().iter().find(|r| r["name"] == "T4").unwrap();
        let phase = &row["columns"]["entry_pvc_mm2"]["phase"];
        assert_eq!(phase, &json!(25));
    }

    #[test]
    fn g7_closed_cells_keep_ids_and_do_not_take_focus() {
        let mut h = g7_table(json!([
            {"name":"object", "a":{"b":12}},
            {"name":"null", "a":null},
            {"name":"list", "a":[1,2]},
            {"name":"number", "a":8},
            {"name":"string", "a":"keep"}
        ]));
        let mut closed = 0;
        let original = h.state().0.clone();
        for name in ["null", "list", "number", "string"] {
            let cell = g7_cell(&h, name, "A b");
            let id = cell.accesskit_node().locate().0;
            cell.focus();
            h.run();
            let cell = g7_cell(&h, name, "A b");
            assert_eq!(cell.accesskit_node().locate().0, id);
            if !cell.is_focused() {
                closed += 1;
            }
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.event(egui::Event::Text("25".into()));
            h.run();
        }
        assert_eq!(h.state().0, original);
        g7_cell(&h, "string", "A b").hover();
        h.run_steps(6);
        let tip = h.query_by_label("This row gives one value for these columns").is_some();
        assert_eq!((closed, tip), (4, true));
    }

    #[test]
    fn g7_dialog_a_dotted_key_is_written_as_itself_in_another_row() {
        let mut h = g7_dialog("energisa");
        let app = h.state_mut();
        let d = app.codes_dialog.as_mut().unwrap();
        let mut tab = d.tab_mut(Discipline::Electrical).clone();
        tab.rules.get_mut("entrance").unwrap().get_mut("tables").unwrap()[0]["rows"][0]["columns"]["custom.flag"] = json!(11);
        app.session.execute("building_code", &tab.params(&app.session.model)).unwrap();
        let mut d = crate::codes::CodesDialog::new(&app.session.model);
        d.tab = 3;
        d.tab_mut(Discipline::Electrical).section = "entrance".into();
        d.table = Some(("entrance".into(), "tables".into()));
        app.codes_dialog = Some(d);
        h.run();
        g7_type(&mut h, "T4", "Custom flag", "25");
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = after["tables"][0]["rows"].as_array().unwrap().iter().find(|r| r["name"] == "T4").unwrap();
        assert_eq!((row["columns"].get("custom.flag"), row["columns"].get("custom")), (Some(&json!(25)), None));
    }

    // The confirmation's cases (G7), as it wrote them.
    // Insert inside codes::tree::tests, after the existing g7 helpers.
    fn g7r2_seed_entrance(h: &mut Harness<'_, crate::BimApp>, seed: impl FnOnce(&mut Value)) {
        let app = h.state_mut();
        let mut tab = app.codes_dialog.as_mut().unwrap().tab_mut(Discipline::Electrical).clone();
        seed(&mut tab.rules.get_mut("entrance").unwrap().get_mut("tables").unwrap()[0]["rows"]);
        app.session.execute("building_code", &tab.params(&app.session.model)).unwrap();
        let mut d = crate::codes::CodesDialog::new(&app.session.model);
        d.tab = 3;
        d.tab_mut(Discipline::Electrical).section = "entrance".into();
        d.table = Some(("entrance".into(), "tables".into()));
        app.codes_dialog = Some(d);
        h.run();
    }

    fn g7r2_entrance(h: &Harness<'_, crate::BimApp>) -> Value {
        Value::Object(h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value)
    }

    fn g7r2_t4(section: &Value) -> usize {
        section["tables"][0]["rows"].as_array().unwrap().iter().position(|r| r["name"] == "T4").unwrap()
    }

    #[test]
    fn g7r2_codes_existing_null_accepts_number_after_hint() {
        let mut h = g7_dialog("energisa");
        let row = g7r2_t4(&g7r2_entrance(&h));
        g7r2_seed_entrance(&mut h, |rows| rows[row]["columns"]["entry_pvc_mm2"] = json!({"phase":null}));
        let mut expected = g7r2_entrance(&h);
        assert_eq!(expected["tables"][0]["rows"][row]["columns"]["entry_pvc_mm2"], json!({"phase":null}));
        g7_cell(&h, "T4", "Entry pvc (mm²) phase").focus();
        h.run();
        h.key_press(egui::Key::End);
        h.event(egui::Event::Text("25".into()));
        h.run();
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        expected["tables"][0]["rows"][row]["columns"]["entry_pvc_mm2"]["phase"] = json!(25);
        assert!(g7r2_entrance(&h) == expected);
    }

    #[test]
    fn g7r2_codes_deep_dotted_segments() {
        let mut h = g7_table(json!([
            {"name":"seed","top.dot":11,"columns":{"outer":{"middle.dot":{"leaf.dot":12}}}},
            {"name":"target","columns":{"keep":8}}
        ]));
        g7_type(&mut h, "target", "Top dot", "23");
        g7_type(&mut h, "target", "Outer middle dot leaf dot", "25");
        assert_eq!(
            h.state().0[1],
            json!({
                "name":"target","top.dot":23,"columns":{"keep":8,"outer":{"middle.dot":{"leaf.dot":25}}}
            })
        );
    }

    #[test]
    fn g7r2_codes_clear_must_not_move_focus_to_another_rule() {
        let mut h = g7_table(json!([{"name":"row","a":{"b":12},"other":17,"source":"keep"}]));
        let other_id = g7_cell(&h, "row", "Other").accesskit_node().locate().0;
        g7_type(&mut h, "row", "A b", "");
        h.run_steps(3);
        let other = g7_cell(&h, "row", "Other");
        let identity = (other.accesskit_node().locate().0, other.is_focused());
        assert_eq!(identity, (other_id, false), "the other cell keeps its id and takes no focus");
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("25".into()));
        h.run();
        // The cell cleared keeps its place and its focus: what is typed next goes there, and the other rule stays.
        assert_eq!(h.state().0[0], json!({"name":"row","a":{"b":25},"other":17,"source":"keep"}));
    }

    #[test]
    fn g7r2_codes_closed_tooltip_is_rendered_in_portuguese() {
        let previous = crate::lang::get();
        crate::lang::set(crate::lang::Lang::from_code("pt").unwrap());
        let mut h = g7_table(json!([{"name":"open","a":{"b":12}},{"name":"closed","a":8}]));
        g7_cell(&h, "closed", "A b").hover();
        h.run_steps(6);
        let displayed = h.query_by_label("Esta linha dá um valor só para estas colunas").is_some();
        crate::lang::set(previous);
        assert!(displayed);
    }

    #[test]
    fn g7r2_codes_cleared_existing_range_bound_must_not_silently_default() {
        let mut h = g7_dialog("energisa");
        let before = g7r2_entrance(&h);
        let row = g7r2_t4(&before);
        let path = |section: Value| section["tables"][0]["rows"][row]["demand_kva"].clone();
        assert_eq!(path(before.clone()), json!({"over":26.6,"up_to":38.1}));
        g7_type(&mut h, "T4", "Demand (kVA) over", "");
        g7_ok(&mut h);
        let after = g7r2_entrance(&h);
        let after_range = path(after);
        let effective: bimcraft_core::entrance::Range = serde_json::from_value(after_range.clone()).unwrap();
        assert_eq!(after_range, path(before), "clearing an explicit bound must not silently commit an implicit {}", effective.over);
    }

    #[test]
    fn g7r2_codes_dotted_edit_round_trips_xml_and_bimc() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| rows[0]["columns"]["custom.flag"] = json!(11));
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        expected["tables"][0]["rows"][row]["columns"]["custom.flag"] = json!(25);
        g7_type(&mut h, "T4", "Custom flag", "25");
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        assert!(g7r2_entrance(&h) == expected);

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("dotted.bimc");
        let xml = dir.path().join("dotted.xml");
        let session = &mut h.state_mut().session;
        session.execute("save", &json!({"path":project})).unwrap();
        session.execute("building_code_export", &json!({"path":xml,"discipline":"electrical","name":"G7 Review"})).unwrap();

        let e = Discipline::Electrical;
        let mut opened = Session::new();
        opened.execute("open", &json!({"path":project})).unwrap();
        assert!(Value::Object(opened.model.resolve_code(e, "entrance").value) == expected, ".bimc");
        let mut imported = Session::new();
        imported.execute("building_code_import", &json!({"path":xml,"set":true})).unwrap();
        let actual = Value::Object(imported.model.resolve_code(e, "entrance").value);
        assert_eq!(actual["tables"][0]["rows"][row], expected["tables"][0]["rows"][row], "XML T4");
        assert!(actual == expected, "XML full entrance section");
    }

    #[test]
    fn g7r2_codes_clearing_required_existing_values_is_rejected() {
        for header in ["Phases", "Demand (kVA) up to"] {
            let mut h = g7_dialog("energisa");
            let before = g7r2_entrance(&h);
            g7_type(&mut h, "T4", header, "");
            g7_ok(&mut h);
            let dialog = h.state().codes_dialog.as_ref().expect(header);
            assert!(!dialog.error.is_empty(), "{header}");
            assert!(g7r2_entrance(&h) == before, "{header}");
        }
    }

    #[test]
    fn g7r2_codes_retyping_after_clear_keeps_the_edit_in_the_last_cell() {
        let mut h = g7_dialog("energisa");
        let row = g7r2_t4(&g7r2_entrance(&h));
        g7r2_seed_entrance(&mut h, |rows| rows[row]["columns"]["review_only"] = json!({"flag":17}));
        let mut expected = g7r2_entrance(&h);
        expected["tables"][0]["rows"][row]["columns"]["review_only"]["flag"] = json!(25);
        g7_type(&mut h, "T4", "Review only flag", "");
        h.event(egui::Event::Text("25".into()));
        h.run();
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        let actual = g7r2_entrance(&h);
        assert_eq!(actual["tables"][0]["rows"][row], expected["tables"][0]["rows"][row]);
    }
}
