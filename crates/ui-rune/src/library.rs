//! The library managers, the pure part: the named things a project holds that a person edits one at a time (materials, line styles,
//! line and fill patterns, phases and their filters, view filters and templates, text types, project parameters, object styles). Each is
//! a table of the model written as JSON (what a file holds of it), and one command of the core that creates, changes, renames or deletes
//! an entry. What is typed in a field goes to the core as that command with just the fields that differ, as Type Properties does
//! (`typeprops.rs`). Which fields can be typed is the command's own: a scalar of the entry that the command's params doc names; the
//! rest is shown. The dialog is `ui/library.rs`, its state `app/library.rs`.

use bimcraft_core::model::Model;
use serde_json::{Map, Value, json};

use crate::forms::Form;
use crate::props::{self, Kind};
use crate::typeprops;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Materials,
    LineStyles,
    LinePatterns,
    FillPatterns,
    Phases,
    PhaseFilters,
    ViewFilters,
    ViewTemplates,
    TextTypes,
    Parameters,
    ObjectStyles,
    /// The project's named sites (Revit's Location, `LO`): the Survey Point's shared coordinates, true north, map system.
    Locations,
    /// The classification tables the project holds (Uniclass, OmniClass, a table of the user's own), loaded from the user's files.
    Classification,
    /// The project's own title blocks, which a sheet names to be drawn with (Revit's title block families).
    TitleBlocks,
    /// The Publisher's presets: where a set is written and the print sets in its folders, each as a format.
    PublishSets,
}

/// How the command takes an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `{name, field, …}`, with `rename` and `delete` where the command has them.
    Named,
    /// `{categories: {name: {field, …}}}`, which is how Object Styles takes a category's pens.
    Categories,
}

pub struct Info {
    pub source: Source,
    /// The manager's name, in English (the dialog's title; `lang::t` translates it).
    pub title: &'static str,
    /// The command that makes and changes an entry.
    pub command: &'static str,
    pub shape: Shape,
}

pub static ALL: [Info; 15] = [
    Info { source: Source::Materials, title: "Materials", command: "material", shape: Shape::Named },
    Info { source: Source::LineStyles, title: "Line Styles", command: "line_style", shape: Shape::Named },
    Info { source: Source::LinePatterns, title: "Line Patterns", command: "line_pattern", shape: Shape::Named },
    Info { source: Source::FillPatterns, title: "Fill Patterns", command: "fill_pattern", shape: Shape::Named },
    Info { source: Source::Phases, title: "Phases", command: "phase", shape: Shape::Named },
    Info { source: Source::PhaseFilters, title: "Phase Filters", command: "phase_filter", shape: Shape::Named },
    Info { source: Source::ViewFilters, title: "View Filters", command: "filter", shape: Shape::Named },
    Info { source: Source::ViewTemplates, title: "View Templates", command: "template", shape: Shape::Named },
    Info { source: Source::TextTypes, title: "Text Types", command: "text_type", shape: Shape::Named },
    Info { source: Source::Parameters, title: "Project Parameters", command: "parameter", shape: Shape::Named },
    Info { source: Source::ObjectStyles, title: "Object Styles", command: "object_styles", shape: Shape::Categories },
    Info { source: Source::Locations, title: "Locations", command: "location", shape: Shape::Named },
    Info { source: Source::Classification, title: "Classification", command: "classification_table", shape: Shape::Named },
    Info { source: Source::TitleBlocks, title: "Title Blocks", command: "title_block", shape: Shape::Named },
    Info { source: Source::PublishSets, title: "Publisher", command: "publish_set", shape: Shape::Named },
];

impl Source {
    pub fn info(self) -> &'static Info {
        ALL.iter().find(|i| i.source == self).unwrap_or(&ALL[0])
    }

    /// Other commands of the core the manager's dialog opens the forms of: the project's points, a point's coordinates, true north and
    /// the map system, which the `location` command alone does not set.
    pub fn also(self) -> &'static [&'static str] {
        match self {
            Source::Locations => &["coordinates", "specify_coordinates", "rotate_project_north", "georeference", "convert_coordinates"],
            Source::Classification => &[
                "classification_systems",
                "classification_parameters",
                "classify",
                "classification_check",
                "classification_brief",
                "classification_answer",
            ],
            Source::TitleBlocks => &["title_values"],
            Source::PublishSets => &["publish", "print_set", "export_formats"],
            _ => &[],
        }
    }
}

/// A table of the model as its entries: the name and what a file writes of it. An entry a file writes as a list (a line pattern's
/// dashes) is the object `{"value": [..]}`, so the dialog shows it as a row like any other.
macro_rules! table {
    ($map:expr) => {
        match serde_json::to_value(&$map) {
            Ok(Value::Object(map)) => map
                .into_iter()
                .map(|(name, v)| if v.is_object() { (name, v) } else { (name, json!({ "value": v })) })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        }
    };
}

