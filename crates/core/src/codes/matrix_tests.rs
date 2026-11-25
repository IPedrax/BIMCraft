//! The building codes' matrix: each utility's method (who), held alone, under another or over another, changed today or by
//! an older file (where), through each thing a project does with its codes (what), with the same checks after each: the
//! project is valid and reads back from its file, its rules in force are a reference project's, and every method it holds
//! answers as the reference answers. The reference is a project made today the plain way, holding what the case should
//! hold. A slice that changes the codes' engine runs it; a case a review finds that it misses becomes an axis here.

use serde_json::{Value, json};

use super::*;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The command that answers by a utility's or a country's method, with a load inside the method's first row.
pub(super) fn method_probe(code: &BuildingCode, key: &str, method: &Value) -> (&'static str, Value) {
    match key {
        "unit_demand" => match code.code.as_str() {
            "cpfl" => ("demand", json!({"utility": "CPFL", "area": 40})),
            "enel" => ("demand", json!({"utility": "ENEL", "network": "aerial", "installed_kw": 8})),
            _ => ("demand", json!({"utility": method["utility"], "installed_kw": 8})),
        },
        "building_demand" => {
            let group = if method["mean_of"] == "area" { json!({"count": 20, "area": 120}) } else { json!({"count": 20, "demand": 10}) };
            ("building_demand", json!({"standard": method["standard"], "dwellings": [group]}))
        }
        _ => {
            let table = &method["tables"][0];
            let row = &table["rows"][0];
            let mut p = json!({"utility": method["id"], "system": table["system"], "entrance": table["use"], "phases": row["phases"]});
            for f in ["installed_kw", "demand_kva"] {
                if let Some(up) = row[f]["up_to"].as_f64() {
                    p[f] = json!((row[f]["over"].as_f64().unwrap_or(0.0) + up) / 2.0);
                }
            }
            ("demand", p)
        }
    }
}

/// Every breaker of a method one ampere up, so an answer by it differs from the library's.
pub(super) fn breakers_up(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                match (k.as_str(), &mut *x) {
                    ("breaker", Value::Number(n)) => *x = json!(n.as_f64().unwrap_or(0.0) + 1.0),
                    ("breaker_a", Value::Array(a)) => a.iter_mut().for_each(|b| *b = json!(b.as_f64().unwrap_or(0.0) + 1.0)),
                    _ => breakers_up(x),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(breakers_up),
        _ => {}
    }
}

/// Method `key` of `code` changed `n` times over (a factor, or every breaker `n` amperes up).
fn changed(code: &BuildingCode, key: &str, n: u32) -> Value {
    let mut v = code.sections[key].clone();
    if key == "building_demand" {
        v["factor"] = json!(1.0 + f64::from(n));
    } else {
        for _ in 0..n {
            breakers_up(&mut v);
        }
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Alone,
    Under,
    Over,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum What {
    Nothing,
    Export { all: bool },
    ImportRenamed,
    Save,
    SaveThenTakeOff,
    EditAgain,
    Reset,
    SwapOut,
}

const WHAT: [What; 9] = [
    What::Nothing,
    What::Export { all: false },
    What::Export { all: true },
    What::ImportRenamed,
    What::Save,
    What::SaveThenTakeOff,
    What::EditAgain,
    What::Reset,
    What::SwapOut,
];

struct Case<'a> {
    code: &'a BuildingCode,
    other: &'a BuildingCode,
    key: &'static str,
    place: Place,
    older: bool,
}

impl Case<'_> {
    fn amendments(&self) -> Vec<&str> {
        match self.place {
            Place::Alone => vec![self.code.code.as_str()],
            Place::Under => vec![self.code.code.as_str(), self.other.code.as_str()],
            Place::Over => vec![self.other.code.as_str(), self.code.code.as_str()],
        }
    }

    fn name(&self) -> String {
        let from = if self.older { ", changed by an older file" } else { "" };
        format!("{} {} ({:?} {}{from})", self.code.code, self.key, self.place, self.other.code)
    }

    /// A project made today the plain way: held as the case says, `code`'s method changed as `n` says (0: not at all).
    fn plain(&self, amendments: &[&str], n: u32) -> Result<Session, String> {
        let mut s = Session::new();
        s.execute("building_code", &json!({"discipline": "electrical", "set": self.code.amends, "amendments": amendments}))?;
        if n > 0 {
            s.execute(
                "building_code",
                &json!({"discipline": "electrical", "amend": self.code.code, "rules": {self.key: changed(self.code, self.key, n)}}),
            )?;
        }
        Ok(s)
    }

    /// The case's project: its method changed once, today or in an older file's way, read back from its text.
    fn subject(&self) -> Result<Session, String> {
        if !self.older {
            return self.plain(&self.amendments(), 1);
        }
        let mut s = self.plain(&self.amendments(), 0)?;
        s.model_mut()
            .codes
            .choice_mut(Discipline::Electrical)
            .changes
            .insert(self.key.into(), changed(self.code, self.key, 1).as_object().cloned().unwrap_or_default());
        let (m, left) = crate::model::Model::read(&serde_json::to_string(&*s.model).map_err(|e| e.to_string())?)?;
        if !left.is_empty() {
            return Err(format!("left out: {left:?}"));
        }
        *s.model_mut() = m;
        Ok(s)
    }

    fn answers(&self, s: &mut Session) -> Vec<Value> {
        let key = self.key;
        [self.code, self.other]
            .into_iter()
            .map(|c| {
                let (cmd, probe) = method_probe(c, key, &c.sections[key]);
                let r = run(s, cmd, probe);
                if key == "building_demand" { r["demand"].clone() } else { r["category"].clone() }
            })
            .collect()
    }
}

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-codes-matrix-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Does `what` to `s` and gives the reference it should then answer as.
fn act(case: &Case, s: &mut Session, what: What, file: &std::path::Path) -> Result<Session, String> {
    let file = file.to_string_lossy().to_string();
    let same = || case.plain(&case.amendments(), 1);
    match what {
        What::Nothing => same(),
        What::Export { all } => {
            s.execute("building_code_export", &json!({"discipline": "electrical", "path": file, "name": "Exported", "all": all}))?;
            *s = Session::new();
            s.execute("building_code_import", &json!({"path": file, "set": true}))?;
            same()
        }
        What::ImportRenamed => {
            s.execute("building_code_export", &json!({"discipline": "electrical", "path": file, "name": "Exported"}))?;
            *s = Session::new();
            s.execute("building_code_import", &json!({"path": file, "set": true, "name": "Renamed"}))?;
            same()
        }
        What::Save => {
            s.execute("building_code", &json!({"discipline": "electrical", "save": "Saved"}))?;
            same()
        }
        What::SaveThenTakeOff => {
            // Saved, then held to the saved code alone: the rules that were in force, the one on top's method for each.
            let top = case.amendments().last().map(|t| t.to_string()).unwrap_or_default();
            s.execute("building_code", &json!({"discipline": "electrical", "save": "Saved"}))?;
            s.execute("building_code", &json!({"discipline": "electrical", "amendments": []}))?;
            let mut r = case.plain(&[&top], 0)?;
            if top == case.code.code {
                r.execute(
                    "building_code",
                    &json!({"discipline": "electrical", "amend": top, "rules": {case.key: changed(case.code, case.key, 1)}}),
                )?;
            }
            Ok(r)
        }
        What::EditAgain => {
            s.execute(
                "building_code",
                &json!({"discipline": "electrical", "amend": case.code.code, "rules": {case.key: changed(case.code, case.key, 2)}}),
            )?;
            case.plain(&case.amendments(), 2)
        }
        What::Reset => {
            s.execute("building_code", &json!({"discipline": "electrical", "amend": case.code.code, "reset": case.key}))?;
            case.plain(&case.amendments(), 0)
        }
        What::SwapOut => {
            s.execute("building_code", &json!({"discipline": "electrical", "amendments": [case.other.code]}))?;
            case.plain(&[&case.other.code], 0)
        }
    }
}

#[test]
fn every_utilitys_method_held_every_way_comes_through_every_operation_as_a_plain_project_made_today() {
    let all: Vec<&BuildingCode> = of(Discipline::Electrical).collect();
    let folder = dir();
    let mut asked = 0;
    let mut refused: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    // While working, every utility and method with one case in five of the rest, a different one for each (seconds); in CI,
    // or with `BIMCRAFT_MATRIX=full`, every case (about 40 s).
    let full = std::env::var_os("CI").is_some() || std::env::var("BIMCRAFT_MATRIX").is_ok_and(|v| v == "full");
    let mut n = 0usize;
    for code in all.iter().copied().filter(|c| !c.amends.is_empty()) {
        for key in ["unit_demand", "building_demand", "entrance"] {
            if !code.sections.contains_key(key) {
                continue;
            }
            let Some(other) = all.iter().copied().find(|o| o.code != code.code && o.amends == code.amends && o.sections.contains_key(key))
            else {
                continue;
            };
            for place in [Place::Alone, Place::Under, Place::Over] {
                for older in [false, true] {
                    let case = Case { code, other, key, place, older };
                    for what in WHAT {
                        n += 1;
                        if !full && !n.is_multiple_of(5) {
                            continue;
                        }
                        let mut s = match case.subject() {
                            Ok(s) => s,
                            Err(e) => {
                                refused.push(format!("{}: {}", case.name(), e.split(';').next().unwrap_or_default()));
                                break;
                            }
                        };
                        let file = folder.join(format!("{}-{key}-{place:?}-{older}-{what:?}.xml", code.code));
                        let mut reference = match act(&case, &mut s, what, &file) {
                            Ok(r) => r,
                            Err(e) => {
                                refused.push(format!("{} (the plain project): {}", case.name(), e.split(';').next().unwrap_or_default()));
                                continue;
                            }
                        };
                        let here = format!("{} after {what:?}", case.name());
                        if let Err(e) = s.model.check() {
                            failed.push(format!("{here}: not a valid project: {e}"));
                            continue;
                        }
                        let text = serde_json::to_string(&*s.model).unwrap();
                        match crate::model::Model::read(&text) {
                            Ok((m, left)) if left.is_empty() => {
                                let mut back = Session::new();
                                *back.model_mut() = m;
                                if case.answers(&mut back) != case.answers(&mut s) {
                                    failed.push(format!("{here}: answers otherwise read back from its file"));
                                }
                            }
                            Ok((_, left)) => failed.push(format!("{here}: its file leaves out {left:?}")),
                            Err(e) => failed.push(format!("{here}: its file does not read: {e}")),
                        }
                        if s.model.resolve_code(Discipline::Electrical, key).value
                            != reference.model.resolve_code(Discipline::Electrical, key).value
                        {
                            failed.push(format!("{here}: rules in force differ from the plain project's"));
                        }
                        let (got, want) = (case.answers(&mut s), case.answers(&mut reference));
                        if got != want {
                            failed.push(format!("{here}: answers {got:?}, the plain project {want:?}"));
                        }
                        asked += 1;
                    }
                }
            }
        }
    }
    assert!(failed.is_empty(), "{} of {asked} cases failed:\n{}", failed.len(), failed.join("\n"));
    // Every case's change is one a project may make: none refused (a method changed under another is its own, G9).
    refused.sort();
    refused.dedup();
    assert!(refused.is_empty(), "refused: {refused:#?}");
    assert!(asked >= if full { 300 } else { 60 }, "cases asked ({asked})");
}

/// CPFL changed under Enel: the pair the cases below start from, and what `demand` answers for each.
fn cpfl_under_enel() -> Session {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["cpfl", "enel"]}));
    let cpfl = find(Discipline::Electrical, "cpfl").unwrap();
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "cpfl", "rules": {"unit_demand": changed(cpfl, "unit_demand", 1)}}),
    );
    s
}

fn cpfl_and_enel(s: &mut Session) -> (Value, Value) {
    let cpfl = run(s, "demand", json!({"utility": "CPFL", "area": 40}))["category"].clone();
    let enel = run(s, "demand", json!({"utility": "ENEL", "network": "aerial", "installed_kw": 8}))["category"].clone();
    (cpfl, enel)
}

#[test]
fn a_long_name_given_to_what_is_exported_or_saved_loses_no_method_and_no_rule() {
    for len in [40, 59, 61, 63, 64] {
        let name: String = "P".repeat(len);
        let mut s = cpfl_under_enel();
        let before = cpfl_and_enel(&mut s);
        let rules = |s: &Session| {
            (s.model.codes.layers(Discipline::Electrical).len(), s.model.resolve_code(Discipline::Electrical, "unit_demand").value)
        };
        let in_force = rules(&s).1;
        let path = dir().join(format!("long-{len}.xml"));
        let path = path.to_string_lossy();
        run(&mut s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": name}));
        let mut t = Session::new();
        run(&mut t, "building_code_import", json!({"path": path, "set": true}));
        assert_eq!(cpfl_and_enel(&mut t), before, "name of {len}: exported and read back");
        assert_eq!(rules(&t).1, in_force, "name of {len}: rules in force, read back");
        let mut saved = cpfl_under_enel();
        run(&mut saved, "building_code", json!({"discipline": "electrical", "save": name}));
        assert_eq!(cpfl_and_enel(&mut saved), before, "name of {len}: saved");
        assert_eq!(rules(&saved).1, in_force, "name of {len}: rules in force, saved");
        assert_eq!(saved.model.codes.choice(Discipline::Electrical).code.as_deref(), Some(name.as_str()), "name of {len}: held to it");
    }
}

#[test]
fn saving_or_importing_again_never_takes_over_a_code_the_project_already_has() {
    // A code of the project's own that happens to bear the name an amendment would get stays as it was.
    let mut s = cpfl_under_enel();
    let before = cpfl_and_enel(&mut s);
    let generated: Vec<String> = super::command::in_force_layered(&s.model.codes, Discipline::Electrical, "Pack")
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|k| k.code)
        .collect();
    assert!(!generated.is_empty());
    for g in &generated {
        let mut t = Session::new();
        *t.model_mut() = (*s.model).clone();
        run(&mut t, "building_code", json!({"discipline": "electrical", "create": g, "over": "nbr-5410"}));
        let theirs = t.model.codes.own.iter().find(|k| k.code == *g).cloned().unwrap();
        run(&mut t, "building_code", json!({"discipline": "electrical", "save": "Pack"}));
        assert_eq!(t.model.codes.own.iter().find(|k| k.code == *g), Some(&theirs), "`{g}` kept as it was");
        assert_eq!(cpfl_and_enel(&mut t), before, "saved beside `{g}`");
    }
    // The same file read twice, the second time under another name and not held to: the first stays whole.
    let path = dir().join("twice.xml");
    let path = path.to_string_lossy();
    run(&mut s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": "Pack"}));
    let mut t = Session::new();
    run(&mut t, "building_code_import", json!({"path": path, "set": true}));
    run(&mut t, "building_code_import", json!({"path": path, "set": false, "name": "Other"}));
    assert_eq!(cpfl_and_enel(&mut t), before, "read again under another name");
    run(&mut t, "building_code_import", json!({"path": path, "set": true, "name": "Other"}));
    assert_eq!(cpfl_and_enel(&mut t), before, "the second held to");
}

#[test]
fn a_bases_own_method_under_an_amendment_with_another_comes_through_export_and_save() {
    // A project's own copy of the NEC (its factor changed) under an amendment of its own holding Spain's method.
    let nec = find(Discipline::Electrical, "nec").unwrap();
    let rebt = find(Discipline::Electrical, "rebt").unwrap();
    let mut s = Session::new();
    {
        let codes = &mut *s.model_mut().codes;
        let mut base = nec.clone();
        base.code = "NEC changed".into();
        base.name = base.code.clone();
        base.based_on = "nec".into();
        base.sections.insert("building_demand".into(), changed(nec, "building_demand", 1));
        let mut over = BuildingCode {
            code: "Spain over".into(),
            name: "Spain over".into(),
            discipline: Discipline::Electrical,
            amends: "NEC changed".into(),
            based_on: "rebt".into(),
            ..BuildingCode::default()
        };
        over.sections.insert("building_demand".into(), rebt.sections["building_demand"].clone());
        codes.own.push(base);
        codes.own.push(over);
        let choice = codes.choice_mut(Discipline::Electrical);
        choice.code = Some("NEC changed".into());
        choice.amendments = vec!["Spain over".into()];
    }
    s.model.check().unwrap();
    let ask = |s: &mut Session, m: &BuildingCode| {
        let (cmd, p) = method_probe(m, "building_demand", &m.sections["building_demand"]);
        run(s, cmd, p)["demand"].clone()
    };
    let before = (ask(&mut s, nec), ask(&mut s, rebt));
    let path = dir().join("countries.xml");
    let path = path.to_string_lossy();
    run(&mut s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": "Countries"}));
    let mut t = Session::new();
    run(&mut t, "building_code_import", json!({"path": path, "set": true}));
    assert_eq!((ask(&mut t, nec), ask(&mut t, rebt)), before, "exported and read back");
    run(&mut s, "building_code", json!({"discipline": "electrical", "save": "Countries"}));
    assert_eq!((ask(&mut s, nec), ask(&mut s, rebt)), before, "saved");
}

#[test]
fn names_at_the_limit_are_refused_or_kept_never_one_code_in_anothers_place() {
    // Whatever is done with names of 64 characters, either it is refused and nothing changes, or every answer stays.
    let long = |c: char| c.to_string().repeat(MAX_NAME);
    let check = |s: &mut Session, id: &str, p: Value, before: &(Value, Value), what: &str| {
        let codes = (*s.model.codes).clone();
        match s.execute(id, &p) {
            Ok(_) => assert_eq!(&cpfl_and_enel(s), before, "{what}: done, and the answers stay"),
            Err(_) => assert!(*s.model.codes == codes, "{what}: refused, and nothing changed"),
        }
    };
    // A layer of the project's own with a name of 64 characters, under Enel.
    let mut s = cpfl_under_enel();
    let before = cpfl_and_enel(&mut s);
    let name = long('L');
    run(&mut s, "building_code", json!({"discipline": "electrical", "create": name, "over": "nbr-5410"}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": ["cpfl", name, "enel"]}));
    let mut t = Session::new();
    *t.model_mut() = (*s.model).clone();
    check(&mut t, "building_code", json!({"discipline": "electrical", "save": long('S')}), &before, "saved under 64 characters");
    let mut t = Session::new();
    *t.model_mut() = (*s.model).clone();
    check(&mut t, "building_code", json!({"discipline": "electrical", "save": "Pack"}), &before, "saved beside a layer of 64");
    // A file exported under 64 characters, read, then read again under another name of 64.
    let mut s = cpfl_under_enel();
    let path = dir().join("limit.xml");
    let path = path.to_string_lossy().to_string();
    check(
        &mut s,
        "building_code_export",
        json!({"discipline": "electrical", "path": path, "name": long('E')}),
        &before,
        "exported under 64",
    );
    if std::path::Path::new(&path).exists() {
        let mut t = Session::new();
        check(&mut t, "building_code_import", json!({"path": path, "set": true}), &before, "read under 64");
        let held = cpfl_and_enel(&mut t);
        check(&mut t, "building_code_import", json!({"path": path, "set": false, "name": long('O')}), &held, "read again under another 64");
        check(&mut t, "building_code_import", json!({"path": path, "set": false, "name": "Other"}), &held, "read again under a short name");
    }
}

/// What a refusal must leave as it was: the codes and the electrical settings.
fn state(s: &Session) -> String {
    let m = &s.model;
    serde_json::to_string(&(&*m.codes, &m.voltages, &m.distribution_systems, &m.wire_types)).unwrap()
}

#[test]
fn a_method_that_would_not_be_valid_is_refused_on_the_layer_it_is_given_wherever_that_lies() {
    // Each utility's method broken in its own amendment, alone, under another and over it: refused, nothing changes, and
    // a whole valid change of the same layer is taken (G4, G9).
    let all: Vec<&BuildingCode> = of(Discipline::Electrical).collect();
    let mut asked = 0;
    for code in all.iter().copied().filter(|c| !c.amends.is_empty()) {
        for key in ["unit_demand", "building_demand", "entrance"] {
            if !code.sections.contains_key(key) {
                continue;
            }
            let Some(other) = all.iter().copied().find(|o| o.code != code.code && o.amends == code.amends && o.sections.contains_key(key))
            else {
                continue;
            };
            let broken = match key {
                "unit_demand" => json!({"method": "oops"}),
                "building_demand" => json!({"diversity": "oops"}),
                _ => json!({"tables": "oops"}),
            };
            for place in [Place::Alone, Place::Under, Place::Over] {
                let case = Case { code, other, key, place, older: false };
                let mut s = case.plain(&case.amendments(), 0).unwrap();
                let before = (state(&s), case.answers(&mut s));
                let r =
                    s.execute("building_code", &json!({"discipline": "electrical", "amend": code.code, "rules": {key: broken.clone()}}));
                assert!(r.is_err(), "{}: a broken method was taken", case.name());
                assert_eq!((state(&s), case.answers(&mut s)), before, "{}: refused, and nothing changed", case.name());
                let r = s.execute(
                    "building_code",
                    &json!({"discipline": "electrical", "amend": code.code, "rules": {key: changed(code, key, 1)}}),
                );
                assert!(r.is_ok(), "{}: its own whole method changed was refused: {r:?}", case.name());
                asked += 1;
            }
        }
    }
    assert!(asked >= 15, "cases asked ({asked})");
}

#[test]
fn a_distribution_system_is_taken_by_the_codes_exactly_when_the_command_would_take_it() {
    // The same systems given to `distribution_system` and to `building_code`'s rules, with the voltages they name: the same
    // answer, and what is taken is in both the code and the project's settings (G8).
    let volts = [("127 V", 127.0), ("220 V", 220.0), ("230 V", 230.0), ("380 V", 380.0), ("400 V", 400.0), ("440 V", 440.0)];
    let mut systems = Vec::new();
    for (phases, wires) in [(1, 2), (2, 3), (3, 3), (3, 4), (3, 5), (0, 2)] {
        for ll in [None, Some("380 V"), Some("400 V"), Some("440 V")] {
            for ln in [None, Some("127 V"), Some("220 V"), Some("230 V")] {
                let mut x = json!({"phases": phases, "wires": wires});
                if let Some(v) = ll {
                    x["l_l"] = json!(v);
                }
                if let Some(v) = ln {
                    x["l_n"] = json!(v);
                }
                systems.push(x);
            }
        }
    }
    let base = || {
        let mut s = Session::new();
        for (n, v) in volts {
            if !s.model.voltages.contains_key(n) {
                run(&mut s, "voltage", json!({"name": n, "nominal": v}));
            }
        }
        s
    };
    let mut took = 0;
    for (i, x) in systems.iter().enumerate() {
        let name = format!("S{i}");
        let mut direct = base();
        let mut p = x.clone();
        p["name"] = json!(name);
        let by_command = direct.execute("distribution_system", &p).is_ok();
        let mut s = base();
        let before = state(&s);
        let by_codes =
            s.execute("building_code", &json!({"discipline": "electrical", "rules": {"distribution_systems": {&name: x}}})).is_ok();
        assert_eq!(by_codes, by_command, "{x}: taken by the codes {by_codes}, by the command {by_command}");
        if by_codes {
            took += 1;
            let in_code = s.model.resolve_code(Discipline::Electrical, "distribution_systems").value.contains_key(&name);
            assert!(in_code && s.model.distribution_systems.contains_key(&name), "{x}: in the code {in_code} and the project");
        } else {
            assert_eq!(state(&s), before, "{x}: refused, and nothing changed");
        }
    }
    assert!(took >= 5 && took < systems.len(), "both kinds asked ({took} of {})", systems.len());
}

#[test]
fn a_refused_change_leaves_the_history_as_it_was() {
    let mut s = cpfl_under_enel();
    let start = state(&s);
    let cpfl = find(Discipline::Electrical, "cpfl").unwrap();
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "cpfl", "rules": {"unit_demand": changed(cpfl, "unit_demand", 2)}}),
    );
    let edited = state(&s);
    assert!(
        s.execute("building_code", &json!({"discipline": "electrical", "amend": "cpfl", "rules": {"unit_demand": {"method": "oops"}}}))
            .is_err()
    );
    assert_eq!(state(&s), edited, "refused: as it was");
    run(&mut s, "undo", json!({}));
    assert_eq!(state(&s), start, "an undo after a refusal takes back the edit before it");
}

#[test]
fn a_file_holding_a_broken_method_under_another_still_opens_and_takes_other_edits() {
    // A file saved before a method was checked on its own layer may hold a broken one under another utility's: it opens,
    // the project takes other edits, the broken method changed and still broken is refused, and its reset is taken.
    let mut s = cpfl_under_enel();
    let cpfl = s.model_mut().codes.own.iter_mut().find(|c| c.code == "cpfl").unwrap();
    cpfl.sections.get_mut("unit_demand").and_then(Value::as_object_mut).unwrap().insert("method".into(), json!("oops"));
    let path = dir().join("broken-under.bimc");
    run(&mut s, "save", json!({"path": path}));
    let mut s = Session::new();
    run(&mut s, "open", json!({"path": path}));
    run(&mut s, "voltage", json!({"name": "Other V", "nominal": 200}));
    run(&mut s, "level", json!({"name": "Above", "elevation": 3000}));
    let r = s.execute("building_code", &json!({"discipline": "electrical", "amend": "cpfl", "rules": {"unit_demand": {"method": "nope"}}}));
    assert!(r.is_err(), "the broken method changed and still broken was taken");
    run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "cpfl", "reset": "unit_demand"}));
    let path = dir().join("broken-under.xml");
    run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical"}));
}

