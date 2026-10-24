//! The Plumbing, Electrical, Lightning Protection and Data tabs, driven the way a user drives them. Their tools share the
//! Structure tab's run and standing tools and one fixture tool, so the checks that every tab passes are in tables.

use super::*;

/// A tool from a tab: its contextual tab takes the ribbon, as in Revit.
fn from_tab(h: &mut Harness<BimApp>, tab: &str, label: &str) {
    press(h, tab);
    press(h, label);
}

fn axis(h: &Harness<BimApp>, id: Id) -> Value {
    field(h, id, "axis")
}

/// A run tab's tool: the tab, the button, the category it makes, its contextual tab, the type it starts with and how wide its
/// section is.
struct RunTool {
    tab: &'static str,
    button: &'static str,
    category: &'static str,
    /// The command that makes it.
    command: &'static str,
    context: &'static str,
    letters: &'static str,
    default_type: &'static str,
    width: f64,
    /// Another type the Type Selector lists.
    other: &'static str,
    /// Whether the default type is round (a diameter) or not (a width and a depth).
    round: bool,
}

const RUN_TOOLS: &[RunTool] = &[
    RunTool {
        tab: "Plumbing",
        button: "Pipe",
        category: "pipe",
        command: "pipe",
        context: "Modify | Place Pipe",
        letters: "PI",
        default_type: "PVC 50",
        width: 50.0,
        other: "Copper 22",
        round: true,
    },
    RunTool {
        tab: "Electrical",
        button: "Conduit",
        category: "conduit",
        command: "conduit",
        context: "Modify | Place Conduit",
        letters: "CN",
        default_type: "PVC 25",
        width: 25.0,
        other: "Steel 25",
        round: true,
    },
    RunTool {
        tab: "Electrical",
        button: "Cable Tray",
        category: "cabletray",
        command: "cable_tray",
        context: "Modify | Place Cable Tray",
        letters: "CT",
        default_type: "Perforated 100 x 50",
        width: 100.0,
        other: "Perforated 200 x 50",
        round: false,
    },
    RunTool {
        tab: "Electrical",
        button: "Wire",
        category: "wire",
        command: "wire",
        context: "Modify | Place Wire",
        letters: "EW",
        default_type: "Wire 2.5",
        width: 4.0,
        other: "Wire 4",
        round: true,
    },
    RunTool {
        tab: "Lightning Protection",
        button: "Down Conductor",
        category: "downconductor",
        command: "down_conductor",
        context: "Modify | Place Down Conductor",
        letters: "DC",
        default_type: "Steel 8",
        width: 8.0,
        other: "Copper 8",
        round: true,
    },
    RunTool {
        tab: "Lightning Protection",
        button: "Earthing Conductor",
        category: "groundconductor",
        command: "ground_conductor",
        context: "Modify | Place Earthing Conductor",
        letters: "GC",
        default_type: "Copper 9",
        width: 9.0,
        other: "Steel 8",
        round: true,
    },
    RunTool {
        tab: "Data",
        button: "Cabling",
        category: "cable",
        command: "cabling",
        context: "Modify | Place Cabling",
        letters: "CB",
        default_type: "Cat6",
        width: 6.0,
        other: "Cat6A",
        round: true,
    },
];

/// What Visibility/Graphics lists the categories of these tabs as.
const CATEGORY_NAMES: &[&str] = &["Pipes", "Conduits", "Cable Trays", "Wires", "Down Conductors", "Earthing Conductors", "Cables"];

/// A fixture tab's tool: the tab, the button, its contextual tab, the shortcut, the families it places and one it doesn't.
struct FixtureTool {
    tab: &'static str,
    button: &'static str,
    key: &'static str,
    context: &'static str,
    letters: &'static str,
    /// A "Family: Type" the Type Selector lists, and one it doesn't.
    listed: &'static str,
    not_listed: &'static str,
    /// The family placed when none is picked.
    first: &'static str,
}

