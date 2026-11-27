//! The desktop's shortcuts, Revit's two letters and the chords beside them, as data any front end matches what is typed against. They
//! were the desktop's alone (`BimApp`'s `SHORTCUTS` in the egui front end, `EXTRA_SEQUENCES` and `DEFAULT_CHORDS` in its `shortcuts.rs`,
//! the MEP fixtures' letters); a second front end needs the same table, so it is here, and a test reads the egui sources and fails when
//! the two differ, so a front end cannot have a shortcut the other lacks (the parity rule of docs/areas/rune-ui.md).
//!
//! A sequence is two typed letters or digits, in any case. What one *does* is the front end's: this table says only which action it
//! names, either a core command (by id: a tool, a form, an immediate command) or one of the desktop's own, by its English label
//! (`Zoom to Fit`, `3D View`, `Snap to Endpoints`). The commands' own aliases (`CR` for Section) are in `commands::ALL`; [`table`] is
//! them and the lists below together.

use std::sync::OnceLock;

use crate::commands::ALL;

/// The desktop's own two-letter shortcuts and what they are called. A label that is a command's label is that command.
pub const DESKTOP: &[(&str, &str)] = &[
    ("3D", "3D View"),
    ("MD", "Modify"),
    ("SB", "Floor: Structural"),
    ("RF", "Reinforce"),
    ("MM", "Mirror Pick Axis"),
    ("DM", "Mirror Draw Axis"),
    ("AL", "Align"),
    ("OF", "Offset"),
    ("DW", "Demolish"),
    ("ZF", "Zoom to Fit"),
    ("ZE", "Zoom to Fit"),
    ("ZA", "Zoom to Fit"),
    ("TL", "Thin Lines"),
    ("CS", "Create Similar"),
    ("PP", "Properties"),
    ("VP", "Properties"),
    ("NP", "Project Browser"),
    ("SA", "Select All Instances"),
    ("AG", "Alignment Guides"),
    ("OG", "Orthogonal Guides"),
    ("SE", "Snap to Endpoints"),
    ("SM", "Snap to Midpoints"),
    ("SI", "Snap to Intersections"),
    ("SC", "Snap to Centres"),
    ("SP", "Snap to Perpendiculars"),
    ("SQ", "Snap to Quadrants"),
    ("ST", "Snap to Tangents"),
    ("SN", "Snap to Nearest"),
    ("SO", "Snaps Off"),
    ("SS", "Turn Snap Override Off"),
    ("VV", "Visibility/Graphics"),
    ("VG", "Visibility/Graphics"),
    ("EH", "Hide Element"),
    ("VH", "Hide Category"),
    ("HH", "Temporary Hide Element"),
    ("HC", "Temporary Hide Category"),
    ("HI", "Temporary Isolate Element"),
    ("IC", "Temporary Isolate Category"),
    ("HR", "Reset Temporary Hide/Isolate"),
    ("RH", "Reveal Hidden Elements"),
    ("EU", "Unhide Elements"),
    ("UE", "Unhide Elements"),
    ("VU", "Unhide Category"),
    ("EO", "Override Graphics by Element"),
    ("FT", "Filters"),
    ("VT", "View Templates"),
    ("PH", "Phasing"),
    ("DO", "Design Options"),
    ("WF", "Wireframe"),
    ("HL", "Hidden Line"),
    ("SD", "Shaded"),
    ("KS", "Keyboard Shortcuts"),
    // Revit has none for Manage > Location; BIMCraft's window of it.
    ("LO", "Location"),
    ("AI", "Assistant"),
    ("CW", "Co-Work"),
    ("BX", "Section Box"),
    ("GD", "Graphic Display Options"),
    // Revit's fly has none; the 3D view's fly mode.
    ("FY", "Fly Mode"),
    // Revit has none for its title blocks; no command or tool takes TB.
    ("TB", "Title Blocks"),
    // Revit has no building codes, and none of these for its stairs; letters it doesn't use.
    ("BC", "Building Codes"),
    ("CK", "Code Check"),
    ("TS", "Convert to Sketch"),
    ("SY", "Separate Storeys"),
    // Revit has no PyAddins: letters free in both, Reload as Load Scripts and Stop as eXit Script.
    ("PY", "Player"),
    ("RS", "Run Script"),
    ("LS", "Reload"),
    ("NS", "New Script"),
    ("XS", "Stop"),
    // Revit has no Classification Manager; CI is free there and here.
    ("CI", "Classification Manager"),
    // Revit has no Underlay Settings of this kind (its Underlay is a plan's range of levels); UD is free there and here.
    ("UD", "Underlay Settings"),
];

/// Two-letter bindings the Revit-style table does not have but the command catalog names.
pub const EXTRA: &[(&str, &str)] = &[
    ("DU", "Duct"),
    ("FI", "Isolated Footing"),
    ("FW", "Wall Footing"),
    ("BM", "Beam"),
    ("PI", "Pipe"),
    ("CN", "Conduit"),
    ("CT", "Cable Tray"),
    ("DC", "Down Conductor"),
    ("GC", "Earthing Conductor"),
    ("CB", "Cabling"),
    ("DL", "Detail Line"),
    ("FR", "Filled Region"),
    ("GP", "Create Group"),
    ("UG", "Ungroup"),
    ("LW", "Linework"),
    ("PT", "Paint"),
    ("PJ", "Join into Polyline"),
    ("EX", "Explode Polyline"),
    ("DE", "Delete"),
    ("UN", "Project Units"),
    ("GD", "Graphic Display Options"),
    ("PU", "Purge Unused"),
    ("LK", "Link Model"),
    ("DP", "Duplicate View"),
    ("CR", "Section"),
    ("TG", "Tag by Category"),
    ("MA", "Match Properties"),
];

