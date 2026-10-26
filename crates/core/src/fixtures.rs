//! The fixtures and devices a new project starts with, BIMCraft's own drawings: what the discipline tabs place as components
//! (Plumbing's basins, sinks, showers, drains, fittings and valves, and the other tabs' as they come). Each stands with its
//! back on its x axis and its front toward -y, as the furniture does; a pipe fitting or a valve is centred on its pipe, so
//! placing it at a pipe's height lines them up.

use bimcraft_i18n::msg;

use serde_json::{Value, json};

use crate::language::Language;

/// The starter fixtures, as family definitions by name, in `language`.
pub fn library(language: Language) -> Vec<(String, Value)> {
    let mut out = plumbing(language);
    out.extend(pipework(language));
    out.extend(electrical(language));
    out.extend(lightning(language));
    out.extend(telecom(language));
    out
}

/// A box from `[x0, y0, x1, y1]` up from `z0` to `z1`, each a number or a formula, of material `m`.
fn block([x0, y0, x1, y1]: [Value; 4], z0: Value, z1: Value, m: &str) -> Value {
    json!({"profile": [[x0, y0], [x1.clone(), y0], [x1, y1.clone()], [x0, y1]], "from": z0, "to": z1, "material": m})
}

/// What a form cuts out of the solids listed before it: the same shape, no material.
fn void(mut form: Value) -> Value {
    if let Some(o) = form.as_object_mut() {
        o.remove("material");
        o.insert("void".into(), json!(true));
    }
    form
}

/// A cylinder standing up from `z0` to `z1`, round (`x`, `y`).
fn post(x: Value, y: Value, r: Value, z0: Value, z1: Value, m: &str) -> Value {
    json!({"circle": {"centre": [x, y], "radius": r}, "from": z0, "to": z1, "material": m})
}

fn size(name: &str, value: f64) -> Value {
    json!({"name": name, "value": value})
}

fn material(name: &str, value: &str) -> Value {
    json!({"name": name, "kind": "material", "value": value})
}

fn f(e: impl Into<String>) -> Value {
    Value::String(e.into())
}

