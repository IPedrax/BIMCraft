//! Visibility and graphics: what a view hides and how it draws the rest, as Revit's
//! Visibility/Graphics does. A view (a level's plan, a section, an elevation or a 3D view) holds
//! [`Graphics`]: overrides by category, by element, and by the project's view filters, each hiding
//! what it matches or drawing it in its own colours, weights and dashes; its lines and fills in
//! projection (what lies below the cut plane, beyond a section's line) apart from those the view
//! cuts. A [`ViewTemplate`] holds the same settings under a name, for views to follow. [`Look`] is
//! what a view's graphics come to for each element: the drawing code asks it what to leave out and
//! how to draw the rest, and it dresses the finished drawing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{Element, Guid, Id, Key, MAX_TEXT, Model, ViewRange};
use crate::pen::{MAX_PEN, Pen};
use crate::render::{Drawing, Part, Prim, Rgb};
use crate::schedule;

/// The categories a plan, a section or a 3D view draws, and so may hide and override: those of the
/// model, then those of the annotation.
pub const MODEL: [&str; 25] = [
    "wall",
    "floor",
    "ceiling",
    "shaft",
    "mass",
    "door",
    "window",
    "room",
    "column",
    "structuralcolumn",
    "footing",
    "beam",
    "stripfooting",
    "pipe",
    "conduit",
    "cabletray",
    "wire",
    "downconductor",
    "groundconductor",
    "cable",
    "component",
    "stair",
    "ramp",
    "railing",
    "roof",
];
pub const ANNOTATION: [&str; 8] = ["dimension", "tag", "text", "spot", "grid", "level", "section", "elevation"];
/// The point clouds, which views hide and override as Revit's Visibility/Graphics does on a tab of their own: not a model
/// category, so Object Styles has no row for them.
pub const SCANS: [&str; 1] = ["pointcloud"];

/// Whether views draw elements of `category`.
pub fn drawn(category: &str) -> bool {
    MODEL.contains(&category) || ANNOTATION.contains(&category) || SCANS.contains(&category)
}

/// How much of a mass a 3D view sees through, percent, unless its graphics say otherwise.
pub const MASS_TRANSPARENCY: u8 = 60;

/// The most overrides a view holds (by category, and by element), the filters it applies, and the view
/// filters and view templates a project has.
pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_APPLIED: usize = 32;
pub const MAX_FILTERS: usize = 64;
pub const MAX_TEMPLATES: usize = 64;

/// How much a view draws of what it cuts. Coarse: the cut in one poché, an opening only its gap and jambs. Medium: a
/// compound element's layers, each a fill with a line between, and an opening's leaf, swing and glass. Fine: the layers'
/// patterns too, and a floor's surface pattern.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    Coarse,
    Medium,
    #[default]
    Fine,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Coarse, Detail::Medium, Detail::Fine];

    /// What it is called, in English.
    pub fn name(self) -> &'static str {
        match self {
            Detail::Coarse => "Coarse",
            Detail::Medium => "Medium",
            Detail::Fine => "Fine",
        }
    }

    /// A level of detail by its English name, in any case.
    pub fn parse(s: &str) -> Option<Detail> {
        Detail::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// How a 3D view draws its surfaces. Shaded: lit, in their materials' colours and textures. Hidden line: paper-white, with
/// their edges, what a face hides staying hidden. Wireframe: edges only, every one, seen through the faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    #[default]
    Shaded,
    HiddenLine,
    Wireframe,
}

impl VisualStyle {
    pub const ALL: [VisualStyle; 3] = [VisualStyle::Shaded, VisualStyle::HiddenLine, VisualStyle::Wireframe];

    /// What it is called, in English.
    pub fn name(self) -> &'static str {
        match self {
            VisualStyle::Shaded => "Shaded",
            VisualStyle::HiddenLine => "Hidden Line",
            VisualStyle::Wireframe => "Wireframe",
        }
    }

    /// A style by its name, in any case, its words apart by a space, an underscore or nothing: `Hidden Line`, `hidden_line`.
    pub fn parse(s: &str) -> Option<VisualStyle> {
        let plain = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        VisualStyle::ALL.into_iter().find(|v| plain(v.name()) == plain(s))
    }
}

/// The scales, 1 : n, a view's annotation is sized for and a viewport shows it at.
pub const SCALES: std::ops::RangeInclusive<f64> = 1.0..=100_000.0;

/// How a line is dashed: not at all, or in dashes or dots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
}

impl Dash {
    /// The on and off lengths a line is drawn with, px.
    fn pattern(self) -> Option<[f32; 2]> {
        match self {
            Dash::Solid => None,
            Dash::Dashed => Some([10.0, 5.0]),
            Dash::Dotted => Some([2.0, 4.0]),
        }
    }
}

/// A colour as JSON gives it: `[r, g, b]`, each 0 to 255, or `"#rrggbb"`.
mod rgb {
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    pub fn serialize<S: Serializer>(c: &Option<[u8; 3]>, s: S) -> Result<S::Ok, S::Error> {
        match c {
            Some(c) => s.collect_seq(c),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 3]>, D::Error> {
        let v = Value::deserialize(d)?;
        if v.is_null() {
            return Ok(None);
        }
        crate::commands::colour(&v)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("{v} isn't a colour: [r, g, b], each 0 to 255, or \"#rrggbb\"")))
    }
}

/// How lines and fills draw: a colour, a weight, a dash for lines, a colour for fills.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    #[serde(with = "rgb", skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<Pen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Dash>,
    #[serde(with = "rgb", skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 3]>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    /// This style's settings, and `base`'s where it has none.
    fn over(&self, base: &Style) -> Style {
        Style {
            color: self.color.or(base.color),
            weight: self.weight.or(base.weight),
            dash: self.dash.or(base.dash),
            fill: self.fill.or(base.fill),
        }
    }

    fn check(&self, who: &str) -> Result<(), String> {
        if let Some(w) = self.weight
            && !w.is_valid()
        {
            return Err(format!("{who}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", w.0));
        }
        Ok(())
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &u8) -> bool {
    *x == 0
}

fn is_fine(d: &Detail) -> bool {
    *d == Detail::Fine
}

fn is_shaded(v: &VisualStyle) -> bool {
    *v == VisualStyle::Shaded
}

/// How a view draws something: hidden, or its lines and fills as they are in projection and
/// where the view cuts them, lightened (`halftone`, halfway to the paper), and in 3D seen through
/// (`transparency`, percent).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Override {
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub projection: Style,
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub cut: Style,
    #[serde(skip_serializing_if = "is_false")]
    pub halftone: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub transparency: u8,
}

impl Override {
    pub fn is_empty(&self) -> bool {
        *self == Override::default()
    }

    /// This override's settings, and `base`'s where it has none.
    fn over(&self, base: &Override) -> Override {
        Override {
            hidden: self.hidden || base.hidden,
            projection: self.projection.over(&base.projection),
            cut: self.cut.over(&base.cut),
            halftone: self.halftone || base.halftone,
            transparency: if self.transparency > 0 { self.transparency } else { base.transparency },
        }
    }

    pub(crate) fn check(&self, who: &str) -> Result<(), String> {
        self.projection.check(&format!("{who} projection"))?;
        self.cut.check(&format!("{who} cut"))?;
        if self.transparency > 100 {
            return Err(format!("{who}: transparency is a percent, 0 to 100, not {}", self.transparency));
        }
        Ok(())
    }
}

/// A view filter a view applies: its override for what the filter picks out (hidden, or drawn
/// another way). Filters apply in order, the first prevailing where they disagree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "AppliedJson", into = "AppliedJson")]
pub struct Applied {
    pub filter: String,
    pub over: Override,
}

/// An applied filter as JSON has it: the override's settings beside the filter's name.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppliedJson {
    filter: String,
    #[serde(default, skip_serializing_if = "is_false")]
    hidden: bool,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    projection: Style,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    cut: Style,
    #[serde(default, skip_serializing_if = "is_false")]
    halftone: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    transparency: u8,
}

impl From<AppliedJson> for Applied {
    fn from(a: AppliedJson) -> Applied {
        let over = Override { hidden: a.hidden, projection: a.projection, cut: a.cut, halftone: a.halftone, transparency: a.transparency };
        Applied { filter: a.filter, over }
    }
}

impl From<Applied> for AppliedJson {
    fn from(a: Applied) -> AppliedJson {
        let Override { hidden, projection, cut, halftone, transparency } = a.over;
        AppliedJson { filter: a.filter, hidden, projection, cut, halftone, transparency }
    }
}

/// A view's visibility and graphics: overrides by category (`categories`, the category's name),
/// by the project's view filters, and by element (`elements`, its GUID); the template it follows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Graphics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub categories: BTreeMap<String, Override>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Applied>,
    /// By element, its GUID.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub elements: BTreeMap<Guid, Override>,
    /// The phase it shows (the project's last unless it says), and the phase filter that says how
    /// each status of what's in it shows (every status as its category does, unless it says).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_filter: Option<String>,
    /// The design option it shows of each set, by set (a set's primary, unless it says).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// The scale, 1 : `scale`, a plan's or a section's annotation is sized for; none, annotation keeps its size on screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// How much it draws of what it cuts.
    #[serde(skip_serializing_if = "is_fine")]
    pub detail: Detail,
    /// How a 3D view draws its surfaces.
    #[serde(skip_serializing_if = "is_shaded")]
    pub style: VisualStyle,
}

impl Graphics {
    pub fn is_default(&self) -> bool {
        *self == Graphics::default()
    }

    /// The scale, 1 : n, these hold a view at in a project in `units`: their own, else the units' default (1 : 100, or
    /// 1/8" = 1'-0"). Every plan, section and elevation has a scale, so what it draws is paper at that scale: its
    /// lettering, marks and pens. Read it of [`Graphics::resolved`] for the one in force, as [`scale_of`] does.
    pub fn scale_n(&self, units: crate::units::Units) -> f64 {
        self.scale.unwrap_or_else(|| units.default_scale())
    }