#[test]
fn a_system_is_checked_with_the_voltages_a_new_code_brings_back() {
    // A voltage the project changed and the new code no longer names comes back as a new project has it: a system the codes
    // take is checked with that one, not the one before.
    let mut s = Session::new();
    run(&mut s, "voltage", json!({"name": "480 V", "nominal": 468, "min": 468, "max": 468}));
    let r = s.execute(
        "building_code",
        &json!({"discipline": "electrical", "set": "generic", "rules": {
            "voltages": {"Review V": {"nominal": 282, "min": 282, "max": 282}},
            "distribution_systems": {"Review": {"phases": 3, "wires": 4, "l_l": "480 V", "l_n": "Review V"}}
        }}),
    );
    assert!(r.is_ok(), "refused with the voltage before: {r:?}");
    assert_eq!(s.model.voltages["480 V"].nominal, 480.0, "back as a new project has it");
    assert!(s.model.distribution_systems.contains_key("Review"), "taken by the codes and in the project");
}

/// A kind of electrical setting: its section, one of it, and the names the project holds.
type SettingKind = (&'static str, Value, fn(&Session) -> Vec<String>);

/// A setting of each of the three kinds.
fn settings_kinds() -> [SettingKind; 3] {
    [
        ("voltages", json!({"nominal": 50, "min": 45, "max": 55}), |s| s.model.voltages.keys().cloned().collect()),
        ("distribution_systems", json!({"phases": 1, "wires": 2, "l_n": "220 V"}), |s| {
            s.model.distribution_systems.keys().cloned().collect()
        }),
        (
            "wire_types",
            json!({"material": "copper", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 1.5, "ampacity": 15}]}),
            |s| s.model.wire_types.keys().cloned().collect(),
        ),
    ]
}

#[test]
fn a_setting_given_at_the_limit_takes_the_place_of_one_taken_away_and_one_past_it_is_refused() {
    // The project holds 64 of each kind. Taking one away and adding another in one change is 64, the new one in both the
    // code and the project (G6); one more than 64 is refused and nothing changes, as the commands refuse it.
    for amend in [None, Some("Site")] {
        for (key, x, names) in settings_kinds() {
            let mut s = Session::new();
            if amend.is_some() {
                run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
            }
            let ask = |rules: Value| {
                let mut p = json!({"discipline": "electrical", "rules": {key: rules}});
                if let Some(a) = amend {
                    p["amend"] = json!(a);
                }
                p
            };
            let fill: Map<String, Value> = (names(&s).len()..64).map(|i| (format!("Filler {i:02}"), x.clone())).collect();
            run(&mut s, "building_code", ask(Value::Object(fill)));
            assert_eq!(names(&s).len(), 64, "{key}: filled to the limit");
            let gone = names(&s).into_iter().find(|n| n.starts_with("Filler")).unwrap();
            let r = s.execute("building_code", &ask(json!({&gone: null, "Replacement": x})));
            assert!(r.is_ok(), "{key} {amend:?}: a change at the limit refused: {r:?}");
            let now = names(&s);
            assert_eq!(now.len(), 64, "{key} {amend:?}: one in the place of another");
            assert!(now.contains(&"Replacement".to_string()) && !now.contains(&gone), "{key} {amend:?}: the new one in the project");
            let in_code = s.model.resolve_code(Discipline::Electrical, key).value;
            assert!(in_code.get("Replacement").is_some_and(|v| !v.is_null()), "{key} {amend:?}: and in the code");
            let before = state(&s);
            let r = s.execute("building_code", &ask(json!({"Extra": x})));
            assert!(r.is_err(), "{key} {amend:?}: a 65th taken");
            assert_eq!(state(&s), before, "{key} {amend:?}: refused, and nothing changed");
        }
    }
}

/// Settings at the limit of 64, in every way a change can bring them (G6 and what its review found).
mod at_the_limit {
    use super::*;

    const V: &str = "voltages";
    const S: &str = "distribution_systems";
    const W: &str = "wire_types";
    const SECTIONS: [&str; 3] = [V, S, W];

    fn v(n: i32) -> Value {
        json!({"nominal":n,"min":n,"max":n})
    }
    fn sys(name: &str) -> Value {
        json!({"phases":1,"wires":2,"l_n":name})
    }
    fn sample(key: &str) -> Value {
        match key {
            V => v(50),
            S => sys("220 V"),
            _ => json!({"material":"copper","temperature":70,"insulation":"PVC","sizes":[{"section":1.5,"ampacity":15}]}),
        }
    }
    fn bc(s: &mut Session, mut p: Value) -> Result<Value, String> {
        p["discipline"] = json!("electrical");
        s.execute("building_code", &p)
    }
    fn section(s: &Session, key: &str) -> Map<String, Value> {
        s.model.resolve_code(Discipline::Electrical, key).value
    }
    fn model(s: &Session, key: &str) -> Map<String, Value> {
        match key {
            V => json!(s.model.voltages),
            S => json!(s.model.distribution_systems),
            _ => json!(s.model.wire_types),
        }
        .as_object()
        .unwrap()
        .clone()
    }
    fn fill(s: &mut Session, keys: &[&str], amend: Option<&str>) {
        let defs: Map<String, Value> = keys
            .iter()
            .map(|&key| {
                let start = model(s, key).len();
                let entries: Map<String, Value> = (start..64).map(|i| (format!("G6 {i:02}"), sample(key))).collect();
                (key.to_string(), Value::Object(entries))
            })
            .collect();
        let mut p = json!({"rules":defs});
        if let Some(a) = amend {
            p["amend"] = json!(a);
        }
        bc(s, p).unwrap();
        for key in keys {
            assert_eq!(model(s, key).len(), 64);
        }
    }
    fn snap(s: &Session) -> (String, String) {
        (state(s), format!("{:?} {:?}", s.steps(false), s.steps(true)))
    }
    fn assert_voltage(s: &Session, name: &str, nominal: f64) {
        assert_eq!(section(s, V)[name]["nominal"].as_f64(), Some(nominal));
        assert_eq!(s.model.voltages[name].nominal, nominal);
    }

    #[test]
    fn matrix_layers_and_combined_create_set_save() {
        let mut anomalies = Vec::new();
        for (label, layers, target) in [
            ("project", vec![], None),
            ("own lower", vec!["Site", "cpfl"], Some("Site")),
            ("own upper", vec!["cpfl", "Site"], Some("Site")),
            ("library lower", vec!["cpfl", "enel"], Some("cpfl")),
            ("library upper", vec!["enel", "cpfl"], Some("cpfl")),
        ] {
            let mut s = Session::new();
            let mut args = json!({"set":"nbr-5410","amendments":layers});
            if layers.contains(&"Site") {
                args["create"] = json!("Site");
            }
            bc(&mut s, args).unwrap();
            fill(&mut s, &SECTIONS, None);
            let mut rules = Map::new();
            for key in SECTIONS {
                rules.insert(key.into(), json!({"G6 62":null,"G6 63":null,"New A":sample(key),"New B":sample(key)}));
            }
            let mut args = json!({"rules":rules});
            if let Some(a) = target {
                args["amend"] = json!(a);
            }
            let r = bc(&mut s, args);
            let changed = r.is_ok()
                && SECTIONS.iter().all(|&key| {
                    let m = model(&s, key);
                    let c = s.model.resolve_code(Discipline::Electrical, key);
                    let layer = target.map_or(Layer::Project, |n| Layer::Amendment(n.into()));
                    m.len() == 64
                        && ["New A", "New B"].iter().all(|n| m.contains_key(*n) && c.from.get(*n) == Some(&layer))
                        && ["G6 62", "G6 63"].iter().all(|n| !m.contains_key(*n) && c.value.get(*n).is_none_or(Value::is_null))
                });
            let before = snap(&s);
            let mut refusals = Vec::new();
            for key in SECTIONS {
                let mut extra = json!({"rules":{key:{"Extra":sample(key)}}});
                if let Some(a) = target {
                    extra["amend"] = json!(a);
                }
                let r = bc(&mut s, extra);
                refusals.push((key, r.as_ref().err().cloned(), snap(&s) == before));
            }
            if !changed || refusals.iter().any(|(_, e, unchanged)| e.is_none() || !unchanged) {
                anomalies.push(label);
            }
        }
        let mut s = Session::new();
        fill(&mut s, &SECTIONS, None);
        let base = s.model.codes.base(Discipline::Electrical).code.clone();
        let r = bc(
            &mut s,
            json!({"set":base,"keep_rules":true,"create":"New site","amendments":["New site"],"amend":"New site","save":"Saved G6",
            "rules":{V:{"G6 63":null,"Incoming":v(61)},S:{"G6 63":null,"Incoming":sys("220 V")},W:{"G6 63":null,"Incoming":sample(W)}}}),
        );
        let combined = r.is_ok()
            && SECTIONS.iter().all(|&k| model(&s, k).contains_key("Incoming"))
            && s.model.codes.base(Discipline::Electrical).code == "Saved G6";
        if !combined {
            anomalies.push("combined");
        }
        assert!(anomalies.is_empty(), "{anomalies:?}");
    }

    #[test]
    fn both_kinds_full_and_voltage_freed_by_removal_of_system() {
        let mut s = Session::new();
        bc(&mut s, json!({"rules":{V:{"Outgoing":v(51)},S:{"Outgoing":sys("Outgoing")}}})).unwrap();
        fill(&mut s, &[V, S], None);
        bc(&mut s, json!({"rules":{V:{"Outgoing":null,"Incoming":v(62)},S:{"Outgoing":null,"Incoming":sys("Incoming")}}})).unwrap();
        assert_eq!(model(&s, V).len(), 64);
        assert_eq!(model(&s, S).len(), 64);
        assert!(!model(&s, V).contains_key("Outgoing"));
        assert!(!model(&s, S).contains_key("Outgoing"));
        assert_voltage(&s, "Incoming", 62.);
        assert_eq!(s.model.distribution_systems["Incoming"].l_n.as_deref(), Some("Incoming"));
    }

    #[test]
    fn new_system_with_room_needs_a_waiting_voltage() {
        let mut s = Session::new();
        fill(&mut s, &[V], None);
        bc(&mut s, json!({"rules":{V:{"G6 63":null,"Incoming":v(62)},S:{"New system":sys("Incoming")}}})).unwrap();
        let voltage = s.model.voltages.contains_key("Incoming");
        let setting = s.model.distribution_systems.get("New system");
        assert!(voltage && setting.is_some());
    }

    #[test]
    fn edited_system_releases_its_previous_voltage() {
        let mut s = Session::new();
        bc(&mut s, json!({"rules":{V:{"Outgoing":v(51)},S:{"Staying":sys("Outgoing")}}})).unwrap();
        fill(&mut s, &[V], None);
        let r = bc(&mut s, json!({"rules":{V:{"Outgoing":null,"Incoming":v(62)},S:{"Staying":sys("Incoming")}}}));
        assert!(
            r.is_ok()
                && !s.model.voltages.contains_key("Outgoing")
                && s.model.voltages.contains_key("Incoming")
                && s.model.distribution_systems["Staying"].l_n.as_deref() == Some("Incoming")
        );
        assert_voltage(&s, "Incoming", 62.);
        check_link(&s, "Staying", "Incoming");
        assert_eq!(s.model.voltages.len(), 64);
    }

    #[test]
    fn edited_system_can_keep_stale_voltage_silently() {
        let mut s = Session::new();
        bc(&mut s, json!({"rules":{V:{"Previous":v(51)},S:{"Staying":sys("Previous")}}})).unwrap();
        fill(&mut s, &[V], None);
        bc(&mut s, json!({"rules":{V:{"G6 63":null,"Incoming":v(62)},S:{"Staying":sys("Incoming")}}})).unwrap();
        assert_voltage(&s, "Previous", 51.);
        assert_voltage(&s, "Incoming", 62.);
        check_link(&s, "Staying", "Incoming");
    }

    #[test]
    fn without_full_settings_voltage_and_system_apply_immediately() {
        let mut s = Session::new();
        bc(&mut s, json!({"rules":{V:{"Outgoing":v(51)},S:{"Staying":sys("Outgoing")}}})).unwrap();
        bc(&mut s, json!({"rules":{V:{"Outgoing":null,"Incoming":v(62)},S:{"Staying":sys("Incoming"),"New system":sys("Incoming")}}}))
            .unwrap();
        assert!(!s.model.voltages.contains_key("Outgoing"));
        assert_voltage(&s, "Incoming", 62.);
        for n in ["Staying", "New system"] {
            assert_eq!(s.model.distribution_systems[n].l_n.as_deref(), Some("Incoming"));
        }
    }

    fn code_file(name: &str, rules: Value) -> std::path::PathBuf {
        let file = dir().join(format!("{name}.xml"));
        let k = BuildingCode {
            code: name.into(),
            name: name.into(),
            discipline: Discipline::Electrical,
            sections: serde_json::from_value(rules).unwrap(),
            ..Default::default()
        };
        std::fs::write(&file, super::super::xml::write(&[k])).unwrap();
        file
    }
    fn import(s: &mut Session, file: &std::path::Path, set: bool) -> Result<Value, String> {
        s.execute("building_code_import", &json!({"path":file,"set":set}))
    }