/// Sanitary fixtures: a kitchen sink, a shower, a floor drain (the WC, the washbasin and the bathtub are in the furniture's
/// library).
fn plumbing(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height) = (w("Width"), w("Depth"), w("Height"));
    let (metal, porcelain, mat) = (w("Metal"), w("Porcelain"), w("Material"));
    let n = |x: f64| json!(x);
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-{width} / 2"));
    let mut out = Vec::new();

    // A kitchen sink on four legs: its steel top, a bowl cut into it with a drainer beside, and a tap at the back.
    let bowl = |inset: f64| {
        block(
            [
                f(format!("-({width}) / 2 + {}", 60.0 + inset)),
                f(format!("-({depth}) + {}", 60.0 + inset)),
                f(format!("{width} / 2 - 300 - {inset}")),
                n(-60.0 - inset),
            ],
            f(format!("{height} - {}", 200.0 - inset)),
            f(height.clone()),
            &metal,
        )
    };
    let leg = |east: bool, north: bool| {
        let x = if east {
            [f(format!("{width} / 2 - 50")), f(format!("{width} / 2 - 10"))]
        } else {
            [f(format!("-({width}) / 2 + 10")), f(format!("-({width}) / 2 + 50"))]
        };
        let y = if north { [n(-50.0), n(-10.0)] } else { [f(format!("-({depth}) + 10")), f(format!("-({depth}) + 50"))] };
        let [x0, x1] = x;
        let [y0, y1] = y;
        block([x0, y0, x1, y1], n(0.0), f(format!("{height} - 40")), &metal)
    };
    // The bowl's middle, whatever the sink's width: its drainer is 300 and its walls 70 wide.
    let tap_x = -120.0;
    let mut forms = vec![
        block(
            [f(minus_half_w.clone()), f(format!("-{depth}")), f(half_w.clone()), n(0.0)],
            f(format!("{height} - 40")),
            f(height.clone()),
            &metal,
        ),
        // The bowl: its walls and floor, then the cut that opens it.
        block(
            [f(format!("-({width}) / 2 + 50")), f(format!("-({depth}) + 50")), f(format!("{width} / 2 - 290")), n(-50.0)],
            f(format!("{height} - 210")),
            f(format!("{height} - 40")),
            &metal,
        ),
    ];
    forms.push(void(bowl(10.0)));
    forms.extend([(true, true), (true, false), (false, true), (false, false)].map(|(e, nn)| leg(e, nn)));
    forms.push(post(n(tap_x), n(-40.0), n(15.0), f(height.clone()), f(format!("{height} + 250")), &metal));
    forms.push(block(
        [n(tap_x - 15.0), n(-220.0), n(tap_x + 15.0), n(-25.0)],
        f(format!("{height} + 220")),
        f(format!("{height} + 250")),
        &metal,
    ));
    out.push((
        w(msg!("Kitchen Sink")),
        json!({
            "category": "plumbing",
            "parameters": [size(&width, 1200.0), size(&depth, 550.0), size(&height, 900.0), material(&mat, &metal)],
            "types": {"1200 x 550": {}, "800 x 500": {&width: 800, &depth: 500}},
            "forms": forms
        }),
    ));

    // A shower: its tray, and the head on an arm from a riser at the wall.
    let hollow = void(block(
        [f(format!("-({width}) / 2 + 60")), f(format!("-({depth}) + 60")), f(format!("{width} / 2 - 60")), n(-60.0)],
        n(25.0),
        n(50.0),
        "",
    ));
    out.push((
        w(msg!("Shower")),
        json!({
            "category": "plumbing",
            "parameters": [size(&width, 900.0), size(&depth, 900.0)],
            "types": {"900 x 900": {}, "800 x 800": {&width: 800, &depth: 800}},
            "forms": [
                block([f(minus_half_w), f(format!("-{depth}")), f(half_w), n(0.0)], n(0.0), n(50.0), &porcelain),
                hollow,
                post(n(0.0), n(-60.0), n(12.0), n(50.0), n(2100.0), &metal),
                block([n(-12.0), n(-300.0), n(12.0), n(-48.0)], n(2070.0), n(2100.0), &metal),
                post(n(0.0), n(-300.0), n(90.0), n(2040.0), n(2070.0), &metal)
            ]
        }),
    ));

    // A floor drain: its frame flush with the floor, a round opening, and the slats of its grate.
    let side = &width;
    let opening = void(post(n(0.0), n(0.0), f(format!("({side}) / 2 - 20")), n(-30.0), n(0.0), ""));
    let slat = |at: f64| {
        block([f(format!("-({side}) / 2 + 20")), n(at - 4.0), f(format!("{side} / 2 - 20")), n(at + 4.0)], n(-10.0), n(0.0), &metal)
    };
    out.push((
        w(msg!("Floor Drain")),
        json!({
            "category": "drain",
            "parameters": [size(&width, 150.0)],
            "types": {"150 x 150": {}, "100 x 100": {&width: 100}},
            "forms": [
                block([f(format!("-({side}) / 2")), f(format!("-({side}) / 2")), f(format!("{side} / 2")), f(format!("{side} / 2"))], n(-50.0), n(0.0), &metal),
                opening,
                slat(-20.0), slat(0.0), slat(20.0)
            ]
        }),
    ));
    out
}

