//! The steel list and the cut plan (BL, CU): `steel_list` and `cut_plan` read the project's bars and change nothing in it. Both
//! take the bars of a level, of some hosts or bars, or of the whole project, and may write what they return as a .csv or .xlsx file.

use super::*;
use crate::steel::{self, Item, Piece, Scope, Stock};

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The bars a call is about: those on `level`, and those among `ids` (a host stands for its bars); the whole project where it
/// gives neither.
fn scope(c: &str, m: &Model, p: &Value) -> Result<Scope, String> {
    let level = if absent(p.get("level")) { None } else { Some(level_param(c, m, p, "level")?) };
    let chosen = if absent(p.get("ids")) && absent(p.get("id")) { None } else { Some(ids_param(c, m, p)?.into_iter().collect()) };
    Ok(Scope { level, only: chosen })
}

/// `stock_length` in mm, if given.
fn stock_length(c: &str, p: &Value) -> Result<Option<f64>, String> {
    if absent(p.get("stock_length")) {
        return Ok(None);
    }
    within(c, p, "stock_length", 0.0, 100.0, 1_000_000.0).map(Some)
}

/// The bars of each grade and diameter (to a hundredth of a millimetre), the thinnest first.
fn by_size<'a>(items: &'a [Item<'a>]) -> BTreeMap<(String, i64), Vec<&'a Item<'a>>> {
    let mut out: BTreeMap<(String, i64), Vec<&Item>> = BTreeMap::new();
    for item in items {
        out.entry((item.bar.grade.clone(), (item.bar.diameter * 100.0).round() as i64)).or_default().push(item);
    }
    out
}

fn pieces<'a>(m: &Model, items: impl IntoIterator<Item = &'a Item<'a>>) -> Vec<Piece> {
    items
        .into_iter()
        .map(|i| Piece { bar: i.id, host_mark: steel::host_mark(m, i.bar), mark: steel::mark_text(i.number), length: i.length })
        .collect()
}

/// What a piece is called in a note: its mark, and its host's when it has one.
fn called(p: &Piece) -> String {
    if p.host_mark.is_empty() { p.mark.clone() } else { format!("{} of {}", p.mark, p.host_mark) }
}

fn too_long(p: &Piece, stock: f64) -> String {
    format!(
        "{} is {} mm, longer than the {} mm stock bar: a lap or a longer bar is the engineer's call",
        called(p),
        r2(p.length),
        r2(stock)
    )
}

fn write_table(c: &str, path: &str, t: &schedule::Table, sheet: &str) -> Result<(), String> {
    let bytes = if path.to_lowercase().ends_with(".xlsx") { schedule::to_xlsx(t, sheet) } else { schedule::to_csv(t).into_bytes() };
    std::fs::write(path, bytes).map_err(|e| bad(c, format!("{path}: {e}")))
}

fn column(heading: &str, kind: schedule::Kind) -> schedule::Column {
    schedule::Column { heading: heading.to_string(), kind }
}