    #[test]
    fn imports_active_inactive_update_overflow_and_roundtrip() {
        let mut fail = Vec::new();
        let mut s = Session::new();
        fill(&mut s, &SECTIONS, None);
        let before: Vec<_> = SECTIONS.iter().map(|&k| model(&s, k)).collect();
        let export = dir().join("G6-export.xml");
        run(&mut s, "building_code_export", json!({"path":export,"discipline":"electrical","name":"G6 export"}));
        let mut t = Session::new();
        import(&mut t, &export, true).unwrap();
        for (i, k) in SECTIONS.iter().enumerate() {
            assert_eq!(model(&t, k), before[i]);
            assert_eq!(section(&t, k), section(&s, k));
        }

        let replacement: Map<String, Value> = SECTIONS
            .iter()
            .map(|&key| {
                let mut defs = section(&s, key);
                defs.insert("G6 63".into(), Value::Null);
                defs.insert("Incoming".into(), sample(key));
                (key.into(), Value::Object(defs))
            })
            .collect();
        let file = code_file("G6 active", json!(replacement));
        import(&mut s, &file, false).unwrap();
        for (i, k) in SECTIONS.iter().enumerate() {
            assert_eq!(model(&s, k), before[i]);
        }
        let r = import(&mut s, &file, true);
        let replaced = r.is_ok()
            && SECTIONS
                .iter()
                .all(|&k| model(&s, k).len() == 64 && model(&s, k).contains_key("Incoming") && !model(&s, k).contains_key("G6 63"));
        if !replaced {
            fail.push("active replacement");
        }

        let mut extra = replacement;
        extra[V]["Extra"] = v(63);
        let file = code_file("G6 active", json!(extra));
        let last = snap(&s);
        let r = import(&mut s, &file, false);
        if r.is_ok() || snap(&s) != last {
            fail.push("active overwrite");
        }

        let mut t = Session::new();
        fill(&mut t, &[V], None);
        let mut rules = section(&t, V);
        rules.insert("Extra".into(), v(63));
        let file = code_file("G6 inactive", json!({V:rules}));
        import(&mut t, &file, false).unwrap();
        assert_eq!(model(&t, V).len(), 64);
        assert!(!section(&t, V).contains_key("Extra"));
        let last = snap(&t);
        let r = import(&mut t, &file, true);
        if r.is_ok() || snap(&t) != last {
            fail.push("active overflow");
        }
        assert!(fail.is_empty(), "{fail:?}");
    }

    #[test]
    fn reset_and_switch_restore_starters_at_limit_and_bimc_roundtrip() {
        let mut s = Session::new();
        let project: Map<String, Value> = SECTIONS
            .iter()
            .map(|&k| {
                let current = model(&s, k);
                let name = match k {
                    V => "480 V",
                    S => "277/480 V",
                    _ => "Cu PVC 70 °C",
                };
                assert!(current.contains_key(name));
                (k.to_string(), json!({name:null}))
            })
            .collect();
        bc(&mut s, json!({"create":"Site","amendments":["Site"],"amend":"Site","rules":project})).unwrap();
        fill(&mut s, &SECTIONS, Some("Site"));
        let before = state(&s);
        let file = dir().join("G6 at limit.bimc");
        run(&mut s, "save", json!({"path":file}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path":file}));
        assert_eq!(state(&t), before);
        for (i, p) in
            [json!({"amend":"Site","reset":true,"save":"G6 reset"}), json!({"set":"generic","amendments":[]})].into_iter().enumerate()
        {
            if i != 0 {
                run(&mut t, "open", json!({"path":file}));
            }
            bc(&mut t, p).unwrap();
            assert_voltage_starter(&t);
        }
    }
    fn assert_voltage_starter(s: &Session) {
        assert_eq!(s.model.voltages.get("480 V").map(|x| x.nominal), Some(480.));
        assert!(s.model.distribution_systems.contains_key("277/480 V"));
        assert!(s.model.wire_types.contains_key("Cu PVC 70 °C"));
        assert!(SECTIONS.iter().all(|k| !model(s, k).contains_key("G6 63")));
    }

    #[test]
    fn overflows_preserve_undo_redo_and_report_the_kind() {
        let mut s = Session::new();
        fill(&mut s, &SECTIONS, None);
        let old = state(&s);
        run(&mut s, "level", json!({"name":"G6 level","elevation":5000}));
        run(&mut s, "undo", json!({}));
        let last = snap(&s);
        let mut fail = Vec::new();
        for (k, kind) in [(V, "voltage"), (S, "distribution system"), (W, "wire type")] {
            let r = bc(&mut s, json!({"rules":{k:{"ZZ overflow":sample(k)}}}));
            if !r.is_err_and(|e| e.contains(&format!("{kind} `ZZ overflow`")) && e.contains("64")) || snap(&s) != last {
                fail.push(kind);
            }
        }
        if fail.is_empty() {
            run(&mut s, "redo", json!({}));
            assert!(s.model.levels().into_iter().any(|(_, l)| l.name == "G6 level"));
            run(&mut s, "undo", json!({}));
            assert_eq!(state(&s), old);
            run(&mut s, "undo", json!({}));
            assert!(SECTIONS.iter().all(|k| model(&s, k).len() < 64));
            run(&mut s, "redo", json!({}));
            assert_eq!(state(&s), old);
        }
        assert!(fail.is_empty(), "{fail:?}");
    }

    #[test]
    fn a_panel_keeps_its_system_and_prevents_a_replacement() {
        let mut s = Session::new();
        bc(&mut s, json!({"create":"Referenced","amendments":["Referenced"],"amend":"Referenced","rules":{S:{"On panel":sys("220 V")}}}))
            .unwrap();
        fill(&mut s, &[S], None);
        crate::circuit_tests::board_on(&mut s, "On panel");
        let before = snap(&s);
        let p = json!({"amendments":[],"rules":{S:{"Replacement":sys("220 V")}}});
        let r = bc(&mut s, p);
        assert!(r.is_err_and(|e| e.contains("distribution system `Replacement`") && e.contains("64")) && snap(&s) == before);
        bc(&mut s, json!({"amendments":[]})).unwrap();
        assert!(s.model.distribution_systems.contains_key("On panel"));
        assert!(!section(&s, S).contains_key("On panel"));
    }

    #[test]
    fn xml_nulls_survive_export_from_project_and_two_layers_at_64() {
        let mut s = Session::new();
        bc(&mut s, json!({"create":"Lower","amendments":["Lower"],"amend":"Lower","rules":{S:{"277/480 V":null}}})).unwrap();
        bc(&mut s, json!({"create":"Upper","amendments":["Lower","Upper"],"amend":"Upper","rules":{V:{"480 V":null}}})).unwrap();
        bc(&mut s, json!({"rules":{W:{"Cu PVC 70 °C":null}}})).unwrap();
        fill(&mut s, &SECTIONS, None);
        let file = dir().join("G6 layers nulls.xml");
        run(&mut s, "building_code_export", json!({"path":file,"discipline":"electrical","name":"G6 nulls"}));
        let mut t = Session::new();
        let result = import(&mut t, &file, true);
        result.unwrap();
        for key in SECTIONS {
            assert_eq!(model(&s, key), model(&t, key));
            assert_eq!(section(&s, key), section(&t, key));
        }
    }

    #[test]
    fn import_system_waiting_for_a_voltage_at_capacity() {
        let mut failures = Vec::new();
        for set in [true, false] {
            let mut s = Session::new();
            if !set {
                let mut donor = Session::new();
                fill(&mut donor, &[V], None);
                let original = code_file("G6 overwrite dependency", json!({V:section(&donor,V)}));
                import(&mut s, &original, true).unwrap();
            } else {
                fill(&mut s, &[V], None);
            }
            assert_eq!(s.model.voltages.len(), 64);
            let mut volts = section(&s, V);
            volts.insert("G6 63".into(), Value::Null);
            volts.insert("Incoming".into(), v(62));
            let file = code_file("G6 overwrite dependency", json!({V:volts,S:{"New system":sys("Incoming")}}));
            let r = import(&mut s, &file, set);
            let voltage = s.model.voltages.contains_key("Incoming");
            let system = s.model.distribution_systems.contains_key("New system");
            if r.is_err() || !voltage || !system {
                failures.push(set);
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn a_starter_system_comes_back_when_its_starter_voltage_waits() {
        let mut s = Session::new();
        bc(&mut s, json!({"rules":{V:{"480 V":null},S:{"277/480 V":null}}})).unwrap();
        fill(&mut s, &[V], None);
        let mut volts = section(&s, V);
        volts.insert("G6 63".into(), Value::Null);
        volts.insert("480 V".into(), serde_json::to_value(&crate::electrical::voltages()["480 V"]).unwrap());
        let file = code_file("G6 back to starter", json!({V:volts}));
        import(&mut s, &file, false).unwrap();
        let r = bc(&mut s, json!({"set":"G6 back to starter"}));
        let voltage = s.model.voltages.get("480 V").map(|x| x.nominal);
        let system = s.model.distribution_systems.contains_key("277/480 V");
        assert!(r.is_ok() && voltage == Some(480.) && system && s.model.voltages.len() == 64);
        assert_voltage(&s, "480 V", 480.);
        let expected = &crate::electrical::distribution_systems()["277/480 V"];
        assert_eq!(model(&s, S)["277/480 V"], json!(expected));
        assert!(!s.model.voltages.contains_key("G6 63"));
    }

    #[test]
    fn two_replacements_from_63_keep_both_systems() {
        let mut s = Session::new();
        fill(&mut s, &[V], None);
        bc(&mut s, json!({"rules":{V:{"G6 61":null}}})).unwrap();
        assert_eq!(s.model.voltages.len(), 63);
        assert!(s.model.distribution_systems.len() < 64);
        let r = bc(
            &mut s,
            json!({"rules":{
                V:{"G6 62":null,"G6 63":null,"A incoming":v(61),"Z incoming":v(62)},
                S:{"A system":sys("A incoming"),"Z system":sys("Z incoming")}
            }}),
        );
        let observed = ["A", "Z"].map(|prefix| {
            (
                s.model.voltages.contains_key(&format!("{prefix} incoming")),
                s.model.distribution_systems.contains_key(&format!("{prefix} system")),
                section(&s, S).contains_key(&format!("{prefix} system")),
            )
        });
        assert!(r.is_ok() && s.model.voltages.len() == 63);
        assert_eq!(observed, [(true, true, true), (true, true, true)]);
    }

    fn check_link(s: &Session, system: &str, voltage: &str) {
        assert_eq!(section(s, S)[system]["l_n"].as_str(), Some(voltage));
        assert_eq!(s.model.distribution_systems[system].l_n.as_deref(), Some(voltage));
    }

    #[test]
    fn review_exact_replacements_from_63_and_64() {
        for count in [63, 64] {
            for route in ["command", "xml-set", "xml-overwrite"] {
                let mut donor = Session::new();
                fill(&mut donor, &[V], None);
                if count == 63 {
                    bc(&mut donor, json!({"rules":{V:{"G6 61":null}}})).unwrap();
                }
                let mut s = donor;
                let name = format!("G6 review {count} {route}");
                if route == "xml-overwrite" {
                    let file = code_file(&name, json!({V:section(&s,V)}));
                    import(&mut s, &file, true).unwrap();
                }
                assert_eq!(s.model.voltages.len(), count);
                let mut delta = json!({"G6 63":null,"Z incoming":v(62)});
                let mut systems = json!({"Z system":sys("Z incoming")});
                if count == 63 {
                    delta["G6 62"] = Value::Null;
                    delta["A incoming"] = v(61);
                    systems["A system"] = sys("A incoming");
                }
                if route == "command" {
                    bc(&mut s, json!({"rules":{V:delta,S:systems}})).unwrap();
                } else {
                    let mut volts = section(&s, V);
                    volts.extend(delta.as_object().unwrap().clone());
                    let file = code_file(&name, json!({V:volts,S:systems}));
                    import(&mut s, &file, route == "xml-set").unwrap();
                }
                assert_eq!(s.model.voltages.len(), count);
                assert!(!s.model.voltages.contains_key("G6 63"));
                assert_voltage(&s, "Z incoming", 62.);
                check_link(&s, "Z system", "Z incoming");
                if count == 63 {
                    assert!(!s.model.voltages.contains_key("G6 62"));
                    assert_voltage(&s, "A incoming", 61.);
                    check_link(&s, "A system", "A incoming");
                }
                let xml = dir().join(format!("{name} export.xml"));
                run(&mut s, "building_code_export", json!({"path":xml,"discipline":"electrical","name":format!("{name} export")}));
                let mut fresh = Session::new();
                import(&mut fresh, &xml, true).unwrap();
                for key in [V, S] {
                    assert_eq!(section(&fresh, key), section(&s, key));
                    assert_eq!(model(&fresh, key), model(&s, key));
                }
                let project = dir().join(format!("{name}.bimc"));
                let expected = state(&s);
                run(&mut s, "save", json!({"path":project}));
                run(&mut fresh, "open", json!({"path":project}));
                assert_eq!(state(&fresh), expected);
            }
        }
    }

    #[test]
    fn review_overflow_dependency_rolls_back() {
        for route in ["command", "xml-set", "xml-overwrite"] {
            let mut s = Session::new();
            fill(&mut s, &[V], None);
            let name = format!("G6 overflow dependent {route}");
            if route == "xml-overwrite" {
                let file = code_file(&name, json!({V:section(&s,V)}));
                import(&mut s, &file, true).unwrap();
            }
            let before = snap(&s);
            let result = if route == "command" {
                bc(&mut s, json!({"rules":{V:{"ZZ blocked":v(62)},S:{"New system":sys("ZZ blocked")}}}))
            } else {
                let mut volts = section(&s, V);
                volts.insert("ZZ blocked".into(), v(62));
                let file = code_file(&name, json!({V:volts,S:{"New system":sys("ZZ blocked")}}));
                import(&mut s, &file, route == "xml-set")
            };
            assert!(result.is_err_and(|e| e.contains("voltage `ZZ blocked`") && e.contains("64")));
            assert_eq!(snap(&s), before);
        }
    }

    #[test]
    fn review_voltage_still_named_by_system() {
        for full in [false, true] {
            let mut s = Session::new();
            bc(&mut s, json!({"rules":{V:{"Kept":v(51)}, S:{"Staying":sys("Kept")}}})).unwrap();
            if full {
                fill(&mut s, &[V], None);
            }
            let before = snap(&s);
            let result = bc(&mut s, json!({"rules":{V:{"Kept":null,"Incoming":v(62)}}}));
            assert!(result.is_err_and(|e| e.contains("Kept")));
            assert_eq!(snap(&s), before);
            assert_voltage(&s, "Kept", 51.);
            check_link(&s, "Staying", "Kept");
        }
    }
}

/// A utility's entrance or building method with a number below zero (a breaker, a factor) is refused, whether given in an
/// edit or found in a file; every one of BIMCraft's own has none.
mod not_negative {
    use super::*;

    const D: Discipline = Discipline::Electrical;

    fn with(key: &str, code: &str, edit: impl Fn(&mut Value)) -> Value {
        let mut v = find(D, code).unwrap().section(key).map(|o| Value::Object(o.clone())).unwrap();
        edit(&mut v);
        v
    }

    fn amend(s: &mut Session, code: &str, key: &str, v: Value) -> Result<Value, String> {
        s.execute("building_code", &json!({"discipline": "electrical", "amend": code, "rules": {key: v}}))
    }

    #[test]
    fn a_negative_breaker_or_factor_is_refused_in_an_edit_and_changes_nothing() {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["fecoergs", "cemig"]}));
        let before = s.model.codes.clone();
        let breaker = with("entrance", "fecoergs", |v| v["tables"][0]["rows"][0]["breaker_a"][0] = json!(-1));
        assert!(amend(&mut s, "fecoergs", "entrance", breaker).is_err());
        let factor = with("building_demand", "cemig", |v| v["factor"] = json!(-1));
        assert!(amend(&mut s, "cemig", "building_demand", factor).is_err());
        assert_eq!(s.model.codes, before);
        // Zero, and the library's own numbers, are taken.
        let zero = with("building_demand", "cemig", |v| v["factor"] = json!(0));
        assert!(amend(&mut s, "cemig", "building_demand", zero).is_ok());
    }

    #[test]
    fn a_file_with_a_negative_breaker_does_not_open() {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["fecoergs"]}));
        amend(&mut s, "fecoergs", "entrance", with("entrance", "fecoergs", |_| {})).unwrap();
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "fecoergs").unwrap();
        own.sections.get_mut("entrance").unwrap()["tables"][0]["rows"][0]["breaker_a"][0] = json!(-1);
        let path = dir().join("negative-breaker.bimc");
        run(&mut s, "save", json!({"path": path}));
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
    }

    #[test]
    fn every_library_entrance_and_building_method_has_no_negative_number() {
        for c in of(D) {
            for key in ["entrance", "building_demand"] {
                let Some(v) = c.section(key) else { continue };
                let def = section_def(D, key).unwrap();
                assert!(def.check(&Value::Object(v.clone())).is_ok(), "{} {key}", c.code);
            }
        }
    }

    #[test]
    fn a_printed_column_or_a_falling_curve_may_be_below_zero_and_the_message_says_where() {
        // Columns are shown as printed, never read; a curve's exponent may be negative.
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["fecoergs"]}));
        let column = with("entrance", "fecoergs", |v| v["tables"][0]["rows"][0]["columns"]["temp_min_c"] = json!(-5));
        assert!(amend(&mut s, "fecoergs", "entrance", column).is_ok());
        let breaker = with("entrance", "fecoergs", |v| v["tables"][0]["rows"][1]["breaker_a"][0] = json!(-1));
        let e = amend(&mut s, "fecoergs", "entrance", breaker).unwrap_err();
        assert!(e.contains("tables[0].rows[1].breaker_a[0]"), "{e}");
    }
}

#[test]
fn two_versions_of_a_utility_answer_by_the_one_on_top() {
    // A project's copy of a utility (`Lower`, its method changed) and BIMCraft's, both held: the one on top answers, in
    // either order, for every utility's method (G3, G3b).
    let mut asked = 0;
    for code in of(Discipline::Electrical).filter(|c| !c.amends.is_empty()) {
        for key in ["unit_demand", "building_demand", "entrance"] {
            if !code.sections.contains_key(key) {
                continue;
            }
            let answer = |s: &mut Session| {
                let (cmd, probe) = method_probe(code, key, &code.sections[key]);
                let r = run(s, cmd, probe);
                if key == "building_demand" { r["demand"].clone() } else { r["category"].clone() }
            };
            let held = |amendments: Value| {
                let mut s = Session::new();
                run(&mut s, "building_code", json!({"discipline": "electrical", "set": code.amends, "amendments": amendments}));
                s
            };
            // The copy: the utility changed, exported under another name.
            let mut source = held(json!([code.code]));
            run(
                &mut source,
                "building_code",
                json!({"discipline": "electrical", "amend": code.code, "rules": {key: changed(code, key, 1)}}),
            );
            let changed_answer = answer(&mut source);
            let file = dir().join(format!("lower-{}-{key}.xml", code.code));
            run(&mut source, "building_code_export", json!({"path": file, "discipline": "electrical", "code": code.code, "name": "Lower"}));
            let library_answer = answer(&mut held(json!([code.code])));
            assert_ne!(changed_answer, library_answer, "{} {key}: the change shows", code.code);
            for (order, expected) in [(json!(["Lower", code.code]), &library_answer), (json!([code.code, "Lower"]), &changed_answer)] {
                let mut s = Session::new();
                run(&mut s, "building_code_import", json!({"path": file, "set": false}));
                run(&mut s, "building_code", json!({"discipline": "electrical", "set": code.amends, "amendments": order}));
                assert_eq!(&answer(&mut s), expected, "{} {key} held as {order}: the one on top answers", code.code);
                asked += 1;
            }
        }
    }
    assert!(asked >= 16, "cases asked ({asked})");
}

