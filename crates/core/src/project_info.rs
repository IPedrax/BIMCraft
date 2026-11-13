//! The project's information, as Revit's Project Information: an element of its own (category `project`), one in a project
//! at most, made by the first `project_info` that sets something. It holds the fields a title block and an IFC file name the
//! project by (its number, client, address, status, issue date, author, organization and building), and the values of the
//! project parameters for the `project` category, as any element holds its parameters. The project's name stays
//! `Model::name`, which `project_info` sets too.

use std::collections::BTreeMap;

use bimcraft_i18n::ui_msg;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::bad;
use crate::engine::Session;
use crate::model::{Element, Id, MAX_TEXT, Model};

/// The project's category, what parameters for it name.
pub const CATEGORY: &str = "project";

/// The longest an address is, in characters: a few lines.
pub const MAX_ADDRESS: usize = 4 * MAX_TEXT;

/// The project's information. Every field is text, empty when not set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client: String,
    /// Its lines, by `\n`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// As people write it: Revit's is text too.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue_date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub organization: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub organization_description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub building: String,
    /// Its values of the project's parameters for the project.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

/// Its fields: the key commands and files use, and the name Revit and IFC files give it (what a title block and a property
/// set say).
pub const FIELDS: [(&str, &str); 9] = [
    ("number", ui_msg!("Project Number")),
    ("client", ui_msg!("Client Name")),
    ("address", ui_msg!("Project Address")),
    ("status", ui_msg!("Project Status")),
    ("issue_date", ui_msg!("Project Issue Date")),
    ("author", ui_msg!("Author")),
    ("organization", ui_msg!("Organization Name")),
    ("organization_description", ui_msg!("Organization Description")),
    ("building", ui_msg!("Building Name")),
];

/// The title-block key each field of [`FIELDS`] fills: an automatic field of every title block ([`crate::titleblock::AUTO`]).
pub const TITLE_KEYS: [(&str, &str); 9] = [
    ("number", "PROJECT_NUMBER"),
    ("client", "CLIENT"),
    ("address", "PROJECT_ADDRESS"),
    ("status", "PROJECT_STATUS"),
    ("issue_date", "ISSUE_DATE"),
    ("author", "AUTHOR"),
    ("organization", "ORGANIZATION"),
    ("organization_description", "ORGANIZATION_DESCRIPTION"),
    ("building", "BUILDING"),
];

/// The field of [`FIELDS`] title-block key `key` names, in any case.
pub fn field_of_key(key: &str) -> Option<&'static str> {
    TITLE_KEYS.iter().find(|(_, k)| k.eq_ignore_ascii_case(key.trim())).map(|(f, _)| *f)
}

impl ProjectInfo {
    /// Field `key` of [`FIELDS`].
    pub fn get(&self, key: &str) -> Option<&str> {
        Some(match key {
            "number" => &self.number,
            "client" => &self.client,
            "address" => &self.address,
            "status" => &self.status,
            "issue_date" => &self.issue_date,
            "author" => &self.author,
            "organization" => &self.organization,
            "organization_description" => &self.organization_description,
            "building" => &self.building,
            _ => return None,
        })
    }

    /// Field `key` of [`FIELDS`], to change.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut String> {
        Some(match key {
            "number" => &mut self.number,
            "client" => &mut self.client,
            "address" => &mut self.address,
            "status" => &mut self.status,
            "issue_date" => &mut self.issue_date,
            "author" => &mut self.author,
            "organization" => &mut self.organization,
            "organization_description" => &mut self.organization_description,
            "building" => &mut self.building,
            _ => return None,
        })
    }

    /// What's wrong with it, said of `who`: a field too long, or of lines where only the address has them.
    pub fn check(&self, who: &str) -> Result<(), String> {
        for (key, label) in FIELDS {
            let v = self.get(key).unwrap_or("");
            let most = if key == "address" { MAX_ADDRESS } else { MAX_TEXT };
            if v.chars().count() > most {
                return Err(format!("{who}: `{key}` ({label}) holds up to {most} characters"));
            }
            if key != "address" && v.contains(['\n', '\r']) {
                return Err(format!("{who}: `{key}` ({label}) is one line; only the address has lines"));
            }
        }
        Ok(())
    }
}

