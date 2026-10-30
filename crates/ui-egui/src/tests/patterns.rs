//! The Fill Patterns and Line Styles dialogs' drafts: what the commands they send do to a name deleted and made again, to
//! renames that chain or swap, to a project that changed under the draft, and to names left blank or used twice.

use super::*;
use bimcraft_core::fills::HatchLine;

/// Replace the text of the `n`th field named `label` (two rows may carry one name while a draft is being renamed).
fn type_into_nth(h: &mut Harness<BimApp>, label: &str, n: usize, text: &str) {
    use egui_kittest::kittest::{By, NodeT};
    let id = h.query_all(By::new().role(Role::TextInput).label(label)).nth(n).map(|f| f.accesskit_node().id()).unwrap();
    h.get_by(|f| f.id() == id).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|f| f.id() == id).type_text(text);
    h.run();
}

/// Whether something on screen says `text` (the dialog and the status box may both).
fn said(h: &Harness<BimApp>, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

fn fill_patterns(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    press(h, "Fill Patterns");
}

fn patterns_tab(h: &mut Harness<BimApp>) {
    fill_patterns(h);
    press(h, "Patterns");
}

#[test]
fn a_pattern_or_a_type_deleted_and_made_again_under_its_name_is_the_new_one() {
    let mut h = app(BOX);
    patterns_tab(&mut h);
    press(&mut h, "Sand: Delete");
    press(&mut h, "New Pattern");
    type_into(&mut h, "Pattern 1: Name", "Sand");
    type_into(&mut h, "Sand: Angle", "30");
    press(&mut h, "Region Types");
    press(&mut h, "Brick: Delete");
    press(&mut h, "New Type");
    type_into(&mut h, "Region Type 1: Name", "Brick");
    choose(&mut h, "Brick: Foreground", "Tile 300");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!(m.fill_patterns["Sand"].lines, vec![HatchLine::simple(30.0, 2.0)], "the new pattern, not gone with the old");
    assert_eq!(m.region_types["Sand"].foreground, None, "the type that had the old one has none");
    assert_eq!(m.region_types["Brick"].foreground.as_deref(), Some("Tile 300"), "the new type, not gone with the old");
}

#[test]
fn renames_that_chain_swap_or_take_a_deleted_name_are_done_in_an_order_the_core_accepts() {
    let lines = |a: f64| json!([{"angle": a, "spacing": 5.0}]);
    let script = format!(
        "{BOX}\nfill_pattern {{\"name\": \"Aa\", \"lines\": {}}}\nfill_pattern {{\"name\": \"Bb\", \"lines\": {}}}\nfill_pattern {{\"name\": \"Cc\", \"lines\": {}}}\nfilled_region_type {{\"name\": \"Uses Aa\", \"foreground\": \"Aa\"}}",
        lines(10.0),
        lines(20.0),
        lines(30.0)
    );
    let angle = |h: &Harness<BimApp>, n: &str| h.state().session.model.fill_patterns[n].lines[0].angle;
    // A chain: Cc becomes Dd, Bb takes Cc's name, Aa takes Bb's.
    let mut h = app(&script);
    patterns_tab(&mut h);
    type_into(&mut h, "Cc: Name", "Dd");
    type_into(&mut h, "Bb: Name", "Cc");
    type_into(&mut h, "Aa: Name", "Bb");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    assert_eq!((angle(&h, "Bb"), angle(&h, "Cc"), angle(&h, "Dd")), (10.0, 20.0, 30.0));
    assert_eq!(h.state().session.model.region_types["Uses Aa"].foreground.as_deref(), Some("Bb"), "the type follows its pattern");
    // A swap.
    let mut h = app(&script);
    patterns_tab(&mut h);
    type_into(&mut h, "Aa: Name", "Bb");
    type_into_nth(&mut h, "Bb: Name", 1, "Aa");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    assert_eq!((angle(&h, "Aa"), angle(&h, "Bb"), angle(&h, "Cc")), (20.0, 10.0, 30.0));
    assert_eq!(h.state().session.model.region_types["Uses Aa"].foreground.as_deref(), Some("Bb"));
    let names: Vec<&String> = h.state().session.model.fill_patterns.keys().collect();
    assert!(names.iter().all(|n| !n.contains("renaming")), "no spare name is left: {names:?}");
    // A name taken from a pattern deleted in the same draft.
    let mut h = app(&script);
    patterns_tab(&mut h);
    press(&mut h, "Bb: Delete");
    type_into(&mut h, "Aa: Name", "Bb");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!((angle(&h, "Bb"), m.fill_patterns.contains_key("Aa")), (10.0, false));
    assert!(m.fill_patterns.keys().all(|n| !n.contains("deleted")), "{:?}", m.fill_patterns.keys().collect::<Vec<_>>());
    // Types too: two region types swap names.
    let mut h = app(&format!("{script}\nfilled_region_type {{\"name\": \"Tb\", \"foreground\": \"Bb\"}}"));
    fill_patterns(&mut h);
    type_into(&mut h, "Tb: Name", "Uses Aa");
    type_into_nth(&mut h, "Uses Aa: Name", 1, "Tb");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!((m.region_types["Tb"].foreground.as_deref(), m.region_types["Uses Aa"].foreground.as_deref()), (Some("Aa"), Some("Bb")));
}

#[test]
fn apply_is_refused_when_the_project_changed_while_the_draft_was_open() {
    let mut h = app(BOX);
    fill_patterns(&mut h);
    type_into(&mut h, "Crosshatch: Name", "Grid");
    // An agent adds a type the draft never saw.
    h.state_mut().session.execute("filled_region_type", &json!({"name": "Agent Type"})).unwrap();
    h.run();
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some(), "it stays open to say why");
    assert!(said(&h, "changed"), "the dialog says it itself");
    let m = &h.state().session.model;
    assert!(m.region_types.contains_key("Agent Type") && m.region_types.contains_key("Crosshatch"), "nothing of the draft was done");
    // Cancelled and opened again, it is the project as it is.
    press(&mut h, "Cancel");
    fill_patterns(&mut h);
    type_into(&mut h, "Crosshatch: Name", "Grid");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert!(m.region_types.contains_key("Grid") && m.region_types.contains_key("Agent Type"));
}