/// `steel_list`: every bar's mark, shape, count, length and weight, by host and mark, with the summary by grade and diameter.
pub(super) fn steel_list(s: &mut Session, p: &Value) -> R {
    let c = "steel_list";
    only(c, p, &["level", "ids", "id", "coil_to", "stock_length", "path"])?;
    let m = &s.model;
    let of = scope(c, m, p)?;
    let coil_to = within(c, p, "coil_to", 0.0, 0.0, 100.0)?;
    let stock = stock_length(c, p)?;
    let bars = steel::Bars::new(m);
    let items = steel::items(m, &bars, &of);
    let rows = steel::rows(m, &items).map_err(|e| bad(c, e))?;
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "host": r.host, "host_mark": r.host_mark, "level": r.level, "mark": r.mark, "grade": r.grade, "diameter": r.diameter,
                "shape": r.shape, "count": r.count, "length": r2(r.length), "total_length": r2(r.total_length),
                "weight_kg": r3(r.weight_kg), "bars": r.bars,
            })
        })
        .collect();
    let (mut summary, mut notes) = (Vec::new(), Vec::new());
    for group in by_size(&items).values() {
        let Some(first) = group.first() else { continue };
        let coil = coil_to > 0.0 && first.bar.diameter <= coil_to;
        let length: f64 = group.iter().map(|i| i.length).sum();
        let weight: f64 = group.iter().map(|i| rebar::weight_kg(i.bar.diameter, i.length)).sum();
        let mut line = json!({
            "grade": first.bar.grade, "diameter": first.bar.diameter, "form": if coil { "coil" } else { "bar" },
            "count": group.len(), "total_length": r2(length), "weight_kg": r3(weight),
        });
        if let (Some(length), false) = (stock, coil) {
            let packed = steel::pack(pieces(m, group.iter().copied()), &Stock { length, kerf: 0.0, min_offcut: 0.0 });
            if let Some(line) = line.as_object_mut() {
                line.insert("stock_bars".into(), json!(packed.stock_bars));
            }
            notes.extend(packed.too_long.iter().take(rebar::MAX_NOTES).map(|p| too_long(p, length)));
        }
        summary.push(line);
    }
    notes.truncate(rebar::MAX_NOTES);
    let weight: f64 = rows.iter().map(|r| r.weight_kg).sum();
    let total_length: f64 = rows.iter().map(|r| r.total_length).sum();
    let mut reply = json!({
        "rows": list, "summary": summary,
        "totals": {"bars": items.len(), "rows": rows.len(), "total_length": r2(total_length), "weight_kg": r3(weight)},
        "notes": notes,
    });
    if let Some(path) = string(c, p, "path")? {
        use schedule::Kind as K;
        let columns = [
            ("Host Mark", K::Text),
            ("Mark", K::Text),
            ("Diameter", K::Length),
            ("Grade", K::Text),
            ("Shape", K::Text),
            ("Count", K::Number),
            ("Length", K::Length),
            ("Total Length", K::Length),
            ("Weight", K::Number),
        ];
        let table = schedule::Table {
            columns: columns.iter().map(|(h, k)| column(h, *k)).collect(),
            rows: rows
                .iter()
                .map(|r| {
                    let text = |t: &str| if t.is_empty() { Value::Null } else { json!(t) };
                    vec![
                        text(&r.host_mark),
                        json!(r.mark),
                        json!(r.diameter),
                        json!(r.grade),
                        json!(r.shape),
                        json!(r.count),
                        json!(r2(r.length)),
                        json!(r2(r.total_length)),
                        json!(r3(r.weight_kg)),
                    ]
                })
                .collect(),
            ids: rows.iter().map(|r| r.bars.clone()).collect(),
            totals: Some(vec![
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                json!(items.len()),
                Value::Null,
                json!(r2(total_length)),
                json!(r3(weight)),
            ]),
        };
        write_table(c, &path, &table, "Steel List")?;
        if let Some(reply) = reply.as_object_mut() {
            reply.insert("path".into(), json!(path));
        }
    }
    Ok(reply)
}

