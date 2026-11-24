//! A rule whose value is more than a number, shown as what it is: an object as a small form, a list of plain values as a row of
//! fields, a list of rows as a table with named columns (an object inside a row flattened into columns of its own), and a list of
//! rows that hold lists of their own as cards, one a row, each shown the same way inside. Long text wraps at the panel's width.
//! The words of a code (`source`, `note`) are kept apart from its numbers: a row's source is its last column, an object's is a
//! line under it.

use std::collections::BTreeMap;

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

/// A list of plain values, shown in one cell: however long, so a row doesn't change its editor while one is typed.
fn short_list(v: &Value) -> bool {
    v.as_array().is_some_and(|a| a.iter().all(scalar))
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
/// A table's columns, each by the keys that reach it: a key with a dot in it (`"a.b"`) and a path into an object (`a` then
/// `b`) are two columns, never one.
fn object_columns(rows: &[Value]) -> Vec<Vec<String>> {
    fn names(path: &mut Vec<String>, v: &Value, out: &mut Vec<Vec<String>>) {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    path.push(k.clone());
                    names(path, x, out);
                    path.pop();
                }
            }
            _ => {
                if !out.iter().any(|n| n == path) {
                    out.push(path.clone());
                }
            }
        }
    }
    let mut cols: Vec<Vec<String>> = Vec::new();
    for r in rows {
        for (k, v) in r.as_object().into_iter().flatten() {
            names(&mut vec![k.clone()], v, &mut cols);
        }
    }
    cols.sort_by_key(|c| (c.as_slice() != ["name"], c.as_slice() == ["source"]));
    cols
}

/// A row's value in a column, by the column's keys.
fn get_at<'a>(row: &'a Value, keys: &[String]) -> Option<&'a Value> {
    keys.iter().try_fold(row, |at, k| at.get(k))
}

/// Writes a cell by its column's keys: where the row has no object for it yet, one is made (another row has that column);
/// inside a value the row gives whole, nothing is written.
fn put_at(row: &mut Value, keys: &[String], to: Value) {
    // A cell emptied where the row lacks a key with a dot in it writes nothing: it is not made just to hold nothing.
    if !reachable_at(row, keys) || (to.is_null() && keys.iter().any(|k| k.contains('.')) && get_at(row, keys).is_none()) {
        return;
    }
    let Some((last, path)) = keys.split_last() else { return };
    let mut at = row;
    for k in path {
        let Value::Object(o) = at else { return };
        at = o.entry(k.clone()).or_insert_with(|| Value::Object(Map::new()));
    }
    if let Value::Object(o) = at {
        o.insert(last.clone(), to);
    }
}

/// A column named by its keys joined with dots, as the older tests name them (none of their keys has a dot in it): the same
/// cell as the dialog's own, read, opened and written by its keys.
#[cfg(test)]
fn keys_of(col: &str) -> Vec<String> {
    col.split('.').map(str::to_owned).collect()
}

#[cfg(test)]
fn get<'a>(row: &'a Value, col: &str) -> Option<&'a Value> {
    get_at(row, &keys_of(col))
}

#[cfg(test)]
fn reachable(row: &Value, col: &str) -> bool {
    reachable_at(row, &keys_of(col))
}

/// Whether a row can take a value at a column's keys: each object on the way is there or can be made.
fn reachable_at(row: &Value, keys: &[String]) -> bool {
    match (row, keys.split_first()) {
        (Value::Object(o), Some((k, rest))) => rest.is_empty() || o.get(k).is_none_or(|x| reachable_at(x, rest)),
        _ => false,
    }
}

/// Writes a cell: where the row has no object for it yet, one is made (another row has that column).
#[cfg(test)]
fn put(row: &mut Value, col: &str, to: Value) {
    put_at(row, &keys_of(col), to);
}

/// A list as a cell shows it, and as it is typed back: values between commas.
fn list_text(v: &Value) -> String {
    v.as_array().map_or_else(|| cell_text(v), |a| a.iter().map(item_text).collect::<Vec<_>>().join(", "))
}

/// An item of a list as it is typed: in quotes where it would not read back as itself (a text with a comma, or one that
/// reads as a number or a word such as `true`).
fn item_text(v: &Value) -> String {
    let text = cell_text(v);
    match v {
        Value::String(s) if text.contains(',') || text.starts_with('"') || cell_value(&text) != *v => {
            serde_json::to_string(s).unwrap_or(text)
        }
        _ => text,
    }
}

/// A list as typed: values between commas, a value in quotes taken as that text whole; or the list itself, written as JSON.
#[cfg(test)]
fn list_value(typed: &str) -> Value {
    list_value_over(typed, &Value::Null)
}

/// `list_value`, over the list `was` the cell held: an item typed as it showed one of `was` at its place is that item as
/// it was, not read again (a number too long to read back exactly stays the number it was).
fn list_value_over(typed: &str, was: &Value) -> Value {
    if typed.trim_start().starts_with('[')
        && let Ok(list @ Value::Array(_)) = serde_json::from_str::<Value>(typed)
    {
        return list;
    }
    let was = was.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut items = Vec::new();
    let mut rest = typed;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        // A quoted item: the text between its quotes, as JSON reads it.
        if rest.starts_with('"') {
            let mut de = serde_json::Deserializer::from_str(rest).into_iter::<String>();
            if let Some(Ok(text)) = de.next() {
                items.push(Value::String(text));
                rest = &rest[de.byte_offset()..];
                rest = rest.trim_start().strip_prefix(',').unwrap_or(rest);
                continue;
            }
        }
        let (item, after) = rest.split_once(',').unwrap_or((rest, ""));
        if !item.trim().is_empty() {
            let as_it_was = was.get(items.len()).filter(|w| item_text(w) == item.trim()).cloned();
            items.push(as_it_was.unwrap_or_else(|| cell_value(item.trim())));
        }
        rest = after;
    }
    Value::Array(items)
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
                    // A list stays a list, empty, so what is typed in it reads as one.
                    Value::Object(
                        o.iter()
                            .map(|(k, v)| {
                                let blank = match v {
                                    Value::Object(_) => empty_lists(blank_of(v)),
                                    Value::Array(_) => Value::Array(Vec::new()),
                                    _ => Value::Null,
                                };
                                (k.clone(), blank)
                            })
                            .collect(),
                    )
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
            stop_typing(ui);
        }
    }
}

/// An object's lists of plain values, emptied, as empty lists (not lists of nulls, which would read as lists already typed).
fn empty_lists(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| match x {
                    Value::Array(a) if a.iter().all(scalar) => (k, Value::Array(Vec::new())),
                    x => (k, empty_lists(x)),
                })
                .collect(),
        ),
        other => other,
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
                    // An empty cell is empty to type in: its dash is a hint, not text typed after.
                    let mut text = if c.is_null() { String::new() } else { cell_text(c) };
                    let w = if c.is_string() { width_of(std::iter::once(text.chars().count())) } else { 64.0 };
                    // As in a table of objects: the text typed while it is typed in, under an id of its own, so its undo
                    // history goes when the rows change or the dialog closes.
                    // Under the table's own place in the window: two tables whose paths read alike (a key with a dot in it)
                    // never share a cell.
                    let typing = ui.make_persistent_id(("plain cell", i, j));
                    typed_so_far(ui, typing, &mut text);
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(w).hint_text("—").id(typing)).changed() {
                        ui.data_mut(|d| d.insert_temp(typing, Typing(text.clone())));
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
        stop_typing(ui);
    }
    table_tools(ui, rows, pasted);
}

/// A table of objects: a column for each key, an object inside a row spread over columns of its own, its source last; a
/// column the code explains says so when pointed at.
fn object_table(ui: &mut Ui, id: &str, rows: &mut Vec<Value>, hints: &Hints, pasted: &mut String) {
    let cols = object_columns(rows);
    // Which rows hold a list in each column, and which a plain value: by row, so a row's own value, typed so far, never
    // decides its own cell's kind.
    let holding = |c: &Vec<String>, f: fn(&Value) -> bool| -> Vec<usize> {
        rows.iter().enumerate().filter(|(_, r)| get_at(r, c).is_some_and(f)).map(|(i, _)| i).collect()
    };
    let lists: BTreeMap<&Vec<String>, Vec<usize>> =
        cols.iter().map(|c| (c, holding(c, |v| v.as_array().is_some_and(|a| !a.is_empty())))).collect();
    let plains: BTreeMap<&Vec<String>, Vec<usize>> = cols.iter().map(|c| (c, holding(c, |v| scalar(v) && !v.is_null()))).collect();
    let others = |m: &BTreeMap<&Vec<String>, Vec<usize>>, c: &Vec<String>, i: usize| m.get(c).is_some_and(|rs| rs.iter().any(|r| *r != i));
    let widths: Vec<f32> = cols
        .iter()
        .map(|c| {
            let texts = rows.iter().filter_map(|r| get_at(r, c)).map(|v| list_text(v).chars().count());
            let head = column_title(leaf_of(c)).chars().count();
            width_of(texts.chain(std::iter::once(head.min(14))))
        })
        .collect();
    let mut gone = None;
    ScrollArea::horizontal().id_salt((id, "scroll")).show(ui, |ui| {
        egui::Grid::new((id, "objects")).striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
            for c in &cols {
                let leaf = leaf_of(c);
                let head = ui.label(RichText::new(column_title(leaf)).color(pal().muted).small());
                if let Some(Value::String(meaning)) = leaf.first().and_then(|k| hints.columns.get(k)) {
                    head.on_hover_text(meaning);
                }
            }
            ui.label("");
            ui.end_row();
            for (i, row) in rows.iter_mut().enumerate() {
                for (c, w) in cols.iter().zip(&widths) {
                    let now = get_at(row, c).cloned().unwrap_or(Value::Null);
                    // An object this row gives here is spread over the columns after it, which are the ones typed in: this
                    // cell would overwrite them all.
                    let spread = now.is_object();
                    // An empty cell is empty to type in: its dash is a hint, not text typed after.
                    let mut text = if now.is_null() || spread { String::new() } else { list_text(&now) };
                    // While a cell is typed in, it keeps what was typed (a list's comma at its end, a number's zeros after its
                    // point), not what that reads as so far; left, it shows what it holds again.
                    let typing = ui.make_persistent_id(("cell", i, c));
                    typed_so_far(ui, typing, &mut text);
                    // A column inside a value this row gives whole is not one it can be typed in.
                    let open = !spread && reachable_at(row, c);
                    let edit = if c.as_slice() == ["source"] || *w >= 300.0 {
                        egui::TextEdit::multiline(&mut text).desired_width(*w).desired_rows(1).hint_text("—")
                    } else {
                        egui::TextEdit::singleline(&mut text).desired_width(*w).hint_text("—")
                    };
                    let r = ui.add_enabled(open, edit.id(typing));
                    let r = if spread {
                        r.on_disabled_hover_text(t("This row gives this value in the columns after it"))
                    } else {
                        r.on_disabled_hover_text(t("This row gives one value for these columns"))
                    };
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(typing, Typing(text.clone())));
                        // The kind of what is typed is the kind of what the cell held when typing began (not of what it holds
                        // so far): a list stays a list; an empty one, in a column other rows hold lists in, takes a list,
                        // and in one they hold plain values in too, when what is typed has a comma or is written as one.
                        let (began, first) = ui.data_mut(|d| {
                            let began = d.get_temp_mut_or_insert_with(typing, || Began(now.clone())).0.clone();
                            let first = d.get_temp_mut_or_insert_with(typing, || First(now.clone())).0.clone();
                            (began, first)
                        });
                        // Undone back to what it showed when first typed in during this dialog (even after leaving and coming
                        // back): what it held then, as it was.
                        let shown_first = if first.is_null() { String::new() } else { list_text(&first) };
                        let empty = began.is_null() || began.as_array().is_some_and(Vec::is_empty);
                        let written_as_list = text.contains(',') || text.trim_start().starts_with('[');
                        let list = if empty {
                            // Other rows hold lists only: a list. Plain values only: a plain value. Both, or neither: as written.
                            match (others(&lists, c, i), others(&plains, c, i)) {
                                (true, false) => true,
                                (false, true) => false,
                                _ => written_as_list,
                            }
                        } else {
                            began.is_array()
                        };
                        // Only an undo or a redo (not the same text typed anew, which reads as it is typed).
                        let undone = ui.input(|i| {
                            i.events.iter().any(|e| {
                                matches!(e, egui::Event::Key { key: egui::Key::Z | egui::Key::Y, pressed: true, modifiers, .. } if modifiers.command)
                            })
                        });
                        let value = if undone && text == shown_first {
                            first.clone()
                        } else if list {
                            list_value_over(&text, &began)
                        } else {
                            cell_value(&text)
                        };
                        put_at(row, c, value);
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
        // The rows after it take its place, and its cells' ids: no cell stays in typing, so what was typed in it goes with
        // it, never into the row after.
        stop_typing(ui);
    }
    table_tools(ui, rows, pasted);
}

/// What a cell holds as typed while it is typed in (`object_table`).
#[derive(Clone, Default)]
struct Typing(String);

/// What a cell held when this typing in it began: the kind of what is typed, and the items it keeps.
#[derive(Clone, Default)]
struct Began(Value);

/// What a cell held when it was first typed in while the dialog is open: what an undo back to its first text restores, even
/// after leaving the cell and coming back. Forgotten with the rest when rows change or the dialog closes.
#[derive(Clone, Default)]
struct First(Value);

/// While cell `typing` is typed in, `text` is what was typed, not what it reads as so far; left, what it holds. The cell
/// is counted among those whose typing and undo history go when the rows change or the dialog closes.
fn typed_so_far(ui: &Ui, typing: egui::Id, text: &mut String) {
    if ui.memory(|m| m.has_focus(typing)) {
        if let Some(Typing(was)) = ui.data(|d| d.get_temp::<Typing>(typing)) {
            *text = was;
        }
    } else {
        ui.data_mut(|d| {
            d.remove::<Typing>(typing);
            d.remove::<Began>(typing);
        });
    }
    ui.data_mut(|d| d.get_temp_mut_or_default::<Drawn>(egui::Id::new("codes cells drawn")).0.insert(typing));
}

/// No cell stays in typing: the rows were replaced or one was taken away, so the ids of the cells now name others' cells,
/// and their undo history goes with them.
fn stop_typing(ui: &Ui) {
    ui.memory_mut(|m| {
        if let Some(f) = m.focused() {
            m.surrender_focus(f);
        }
    });
    forget_typing(ui.ctx());
}

/// The cells drawn while the dialog is open, so their typing and undo history go when it closes.
#[derive(Clone, Default)]
struct Drawn(std::collections::HashSet<egui::Id>);

/// Forgets what cells held as typed, and their undo history: the dialog closed, so a cancelled edit can't come back.
pub(crate) fn forget_typing(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove_by_type::<Typing>();
        d.remove_by_type::<Began>();
        d.remove_by_type::<First>();
        let key = egui::Id::new("codes cells drawn");
        for id in d.get_temp::<Drawn>(key).unwrap_or_default().0 {
            d.remove::<egui::text_edit::TextEditState>(id);
        }
        d.remove::<Drawn>(key);
    });
}

