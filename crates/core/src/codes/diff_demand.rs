//! Scratch, not committed: every demand answer over many layerings, to compare before and after.
use serde_json::{Value, json};

use super::*;
use crate::engine::Session;

#[test]
fn dump_demand_answers() {
    let Ok(out) = std::env::var("DIFF_OUT") else { return };
    let keys = ["entrance", "unit_demand"];
    let all: Vec<&BuildingCode> = of(Discipline::Electrical).collect();
    let with: Vec<&BuildingCode> =
        all.iter().copied().filter(|c| !c.amends.is_empty() && keys.iter().any(|k| c.sections.contains_key(*k))).collect();
    let without: Vec<&BuildingCode> =
        all.iter().copied().filter(|c| !c.amends.is_empty() && !keys.iter().any(|k| c.sections.contains_key(*k))).collect();
    // Names: every code, entrance id and member, unit utility, in two cases; an unknown one.
    let mut names: Vec<String> = vec!["Nope".into()];
    for c in &with {
        names.push(c.code.clone());
        names.push(c.code.to_uppercase());
        if let Some(e) = c.sections.get("entrance") {
            names.push(e["id"].as_str().unwrap_or_default().to_string());
            names.push(e["id"].as_str().unwrap_or_default().to_lowercase());
            for m in e["members"].as_array().into_iter().flatten() {
                names.push(m["id"].as_str().unwrap_or_default().to_string());
                names.push(m["id"].as_str().unwrap_or_default().to_lowercase());
            }
        }
        if let Some(u) = c.sections.get("unit_demand") {
            names.push(u["utility"].as_str().unwrap_or_default().to_string());
            names.push(u["utility"].as_str().unwrap_or_default().to_lowercase());
        }
    }
    names.sort();
    names.dedup();
    // Probes: each code's own probe for each key it holds, its utility left out (asked by `names`, or not at all).
    let mut probes: Vec<Value> = vec![json!({"installed_kw": 8}), json!({"area": 40})];
    for c in &with {
        for k in keys {
            if let Some(m) = c.sections.get(k) {
                let (_, mut p) = super::matrix_tests::method_probe(c, k, m);
                p.as_object_mut().unwrap().remove("utility");
                probes.push(p);
            }
        }
    }
    probes.sort_by_key(|v| v.to_string());
    probes.dedup();
    let mut stacks: Vec<Vec<&BuildingCode>> = Vec::new();
    for a in &with {
        stacks.push(vec![a]);
        for b in with.iter().chain(&without).filter(|b| b.code != a.code && b.amends == a.amends) {
            stacks.push(vec![a, b]);
            stacks.push(vec![b, a]);
        }
    }
    let mut lines = Vec::new();
    for stack in &stacks {
        let codes: Vec<&str> = stack.iter().map(|c| c.code.as_str()).collect();
        // Variants: none; for the top or the bottom and each key it holds: changed; broken as a text; broken as an object.
        let mut variants: Vec<(usize, &str, u8)> = vec![(0, "", 0)];
        for (i, c) in stack.iter().enumerate() {
            for k in keys {
                if c.sections.contains_key(k) {
                    for how in 1..=3 {
                        variants.push((i, k, how));
                    }
                }
                // 4: the other code's method copied into this layer; 5: so, and broken as an object.
                if stack.len() == 2 && stack[1 - i].sections.contains_key(k) {
                    variants.push((i, k, 4));
                    variants.push((i, k, 5));
                }
                // 6: a unit demand for the other code's group of utilities in this layer; 7: so, and broken as an object.
                if k == "unit_demand"
                    && stack.len() == 2
                    && stack[1 - i].sections.get("entrance").is_some_and(|e| e["members"].as_array().is_some_and(|m| !m.is_empty()))
                {
                    variants.push((i, k, 6));
                    variants.push((i, k, 7));
                }
            }
        }
        for (i, k, how) in variants {
            let mut s = Session::new();
            if s.execute("building_code", &json!({"discipline": "electrical", "set": stack[0].amends, "amendments": codes})).is_err() {
                continue;
            }
            let mut ok = true;
            if how > 0 {
                let t = stack[i];
                let from = if how >= 4 { stack[1 - i] } else { t };
                let mut v = if how >= 6 {
                    let mut u = find(Discipline::Electrical, "cpfl").unwrap().sections["unit_demand"].clone();
                    u["utility"] = from.sections["entrance"]["id"].clone();
                    u
                } else {
                    from.sections[k].clone()
                };
                super::matrix_tests::breakers_up(&mut v);
                if s.execute("building_code", &json!({"discipline": "electrical", "amend": t.code, "rules": {k: v}})).is_err() {
                    ok = false;
                }
                if how > 1 && how != 4 && how != 6 {
                    match s.model_mut().codes.own.iter_mut().find(|c| c.code == t.code) {
                        Some(o) => {
                            let sec = o.sections.get_mut(k).unwrap();
                            if how == 2 { *sec = json!("oops") } else { sec[if k == "entrance" { "source" } else { "method" }] = json!(7) }
                            let _ = &sec;
                        }
                        None => ok = false,
                    }
                }
            }
            let mut asks: Vec<Option<&String>> = vec![None];
            asks.extend(names.iter().map(Some));
            for n in asks {
                for p in &probes {
                    let mut q = p.clone();
                    if let Some(n) = n {
                        q["utility"] = json!(n);
                    }
                    let r = s.execute("demand", &q);
                    lines.push(format!("{codes:?} {i}{k}{how} ok={ok} {q} => {:?}", r.map(|v| v.to_string())));
                }
            }
        }
    }
    std::fs::write(out, lines.join("\n")).unwrap();
}