/// Pipe fittings and a valve, centred on the pipe they join (its centre line at their z = 0, along x, and a branch toward +y):
/// they are components until fittings have connectors.
pub fn pipework(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (diameter, mat) = (w(msg!("Diameter")), w("Material"));
    let (pvc, metal) = (w(msg!("PVC")), w("Metal"));
    let r = format!("{diameter} / 2");
    // A hub's length past the pipe's axis: a pipe's diameter and a half, and 30 mm.
    let hub = format!("{diameter} * 1.5 + 30");
    // A pipe along x from `a` to `b`, and one along y; round, in the sketch planes that sweep along them.
    let along_x =
        |a: &str, b: &str, m: &str| json!({"plane": "yz", "circle": {"centre": [0, 0], "radius": r}, "from": a, "to": b, "material": m});
    let along_y =
        |a: &str, b: &str, m: &str| json!({"plane": "xz", "circle": {"centre": [0, 0], "radius": r}, "from": a, "to": b, "material": m});
    let sizes = |names: &[&str]| -> Value {
        names
            .iter()
            .map(|n| (n.to_string(), json!({&diameter: n.parse::<f64>().unwrap_or(50.0)})))
            .collect::<serde_json::Map<String, Value>>()
            .into()
    };
    let neg = |e: &str| format!("-({e})");
    vec![
        (
            w(msg!("Elbow 90")),
            json!({
                "category": "pipefitting",
                "parameters": [size(&diameter, 50.0), material(&mat, &pvc)],
                "types": sizes(&["25", "50", "110"]),
                "forms": [along_x(&neg(&hub), &r, &mat), along_y(&neg(&r), &hub, &mat)]
            }),
        ),
        (
            w(msg!("Tee")),
            json!({
                "category": "pipefitting",
                "parameters": [size(&diameter, 50.0), material(&mat, &pvc)],
                "types": sizes(&["25", "50", "110"]),
                "forms": [along_x(&neg(&hub), &hub, &mat), along_y("0", &hub, &mat)]
            }),
        ),
        (
            w(msg!("Gate Valve")),
            json!({
                "category": "pipeaccessory",
                "parameters": [size(&diameter, 50.0), material(&mat, &metal)],
                "types": sizes(&["25", "50"]),
                "forms": [
                    along_x(&neg(&hub), &hub, &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.7")), f(neg(&r)), f(format!("{diameter} * 0.7")), &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.25")), f(format!("{diameter} * 0.7")), f(format!("{diameter} * 1.6")), &mat),
                    post(json!(0), json!(0), f(format!("{diameter} * 0.8")), f(format!("{diameter} * 1.6")), f(format!("{diameter} * 1.7")), &mat)
                ]
            }),
        ),
    ]
}