/// A table's column in words: `demand_kva.over` is "Demand (kVA) over", `connection_mm2.copper` "Connection (mm²) copper".
fn column_name(col: &str) -> String {
    match col.split_once('.') {
        Some((a, b)) => format!("{} {}", human(a), column_name(b).to_lowercase()),
        None => human(col),
    }
}

/// A column's keys under the table's `columns`, which every one of them is under where it is.
fn leaf_of(c: &[String]) -> &[String] {
    match c.split_first() {
        Some((k, rest)) if k == "columns" && !rest.is_empty() => rest,
        _ => c,
    }
}

/// A column in words, by its keys: `demand_kva` then `over` is "Demand (kVA) over"; a key with a dot in it reads as it is
/// written, so it never reads as a path into an object.
fn column_title(keys: &[String]) -> String {
    let mut out = String::new();
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // Written as it is, in any place: a key with a dot in it is never read as words.
        match (k.contains('.'), i) {
            (true, _) => out.push_str(k),
            (false, 0) => out.push_str(&column_name(k)),
            (false, _) => out.push_str(&column_name(k).to_lowercase()),
        }
    }
    out
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
        // The cards after it take its place, and its tables' cells' ids.
        stop_typing(ui);
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

    /// The columns as the older tests name them, by their keys joined with dots.
    fn col_names(rows: &[Value]) -> Vec<String> {
        object_columns(rows).iter().map(|c| c.join(".")).collect()
    }

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
            let cols = col_names(&rows);
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
        assert_eq!(col_names(original.as_array().unwrap()), ["name", "columns.a.b", "columns.a", "source"]);
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
        // A key with a dot in it (`"a.b"`) and a path into an object (`a` then `b`) are two columns: each cell typed in
        // writes where its column is, never where the other one is.
        let mut rows = json!([{"a.b":7,"a":{"b":11}},{"a.b":13},{"a":{"b":17}}]);
        let rows = rows.as_array_mut().unwrap();
        let (literal, path) = (vec!["a.b".to_string()], vec!["a".to_string(), "b".to_string()]);
        assert_eq!(object_columns(rows), [path.clone(), literal.clone()]);
        assert_eq!((get_at(&rows[0], &literal), get_at(&rows[0], &path)), (Some(&json!(7)), Some(&json!(11))));
        assert_eq!((get_at(&rows[1], &path), get_at(&rows[2], &literal)), (None, None));
        put_at(&mut rows[0], &literal, json!(23));
        put_at(&mut rows[0], &path, json!(29));
        put_at(&mut rows[1], &literal, json!(25));
        put_at(&mut rows[2], &path, json!(31));
        assert_eq!(rows.as_slice(), [json!({"a.b":23,"a":{"b":29}}), json!({"a.b":25}), json!({"a":{"b":31}})]);
        // Each cell is empty where its row lacks it, and typed in, makes only its own.
        put_at(&mut rows[1], &path, json!(37));
        put_at(&mut rows[2], &literal, json!(41));
        assert_eq!(rows[1], json!({"a.b":25,"a":{"b":37}}));
        assert_eq!(rows[2], json!({"a":{"b":31},"a.b":41}));
        // The review's: a row with `columns["custom.flag"]` beside a table that only has `columns.custom.flag`.
        let mut rows = json!([{"columns":{"custom.flag":11}},{"columns":{"custom":{"flag":17}}}]);
        let rows = rows.as_array_mut().unwrap();
        let cols = object_columns(rows);
        let literal = vec!["columns".to_string(), "custom.flag".to_string()];
        assert_eq!(cols.len(), 2);
        assert!(cols.contains(&literal));
        put_at(&mut rows[1], &literal, json!(25));
        assert_eq!(rows[1], json!({"columns":{"custom":{"flag":17},"custom.flag":25}}));
        assert_eq!(column_title(leaf_of(&literal)), "custom.flag");
        assert_eq!(column_title(&["custom".to_string(), "flag".to_string()]), "Custom flag");
        // Emptied where the row lacks the key, nothing is made to hold nothing; a key with a dot reads as written in any
        // place, so two that differ only in capitals never read alike.
        let mut lacking = json!({"columns":{"custom":{"flag":17}}});
        put_at(&mut lacking, &literal, Value::Null);
        assert_eq!(lacking, json!({"columns":{"custom":{"flag":17}}}));
        let deep = |leaf: &str| column_title(&["outer".into(), "middle.dot".into(), leaf.into()]);
        assert_eq!(
            (deep("Leaf.Dot"), deep("leaf.dot")),
            ("Outer middle.dot Leaf.Dot".to_string(), "Outer middle.dot leaf.dot".to_string())
        );
    }

    #[test]
    fn g7_add_delete_and_lists() {
        let mut h = g7_table(json!([{"name":"seed","a":{"b":12},"list":["old"],"source":"end"},{"name":"last","list":["new"]}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        assert_eq!(h.state().0[2], json!({"name":null,"list":[]}));
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
        assert_eq!(staged, json!({"name":"added","a":{"b":25},"list":[]}));
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
                let cols = col_names(rows);
                assert_eq!(cols.len(), paths.len(), "{name}");
                for row in *rows {
                    for path in &paths {
                        let col = path.join(".");
                        assert!(cols.contains(&col));
                        let mut observed = row.clone();
                        let input = if get_at(row, path).is_some_and(Value::is_array) { list_value("a, b") } else { cell_value("999") };
                        let expected = g7_expected(row, path, input.clone());
                        put_at(&mut observed, path, input);
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
                    for col in col_names(rows) {
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
        g7_type(&mut h, "T4", "custom.flag", "25");
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
    fn a_key_with_a_dot_and_a_path_alike_are_two_columns_in_the_dialog() {
        // The review's (G7, round 2): one row gives `columns["custom.flag"]`, another only `columns.custom.flag` as a path.
        // Typed in the key's column, the second row takes the key; the path's value stays as it was.
        let mut h = g7_table(json!([
            {"name":"seed","columns":{"custom.flag":11}},
            {"name":"T4","columns":{"custom":{"flag":17}}}
        ]));
        g7_type(&mut h, "T4", "custom.flag", "25");
        assert_eq!(h.state().0[1], json!({"name":"T4","columns":{"custom":{"flag":17},"custom.flag":25}}));
        g7_type(&mut h, "seed", "Custom flag", "29");
        assert_eq!(h.state().0[0], json!({"name":"seed","columns":{"custom.flag":11,"custom":{"flag":29}}}));
    }

    #[test]
    fn g7r2_codes_deep_dotted_segments() {
        let mut h = g7_table(json!([
            {"name":"seed","top.dot":11,"columns":{"outer":{"middle.dot":{"leaf.dot":12}}}},
            {"name":"target","columns":{"keep":8}}
        ]));
        g7_type(&mut h, "target", "top.dot", "23");
        g7_type(&mut h, "target", "Outer middle.dot leaf.dot", "25");
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
        g7_type(&mut h, "T4", "custom.flag", "25");
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

    // G7b: the whole cell of an object, lists typed slowly, a row added (the reviews' cases, to the contract).
    #[test]
    fn g7b_a_created_child_leaves_the_whole_cell_empty() {
        let mut h = g7_table(json!([{"name":"nested","a":{"b":12}},{"name":"whole","a":"keep"},{"name":"missing"}]));
        g7_type(&mut h, "missing", "A b", "25");
        let child = g7_cell(&h, "missing", "A b").value();
        let parent = g7_cell(&h, "missing", "A").value();
        // The object made is in its own columns; the cell of the whole shows nothing to type over.
        assert_eq!(child.as_deref(), Some("25"));
        assert!(matches!(parent.as_deref(), None | Some("")), "{parent:?}");
    }

    #[test]
    fn g7b_the_whole_cell_of_an_object_does_not_overwrite_its_columns() {
        let mut h = g7_table(json!([{"name":"nested","a":{"b":12,"c":33},"source":"end"},{"name":"whole","a":"keep","source":"other"}]));
        g7_type(&mut h, "nested", "A", "5");
        assert_eq!(h.state().0[0]["a"], json!({"b":12,"c":33}));
    }

    #[test]
    fn g7b_dialog_fecoergs_one_cell_edited_is_the_only_change() {
        let mut h = g7_dialog("fecoergs");
        let before = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = before["tables"][0]["rows"].as_array().unwrap().iter().position(|r| r["name"] == "A1").unwrap();
        let parent_before = g7_cell(&h, "A1", "Resistive (kW)").value();
        g7_type(&mut h, "A1", "Resistive (kW) fn", "25");
        let parent_after = g7_cell(&h, "A1", "Resistive (kW)").value();
        g7_ok(&mut h);
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let mut expected = json!(before);
        expected["tables"][0]["rows"][row]["columns"]["resistive_kw"]["fn"] = json!(25);
        assert!(Value::Object(after) == expected, "the edit should be the only difference in the resolved section");
        assert_eq!(parent_after, parent_before);
    }

    #[test]
    fn g7b_dialog_fecoergs_whole_cell_is_closed_and_keeps_its_number() {
        let mut h = g7_dialog("fecoergs");
        // The cell of the whole object is closed: nothing typed there reaches its columns.
        assert!(g7_cell(&h, "A1", "Resistive (kW)").accesskit_node().is_disabled());
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = after["tables"][0]["rows"].as_array().unwrap().iter().find(|r| r["name"] == "A1").unwrap();
        assert_eq!(row["columns"]["resistive_kw"]["fn"], json!(5.4));
    }

    #[test]
    fn g7b_a_list_typed_over_several_frames_keeps_its_commas() {
        let mut h = g7_table(json!([{"name":"one","list":["old"]}]));
        g7_type(&mut h, "one", "List", "a");
        for s in [",", " ", "b"] {
            h.key_press(egui::Key::End);
            h.event(egui::Event::Text(s.into()));
            h.run();
        }
        assert_eq!(h.state().0[0]["list"], json!(["a", "b"]));
    }

    #[test]
    fn g7b_a_row_added_keeps_a_list_a_list() {
        let mut h = g7_table(json!([
            {"name":"seed","columns":{"a":{"b":12,"c":13}},"breaker_a":[63,80],"source":"table"},
            {"name":"last","columns":{"a":{"c":15}},"breaker_a":[100,125],"source":"last source"}
        ]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        let name = h
            .get_all_by(|n| n.role() == Role::TextInput && matches!(n.value().as_deref(), None | Some("")))
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()).then_with(|| b.rect().left().total_cmp(&a.rect().left())))
            .unwrap();
        name.focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("added".into()));
        h.run();
        g7_type(&mut h, "added", "A c", "24");
        g7_type(&mut h, "added", "A b", "25");
        g7_type(&mut h, "added", "Breaker (A)", "100, 125");
        let row = &h.state().0[2];
        assert_eq!(row["columns"]["a"]["c"], json!(24));
        assert_eq!(
            (&row["columns"]["a"]["b"], &row["breaker_a"]),
            (&json!(25), &json!([100, 125])),
            "new nested cells and comma-separated list in the cloned row"
        );
    }

    #[test]
    fn g7b_dialog_a_breaker_list_typed_slowly_is_saved_as_typed() {
        let mut h = g7_dialog("energisa");
        g7_type(&mut h, "T4", "Breaker (A)", "100");
        for s in [",", " ", "1", "2", "5"] {
            h.key_press(egui::Key::End);
            h.event(egui::Event::Text(s.into()));
            h.run();
        }
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        let after = h.state().session.model.resolve_code(Discipline::Electrical, "entrance").value;
        let row = after["tables"][0]["rows"].as_array().unwrap().iter().find(|r| r["name"] == "T4").unwrap();
        assert_eq!(row["breaker_a"], json!([100, 125]));
    }

    // The review's cases (G7b).
    // Paste inside codes::tree::tests, alongside the existing g7 helpers.

    fn review_append<S>(h: &mut Harness<'_, S>, pieces: &[&str]) {
        for piece in pieces {
            h.key_press(egui::Key::End);
            h.event(egui::Event::Text((*piece).into()));
            h.run();
        }
    }

    fn review_open_entrance(h: &mut Harness<'_, crate::BimApp>) {
        let app = h.state_mut();
        let mut d = crate::codes::CodesDialog::new(&app.session.model);
        d.tab = 3;
        d.tab_mut(Discipline::Electrical).section = "entrance".into();
        d.table = Some(("entrance".into(), "tables".into()));
        app.codes_dialog = Some(d);
        h.run();
    }

    fn review_expect_entrance(h: &Harness<'_, crate::BimApp>, expected: &Value) {
        let got = g7r2_entrance(h);
        assert!(
            &got == expected && h.state().codes_dialog.is_none(),
            "dialog error: {:?}; T4 actual: {}; T4 expected: {}",
            h.state().codes_dialog.as_ref().map(|d| &d.error),
            got["tables"][0]["rows"][g7r2_t4(&got)],
            expected["tables"][0]["rows"][g7r2_t4(expected)]
        );
    }

    fn review_leaving_list(how: &str) {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "90");
        review_append(&mut h, &[",", " ", "1", "2", "0"]);
        match how {
            "Tab" => h.key_press(egui::Key::Tab),
            "click" => g7_cell(&h, "T4", "Name").click(),
            // OK, without leaving the cell.
            _ => {}
        }
        h.run();
        if how != "OK" {
            assert!(!g7_cell(&h, "T4", "Breaker (A)").is_focused());
        }
        h.get_by_label("OK").click();
        h.run();
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([90, 120]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_list_then_tab() {
        review_leaving_list("Tab");
    }

    #[test]
    fn review_codes_list_then_click_elsewhere() {
        review_leaving_list("click");
    }

    #[test]
    fn review_codes_list_then_ok() {
        review_leaving_list("OK");
    }

    #[test]
    fn review_codes_two_lists_and_source() {
        let mut h = g7_table(json!([
            {"name":"one","list":[1],"second":[2],"source":"source 1"},
            {"name":"two","list":[3],"second":[4],"source":"source 2"}
        ]));
        g7_type(&mut h, "one", "List", "10");
        review_append(&mut h, &[",", "20"]);
        g7_type(&mut h, "one", "Second", "30");
        review_append(&mut h, &[",", "40"]);
        g7_type(&mut h, "two", "Source", "two\nlines");
        assert_eq!(
            h.state().0,
            json!([
                {"name":"one","list":[10,20],"second":[30,40],"source":"source 1"},
                {"name":"two","list":[3],"second":[4],"source":"two\nlines"}
            ])
        );
    }

    #[test]
    fn review_codes_list_then_add_row() {
        let mut h = g7_table(json!([
            {"name":"one","list":[1],"group":{"limit":12,"options":[2,3]},"source":"original"}
        ]));
        g7_type(&mut h, "one", "List", "10");
        review_append(&mut h, &[",", "20"]);
        h.get_by_label("Add Row").click();
        h.run();
        let name_x = h.get_by_label("Name").rect().left();
        h.get_all_by(|n| n.role() == Role::TextInput)
            .filter(|n| (n.rect().left() - name_x).abs() < 12.0)
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap()
            .focus();
        h.run();
        h.event(egui::Event::Text("two".into()));
        h.run();
        g7_type(&mut h, "two", "Group limit", "25");
        g7_type(&mut h, "two", "Group options", "5");
        review_append(&mut h, &[",", "6"]);
        g7_type(&mut h, "two", "List", "30");
        review_append(&mut h, &[",", "40"]);
        g7_type(&mut h, "two", "Source", "my source");
        assert_eq!(
            h.state().0,
            json!([
                {"name":"one","list":[10,20],"group":{"limit":12,"options":[2,3]},"source":"original"},
                {"name":"two","list":[30,40],"group":{"limit":25,"options":[5,6]},"source":"my source"}
            ])
        );
    }

    #[test]
    fn review_codes_list_then_delete_row() {
        let mut h = g7_table(json!([
            {"name":"one","list":[1],"source":"first"},
            {"name":"two","list":[2],"source":"second"}
        ]));
        g7_type(&mut h, "one", "List", "10");
        review_append(&mut h, &[","]);
        h.get_all_by_label("×").next().unwrap().click();
        h.run();
        h.event(egui::Event::Text("99".into()));
        h.run();
        assert_eq!(h.state().0, json!([{"name":"two","list":[2],"source":"second"}]));
    }

    #[test]
    fn review_codes_list_then_delete_row_accesskit() {
        let mut h = g7_table(json!([
            {"name":"one","list":[1],"source":"first"},
            {"name":"two","list":[2],"source":"second"}
        ]));
        g7_type(&mut h, "one", "List", "10");
        review_append(&mut h, &[","]);
        h.get_all_by_label("×").next().unwrap().click_accesskit();
        h.run();
        h.event(egui::Event::Text("99".into()));
        h.run();
        assert_eq!(h.state().0, json!([{"name":"two","list":[2],"source":"second"}]));
    }

    #[test]
    fn review_codes_cancel_reopen_then_append() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "999");
        review_append(&mut h, &[","]);
        h.get_by_label("Cancel").click();
        h.run();
        review_expect_entrance(&h, &expected);

        review_open_entrance(&mut h);
        g7_cell(&h, "T4", "Breaker (A)").focus();
        h.run();
        review_append(&mut h, &[",", "63"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"].as_array_mut().unwrap().push(json!(63));
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_clear_list_through_ok() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "");
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_mixed_object_and_plain_in_both_orders() {
        let nested = json!({"name":"nested","a":{"b":5.4,"c":33},"source":"nested source"});
        let plain = json!({"name":"plain","a":8,"source":"plain source"});
        for rows in [json!([nested, plain]), json!([plain, nested])] {
            let mut expected = rows.clone();
            let mut h = g7_table(rows);
            g7_type(&mut h, "plain", "A", "25");
            g7_cell(&h, "nested", "A").click();
            h.run();
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.event(egui::Event::Text("91".into()));
            h.run();
            let rows = expected.as_array_mut().unwrap();
            rows.iter_mut().find(|row| row["name"] == "plain").unwrap()["a"] = json!(25);
            assert_eq!(h.state().0, expected);
        }
    }

    #[test]
    fn review_codes_fecoergs_whole_cell_keeps_original_through_ok() {
        let mut h = g7_dialog("fecoergs");
        let expected = g7r2_entrance(&h);
        g7_type(&mut h, "A1", "Resistive (kW)", "91");
        g7_ok(&mut h);
        assert!(
            g7r2_entrance(&h) == expected && h.state().codes_dialog.is_none(),
            "A1 actual: {}; dialog error: {:?}",
            g7r2_entrance(&h)["tables"][0]["rows"][0],
            h.state().codes_dialog.as_ref().map(|d| &d.error)
        );
    }

    #[test]
    fn review_codes_typing_a_list_into_an_empty_cell_keeps_focus() {
        let mut h = g7_table(json!([{"name":"one","list":null,"source":"keep"}]));
        g7_type(&mut h, "one", "List", "[1,2]");
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("3,4".into()));
        h.run();
        assert_eq!(h.state().0, json!([{"name":"one","list":[3,4],"source":"keep"}]));
    }

    fn review_cell_at<'a, S>(h: &'a Harness<'_, S>, row: usize, header: &str) -> egui_kittest::Node<'a> {
        let label = h.get_all_by_label(header).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap();
        let x = label.rect().left();
        let y = label.rect().top();
        let mut cells: Vec<_> = h
            .get_all_by(|n| matches!(n.role(), Role::TextInput | Role::MultilineTextInput))
            .filter(|n| (n.rect().left() - x).abs() < 12.0 && n.rect().top() > y)
            .collect();
        cells.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
        cells.into_iter().nth(row).unwrap_or_else(|| panic!("missing cell {row} / {header}"))
    }

    fn review_type_at<S>(h: &mut Harness<'_, S>, row: usize, header: &str, text: &str) {
        review_cell_at(h, row, header).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(text.into()));
        h.run();
    }

    fn review_data_table(rows: &[Value]) -> Harness<'static, (Value, String)> {
        Harness::builder().with_size([5000.0, 2600.0]).with_step_dt(0.25).build_ui_state(
            |ui, (rows, pasted)| editor(ui, "review", "rows", rows, &Hints::default(), pasted),
            (json!(rows), String::new()),
        )
    }

    #[test]
    fn review_codes_electrical_catalogue_table_samples() {
        let mut differences = Vec::new();
        for (name, doc) in g7_electrical_files() {
            let mut real_tables = Vec::new();
            g7_tables(&doc["sections"], &mut real_tables);
            for (table_no, rows) in real_tables.into_iter().enumerate() {
                let mut paths = std::collections::BTreeSet::new();
                for row in rows {
                    g7_paths(row, &mut Vec::new(), &mut paths);
                }
                let mut cases = Vec::new();
                let mut numeric = false;
                for path in &paths {
                    let at = |row: &Value| -> Option<Value> { path.iter().try_fold(row, |v, key| v.get(key)).cloned() };
                    let header = |path: &[String]| column_name(path.join(".").trim_start_matches("columns."));
                    if !numeric && let Some(i) = rows.iter().position(|r| at(r).is_some_and(|v| v.is_number())) {
                        numeric = true;
                        cases.push((i, header(path), "25", g7_expected(&rows[i], path, json!(25)).unwrap(), false));
                    }
                    if let Some(i) = rows.iter().position(|r| at(r).is_some_and(|v| v.is_array())) {
                        let value = at(&rows[i]).unwrap();
                        let (text, expected) = if value.as_array().unwrap().first().is_some_and(Value::is_number) {
                            ("90", json!([90, 120]))
                        } else {
                            ("I", json!(["I", "II"]))
                        };
                        cases.push((i, header(path), text, g7_expected(&rows[i], path, expected).unwrap(), true));
                    }
                    if let Some(i) = rows.iter().position(|r| at(r).is_some_and(|v| v.is_object())) {
                        cases.push((i, header(path), "25", rows[i].clone(), false));
                        if let Some(i) = rows.iter().position(|r| at(r).is_some_and(|v| v.is_string())) {
                            cases.push((i, header(path), "review", g7_expected(&rows[i], path, json!("review")).unwrap(), false));
                        }
                    }
                }
                if cases.is_empty() {
                    let path = paths.iter().next().expect("object table has an editable leaf");
                    let i = rows.iter().position(|row| path.iter().try_fold(row, |v, key| v.get(key)).is_some()).unwrap();
                    let header = column_name(path.join(".").trim_start_matches("columns."));
                    let expected = g7_expected(&rows[i], path, json!("review")).unwrap();
                    cases.push((i, header, "review", expected, false));
                }
                for (i, header, text, expected_row, array) in cases {
                    let mut h = review_data_table(rows);
                    review_type_at(&mut h, i, &header, text);
                    if array {
                        let tail = if text == "90" { "120" } else { "II" };
                        review_append(&mut h, &[",", " ", tail]);
                    }
                    let mut expected = rows.to_vec();
                    expected[i] = expected_row;
                    if h.state().0 != json!(expected) {
                        differences.push(format!(
                            "{name} table {table_no} row {i} {header}: actual {}; expected {}",
                            h.state().0[i],
                            expected[i]
                        ));
                    }
                }
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }

    #[test]
    fn review_codes_slow_decimal_through_ok() {
        let mut h = g7_dialog("fecoergs");
        let mut expected = g7r2_entrance(&h);
        let row = expected["tables"][0]["rows"].as_array().unwrap().iter().position(|r| r["name"] == "A1").unwrap();
        g7_type(&mut h, "A1", "Resistive (kW) fn", "5");
        review_append(&mut h, &[".", "0", "0", "4"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["columns"]["resistive_kw"]["fn"] = json!(5.004);
        let actual = g7r2_entrance(&h);
        assert_eq!(actual["tables"][0]["rows"][row], expected["tables"][0]["rows"][row]);
        assert!(actual == expected);
    }

    #[test]
    fn review_codes_slow_list_round_trips_bimc_and_xml() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "90");
        review_append(&mut h, &[",", "120"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([90, 120]);
        review_expect_entrance(&h, &expected);

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("review.bimc");
        let xml = dir.path().join("review.xml");
        let session = &mut h.state_mut().session;
        session.execute("save", &json!({"path":project})).unwrap();
        session.execute("building_code_export", &json!({"path":xml,"discipline":"electrical","name":"Review"})).unwrap();

        let mut opened = Session::new();
        opened.execute("open", &json!({"path":project})).unwrap();
        assert!(Value::Object(opened.model.resolve_code(Discipline::Electrical, "entrance").value) == expected, ".bimc");
        let mut imported = Session::new();
        imported.execute("building_code_import", &json!({"path":xml,"set":true})).unwrap();
        assert!(Value::Object(imported.model.resolve_code(Discipline::Electrical, "entrance").value) == expected, "XML");
    }

    #[test]
    fn review_codes_delete_accesskit_cannot_write_to_successor_in_session() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "90");
        review_append(&mut h, &[","]);
        let y = g7_cell(&h, "T4", "Breaker (A)").rect().center().y;
        h.get_all_by_label("×").find(|n| (n.rect().center().y - y).abs() < 8.0).unwrap().click_accesskit();
        h.run();
        h.event(egui::Event::Text("120".into()));
        h.run();
        g7_ok(&mut h);
        expected["tables"][0]["rows"].as_array_mut().unwrap().remove(row);
        let got = g7r2_entrance(&h);
        let successor = &got["tables"][0]["rows"][row];
        assert!(
            got == expected && h.state().codes_dialog.is_none(),
            "successor: {successor}; expected: {}; dialog: {:?}",
            expected["tables"][0]["rows"][row],
            h.state().codes_dialog.as_ref().map(|d| &d.error)
        );
    }

    // The confirmation's cases (G7b).
    // Insert inside codes::tree::tests in the archive copy.

    fn g7br2_close_with_draft(accept: bool, pointer: bool) {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        let before = h.ctx.data(|d| d.count::<Typing>());
        g7_type(&mut h, "T4", "Breaker (A)", "90");
        review_append(&mut h, &[",", "120", ","]);
        assert!(h.ctx.data(|d| d.count::<Typing>()) > before, "no staged cell text");
        let button = h.get_by_label(if accept { "OK" } else { "Cancel" });
        if pointer {
            button.click();
        } else {
            button.click_accesskit();
        }
        h.run_steps(4);
        if accept {
            expected["tables"][0]["rows"][row]["breaker_a"] = json!([90, 120]);
        }
        review_expect_entrance(&h, &expected);
        assert_eq!(h.ctx.data(|d| d.count::<Typing>()), before, "closed dialog retained staged cell text");
    }

    #[test]
    fn review_codes_r2_no_draft_after_accesskit_ok() {
        g7br2_close_with_draft(true, false);
    }

    #[test]
    fn review_codes_r2_no_draft_after_accesskit_cancel() {
        g7br2_close_with_draft(false, false);
    }

    #[test]
    fn review_codes_r2_no_draft_after_pointer_ok() {
        g7br2_close_with_draft(true, true);
    }

    #[test]
    fn review_codes_r2_no_draft_after_pointer_cancel() {
        g7br2_close_with_draft(false, true);
    }

    fn g7br2_paste_while_editing(pointer: bool) {
        let mut h = g7_table(json!([{"name":"old","a":1},{"name":"last","a":2}]));
        h.state_mut().1 = "name\ta\nreplacement\t17\nlast\t23".into();
        h.run();
        g7_type(&mut h, "old", "A", "91");
        let paste = h.get_by_label("Use Pasted Rows");
        if pointer {
            paste.click();
        } else {
            paste.click_accesskit();
        }
        h.run();
        let expected = json!([{"name":"replacement","a":17},{"name":"last","a":23}]);
        assert_eq!(h.state().0, expected);
        // No new edit of the replacement row has been requested.
        review_append(&mut h, &["4"]);
        assert_eq!(h.state().0, expected);
    }

    #[test]
    fn review_codes_r2_paste_replaces_focused_row_by_pointer() {
        g7br2_paste_while_editing(true);
    }

    #[test]
    fn review_codes_r2_paste_replaces_focused_row_by_accesskit() {
        g7br2_paste_while_editing(false);
    }

    #[test]
    fn review_codes_r2_plain_cell_and_single_event_number() {
        let mut h = g7_table(json!([{"name":"one","label":"before","number":12},{"name":"two","label":"keep","number":21}]));
        g7_type(&mut h, "one", "Label", "after");
        g7_type(&mut h, "one", "Number", "5.004");
        g7_type(&mut h, "two", "Label", "also kept");
        assert_eq!(h.state().0, json!([{"name":"one","label":"after","number":5.004},{"name":"two","label":"also kept","number":21}]));
    }

    fn g7br2_replace_input(h: &mut Harness<'_, (Value, String)>, old: &str, new: &str) {
        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some(old)).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(new.into()));
        h.run();
    }

    #[test]
    fn review_codes_r2_plain_list_table_edits_and_adds() {
        let mut h = g7_table(json!([[17, 22], [33, 44]]));
        g7br2_replace_input(&mut h, "17", "55");
        h.get_by_label("Add Row").click();
        h.run();
        assert_eq!(h.state().0, json!([[55, 22], [33, 44], [null, null]]));
    }

    #[test]
    fn review_codes_r2_card_form_and_nested_table_edit() {
        let mut h = g7_table(json!([
            {"name":"one","label":"before","rows":[{"name":"inner","limit":12}]},
            {"name":"two","label":"keep","rows":[{"name":"other","limit":21}]}
        ]));
        g7br2_replace_input(&mut h, "before", "after");
        g7_type(&mut h, "inner", "Limit", "34");
        assert_eq!(
            h.state().0,
            json!([
                {"name":"one","label":"after","rows":[{"name":"inner","limit":34}]},
                {"name":"two","label":"keep","rows":[{"name":"other","limit":21}]}
            ])
        );
    }

    #[test]
    fn review_codes_r2_switch_tables_keeps_edits_at_their_rules() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "90,120");
        let label = crate::codes::named("entrance", "members").0;
        h.get_all_by_label(&label).rfind(|n| n.accesskit_node().role() == Role::Button).unwrap().click_accesskit();
        h.run();
        let seat = h
            .get_all_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("Rio Branco"))
            .next()
            .expect("Members editor did not open");
        seat.focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("Rio Branco atualizado".into()));
        h.run();
        let label = crate::codes::named("entrance", "tables").0;
        h.get_all_by_label(&label).rfind(|n| n.accesskit_node().role() == Role::Button).unwrap().click_accesskit();
        h.run();
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([90, 120]);
        expected["members"][0]["seat"] = json!("Rio Branco atualizado");
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_r2_leaving_cell_discards_raw_draft() {
        let mut h = g7_table(json!([{"name":"one","list":[1]},{"name":"two","list":[2]}]));
        let before = h.ctx.data(|d| d.count::<Typing>());
        g7_type(&mut h, "one", "List", "10");
        review_append(&mut h, &[","]);
        assert!(h.ctx.data(|d| d.count::<Typing>()) > before);
        g7_cell(&h, "two", "List").focus();
        h.run_steps(3);
        assert_eq!(h.state().0, json!([{"name":"one","list":[10]},{"name":"two","list":[2]}]));
        assert_eq!(h.ctx.data(|d| d.count::<Typing>()), before);
    }

    #[test]
    fn review_codes_r2_add_row_while_number_is_focused() {
        for pointer in [false, true] {
            let mut h = g7_table(json!([{"name":"one","limit":12},{"name":"two","limit":21}]));
            g7_type(&mut h, "two", "Limit", "50");
            let add = h.get_by_label("Add Row");
            if pointer {
                add.click();
            } else {
                add.click_accesskit();
            }
            h.run();
            assert_eq!(
                h.state().0,
                json!([
                    {"name":"one","limit":12},{"name":"two","limit":50},{"name":null,"limit":null}
                ])
            );
        }
    }

    // G7c: long lists, undo after Cancel, a list column a row added lacks, items with commas (the review's cases).

    #[test]
    fn review_codes_long_list_keeps_receiving_input() {
        let mut h = g7_table(json!([{"name":"one","list":[0],"source":"keep"}]));
        g7_type(&mut h, "one", "List", "1,2,3,4,5,6,7,8,9");
        review_append(&mut h, &[", 10"]);
        assert_eq!(h.state().0, json!([{"name":"one","list":[1,2,3,4,5,6,7,8,9,10],"source":"keep"}]));
    }

    #[test]
    fn review_codes_long_list_through_ok() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "10,20,30,40,50,60,70,80");
        review_append(&mut h, &[",", "90", ",", "100"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_cancelled_list_edit_cannot_return_via_undo() {
        let mut h = g7_dialog("energisa");
        let expected = g7r2_entrance(&h);
        g7_type(&mut h, "T4", "Breaker (A)", "999");
        h.run_steps(5);
        g7_type(&mut h, "T4", "Breaker (A)", "998");
        h.run_steps(5);
        h.get_by_label("Cancel").click();
        h.run();
        review_expect_entrance(&h, &expected);

        review_open_entrance(&mut h);
        g7_cell(&h, "T4", "Breaker (A)").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_codes_appending_preserves_existing_list_items_and_types() {
        let mut h = g7_table(json!([{"name":"one","list":["125", "true", "A, B"],"source":"keep"}]));
        g7_cell(&h, "one", "List").focus();
        h.run();
        review_append(&mut h, &[", final"]);
        assert_eq!(h.state().0, json!([{"name":"one","list":["125", "true", "A, B", "final"],"source":"keep"}]));
    }

    #[test]
    fn a_list_shown_reads_back_as_itself() {
        // What a list cell shows reads back as the same list: texts with commas, quotes, numbers or words in them.
        for v in [json!(["125", "true", "A, B", "say \"hi\"", "", " x "]), json!([125, true, null, 1.5, "plain"]), json!([])] {
            let shown = list_text(&v);
            let back = list_value(&shown);
            assert_eq!(back, v, "{v} shown as {shown}");
        }
        assert_eq!(list_value("\"a, b\", c"), json!(["a, b", "c"]));
        assert_eq!(list_value("\"open, x"), json!(["\"open", "x"]));
    }

    // The review's cases (G7c).
    // Paste inside codes::tree::tests, alongside the g7 and review helpers.

    #[test]
    fn review_g7c_codes_appending_preserves_quoted_items_and_types() {
        let items = json!(["a,\"b\"", "C:\\temp, logs", "one\ntwo", "\"starts", "007", "1e3", "true", "", null, 2, false]);
        let mut h = g7_table(json!([{"name":"one","list":items,"source":"keep"}]));
        let mut expected = h.state().0.clone();
        expected[0]["list"].as_array_mut().unwrap().push(json!("final"));
        review_cell_at(&h, 0, "List").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::End);
        h.event(egui::Event::Text(", final".into()));
        h.run();
        assert_eq!(h.state().0, expected);
    }

    #[test]
    fn review_g7c_codes_typed_escapes_make_one_item_each() {
        let mut h = g7_table(json!([{"name":"one","list":[0],"source":"keep"}]));
        g7_type(&mut h, "one", "List", r#""say \"hi\"", "C:\\temp", "one\ntwo", "007", "1e3", true, "", null"#);
        assert_eq!(
            h.state().0,
            json!([{"name":"one","list":["say \"hi\"","C:\\temp","one\ntwo","007","1e3",true,"",null],"source":"keep"}])
        );
    }

    #[test]
    fn review_g7c_codes_closing_quote_finishes_the_item() {
        let mut h = g7_table(json!([{"name":"one","list":[0],"source":"keep"}]));
        g7_type(&mut h, "one", "List", "\"open, x");
        review_append(&mut h, &["\""]);
        assert_eq!(h.state().0, json!([{"name":"one","list":["open, x"],"source":"keep"}]));
    }

    #[test]
    fn review_g7c_codes_bracketed_list_reaches_session() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "[1,2]");
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([1, 2]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7c_codes_long_list_does_not_move_the_edit_to_a_neighbor() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "10,20,30,40,50,60,70,80");
        for suffix in [",90", ",100", ",110", ",120", ",130", ",140", ",150", ",160", ",170", ",180", ",190", ",200"] {
            h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::End);
            h.event(egui::Event::Text(suffix.into()));
            h.run();
        }
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] =
            json!([10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7c_codes_tsv_paste_reads_quoted_list_into_a_plain_table() {
        let mut h = g7_table(json!([{"name":"one","list":0,"source":"keep"}]));
        review_g7c_paste_tsv(
            &mut h,
            "name\tlist\tsource\none\t[\"a,b\",\"say \\\"hi\\\"\",\"C:\\\\temp\",\"one\\ntwo\",\"007\",\"1e3\",\"\",null,2]\tkeep\n",
        );
        assert_eq!(
            h.state().0,
            json!([{"name":"one","list":["a,b","say \"hi\"","C:\\temp","one\ntwo","007","1e3","",null,2],"source":"keep"}])
        );
    }

    #[test]
    fn review_g7c_codes_tsv_copy_round_trips_quoted_list_items() {
        let rows = json!([{"name":"one","list":["a,b","say \"hi\"","C:\\temp","one\ntwo","1e3","",null,2],"source":"keep"}]);
        let mut source = g7_table(rows.clone());
        let text = review_g7c_copy_tsv(&mut source);
        let mut target = g7_table(json!([{"name":"two","list":0,"source":"old"}]));
        review_g7c_paste_tsv(&mut target, &text);
        assert_eq!(target.state().0, rows);
    }

    fn review_g7c_paste_tsv(target: &mut Harness<'_, (Value, String)>, text: &str) {
        target.get_by(|n| n.role() == Role::MultilineTextInput && n.value().is_none_or(|value| value.is_empty())).focus();
        target.run();
        target.event(egui::Event::Paste(text.into()));
        target.run();
        target.get_by_label("Use Pasted Rows").click_accesskit();
        target.run();
    }

    fn review_g7c_copy_tsv(source: &mut Harness<'_, (Value, String)>) -> String {
        source.get_by_label("Copy").click_accesskit();
        source.step();
        source
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .expect("Copy sent no text to the clipboard")
    }

    #[test]
    fn review_g7c_codes_cancelled_object_table_edits_cannot_return() {
        for redo in [false, true] {
            let mut h = g7_dialog("energisa");
            let expected = g7r2_entrance(&h);
            g7_type(&mut h, "T4", "Breaker (A)", "999");
            h.run_steps(5);
            g7_type(&mut h, "T4", "Breaker (A)", "998");
            h.run_steps(5);
            if redo {
                h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
                h.run();
            }
            h.get_by_label("Cancel").click_accesskit();
            h.run();
            review_expect_entrance(&h, &expected);
            review_open_entrance(&mut h);
            g7_cell(&h, "T4", "Breaker (A)").focus();
            h.run();
            let modifiers = if redo { egui::Modifiers::COMMAND | egui::Modifiers::SHIFT } else { egui::Modifiers::COMMAND };
            h.key_press_modifiers(modifiers, egui::Key::Z);
            h.run();
            g7_ok(&mut h);
            review_expect_entrance(&h, &expected);
        }
    }

    #[test]
    fn review_g7c_codes_ok_keeps_project_history_across_dialogs() {
        let mut h = g7_dialog("energisa");
        let original = g7r2_entrance(&h);
        let mut expected = original.clone();
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "91");
        h.run_steps(5);
        g7_type(&mut h, "T4", "Breaker (A)", "92");
        h.run_steps(5);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([92]);
        review_expect_entrance(&h, &expected);
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        review_expect_entrance(&h, &original);
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        review_expect_entrance(&h, &expected);
        review_open_entrance(&mut h);
        g7_cell(&h, "T4", "Breaker (A)").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7c_codes_deleted_row_undo_cannot_write_into_its_successor() {
        let mut h = g7_table(json!([{"name":"old","number":12},{"name":"successor","number":23}]));
        g7_type(&mut h, "old", "Number", "96");
        h.run_steps(5);
        g7_type(&mut h, "old", "Number", "97");
        h.run_steps(5);
        h.get_all_by_label("×").next().unwrap().click_accesskit();
        h.run();
        g7_cell(&h, "successor", "Number").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert_eq!(h.state().0, json!([{"name":"successor","number":23}]));
    }

    #[test]
    fn review_g7c_codes_dialog_cleanup_leaves_external_undo_intact() {
        let mut h = Harness::builder().with_step_dt(0.25).build_ui_state(
            |ui, (rows, pasted, outside, close): &mut (Value, String, String, bool)| {
                ui.text_edit_singleline(outside);
                if *close {
                    forget_typing(ui.ctx());
                    *close = false;
                } else {
                    editor(ui, "g7c.external-check", "rows", rows, &Hints::default(), pasted);
                }
            },
            (json!([{"name":"one","list":[0]}]), String::new(), "external original".into(), false),
        );
        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("external original")).focus();
        h.run_steps(5);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("external changed".into()));
        h.run_steps(5);
        g7_type(&mut h, "one", "List", "91,92");
        h.state_mut().3 = true;
        h.step();
        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("external changed")).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert_eq!(h.state().2, "external original");
    }

    #[test]
    fn review_g7c_codes_mixed_column_keeps_number_after_clearing() {
        let mut h = g7_table(json!([{"name":"one","list":[1,2]},{"name":"two","list":15}]));
        g7_type(&mut h, "two", "List", "16");
        assert_eq!(h.state().0[1]["list"], json!(16));
        g7_type(&mut h, "two", "List", "");
        review_append(&mut h, &["17"]);
        assert_eq!(h.state().0, json!([{"name":"one","list":[1,2]},{"name":"two","list":17}]));
    }

    #[test]
    fn review_g7c_codes_mixed_scalar_edit_reaches_the_session() {
        let mut h = g7_dialog("energisa");
        let row = g7r2_t4(&g7r2_entrance(&h));
        g7r2_seed_entrance(&mut h, |rows| {
            rows[0]["columns"]["review_mixed"] = json!([11, 12]);
            rows[row]["columns"]["review_mixed"] = json!(15);
        });
        let mut expected = g7r2_entrance(&h);
        g7_type(&mut h, "T4", "Review mixed", "");
        review_append(&mut h, &["17"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["columns"]["review_mixed"] = json!(17);
        review_expect_entrance(&h, &expected);
    }

    // The confirmation's cases (G7c).
    // Insert inside codes::tree::tests. These cases use the existing egui_kittest helpers.

    #[test]
    fn review_g7cr2_codes_cleared_list_stays_empty() {
        let mut h = g7_table(json!([{"name":"one","list":[1,2]},{"name":"two","list":[3]}]));
        g7_type(&mut h, "one", "List", "");
        g7_cell(&h, "two", "List").focus();
        h.run();
        assert_eq!(h.state().0, json!([{"name":"one","list":[]},{"name":"two","list":[3]}]));
    }

    #[test]
    fn review_g7cr2_codes_json_list_typed_one_character_at_a_time_reaches_session() {
        let mut h = g7_dialog("energisa");
        let mut expected = g7r2_entrance(&h);
        let row = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Breaker (A)", "[");
        review_append(&mut h, &["1", ",", "2", "]"]);
        g7_ok(&mut h);
        expected["tables"][0]["rows"][row]["breaker_a"] = json!([1, 2]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7cr2_codes_list_text_can_start_with_a_bracket() {
        for (typed, item) in [("[draft]", "[draft]"), (r#""[1,2]""#, "[1,2]")] {
            let mut h = g7_table(json!([{"name":"one","list":[0]}]));
            g7_type(&mut h, "one", "List", typed);
            review_append(&mut h, &[", final"]);
            assert_eq!(h.state().0, json!([{"name":"one","list":[item,"final"]}]));
        }
    }

    #[test]
    fn review_g7cr2_codes_delete_unrelated_row_keeps_neighbor_edit() {
        let mut h = g7_table(json!([{"name":"keep","number":23},{"name":"remove","number":12}]));
        review_g7cr2_edit_number_twice(&mut h, "keep");
        h.get_all_by_label("×").nth(1).unwrap().click_accesskit();
        h.run();
        review_g7cr2_undo_number(&mut h, "keep");
        assert_eq!(h.state().0, json!([{"name":"keep","number":97}]));
    }

    #[test]
    fn review_g7cr2_codes_paste_keeps_replacement_when_cell_undo_is_pressed() {
        let mut h = g7_table(json!([{"name":"old","number":12}]));
        review_g7cr2_edit_number_twice(&mut h, "old");
        review_g7c_paste_tsv(&mut h, "name\tnumber\nreplacement\t23\n");
        review_g7cr2_undo_number(&mut h, "replacement");
        assert_eq!(h.state().0, json!([{"name":"replacement","number":23}]));
    }

    #[test]
    fn review_g7cr2_codes_undo_still_works_without_row_changes() {
        let mut h = g7_table(json!([{"name":"keep","number":23},{"name":"neighbor","number":12}]));
        review_g7cr2_edit_number_twice(&mut h, "keep");
        review_g7cr2_undo_number(&mut h, "keep");
        assert_eq!(h.state().0, json!([{"name":"keep","number":96},{"name":"neighbor","number":12}]));
    }

    fn review_g7cr2_edit_number_twice(h: &mut Harness<'_, (Value, String)>, row: &str) {
        g7_type(h, row, "Number", "96");
        h.run_steps(5);
        g7_type(h, row, "Number", "97");
        h.run_steps(5);
        assert_eq!(h.state().0[0]["number"], json!(97));
    }

    fn review_g7cr2_undo_number(h: &mut Harness<'_, (Value, String)>, row: &str) {
        g7_cell(h, row, "Number").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
    }

    #[test]
    fn review_g7cr2_codes_undo_preserves_bracketed_text_item() {
        let original = json!([{"name":"one","list":["[1]"]}]);
        let mut h = g7_table(original.clone());
        g7_cell(&h, "one", "List").focus();
        h.run_steps(5);
        review_append(&mut h, &[", tail"]);
        h.run_steps(5);
        assert_eq!(h.state().0, json!([{"name":"one","list":["[1]","tail"]}]));
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert_eq!(h.state().0, original);
    }

    // G7d: a row added in a list column, positional tables, and the rest the reviews of G7c left.

    #[test]
    fn review_g7c_codes_cancelled_plain_table_numbers_cannot_return() {
        let mut h = g7_dialog("energisa");
        review_g7c_open_temperature_table(&mut h);
        let expected = review_g7c_temperature_table(&h);
        assert_eq!(expected[0][1], json!(1.22));
        review_g7c_type_temperature(&mut h, "1.22", "1.27");
        review_g7c_type_temperature(&mut h, "1.27", "1.28");
        h.get_by_label("Cancel").click_accesskit();
        h.run();
        assert_eq!(review_g7c_temperature_table(&h), expected);
        review_g7c_open_temperature_table(&mut h);
        review_g7c_temperature_input(&h, "1.22").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        assert_eq!(review_g7c_temperature_table(&h), expected);
    }

    fn review_g7c_open_temperature_table(h: &mut Harness<'_, crate::BimApp>) {
        let app = h.state_mut();
        let mut dialog = crate::codes::CodesDialog::new(&app.session.model);
        dialog.tab = 3;
        dialog.tab_mut(Discipline::Electrical).section = "circuits".into();
        dialog.table = Some(("circuits".into(), "temperature_air".into()));
        app.codes_dialog = Some(dialog);
        h.run();
    }

    fn review_g7c_temperature_table(h: &Harness<'_, crate::BimApp>) -> Value {
        h.state().session.model.resolve_code(Discipline::Electrical, "circuits").value["temperature_air"].clone()
    }

    fn review_g7c_temperature_input<'a>(h: &'a Harness<'_, crate::BimApp>, text: &'a str) -> egui_kittest::Node<'a> {
        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some(text))
    }

    fn review_g7c_type_temperature(h: &mut Harness<'_, crate::BimApp>, before: &str, after: &str) {
        review_g7c_temperature_input(h, before).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(after.into()));
        h.run_steps(5);
    }

    #[test]
    fn review_codes_added_row_accepts_list_from_earlier_nbr5410_rows() {
        let doc = g7_electrical_files().into_iter().find(|(name, _)| name == "nbr-5410").unwrap().1;
        let rows = doc["sections"]["bath_volumes"]["volumes"][1]["allows"].as_array().unwrap();
        let mut h = review_data_table(rows);
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_type_at(&mut h, rows.len(), "Kinds", "heater");
        review_type_at(&mut h, rows.len(), "Any of", "selv");
        review_type_at(&mut h, rows.len(), "Classes", "I, II");
        review_type_at(&mut h, rows.len(), "Source", "review");
        let mut expected = rows.clone();
        expected.push(json!({"kinds":["heater"],"any_of":["selv"],"classes":["I","II"],"source":"review"}));
        assert_eq!(h.state().0, json!(expected));
    }

    #[test]
    fn review_g7c_codes_added_row_accepts_list_beside_existing_number() {
        let mut h = g7_table(json!([{"name":"one","list":[1,2]},{"name":"two","number":15}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_type_at(&mut h, 2, "List", "3,4");
        assert_eq!(h.state().0[0], json!({"name":"one","list":[1,2]}));
        assert_eq!(h.state().0[1], json!({"name":"two","number":15}));
        assert_eq!(h.state().0[2]["list"], json!([3, 4]));
    }

    #[test]
    fn review_g7cr2_codes_added_row_in_mixed_column_accepts_list() {
        let mut h = g7_table(json!([{"name":"list row","list":[1,2]},{"name":"number row","list":15}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_type_at(&mut h, 2, "List", "3,4");
        assert_eq!(h.state().0[0], json!({"name":"list row","list":[1,2]}));
        assert_eq!(h.state().0[1], json!({"name":"number row","list":15}));
        assert_eq!(h.state().0[2]["list"], json!([3, 4]));
    }

    #[test]
    fn review_g7cr2_codes_positional_paste_keeps_replacement_when_undo_is_pressed() {
        let mut h = g7_table(json!([[12, 31]]));
        for (before, after) in [("12", "96"), ("96", "97")] {
            h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some(before)).focus();
            h.run();
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.event(egui::Event::Text(after.into()));
            h.run_steps(5);
        }
        assert_eq!(h.state().0, json!([[97, 31]]));
        let paste = review_g7c_copy_tsv(&mut g7_table(json!([[23, 41]])));
        review_g7c_paste_tsv(&mut h, &paste);
        assert_eq!(h.state().0, json!([[23, 41]]));
        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("23")).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert_eq!(h.state().0, json!([[23, 41]]));
    }

    #[test]
    fn review_g7cr2_codes_positional_paste_keeps_numbers_through_ok() {
        let mut h = g7_dialog("energisa");
        let mut dialog = crate::codes::CodesDialog::new(&h.state().session.model);
        dialog.tab = 3;
        dialog.tab_mut(Discipline::Electrical).section = "circuits".into();
        dialog.table = Some(("circuits".into(), "temperature_air".into()));
        h.state_mut().codes_dialog = Some(dialog);
        h.run();

        let temperature =
            |app: &crate::BimApp| app.session.model.resolve_code(Discipline::Electrical, "circuits").value["temperature_air"].clone();
        let mut expected = temperature(h.state());
        assert_eq!(expected[0][1], json!(1.22));
        for (before, after) in [("1.22", "1.27"), ("1.27", "1.28")] {
            h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some(before)).focus();
            h.run();
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.event(egui::Event::Text(after.into()));
            h.run_steps(5);
        }

        expected[0][1] = json!(1.29);
        let tsv = as_tsv(&expected);
        h.get_by(|n| n.role() == Role::MultilineTextInput && n.value().is_none_or(|value| value.is_empty())).focus();
        h.run();
        h.event(egui::Event::Paste(tsv));
        h.run();
        h.get_by_label("Use Pasted Rows").click_accesskit();
        h.run();

        h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("1.29")).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        assert_eq!(temperature(h.state()), expected);
    }

    #[test]
    fn review_g7cr2_codes_added_row_in_mixed_column_keeps_list_through_ok() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| {
            rows[0]["columns"]["review_mixed"] = json!([11, 12]);
            rows.as_array_mut().unwrap().last_mut().unwrap()["columns"]["review_mixed"] = json!(15);
        });
        let mut expected = g7r2_entrance(&h);
        let rows = expected["tables"][0]["rows"].as_array_mut().unwrap();
        let row = rows.len();

        h.get_all_by_label("Add Row").next().unwrap().click_accesskit();
        h.run();
        for (header, text) in [
            ("Name", "T7 review"),
            ("Phases", "3"),
            ("Demand (kVA) over", "81.5"),
            ("Demand (kVA) up to", "100"),
            ("Breaker (A)", "225"),
            ("Review mixed", "3,4"),
        ] {
            review_type_at(&mut h, row, header, text);
        }
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        let after = g7r2_entrance(&h);
        let saved_rows = after["tables"][0]["rows"].as_array().unwrap();
        assert_eq!(&saved_rows[..row], rows.as_slice());
        assert_eq!(saved_rows[row]["name"], json!("T7 review"));
        assert_eq!(saved_rows[row]["columns"]["review_mixed"], json!([3, 4]));
    }

    // The review's cases (G7d).

    fn g7d_review_input<'a, S>(h: &'a Harness<'_, S>, value: &Value) -> egui_kittest::Node<'a> {
        // An empty cell shows nothing (its dash is a hint).
        let text = if value.is_null() { String::new() } else { cell_text(value) };
        h.query_all_by(move |n| n.role() == Role::TextInput && n.value().unwrap_or_default() == text)
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap_or_else(|| panic!("input for {value:?} missing"))
    }

    fn g7d_review_undo<S>(h: &mut Harness<'_, S>) {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(4);
    }

    fn g7d_review_replace<S>(h: &mut Harness<'_, S>, new: &str) {
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        if new.is_empty() {
            h.key_press(egui::Key::Backspace);
        } else {
            h.event(egui::Event::Text(new.into()));
        }
        h.run_steps(5);
    }

    #[test]
    fn review_g7d_codes_plain_text_with_comma_in_existing_null_and_added_rows() {
        let original = json!([{"name":"one","note":"old"},{"name":"two","note":null}]);
        let mut h = g7_table(original);
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        for (row, text) in [(0, "north, east"), (1, "south, west"), (2, "center, outside")] {
            review_type_at(&mut h, row, "Note", text);
        }
        assert_eq!(
            h.state().0,
            json!([
                {"name":"one","note":"north, east"},
                {"name":"two","note":"south, west"},
                {"name":null,"note":"center, outside"}
            ])
        );
    }

    #[test]
    fn review_g7d_codes_one_item_in_list_only_column_with_missing_last_cell() {
        let mut h = g7_table(json!([{"name":"one","values":[1]},{"name":"two"}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_cell_at(&h, 2, "Values").focus();
        h.run();
        review_append(&mut h, &["2", "5"]);
        assert_eq!(h.state().0[2]["values"], json!([25]));
    }

    fn g7d_review_seed_mixed(h: &mut Harness<'_, crate::BimApp>, last: Option<Value>) {
        g7r2_seed_entrance(h, |rows| {
            rows[0]["columns"]["audit_kind"] = json!([11, 12]);
            rows[1]["columns"]["audit_kind"] = json!(15);
            if let Some(value) = last {
                rows.as_array_mut().unwrap().last_mut().unwrap()["columns"]["audit_kind"] = value;
            }
        });
    }

    fn g7d_review_added_entrance(h: &mut Harness<'_, crate::BimApp>) -> usize {
        let row = g7r2_entrance(h)["tables"][0]["rows"].as_array().unwrap().len();
        h.get_all_by_label("Add Row").next().unwrap().click_accesskit();
        h.run();
        for (header, input) in
            [("Name", "Audit G7d"), ("Phases", "3"), ("Demand (kVA) over", "81.5"), ("Demand (kVA) up to", "100"), ("Breaker (A)", "225")]
        {
            review_type_at(h, row, header, input);
        }
        row
    }

    fn g7d_review_saved_added_entrance(h: &mut Harness<'_, crate::BimApp>, before: &Value, row: usize, expected: Value) {
        g7_ok(h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        let current = g7r2_entrance(h);
        let rows = current["tables"][0]["rows"].as_array().unwrap();
        assert_eq!(&rows[..row], before["tables"][0]["rows"].as_array().unwrap().as_slice());
        assert_eq!(rows[row]["name"], json!("Audit G7d"));
        assert_eq!(rows[row]["columns"]["audit_kind"], expected);
    }

    #[test]
    fn review_g7d_codes_mixed_added_after_missing_cell_single_event_list_through_ok() {
        let mut h = g7_dialog("energisa");
        g7d_review_seed_mixed(&mut h, None);
        let before = g7r2_entrance(&h);
        let row = g7d_review_added_entrance(&mut h);
        review_type_at(&mut h, row, "Audit kind", "3,4");
        g7d_review_saved_added_entrance(&mut h, &before, row, json!([3, 4]));
    }

    #[test]
    fn review_g7d_codes_mixed_added_after_scalar_starts_as_plain_then_accepts_brackets() {
        let mut h = g7_dialog("energisa");
        g7d_review_seed_mixed(&mut h, Some(json!(18)));
        let before = g7r2_entrance(&h);
        let row = g7d_review_added_entrance(&mut h);
        review_type_at(&mut h, row, "Audit kind", "25");
        let draft = &h.state().codes_dialog.as_ref().unwrap().tabs[2].rules["entrance"]["tables"][0]["rows"][row]["columns"]["audit_kind"];
        assert_eq!(draft, &json!(25));
        review_type_at(&mut h, row, "Audit kind", "[25]");
        g7d_review_saved_added_entrance(&mut h, &before, row, json!([25]));
    }

    #[test]
    fn review_g7d_codes_mixed_existing_scalar_clears_and_retypes_as_scalar_through_ok() {
        let mut h = g7_dialog("energisa");
        g7d_review_seed_mixed(&mut h, None);
        let mut expected = g7r2_entrance(&h);
        review_cell_at(&h, 1, "Audit kind").focus();
        g7d_review_replace(&mut h, "");
        review_append(&mut h, &["2", "5"]);
        expected["tables"][0]["rows"][1]["columns"]["audit_kind"] = json!(25);
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    fn g7d_review_dialog(discipline: Discipline, code: &str, section: &str, rule: &str) -> Harness<'static, crate::BimApp> {
        let mut session = Session::new();
        session.execute("building_code", &json!({"discipline": discipline.key(), "set": code})).unwrap();
        let mut app = crate::BimApp::new(session);
        g7d_review_open(&mut app, discipline, section, rule);
        Harness::builder().with_size([1800.0, 1600.0]).with_step_dt(0.25).build_ui_state(|ui, app| app.codes_window(ui.ctx()), app)
    }

    fn g7d_review_open(app: &mut crate::BimApp, discipline: Discipline, section: &str, rule: &str) {
        let mut dialog = crate::codes::CodesDialog::new(&app.session.model);
        dialog.tab = Discipline::ALL.iter().position(|d| *d == discipline).unwrap() + 1;
        dialog.tab_mut(discipline).section = section.into();
        dialog.table = Some((section.into(), rule.into()));
        app.codes_dialog = Some(dialog);
    }

    fn g7d_review_saved(h: &Harness<'_, crate::BimApp>, discipline: Discipline, section: &str, rule: &str) -> Value {
        h.state().session.model.resolve_code(discipline, section).value[rule].clone()
    }

    fn g7d_review_expect_saved(h: &Harness<'_, crate::BimApp>, discipline: Discipline, section: &str, rule: &str, expected: &Value) {
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        assert_eq!(&g7d_review_saved(h, discipline, section, rule), expected);
    }

    #[test]
    fn review_g7d_codes_temperature_slow_decimal_after_ok_then_undo_redo_and_reopen() {
        let mut h = g7d_review_dialog(Discipline::Electrical, "nbr-5410", "circuits", "temperature_air");
        let table = |h: &Harness<'_, crate::BimApp>| g7d_review_saved(h, Discipline::Electrical, "circuits", "temperature_air");
        let original = table(&h);
        g7d_review_input(&h, &original[0][1]).focus();
        g7d_review_replace(&mut h, "1");
        review_append(&mut h, &[".", "0", "0", "7"]);
        g7_ok(&mut h);
        let mut expected = original.clone();
        expected[0][1] = json!(1.007);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &expected);
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        assert_eq!(table(&h), original);
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(table(&h), expected);
        g7d_review_open(h.state_mut(), Discipline::Electrical, "circuits", "temperature_air");
        h.run();
        g7d_review_input(&h, &json!(1.007)).focus();
        h.run();
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &expected);
    }

    fn g7d_review_delete_at_y<S>(h: &mut Harness<'_, S>, y: f32) {
        h.get_all_by_label("×")
            .min_by(|a, b| (a.rect().center().y - y).abs().total_cmp(&(b.rect().center().y - y).abs()))
            .unwrap()
            .click_accesskit();
        h.run_steps(4);
    }

    #[test]
    fn review_g7d_codes_temperature_deleted_row_cannot_write_to_successor() {
        let mut h = g7d_review_dialog(Discipline::Electrical, "nbr-5410", "circuits", "temperature_air");
        let mut expected = g7d_review_saved(&h, Discipline::Electrical, "circuits", "temperature_air");
        let old = expected[0][1].clone();
        let successor = expected[1][1].clone();
        let input = g7d_review_input(&h, &old);
        let y = input.rect().center().y;
        input.focus();
        g7d_review_replace(&mut h, "1.26");
        g7d_review_replace(&mut h, "1.27");
        g7d_review_delete_at_y(&mut h, y);
        g7d_review_input(&h, &successor).focus();
        h.run();
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        expected.as_array_mut().unwrap().remove(0);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &expected);
    }

    #[test]
    fn review_g7d_codes_deleting_vent_card_cannot_undo_into_another_rule() {
        let mut h = g7d_review_dialog(Discipline::Plumbing, "nbr", "drainage", "vent_stacks");
        let mut expected = g7d_review_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks");
        let input = g7d_review_input(&h, &expected[0]["longest"][0][1]);
        let cell_center = input.rect().center();
        input.focus();
        g7d_review_replace(&mut h, "46.26");
        g7d_review_replace(&mut h, "46.27");
        let header = h.get_by(|n| n.role() == Role::Label && n.value().as_deref() == Some("#1")).parent().unwrap();
        header.get_by_label("×").click_accesskit();
        h.run_steps(4);
        assert_eq!(
            h.state().codes_dialog.as_ref().unwrap().tabs[4].rules["drainage"]["vent_stacks"][0],
            expected[1],
            "the first vent-stack row after deleting card #1"
        );
        h.get_all_by(|n| n.role() == Role::TextInput)
            .min_by(|a, b| a.rect().center().distance(cell_center).total_cmp(&b.rect().center().distance(cell_center)))
            .unwrap()
            .focus();
        h.run();
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        expected.as_array_mut().unwrap().remove(0);
        g7d_review_expect_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks", &expected);
    }

    #[test]
    fn review_g7d_codes_ragged_positional_rows_keep_their_own_values_and_undo() {
        let mut h = g7_table(json!([[11, 12, 13], [21], [31, [32, 33]], [41, 42]]));
        for (old, first, final_value) in [(12, "12.26", "12.27"), (21, "21.26", "21.27"), (32, "32.26", "32.27"), (42, "42.26", "42.27")] {
            g7d_review_input(&h, &json!(old)).focus();
            g7d_review_replace(&mut h, first);
            g7d_review_replace(&mut h, final_value);
        }
        g7d_review_undo(&mut h);
        assert_eq!(h.state().0, json!([[11, 12.27, 13], [21.27], [31, [32.27, 33]], [41, 42.26]]));
    }

    fn g7d_review_bundled_tables() -> Vec<(String, &'static Vec<Value>)> {
        fn walk<'a>(value: &'a Value, path: String, tables: &mut Vec<(String, &'a Vec<Value>)>) {
            if let Some(rows) = value.as_array() {
                if !rows.is_empty() && rows.iter().all(Value::is_array) {
                    tables.push((path, rows));
                } else {
                    for (i, row) in rows.iter().enumerate() {
                        walk(row, format!("{path}[{i}]"), tables);
                    }
                }
            } else if let Some(object) = value.as_object() {
                for (key, child) in object {
                    walk(child, format!("{path}.{key}"), tables);
                }
            }
        }

        let mut tables = Vec::new();
        for code in bimcraft_core::codes::codes() {
            for (section, value) in &code.sections {
                walk(value, format!("{}:{section}", code.code), &mut tables);
            }
        }
        tables
    }

    fn g7d_review_first_number(value: &mut Value) -> Option<&mut Value> {
        if value.is_array() {
            value.as_array_mut().unwrap().iter_mut().find_map(g7d_review_first_number)
        } else {
            value.is_number().then_some(value)
        }
    }

    #[test]
    fn review_g7d_codes_all_bundled_positional_tables_keep_slow_decimal_in_its_cell() {
        let tables = g7d_review_bundled_tables();
        let mut failures = Vec::new();
        for (path, rows) in &tables {
            let mut expected = json!(rows);
            let cell = g7d_review_first_number(&mut expected[0]).unwrap_or_else(|| panic!("no numeric cell in {path}"));
            let old = cell.clone();
            *cell = json!(123.007);
            let mut h = review_data_table(rows);
            g7d_review_input(&h, &old).focus();
            g7d_review_replace(&mut h, "123");
            review_append(&mut h, &[".", "0", "0", "7"]);
            if h.state().0 != expected {
                failures.push(format!("{path}: {:?} (expected {:?})", h.state().0[0], expected[0]));
            }
        }
        assert!(failures.is_empty(), "{} of {} tables:\n{}", failures.len(), tables.len(), failures.join("\n"));
    }

    fn g7d_review_focus_at_x(h: &Harness<'_, (Value, String)>, x: f32) {
        h.get_all_by(|n| n.role() == Role::TextInput)
            .filter(|n| (n.rect().left() - x).abs() < 6.0)
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap()
            .focus();
    }

    #[test]
    fn review_g7d_codes_bundled_positional_tables_discard_deleted_row_history() {
        let mut failures = Vec::new();
        let mut checked = 0;
        for (path, rows) in g7d_review_bundled_tables() {
            let first = flat(&rows[0]);
            let column = first.iter().position(Value::is_number).unwrap();
            if rows.len() < 2 || column >= flat(&rows[1]).len() {
                continue;
            }
            let mut h = review_data_table(rows);
            let target = g7d_review_input(&h, &first[column]);
            let x = target.rect().left();
            target.focus();
            g7d_review_replace(&mut h, "981.125");
            g7d_review_replace(&mut h, "981.375");
            let y = g7d_review_input(&h, &json!(981.375)).rect().center().y;
            g7d_review_delete_at_y(&mut h, y);
            g7d_review_focus_at_x(&h, x);
            h.run();
            g7d_review_undo(&mut h);
            let expected = json!(&rows[1..]);
            if h.state().0 != expected {
                failures.push(format!("{path}: first row {:?}, expected {:?}", h.state().0[0], expected[0]));
            }
            checked += 1;
        }
        assert!(failures.is_empty(), "{} of {checked} tables:\n{}", failures.len(), failures.join("\n"));
    }

    #[test]
    fn review_g7d_codes_bundled_positional_tables_discard_history_after_paste() {
        let mut failures = Vec::new();
        let mut checked = 0;
        for (path, rows) in g7d_review_bundled_tables() {
            if !rows.iter().all(|r| r.as_array().is_some_and(|a| a.len() >= 2 && a.iter().all(scalar))) {
                continue;
            }
            let mut h = review_data_table(rows);
            let mut expected = json!(rows);
            let cell = g7d_review_first_number(&mut expected[0]).unwrap();
            let old = cell.clone();
            *cell = json!(982.625);
            h.get_by(|n| n.role() == Role::MultilineTextInput && n.value().is_none_or(|v| v.is_empty())).focus();
            h.run();
            h.event(egui::Event::Paste(as_tsv(&expected)));
            h.run();
            g7d_review_input(&h, &old).focus();
            g7d_review_replace(&mut h, "981.125");
            g7d_review_replace(&mut h, "981.375");
            h.get_by_label("Use Pasted Rows").click_accesskit();
            h.run_steps(4);
            g7d_review_input(&h, &json!(982.625)).focus();
            h.run();
            g7d_review_undo(&mut h);
            if h.state().0 != expected {
                failures.push(format!("{path}: first row {:?}, expected {:?}", h.state().0[0], expected[0]));
            }
            checked += 1;
        }
        assert!(failures.is_empty(), "{} of {checked} tables:\n{}", failures.len(), failures.join("\n"));
    }

    #[test]
    fn review_g7d_codes_temperature_cancel_and_reopen_cannot_restore_draft() {
        let mut h = g7d_review_dialog(Discipline::Electrical, "nbr-5410", "circuits", "temperature_air");
        let original = g7d_review_saved(&h, Discipline::Electrical, "circuits", "temperature_air");
        g7d_review_input(&h, &original[0][1]).focus();
        g7d_review_replace(&mut h, "1.26");
        g7d_review_replace(&mut h, "1.27");
        h.get_by_label("Cancel").click_accesskit();
        h.run_steps(4);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &original);
        g7d_review_open(h.state_mut(), Discipline::Electrical, "circuits", "temperature_air");
        h.run();
        g7d_review_input(&h, &original[0][1]).focus();
        h.run();
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &original);
    }

    #[test]
    fn review_g7d_codes_temperature_copy_paste_and_undo_keeps_pasted_number_through_ok() {
        let mut h = g7d_review_dialog(Discipline::Electrical, "nbr-5410", "circuits", "temperature_air");
        let mut expected = g7d_review_saved(&h, Discipline::Electrical, "circuits", "temperature_air");
        expected[0][1] = json!(1.32);
        let tsv = review_g7c_copy_tsv(&mut g7_table(expected.clone()));
        h.get_by(|n| n.role() == Role::MultilineTextInput && n.value().is_none_or(|value| value.is_empty())).focus();
        h.run();
        h.event(egui::Event::Paste(tsv));
        h.run();
        g7d_review_input(&h, &json!(1.22)).focus();
        g7d_review_replace(&mut h, "1.26");
        g7d_review_replace(&mut h, "1.27");
        h.get_by_label("Use Pasted Rows").click_accesskit();
        h.run_steps(4);
        g7d_review_input(&h, &expected[0][1]).focus();
        h.run();
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &expected);
    }

    #[test]
    fn review_g7d_codes_positional_literal_dot_path_keeps_draft_in_its_own_table() {
        let mut h = g7_table(json!({"group":{"table":[[11,12]]},"group.table":[[21,22]]}));
        let literal_y = h
            .get_by(|n| n.label().is_some_and(|label| label.starts_with("Group Table ·") || label.starts_with("Group table ·")))
            .rect()
            .center()
            .y;
        let header = h
            .get_all_by_label("2")
            .min_by(|a, b| (a.rect().center().y - literal_y).abs().total_cmp(&(b.rect().center().y - literal_y).abs()))
            .unwrap()
            .rect();
        let input = h
            .get_all_by(|n| n.role() == Role::TextInput)
            .min_by(|a, b| (a.rect().left() - header.left()).abs().total_cmp(&(b.rect().left() - header.left()).abs()))
            .unwrap()
            .rect();
        let pos = egui::pos2(input.center().x, header.center().y + 23.0);
        h.event(egui::Event::PointerMoved(pos));
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.run();
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        g7d_review_replace(&mut h, "26");
        assert_eq!(h.state().0, json!({"group":{"table":[[11,12]]},"group.table":[[21,26]]}));
    }

    #[test]
    fn review_g7d_codes_positional_literal_dot_path_changes_only_its_own_rule_through_ok() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| {
            rows[0]["columns"]["audit_shape"] = json!({"group":{"table":[[11,12]]},"group.table":[[21,22]]});
        });
        let mut expected = g7r2_entrance(&h);
        expected["tables"][0]["rows"][0]["columns"]["audit_shape"]["group.table"][0][1] = json!(26);
        g7d_review_input(&h, &json!(22)).focus();
        g7d_review_replace(&mut h, "26");
        g7_ok(&mut h);
        let got = g7r2_entrance(&h);
        assert!(h.state().codes_dialog.is_none(), "{:?}", h.state().codes_dialog.as_ref().map(|d| &d.error));
        assert_eq!(got["tables"][0]["rows"][0]["columns"]["audit_shape"], expected["tables"][0]["rows"][0]["columns"]["audit_shape"]);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7d_codes_temperature_null_cell_accepts_number_without_erasing_display_dash() {
        let mut h = g7d_review_dialog(Discipline::Electrical, "nbr-5410", "circuits", "temperature_air");
        let mut expected = g7d_review_saved(&h, Discipline::Electrical, "circuits", "temperature_air");
        let row = expected.as_array().unwrap().iter().position(|row| row[1].is_null()).unwrap();
        // The empty cell beside that row's first one (other fields of the dialog may be empty too).
        let at = g7d_review_input(&h, &expected[row][0]).rect();
        h.query_all_by(|n| n.role() == Role::TextInput && n.value().unwrap_or_default().is_empty())
            .filter(|n| (n.rect().center().y - at.center().y).abs() < 2.0 && n.rect().left() > at.left())
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .unwrap()
            .focus();
        h.run();
        review_append(&mut h, &["0", ".", "4", "7"]);
        expected[row][1] = json!(0.47);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &expected);
    }

    // The confirmation's cases (G7d).
    // Include inside codes::tree::tests at 7cb22130.

    #[test]
    fn review_g7dr2_codes_card_and_collapsing_section_keep_separate_tables() {
        let mut h = g7_table(json!({
            "cards": [
                {"name": "card", "positional": [[11, 12]], "records": [{"value": 31}]}
            ],
            "section": {
                "positional": [[21, 22]], "records": [{"value": 41}]
            }
        }));
        g7d_review_input(&h, &json!(12)).focus();
        g7d_review_replace(&mut h, "12.");
        review_append(&mut h, &["0", "7"]);
        g7d_review_input(&h, &json!(41)).focus();
        g7d_review_replace(&mut h, "41.");
        review_append(&mut h, &["0", "9"]);

        assert_eq!(
            h.state().0,
            json!({
                "cards": [
                    {"name": "card", "positional": [[11, 12.07]], "records": [{"value": 31}]}
                ],
                "section": {
                    "positional": [[21, 22]], "records": [{"value": 41.09}]
                }
            })
        );
    }

    #[test]
    fn review_g7dr2_codes_horizontal_scroll_keeps_an_active_edit_in_its_cell() {
        for positional in [true, false] {
            let row = if positional {
                json!([101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112])
            } else {
                json!({"a":101, "b":102, "c":103, "d":104, "e":105, "f":106,
                       "g":107, "h":108, "i":109, "j":110, "k":111, "l":112})
            };
            let mut expected = json!([row]);
            let mut h = Harness::builder().with_size([410.0, 300.0]).with_step_dt(0.25).build_ui_state(
                |ui, (rows, pasted)| editor(ui, "scroll", "rows", rows, &Hints::default(), pasted),
                (expected.clone(), String::new()),
            );
            let pointer = g7d_review_input(&h, &json!(103)).rect().center();
            let initial_x = g7d_review_input(&h, &json!(104)).rect().center().x;
            g7d_review_input(&h, &json!(103)).focus();
            g7d_review_replace(&mut h, "103.");
            h.event(egui::Event::PointerMoved(pointer));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(-90.0, 0.0),
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(8);
            // Verify the wheel gesture actually exercised horizontal scrolling.
            let moved = h.get_by(|n| n.role() == Role::TextInput && n.value().as_deref() == Some("104"));
            assert!(moved.rect().center().x < initial_x - 20.0);
            review_append(&mut h, &["0", "7"]);
            if positional {
                expected[0][2] = json!(103.07);
            } else {
                expected[0]["c"] = json!(103.07);
            }
            assert_eq!(h.state().0, expected);
        }
    }

    #[test]
    fn review_g7dr2_codes_deleting_one_card_preserves_the_other_cards_active_edit_through_ok() {
        let mut h = g7d_review_dialog(Discipline::Plumbing, "nbr", "drainage", "vent_stacks");
        let mut expected = g7d_review_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks");
        g7d_review_input(&h, &expected[1]["longest"][0][1]).focus();
        g7d_review_replace(&mut h, "30.");
        review_append(&mut h, &["0", "7"]);
        h.get_by(|n| n.role() == Role::Label && n.value().as_deref() == Some("#1")).parent().unwrap().get_by_label("×").click_accesskit();
        h.run_steps(4);
        expected[1]["longest"][0][1] = json!(30.07);
        expected.as_array_mut().unwrap().remove(0);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks", &expected);
    }

    #[test]
    fn review_g7dr2_codes_two_different_dialogs_in_the_same_context_keep_separate_edits() {
        let mut h = g7d_review_dialog(Discipline::Plumbing, "nbr", "drainage", "vent_stacks");
        let plumbing = g7d_review_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks");
        g7d_review_input(&h, &plumbing[0]["longest"][0][1]).focus();
        g7d_review_replace(&mut h, "46.26");
        g7d_review_replace(&mut h, "46.27");
        h.get_by_label("Cancel").click_accesskit();
        h.run_steps(4);
        g7d_review_expect_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks", &plumbing);

        h.state_mut().session.execute("building_code", &json!({"discipline":"electrical", "set":"nbr-5410"})).unwrap();
        g7d_review_open(h.state_mut(), Discipline::Electrical, "circuits", "temperature_air");
        h.run_steps(4);
        let mut electrical = g7d_review_saved(&h, Discipline::Electrical, "circuits", "temperature_air");
        g7d_review_input(&h, &electrical[0][1]).focus();
        h.run();
        g7d_review_undo(&mut h);
        let electrical_tab =
            h.state().codes_dialog.as_ref().unwrap().tabs.iter().find(|tab| tab.discipline == Discipline::Electrical).unwrap();
        assert_eq!(electrical_tab.rules["circuits"]["temperature_air"], electrical);
        g7d_review_replace(&mut h, "1.007");
        electrical[0][1] = json!(1.007);
        g7_ok(&mut h);
        g7d_review_expect_saved(&h, Discipline::Electrical, "circuits", "temperature_air", &electrical);
        g7d_review_expect_saved(&h, Discipline::Plumbing, "drainage", "vent_stacks", &plumbing);
    }

    // G7e: how a list is typed (the reviews' cases).

    #[test]
    fn review_g7c_codes_appending_does_not_round_an_existing_number() {
        let original = 9_007_199_254_740_991.0;
        let mut h = g7_table(json!([{"name":"one","list":[original]}]));
        g7_cell(&h, "one", "List").focus();
        h.run();
        review_append(&mut h, &[", final"]);
        assert_eq!(h.state().0, json!([{"name":"one","list":[original,"final"]}]));
    }

    #[test]
    fn review_g7d_codes_one_item_in_list_only_column_after_explicit_null() {
        let mut h = g7_table(json!([{"name":"one","values":[1]},{"name":"two","values":null}]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_type_at(&mut h, 2, "Values", "25");
        assert_eq!(h.state().0[2]["values"], json!([25]));
    }

    #[test]
    fn review_g7d_codes_mixed_added_after_list_keeps_plain_number_through_ok() {
        let mut h = g7_dialog("energisa");
        g7d_review_seed_mixed(&mut h, Some(json!([18, 19])));
        let before = g7r2_entrance(&h);
        let row = g7d_review_added_entrance(&mut h);
        review_type_at(&mut h, row, "Audit kind", "25");
        g7d_review_saved_added_entrance(&mut h, &before, row, json!(25));
    }

    #[test]
    fn review_g7d_codes_mixed_added_after_missing_cell_slow_comma_list_through_ok() {
        let mut h = g7_dialog("energisa");
        g7d_review_seed_mixed(&mut h, None);
        let before = g7r2_entrance(&h);
        let row = g7d_review_added_entrance(&mut h);
        review_cell_at(&h, row, "Audit kind").focus();
        h.run();
        review_append(&mut h, &["3", ",", "4"]);
        g7d_review_saved_added_entrance(&mut h, &before, row, json!([3, 4]));
    }

    // The review's cases (G7e).
    // Paste inside codes::tree::tests. Uses the helpers already in that module.

    #[test]
    fn review_g7e_codes_same_length_middle_edit_keeps_unchanged_duplicate_numbers() {
        let number = json!(9_007_199_254_740_991.0);
        let mut h = g7_table(json!([{"name":"one","values":[number, "middle", number]}]));
        g7_type(&mut h, "one", "Values", &format!("{number}, changed, {number}"));
        assert_eq!(h.state().0, json!([{"name":"one","values":[number, "changed", number]}]));
    }

    #[test]
    fn review_g7e_codes_only_other_null_does_not_choose_kind() {
        let mut h = g7_table(json!([{"name":"one","values":null},{"name":"two","values":[]}]));
        g7_type(&mut h, "two", "Values", "25");
        assert_eq!(h.state().0, json!([{"name":"one","values":null},{"name":"two","values":25}]));
    }

    #[test]
    fn review_g7e_codes_only_other_null_allows_list_as_written() {
        let mut h = g7_table(json!([{"name":"one","values":null},{"name":"two","values":null}]));
        g7_type(&mut h, "two", "Values", "2, 5");
        assert_eq!(h.state().0, json!([{"name":"one","values":null},{"name":"two","values":[2, 5]}]));
    }

    #[test]
    fn review_g7e_codes_other_empty_list_does_not_choose_kind() {
        let mut h = g7_table(json!([{"name":"one","values":[]},{"name":"two","values":null}]));
        g7_type(&mut h, "two", "Values", "25");
        assert_eq!(h.state().0, json!([{"name":"one","values":[]},{"name":"two","values":25}]));
    }

    #[test]
    fn review_g7e_codes_other_empty_list_does_not_make_plain_column_mixed() {
        let mut h = g7_table(json!([{"name":"one","values":[]},{"name":"two","values":1},{"name":"three","values":null}]));
        g7_type(&mut h, "three", "Values", "2, 5");
        assert_eq!(h.state().0, json!([{"name":"one","values":[]},{"name":"two","values":1},{"name":"three","values":"2, 5"}]));
    }

    #[test]
    fn review_g7e_codes_refocusing_uses_kind_committed_on_leaving_through_ok() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| {
            let i = rows.as_array().unwrap().iter().position(|r| r["name"] == "T4").unwrap();
            rows[i]["columns"]["audit_kind"] = Value::Null;
        });
        let mut expected = g7r2_entrance(&h);
        let i = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Audit kind", "3");
        g7_cell(&h, "T4", "Name").focus();
        h.run_steps(3);
        g7_cell(&h, "T4", "Audit kind").focus();
        h.run();
        review_append(&mut h, &[",", " 4"]);
        expected["tables"][0]["rows"][i]["columns"]["audit_kind"] = json!("3, 4");
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7e_codes_undo_within_edit_restores_plain_number_through_ok() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| {
            let i = rows.as_array().unwrap().iter().position(|r| r["name"] == "T4").unwrap();
            rows[i]["columns"]["audit_kind"] = Value::Null;
        });
        let mut expected = g7r2_entrance(&h);
        let i = g7r2_t4(&expected);
        g7_type(&mut h, "T4", "Audit kind", "3");
        h.run_steps(5);
        review_append(&mut h, &[", 4"]);
        h.run_steps(5);
        let rows = &h
            .state()
            .codes_dialog
            .as_ref()
            .unwrap()
            .tabs
            .iter()
            .find(|tab| tab.discipline == Discipline::Electrical)
            .unwrap()
            .rules["entrance"]["tables"][0]["rows"];
        assert_eq!(rows[i]["columns"]["audit_kind"], json!([3, 4]));
        g7d_review_undo(&mut h);
        expected["tables"][0]["rows"][i]["columns"]["audit_kind"] = json!(3);
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7e_codes_undo_after_refocusing_restores_original_text_through_ok() {
        let mut h = g7_dialog("energisa");
        g7r2_seed_entrance(&mut h, |rows| {
            let i = rows.as_array().unwrap().iter().position(|r| r["name"] == "T4").unwrap();
            rows[i]["columns"]["audit_kind"] = json!("alpha, beta");
        });
        let expected = g7r2_entrance(&h);
        g7_cell(&h, "T4", "Audit kind").focus();
        h.run_steps(5);
        g7d_review_replace(&mut h, "");
        g7_cell(&h, "T4", "Name").focus();
        h.run_steps(3);
        g7_cell(&h, "T4", "Audit kind").focus();
        h.run_steps(3);
        g7d_review_undo(&mut h);
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }

    #[test]
    fn review_g7e_codes_undo_of_first_edit_restores_empty_list() {
        let mut h = g7_table(json!([{"name":"one","values":[]},{"name":"two","values":1}]));
        g7_cell(&h, "one", "Values").focus();
        h.run_steps(5);
        review_append(&mut h, &["3"]);
        h.run_steps(5);
        g7d_review_undo(&mut h);
        assert_eq!(h.state().0, json!([{"name":"one","values":[]},{"name":"two","values":1}]));
    }

    #[test]
    fn review_g7e_codes_deleted_row_does_not_give_successor_its_kind() {
        let mut h = g7_table(json!([{"name":"old","values":[11,12]},{"name":"next","values":"plain"}]));
        g7_type(&mut h, "old", "Values", "31, 32");
        h.get_all_by_label("×").next().unwrap().click_accesskit();
        h.run();
        g7_type(&mut h, "next", "Values", "3, 4");
        assert_eq!(h.state().0, json!([{"name":"next","values":"3, 4"}]));
    }

    #[test]
    fn review_g7e_codes_pasted_row_does_not_inherit_old_plain_kind() {
        let mut h = g7_table(json!([{"name":"old","values":"plain"}]));
        h.state_mut().1 = "name\tvalues\nreplacement\t[17,23]\n".into();
        h.run();
        g7_type(&mut h, "old", "Values", "91, 92");
        h.get_by_label("Use Pasted Rows").click_accesskit();
        h.run();
        g7_cell(&h, "replacement", "Values").focus();
        h.run();
        review_append(&mut h, &[", 4"]);
        assert_eq!(h.state().0, json!([{"name":"replacement","values":[17, 23, 4]}]));
    }

    #[test]
    fn review_g7e_codes_add_row_preserves_positional_places() {
        let mut h = g7_table(json!([[1, [2, 3], 4]]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        review_type_at(&mut h, 1, "3", "25");
        assert_eq!(h.state().0, json!([[1, [2, 3], 4], [null, [null, 25], null]]));
    }

    #[test]
    fn review_g7e_codes_add_card_keeps_the_shape_of_nested_rows_and_lists() {
        let original = json!({"name":"card","rows":[{"name":"inner","limit":12,"options":[1,2]}],"values":[3,4]});
        let mut h = g7_table(json!([original]));
        h.get_all_by_label("Add Row").max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click_accesskit();
        h.run();
        assert_eq!(
            h.state().0,
            json!([original,{"name":null,"rows":[{"name":null,"limit":null,"options":[null,null]}],"values":[null,null]}])
        );
    }

    // The confirmation's cases (G7e).
    // Insert inside codes::tree::tests.

    #[test]
    fn review_g7er2_codes_add_row_restarts_original_value_for_existing_cell() {
        let mut h = g7_dialog("energisa");
        let i = g7r2_t4(&g7r2_entrance(&h));
        g7r2_seed_entrance(&mut h, |rows| rows[i]["columns"]["audit_kind"] = json!("7"));
        g7_type(&mut h, "T4", "Audit kind", "8");
        let added = g7d_review_added_entrance(&mut h);
        g7_type(&mut h, "T4", "Audit kind", "7");
        g7_ok(&mut h);
        assert!(h.state().codes_dialog.is_none());
        let saved = g7r2_entrance(&h);
        assert_eq!(saved["tables"][0]["rows"][added]["name"], json!("Audit G7d"));
        assert_eq!(saved["tables"][0]["rows"][i]["name"], json!("T4"));
        assert_eq!(saved["tables"][0]["rows"][i]["columns"]["audit_kind"], json!(7));
    }

    #[test]
    fn review_g7er2_codes_delete_row_keeps_edits_and_restarts_shifted_cells() {
        let mut h = g7_table(json!([{"name":"old","value":"7"},{"name":"next","value":"7"}]));
        g7_type(&mut h, "old", "Value", "9");
        g7_type(&mut h, "next", "Value", "8");
        h.get_all_by_label("×").next().unwrap().click_accesskit();
        h.run();
        assert_eq!(h.state().0, json!([{"name":"next","value":8}]));
        g7_type(&mut h, "next", "Value", "7");
        assert_eq!(h.state().0, json!([{"name":"next","value":7}]));
    }

    #[test]
    fn review_g7er2_codes_first_edit_after_paste_can_restore_the_pasted_number() {
        let mut h = g7_table(json!([{"name":"old","value":"7"}]));
        g7_type(&mut h, "old", "Value", "8");
        review_g7c_paste_tsv(&mut h, "name\tvalue\nreplacement\t7\n");
        assert_eq!(h.state().0, json!([{"name":"replacement","value":7}]));
        g7_type(&mut h, "replacement", "Value", "9");
        g7_cell(&h, "replacement", "Name").focus();
        h.run_steps(3);
        g7_type(&mut h, "replacement", "Value", "7");
        assert_eq!(h.state().0, json!([{"name":"replacement","value":7}]));
    }

    #[test]
    fn review_g7er2_codes_add_object_row_blanks_its_nested_plain_list() {
        let original = json!({"name":"one","details":{"values":[1,2]}});
        let mut h = g7_table(json!([original]));
        h.get_by_label("Add Row").click_accesskit();
        h.run();
        assert_eq!(h.state().0, json!([original,{"name":null,"details":{"values":[]}}]));
    }

    #[test]
    fn review_g7er2_codes_cancel_discards_first_value_before_reopening() {
        let mut h = g7_dialog("energisa");
        let i = g7r2_t4(&g7r2_entrance(&h));
        g7r2_seed_entrance(&mut h, |rows| rows[i]["columns"]["audit_kind"] = json!("7"));
        let original = g7r2_entrance(&h);
        g7_type(&mut h, "T4", "Audit kind", "9");
        h.get_by_label("Cancel").click_accesskit();
        h.run();
        review_expect_entrance(&h, &original);

        // Another project edit while the dialog is closed distinguishes old and new histories.
        let app = h.state_mut();
        let mut fresh = crate::codes::CodesDialog::new(&app.session.model);
        let tab = fresh.tab_mut(Discipline::Electrical);
        tab.rules.get_mut("entrance").unwrap()["tables"][0]["rows"][i]["columns"]["audit_kind"] = json!(8);
        app.session.execute("building_code", &tab.params(&app.session.model)).unwrap();
        let mut expected = g7r2_entrance(&h);
        expected["tables"][0]["rows"][i]["columns"]["audit_kind"] = json!(7);

        review_open_entrance(&mut h);
        g7_type(&mut h, "T4", "Audit kind", "7");
        g7_ok(&mut h);
        review_expect_entrance(&h, &expected);
    }
}
