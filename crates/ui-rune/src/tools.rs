//! The Tools palette's table: BIMCraft's own profile has no ribbon, a column of tools by discipline. Data, no widget. This is the
//! piece of the migration plan that goes to the core first (docs/areas/rune-ui.md, "what stays shared"): a table of tabs, panels and
//! tools with their command ids and shortcuts that both front ends draw. Here it is a short table of the first tools, kept by id.

/// (section, tools) in the order the palette shows them; a tool is its id and its English label (translated by `lang::t`).
pub const TOOLS: &[(&str, &[(&str, &str)])] = &[
    (
        "Architecture",
        &[
            ("wall", "Wall"),
            ("curtain_wall", "Curtain Wall"),
            ("door", "Door"),
            ("window", "Window"),
            ("column", "Column"),
            ("component", "Component"),
            ("roof", "Roof"),
            ("floor", "Floor"),
            ("ceiling", "Ceiling"),
            ("stair", "Stair"),
            ("ramp", "Ramp"),
            ("railing", "Railing"),
            ("room", "Room"),
        ],
    ),
    (
        "Structure",
        &[("beam", "Beam"), ("structural_column", "Structural Column"), ("footing", "Footing"), ("strip_footing", "Strip Footing")],
    ),
    ("Plumbing", &[("pipe", "Pipe"), ("plumbing_fixture", "Plumbing Fixture"), ("drain", "Drain")]),
    (
        "Electrical",
        &[
            ("conduit", "Conduit"),
            ("outlet", "Outlet"),
            ("switch", "Switch"),
            ("lighting", "Lighting Fixture"),
            ("cable_tray", "Cable Tray"),
        ],
    ),
];

/// The Modify group: the tools that act on what is there, which a ribbon shows beside the tab's own.
pub const MODIFY: &[(&str, &str)] = &[
    ("move", "Move"),
    ("copy", "Copy"),
    ("rotate", "Rotate"),
    ("mirror", "Mirror"),
    ("array", "Array"),
    ("scale", "Scale"),
    ("trim", "Trim"),
    ("split", "Split"),
    ("align", "Align"),
    ("offset", "Offset"),
];

fn every() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    TOOLS.iter().flat_map(|(_, tools)| tools.iter()).chain(MODIFY.iter())
}

/// The English label of the tool with this id.
pub fn label(id: &str) -> Option<&'static str> {
    every().find(|(tool, _)| *tool == id).map(|(_, label)| *label)
}

/// The id a tool is picked by, for an id that is in the table (a tool that is not has no entry, so a stale id picks nothing).
pub fn id_of(id: &str) -> Option<&'static str> {
    every().find(|(tool, _)| *tool == id).map(|(tool, _)| *tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_unique_id_and_a_label() {
        let mut ids: Vec<&str> = every().map(|(id, _)| *id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "ids are unique");
        assert!(n >= 20);
        assert!(TOOLS.iter().flat_map(|(_, t)| t.iter()).all(|(id, label)| !id.is_empty() && !label.is_empty()));
    }

    #[test]
    fn a_tool_is_found_by_its_id_and_a_stale_id_finds_none() {
        assert_eq!(label("wall"), Some("Wall"));
        assert_eq!(label("move"), Some("Move"), "the Modify group is in the table too");
        assert_eq!(id_of("curtain_wall"), Some("curtain_wall"));
        assert_eq!(label("no such tool"), None);
        assert_eq!(id_of(""), None);
    }
}
