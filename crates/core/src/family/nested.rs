//! Nested families (T6a): families loaded into a family, and their instances placed in it, as Revit's Insert > Load Family and
//! Component do inside the Family Editor. A host carries each nested family's whole definition (`NestedFamily`), so a family
//! file holds what it nests; an instance (`NestedInstance`) places one of its types on a plane of the host, at a point of that
//! plane, an offset along its normal and a turn about it, its parameters set by numbers or formulas over the host's (Revit's
//! "=", Associate Family Parameter). The host's pieces and marks take the instances' own, so everything that reads
//! [`Family::pieces_at`] (plans, sections, the 3D view, IFC, `render`) shows them.
//!
//! A nested family's own `shared` option makes it shared: loading the host into a project (or into another family) loads its
//! shared nested families there too, asking about the ones the target has defined otherwise ("Shared Family Already Loaded"),
//! and one definition of a shared family is kept across the target. The instances are built into the host either way: a
//! shared instance as an element of its own in the project (scheduled, tagged, selected apart) is T6b's.
use super::*;
use crate::ids::{Guid, Id};
use crate::view3d::{dot, sub};

/// How many levels of nested families a project's family may hold under it.
pub const MAX_NESTING: usize = 5;
/// How many families one family may nest.
pub const MAX_NESTED: usize = 32;
/// How many nested instances one family may place.
pub const MAX_NESTED_INSTANCES: usize = 256;
/// How many nested definitions one family's tree may hold, at every depth.
pub const MAX_NESTED_TOTAL: usize = 128;
/// How many nested instances one build of a family may build, at every depth: what a hostile tree (a thousand instances of a
/// thousand instances) is stopped by.
pub const MAX_NESTED_BUILDS: usize = 2048;

/// A family loaded into another: its name there and its whole definition, carried inside the host.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedFamily {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    pub name: String,
    pub family: Family,
}

/// An instance of a nested family placed in its host: a type of `family`, on the host's plane `host` (Ref. Level when none) at
/// `at` in that plane's axes, `offset` along its normal and turned `angle` degrees about it; `visible` whether it is there.
/// `params` sets the nested family's parameters: a number, or a formula over the host's parameters (`Width / 2`); a material,
/// a text or a URL parameter names a host parameter of its kind, or holds the material's name or the words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedInstance {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    pub family: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub host: PlaneRef,
    #[serde(default)]
    pub at: [Expr; 2],
    #[serde(default)]
    pub offset: Expr,
    #[serde(default)]
    pub angle: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Expr>,
}

impl NestedInstance {
    /// Every formula of its own place: `at`, `offset`, `angle` and `visible` (its parameters' are [`Family::instance_exprs`]').
    pub fn place_exprs(&self) -> impl Iterator<Item = &Expr> {
        self.at.iter().chain([&self.offset, &self.angle]).chain(&self.visible)
    }
}

/// Which version of a shared nested family a target keeps when the family loaded has it otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keep {
    /// The target's own, as it is: the family loaded takes it.
    #[default]
    Project,
    /// The one in the family loaded, overwriting the target's.
    Family,
}

/// What a build of nested instances may still cost.
#[derive(Debug, Default)]
pub struct Meter {
    builds: usize,
}

impl Meter {
    fn spend(&mut self) -> Result<(), String> {
        self.builds += 1;
        if self.builds > MAX_NESTED_BUILDS {
            Err(format!("its nested instances build more than {MAX_NESTED_BUILDS} instances, at every depth"))
        } else {
            Ok(())
        }
    }
}

/// Where an instance stands in its host: its origin and axes in the host's, and whether they are mirrored.
#[derive(Clone, Copy, Debug)]
struct Placed {
    origin: P3,
    x: P3,
    y: P3,
    z: P3,
}

impl Placed {
    fn point(&self, p: P3) -> P3 {
        std::array::from_fn(|k| self.origin[k] + self.x[k] * p[0] + self.y[k] * p[1] + self.z[k] * p[2])
    }

    fn right_handed(&self) -> bool {
        dot(cross(self.x, self.y), self.z) > 0.0
    }