/// Electrical equipment, luminaires and devices: a distribution board, a ceiling and a recessed luminaire, an outlet and a switch.
/// A wall device stands with its back on its x axis, its front toward -y, at the height its type says (`Mounting Height`: a board's
/// bottom, a device's centre); a luminaire hangs from the height it is given, the underside of the slab by default.
fn electrical(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height, mounting) = (w("Width"), w("Depth"), w("Height"), w(msg!("Mounting Height")));
    let (diameter, mat, metal, plaster, glass) = (w("Diameter"), w("Material"), w("Metal"), w(msg!("Plaster")), w("Glass"));
    let n = |x: f64| json!(x);
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-({width}) / 2"));
    // The mounting height, and `d` mm over it.
    let up = |d: f64| f(format!("{mounting} + ({d})"));
    let mut out = Vec::new();

    // A distribution board: a box on the wall, its door a little proud of it.
    out.push((
        w(msg!("Distribution Board")),
        json!({
            "category": "equipment",
            "parameters": [size(&width, 400.0), size(&height, 500.0), size(&depth, 120.0), size(&mounting, 1300.0), material(&mat, &metal)],
            "types": {"12 circuits": {}, "24 circuits": {&width: 500, &height: 700}, "36 circuits": {&width: 600, &height: 900}},
            "forms": [
                block([f(minus_half_w.clone()), f(format!("-({depth})")), f(half_w.clone()), n(0.0)], f(mounting.clone()), f(format!("{mounting} + {height}")), &mat),
                block(
                    [f(format!("{minus_half_w} + 15")), f(format!("-({depth}) - 12")), f(format!("{half_w} - 15")), f(format!("-({depth})"))],
                    up(15.0),
                    f(format!("{mounting} + {height} - 15")),
                    &mat
                )
            ]
        }),
    ));

    // A ceiling luminaire, round: its body against the slab and its lens under it.
    let r = format!("{diameter} / 2");
    out.push((
        w(msg!("Ceiling Luminaire")),
        json!({
            "category": "lighting",
            "parameters": [size(&diameter, 300.0), size(&mounting, 3000.0)],
            "types": {"300": {}, "400": {&diameter: 400}},
            "forms": [
                post(n(0.0), n(0.0), f(r.clone()), up(-40.0), f(mounting.clone()), &plaster),
                post(n(0.0), n(0.0), f(format!("{r} * 0.9")), up(-60.0), up(-40.0), &glass)
            ]
        }),
    ));

    // A recessed luminaire, square, flush with the ceiling's underside.
    out.push((
        w(msg!("Recessed Luminaire")),
        json!({
            "category": "lighting",
            "parameters": [size(&width, 600.0), size(&depth, 600.0), size(&mounting, 3000.0)],
            "types": {"600 x 600": {}, "1200 x 300": {&width: 1200, &depth: 300}},
            "forms": [
                block([f(minus_half_w.clone()), f(format!("-({depth}) / 2")), f(half_w.clone()), f(format!("{depth} / 2"))], up(-30.0), f(mounting.clone()), &metal),
                block(
                    [f(format!("{minus_half_w} + 15")), f(format!("-({depth}) / 2 + 15")), f(format!("{half_w} - 15")), f(format!("{depth} / 2 - 15"))],
                    up(-35.0),
                    up(-30.0),
                    &glass
                )
            ]
        }),
    ));

    // An outlet and a switch: a 100 mm plate on the wall, centred on the height its type says, a socket cut into the one and a
    // rocker on the other.
    let plate = block([n(-50.0), n(-12.0), n(50.0), n(0.0)], up(-50.0), up(50.0), &plaster);
    let socket = void(json!({"plane": "xz", "circle": {"centre": [0, mounting.clone()], "radius": 25}, "from": -12, "to": -6}));
    out.push((
        w(msg!("Outlet")),
        json!({
            "category": "outlet",
            "parameters": [size(&mounting, 300.0)],
            "types": {"Low 300": {}, "Medium 1100": {&mounting: 1100}, "High 2100": {&mounting: 2100}},
            "forms": [plate.clone(), socket]
        }),
    ));
    out.push((
        w(msg!("Switch")),
        json!({
            "category": "switch",
            "parameters": [size(&mounting, 1100.0)],
            "types": {"Medium 1100": {}, "Low 300": {&mounting: 300}},
            "forms": [plate, block([n(-12.0), n(-18.0), n(12.0), n(-12.0)], up(-30.0), up(30.0), &plaster)]
        }),
    ));
    out
}

/// Lightning protection (NBR 5419): an air terminal, an earthing electrode and an inspection box. A terminal stands on its base; a
/// rod and an inspection box go down from the level they are placed on, which is where the ground is.
fn lightning(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (height, diameter, mat, metal, concrete) = (w("Height"), w("Diameter"), w("Material"), w("Metal"), w("Concrete"));
    let n = |x: f64| json!(x);
    // Types named by one size each, that parameter set to it.
    let sizes = |name: &str, items: &[(&str, f64)]| -> Value {
        items.iter().map(|(k, v)| (k.to_string(), json!({name: v}))).collect::<serde_json::Map<String, Value>>().into()
    };
    vec![
        // A Franklin air terminal: a rod on a round base, as tall as `Height`.
        (
            w(msg!("Air Terminal")),
            json!({
                "category": "airterminal",
                "parameters": [size(&height, 500.0), size(&diameter, 10.0), material(&mat, &metal)],
                "types": sizes(&height, &[("500", 500.0), ("1000", 1000.0), ("2000", 2000.0)]),
                "forms": [
                    post(n(0.0), n(0.0), n(60.0), n(0.0), n(15.0), &mat),
                    post(n(0.0), n(0.0), f(format!("{diameter} / 2")), n(15.0), f(height.clone()), &mat)
                ]
            }),
        ),
        // An earthing electrode: a rod driven into the ground, its head at the level.
        (
            w(msg!("Earthing Electrode")),
            json!({
                "category": "earthing",
                "parameters": [size(&height, 2400.0), size(&diameter, 16.0), material(&mat, &metal)],
                "types": sizes(&height, &[("2400", 2400.0), ("3000", 3000.0)]),
                "forms": [post(n(0.0), n(0.0), f(format!("{diameter} / 2")), f(format!("-({height})")), n(0.0), &mat)]
            }),
        ),
        // An inspection box: a round box in the ground, open at the top, its rim flush with the level.
        (
            w(msg!("Inspection Box")),
            json!({
                "category": "inspectionbox",
                "parameters": [size(&diameter, 300.0), size(&height, 300.0), material(&mat, &concrete)],
                "types": sizes(&diameter, &[("300", 300.0), ("250", 250.0)]),
                "forms": [
                    post(n(0.0), n(0.0), f(format!("{diameter} / 2")), f(format!("-({height})")), n(0.0), &mat),
                    void(post(n(0.0), n(0.0), f(format!("{diameter} / 2 - 20")), f(format!("-({height}) + 20")), n(0.0), ""))
                ]
            }),
        ),
    ]
}