/// Two versions of a utility, and the one on top (G3 and what its review found).
mod one_on_top {
    use super::*;

    const D: Discipline = Discipline::Electrical;

    fn bc(s: &mut Session, mut p: Value) -> Value {
        p["discipline"] = json!("electrical");
        run(s, "building_code", p)
    }

    fn choose(s: &mut Session, order: &[&str]) {
        bc(s, json!({"set": "nbr-5410", "amendments": order}));
    }

    fn make_copy(s: &mut Session, code: &BuildingCode, key: &str, name: &str, delta: u32) {
        let mut source = Session::new();
        choose(&mut source, &[&code.code]);
        bc(&mut source, json!({"amend": code.code, "rules": {key: changed(code, key, delta)}}));
        let file = dir().join(format!("g3-{}-{key}-{name}.xml", code.code));
        run(&mut source, "building_code_export", json!({"path": file, "discipline": "electrical", "code": code.code, "name": name}));
        run(s, "building_code_import", json!({"path": file, "set": false}));
    }

    fn answer(s: &mut Session, cmd: &str, p: &Value) -> Result<Value, String> {
        s.execute(cmd, p).map(|v| {
            if cmd == "building_demand" {
                v["demand"].clone()
            } else {
                let breaker = &v["category"]["breaker_a"];
                if breaker.is_array() { breaker[0].clone() } else { breaker.clone() }
            }
        })
    }