const FIXTURE_TOOLS: &[FixtureTool] = &[
    FixtureTool {
        tab: "Plumbing",
        button: "Plumbing Fixture",
        key: "plumbing",
        context: "Modify | Place Plumbing Fixture",
        letters: "PX",
        listed: "WC: 400 x 650",
        not_listed: "Table: 1600 x 900",
        first: "Bathtub",
    },
    FixtureTool {
        tab: "Plumbing",
        button: "Pipe Fitting",
        key: "fitting",
        context: "Modify | Place Pipe Fitting",
        letters: "PF",
        listed: "Tee: 50",
        not_listed: "WC: 400 x 650",
        first: "Elbow 90",
    },
    FixtureTool {
        tab: "Plumbing",
        button: "Pipe Accessory",
        key: "accessory",
        context: "Modify | Place Pipe Accessory",
        letters: "PA",
        listed: "Gate Valve: 25",
        not_listed: "Tee: 50",
        first: "Gate Valve",
    },
    FixtureTool {
        tab: "Electrical",
        button: "Electrical Equipment",
        key: "equipment",
        context: "Modify | Place Electrical Equipment",
        letters: "EE",
        listed: "Distribution Board: 24 circuits",
        not_listed: "Outlet: Low 300",
        first: "Distribution Board",
    },
    FixtureTool {
        tab: "Electrical",
        button: "Device",
        key: "device",
        context: "Modify | Place Device",
        letters: "DV",
        listed: "Switch: Medium 1100",
        not_listed: "Ceiling Luminaire: 300",
        first: "Outlet",
    },
    FixtureTool {
        tab: "Electrical",
        button: "Lighting Fixture",
        key: "lighting",
        context: "Modify | Place Lighting Fixture",
        letters: "LF",
        listed: "Recessed Luminaire: 600 x 600",
        not_listed: "Switch: Medium 1100",
        first: "Ceiling Luminaire",
    },
    FixtureTool {
        tab: "Lightning Protection",
        button: "Air Terminal",
        key: "airterminal",
        context: "Modify | Place Air Terminal",
        letters: "AT",
        listed: "Air Terminal: 1000",
        not_listed: "Distribution Board: 24 circuits",
        first: "Air Terminal",
    },
    FixtureTool {
        tab: "Lightning Protection",
        button: "Earthing Electrode",
        key: "earthing",
        context: "Modify | Place Earthing Electrode",
        letters: "EG",
        listed: "Earthing Electrode: 3000",
        not_listed: "Air Terminal: 500",
        first: "Earthing Electrode",
    },
    FixtureTool {
        tab: "Lightning Protection",
        button: "Inspection Box",
        key: "inspection",
        context: "Modify | Place Inspection Box",
        letters: "IB",
        listed: "Inspection Box: 300",
        not_listed: "Earthing Electrode: 3000",
        first: "Inspection Box",
    },
    FixtureTool {
        tab: "Data",
        button: "Data Outlet",
        key: "dataoutlet",
        context: "Modify | Place Data Outlet",
        letters: "DT",
        listed: "Data Outlet: Medium 1100",
        not_listed: "Rack: 600 x 1000",
        first: "Data Outlet",
    },
    FixtureTool {
        tab: "Data",
        button: "Voice Outlet",
        key: "voiceoutlet",
        context: "Modify | Place Voice Outlet",
        letters: "VO",
        listed: "Voice Outlet: Medium 1100",
        not_listed: "Data Outlet: Medium 1100",
        first: "Voice Outlet",
    },
    FixtureTool {
        tab: "Data",
        button: "Rack",
        key: "rack",
        context: "Modify | Place Rack",
        letters: "RK",
        listed: "Rack: 800 x 1000",
        not_listed: "Patch Panel: 24 ports",
        first: "Rack",
    },
    FixtureTool {
        tab: "Data",
        button: "Patch Panel",
        key: "patchpanel",
        context: "Modify | Place Patch Panel",
        letters: "PN",
        listed: "Patch Panel: 48 ports",
        not_listed: "Rack: 600 x 1000",
        first: "Patch Panel",
    },
];

#[test]
fn each_discipline_tab_has_its_tools_and_none_of_the_architecture_tabs() {
    let tabs: [(&str, &[&str]); 4] = [
        ("Plumbing", &["Pipe", "Plumbing Fixture", "Pipe Fitting", "Pipe Accessory"]),
        ("Electrical", &["Electrical Equipment", "Device", "Lighting Fixture", "Conduit", "Cable Tray", "Wire"]),
        ("Lightning Protection", &["Air Terminal", "Down Conductor", "Earthing Electrode", "Inspection Box", "Earthing Conductor"]),
        ("Data", &["Data Outlet", "Voice Outlet", "Rack", "Patch Panel", "Cable Tray", "Cabling"]),
    ];
    for (tab, labels) in tabs {
        let mut h = app(BOX);
        press(&mut h, tab);
        for label in labels {
            assert!(h.query_by_label(label).is_some(), "{tab}: {label}");
        }
        assert!(h.query_by_label("Door").is_none(), "{tab} has the Architecture tab's tools");
    }
}

