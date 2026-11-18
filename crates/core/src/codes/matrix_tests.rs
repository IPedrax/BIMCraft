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
    assert!(asked >= 300, "cases asked ({asked})");
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