#[test]
fn blank_and_repeated_names_are_refused_inside_the_dialog_and_nothing_is_done() {
    let mut h = app(BOX);
    patterns_tab(&mut h);
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "New Pattern");
    type_into(&mut h, "Pattern 1: Name", "  ");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some() && said(&h, "Enter a name"));
    type_into(&mut h, ": Name", "Sand");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some() && said(&h, "already in use"));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
}

/// What a frame painted, in the order it was painted, with groups of shapes opened up.
fn painted(h: &Harness<BimApp>) -> Vec<egui::Shape> {
    fn walk(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// Where the Type Properties preview painted its paper, the line across it, the paper over its middle (a type that masks),
/// and the strokes of its patterns: indices into [`painted`].
struct Preview {
    sheet: Option<usize>,
    line: Option<usize>,
    cover: Option<usize>,
    strokes: Vec<usize>,
}

fn preview_order(h: &Harness<BimApp>, paper: egui::Color32) -> Preview {
    let shapes = painted(h);
    let rect = |height: f32| {
        shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.fill == paper && (r.rect.height() - height).abs() < 0.5))
    };
    let (sheet, cover) = (rect(100.0), rect(76.0));
    let bounds = sheet.and_then(|i| match &shapes[i] {
        egui::Shape::Rect(r) => Some(r.rect),
        _ => None,
    });
    let line = shapes
        .iter()
        .enumerate()
        .position(|(i, s)| Some(i) > sheet && matches!(s, egui::Shape::LineSegment { stroke, .. } if stroke.width == 2.0));
    let strokes = (shapes.iter().enumerate())
        .filter(|(_, s)| {
            matches!(s, egui::Shape::LineSegment { points, stroke } if stroke.width == 1.0
                && bounds.is_some_and(|b| points.iter().all(|p| b.contains(*p))))
        })
        .map(|(i, _)| i)
        .collect();
    Preview { sheet, line, cover, strokes }
}

