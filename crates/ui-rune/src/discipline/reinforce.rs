//! Structure > Reinforce (RF) as a discipline dialog: arms the selected beam, column, footing or floor by the `reinforce` command. The engineer types how
//! many bars, how thick and how far apart; the dialog builds the command's parameters from the fields that are filled and the command
//! checks them, so a refusal is the command's words, said inside the dialog. Presets save and load the fields for a kind of host. The groups
//! of fields for each kind are egui's (`crates/ui-egui/src/reinforce.rs`), copied here as data.

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::units::Units;
use rune::TextEdit;
use serde_json::{Value, json};

use super::{Cell, Choice, Discipline, Foot, Host, Outcome, Section, edits, typed};
use crate::lang::{t, tf};
use crate::props::names::category;

/// How a field's text is read: a whole number, a bar size in mm, a plain number (an angle, a slope, a strength) or a length in the
/// project's units.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    Count,
    Diameter,
    Number,
    Length,
}

/// A field of the form: the path of the parameter it sets (`bottom.count`), its label and how it reads.
struct Field {
    path: &'static str,
    label: &'static str,
    read: Read,
}

/// A row of the form: a name and its fields; a row with no field filled is left out of the parameters.
struct Group {
    name: &'static str,
    fields: &'static [Field],
}

const fn field(path: &'static str, label: &'static str, read: Read) -> Field {
    Field { path, label, read }
}

const BOTTOM: Group = Group {
    name: "Bottom bars",
    fields: &[
        field("bottom.count", "Count", Read::Count),
        field("bottom.diameter", "Diameter", Read::Diameter),
        field("bottom.hook", "Hook", Read::Length),
    ],
};
const TOP: Group = Group {
    name: "Top bars",
    fields: &[
        field("top.count", "Count", Read::Count),
        field("top.diameter", "Diameter", Read::Diameter),
        field("top.hook", "Hook", Read::Length),
    ],
};
const STIRRUPS: Group = Group {
    name: "Stirrups",
    fields: &[
        field("stirrup.diameter", "Diameter", Read::Diameter),
        field("stirrup.spacing", "Spacing", Read::Length),
        field("stirrup.ends.length", "End zone length", Read::Length),
        field("stirrup.ends.spacing", "End zone spacing", Read::Length),
        field("stirrup.hook", "Hook", Read::Length),
    ],
};
const JOG: Group = Group {
    name: "Jog",
    fields: &[
        field("bars.jog.by", "Step in", Read::Length),
        field("bars.jog.from_top", "Starts below the top", Read::Length),
        field("bars.jog.slope", "Slope", Read::Number),
    ],
};
const COLUMN_ANCHOR: Group =
    Group { name: "Anchor", fields: &[field("anchor.below", "Below", Read::Length), field("anchor.above", "Above", Read::Length)] };

