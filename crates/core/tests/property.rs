//! A property test of the electrical building codes. A LEDGER (a reference model written from docs/areas/building-codes.md:
//! base, amendments in order, own codes, the project's changes, a utility's method whole, the library under them) is driven by
//! the same random operations as a Session: `building_code` (set, amendments, keep_rules, rules, amend, reset, create, save,
//! delete), export and import, and the voltage, distribution_system and wire_type commands. After every step:
//!   rules    every rule in force (`code_rules`) equals the ledger's                                   (I1, I2)
//!   answers  every probe answers what a fresh project answers whose codes are ONLY the ledger's rules  (I1, I2)
//!   sane     no number of an answer is negative, missing (NaN) or 1000 times the library's             (I3)
//!   settings the project's voltages / systems / wire types equal what the ledger's codes say           (I6)
//!   roundtrip (an op, on copies) export -> import keeps rules, answers and settings                    (I4)
//! The no-change campaign makes no change of rule or setting and also asks that every named probe answers the library's (I5).
//! A failure prints its seed, sequence and the sequence shrunk to what it needs.
//!
//! A short run goes with `cargo test`; a long one:
//!   P_SEQS=5000 cargo test -p bimcraft-core --release --test property -- --nocapture --test-threads=1
//! Knobs: P_SEQS (200), P_SEED (1), P_THREADS (the cores), P_TMP, P_OUT (a file for the report), P_ONLY (changes|nochange).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // A test: a fixture that fails is the test failing.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use bimcraft_core::codes::{self, BuildingCode, Codes, Discipline};
use bimcraft_core::engine::Session;
use bimcraft_core::shared::Shared;
use serde_json::{Map, Value, json};

const E: Discipline = Discipline::Electrical;
const SETTINGS: [&str; 3] = ["voltages", "distribution_systems", "wire_types"];
const LISTS: [(&str, &str, &str); 3] = [
    ("voltage", "voltages", "voltage"),
    ("distribution_system", "distribution_systems", "distribution_system"),
    ("wire_type", "wire_types", "wire_type"),
];
const TARGETS: [&str; 6] = ["unit_demand", "entrance", "building_demand", "circuits", "conduit_fill", "surge"];
/// A utility's or a country's method, changed only in the code that holds it (`amend`), never among the project's changes.
const WHOLES: [&str; 3] = ["unit_demand", "entrance", "building_demand"];
const OWN_NAMES: [&str; 5] = ["P1", "P2", "P3", "S1", "S2"];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn pct(&mut self, p: usize) -> bool {
        self.below(100) < p
    }
    fn pick<T: Clone>(&mut self, v: &[T]) -> T {
        v[self.below(v.len())].clone()
    }
}

/// Equal, a number by its value and a field left out as null.
fn same(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Number(x), Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            x == y || (x - y).abs() <= 1e-9 * x.abs().max(y.abs())
        }
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (Object(x), Object(y)) => {
            let keys: BTreeSet<&std::string::String> = x.keys().chain(y.keys()).collect();
            keys.into_iter().all(|k| same(x.get(k).unwrap_or(&Null), y.get(k).unwrap_or(&Null)))
        }
        _ => a == b,
    }
}

/// Where two values first differ.
fn diff(e: &Value, a: &Value, at: &str) -> Option<String> {
    if same(e, a) {
        return None;
    }
    match (e, a) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            keys.into_iter().find_map(|k| diff(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), &format!("{at}.{k}")))
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            x.iter().zip(y).enumerate().find_map(|(i, (x, y))| diff(x, y, &format!("{at}[{i}]")))
        }
        _ => Some(format!("{at}: expected {} actual {}", short(e), short(a))),
    }
}

fn short(v: &Value) -> String {
    let t = v.to_string();
    if t.chars().count() > 160 { format!("{}…", t.chars().take(160).collect::<String>()) } else { t }
}

fn strip(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|k, _| k != "source" && k != "sources");
            o.values_mut().for_each(strip);
        }
        Value::Array(a) => a.iter_mut().for_each(strip),
        _ => {}
    }
}

fn ask(s: &mut Session, cmd: &str, p: &Value) -> Value {
    let mut v = match s.execute(cmd, p) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    };
    strip(&mut v);
    v
}

fn bc(mut p: Value) -> Value {
    p["discipline"] = json!("electrical");
    p
}

struct Mutation {
    holder: String,
    section: String,
    rule: String,
    value: Value,
}