    /// The settings in force: these, but for the groups the template followed controls, which are its.
    pub fn resolved(&self, m: &Model) -> std::borrow::Cow<'_, Graphics> {
        let Some(t) = self.template.as_ref().and_then(|n| m.view_templates.get(n)) else { return std::borrow::Cow::Borrowed(self) };
        let mut g = self.clone();
        if t.controls.contains(&Group::Categories) {
            g.categories = t.graphics.categories.clone();
        }
        if t.controls.contains(&Group::Filters) {
            g.filters = t.graphics.filters.clone();
        }
        if t.controls.contains(&Group::Phase) {
            (g.phase, g.phase_filter) = (t.graphics.phase.clone(), t.graphics.phase_filter.clone());
        }
        if t.controls.contains(&Group::Options) {
            g.options = t.graphics.options.clone();
        }
        if t.controls.contains(&Group::Scale) {
            g.scale = t.graphics.scale;
        }
        if t.controls.contains(&Group::Detail) {
            g.detail = t.graphics.detail;
        }
        if t.controls.contains(&Group::Style) {
            g.style = t.graphics.style;
        }
        std::borrow::Cow::Owned(g)
    }

    /// The view template it follows, when that controls `group`: the group's settings aren't the view's own to change.
    pub fn controlled_by(&self, m: &Model, group: Group) -> Option<&str> {
        let name = self.template.as_deref()?;
        m.view_templates.get(name)?.controls.contains(&group).then_some(name)
    }

    /// Whether it hides and overrides nothing: its scale and detail level change what the drawing code draws, not what a
    /// view's [`Look`] says to leave out.
    fn draws_as_is(&self) -> bool {
        (&self.template, &self.phase, &self.phase_filter) == (&None, &None, &None)
            && self.categories.is_empty()
            && self.filters.is_empty()
            && self.elements.is_empty()
            && self.options.is_empty()
    }

    /// Whether category `c` is hidden.
    pub fn hides(&self, c: &str) -> bool {
        self.categories.get(c).is_some_and(|o| o.hidden)
    }

    /// Whether it is sound in `m`: known categories, overrides in range, filters and the template
    /// there are.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        if self.categories.len() > MAX_ENTRIES || self.elements.len() > MAX_ENTRIES {
            return Err(format!("it has more than {MAX_ENTRIES} overrides"));
        }
        if self.filters.len() > MAX_APPLIED {
            return Err(format!("it applies {} filters; a view applies up to {MAX_APPLIED}", self.filters.len()));
        }
        for (c, o) in &self.categories {
            if !drawn(c) {
                return Err(format!(
                    "`{c}` isn't a category views draw; there are: {}",
                    MODEL.iter().chain(&ANNOTATION).chain(&SCANS).copied().collect::<Vec<_>>().join(", ")
                ));
            }
            o.check(&format!("category `{c}`"))?;
        }
        for (id, o) in &self.elements {
            o.check(&format!("element {id}"))?;
        }
        let mut seen = BTreeSet::new();
        for a in &self.filters {
            if !m.view_filters.contains_key(&a.filter) {
                return Err(format!("there's no view filter `{}`{}", a.filter, known(m.view_filters.keys())));
            }
            if !seen.insert(a.filter.as_str()) {
                return Err(format!("filter `{}` is applied twice", a.filter));
            }
            a.over.check(&format!("filter `{}`", a.filter))?;
        }
        if let Some(t) = &self.template
            && !m.view_templates.contains_key(t)
        {
            return Err(format!("there's no view template `{t}`{}", known(m.view_templates.keys())));
        }
        if let Some(p) = &self.phase
            && !m.phases.contains(p)
        {
            return Err(format!("there's no phase `{p}` (there are: {})", m.phases.join(", ")));
        }
        if let Some(f) = &self.phase_filter
            && !m.phase_filters.contains_key(f)
        {
            return Err(format!("there's no phase filter `{f}`{}", known(m.phase_filters.keys())));
        }
        if let Some(n) = self.scale
            && !SCALES.contains(&n)
        {
            return Err(format!("its scale 1 : {n} isn't between 1 : {} and 1 : {}", SCALES.start(), SCALES.end()));
        }
        if self.options.len() > crate::phasing::MAX_OPTION_SETS {
            return Err(format!(
                "it chooses options of {} sets; a project has up to {}",
                self.options.len(),
                crate::phasing::MAX_OPTION_SETS
            ));
        }
        for (set, option) in &self.options {
            match m.option_sets.get(set) {
                None => return Err(format!("there's no design option set `{set}`{}", known(m.option_sets.keys()))),
                Some(s) if !s.options.contains(option) => {
                    return Err(format!("design option set `{set}` has no option `{option}` (there are: {})", s.options.join(", ")));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// ` (there are: a, b)`, or nothing when there are none.
fn known<'a>(names: impl Iterator<Item = &'a String>) -> String {
    let names: Vec<&str> = names.map(String::as_str).collect();
    if names.is_empty() { String::new() } else { format!(" (there are: {})", names.join(", ")) }
}

/// A view filter: the elements of its categories that hold every one of its rules (a schedule
/// filter's: a field, how to compare it, a value), for views to hide or override.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewFilter {
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<schedule::Filter>,
}

impl ViewFilter {
    /// The elements it picks out.
    pub fn matching(&self, m: &Model) -> BTreeSet<Id> {
        schedule::picks(m, &self.categories, &self.rules)
    }
}

impl ViewFilter {
    /// Whether it is sound: categories there are schedules of, and rules every one of them has.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        if self.categories.is_empty() {
            return Err(format!("it needs categories, one or more of: {}", schedule::categories().join(", ")));
        }
        for c in &self.categories {
            if !schedule::categories().contains(&c.as_str()) {
                return Err(format!("`{c}` isn't a category filters pick from; there are: {}", schedule::categories().join(", ")));
            }
        }
        schedule::check_rules(m, &self.categories, &self.rules)
    }
}

/// A group of a view's settings a template can control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    /// The categories' overrides and which are hidden.
    Categories,
    /// The filters applied.
    Filters,
    /// A plan's view range.
    Range,
    /// The phase it shows and its phase filter.
    Phase,
    /// The design options it shows.
    Options,
    /// A plan's, section's or elevation's scale.
    Scale,
    /// A plan's, section's or elevation's detail level.
    Detail,
    /// A 3D view's visual style.
    Style,
}

impl Group {
    pub const ALL: [Group; 8] =
        [Group::Categories, Group::Filters, Group::Range, Group::Phase, Group::Options, Group::Scale, Group::Detail, Group::Style];
}

/// A view template: settings under a name for views to follow, the groups in `controls`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewTemplate {
    pub controls: BTreeSet<Group>,
    #[serde(default, skip_serializing_if = "Graphics::is_default")]
    pub graphics: Graphics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_range: Option<ViewRange>,
}

impl ViewTemplate {
    /// Whether it is sound: its graphics are a view's, without the parts that are a view's own.
    pub fn check(&self, m: &Model) -> Result<(), String> {
        if self.graphics.template.is_some() || !self.graphics.elements.is_empty() {
            return Err("a template holds categories and filters; the elements and the template a view follows are its own".into());
        }
        self.graphics.check(m)?;
        if let Some(r) = &self.view_range {
            crate::model::range_ok("its", r)?;
        }
        Ok(())
    }
}

/// Whether the project's view filters and view templates are sound.
pub(crate) fn check_project(m: &Model) -> Result<(), String> {
    if m.view_filters.len() > MAX_FILTERS {
        return Err(format!("the project has {} view filters; it holds up to {MAX_FILTERS}", m.view_filters.len()));
    }
    if m.view_templates.len() > MAX_TEMPLATES {
        return Err(format!("the project has {} view templates; it holds up to {MAX_TEMPLATES}", m.view_templates.len()));
    }
    let named = |who: &str, n: &str| {
        if n.trim().is_empty() || n.chars().count() > MAX_NAME {
            Err(format!("{who} `{n}`: a name is 1 to {MAX_NAME} characters"))
        } else {
            Ok(())
        }
    };
    for (n, f) in &m.view_filters {
        named("view filter", n)?;
        f.check(m).map_err(|e| format!("view filter `{n}`: {e}"))?;
    }
    for (n, t) in &m.view_templates {
        named("view template", n)?;
        t.check(m).map_err(|e| format!("view template `{n}`: {e}"))?;
    }
    Ok(())
}

/// Whether changing view `old` into `new` leaves be what the view template `new` follows controls:
/// its categories, its filters, phase, design options, scale, detail level, visual style, a plan's view range.
/// (Unfollowing and changing in one go is fine.)
pub fn check_followed(m: &Model, old: &Element, new: &Element) -> Result<(), String> {
    let (Some(a), Some(b)) = (graphics_of_element(old), graphics_of_element(new)) else { return Ok(()) };
    let Some((name, t)) = b.template.as_ref().and_then(|n| Some((n, m.view_templates.get(n)?))) else { return Ok(()) };
    let follows = |what: Group, same: bool, label: &str| {
        if t.controls.contains(&what) && !same {
            return Err(format!("its {label} follow view template `{name}`; change the template, or unfollow it (`template: null`)"));
        }
        Ok(())
    };
    follows(Group::Categories, a.categories == b.categories, "categories")?;
    follows(Group::Filters, a.filters == b.filters, "filters")?;
    follows(Group::Phase, (&a.phase, &a.phase_filter) == (&b.phase, &b.phase_filter), "phase and phase filter")?;
    follows(Group::Options, a.options == b.options, "design options")?;
    follows(Group::Scale, a.scale == b.scale, "scale")?;
    follows(Group::Detail, a.detail == b.detail, "detail level")?;
    follows(Group::Style, a.style == b.style, "visual style")?;
    if let (Element::Level(x), Element::Level(y)) = (old, new) {
        follows(Group::Range, x.view_range == y.view_range, "view range")?;
    }
    if let (Element::PlanView(x), Element::PlanView(y)) = (old, new) {
        follows(Group::Range, x.view_range == y.view_range, "view range")?;
    }
    Ok(())
}

/// The graphics element `e` holds, if it is a view.
pub fn graphics_of_element(e: &Element) -> Option<&Graphics> {
    match e {
        Element::Level(l) => Some(&l.graphics),
        Element::PlanView(v) => Some(&v.graphics),
        Element::Section(v) | Element::Elevation(v) => Some(&v.graphics),
        Element::View3D(v) => Some(&v.graphics),
        _ => None,
    }
}

/// The scale, 1 : n, view `id` is held at: its own, or its template's where that controls it, else its project units'
/// default.
pub fn scale_of(m: &Model, id: impl Key) -> f64 {
    graphics_of(m, id).map_or_else(|| m.units.default_scale(), |g| g.resolved(m).scale_n(m.units))
}

/// The graphics view `view` holds, to change.
pub fn graphics_mut(e: &mut Element) -> Option<&mut Graphics> {
    match e {
        Element::Level(l) => Some(&mut l.graphics),
        Element::PlanView(v) => Some(&mut v.graphics),
        Element::Section(v) | Element::Elevation(v) => Some(&mut v.graphics),
        Element::View3D(v) => Some(&mut v.graphics),
        _ => None,
    }
}

/// The graphics view `view` holds: a level's, a plan view's, a section's, an elevation's or a 3D view's. A dependent view has
/// its parent's.
pub fn graphics_of(m: &Model, view: impl Key) -> Option<&Graphics> {
    graphics_of_element(m.elements.get(m.plan_source(view))?)
}

/// What a view's graphics come to for each element of the model.
#[derive(Default)]
pub struct Look {
    hidden: BTreeSet<Id>,
    paint: BTreeMap<Id, Override>,
    style: VisualStyle,
}

/// `c` halfway to the paper.
fn halftone(c: Rgb) -> Rgb {
    let half = |v: u8| v + (255 - v) / 2;
    Rgb(half(c.0), half(c.1), half(c.2))
}

impl Look {
    /// What view `view` looks like: nothing hidden or painted when it isn't a view, or has no graphics.
    pub fn of(m: &Model, view: impl Key) -> Look {
        graphics_of(m, view).map_or_else(Look::default, |own| Look::with(m, own))
    }

