//! The building code commands: `building_code` (choose, amend, change, save, delete), `building_code_export` and
//! `building_code_import` (XML), `code_rules` (what is in force and why), `site_climate` and `code_check`.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::{
    BuildingCode, Codes, Discipline, Kind, Layer, ORDER, Site, Source, find, layered, own_name, resolve_in, section_def, sections,
};
use crate::commands::{bad, flag, only, string};
use crate::engine::Session;
use crate::shared::Shared;

type R = Result<Value, String>;

fn absent(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// The discipline `p` names, architectural where it names none.
fn discipline(c: &str, p: &Value) -> Result<Discipline, String> {
    match string(c, p, "discipline")? {
        None => Ok(Discipline::Architectural),
        Some(t) => Discipline::parse(&t).ok_or_else(|| {
            let all: Vec<&str> = Discipline::ALL.iter().map(|d| d.key()).collect();
            bad(c, format!("`discipline` is one of {}, not `{t}`", all.join(", ")))
        }),
    }
}

/// The rules in force for discipline `d` as one code of its own, called `name`: every section one of its layers has or
/// the project changed, each rule with where it came from.
pub fn in_force(codes: &Codes, d: Discipline, name: &str) -> BuildingCode {
    let layers = codes.layers(d);
    let base = layers[0];
    let mut keys: BTreeSet<String> = layers.iter().flat_map(|c| c.sections.keys().cloned()).collect();
    keys.extend(codes.choice(d).changes.keys().cloned());
    let named: Vec<String> = layers.iter().map(|c| c.named()).collect();
    let mut out = BuildingCode {
        code: name.to_string(),
        discipline: d,
        name: name.to_string(),
        region: base.region.clone(),
        references: layers.iter().flat_map(|c| c.references.clone()).collect::<Vec<_>>(),
        note: format!("The project's own, based on {}.", named.join(" + ")),
        based_on: if base.based_on.is_empty() { base.code.clone() } else { base.based_on.clone() },
        ..BuildingCode::default()
    };
    out.references.dedup();
    out.references.truncate(50);
    for key in keys {
        let r = resolve_in(codes, d, &key);
        for k in r.value.keys() {
            let at = format!("{key}.{k}");
            if r.from.get(k) == Some(&Layer::Project) {
                out.sources.insert(at.clone(), Source::Text(format!("{name}: the project's own number")));
                out.kinds.insert(at, Kind::Assumption);
                continue;
            }
            if let Some(s) = r.sources.get(k) {
                out.sources.insert(at.clone(), Source::Text(s.clone()));
            }
            if let Some(kind) = r.kinds.get(k).filter(|k| **k != Kind::Requirement) {
                out.kinds.insert(at, *kind);
            }
        }
        out.sections.insert(key, Value::Object(r.value));
    }
    out
}

/// The rules in force for discipline `d` as codes of the project's own, the first called `name`: `in_force`, whole as it
/// was, and where more than one layer holds a utility's or a country's method whole (`WHOLE`), each of those layers' methods
/// as an amendment over it, in the layers' order, so that one utility held under another keeps its own method once exported
/// or saved (one code keeps one method a section). The last of them is the method in force, which the first holds too.
pub fn in_force_layered(codes: &Codes, d: Discipline, name: &str) -> Result<Vec<BuildingCode>, String> {
    let base = in_force(codes, d, name);
    let layers = codes.layers(d);
    // Every method held, once more than one layer holds any: which answers depends on the layers' order across keys too (a
    // utility's entrance in one layer, its unit demand in another), so each layer's methods keep their place.
    let whole: Vec<&str> = super::WHOLE.iter().filter(|(wd, _)| *wd == d).map(|(_, k)| *k).collect();
    let holders = layers.iter().filter(|c| whole.iter().any(|k| c.sections.contains_key(*k))).count();
    let split: Vec<&str> =
        if holders > 1 { whole.into_iter().filter(|k| layers.iter().any(|c| c.sections.contains_key(*k))).collect() } else { Vec::new() };
    if split.is_empty() {
        return Ok(vec![base]);
    }
    let of_key = |at: &str, key: &str| at == key || at.starts_with(&format!("{key}."));
    // Each amendment is named after the code and the layer it keeps, never a name the project or BIMCraft has.
    let mut taken: Vec<String> = codes.own.iter().filter(|k| k.discipline == d).map(|k| k.code.to_lowercase()).collect();
    taken.push(base.name.to_lowercase());
    let mut out = Vec::new();
    for layer in &layers {
        let mut a = BuildingCode {
            discipline: d,
            region: layer.region.clone(),
            note: format!("{} as the project holds it under {}.", layer.named(), base.name),
            based_on: if layer.based_on.is_empty() { layer.code.clone() } else { layer.based_on.clone() },
            amends: base.name.clone(),
            documents: layer.documents.clone(),
            ..BuildingCode::default()
        };
        for key in split.iter().filter(|k| layer.sections.contains_key(**k)) {
            let Some(w) = super::whole_section(layer, key) else { continue };
            a.sections.insert((*key).to_string(), Value::Object(w));
            a.sources.extend(layer.sources.iter().filter(|(at, _)| of_key(at, key)).map(|(k, s)| (k.clone(), s.clone())));
            a.kinds.extend(layer.kinds.iter().filter(|(at, _)| of_key(at, key)).map(|(k, s)| (k.clone(), *s)));
        }
        if a.sections.is_empty() {
            continue;
        }
        // No name left that fits beside the others: refused, rather than one amendment taking another's place.
        a.code = amendment_name(&base.name, &layer.code, &taken).ok_or_else(|| {
            format!(
                "no name of {} characters is left for {}'s method beside `{}`: give a shorter name",
                super::MAX_NAME,
                layer.code,
                base.name
            )
        })?;
        a.name = a.code.clone();
        taken.push(a.code.to_lowercase());
        out.push(a);
    }
    Ok(std::iter::once(base).chain(out).collect())
}

/// A name for the amendment that keeps layer `layer` of the code called `base`: `<base> · <layer>`, the base's part cut to
/// fit, numbered past any name `taken` or BIMCraft's, so it never stands for another code.
pub(super) fn amendment_name(base: &str, layer: &str, taken: &[String]) -> Option<String> {
    for n in 1..1000 {
        let tail = if n == 1 { format!(" · {layer}") } else { format!(" · {layer} ({n})") };
        let room = super::MAX_NAME.saturating_sub(tail.chars().count());
        let name = format!("{}{tail}", base.chars().take(room).collect::<String>().trim_end());
        if !taken.contains(&name.to_lowercase()) && own_name(&name).is_ok_and(|ok| ok == name) {
            return Some(name);
        }
    }
    None
}

/// The rules of section `key` the project changed, with their values, that are no longer its layers' own.
fn trimmed(codes: &Codes, d: Discipline, key: &str, changed: Map<String, Value>) -> Map<String, Value> {
    let own = layered(codes, d, key).value;
    changed
        .into_iter()
        .filter(|(k, v)| match own.get(k) {
            Some(o) => o != v && o.as_f64().zip(v.as_f64()).is_none_or(|(a, b)| a != b),
            None => true,
        })
        .collect()
}

/// What discipline `d` of `codes` says: its base and amendments, the rules in force by section, what the project
/// changed, and every code it may choose.
pub fn said(codes: &Codes, d: Discipline) -> Value {
    let base = codes.base(d);
    let layers = codes.layers(d);
    let mut in_force = Map::new();
    let mut changes = Map::new();
    for def in sections().iter().filter(|s| s.discipline == d) {
        let r = resolve_in(codes, d, def.key);
        if r.value.is_empty() {
            continue;
        }
        let changed: Vec<&String> = r.from.iter().filter(|(_, l)| **l == Layer::Project).map(|(k, _)| k).collect();
        if !changed.is_empty() {
            changes.insert(def.key.into(), json!(changed));
        }
        in_force.insert(def.key.into(), Value::Object(r.value));
    }
    let gone: Vec<Value> =
        codes.editions_gone(d).into_iter().map(|(code, pinned, now)| json!({"code": code, "pinned": pinned, "now": now})).collect();
    let mut out = json!({
        "discipline": d.key(),
        "code": base.code,
        "name": base.name,
        "edition": base.edition,
        "region": base.region,
        "references": base.references,
        "note": base.note,
        "based_on": base.based_on,
        "checked": base.checked,
        "custom": codes.own.iter().any(|k| k.code == base.code && k.discipline == d),
        "amendments": layers.iter().skip(1).map(|c| json!({
            "code": c.code, "name": c.name, "edition": c.edition, "amends": c.amends,
            "custom": codes.own.iter().any(|k| k.code == c.code && k.discipline == d),
        })).collect::<Vec<_>>(),
        "editions": codes.choice(d).editions,
        "editions_gone": gone,
        "sections": in_force,
        "changes": changes,
        "all": codes.all(d).map(|(k, own)| json!({
            "code": k.code, "name": k.name, "region": k.region, "edition": k.edition, "references": k.references, "note": k.note,
            "custom": own, "amends": k.amends, "checked": !k.checked.is_empty(),
        })).collect::<Vec<_>>(),
    });
    // The stairs' rules and what of them the project changed, as #386 answered, for the architectural discipline.
    if d == Discipline::Architectural {
        let r = resolve_in(codes, d, "stairs");
        out["stairs"] = Value::Object(r.value.clone());
        out["changed"] = json!(ORDER.iter().filter(|k| r.from.get(**k) == Some(&Layer::Project)).collect::<Vec<_>>());
    }
    out
}

/// `building_code`: a discipline's code chosen, its amendments, its rules changed (the project's, or an amendment's), kept
/// under a name, an amendment made, or one taken away.
pub(crate) fn building_code(s: &mut Session, p: &Value) -> R {
    let c = "building_code";
    // The first Building Codes named a method in an `electrical` part; a utility's method is its amendment now, a country's its code.
    if p.get("electrical").is_some() {
        return Err(bad(
            c,
            "`electrical` is gone: a utility's method is its amendment and a country's its base code, of the electrical discipline \
             (`discipline: electrical`, then `set` or `amendments`; `code_rules` lists what a code holds)",
        ));
    }
    only(
        c,
        p,
        &["discipline", "set", "amendments", "keep_rules", "rules", "stairs", "reset", "save", "delete", "amend", "create", "over"],
    )?;
    let d = discipline(c, p)?;
    let mut now = (*s.model.codes).clone();
    let unchanged = now.clone();
    // A new amendment of the project's own, empty, over the base: chosen by `amendments`, edited by `amend`.
    let create = string(c, p, "create")?.map(|n| own_name(&n).map_err(|e| bad(c, e))).transpose()?;
    if let Some(name) = &create {
        if now.all(d).any(|(k, _)| k.code == *name) {
            return Err(bad(c, format!("`create`: there is a {} code called `{name}` already", d.key())));
        }
        let over = match string(c, p, "over")? {
            Some(o) if now.all(d).any(|(k, _)| k.code == o) => o,
            Some(o) => return Err(bad(c, format!("`over` is the {} code the new amendment amends, and `{o}` is none", d.key()))),
            None => now.base(d).code.clone(),
        };
        now.own.push(BuildingCode {
            code: name.clone(),
            name: name.clone(),
            discipline: d,
            amends: over.clone(),
            note: format!("The project's own amendment over {over}."),
            ..BuildingCode::default()
        });
    }
    let names: Vec<String> = now.all(d).map(|(k, _)| k.code.clone()).collect();
    let bases: Vec<String> = now.all(d).filter(|(k, _)| k.amends.is_empty()).map(|(k, _)| k.code.clone()).collect();
    let set = match p.get("set") {
        v if absent(v) => None,
        Some(Value::String(t)) if bases.contains(t) => Some(t.clone()),
        Some(Value::String(t)) if d == Discipline::Architectural && t == "nbr" => Some("nbr-9050".to_string()),
        Some(Value::String(t)) if names.contains(t) => {
            return Err(bad(c, format!("`{t}` amends another code: give it in `amendments`, over the code it amends")));
        }
        Some(v) => return Err(bad(c, format!("`set` is one of {}, not {v}", bases.join(", ")))),
        None => None,
    };
    // A new amendment stands over the base chosen with it.
    if let (Some(name), Some(set)) = (&create, &set)
        && let Some(k) = now.own.iter_mut().find(|k| k.code == *name && k.discipline == d)
    {
        k.amends = set.clone();
        k.note = format!("The project's own amendment over {set}.");
    }
    let amendments = match p.get("amendments") {
        v if absent(v) => None,
        Some(Value::Array(a)) if a.len() <= super::MAX_AMENDMENTS => {
            let mut out = Vec::new();
            for x in a {
                match x.as_str() {
                    Some(t) if names.contains(&t.to_string()) => out.push(t.to_string()),
                    _ => return Err(bad(c, format!("`amendments` are codes that amend another, of {}; not {x}", names.join(", ")))),
                }
            }
            Some(out)
        }
        Some(_) => return Err(bad(c, format!("`amendments` is a list of {} codes at most", super::MAX_AMENDMENTS))),
        None => None,
    };
    let save = string(c, p, "save")?.map(|n| own_name(&n).map_err(|e| bad(c, e))).transpose()?;
    let delete = string(c, p, "delete")?;
    if let Some(name) = &delete
        && !now.own.iter().any(|k| k.code == *name && k.discipline == d)
    {
        return Err(bad(c, format!("`delete` takes one of the project's own codes away, and `{name}` is none of them")));
    }
    let keep = flag(c, p, "keep_rules")?;
    let reset = match p.get("reset") {
        v if absent(v) => None,
        Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(None),
        Some(Value::String(k)) if section_def(d, k).is_some() => Some(Some(k.clone())),
        Some(v) => return Err(bad(c, format!("`reset` is true (every section) or a section's key, not {v}"))),
        None => None,
    };
    // The rules to change, by section: `stairs` is the architectural `rules.stairs`.
    let mut patches: Patches = Vec::new();
    match p.get("rules") {
        v if absent(v) => {}
        Some(Value::Object(o)) => {
            for (k, v) in o {
                let Some(def) = section_def(d, k) else {
                    let known: Vec<&str> = sections().iter().filter(|s| s.discipline == d).map(|s| s.key).collect();
                    return Err(bad(c, format!("`{k}` is no {} section; they are {}", d.key(), known.join(", "))));
                };
                let Value::Object(rules) = v else { return Err(bad(c, format!("`rules.{k}` is the rules to change, by name"))) };
                patches.push((def.key.to_string(), rules.clone()));
            }
        }
        Some(_) => return Err(bad(c, "`rules` is the rules to change, by section: {stairs: {width_min: 1100}, …}")),
        None => {}
    }
    match p.get("stairs") {
        v if absent(v) => {}
        Some(Value::Object(o)) if d == Discipline::Architectural => patches.push(("stairs".into(), o.clone())),
        Some(_) => return Err(bad(c, "`stairs` is the stair rules to change, by name: {width_min: 1100, …}")),
        None => {}
    }
    if reset.is_some() && !patches.is_empty() {
        return Err(bad(c, "`reset` takes the project's own rules away: give it without rules to change"));
    }
    let amend = string(c, p, "amend")?;

    let mut next = now.clone();
    // The base in force: the one chosen, or the default where none is.
    let base = now.base(d).code.clone();
    {
        let choice = next.choice_mut(d);
        if let Some(set) = &set {
            // Another base brings its own amendments; any `set` brings its rules, unless the project's are kept.
            if base != *set && amendments.is_none() {
                choice.amendments.clear();
            }
            choice.code = Some(set.clone());
            if !keep {
                choice.changes.clear();
            }
        }
        if let Some(a) = &amendments {
            choice.amendments = a.clone();
        }
        match &reset {
            _ if amend.is_some() => {}
            Some(None) => choice.changes.clear(),
            Some(Some(k)) => {
                choice.changes.remove(k);
            }
            None => {}
        }
    }
    // An amendment over a code that isn't one of the layers would be left out: said, not done quietly.
    if let Some(a) = &amendments {
        let held: Vec<String> = next.layers(d).iter().map(|k| k.code.clone()).collect();
        if let Some(gone) = a.iter().find(|x| !held.contains(x)) {
            let over = next.find(d, gone).map_or(String::new(), |k| k.amends.clone());
            return Err(bad(c, format!("`{gone}` amends `{over}`, which is neither the base nor another amendment chosen")));
        }
    }
    // The settings taken away by name, for `settings_fit`.
    let asked: Vec<(String, String)> = patches
        .iter()
        .filter(|(key, _)| d == Discipline::Electrical && crate::electrical::SECTIONS.contains(&key.as_str()))
        .flat_map(|(key, rules)| rules.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| (key.clone(), k.clone())))
        .collect();
    // The rules changed in an amendment are its own: one of BIMCraft's becomes the project's copy of it.
    if let Some(target) = &amend {
        let held: Vec<String> = next.layers(d).iter().skip(1).map(|k| k.code.clone()).collect();
        if !held.contains(target) {
            let what = if held.is_empty() { "it has none".to_string() } else { held.join(", ") };
            return Err(bad(c, format!("`amend` is one of the project's {} amendments ({what}), not `{target}`", d.key())));
        }
        amend_rules(&mut next, d, target, reset.clone(), patches).map_err(|e| bad(c, e))?;
        patches = Vec::new();
    }
    for (key, rules) in patches {
        // A utility's or a country's method whole is changed in its amendment: the project's own change would have no
        // utility it belongs to once another is chosen.
        if super::is_whole(d, &key) {
            return Err(bad(
                c,
                format!("`{key}` is a utility's or a country's method: change it in its amendment, with `amend: \"<its code>\"`"),
            ));
        }
        // Which rules a section has: what its layers, or the library, give it; a section of reference takes new ones.
        let known = resolve_in(&next, d, &key).value;
        let reference = section_def(d, &key).is_some_and(|def| def.read_by.is_empty())
            || (d == Discipline::Electrical && crate::electrical::SECTIONS.contains(&key.as_str()));
        let mut changed = next.choice(d).changes.get(&key).cloned().unwrap_or_default();
        for (k, v) in rules {
            if !known.contains_key(&k) && !reference {
                let names: Vec<&String> = known.keys().collect();
                let names: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
                return Err(bad(c, format!("`{k}` is no rule of {key}; they are {}", names.join(", "))));
            }
            if k.chars().count() > 128 {
                return Err(bad(c, "a rule's name is 128 characters at most"));
            }
            changed.insert(k, v);
        }
        // As the section types its rules, so `1100` is the `1100.0` the code has.
        if let Some(def) = section_def(d, &key) {
            let mut all = known.clone();
            all.extend(changed.clone());
            let read = def.read;
            if let Ok(Value::Object(canon)) = read(&Value::Object(all)) {
                for (k, v) in changed.iter_mut() {
                    if let Some(x) = canon.get(k) {
                        *v = x.clone();
                    }
                }
            }
        }
        next.choice_mut(d).changes.insert(key, changed);
    }
    // The rules in force, kept as a code of the project's own, which it is then held to.
    if let Some(name) = &save {
        // Each utility's method held under another's stays its own, as an amendment over the saved code.
        let saved = in_force_layered(&next, d, name).map_err(|e| bad(c, e))?;
        let mut amendments = Vec::new();
        for (i, mut own) in saved.into_iter().enumerate() {
            if i == 0 {
                own.code = name.clone();
            } else {
                amendments.push(own.code.clone());
            }
            next.own.retain(|k| !(k.code == own.code && k.discipline == d));
            next.own.push(own);
        }
        let choice = next.choice_mut(d);
        choice.code = Some(name.clone());
        choice.amendments = amendments;
        choice.changes.clear();
    }
    // One taken away: a project held to it keeps its rules, as its own, over the code it was based on.
    if let Some(name) = &delete
        && let Some(i) = next.own.iter().position(|k| k.code == *name && k.discipline == d)
    {
        let held = next.choice(d).code.as_deref() == Some(name.as_str()) || next.choice(d).amendments.contains(name);
        let before: Vec<(String, Map<String, Value>)> = if held {
            let keys: BTreeSet<String> = next.layers(d).iter().flat_map(|k| k.sections.keys().cloned()).collect();
            keys.into_iter().map(|k| (k.clone(), resolve_in(&next, d, &k).value)).collect()
        } else {
            Vec::new()
        };
        // The project's copy of one of BIMCraft's: the library's is there again, and still chosen.
        let held = held && next.library_of(d, name).is_none();
        // A utility's or a country's method is no rule of the project's own: it would be laid over whatever code holds the
        // section then, and answer as that one. The project chooses another code first.
        if held && let Some(key) = next.own[i].sections.keys().find(|k| super::is_whole(d, k)) {
            return Err(bad(
                c,
                format!(
                    "the project is held to `{name}`, whose `{key}` is a utility's or a country's method and can't be kept as the \
                     project's own: choose another code first (or `save` the rules in force under another name)"
                ),
            ));
        }
        let gone = next.own.remove(i);
        if held {
            let choice = next.choice_mut(d);
            choice.amendments.retain(|a| a != name);
            if choice.code.as_deref() == Some(name.as_str()) {
                choice.code = find(d, &gone.based_on).filter(|k| k.amends.is_empty()).map(|k| k.code.clone());
            }
            for (key, rules) in before {
                next.choice_mut(d).changes.insert(key, rules);
            }
        }
    }
    // Rules that are the code's own are not the project's; a section with none changed has no entry.
    let keys: Vec<String> = next.choice(d).changes.keys().cloned().collect();
    for key in keys {
        let changed = next.choice_mut(d).changes.remove(&key).unwrap_or_default();
        let left = trimmed(&next, d, &key, changed);
        if !left.is_empty() {
            next.choice_mut(d).changes.insert(key, left);
        }
    }
    // The editions of what it chose, pinned, when it chose.
    if set.is_some() || amendments.is_some() || save.is_some() || delete.is_some() {
        let pins = next.pins(d);
        next.choice_mut(d).editions = pins;
    }
    next.check_keeping(Some(&unchanged)).map_err(|e| bad(c, e))?;
    if d == Discipline::Electrical {
        crate::electrical::settings_fit(&s.model, &unchanged, &next, &asked).map_err(|e| bad(c, e))?;
    }
    if next != unchanged {
        let m = s.model_mut();
        let before = std::mem::replace(&mut m.codes, Shared::new(next));
        if d == Discipline::Electrical {
            full(crate::electrical::from_codes(m, &before)).map_err(|e| bad(c, e))?;
        }
    }
    Ok(said(&s.model.codes, d))
}