#[test]
fn the_preview_keeps_the_model_line_over_a_fill_that_does_not_mask_and_under_the_paper_of_one_that_does() {
    let mut h = app(BOX);
    shortcut(&mut h, "FR");
    press(&mut h, "Edit Type");
    choose(&mut h, "Foreground Pattern", "Diagonal Up");
    let paper = crate::canvas::color(bimcraft_core::render::PAPER);
    let p = preview_order(&h, paper);
    let (sheet, line) = (p.sheet.expect("the paper"), p.line.expect("the line"));
    assert!(!p.strokes.is_empty() && p.cover.is_none() && line > sheet);
    assert!(p.strokes.iter().all(|s| *s < line), "the model line lies over the fills of a region that doesn't mask");
    press(&mut h, "Masking");
    let p = preview_order(&h, paper);
    let (sheet, line, cover) = (p.sheet.expect("the paper"), p.line.expect("the line"), p.cover.expect("the paper over the middle"));
    assert!(sheet < line && line < cover, "the paper hides the line where the region masks: {sheet} {line} {cover}");
    assert!(p.strokes.iter().all(|s| *s > cover), "and the fills lie over the paper");
}

#[test]
fn the_preview_is_painted_in_the_canvas_colours_in_the_dark_look_too() {
    let mut h = app(BOX);
    h.ctx.set_theme(egui::Theme::Dark);
    h.run();
    shortcut(&mut h, "FR");
    press(&mut h, "Edit Type");
    let dark = crate::canvas::color(bimcraft_core::render::PAPER);
    assert_ne!(dark, egui::Color32::WHITE, "a dark canvas has dark paper");
    assert!(preview_order(&h, dark).sheet.is_some(), "the preview's paper is the canvas's");
    assert!(preview_order(&h, egui::Color32::WHITE).sheet.is_none(), "and not a literal white");
}

#[test]
fn a_crosshatch_that_is_not_square_the_way_angle_plus_90_is_is_left_as_it_is() {
    let script = format!(
        "{BOX}\nfill_pattern {{\"name\": \"Odd\", \"lines\": [{{\"angle\": 90, \"spacing\": 5}}, {{\"angle\": 0, \"spacing\": 5}}]}}"
    );
    let mut h = app(&script);
    let (steps, before) = (h.state().session.history().len(), h.state().session.model.fill_patterns["Odd"].clone());
    patterns_tab(&mut h);
    h.run_steps(3);
    press(&mut h, "OK");
    assert_eq!(h.state().session.history().len(), steps, "looking at it sent no command");
    assert_eq!(h.state().session.model.fill_patterns["Odd"], before);
    // Touching its angle does rewrite it.
    patterns_tab(&mut h);
    type_into(&mut h, "Odd: Angle", "30");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.fill_patterns["Odd"].lines[1].angle, 120.0);
}

/// The button named `label` pressed as a screen reader does: the dialog's rows shift as a long note changes its height, and a
/// pointer click made where the button was a frame ago can miss it.
fn activate(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).click_accesskit();
    h.run();
}

/// The system's file dialog the last button press began, answered with `path`.
fn answer(h: &mut Harness<BimApp>, path: &std::path::Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

#[test]
fn the_import_and_export_buttons_ask_for_a_file_and_work_on_the_draft_not_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("in.pat");
    std::fs::write(&file, ";%UNITS=MM\n*Wicker\n45,0,0,0,2\n*Sand\n0,0,0,0,9\n").unwrap();
    let mut h = app(BOX);
    patterns_tab(&mut h);
    activate(&mut h, "Import...");
    answer(&mut h, &file);
    // The pattern the project lacks came in; the one it has was kept, and the dialog says it was left out, in itself.
    assert!(h.query_by_label("Wicker: Name").is_some() && said(&h, "left out") && said(&h, "Sand"));
    assert!(!h.state().session.model.fill_patterns.contains_key("Wicker"), "a draft until OK");
    // With Replace Same Name, the file's Sand takes the place of the draft's.
    press(&mut h, "Replace Same Name");
    activate(&mut h, "Import...");
    answer(&mut h, &file);
    // Export writes what the draft holds, which the project doesn't yet.
    let out = dir.path().join("out.pat");
    activate(&mut h, "Export...");
    answer(&mut h, &out);
    let back = bimcraft_core::pat::read_file(&out).unwrap();
    let named = |n: &str| back.patterns.iter().find(|(name, _)| name == n).map(|(_, p)| p.clone());
    assert!(named("Wicker").is_some(), "the draft's pattern, not in the project");
    assert_eq!(named("Sand").map(|p| p.lines[0].spacing), Some(9.0), "the draft's Sand, the file's");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.fill_patterns["Sand"].lines[0].spacing, 9.0);
}