    /// Whether it stands upright: its z the host's, so its plan marks are drawn in the host's plan.
    fn level(&self) -> bool {
        self.z[2] > 1.0 - 1e-9
    }
}

/// A nested instance worked out: its family, that family's values, and where it stands.
type Instanced<'a> = (&'a Family, BTreeMap<String, Val>, Placed);

/// The nested family named `name` in `list`.
pub fn find<'a>(list: &'a [NestedFamily], name: &str) -> Option<&'a NestedFamily> {
    list.iter().find(|n| n.name == name)
}

impl Family {
    /// The family it nests under `name`.
    pub fn nested_family(&self, name: &str) -> Option<&NestedFamily> {
        find(&self.nested, name)
    }

    /// Instance `inst`'s parameter values worked out over the host's `values`: the nested family's parameter, its kind, and
    /// what it comes to. A text-like parameter takes a host parameter's words where it names one, else its own.
    fn instance_seeds(
        &self,
        inst: &NestedInstance,
        nested: &Family,
        values: &BTreeMap<String, Val>,
    ) -> Result<BTreeMap<String, Val>, String> {
        let mut out = BTreeMap::new();
        let mut read = HashMap::new();
        for (name, e) in &inst.params {
            let p = nested
                .parameters
                .iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("`{name}` isn't a parameter of nested family `{}`", inst.family))?;
            let v = if p.kind.text() {
                match e {
                    Expr::Formula(s) => match values.get(s.as_str()) {
                        Some(Val::Text(t)) if self.parameters.iter().any(|q| q.name == *s) => Val::Text(t.clone()),
                        _ => Val::Text(s.clone()),
                    },
                    Expr::Number(x) => Val::Text(x.to_string()),
                }
            } else {
                let x = number(e, values, &mut read).map_err(|e| format!("`{name}`: {e}"))?;
                Val::Number(if p.kind == Kind::Integer { x.round() } else { x })
            };
            out.insert(name.clone(), v);
        }
        Ok(out)
    }

    /// Nested instance `i`, worked out for the host's `values`: its family, that family's values for its type and its
    /// parameters, and where it stands; none where `visible` says it isn't there.
    fn placed_instance(&self, i: usize, values: &BTreeMap<String, Val>) -> Result<Option<Instanced<'_>>, String> {
        let inst = &self.nested_instances[i];
        let mut read = HashMap::new();
        if let Some(v) = &inst.visible
            && number(v, values, &mut read)? == 0.0
        {
            return Ok(None);
        }
        let nested = self.nested_family(&inst.family).ok_or_else(|| format!("it places `{}`, which it doesn't nest", inst.family))?;
        let seeds = self.instance_seeds(inst, &nested.family, values)?;
        let wall = match values.get(WALL) {
            Some(Val::Number(t)) => *t,
            _ => DEFAULT_WALL,
        };
        let own =
            nested.family.values_seeded(&inst.type_name, &BTreeMap::new(), wall, &seeds).map_err(|e| format!("`{}`: {e}", inst.family))?;
        let frame = self.frame(&inst.host).map_err(|e| format!("its host: {e}"))?;
        let (x, y) = (number(&inst.at[0], values, &mut read)?, number(&inst.at[1], values, &mut read)?);
        let offset = number(&inst.offset, values, &mut read)?;
        let angle = number(&inst.angle, values, &mut read)?;
        let (s, c) = angle.to_radians().sin_cos();
        let placed = Placed {
            origin: frame.point(P2(x, y), offset),
            x: add(mul(frame.x, c), mul(frame.y, s)),
            y: add(mul(frame.x, -s), mul(frame.y, c)),
            z: frame.z,
        };
        Ok(Some((&nested.family, own, placed)))
    }

    /// Its nested instances' pieces, sized by `values` (the host's), where each stands in it.
    pub(super) fn nested_pieces(&self, values: &BTreeMap<String, Val>, meter: &mut Meter) -> Result<Vec<Piece>, String> {
        let mut out = Vec::new();
        for i in 0..self.nested_instances.len() {
            out.extend(self.instance_built(i, values, meter).map_err(|e| format!("nested instance {}: {e}", i + 1))?);
        }
        Ok(out)
    }

    /// Nested instance `i`'s pieces, sized by `values` (the host's), where it stands in the host flexed for them: what a view
    /// picks it by. Empty where it isn't there or doesn't build.
    pub fn instance_pieces(&self, i: usize, values: &BTreeMap<String, Val>) -> Vec<Piece> {
        let Ok(flexed) = self.flexed(values) else { return Vec::new() };
        flexed.instance_built(i, values, &mut Meter::default()).unwrap_or_default()
    }

    /// Nested instance `i`'s pieces, sized by `values`, in the host's axes.
    fn instance_built(&self, i: usize, values: &BTreeMap<String, Val>, meter: &mut Meter) -> Result<Vec<Piece>, String> {
        let mut out = Vec::new();
        if i >= self.nested_instances.len() {
            return Ok(out);
        }
        let Some((nested, own, placed)) = self.placed_instance(i, values)? else { return Ok(out) };
        meter.spend()?;
        let pieces = nested.pieces_metered(&own, None, meter)?;
        let mirrored = !placed.right_handed();
        for piece in pieces {
            let mut tris: Vec<[P3; 3]> = piece.tris.iter().map(|t| t.map(|p| placed.point(p))).collect();
            if mirrored {
                tris.iter_mut().for_each(|t| t.swap(1, 2));
            }
            let (bottom, top) = heights(&tris);
            let at = |ring: &[P2]| -> Vec<P2> { ring.iter().map(|p| placed.point([p.0, p.1, 0.0])).map(|q| P2(q[0], q[1])).collect() };
            let (outline, holes) = if placed.level() {
                (at(&piece.outline), piece.holes.iter().map(|h| at(h)).collect())
            } else {
                (crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect()), Vec::new())
            };
            out.push(Piece { tris, caps: piece.caps, outline, holes, bottom, top, material: piece.material, round: piece.round });
        }
        Ok(out)
    }

    /// Its nested instances' plan marks, sized by `values`, for those that stand upright on a level plane.
    pub(super) fn nested_marks(&self, values: &BTreeMap<String, Val>, meter: &mut Meter) -> Result<Vec<Tagged>, String> {
        let mut out = Vec::new();
        for i in 0..self.nested_instances.len() {
            let who = |e: String| format!("nested instance {}: {e}", i + 1);
            let Some((nested, own, placed)) = self.placed_instance(i, values).map_err(who)? else { continue };
            if !placed.level() {
                continue;
            }
            meter.spend()?;
            let to = |p: P2| {
                let q = placed.point([p.0, p.1, 0.0]);
                P2(q[0], q[1])
            };
            out.extend(nested.marks_metered(&own, meter).map_err(who)?.into_iter().map(|(m, sub)| (m.map(to), sub)));
        }
        Ok(out)
    }

    /// Whether it nests a family named `name`, at any depth.
    pub fn nests(&self, name: &str) -> bool {
        self.nested.iter().any(|n| n.name == name || n.family.nests(name))
    }

    /// The shared families nested in it, each once (the first met), reached through shared ones only: a family that isn't
    /// shared keeps what it nests to itself.
    pub fn shared_within(&self) -> Vec<(String, Family)> {
        let mut out: Vec<(String, Family)> = Vec::new();
        let mut stack: Vec<&NestedFamily> = self.nested.iter().rev().collect();
        let mut seen = 0;
        while let Some(n) = stack.pop() {
            seen += 1;
            if seen > MAX_NESTED_TOTAL * 4 {
                break;
            }
            if !n.family.shared {
                continue;
            }
            if !out.iter().any(|(name, _)| *name == n.name) {
                out.push((n.name.clone(), n.family.clone()));
            }
            stack.extend(n.family.nested.iter().rev());
        }
        out
    }

    /// Every shared family nested in it named `name`, at any depth, made `definition`: one definition of a shared family.
    /// Whether any was.
    pub fn replace_shared(&mut self, name: &str, definition: &Family) -> bool {
        replace_shared(&mut self.nested, name, definition, MAX_NESTING + 1)
    }

    /// Its parameters an instance's `params` name that hold words (a material, text, a URL): what is no formula there.
    pub fn instance_words<'a>(&self, inst: &'a NestedInstance) -> Vec<&'a str> {
        let Some(n) = self.nested_family(&inst.family) else { return Vec::new() };
        inst.params.keys().filter(|k| n.family.parameters.iter().any(|p| p.name == **k && p.kind.text())).map(String::as_str).collect()
    }

    /// Each formula instance `inst`'s parameters hold over the host's, but the words (a material, text, a URL), which name a
    /// host parameter or are words.
    pub fn instance_exprs<'a>(&self, inst: &'a NestedInstance) -> Vec<(&'a str, &'a Expr)> {
        let words = self.instance_words(inst);
        inst.params.iter().filter(|(k, _)| !words.contains(&k.as_str())).map(|(k, e)| (k.as_str(), e)).collect()
    }

    /// Whether its nested families and instances are sound, `chain` the names of the families it is nested in (its own
    /// first, where known) and `total` the definitions counted so far in the tree: names unique and none of the chain's (no
    /// family is loaded into itself), within the caps, each definition sound in turn; each instance of a family it nests and
    /// one of its types, on one of its planes, setting parameters that family has and may be set.
    pub(super) fn check_nested(
        &self,
        has_material: &dyn Fn(&str) -> bool,
        chain: &mut Vec<String>,
        total: &mut usize,
    ) -> Result<(), String> {
        if self.nested.len() > MAX_NESTED {
            return Err(format!("it nests more than {MAX_NESTED} families"));
        }
        if self.nested_instances.len() > MAX_NESTED_INSTANCES {
            return Err(format!("it places more than {MAX_NESTED_INSTANCES} nested instances"));
        }
        if !self.nested.is_empty() && chain.len() > MAX_NESTING {
            return Err(format!("its families are nested more than {MAX_NESTING} deep"));
        }
        for (i, n) in self.nested.iter().enumerate() {
            let who = format!("nested family `{}`", n.name.chars().take(40).collect::<String>());
            if n.name.trim().is_empty()
                || n.name.trim() != n.name
                || n.name.chars().count() > MAX_TEXT
                || n.name.chars().any(char::is_control)
            {
                return Err(format!("nested family {}: a name of 1 to {MAX_TEXT} characters on one line, no spaces round it", i + 1));
            }
            if self.nested[..i].iter().any(|m| m.name == n.name) {
                return Err(format!("{who}: two nested families have that name"));
            }
            if chain.contains(&n.name) {
                return Err(format!("{who}: a family can't be loaded into itself, nor into a family it nests"));
            }
            *total += 1;
            if *total > MAX_NESTED_TOTAL {
                return Err(format!("its tree holds more than {MAX_NESTED_TOTAL} nested families"));
            }
            chain.push(n.name.clone());
            let checked = n.family.check_in(has_material, chain, total);
            chain.pop();
            checked.map_err(|e| format!("{who}: {e}"))?;
        }
        let long = |e: &Expr| matches!(e, Expr::Formula(f) if f.chars().count() > MAX_FORMULA);
        for (i, inst) in self.nested_instances.iter().enumerate() {
            let who = format!("nested instance {}", i + 1);
            let n = self.nested_family(&inst.family).ok_or_else(|| {
                format!("{who}: it places `{}`, which the family doesn't nest", inst.family.chars().take(40).collect::<String>())
            })?;
            if !n.family.types.contains_key(&inst.type_name) {
                let names: Vec<&str> = n.family.types.keys().map(String::as_str).collect();
                return Err(format!(
                    "{who}: `{}` has no type `{}` (it has {})",
                    inst.family,
                    inst.type_name.chars().take(40).collect::<String>(),
                    names.join(", ")
                ));
            }
            self.frame(&inst.host).map_err(|e| format!("{who}: its host: {e}"))?;
            if inst.place_exprs().any(long) || inst.params.values().any(long) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
            for (name, e) in &inst.params {
                let p = n.family.parameters.iter().find(|p| p.name == *name).ok_or_else(|| {
                    format!("{who}: `{}` isn't a parameter of `{}`", name.chars().take(40).collect::<String>(), inst.family)
                })?;
                if p.formula.is_some() {
                    return Err(format!("{who}: `{name}` is worked out by a formula of `{}`: it can't be set from here", inst.family));
                }
                if p.reporting {
                    return Err(format!("{who}: `{name}` is a reporting parameter: it reads a dimension and is set by none"));
                }
                if p.kind == Kind::FamilyType {
                    return Err(format!("{who}: `{name}` is a family type: setting one from its host is T24's"));
                }
                if p.kind.text() {
                    let Expr::Formula(s) = e else {
                        return Err(format!("{who}: `{name}` is {}: it takes words or a parameter's name, not a number", p.kind.said()));
                    };
                    if s.chars().count() > MAX_TEXT || s.chars().any(char::is_control) {
                        return Err(format!("{who}: `{name}`: words of up to {MAX_TEXT} characters on one line"));
                    }
                    match self.parameters.iter().find(|q| q.name == *s) {
                        Some(q) if q.kind != p.kind => {
                            return Err(format!(
                                "{who}: `{name}` is {} and `{s}` is {}: associate it with one of its kind",
                                p.kind.said(),
                                q.kind.said()
                            ));
                        }
                        Some(_) => {}
                        None if p.kind == Kind::Material && !has_material(s) => {
                            return Err(format!("{who}: `{name}`: there's no material or material parameter `{s}`"));
                        }
                        None => {}
                    }
                } else if let Expr::Number(x) = e
                    && !(x.is_finite() && x.abs() <= MAX_COORD)
                {
                    return Err(format!("{who}: `{name}` is {x}, not a usable number"));
                }
            }
        }
        Ok(())
    }

    /// Its nested families and instances drawn on the plane or line `guid`: `nested instance 2`.
    pub(super) fn nested_on(&self, guid: Guid) -> impl Iterator<Item = String> + '_ {
        self.nested_instances
            .iter()
            .enumerate()
            .filter(move |(_, n)| n.host.guid() == guid)
            .map(|(i, _)| format!("nested instance {}", i + 1))
    }
}

