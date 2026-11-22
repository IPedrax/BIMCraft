//! What a family document selects, and the selection commands a family document has on it (FE-1, FE-20), for any front
//! end: what one thing selected is ([`Picked`]); Filter's kinds, each with what of the selection is of it ([`groups`],
//! [`kept`]); and Select by ID, the parts a list of ids and GUIDs names ([`find`]) and what names a part ([`names`], IDs of
//! Selection).
//!
//! A part is named as an element is: by its id (six digits, given when the family is loaded into a project: one made since
//! has none yet) or its GUID, however written (`inquiry`'s words).

use bimcraft_i18n::ui_msg;

use super::Family;
use super::forms::FormKind;
use crate::ids::{Guid, Id};

/// One thing selected in a family document that isn't adaptive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Picked {
    /// A form, by its place in the family's forms.
    Form(usize),
    /// A reference plane, a datum or the family's own.
    Plane(Guid),
    /// A reference line.
    Line(Guid),
    Dimension(Guid),
    /// A plan symbol, by its place in the family's symbols.
    Symbol(usize),
    /// A connector, by its place in the family's connectors.
    Connector(usize),
    /// A nested instance, by its place in the family's nested instances.
    Nested(usize),
}

/// What Filter counts a selection by, as the project's Filter counts by category: a form by what it is, solid or void, and
/// the other things a family document selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A solid form.
    Form(FormKind),
    /// A void form.
    Void(FormKind),
    /// A form of the triangles a file brought, solid or void.
    Mesh,
    ReferencePlane,
    ReferenceLine,
    Dimension,
    Symbol,
    Connector,
    Nested,
}

impl Kind {
    /// Where Filter lists it, the same in every language: the forms first, each kind solid then void as the Create tab
    /// has them (Extrusion, Blend, Revolve, Sweep, Swept Blend), and a file's meshes; then the datums (reference planes,
    /// then lines), dimensions, symbols, connectors and nested instances, in the order a family's lists hold them.
    pub fn rank(self) -> u8 {
        let form = |k: FormKind| match k {
            FormKind::Extrusion => 0,
            FormKind::Blend => 1,
            FormKind::Revolve => 2,
            FormKind::Sweep => 3,
            FormKind::SweptBlend => 4,
        };
        match self {
            Kind::Form(k) => form(k),
            Kind::Void(k) => 5 + form(k),
            Kind::Mesh => 10,
            Kind::ReferencePlane => 11,
            Kind::ReferenceLine => 12,
            Kind::Dimension => 13,
            Kind::Symbol => 14,
            Kind::Connector => 15,
            Kind::Nested => 16,
        }
    }

    /// What Filter's list calls it, in English: several of it, as a list of categories names them.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Form(FormKind::Extrusion) => ui_msg!("Extrusions"),
            Kind::Form(FormKind::Blend) => ui_msg!("Blends"),
            Kind::Form(FormKind::Revolve) => ui_msg!("Revolves"),
            Kind::Form(FormKind::Sweep) => ui_msg!("Sweeps"),
            Kind::Form(FormKind::SweptBlend) => ui_msg!("Swept Blends"),
            Kind::Void(FormKind::Extrusion) => ui_msg!("Void Extrusions"),
            Kind::Void(FormKind::Blend) => ui_msg!("Void Blends"),
            Kind::Void(FormKind::Revolve) => ui_msg!("Void Revolves"),
            Kind::Void(FormKind::Sweep) => ui_msg!("Void Sweeps"),
            Kind::Void(FormKind::SweptBlend) => ui_msg!("Void Swept Blends"),
            Kind::Mesh => ui_msg!("Meshes"),
            Kind::ReferencePlane => ui_msg!("Reference Planes"),
            Kind::ReferenceLine => ui_msg!("Reference Lines"),
            Kind::Dimension => ui_msg!("Dimensions"),
            Kind::Symbol => ui_msg!("Symbols"),
            Kind::Connector => ui_msg!("Connectors"),
            Kind::Nested => ui_msg!("Nested Instances"),
        }
    }
}