struct Lib {
    probes: Vec<(&'static str, Value)>,
    /// Whether the probe names a utility or a standard (I5: its answer is the library's whatever is chosen).
    named: Vec<bool>,
    answers: Vec<Value>,
    muts: Vec<Mutation>,
    starter: [Map<String, Value>; 3],
    bases: Vec<String>,
    amendments: Vec<String>,
    other: Vec<String>,
    tmp: String,
}

fn starter_has(key: &str, name: &str) -> bool {
    use bimcraft_core::electrical;
    match key {
        "voltages" => electrical::voltages().contains_key(name),
        "distribution_systems" => electrical::distribution_systems().contains_key(name),
        "wire_types" => electrical::wire_types().contains_key(name),
        _ => false,
    }
}

fn settings_of(s: &mut Session) -> [Map<String, Value>; 3] {
    LISTS.map(|(cmd, key, _)| {
        s.execute(cmd, &json!({})).ok().and_then(|v| v.get(key).and_then(Value::as_object).cloned()).unwrap_or_default()
    })
}

fn build_probes() -> (Vec<(&'static str, Value)>, Vec<bool>) {
    let mut out: Vec<(&'static str, Value)> = vec![
        (
            "demand",
            json!({"utility": "CPFL", "area": 120, "heaters": [{"w": 5500, "count": 2}], "stoves": [{"w": 6000}], "air_conditioners": [{"btu": 12000, "count": 2}]}),
        ),
        (
            "demand",
            json!({"utility": "CPFL", "building": "other", "lighting_outlets": {"use": "office", "kw": 30}, "motors": [{"cv": 5, "phases": 3, "count": 2}], "system": "220/380"}),
        ),
        ("demand", json!({"utility": "ENEL", "network": "aerial", "installed_kw": 18, "phases": 3})),
        ("demand", json!({"utility": "ENEL", "network": "underground", "installed_kw": 40})),
        ("demand", json!({"utility": "CELESC", "installed_kw": 22, "phases": 3})),
        ("demand", json!({"utility": "CELESC", "installed_kw": 9})),
    ];
    for u in ["COPEL", "ENERGISA", "EQUATORIAL", "FECOERGS", "HIDROPAN"] {
        out.push(("demand", json!({"utility": u, "installed_kw": 20, "demand_kva": 18, "phases": 3})));
        out.push(("demand", json!({"utility": u, "installed_kw": 8, "demand_kva": 6})));
    }
    let mut s = Session::new();
    let groups = [
        json!([{"count": 20, "area": 100}, {"count": 16, "area": 80}]),
        json!([{"count": 12, "demand": 8}, {"count": 6, "demand": 5}]),
        json!([{"count": 30, "demand": 12}]),
    ];
    for c in codes::of(E) {
        let Some(sec) = c.section("building_demand") else { continue };
        let who = match (c.amends.is_empty(), sec.get("utilities").and_then(|u| u.get(0)).and_then(Value::as_str)) {
            (false, Some(u)) => json!({"utility": u}),
            _ => json!({"standard": sec.get("standard").cloned().unwrap_or(Value::Null)}),
        };
        for g in &groups {
            let mut p = who.clone();
            p["dwellings"] = g.clone();
            if s.execute("building_demand", &p).is_ok() {
                out.push(("building_demand", p));
                break;
            }
        }
    }
    let named_to = out.len();
    out.push(("building_demand", json!({"dwellings": groups[0]})));
    out.push(("building_demand", json!({"dwellings": groups[1]})));
    out.push(("circuit", json!({"load": 4400, "length": 25000})));
    out.push(("circuit", json!({"load": 12000, "phases": "3FN", "system": "220/380", "length": 60000, "insulation": "epr"})));
    out.push(("circuit", json!({"load": 1200, "use": "lighting", "length": 15000, "grouped": 3})));
    out.push(("conduit_fill", json!({"section": 2.5, "circuits": 3})));
    out.push(("conduit_fill", json!({"section": 10, "phases": "3FN"})));
    out.push(("dps_check", json!({"region": "AQ2", "overhead": true, "earthing": "TT", "uo": 127})));
    out.push(("dps_check", json!({"region": "AQ3", "purpose": "both", "earthing": "TN-S", "uo": 220, "phases": 1, "system": "220/380"})));
    let named = (0..out.len()).map(|i| i < named_to).collect();
    (out, named)
}

fn leaves(v: &Value, path: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
    match v {
        Value::Number(_) => out.push(path.clone()),
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                path.push(json!(i));
                leaves(x, path, out);
                path.pop();
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                path.push(json!(k));
                leaves(x, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn at_mut<'a>(v: &'a mut Value, path: &[Value]) -> Option<&'a mut Value> {
    let mut at = v;
    for s in path {
        at = match s {
            Value::String(k) => at.get_mut(k.as_str())?,
            Value::Number(i) => at.get_mut(i.as_u64()? as usize)?,
            _ => return None,
        };
    }
    Some(at)
}

fn lib() -> &'static Lib {
    static L: OnceLock<Lib> = OnceLock::new();
    L.get_or_init(|| {
        let tmp = std::env::var("P_TMP").unwrap_or_else(|_| std::env::temp_dir().join("bimcraft-property").to_string_lossy().into_owned());
        std::fs::create_dir_all(&tmp).expect("tmp");
        let (probes, named) = build_probes();
        let mut s = Session::new();
        let answers: Vec<Value> = probes.iter().map(|(c, p)| ask(&mut s, c, p)).collect();
        for (((c, p), a), named) in probes.iter().zip(&answers).zip(&named) {
            assert!(!named || a.get("error").is_none(), "a probe the library refuses: {c} {p}: {a}");
        }
        let starter = settings_of(&mut s);
        let bases: Vec<String> = codes::of(E).filter(|c| c.amends.is_empty()).map(|c| c.code.clone()).collect();
        let amendments: Vec<String> = codes::of(E).filter(|c| !c.amends.is_empty()).map(|c| c.code.clone()).collect();
        // Mutations: one number of a rule halved or doubled, that a fresh project takes and that moves an answer.
        let mut muts: Vec<Mutation> = Vec::new();
        let mut rng = Rng(0xC0DE);
        for c in codes::of(E) {
            for key in TARGETS {
                let Some(sec) = c.section(key) else { continue };
                let mut f = Session::new();
                let setup = if c.amends.is_empty() { json!({"set": c.code}) } else { json!({"set": c.amends, "amendments": [c.code]}) };
                if f.execute("building_code", &bc(setup.clone())).is_err() {
                    continue;
                }
                let before: Vec<Value> = probes.iter().map(|(c, p)| ask(&mut f, c, p)).collect();
                let cap = if key == "circuits" { 10 } else { 6 };
                let mut got = 0;
                let mut rules: Vec<(&String, &Value)> = sec.iter().collect();
                let passes = if key == "unit_demand" || key == "entrance" { 40 } else { 3 };
                for _ in 0..passes {
                    for i in (1..rules.len()).rev() {
                        rules.swap(i, rng.below(i + 1));
                    }
                    for (rule, v) in &rules {
                        if got >= cap {
                            break;
                        }
                        let mut all = Vec::new();
                        leaves(v, &mut Vec::new(), &mut all);
                        if all.is_empty() {
                            continue;
                        }
                        let path = rng.pick(&all);
                        let mult = if rng.pct(50) { 0.5 } else { 2.0 };
                        let mut nv = (*v).clone();
                        if let Some(x) = at_mut(&mut nv, &path) {
                            let was = x.as_f64().unwrap_or(0.0);
                            let now = (was * mult * 1e6).round() / 1e6;
                            *x = if x.is_i64() && now.fract() == 0.0 { json!(now as i64) } else { json!(now) };
                        }
                        let mut g = Session::new();
                        let _ = g.execute("building_code", &bc(setup.clone()));
                        let rule: &str = rule.as_str();
                        let change = match WHOLES.contains(&key) {
                            true => json!({"amend": c.code, "rules": {key: {rule: nv.clone()}}}),
                            false => json!({"rules": {key: {rule: nv.clone()}}}),
                        };
                        if g.execute("building_code", &bc(change)).is_err() {
                            continue;
                        }
                        let after: Vec<Value> = probes.iter().map(|(c, p)| ask(&mut g, c, p)).collect();
                        let most = if passes > 3 { 3 } else { 1 };
                        if after != before
                            && muts.iter().filter(|m| m.holder == c.code && m.section == key && m.rule == rule).count() < most
                            && !muts.iter().any(|m| m.holder == c.code && m.section == key && m.rule == rule && m.value == nv)
                        {
                            muts.push(Mutation { holder: c.code.clone(), section: key.into(), rule: rule.to_string(), value: nv });
                            got += 1;
                        }
                    }
                }
            }
        }
        // Files of other disciplines.
        let mut other = Vec::new();
        let mut o = Session::new();
        for (i, p) in [
            json!({"discipline": "plumbing", "name": "OTHER-P"}),
            json!({"discipline": "architectural", "name": "P1"}),
            json!({"discipline": "structural", "name": "S1"}),
        ]
        .into_iter()
        .enumerate()
        {
            let path = format!("{tmp}/other{i}.xml");
            let mut p = p;
            p["path"] = json!(path);
            o.execute("building_code_export", &p).expect("export of another discipline");
            other.push(path);
        }
        eprintln!("library: {} probes, {} mutations, bases {:?}, amendments {:?}", probes.len(), muts.len(), bases, amendments);
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for m in &muts {
            *by.entry(format!("{}/{}", m.holder, m.section)).or_default() += 1;
        }
        eprintln!("mutations by holder/section: {by:?}");
        // Candidates are sampled against today's library. Fixed test cases below check the required edit paths.
        assert!(!muts.is_empty(), "no generated rule changes");
        Lib { probes, named, answers, muts, starter, bases, amendments, other, tmp }
    })
}

// ---------------------------------------------------------------------------------------------------------------- ledger

#[derive(Clone, Default, Debug)]
struct Own {
    amends: String,
    based_on: String,
    sections: BTreeMap<String, Map<String, Value>>,
}

#[derive(Clone, PartialEq, Debug)]
enum From {
    Lib,
    Layer(String),
    Project,
}

enum CodeRef<'a> {
    Own(&'a Own),
    Lib(&'static BuildingCode),
}

impl CodeRef<'_> {
    fn amends(&self) -> &str {
        match self {
            CodeRef::Own(o) => &o.amends,
            CodeRef::Lib(c) => &c.amends,
        }
    }
    fn based_on(&self) -> &str {
        match self {
            CodeRef::Own(o) => &o.based_on,
            CodeRef::Lib(c) => &c.based_on,
        }
    }
    fn section(&self, key: &str) -> Option<&Map<String, Value>> {
        match self {
            CodeRef::Own(o) => o.sections.get(key),
            CodeRef::Lib(c) => c.section(key),
        }
    }
    fn keys(&self) -> Vec<String> {
        match self {
            CodeRef::Own(o) => o.sections.keys().cloned().collect(),
            CodeRef::Lib(c) => c.sections.keys().cloned().collect(),
        }
    }
    fn sections(&self) -> BTreeMap<String, Map<String, Value>> {
        self.keys().into_iter().filter_map(|k| self.section(&k).map(|s| (k.clone(), s.clone()))).collect()
    }
}

#[derive(Clone, Default, Debug)]
struct Ledger {
    base: Option<String>,
    amendments: Vec<String>,
    own: BTreeMap<String, Own>,
    changes: BTreeMap<String, Map<String, Value>>,
}

type Resolved = (Map<String, Value>, BTreeMap<String, From>);

impl Ledger {
    fn find(&self, n: &str) -> Option<CodeRef<'_>> {
        self.own.get(n).map(CodeRef::Own).or_else(|| codes::find(E, n).map(CodeRef::Lib))
    }

    fn base_code(&self) -> String {
        match &self.base {
            Some(b) if self.find(b).is_some_and(|c| c.amends().is_empty()) => b.clone(),
            _ => codes::default_of(E).code.clone(),
        }
    }

    fn layers(&self) -> Vec<(String, CodeRef<'_>)> {
        let base = self.base_code();
        let chosen: Vec<(String, CodeRef)> =
            self.amendments.iter().filter_map(|a| self.find(a).map(|c| (a.clone(), c))).filter(|(_, c)| !c.amends().is_empty()).collect();
        let mut held = vec![base.clone()];
        let mut on = vec![false; chosen.len()];
        loop {
            let mut grew = false;
            for (i, (n, c)) in chosen.iter().enumerate() {
                if !on[i] && held.iter().any(|h| h == c.amends()) && !held.contains(n) {
                    on[i] = true;
                    held.push(n.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        let mut out = vec![(base.clone(), self.find(&base).expect("the base"))];
        out.extend(chosen.into_iter().zip(on).filter(|(_, on)| *on).map(|(c, _)| c));
        out
    }

    fn resolved(&self, key: &str, with_changes: bool) -> Resolved {
        let layers = self.layers();
        let whole_key = WHOLES.contains(&key);
        // A utility's or a country's method (`codes::WHOLE`) is one layer's alone: an own copy keeps the rules of the code it
        // was made from that it did not take out, and the layer replaces whatever is under it.
        let section_of = |n: &str, c: &CodeRef| -> Option<Map<String, Value>> {
            if whole_key { Self::whole_of(n, c, key) } else { c.section(key).cloned() }
        };
        let lay = |out: &mut Resolved| {
            for (n, c) in &layers {
                if let Some(sec) = section_of(n, c) {
                    if whole_key {
                        *out = Default::default();
                    }
                    for (k, v) in &sec {
                        out.0.insert(k.clone(), v.clone());
                        out.1.insert(k.clone(), From::Layer(n.clone()));
                    }
                }
            }
        };
        let mut out: Resolved = Default::default();
        lay(&mut out);
        let base_has = layers[0].1.section(key).is_some();
        let whole = |o: &Resolved| codes::section_def(E, key).is_none_or(|d| d.check(&Value::Object(o.0.clone())).is_ok());
        // The settings are entries by name: an amendment's few go over the library's others.
        let entries = SETTINGS.contains(&key);
        if (out.0.is_empty() || (!whole_key && !base_has && (entries || !whole(&out))))
            && let Some(l) = codes::library(E, key)
            && let Some(sec) = l.section(key)
        {
            out = Default::default();
            for (k, v) in sec {
                out.0.insert(k.clone(), v.clone());
                out.1.insert(k.clone(), From::Lib);
            }
            lay(&mut out);
        }
        if with_changes && let Some(ch) = self.changes.get(key) {
            for (k, v) in ch {
                out.0.insert(k.clone(), v.clone());
                out.1.insert(k.clone(), From::Project);
            }
        }
        out
    }

    fn snapshot(&self) -> Own {
        let layers = self.layers();
        let mut keys: BTreeSet<String> = layers.iter().flat_map(|(_, c)| c.keys()).collect();
        keys.extend(self.changes.keys().cloned());
        let base = &layers[0];
        let based_on = if base.1.based_on().is_empty() { base.0.clone() } else { base.1.based_on().to_string() };
        Own { amends: String::new(), based_on, sections: keys.into_iter().map(|k| (k.clone(), self.resolved(&k, true).0)).collect() }
    }

    /// The rules in force kept as codes called `name`: the snapshot, and where more than one layer holds a utility's or a
    /// country's method, each of those layers' methods as an amendment over it, in the layers' order (one code keeps one
    /// method a section), named after the code and the layer, never a name taken.
    fn saved(&self, name: &str) -> Option<Vec<(String, Own)>> {
        let layers = self.layers();
        let mut out = vec![(name.to_string(), self.snapshot())];
        let split: Vec<&str> = WHOLES.into_iter().filter(|k| layers.iter().filter(|(_, c)| c.section(k).is_some()).count() > 1).collect();
        if split.is_empty() {
            return Some(out);
        }
        let mut taken: Vec<String> = self.own.keys().map(|k| k.to_lowercase()).collect();
        taken.push(name.to_lowercase());
        for (n, c) in &layers {
            let sections: BTreeMap<String, Map<String, Value>> =
                split.iter().filter_map(|k| Self::whole_of(n, c, k).map(|w| ((*k).to_string(), w))).collect();
            if sections.is_empty() {
                continue;
            }
            let code = (1..1000).find_map(|i| {
                let suffix = if i == 1 { format!(" · {n}") } else { format!(" · {n} ({i})") };
                let room = codes::MAX_NAME.checked_sub(suffix.chars().count())?;
                let prefix: String = name.chars().take(room).collect();
                let candidate = format!("{}{suffix}", prefix.trim_end());
                let reserved =
                    codes::codes().iter().any(|c| c.code.eq_ignore_ascii_case(&candidate) || c.name.eq_ignore_ascii_case(&candidate));
                (candidate.trim() == candidate
                    && !candidate.chars().any(char::is_control)
                    && !reserved
                    && !taken.contains(&candidate.to_lowercase()))
                .then_some(candidate)
            })?;
            taken.push(code.to_lowercase());
            let based_on = if c.based_on().is_empty() { n.clone() } else { c.based_on().to_string() };
            out.push((code, Own { amends: name.to_string(), based_on, sections }));
        }
        Some(out)
    }

    fn trim(&mut self) {
        let keys: Vec<String> = self.changes.keys().cloned().collect();
        for key in keys {
            let own = self.resolved(&key, false).0;
            let mut ch = self.changes.remove(&key).unwrap_or_default();
            ch.retain(|k, v| own.get(k).is_none_or(|o| !same(o, v)));
            if !ch.is_empty() {
                self.changes.insert(key, ch);
            }
        }
    }

    fn amend(&mut self, t: &str, reset: &Value, rules: Option<&Map<String, Value>>) {
        let lib = codes::find(E, t);
        if !self.own.contains_key(t) {
            let Some(l) = lib else { return };
            self.own.insert(t.into(), Own { amends: l.amends.clone(), based_on: l.code.clone(), sections: CodeRef::Lib(l).sections() });
        }
        match reset {
            Value::Bool(true) => {
                if lib.is_some() {
                    self.own.remove(t);
                } else if let Some(o) = self.own.get_mut(t) {
                    o.sections.clear();
                }
                return;
            }
            Value::String(k) => {
                let o = self.own.get_mut(t).expect("own");
                o.sections.remove(k);
                if let Some(sec) = lib.and_then(|l| l.section(k)) {
                    o.sections.insert(k.clone(), sec.clone());
                }
                return;
            }
            _ => {}
        }
        for (key, rules) in rules.into_iter().flatten() {
            let from = self.resolved(key, true).1;
            let settings = SETTINGS.contains(&key.as_str());
            let mut section = self.own[t].sections.get(key).cloned().unwrap_or_default();
            for (k, v) in rules.as_object().into_iter().flatten() {
                // A layer under it names it, whether or not this one gives it too.
                let under = v.is_null() && settings && {
                    let mut below = self.clone();
                    if let Some(at) = below.amendments.iter().position(|a| a == t) {
                        below.amendments.truncate(at);
                    } else if let Some(o) = below.own.get_mut(t).and_then(|o| o.sections.get_mut(key)) {
                        o.remove(k);
                    }
                    below.resolved(key, false).0.get(k).is_some_and(|x| !x.is_null())
                };
                if v.is_null() && settings && (under || starter_has(key, k) || from.get(k) == Some(&From::Project)) {
                    section.insert(k.clone(), Value::Null);
                    if let Some(ch) = self.changes.get_mut(key.as_str()) {
                        ch.remove(k);
                    }
                } else if v.is_null() {
                    section.remove(k);
                } else {
                    section.insert(k.clone(), v.clone());
                }
            }
            let o = self.own.get_mut(t).expect("own");
            if section.is_empty() {
                o.sections.remove(key);
            } else {
                o.sections.insert(key.clone(), section);
            }
        }
    }

    /// `building_code` as the docs say it works, in the order its parts are laid.
    fn building_code(&mut self, p: &Value) {
        let s = |k: &str| p.get(k).and_then(Value::as_str).map(String::from);
        let set = s("set");
        if let Some(name) = s("create") {
            let over = set.clone().or(s("over")).unwrap_or_else(|| self.base_code());
            self.own.insert(name, Own { amends: over, ..Own::default() });
        }
        let amendments: Option<Vec<String>> =
            p.get("amendments").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
        if let Some(set) = &set {
            if self.base_code() != *set && amendments.is_none() {
                self.amendments.clear();
            }
            self.base = Some(set.clone());
            if p.get("keep_rules") != Some(&json!(true)) {
                self.changes.clear();
            }
        }
        if let Some(a) = amendments {
            self.amendments = a;
        }
        let reset = p.get("reset").cloned().unwrap_or(Value::Null);
        let rules = p.get("rules").and_then(Value::as_object);
        if let Some(t) = s("amend") {
            self.amend(&t, &reset, rules);
        } else {
            match &reset {
                Value::Bool(true) => self.changes.clear(),
                Value::String(k) => {
                    self.changes.remove(k);
                }
                _ => {}
            }
            for (key, rules) in rules.into_iter().flatten() {
                let ch = self.changes.entry(key.clone()).or_default();
                for (k, v) in rules.as_object().into_iter().flatten() {
                    ch.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(name) = s("save") {
            let saved = self.saved(&name).expect("engine saved a pack with no available amendment name");
            self.amendments = saved.iter().skip(1).map(|(n, _)| n.clone()).collect();
            for (n, own) in saved {
                self.own.insert(n, own);
            }
            self.base = Some(name);
            self.changes.clear();
        }
        if let Some(name) = s("delete")
            && self.own.contains_key(&name)
        {
            let held = self.base_code() == name || self.amendments.contains(&name);
            let copy = codes::find(E, &name).is_some();
            if held && !copy {
                let keys: BTreeSet<String> = self.layers().iter().flat_map(|(_, c)| c.keys()).collect();
                let before: Vec<(String, Map<String, Value>)> = keys.into_iter().map(|k| (k.clone(), self.resolved(&k, true).0)).collect();
                let stood: Vec<String> = self.layers().iter().skip(1).map(|(n, _)| n.clone()).collect();
                let gone = self.own.remove(&name).expect("own");
                self.amendments.retain(|a| *a != name);
                if self.base.as_deref() == Some(name.as_str()) {
                    self.base = codes::find(E, &gone.based_on).filter(|k| k.amends.is_empty()).map(|k| k.code.clone());
                }
                // One chosen that didn't stand over the code taken away, and would now, is no longer chosen.
                let stands: Vec<String> = self.layers().iter().skip(1).map(|(n, _)| n.clone()).collect();
                self.amendments.retain(|a| stood.contains(a) || !stands.contains(a));
                for (k, v) in before {
                    self.changes.insert(k, v);
                }
            } else {
                self.own.remove(&name);
            }
        }
        self.trim();
    }

    fn write_setting(&mut self, key: &str, name: &str, value: Option<Value>) {
        let layered = self.resolved(key, false).0;
        let ch = self.changes.entry(key.into()).or_default();
        match (value, layered.get(name)) {
            (Some(v), Some(was)) if same(was, &v) => {
                ch.remove(name);
            }
            (Some(v), _) => {
                ch.insert(name.into(), v);
            }
            (None, Some(_)) => {
                ch.insert(name.into(), Value::Null);
            }
            // One a new project has is taken away for good, so as not to come back on import.
            (None, None) if starter_has(key, name) => {
                ch.insert(name.into(), Value::Null);
            }
            (None, None) => {
                ch.remove(name);
            }
        }
        if ch.is_empty() {
            self.changes.remove(key);
        }
    }

    fn import(&mut self, name: &str, mut code: Own, set: bool) {
        // A rule the file leaves out of a section a module reads is the code's it is based on.
        let from = codes::find(E, &code.based_on).or_else(|| codes::find(E, &code.amends)).unwrap_or_else(|| codes::default_of(E));
        for (key, sec) in code.sections.iter_mut() {
            if codes::section_def(E, key).is_none_or(|d| d.read_by.is_empty()) {
                continue;
            }
            if WHOLES.contains(&key.as_str()) {
                continue;
            }
            if let Some(theirs) = from.section(key).or_else(|| codes::library(E, key).and_then(|c| c.section(key))) {
                for (k, v) in theirs {
                    sec.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
        }
        let amends = code.amends.clone();
        self.own.insert(name.into(), code);
        if set {
            if amends.is_empty() {
                self.base = Some(name.into());
                self.amendments.clear();
                self.changes.clear();
            } else if !self.amendments.iter().any(|a| a == name) {
                self.amendments.push(name.into());
            }
        }
    }

    /// A utility's or a country's method (`codes::WHOLE`) as layer `n` holds it: an own copy keeps the rules of the code it
    /// was made from that it did not take out.
    fn whole_of(n: &str, c: &CodeRef, key: &str) -> Option<Map<String, Value>> {
        let own = c.section(key)?;
        let origin = match c {
            CodeRef::Own(o) => codes::find(E, &o.based_on).or_else(|| codes::find(E, n)),
            CodeRef::Lib(_) => None,
        };
        let mut out = origin.and_then(|o| o.section(key)).cloned().unwrap_or_default();
        out.extend(own.clone());
        Some(out)
    }

    /// Select the method in the ledger before letting the command compute the answer.
    /// The method a probe's answer comes from, chosen from the ledger's layers as the commands' contract says: of the
    /// layers holding one for the identity the probe names, the one on top, the project's or BIMCraft's (a copy under
    /// BIMCraft's own is passed over).
    fn probe_method(&self, cmd: &str, p: &Value) -> Option<(&'static str, Map<String, Value>)> {
        let layers = self.layers();
        let named_in = |v: Option<&Value>, n: &str| v.and_then(Value::as_str).is_some_and(|x| x.eq_ignore_ascii_case(n));
        let lists = |w: &Map<String, Value>, field: &str, n: &str| {
            w.get(field).and_then(Value::as_array).is_some_and(|a| a.iter().any(|m| named_in(m.get("id").or(Some(m)), n)))
        };
        match cmd {
            "demand" => {
                let utility = p["utility"].as_str().expect("demand probe names a utility");
                // A member of a group answers by its group's methods too: the groups that list it, held or BIMCraft's.
                let mut groups: Vec<String> = layers
                    .iter()
                    .filter_map(|(n, c)| Self::whole_of(n, c, "entrance"))
                    .filter(|w| lists(w, "members", utility))
                    .filter_map(|w| w.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect();
                if let Some((g, Some(_))) = bimcraft_core::entrance::find(utility) {
                    groups.push(g.id.clone());
                }
                // Layer by layer from the top; in one layer, the entrance before the unit demand.
                layers.iter().rev().find_map(|(n, c)| {
                    if let Some(w) = Self::whole_of(n, c, "entrance")
                        && (named_in(w.get("id"), utility) || lists(&w, "members", utility))
                    {
                        return Some(("entrance", w));
                    }
                    Self::whole_of(n, c, "unit_demand")
                        .filter(|w| {
                            std::iter::once(utility).chain(groups.iter().map(String::as_str)).any(|g| named_in(w.get("utility"), g))
                        })
                        .map(|w| ("unit_demand", w))
                })
            }
            "building_demand" => {
                let key = "building_demand";
                let named = |w: &Map<String, Value>| {
                    if let Some(s) = p["standard"].as_str() {
                        w.get("standard").and_then(Value::as_str) == Some(s)
                    } else if let Some(u) = p["utility"].as_str() {
                        lists(w, "utilities", u)
                    } else {
                        true
                    }
                };
                // Naming none: the method in force, if the project holds one.
                if p.get("standard").is_none() && p.get("utility").is_none() {
                    let (top, from) = self.resolved(key, true);
                    return (from.values().any(|f| *f != From::Lib) && !top.is_empty()).then_some((key, top));
                }
                layers.iter().rev().find_map(|(n, c)| Self::whole_of(n, c, key).filter(named).map(|w| (key, w)))
            }
            _ => None,
        }
    }

    /// The non-method rules of a fresh project, reduced to a single base.
    fn injected(&self) -> BuildingCode {
        let mut own = BuildingCode { code: "ORACLE".into(), name: "ORACLE".into(), discipline: E, ..BuildingCode::default() };
        for def in codes::sections().iter().filter(|d| d.discipline == E && !SETTINGS.contains(&d.key) && !WHOLES.contains(&d.key)) {
            let (v, from) = self.resolved(def.key, true);
            if from.values().any(|f| *f != From::Lib) {
                own.sections.insert(def.key.into(), Value::Object(v));
            }
        }
        own
    }
}

// ------------------------------------------------------------------------------------------------------------------ ops

#[derive(Clone, Debug)]
enum Op {
    Bc(Value),
    Setting(&'static str, Value),
    /// slot, kind 0 in force, 1 all, 2 a code named
    Export(usize, u8, String),
    Import(usize, bool),
    ImportOther(usize, bool),
    RoundTrip(u8),
    /// A distribution board on a distribution system.
    Board(String),
}

/// What a board keeps in the project when the codes don't name it (`from_codes`), by settings section: its system, and the
/// voltages that system names.
fn kept_by_boards(s: &Session) -> [BTreeSet<String>; 3] {
    let on = boards_on(s);
    let voltages = on
        .iter()
        .filter_map(|k| s.model.distribution_systems.get(k))
        .flat_map(|d| d.voltage_names().cloned().collect::<Vec<_>>())
        .collect();
    [voltages, on, BTreeSet::new()]
}

/// The distribution systems the project's boards are on.
fn boards_on(s: &Session) -> BTreeSet<String> {
    use bimcraft_core::model::{Component, Element};
    s.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Component(Component { panel: Some(p), .. }) => p.distribution.clone(),
            _ => None,
        })
        .collect()
}

fn shape(op: &Op) -> String {
    let keys = |p: &Value| {
        let mut k: Vec<String> = p
            .as_object()
            .map(|o| {
                o.iter()
                    .filter(|(k, _)| *k != "discipline" && *k != "name")
                    .map(|(k, v)| match (k.as_str(), v) {
                        ("rules", Value::Object(r)) => {
                            let sec = r.keys().next().cloned().unwrap_or_default();
                            let null = r.values().next().and_then(Value::as_object).is_some_and(|o| o.values().any(Value::is_null));
                            format!(
                                "rules[{}{}]",
                                if SETTINGS.contains(&sec.as_str()) { "settings" } else { "table" },
                                if null { "=null" } else { "" }
                            )
                        }
                        ("reset", Value::String(_)) => "reset[section]".into(),
                        _ => k.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        k.sort();
        k.join("+")
    };
    match op {
        Op::Bc(p) => format!("building_code{{{}}}", keys(p)),
        Op::Setting(c, p) => format!("{c}{{{}}}", keys(p)),
        Op::Export(_, k, _) => format!("export[{}]", ["in force", "all", "code"][*k as usize]),
        Op::Import(_, set) => format!("import[set={set}]"),
        Op::ImportOther(i, set) => format!("import-other[{i},set={set}]"),
        Op::RoundTrip(k) => format!("roundtrip[{k}]"),
        Op::Board(_) => "board".into(),
    }
}

fn said(op: &Op) -> String {
    match op {
        Op::Bc(p) => format!("building_code {}", short_op(p)),
        Op::Setting(c, p) => format!("{c} {p}"),
        Op::Export(slot, k, code) => match k {
            0 => format!("building_code_export {{discipline: electrical, name: F{slot}, path: f{slot}.xml}}"),
            1 => format!("building_code_export {{all: true, name: F{slot}, path: f{slot}.xml}}"),
            _ => format!("building_code_export {{discipline: electrical, code: {code}, name: F{slot}, path: f{slot}.xml}}"),
        },
        Op::Import(slot, set) => format!("building_code_import {{path: f{slot}.xml, set: {set}}}"),
        Op::ImportOther(i, set) => format!("building_code_import {{path: other{i}.xml (another discipline), set: {set}}}"),
        Op::RoundTrip(k) => format!("(round trip check {k})"),
        Op::Board(sys) => format!("component {{family: Distribution Board}} + panel {{distribution: {sys}}}"),
    }
}

fn short_op(p: &Value) -> String {
    let t = p.to_string();
    if t.chars().count() > 300 { format!("{}…", t.chars().take(300).collect::<String>()) } else { t }
}

#[derive(Clone, Debug)]
struct Fail {
    inv: &'static str,
    kind: String,
    detail: String,
    step: usize,
}

struct Run {
    s: Session,
    l: Ledger,
    files: [Option<Vec<(String, Own)>>; 4],
    dir: String,
    nochange: bool,
    memo: HashMap<String, Vec<Value>>,
    /// Every failure so far; the ones that leave the ledger and the session in step (I4, I6, kept) don't end the sequence.
    fails: Vec<Fail>,
    active: Vec<(String, String)>,
}

fn copy_of(s: &Session) -> Session {
    let mut c = Session::new();
    *c.model_mut() = (*s.model).clone();
    c
}

fn rules_of(s: &mut Session) -> BTreeMap<String, Map<String, Value>> {
    let mut out = BTreeMap::new();
    if let Ok(v) = s.execute("code_rules", &json!({"discipline": "electrical"})) {
        for sec in v.get("sections").and_then(Value::as_array).into_iter().flatten() {
            let key = sec.get("section").and_then(Value::as_str).unwrap_or_default().to_string();
            let rules: Map<String, Value> = sec
                .get("rules")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|r| {
                    (r.get("rule").and_then(Value::as_str).unwrap_or_default().to_string(), r.get("value").cloned().unwrap_or(Value::Null))
                })
                .collect();
            out.insert(key, rules);
        }
    }
    out
}

/// A number of an answer that is negative, or a thousand times the library's, or gone where the library has one.
fn insane(lib: &Value, a: &Value, at: &str) -> Option<String> {
    match (lib, a) {
        (Value::Number(l), Value::Number(x)) => {
            let (l, x) = (l.as_f64().unwrap_or(0.0), x.as_f64().unwrap_or(f64::NAN));
            (!x.is_finite() || (x < 0.0 && l >= 0.0) || x.abs() > 1000.0 * l.abs().max(1.0)).then(|| format!("{at}: {x} (library {l})"))
        }
        (Value::Object(l), Value::Object(x)) => {
            x.iter().find_map(|(k, v)| insane(l.get(k).unwrap_or(&Value::Null), v, &format!("{at}.{k}")))
        }
        (Value::Array(l), Value::Array(x)) => {
            x.iter().enumerate().find_map(|(i, v)| insane(l.get(i).unwrap_or(&Value::Null), v, &format!("{at}[{i}]")))
        }
        (_, Value::Number(x)) => x.as_f64().filter(|x| !x.is_finite() || *x < 0.0).map(|x| format!("{at}: {x}")),
        (_, Value::Object(x)) => x.iter().find_map(|(k, v)| insane(&Value::Null, v, &format!("{at}.{k}"))),
        (_, Value::Array(x)) => x.iter().enumerate().find_map(|(i, v)| insane(&Value::Null, v, &format!("{at}[{i}]"))),
        _ => None,
    }
}

fn failed(inv: &'static str, kind: String, detail: String) -> Option<Fail> {
    Some(Fail { inv, kind, detail, step: 0 })
}

impl Run {
    fn new(dir: String, nochange: bool) -> Run {
        std::fs::create_dir_all(&dir).expect("dir");
        Run {
            s: Session::new(),
            l: Ledger::default(),
            files: Default::default(),
            dir,
            nochange,
            memo: HashMap::new(),
            fails: Vec::new(),
            active: Vec::new(),
        }
    }

    /// One op and the checks after it; whether the sequence ends here.
    fn step(&mut self, i: usize, op: &Op) -> bool {
        let n = self.fails.len();
        let hard = self.apply(op).or_else(|| self.check());
        let ends = hard.is_some();
        self.fails.extend(hard);
        for f in &mut self.fails[n..] {
            f.step = i;
            f.kind = format!("{} <- {}", f.kind, shape(op));
        }
        ends
    }

    fn path(&self, slot: usize) -> String {
        format!("{}/f{slot}.xml", self.dir)
    }

    fn apply(&mut self, op: &Op) -> Option<Fail> {
        match op {
            Op::Bc(p) => {
                // `save` and `delete` of one of the project's own keep the rules in force.
                let keeps = p.get("save").is_some()
                    || p.get("delete").and_then(Value::as_str).is_some_and(|n| codes::find(E, n).is_none() && self.l.own.contains_key(n));
                let before = keeps.then(|| rules_of(&mut self.s));
                let project_method = p.get("amend").is_none()
                    && p.get("rules").and_then(Value::as_object).is_some_and(|r| WHOLES.iter().any(|k| r.contains_key(*k)));
                let before_refusal = project_method.then(|| self.s.model.codes.clone());
                // An edit of rules alone never takes from a board the system in force it is on, nor changes it, unless it
                // gives that system's rules itself (I7).
                let rules_only = p.as_object().is_some_and(|o| o.keys().all(|k| ["discipline", "rules", "amend"].contains(&k.as_str())));
                let on = boards_on(&self.s);
                let systems = |s: &Session| s.model.resolve_code(E, "distribution_systems").value;
                let systems_before = systems(&self.s);
                // Where the ledger names the amendments chosen as the engine does (a `save` renames them), a delete keeps
                // agreeing.
                let agreed = self.s.model.codes.choice(E).amendments == self.l.amendments;
                let result = self.s.execute("building_code", p);
                if let Some(before) = before_refusal {
                    if result.is_ok() || self.s.model.codes != before {
                        return failed("I2", "whole method accepted as a project change".into(), short(p));
                    }
                    return None;
                }
                if result.is_ok() {
                    self.l.building_code(p);
                    if rules_only {
                        let after = systems(&self.s);
                        // What the edit gives the system itself is the edit's; anything else, a null for it included, isn't.
                        let given = |k: &String| p["rules"]["distribution_systems"].get(k.as_str()).is_some_and(|v| !v.is_null());
                        for sys in on.iter().filter(|k| systems_before.get(*k).is_some_and(|v| !v.is_null()) && !given(k)) {
                            if systems_before.get(sys) != after.get(sys) {
                                let d = format!(
                                    "{sys}: {} then {}",
                                    short(systems_before.get(sys).unwrap_or(&Value::Null)),
                                    short(after.get(sys).unwrap_or(&Value::Null))
                                );
                                self.fails.extend(failed("I7", "kept:a board's system".into(), d));
                            }
                        }
                    }
                    if let Some(before) = before {
                        let after = rules_of(&mut self.s);
                        for (key, rules) in &before {
                            let what = if SETTINGS.contains(&key.as_str()) { "settings" } else { key.as_str() };
                            if let Some(d) =
                                diff(&Value::Object(rules.clone()), &Value::Object(after.get(key).cloned().unwrap_or_default()), key)
                            {
                                self.fails.extend(failed("I2", format!("kept:{what}"), d));
                                break;
                            }
                        }
                        // A delete keeps the amendments chosen the ledger keeps: none chosen is lost, none dropped is kept.
                        let chosen = self.s.model.codes.choice(E).amendments.clone();
                        if agreed && p.get("delete").is_some() && chosen != self.l.amendments {
                            self.fails.extend(failed("I2", "kept:chosen".into(), format!("{chosen:?} not {:?}", self.l.amendments)));
                        }
                    }
                }
            }
            Op::Board(sys) => {
                if let Ok(r) = self.s.execute("component", &json!({"family": "Distribution Board", "type": "12 circuits", "at": [0, 0]})) {
                    let _ = self.s.execute("panel", &json!({"id": r["id"], "distribution": sys}));
                }
            }
            Op::Setting(cmd, p) => {
                // The systems that supply a voltage renamed name it again, in the project and in its code.
                let old = p.get("name").and_then(Value::as_str).unwrap_or_default();
                let supplying: Vec<String> = if *cmd == "voltage" {
                    let all = self.s.model.distribution_systems.clone();
                    all.iter().filter(|(_, d)| d.voltage_names().any(|v| v == old)).map(|(k, _)| k.clone()).collect()
                } else {
                    Vec::new()
                };
                if let Ok(r) = self.s.execute(cmd, p) {
                    let renamed = r.get("name").and_then(Value::as_str).is_some_and(|n| n != old);
                    if renamed {
                        for k in &supplying {
                            let d = self.s.model.distribution_systems.get(k).map(|d| json!(d));
                            self.l.write_setting("distribution_systems", k, d);
                        }
                    }
                    let (_, key, field) = LISTS.iter().find(|(c, ..)| c == cmd).expect("a settings command");
                    let name = p.get("name").and_then(Value::as_str).unwrap_or_default();
                    if p.get("delete") == Some(&json!(true)) {
                        self.l.write_setting(key, name, None);
                    } else {
                        let now = r.get("name").and_then(Value::as_str).unwrap_or(name).to_string();
                        if now != name {
                            self.l.write_setting(key, name, None);
                        }
                        self.l.write_setting(key, &now, r.get(*field).cloned());
                    }
                }
            }
            Op::Export(slot, kind, code) => {
                let name = format!("F{slot}");
                let mut p = json!({"path": self.path(*slot), "name": name});
                let held: Option<Vec<(String, Own)>> = match kind {
                    0 => {
                        p["discipline"] = json!("electrical");
                        self.l.saved(&name)
                    }
                    1 => {
                        p["all"] = json!(true);
                        self.l.saved(&name)
                    }
                    _ => {
                        p["discipline"] = json!("electrical");
                        p["code"] = json!(code);
                        self.l.find(code).map(|c| {
                            let based_on = if c.based_on().is_empty() { code.clone() } else { c.based_on().to_string() };
                            vec![(name.clone(), Own { amends: c.amends().to_string(), based_on, sections: c.sections() })]
                        })
                    }
                };
                match self.s.execute("building_code_export", &p) {
                    Ok(_) => {
                        if held.is_none() {
                            return failed("I4", "export accepted without a ledger snapshot".into(), short(&p));
                        }
                        self.files[*slot] = held;
                    }
                    Err(_) => self.files[*slot] = None,
                }
            }
            Op::Import(slot, set) => {
                if let Some(held) = self.files[*slot].clone()
                    && let Ok(r) = self.s.execute("building_code_import", &json!({"path": self.path(*slot), "set": set}))
                {
                    for (name, code) in held {
                        self.l.import(&name, code, *set);
                    }
                    if r.get("left_out").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
                        // Said: the ledger can't follow what was left out; the sequence ends here, unjudged.
                        let f = failed("I4", "import left a rule out, and said so".into(), short(&r["left_out"]));
                        self.fails.extend(f);
                        return failed("-", "left_out said".into(), short(&r));
                    }
                }
            }
            Op::ImportOther(i, set) => {
                let _ = self.s.execute("building_code_import", &json!({"path": lib().other[*i], "set": set}));
            }
            Op::RoundTrip(k) => {
                let f = self.roundtrip(*k);
                self.fails.extend(f);
            }
        }
        None
    }

    fn oracle(&mut self) -> Vec<Value> {
        self.oracle_for(&lib().probes)
    }

    fn oracle_for(&mut self, probes: &[(&str, Value)]) -> Vec<Value> {
        let base = self.l.injected();
        let methods: Vec<_> = probes.iter().map(|(cmd, p)| self.l.probe_method(cmd, p)).collect();
        let key = serde_json::to_string(&(&base.sections, &methods)).expect("oracle key");
        if let Some(a) = self.memo.get(&key) {
            return a.clone();
        }
        let oracle_codes = |base: BuildingCode| {
            let mut codes = Codes::default();
            codes.electrical.code = Some(base.code.clone());
            codes.own.push(base);
            Shared::new(codes)
        };
        let without_method = oracle_codes(base.clone());
        let mut f = Session::new();
        let a: Vec<Value> = probes
            .iter()
            .zip(methods)
            .map(|((cmd, p), method)| {
                f.model_mut().codes = match method {
                    Some((key, w)) => {
                        let mut code = base.clone();
                        code.sections.insert(key.into(), Value::Object(w));
                        oracle_codes(code)
                    }
                    None => without_method.clone(),
                };
                ask(&mut f, cmd, p)
            })
            .collect();
        if self.memo.len() > 500 {
            self.memo.clear();
        }
        self.memo.insert(key, a.clone());
        a
    }

    fn check(&mut self) -> Option<Fail> {
        // rules
        let live = rules_of(&mut self.s);
        for (key, rules) in &live {
            let exp = self.l.resolved(key, true).0;
            if let Some(d) = diff(&Value::Object(exp.clone()), &Value::Object(rules.clone()), key) {
                let rule = d.split(':').next().unwrap_or("").split('.').nth(1).unwrap_or("").split('[').next().unwrap_or("").to_string();
                let lost = exp.get(&rule).is_some_and(|e| rules.get(&rule).is_none_or(|a| !same(e, a)));
                let what = if SETTINGS.contains(&key.as_str()) { "settings".to_string() } else { key.clone() };
                return failed(if lost { "I2" } else { "I1" }, format!("rules:{what}"), d);
            }
        }
        // answers
        let exp = self.oracle();
        let l = lib();
        for (i, (cmd, p)) in l.probes.iter().enumerate() {
            let a = ask(&mut self.s, cmd, p);
            if let Some(d) = diff(&exp[i], &a, "") {
                return failed("I1", format!("answer:{cmd}"), format!("{cmd} {p} {d}"));
            }
            if let Some(d) = insane(&l.answers[i], &a, "") {
                return failed("I3", format!("sane:{cmd}"), format!("{cmd} {p} {d}"));
            }
            if self.nochange
                && l.named[i]
                && let Some(d) = diff(&l.answers[i], &a, "")
            {
                return failed("I5", format!("library:{cmd}"), format!("{cmd} {p} {d}"));
            }
        }
        let f = self.check_settings();
        self.fails.extend(f);
        None
    }

    /// The model's settings against the codes': each thing wrong is said once, at the step it first shows.
    fn check_settings(&mut self) -> Vec<Fail> {
        let l = lib();
        let model = settings_of(&mut self.s);
        // A system a board is on stays in the project when the codes switched stop naming it, with the voltages it names.
        let by_boards = kept_by_boards(&self.s);
        let kept = |i: usize, name: &String| by_boards[i].contains(name);
        let mut now: Vec<(String, String)> = Vec::new();
        for (i, key) in SETTINGS.iter().enumerate() {
            let says = self.l.resolved(key, true).0;
            for (name, v) in &says {
                match (v.is_null(), model[i].get(name)) {
                    (true, Some(_)) if kept(i, name) => {}
                    (true, Some(_)) => now.push((format!("settings:{key}:taken away by the codes, still in the model"), name.clone())),
                    (false, None) => now.push((format!("settings:{key}:named by the codes, not in the model"), name.clone())),
                    (false, Some(m)) if !same(v, m) => now.push((
                        format!("settings:{key}:the model's differs from the codes'"),
                        format!("{name}: codes {} model {}", short(v), short(m)),
                    )),
                    _ => {}
                }
            }
            for (name, m) in &model[i] {
                if says.contains_key(name) {
                    continue;
                }
                // A board keeps an entry no code names, and the system it is on as it was; a voltage a new project has still
                // comes back as a new project has it.
                match l.starter[i].get(name) {
                    None if kept(i, name) => {}
                    Some(_) if i == 1 && kept(i, name) => {}
                    None => now.push((format!("settings:{key}:stale entry no code in force names"), name.clone())),
                    Some(st) if !same(st, m) => {
                        now.push((format!("settings:{key}:stale value no code in force gives"), format!("{name}: {}", short(m))));
                    }
                    _ => {}
                }
            }
        }
        let new: Vec<Fail> =
            now.iter().filter(|x| !self.active.contains(x)).filter_map(|(k, d)| failed("I6", k.clone(), d.clone())).collect();
        self.active = now;
        new
    }

    fn roundtrip(&mut self, kind: u8) -> Option<Fail> {
        let path = format!("{}/rt.xml", self.dir);
        let fail = |kind: String, detail: String| failed("I4", kind, detail);
        let mut c = copy_of(&self.s);
        let export = match kind {
            0 | 2 => json!({"discipline": "electrical", "name": "RT", "path": path}),
            _ => json!({"all": true, "name": "RT", "path": path}),
        };
        if let Err(e) = c.execute("building_code_export", &export) {
            return fail(format!("rt{kind}:export refused"), e);
        }
        let mut target = if kind == 0 || kind == 3 { Session::new() } else { c };
        let set = kind != 2;
        match target.execute("building_code_import", &json!({"path": path, "set": set})) {
            Err(e) => return fail(format!("rt{kind}:import refused"), e),
            Ok(r) if r.get("left_out").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) => {
                let what = if r["left_out"].to_string().contains("rule `") { "a rule" } else { "something else" };
                return fail(format!("rt{kind}:import left {what} out, and said so"), short(&r["left_out"]));
            }
            Ok(_) => {}
        }
        let (live, got) = (rules_of(&mut self.s), rules_of(&mut target));
        for (key, rules) in &live {
            let what = if SETTINGS.contains(&key.as_str()) { "settings" } else { key.as_str() };
            if let Some(d) = diff(&Value::Object(rules.clone()), &Value::Object(got.get(key).cloned().unwrap_or_default()), key) {
                return fail(format!("rt{kind}:rules:{what}"), d);
            }
        }
        for (cmd, p) in &lib().probes {
            let (a, b) = (ask(&mut self.s, cmd, p), ask(&mut target, cmd, p));
            if let Some(d) = diff(&a, &b, "") {
                return fail(format!("rt{kind}:answer:{cmd}"), format!("{cmd} {p} {d}"));
            }
        }
        let (a, b) = (settings_of(&mut self.s), settings_of(&mut target));
        // What only a board keeps, and no code in force names, is not in the codes; a new project has no board.
        let by_boards = if kind == 0 || kind == 3 { kept_by_boards(&self.s) } else { Default::default() };
        let named = |i: usize, n: &String| self.s.model.resolve_code(E, SETTINGS[i]).value.get(n).is_some_and(|v| !v.is_null());
        for i in 0..3 {
            for (name, v) in a[i].iter().filter(|(n, _)| !by_boards[i].contains(*n) || named(i, n)) {
                match b[i].get(name) {
                    None => return fail(format!("rt{kind}:model {} lost", SETTINGS[i]), name.clone()),
                    Some(x) if !same(v, x) => {
                        return fail(
                            format!("rt{kind}:model {} changed", SETTINGS[i]),
                            format!("{name}: {} became {}", short(v), short(x)),
                        );
                    }
                    _ => {}
                }
            }
            if let Some(name) = b[i].keys().find(|n| !a[i].contains_key(*n)) {
                return fail(format!("rt{kind}:model {} came back", SETTINGS[i]), name.clone());
            }
        }
        None
    }
}

/// The failures of a sequence, up to the first that ends it.
fn run(ops: &[Op], dir: &str, nochange: bool) -> Vec<Fail> {
    let mut r = Run::new(dir.to_string(), nochange);
    for (i, op) in ops.iter().enumerate() {
        if r.step(i, op) {
            break;
        }
    }
    r.fails
}

fn sig(f: &Fail) -> String {
    format!("{} {}", f.inv, f.kind)
}

/// Two amendments of the project's own, chosen in a random order, that both give `48 V`, each its own; the second also
/// gives `48 DC`, which a board is on.
fn shared_start(r: &mut Rng) -> Vec<Op> {
    let (a, b) = if r.pct(50) { (OWN_NAMES[0], OWN_NAMES[1]) } else { (OWN_NAMES[1], OWN_NAMES[0]) };
    let low = json!({"nominal": 48, "min": 43.2, "max": 52.8});
    let high = json!({"nominal": 48, "min": 44, "max": 52});
    vec![
        Op::Bc(bc(json!({"create": a, "amendments": [a]}))),
        Op::Bc(bc(json!({"create": b, "amendments": [a, b]}))),
        Op::Bc(bc(json!({"amend": a, "rules": {"voltages": {"48 V": low}}}))),
        Op::Bc(bc(json!({"amend": b, "rules": {"voltages": {"48 V": high}}}))),
        // A system only the upper one gives, and a board on it.
        Op::Bc(bc(json!({"amend": b, "rules": {"distribution_systems": {"48 DC": {"phases": 1, "wires": 2, "l_n": "48 V"}}}}))),
        Op::Board("48 DC".into()),
    ]
}

fn gen_op(r: &mut Rng, l: &Ledger, nochange: bool) -> Op {
    let lb = lib();
    let held: Vec<String> = l.layers().iter().skip(1).map(|(n, _)| n.clone()).collect();
    let own: Vec<String> = l.own.keys().cloned().collect();
    let own_amend: Vec<String> = l.own.iter().filter(|(_, o)| !o.amends.is_empty()).map(|(n, _)| n.clone()).collect();
    let mut bases: Vec<String> = lb.bases.clone();
    bases.extend(l.own.iter().filter(|(_, o)| o.amends.is_empty()).map(|(n, _)| n.clone()));
    let mut cands: Vec<String> = lb.amendments.clone();
    cands.extend(own_amend.iter().cloned());
    let some_amendments = |r: &mut Rng| -> Vec<String> {
        let n = r.below(4);
        let mut out: Vec<String> = Vec::new();
        for _ in 0..n {
            let a = if !own_amend.is_empty() && r.pct(35) { r.pick(&own_amend) } else { r.pick(&cands) };
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    };
    let setting_rule = |r: &mut Rng| -> (&'static str, &'static str, Value) {
        let all: [(&str, &str, Value); 12] = [
            ("voltages", "220 V", json!({"nominal": 220, "min": 190, "max": 250})),
            ("voltages", "48 V", json!({"nominal": 48, "min": 43.2, "max": 52.8})),
            ("voltages", "230 V", Value::Null),
            ("voltages", "127 V", Value::Null),
            ("voltages", "48 V", Value::Null),
            ("distribution_systems", "48 DC", json!({"phases": 1, "wires": 2, "l_n": "48 V"})),
            ("distribution_systems", "127/220 V", json!({"phases": 3, "wires": 4, "l_l": "220 V", "l_n": "127 V", "ground": "TT"})),
            ("distribution_systems", "230/400 V", Value::Null),
            ("distribution_systems", "48 DC", Value::Null),
            (
                "wire_types",
                "Al X",
                json!({"material": "aluminium", "temperature": 90, "insulation": "XLPE", "sizes": [{"section": 16, "ampacity": 60}, {"section": 25, "ampacity": 80}]}),
            ),
            ("wire_types", "Cu PVC 70 °C", Value::Null),
            ("wire_types", "Al X", Value::Null),
        ];
        r.pick(&all)
    };
    // A board on a system in force, so the codes' edits meet a system an element is on.
    if !nochange && r.pct(6) {
        let systems: Vec<String> =
            l.resolved("distribution_systems", true).0.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, _)| k).collect();
        if !systems.is_empty() {
            return Op::Board(r.pick(&systems));
        }
    }
    loop {
        let w = r.below(100);
        let op = match w {
            0..=9 => {
                let mut p = json!({"set": r.pick(&bases)});
                if r.pct(30) {
                    p["keep_rules"] = json!(true);
                }
                if r.pct(40) {
                    p["amendments"] = json!(some_amendments(r));
                }
                Op::Bc(bc(p))
            }
            10..=22 => Op::Bc(bc(json!({"amendments": some_amendments(r)}))),
            23..=34 if !nochange => {
                if r.pct(30) {
                    let (sec, name, v) = setting_rule(r);
                    Op::Bc(bc(json!({"rules": {sec: {name: v}}})))
                } else {
                    let m = &lb.muts[r.below(lb.muts.len())];
                    let v = if r.pct(20) {
                        codes::find(E, &m.holder)
                            .and_then(|c| c.section(&m.section))
                            .and_then(|s| s.get(&m.rule))
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else {
                        m.value.clone()
                    };
                    let (sec, rule) = (m.section.as_str(), m.rule.as_str());
                    // A method is changed in the code holding it, where the project holds that code; else the project's
                    // change is tried, and refused.
                    match WHOLES.contains(&sec) && held.contains(&m.holder) {
                        true => Op::Bc(bc(json!({"amend": m.holder, "rules": {sec: {rule: v}}}))),
                        false => Op::Bc(bc(json!({"rules": {sec: {rule: v}}}))),
                    }
                }
            }
            35..=48 if !nochange && !held.is_empty() => {
                let t = r.pick(&held);
                // A setting a chosen amendment gives (another, or this one), given here too or taken away here: the same name
                // in two layers, where which one stands over the other decides what is in force.
                let shared: Vec<(String, String, Value)> = held
                    .iter()
                    .filter_map(|h| l.own.get(h))
                    .flat_map(|o| {
                        SETTINGS.iter().flat_map(move |sec| {
                            o.sections
                                .get(*sec)
                                .into_iter()
                                .flatten()
                                .filter(|(_, v)| !v.is_null())
                                .map(move |(n, v)| (sec.to_string(), n.clone(), v.clone()))
                        })
                    })
                    .collect();
                let mut p = if !shared.is_empty() && r.pct(40) {
                    let (sec, name, v) = shared[r.below(shared.len())].clone();
                    let v = if r.pct(50) { Value::Null } else { v };
                    json!({"amend": t, "rules": {sec: {name: v}}})
                } else if r.pct(25) {
                    let (sec, name, v) = setting_rule(r);
                    json!({"amend": t, "rules": {sec: {name: v}}})
                } else {
                    let mine: Vec<&Mutation> = lb.muts.iter().filter(|m| m.holder == t).collect();
                    let m = if !mine.is_empty() && r.pct(65) { mine[r.below(mine.len())] } else { &lb.muts[r.below(lb.muts.len())] };
                    let v = if r.pct(15) { Value::Null } else { m.value.clone() };
                    let (sec, rule) = (m.section.as_str(), m.rule.as_str());
                    // Another code's method copied whole into this one, changed (an own code that holds none, or a utility's
                    // copy taking another's): two layers then hold a method for one identity, which only the order decides.
                    let whole = WHOLES.contains(&sec) && m.holder != t && r.pct(50);
                    match codes::find(E, &m.holder).and_then(|c| c.section(sec)).filter(|_| whole) {
                        Some(full) => {
                            let mut full = full.clone();
                            full.insert(rule.to_string(), m.value.clone());
                            json!({"amend": t, "rules": {sec: full}})
                        }
                        None => json!({"amend": t, "rules": {sec: {rule: v}}}),
                    }
                };
                if r.pct(10) {
                    p["amendments"] = json!(some_amendments(r));
                }
                Op::Bc(bc(p))
            }
            49..=53 if !held.is_empty() => {
                let t = r.pick(&held);
                let reset = if r.pct(50) {
                    json!(true)
                } else {
                    json!(r.pick(&["unit_demand", "entrance", "building_demand", "voltages", "circuits"]))
                };
                Op::Bc(bc(json!({"amend": t, "reset": reset})))
            }
            54..=57 => {
                let reset = if r.pct(50) {
                    json!(true)
                } else {
                    json!(r.pick(&["unit_demand", "entrance", "building_demand", "voltages", "circuits", "wire_types"]))
                };
                Op::Bc(bc(json!({"reset": reset})))
            }
            58..=63 => {
                let name = r.pick(&OWN_NAMES[..3]);
                let mut p = json!({"create": name});
                if r.pct(60) {
                    let mut a = l.amendments.clone();
                    a.push(name.to_string());
                    p["amendments"] = json!(a);
                }
                if r.pct(25) && !held.is_empty() {
                    p["over"] = json!(r.pick(&held));
                }
                Op::Bc(bc(p))
            }
            64..=67 => Op::Bc(bc(json!({"save": r.pick(&OWN_NAMES[3..])}))),
            68..=72 if !own.is_empty() => Op::Bc(bc(json!({"delete": r.pick(&own)}))),
            73..=78 => {
                let slot = r.below(4);
                match r.below(3) {
                    2 => {
                        let mut all: Vec<String> = own.clone();
                        all.extend(held.iter().cloned());
                        all.push(r.pick(&lb.amendments));
                        Op::Export(slot, 2, r.pick(&all))
                    }
                    k => Op::Export(slot, k as u8, String::new()),
                }
            }
            79..=85 => Op::Import(r.below(4), r.pct(55)),
            86..=88 => Op::ImportOther(r.below(3), r.pct(60)),
            89..=95 if !nochange => {
                let all: [(&'static str, Value); 18] = [
                    ("voltage", json!({"name": "60 V", "nominal": 60})),
                    ("voltage", json!({"name": "220 V", "nominal": 220, "min": 200, "max": 240})),
                    ("voltage", json!({"name": "230 V", "max": 260})),
                    ("voltage", json!({"name": "60 V", "rename": "65 V"})),
                    ("voltage", json!({"name": "230 V", "rename": "231 V"})),
                    ("voltage", json!({"name": "60 V", "delete": true})),
                    ("voltage", json!({"name": "277 V", "delete": true})),
                    ("voltage", json!({"name": "48 V", "delete": true})),
                    ("distribution_system", json!({"name": "S1", "phases": 1, "wires": 2, "l_n": "220 V"})),
                    ("distribution_system", json!({"name": "230/400 V", "ground": "TT"})),
                    ("distribution_system", json!({"name": "S1", "rename": "S2"})),
                    ("distribution_system", json!({"name": "230/400 V", "rename": "EU"})),
                    ("distribution_system", json!({"name": "S1", "delete": true})),
                    ("distribution_system", json!({"name": "230/400 V", "delete": true})),
                    (
                        "wire_type",
                        json!({"name": "W1", "material": "copper", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 1.5, "ampacity": 15}, {"section": 2.5, "ampacity": 21}]}),
                    ),
                    ("wire_type", json!({"name": "Cu PVC 70 °C", "temperature": 75})),
                    ("wire_type", json!({"name": "Cu PVC 70 °C", "rename": "Cu PVC"})),
                    ("wire_type", json!({"name": "W1", "delete": true})),
                ];
                let (c, p) = r.pick(&all);
                Op::Setting(c, p)
            }
            96..=99 => Op::RoundTrip(r.below(4) as u8),
            _ => continue,
        };
        return op;
    }
}

#[derive(Default)]
struct Tally {
    seqs: usize,
    steps: usize,
    clean: usize,
    by: BTreeMap<String, (usize, Vec<String>)>,
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn campaign(label: &str, nochange: bool) {
    let l = lib();
    let seqs = env("P_SEQS", 200) as usize;
    let seed = env("P_SEED", 1);
    let threads = env("P_THREADS", std::thread::available_parallelism().map_or(4, |n| n.get() as u64)) as usize;
    let next = AtomicUsize::new(0);
    let tally = Mutex::new(Tally::default());
    std::thread::scope(|sc| {
        for t in 0..threads {
            let (next, tally) = (&next, &tally);
            sc.spawn(move || {
                let dir = format!("{}/{label}-{t}", l.tmp);
                loop {
                    let seq = next.fetch_add(1, Ordering::Relaxed);
                    if seq >= seqs {
                        break;
                    }
                    let mut rng = Rng(seed.wrapping_mul(1_000_003).wrapping_add(seq as u64));
                    let n = 8 + rng.below(22);
                    // The ops are made against the ledger as the run goes, then replayed as data.
                    let mut r = Run::new(dir.clone(), nochange);
                    let mut ops = Vec::new();
                    // Some sequences start with two amendments of the project's own that both give the same voltage, each its
                    // own: what is in force then depends on which stands over the other, and on a null in either.
                    let mut first = Vec::new();
                    if !nochange && rng.pct(30) {
                        first = shared_start(&mut rng);
                    }
                    let mut from = first.into_iter();
                    for step in 0..n + 6 {
                        let op = from.next().unwrap_or_else(|| gen_op(&mut rng, &r.l, nochange));
                        ops.push(op.clone());
                        if r.step(step, &op) {
                            break;
                        }
                    }
                    let steps = ops.len();
                    let mut sigs: Vec<String> = Vec::new();
                    let mut notes: Vec<(String, Option<String>)> = Vec::new();
                    for f in r.fails.iter().filter(|f| f.inv != "-") {
                        let s = sig(f);
                        if sigs.contains(&s) {
                            continue;
                        }
                        sigs.push(s.clone());
                        let wanted = tally.lock().map(|t| t.by.get(&s).is_none_or(|(_, ex)| ex.len() < 2)).unwrap_or(false);
                        let mut note = None;
                        if wanted {
                            // Shrink: drop every op the failure doesn't need.
                            let mut ops: Vec<Op> = ops.iter().take(f.step + 1).cloned().collect();
                            let mut i = 0;
                            let mut last = f.clone();
                            while i < ops.len() {
                                let mut fewer = ops.clone();
                                fewer.remove(i);
                                match run(&fewer, &dir, nochange).into_iter().find(|g| sig(g) == s) {
                                    Some(g) => {
                                        ops = fewer;
                                        ops.truncate(g.step + 1);
                                        last = g;
                                    }
                                    None => i += 1,
                                }
                            }
                            let lines: Vec<String> = ops.iter().map(|o| format!("      {}", said(o))).collect();
                            note = Some(format!("    seed {seed} seq {seq}:\n{}\n      => {}", lines.join("\n"), last.detail));
                        }
                        notes.push((s, note));
                    }
                    if let Ok(mut t) = tally.lock() {
                        t.seqs += 1;
                        t.steps += steps;
                        if notes.is_empty() {
                            t.clean += 1;
                        }
                        for (s, note) in notes {
                            let e = t.by.entry(s).or_default();
                            e.0 += 1;
                            if let Some(n) = note
                                && e.1.len() < 2
                            {
                                e.1.push(n);
                            }
                        }
                    }
                }
            });
        }
    });
    let t = tally.into_inner().expect("tally");
    let mut out = format!("== campaign {label}: {} sequences, {} steps, seed {seed}; {} sequences clean\n", t.seqs, t.steps, t.clean);
    let mut inv: BTreeMap<&str, usize> = BTreeMap::new();
    for (sig, (n, _)) in &t.by {
        *inv.entry(sig.split(' ').next().unwrap_or("")).or_default() += n;
    }
    for i in ["I1", "I2", "I3", "I4", "I5", "I6", "I7"] {
        inv.entry(i).or_default();
    }
    out.push_str(&format!("   sequences with a failure, by invariant (a sequence counts once for each kind of failure it has): {inv:?}\n"));
    for (sig, (n, ex)) in &t.by {
        out.push_str(&format!("-- {n:5}  {sig}\n"));
        for e in ex {
            out.push_str(e);
            out.push('\n');
        }
    }
    println!("{out}");
    if let Ok(path) = std::env::var("P_OUT") {
        std::fs::write(path, &out).expect("P_OUT");
    }
    assert!(t.by.is_empty(), "{label}: {} of {} sequences failed\n{out}", t.seqs - t.clean, t.seqs);
}

#[test]
fn changes() {
    if std::env::var("P_ONLY").is_ok_and(|v| v != "changes") {
        return;
    }
    campaign("changes", false);
}

#[test]
fn nochange() {
    if std::env::var("P_ONLY").is_ok_and(|v| v != "nochange") {
        return;
    }
    campaign("nochange", true);
}

fn set_code(s: &mut Session, p: Value) {
    s.execute("building_code", &bc(p)).expect("fixture building_code");
}

#[test]
fn named_country_uses_the_highest_matching_project_method() {
    let mut s = Session::new();
    set_code(&mut s, json!({"set": "nec", "save": "US base"}));
    for name in ["US lower", "US higher"] {
        set_code(&mut s, json!({"create": name}));
    }
    let mut spain = codes::find(E, "rebt").unwrap().clone();
    spain.name = "Spain".into();
    spain.based_on = "rebt".into();
    spain.amends = "US base".into();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spain.xml");
    std::fs::write(&path, spain.to_xml()).unwrap();
    s.execute("building_code_import", &json!({"path": path})).unwrap();
    set_code(&mut s, json!({"amendments": ["US lower", "US higher", "Spain"]}));
    for (name, factor) in [("US lower", 2.0), ("US higher", 3.0)] {
        let mut method = codes::find(E, "nec").unwrap().sections["building_demand"].clone();
        method["factor"] = json!(factor);
        set_code(&mut s, json!({"amend": name, "rules": {"building_demand": method}}));
    }
    set_code(&mut s, json!({"amend": "Spain", "rules": {"building_demand": {"factor": 2.0}}}));
    let demand = |s: &mut Session, standard: &str| {
        let out = ask(s, "building_demand", &json!({"standard": standard, "dwellings": [{"count": 5, "demand": 10}]}));
        out["demand"].as_f64().unwrap_or_else(|| panic!("{out}"))
    };
    // NEC Table 220.84 gives 45% for five units. ITC-BT-10 table 1 gives a coefficient of 4.6.
    for (amendments, us) in [(["US lower", "US higher", "Spain"], 67.5), (["US higher", "US lower", "Spain"], 45.0)] {
        set_code(&mut s, json!({"amendments": amendments}));
        assert_eq!(demand(&mut s, "NEC 220.84"), us);
        assert_eq!(demand(&mut s, "ITC-BT-10"), 92.0);
    }
}

#[test]
fn held_utility_method_edits_reach_the_answer() {
    let mut s = Session::new();
    let enel_probe = json!({"utility": "ENEL", "network": "aerial", "installed_kw": 18, "phases": 3});
    let copel_probe = json!({"utility": "COPEL", "system": "127/220", "installed_kw": 8, "demand_kva": 6, "phases": 1});
    set_code(&mut s, json!({"set": "nbr-5410", "amendments": ["copel", "enel"]}));
    let enel = ask(&mut s, "demand", &enel_probe);
    let copel = ask(&mut s, "demand", &copel_probe);
    assert!(enel.get("error").is_none(), "{enel}");
    assert_eq!(copel["category"]["breaker_a"][0].as_f64(), Some(50.0));
    let mut tables = codes::find(E, "copel").unwrap().sections["entrance"]["tables"].clone();
    tables[0]["rows"][0]["breaker_a"][0] = json!(51.0);
    set_code(&mut s, json!({"amend": "copel", "rules": {"entrance": {"tables": tables}}}));
    let out = ask(&mut s, "demand", &copel_probe);
    assert_eq!(out["category"]["breaker_a"][0].as_f64(), Some(51.0), "{out}");
    assert_eq!(ask(&mut s, "demand", &enel_probe), enel);

    set_code(&mut s, json!({"create": "Enel higher", "amendments": ["copel", "enel", "Enel higher"]}));
    let mut method = codes::find(E, "enel").unwrap().sections["unit_demand"].clone();
    let row = |method: &mut Value, breaker| {
        method["method"]["annexes"]["networks"][0]["rows"][5]["breaker"] = json!(breaker);
    };
    row(&mut method, 51);
    set_code(&mut s, json!({"amend": "enel", "rules": {"unit_demand": method}}));
    row(&mut method, 52);
    set_code(&mut s, json!({"amend": "Enel higher", "rules": {"unit_demand": method}}));
    for (amendments, breaker) in [(["copel", "enel", "Enel higher"], 52.0), (["copel", "Enel higher", "enel"], 51.0)] {
        set_code(&mut s, json!({"amendments": amendments}));
        let out = ask(&mut s, "demand", &enel_probe);
        assert_eq!(out["category"]["breaker_a"].as_f64(), Some(breaker), "{out}");
    }
}

#[test]
fn a_whole_method_requires_a_held_amendment() {
    let mut s = Session::new();
    set_code(&mut s, json!({"set": "nbr-5410", "amendments": ["cpfl"]}));
    let method = codes::find(E, "enel").unwrap().sections["unit_demand"].clone();
    for p in [json!({"rules": {"unit_demand": method}}), json!({"amend": "enel", "rules": {"unit_demand": method}})] {
        let before = s.model.codes.clone();
        assert!(s.execute("building_code", &bc(p)).is_err());
        assert_eq!(s.model.codes, before);
    }
}

#[test]
fn saved_pack_uses_character_limit_and_keeps_occupied_names() {
    let mut s = Session::new();
    let mut ledger = Ledger::default();
    let name = "É".repeat(64);
    let taken = format!("{} · cpfl", "é".repeat(57));
    for p in [json!({"set": "nbr-5410", "amendments": ["cpfl", "enel"]}), json!({"create": taken})] {
        set_code(&mut s, p.clone());
        ledger.building_code(&p);
    }
    let cpfl = format!("{} · cpfl (2)", "É".repeat(53));
    let enel = format!("{} · enel", "É".repeat(57));
    let expected = vec![cpfl, enel];
    assert_eq!(ledger.saved(&name).unwrap().into_iter().skip(1).map(|(n, _)| n).collect::<Vec<_>>(), expected);
    set_code(&mut s, json!({"save": name}));
    assert_eq!(s.model.codes.electrical.amendments, expected);
    for (layer, origin) in expected.iter().zip(["cpfl", "enel"]) {
        let held = s.model.codes.find(E, layer).unwrap();
        assert_eq!(held.amends, name);
        assert_eq!(held.based_on, origin);
        assert!(same(&held.sections["unit_demand"], &codes::find(E, origin).unwrap().sections["unit_demand"]));
    }
    assert!(s.model.codes.find(E, &taken).unwrap().sections.is_empty());
}

fn confirm_run(dir: &tempfile::TempDir) -> Run {
    let mut run = Run::new(dir.path().display().to_string(), false);
    confirm_set(&mut run, json!({"set": "nbr-5410"}));
    run
}

fn confirm_set(run: &mut Run, p: Value) {
    set_code(&mut run.s, p.clone());
    run.l.building_code(&p);
}

fn confirm_answers(run: &mut Run, cmd: &str, p: &Value) -> (Value, Value) {
    // The campaign memo only varies the method and base. These tests vary the probe as well.
    run.memo.clear();
    let reference = run.oracle_for(&[(cmd, p.clone())]).remove(0);
    let live = ask(&mut run.s, cmd, p);
    (live, reference)
}

fn confirm_entrance(utility: &str) -> Value {
    let mut entrance = codes::find(E, "energisa").unwrap().sections["entrance"].clone();
    entrance["id"] = json!(utility);
    entrance["tables"][0]["rows"][0]["breaker_a"][0] = json!(31);
    entrance
}

#[test]
fn confirm_demand_uses_the_higher_layer_across_sections() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = confirm_run(&dir);
    confirm_set(&mut run, json!({"create": "Entrance", "amendments": ["Entrance", "celesc"]}));
    confirm_set(&mut run, json!({"amend": "Entrance", "rules": {"entrance": confirm_entrance("CELESC")}}));
    // Editing the library holder creates a project copy, so both competing methods are own.
    let mut unit = codes::find(E, "celesc").unwrap().sections["unit_demand"].clone();
    unit["method"]["table"]["networks"][0]["rows"][0]["breaker"] = json!(52);
    confirm_set(&mut run, json!({"amend": "celesc", "rules": {"unit_demand": unit}}));
    let p = json!({"utility": "CELESC", "installed_kw": 3, "phases": 1});
    let (live, reference) = confirm_answers(&mut run, "demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    let breaker = |answer: &Value| {
        let a = &answer["category"]["breaker_a"];
        a.as_f64().or_else(|| a[0].as_f64())
    };
    let unit_on_top = (breaker(&live), breaker(&reference));

    confirm_set(&mut run, json!({"amendments": ["celesc"]}));
    let (live, reference) = confirm_answers(&mut run, "demand", &p);
    assert_eq!((breaker(&live), breaker(&reference)), (Some(52.0), Some(52.0)));

    confirm_set(&mut run, json!({"amendments": ["celesc", "Entrance"]}));
    let (live, reference) = confirm_answers(&mut run, "demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["category"]["breaker_a"][0].as_f64(), Some(31.0));
    assert_eq!(unit_on_top, (Some(52.0), Some(52.0)));
}

#[test]
fn confirm_demand_higher_library_layer_of_the_same_utility() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = confirm_run(&dir);
    confirm_set(&mut run, json!({"create": "Enel copy", "amendments": ["enel", "Enel copy"]}));
    let mut unit = codes::find(E, "enel").unwrap().sections["unit_demand"].clone();
    unit["method"]["annexes"]["networks"][0]["rows"][5]["breaker"] = json!(51);
    confirm_set(&mut run, json!({"amend": "Enel copy", "rules": {"unit_demand": unit}}));
    let p = json!({"utility": "ENEL", "network": "aerial", "installed_kw": 18, "phases": 3});
    let (live, reference) = confirm_answers(&mut run, "demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["category"]["breaker_a"].as_f64(), Some(51.0));
    confirm_set(&mut run, json!({"amendments": []}));
    assert_eq!(ask(&mut run.s, "demand", &p)["category"]["breaker_a"].as_f64(), Some(50.0));

    confirm_set(&mut run, json!({"amendments": ["Enel copy", "enel"]}));
    let (live, reference) = confirm_answers(&mut run, "demand", &p);
    assert_eq!((live["category"]["breaker_a"].as_f64(), reference["category"]["breaker_a"].as_f64()), (Some(50.0), Some(50.0)),);
}

#[test]
fn confirm_demand_group_member_and_group_unit_method() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = confirm_run(&dir);
    confirm_set(&mut run, json!({"create": "Group unit", "amendments": ["Group unit"]}));
    let mut unit = codes::find(E, "celesc").unwrap().sections["unit_demand"].clone();
    unit["utility"] = json!("ENERGISA");
    unit["method"]["table"]["networks"][0]["rows"][0]["breaker"] = json!(52);
    confirm_set(&mut run, json!({"amend": "Group unit", "rules": {"unit_demand": unit}}));
    let member = json!({"utility": "EAC", "installed_kw": 3, "phases": 1});
    let (live, reference) = confirm_answers(&mut run, "demand", &member);
    assert!(same(&live, &reference), "{live} != {reference}");
    // A member answers by its group's unit demand, which the project holds.
    let b = &live["category"]["breaker_a"];
    assert_eq!(b.as_f64().or_else(|| b[0].as_f64()), Some(52.0), "{live}");

    let group = json!({"utility": "ENERGISA", "installed_kw": 3, "phases": 1});
    let (live, reference) = confirm_answers(&mut run, "demand", &group);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["category"]["breaker_a"].as_f64(), Some(52.0));

    confirm_set(&mut run, json!({"amendments": ["Group unit", "energisa"]}));
    confirm_set(&mut run, json!({"amend": "energisa", "rules": {"entrance": confirm_entrance("ENERGISA")}}));
    let (live, reference) = confirm_answers(&mut run, "demand", &member);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["group"], "ENERGISA");
    assert_eq!(live["category"]["breaker_a"][0].as_f64(), Some(31.0));
}

#[test]
fn confirm_building_demand_without_selector_and_by_other_utility() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = confirm_run(&dir);
    let mut p = json!({"dwellings": [{"count": 20, "area": 100}, {"count": 16, "area": 80}]});
    let (live, reference) = confirm_answers(&mut run, "building_demand", &p);
    assert!(live["error"].is_string() && same(&live, &reference), "{live} != {reference}");

    confirm_set(&mut run, json!({"amendments": ["celesc"]}));
    let (live, reference) = confirm_answers(&mut run, "building_demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["standard"], "CELESC N-321.0003");
    assert_eq!(live["factor"].as_f64(), Some(1.2));

    confirm_set(&mut run, json!({"amendments": ["celesc", "hidropan"]}));
    confirm_set(&mut run, json!({"amend": "celesc", "rules": {"building_demand": {"factor": 2.0}}}));
    confirm_set(&mut run, json!({"amend": "hidropan", "rules": {"building_demand": {"factor": 3.0}}}));
    let (live, reference) = confirm_answers(&mut run, "building_demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["standard"], "HIDROPAN RIC BT 2014");
    assert_eq!(live["factor"].as_f64(), Some(3.0));

    p["utility"] = json!("celesc");
    let (live, reference) = confirm_answers(&mut run, "building_demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["standard"], "CELESC N-321.0003");
    assert_eq!(live["factor"].as_f64(), Some(2.0));

    confirm_set(&mut run, json!({"amend": "celesc", "reset": true}));
    let (live, reference) = confirm_answers(&mut run, "building_demand", &p);
    assert!(same(&live, &reference), "{live} != {reference}");
    assert_eq!(live["standard"], "CELESC N-321.0003");
    assert_eq!(live["factor"].as_f64(), Some(1.2));
}
