//! How a saved layout is read: the window's own (`Policy::Live`) and a preset's (`Policy::Preset`), and the names of palettes a
//! build has no `Palette` for, which must survive a read and a write.

use super::*;
use crate::dock::{Docking, Palette, Policy, Side};
use crate::presets::presets_from_json;

/// A layout naming `Properties`, with `closed` as given and nothing else: the shape of a settings file's `palettes`.
fn saved(closed: Value) -> Value {
    json!({
        "left": [{"tabs": ["Properties"], "active": 0}],
        "right": [],
        "bottom": [],
        "floats": [],
        "closed": closed,
        "home": {},
    })
}

#[test]
fn the_live_layout_still_opens_a_palette_it_does_not_know() {
    // The window's own file: what it never named opens, so a palette added by a newer version still shows.
    let d = Docking::from_json(&saved(json!([])), Policy::Live, &Palette::known());
    assert!(d.shown(Palette::Tools), "the palette the file never named opens");
    assert!(d.shown(Palette::Browser));
    // Script Automation starts closed, as it did, and the Assistant opens only when asked for.
    assert!(!d.shown(Palette::Scripts));
    assert!(!d.shown(Palette::Assistant));
    // A preset keeps exactly what it was saved with: the same file leaves the rest closed.
    let d = Docking::from_json(&saved(json!([])), Policy::Preset, &Palette::known());
    assert!(d.shown(Palette::Properties));
    for p in [Palette::Browser, Palette::Tools, Palette::Assistant, Palette::Scripts] {
        assert!(!d.shown(p), "{p:?} was not in the preset");
    }
}

#[test]
fn a_preset_keeps_only_the_palettes_it_was_saved_with() {
    let preset = json!([{
        "name": "Documentation",
        "palettes": saved(json!(["Assistant"])),
    }]);
    let kept = presets_from_json(&preset);
    let p = &kept[0];
    assert!(p.docking.shown(Palette::Properties));
    assert!(!p.docking.shown(Palette::Assistant), "closed stays closed");
    assert!(!p.docking.shown(Palette::Scripts), "a palette the preset never named stays closed");
}

#[test]
fn a_palette_added_later_stays_closed_in_an_old_preset() {
    // A preset from before Script Automation, or before a plugin's palette: only what it named shows.
    let preset = json!([{"name": "Old", "palettes": saved(json!([]))}]);
    let kept = presets_from_json(&preset);
    let p = &kept[0];
    for palette in [Palette::Tools, Palette::Assistant, Palette::Scripts] {
        assert!(!p.docking.shown(palette), "{palette:?} came with a later version, not with this preset");
    }
    // The window's own layout opens it, so the palette is not lost from the user's window.
    let live = Docking::from_json(&saved(json!([])), Policy::Live, &Palette::known());
    assert!(live.shown(Palette::Tools));
}

#[test]
fn an_unknown_palette_name_survives_a_read_write_round_trip() {
    for policy in [Policy::Live, Policy::Preset] {
        let file = saved(json!(["plugin:rooms:areas"]));
        let d = Docking::from_json(&file, policy, &Palette::known());
        assert_eq!(d.unknown, ["plugin:rooms:areas"], "{policy:?}: a name this build has no palette for is kept");
        let written = d.to_json(&Palette::known());
        let closed: Vec<&str> = written["closed"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(closed.contains(&"plugin:rooms:areas"), "{policy:?}: and written back in closed");
        let again = Docking::from_json(&written, policy, &Palette::known());
        assert_eq!(again.unknown, d.unknown, "{policy:?}: the second read keeps it too");
    }
}

#[test]
fn every_palette_the_ui_shows_comes_from_known() {
    let known = Palette::known();
    for palette in [Palette::Properties, Palette::Browser, Palette::Tools, Palette::Assistant, Palette::Scripts] {
        assert!(known.contains(&palette), "{palette:?} is a palette of every build");
        assert_eq!(Palette::named(palette.name(), &known), Some(palette));
    }
    // A plugin's palette joins the list with the registry, and its name is `plugin:<plugin id>:<panel id>`.
    assert_eq!(Palette::named("plugin:rooms:areas", &known), None, "no plugin is loaded in this test");
    // Docked or floated, a palette is the same one: the layout's names all come back to known.
    let d = Docking::revit();
    assert_eq!(d.docked(Palette::Properties), Some(Side::Left));
}