/// The id and the GUID `p` holds in `family`: none where the family hasn't it. An id of 0 and a nil GUID are ones not given
/// yet.
fn ident(family: &Family, p: Picked) -> Option<(Id, Guid)> {
    match p {
        Picked::Form(i) => family.forms.get(i).map(|f| (f.id, f.guid)),
        Picked::Plane(g) => family.planes.iter().find(|q| q.guid == g).map(|q| (q.id, q.guid)),
        Picked::Line(g) => family.lines.iter().find(|l| l.guid == g).map(|l| (l.id, l.guid)),
        Picked::Dimension(g) => family.dimensions.iter().find(|d| d.guid == g).map(|d| (d.id, d.guid)),
        Picked::Symbol(i) => family.symbols.get(i).map(|y| (y.id, y.guid)),
        Picked::Connector(i) => family.connectors.get(i).map(|c| (c.id, c.guid)),
        Picked::Nested(i) => family.nested_instances.get(i).map(|n| (n.id, n.guid)),
    }
}

/// Whether `family` has `p`.
pub fn holds(family: &Family, p: Picked) -> bool {
    ident(family, p).is_some()
}

/// What `p` is, for Filter: none where `family` hasn't it.
pub fn kind(family: &Family, p: Picked) -> Option<Kind> {
    if !holds(family, p) {
        return None;
    }
    Some(match p {
        Picked::Form(i) => match family.forms.get(i)? {
            f if !f.mesh.is_empty() => Kind::Mesh,
            f if f.void => Kind::Void(f.kind()),
            f => Kind::Form(f.kind()),
        },
        Picked::Plane(_) => Kind::ReferencePlane,
        Picked::Line(_) => Kind::ReferenceLine,
        Picked::Dimension(_) => Kind::Dimension,
        Picked::Symbol(_) => Kind::Symbol,
        Picked::Connector(_) => Kind::Connector,
        Picked::Nested(_) => Kind::Nested,
    })
}

/// A kind the selection holds, and what of the selection is of it, in the order selected.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub kind: Kind,
    pub items: Vec<Picked>,
}

/// Filter's list for `picked`, a selection of `family`'s: each kind it holds with what is of it (each thing once), the kinds
/// in an order of their own ([`Kind::rank`]), never by their names, which a translation would reorder. What the family
/// hasn't is left out.
pub fn groups(family: &Family, picked: &[Picked]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for p in picked {
        let Some(kind) = kind(family, *p) else { continue };
        match out.iter_mut().find(|g| g.kind == kind) {
            Some(g) if g.items.contains(p) => {}
            Some(g) => g.items.push(*p),
            None => out.push(Group { kind, items: vec![*p] }),
        }
    }
    out.sort_by_key(|g| g.kind.rank());
    out
}

/// What of `picked` is of one of the kinds `keep`: the selection Filter leaves, in the order selected, each thing once.
pub fn kept(family: &Family, picked: &[Picked], keep: &[Kind]) -> Vec<Picked> {
    let mut out: Vec<Picked> = Vec::new();
    for p in picked {
        if kind(family, *p).is_some_and(|k| keep.contains(&k)) && !out.contains(p) {
            out.push(*p);
        }
    }
    out
}

/// What a list found in a family: its parts, each once, in the order named, and what named none.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub picked: Vec<Picked>,
    pub missing: Vec<String>,
}

/// Everything `family` holds that a front end selects, in the order its lists hold it.
pub fn all(family: &Family) -> Vec<Picked> {
    let mut out: Vec<Picked> = (0..family.forms.len()).map(Picked::Form).collect();
    out.extend(family.planes.iter().map(|p| Picked::Plane(p.guid)));
    out.extend(family.lines.iter().map(|l| Picked::Line(l.guid)));
    out.extend(family.dimensions.iter().map(|d| Picked::Dimension(d.guid)));
    out.extend((0..family.symbols.len()).map(Picked::Symbol));
    out.extend((0..family.connectors.len()).map(Picked::Connector));
    out.extend((0..family.nested_instances.len()).map(Picked::Nested));
    out
}

