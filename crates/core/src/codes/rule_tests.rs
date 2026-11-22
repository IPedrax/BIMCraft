//! The rule of `CLAUDE.md`: no code values in code. A number a standard, a utility, a fire brigade or a common practice gives
//! is a rule of a building code, in `data/codes/` with its source; a module that sizes or checks by a code takes it from there.
//! Physical constants and the safety caps stay in Rust.

/// The modules that read a code's rules: none may hold a numeric constant or table of its own, but those `ALLOWED` names.
const READERS: [&str; 19] = [
    "codes.rs",
    "codes/stairs.rs",
    "codes/checks.rs",
    "plumbing.rs",
    "water.rs",
    "reservoir.rs",
    "drainage.rs",
    "storm.rs",
    "hydrant.rs",
    "reinforce/norm.rs",
    "dr.rs",
    "bath.rs",
    "loads.rs",
    "building_demand.rs",
    "entrance.rs",
    "nbr5410.rs",
    "conduit.rs",
    "dps.rs",
    "demand.rs",
];

/// What a reader may keep: caps on what it takes, tolerances, and physics.
const ALLOWED: [&str; 16] = [
    "MAX_OWN",
    "MAX_NAME",
    // How many names of rooms a row may hold in each list.
    "MAX_NAMES",
    "MAX_XML",
    "MAX_VALUES",
    "MAX_AMENDMENTS",
    "MAX_RULE",
    "MAX_NOTES",
    "EPS",
    // Physics: a metre of water in kPa, water's kinematic viscosity at 20 °C, the acceleration of gravity.
    "KPA_PER_M",
    "NU",
    "G",
    // Copper's temperature coefficient of resistance.
    "COPPER_ALPHA",
    // A cap on the circuits a grouping factor is looked up for.
    "MAX_CIRCUITS",
    // Site and Climate's ranges, a guard on what a project types, not a code's numbers.
    "SITE_NUMBERS",
    "SITE_TEXTS",
];

/// A `const` or `static` of `text` whose value holds a number: its name, where it isn't allowed.
fn numbers_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        let line = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub(super) "))
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        let decl = line.strip_prefix("const ").or_else(|| line.strip_prefix("static "));
        if let Some(rest) = decl
            && let Some((name, _)) = rest.split_once(':')
            && !rest.starts_with("fn ")
        {
            // The whole declaration, to its `;`.
            let mut whole = String::from(rest);
            let mut j = i;
            while !whole.trim_end().ends_with(';') && j + 1 < lines.len() {
                j += 1;
                whole.push_str(lines[j]);
            }
            let value = whole.split_once('=').map_or("", |(_, v)| v);
            let numeric = value.chars().any(|c| c.is_ascii_digit()) && !value.trim_start().starts_with('"');
            let name = name.trim();
            if numeric && !ALLOWED.contains(&name) {
                out.push(name.to_string());
            }
            i = j;
        }
        i += 1;
    }
    out
}

#[test]
fn code_values_live_in_codes() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in READERS {
        let text = std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let found = numbers_of(&text);
        assert!(
            found.is_empty(),
            "{file} holds numbers of its own: {found:?}. A code's number goes in a section of data/codes/ with its source \
             (CLAUDE.md, \"No code values in code\"); physics and caps go in ALLOWED"
        );
    }
}

#[test]
fn the_scan_finds_a_number_and_leaves_physics_alone() {
    let text = "pub const MAX_SPEED: f64 = 3.0;\nconst NU: f64 = 1.004e-6;\npub(crate) const TABLE: &[(f64, f64)] =\n    &[(20.0, 1.5)];\nconst NAME: &str = \"x 1\";\npub const fn f() {}";
    assert_eq!(numbers_of(text), ["MAX_SPEED", "TABLE"]);
}