/// `cut_plan`: how many stock bars of `stock_length` to buy for the bars, which pieces go in which, and what is left over.
pub(super) fn cut_plan(s: &mut Session, p: &Value) -> R {
    let c = "cut_plan";
    only(c, p, &["stock_length", "level", "ids", "id", "kerf", "min_offcut", "coil_to", "path"])?;
    let m = &s.model;
    let length =
        stock_length(c, p)?.ok_or_else(|| bad(c, "`stock_length` (mm) is required: it is a purchase, and the engineer says it"))?;
    let kerf = within(c, p, "kerf", 0.0, 0.0, 100.0)?;
    let min_offcut = within(c, p, "min_offcut", 0.0, 0.0, length)?;
    let coil_to = within(c, p, "coil_to", 0.0, 0.0, 100.0)?;
    let of = scope(c, m, p)?;
    let bars = steel::Bars::new(m);
    let items = steel::items(m, &bars, &of);
    if items.len() > steel::MAX_PIECES {
        return Err(bad(
            c,
            format!("{} bars, more than the {} pieces a cut plan packs: ask for one level or some hosts", items.len(), steel::MAX_PIECES),
        ));
    }
    let stock = Stock { length, kerf, min_offcut };
    let (mut diameters, mut coil, mut notes, mut table_rows) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut stock_bars, mut used, mut bought, mut wasted) = (0usize, 0.0, 0.0, 0.0);
    for group in by_size(&items).values() {
        let Some(first) = group.first() else { continue };
        let (grade, diameter) = (first.bar.grade.clone(), first.bar.diameter);
        if coil_to > 0.0 && diameter <= coil_to {
            let total: f64 = group.iter().map(|i| i.length).sum();
            coil.push(json!({
                "grade": grade, "diameter": diameter, "pieces": group.len(), "total_length": r2(total),
                "weight_kg": r3(rebar::weight_kg(diameter, total)),
            }));
            continue;
        }
        let packed = steel::pack(pieces(m, group.iter().copied()), &stock);
        let bought_mm = length * packed.stock_bars as f64;
        let percent = if bought_mm > 0.0 { packed.waste / bought_mm * 100.0 } else { 0.0 };
        let reusable: f64 = packed.reusable.iter().sum();
        let mut note = format!("{diameter} mm {grade}: {} stock bars, {percent:.1} % waste", packed.stock_bars);
        if !packed.reusable.is_empty() {
            note.push_str(&format!(", {} offcuts of at least {} mm", packed.reusable.len(), r2(min_offcut)));
        }
        notes.push(note);
        notes.extend(packed.too_long.iter().map(|p| too_long(p, length)));
        let listed = packed.patterns.len().min(steel::MAX_PATTERNS);
        if packed.patterns.len() > listed {
            notes.push(format!("{diameter} mm {grade}: {} patterns, the first {listed} listed", packed.patterns.len()));
        }
        let patterns: Vec<Value> = packed
            .patterns
            .iter()
            .take(listed)
            .map(|pat| {
                let pieces: Vec<Value> = pat
                    .pieces
                    .iter()
                    .map(|(host, mark, len, ids)| json!({"host_mark": host, "mark": mark, "length": r2(*len), "ids": ids}))
                    .collect();
                table_rows.push((grade.clone(), diameter, pat.count, pat.pieces.clone(), pat.offcut));
                json!({"count": pat.count, "pieces": pieces, "offcut": r2(pat.offcut)})
            })
            .collect();
        diameters.push(json!({
            "grade": grade, "diameter": diameter, "form": "bar", "stock_bars": packed.stock_bars,
            "pieces": group.len() - packed.too_long.len(), "used_mm": r2(packed.used), "waste_mm": r2(packed.waste),
            "waste_percent": r2(percent), "bought_kg": r3(rebar::weight_kg(diameter, bought_mm)),
            "used_kg": r3(rebar::weight_kg(diameter, packed.used)),
            "reusable_offcuts": {"count": packed.reusable.len(), "total_mm": r2(reusable)}, "patterns": patterns,
        }));
        stock_bars += packed.stock_bars;
        used += rebar::weight_kg(diameter, packed.used);
        bought += rebar::weight_kg(diameter, bought_mm);
        wasted += packed.waste;
    }
    notes.truncate(rebar::MAX_NOTES);
    let mut reply = json!({
        "stock_length": length, "kerf": kerf, "diameters": diameters, "coil": coil,
        "totals": {"stock_bars": stock_bars, "bought_kg": r3(bought), "used_kg": r3(used), "waste_mm": r2(wasted)},
        "notes": notes,
    });
    if let Some(path) = string(c, p, "path")? {
        use schedule::Kind as K;
        let table =
            schedule::Table {
                columns: vec![
                    column("Grade", K::Text),
                    column("Diameter", K::Length),
                    column("Stock Bars", K::Number),
                    column("Pieces", K::Text),
                    column("Offcut", K::Length),
                ],
                rows: table_rows
                    .iter()
                    .map(|(grade, diameter, count, held, offcut)| {
                        let text: Vec<String> =
                            held.iter()
                                .map(|(host, mark, len, _)| {
                                    if host.is_empty() { format!("{mark} {}", r2(*len)) } else { format!("{mark} of {host} {}", r2(*len)) }
                                })
                                .collect();
                        vec![json!(grade), json!(diameter), json!(count), json!(text.join(" + ")), json!(r2(*offcut))]
                    })
                    .collect(),
                ids: table_rows.iter().map(|(.., held, _)| held.iter().flat_map(|(.., ids)| ids.iter().copied()).collect()).collect(),
                totals: None,
            };
        write_table(c, &path, &table, "Cut Plan")?;
        if let Some(reply) = reply.as_object_mut() {
            reply.insert("path".into(), json!(path));
        }
    }
    Ok(reply)
}