/// The parts of `family` `text` names, as Select by ID finds a project's elements (`inquiry::find`): by id or by GUID
/// (dashed or not, braced or not, in either case, or as an IFC GlobalId's 22 characters), any number of them, separated by
/// spaces, commas, semicolons or lines; too long a list is cut as a project's is. A part with no id yet is found by its
/// GUID alone, one with neither by nothing.
pub fn find(family: &Family, text: &str) -> Found {
    let parts: Vec<(Picked, Id, Guid)> = all(family).into_iter().filter_map(|p| ident(family, p).map(|(id, g)| (p, id, g))).collect();
    let mut out = Found::default();
    for word in crate::inquiry::words(text) {
        let named = crate::inquiry::guid_of(word).and_then(|g| {
            parts.iter().find(|(_, id, guid)| match g.pending() {
                Some(n) => !crate::ids::unset(id) && *id == n,
                None => !guid.is_nil() && *guid == g,
            })
        });
        match named {
            Some((p, ..)) if out.picked.contains(p) => {}
            Some((p, ..)) => out.picked.push(*p),
            None => out.missing.push(word.to_string()),
        }
    }
    out
}

/// What names a part of a family: its id (0: none yet) and its GUID (nil: none yet), and what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Names {
    pub picked: Picked,
    pub kind: Kind,
    pub id: Id,
    pub guid: Guid,
}