/// What the codes name that the project had no room for: refused, as the commands refuse it.
fn full(left: Vec<String>) -> Result<(), String> {
    match left.first() {
        None => Ok(()),
        Some(first) => Err(format!(
            "the project holds up to {} of each electrical setting, and {first} would be past that; take one away in the same change",
            crate::electrical::MAX_VOLTAGES
        )),
    }
}

/// Rules to change, by section.
type Patches = Vec<(String, Map<String, Value>)>;

/// `patches` laid into amendment `target` of discipline `d`, or `reset` taken out of it: a rule set to null leaves it, so the
/// layers under it give it again. One of BIMCraft's is copied as the project's own first, under its name; a reset of the
/// whole of such a copy takes the copy away, and of a section gives it the library's section back.
fn amend_rules(next: &mut Codes, d: Discipline, target: &str, reset: Option<Option<String>>, patches: Patches) -> Result<(), String> {
    let library = next.library_of(d, target);
    if !next.own.iter().any(|k| k.code == target && k.discipline == d) {
        let Some(lib) = library else { return Err(format!("no {} code `{target}`", d.key())) };
        let mut copy = lib.clone();
        copy.default = false;
        copy.superseded_by.clear();
        copy.based_on = lib.code.clone();
        next.own.push(copy);
    }
    let i = next.own.iter().position(|k| k.code == target && k.discipline == d).ok_or_else(|| format!("no code `{target}`"))?;
    match reset {
        Some(None) => {
            if library.is_some() {
                next.own.remove(i);
            } else {
                let own = &mut next.own[i];
                own.sections.clear();
                own.sources.clear();
                own.kinds.clear();
            }
            return Ok(());
        }
        Some(Some(key)) => {
            let own = &mut next.own[i];
            let of = |at: &String| at == &key || at.starts_with(&format!("{key}."));
            own.sources.retain(|at, _| !of(at));
            own.kinds.retain(|at, _| !of(at));
            own.sections.remove(&key);
            if let Some(lib) = library
                && let Some(v) = lib.sections.get(&key)
            {
                own.sections.insert(key.clone(), v.clone());
                own.sources.extend(lib.sources.iter().filter(|(at, _)| of(at)).map(|(k, v)| (k.clone(), v.clone())));
                own.kinds.extend(lib.kinds.iter().filter(|(at, _)| of(at)).map(|(k, v)| (k.clone(), *v)));
            }
            return Ok(());
        }
        None => {}
    }
    let mut ordinary = BTreeSet::new();
    for (key, rules) in patches {
        // A section asked with no rule changes nothing, and is not checked.
        let asked_none = rules.is_empty();
        let resolved = resolve_in(next, d, &key);
        // A utility's or a country's method is the amendment's own whole, whatever another's over it holds.
        let known = if super::is_whole(d, &key) {
            super::whole_section(&next.own[i], &key).unwrap_or_else(|| resolved.value.clone())
        } else {
            resolved.value.clone()
        };
        let def = section_def(d, &key);
        // The electrical settings are entries by name: a new one is added, and one a code under it names is taken away as null.
        let settings = d == Discipline::Electrical && crate::electrical::SECTIONS.contains(&key.as_str());
        let reference = def.is_some_and(|def| def.read_by.is_empty()) || settings;
        let mut section = next.own[i].sections.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
        let mut set = Vec::new();
        for (k, v) in rules {
            if !known.contains_key(&k) && !reference && !v.is_null() {
                let names: Vec<&str> = known.keys().map(String::as_str).collect();
                return Err(format!("`{k}` is no rule of {key}; they are {}", names.join(", ")));
            }
            if k.chars().count() > 128 {
                return Err("a rule's name is 128 characters at most".into());
            }
            // A setting taken away that a code under it names, or that a new project has, is null here, so it stays away;
            // the project's own change of it goes with it. A code under it names it whether or not this one gives it too:
            // the layers under this amendment alone, without it and those over it.
            let under = settings && v.is_null() && {
                let mut below = next.clone();
                let target = &below.own[i].code;
                if let Some(at) = below.choice(d).amendments.iter().position(|a| a == target) {
                    below.choice_mut(d).amendments.truncate(at);
                } else if let Some(o) = below.own[i].sections.get_mut(&key).and_then(Value::as_object_mut) {
                    o.remove(&k);
                }
                layered(&below, d, &key).value.get(&k).is_some_and(|v| !v.is_null())
            };
            if v.is_null()
                && settings
                && (under || crate::electrical::starter_has(&key, &k) || resolved.from.get(&k) == Some(&Layer::Project))
            {
                section.insert(k.clone(), Value::Null);
                if let Some(changes) = next.choice_mut(d).changes.get_mut(&key) {
                    changes.remove(&k);
                }
            } else if v.is_null() {
                section.remove(&k);
            } else {
                section.insert(k.clone(), v);
                set.push(k);
            }
        }
        // As the section types its rules, so `1100` is the `1100.0` the code has.
        if let Some(def) = def {
            let mut all = known.clone();
            all.extend(section.clone());
            if let Ok(Value::Object(canon)) = (def.read)(&Value::Object(all)) {
                for (k, v) in section.iter_mut() {
                    if let Some(x) = canon.get(k) {
                        *v = x.clone();
                    }
                }
            }
        }
        let own = &mut next.own[i];
        // A number of BIMCraft's copy that is changed is no longer its document's: said so where it is.
        for k in &set {
            let at = format!("{key}.{k}");
            if library.is_some_and(|lib| lib.section(&key).and_then(|o| o.get(k)) != section.get(k)) {
                own.sources.insert(at.clone(), Source::Text(format!("{target}: changed in this project")));
                own.kinds.insert(at, Kind::Assumption);
            }
        }
        let gone: Vec<String> = own
            .sources
            .keys()
            .filter(|at| at.split_once('.').is_some_and(|(s, r)| s == key && !section.contains_key(r)))
            .cloned()
            .collect();
        for at in gone {
            own.sources.remove(&at);
            own.kinds.remove(&at);
        }
        if section.is_empty() {
            own.sections.remove(&key);
        } else {
            own.sections.insert(key.clone(), Value::Object(section));
        }
        // A utility's or a country's method answers for its own utility wherever it lies, under another's too: the one
        // changed is checked whole on its layer, not only the one in force. Only here, as it is changed: a file that holds
        // one broken still opens, and takes other edits.
        if super::is_whole(d, &key)
            && let (Some(def), Some(whole)) = (def, super::whole_section(&next.own[i], &key))
        {
            def.check(&Value::Object(whole)).map_err(|e| format!("{} {key} of {target}: {e}", d.key()))?;
        } else if def.is_some() && !settings && !asked_none && !super::is_whole(d, &key) {
            ordinary.insert(key.clone());
        }
    }
    // An ordinary section changed is checked, once all its patches are in, as this layer and the ones under it make it,
    // not only as it is in force, where a layer over it may hide what it holds until that layer goes. The layers in the
    // order they stand (an amendment over another stands over it, whatever order the project chose them in).
    if !ordinary.is_empty() {
        let standing: Vec<String> = next.layers(d).iter().skip(1).map(|c| c.code.clone()).collect();
        let mut upto = next.clone();
        if let Some(at) = standing.iter().position(|a| a == target) {
            let under = &standing[..=at];
            upto.choice_mut(d).amendments = under.to_vec();
            // Each stands where it stood, though what it was made over may now be left out of the list.
            let base = upto.base(d).code.clone();
            for c in upto.own.iter_mut().filter(|c| c.discipline == d && under.contains(&c.code)) {
                c.amends = base.clone();
            }
        }
        for key in &ordinary {
            let Some(def) = section_def(d, key) else { continue };
            let made = super::layered(&upto, d, key).value;
            def.check(&Value::Object(made)).map_err(|e| format!("{} {key} of {target}: {e}", d.key()))?;
        }
    }
    Ok(())
}

