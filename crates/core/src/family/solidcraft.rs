//! Opt-in local bridge. A family carries its saved SC document. Only parameter
//! changes constructed here are sent to the user-configured evaluator.
use super::{MAX_BUILT, Piece, Val};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
#[cfg(test)]
use std::fs;
use std::sync::{Arc, Mutex};

const MAX_SOURCE_BASE64: usize = 24 * 1024 * 1024;

fn evaluator_path(value: Option<std::ffi::OsString>) -> Result<std::path::PathBuf, String> {
    value.filter(|s| !s.is_empty()).map(std::path::PathBuf::from).ok_or_else(|| {
        "Configure BIMCRAFT_SOLIDCRAFT_CLI with your trusted vanilla SolidCraft 0.44.1 CLI before loading a source-backed family".into()
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub document: String,
    pub inputs: BTreeMap<String, Input>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub unit: String,
    pub original: Val,
    /// Explicit publisher choice for a unitless source input; never inferred from its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<super::Kind>,
}

struct Entry {
    evaluator: std::path::PathBuf,
    source: Arc<Source>,
    changes: BTreeMap<String, String>,
    pieces: Vec<Piece>,
}
// Several views, validation and export can ask for precisely the same family.
// Retain only successful results, bounded in both entry count and triangles.
static CACHE: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

impl Source {
    /// Bindings name source parameters, not display aliases. Until the adapter has stable source IDs,
    /// deleting or renaming a published input must be an explicit source edit and republish.
    pub fn check_parameters(&self, parameters: &[super::Parameter]) -> Result<(), String> {
        for (name, input) in &self.inputs {
            let parameter = parameters.iter().find(|p| p.name == *name).ok_or_else(||
                format!("SolidCraft input `{name}` cannot be renamed or deleted here. Change it in SolidCraft and republish with reviewed bindings."))?;
            let kind = match input.unit.as_str() {
                "mm" | "cm" | "m" | "in" | "ft" => super::Kind::Length,
                "deg" | "rad" => super::Kind::Angle,
                "" => super::Kind::Number,
                "text" => super::Kind::Text,
                unit => return Err(format!("Unsupported SolidCraft input unit `{unit}` for `{name}`")),
            };
            let kind = match input.kind {
                Some(k @ (super::Kind::Integer | super::Kind::YesNo)) if input.unit.is_empty() => k,
                Some(k) if k == kind => k,
                None => kind,
                _ => return Err(format!("Incompatible SolidCraft control kind for `{name}`")),
            };
            if parameter.kind != kind {
                return Err(format!("SolidCraft input `{name}` must keep its published value kind"));
            }
        }
        Ok(())
    }

    pub(super) fn pieces(self: &Arc<Self>, values: &BTreeMap<String, Val>) -> Result<Vec<Piece>, String> {
        let evaluator = evaluator_path(std::env::var_os("BIMCRAFT_SOLIDCRAFT_CLI"))?;
        let mut changes = BTreeMap::new();
        for (name, input) in &self.inputs {
            let value = values.get(name).ok_or_else(|| format!("Missing SolidCraft input {name}"))?;
            if value == &input.original {
                continue;
            }
            let expr = match (value, input.unit.as_str()) {
                (Val::Number(x), "mm" | "cm" | "m" | "in" | "ft") => format!("{x} mm"),
                (Val::Number(x), "deg" | "rad") => format!("{x} deg"),
                (Val::Number(x), "") => x.to_string(),
                (Val::Text(s), "text") => s.clone(),
                _ => return Err(format!("Incompatible SolidCraft input {name} ({})", input.unit)),
            };
            changes.insert(name.clone(), expr);
        }
        let mut cache = CACHE.lock().map_err(|_| "SolidCraft cache unavailable")?;
        if let Some(hit) =
            cache.iter().find(|e| e.evaluator == evaluator && (Arc::ptr_eq(self, &e.source) || **self == *e.source) && e.changes == changes)
        {
            return Ok(hit.pieces.clone());
        }
        let pieces = self.evaluate(&changes)?;
        fn size(p: &[Piece]) -> usize {
            p.iter().map(|p| p.tris.len()).sum()
        }
        let needed = size(&pieces);
        while !cache.is_empty() && (cache.len() >= 32 || cache.iter().map(|e| size(&e.pieces)).sum::<usize>() + needed > MAX_BUILT) {
            cache.pop_front();
        }
        cache.push_back(Entry { evaluator, source: self.clone(), changes, pieces: pieces.clone() });
        Ok(pieces)
    }

    fn evaluate(&self, changes: &BTreeMap<String, String>) -> Result<Vec<Piece>, String> {
        if self.document.len() > MAX_SOURCE_BASE64 {
            return Err("SolidCraft source document exceeds the 24 MiB encoded budget".into());
        }
        let bytes = STANDARD.decode(&self.document).map_err(|e| e.to_string())?;
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err("Expected a saved SolidCraft document".into());
        }
        density::evaluate(&bytes, changes)
    }
}

mod density;

#[cfg(test)]
mod control_tests {
    use super::*;
    #[test]
    fn evaluator_is_explicitly_configured_and_source_is_bounded() {
        assert!(evaluator_path(None).unwrap_err().contains("BIMCRAFT_SOLIDCRAFT_CLI"));
        assert!(evaluator_path(Some("".into())).is_err());
        assert_eq!(evaluator_path(Some("/opt/sc/solidcraft-cli".into())).unwrap(), std::path::PathBuf::from("/opt/sc/solidcraft-cli"));
        let source = Source { document: "A".repeat(MAX_SOURCE_BASE64 + 1), inputs: BTreeMap::new() };
        assert!(source.evaluate(&BTreeMap::new()).unwrap_err().contains("source document exceeds"));
    }
    #[test]
    fn published_control_kinds_are_explicit_and_unit_compatible() {
        for kind in [super::super::Kind::Integer, super::super::Kind::YesNo] {
            let mut source: Source =
                serde_json::from_value(json!({"document":"", "inputs":{"ArbitraryInput":{"unit":"","original":1,"kind":kind}}})).unwrap();
            let p: super::super::Parameter = serde_json::from_value(
                json!({"name":"ArbitraryInput","kind":kind,"value":if kind==super::super::Kind::YesNo {json!(true)} else {json!(1)}}),
            )
            .unwrap();
            assert!(source.check_parameters(std::slice::from_ref(&p)).is_ok());
            source.inputs.get_mut("ArbitraryInput").unwrap().kind = None;
            assert!(source.check_parameters(std::slice::from_ref(&p)).is_err(), "no inference from the parameter name or default");
            source.inputs.get_mut("ArbitraryInput").unwrap().kind = Some(kind);
            source.inputs.get_mut("ArbitraryInput").unwrap().unit = "mm".into();
            assert!(source.check_parameters(&[p]).is_err(), "a length cannot be reinterpreted as a switch or count");
        }
        let old: Source = serde_json::from_value(json!({"document":"","inputs":{"Plain":{"unit":"","original":1}}})).unwrap();
        let p = serde_json::from_value(json!({"name":"Plain","kind":"number","value":1})).unwrap();
        assert!(old.check_parameters(&[p]).is_ok());
    }
}

#[cfg(test)]
mod density_tests {
    use super::*;
    #[test]
    #[ignore = "requires SC_DENSITY_SOURCE and pinned vanilla SolidCraft 0.44.1"]
    fn native_source_keeps_every_piece_within_the_family_budget() {
        let path = std::env::var("SC_DENSITY_SOURCE").unwrap();
        let source = Arc::new(Source { document: STANDARD.encode(fs::read(path).unwrap()), inputs: BTreeMap::new() });
        let pieces = source.pieces(&BTreeMap::new()).unwrap();
        assert_eq!(pieces.len(), 62);
        assert!(pieces.iter().map(|p| p.tris.len()).sum::<usize>() < MAX_BUILT);
    }
}