#[test]
fn the_count_of_patterns_left_out_is_the_commands_total_not_the_few_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bad.pat");
    // 120 patterns with a line that isn't numbers: the command names 100 of them and counts all 120.
    std::fs::write(&file, (0..120).map(|i| format!("*Bad{i}\nnot numbers\n")).collect::<String>()).unwrap();
    let mut h = app(BOX);
    patterns_tab(&mut h);
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(said(&h, "and 115 more"), "five shown of 120");
}

#[test]
fn a_long_list_of_patterns_left_out_is_counted_and_a_file_is_not_written_over_unasked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("all.pat");
    let names = ["Diagonal Up", "Diagonal Down", "Diagonal Crosshatch", "Crosshatch", "Horizontal", "Vertical", "Sand", "Tile 300"];
    std::fs::write(&file, names.iter().map(|n| format!("*{n}\n0,0,0,0,5\n")).collect::<String>()).unwrap();
    let mut h = app(BOX);
    patterns_tab(&mut h);
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(said(&h, "and 3 more"), "five named, the rest counted");
    assert!(!said(&h, "Tile 300 ("), "the sixth is not named");
    // A name the Save dialog took without .pat is given it; one already there is not written over.
    let out = dir.path().join("out");
    std::fs::write(dir.path().join("out.pat"), "keep").unwrap();
    activate(&mut h, "Export...");
    answer(&mut h, &out);
    assert_eq!(std::fs::read_to_string(dir.path().join("out.pat")).unwrap(), "keep");
    assert!(said(&h, "already exists"));
    let fresh = dir.path().join("fresh");
    activate(&mut h, "Export...");
    answer(&mut h, &fresh);
    assert!(std::fs::read_to_string(dir.path().join("fresh.pat")).unwrap().contains("*Sand"));
}

#[test]
fn an_import_says_one_pattern_in_the_singular_and_keeps_the_reason_a_pattern_was_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("one.pat");
    std::fs::write(&file, "*Weave\n0,0,0,0,5\n").unwrap();
    let mut h = app(BOX);
    patterns_tab(&mut h);
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(said(&h, "1 pattern imported."), "one pattern, in the singular");
    assert!(!said(&h, "1 patterns"));
    // A pattern whose reason is longer than the cut used to lose it ("a family's lines lie on each other: its…"), and a long name
    // is cut at a word, not in the middle of one.
    let name = "Alpha Beta Gamma Delta Epsilon Zeta Eta Theta Iota";
    std::fs::write(&file, format!("*Basket\n0,0,0,0,5\n*{name}\n0,0,0,0,0\n*{}\n0,0,0,0,0\n", "x".repeat(60))).unwrap();
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(said(&h, "1 pattern imported; left out: Alpha Beta Gamma Delta Epsilon… ("), "the name cut at a word");
    assert!(said(&h, &format!("{}… (", "x".repeat(30))), "a name of no words cut where it must");
    assert!(said(&h, "its offset is 0)"), "the reason, whole");
    // A name whose 30th character is inside a word is cut before that word, not through it.
    std::fs::write(&file, "*Alpha Beta Gamma Delta Epsilonic Zeta\n0,0,0,0,0\n").unwrap();
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(said(&h, "left out: Alpha Beta Gamma Delta… ("), "the name cut before the word the limit falls in");
    assert!(!said(&h, "Epsilon…"), "no word cut in two");
    // A pattern the core's check refuses has its reason told without the "fill pattern `name`" it begins with: the note names it.
    std::fs::write(&file, "*Huge Spacing Pattern\n0,0,0,0,999999999\n").unwrap();
    h.state_mut().import_patterns(&file);
    h.run();
    assert!(
        said(&h, "Huge Spacing Pattern (line 1: its lines are 0.05 to 100000 apart, not 999999999)"),
        "the reason, after its name once"
    );
    assert!(!said(&h, "fill pattern `"), "the name is not said twice");
}