#[test]
fn runs_of_the_mep_go_between_two_clicks_centred_on_their_height_and_chain_on() {
    for c in RUN_TOOLS {
        let mut h = app(BOX);
        from_tab(&mut h, c.tab, c.button);
        assert!(h.query_by_label(c.context).is_some(), "{}", c.context);
        click(&mut h, P2(1000.0, 1000.0));
        click(&mut h, P2(5000.0, 1000.0));
        let made = ids(&h, c.category);
        assert_eq!(made.len(), 1, "{}", c.button);
        assert_eq!((field(&h, made[0], "type"), field(&h, made[0], "justify")), (json!(c.default_type), json!("centre")));
        assert_eq!(axis(&h, made[0]), json!([[1000.0, 1000.0, 0.0], [5000.0, 1000.0, 0.0]]));
        // Chain: the next starts where it ended; Esc drops it, then the tool.
        assert_eq!(h.state().tool, Tool::Run(c.category, Some(P2(5000.0, 1000.0))));
        click(&mut h, P2(5000.0, 4000.0));
        assert_eq!(ids(&h, c.category).len(), 2);
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
        assert_eq!(h.state().tool, Tool::Modify);
        assert_eq!(h.state().session.history().last().copied(), Some(c.button));
    }
}

#[test]
fn the_options_of_a_run_tool_set_its_justification_height_and_type_and_are_its_own() {
    for c in RUN_TOOLS {
        let mut h = app(BOX);
        from_tab(&mut h, c.tab, c.button);
        press(&mut h, "Chain");
        h.get_by_role_and_label(Role::ComboBox, "Justification:").click();
        h.run();
        press(&mut h, "Bottom");
        type_into(&mut h, "Offset:", "2400");
        press(&mut h, "Type Selector");
        press(&mut h, c.other);
        click(&mut h, P2(1000.0, 1000.0));
        click(&mut h, P2(5000.0, 1000.0));
        let made = ids(&h, c.category)[0];
        assert_eq!((field(&h, made, "type"), field(&h, made, "justify")), (json!(c.other), json!("bottom")));
        assert_eq!(field(&h, made, "offset"), json!(2400.0));
        // What isn't a height is said, and makes nothing.
        type_into(&mut h, "Offset:", "high");
        click(&mut h, P2(1000.0, 2000.0));
        click(&mut h, P2(5000.0, 2000.0));
        assert!(h.state().status.error && h.state().status.text.contains("high"), "{:?}", h.state().status);
        assert_eq!(ids(&h, c.category).len(), 1);
        // The options a beam was left with are not this tool's: it starts centred, at the level.
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
        press(&mut h, "Structure");
        press(&mut h, "Beam");
        assert_eq!(h.state().run_justify, bimcraft_core::model::Justify::Top);
        key(&mut h, Key::Escape);
        from_tab(&mut h, c.tab, c.button);
        assert_eq!(h.state().run_justify, bimcraft_core::model::Justify::Bottom, "each tool keeps its own");
    }
}

#[test]
fn a_run_tool_shows_the_run_a_click_would_make_and_how_long_it_is() {
    for c in RUN_TOOLS {
        let mut h = app(BOX);
        from_tab(&mut h, c.tab, c.button);
        click(&mut h, P2(1000.0, 1000.0));
        h.hover_at(h.state().screen(P2(5000.0, 1000.0)));
        h.run();
        let outlines = tests_previewed(&h);
        let (a, b) = (h.state().screen(P2(0.0, 0.0)), h.state().screen(P2(1000.0, 0.0)));
        let width = f64::from(b.x - a.x) / 1000.0 * c.width;
        assert!(
            outlines.iter().any(|o| {
                let (lo, hi) = o.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
                ((hi - lo) as f64 - width).abs() < 1.5 && o.len() == 4
            }),
            "{}: {outlines:?}",
            c.button
        );
        assert!(painted_text(&h).contains(&"4000".to_string()), "{:?}", painted_text(&h));
        assert!(ids(&h, c.category).is_empty(), "a sketch isn't an element");
    }
}