    /// What the default 3D view looks like: as its own view's graphics say once something has changed
    /// them, else as those of a view with none (its last phase, and its sets' primary options).
    pub fn of_3d(m: &Model) -> Look {
        m.default_3d().map_or_else(|| Look::with(m, &Graphics::default()), |id| Look::of(m, id))
    }

    /// What a view with graphics `own` looks like.
    /// ponytail: worked out on every draw; cache it per model and view if a filter on a slow field
    /// (a room's area) makes redraws slow.
    pub fn with(m: &Model, own: &Graphics) -> Look {
        let g = own.resolved(m);
        let mut look = Look { style: g.style, ..Look::default() };
        // Nothing to look for: no graphics of its own, and nothing in the project phased or in an option.
        let linked = m.elements.keys().next_back().is_some_and(|id| crate::link::is_linked(*id));
        if own.draws_as_is() && !linked && !crate::phasing::any(m) && !m.elements.values().any(|e| matches!(e, Element::Mass(_))) {
            return look;
        }
        // The phase it shows, as where it stands in the project's order, and how its phase filter shows each status.
        let at = g.phase.as_deref().and_then(|n| crate::phasing::index(m, n)).unwrap_or(m.phases.len().saturating_sub(1));
        let filter = g.phase_filter.as_deref().and_then(|n| m.phase_filters.get(n));
        // Each applied filter, and the elements it picks out.
        let applied: Vec<(&Applied, BTreeSet<Id>)> =
            g.filters.iter().filter_map(|a| Some((a, m.view_filters.get(&a.filter)?.matching(m)))).collect();
        for (&id, e) in &m.elements {
            let cat = e.category();
            if !drawn(cat) {
                continue;
            }
            // What isn't in its phase isn't there; what its phase filter doesn't display is hidden; what it
            // overrides is drawn as the project's override says, under whatever else the view says.
            let mut base = Override::default();
            if let Some(status) = crate::phasing::status(m, e, at) {
                match filter.map_or(crate::phasing::Presentation::ByCategory, |f| f.of(status)) {
                    crate::phasing::Presentation::NotDisplayed => {
                        look.hidden.insert(id);
                        continue;
                    }
                    crate::phasing::Presentation::Overridden => base = m.phase_overrides.get(&status).cloned().unwrap_or_default(),
                    crate::phasing::Presentation::ByCategory => {}
                }
            } else if e.params().is_some() {
                look.hidden.insert(id);
                continue;
            }
            // What a link builds is a reference: halftone, under whatever the view says.
            if crate::link::is_linked(id) {
                base.halftone = true;
            }
            // A mass is seen through in 3D unless the view says otherwise.
            if matches!(e, Element::Mass(_)) && base.transparency == 0 {
                base.transparency = MASS_TRANSPARENCY;
            }
            // In a design option the view doesn't show, it isn't shown either.
            if !crate::phasing::option_shown(m, &g, e) {
                look.hidden.insert(id);
                continue;
            }
            // The category's, then each filter's from the last to the first, then the element's own.
            let mut over = g.categories.get(cat).map_or_else(|| base.clone(), |c| c.over(&base));
            for (a, picked) in applied.iter().rev().filter(|(_, picked)| picked.contains(&id)) {
                let _ = picked;
                over = a.over.over(&over);
            }
            if let Some(own) = m.elements.guid(id).and_then(|k| g.elements.get(&k)) {
                over = own.over(&over);
            }
            if over.hidden {
                look.hidden.insert(id);
            } else if !over.is_empty() {
                look.paint.insert(id, over);
            }
        }
        look
    }

    /// How the view draws surfaces in 3D.
    pub fn style(&self) -> VisualStyle {
        self.style
    }

    /// Whether the view draws element `id`.
    pub fn shows(&self, id: Id) -> bool {
        !self.hidden.contains(&id)
    }