#[test]
fn a_solid_fill_is_told_solid_in_portuguese_and_spanish_not_as_a_continuous_line() {
    use crate::lang::{Lang, translated_in};
    for l in [Lang::Portuguese, Lang::Spanish, Lang::Chinese] {
        let mut h = app(BOX);
        crate::lang::assume_cjk_font(&h.ctx);
        h.state_mut().lang = l;
        h.run();
        let model = h.state().session.model.clone();
        h.state_mut().fill_patterns_dialog = Some(crate::fillpatterns::FillPatternsDialog::new(&model));
        h.run();
        press(&mut h, translated_in(l, "Patterns").unwrap_or("Patterns"));
        let (fill, line) = (translated_in(l, "Solid Fill").unwrap(), translated_in(l, "Solid").unwrap());
        assert_ne!(fill, line, "{l:?}: a fill and a line pattern are two words");
        assert!(h.query_all_by_label(fill).next().is_some(), "{l:?}: the solid fill is `{fill}`");
        assert!(h.query_all_by_label(line).next().is_none(), "{l:?}: a fill is no `{line}` line");
    }
}

fn line_styles(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    press(h, "Line Styles");
}

#[test]
fn a_line_pattern_or_style_deleted_and_made_again_under_its_name_is_the_new_one() {
    use bimcraft_core::lines::{LinePattern, Segment};
    let mut h = app(BOX);
    line_styles(&mut h);
    press(&mut h, "Medium Lines: Delete");
    press(&mut h, "New Style");
    type_into(&mut h, "Style 1: Name", "Medium Lines");
    press(&mut h, "Patterns");
    press(&mut h, "Long Dash: Delete");
    press(&mut h, "New Pattern");
    type_into(&mut h, "Pattern Name", "Long Dash");
    press(&mut h, "Save Pattern");
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!(
        m.line_patterns["Long Dash"],
        LinePattern(vec![Segment::Dash(3.0), Segment::Space(1.5)]),
        "the new one, not gone with the old"
    );
    assert_eq!(m.line_styles["Medium Lines"].pen, Pen::FINE, "the new style, not the old one");
}

#[test]
fn line_style_and_pattern_renames_that_chain_or_swap_are_done_in_an_order_the_core_accepts() {
    use bimcraft_core::lines::{LinePattern, Segment};
    let mut h = app(BOX);
    line_styles(&mut h);
    // A chain of styles: Wide Lines becomes Heavy, Medium Lines takes the name Wide Lines had.
    type_into(&mut h, "Wide Lines: Name", "Heavy");
    type_into(&mut h, "Medium Lines: Name", "Wide Lines");
    // A swap of patterns, which the style that has one keeps hold of by the pattern, not the name.
    press(&mut h, "Patterns");
    type_into(&mut h, "Dash: Name", "Dot");
    type_into_nth(&mut h, "Dot: Name", 1, "Dash");
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert_eq!((m.line_styles["Heavy"].pen, m.line_styles["Wide Lines"].pen), (Pen::CUT, Pen::PROJECTION));
    assert_eq!(m.line_patterns["Dot"], LinePattern(vec![Segment::Dash(3.0), Segment::Space(1.5)]));
    assert_eq!(m.line_patterns["Dash"], LinePattern(vec![Segment::Dot, Segment::Space(1.0)]));
    assert_eq!(m.line_styles["Overhead Lines"].pattern.as_deref(), Some("Dot"), "it follows its pattern");
    assert!(m.line_patterns.keys().all(|n| !n.contains("renaming")), "{:?}", m.line_patterns.keys().collect::<Vec<_>>());
}

#[test]
fn the_line_styles_draft_refuses_blank_and_repeated_names_and_a_project_that_changed() {
    let mut h = app(BOX);
    line_styles(&mut h);
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "New Style");
    type_into(&mut h, "Style 1: Name", "Wide Lines");
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_some() && said(&h, "already in use"), "two styles of one name are not merged");
    type_into_nth(&mut h, "Wide Lines: Name", 1, "  ");
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_some() && said(&h, "Enter a name"), "a blank name does not list instead of making");
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    // An agent changes a style meanwhile: the draft is the project as it was.
    type_into_nth(&mut h, ": Name", 0, "Fresh");
    h.state_mut().session.execute("line_style", &json!({"name": "Agent Style", "pen": 3})).unwrap();
    h.run();
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_some() && said(&h, "changed"));
    assert!(h.state().session.model.line_styles.contains_key("Agent Style") && !h.state().session.model.line_styles.contains_key("Fresh"));
}