/// Every shared family in `list` named `name`, at most `depth` deep, made `definition`; whether any was.
fn replace_shared(list: &mut [NestedFamily], name: &str, definition: &Family, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    let mut any = false;
    for n in list.iter_mut() {
        if n.name == name && n.family.shared {
            if n.family != *definition {
                n.family = definition.clone();
                any = true;
            }
        } else {
            any |= replace_shared(&mut n.family.nested, name, definition, depth - 1);
        }
    }
    any
}

/// What loading family `incoming`, named `name`, into a target holding `has` asks: whether the target has `name` defined
/// otherwise (Family Already Exists), and the shared families nested in it that the target has otherwise (Shared Family
/// Already Loaded).
pub fn conflicts(incoming: &Family, name: &str, has: &dyn Fn(&str) -> Option<Family>) -> (bool, Vec<String>) {
    let exists = has(name).is_some_and(|f| f != *incoming);
    let shared =
        incoming.shared_within().into_iter().filter(|(n, def)| n != name && has(n).is_some_and(|f| f != *def)).map(|(n, _)| n).collect();
    (exists, shared)
}

/// The shared families nested in `incoming` (a family loaded into a target holding `has`) reconciled with the target's, as
/// `answers` say for each the target has otherwise (none: the target's own): `Keep::Project` gives `incoming` the target's,
/// `Keep::Family` overwrites the target's with `incoming`'s, its values too (as `version_and_values` does, keeping the
/// target's own types and part identities). What the target
/// takes: each shared family it gains or has overwritten, by name; and the names it was asked about.
pub fn reconcile_shared(
    incoming: &mut Family,
    name: &str,
    has: &dyn Fn(&str) -> Option<Family>,
    answers: &BTreeMap<String, Keep>,
) -> (Vec<(String, Family)>, Vec<String>) {
    let (mut takes, mut asked) = (Vec::new(), Vec::new());
    for (s, def) in incoming.shared_within() {
        if s == name {
            continue;
        }
        match has(&s) {
            Some(own) if own != def => {
                asked.push(s.clone());
                match answers.get(&s).copied().unwrap_or_default() {
                    Keep::Project => {
                        incoming.replace_shared(&s, &own);
                    }
                    Keep::Family => {
                        let overwritten = crate::commands::family_reload(def, &own, false);
                        incoming.replace_shared(&s, &overwritten);
                        takes.push((s, overwritten));
                    }
                }
            }
            Some(_) => {}
            None => takes.push((s, def)),
        }
    }
    (takes, asked)
}