/// Data and voice (NBR 14565): outlets, a patch panel and a rack. A wall device stands as an electrical one does, its back on its
/// x axis, at the height its type says, here its bottom.
fn telecom(language: Language) -> Vec<(String, Value)> {
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height, mounting) = (w("Width"), w("Depth"), w("Height"), w("Mounting Height"));
    let (mat, metal, plaster) = (w("Material"), w("Metal"), w("Plaster"));
    let n = |x: f64| json!(x);
    // The mounting height, and `d` mm over it.
    let up = |d: f64| f(format!("{mounting} + ({d})"));
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-({width}) / 2"));
    // A wall plate with a jack opening cut into it for each of `ports`.
    let plate = |ports: usize| {
        let mut forms = vec![block([n(-42.0), n(-12.0), n(42.0), n(0.0)], f(mounting.clone()), up(85.0), &plaster)];
        for k in 0..ports {
            let x = (k as f64 - (ports as f64 - 1.0) / 2.0) * 28.0;
            forms.push(void(block([n(x - 8.0), n(-12.0), n(x + 8.0), n(-5.0)], up(30.0), up(55.0), "")));
        }
        forms
    };
    vec![
        (
            w(msg!("Data Outlet")),
            json!({
                "category": "dataoutlet",
                "parameters": [size(&mounting, 300.0)],
                "types": {"Low 300": {}, "Medium 1100": {&mounting: 1100}},
                "forms": plate(2)
            }),
        ),
        (
            w(msg!("Voice Outlet")),
            json!({
                "category": "voiceoutlet",
                "parameters": [size(&mounting, 300.0)],
                "types": {"Low 300": {}, "Medium 1100": {&mounting: 1100}},
                "forms": plate(1)
            }),
        ),
        // A patch panel, 19 inches wide and one unit high, on the wall.
        (
            w(msg!("Patch Panel")),
            json!({
                "category": "patchpanel",
                "parameters": [size(&mounting, 1500.0), material(&mat, &metal)],
                "types": {"24 ports": {}, "48 ports": {}},
                "forms": [block([n(-241.5), n(-50.0), n(241.5), n(0.0)], f(mounting.clone()), up(44.0), &mat)]
            }),
        ),
        // A floor rack, 19 inches: its frame and its front door.
        (
            w("Rack"),
            json!({
                "category": "rack",
                "parameters": [size(&width, 600.0), size(&depth, 1000.0), size(&height, 2000.0), material(&mat, &metal)],
                "types": {"600 x 1000": {}, "800 x 1000": {&width: 800}, "600 x 800": {&depth: 800}},
                "forms": [
                    block([f(minus_half_w.clone()), f(format!("-({depth})")), f(half_w.clone()), n(0.0)], n(0.0), f(height.clone()), &mat),
                    block(
                        [f(format!("{minus_half_w} + 20")), f(format!("-({depth}) - 25")), f(format!("{half_w} - 20")), f(format!("-({depth})"))],
                        n(60.0),
                        f(format!("{height} - 40")),
                        &plaster
                    )
                ]
            }),
        ),
    ]
}