    /// The colour of element `id`'s fill where the view cuts it, which is `default` unless it says
    /// otherwise: what the drawing code fills the union of cut walls with.
    pub fn cut_fill(&self, id: Id, default: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return default };
        let c = o.cut.fill.map_or(default, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// The colour a 3D view shades element `id` in, `base` unless it says otherwise.
    pub fn surface(&self, id: Id, base: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return base };
        let c = o.projection.fill.map_or(base, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// Whether the view fills element `id`'s surfaces with a colour of its own (its projection's fill), whatever theirs are.
    pub fn filled(&self, id: Id) -> bool {
        self.paint.get(&id).is_some_and(|o| o.projection.fill.is_some())
    }

    /// The colour of the edges of element `id`'s surfaces in a 3D view, and of what a section sees
    /// beyond its line: `base` unless a projection's line colour says otherwise, lighter in halftone.
    pub fn edge(&self, id: Id, base: Rgb) -> Rgb {
        let Some(o) = self.paint.get(&id) else { return base };
        let c = o.projection.color.map_or(base, |[r, g, b]| Rgb(r, g, b));
        if o.halftone { halftone(c) } else { c }
    }

    /// How much of element `id`'s surface a 3D view sees through, percent.
    pub fn transparency(&self, id: Id) -> u8 {
        self.paint.get(&id).map_or(0, |o| o.transparency)
    }

    /// Whether it changes anything.
    pub fn is_plain(&self) -> bool {
        self.hidden.is_empty() && self.paint.is_empty()
    }

    /// `d` with its primitives drawn as the view says: the colours, weights and dashes of each
    /// element's lines in projection and cut, its fills, lighter in halftone.
    pub fn dress(&self, d: &mut Drawing) {
        if self.paint.is_empty() {
            return;
        }
        for (((prim, owner), part), pen) in d.prims.iter_mut().zip(&d.owners).zip(&d.parts).zip(d.pens.iter_mut()) {
            let Some(over) = owner.and_then(|id| self.paint.get(&id)) else { continue };
            let style = if *part == Part::Cut { &over.cut } else { &over.projection };
            match prim {
                Prim::Line { color, dash, .. } => {
                    if *part != Part::Pattern
                        && let Some([r, g, b]) = style.color
                    {
                        *color = Rgb(r, g, b);
                    }
                    if !matches!(part, Part::Text | Part::Pattern) {
                        if let Some(p) = style.weight {
                            *pen = Some(p);
                        }
                        if let Some(k) = style.dash {
                            *dash = k.pattern();
                        }
                    }
                    if over.halftone {
                        *color = halftone(*color);
                    }
                }
                Prim::Fill { color, .. } => {
                    if *part != Part::Pattern
                        && let Some([r, g, b]) = style.fill
                    {
                        *color = Rgb(r, g, b);
                    }
                    if over.halftone {
                        *color = halftone(*color);
                    }
                }
                Prim::Image { .. } => {}
            }
        }
    }
}

/// Longest name of a filter or a template.
pub const MAX_NAME: usize = MAX_TEXT;

/// What the graphics tests and the phasing tests share: commands, a house to draw, and ways to read what was drawn.
#[cfg(test)]
pub(crate) mod testing {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::geom::P2;
    use crate::render::{self, Drawing, Prim, Rgb};

    pub(crate) fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// An 8 x 6 m box of walls on Level 1 (ids 3 to 6, the bottom one first).
    pub(crate) fn house(s: &mut Session) -> Vec<u64> {
        let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
    }

    pub(crate) fn plan(s: &Session, level: u64) -> Drawing {
        render::plan(&s.model, level, 1600, 1000)
    }

    /// The primitives `id` owns.
    pub(crate) fn owned(d: &Drawing, id: u64) -> Vec<&Prim> {
        d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
    }

    /// Pen, colour and dash of the lines `id` owns: `u8::MAX` for a line no pen draws (lettering's).
    pub(crate) fn lines(d: &Drawing, id: u64) -> Vec<(u8, Rgb, Option<[f32; 2]>)> {
        (d.prims.iter().zip(&d.owners).zip(&d.pens))
            .filter_map(|((p, o), pen)| match p {
                Prim::Line { color, dash, .. } if *o == Some(id) => Some((pen.map_or(u8::MAX, |p| p.0), *color, *dash)),
                _ => None,
            })
            .collect()
    }

    /// How many primitives of `d` have a point within `r` px of model point `at`: what is drawn there.
    pub(crate) fn drawn_near(d: &Drawing, at: P2, r: f32) -> usize {
        let (x, y) = d.view.px(at);
        let near = |p: &(f32, f32)| (p.0 - x).hypot(p.1 - y) <= r;
        d.prims
            .iter()
            .filter(|p| match p {
                Prim::Fill { rings, .. } => rings.iter().flatten().any(near),
                Prim::Line { pts, .. } => pts.iter().any(near),
                Prim::Image { .. } => false,
            })
            .count()
    }

    pub(crate) fn pixel(d: &Drawing, p: P2) -> Rgb {
        let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
        let (x, y) = d.view.px(p);
        let c = pm.pixel(x as u32, y as u32).unwrap();
        Rgb(c.red(), c.green(), c.blue())
    }

    pub(crate) const RED: Rgb = Rgb(200, 0, 0);
    pub(crate) const PAPER: Rgb = render::PAPER;

    pub(crate) fn picture(s: &mut Session, view: &str, name: &str) -> tiny_skia::Pixmap {
        let path = std::env::temp_dir().join(format!("bimcraft-gfx-{}-{name}.png", std::process::id()));
        run(s, "render", json!({"view": view, "width": 240, "height": 180, "path": path.to_str().unwrap()}));
        let pm = tiny_skia::Pixmap::load_png(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        pm
    }

    pub(crate) fn painted(pm: &tiny_skia::Pixmap, keep: impl Fn(u8, u8, u8) -> bool) -> usize {
        pm.pixels().iter().filter(|c| keep(c.red(), c.green(), c.blue())).count()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::geom::P2;
    use crate::graphics::Look;
    use crate::graphics::testing::*;
    use crate::model::Element;
    use crate::render::{self, Drawing, POCHE, Prim, Rgb};

    #[test]
    fn a_category_hidden_in_a_view_is_left_out_of_that_view_only() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}))["id"].as_u64().unwrap();
        let grid = run(&mut s, "grid", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        assert!(!owned(&plan(&s, 100000), room).is_empty() && !owned(&plan(&s, 100000), grid).is_empty());
        let r = run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["rooms", "Grids"]}));
        assert_eq!(r["hidden"], json!(["grid", "room"]));
        let d = plan(&s, 100000);
        assert!(owned(&d, room).is_empty(), "the room's tag is gone");
        assert!(owned(&d, grid).is_empty(), "so is the grid");
        assert!(!owned(&d, walls[0]).is_empty(), "the walls stay");
        // Level 2's plan is its own view: its grids are still there.
        assert!(!owned(&plan(&s, 100001), grid).is_empty());
        let r = run(&mut s, "graphics", json!({"view": 100000, "show": ["room"]}));
        assert_eq!(r["hidden"], json!(["grid"]));
        assert!(!owned(&plan(&s, 100000), room).is_empty());
        // Walls hidden leave no poché behind: the fill under their joins goes with them.
        let view = plan(&s, 100000).view;
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["walls"]}));
        assert_eq!(pixel(&render::draw(&s.model, 100000, view), P2(4000.0, 0.0)), PAPER);
    }

    #[test]
    fn an_element_hidden_in_a_view_leaves_the_rest_as_it_was() {
        let mut s = Session::new();
        let walls = house(&mut s);
        assert_eq!(pixel(&plan(&s, 100000), P2(4000.0, 0.0)), POCHE);
        let r = run(&mut s, "graphics", json!({"view": "Level 1", "hide_elements": [walls[0]]}));
        assert_eq!(r["hidden_elements"], json!([walls[0]]));
        let d = plan(&s, 100000);
        assert!(owned(&d, walls[0]).is_empty());
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), PAPER, "no poché where it was");
        assert_eq!(pixel(&d, P2(8000.0, 3000.0)), POCHE, "the east wall is still cut");
        // Its other views still draw it; hidden or not, it's still in the model and still joins.
        assert!(!owned(&plan(&s, 100001), walls[0]).is_empty() || run(&mut s, "elements", json!({"category": "walls"}))["count"] == 4);
        run(&mut s, "graphics", json!({"view": "Level 1", "show_elements": [walls[0]]}));
        assert_eq!(pixel(&plan(&s, 100000), P2(4000.0, 0.0)), POCHE);
    }

    #[test]
    fn cut_and_projection_lines_take_their_own_overrides_in_a_view_alone() {
        let mut s = Session::new();
        let walls = house(&mut s);
        // Low enough to lie under the cut plane: seen from it, in projection.
        let low = run(&mut s, "wall", json!({"points": [[1000, 1000], [3000, 1000]], "height": 1000}))["ids"][0].as_u64().unwrap();
        let up =
            run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "level": "Level 2", "height": 2500}))["ids"][0].as_u64().unwrap();
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).contains(&(10, Rgb(25, 25, 25), None)));
        assert!(lines(&d, low).contains(&(4, Rgb(25, 25, 25), None)));
        run(
            &mut s,
            "graphics",
            json!({"view": "Level 1", "categories": {"walls": {
                "cut": {"color": [200, 0, 0], "weight": 6},
                "projection": {"color": "#0000c8", "weight": 3, "dash": "dashed"}}}}),
        );
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).contains(&(6, RED, None)), "{:?}", lines(&d, walls[0]));
        assert!(lines(&d, low).contains(&(3, Rgb(0, 0, 200), Some([10.0, 5.0]))), "{:?}", lines(&d, low));
        // Another view keeps its own look.
        assert!(lines(&plan(&s, 100001), up).contains(&(10, Rgb(25, 25, 25), None)));
        // An override merges into the last, and null takes a field back.
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 2}}}}));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(2, RED, None)), "the colour is kept");
        let r = run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"cut": {"color": null}}}}));
        assert_eq!(r["categories"]["wall"]["cut"], json!({"weight": 2}));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(2, Rgb(25, 25, 25), None)));
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": null}}));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(10, Rgb(25, 25, 25), None)));
    }

    #[test]
    fn an_element_override_beats_its_categorys_and_fills_take_their_own_colour() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"cut": {"fill": [220, 160, 40]}}}}));
        let d = plan(&s, 100000);
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), Rgb(220, 160, 40));
        assert_eq!(pixel(&d, P2(8000.0, 3000.0)), Rgb(220, 160, 40));
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "elements": {walls[0].to_string(): {"cut": {"fill": [10, 120, 200], "color": [0, 200, 0]}}}}),
        );
        let d = plan(&s, 100000);
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), Rgb(10, 120, 200), "the wall's own");
        assert_eq!(pixel(&d, P2(8000.0, 3000.0)), Rgb(220, 160, 40), "the others' category's");
        assert!(lines(&d, walls[0]).iter().any(|l| l.1 == Rgb(0, 200, 0)));
        assert!(lines(&d, walls[1]).iter().all(|l| l.1 != Rgb(0, 200, 0)));
        // The walls joined are filled together in each colour, so no seam of the usual grey shows.
        let union = |color: Rgb| {
            d.prims.iter().zip(&d.owners).filter(|(p, o)| o.is_none() && matches!(p, Prim::Fill { color: c, .. } if *c == color)).count()
        };
        assert_eq!((union(POCHE), union(Rgb(10, 120, 200)), union(Rgb(220, 160, 40))), (0, 1, 1));
    }

    #[test]
    fn halftone_draws_a_category_halfway_to_the_paper() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"halftone": true}}}));
        let d = plan(&s, 100000);
        let half = |c: u8| c + (255 - c) / 2;
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), Rgb(half(POCHE.0), half(POCHE.1), half(POCHE.2)));
        assert!(lines(&d, walls[0]).contains(&(10, Rgb(half(25), half(25), half(25)), None)));
    }

    #[test]
    fn the_3d_view_hides_and_paints_by_category() {
        let mut s = Session::new();
        house(&mut s);
        let shown = |pm: &tiny_skia::Pixmap| painted(pm, |r, g, b| (r, g, b) != (255, 255, 255));
        assert!(shown(&picture(&mut s, "{3D}", "a")) > 500);
        let r = run(&mut s, "graphics", json!({"view": "{3D}", "categories": {"walls": {"projection": {"fill": [255, 0, 0]}}}}));
        assert_eq!(r["name"], "{3D}");
        let red = |pm: &tiny_skia::Pixmap| painted(pm, |r, g, b| r > 120 && g < 40 && b < 40);
        assert!(red(&picture(&mut s, "{3D}", "b")) > 500, "walls in red");
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls"]}));
        assert!(shown(&picture(&mut s, "{3D}", "c")) > 0, "the levels' planes");
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls", "levels"]}));
        assert_eq!(shown(&picture(&mut s, "{3D}", "d")), 0, "nothing else to see");
        // The placed 3D view is that same view; the plan is its own.
        assert!(!owned(&plan(&s, 100000), 100002).is_empty());
    }

    #[test]
    fn hiding_a_category_leaves_every_one_of_its_elements_out_of_plans_ceiling_plans_and_sections() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s.script(
            r#"column {"at": [5000, 1000]}
               stair {"start": [1000, 1000], "end": [3500, 1000], "railings": true}
               ramp {"start": [1000, 2500], "angle": 0, "height": 500}
               shaft {"boundary": [[7000, 5000], [7800, 5000], [7800, 5800], [7000, 5800]]}"#,
        )
        .unwrap();
        let ids = |s: &mut Session, category: &str| -> Vec<u64> {
            run(s, "elements", json!({"category": category}))["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_u64().unwrap())
                .collect()
        };
        let section = ids(&mut s, "sections")[0];
        let elevation = ids(&mut s, "elevations")[0];
        // Every drawing of the model there is, but the 3D view's.
        let drawings = |s: &Session| -> Vec<Drawing> {
            let view = render::section_fit(&s.model, section, 800, 600);
            let wide = render::section_fit(&s.model, elevation, 800, 600);
            vec![
                plan(s, 100000),
                plan(s, 100001),
                render::ceiling_plan(&s.model, 100000, 1600, 1000),
                render::section(&s.model, section, view),
                render::section(&s.model, elevation, wide),
            ]
        };
        let drawn = |s: &Session, ids: &[u64]| {
            let all = drawings(s);
            ids.iter().filter(|id| all.iter().any(|d| !owned(d, **id).is_empty())).count()
        };
        for category in crate::graphics::MODEL.iter().chain(&crate::graphics::ANNOTATION) {
            let of = ids(&mut s, category);
            if of.is_empty() {
                continue;
            }
            let before = drawn(&s, &of);
            run(&mut s, "graphics", json!({"view": 100000, "hide": [category]}));
            run(&mut s, "graphics", json!({"view": 100001, "hide": [category]}));
            run(&mut s, "graphics", json!({"view": section, "hide": [category]}));
            run(&mut s, "graphics", json!({"view": elevation, "hide": [category]}));
            // The level's ceiling plan is its plan's view.
            assert_eq!(drawn(&s, &of), 0, "{category}: {before} drawn before, and some still are");
            run(&mut s, "graphics", json!({"view": 100000, "reset": true}));
            run(&mut s, "graphics", json!({"view": 100001, "reset": true}));
            run(&mut s, "graphics", json!({"view": section, "reset": true}));
            run(&mut s, "graphics", json!({"view": elevation, "reset": true}));
            // Nothing in the setup is left undrawn, or the test would prove nothing.
            assert!(drawn(&s, &of) > 0, "{category} isn't drawn in any of them");
        }
    }

    #[test]
    fn hiding_doors_and_windows_leaves_their_openings_in_the_wall() {
        let mut s = Session::new();
        house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        let window = run(&mut s, "window", json!({"at": [8000, 3000]}))["id"].as_u64().unwrap();
        let view = plan(&s, 100000).view;
        let clear = |s: &Session| {
            let d = render::draw(&s.model, 100000, view);
            // Between a window's glass lines, which run along its wall's centre line and its faces.
            (pixel(&d, P2(2000.0, 0.0)) == PAPER, pixel(&d, P2(7950.0, 3000.0)) == PAPER, owned(&d, door).len(), owned(&d, window).len())
        };
        let (c0, c1, doors, windows) = clear(&s);
        assert!(c0 && c1 && doors > 0 && windows > 0, "open to begin with");
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["doors", "windows"]}));
        assert_eq!(clear(&s), (true, true, 0, 0), "the openings stay, the doors and windows are gone");
        run(&mut s, "graphics", json!({"view": 100000, "show": ["doors", "windows"], "hide_elements": [door]}));
        let (c0, c1, doors, windows) = clear(&s);
        assert!(c0 && c1 && doors == 0 && windows > 0);
        // What is left of a hidden door isn't its own: nothing of it can be picked or painted.
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"doors": {"cut": {"fill": [200, 0, 0]}}}}));
        assert_eq!(pixel(&render::draw(&s.model, 100000, view), P2(2000.0, 0.0)), PAPER);
    }

    #[test]
    fn a_doors_swing_is_projection_and_the_rest_of_it_is_cut() {
        let mut s = Session::new();
        house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "categories": {"doors": {"projection": {"color": [0, 0, 200]}, "cut": {"color": [200, 0, 0]}}}}),
        );
        let l = lines(&plan(&s, 100000), door);
        let blue = Rgb(0, 0, 200);
        assert!(l.iter().any(|x| x.1 == blue && x.0 == 4), "its swing: {l:?}");
        assert!(l.iter().any(|x| x.1 == RED && x.0 == 7), "its jambs: {l:?}");
        assert!(l.iter().all(|x| x.1 == blue || x.1 == RED));
    }

    #[test]
    fn a_template_called_by_name_alone_is_read_never_made() {
        let mut s = Session::new();
        house(&mut s);
        let before = s.model.clone();
        let e = s.execute("template", &json!({"name": "Nope"})).unwrap_err();
        assert!(e.contains("Nope"), "{e}");
        assert_eq!(*s.model, *before);
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["rooms"]}));
        run(&mut s, "template", json!({"name": "T", "from": 100000}));
        let steps = s.history().len();
        let r = run(&mut s, "template", json!({"name": "T"}));
        assert_eq!(r["hidden"], json!(["room"]));
        assert_eq!(s.history().len(), steps, "reading is no step");
    }

    #[test]
    fn a_view_stops_following_a_template_and_changes_what_it_controlled_in_one_call() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "template", json!({"name": "T", "from": 100000, "controls": ["categories"]}));
        run(&mut s, "graphics", json!({"view": 100001, "template": "T"}));
        let e = s.execute("graphics", &json!({"view": 100001, "hide": ["rooms"]})).unwrap_err();
        assert!(e.contains("`T`"), "{e}");
        let r = run(&mut s, "graphics", json!({"view": 100001, "template": null, "hide": ["rooms"]}));
        assert_eq!((r["template"].clone(), r["hidden"].clone()), (Value::Null, json!(["room"])));
        // Following one that controls them while changing them is the same mistake, made in one go.
        let e = s.execute("graphics", &json!({"view": 100001, "template": "T", "hide": ["grids"]})).unwrap_err();
        assert!(e.contains("`T`"), "{e}");
    }

    #[test]
    fn a_call_that_changes_nothing_makes_nothing_and_leaves_no_undo_step() {
        let mut s = Session::new();
        house(&mut s);
        let (before, steps) = (s.model.clone(), s.history().len());
        for p in [
            json!({"view": "{3D}", "reset": false}),
            json!({"view": "{3D}", "show": ["walls"]}),
            json!({"view": "{3D}", "hide_elements": []}),
            json!({"view": 100000, "show": ["walls"]}),
            json!({"view": 100000, "reset": true}),
            json!({"view": 100000, "categories": {}}),
            json!({"view": 100000}),
        ] {
            run(&mut s, "graphics", p);
        }
        assert!(s.model.default_3d().is_none(), "no {{3D}} view made for nothing");
        assert_eq!((*s.model == *before, s.history().len()), (true, steps));
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls"]}));
        assert!(s.model.default_3d().is_some());
        assert_eq!(s.history().len(), steps + 1);
    }

    #[test]
    fn modify_leaves_a_view_what_its_template_controls() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "filter", json!({"name": "F", "categories": ["walls"]}));
        run(&mut s, "template", json!({"name": "T", "from": 100000}));
        run(&mut s, "graphics", json!({"view": 100001, "template": "T"}));
        let before = s.model.clone();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("modify", &json!({"id": 100001, "graphics": p})).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"categories": {"room": {"hidden": true}}}), "`T`");
        bad(json!({"filters": [{"filter": "F"}]}), "`T`");
        let e = s.execute("modify", &json!({"id": 100001, "view_range": {"cut": 1500}})).unwrap_err();
        assert!(e.contains("`T`") && e.contains("view range"), "{e}");
        assert_eq!(*s.model, *before);
        // Its name, and what the template leaves be: free.
        run(&mut s, "modify", json!({"id": 100001, "name": "Second"}));
        run(&mut s, "modify", json!({"id": 100001, "graphics": {"elements": {"100002": {"hidden": true}}}}));
        run(&mut s, "template", json!({"name": "T", "controls": ["filters"]}));
        run(&mut s, "modify", json!({"id": 100001, "view_range": {"cut": 1500}}));
        run(&mut s, "modify", json!({"id": 100001, "graphics": {"categories": {"room": {"hidden": true}}}}));
        // Unfollowing and changing it together is allowed, as with `graphics`.
        run(&mut s, "template", json!({"name": "T", "controls": ["categories", "range"]}));
        run(&mut s, "modify", json!({"id": 100001, "graphics": {"template": null}, "view_range": {"cut": 1700}}));
        assert_eq!(s.model.plan_range(100001).cut, 3000.0 + 1700.0);
    }

    #[test]
    fn a_cut_weight_leaves_the_thin_lines_between_a_walls_layers_thin() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "modify", json!({"ids": [walls[0], walls[1]], "type": "Exterior - 300mm"}));
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 8, "dash": "dashed"}}}}));
        run(&mut s, "graphics", json!({"view": section, "categories": {"walls": {"cut": {"weight": 8}}}}));
        let thin = (4, Rgb(25, 25, 25), None);
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).contains(&(8, Rgb(25, 25, 25), Some([10.0, 5.0]))), "the outline");
        assert!(lines(&d, walls[0]).contains(&thin), "the layers' lines");
        let view = render::section_fit(&s.model, section, 800, 600);
        let d = render::section(&s.model, section, view);
        assert!(lines(&d, walls[1]).iter().any(|l| l.0 == 8));
        assert!(lines(&d, walls[1]).contains(&thin), "the layers' lines, in a section");
    }

    #[test]
    fn deleting_an_element_takes_its_overrides_from_every_view() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": 100000, "hide_elements": [walls[0]]}));
        run(
            &mut s,
            "graphics",
            json!({"view": section, "elements": {walls[0].to_string(): {"halftone": true}, walls[1].to_string(): {"halftone": true}}}),
        );
        run(&mut s, "graphics", json!({"view": "{3D}", "hide_elements": [walls[0], walls[2]]}));
        run(&mut s, "delete", json!({"ids": [walls[0]]}));
        let held = |s: &Session, view: u64| match crate::graphics::graphics_of(&s.model, view) {
            Some(g) => g.elements.keys().map(|k| s.model.elements.id(k).unwrap()).collect::<Vec<_>>(),
            _ => panic!("no view {view}"),
        };
        let three_d = s.model.default_3d().unwrap();
        assert_eq!((held(&s, 100000), held(&s, section), held(&s, three_d)), (vec![], vec![walls[1]], vec![walls[2]]));
        run(&mut s, "undo", json!({}));
        assert_eq!(held(&s, 100000), [walls[0]], "undone with it");
    }

    #[test]
    fn a_pick_snaps_to_what_the_view_draws_and_nothing_else() {
        use crate::snap::{Kind, Reach, Snaps, snap};
        let mut s = Session::new();
        let walls = house(&mut s);
        let grid = run(&mut s, "grid", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        let reach = Reach { point: 40.0, line: 20.0 };
        let kind = |s: &Session, at: P2| snap(&s.model, 100000, at, reach, &[], false, &Snaps::default()).kind;
        let (grid_end, corner) = (P2(-995.0, 3005.0), P2(5.0, 5.0));
        assert_eq!((kind(&s, grid_end), kind(&s, corner)), (Kind::Endpoint, Kind::Endpoint));
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["grids"]}));
        assert_ne!(kind(&s, grid_end), Kind::Endpoint, "the grid isn't there to snap to");
        assert_eq!(kind(&s, corner), Kind::Endpoint);
        run(&mut s, "graphics", json!({"view": 100000, "show": ["grids"], "hide_elements": [walls[0], walls[3]]}));
        assert_eq!(kind(&s, grid_end), Kind::Endpoint);
        assert_ne!(kind(&s, corner), Kind::Endpoint, "nor are the walls that met there");
        // Another view's snaps are its own.
        run(&mut s, "graphics", json!({"view": 100001, "hide": ["grids"]}));
        assert_eq!(kind(&s, grid_end), Kind::Endpoint);
        let _ = grid;
    }

    #[test]
    fn tag_all_leaves_out_what_the_plan_hides() {
        let mut s = Session::new();
        house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        let window = run(&mut s, "window", json!({"at": [8000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["windows"]}));
        let tagged = |s: &mut Session| -> Vec<u64> {
            let r = run(s, "tag", json!({}));
            let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            ids.iter()
                .filter_map(|t| match s.model.elements.get(t) {
                    Some(Element::Tag(x)) => s.model.elements.id(x.host),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(tagged(&mut s), [door], "the window is hidden here");
        run(&mut s, "undo", json!({}));
        run(&mut s, "graphics", json!({"view": 100000, "show": ["windows"], "hide_elements": [door]}));
        assert_eq!(tagged(&mut s), [window]);
        run(&mut s, "undo", json!({}));
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["doors", "windows"]}));
        let e = s.execute("tag", &json!({})).unwrap_err();
        assert!(e.contains("nothing to tag"), "{e}");
        // Asked for by name, a hidden one is tagged all the same: it is a deliberate request.
        run(&mut s, "tag", json!({"ids": [door]}));
    }

    #[test]
    fn views_are_framed_to_what_they_show() {
        use crate::view3d::{self, Scene};
        let mut s = Session::new();
        house(&mut s);
        // A column well above the walls: what a section, a 3D view and a sheet's 3D view frame.
        run(&mut s, "column", json!({"at": [4000, 3000], "height": 9000}));
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        let framed = |s: &Session| {
            let scene = Scene::new(&s.model);
            let look = Look::of_3d(&s.model);
            (
                render::section_fit(&s.model, section, 800, 600),
                scene.fit_in(-45.0, 30.0, 400, 300, &look),
                view3d::camera_in(&s.model, -45.0, 30.0, 0.05, 20.0, &look),
            )
        };
        let all = framed(&s);
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["columns"]}));
        run(&mut s, "graphics", json!({"view": section, "hide": ["columns"]}));
        let some = framed(&s);
        assert_ne!(all.0, some.0, "a section's frame");
        assert_ne!(all.1, some.1, "a 3D view's fit");
        assert_ne!((all.2.width, all.2.height), (some.2.width, some.2.height), "a sheet's 3D view");
        run(&mut s, "graphics", json!({"view": "{3D}", "show": ["columns"]}));
        run(&mut s, "graphics", json!({"view": section, "show": ["columns"]}));
        assert_eq!(framed(&s).0, all.0);
    }

    #[test]
    fn a_wall_under_the_cut_plane_is_hidden_with_the_others() {
        let mut s = Session::new();
        house(&mut s);
        let low = run(&mut s, "wall", json!({"points": [[1000, 1000], [3000, 1000]], "height": 1000}))["ids"][0].as_u64().unwrap();
        assert!(!owned(&plan(&s, 100000), low).is_empty());
        run(&mut s, "graphics", json!({"view": 100000, "hide_elements": [low]}));
        assert!(owned(&plan(&s, 100000), low).is_empty());
        run(&mut s, "graphics", json!({"view": 100000, "show_elements": [low], "hide": ["walls"]}));
        assert!(owned(&plan(&s, 100000), low).is_empty());
    }

    #[test]
    fn a_3d_view_on_a_sheet_is_drawn_as_its_view_says() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "sheet", json!({"name": "Views"}));
        run(&mut s, "place", json!({"sheet": "A101", "view": "{3D}", "scale": 200}));
        let (sheet, vp) = (100006, 100008);
        let shaded = |s: &Session| {
            let d = crate::sheet::draw(&s.model, sheet).unwrap();
            let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
            let (frame, (ox, oy)) = crate::sheet::frame(&s.model, vp).unwrap();
            (0..frame.width)
                .flat_map(|x| (0..frame.height).map(move |y| (x, y)))
                .filter_map(|(x, y)| pm.pixel(ox as u32 + x, oy as u32 + y))
                .filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255))
                .count()
        };
        assert!(shaded(&s) > 100);
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls"]}));
        assert_eq!(shaded(&s), 0);
    }

    #[test]
    fn a_3d_view_sees_through_what_it_makes_transparent_and_can_still_pick_it() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "floor", json!({}));
        let scene = crate::view3d::Scene::new(&s.model);
        let cam = scene.fit(-45.0, 50.0, 320, 240);
        let render = |s: &mut Session, walls_as: Value| {
            run(s, "graphics", json!({"view": "{3D}", "reset": true, "categories": {"walls": walls_as}}));
            let look = Look::of_3d(&s.model);
            scene.render_in(&cam, &[], &look)
        };
        let solid = render(&mut s, json!({"halftone": false}));
        let gone = render(&mut s, json!({"hidden": true}));
        let clear = render(&mut s, json!({"transparency": 100}));
        let half = render(&mut s, json!({"transparency": 50}));
        assert_eq!(clear.rgba, gone.rgba, "all the way through, it is as if it were hidden");
        assert_ne!(solid.rgba, gone.rgba);
        // Halfway, a pixel the wall covers is between the wall's and what lies behind it.
        let luma = |img: &crate::view3d::Image, i: usize| {
            img.rgba.get(i * 4..i * 4 + 3).map_or(0, |c| c.iter().map(|v| u32::from(*v)).sum::<u32>())
        };
        let covered: Vec<usize> = (0..320 * 240).filter(|i| luma(&solid, *i) != luma(&gone, *i)).collect();
        assert!(covered.len() > 300);
        let between = covered.iter().filter(|i| {
            let (a, b, m) = (luma(&solid, **i), luma(&gone, **i), luma(&half, **i));
            m >= a.min(b) && m <= a.max(b)
        });
        assert!(between.count() * 10 > covered.len() * 9, "most of them");
        assert!(covered.iter().any(|i| luma(&half, *i) != luma(&solid, *i) && luma(&half, *i) != luma(&gone, *i)));
        // The wall is still what a click on it picks, seen through or not; hidden, it is not.
        let at = covered.iter().copied().find(|i| solid.pick((i % 320) as f64, (i / 320) as f64) == Some(walls[0])).unwrap();
        let (x, y) = ((at % 320) as f64, (at / 320) as f64);
        assert_eq!(half.pick(x, y), Some(walls[0]));
        assert_eq!(clear.pick(x, y), Some(walls[0]));
        assert_ne!(gone.pick(x, y), Some(walls[0]));
        // A section's hidden line does not see through anything.
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": section, "categories": {"walls": {"transparency": 100}}}));
        let view = render::section_fit(&s.model, section, 800, 600);
        let d = render::section(&s.model, section, view);
        assert!(!owned(&d, walls[1]).is_empty());
    }

    #[test]
    fn a_projections_line_colour_draws_the_edges_in_3d_and_beyond_a_sections_line() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        let exactly = |pm: &tiny_skia::Pixmap, c: Rgb| painted(pm, |r, g, b| (r, g, b) == (c.0, c.1, c.2));
        let beyond = |s: &Session, c: Rgb| {
            let view = render::section_fit(&s.model, section, 800, 600);
            let pm = tiny_skia::Pixmap::decode_png(&render::section_png(&s.model, section, view).unwrap()).unwrap();
            exactly(&pm, c)
        };
        let edge = Rgb(48, 48, 48);
        let (edges3d, edges_section) = (exactly(&picture(&mut s, "{3D}", "e0"), edge), beyond(&s, edge));
        assert!(edges3d > 100 && edges_section > 20, "{edges3d} {edges_section}");
        run(&mut s, "graphics", json!({"view": "{3D}", "categories": {"walls": {"projection": {"color": [200, 0, 0]}}}}));
        run(&mut s, "graphics", json!({"view": section, "categories": {"walls": {"projection": {"color": [200, 0, 0]}}}}));
        assert!(exactly(&picture(&mut s, "{3D}", "e1"), RED) > 100, "3D edges in red");
        assert!(beyond(&s, RED) > 20, "a section's lines beyond its line");
        assert_eq!(exactly(&picture(&mut s, "{3D}", "e2"), edge), 0);
        // Halftone lightens them, and an element's own colour beats its category's.
        run(&mut s, "graphics", json!({"view": section, "categories": {"walls": {"projection": {"color": null}, "halftone": true}}}));
        let light = Rgb(48 + (255 - 48) / 2, 48 + (255 - 48) / 2, 48 + (255 - 48) / 2);
        assert!(beyond(&s, light) > 20);
        // The north wall, all of it beyond the line.
        run(&mut s, "graphics", json!({"view": section, "elements": {walls[2].to_string(): {"projection": {"color": [0, 0, 200]}}}}));
        let half = |c: u8| c + (255 - c) / 2;
        assert!(beyond(&s, Rgb(half(0), half(0), half(200))) > 5, "its own colour, in the category's halftone");
    }

    #[test]
    fn a_selected_element_seen_through_shows_its_tint() {
        use crate::view3d::{Image, Scene};
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "floor", json!({}));
        run(&mut s, "graphics", json!({"view": "{3D}", "categories": {"walls": {"transparency": 100}}}));
        let scene = Scene::new(&s.model);
        let cam = scene.fit(-45.0, 50.0, 320, 240);
        let look = Look::of(&s.model, s.model.default_3d().unwrap());
        let plain = scene.render_in(&cam, &[], &look);
        let tinted = scene.render_in(&cam, &[(walls[0], Rgb(37, 99, 235))], &look);
        let differing = |a: &Image, b: &Image| a.rgba.chunks(4).zip(b.rgba.chunks(4)).filter(|(x, y)| x != y).count();
        assert!(differing(&plain, &tinted) > 100, "a selected wall you can see through still shows it: {}", differing(&plain, &tinted));
        let other = scene.render_in(&cam, &[(walls[1], Rgb(37, 99, 235))], &look);
        assert!(differing(&tinted, &other) > 100, "each its own");
    }

    #[test]
    fn a_section_leaves_out_what_its_view_hides() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        let view = render::section_fit(&s.model, section, 800, 600);
        let d = render::section(&s.model, section, view);
        assert!(!owned(&d, walls[1]).is_empty() && d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
        run(&mut s, "graphics", json!({"view": section, "hide": ["walls"]}));
        let d = render::section(&s.model, section, view);
        assert!(owned(&d, walls[1]).is_empty() && owned(&d, walls[3]).is_empty());
        assert!(!d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "no poché left over");
        // And the picture beyond the line: nothing of the walls.
        let png = render::section_png(&s.model, section, view).unwrap();
        let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let dark = painted(&pm, |r, g, b| r < 100 && g < 100 && b < 100);
        assert!(dark < 3000, "{dark}: levels and a title, no walls");
        // A cut line weight of its own.
        run(&mut s, "graphics", json!({"view": section, "show": ["walls"], "categories": {"walls": {"cut": {"weight": 7}}}}));
        let d = render::section(&s.model, section, view);
        assert!(lines(&d, walls[1]).iter().any(|l| l.0 == 7));
    }

    #[test]
    fn a_sheet_prints_its_views_as_they_look() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "sheet", json!({"name": "Plans"}));
        run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}));
        let (sheet, vp) = (100006, 100007);
        let at = |s: &Session, p: P2| {
            let (v, (ox, oy)) = crate::sheet::frame(&s.model, vp).unwrap();
            let (x, y) = v.px(p);
            (x + ox, y + oy)
        };
        let colour = |s: &Session| {
            let d = crate::sheet::draw(&s.model, sheet).unwrap();
            let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
            let (x, y) = at(s, P2(4000.0, 0.0));
            let c = pm.pixel(x as u32, y as u32).unwrap();
            Rgb(c.red(), c.green(), c.blue())
        };
        assert_eq!(colour(&s), POCHE);
        run(&mut s, "graphics", json!({"view": 100000, "hide_elements": [walls[0]]}));
        assert_eq!(colour(&s), PAPER);
    }

    #[test]
    fn a_file_from_before_pens_opens_with_each_weight_the_nearest_pen() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "filter", json!({"name": "Long", "categories": ["walls"]}));
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 7}, "projection": {"weight": 3}}},
                   "filters": [{"filter": "Long", "cut": {"weight": 10}}]}),
        );
        let now = s.model.to_json().unwrap();
        // Saved before pens, the same weights were pixels of a sheet's drawing, 5 to the mm.
        fn as_pixels(v: &mut Value) {
            match v {
                Value::Object(o) => {
                    for (k, v) in o.iter_mut() {
                        match (k.as_str(), v.as_u64()) {
                            ("weight", Some(7)) => *v = json!(1.25),
                            ("weight", Some(3)) => *v = json!(0.75),
                            ("weight", Some(10)) => *v = json!(1.75),
                            _ => as_pixels(v),
                        }
                    }
                }
                Value::Array(a) => a.iter_mut().for_each(as_pixels),
                _ => {}
            }
        }
        let mut old: Value = serde_json::from_str(&now).unwrap();
        old["format"] = json!(3);
        as_pixels(&mut old);
        let (m, left_out) = crate::model::Model::read(&old.to_string()).unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(m.to_json().unwrap(), now);
    }

    #[test]
    fn graphics_that_make_no_sense_are_refused_saying_what_to_fix() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "sheet", json!({"name": "Plans"}));
        let before = s.model.clone();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("graphics", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"hide": ["balloons"]}), "balloons");
        bad(json!({"view": "nowhere"}), "nowhere");
        bad(json!({"hide_elements": [9999]}), "9999");
        bad(json!({"hide_elements": [100006]}), "sheet");
        bad(json!({"categories": {"walls": {"cut": {"weight": 17}}}}), "a pen, a whole number from 0 (a hairline) to 16, not 17");
        bad(json!({"categories": {"walls": {"cut": {"weight": 2.5}}}}), "weight");
        bad(json!({"categories": {"walls": {"cut": {"weight": -1}}}}), "weight");
        bad(json!({"categories": {"walls": {"projection": {"weight": "heavy"}}}}), "heavy");
        bad(json!({"categories": {"walls": {"transparency": 101}}}), "transparency");
        bad(json!({"categories": {"walls": {"projection": {"dash": "wavy"}}}}), "wavy");
        bad(json!({"categories": {"walls": {"projection": {"color": [300, 0, 0]}}}}), "300");
        bad(json!({"categories": {"walls": {"projection": {"colour": [3, 0, 0]}}}}), "colour");
        bad(json!({"categories": {"balloons": {"halftone": true}}}), "balloons");
        bad(json!({"elements": {"abc": {"halftone": true}}}), "abc");
        bad(json!({"elements": {"9999": {"halftone": true}}}), "9999");
        bad(json!({"filters": [{"filter": "Nope"}]}), "Nope");
        bad(json!({"template": "Nope"}), "Nope");
        bad(json!({"hide": "walls"}), "hide");
        assert_eq!(*s.model, *before, "a refused command changes nothing");
        // Walls are drawn; levels in sections and grids and so on are categories too; sheets aren't.
        assert!(s.execute("graphics", &json!({"hide": ["sheets"]})).is_err());
        let _ = walls;
    }

    #[test]
    fn a_view_filter_picks_elements_by_rules_and_a_view_draws_them_its_own_way() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "modify", json!({"id": walls[0], "type": "Interior - 100mm"}));
        let r = run(
            &mut s,
            "filter",
            json!({"name": "Interior", "categories": ["walls"], "rules": [{"field": "Type", "op": "equals", "value": "Interior - 100mm"}]}),
        );
        assert_eq!((r["categories"].clone(), r["matches"].clone()), (json!(["wall"]), json!([walls[0]])));
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "filters": [{"filter": "Interior", "cut": {"color": [200, 0, 0], "fill": [250, 220, 220]}}]}),
        );
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).iter().any(|l| l.1 == RED), "{:?}", lines(&d, walls[0]));
        assert!(lines(&d, walls[1]).iter().all(|l| l.1 != RED));
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), Rgb(250, 220, 220));
        // Another view draws it as it always did.
        assert!(lines(&plan(&s, 100001), walls[0]).iter().all(|l| l.1 != RED));
        // Hidden by the filter alone.
        let view = plan(&s, 100000).view;
        run(&mut s, "graphics", json!({"view": 100000, "filters": [{"filter": "Interior", "hidden": true}]}));
        let d = render::draw(&s.model, 100000, view);
        assert!(owned(&d, walls[0]).is_empty() && !owned(&d, walls[1]).is_empty());
        assert_eq!(pixel(&d, P2(4000.0, 0.0)), PAPER);
        // A project parameter, as a schedule's filters read it; the filter follows the model.
        run(&mut s, "parameter", json!({"name": "Fire Rating", "kind": "number", "categories": ["walls"]}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Fire Rating": 2}}));
        let r = run(
            &mut s,
            "filter",
            json!({"name": "Rated", "categories": ["wall"], "rules": [{"field": "Fire Rating", "op": "greater-or-equal", "value": 1}]}),
        );
        assert_eq!(r["matches"], json!([walls[1]]));
        run(&mut s, "parameter", json!({"name": "Fire Rating", "rename": "Fire Class"}));
        let r = run(&mut s, "filter", json!({"name": "Rated"}));
        assert_eq!(
            (r["rules"][0]["field"].as_str(), r["matches"].clone()),
            (Some("Fire Class"), json!([walls[1]])),
            "the rule follows the rename"
        );
        // Not a filter without its rules: one with none picks every element of its categories.
        let r = run(&mut s, "filter", json!({"name": "Walls", "categories": ["walls"]}));
        assert_eq!(r["matches"].as_array().unwrap().len(), 4);
        // The parameter is in use: it stays until the filter lets go of it.
        let e = s.execute("parameter", &json!({"name": "Fire Class", "delete": true})).unwrap_err();
        assert!(e.contains("Rated"), "{e}");
        run(&mut s, "filter", json!({"name": "Rated", "delete": true}));
        run(&mut s, "parameter", json!({"name": "Fire Class", "delete": true}));
    }

    #[test]
    fn filters_apply_in_order_the_first_prevailing_and_an_elements_own_beats_them() {
        let mut s = Session::new();
        let walls = house(&mut s);
        for (name, op, value) in [("A", "equals", "Generic - 200mm"), ("B", "contains", "200")] {
            run(&mut s, "filter", json!({"name": name, "categories": ["walls"], "rules": [{"field": "Type", "op": op, "value": value}]}));
        }
        let first = json!({"filter": "A", "cut": {"color": [200, 0, 0]}});
        let second = json!({"filter": "B", "cut": {"color": [0, 200, 0], "weight": 3}});
        run(&mut s, "graphics", json!({"view": 100000, "filters": [first, second]}));
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).contains(&(3, RED, None)), "the first one's colour, the second's weight: {:?}", lines(&d, walls[0]));
        run(&mut s, "graphics", json!({"view": 100000, "filters": [second, first]}));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(3, Rgb(0, 200, 0), None)));
        run(&mut s, "graphics", json!({"view": 100000, "elements": {walls[0].to_string(): {"cut": {"color": [0, 0, 200]}}}}));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(3, Rgb(0, 0, 200), None)), "its own colour, the filters' weight");
        // The category's override is the weakest, and a filter that hides hides, whatever the others do.
        run(&mut s, "graphics", json!({"view": 100000, "categories": {"walls": {"cut": {"color": [9, 9, 9], "dash": "dotted"}}}}));
        let lines = lines(&plan(&s, 100000), walls[1]);
        assert!(lines.contains(&(3, Rgb(0, 200, 0), Some([2.0, 4.0]))), "{lines:?}");
        run(&mut s, "graphics", json!({"view": 100000, "filters": [first, {"filter": "B", "hidden": true}]}));
        assert!(owned(&plan(&s, 100000), walls[1]).is_empty());
        // A filter applies once, and in a section as in a plan.
        let e = s.execute("graphics", &json!({"filters": [first, first]})).unwrap_err();
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn view_filters_must_make_sense_and_when_one_goes_the_views_stop_applying_it() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("filter", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"categories": ["walls"]}), "name");
        bad(json!({"name": "X"}), "categories");
        bad(json!({"name": "X", "categories": []}), "categories");
        bad(json!({"name": "X", "categories": ["balloons"]}), "balloons");
        bad(json!({"name": "X", "categories": ["walls"], "rules": [{"field": "Nope", "op": "equals", "value": 1}]}), "Nope");
        bad(json!({"name": "X", "categories": ["walls", "rooms"], "rules": [{"field": "Type", "op": "equals", "value": "a"}]}), "Type");
        bad(json!({"name": "X", "categories": ["walls"], "rules": [{"field": "Length", "op": "contains", "value": "a"}]}), "Length");
        bad(json!({"name": "X", "categories": ["walls"], "rules": [{"field": "Length", "op": "greater", "value": "long"}]}), "Length");
        bad(json!({"name": "X", "categories": ["walls"], "rules": "all"}), "rules");
        bad(json!({"name": "Nope", "delete": true}), "Nope");
        run(
            &mut s,
            "filter",
            json!({"name": "Long", "categories": ["walls"], "rules": [{"field": "Length", "op": "greater", "value": 7000}]}),
        );
        let long = run(&mut s, "filter", json!({"name": "Long"}));
        assert_eq!(long["matches"], json!([walls[0], walls[2]]), "read back by name alone");
        for view in [json!(100000), json!(section)] {
            run(&mut s, "graphics", json!({"view": view, "filters": [{"filter": "Long", "hidden": true}]}));
        }
        assert!(owned(&plan(&s, 100000), walls[0]).is_empty());
        let r = run(&mut s, "filter", json!({"name": "Long", "delete": true}));
        assert_eq!(r["deleted"], true);
        assert!(!owned(&plan(&s, 100000), walls[0]).is_empty(), "no longer applied");
        assert_eq!(run(&mut s, "graphics", json!({"view": section}))["filters"], json!([]));
        let e = s.execute("graphics", &json!({"filters": [{"filter": "Long"}]})).unwrap_err();
        assert!(e.contains("Long"), "{e}");
    }

    #[test]
    fn a_view_template_holds_a_views_settings_for_other_views_to_follow() {
        let mut s = Session::new();
        house(&mut s);
        let up =
            run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "level": "Level 2", "height": 2500}))["ids"][0].as_u64().unwrap();
        let grid = run(&mut s, "grid", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["grids"], "categories": {"walls": {"cut": {"color": [200, 0, 0]}}}}));
        let r = run(&mut s, "template", json!({"name": "Presentation", "from": "Level 1"}));
        assert_eq!(r["controls"], json!(["categories", "filters", "range", "phase", "options", "scale", "detail", "style"]));
        assert_eq!((r["hidden"].clone(), r["categories"]["wall"]["cut"]["color"].clone()), (json!(["grid"]), json!([200, 0, 0])));
        assert!(!owned(&plan(&s, 100001), grid).is_empty() && lines(&plan(&s, 100001), up).iter().all(|l| l.1 != RED));
        run(&mut s, "graphics", json!({"view": "Level 2", "template": "Presentation"}));
        assert!(owned(&plan(&s, 100001), grid).is_empty(), "its grids are hidden by the template");
        assert!(lines(&plan(&s, 100001), up).iter().any(|l| l.1 == RED), "its walls are red by it");
        // Reading the view tells what is in force, and what controls it.
        let r = run(&mut s, "graphics", json!({"view": "Level 2"}));
        assert_eq!(
            (r["template"].clone(), r["hidden"].clone(), r["controlled"].clone()),
            (
                json!("Presentation"),
                json!(["grid"]),
                json!(["categories", "filters", "range", "phase", "options", "scale", "detail", "style"])
            )
        );
        // Change the template, and its views follow; the view it came from doesn't.
        run(&mut s, "template", json!({"name": "Presentation", "categories": {"walls": {"cut": {"color": [0, 0, 200]}}}}));
        assert!(lines(&plan(&s, 100001), up).iter().any(|l| l.1 == Rgb(0, 0, 200)));
        assert!(lines(&plan(&s, 100000), 100002).iter().any(|l| l.1 == RED));
        // What a template controls isn't the view's to change.
        let e = s.execute("graphics", &json!({"view": 100001, "hide": ["rooms"]})).unwrap_err();
        assert!(e.contains("Presentation"), "{e}");
        // It can control some of it: here its filters alone, and the view's categories are its own again.
        run(&mut s, "template", json!({"name": "Presentation", "controls": ["filters"]}));
        assert!(!owned(&plan(&s, 100001), grid).is_empty() && lines(&plan(&s, 100001), up).iter().all(|l| l.1 != RED));
        run(&mut s, "graphics", json!({"view": 100001, "hide": ["rooms"]}));
        assert_eq!(run(&mut s, "graphics", json!({"view": 100001}))["hidden"], json!(["room"]));
        // Following no more, a view has its own settings again.
        run(&mut s, "template", json!({"name": "Presentation", "controls": ["categories"]}));
        assert!(run(&mut s, "graphics", json!({"view": 100001, "template": null}))["template"].is_null());
        assert_eq!(run(&mut s, "graphics", json!({"view": 100001}))["hidden"], json!(["room"]));
    }

    #[test]
    fn a_template_controls_a_plans_view_range_and_a_3d_views_settings() {
        let mut s = Session::new();
        house(&mut s);
        let r = run(&mut s, "template", json!({"name": "Low cut", "view_range": {"cut": 500, "top": 2000}, "controls": ["range"]}));
        assert_eq!(r["view_range"], json!({"top": 2000.0, "cut": 500.0, "bottom": 0.0, "depth": 0.0}));
        // The level's own range isn't in force while the template controls it, and stays what it was.
        run(&mut s, "modify", json!({"id": 100001, "view_range": {"cut": 1500}}));
        run(&mut s, "graphics", json!({"view": 100001, "template": "Low cut"}));
        let cut = |s: &Session, level| s.model.plan_range(level).cut;
        assert_eq!((cut(&s, 100000), cut(&s, 100001)), (1200.0, 3500.0));
        let e = s.execute("modify", &json!({"id": 100001, "view_range": {"cut": 1800}})).unwrap_err();
        assert!(e.contains("`Low cut`"), "{e}");
        run(&mut s, "graphics", json!({"view": 100001, "template": null}));
        assert_eq!(cut(&s, 100001), 4500.0);
        // A 3D view follows one as a plan does.
        run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["walls", "levels"]}));
        run(&mut s, "template", json!({"name": "Walls out", "from": "{3D}"}));
        run(&mut s, "sheet", json!({"name": "Views"}));
        run(&mut s, "place", json!({"sheet": "A101", "view": "{3D}", "scale": 200}));
        run(&mut s, "graphics", json!({"view": "{3D}", "reset": true, "template": "Walls out"}));
        let pm = picture(&mut s, "{3D}", "t");
        assert_eq!(painted(&pm, |r, g, b| (r, g, b) != (255, 255, 255)), 0);
    }

    #[test]
    fn a_template_controls_the_scale_detail_level_and_visual_style_of_the_views_that_follow_it() {
        use crate::graphics::{Detail, VisualStyle};
        use crate::render::View;
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true, "type": "Exterior - 300mm"}));
        // Level 2 is held at 1 : 100, coarse; a template made from it says so, and controls every group.
        run(&mut s, "scale", json!({"view": 100001, "scale": 100}));
        run(&mut s, "detail", json!({"view": 100001, "detail": "coarse"}));
        let r = run(&mut s, "template", json!({"name": "Plans", "from": 100001}));
        assert_eq!((r["scale"].clone(), r["detail"].clone(), r["visual_style"].clone()), (json!(100.0), json!("coarse"), json!("shaded")));
        for group in ["scale", "detail", "style"] {
            assert!(r["controls"].as_array().unwrap().contains(&json!(group)), "{group}: {r}");
        }
        // Level 1 follows it: its annotation and its detail are the template's, its own settings stay as they were.
        run(&mut s, "scale", json!({"view": 100000, "scale": 50}));
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let k = |s: &Session| render::annotation(&s.model, 100000, view);
        let layered = |s: &Session| render::draw(&s.model, 100000, view).prims.len();
        let (alone, fine) = (k(&s), layered(&s));
        run(&mut s, "graphics", json!({"view": 100000, "template": "Plans"}));
        assert_eq!((alone, k(&s)), (1.0, 2.0), "1 : 100 at a tenth of a pixel to the mm is twice 1 : 50");
        assert!(layered(&s) < fine, "coarse draws less");
        let own = run(&mut s, "scale", json!({"view": 100000}));
        assert_eq!(own["scale"], 100.0, "it says what is in force");
        assert!(
            matches!(&s.model.elements[&100000], Element::Level(l) if l.graphics.scale == Some(50.0) && l.graphics.detail == Detail::Fine)
        );
        // It can't set what the template controls, and says so as the other groups do; nothing changes.
        let before = s.model.clone();
        for (id, p, says) in [
            ("scale", json!({"view": 100000, "scale": 20}), "scale"),
            ("detail", json!({"view": 100000, "detail": "medium"}), "detail level"),
            ("modify", json!({"id": 100000, "graphics": {"scale": 20}}), "scale"),
            ("modify", json!({"id": 100000, "graphics": {"detail": "medium"}}), "detail level"),
            ("modify", json!({"id": 100000, "graphics": {"style": "wireframe"}}), "visual style"),
        ] {
            let e = s.execute(id, &p).unwrap_err();
            assert!(e.contains("`Plans`") && e.contains(says) && e.contains("template: null"), "{p}: {e}");
        }
        assert_eq!(*s.model, *before);
        // Asking is fine, and so is what it leaves be: a template that controls only the scale leaves the detail level.
        run(&mut s, "template", json!({"name": "Plans", "controls": ["scale"]}));
        run(&mut s, "detail", json!({"view": 100000, "detail": "medium"}));
        assert_eq!(k(&s), 2.0);
        assert!(s.execute("scale", &json!({"view": 100000, "scale": 20})).is_err());
        // The template's own are set on it, and the views that follow it take the change at once.
        run(&mut s, "template", json!({"name": "Plans", "scale": 200, "detail": "fine", "visual_style": "wireframe"}));
        assert_eq!(k(&s), 4.0);
        let r = run(&mut s, "template", json!({"name": "Plans", "scale": 50}));
        assert_eq!((r["scale"].clone(), k(&s)), (json!(50.0), 1.0));
        for mut p in [
            json!({"scale": null}),
            json!({"scale": 0}),
            json!({"scale": 1e9}),
            json!({"scale": "1:50"}),
            json!({"detail": "huge"}),
            json!({"visual_style": "pretty"}),
        ] {
            p["name"] = json!("Plans");
            assert!(s.execute("template", &p).is_err(), "{p}");
        }
        // Followed, it can't be set; unfollowed, it can, and its own settings are what count.
        run(&mut s, "template", json!({"name": "Plans", "scale": 100, "controls": ["scale", "detail"]}));
        let e = s.execute("scale", &json!({"view": 100000, "scale": 50})).unwrap_err();
        assert!(e.contains("`Plans`"), "{e}");
        run(&mut s, "graphics", json!({"view": 100000, "template": null}));
        run(&mut s, "scale", json!({"view": 100000, "scale": 50}));
        assert_eq!(k(&s), 1.0, "1 : 50 at a tenth of a pixel to the mm is what is on screen");
        // A 3D view follows the visual style: wireframe in force while its own is shaded.
        run(&mut s, "visual_style", json!({"style": "wireframe"}));
        run(&mut s, "template", json!({"name": "Wire", "from": "{3D}"}));
        run(&mut s, "graphics", json!({"view": "{3D}", "reset": true, "template": "Wire"}));
        assert_eq!(Look::of_3d(&s.model).style(), VisualStyle::Wireframe);
        assert!(matches!(&s.model.elements[&s.model.default_3d().unwrap()], Element::View3D(v) if v.graphics.style == VisualStyle::Shaded));
        let e = s.execute("visual_style", &json!({"style": "hidden_line"})).unwrap_err();
        assert!(e.contains("`Wire`") && e.contains("visual style"), "{e}");
        run(&mut s, "template", json!({"name": "Wire", "visual_style": "hidden_line"}));
        assert_eq!(Look::of_3d(&s.model).style(), VisualStyle::HiddenLine);
        // A template that doesn't control a group lets the view keep its own.
        run(&mut s, "template", json!({"name": "Wire", "controls": ["categories"]}));
        assert_eq!(Look::of_3d(&s.model).style(), VisualStyle::Shaded);
        run(&mut s, "visual_style", json!({"style": "hidden_line"}));
        assert_eq!(Look::of_3d(&s.model).style(), VisualStyle::HiddenLine);
    }

    #[test]
    fn templates_must_make_sense_and_when_one_goes_its_views_keep_their_own_settings() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "sheet", json!({"name": "Plans"}));
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("template", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"from": "Level 1"}), "name");
        bad(json!({"name": "T", "from": "nowhere"}), "nowhere");
        bad(json!({"name": "T", "from": 100006}), "sheet");
        bad(json!({"name": "T", "controls": ["layout"]}), "layout");
        bad(json!({"name": "T", "controls": "all"}), "controls");
        bad(json!({"name": "T", "view_range": {"cut": -5, "bottom": 10}}), "view range");
        bad(json!({"name": "T", "view_range": [1]}), "view_range");
        bad(json!({"name": "T", "hide_elements": [100002]}), "elements");
        bad(json!({"name": "T", "elements": {"100002": {"halftone": true}}}), "elements");
        bad(json!({"name": "T", "template": "U"}), "template");
        bad(json!({"name": "T", "hide": ["balloons"]}), "balloons");
        bad(json!({"name": "Nope", "delete": true}), "Nope");
        run(&mut s, "graphics", json!({"view": 100000, "hide": ["rooms"]}));
        run(&mut s, "template", json!({"name": "T", "from": 100000}));
        run(&mut s, "graphics", json!({"view": 100001, "template": "T"}));
        let r = run(&mut s, "template", json!({"name": "T", "delete": true}));
        assert_eq!(r["deleted"], true);
        let g = run(&mut s, "graphics", json!({"view": 100001}));
        assert!(g["template"].is_null() && g["controlled"] == json!([]));
        assert_eq!(run(&mut s, "graphics", json!({"view": 100000}))["hidden"], json!(["room"]), "a view's own settings stay");
        let e = s.execute("graphics", &json!({"view": 100001, "template": "T"})).unwrap_err();
        assert!(e.contains('T'), "{e}");
    }

    #[test]
    fn modify_reaches_a_views_graphics_and_the_model_checks_what_it_leaves() {
        let mut s = Session::new();
        house(&mut s);
        let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}))["id"].as_u64().unwrap();
        run(&mut s, "modify", json!({"id": 100000, "graphics": {"categories": {"room": {"hidden": true}}}}));
        assert!(owned(&plan(&s, 100000), room).is_empty());
        let before = s.model.clone();
        for (patch, says) in [
            (json!({"categories": {"balloon": {"hidden": true}}}), "balloon"),
            (json!({"categories": {"wall": {"cut": {"weight": 500}}}}), "weight"),
            (json!({"categories": {"wall": {"transparency": 120}}}), "transparency"),
            (json!({"filters": [{"filter": "Nope"}]}), "Nope"),
            (json!({"template": "Nope"}), "Nope"),
            (json!({"elements": {"x": {"hidden": true}}}), "x"),
        ] {
            let e = s.execute("modify", &json!({"id": 100000, "graphics": patch})).unwrap_err();
            assert!(e.contains(says), "{patch}: {e}");
        }
        assert_eq!(*s.model, *before, "a refused modify changes nothing");
        // And the way back: null takes the override away, and what's left is a plan as it was.
        run(&mut s, "modify", json!({"id": 100000, "graphics": {"categories": {"room": null}}}));
        assert!(!owned(&plan(&s, 100000), room).is_empty());
        assert!(plan_graphics_default(&s));
    }

    fn plan_graphics_default(s: &Session) -> bool {
        matches!(s.model.elements.get(100000), Some(Element::Level(l)) if l.graphics.is_default())
    }

    #[test]
    fn a_views_graphics_are_saved_with_the_project_and_read_back() {
        let mut s = Session::new();
        house(&mut s);
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "hide": ["rooms"], "hide_elements": [100002], "categories": {"walls": {"cut": {"fill": [1, 2, 3], "weight": 5}}}}),
        );
        let path = std::env::temp_dir().join(format!("bimcraft-gfx-{}.bimcraft", std::process::id()));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(t.model, s.model);
        let Some(Element::Level(l)) = t.model.elements.get(100000) else { panic!("level") };
        assert!(l.graphics.hides("room") && l.graphics.elements.get(&t.model.guid(100002)).is_some_and(|o| o.hidden));
        // Reading a view's settings changes nothing.
        let r = run(&mut t, "graphics", json!({"view": 100000}));
        assert_eq!(r["hidden"], json!(["room"]));
        assert_eq!(r["categories"]["wall"]["cut"], json!({"fill": [1, 2, 3], "weight": 5}));
    }
}
