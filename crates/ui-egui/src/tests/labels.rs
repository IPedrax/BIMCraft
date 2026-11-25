//! Every name the window shows of a command or a tool reaches the translators: the Undo list names a step by its command's
//! label (`step_name`), the tools by their names in `shortcuts::ACTIONS`, so each must be a message of the interface's
//! template, with no context, for `lang::t` to find its translation.

use super::*;

/// The template's messages that have no context, as `lang::t` looks them up.
fn template() -> std::collections::HashSet<String> {
    let pot = include_str!("../../../../translations/ui/bimcraft-ui.pot");
    let mut out = std::collections::HashSet::new();
    let mut context = false;
    for line in pot.lines() {
        if line.starts_with("msgctxt ") {
            context = true;
        } else if let Some(id) = line.strip_prefix("msgid \"").and_then(|l| l.strip_suffix('"')) {
            if !context {
                out.insert(id.replace("\\\"", "\""));
            }
            context = false;
        }
    }
    out
}

#[test]
fn every_command_and_tool_name_the_window_shows_is_in_the_template() {
    let messages = template();
    let commands: Vec<&str> = bimcraft_core::engine::commands().iter().map(|c| c.label).filter(|l| !messages.contains(*l)).collect();
    assert!(commands.is_empty(), "commands whose label no translator sees: {commands:?}");
    let tools: Vec<&str> = crate::shortcuts::ACTIONS.iter().copied().filter(|a| !messages.contains(*a)).collect();
    assert!(tools.is_empty(), "tools whose name no translator sees: {tools:?}");
}

#[test]
fn the_room_separator_tool_and_its_undo_step_are_in_italian() {
    let mut h = app(BOX);
    h.state_mut().lang = crate::lang::Lang::Italian;
    // Wide enough for the Architecture tab's every panel: a narrower window folds some into their panel's list.
    h.set_size(vec2(1800.0, 900.0));
    h.run();
    press(&mut h, "Architettura");
    assert!(h.query_by_label("Separatore di zona").is_some(), "the tool's button, in Italian");
    let s = &mut h.state_mut().session;
    s.execute("room_separator", &json!({"points": [[1000, 1000], [3000, 1000]]})).unwrap();
    h.run();
    let step = h.state().session.steps(false).first().map(|s| crate::step_name(s)).unwrap_or_default();
    assert!(step.contains("eparator") && step.contains("zona"), "Undo names the step in Italian: {step}");
}