/// What names `p` in `family`; none where the family hasn't it.
pub fn names(family: &Family, p: Picked) -> Option<Names> {
    let (id, guid) = ident(family, p)?;
    Some(Names { picked: p, kind: kind(family, p)?, id, guid })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const EDGE: &str = "fe200000-0000-4000-8000-000000000001";
    const LINE: &str = "fe200000-0000-4000-8000-000000000002";
    const DIM: &str = "fe200000-0000-4000-8000-000000000003";
    const SOLID: &str = "fe200000-0000-4000-8000-000000000004";

    fn g(s: &str) -> Guid {
        Guid::parse(s).unwrap()
    }

    /// Two extrusions (the first with an id and a GUID, as a project gives them), a void extrusion and a revolve; a plane of
    /// its own, a reference line, a dimension, two symbols and a connector; a small box nested, placed three times, one of
    /// another type and one switched off.
    fn family() -> Family {
        let square = |x: f64| json!([[x, 0], [x + 100.0, 0], [x + 100.0, 100], [x, 100]]);
        let small = json!({"category": "generic", "parameters": [{"name": "W", "value": 100}], "types": {"S": {}, "L": {"W": 300}},
            "forms": [{"profile": [[0, 0], ["W", 0], ["W", 100], [0, 100]], "to": 100}]});
        serde_json::from_value(json!({
            "category": "generic",
            "parameters": [{"name": "Shown", "kind": "yesno", "value": false}],
            "types": {"T": {}},
            "forms": [
                {"id": 123456, "guid": SOLID, "profile": square(0.0), "to": 100},
                {"profile": square(200.0), "to": 100},
                {"profile": square(400.0), "to": 100, "void": true},
                {"profile": square(600.0), "axis": [[590, 0], [590, 100]], "from": 0, "to": 180},
            ],
            "planes": [{"guid": EDGE, "id": 123457, "name": "Edge", "start": [-400, -1000, 0], "end": [-400, 1000, 0], "up": [0, 0, 1]}],
            "lines": [{"guid": LINE, "start": [-200, 900, 0], "end": [200, 900, 0]}],
            "dimensions": [{"guid": DIM, "refs": [{"plane": EDGE}, {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}], "view": "plan", "at": [-200, -1200, 0]}],
            "symbols": [{"line": [[-100, -700], [100, -700]]}, {"dot": {"centre": [0, -800], "radius": 20}}],
            "connectors": [{"kind": "power"}],
            "nested": [{"name": "Small", "family": small}],
            "nested_instances": [
                {"family": "Small", "type": "S", "at": [1000, 0]},
                {"family": "Small", "type": "L", "at": [1000, 500]},
                {"family": "Small", "type": "S", "at": [1000, 1000]},
                {"family": "Small", "type": "S", "at": [1000, 1500], "visible": "Shown"},
            ],
        }))
        .unwrap()
    }

    #[test]
    fn filter_counts_a_selection_by_kind_and_keeps_the_kinds_checked_in_the_order_selected() {
        let f = family();
        let picked = [
            Picked::Symbol(1),
            Picked::Form(1),
            Picked::Plane(g(EDGE)),
            Picked::Form(2),
            Picked::Nested(0),
            Picked::Form(0),
            Picked::Form(3),
            Picked::Dimension(g(DIM)),
            Picked::Line(g(LINE)),
            Picked::Connector(0),
            Picked::Symbol(0),
            Picked::Form(1),
            Picked::Form(9),
            Picked::Plane(g(LINE)),
        ];
        let listed: Vec<(&str, usize)> = groups(&f, &picked).iter().map(|g| (g.kind.name(), g.items.len())).collect();
        assert_eq!(
            listed,
            [
                ("Extrusions", 2),
                ("Revolves", 1),
                ("Void Extrusions", 1),
                ("Reference Planes", 1),
                ("Reference Lines", 1),
                ("Dimensions", 1),
                ("Symbols", 2),
                ("Connectors", 1),
                ("Nested Instances", 1),
            ],
            "each kind once, in an order of their own that no language changes: forms, solid then void, then datums, dimensions, \
             symbols, connectors and nested instances; what the family hasn't and what is there twice left out"
        );
        let extrusions = groups(&f, &picked).into_iter().find(|g| g.kind == Kind::Form(FormKind::Extrusion)).unwrap();
        assert_eq!(extrusions.items, [Picked::Form(1), Picked::Form(0)], "in the order selected");
        assert_eq!(
            kept(&f, &picked, &[Kind::Symbol, Kind::Form(FormKind::Extrusion)]),
            [Picked::Symbol(1), Picked::Form(1), Picked::Form(0), Picked::Symbol(0)]
        );
        assert!(kept(&f, &picked, &[]).is_empty() && kept(&f, &picked, &[Kind::Mesh]).is_empty());
        assert!(groups(&f, &[]).is_empty() && groups(&Family::default(), &picked).is_empty());
        // A mesh a file brought is one, solid or void.
        let mut meshed = f.clone();
        meshed.forms[2].mesh = std::sync::Arc::new(vec![[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
        assert_eq!(kind(&meshed, Picked::Form(2)), Some(Kind::Mesh));
        assert_eq!(kind(&f, Picked::Form(2)), Some(Kind::Void(FormKind::Extrusion)));
        assert_eq!(kind(&f, Picked::Form(9)), None);
    }

    #[test]
    fn a_part_is_found_by_its_id_or_its_guid_however_written_and_what_names_none_is_said() {
        let f = family();
        let ifc = crate::ifc::compress(g(LINE).as_u128());
        let text = format!("123456, {EDGE};{}\n{{{DIM}}} {ifc} nowhere 100099 123456 0", SOLID.replace('-', "").to_uppercase());
        assert_eq!(
            find(&f, &text),
            Found {
                picked: vec![Picked::Form(0), Picked::Plane(g(EDGE)), Picked::Dimension(g(DIM)), Picked::Line(g(LINE))],
                missing: vec!["nowhere".into(), "100099".into(), "0".into()],
            },
            "each once, in the order named; an id of none (0) names nothing"
        );
        assert_eq!(find(&f, "123457").picked, [Picked::Plane(g(EDGE))], "a plane by its id");
        assert_eq!(find(&f, " \n,, ;"), Found::default());
        // What has neither an id nor a GUID yet is found by nothing: the nil GUID names none of them.
        assert_eq!(find(&f, "00000000-0000-0000-0000-000000000000").picked, []);
        // A hostile list is cut short, never a hang or a panic.
        let many = (0..2 * crate::inquiry::MAX_NAMES).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(find(&f, &many).missing.len(), crate::inquiry::MAX_NAMES);
        assert!(find(&f, &"é".repeat(crate::inquiry::MAX_LIST)).picked.is_empty());
    }

    #[test]
    fn a_part_is_named_by_its_id_and_its_guid_or_by_neither_until_it_is_given_them() {
        let f = family();
        let n = names(&f, Picked::Form(0)).unwrap();
        assert_eq!((n.id, n.guid, n.kind), (123_456, g(SOLID), Kind::Form(FormKind::Extrusion)));
        let n = names(&f, Picked::Symbol(0)).unwrap();
        assert!(crate::ids::unset(&n.id) && n.guid.is_nil(), "made since the family was loaded: none yet");
        let n = names(&f, Picked::Line(g(LINE))).unwrap();
        assert_eq!((n.id, n.guid, n.kind), (0, g(LINE), Kind::ReferenceLine));
        assert_eq!(names(&f, Picked::Nested(7)), None);
        assert_eq!(all(&f).len(), 4 + 4 + 1 + 1 + 2 + 1 + 4, "the datums are planes too");
        assert!(all(&f).into_iter().all(|p| holds(&f, p) && names(&f, p).is_some()));
    }
}