impl Model {
    /// Field `field` of [`FIELDS`] set to `value`, the project's information made if it has none.
    pub(crate) fn set_info(&mut self, field: &str, value: String) {
        let id = match self.project_info() {
            Some((id, _)) => id,
            None => self.insert(Element::ProjectInfo(ProjectInfo::default())),
        };
        if let Some(Element::ProjectInfo(info)) = self.elements.get_mut(id)
            && let Some(slot) = info.get_mut(field)
        {
            *slot = value;
        }
    }

    /// What the project's information gives title blocks, by key ([`TITLE_KEYS`]): each field, empty where it says nothing.
    pub fn info_title_values(&self) -> BTreeMap<String, String> {
        let info = self.project_info().map(|(_, i)| i);
        TITLE_KEYS.iter().map(|(f, k)| ((*k).to_string(), info.and_then(|i| i.get(f)).unwrap_or("").to_string())).collect()
    }

    /// The project's information element, if it has one: its id and what it holds.
    pub fn project_info(&self) -> Option<(Id, &ProjectInfo)> {
        self.elements.iter().find_map(|(id, e)| match e {
            Element::ProjectInfo(p) => Some((*id, p)),
            _ => None,
        })
    }
}

/// What `info` says, with the project's name and, if it is an element, its id: what `project_info` answers.
fn answer(m: &Model) -> Value {
    let empty = ProjectInfo::default();
    let (id, info) = m.project_info().map_or((None, &empty), |(id, p)| (Some(id), p));
    let mut r = json!({"id": id, "name": m.name, "params": info.params});
    for (key, _) in FIELDS {
        r[key] = json!(info.get(key).unwrap_or(""));
    }
    r
}

/// `project_info`: reads the project's information or sets fields of it, made the first time one is set.
pub(crate) fn project_info(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "project_info";
    let Value::Object(given) = p else { return Err(bad(c, "its params are an object")) };
    let known = |k: &str| k == "name" || k == "params" || FIELDS.iter().any(|(f, _)| *f == k);
    if let Some(k) = given.keys().find(|k| !known(k)) {
        let keys: Vec<&str> = FIELDS.iter().map(|(f, _)| *f).collect();
        return Err(bad(c, format!("`{k}` isn't a field of it; there are name, {}, params", keys.join(", "))));
    }
    let given: Vec<(&String, &Value)> = given.iter().filter(|(_, v)| !v.is_null()).collect();
    if given.is_empty() {
        return Ok(answer(&s.model));
    }
    let mut name = None;
    let mut fields = Vec::new();
    let mut params = None;
    for (k, v) in given {
        match k.as_str() {
            "params" => {
                let Value::Object(o) = v else { return Err(bad(c, "`params` is {parameter: value | null}")) };
                params = Some(o);
            }
            _ => {
                let text = v.as_str().ok_or_else(|| bad(c, format!("`{k}` is text, not {v}")))?;
                if k == "name" {
                    let n = text.trim();
                    if n.is_empty() {
                        return Err(bad(c, "`name`: a project has a name"));
                    }
                    name = Some(n.to_string());
                } else {
                    fields.push((k.as_str(), text.trim().to_string()));
                }
            }
        }
    }
    let m = s.model_mut();
    if let Some(n) = name {
        m.name = n;
    }
    let id = match m.project_info() {
        Some((id, _)) => id,
        None => m.insert(Element::ProjectInfo(ProjectInfo::default())),
    };
    let Some(Element::ProjectInfo(info)) = m.elements.get_mut(id) else { return Err(bad(c, "the project's information is gone")) };
    for (k, v) in fields {
        if let Some(slot) = info.get_mut(k) {
            *slot = v;
        }
    }
    for (k, v) in params.into_iter().flatten() {
        if v.is_null() {
            info.params.remove(k);
        } else {
            info.params.insert(k.clone(), v.clone());
        }
    }
    Ok(answer(m))
}