const BEAM: &[Group] = &[
    BOTTOM,
    TOP,
    STIRRUPS,
    Group {
        name: "Bent-up bars",
        fields: &[
            field("bottom.bent.count", "Count", Read::Count),
            field("bottom.bent.angle", "Angle", Read::Number),
            field("bottom.bent.top", "Along the top", Read::Length),
        ],
    },
    Group {
        name: "Skin bars",
        fields: &[field("skin.diameter", "Diameter", Read::Diameter), field("skin.spacing", "Spacing", Read::Length)],
    },
    Group { name: "Anchor", fields: &[field("anchor.start", "Start", Read::Length), field("anchor.end", "End", Read::Length)] },
];
const COLUMN: &[Group] = &[
    Group {
        name: "Bars",
        fields: &[
            field("bars.diameter", "Diameter", Read::Diameter),
            field("bars.x", "Along width", Read::Count),
            field("bars.y", "Along depth", Read::Count),
            field("bars.hook", "Hook", Read::Length),
        ],
    },
    JOG,
    STIRRUPS,
    Group { name: "Ties", fields: &[field("ties.diameter", "Diameter", Read::Diameter), field("ties.hook", "Hook", Read::Length)] },
    COLUMN_ANCHOR,
];
/// A round column: the bars are counted round its circle, and it has no ties.
const ROUND_COLUMN: &[Group] = &[
    Group {
        name: "Bars",
        fields: &[
            field("bars.diameter", "Diameter", Read::Diameter),
            field("bars.count", "Count", Read::Count),
            field("bars.hook", "Hook", Read::Length),
        ],
    },
    JOG,
    STIRRUPS,
    COLUMN_ANCHOR,
];
const FOOTING: &[Group] = &[
    Group {
        name: "Mesh along width",
        fields: &[field("mesh.x.diameter", "Diameter", Read::Diameter), field("mesh.x.spacing", "Spacing", Read::Length)],
    },
    Group {
        name: "Mesh along depth",
        fields: &[field("mesh.y.diameter", "Diameter", Read::Diameter), field("mesh.y.spacing", "Spacing", Read::Length)],
    },
    Group { name: "Ends turned up", fields: &[field("mesh.hook", "Hook", Read::Length)] },
];
const FLOOR: &[Group] = &[
    Group { name: "Bottom ends turned up", fields: &[field("bottom.hook", "Hook", Read::Length)] },
    Group { name: "Top ends turned down", fields: &[field("top.hook", "Hook", Read::Length)] },
    Group {
        name: "Bottom, bars along east",
        fields: &[field("bottom.x.diameter", "Diameter", Read::Diameter), field("bottom.x.spacing", "Spacing", Read::Length)],
    },
    Group {
        name: "Bottom, bars along north",
        fields: &[field("bottom.y.diameter", "Diameter", Read::Diameter), field("bottom.y.spacing", "Spacing", Read::Length)],
    },
    Group {
        name: "Top, bars along east",
        fields: &[field("top.x.diameter", "Diameter", Read::Diameter), field("top.x.spacing", "Spacing", Read::Length)],
    },
    Group {
        name: "Top, bars along north",
        fields: &[field("top.y.diameter", "Diameter", Read::Diameter), field("top.y.spacing", "Spacing", Read::Length)],
    },
];

/// The standards the Check row offers: what `check` takes.
const NBR6118: &str = "NBR 6118";
const EN1992: &str = "EN 1992-1-1";

/// The cover's fields: the key of each and its label.
const COVER: [(&str, &str); 4] = [("cover", "Cover"), ("cover_top", "Top"), ("cover_bottom", "Bottom"), ("cover_sides", "Sides")];

/// The fields that are not a group's: the steel, the check, the room between bars and the name a preset is saved under.
const OTHER: [&str; 9] = ["grade", "grade_stirrup", "clearance", "check", "exposure", "aggressiveness", "fck", "fyk", "save_as"];

/// What a host is armed as: the form it has.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Beam,
    Column,
    RoundColumn,
    Footing,
    Floor,
}

impl Form {
    fn of(m: &Model, e: &Element) -> Option<Form> {
        match e {
            Element::Beam(_) => Some(Form::Beam),
            Element::Column(_) | Element::StructuralColumn(_) if m.column_type(e).is_some_and(|t| t.round) => Some(Form::RoundColumn),
            Element::Column(_) | Element::StructuralColumn(_) => Some(Form::Column),
            Element::Footing(_) => Some(Form::Footing),
            Element::Floor(_) => Some(Form::Floor),
            _ => None,
        }
    }

    /// The kind of host a preset for this form is for.
    fn kind(self) -> &'static str {
        match self {
            Form::Beam => "beam",
            Form::Column | Form::RoundColumn => "column",
            Form::Footing => "footing",
            Form::Floor => "floor",
        }
    }

    fn groups(self) -> &'static [Group] {
        match self {
            Form::Beam => BEAM,
            Form::Column => COLUMN,
            Form::RoundColumn => ROUND_COLUMN,
            Form::Footing => FOOTING,
            Form::Floor => FLOOR,
        }
    }
}