/// Every entry of the source in the project, by name.
pub fn entries(model: &Model, source: Source) -> Vec<(String, Value)> {
    match source {
        Source::Materials => table!(*model.materials),
        Source::LineStyles => table!(model.line_styles),
        Source::LinePatterns => table!(model.line_patterns),
        Source::FillPatterns => table!(model.fill_patterns),
        Source::Phases => model.phases.iter().map(|name| (name.clone(), json!({}))).collect(),
        Source::PhaseFilters => table!(model.phase_filters),
        Source::ViewFilters => table!(model.view_filters),
        Source::ViewTemplates => table!(model.view_templates),
        Source::TextTypes => table!(model.text_types),
        Source::Parameters => table!(model.parameters),
        Source::ObjectStyles => table!(model.object_styles),
        Source::TitleBlocks => table!(model.title_blocks),
        Source::PublishSets => table!(model.publish_sets),
        Source::Classification => model
            .classification
            .tables
            .iter()
            .map(|(name, t)| {
                let rows = json!({"system": t.system, "key": t.key, "edition": t.edition, "source": t.source, "rows": t.items.len()});
                (name.clone(), rows)
            })
            .collect(),
        Source::Locations => {
            let co = &model.coordinates;
            co.names()
                .into_iter()
                .filter_map(|name| {
                    let site = model.site_named(name)?;
                    let at = site.location(co.survey);
                    let fields =
                        json!({"east": at.east, "north": at.north, "elevation": at.elevation, "angle": at.angle, "scale": at.scale});
                    let Value::Object(mut fields) = fields else { return None };
                    fields.insert("crs".into(), json!(site.crs));
                    fields.insert("active".into(), json!(name == co.active));
                    Some((name.to_string(), Value::Object(fields)))
                })
                .collect()
        }
    }
}

/// Whether the source's command takes a field of this name (`rename`, `delete`).
pub fn takes(info: &Info, field: &str) -> bool {
    info.shape == Shape::Named && Form::named(info.command).is_some_and(|f| f.fields.iter().any(|x| x.name == field))
}

/// The fields of an entry that can be typed: its scalars (`props::editable`) that the command's params doc names. An object style's
/// are all its scalars, since its command names the category and not the pens.
pub fn editable(info: &Info, json: &Value) -> Vec<(String, Kind, String)> {
    // A loaded table is read again from its file, with the command's form, not edited field by field.
    if info.source == Source::Classification {
        return Vec::new();
    }
    let all = props::editable(json);
    match info.shape {
        Shape::Categories => all,
        Shape::Named => {
            let allowed: Vec<String> =
                Form::named(info.command).map(|f| f.fields.into_iter().map(|x| x.name).collect()).unwrap_or_default();
            all.into_iter().filter(|(name, _, _)| allowed.contains(name)).collect()
        }
    }
}

/// The command's parameters for the entry `name` with the fields typed (and a new name, when `rename` is given and the command takes
/// one): just what differs. An error says which field could not be read. None when nothing is different.
pub fn changes(info: &Info, name: &str, original: &Value, typed: &[(String, Kind, String)], rename: &str) -> Result<Option<Value>, String> {
    let mut fields: Map<String, Value> = typeprops::differing(original, typed)?;
    let rename = rename.trim();
    if !rename.is_empty() {
        if !takes(info, "rename") {
            return Err(format!("{} cannot be renamed here", info.title));
        }
        if rename.chars().count() > crate::forms::MAX_TEXT {
            return Err("rename: too long".into());
        }
        fields.insert("rename".into(), Value::String(rename.to_string()));
    }
    if fields.is_empty() {
        return Ok(None);
    }
    Ok(Some(match info.shape {
        Shape::Named => {
            fields.insert("name".into(), Value::String(name.to_string()));
            Value::Object(fields)
        }
        Shape::Categories => json!({"categories": {name: Value::Object(fields)}}),
    }))
}

/// Whether the source's command takes an entry away: `delete`, or `remove` (a classification table).
pub fn deletes(info: &Info) -> bool {
    ["delete", "remove"].iter().any(|k| takes(info, k))
}