/// `building_code_export`: a discipline's rules in force, a code named, or all five disciplines', as an XML file.
pub(crate) fn building_code_export(s: &mut Session, p: &Value) -> R {
    let c = "building_code_export";
    only(c, p, &["path", "discipline", "all", "code", "name"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is the XML file to write"))?;
    let d = discipline(c, p)?;
    let all = flag(c, p, "all")?;
    let name = string(c, p, "name")?.map(|n| own_name(&n).map_err(|e| bad(c, e))).transpose()?;
    let codes = &s.model.codes;
    let out: Vec<BuildingCode> = if all {
        Discipline::ALL
            .iter()
            .map(|d| {
                let base = codes.base(*d);
                let mut ks = in_force_layered(codes, *d, name.as_deref().unwrap_or(&base.name)).map_err(|e| bad(c, e))?;
                if let Some(k) = ks.first_mut() {
                    k.note = base.note.clone();
                    k.checked = base.checked.clone();
                }
                Ok(ks)
            })
            .collect::<Result<Vec<Vec<BuildingCode>>, String>>()?
            .into_iter()
            .flatten()
            .collect()
    } else {
        let mut out = match string(c, p, "code")? {
            Some(k) => vec![codes.find(d, &k).cloned().ok_or_else(|| bad(c, format!("no {} building code `{k}`", d.key())))?],
            // The project's, as it is held to it: with the rules it changed, and each utility's method under another's
            // as an amendment of its own (`in_force_layered`).
            None => {
                let base = codes.base(d);
                let mut ks = in_force_layered(codes, d, name.as_deref().unwrap_or(&base.name)).map_err(|e| bad(c, e))?;
                if let Some(k) = ks.first_mut()
                    && codes.layers(d).len() == 1
                    && codes.choice(d).changes.is_empty()
                {
                    k.note = base.note.clone();
                    k.checked = base.checked.clone();
                }
                ks
            }
        };
        if let Some(one) = out.first_mut() {
            if let Some(n) = &name {
                one.name = n.clone();
            }
            if one.based_on.is_empty() {
                one.based_on = one.code.clone();
            }
        }
        out
    };
    std::fs::write(&path, super::xml::write(&out)).map_err(|e| bad(c, format!("{path}: {e}")))?;
    let first = out.first().map(|k| (k.name.clone(), k.based_on.clone())).unwrap_or_default();
    Ok(json!({"path": path, "name": first.0, "based_on": first.1, "codes": out.len()}))
}

/// `building_code_import`: the codes an XML file holds, kept as the project's own; `set` holds the project to them.
pub(crate) fn building_code_import(s: &mut Session, p: &Value) -> R {
    let c = "building_code_import";
    only(c, p, &["path", "name", "set"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is the XML file to read"))?;
    let set = flag(c, p, "set")?;
    let size = std::fs::metadata(&path).map_err(|e| bad(c, format!("{path}: {e}")))?.len();
    if size > super::MAX_XML as u64 {
        return Err(bad(c, format!("{path}: larger than building codes' file ({} MB)", super::MAX_XML >> 20)));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(c, format!("{path}: {e}")))?;
    // A file named after a code BIMCraft has (one exported as it was) comes in under another name.
    let renamed = string(c, p, "name")?;
    let (read, left_out) = super::read_xml(&text, renamed.as_deref()).map_err(|e| bad(c, format!("{path}: {e}")))?;
    let mut next = (*s.model.codes).clone();
    // A code the file lays over its first, named as one the project has over another, would take that one's place: refused.
    for k in read.iter().skip(1).filter(|k| !k.amends.is_empty()) {
        if next.own.iter().any(|o| o.code == k.code && o.discipline == k.discipline && o.amends != k.amends) {
            return Err(bad(
                c,
                format!("{path}: `{}` is a code the project has over `{}`: import it under another `name`", k.code, k.amends),
            ));
        }
    }
    for k in &read {
        next.own.retain(|o| !(o.code == k.code && o.discipline == k.discipline));
        next.own.push(k.clone());
        if set {
            let choice = next.choice_mut(k.discipline);
            if k.amends.is_empty() {
                choice.code = Some(k.code.clone());
                choice.amendments.clear();
                choice.changes.clear();
            } else if !choice.amendments.contains(&k.code) {
                choice.amendments.push(k.code.clone());
            }
        }
    }
    for d in Discipline::ALL {
        let pins = next.pins(d);
        next.choice_mut(d).editions = pins;
    }
    next.check_keeping(Some(&s.model.codes)).map_err(|e| bad(c, format!("{path}: {e}")))?;
    crate::electrical::settings_fit(&s.model, &s.model.codes, &next, &[]).map_err(|e| bad(c, format!("{path}: {e}")))?;
    let first = read.first().map(|k| k.code.clone()).unwrap_or_default();
    let all: Vec<Value> = read.iter().map(|k| json!({"code": k.code, "discipline": k.discipline.key()})).collect();
    let m = s.model_mut();
    let before = std::mem::replace(&mut m.codes, Shared::new(next));
    full(crate::electrical::from_codes(m, &before)).map_err(|e| bad(c, format!("{path}: {e}")))?;
    Ok(json!({"code": first, "codes": all, "left_out": left_out, "set": set}))
}

/// `code_rules`: the rules in force, by section, each with where it comes from, its source and its kind.
pub(crate) fn code_rules(s: &mut Session, p: &Value) -> R {
    let c = "code_rules";
    only(c, p, &["discipline", "section"])?;
    let d = discipline(c, p)?;
    let one = string(c, p, "section")?;
    if let Some(k) = &one
        && section_def(d, k).is_none()
    {
        return Err(bad(c, format!("`{k}` is no {} section", d.key())));
    }
    let m = &s.model;
    let mut out = Vec::new();
    for def in sections().iter().filter(|x| x.discipline == d && one.as_deref().is_none_or(|k| k == x.key)) {
        let r = m.resolve_code(d, def.key);
        let rules: Vec<Value> = r
            .value
            .iter()
            .map(|(k, v)| json!({"rule": k, "value": v, "from": r.from.get(k), "source": r.sources.get(k), "kind": r.kinds.get(k)}))
            .collect();
        out.push(json!({
            "section": def.key, "label": def.label, "about": def.about, "read_by": def.read_by,
            "reference": def.read_by.is_empty(), "rules": rules,
        }));
    }
    let base = m.codes.base(d);
    Ok(json!({"discipline": d.key(), "code": base.code, "edition": base.edition, "sections": out}))
}

/// `site_climate`: the project's site numbers, each with the map it was read from.
pub(crate) fn site_climate(s: &mut Session, p: &Value) -> R {
    let c = "site_climate";
    let mut keys: Vec<&str> = super::SITE_NUMBERS.iter().map(|(k, ..)| *k).filter(|k| *k != "_").collect();
    keys.extend(super::SITE_TEXTS.iter().map(|(k, _)| *k));
    keys.push("sources");
    only(c, p, &keys)?;
    let mut site = s.model.codes.site.clone();
    let Ok(Value::Object(mut now)) = serde_json::to_value(&site) else { return Err(bad(c, "the site's numbers could not be read")) };
    let taken: Vec<String> =
        p.as_object().map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    if let Some(o) = p.as_object() {
        for (k, v) in o {
            if k == "sources" {
                let Value::Object(src) = v else { return Err(bad(c, "`sources` is a text for each number, by its key")) };
                let mut have: Map<String, Value> = now.get("sources").and_then(Value::as_object).cloned().unwrap_or_default();
                for (sk, sv) in src {
                    match sv {
                        Value::Null => {
                            have.remove(sk);
                        }
                        Value::String(_) => {
                            have.insert(sk.clone(), sv.clone());
                        }
                        _ => return Err(bad(c, format!("`sources.{sk}` is a text"))),
                    }
                }
                now.insert("sources".into(), Value::Object(have));
            } else if v.is_null() {
                now.remove(k);
            } else {
                now.insert(k.clone(), v.clone());
            }
        }
    }
    site = serde_json::from_value::<Site>(Value::Object(now)).map_err(|e| bad(c, e))?;
    // A source for a number taken away goes with it.
    site.sources.retain(|k, _| !taken.contains(k));
    site.check().map_err(|e| bad(c, e))?;
    if site != s.model.codes.site {
        s.model_mut().codes.site = site;
    }
    Ok(serde_json::to_value(&s.model.codes.site).unwrap_or(Value::Null))
}

/// `code_check`: where elements are outside the project's building codes, by element.
pub(crate) fn code_check(s: &mut Session, p: &Value) -> R {
    let c = "code_check";
    only(c, p, &["ids"])?;
    let m = &s.model;
    let ids: Vec<crate::ids::Id> = match p.get("ids") {
        v if absent(v) => m.elements.keys().copied().collect(),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for x in a {
                let id = x.as_u64().filter(|id| m.elements.contains_key(*id)).ok_or_else(|| bad(c, format!("no element {x}")))?;
                out.push(id);
            }
            out
        }
        Some(v) => return Err(bad(c, format!("`ids` is a list of elements, not {v}"))),
        None => Vec::new(),
    };
    let mut out = Vec::new();
    let mut count = 0;
    for id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        let findings = super::findings(m, id, e);
        if findings.is_empty() {
            continue;
        }
        count += findings.len();
        out.push(json!({"id": id, "category": e.category(), "findings": findings}));
    }
    Ok(json!({"elements": out, "count": count}))
}