    fn display(s: &mut Session, key: &str) -> Value {
        let said = bc(s, json!({}));
        let r = run(s, "code_rules", json!({"discipline": "electrical", "section": key}));
        let got: serde_json::Map<String, Value> = r["sections"][0]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v["rule"].as_str().unwrap().to_string(), v["value"].clone()))
            .collect();
        assert_eq!(Value::Object(got), said["sections"][key]);
        said["sections"][key].clone()
    }

    fn probe(failures: &mut Vec<String>, label: &str, s: &mut Session, cmd: &str, p: &Value, wanted: &Value) {
        let got = answer(s, cmd, p);
        if got.as_ref() != Ok(wanted) {
            failures.push(format!("{label}: {cmd} {p}: expected {wanted}, got {got:?}"));
        }
    }

    #[test]
    fn all_library_methods_and_copies() {
        let mut failed = Vec::new();
        let mut tested = Vec::new();
        for code in of(D).filter(|c| c.amends == "nbr-5410") {
            for key in ["unit_demand", "entrance", "building_demand"] {
                let Some(method) = code.sections.get(key) else {
                    continue;
                };
                let id = &*code.code;
                tested.push(format!("{id}/{key}"));
                let (cmd, params) = method_probe(code, key, method);
                let mut s = Session::new();
                let lib = answer(&mut s, cmd, &params).unwrap();
                assert!(lib.is_number(), "{id}/{key}: {lib}");
                make_copy(&mut s, code, key, "ReviewCopyA", 1);
                make_copy(&mut s, code, key, "ReviewCopyB", 2);
                let mut only = params.clone();
                only.as_object_mut().unwrap().remove("utility");
                only.as_object_mut().unwrap().remove("standard");
                let mut by_utility = only.clone();
                by_utility["utility"] = if cmd == "building_demand" { method["utilities"][0].clone() } else { params["utility"].clone() };
                let mut lower_case = by_utility.clone();
                lower_case["utility"] = json!(by_utility["utility"].as_str().unwrap().to_ascii_lowercase());
                let mut refs = Vec::new();
                for name in ["ReviewCopyA", "ReviewCopyB"] {
                    choose(&mut s, &[name]);
                    refs.push((answer(&mut s, cmd, &params).unwrap(), display(&mut s, key)));
                }
                assert_ne!(lib, refs[0].0);
                assert_ne!(refs[0].0, refs[1].0);
                if cmd == "demand" {
                    for (i, (breaker, _)) in refs.iter().enumerate() {
                        assert_eq!(breaker.as_f64().unwrap(), lib.as_f64().unwrap() + (i + 1) as f64);
                    }
                }
                choose(&mut s, &[id]);
                let lib_rules = display(&mut s, key);
                for (order, want, rules) in [
                    ([id, "ReviewCopyA"], &refs[0].0, &refs[0].1),
                    (["ReviewCopyA", id], &lib, &lib_rules),
                    (["ReviewCopyA", "ReviewCopyB"], &refs[1].0, &refs[1].1),
                    (["ReviewCopyB", "ReviewCopyA"], &refs[0].0, &refs[0].1),
                ] {
                    choose(&mut s, &order);
                    let label = format!("{id}/{key} {order:?}");
                    assert_eq!(&display(&mut s, key), rules, "{label}");
                    for p in [&params, &by_utility, &lower_case, &only] {
                        probe(&mut failed, &label, &mut s, cmd, p, want);
                    }
                }
                // NBR 5410's CEMIG has no unit or entrance method. CPFL has no building method.
                let no_method = if key == "building_demand" { "cpfl" } else { "cemig" };
                choose(&mut s, &["ReviewCopyA", no_method]);
                for p in [&params, &by_utility, &lower_case] {
                    probe(&mut failed, "library top without section", &mut s, cmd, p, &refs[0].0);
                }
                probe(&mut failed, "implicit with no section on top", &mut s, cmd, &only, &refs[0].0);
                // Different utility with the same section on top forces lookup by identity.
                if let Some(other) = of(D).find(|o| o.amends == code.amends && o.code != code.code && o.sections.contains_key(key)) {
                    choose(&mut s, &["ReviewCopyA", id, &other.code]);
                    for p in [&params, &by_utility, &lower_case] {
                        probe(&mut failed, "other utility on top of library", &mut s, cmd, p, &lib);
                    }
                    choose(&mut s, &[id, "ReviewCopyA", &other.code]);
                    for p in [&params, &by_utility, &lower_case] {
                        probe(&mut failed, "other utility on top of copy", &mut s, cmd, p, &refs[0].0);
                    }
                }
            }
        }
        assert_eq!(tested.len(), 12, "{tested:?}");
        assert!(failed.is_empty(), "{} different answers:\n{}", failed.len(), failed.join("\n"));
    }

    #[test]
    fn cpfl_same_identity_reset_save_export_and_reopen() {
        let cpfl = find(D, "cpfl").unwrap();
        let p = json!({"utility": "cpfl", "area": 40});
        let mut failed = Vec::new();
        for (order, expected) in [(["ResetCopy", "cpfl"], 32.0), (["cpfl", "ResetCopy"], 33.0)] {
            let mut s = Session::new();
            make_copy(&mut s, cpfl, "unit_demand", "ResetCopy", 1);
            choose(&mut s, &order);
            let before = display(&mut s, "unit_demand");
            let mut implicit = p.clone();
            implicit.as_object_mut().unwrap().remove("utility");
            let expected = json!(expected);
            for params in [&p, &implicit] {
                probe(&mut failed, "before persistence", &mut s, "demand", params, &expected);
            }
            let bimc = dir().join("g3-same-identity.bimc");
            run(&mut s, "save", json!({"path": bimc, "compress": true}));
            let mut reopened = Session::new();
            run(&mut reopened, "open", json!({"path": bimc}));
            let xml = dir().join("g3-same-identity.xml");
            run(&mut s, "building_code_export", json!({"path": xml, "discipline": "electrical", "name": "ReviewExport"}));
            let mut imported = Session::new();
            run(&mut imported, "building_code_import", json!({"path": xml, "set": true}));
            let mut saved = Session::new();
            *saved.model_mut() = (*s.model).clone();
            bc(&mut saved, json!({"save": "ReviewSave"}));
            for (label, t) in [("bimc", &mut reopened), ("xml", &mut imported), ("building_code save", &mut saved)] {
                assert_eq!(display(t, "unit_demand"), before, "{order:?} {label}");
                for params in [&p, &implicit] {
                    probe(&mut failed, label, t, "demand", params, &expected);
                }
            }
            bc(&mut s, json!({"amend": "ResetCopy", "reset": "unit_demand"}));
            for params in [&p, &implicit] {
                probe(&mut failed, "reset copy", &mut s, "demand", params, &json!(32.0));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    #[test]
    fn building_demand_country_base_without_amendments() {
        let nec = find(D, "nec").unwrap();
        let key = "building_demand";
        let (cmd, p) = method_probe(nec, key, &nec.sections[key]);
        let mut s = Session::new();
        let want = answer(&mut s, cmd, &p).unwrap();
        assert!(want.is_number());
        bc(&mut s, json!({"set": "nec", "amendments": []}));
        assert_eq!(display(&mut s, key), nec.sections[key]);
        let mut implicit = p.clone();
        implicit.as_object_mut().unwrap().remove("standard");
        assert_eq!(answer(&mut s, cmd, &p), Ok(want.clone()));
        assert_eq!(answer(&mut s, cmd, &implicit), Ok(want));
    }
    #[test]
    fn covered_building_library_across_save_and_xml_roundtrip() {
        let cemig = find(D, "cemig").unwrap();
        let key = "building_demand";
        let (cmd, params) = method_probe(cemig, key, &cemig.sections[key]);
        let mut s = Session::new();
        let library = answer(&mut s, cmd, &params).unwrap();
        make_copy(&mut s, cemig, key, "CoveredCopy", 1);
        choose(&mut s, &["CoveredCopy", "cemig", "celesc"]);
        let before = answer(&mut s, cmd, &params).unwrap();
        let effective = display(&mut s, key);
        let xml = dir().join("g3-covered-building-library.xml");
        run(&mut s, "building_code_export", json!({"path": xml, "discipline": "electrical", "name": "BuildingExport"}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"path": xml, "set": true}));
        bc(&mut s, json!({"save": "BuildingSave"}));
        let mut failed = Vec::new();
        if before != library {
            failed.push(format!("before persistence: expected {library}, got {before}"));
        }
        for (label, t) in [("save", &mut s), ("xml", &mut imported)] {
            assert_eq!(display(t, key), effective);
            probe(&mut failed, label, t, cmd, &params, &before);
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    // The confirmation's cases (G3).
    // Insert inside matrix_tests::one_on_top.

    #[test]
    fn review_building_both_orders_survive_all_roundtrips() {
        let mut failures = Vec::new();
        for (id, library) in [("celesc", 53.157), ("cemig", 62.017), ("fecoergs", 53.157), ("hidropan", 53.157)] {
            let code = find(D, id).unwrap();
            let key = "building_demand";
            let method = &code.sections[key];
            let other = if id == "celesc" { "cemig" } else { "celesc" };
            let (_, standard) = method_probe(code, key, method);
            let utility = json!({"utility": method["utilities"][0], "dwellings": standard["dwellings"]});
            let mut s = Session::new();
            make_copy(&mut s, code, key, "RoundtripCopy", 1);
            for (pair, expected) in [(["RoundtripCopy", id], library), ([id, "RoundtripCopy"], 88.595)] {
                choose(&mut s, &[pair[0], pair[1], other]);
                let mut check = |stage: &str, t: &mut Session| {
                    for p in [&standard, &utility] {
                        probe(&mut failures, &format!("{id} {pair:?} {stage}"), t, "building_demand", p, &json!(expected));
                    }
                    let factors: Vec<_> = t
                        .model
                        .codes
                        .layers(D)
                        .iter()
                        .filter_map(|layer| layer.sections.get(key))
                        .filter(|rules| rules["standard"] == method["standard"])
                        .map(|rules| rules["factor"].as_f64().unwrap())
                        .collect();
                    let library_factor = method["factor"].as_f64().unwrap();
                    let expected_factors = if pair[0] == id { vec![library_factor, 2.0] } else { vec![2.0, library_factor] };
                    if factors != expected_factors {
                        failures.push(format!("{id} {pair:?} {stage}: expected both factors {expected_factors:?}, got {factors:?}"));
                    }
                    // Check the rule in force for the unrelated utility as well.
                    let expected_other = if other == "cemig" { 62.017 } else { 53.157 };
                    probe(
                        &mut failures,
                        &format!("{id} {pair:?} {stage}, implicit"),
                        t,
                        "building_demand",
                        &json!({"dwellings": standard["dwellings"]}),
                        &json!(expected_other),
                    );
                };
                check("before", &mut s);
                let bimc = dir().join("g3r2-building.bimc");
                run(&mut s, "save", json!({"path": bimc, "compress": true}));
                let mut reopened = Session::new();
                run(&mut reopened, "open", json!({"path": bimc}));
                check("bimc", &mut reopened);
                let xml = dir().join("g3r2-building.xml");
                run(&mut s, "building_code_export", json!({"path": xml, "discipline": "electrical", "name": "BuildingExport"}));
                let mut imported = Session::new();
                run(&mut imported, "building_code_import", json!({"path": xml, "set": true}));
                check("xml", &mut imported);
                let mut saved = Session::new();
                *saved.model_mut() = (*s.model).clone();
                bc(&mut saved, json!({"save": "BuildingSave"}));
                check("building_code save", &mut saved);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn review_named_and_implicit_with_or_without_a_single_amendment() {
        for (id, key) in [
            ("fecoergs", "entrance"),
            ("cpfl", "unit_demand"),
            ("cemig", "building_demand"),
            ("celesc", "building_demand"),
            ("fecoergs", "building_demand"),
            ("hidropan", "building_demand"),
        ] {
            let code = find(D, id).unwrap();
            let method = &code.sections[key];
            let (cmd, standard_or_named) = method_probe(code, key, method);
            let mut named = standard_or_named.clone();
            let mut implicit = named.clone();
            implicit.as_object_mut().unwrap().remove("standard");
            implicit.as_object_mut().unwrap().remove("utility");
            if cmd == "building_demand" {
                named = implicit.clone();
                named["utility"] = method["utilities"][0].clone();
            }
            let mut none = Session::new();
            choose(&mut none, &[]);
            let baseline = run(&mut none, cmd, named.clone());
            assert!(none.execute(cmd, &implicit).is_err(), "{id}: no utility in force");
            let mut held = Session::new();
            choose(&mut held, &[id]);
            for params in [named, standard_or_named, implicit] {
                assert_eq!(run(&mut held, cmd, params), baseline, "{id}: all output, including source");
            }
        }
    }

    #[test]
    fn review_group_member_held_alone_keeps_50_a_and_source() {
        let mut none = Session::new();
        let mut held = Session::new();
        choose(&mut held, &["fecoergs"]);
        for id in ["CERTEL", "certel"] {
            let p = json!({"utility": id, "installed_kw": 8, "phases": 1});
            let baseline = run(&mut none, "demand", p.clone());
            assert_eq!(answer(&mut held, "demand", &p).unwrap().as_f64(), Some(50.0));
            assert_eq!(run(&mut held, "demand", p), baseline);
        }
    }

    #[test]
    fn review_separate_entrance_and_unit_demand_keep_their_own_rules() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs", "cpfl"]);
        let entrance = json!({"utility": "FECOERGS", "installed_kw": 8, "phases": 1});
        let unit = json!({"utility": "CPFL", "area": 40});
        let implicit = json!({"area": 40});
        assert_eq!(answer(&mut s, "demand", &entrance).unwrap().as_f64(), Some(50.0));
        for p in [&unit, &implicit] {
            assert_eq!(answer(&mut s, "demand", p).unwrap().as_f64(), Some(32.0));
        }
    }

    #[test]
    fn review_project_edits_win_and_survive_reopen_and_xml() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs", "cpfl", "cemig"]);
        for (id, key) in [("fecoergs", "entrance"), ("cpfl", "unit_demand"), ("cemig", "building_demand")] {
            let code = find(D, id).unwrap();
            bc(&mut s, json!({"amend": id, "rules": {key: changed(code, key, 1)}}));
        }
        let check = |t: &mut Session| {
            for (cmd, p, expected) in [
                ("demand", json!({"utility": "CERTEL", "installed_kw": 8, "phases": 1}), 51.0),
                ("demand", json!({"utility": "CPFL", "area": 40}), 33.0),
                ("demand", json!({"area": 40}), 33.0),
                ("building_demand", json!({"utility": "CEMIG", "dwellings": [{"count": 20, "area": 120}]}), 88.595),
                ("building_demand", json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 20, "area": 120}]}), 88.595),
                ("building_demand", json!({"dwellings": [{"count": 20, "area": 120}]}), 88.595),
            ] {
                assert_eq!(answer(t, cmd, &p).unwrap().as_f64(), Some(expected), "{cmd} {p}");
            }
        };
        check(&mut s);
        let bimc = dir().join("g3r2-project-edits.bimc");
        run(&mut s, "save", json!({"path": bimc, "compress": true}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": bimc}));
        check(&mut reopened);
        let xml = dir().join("g3r2-project-edits.xml");
        run(&mut s, "building_code_export", json!({"path": xml, "discipline": "electrical", "name": "EditExport"}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"path": xml, "set": true}));
        check(&mut imported);
    }

    #[test]
    fn review_project_unit_demand_above_library_entrance_for_same_utility() {
        let mut s = Session::new();
        make_copy(&mut s, find(D, "celesc").unwrap(), "unit_demand", "UnitCopy", 2);
        choose(&mut s, &["fecoergs", "UnitCopy"]);
        bc(&mut s, json!({"amend": "UnitCopy", "rules": {"unit_demand": {"utility": "FECOERGS"}}}));
        let mut failures = Vec::new();
        for p in [json!({"utility": "FECOERGS", "installed_kw": 8, "phases": 1}), json!({"installed_kw": 8, "phases": 1})] {
            let actual = answer(&mut s, "demand", &p).unwrap().as_f64().unwrap();
            if actual != 52.0 {
                failures.push(format!("{p}: expected the project's 52 A, got {actual} A"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // G3b: the service entrance among versions.

    #[test]
    fn group_member_with_both_orders() {
        let code = find(D, "fecoergs").unwrap();
        let method = &code.sections["entrance"];
        let (cmd, mut p) = method_probe(code, "entrance", method);
        p["utility"] = json!("CERTEL");
        let mut s = Session::new();
        let lib = answer(&mut s, cmd, &p).unwrap();
        assert!(lib.is_number(), "{lib}");
        make_copy(&mut s, code, "entrance", "GroupCopy", 1);
        choose(&mut s, &["GroupCopy"]);
        let changed = answer(&mut s, cmd, &p).unwrap();
        assert_ne!(changed, lib);
        let mut failures = Vec::new();
        for (order, expected) in [(["GroupCopy", "fecoergs"], lib), (["fecoergs", "GroupCopy"], changed)] {
            choose(&mut s, &order);
            for utility in ["CERTEL", "certel"] {
                p["utility"] = json!(utility);
                probe(&mut failures, &format!("{order:?}"), &mut s, cmd, &p, &expected);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn top_library_entrance_over_own_unit_demand_and_entrance() {
        let fecoergs = find(D, "fecoergs").unwrap();
        let celesc = find(D, "celesc").unwrap();
        let mut s = Session::new();
        make_copy(&mut s, fecoergs, "entrance", "EntranceCopy", 1);
        make_copy(&mut s, celesc, "unit_demand", "UnitCopy", 2);
        choose(&mut s, &["UnitCopy", "EntranceCopy", "fecoergs"]);
        bc(&mut s, json!({"amend": "UnitCopy", "rules": {"unit_demand": {"utility": "FECOERGS"}}}));
        let p = json!({"utility": "FECOERGS", "installed_kw": 8, "phases": 1});
        let mut reference = Session::new();
        let want = answer(&mut reference, "demand", &p).unwrap();
        assert!(want.is_number());
        assert_eq!(display(&mut s, "entrance"), fecoergs.sections["entrance"]);
        let mut failed = Vec::new();
        probe(&mut failed, "library entrance on top, named", &mut s, "demand", &p, &want);
        probe(&mut failed, "library entrance on top, implicit", &mut s, "demand", &json!({"installed_kw": 8, "phases": 1}), &want);
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    #[test]
    fn review_fecoergs_three_layers_is_50_a() {
        let mut s = Session::new();
        make_copy(&mut s, find(D, "fecoergs").unwrap(), "entrance", "EntranceCopy", 1);
        make_copy(&mut s, find(D, "celesc").unwrap(), "unit_demand", "UnitCopy", 2);
        choose(&mut s, &["UnitCopy", "EntranceCopy", "fecoergs"]);
        bc(&mut s, json!({"amend": "UnitCopy", "rules": {"unit_demand": {"utility": "FECOERGS"}}}));
        let mut p = json!({"installed_kw": 8, "phases": 1});
        for utility in [Some("FECOERGS"), None] {
            match utility {
                Some(id) => p["utility"] = json!(id),
                None => {
                    p.as_object_mut().unwrap().remove("utility");
                }
            }
            assert_eq!(answer(&mut s, "demand", &p).unwrap().as_f64(), Some(50.0));
        }
    }

    #[test]
    fn named_and_implicit_legacy_change_of_library_method_with_no_amendment() {
        let cpfl = find(D, "cpfl").unwrap();
        let mut s = Session::new();
        let rules = changed(cpfl, "unit_demand", 1);
        s.model_mut().codes.electrical.changes.insert("unit_demand".into(), rules.as_object().unwrap().clone());
        let path = dir().join("g3-legacy-unheld.bimc");
        run(&mut s, "save", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path}));
        assert_eq!(display(&mut t, "unit_demand"), rules);
        let mut failures = Vec::new();
        for p in [json!({"area": 40}), json!({"utility": "CPFL", "area": 40})] {
            probe(&mut failures, "project-wide change after reopen", &mut t, "demand", &p, &json!(33.0));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[test]
fn a_utility_held_alone_answers_as_bimcrafts_own_word_for_word() {
    // Held as BIMCraft has it, alone: `demand` answers exactly as BIMCraft's own, its source too, by name and by none.
    for code in of(Discipline::Electrical).filter(|c| !c.amends.is_empty()) {
        for key in ["unit_demand", "entrance"] {
            if !code.sections.contains_key(key) {
                continue;
            }
            let (cmd, probe) = method_probe(code, key, &code.sections[key]);
            let mut held = Session::new();
            run(&mut held, "building_code", json!({"discipline": "electrical", "set": code.amends, "amendments": [code.code]}));
            let mut none = Session::new();
            let named = run(&mut held, cmd, probe.clone());
            assert_eq!(named, run(&mut none, cmd, probe.clone()), "{} {key}: held alone, as BIMCraft's own", code.code);
            let mut implicit = probe.clone();
            if let Some(o) = implicit.as_object_mut() {
                o.remove("utility");
            }
            assert_eq!(run(&mut held, cmd, implicit), named, "{} {key}: by none, as by name", code.code);
        }
    }
}

/// A utility's methods across layers, members and saving (G3b's review).
mod review_g3b {
    use super::*;

    const D: Discipline = Discipline::Electrical;

    fn bc(s: &mut Session, mut p: Value) -> Value {
        p["discipline"] = json!("electrical");
        run(s, "building_code", p)
    }

    fn copy(s: &mut Session, name: &str, origin: &str, key: &str, identity: Option<&str>, delta: u32) {
        let code = find(D, origin).unwrap();
        let mut method = changed(code, key, delta);
        if let Some(id) = identity {
            if key == "entrance" {
                method["id"] = json!(id);
                method["members"] = json!([]);
            } else {
                method["utility"] = json!(id);
            }
        }
        bc(s, json!({"set": "nbr-5410", "create": name, "amendments": [name]}));
        bc(s, json!({"amend": name, "rules": {key: method}}));
    }

    fn choose(s: &mut Session, order: &[&str]) {
        bc(s, json!({"set": "nbr-5410", "amendments": order}));
    }

    fn breaker(s: &mut Session, p: &Value) -> Result<f64, String> {
        let reply = s.execute("demand", p)?;
        let n = &reply["category"]["breaker_a"];
        n.as_f64().or_else(|| n[0].as_f64()).ok_or_else(|| format!("sem disjuntor: {reply}"))
    }

    fn expect_breaker(s: &mut Session, p: &Value, expected: f64) {
        assert_eq!(breaker(s, p), Ok(expected), "{p}");
    }

    fn fecoergs() -> Value {
        json!({"utility": "fEcOeRgS", "installed_kw": 8, "phases": 1})
    }

    fn mixed_layers() -> Session {
        let mut s = Session::new();
        copy(&mut s, "Entrada", "fecoergs", "entrance", None, 1);
        copy(&mut s, "Unidade", "celesc", "unit_demand", Some("FECOERGS"), 2);
        choose(&mut s, &["Entrada", "fecoergs", "Unidade"]);
        s
    }

    #[test]
    fn unit_above_two_entrances_survives_building_code_save() {
        let mut s = mixed_layers();
        let before = breaker(&mut s, &fecoergs());
        bc(&mut s, json!({"save": "ReviewMixedSaved"}));
        let after = breaker(&mut s, &fecoergs());
        assert_eq!((before, after), (Ok(52.0), Ok(52.0)));
    }

    #[test]
    fn unit_above_two_entrances_survives_xml_roundtrip() {
        let mut s = mixed_layers();
        let before = breaker(&mut s, &fecoergs());
        let path = dir().join("review-g3b-mixed.xml");
        run(&mut s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": "ReviewMixedXml"}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"path": path, "set": true}));
        let after = breaker(&mut imported, &fecoergs());
        assert_eq!((before, after), (Ok(52.0), Ok(52.0)));
    }

    #[test]
    fn unit_above_two_entrances_survives_bimc_and_reset() {
        let mut s = mixed_layers();
        let before = breaker(&mut s, &fecoergs());
        let path = dir().join("review-g3b-mixed.bimc");
        run(&mut s, "save", json!({"path": path, "compress": true}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        let after = breaker(&mut reopened, &fecoergs());
        bc(&mut reopened, json!({"amend": "Unidade", "reset": "unit_demand"}));
        let reset = breaker(&mut reopened, &fecoergs());
        assert_eq!((before, after, reset), (Ok(52.0), Ok(52.0), Ok(50.0)));
    }

    #[test]
    fn implicit_utility_is_the_entrance_in_force_over_another_utility() {
        let mut s = Session::new();
        choose(&mut s, &["celesc", "fecoergs"]);
        expect_breaker(&mut s, &fecoergs(), 50.0);
        expect_breaker(&mut s, &json!({"utility": "celesc", "installed_kw": 8, "phases": 1}), 50.0);
        // Give the other utility a different number so an accidental match cannot pass.
        bc(&mut s, json!({"amend": "celesc", "rules": {"unit_demand": changed(find(D, "celesc").unwrap(), "unit_demand", 2)}}));
        expect_breaker(&mut s, &json!({"installed_kw": 8, "phases": 1}), 50.0);
    }

    #[test]
    fn group_unit_above_group_entrance_answers_for_its_member() {
        let mut s = Session::new();
        copy(&mut s, "Grupo unidade", "celesc", "unit_demand", Some("FECOERGS"), 2);
        choose(&mut s, &["fecoergs", "Grupo unidade"]);
        expect_breaker(&mut s, &fecoergs(), 52.0);
        expect_breaker(&mut s, &json!({"utility": "cErTeL", "installed_kw": 8, "phases": 1}), 52.0);
    }

    #[test]
    fn member_unit_above_group_entrance_answers_for_that_member() {
        let mut s = Session::new();
        copy(&mut s, "Certel unidade", "celesc", "unit_demand", Some("CERTEL"), 2);
        choose(&mut s, &["fecoergs", "Certel unidade"]);
        expect_breaker(&mut s, &fecoergs(), 50.0);
        expect_breaker(&mut s, &json!({"utility": "cErTeL", "installed_kw": 8, "phases": 1}), 52.0);
        choose(&mut s, &["Certel unidade", "fecoergs"]);
        expect_breaker(&mut s, &json!({"utility": "cErTeL", "installed_kw": 8, "phases": 1}), 50.0);
    }

    #[test]
    fn entrance_and_unit_on_the_same_layer_answer_by_entrance() {
        let mut s = Session::new();
        copy(&mut s, "Ambos", "fecoergs", "entrance", None, 1);
        let mut unit = changed(find(D, "celesc").unwrap(), "unit_demand", 2);
        unit["utility"] = json!("FECOERGS");
        bc(&mut s, json!({"amend": "Ambos", "rules": {"unit_demand": unit}}));
        choose(&mut s, &["Ambos"]);
        expect_breaker(&mut s, &fecoergs(), 51.0);
        expect_breaker(&mut s, &json!({"installed_kw": 8, "phases": 1}), 51.0);
    }

    #[test]
    fn member_entry_and_group_entrance_follow_layer_order() {
        let mut s = Session::new();
        copy(&mut s, "Certel entrada", "fecoergs", "entrance", Some("CERTEL"), 3);
        let p = json!({"utility": "cErTeL", "installed_kw": 8, "phases": 1});
        choose(&mut s, &["fecoergs", "Certel entrada"]);
        expect_breaker(&mut s, &p, 53.0);
        expect_breaker(&mut s, &fecoergs(), 50.0);
        choose(&mut s, &["Certel entrada", "fecoergs"]);
        expect_breaker(&mut s, &p, 50.0);
    }

    #[test]
    fn entrance_above_two_units_survives_save_and_xml() {
        let mut s = Session::new();
        copy(&mut s, "Unidade baixa", "celesc", "unit_demand", Some("FECOERGS"), 2);
        copy(&mut s, "Unidade alta", "celesc", "unit_demand", Some("FECOERGS"), 3);
        copy(&mut s, "Entrada alta", "fecoergs", "entrance", None, 1);
        choose(&mut s, &["Unidade baixa", "Unidade alta", "Entrada alta"]);
        let before = breaker(&mut s, &fecoergs());
        let path = dir().join("review-g3b-entrance-over.xml");
        run(&mut s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": "ReviewEntranceXml"}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"path": path, "set": true}));
        bc(&mut s, json!({"save": "ReviewEntranceSaved"}));
        assert_eq!(
            (before, breaker(&mut s, &fecoergs()), breaker(&mut imported, &fecoergs())),
            (Ok(51.0), Ok(51.0), Ok(51.0)),
            "before, building_code save, XML"
        );
    }

    #[test]
    fn every_group_member_answers_with_and_without_its_library_layer() {
        for (group, system, load, expected) in [("fecoergs", "127/220", 8, 50.0), ("energisa", "220/380", 3, 30.0)] {
            let code = find(D, group).unwrap();
            let mut s = Session::new();
            for order in [&[][..], &[group][..]] {
                choose(&mut s, order);
                for member in code.sections["entrance"]["members"].as_array().unwrap() {
                    let id = member["id"].as_str().unwrap().to_ascii_lowercase();
                    expect_breaker(&mut s, &json!({"utility": id, "installed_kw": load, "system": system, "phases": 1}), expected);
                }
            }
        }
    }

    #[test]
    fn legacy_project_change_to_base_entrance_answers_as_displayed() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs"]);
        bc(&mut s, json!({"save": "ReviewBaseEntrance"}));
        expect_breaker(&mut s, &fecoergs(), 50.0);
        let method = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        let changes = json!({"tables": method["tables"]});
        s.model_mut().codes.choice_mut(D).changes.insert("entrance".into(), changes.as_object().unwrap().clone());
        let path = dir().join("review-g3b-legacy-project-change.bimc");
        run(&mut s, "save", json!({"path": path}));
        let mut opened = Session::new();
        run(&mut opened, "open", json!({"path": path}));

        let shown = bc(&mut opened, json!({}));
        let rules = run(&mut opened, "code_rules", json!({"discipline": "electrical", "section": "entrance"}));
        let tables = rules["sections"][0]["rules"].as_array().unwrap().iter().find(|r| r["rule"] == "tables").unwrap();
        let displayed = &shown["sections"]["entrance"]["tables"][0]["rows"][0]["breaker_a"][0];
        let displayed_rules = &tables["value"][0]["rows"][0]["breaker_a"][0];
        let actual = breaker(&mut opened, &fecoergs());
        assert_eq!((displayed.as_f64(), displayed_rules.as_f64(), actual), (Some(51.0), Some(51.0), Ok(51.0)));
    }

    #[test]
    fn legacy_invalid_entrance_hides_older_method_with_the_same_id() {
        let mut s = Session::new();
        copy(&mut s, "Entrada antiga", "fecoergs", "entrance", None, 1);
        copy(&mut s, "Entrada quebrada", "fecoergs", "entrance", None, 2);
        // This last layer keeps the resolved section valid, so this legacy file can open.
        choose(&mut s, &["Entrada antiga", "Entrada quebrada", "hidropan"]);
        s.model_mut().codes.own.iter_mut().find(|c| c.code == "Entrada quebrada").unwrap().sections.get_mut("entrance").unwrap()["tables"] =
            json!("invalid");
        let path = dir().join("review-g3b-invalid.bimc");
        run(&mut s, "save", json!({"path": path}));
        let mut opened = Session::new();
        run(&mut opened, "open", json!({"path": path}));
        expect_breaker(&mut opened, &json!({"installed_kw": 8, "phases": 1}), 35.0);
        let broken = breaker(&mut opened, &fecoergs());
        bc(&mut opened, json!({"amend": "Entrada quebrada", "reset": "entrance"}));
        let reset = breaker(&mut opened, &fecoergs());
        assert!(broken.is_err() && reset == Ok(51.0), "broken={broken:?}, reset={reset:?}");
    }
}

/// Saving and exporting with methods in several layers and keys, and group members (G3b's confirmation).
mod review_g3br2 {
    use super::*;

    const D: Discipline = Discipline::Electrical;
    const UNIT: &str = "unit_demand";
    const BUILDING: &str = "building_demand";

    fn bc(s: &mut Session, mut p: Value) -> Value {
        p["discipline"] = json!("electrical");
        run(s, "building_code", p)
    }

    fn unit(s: &mut Session, name: &str, utility: &str) {
        let mut method = changed(find(D, "celesc").unwrap(), UNIT, 2);
        method["utility"] = json!(utility);
        bc(s, json!({"set": "nbr-5410", "create": name, "amendments": [name]}));
        bc(s, json!({"amend": name, "rules": {UNIT: method}}));
    }

    fn breaker(s: &mut Session, p: Value, expected: f64) {
        let reply = run(s, "demand", p.clone());
        let n = &reply["category"]["breaker_a"];
        assert_eq!(n.as_f64().or_else(|| n[0].as_f64()), Some(expected), "{p}");
    }

    fn xml(s: &mut Session, name: &str) -> (Session, Value) {
        let path = dir().join(format!("g3br2-{name}.xml"));
        let reply = run(s, "building_code_export", json!({"discipline": "electrical", "path": path, "name": name}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"path": path, "set": true}));
        (imported, reply)
    }

    #[test]
    fn single_utility_stays_one_code_and_answers_52_a() {
        let mut s = Session::new();
        unit(&mut s, "Celesc local", "Celesc");
        let p = json!({"installed_kw": 8, "phases": 1});
        breaker(&mut s, p.clone(), 52.0);
        let (mut imported, reply) = xml(&mut s, "Single exported");
        assert_eq!(reply["codes"], 1);
        breaker(&mut imported, p.clone(), 52.0);
        bc(&mut s, json!({"save": "Single saved"}));
        assert!(s.model.codes.choice(D).amendments.is_empty());
        breaker(&mut s, p, 52.0);
    }

    fn cemig_celesc_answers(s: &mut Session) {
        breaker(s, json!({"installed_kw": 8, "phases": 1}), 52.0);
        for selector in [json!({"utility": "CEMIG"}), json!({"standard": "CEMIG ND-5.2"}), json!({})] {
            let mut p = selector;
            p["dwellings"] = json!([{"count": 20, "area": 120}]);
            assert_eq!(run(s, "building_demand", p)["demand"].as_f64(), Some(88.595));
        }
        let reply = run(s, "code_rules", json!({"discipline": "electrical", "section": BUILDING}));
        let factor = reply["sections"][0]["rules"].as_array().unwrap().iter().find(|r| r["rule"] == "factor").unwrap();
        assert_eq!(factor["value"].as_f64(), Some(2.0));
    }

    fn mixed_keys(name: &str) -> Session {
        let mut s = Session::new();
        unit(&mut s, name, "Celesc");
        bc(&mut s, json!({"amendments": ["cemig", name]}));
        bc(&mut s, json!({"amend": "cemig", "rules": {BUILDING: changed(find(D, "cemig").unwrap(), BUILDING, 1)}}));
        s
    }

    #[test]
    fn cemig_building_below_celesc_unit_survives_a_64_character_package_name() {
        let mut s = mixed_keys("Celesc local");
        cemig_celesc_answers(&mut s);
        let (mut imported, _) = xml(&mut s, &"X".repeat(64));
        cemig_celesc_answers(&mut imported);
        bc(&mut s, json!({"save": "S".repeat(64)}));
        cemig_celesc_answers(&mut s);
        let path = dir().join("g3br2.bimc");
        run(&mut s, "save", json!({"path": path, "compress": true}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        cemig_celesc_answers(&mut reopened);
    }

    #[test]
    fn nec_base_with_celesc_unit_survives_save_and_xml() {
        let mut s = Session::new();
        let name = "Celesc over NEC";
        let method = changed(find(D, "celesc").unwrap(), UNIT, 2);
        bc(&mut s, json!({"set": "nec", "create": name, "amendments": [name]}));
        bc(&mut s, json!({"amend": name, "rules": {UNIT: method}}));
        let check = |s: &mut Session| {
            breaker(s, json!({"installed_kw": 8, "phases": 1}), 52.0);
            for mut p in [json!({"standard": "NEC 220.84"}), json!({})] {
                p["dwellings"] = json!([{"count": 30, "demand": 20}]);
                assert_eq!(run(s, "building_demand", p)["demand"].as_f64(), Some(198.0));
            }
        };
        check(&mut s);
        let (mut imported, _) = xml(&mut s, "NEC exported");
        check(&mut imported);
        bc(&mut s, json!({"save": "NEC saved"}));
        check(&mut s);
    }

    #[test]
    fn energisa_members_follow_group_unit_and_entrance_in_both_orders() {
        let mut s = Session::new();
        unit(&mut s, "Energisa local", "Energisa");
        let group = find(D, "energisa").unwrap();
        let member = group.sections["entrance"]["members"][0]["id"].as_str().unwrap();
        for (order, expected) in [(["energisa", "Energisa local"], 52.0), (["Energisa local", "energisa"], 30.0)] {
            bc(&mut s, json!({"amendments": order}));
            let p = if expected == 52.0 {
                json!({"utility": member, "installed_kw": 8, "phases": 1})
            } else {
                json!({"utility": member, "installed_kw": 3, "system": "220/380", "phases": 1})
            };
            breaker(&mut s, p, expected);
        }
    }
}

/// An older file's change of a method no amendment holds (G2b and its review).
mod legacy_demand_review {
    use super::*;

    const D: Discipline = Discipline::Electrical;

    fn code(s: &mut Session, mut params: Value) -> Value {
        params["discipline"] = json!("electrical");
        run(s, "building_code", params)
    }

    fn choose(s: &mut Session, amendments: &[&str]) {
        code(s, json!({"set": "nbr-5410", "amendments": amendments}));
    }

    fn patch(s: &mut Session, key: &str, rules: Value) {
        s.model_mut().codes.choice_mut(D).changes.insert(key.into(), rules.as_object().unwrap().clone());
    }

    fn reopen(s: &mut Session, name: &str) -> Session {
        let path = dir().join(format!("legacy-demand-review-{name}.bimc"));
        run(s, "save", json!({"path": path}));
        let mut reopened = Session::new();
        run(&mut reopened, "open", json!({"path": path}));
        reopened
    }

    fn rated_breaker(s: &mut Session, params: &Value) -> Result<f64, String> {
        let reply = s.execute("demand", params)?;
        let n = &reply["category"]["breaker_a"];
        n.as_f64().or_else(|| n[0].as_f64()).ok_or_else(|| format!("sem disjuntor: {reply}"))
    }

    fn entrance(utility: &str) -> Value {
        json!({"utility": utility, "installed_kw": 8, "phases": 1})
    }

    fn building_number(s: &mut Session, params: &Value) -> Result<f64, String> {
        let reply = s.execute("building_demand", params)?;
        reply["demand"].as_f64().ok_or_else(|| format!("sem demanda: {reply}"))
    }

    fn amendment(s: &mut Session, name: &str, key: &str, method: Value) {
        code(s, json!({"set": "nbr-5410", "create": name, "amendments": [name]}));
        code(s, json!({"amend": name, "rules": {key: method}}));
    }

    /// The amendment an older file's change of a method went into, the project's own.
    fn project_layer(s: &Session) -> String {
        s.model.codes.own.iter().find(|c| c.code.ends_with("(project)")).map(|c| c.code.clone()).unwrap_or_default()
    }

    #[test]
    fn partial_entrance_over_library_changes_only_its_utility_and_can_reset() {
        let mut s = Session::new();
        let key = "entrance";
        let method = changed(find(D, "fecoergs").unwrap(), key, 1);
        let params = entrance("FECOERGS");
        assert_eq!(rated_breaker(&mut s, &params), Ok(50.0));
        patch(&mut s, key, json!({"tables": method["tables"]}));
        let mut s = reopen(&mut s, "library-tables");
        let changed = rated_breaker(&mut s, &params);
        let cpfl = rated_breaker(&mut s, &json!({"utility": "CPFL", "area": 40}));
        let layer = project_layer(&s);
        code(&mut s, json!({"amend": layer, "reset": key}));
        assert_eq!((changed, cpfl, rated_breaker(&mut s, &params)), (Ok(51.0), Ok(32.0), Ok(50.0)));
    }

    #[test]
    fn partial_entrance_migrates_into_amendment_and_keeps_its_identity() {
        let mut s = Session::new();
        let key = "entrance";
        choose(&mut s, &["energisa", "fecoergs"]);
        let method = changed(find(D, "fecoergs").unwrap(), key, 1);
        patch(&mut s, key, json!({"tables": method["tables"]}));
        let mut s = reopen(&mut s, "amendment-tables");
        let energisa = json!({"utility": "EAC", "system": "220/380", "installed_kw": 3, "phases": 1});
        assert_eq!(rated_breaker(&mut s, &entrance("FECOERGS")), Ok(51.0));
        assert_eq!(rated_breaker(&mut s, &energisa), Ok(30.0));
        code(&mut s, json!({"amend": "fecoergs", "reset": key}));
        assert_eq!(rated_breaker(&mut s, &entrance("FECOERGS")), Ok(50.0));
    }

    #[test]
    fn legacy_building_factor_answers_by_standard_and_resets() {
        let mut s = Session::new();
        let key = "building_demand";
        patch(&mut s, key, changed(find(D, "cemig").unwrap(), key, 1));
        let mut s = reopen(&mut s, "building-standard");
        let p = json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 20, "area": 120}]});
        let ce = json!({"utility": "CELESC", "dwellings": p["dwellings"]});
        let changed = building_number(&mut s, &p);
        let other = building_number(&mut s, &ce);
        let layer = project_layer(&s);
        code(&mut s, json!({"amend": layer, "reset": key}));
        assert_eq!((changed, other, building_number(&mut s, &p)), (Ok(88.595), Ok(53.157), Ok(62.017)));
    }

    #[test]
    fn legacy_partial_entrance_with_another_identity_preserves_energisa_networks() {
        let mut s = Session::new();
        let key = "entrance";
        choose(&mut s, &["fecoergs"]);
        let method = changed(find(D, "energisa").unwrap(), key, 1);
        patch(&mut s, key, json!({"id": "ENERGISA", "tables": method["tables"]}));
        let mut s = reopen(&mut s, "energisa-identity");
        let group = json!({"utility": "ENERGISA", "system": "220/380", "installed_kw": 3, "phases": 1});
        let mut member = group.clone();
        member["utility"] = json!("EAC");
        assert_eq!(
            (rated_breaker(&mut s, &group), rated_breaker(&mut s, &member), rated_breaker(&mut s, &entrance("FECOERGS"))),
            (Ok(31.0), Ok(31.0), Ok(50.0)),
        );
    }

    #[test]
    fn legacy_change_stays_above_older_layer_after_xml_export_import() {
        let mut original = Session::new();
        choose(&mut original, &["fecoergs", "hidropan"]);
        let key = "entrance";
        let method = changed(find(D, "energisa").unwrap(), key, 1);
        patch(&mut original, key, method);
        let mut original = reopen(&mut original, "export");
        let params = json!({"utility": "EAC", "system": "220/380", "installed_kw": 3, "phases": 1});
        let pre = rated_breaker(&mut original, &params);
        let mut implicit = params.clone();
        implicit.as_object_mut().unwrap().remove("utility");
        let pre_implicit = rated_breaker(&mut original, &implicit);
        let path = dir().join("legacy-demand-review-export.xml");
        run(&mut original, "building_code_export", json!({"discipline": "electrical", "name": "Review Legacy Export", "path": path}));
        let mut imported = Session::new();
        run(&mut imported, "building_code_import", json!({"set": true, "path": path}));
        let named = rated_breaker(&mut imported, &params);
        let unnamed = rated_breaker(&mut imported, &implicit);
        assert_eq!((pre, pre_implicit, named, unnamed), (Ok(31.0), Ok(31.0), Ok(31.0), Ok(31.0)));
    }
    #[test]
    fn empty_legacy_patch_leaves_named_method_and_implicit_precedence_unchanged() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs"]);
        // An empty patch stays in Choice::changes when no amendment holds its section.
        patch(&mut s, "unit_demand", json!({}));
        let mut s = reopen(&mut s, "empty-patch");
        let implicit = json!({"installed_kw": 8, "phases": 1});
        assert_eq!(rated_breaker(&mut s, &implicit), Ok(50.0));
        assert_eq!(rated_breaker(&mut s, &json!({"utility": "CPFL", "area": 40})), Ok(32.0));
    }

    #[test]
    fn legacy_unit_demand_beats_entrance_in_the_top_amendment() {
        let mut s = Session::new();
        let mut layer = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        layer["id"] = json!("CPFL");
        layer["members"] = json!([]);
        amendment(&mut s, "Review CPFL Entrance", "entrance", layer);
        let method = changed(find(D, "cpfl").unwrap(), "unit_demand", 1);
        patch(&mut s, "unit_demand", method);
        let mut s = reopen(&mut s, "unit-beats-entrance");
        let named = rated_breaker(&mut s, &json!({"utility": "CPFL", "area": 40}));
        let implicit = rated_breaker(&mut s, &json!({"area": 40}));
        assert_eq!((named, implicit), (Ok(33.0), Ok(33.0)));
    }

    #[test]
    fn legacy_cpfl_change_preserves_celesc_precedence_across_sections() {
        let mut s = Session::new();
        let mut layer = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        layer["id"] = json!("CELESC");
        layer["members"] = json!([]);
        amendment(&mut s, "Review CELESC Entrance", "entrance", layer);
        choose(&mut s, &["Review CELESC Entrance", "celesc"]);
        let key = "unit_demand";
        code(&mut s, json!({"amend": "celesc", "rules": {key: changed(find(D, "celesc").unwrap(), key, 2)}}));
        let params = entrance("CELESC");
        assert_eq!(rated_breaker(&mut s, &params), Ok(52.0));
        patch(&mut s, key, changed(find(D, "cpfl").unwrap(), key, 1));
        let mut s = reopen(&mut s, "unrelated-cpfl-change");
        assert_eq!(rated_breaker(&mut s, &params), Ok(52.0));
    }

    #[test]
    fn partial_entrance_over_project_base_can_reset() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs"]);
        code(&mut s, json!({"save": "Review FECOERGS Base"}));
        let method = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        patch(&mut s, "entrance", json!({"tables": method["tables"]}));
        let mut s = reopen(&mut s, "base-tables");
        let p = entrance("FECOERGS");
        let changed = rated_breaker(&mut s, &p);
        let layer = project_layer(&s);
        code(&mut s, json!({"amend": layer, "reset": "entrance"}));
        assert_eq!((changed, rated_breaker(&mut s, &p)), (Ok(51.0), Ok(50.0)));
    }
    #[test]
    fn empty_legacy_patch_does_not_promote_a_library_method_above_a_held_entrance() {
        let mut s = Session::new();
        choose(&mut s, &["hidropan", "cemig"]);
        let p = json!({"installed_kw": 8, "phases": 1});
        assert_eq!(rated_breaker(&mut s, &p), Ok(35.0));
        patch(&mut s, "unit_demand", json!({}));
        let mut s = reopen(&mut s, "empty-patch-above-entrance");
        assert_eq!(rated_breaker(&mut s, &p), Ok(35.0));
    }

    // The confirmation's cases (G2b): names and room at the edges.
    fn legacy_entrance_patch(s: &mut Session) {
        let method = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        patch(s, "entrance", json!({"tables": method["tables"]}));
    }

    fn legacy_capacity(s: &mut Session, count: usize, hold: bool) {
        for i in 0..count {
            let name = format!("Review capacity {i}");
            code(s, json!({"create": name}));
            if hold {
                s.model_mut().codes.choice_mut(D).amendments.push(name);
            }
        }
    }

    #[test]
    fn review_legacy_full_amendment_list_still_opens_and_answers() {
        let mut s = Session::new();
        legacy_capacity(&mut s, MAX_AMENDMENTS, true);
        legacy_entrance_patch(&mut s);
        s.model.check().expect("the model before migration is valid");
        let opened = reopen(&mut s, "review-full-amendments");
        // No room for another amendment: the change stays among the project's own, as before, and the file opens.
        assert!(opened.model.codes.choice(D).changes.contains_key("entrance"));
    }

    #[test]
    fn review_legacy_exact_name_collision_and_command_lifecycle() {
        let mut s = Session::new();
        amendment(&mut s, "FECOERGS (project)", "building_demand", changed(find(D, "cemig").unwrap(), "building_demand", 1));
        let cemig = json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 20, "area": 120}]});
        let method = changed(find(D, "fecoergs").unwrap(), "entrance", 1);
        patch(&mut s, "entrance", json!({"id": "FECOERGS", "tables": method["tables"]}));
        let p = entrance("FECOERGS");
        let mut opened = reopen(&mut s, "review-collision-lifecycle");
        assert_eq!(rated_breaker(&mut opened, &p), Ok(51.0));
        assert_eq!(building_number(&mut opened, &cemig), Ok(88.595));
        let layer = opened.model.codes.layers(D).last().unwrap().code.clone();
        let edited = changed(find(D, "fecoergs").unwrap(), "entrance", 2);
        code(&mut opened, json!({"amend": layer, "rules": {"entrance": {"tables": edited["tables"]}}}));
        assert_eq!(rated_breaker(&mut opened, &p), Ok(52.0));
        assert!(opened.execute("building_code", &json!({"discipline": "electrical", "delete": layer})).is_err());
        assert_eq!(rated_breaker(&mut opened, &p), Ok(52.0));
        code(&mut opened, json!({"amend": layer, "reset": "entrance"}));
        assert_eq!(rated_breaker(&mut opened, &p), Ok(50.0));
        code(&mut opened, json!({"delete": layer}));
        assert_eq!(rated_breaker(&mut opened, &p), Ok(50.0));
        assert_eq!(building_number(&mut opened, &cemig), Ok(88.595));
    }

    #[test]
    fn review_legacy_three_changes_keep_each_number() {
        let mut s = Session::new();
        for (library, key) in [("cpfl", "unit_demand"), ("cemig", "building_demand"), ("energisa", "entrance")] {
            patch(&mut s, key, changed(find(D, library).unwrap(), key, 1));
        }
        let mut opened = reopen(&mut s, "review-three");
        assert_eq!(rated_breaker(&mut opened, &json!({"utility": "CPFL", "area": 40})), Ok(33.0));
        assert_eq!(
            building_number(&mut opened, &json!({"standard": "CEMIG ND-5.2", "dwellings": [{"count": 20, "area": 120}]})),
            Ok(88.595)
        );
        assert_eq!(rated_breaker(&mut opened, &json!({"utility": "EAC", "system": "220/380", "installed_kw": 3, "phases": 1})), Ok(31.0));
    }

    #[test]
    fn review_legacy_two_keys_with_same_utility_name_keep_both_answers() {
        let mut s = Session::new();
        patch(&mut s, "unit_demand", changed(find(D, "cpfl").unwrap(), "unit_demand", 1));
        let mut building = changed(find(D, "cemig").unwrap(), "building_demand", 1);
        building["standard"] = json!("CPFL");
        building["utilities"] = json!(["CPFL"]);
        patch(&mut s, "building_demand", building);
        let mut opened = reopen(&mut s, "review-two-keys");
        assert_eq!(rated_breaker(&mut opened, &json!({"utility": "CPFL", "area": 40})), Ok(33.0));
        assert_eq!(building_number(&mut opened, &json!({"utility": "CPFL", "dwellings": [{"count": 20, "area": 120}]})), Ok(88.595));
    }

    #[test]
    fn review_legacy_library_code_stem_preserves_library_answers() {
        let mut s = Session::new();
        let mut method = changed(find(D, "energisa").unwrap(), "entrance", 1);
        // This identity is also a reserved library code identifier.
        method["id"] = json!("fecoergs");
        patch(&mut s, "entrance", method);
        let mut opened = reopen(&mut s, "review-reserved-stem");
        let p = json!({"system": "220/380", "installed_kw": 3, "phases": 1});
        assert_eq!(rated_breaker(&mut opened, &p), Ok(31.0));
        let mut clean = Session::new();
        choose(&mut clean, &["fecoergs"]);
        assert_eq!(rated_breaker(&mut clean, &entrance("FECOERGS")), Ok(50.0));
    }

    #[test]
    fn review_legacy_building_code_reports_the_same_effective_rules_after_open() {
        let mut s = Session::new();
        legacy_entrance_patch(&mut s);
        let before = code(&mut s, json!({}));
        let mut opened = reopen(&mut s, "review-building-code");
        let after = code(&mut opened, json!({}));
        assert_eq!(before["sections"]["entrance"], after["sections"]["entrance"]);
        assert_eq!(rated_breaker(&mut opened, &entrance("FECOERGS")), Ok(51.0));
        let mut reopened = reopen(&mut opened, "review-building-code-second");
        assert_eq!(after, code(&mut reopened, json!({})));
        assert_eq!(rated_breaker(&mut reopened, &entrance("FECOERGS")), Ok(51.0));
    }

    #[test]
    fn review_legacy_63_character_base_still_opens_and_answers() {
        let mut s = Session::new();
        choose(&mut s, &["fecoergs"]);
        let base = "R".repeat(63);
        code(&mut s, json!({"save": base}));
        legacy_entrance_patch(&mut s);
        s.model.check().expect("the model before migration is valid");
        let mut opened = reopen(&mut s, "review-63");
        assert_eq!(rated_breaker(&mut opened, &entrance("FECOERGS")), Ok(51.0));
    }

    #[test]
    fn review_legacy_64_character_base_and_collision_keep_the_project_answer() {
        for (tag, collision) in [("free", false), ("taken", true)] {
            let mut s = Session::new();
            choose(&mut s, &["fecoergs"]);
            let base = "R".repeat(64);
            if collision {
                code(&mut s, json!({"save": base}));
            } else {
                // A 64-character display name and a shorter identifier are valid in a file.
                code(&mut s, json!({"save": "Review long display"}));
                s.model_mut().codes.own.iter_mut().find(|c| c.code == "Review long display").unwrap().name = base;
            }
            legacy_entrance_patch(&mut s);
            let mut opened = reopen(&mut s, &format!("review-long-{tag}"));
            assert_eq!(rated_breaker(&mut opened, &entrance("FECOERGS")), Ok(51.0), "{tag}");
        }
    }

    // A method that can't be read, over the one that would answer (the reviews of G2b and G3b).

    #[test]
    fn unreadable_entrance_of_energisa_local_member_refuses_to_use_older_table() {
        let mut s = Session::new();
        let key = "entrance";
        let mut older = changed(find(D, "energisa").unwrap(), key, 1);
        older["id"] = json!("Local Review");
        older["members"] = json!([]);
        let mut broken = changed(find(D, "energisa").unwrap(), key, 2);
        broken["members"].as_array_mut().unwrap().push(json!({"id": "Local Review", "seat": "Local"}));
        amendment(&mut s, "Review Older Group", key, older);
        amendment(&mut s, "Review Broken Group", key, broken);
        choose(&mut s, &["Review Older Group", "energisa", "Review Broken Group", "fecoergs"]);
        damage(&mut s, "Review Broken Group", key, "tables");
        let mut s = reopen(&mut s, "unreadable-local-member");
        let local = json!({"utility": "Local Review", "system": "220/380", "installed_kw": 3, "phases": 1});
        let eac = json!({"utility": "EAC", "system": "220/380", "installed_kw": 3, "phases": 1});
        assert_eq!(rated_breaker(&mut s, &entrance("FECOERGS")), Ok(50.0));
        let local_reply = rated_breaker(&mut s, &local);
        let eac_reply = rated_breaker(&mut s, &eac);
        assert!(
            matches!(&local_reply, Err(e) if e.contains("Review Broken Group"))
                && matches!(&eac_reply, Err(e) if e.contains("Review Broken Group")),
            "Local Review: {local_reply:?}, EAC: {eac_reply:?}",
        );
    }

    #[test]
    fn unreadable_entrance_below_answering_layer_does_not_block() {
        let mut s = Session::new();
        let key = "entrance";
        let base = find(D, "fecoergs").unwrap();
        amendment(&mut s, "Review Broken Lower", key, changed(base, key, 1));
        amendment(&mut s, "Review Readable Upper", key, changed(base, key, 2));
        choose(&mut s, &["Review Broken Lower", "Review Readable Upper"]);
        damage(&mut s, "Review Broken Lower", key, "tables");
        let mut s = reopen(&mut s, "unreadable-below");
        assert_eq!(rated_breaker(&mut s, &entrance("FECOERGS")), Ok(52.0));
    }

    #[test]
    fn unreadable_building_method_blocks_lower_number_by_standard_and_utility() {
        let mut s = Session::new();
        let key = "building_demand";
        let cemig = find(D, "cemig").unwrap();
        amendment(&mut s, "Review Broken Building", key, changed(cemig, key, 1));
        choose(&mut s, &["cemig", "Review Broken Building", "celesc"]);
        damage(&mut s, "Review Broken Building", key, "factor");
        let mut s = reopen(&mut s, "unreadable-building");
        let groups = json!([{"count": 20, "area": 120}]);
        let by_standard = building_number(&mut s, &json!({"standard": "CEMIG ND-5.2", "dwellings": groups}));
        let by_utility = building_number(&mut s, &json!({"utility": "CEMIG", "dwellings": groups}));
        assert_eq!(building_number(&mut s, &json!({"utility": "CELESC", "dwellings": groups})), Ok(53.157));
        assert!(
            matches!(&by_standard, Err(e) if e.contains("Review Broken Building"))
                && matches!(&by_utility, Err(e) if e.contains("Review Broken Building")),
            "standard: {by_standard:?}, utility: {by_utility:?}",
        );
    }

    #[test]
    fn unreadable_energisa_group_without_eac_does_not_block_eac() {
        let mut s = Session::new();
        let key = "entrance";
        let mut broken = changed(find(D, "energisa").unwrap(), key, 2);
        broken["members"].as_array_mut().unwrap().retain(|m| m["id"] != "EAC");
        amendment(&mut s, "Review Different Group", key, broken);
        choose(&mut s, &["energisa", "Review Different Group", "fecoergs"]);
        damage(&mut s, "Review Different Group", key, "tables");
        let mut s = reopen(&mut s, "unreadable-excludes-eac");
        let p = json!({"utility": "EAC", "system": "220/380", "installed_kw": 3, "phases": 1});
        assert_eq!(rated_breaker(&mut s, &p), Ok(30.0));
    }

    #[test]
    fn unreadable_entrance_with_corrupted_id_still_identifies_its_library_copy() {
        let mut s = Session::new();
        let key = "entrance";
        let fecoergs = find(D, "fecoergs").unwrap();
        amendment(&mut s, "Review Older FECOERGS", key, changed(fecoergs, key, 2));
        choose(&mut s, &["Review Older FECOERGS", "fecoergs", "hidropan"]);
        code(&mut s, json!({"amend": "fecoergs", "rules": {key: changed(fecoergs, key, 1)}}));
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "fecoergs").unwrap();
        own.sections.get_mut(key).unwrap()["id"] = json!(17);
        let mut s = reopen(&mut s, "unreadable-id");
        let reply = rated_breaker(&mut s, &entrance("FECOERGS"));
        assert!(matches!(&reply, Err(e) if e.contains("fecoergs")), "{reply:?}");
    }

    fn damage(s: &mut Session, name: &str, key: &str, field: &str) {
        let layer = s.model_mut().codes.own.iter_mut().find(|c| c.code == name).unwrap();
        layer.sections.get_mut(key).unwrap()[field] = json!("ilegível");
    }
}

mod unreadable_method_contract_review {
    use super::*;

    const D: Discipline = Discipline::Electrical;

    fn codes(s: &mut Session, mut p: Value) -> Value {
        p["discipline"] = json!("electrical");
        run(s, "building_code", p)
    }

    fn select(s: &mut Session, layers: &[&str]) {
        codes(s, json!({"set": "nbr-5410", "amendments": layers}));
    }

    fn add(s: &mut Session, name: &str, key: &str, method: Value) {
        codes(s, json!({"create": name, "set": "nbr-5410", "amendments": [name]}));
        codes(s, json!({"amend": name, "rules": {key: method}}));
    }

    fn replace(s: &mut Session, name: &str, key: &str, field: &str, value: Value) {
        let layer = s.model_mut().codes.own.iter_mut().find(|c| c.code == name).unwrap();
        layer.sections.get_mut(key).unwrap()[field] = value;
    }

    fn legacy_file(s: &mut Session, name: &str) -> std::path::PathBuf {
        let path = dir().join(format!("unreadable-contract-{name}.bimc"));
        run(s, "save", json!({"path": path}));
        path
    }

    fn reopen(s: &mut Session, name: &str) -> Session {
        let path = legacy_file(s, name);
        let mut opened = Session::new();
        run(&mut opened, "open", json!({"path": path}));
        opened
    }

    fn supply() -> Value {
        json!({"installed_kw": 8, "phases": 1})
    }

    fn dwellings() -> Value {
        json!({"dwellings": [{"count": 20, "area": 120}]})
    }

    fn breaker(s: &mut Session, p: &Value) -> f64 {
        let result = run(s, "demand", p.clone());
        let n = &result["category"]["breaker_a"];
        n.as_f64().or_else(|| n[0].as_f64()).unwrap_or_else(|| panic!("missing breaker: {result}"))
    }

    fn building(s: &mut Session, p: &Value) -> f64 {
        run(s, "building_demand", p.clone())["demand"].as_f64().unwrap()
    }

    fn refuses(s: &mut Session, command: &str, p: &Value, layer: &str) {
        let result = s.execute(command, p);
        match result {
            Err(e) => assert!(e.contains(layer), "expected refusal naming {layer}: {e}"),
            Ok(reply) => panic!("expected refusal naming {layer}: demand={}, breaker={}", reply["demand"], reply["category"]["breaker_a"]),
        }
    }

    #[test]
    fn unnamed_demand_keeps_readable_winner_when_a_different_identity_is_broken() {
        let mut s = Session::new();
        let fecoergs = find(D, "fecoergs").unwrap();
        add(&mut s, "Broken implicit FECOERGS", "entrance", changed(fecoergs, "entrance", 1));
        // CELESC holds the readable method in the highest layer. Its method has a different identity.
        // The visible entrance stays valid for a legacy file, and CELESC is now above it for the implicit request.
        select(&mut s, &["fecoergs", "Broken implicit FECOERGS", "hidropan", "celesc"]);
        replace(&mut s, "Broken implicit FECOERGS", "entrance", "tables", json!("bad"));
        let mut s = reopen(&mut s, "implicit-other");
        assert_eq!(breaker(&mut s, &supply()), 50.0);
    }

    #[test]
    fn unnamed_demand_refuses_if_the_readable_default_has_a_broken_newer_method() {
        let mut s = Session::new();
        let celesc = find(D, "celesc").unwrap();
        let mut other = changed(find(D, "fecoergs").unwrap(), "entrance", 2);
        other["id"] = json!("CELESC");
        add(&mut s, "Broken implicit CELESC", "entrance", other);
        add(&mut s, "Readable default CELESC", "unit_demand", changed(celesc, "unit_demand", 1));
        // Exercise the lookup before the file validator, which is covered by a separate test.
        select(&mut s, &["hidropan", "Readable default CELESC", "Broken implicit CELESC"]);
        replace(&mut s, "Broken implicit CELESC", "entrance", "tables", json!("bad"));
        refuses(&mut s, "demand", &supply(), "Broken implicit CELESC");
    }

    #[test]
    fn nameless_broken_amendment_does_not_block_named_utility() {
        let mut s = Session::new();
        let key = "entrance";
        let fecoergs = find(D, "fecoergs").unwrap();
        add(&mut s, "Unattributed entrance", key, changed(fecoergs, key, 1));
        select(&mut s, &["fecoergs", "Unattributed entrance", "hidropan"]);
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "Unattributed entrance").unwrap();
        let rules = own.sections.get_mut(key).unwrap().as_object_mut().unwrap();
        for field in ["id", "utility", "standard", "utilities", "members"] {
            rules.remove(field);
        }
        let mut s = reopen(&mut s, "unattributed");
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 50.0);
    }

    #[test]
    fn broken_unit_above_entrance_refuses_for_the_same_utility() {
        let mut s = Session::new();
        let mut unit = changed(find(D, "celesc").unwrap(), "unit_demand", 1);
        unit["utility"] = json!("FeCoErGs");
        add(&mut s, "Broken FECOERGS unit", "unit_demand", unit);
        select(&mut s, &["fecoergs", "Broken FECOERGS unit", "celesc"]);
        replace(&mut s, "Broken FECOERGS unit", "unit_demand", "method", json!("bad"));
        let mut s = reopen(&mut s, "unit-above-entrance");
        let mut p = supply();
        p["utility"] = json!("fEcOeRgS");
        refuses(&mut s, "demand", &p, "Broken FECOERGS unit");
    }

    #[test]
    fn broken_entrance_above_unit_refuses_for_the_same_utility() {
        let mut s = Session::new();
        let key = "entrance";
        let mut entrance = changed(find(D, "fecoergs").unwrap(), key, 1);
        entrance["id"] = json!("CELESC");
        add(&mut s, "Broken CELESC entrance", key, entrance);
        select(&mut s, &["celesc", "Broken CELESC entrance", "hidropan"]);
        replace(&mut s, "Broken CELESC entrance", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "entrance-above-unit");
        let mut p = supply();
        p["utility"] = json!("CELESC");
        refuses(&mut s, "demand", &p, "Broken CELESC entrance");
    }

    #[test]
    fn member_listed_only_in_the_broken_entrance_gets_a_named_refusal() {
        let mut s = Session::new();
        let key = "entrance";
        let mut method = changed(find(D, "energisa").unwrap(), key, 1);
        method["members"].as_array_mut().unwrap().push(json!({"id": "Local Review Only", "seat": "Local"}));
        add(&mut s, "Broken private member entrance", key, method);
        select(&mut s, &["energisa", "Broken private member entrance", "hidropan"]);
        replace(&mut s, "Broken private member entrance", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "only-member");
        let p = json!({"utility": "local review only", "system": "220/380", "installed_kw": 3, "phases": 1});
        refuses(&mut s, "demand", &p, "Broken private member entrance");
    }

    #[test]
    fn broken_group_unit_blocks_older_entrance_for_its_member() {
        let mut s = Session::new();
        let mut unit = changed(find(D, "celesc").unwrap(), "unit_demand", 1);
        unit["utility"] = json!("ENERGISA");
        add(&mut s, "Broken ENERGISA unit", "unit_demand", unit);
        select(&mut s, &["energisa", "Broken ENERGISA unit", "celesc"]);
        let p = json!({"utility": "eAc", "installed_kw": 3, "phases": 1});
        assert_eq!(breaker(&mut s, &p), 51.0);
        replace(&mut s, "Broken ENERGISA unit", "unit_demand", "method", json!("bad"));
        let mut s = reopen(&mut s, "broken-group-unit");
        refuses(&mut s, "demand", &p, "Broken ENERGISA unit");
    }

    #[test]
    fn a_matching_standard_name_cannot_hide_a_broken_utility_method() {
        let mut s = Session::new();
        let key = "building_demand";
        let cemig = find(D, "cemig").unwrap();
        add(&mut s, "Broken CEMIG building", key, changed(cemig, key, 1));
        let mut other = changed(cemig, key, 2);
        other["standard"] = json!("CEMIG");
        other["utilities"] = json!(["Independent building utility"]);
        add(&mut s, "Unrelated building standard", key, other);
        select(&mut s, &["cemig", "Broken CEMIG building", "Unrelated building standard"]);
        let mut p = dwellings();
        p["utility"] = json!("cEmIg");
        assert_eq!(building(&mut s, &p), 88.595);
        replace(&mut s, "Broken CEMIG building", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "standard-shields-utility");
        refuses(&mut s, "building_demand", &p, "Broken CEMIG building");
    }

    #[test]
    fn an_unrelated_broken_standard_named_like_a_utility_does_not_block_it() {
        let mut s = Session::new();
        let key = "building_demand";
        let mut other = changed(find(D, "cemig").unwrap(), key, 1);
        other["standard"] = json!("CEMIG");
        other["utilities"] = json!(["Independent building utility"]);
        add(&mut s, "Broken unrelated standard", key, other);
        select(&mut s, &["cemig", "Broken unrelated standard", "celesc"]);
        replace(&mut s, "Broken unrelated standard", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "unrelated-standard");
        let mut p = dwellings();
        p["utility"] = json!("CEMIG");
        assert_eq!(building(&mut s, &dwellings()), 53.157);
        assert_eq!(building(&mut s, &p), 62.017);
    }

    #[test]
    fn case_variant_that_cannot_answer_by_standard_cannot_hide_a_broken_method() {
        let mut s = Session::new();
        let key = "building_demand";
        let cemig = find(D, "cemig").unwrap();
        add(&mut s, "Broken exact standard", key, changed(cemig, key, 1));
        let mut other = changed(cemig, key, 2);
        other["standard"] = json!("cemig nd-5.2");
        add(&mut s, "Different case standard", key, other);
        select(&mut s, &["cemig", "Broken exact standard", "Different case standard"]);
        let mut p = dwellings();
        p["standard"] = json!("CEMIG ND-5.2");
        assert_eq!(building(&mut s, &p), 88.595);
        replace(&mut s, "Broken exact standard", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "standard-case");
        refuses(&mut s, "building_demand", &p, "Broken exact standard");
    }

    #[test]
    fn resetting_the_named_section_recovers_the_older_answer() {
        let mut s = Session::new();
        let key = "entrance";
        add(&mut s, "Broken reset target", key, changed(find(D, "fecoergs").unwrap(), key, 1));
        select(&mut s, &["fecoergs", "Broken reset target", "hidropan"]);
        replace(&mut s, "Broken reset target", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "reset");
        run(&mut s, "level", json!({"name": "Independent review level", "elevation": 3000}));
        codes(&mut s, json!({"amend": "Broken reset target", "reset": key}));
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 50.0);
    }
    #[test]
    fn whole_non_object_section_of_library_copy_refuses_instead_of_using_the_older_entrance() {
        let mut s = Session::new();
        let key = "entrance";
        let method = changed(find(D, "fecoergs").unwrap(), key, 1);
        add(&mut s, "Older scalar test", key, method);
        select(&mut s, &["Older scalar test", "fecoergs", "hidropan"]);
        codes(&mut s, json!({"amend": "fecoergs", "rules": {key: changed(find(D, "fecoergs").unwrap(), key, 2)}}));
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 52.0);
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "fecoergs").unwrap();
        own.sections.insert(key.into(), json!("old representation"));
        let mut s = reopen(&mut s, "scalar-library-copy");
        refuses(&mut s, "demand", &p, "fecoergs");
    }

    #[test]
    fn an_entrance_standard_is_not_the_name_of_a_different_utility() {
        let mut s = Session::new();
        let key = "entrance";
        let mut other = changed(find(D, "hidropan").unwrap(), key, 1);
        other["standard"] = json!("FECOERGS");
        add(&mut s, "Unrelated entrance standard", key, other);
        select(&mut s, &["fecoergs", "Unrelated entrance standard", "hidropan"]);
        replace(&mut s, "Unrelated entrance standard", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "unrelated-entrance-standard");
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 50.0);
    }

    #[test]
    fn named_demand_uses_library_when_a_different_utility_is_broken() {
        let mut s = Session::new();
        let key = "entrance";
        add(&mut s, "Broken HIDROPAN for library check", key, changed(find(D, "hidropan").unwrap(), key, 1));
        select(&mut s, &["Broken HIDROPAN for library check", "hidropan"]);
        replace(&mut s, "Broken HIDROPAN for library check", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "library-unrelated");
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 50.0);
    }

    #[test]
    fn demand_names_the_highest_broken_method_for_the_requested_utility() {
        let mut s = Session::new();
        let fecoergs = find(D, "fecoergs").unwrap();
        add(&mut s, "Lower broken entrance", "entrance", changed(fecoergs, "entrance", 1));
        let mut unit = changed(find(D, "celesc").unwrap(), "unit_demand", 2);
        unit["utility"] = json!("FECOERGS");
        add(&mut s, "Highest broken unit", "unit_demand", unit);
        select(&mut s, &["fecoergs", "Lower broken entrance", "Highest broken unit", "hidropan", "celesc"]);
        replace(&mut s, "Lower broken entrance", "entrance", "tables", json!("bad"));
        replace(&mut s, "Highest broken unit", "unit_demand", "method", json!("bad"));
        let mut s = reopen(&mut s, "two-broken");
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        refuses(&mut s, "demand", &p, "Highest broken unit");
    }
    #[test]
    fn an_explicit_identity_on_a_copy_keeps_the_original_utility_independent() {
        let mut s = Session::new();
        let key = "entrance";
        let library = find(D, "fecoergs").unwrap();
        add(&mut s, "Older original identity", key, changed(library, key, 1));
        select(&mut s, &["Older original identity", "fecoergs", "hidropan"]);
        let mut renamed = changed(library, key, 2);
        renamed["id"] = json!("Independent supply utility");
        renamed["members"] = json!([]);
        codes(&mut s, json!({"amend": "fecoergs", "rules": {key: renamed}}));
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        assert_eq!(breaker(&mut s, &p), 51.0);
        replace(&mut s, "fecoergs", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "copy-with-own-identity");
        assert_eq!(breaker(&mut s, &p), 51.0);
    }

    #[test]
    fn confirmation_rejects_both_building_selectors_before_reading_broken_method() {
        let mut s = Session::new();
        let key = "building_demand";
        let cemig = find(D, "cemig").unwrap();
        add(&mut s, "Broken selector check", key, changed(cemig, key, 1));
        select(&mut s, &["cemig", "Broken selector check", "celesc"]);
        replace(&mut s, "Broken selector check", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "r2-both-selectors");
        let mut p = dwellings();
        p["standard"] = json!("CEMIG ND-5.2");
        refuses(&mut s, "building_demand", &p, "Broken selector check");
        p["utility"] = json!("CEMIG");
        refuses(&mut s, "building_demand", &p, "give `standard` or `utility`, not both");
    }

    #[test]
    fn confirmation_numeric_id_recovers_library_identity() {
        let mut s = Session::new();
        let key = "entrance";
        let library = find(D, "fecoergs").unwrap();
        add(&mut s, "Older numeric id check", key, changed(library, key, 1));
        select(&mut s, &["Older numeric id check", "fecoergs", "hidropan"]);
        codes(&mut s, json!({"amend": "fecoergs", "rules": {key: changed(library, key, 2)}}));
        let mut p = supply();
        p["utility"] = json!("fEcOeRgS");
        assert_eq!(breaker(&mut s, &p), 52.0);
        replace(&mut s, "fecoergs", key, "id", json!(123));
        let mut s = reopen(&mut s, "r2-numeric-id");
        refuses(&mut s, "demand", &p, "fecoergs");
    }

    #[test]
    fn confirmation_names_members_from_objects_and_legacy_strings() {
        let mut s = Session::new();
        let key = "entrance";
        let energisa = find(D, "energisa").unwrap();
        add(&mut s, "Broken member shapes", key, changed(energisa, key, 1));
        select(&mut s, &["energisa", "Broken member shapes", "hidropan"]);
        let p = json!({"utility": "eAc", "installed_kw": 3, "phases": 1});
        let original = breaker(&mut s, &p);
        assert!(original > 0.0);
        replace(&mut s, "Broken member shapes", key, "tables", json!("bad"));
        for (file, member) in [("r2-member-object", json!({"id": "EAC", "seat": "Local"})), ("r2-member-string", json!("EAC"))] {
            replace(&mut s, "Broken member shapes", key, "members", json!([member]));
            let mut opened = reopen(&mut s, file);
            refuses(&mut opened, "demand", &p, "Broken member shapes");
        }
    }

    #[test]
    fn confirmation_broken_unit_matches_a_group_known_only_to_the_library() {
        let mut s = Session::new();
        let mut unit = changed(find(D, "celesc").unwrap(), "unit_demand", 1);
        unit["utility"] = json!("EnErGiSa");
        add(&mut s, "Broken library-only group unit", "unit_demand", unit);
        select(&mut s, &["Broken library-only group unit", "celesc"]);
        let p = json!({"utility": "eAc", "installed_kw": 3, "phases": 1});
        assert_eq!(breaker(&mut s, &p), 51.0);
        replace(&mut s, "Broken library-only group unit", "unit_demand", "method", json!("bad"));
        let mut s = reopen(&mut s, "r2-library-group");
        refuses(&mut s, "demand", &p, "Broken library-only group unit");
    }

    #[test]
    fn confirmation_building_utility_uses_the_highest_answering_layer() {
        let mut s = Session::new();
        let key = "building_demand";
        let cemig = find(D, "cemig").unwrap();
        add(&mut s, "Lower shared utility", key, changed(cemig, key, 1));
        let mut upper = changed(cemig, key, 2);
        upper["standard"] = json!("Second method for CEMIG");
        upper["utilities"] = json!(["CeMiG"]);
        add(&mut s, "Upper shared utility", key, upper);
        select(&mut s, &["cemig", "Lower shared utility", "Upper shared utility", "celesc"]);
        let mut p = dwellings();
        p["utility"] = json!("cEmIg");
        assert_eq!(building(&mut s, &p), 132.893);
        replace(&mut s, "Lower shared utility", key, "factor", json!("bad"));
        let mut opened = reopen(&mut s, "r2-shared-lower-broken");
        assert_eq!(building(&mut opened, &p), 132.893);
        replace(&mut s, "Upper shared utility", key, "factor", json!("bad"));
        let mut opened = reopen(&mut s, "r2-shared-upper-broken");
        refuses(&mut opened, "building_demand", &p, "Upper shared utility");
    }

    #[test]
    fn confirmation_objects_in_utilities_do_not_name_a_building_utility() {
        let mut s = Session::new();
        let key = "building_demand";
        let celesc = find(D, "celesc").unwrap();
        add(&mut s, "Broken object in utilities", key, changed(celesc, key, 1));
        select(&mut s, &["cemig", "Broken object in utilities", "celesc"]);
        let mut p = dwellings();
        p["utility"] = json!("CEMIG");
        assert_eq!(building(&mut s, &p), 62.017);
        replace(&mut s, "Broken object in utilities", key, "utilities", json!([{"id": "CEMIG"}]));
        let mut s = reopen(&mut s, "r2-object-utilities");
        assert_eq!(building(&mut s, &p), 62.017);
    }
    // A file whose method on top can't be read opens (the method is refused when read, and can be reset); anything else
    // that can't be, and an edit that would make a method unreadable, are refused as before.
    #[test]
    fn a_legacy_file_with_a_broken_top_building_method_still_opens() {
        let mut s = Session::new();
        let key = "building_demand";
        add(&mut s, "Broken default building", key, changed(find(D, "cemig").unwrap(), key, 1));
        select(&mut s, &["cemig", "Broken default building"]);
        replace(&mut s, "Broken default building", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "top-building");
        refuses(&mut s, "building_demand", &dwellings(), "Broken default building");
        // Other edits go on with it held; its reset makes it answer again.
        codes(&mut s, json!({"rules": {"voltages": {"60 V": {"nominal": 60, "min": 54, "max": 66}}}}));
        codes(&mut s, json!({"amend": "Broken default building", "reset": key}));
        assert!(building(&mut s, &dwellings()) > 0.0);
    }

    #[test]
    fn a_broken_entrance_on_top_opens_and_names_itself() {
        let mut s = Session::new();
        let fecoergs = find(D, "fecoergs").unwrap();
        add(&mut s, "Broken top entrance", "entrance", changed(fecoergs, "entrance", 1));
        select(&mut s, &["fecoergs", "Broken top entrance"]);
        replace(&mut s, "Broken top entrance", "entrance", "tables", json!("bad"));
        let mut s = reopen(&mut s, "top-entrance");
        let mut p = supply();
        p["utility"] = json!("FECOERGS");
        refuses(&mut s, "demand", &p, "Broken top entrance");
        // Naming none, the utility on top is the broken one's: said too, not the one under it.
        refuses(&mut s, "demand", &supply(), "Broken top entrance");
    }

    #[test]
    fn an_edit_that_breaks_a_method_is_still_refused() {
        let mut s = Session::new();
        let key = "building_demand";
        add(&mut s, "Edited building", key, changed(find(D, "cemig").unwrap(), key, 1));
        select(&mut s, &["cemig", "Edited building"]);
        let e =
            s.execute("building_code", &json!({"discipline": "electrical", "amend": "Edited building", "rules": {key: {"factor": "bad"}}}));
        assert!(e.is_err(), "{e:?}");
    }

    #[test]
    fn a_file_with_another_section_that_cannot_be_still_does_not_open() {
        // Only a method may be unreadable: a section of rules out of range keeps the file out.
        let mut s = Session::new();
        codes(&mut s, json!({"create": "Broken circuits", "set": "nbr-5410", "amendments": ["Broken circuits"]}));
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "Broken circuits").unwrap();
        own.sections.insert("circuits".into(), json!({"systems": "many"}));
        let path = legacy_file(&mut s, "other-section");
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
        // In a base of the project's own too.
        let mut s = Session::new();
        codes(&mut s, json!({"save": "Own base"}));
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "Own base").unwrap();
        own.sections.insert("circuits".into(), json!({"systems": "many"}));
        let path = legacy_file(&mut s, "own-base-section");
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
    }

    // From the review of the open path.
    // The legacy fixtures deliberately put values on disk that current edit commands refuse.
    fn review_broken_cemig() -> Session {
        let mut s = Session::new();
        let key = "building_demand";
        add(&mut s, "Broken review CEMIG", key, changed(find(D, "cemig").unwrap(), key, 1));
        select(&mut s, &["cemig", "Broken review CEMIG"]);
        replace(&mut s, "Broken review CEMIG", key, "factor", json!("bad"));
        s
    }

    #[test]
    fn open_review_unit_numbers_that_deserialize_still_fail_validation() {
        let mut s = Session::new();
        select(&mut s, &["enel"]);
        codes(&mut s, json!({"amend": "enel", "rules": {"unit_demand": changed(find(D, "enel").unwrap(), "unit_demand", 1)}}));
        let own = s.model_mut().codes.own.iter_mut().find(|c| c.code == "enel").unwrap();
        let rows = own.sections.get_mut("unit_demand").unwrap()["method"]["annexes"]["networks"].as_array_mut().unwrap();
        for network in rows {
            for row in network["rows"].as_array_mut().unwrap() {
                row["breaker"] = json!(-1);
            }
        }
        let path = legacy_file(&mut s, "open-review-numbers");
        let mut opened = Session::new();
        if opened.execute("open", &json!({"path": path})).is_ok() {
            let answer = opened.execute("demand", &json!({"utility": "ENEL", "network": "aerial", "installed_kw": 8, "phases": 1}));
            panic!("a typed negative breaker must still keep the file out; demand returned {answer:?}");
        }
    }

    #[test]
    fn open_review_save_as_cannot_copy_a_broken_method() {
        let mut s = review_broken_cemig();
        let mut s = reopen(&mut s, "open-review-save-as");
        let before = s.model.codes.clone();
        let r = s.execute("building_code", &json!({"discipline": "electrical", "save": "Broken copy must fail"}));
        assert!(r.is_err(), "save as moved a broken method to new layers");
        assert_eq!(s.model.codes, before);
    }

    #[test]
    fn open_review_native_save_edits_delete_and_reset() {
        let mut s = review_broken_cemig();
        let mut s = reopen(&mut s, "open-review-native-save");
        let before = s.model.codes.clone();
        run(&mut s, "level", json!({"name": "Review Level", "elevation": 3000}));
        codes(&mut s, json!({"create": "Unrelated empty amendment"}));
        let mut s = reopen(&mut s, "open-review-native-save-again");
        assert_eq!(s.model.codes.find(D, "Broken review CEMIG"), before.find(D, "Broken review CEMIG"));
        refuses(&mut s, "building_demand", &dwellings(), "Broken review CEMIG");
        assert!(s.execute("building_code", &json!({"discipline": "electrical", "delete": "Broken review CEMIG"})).is_err());
        codes(&mut s, json!({"amend": "Broken review CEMIG", "reset": "building_demand"}));
        assert_eq!(building(&mut s, &dwellings()), 62.017);
        codes(&mut s, json!({"delete": "Broken review CEMIG"}));
    }

    #[test]
    fn open_review_global_value_cap_and_other_model_checks_still_apply() {
        let mut s = review_broken_cemig();
        replace(&mut s, "Broken review CEMIG", "building_demand", "factor", json!(vec![0; MAX_VALUES]));
        let path = legacy_file(&mut s, "open-review-value-cap");
        refuses(&mut Session::new(), "open", &json!({"path": path}), "400000");
        let mut s = review_broken_cemig();
        s.model_mut().codes.site.rain_intensity = Some(f64::MAX);
        let path = legacy_file(&mut s, "open-review-site");
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
    }

    #[test]
    fn open_review_full_own_list_does_not_hide_a_broken_project_change() {
        let mut s = Session::new();
        select(&mut s, &["cemig"]);
        s.model_mut().codes.own =
            (0..MAX_OWN).map(|i| BuildingCode { code: format!("Full review entry {i}"), discipline: D, ..Default::default() }).collect();
        let key = "building_demand";
        let mut patch = serde_json::Map::new();
        patch.insert("factor".into(), json!("bad"));
        s.model_mut().codes.choice_mut(D).changes.insert(key.into(), patch);
        // A method in the project's own changes is not a layer's: the file stays out, as before.
        let path = legacy_file(&mut s, "open-review-full-own");
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
    }

    #[test]
    fn open_review_demand_without_selector_names_the_only_broken_method() {
        let mut s = Session::new();
        let key = "entrance";
        add(&mut s, "Broken only entrance", key, changed(find(D, "fecoergs").unwrap(), key, 1));
        replace(&mut s, "Broken only entrance", key, "tables", json!("bad"));
        let mut s = reopen(&mut s, "open-review-only-entrance");
        let mut asked = supply();
        asked["utility"] = json!("fEcOeRgS");
        refuses(&mut s, "demand", &asked, "Broken only entrance");
        refuses(&mut s, "demand", &supply(), "Broken only entrance");
    }

    #[test]
    fn open_review_every_utility_in_a_broken_building_method_is_named() {
        let mut s = Session::new();
        let key = "building_demand";
        let mut raw = changed(find(D, "cemig").unwrap(), key, 1);
        raw["utilities"] = json!(["Local first review", "Local second review"]);
        add(&mut s, "Broken two utility review", key, raw);
        replace(&mut s, "Broken two utility review", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "open-review-two-utilities");
        for utility in ["LOCAL FIRST REVIEW", "local SECOND review"] {
            let mut p = dwellings();
            p["utility"] = json!(utility);
            refuses(&mut s, "building_demand", &p, "Broken two utility review");
        }
    }

    #[test]
    fn open_review_implicit_building_demand_never_uses_the_method_below_a_scalar() {
        let mut s = Session::new();
        let key = "building_demand";
        let lib = find(D, "cemig").unwrap();
        add(&mut s, "Readable lower CEMIG", key, changed(lib, key, 1));
        select(&mut s, &["Readable lower CEMIG", "cemig"]);
        codes(&mut s, json!({"amend": "cemig", "rules": {key: changed(lib, key, 2)}}));
        s.model_mut().codes.own.iter_mut().find(|c| c.code == "cemig").unwrap().sections.insert(key.into(), json!(false));
        let mut s = reopen(&mut s, "open-review-scalar-above-building");
        eprintln!(
            "DBG layers {:?}",
            s.model
                .codes
                .layers(D)
                .iter()
                .map(|c| (c.code.clone(), c.sections.get("building_demand").map(|v| v.is_object())))
                .collect::<Vec<_>>()
        );
        let mut named = dwellings();
        named["utility"] = json!("cEmIg");
        refuses(&mut s, "building_demand", &named, "cemig");
        refuses(&mut s, "building_demand", &dwellings(), "cemig");
    }

    #[test]
    fn open_review_full_amendment_list_does_not_hide_a_broken_project_change() {
        let mut s = Session::new();
        let key = "building_demand";
        select(&mut s, &[]);
        for i in 0..MAX_AMENDMENTS {
            let name = format!("Empty review amendment {i}");
            s.model_mut().codes.own.push(BuildingCode {
                code: name.clone(),
                discipline: D,
                amends: "nbr-5410".into(),
                ..Default::default()
            });
            s.model_mut().codes.choice_mut(D).amendments.push(name);
        }
        s.model_mut().codes.choice_mut(D).changes.insert(key.into(), json!({"factor": "bad"}).as_object().unwrap().clone());
        let path = legacy_file(&mut s, "open-review-full-amendments");
        assert!(Session::new().execute("open", &json!({"path": path})).is_err());
    }

    #[test]
    fn open_review_broken_method_does_not_change_own_name_and_list_limits() {
        let mut s = review_broken_cemig();
        let mut s = reopen(&mut s, "open-review-name-limits");
        for len in [63, 64] {
            codes(&mut s, json!({"create": "x".repeat(len)}));
        }
        let before = s.model.codes.clone();
        assert!(s.execute("building_code", &json!({"discipline": "electrical", "create": "x".repeat(65)})).is_err());
        assert_eq!(s.model.codes, before);
        for i in s.model.codes.own.len()..MAX_OWN {
            let name = format!("Unused review own {i}");
            codes(&mut s, json!({"create": name}));
        }
        assert_eq!(s.model.codes.own.len(), MAX_OWN);
        let before = s.model.codes.clone();
        assert!(s.execute("building_code", &json!({"discipline": "electrical", "create": "One own code too many"})).is_err());
        assert_eq!(s.model.codes, before);
        let amendments: Vec<String> = (0..=MAX_AMENDMENTS).map(|i| format!("Review amendment limit {i}")).collect();
        let mut s = review_broken_cemig();
        let mut s = reopen(&mut s, "open-review-amendment-limits");
        for name in &amendments {
            codes(&mut s, json!({"create": name}));
        }
        codes(&mut s, json!({"amendments": &amendments[..MAX_AMENDMENTS]}));
        let before = s.model.codes.clone();
        assert!(s.execute("building_code", &json!({"discipline": "electrical", "amendments": amendments})).is_err());
        assert_eq!(s.model.codes, before);
        codes(&mut s, json!({"amendments": []}));
    }

    #[test]
    fn open_review_broken_method_keeps_json_depth_and_number_limits() {
        let mut s = review_broken_cemig();
        let marker = "review-bad-method-json-marker";
        replace(&mut s, "Broken review CEMIG", "building_demand", "factor", json!(marker));
        let path = legacy_file(&mut s, "open-review-json-limits");
        let original = std::fs::read_to_string(&path).unwrap();
        let depth = format!("{}0{}", "[".repeat(150), "]".repeat(150));
        for (value, diagnostic) in [("1e9999", "number out of range"), (&depth, "recursion limit exceeded")] {
            let text = original.replace(&format!("\"{marker}\""), value);
            assert_ne!(text, original);
            std::fs::write(&path, text).unwrap();
            refuses(&mut Session::new(), "open", &json!({"path": path}), diagnostic);
        }
    }

    #[test]
    fn a_base_of_its_own_with_a_broken_method_under_a_readable_one_stays_editable() {
        // The broken method is under one that reads (not in force): the file opens, and other edits go on; it stays in its
        // own code only, never copied into another.
        let mut s = Session::new();
        select(&mut s, &["cemig"]);
        codes(&mut s, json!({"save": "Own broken base"}));
        replace(&mut s, "Own broken base", "building_demand", "factor", json!("bad"));
        codes(&mut s, json!({"create": "Readable over", "amendments": ["Readable over"]}));
        codes(
            &mut s,
            json!({"amend": "Readable over", "rules": {"building_demand": changed(find(D, "cemig").unwrap(), "building_demand", 1)}}),
        );
        let mut s = reopen(&mut s, "own-base-under-readable");
        codes(&mut s, json!({"rules": {"voltages": {"60 V": {"nominal": 60, "min": 54, "max": 66}}}}));
        assert_eq!(building(&mut s, &dwellings()), 88.595);
        assert!(s.execute("building_code", &json!({"discipline": "electrical", "save": "Copy of a broken base"})).is_err());
    }

    #[test]
    fn a_broken_unit_demand_on_top_opens_and_names_itself() {
        let mut s = Session::new();
        let celesc = find(D, "celesc").unwrap();
        add(&mut s, "Broken top unit", "unit_demand", changed(celesc, "unit_demand", 1));
        select(&mut s, &["celesc", "Broken top unit"]);
        replace(&mut s, "Broken top unit", "unit_demand", "method", json!("bad"));
        let mut s = reopen(&mut s, "top-unit");
        let mut p = supply();
        p["utility"] = json!("CELESC");
        refuses(&mut s, "demand", &p, "Broken top unit");
    }

    #[test]
    fn a_named_standard_answers_under_another_utility_s_broken_method() {
        // Naming one, another's broken method on top is not its own: it answers as it did.
        let mut s = Session::new();
        let key = "building_demand";
        add(&mut s, "Broken HIDROPAN on top", key, changed(find(D, "hidropan").unwrap(), key, 1));
        select(&mut s, &["Broken HIDROPAN on top"]);
        let mut p = dwellings();
        p["standard"] = json!("CEMIG ND-5.2");
        let before = building(&mut s, &p);
        replace(&mut s, "Broken HIDROPAN on top", key, "factor", json!("bad"));
        let mut s = reopen(&mut s, "named-under-other");
        assert_eq!(building(&mut s, &p), before);
    }
}