/// The command's parameters that delete the entry, when its command can.
pub fn deletion(info: &Info, name: &str) -> Option<Value> {
    let key: &str = ["delete", "remove"].into_iter().find(|k| takes(info, k))?;
    Some(json!({"name": name, key: true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;

    fn session() -> Session {
        let mut s = Session::new();
        for (command, params) in [
            ("material", json!({"name": "Rune Mat", "color": "#cc3333"})),
            ("line_style", json!({"name": "Rune Style", "pen": 3})),
            ("phase", json!({"name": "Rune Phase"})),
            ("text_type", json!({"name": "Rune Text", "size": 3})),
            ("parameter", json!({"name": "Rune Param", "kind": "text", "categories": ["wall"]})),
        ] {
            let made = s.execute(command, &params);
            assert!(made.is_ok(), "{command}: {made:?}");
        }
        s
    }

    #[test]
    fn what_a_command_made_is_an_entry_of_its_source_and_every_source_reads_on_any_project() {
        let s = session();
        let has = |source: Source, name: &str| entries(&s.model, source).iter().any(|(n, _)| n == name);
        assert!(has(Source::Materials, "Rune Mat") && has(Source::LineStyles, "Rune Style") && has(Source::Phases, "Rune Phase"));
        assert!(has(Source::TextTypes, "Rune Text") && has(Source::Parameters, "Rune Param"));
        // A pattern a file writes as a list is an object with that list as its `value`, so it has a row to show.
        let patterns = entries(&s.model, Source::LinePatterns);
        assert!(!patterns.is_empty(), "a new project has line patterns");
        assert!(patterns.iter().all(|(_, json)| json.get("value").is_some_and(Value::is_array)), "{patterns:?}");
        let house = crate::app::App::sample();
        for info in &ALL {
            assert_eq!(info.source.info().command, info.command, "{}: a source finds its own info", info.title);
            for model in [&*s.model, &*house.session.model] {
                assert!(entries(model, info.source).iter().all(|(name, json)| !name.is_empty() && json.is_object()), "{}", info.title);
            }
            assert!(Form::named(info.command).is_some(), "{}: the command is in the core's table", info.title);
        }
    }

    #[test]
    fn the_commands_a_manager_offers_beside_its_own_are_in_the_core_and_the_locations_are_the_sites_of_the_project() {
        for info in &ALL {
            for id in info.source.also() {
                assert!(Form::named(id).is_some(), "{}: {id} is a command of the core", info.title);
            }
        }
        let s = session();
        let sites = entries(&s.model, Source::Locations);
        assert!(sites.iter().any(|(name, json)| *name == s.model.coordinates.active && json["active"] == json!(true)), "{sites:?}");
        assert!(sites.iter().all(|(_, json)| ["east", "north", "elevation", "angle", "scale"].iter().all(|k| json[k].is_number())));
        assert!(takes(Source::Locations.info(), "activate") && takes(Source::Locations.info(), "delete"));
        let tables = Source::Classification.info();
        assert!(deletes(tables) && !takes(tables, "delete"), "a table is taken out with `remove`");
        assert_eq!(deletion(tables, "T"), Some(json!({"name": "T", "remove": true})));
    }

    #[test]
    fn the_fields_that_can_be_typed_are_the_commands_own() {
        let s = session();
        let text = &ALL[8];
        let (_, json) = entries(&s.model, Source::TextTypes).into_iter().find(|(n, _)| n == "Rune Text").expect("the text type");
        let fields = editable(text, &json);
        assert!(fields.iter().any(|(name, kind, _)| name == "size" && *kind == Kind::Number), "{fields:?}");
        assert!(
            editable(text, &json!({"size": 3, "nothing_the_command_takes": 1}))
                .iter()
                .all(|(name, _, _)| name != "nothing_the_command_takes")
        );
        // An object style's pens are not the command's own names: all its scalars are offered.
        assert_eq!(editable(&ALL[10], &json!({"projection": 2, "cut": 4})).len(), 2);
    }

    #[test]
    fn only_what_differs_is_sent_in_the_shape_the_command_takes() {
        let text = &ALL[8];
        let original = json!({"size": 3.0, "font": "sans"});
        let typed =
            |size: &str| vec![("size".to_string(), Kind::Number, size.to_string()), ("font".to_string(), Kind::Text, "sans".to_string())];
        assert_eq!(changes(text, "T", &original, &typed("3"), ""), Ok(None));
        assert_eq!(changes(text, "T", &original, &typed("5"), "  "), Ok(Some(json!({"name": "T", "size": 5}))));
        assert_eq!(
            changes(text, "T", &original, &typed("3"), "U"),
            Ok(Some(json!({"name": "T", "rename": "U"}))),
            "a new name alone is a change"
        );
        assert!(changes(text, "T", &original, &typed("x"), "").is_err_and(|e| e.starts_with("size: ")));
        let styles = &ALL[10];
        let pen = vec![("cut".to_string(), Kind::Number, "6".to_string())];
        assert_eq!(changes(styles, "wall", &json!({"cut": 4}), &pen, ""), Ok(Some(json!({"categories": {"wall": {"cut": 6}}}))));
        assert!(changes(styles, "wall", &json!({"cut": 4}), &pen, "x").is_err(), "an object style is not renamed");
        assert_eq!(deletion(text, "T"), takes(text, "delete").then(|| json!({"name": "T", "delete": true})));
        assert_eq!(deletion(styles, "wall"), None, "an object style is not deleted");
    }
}