/// What loading a family into a family did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nesting {
    /// It was there already, defined the same: nothing changed.
    pub unchanged: bool,
    /// It was there already, defined otherwise, and overwritten.
    pub overwritten: bool,
    /// The shared families nested in it the host had otherwise, which were asked about.
    pub shared_asked: Vec<String>,
    /// The shared families nested in it the host gained.
    pub shared_added: Vec<String>,
}

/// Family `incoming` loaded into the nested families `list` under `name`, as Revit's Insert > Load Family in a family does:
/// the shared families nested in it reconciled with `list`'s as `answers` say ([`reconcile_shared`]) and the ones `list`
/// hasn't added to it; then `incoming` itself, added, or over one of that name defined otherwise as `overwrite` says (`Some(true)`:
/// `version`, keeping `list`'s values where they fit; `Some(false)`: `version_and_values`), refused when it doesn't say. A
/// shared family is one definition across `list`, at every depth. `list` is unchanged on a refusal.
pub fn nest_into(
    list: &mut Vec<NestedFamily>,
    name: &str,
    mut incoming: Family,
    overwrite: Option<bool>,
    answers: &BTreeMap<String, Keep>,
) -> Result<Nesting, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a nested family needs a name".into());
    }
    if incoming.nests(name) {
        return Err(format!("`{name}` nests a family of its own name: a family can't be loaded into itself"));
    }
    let mut report = Nesting::default();
    let at = list.iter().position(|n| n.name == name);
    if let Some(i) = at
        && list[i].family != incoming
        && overwrite.is_none()
    {
        return Err(format!(
            "it nests a family `{name}` already, defined otherwise: load it with `overwrite` `version` or `version_and_values`"
        ));
    }
    let mut work = list.clone();
    let has = |n: &str| find(&work, n).map(|f| f.family.clone());
    let (takes, asked) = reconcile_shared(&mut incoming, name, &has, answers);
    report.shared_asked = asked;
    for (s, def) in takes {
        match work.iter().position(|n| n.name == s) {
            Some(j) => work[j].family = def.clone(),
            None => {
                report.shared_added.push(s.clone());
                work.push(NestedFamily { name: s.clone(), family: def.clone(), ..Default::default() });
            }
        }
        replace_shared(&mut work, &s, &def, MAX_NESTING + 1);
    }
    let now = match at {
        Some(i) if work[i].family == incoming => {
            report.unchanged = true;
            incoming
        }
        Some(i) => {
            let keep = overwrite.unwrap_or(true);
            let now = crate::commands::family_reload(incoming, &work[i].family, keep);
            work[i].family = now.clone();
            report.overwritten = true;
            now
        }
        None => {
            work.push(NestedFamily { name: name.to_string(), family: incoming.clone(), ..Default::default() });
            incoming
        }
    };
    if now.shared {
        replace_shared(&mut work, name, &now, MAX_NESTING + 1);
    }
    *list = work;
    Ok(report)
}

/// `p`, a point of family `f`, in the axes of its plane `host`: where on the plane it lies (x, y) and how far along its
/// normal, for placing an instance at a point picked in a view.
pub fn on_plane(f: &Family, host: &PlaneRef, p: P3) -> Option<(P2, f64)> {
    let frame = f.frame(host).ok()?;
    let d = sub(p, frame.origin);
    Some((P2(dot(d, frame.x), dot(d, frame.y)), dot(d, frame.z)))
}
