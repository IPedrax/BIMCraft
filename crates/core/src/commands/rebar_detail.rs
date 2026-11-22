//! `rebar_detail`: makes, or changes, the detail drawings of reinforced beams, columns, footings and floors. A detail holds only the
//! member, a scale, where its sections are cut and whether its steel list is in; what it shows is worked out from the beam and its bars each time it is drawn
//! ([`crate::rebar_detail`]), so the reply's sets are what the drawing says.

use std::collections::HashMap;

use super::*;
use crate::model::{Guid, Id};
use crate::rebar_detail::{self, MAX_DETAILS, MAX_STATIONS, RebarDetail, SCALE};
use crate::reinforce::{Kind, member_frame};
use crate::steel::{self, Bars};

/// The scale a detail is made at, 1 : n.
const SCALE_DEFAULT: f64 = 50.0;

/// What a call asks for, besides which beams.
struct Ask {
    stations: Option<Vec<f64>>,
    scale: Option<f64>,
    list: Option<bool>,
    replace: bool,
}

fn ask(c: &str, p: &Value) -> Result<Ask, String> {
    let stations = match p.get("stations") {
        v if absent(v) => None,
        Some(Value::Array(a)) if a.len() <= MAX_STATIONS => Some(
            a.iter()
                .map(|x| {
                    x.as_f64()
                        .filter(|x| x.is_finite() && (0.0..=MAX_COORD).contains(x))
                        .ok_or_else(|| bad(c, format!("a station must be a length along the member, mm, got {x}")))
                })
                .collect::<Result<Vec<f64>, String>>()?,
        ),
        Some(Value::Array(a)) => {
            return Err(bad(c, format!("`stations` lists {} sections, more than the {MAX_STATIONS} a detail cuts", a.len())));
        }
        Some(v) => return Err(bad(c, format!("`stations` must be a list of lengths along the member, got {v}"))),
        None => None,
    };
    let scale = num(c, p, "scale")?;
    if let Some(x) = scale.filter(|x| !(x.is_finite() && SCALE.contains(x))) {
        return Err(bad(c, format!("`scale` is {} to {}, not {x}", SCALE.start(), SCALE.end())));
    }
    let list = match p.get("list") {
        v if absent(v) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(v) => return Err(bad(c, format!("`list` must be true or false, got {v}"))),
        None => None,
    };
    Ok(Ask { stations, scale, list, replace: flag(c, p, "replace")? })
}

/// What detail `id` says, as the reply gives it.
fn json_of(m: &Model, bars: &Bars, id: Id) -> Value {
    let Some(Element::RebarDetail(rd)) = m.elements.get(id) else { return Value::Null };
    let host = m.elements.id(rd.host);
    let located = m.elements.get(rd.host).and_then(|e| member_frame(m, e));
    let sets = host.map(|h| rebar_detail::sets(m, bars, h)).unwrap_or_default();
    let stations = match (&located, rd.stations.is_empty()) {
        (Some(b), true) => rebar_detail::default_stations(m, &sets, b),
        _ => rd.stations.clone(),
    };
    let rows: Vec<Value> = sets
        .iter()
        .map(|st| {
            let spread = located.as_ref().is_some_and(|b| rebar_detail::spread_in(st, b));
            json!({
                "mark": steel::mark_text(st.mark), "layer": st.layer, "diameter": st.diameter, "grade": st.grade, "count": st.count,
                "spacing": st.spacing, "length": st.length, "tag": rebar_detail::tag_text(m, st, spread), "ids": st.bars,
            })
        })
        .collect();
    json!({"id": id, "name": rd.name, "host": host, "scale": rd.scale, "stations": stations, "list": rd.list, "sets": rows})
}