/// The accent-coloured closed outlines the window drew, as the Structure tests find a preview.
fn tests_previewed(h: &Harness<BimApp>) -> Vec<Vec<Pos2>> {
    fn walk(s: &egui::Shape, out: &mut Vec<Vec<Pos2>>) {
        match s {
            egui::Shape::Path(p) if p.closed && p.stroke.color == egui::epaint::ColorMode::Solid(crate::style::pal().accent) => {
                out.push(p.points.clone())
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn a_fixture_tool_places_a_family_of_its_kind_where_clicked_and_lists_only_those() {
    for c in FIXTURE_TOOLS {
        let mut h = app(BOX);
        from_tab(&mut h, c.tab, c.button);
        assert!(h.query_by_label(c.context).is_some(), "{}", c.context);
        assert_eq!(h.state().tool, Tool::Fixture(c.key));
        // With none picked, the first of its kind goes where clicked.
        click(&mut h, P2(2000.0, 2000.0));
        let placed = ids(&h, "component");
        assert_eq!((placed.len(), field(&h, placed[0], "family")), (1, json!(c.first)), "{}", c.button);
        // The Type Selector lists the families of its kind, and no others; the one picked is the one placed, turned by Space.
        press(&mut h, "Type Selector");
        assert!(h.query_by_label(c.listed).is_some(), "{} lists {}", c.button, c.listed);
        assert!(h.query_by_label(c.not_listed).is_none(), "{} lists {}", c.button, c.not_listed);
        press(&mut h, c.listed);
        key(&mut h, Key::Space);
        click(&mut h, P2(4000.0, 2000.0));
        let (family, ty) = c.listed.split_once(": ").unwrap_or_default();
        let second = ids(&h, "component")[1];
        assert_eq!((field(&h, second, "family"), field(&h, second, "type")), (json!(family), json!(ty)), "{}", c.button);
        assert_eq!(field(&h, second, "angle"), json!(90.0), "{}", c.button);
        key(&mut h, Key::Escape);
        assert_eq!(h.state().tool, Tool::Modify);
    }
}

#[test]
fn the_fixture_tool_shows_the_fixture_a_click_would_place() {
    let mut h = app(BOX);
    from_tab(&mut h, "Plumbing", "Plumbing Fixture");
    h.hover_at(h.state().screen(P2(3000.0, 3000.0)));
    h.run();
    assert!(!tests_previewed(&h).is_empty(), "the bathtub it would place is drawn in the accent");
    assert!(ids(&h, "component").is_empty());
}

#[test]
fn revits_shortcuts_pick_the_tools_of_the_discipline_tabs() {
    let mut h = app(BOX);
    for c in RUN_TOOLS {
        shortcut(&mut h, c.letters);
        assert_eq!(h.state().tool, Tool::Run(c.category, None), "{}", c.letters);
        key(&mut h, Key::Escape);
    }
    for c in FIXTURE_TOOLS {
        shortcut(&mut h, c.letters);
        assert_eq!(h.state().tool, Tool::Fixture(c.key), "{}", c.letters);
        key(&mut h, Key::Escape);
    }
    let listed: Vec<(&str, &str)> = crate::shortcuts();
    // Each is listed by its button's name, which the command's label (Conduit, Cable Tray, Wire…) is.
    let names = RUN_TOOLS.iter().map(|c| (c.letters, c.button)).chain(FIXTURE_TOOLS.iter().map(|c| (c.letters, c.button)));
    for (letters, name) in names {
        assert!(listed.contains(&(letters, name)), "{letters} {name}");
    }
}

#[test]
fn a_selected_run_of_the_mep_has_its_palette_and_edit_type_changes_its_round_section_and_material() {
    for c in RUN_TOOLS {
        let mut h = app(&format!("{BOX}\n{} {{\"points\": [[1000, 1000], [5000, 1000]], \"offset\": 2400}}", c.command));
        let a = ids(&h, c.category)[0];
        click(&mut h, P2(3000.0, 1000.0));
        assert_eq!(h.state().selection, [a]);
        for row in ["Reference Level", "Start Level Offset", "End Level Offset", "Z Justification", "System", "Length"] {
            assert!(h.query_by_label(row).is_some(), "{row}");
        }
        type_into(&mut h, "System", "Esgoto");
        key(&mut h, Key::Enter);
        assert_eq!(field(&h, a, "system"), json!("Esgoto"));
        // Edit Type: a round type says its diameter, not a depth; a rectangular one its width and its depth; and its roundness and
        // material can change.
        press(&mut h, "Edit Type");
        let (size, depth) = if c.round { ("Diameter", "Depth") } else { ("Width", "Diameter") };
        assert!(h.query_by_label(size).is_some() && h.query_by_label(depth).is_none(), "{}", c.category);
        type_into(&mut h, size, "63");
        h.get_by_role_and_label(Role::ComboBox, "Material").click();
        h.run();
        press(&mut h, "Copper");
        press(&mut h, "OK");
        let t = h.state().session.model.types.runs[c.category][c.default_type].clone();
        let (d, round) = if c.round { (63.0, true) } else { (t.depth, false) };
        assert_eq!((t.width, t.depth, t.round, t.material.as_str()), (63.0, d, round, "Copper"), "{}", c.category);
        assert_eq!(field(&h, a, "width"), json!(63.0));
        // Made round or not, it takes the other shape.
        press(&mut h, "Edit Type");
        press(&mut h, "Round");
        if c.round {
            type_into(&mut h, "Depth", "40");
        } else {
            type_into(&mut h, "Diameter", "80");
        }
        press(&mut h, "OK");
        let t = h.state().session.model.types.runs[c.category][c.default_type].clone();
        let expected = if c.round { (63.0, 40.0, false) } else { (80.0, 80.0, true) };
        assert_eq!((t.width, t.depth, t.round), expected, "{}", c.category);
    }
}

#[test]
fn a_duplicated_run_type_can_change_its_roundness_and_material() {
    let mut h = app("pipe {\"points\": [[0, 0], [4000, 0]], \"offset\": 2400}");
    let a = ids(&h, "pipe")[0];
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Tray 100 x 50");
    press(&mut h, "Create");
    press(&mut h, "Round");
    type_into(&mut h, "Width", "100");
    type_into(&mut h, "Depth", "50");
    h.get_by_role_and_label(Role::ComboBox, "Material").click();
    h.run();
    press(&mut h, "Copper");
    press(&mut h, "OK");
    let made = h.state().session.model.types.runs["pipe"]["Tray 100 x 50"].clone();
    assert_eq!((made.width, made.depth, made.round, made.material.as_str()), (100.0, 50.0, false, "Copper"));
    assert_eq!(field(&h, a, "type"), json!("Tray 100 x 50"));
    // And the type it copied is as it was.
    let was = h.state().session.model.types.runs["pipe"]["PVC 50"].clone();
    assert_eq!((was.width, was.round, was.material.as_str()), (50.0, true, "PVC"));
}

#[test]
fn create_similar_and_the_view_lists_know_the_categories_of_the_discipline_tabs() {
    let mut h = app("pipe {\"points\": [[0, 0], [4000, 0]], \"type\": \"Copper 22\"}");
    let a = ids(&h, "pipe")[0];
    h.state_mut().selection = vec![a];
    shortcut(&mut h, "CS");
    assert_eq!(h.state().tool, Tool::Run("pipe", None));
    click(&mut h, P2(100.0, 100.0));
    click(&mut h, P2(2100.0, 100.0));
    assert_eq!(field(&h, *ids(&h, "pipe").last().unwrap(), "type"), json!("Copper 22"), "the type of what it copies");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    for name in CATEGORY_NAMES {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
}

#[test]
fn the_new_schedule_dialog_lists_the_categories_of_the_discipline_tabs_and_names_a_schedule_after_one() {
    let mut h = app("pipe {\"points\": [[0, 0], [4000, 0]], \"system\": \"Esgoto\"}");
    press(&mut h, "View");
    press(&mut h, "Schedules");
    press(&mut h, "Schedule/Quantities…");
    // The list scrolls: its entry is there for a screen reader if not under the pointer.
    h.get_by_role_and_label(Role::Button, "Pipes").click_accesskit();
    h.run();
    let named = {
        use egui_kittest::kittest::NodeT;
        h.get_by_role_and_label(Role::TextInput, "Name:").accesskit_node().value()
    };
    assert_eq!(named.as_deref(), Some("Pipe Schedule"));
    press(&mut h, "OK");
    assert_eq!(ids(&h, "schedule").len(), 1);
}

#[test]
fn the_discipline_tabs_speak_portuguese_and_spanish_and_leave_no_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    let script = format!(
        "{BOX}\npipe {{\"points\": [[1000, 1000], [5000, 1000]], \"offset\": 2400}}\nconduit {{\"points\": [[1000, 2000], [5000, 2000]], \"offset\": 2400}}\n\
         cable_tray {{\"points\": [[1000, 3000], [5000, 3000]], \"offset\": 2600}}\nwire {{\"points\": [[1000, 4000], [5000, 4000]], \"offset\": 2400}}\n\
         component {{\"family\": \"WC\", \"at\": [7000, 1000]}}\ncomponent {{\"family\": \"Outlet\", \"type\": \"Low 300\", \"at\": [7000, 3000]}}\n\
         down_conductor {{\"points\": [[6000, 5000], [6000, 5000]], \"offset\": 0, \"end_offset\": 3000}}\n\
         component {{\"family\": \"Air Terminal\", \"type\": \"1000\", \"at\": [7000, 5000]}}\n\
         cabling {{\"points\": [[500, 5000], [2000, 5000]], \"offset\": 2400}}"
    );
    let mut h = app(&script);
    assert!(english(&h, Lang::Spanish).contains(&"Plumbing".to_string()), "the probe finds what it looks for");
    let expected = [
        (
            Lang::Portuguese,
            [
                ("Hidrossanitário", vec!["Tubulação", "Peça sanitária", "Conexão de tubulação", "Acessório de tubulação"]),
                ("Elétrico", vec!["Equipamento elétrico", "Dispositivo", "Luminária", "Eletroduto", "Eletrocalha", "Fio"]),
                ("SPDA", vec!["Captor", "Condutor de descida", "Haste de aterramento", "Caixa de inspeção", "Condutor de aterramento"]),
                ("Lógica, Dados e Voz", vec!["Ponto de dados", "Ponto de voz", "Rack", "Patch panel", "Eletrocalha", "Cabeamento"]),
            ],
        ),
        (
            Lang::Spanish,
            [
                ("Fontanería", vec!["Tubería", "Aparato sanitario", "Conexión de tubería", "Accesorio de tubería"]),
                ("Eléctrico", vec!["Equipo eléctrico", "Dispositivo", "Luminaria", "Tubo conduit", "Bandeja portacables", "Cable"]),
                (
                    "SPDA",
                    vec![
                        "Terminal aéreo",
                        "Conductor de bajada",
                        "Electrodo de puesta a tierra",
                        "Caja de inspección",
                        "Conductor de puesta a tierra",
                    ],
                ),
                ("Datos y Voz", vec!["Punto de datos", "Punto de voz", "Rack", "Panel de parcheo", "Bandeja portacables", "Cableado"]),
            ],
        ),
    ];
    for (l, tabs) in expected {
        h.state_mut().lang = l;
        h.run();
        let mut missed = english(&h, l);
        for (tab, labels) in tabs {
            press(&mut h, tab);
            missed.extend(english(&h, l));
            for label in labels {
                assert!(h.query_by_label(label).is_some(), "{label}");
                press(&mut h, label);
                missed.extend(english(&h, l));
                click(&mut h, P2(100.0, 100.0));
                missed.extend(english(&h, l));
                key(&mut h, Key::Escape);
                key(&mut h, Key::Escape);
            }
        }
        // What they made, selected: the palette and the contextual tab; and a pipe's type dialog.
        for point in [
            P2(3000.0, 1000.0),
            P2(3000.0, 2000.0),
            P2(3000.0, 3000.0),
            P2(3000.0, 4000.0),
            P2(7000.0, 3000.0),
            P2(7000.0, 1000.0),
            P2(6000.0, 5000.0),
            P2(7000.0, 5000.0),
            P2(1000.0, 5000.0),
        ] {
            click(&mut h, point);
            missed.extend(english(&h, l));
            key(&mut h, Key::Escape);
        }
        click(&mut h, P2(3000.0, 1000.0));
        press(&mut h, crate::lang::translated_in(l, "Edit Type").unwrap_or("Edit Type"));
        missed.extend(english(&h, l));
        press(&mut h, crate::lang::translated_in(l, "Cancel").unwrap_or("Cancel"));
        key(&mut h, Key::Escape);
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?}: {missed:?}");
    }
    h.state_mut().lang = Lang::English;
    h.run();
    use crate::lang::category;
    assert_eq!([category("pipe", false), category("cabletray", true)], ["Pipe", "Cable Trays"]);
    h.state_mut().lang = Lang::Portuguese;
    h.run();
    assert_eq!(
        [category("pipe", true), category("conduit", false), category("cabletray", false)],
        ["Tubulações", "Eletroduto", "Eletrocalha"]
    );
}