#[test]
fn a_duplicate_made_in_edit_type_goes_to_the_regions_selected_and_not_to_the_others_of_the_type() {
    let square = |x: i32| {
        format!(
            "[[[{x},1000],[{},1000]],[[{},1000],[{},2000]],[[{},2000],[{x},2000]],[[{x},2000],[{x},1000]]]",
            x + 900,
            x + 900,
            x + 900,
            x + 900
        )
    };
    let mut h = app(&format!(
        "{BOX}\nfilled_region {{\"sketch\": {}, \"type\": \"Sand\"}}\nfilled_region {{\"sketch\": {}, \"type\": \"Sand\"}}\nfilled_region {{\"sketch\": {}, \"type\": \"Sand\"}}\nfilled_region {{\"sketch\": {}, \"type\": \"Crosshatch\"}}",
        square(1000),
        square(2500),
        square(4000),
        square(5500)
    ));
    let regions = ids(&h, "region");
    let type_of = |h: &Harness<BimApp>, id: Id| field(h, id, "type");
    // Two of the three of one type selected: both go to the copy, the third and the other type stay.
    h.state_mut().selection = vec![regions[0], regions[1]];
    h.run();
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Sand Two");
    press(&mut h, "Create");
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_none(), "{}", h.state().status.text);
    let kinds: Vec<_> = regions.iter().map(|r| type_of(&h, *r)).collect();
    assert_eq!(kinds, [json!("Sand Two"), json!("Sand Two"), json!("Sand"), json!("Crosshatch")]);
    // One selected: it alone.
    h.state_mut().selection = vec![regions[2]];
    h.run();
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Sand Three");
    press(&mut h, "Create");
    press(&mut h, "OK");
    let kinds: Vec<_> = regions.iter().map(|r| type_of(&h, *r)).collect();
    assert_eq!(kinds, [json!("Sand Two"), json!("Sand Two"), json!("Sand Three"), json!("Crosshatch")]);
}

#[test]
fn an_apply_that_fails_part_way_does_nothing_and_says_why_inside_the_dialog() {
    let mut h = app(&format!(
        "{BOX}\nfilled_region {{\"sketch\": [[[1000,1000],[2000,1000]],[[2000,1000],[2000,2000]],[[2000,2000],[1000,1000]]], \"type\": \"Sand\"}}"
    ));
    let before = h.state().session.model.to_json().unwrap();
    fill_patterns(&mut h);
    // The first command of the draft would do; the last (deleting the type a region is of) is refused.
    patterns_tab(&mut h);
    type_into(&mut h, "Crosshatch: Name", "Grid");
    press(&mut h, "Region Types");
    press(&mut h, "Sand: Delete");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().fill_patterns_dialog.is_some(), "it stays open to fix it");
    assert!(said(&h, "filled region"), "and says why");
    assert_eq!(h.state().session.model.to_json().unwrap(), before, "the rename that came first is undone");
    assert_eq!(h.state().session.history().len(), steps, "and no step is left");
}

#[test]
fn the_detail_panels_create_group_button_groups_what_is_selected() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}"));
    press(&mut h, "Annotate");
    // Nothing selected: it says what to select, and makes nothing.
    press(&mut h, "Create Group");
    assert!(h.state().status.error && ids(&h, "detailgroup").is_empty());
    click(&mut h, P2(2000.0, 1000.0));
    press(&mut h, "Annotate");
    press(&mut h, "Create Group");
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (1, 0));
    assert_eq!(h.state().selection, ids(&h, "detailgroup"));
}

#[test]
fn the_type_swatches_stand_on_the_canvas_paper_in_every_look() {
    // Ink is the canvas's colour, so it needs the canvas's ground: in a dark interface with a light canvas, black on the dialog's
    // own dark ground would be lost.
    for (dark, light_canvas) in [(false, false), (true, false), (true, true)] {
        let mut h = app(BOX);
        h.state_mut().light_canvas = light_canvas;
        h.ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        h.run();
        fill_patterns(&mut h);
        h.run();
        let paper = crate::canvas::color(bimcraft_core::render::PAPER);
        let swatches = painted(&h)
            .iter()
            .filter(|s| matches!(s, egui::Shape::Rect(r) if r.fill == paper && (r.rect.width() - 60.0).abs() < 0.5 && (r.rect.height() - 22.0).abs() < 0.5))
            .count();
        // Six types, each with a foreground and a background.
        assert_eq!(swatches, 12, "dark {dark}, light canvas {light_canvas}");
        assert_eq!(paper == egui::Color32::WHITE, !dark || light_canvas, "the canvas is as light as the look says");
    }
}