pub(super) fn rebar_detail(s: &mut Session, p: &Value) -> R {
    let c = "rebar_detail";
    only(c, p, &["host", "hosts", "stations", "scale", "list", "sheet", "replace"])?;
    let want = ask(c, p)?;
    let m = &s.model;
    let hosts: Vec<Id> = match (p.get("hosts"), p.get("host")) {
        (Some(Value::Array(a)), _) if a.len() <= rebar::MAX_HOSTS => {
            let mut seen = std::collections::BTreeSet::new();
            let mut ids = Vec::new();
            for v in a {
                let id = element_id(c, m, v)?;
                if seen.insert(id) {
                    ids.push(id);
                }
            }
            ids
        }
        (Some(Value::Array(_)), _) => return Err(bad(c, format!("`hosts` lists more than {} members", rebar::MAX_HOSTS))),
        (v, Some(one)) if absent(v) => vec![element_id(c, m, one)?],
        _ => return Err(bad(c, "`host`, a member's id, or `hosts`, a list of them, is required")),
    };
    let bars = Bars::new(m);
    let held: HashMap<Guid, Id> =
        m.elements.iter().filter_map(|(id, e)| if let Element::RebarDetail(rd) = e { Some((rd.host, *id)) } else { None }).collect();
    // Every member is checked before any detail is made, so a refusal leaves nothing half done.
    let mut fresh = 0;
    for host in &hosts {
        let Some(e) = m.elements.get(*host) else { return Err(bad(c, format!("no element {host}"))) };
        if !matches!(e, Element::Beam(_) | Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Floor(_)) {
            return Err(bad(c, format!("element {host} is a {}; only a beam, a column, a footing or a floor has a detail", e.category())));
        }
        let b = member_frame(m, e).ok_or_else(|| bad(c, format!("{} {host} has no type or no outline to draw", e.category())))?;
        if rebar_detail::sets(m, &bars, *host).is_empty() {
            return Err(bad(c, format!("{} {host} has no bars: reinforce it first (`reinforce`)", e.category())));
        }
        if want.stations.as_ref().is_some_and(|x| !x.is_empty()) && matches!(b.kind, Kind::Footing | Kind::Floor) {
            return Err(bad(
                c,
                format!("{} {host} takes no stations: its detail has a plan and, for a footing, one section", e.category()),
            ));
        }
        if let Some(x) = want.stations.iter().flatten().find(|x| **x > b.length) {
            return Err(bad(
                c,
                format!("a station of {x} mm is past the end of {} {host}, which is {} mm long", e.category(), b.length.round()),
            ));
        }
        if !held.contains_key(&m.guid(*host)) {
            fresh += 1;
        }
    }
    if held.len() + fresh > MAX_DETAILS {
        return Err(bad(c, format!("a project holds no more than {MAX_DETAILS} details")));
    }
    let mut views = Vec::new();
    let mut viewports = Vec::new();
    for host in hosts {
        let guid = s.model.guid(host);
        let id = match held.get(&guid) {
            Some(id) => {
                let Some(Element::RebarDetail(old)) = s.model.elements.get(*id) else { return Err(bad(c, "a detail went missing")) };
                let mut rd = old.clone();
                if want.replace {
                    rd = RebarDetail::new(rd.name, guid, SCALE_DEFAULT);
                }
                apply(&mut rd, &want);
                s.model_mut().elements.insert(*id, Element::RebarDetail(rd));
                *id
            }
            None => {
                let m = &s.model;
                let mark = m
                    .elements
                    .get(host)
                    .and_then(Element::params)
                    .and_then(|v| v.get("Mark"))
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty());
                let label = mark.map_or_else(|| host.to_string(), str::to_string);
                let kind = match m.elements.get(host).and_then(|e| member_frame(m, e)).map(|b| b.kind) {
                    Some(Kind::Column) => m.language.word("Column"),
                    Some(Kind::Footing) => m.language.word("Footing"),
                    Some(Kind::Floor) => m.language.word("Floor"),
                    _ => m.language.word("Beam"),
                };
                let name = m.free_view_name(&format!("{} {} {}", kind, label, m.language.word("Detail")), false);
                let mut rd = RebarDetail::new(name, guid, SCALE_DEFAULT);
                apply(&mut rd, &want);
                s.model_mut().insert(Element::RebarDetail(rd))
            }
        };
        if let Some(sheet) = p.get("sheet").filter(|v| !v.is_null()) {
            viewports.push(place(s, &json!({"sheet": sheet, "view": id}))?);
        }
        views.push(id);
    }
    let rows: Vec<Value> = views.iter().map(|id| json_of(&s.model, &bars, *id)).collect();
    let mut out = json!({"views": rows});
    if !viewports.is_empty() {
        out["viewports"] = json!(viewports);
    }
    Ok(out)
}

fn apply(rd: &mut RebarDetail, want: &Ask) {
    if let Some(x) = want.scale {
        rd.scale = x;
    }
    if let Some(list) = &want.stations {
        rd.stations = list.clone();
        rd.stations.sort_by(f64::total_cmp);
        rd.stations.dedup();
    }
    if let Some(b) = want.list {
        rd.list = b;
    }
}