/// The MEP fixtures' letters, and what each places.
pub const FIXTURES: &[(&str, &str)] = &[
    ("PX", "Plumbing Fixture"),
    ("PF", "Pipe Fitting"),
    ("PA", "Pipe Accessory"),
    ("EE", "Electrical Equipment"),
    ("DV", "Device"),
    ("LF", "Lighting Fixture"),
    ("AT", "Air Terminal"),
    ("EG", "Earthing Electrode"),
    ("IB", "Inspection Box"),
    ("DT", "Data Outlet"),
    ("VO", "Voice Outlet"),
    ("RK", "Rack"),
    ("PB", "Patch Panel"),
];

/// The chords the BIMCraft profile starts with, by the action each runs. Ctrl is the platform's primary modifier (Cmd on a Mac).
pub const CHORDS: &[(&str, &[&str])] = &[
    ("cmd.save", &["Ctrl+S"]),
    ("ui.file.save_as", &["Ctrl+Shift+S"]),
    ("cmd.open", &["Ctrl+O"]),
    ("cmd.new", &["Ctrl+N"]),
    ("ui.find", &["Ctrl+K"]),
    ("ui.settings", &["Ctrl+,"]),
    ("ui.settings.shortcuts", &["Ctrl+Shift+K"]),
    ("ui.palette.properties", &["Ctrl+1"]),
    ("ui.palette.browser", &["Ctrl+2"]),
    ("ui.file.export_pdf", &["Ctrl+P"]),
    ("ui.view.home", &["Ctrl+D"]),
    ("cmd.undo", &["Ctrl+Z"]),
    ("cmd.redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("cmd.delete", &["Delete"]),
    ("cmd.copy", &["Ctrl+Shift+D"]),
    ("cmd.rotate", &["Ctrl+R"]),
    ("cmd.mirror", &["Ctrl+M"]),
    ("ui.tool.mirror_draw", &["Ctrl+Shift+M"]),
    ("cmd.offset", &["Ctrl+Shift+O"]),
    ("cmd.corner", &["Ctrl+T"]),
    ("cmd.split", &["Ctrl+Shift+T"]),
    ("cmd.align", &["Ctrl+Shift+A"]),
    ("ui.view.zoom_fit", &["Ctrl+0"]),
    ("ui.view.3d", &["F3"]),
    ("ui.view.plan", &["F2"]),
    ("ui.view.alignment_guides", &["F8"]),
];

/// What a shortcut names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A core command, by id (`wall`, `undo`): a tool, a form or an immediate command, as the front end has it.
    Command(&'static str),
    /// One of the desktop's own actions, by its English label.
    Desktop(&'static str),
}

/// Appends `code` for `target` unless an earlier claimant has it: the order of [`table`] is the precedence.
fn claim(out: &mut Vec<(&'static str, Target)>, code: &'static str, target: Target) {
    if !out.iter().any(|(c, _)| c.eq_ignore_ascii_case(code)) {
        out.push((code, target));
    }
}

/// Labels the desktop does itself though a command has them: `modify` edits fields by id and `select_instances` reads ids, where
/// `Modify` puts the tool down and `Select All Instances` selects by the selection's category and type.
const OWN: &[&str] = &["Modify", "Select All Instances"];

fn built() -> Vec<(&'static str, Target)> {
    let lists: [&[(&'static str, &'static str)]; 3] = [DESKTOP, EXTRA, FIXTURES];
    let mut out = Vec::new();
    // A command's own aliases first, each command followed by the codes the lists give its label, in the order of the command table.
    for c in ALL {
        for &alias in c.aliases {
            claim(&mut out, alias, Target::Command(c.id));
        }
        for list in lists {
            for &(code, label) in list {
                if label == c.label && !OWN.contains(&label) {
                    claim(&mut out, code, Target::Command(c.id));
                }
            }
        }
    }
    // Then the desktop's own actions, which no command has the label of.
    for list in lists {
        for &(code, label) in list {
            claim(&mut out, code, Target::Desktop(label));
        }
    }
    out
}

/// Every two-letter shortcut and what it names: a code once, claimed by the first of the commands' aliases (in the command table's
/// order), the codes of a command's label, and then the desktop's own lists.
pub fn table() -> &'static [(&'static str, Target)] {
    static TABLE: OnceLock<Vec<(&'static str, Target)>> = OnceLock::new();
    TABLE.get_or_init(built)
}

/// What `typed` names, in any case: none for a code that is not a shortcut.
pub fn target(typed: &str) -> Option<Target> {
    table().iter().find(|(code, _)| code.eq_ignore_ascii_case(typed)).map(|(_, target)| *target)
}

/// Whether some shortcut starts with `typed` and is longer, so the next letter may still come. Nothing typed begins nothing.
pub fn begins(typed: &str) -> bool {
    !typed.is_empty() && table().iter().any(|(code, _)| code.len() > typed.len() && code[..typed.len()].eq_ignore_ascii_case(typed))
}

/// The action a chord names (`Ctrl+Z`, `F3`, `Delete`), as an action id: `cmd.<command id>` or `ui.<area>.<action>`. A chord is
/// written as the shortcuts dialog writes it: modifiers in the order Ctrl, Shift, Alt, then the key.
pub fn chord(written: &str) -> Option<&'static str> {
    CHORDS.iter().find(|(_, keys)| keys.iter().any(|k| k.eq_ignore_ascii_case(written))).map(|(action, _)| *action)
}

#[cfg(test)]
mod tests;