/// `value` at the dotted `path` of `p`, the objects on the way made.
fn set(p: &mut Value, path: &str, value: Value) {
    let mut at = p;
    for key in path.split('.') {
        let Some(map) = at.as_object_mut() else { return };
        at = map.entry(key.to_string()).or_insert(Value::Null);
        if at.is_null() {
            *at = json!({});
        }
    }
    *at = value;
}

/// The Reinforce dialog: the hosts, what is typed (one edit for each of `keys`), the presets of the kind, what the command last refused,
/// and the bars the command made for the host before, which Remove Bars takes away.
pub struct Reinforce {
    hosts: Vec<Id>,
    category: &'static str,
    form: Form,
    keys: Vec<&'static str>,
    edits: Vec<TextEdit>,
    units: Units,
    /// The names of the presets of this kind, and the one the fields were last filled from.
    names: Vec<String>,
    preset: String,
    made: Vec<Id>,
    notes: Vec<String>,
    error: Option<String>,
}

impl Reinforce {
    /// Opens on the elements selected, if they are ones that can be armed, all of one kind. Why not, in words.
    pub fn open(m: &Model, selection: &[Id]) -> Result<Reinforce, String> {
        let picked: Vec<(Id, &'static str, Form)> =
            selection.iter().filter_map(|id| m.elements.get(*id).and_then(|e| Form::of(m, e).map(|f| (*id, e.category(), f)))).collect();
        let Some(&(_, category, form)) = picked.first().filter(|_| picked.len() == selection.len()) else {
            return Err(t("Select a beam, a column, a footing or a floor to reinforce first.").to_string());
        };
        if picked.iter().any(|(_, _, f)| *f != form) {
            return Err(t("Select elements of one kind: all beams, all columns, all footings or all floors.").to_string());
        }
        let guids: std::collections::BTreeSet<_> = picked.iter().map(|(id, _, _)| m.guid(*id)).collect();
        let made = m
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Rebar(r) if r.host.is_some_and(|h| guids.contains(&h)) && !r.layer.is_empty()))
            .map(|(i, _)| *i)
            .collect();
        let keys: Vec<&'static str> =
            form.groups().iter().flat_map(|g| g.fields.iter().map(|f| f.path)).chain(COVER.iter().map(|(k, _)| *k)).chain(OTHER).collect();
        let edits = edits(keys.iter().map(|_| String::new()));
        let mut d = Reinforce {
            hosts: picked.iter().map(|(id, _, _)| *id).collect(),
            category,
            form,
            keys,
            edits,
            units: m.units,
            names: Vec::new(),
            preset: String::new(),
            made,
            notes: Vec::new(),
            error: None,
        };
        d.refresh(m);
        Ok(d)
    }

    /// The presets of this kind, as the project has them now.
    fn refresh(&mut self, m: &Model) {
        let kind = self.form.kind();
        self.names = m.reinforcement.iter().filter(|(_, p)| p.kind == kind).map(|(n, _)| n.clone()).collect();
    }

    fn at(&self, key: &str) -> usize {
        self.keys.iter().position(|k| *k == key).unwrap_or(0)
    }

    /// The text of field `key`, trimmed.
    fn text(&self, key: &str) -> String {
        self.edits.get(self.at(key)).map(|e| typed(e).trim().to_string()).unwrap_or_default()
    }

    fn put(&mut self, key: &str, text: String) {
        let i = self.at(key);
        if let Some(e) = self.edits.get_mut(i) {
            *e = TextEdit::new(text);
        }
    }

    /// What field `key`, called `name`, says, if it says anything.
    fn read(&self, key: &str, read: Read, name: &str) -> Result<Option<Value>, String> {
        let text = self.text(key);
        if text.is_empty() {
            return Ok(None);
        }
        let not = |what: &str| tf("{} is not {}: `{}`", &[name, what, &text]);
        match read {
            Read::Count => text.parse::<u32>().map(|n| Some(json!(n))).map_err(|_| not(t("a whole number"))),
            Read::Diameter | Read::Number => text
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .map(|x| Some(json!(x)))
                .ok_or_else(|| not(if read == Read::Number { t("a number") } else { t("a size in mm") })),
            Read::Length => self.units.parse(&text).filter(|x| x.is_finite()).map(|x| Some(json!(x))).ok_or_else(|| not(t("a length"))),
        }
    }

    /// The fields, filled as the parameters of a preset say.
    fn load(&mut self, m: &Model, params: &serde_json::Map<String, Value>) {
        for e in &mut self.edits {
            *e = TextEdit::new("");
        }
        let p = Value::Object(params.clone());
        let form = self.form;
        for g in form.groups() {
            for f in g.fields {
                if let Some(v) = f.path.split('.').try_fold(&p, |v, k| v.get(k)).and_then(Value::as_f64) {
                    let text = match f.read {
                        Read::Count => format!("{}", v.round()),
                        Read::Diameter | Read::Number => format!("{v}"),
                        Read::Length => m.field(v),
                    };
                    self.put(f.path, text);
                }
            }
        }
        match p.get("cover") {
            Some(Value::Object(o)) => {
                for (face, key) in [("top", "cover_top"), ("bottom", "cover_bottom"), ("sides", "cover_sides")] {
                    if let Some(v) = o.get(face).and_then(Value::as_f64) {
                        self.put(key, m.field(v));
                    }
                }
            }
            Some(v) => {
                if let Some(x) = v.as_f64() {
                    self.put("cover", m.field(x));
                }
            }
            None => {}
        }
        for key in ["grade", "grade_stirrup", "check", "exposure"] {
            if let Some(g) = p.get(key).and_then(Value::as_str) {
                self.put(key, g.to_string());
            }
        }
        for key in ["aggressiveness", "fck", "fyk"] {
            if let Some(v) = p.get(key).and_then(Value::as_f64) {
                self.put(key, format!("{v}"));
            }
        }
        if let Some(v) = p.get("clearance").and_then(Value::as_f64) {
            self.put("clearance", m.field(v));
        }
    }

    /// The command's parameters from the fields filled.
    pub fn params(&self) -> Result<Value, String> {
        let mut p = json!({"hosts": self.hosts});
        for g in self.form.groups() {
            for f in g.fields {
                let name = format!("{}: {}", t(g.name), t(f.label));
                if let Some(v) = self.read(f.path, f.read, &name)? {
                    set(&mut p, f.path, v);
                }
            }
        }
        let face = |key: &str, label: &str| self.read(key, Read::Length, &format!("{}: {}", t("Cover"), t(label)));
        let (all, top, bottom, sides) = (
            self.read("cover", Read::Length, t("Cover"))?,
            face("cover_top", "Top")?,
            face("cover_bottom", "Bottom")?,
            face("cover_sides", "Sides")?,
        );
        if top.is_some() || bottom.is_some() || sides.is_some() {
            let mut cover = json!({});
            for (key, v) in [("top", top), ("bottom", bottom), ("sides", sides)] {
                if let Some(v) = v.or_else(|| all.clone()) {
                    cover[key] = v;
                }
            }
            p["cover"] = cover;
        } else if let Some(all) = all {
            p["cover"] = all;
        }
        for key in ["grade", "grade_stirrup"] {
            if !self.text(key).is_empty() {
                p[key] = json!(self.text(key));
            }
        }
        if let Some(v) = self.read("clearance", Read::Length, t("Steel: Clearance"))? {
            p["clearance"] = v;
        }
        if !self.text("check").is_empty() {
            p["check"] = json!(self.text("check"));
        }
        // Only what the standard takes: the other's fields may hold what was typed before it was changed.
        let en = self.text("check") == EN1992;
        if en && !self.text("exposure").is_empty() {
            p["exposure"] = json!(self.text("exposure"));
        }
        for (key, read, label, own) in [
            ("aggressiveness", Read::Count, "Check: Aggressiveness class", !en),
            ("fck", Read::Number, "Check: fck", true),
            ("fyk", Read::Number, "Check: fyk", en),
        ] {
            if own && let Some(v) = self.read(key, read, t(label))? {
                p[key] = v;
            }
        }
        Ok(p)
    }

    /// OK: the command with the fields. The check's notes, when it has any, keep the dialog open with the bars made.
    fn ok(&mut self, host: &mut dyn Host) -> Outcome {
        let params = match self.params() {
            Ok(p) => p,
            Err(why) => {
                self.error = Some(why);
                return Outcome::Keep;
            }
        };
        match host.execute("reinforce", &params) {
            Ok(reply) => {
                let (count, kg) = (reply["count"].as_u64().unwrap_or(0), reply["weight_kg"].as_f64().unwrap_or(0.0));
                host.say(tf("Reinforced: {} bars, {} kg of steel.", &[&count.to_string(), &kg.to_string()]));
                self.error = None;
                self.notes = reply["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).collect();
                if self.notes.is_empty() {
                    return Outcome::Close;
                }
                // What the check found stays, with the bars made: change a field and OK again to remake them.
                self.made = reply["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
                Outcome::Keep
            }
            Err(why) => {
                self.error = Some(why);
                Outcome::Keep
            }
        }
    }

    /// Save Preset: the fields as they are, under the name typed.
    fn save(&mut self, host: &mut dyn Host) {
        let name = self.text("save_as");
        self.error = None;
        if name.is_empty() {
            self.error = Some(t("Type a name for the preset first.").to_string());
            return;
        }
        let mut p = match self.params() {
            Ok(p) => p,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        if let Some(o) = p.as_object_mut() {
            o.remove("hosts");
        }
        match host.execute("reinforcement_preset", &json!({"name": name, "kind": self.form.kind(), "params": p})) {
            Ok(_) => {
                self.preset = name;
                self.put("save_as", String::new());
                self.refresh(host.model());
            }
            Err(why) => self.error = Some(why),
        }
    }
}

impl Discipline for Reinforce {
    fn title(&self) -> String {
        match self.hosts.as_slice() {
            [one] => tf("Reinforce {} {}", &[&category(self.category, false), &one.to_string()]),
            many => tf("Reinforce {} {}", &[&many.len().to_string(), &category(self.category, true)]),
        }
    }

    fn about(&self) -> String {
        t("How many bars, how thick and how far apart is yours to say; a field left empty is not used. A spacing is the most between bars.")
            .to_string()
    }

    fn width(&self) -> f32 {
        780.0
    }

    fn sections(&self) -> Vec<Section> {
        let hint = self.units.name();
        let at = |key: &str, name: String, hint: &str, width: f32| Cell::field(self.at(key), name, hint, width);
        let label = |text: &str| Cell::text(t(text), 118.0);
        let mut out = vec![Section::new(
            "",
            vec![vec![
                Cell::text(t("Preset:"), 60.0),
                Cell::Pick { pick: 0, width: 180.0 },
                Cell::text(t("Save as:"), 60.0),
                at("save_as", t("Preset: Name").to_string(), "", 120.0),
                Cell::button("save_preset", 0, t("Save Preset"), true),
                Cell::button("delete_preset", 0, t("Delete Preset"), !self.preset.is_empty()),
            ]],
        )];
        let mut cover = Vec::new();
        for (key, text) in COVER {
            // The first field is the cover, the others its faces; its label has a colon so no two nodes carry its name.
            let (name, shown) = if key == "cover" {
                (t(text).to_string(), tf("{}:", &[t(text)]))
            } else {
                (format!("{}: {}", t("Cover"), t(text)), t(text).to_string())
            };
            cover.push(Cell::text(shown, 70.0));
            cover.push(at(key, name, hint, 64.0));
        }
        out.push(Section::new("", vec![cover]));
        for g in self.form.groups() {
            let mut rows = Vec::new();
            for (n, chunk) in g.fields.chunks(3).enumerate() {
                let mut row = vec![if n == 0 { Cell::head(t(g.name), 120.0) } else { Cell::text("", 120.0) }];
                for f in chunk {
                    row.push(label(f.label));
                    row.push(at(f.path, format!("{}: {}", t(g.name), t(f.label)), if f.read == Read::Length { hint } else { "" }, 64.0));
                }
                rows.push(row);
            }
            out.push(Section::new("", rows));
        }
        out.push(Section::new(
            "",
            vec![vec![
                Cell::head(t("Steel"), 120.0),
                label("Grade"),
                at("grade", t("Steel: Grade").to_string(), "CA-50", 64.0),
                label("Stirrup grade"),
                at("grade_stirrup", t("Steel: Stirrup grade").to_string(), "", 64.0),
                label("Clearance"),
                at("clearance", t("Steel: Clearance").to_string(), hint, 64.0),
            ]],
        ));
        let en = self.text("check") == EN1992;
        let mut check = vec![Cell::head(t("Check"), 120.0), label("Standard"), Cell::Pick { pick: 1, width: 130.0 }];
        if en {
            check.extend([label("Exposure class"), at("exposure", t("Check: Exposure class").to_string(), "XC1", 56.0)]);
        } else {
            check
                .extend([label("Aggressiveness class"), at("aggressiveness", t("Check: Aggressiveness class").to_string(), "1 - 4", 56.0)]);
        }
        check.extend([label("fck (MPa)"), at("fck", t("Check: fck").to_string(), "", 56.0)]);
        if en {
            check.extend([label("fyk (MPa)"), at("fyk", t("Check: fyk").to_string(), "", 56.0)]);
        }
        out.push(Section::new("", vec![check]));
        out
    }

    fn edit(&self, i: usize) -> Option<&TextEdit> {
        self.edits.get(i)
    }

    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit> {
        self.edits.get_mut(i)
    }

    fn choice(&self, pick: usize) -> Option<Choice> {
        match pick {
            0 => Some(Choice {
                name: t("Preset:").to_string(),
                options: self.names.clone(),
                current: self.names.iter().position(|n| *n == self.preset),
            }),
            1 => {
                let now = self.text("check");
                let options = vec![t("None").to_string(), NBR6118.to_string(), EN1992.to_string()];
                let current = [String::new(), NBR6118.to_string(), EN1992.to_string()].iter().position(|n| *n == now);
                Some(Choice { name: t("Standard").to_string(), options, current })
            }
            _ => None,
        }
    }

    fn choose(&mut self, pick: usize, option: usize, host: &mut dyn Host) {
        if pick == 1 {
            let text = [String::new(), NBR6118.to_string(), EN1992.to_string()].get(option).cloned().unwrap_or_default();
            self.put("check", text);
        } else if let Some(name) = self.names.get(option).cloned() {
            // A preset chosen fills the fields.
            let m = host.model();
            let params = m.reinforcement.get(&name).map(|p| p.params.clone());
            if let Some(p) = params {
                self.load(m, &p);
            }
            (self.preset, self.error) = (name, None);
        }
    }

    fn press(&mut self, action: &str, _arg: usize, host: &mut dyn Host) -> Outcome {
        match action {
            "cancel" => return Outcome::Close,
            "ok" => return self.ok(host),
            "remove" => {
                if host.execute("delete", &json!({"ids": self.made})).is_ok() {
                    host.say(tf("Removed {} bars.", &[&self.made.len().to_string()]));
                }
                return Outcome::Close;
            }
            "save_preset" => self.save(host),
            "delete_preset" => {
                if host.execute("reinforcement_preset", &json!({"name": self.preset, "delete": true})).is_ok() {
                    self.preset.clear();
                    self.refresh(host.model());
                }
            }
            _ => {}
        }
        Outcome::Keep
    }

    fn foot(&self) -> Vec<Foot> {
        let mut out = Vec::new();
        if !self.made.is_empty() {
            out.push(Foot::new("remove", t("Remove Bars"), false));
        }
        out.push(Foot::new("cancel", t("Cancel"), false));
        out.push(Foot::new("ok", t("OK"), true));
        out
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests;
