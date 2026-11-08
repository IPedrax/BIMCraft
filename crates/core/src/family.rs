//! Families: BIMCraft's own parametric components (never Autodesk's `.rfa`). A family has
//! parameters, types that set its type parameters, and forms, each a profile swept across a
//! sketch plane and sized by the parameters; a component places one of its types. Formulas read
//! as Revit's do: `Width / 2`, `if(Depth > 600, 4, 2)`.

use bimcraft_i18n::{msg, ui_msg};

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::csg;
use crate::font;
use crate::geom::P2;
use crate::model::{MAX_COORD, MAX_TEXT};
use crate::view3d::{P3, add, cross, mul};

mod bounds;
pub mod constraints;
pub mod forms;
pub mod nested;
mod options;
mod planes;
pub mod sketch;
mod validate;
pub mod views;
pub mod visibility;

pub(crate) use validate::check_family_edits;

pub use constraints::{
    DimKind, DimRef, DimView, Extent, FamilyDimension, MAX_DIMENSIONS, MAX_SWING, Pivot, PivotAt, PivotFrom, PivotMark, PivotPoint,
    Pivoted, Spot, Unsat,
};
pub use nested::{Keep, NestedFamily, NestedInstance};
pub use options::{MAX_OMNICLASS, OPTION_KEYS, OmniClass};
pub use visibility::Visibility;

pub use planes::{
    CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DATUMS, Face, Frame, MAX_LINES, MAX_PLANES, PlaneRef, Planes, REF_LEVEL, RefLine, RefPlane,
    Reference, datums,
};

/// What a family is: how IFC names its components; a door or a window is hosted in a wall, which
/// its `Width` and `Height` open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    #[default]
    Furniture,
    Casework,
    /// A sanitary fixture: a basin, a WC, a sink, a shower.
    Plumbing,
    /// A floor drain.
    Drain,
    /// An elbow, a tee: what joins pipes, carried by a component until fittings have connectors.
    PipeFitting,
    /// A valve, a cleanout.
    PipeAccessory,
    /// A distribution board.
    Equipment,
    /// A power outlet (tomada).
    Outlet,
    /// A light switch (interruptor).
    Switch,
    /// A lightning air terminal (captor).
    AirTerminal,
    /// An earthing electrode (haste de aterramento).
    Earthing,
    /// An earthing inspection box (caixa de inspeção).
    InspectionBox,
    Lighting,
    Generic,
    /// A data outlet (ponto de dados).
    DataOutlet,
    /// A voice outlet (ponto de voz).
    VoiceOutlet,
    /// A patch panel.
    PatchPanel,
    /// A rack.
    Rack,
    Door,
    Window,
}

impl Category {
    /// The categories a component may be of (a door's or a window's family is placed in a wall), in the order an import reads
    /// them.
    pub const PLACED: [Category; 17] = [
        Category::Furniture,
        Category::Plumbing,
        Category::Drain,
        Category::PipeFitting,
        Category::PipeAccessory,
        Category::Equipment,
        Category::Outlet,
        Category::Switch,
        Category::AirTerminal,
        Category::Earthing,
        Category::InspectionBox,
        Category::Lighting,
        Category::Generic,
        Category::DataOutlet,
        Category::VoiceOutlet,
        Category::PatchPanel,
        Category::Rack,
    ];

    /// Whether a section cuts a family of it, poché where its plane passes through a form: what is built into the
    /// building (a door, a window, casework, equipment, a generic model) is; what stands in a room or on a wall (furniture,
    /// fixtures, devices, light fittings) is shown beyond the line and never cut.
    pub fn cut(self) -> bool {
        matches!(
            self,
            Category::Door
                | Category::Window
                | Category::Casework
                | Category::Generic
                | Category::Equipment
                | Category::Rack
                | Category::PipeFitting
                | Category::PipeAccessory
        )
    }

    /// How IFC writes a component of it: the product's class, its predefined type, and the object type a proxy carries where
    /// the class has no home for it (IFC4 has no lightning protection).
    pub fn ifc(self) -> (&'static str, &'static str, Option<&'static str>) {
        const OPEN: &str = ".NOTDEFINED.";
        match self {
            Category::Furniture | Category::Casework => ("IFCFURNITURE", OPEN, None),
            Category::Plumbing => ("IFCSANITARYTERMINAL", OPEN, None),
            Category::Drain => ("IFCWASTETERMINAL", ".FLOORTRAP.", None),
            Category::PipeFitting => ("IFCPIPEFITTING", OPEN, None),
            Category::PipeAccessory => ("IFCVALVE", OPEN, None),
            Category::Equipment => ("IFCELECTRICDISTRIBUTIONBOARD", ".DISTRIBUTIONBOARD.", None),
            Category::Outlet => ("IFCOUTLET", ".POWEROUTLET.", None),
            Category::Switch => ("IFCSWITCHINGDEVICE", ".TOGGLESWITCH.", None),
            // IFC4 and IFC4X3 have no class for lightning protection: a proxy of an object type a reader can filter on.
            Category::DataOutlet => ("IFCOUTLET", ".DATAOUTLET.", None),
            Category::VoiceOutlet => ("IFCOUTLET", ".TELEPHONEOUTLET.", None),
            Category::PatchPanel => ("IFCCOMMUNICATIONSAPPLIANCE", ".NETWORKAPPLIANCE.", None),
            Category::Rack => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some(msg!("Rack"))),
            Category::AirTerminal => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("LightningTerminal")),
            Category::Earthing => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("EarthingRod")),
            Category::InspectionBox => ("IFCBUILDINGELEMENTPROXY", ".USERDEFINED.", Some("InspectionBox")),
            Category::Lighting => ("IFCLIGHTFIXTURE", OPEN, None),
            Category::Generic | Category::Door | Category::Window => ("IFCBUILDINGELEMENTPROXY", OPEN, None),
        }
    }

    /// The kinds a family of it may say it is, each with the predefined type IFC writes a component of that kind with, the
    /// category's own where it has none. A kind says which fixture or fitting it is, by the names the sizing commands count
    /// fixtures by (`drain_size`'s and `water_size`'s `wc`, `lav`…), so a WC reaches IFC as a toilet pan, not as a sanitary
    /// terminal of no type. Where two kinds share a predefined type, reading it back gives the first.
    pub fn kinds(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Category::Plumbing => &[
                ("wc", ".TOILETPAN."),
                ("lav", ".WASHHANDBASIN."),
                ("sink", ".SINK."),
                ("laundry", ".SINK."),
                ("shower", ".SHOWER."),
                ("bath", ".BATH."),
                ("bidet", ".BIDET."),
                ("urinal", ".URINAL."),
            ],
            Category::Drain => &[("floor_drain", ".FLOORTRAP."), ("floor_waste", ".FLOORWASTE."), ("roof_drain", ".ROOFDRAIN.")],
            Category::PipeFitting => &[
                ("bend", ".BEND."),
                ("tee", ".JUNCTION."),
                ("cross", ".JUNCTION."),
                ("transition", ".TRANSITION."),
                ("coupling", ".CONNECTOR."),
            ],
            _ => &[],
        }
    }

    /// The kind of it an IFC product of predefined type `predefined` (as the file spells it, without the dots) is, if one is.
    pub fn kind_of(self, predefined: Option<&str>) -> Option<&'static str> {
        let p = predefined?;
        self.kinds().iter().find(|(_, t)| t.trim_matches('.') == p).map(|(k, _)| *k)
    }

    /// Whether a device of it draws or switches a load, so its family should hold a connector that says how much.
    pub fn electrical_device(self) -> bool {
        matches!(self, Category::Lighting | Category::Outlet | Category::Switch)
    }

    /// The classes other tools write a product of it as, besides the one BIMCraft does.
    fn classes(self) -> &'static [&'static str] {
        match self {
            Category::Furniture => &["IFCFURNISHINGELEMENT", "IFCSYSTEMFURNITUREELEMENT"],
            _ => &[],
        }
    }

    /// Every IFC class a component may be read from.
    pub fn read_classes() -> Vec<&'static str> {
        Category::PLACED.into_iter().flat_map(|c| std::iter::once(c.ifc().0).chain(c.classes().iter().copied())).collect()
    }

    /// The category an IFC product of `class` is, from its predefined type `kind` (as the file spells it, without the dots) and
    /// its object type: the one that names the object type, else the one whose predefined type it is, else the class's first.
    pub fn from_ifc(class: &str, kind: Option<&str>, object_type: Option<&str>) -> Option<Category> {
        Category::PLACED
            .into_iter()
            .filter(|c| {
                let (own, _, object) = c.ifc();
                (own == class || c.classes().contains(&class)) && (object.is_none() || object == object_type)
            })
            .fold(None, |best: Option<(Category, u8)>, c| {
                let (_, predefined, object) = c.ifc();
                let score = 2 * u8::from(object.is_some()) + u8::from(Some(predefined.trim_matches('.')) == kind);
                // Strictly better, so a tie goes to the first.
                if best.is_none_or(|(_, s)| score > s) { Some((c, score)) } else { best }
            })
            .map(|(c, _)| c)
    }

    /// Whether a family of it stands in a wall: a door's or a window's, drawn in the opening's own axes (x along
    /// the wall from the opening's middle, y across it toward the wall's left, z up from its sill).
    pub fn hosted(self) -> bool {
        matches!(self, Category::Door | Category::Window)
    }

    /// The category views and Object Styles know a family of it by: `door` and `window` for the hosted ones, else its
    /// components' ([`crate::graphics::component_category`]).
    pub fn graphics_category(self) -> &'static str {
        match self {
            Category::Door => "door",
            Category::Window => "window",
            other => crate::graphics::component_category(other).unwrap_or(crate::graphics::COMPONENT),
        }
    }
}

/// What a hosted family reads of its wall: the thickness, mm, in formulas.
pub const WALL: &str = "Wall Thickness";
/// A wall as thick as this is assumed where a hosted family is built without one: 200 mm.
pub const DEFAULT_WALL: f64 = 200.0;
/// The parameters that open a hosted family's wall: its width and its height, and the height of its sill.
pub const WIDTH: &str = msg!("Width");
pub const HEIGHT: &str = msg!("Height");
pub const SILL: &str = ui_msg!("Sill Height");

/// What a parameter holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Length,
    Number,
    Angle,
    YesNo,
    Material,
    /// Words, up to [`MAX_TEXT`] characters: no formula reads them, and none works them out.
    Text,
    /// A whole number; a formula's result is rounded half away from zero.
    Integer,
    /// Square millimetres, as a formula over lengths in mm works it out; schedules and labels show m².
    Area,
    /// Cubic millimetres; schedules and labels show m³.
    Volume,
    /// A web address, up to [`MAX_URL`] characters on one line: text, as [`Kind::Text`] is.
    Url,
    /// A type of a family of the category its parameter's `of` names, for nested families: none is chosen yet.
    FamilyType,
}

impl Kind {
    /// Whether its values are words (a material's name, text, a URL, a family type), which no formula reads or works out.
    pub fn text(self) -> bool {
        matches!(self, Kind::Material | Kind::Text | Kind::Url | Kind::FamilyType)
    }

    /// What it holds, as messages say: `a length`, `text`.
    pub fn said(self) -> &'static str {
        match self {
            Kind::Length => "a length",
            Kind::Number => "a number",
            Kind::Angle => "an angle",
            Kind::YesNo => "yes or no",
            Kind::Material => "a material",
            Kind::Text => "text",
            Kind::Integer => "a whole number",
            Kind::Area => "an area",
            Kind::Volume => "a volume",
            Kind::Url => "a URL",
            Kind::FamilyType => "a family type",
        }
    }
}

/// The heading a parameter is listed under, as Revit's "group parameter under" sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Constraints,
    Construction,
    Dimensions,
    Graphics,
    IdentityData,
    MaterialsAndFinishes,
    Text,
    Visibility,
    Electrical,
    Mechanical,
    Plumbing,
    Structural,
    Data,
    Other,
}

impl Group {
    /// Every group, in the order a palette lists them.
    pub const ALL: [Group; 14] = [
        Group::Constraints,
        Group::Construction,
        Group::Graphics,
        Group::MaterialsAndFinishes,
        Group::Dimensions,
        Group::IdentityData,
        Group::Text,
        Group::Visibility,
        Group::Electrical,
        Group::Mechanical,
        Group::Plumbing,
        Group::Structural,
        Group::Data,
        Group::Other,
    ];

    /// Its heading, in English: an interface translates it.
    pub fn label(self) -> &'static str {
        match self {
            Group::Constraints => ui_msg!("Constraints"),
            Group::Construction => ui_msg!("Construction"),
            Group::Dimensions => ui_msg!("Dimensions"),
            Group::Graphics => ui_msg!("Graphics"),
            Group::IdentityData => ui_msg!("Identity Data"),
            Group::MaterialsAndFinishes => ui_msg!("Materials and Finishes"),
            Group::Text => ui_msg!("Text"),
            Group::Visibility => ui_msg!("Visibility"),
            Group::Electrical => ui_msg!("Electrical"),
            Group::Mechanical => ui_msg!("Mechanical"),
            Group::Plumbing => ui_msg!("Plumbing"),
            Group::Structural => ui_msg!("Structural"),
            Group::Data => ui_msg!("Data"),
            Group::Other => ui_msg!("Other"),
        }
    }
}

/// The longest URL a parameter holds, in characters.
pub const MAX_URL: usize = 2048;

/// A parameter: a type's (each type its own value) or, `instance`, each component's. A family parameter is the family's
/// own; with `shared`, it is a shared parameter, the same one in every family and project that holds that GUID.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub instance: bool,
    /// Where no type or component sets it: a number (yes/no, true or false), or a material's name.
    #[serde(default)]
    pub value: Value,
    /// Computed from the others, as Revit's formulas are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The heading it is listed under; none, its kind's ([`Parameter::group`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Group>,
    /// A shared parameter's GUID, which is the same wherever it is used: none, a family parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<crate::ids::Guid>,
    /// Reads a dimension and never drives one: a length or an angle that no type or component sets. Until labelled
    /// dimensions exist it holds its `value`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reporting: bool,
    /// A family type parameter's: the category of the families whose types it picks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<Category>,
}

impl Parameter {
    /// The heading it is listed under: its own, else its kind's (lengths, areas, volumes and angles under Dimensions,
    /// materials under Materials and Finishes, the rest under Other).
    pub fn group(&self) -> Group {
        self.group.unwrap_or(match self.kind {
            Kind::Length | Kind::Area | Kind::Volume | Kind::Angle => Group::Dimensions,
            Kind::Material => Group::MaterialsAndFinishes,
            _ => Group::Other,
        })
    }

    /// The formula that works it out, when one does: then no type or component sets it, and Family Types and Properties
    /// show what it comes to, greyed, as Revit's do.
    pub fn driven(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    /// What it may be on its own, `who` saying which it is: a family type parameter names its category and holds no value,
    /// nothing else names one; a reporting parameter is a length or an angle worked out by no formula; a shared one's GUID
    /// is a GUID.
    pub fn check_own(&self, who: &str) -> Result<(), String> {
        match (self.kind, self.of) {
            (Kind::FamilyType, None) => {
                return Err(format!("{who}: a family type parameter needs `of`, the category of the families whose types it picks"));
            }
            (Kind::FamilyType, Some(_)) if !self.value.is_null() => {
                return Err(format!("{who}: a family type parameter holds no value until nested families exist; leave `value` out"));
            }
            (k, Some(_)) if k != Kind::FamilyType => return Err(format!("{who}: `of` is for a family type parameter; leave it out")),
            _ => {}
        }
        if self.reporting {
            if !matches!(self.kind, Kind::Length | Kind::Angle) {
                return Err(format!("{who}: a reporting parameter reads a dimension, so it is a length or an angle"));
            }
            if self.formula.is_some() {
                return Err(format!("{who}: a reporting parameter takes no formula: its dimension says what it is"));
            }
        }
        if let Some(g) = self.shared
            && g.pending().is_some()
        {
            return Err(format!("{who}: `shared` is a GUID, xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx, not {g}"));
        }
        Ok(())
    }
}

/// A number, or a formula over the family's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expr {
    Number(f64),
    Formula(String),
}

impl Default for Expr {
    fn default() -> Expr {
        Expr::Number(0.0)
    }
}

/// The shorthand a form (and a file of 19 or before) names its sketch plane by, from before reference planes: across the
/// family's x and y, swept up (Ref. Level); upright, across x and z, swept along +y (Center (Front/Back), whose normal is
/// -y, so its `from` and `to` are negated and swap places, a formula wrapped as `-(…)`; a revolve's angles stay as they
/// are); or across y and z, swept along x (Center (Left/Right)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[default]
    Xy,
    Xz,
    Yz,
}

impl Plane {
    /// The datum it stands for.
    pub fn host(self) -> PlaneRef {
        PlaneRef::Plane(match self {
            Plane::Xy => REF_LEVEL,
            Plane::Xz => CENTER_FRONT_BACK,
            Plane::Yz => CENTER_LEFT_RIGHT,
        })
    }
}

impl Expr {
    /// Its negative: a number's, or the formula wrapped as `-(…)`.
    pub fn negated(&self) -> Expr {
        match self {
            Expr::Number(x) if *x == 0.0 => Expr::Number(0.0),
            Expr::Number(x) => Expr::Number(-x),
            Expr::Formula(f) => Expr::Formula(format!("-({f})")),
        }
    }
}

/// A round profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Circle {
    pub centre: [Expr; 2],
    pub radius: Expr,
}

/// A round line: its middle, its radius, and the angles it runs between, degrees counter-clockwise from +x.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub centre: [Expr; 2],
    pub radius: Expr,
    #[serde(default)]
    pub from: Expr,
    pub to: Expr,
}

/// Which side of its point a label's lettering starts, is centred or ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    fn is_left(&self) -> bool {
        *self == Align::Left
    }
}

/// A label a symbol letters: capitals `height` mm tall, `at` the middle of the line of text vertically, where `{Parameter Name}` in `value` reads
/// that parameter of the family, started, centred or ended (`align`) at `at`; it is drawn upright and unmirrored whatever the component does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub at: [Expr; 2],
    pub height: Expr,
    pub value: String,
    #[serde(default, skip_serializing_if = "Align::is_left")]
    pub align: Align,
}

/// A mark a family's plan draws in its own axes, sized by its parameters: the polyline of a `line` or an `arc`, a solid
/// disc, or a label: its anchor, which is placed with the component, and its lettering, which never is (it is drawn upright,
/// by [`lettering`], wherever it ends up).
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line(Vec<P2>),
    Dot { centre: P2, radius: f64 },
    Label { at: P2, height: f64, text: String, align: Align },
}

/// A mark and the subcategory key it is drawn in (`dooropening`); none, its family's category.
pub type Tagged = (Mark, Option<&'static str>);

impl Mark {
    /// The same mark with every point taken through `f`.
    pub fn map(&self, f: impl Fn(P2) -> P2) -> Mark {
        match self {
            Mark::Line(pts) => Mark::Line(pts.iter().map(|p| f(*p)).collect()),
            Mark::Dot { centre, radius } => Mark::Dot { centre: f(*centre), radius: *radius },
            Mark::Label { at, height, text, align } => Mark::Label { at: f(*at), height: *height, text: text.clone(), align: *align },
        }
    }
}

/// A mark a family's plan draws in its own axes, and no view shows otherwise (a door's swing, a socket's circle): a `line`
/// through points, an `arc`, a `text` label or a `dot`, a solid disc; `visible` (a yes/no parameter, or a formula) whether it's
/// there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Symbol {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line: Vec<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dot: Option<Circle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// The subcategory of its family's category it is drawn in, by its English name (`Opening`: a door's swing, a window's
    /// opening lines), which a view hides and restyles apart from the rest ([`crate::graphics::SUBCATEGORIES`]); none, the
    /// category itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
    /// Its Visibility Settings (F8): the detail levels it is drawn at; none, Medium and Fine
    /// ([`visibility::SYMBOL_DEFAULT`]), as a plan's symbols always were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

/// What a connector joins: `power` carries a load, `switch` only switches one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorKind {
    #[default]
    Power,
    Switch,
}

/// How a connector's load is shared among its poles: the same on each (`power-balanced`), or each its own
/// (`power-unbalanced`); a `switch` has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorSystem {
    #[default]
    PowerBalanced,
    PowerUnbalanced,
    Switch,
}

/// A connector's load in volt-amperes: one number (shared evenly among the poles), or, for an unbalanced connector,
/// one for each pole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Load {
    One(Expr),
    PerPhase(Vec<Expr>),
}

impl Default for Load {
    fn default() -> Load {
        Load::One(Expr::default())
    }
}

/// Where a family joins a system, as Revit's connector does: what an electrical device draws, what a switch controls.
/// It has no place yet: upstream #269 gives it `at` and `direction`, and then it takes an optional `host` ([`PlaneRef`]),
/// `at` read in that plane's axes and `direction` its normal by default.
/// `voltage`, `load_va` and `power_factor` are numbers or formulas over the family's parameters, so a type or a
/// component can set them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    pub kind: ConnectorKind,
    pub system: ConnectorSystem,
    #[serde(default = "one_pole")]
    pub poles: u8,
    /// Volts.
    #[serde(default)]
    pub voltage: Expr,
    #[serde(default)]
    pub load_va: Load,
    #[serde(default = "unity")]
    pub power_factor: Expr,
    /// What the device is for: `lighting`, `receptacle`…, as a schedule would group it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub classification: String,
}

fn one_pole() -> u8 {
    1
}

fn unity() -> Expr {
    Expr::Number(1.0)
}

/// A connector, worked out for one type and one component's values.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectorValues {
    pub id: crate::ids::Id,
    pub guid: crate::ids::Guid,
    pub kind: ConnectorKind,
    pub system: ConnectorSystem,
    pub poles: u8,
    pub voltage: f64,
    /// The whole load, VA: the phases' sum.
    pub load_va: f64,
    /// Each phase's load, VA; none for a switch.
    pub load_va_by_phase: Vec<f64>,
    pub power_factor: f64,
    pub classification: String,
}

/// A solid of a family, in its own axes (x along the component's angle, y to its left, z up from
/// its base): a profile drawn on `host` and swept along its normal from `from` to `to`; or, with an
/// `axis`, turned about it from `from` to `to` degrees; or triangles, as a file brings them. A
/// material's name, or a material parameter's, says what it's made of; `visible` (a yes/no
/// parameter, or a formula) whether it's there. A `void` is no solid: it cuts the solid forms listed
/// before it that it passes through, and is never seen itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "FormIn")]
pub struct Form {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    /// The plane its profile is drawn on, in that plane's axes, and swept along its normal from `from` to `to`
    /// (Revit's Extrusion Start and End); a revolve turns in it. Ref. Level unless said.
    #[serde(default)]
    pub host: PlaneRef,
    /// Its profile's lines and arcs, and a revolve's axis line ([`sketch`]): in place of a file of 23's `profile`,
    /// `circle` and `axis`, which are read as such lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<sketch::FormLine>,
    /// Its start and end along its plane's normal; a revolve's, the angles it turns from and to, right-handed from its
    /// axis' first point toward its second, a full turn at most.
    #[serde(default)]
    pub from: Expr,
    #[serde(default)]
    pub to: Expr,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub void: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// Shared, so an undo step's copy of the model doesn't copy it.
    #[serde(default, skip_serializing_if = "no_mesh")]
    pub mesh: Arc<Vec<[P3; 3]>>,
    /// A Blend's top (F6): lines in its host plane's axes, at `to` along its normal; its `sketch` is then its base, at `from`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top: Vec<sketch::FormLine>,
    /// A Sweep's profile, or a Swept Blend's two (F6): its `sketch` is then the path they are swept along.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<forms::Swept>,
    /// Edit Vertices' Twist (F6): a Blend's top's, or a Swept Blend's second profile's, first vertex moved on by so many.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub twist: i32,
    /// Its Visibility Settings (F8): the views it is drawn in by the way they look and their detail level. None draws as a
    /// file of 30 did. A void cuts whatever they say: they are drawing, and a void is never drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

fn is_zero_i32(x: &i32) -> bool {
    *x == 0
}

fn no_mesh(mesh: &Arc<Vec<[P3; 3]>>) -> bool {
    mesh.is_empty()
}

/// A form as a command or a file gives it: `host`, or `plane`, the shorthand from before reference planes ([`Plane`]).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormIn {
    #[serde(default)]
    id: crate::ids::Id,
    #[serde(default)]
    guid: crate::ids::Guid,
    #[serde(default)]
    host: Option<PlaneRef>,
    #[serde(default)]
    plane: Option<Plane>,
    #[serde(default)]
    sketch: Vec<sketch::FormLine>,
    #[serde(default)]
    profile: Vec<[Expr; 2]>,
    #[serde(default)]
    circle: Option<Circle>,
    #[serde(default)]
    from: Expr,
    #[serde(default)]
    to: Expr,
    #[serde(default)]
    axis: Option<[[Expr; 2]; 2]>,
    #[serde(default)]
    void: bool,
    #[serde(default)]
    material: String,
    #[serde(default)]
    visible: Option<Expr>,
    #[serde(default)]
    mesh: Arc<Vec<[P3; 3]>>,
    #[serde(default)]
    top: Vec<sketch::FormLine>,
    #[serde(default)]
    sweep: Option<forms::Swept>,
    #[serde(default)]
    twist: i32,
    #[serde(default)]
    visibility: Option<Visibility>,
}

impl TryFrom<FormIn> for Form {
    type Error = String;
    fn try_from(f: FormIn) -> Result<Form, String> {
        let (host, from, to) = match (f.host, f.plane) {
            (Some(_), Some(_)) => return Err("a form takes `host` or `plane` (the shorthand for a datum), not both".into()),
            (Some(host), None) => (host, f.from, f.to),
            (None, None) => (PlaneRef::default(), f.from, f.to),
            // Across x and z it swept along +y; Center (Front/Back)'s normal is -y.
            (None, Some(Plane::Xz)) if f.axis.is_none() && !f.sketch.iter().any(|l| l.axis) => {
                (Plane::Xz.host(), f.to.negated(), f.from.negated())
            }
            (None, Some(plane)) => (plane.host(), f.from, f.to),
        };
        // The shorthands from before sketches add to the sketch what it doesn't have: its profile, its axis.
        let mut lines = f.sketch;
        let profile = usize::from(!f.profile.is_empty()) + usize::from(f.circle.is_some());
        if profile > 1 || (profile == 1 && lines.iter().any(|l| !l.axis)) {
            return Err("a form's profile is its `sketch`, its `profile` or its `circle`, one of them".into());
        }
        if !f.profile.is_empty() {
            lines.splice(0..0, sketch::profile_lines(&f.profile));
        }
        if let Some(c) = &f.circle {
            lines.insert(0, sketch::circle_line(c));
        }
        if let Some(a) = &f.axis {
            if lines.iter().any(|l| l.axis) {
                return Err("a form's axis is its `axis` or its sketch's axis line, one of them".into());
            }
            lines.push(sketch::axis_line(a));
        }
        Ok(Form {
            id: f.id,
            guid: f.guid,
            host,
            sketch: lines,
            from,
            to,
            void: f.void,
            material: f.material,
            visible: f.visible,
            mesh: f.mesh,
            top: f.top,
            sweep: f.sweep,
            twist: f.twist,
            visibility: f.visibility,
        })
    }
}

/// A family: its parameters, its types (each its type parameters' values where they aren't the
/// parameters' own), its reference planes and lines, and its forms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<crate::adaptive::Definition>,
    #[serde(default)]
    pub category: Category,
    /// Which fixture or fitting of its category it is (`wc`, `bend`…), one of [`Category::kinds`]: IFC writes its
    /// components' predefined type from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The IFC class its components came in from a file as, which a generic family's go back out as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifc: Option<IfcClass>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub types: BTreeMap<String, BTreeMap<String, Value>>,
    /// Its reference planes: its three datums first ([`datums`], pinned), then its own. A family read without the datums
    /// gets them.
    #[serde(default = "datums", deserialize_with = "planes::with_datums")]
    pub planes: Vec<RefPlane>,
    /// Its reference lines, each with four planes a form may be drawn on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<RefLine>,
    #[serde(default)]
    pub forms: Vec<Form>,
    /// Its dimensions (F7): between its planes and its extrusions' ends, labelled, locked and EQ, and the locks Align leaves;
    /// what they hold is solved when a type is built (`constraints.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<FamilyDimension>,
    /// What its plan draws besides its forms: lines, arcs, labels and dots of its own, in any category.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
    /// How a door's or a window's leaves open, which IFC and schedules say; none, a door swings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<crate::model::Operation>,
    /// Where it joins a system: an electrical device's load, a switch. At most one is `power`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<Connector>,
    /// Revit's Work Plane-Based: its components may be hosted on a reference plane (`Component::host`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub work_plane_based: bool,
    /// Revit's Always Vertical, on by default: a component hosted on a sloped plane stays upright, turning about z only;
    /// off, it tilts with the plane (`Model::component_placement`).
    #[serde(default = "options::yes", skip_serializing_if = "options::is_yes")]
    pub always_vertical: bool,
    /// Revit's Default Elevation, mm: the height over its level a component of a family that isn't work plane-based is
    /// placed at when it is given none.
    #[serde(default, skip_serializing_if = "options::is_zero")]
    pub default_elevation: f64,
    /// Revit's Cut with Voids When Loaded: kept for a project's voids cutting it (not read yet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut_with_voids_when_loaded: bool,
    /// Revit's Shared, for nested families: kept (not read yet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// The point, in its own axes (mm, numbers or formulas over its length parameters), whose room a component is in; none,
    /// its insertion point's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_calculation_point: Option<[Expr; 3]>,
    /// Whether sections cut its forms, over its category's rule ([`Family::cuts_in_views`]); none, the category's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_in_views: Option<bool>,
    /// Its OmniClass number and title, which IFC writes as a classification reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omniclass: Option<OmniClass>,
    /// The families loaded into it (T6a), each its whole definition, carried inside it ([`nested`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested: Vec<NestedFamily>,
    /// The instances of its nested families placed in it, built into its own pieces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_instances: Vec<NestedInstance>,
}

impl Default for Family {
    fn default() -> Family {
        Family {
            adaptive: None,
            category: Category::default(),
            kind: None,
            ifc: None,
            parameters: Vec::new(),
            types: BTreeMap::new(),
            planes: datums(),
            lines: Vec::new(),
            forms: Vec::new(),
            dimensions: Vec::new(),
            symbols: Vec::new(),
            operation: None,
            connectors: Vec::new(),
            work_plane_based: false,
            always_vertical: true,
            default_elevation: 0.0,
            cut_with_voids_when_loaded: false,
            shared: false,
            room_calculation_point: None,
            cut_in_views: None,
            omniclass: None,
            nested: Vec::new(),
            nested_instances: Vec::new(),
        }
    }
}

/// The IFC class a product came in as, which a generic family remembers (BIMCraft has no element for it): the class as STEP
/// writes it (`IFCPLATE`), its predefined type (`CURTAIN_PANEL`), and the object type that says what it is where its predefined
/// type is `USERDEFINED` or its type object is named otherwise.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IfcClass {
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predefined: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
}

/// The longest IFC class or predefined type a family remembers: IFC's longest are half of it.
pub const MAX_IFC_NAME: usize = 64;

impl IfcClass {
    /// As IFC writes it: the class and the predefined type in capitals, whatever case they were typed in.
    pub fn written(mut self) -> IfcClass {
        self.class = self.class.trim().to_ascii_uppercase();
        self.predefined = self.predefined.map(|p| p.trim().trim_matches('.').to_ascii_uppercase());
        self
    }

    /// Why it isn't one IFC can write, if it isn't: a class `IFC` and capitals or digits, a predefined type of capitals, digits
    /// and `_`, an object type of text.
    pub fn check(&self) -> Result<(), String> {
        let name = |what: &str, v: &str, ok: &dyn Fn(char) -> bool| {
            let shown: String = v.chars().take(40).collect();
            if v.is_empty() || v.len() > MAX_IFC_NAME || !v.chars().all(ok) {
                Err(format!("`ifc` `{what}` `{shown}`: IFC's names are capitals, digits and `_`, up to {MAX_IFC_NAME}"))
            } else {
                Ok(())
            }
        };
        name("class", &self.class, &|c| c.is_ascii_uppercase() || c.is_ascii_digit())?;
        if !self.class.starts_with("IFC") {
            return Err(format!("`ifc` `class` `{}`: an IFC class starts with `IFC` (`IFCPLATE`)", self.class));
        }
        if let Some(p) = &self.predefined {
            name("predefined", p, &|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')?;
        }
        if let Some(o) = &self.object_type
            && (o.trim().is_empty() || o.chars().count() > MAX_TEXT || o.chars().any(char::is_control))
        {
            return Err(format!("`ifc` `object_type`: text of 1 to {MAX_TEXT} characters on one line"));
        }
        Ok(())
    }
}

/// A parameter's value, worked out: a number (yes/no, 1 or 0), or a material's name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Val {
    Number(f64),
    Text(String),
}

/// A form's solid in the family's own axes: its triangles, its caps' first; its outline seen from
/// above, from its bottom to its top, what it's made of; a round one's sides read as one surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub tris: Vec<[P3; 3]>,
    pub caps: usize,
    pub outline: Vec<P2>,
    /// The holes in its outline seen from above (F6: a sweep round a closed path on a level plane), each clockwise.
    pub holes: Vec<Vec<P2>>,
    pub bottom: f64,
    pub top: f64,
    pub material: String,
    pub round: bool,
    /// Its form's Visibility Settings (and a nested instance's over them): where views draw it. None, as a file of 30 drew it.
    pub vis: Option<Visibility>,
}

/// Where one form lies before any void cuts it: its box in the family's own axes (x along a
/// component's angle, y to its left, z up from its base), which is all a warning about where a form
/// stands needs. [`Family::shapes`] works them out, one per form that builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Its place in [`Family::forms`], counting from zero.
    pub index: usize,
    pub void: bool,
    pub min: P3,
    pub max: P3,
}

impl Shape {
    /// The box round `tris`, form `index`'s; none for no triangles.
    fn of(index: usize, void: bool, tris: &[[P3; 3]]) -> Option<Shape> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in tris.iter().flatten() {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        (min[0].is_finite() && max[0].is_finite()).then_some(Shape { index, void, min, max })
    }

    /// Whether it and `other` overlap where all three of them have substance: strictly, a face
    /// touching another's is no overlap, so a void grazing a form cuts nothing.
    pub fn over(&self, other: &Shape) -> bool {
        (0..3).all(|k| self.min[k] < other.max[k] && other.min[k] < self.max[k])
    }

    /// Whether it lies wholly outside `x` wide each way and `z` tall from `low` up: the box a door's
    /// or a window's opening fills.
    pub fn outside(&self, half_width: f64, low: f64, high: f64) -> bool {
        self.max[0] < -half_width || self.min[0] > half_width || self.max[2] < low || self.min[2] > high
    }
}

/// How deep a formula may nest.
const MAX_DEPTH: usize = 64;
/// Sides a circle's profile is drawn with.
const SEGMENTS: usize = 32;
/// Degrees a symbol's arc turns between the corners it is drawn with.
const ARC_STEP: f64 = 3.75;
/// Points a symbol's line may have, and corners a face read from a file is cut into ears with.
pub const MAX_PROFILE: usize = 64;
/// Formulas one may take from in a row: `A` from `B` from `C`…
pub const MAX_CHAIN: usize = 16;
/// Triangles a family's types may build together.
pub const MAX_BUILT: usize = 400_000;
/// Triangles one form, as a file brings it, may have.
pub const MAX_TRIANGLES: usize = 100_000;

/// Parameters, forms, symbols and types one family may have.
pub const MAX_PARAMETERS: usize = 64;
pub const MAX_FORMS: usize = 256;
pub const MAX_SYMBOLS: usize = 64;
pub const MAX_CONNECTORS: usize = 8;
/// The highest voltage and load a connector may hold: far past any real device, short of overflow.
pub const MAX_VOLTAGE: f64 = 1.0e5;
pub const MAX_LOAD_VA: f64 = 1.0e9;
pub const MAX_TYPES: usize = 1000;
/// The longest formula, in characters.
pub const MAX_FORMULA: usize = 1024;

/// The units a number in a formula may be given in, each with what one of it is in the family's own: lengths in mm, areas in
/// mm², volumes in mm³. An angle's `°` (or `º`) is a degree, as angles already are.
const UNITS: [(&str, f64); 10] = [
    ("mm", 1.0),
    ("cm", 10.0),
    ("dm", 100.0),
    ("m", 1000.0),
    ("mm²", 1.0),
    ("cm²", 100.0),
    ("m²", 1.0e6),
    ("mm³", 1.0),
    ("cm³", 1000.0),
    ("m³", 1.0e9),
];

/// How long the word at the start of `chars` is (a letter, then letters, digits and `_`, `²` and `³` among them); none when
/// it doesn't start with a letter.
fn unit_word(chars: &[char]) -> Option<usize> {
    chars.first().filter(|c| c.is_alphabetic())?;
    Some(chars.iter().take_while(|c| c.is_alphanumeric() || **c == '_').count())
}

/// A formula, read.
pub(crate) enum Node {
    Number(f64),
    Name(String),
    Minus(Box<Node>),
    /// A sum's or a product's first term, then each operator and term after it: flat, so a long one
    /// doesn't nest.
    Chain(Box<Node>, Vec<(char, Node)>),
    Op(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// Reads a formula: comparisons over sums over products over powers, which are right-associative.
struct Reader {
    chars: Vec<char>,
    at: usize,
    depth: usize,
}

impl Reader {
    fn peek(&mut self) -> Option<char> {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        self.at += usize::from(found);
        found
    }

    /// One level deeper; refused past `MAX_DEPTH`, so a hostile formula can't exhaust the stack.
    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(format!("it's nested too deep (more than {MAX_DEPTH} levels)")) } else { Ok(()) }
    }

    /// The unit a number just read is given in, as what one of it is in the family's own (mm, mm², mm³, degrees); none, 1.
    /// A word after a number is a unit or nothing a formula may say, since a number is never followed by a name.
    fn unit(&mut self) -> Result<f64, String> {
        let start = self.at;
        let Some(c) = self.peek() else { return Ok(1.0) };
        if matches!(c, '°' | 'º') {
            self.at += 1;
            return Ok(1.0);
        }
        let Some(len) = unit_word(self.chars.get(self.at..).unwrap_or_default()) else {
            self.at = start;
            return Ok(1.0);
        };
        let word: String = self.chars.get(self.at..self.at + len).unwrap_or_default().iter().collect();
        match UNITS.iter().find(|(u, _)| *u == word) {
            Some((_, x)) => {
                self.at += len;
                Ok(*x)
            }
            None => Err(format!("`{word}` isn't a unit a number takes: {}", UNITS.map(|(u, _)| u).join(", "))),
        }
    }

    fn compare(&mut self) -> Result<Node, String> {
        self.deeper()?;
        let a = self.sum()?;
        let op = match self.peek() {
            Some('<') => {
                self.at += 1;
                if self.eat('=') { 'l' } else { '<' }
            }
            Some('>') => {
                self.at += 1;
                if self.eat('=') { 'g' } else { '>' }
            }
            Some('=') => {
                self.at += 1;
                '='
            }
            _ => {
                self.depth -= 1;
                return Ok(a);
            }
        };
        let b = self.sum()?;
        self.depth -= 1;
        Ok(Node::Op(op, Box::new(a), Box::new(b)))
    }

    fn sum(&mut self) -> Result<Node, String> {
        let first = self.product()?;
        let mut rest = Vec::new();
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            rest.push((op, self.product()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn product(&mut self) -> Result<Node, String> {
        let first = self.power()?;
        let mut rest = Vec::new();
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            rest.push((op, self.power()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn power(&mut self) -> Result<Node, String> {
        let a = self.unary()?;
        if !self.eat('^') {
            return Ok(a);
        }
        self.deeper()?;
        let b = self.power()?;
        self.depth -= 1;
        Ok(Node::Op('^', Box::new(a), Box::new(b)))
    }

    fn unary(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(sign @ ('-' | '+')) => {
                self.at += 1;
                self.deeper()?;
                let a = self.unary()?;
                self.depth -= 1;
                Ok(if sign == '-' { Node::Minus(Box::new(a)) } else { a })
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let a = self.compare()?;
                if self.eat(')') { Ok(a) } else { Err("a `(` isn't closed".into()) }
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    self.at += 1;
                }
                // An exponent: 1e6, 2.5E-3.
                if self.chars.get(self.at).is_some_and(|c| matches!(c, 'e' | 'E')) {
                    let sign = usize::from(self.chars.get(self.at + 1).is_some_and(|c| matches!(c, '+' | '-')));
                    if self.chars.get(self.at + 1 + sign).is_some_and(char::is_ascii_digit) {
                        self.at += 1 + sign;
                        while self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
                            self.at += 1;
                        }
                    }
                }
                let text: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect();
                let x: f64 = text.parse().map_err(|_| format!("`{text}` isn't a number"))?;
                Ok(Node::Number(x * self.unit()?))
            }
            // A name may hold spaces, as Revit's do: `Leg Height`.
            Some(c) if c.is_alphabetic() || c == '_' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == ' ') {
                    self.at += 1;
                }
                let name: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect::<String>().trim_end().to_string();
                if !self.eat('(') {
                    return Ok(Node::Name(name));
                }
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        args.push(self.compare()?);
                        if self.eat(')') {
                            break;
                        }
                        if !self.eat(',') {
                            return Err(format!("`{name}(` needs a `,` or a `)`"));
                        }
                    }
                }
                Ok(Node::Call(name, args))
            }
            Some(c) => Err(format!("`{c}` can't start a value there")),
            None => Err("something's missing at its end".into()),
        }
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Node `n`'s value: its names' from `get`; an `if` works out only the branch it takes.
fn value(n: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    Ok(match n {
        Node::Number(x) => *x,
        Node::Name(name) => get(name)?,
        Node::Minus(a) => -value(a, get)?,
        Node::Chain(first, rest) => {
            let mut x = value(first, get)?;
            for (op, n) in rest {
                let y = value(n, get)?;
                x = match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    _ => x / y,
                };
            }
            x
        }
        Node::Op(op, a, b) => {
            let (x, y) = (value(a, get)?, value(b, get)?);
            match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                '/' => x / y,
                '^' => x.powf(y),
                '<' => truth(x < y),
                '>' => truth(x > y),
                'l' => truth(x <= y),
                'g' => truth(x >= y),
                // Equal to within float noise, as Revit compares.
                _ => truth((x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)),
            }
        }
        Node::Call(f, args) => {
            let one = |get: &mut dyn FnMut(&str) -> Result<f64, String>| match args.as_slice() {
                [a] => value(a, get),
                _ => Err(format!("`{f}` takes one value, not {}", args.len())),
            };
            let deg = |x: f64| x.to_radians();
            match f.to_lowercase().as_str() {
                "if" => match args.as_slice() {
                    [c, a, b] => value(if value(c, get)? != 0.0 { a } else { b }, get)?,
                    _ => return Err(format!("`{f}` takes a condition and two values, not {} values", args.len())),
                },
                name @ ("and" | "or" | "min" | "max") => {
                    if args.is_empty() {
                        return Err(format!("`{f}` takes one value or more"));
                    }
                    let xs = args.iter().map(|a| value(a, get)).collect::<Result<Vec<f64>, String>>()?;
                    match name {
                        "and" => truth(xs.iter().all(|x| *x != 0.0)),
                        "or" => truth(xs.iter().any(|x| *x != 0.0)),
                        "min" => xs.iter().copied().fold(f64::INFINITY, f64::min),
                        _ => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    }
                }
                "not" => truth(one(get)? == 0.0),
                "abs" => one(get)?.abs(),
                "sqrt" => one(get)?.sqrt(),
                "round" => one(get)?.round(),
                "roundup" => one(get)?.ceil(),
                "rounddown" => one(get)?.floor(),
                "sin" => deg(one(get)?).sin(),
                "cos" => deg(one(get)?).cos(),
                "tan" => deg(one(get)?).tan(),
                "asin" => one(get)?.asin().to_degrees(),
                "acos" => one(get)?.acos().to_degrees(),
                "atan" => one(get)?.atan().to_degrees(),
                "exp" => one(get)?.exp(),
                "ln" => one(get)?.ln(),
                "log" => one(get)?.log10(),
                "pi" if args.is_empty() => std::f64::consts::PI,
                _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
            }
        }
    })
}

/// The functions a formula calls.
const FUNCTIONS: &str =
    "if, and, or, not, min, max, abs, sqrt, round, roundup, rounddown, sin, cos, tan, asin, acos, atan, exp, ln, log and pi()";

/// The names `formula` reads, in every branch of its `if`s; or why it can't be worked out: it
/// doesn't read, or calls a function there isn't or with the wrong number of values.
pub fn names(formula: &str) -> Result<Vec<String>, String> {
    fn walk(n: &Node, out: &mut Vec<String>) -> Result<(), String> {
        match n {
            Node::Number(_) => {}
            Node::Name(name) if !out.contains(name) => out.push(name.clone()),
            Node::Name(_) => {}
            Node::Minus(a) => walk(a, out)?,
            Node::Chain(first, rest) => {
                walk(first, out)?;
                for (_, n) in rest {
                    walk(n, out)?;
                }
            }
            Node::Op(_, a, b) => {
                walk(a, out)?;
                walk(b, out)?;
            }
            Node::Call(f, args) => {
                let fits = match f.to_lowercase().as_str() {
                    "if" => args.len() == 3,
                    "and" | "or" | "min" | "max" => !args.is_empty(),
                    "pi" => args.is_empty(),
                    "not" | "abs" | "sqrt" | "round" | "roundup" | "rounddown" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
                    | "exp" | "ln" | "log" => args.len() == 1,
                    _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
                };
                if !fits {
                    return Err(format!("`{f}` doesn't take {} values", args.len()));
                }
                for a in args {
                    walk(a, out)?;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&parse(formula)?, &mut out).map_err(|e| format!("formula `{formula}`: {e}"))?;
    Ok(out)
}

/// `formula` worked out, its parameters' values from `get`: numbers, lengths in mm and angles in
/// degrees; a comparison is 1 when it holds, else 0.
pub fn eval(formula: &str, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    worked(formula, &parse(formula)?, get)
}

/// `formula`, read.
fn parse(formula: &str) -> Result<Node, String> {
    if formula.trim().is_empty() {
        return Err("a formula is empty".into());
    }
    let mut r = Reader { chars: formula.chars().collect(), at: 0, depth: 0 };
    // Where it stopped, a character of the formula counted from 1: what can't be read is worth
    // pointing at, not only naming.
    let node = match r.compare() {
        Ok(node) => node,
        Err(e) => return Err(format!("formula `{formula}`: {e} (character {})", r.at + 1)),
    };
    match r.peek() {
        Some(c) => Err(format!("formula `{formula}`: `{c}` follows where it should end (character {})", r.at + 1)),
        None => Ok(node),
    }
}

/// `formula`, read as `node`, worked out: a number, or why it isn't one.
fn worked(formula: &str, node: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let said = |e: String| format!("formula `{formula}`: {e}");
    let x = value(node, get).map_err(said)?;
    if x.is_finite() { Ok(x) } else { Err(said(format!("it comes to {x}, which isn't a number"))) }
}

/// Whether `name` is one a formula can say: a letter or `_` first, then letters, digits, `_` and
/// spaces, none at its end; up to `MAX_TEXT` long.
pub fn nameable(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
        && !name.ends_with(' ')
        && name.chars().count() <= MAX_TEXT
}

/// Why a formula can't say `name`, which none of `parameters` is named, in Revit's words: names are case sensitive, so one that
/// differs from a parameter's in case only is offered.
pub(crate) fn not_a_parameter(name: &str, parameters: &[Parameter]) -> String {
    let near = parameters.iter().find(|p| p.name.to_lowercase() == name.to_lowercase());
    let near = near.map(|p| format!(" (the family has `{}`)", p.name)).unwrap_or_default();
    format!("the following is not a valid parameter: {name}. Note that parameter names are case sensitive{near}.")
}

/// A stored value read as parameter `p` holds it.
pub(crate) fn read(p: &Parameter, v: &Value) -> Result<Val, String> {
    let bad = |what: &str| format!("parameter `{}` holds {what}, not {v}", p.name);
    match p.kind {
        Kind::Material => match v {
            Value::String(s) => Ok(Val::Text(s.clone())),
            Value::Null => Ok(Val::Text(String::new())),
            _ => Err(bad("a material's name")),
        },
        Kind::YesNo => match v {
            Value::Bool(b) => Ok(Val::Number(truth(*b))),
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(|x| Val::Number(truth(x != 0.0))).ok_or_else(|| bad("yes or no")),
        },
        Kind::Length | Kind::Number | Kind::Angle | Kind::Area | Kind::Volume => match v {
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(Val::Number).ok_or_else(|| bad("a number")),
        },
        Kind::Integer => match v {
            Value::Null => Ok(Val::Number(0.0)),
            v => v
                .as_f64()
                .filter(|x| x.is_finite() && x.fract() == 0.0 && x.abs() <= MAX_INTEGER)
                .map(Val::Number)
                .ok_or_else(|| bad("a whole number")),
        },
        Kind::Text | Kind::Url => {
            let (most, what) = if p.kind == Kind::Url { (MAX_URL, "a URL") } else { (MAX_TEXT, "text") };
            match v {
                Value::Null => Ok(Val::Text(String::new())),
                Value::String(t) if t.chars().count() <= most && !t.chars().any(char::is_control) => Ok(Val::Text(t.clone())),
                _ => Err(format!("parameter `{}` holds {what} on one line, up to {most} characters, not {v}", p.name)),
            }
        }
        // None is chosen until nested families exist.
        Kind::FamilyType => match v {
            Value::Null => Ok(Val::Text(String::new())),
            _ => {
                Err(format!("parameter `{}` is a family type parameter, which holds no value until nested families exist, not {v}", p.name))
            }
        },
    }
}

/// The largest whole number an integer parameter holds: past it, a float no longer counts every one.
pub const MAX_INTEGER: f64 = 9_007_199_254_740_992.0;

/// `formula` with every name in it that is `from` said `to` instead: names as the formula reader reads them (a name may hold
/// spaces, so `Door Width` is not `Width`), and never a function's.
pub fn renamed_in_formula(formula: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if c.is_ascii_digit() || c == '.' {
            // A number, with its exponent: `1e6` holds no name `e6`.
            let start = at;
            while chars.get(at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                at += 1;
            }
            if chars.get(at).is_some_and(|c| matches!(c, 'e' | 'E')) {
                let sign = usize::from(chars.get(at + 1).is_some_and(|c| matches!(c, '+' | '-')));
                if chars.get(at + 1 + sign).is_some_and(char::is_ascii_digit) {
                    at += 1 + sign;
                    while chars.get(at).is_some_and(char::is_ascii_digit) {
                        at += 1;
                    }
                }
            }
            // Its unit, which is no name: `2 m` holds no parameter `m`.
            let mut end = at;
            while chars.get(end).is_some_and(|c| c.is_whitespace()) {
                end += 1;
            }
            if let Some(len) = unit_word(&chars[end..]) {
                at = end + len;
            }
            out.extend(&chars[start..at]);
        } else if c.is_alphabetic() || c == '_' {
            let start = at;
            while chars.get(at).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == ' ') {
                at += 1;
            }
            let run: String = chars[start..at].iter().collect();
            let name = run.trim_end();
            let call = chars[at..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
            if name == from && !call {
                out.push_str(to);
                out.push_str(&run[name.len()..]);
            } else {
                out.push_str(&run);
            }
        } else {
            out.push(c);
            at += 1;
        }
    }
    out
}

/// A label's `value` with each `{from}` said `{to}`.
fn renamed_in_label(value: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| c + open) else { break };
        out.push_str(&rest[..=open]);
        let inner = &rest[open + 1..close];
        out.push_str(if inner.trim() == from { to } else { inner });
        out.push('}');
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

impl Family {
    /// Each material it names, given to `f`, which may put another in its place: its material parameters' values, its
    /// types' values of them, and its forms' materials (a form made of a material parameter names that, which is no
    /// material's name).
    pub fn material_refs_mut(&mut self, f: &mut dyn FnMut(&mut String)) {
        let params: BTreeSet<String> = self.parameters.iter().filter(|p| p.kind == Kind::Material).map(|p| p.name.clone()).collect();
        for p in self.parameters.iter_mut().filter(|p| p.kind == Kind::Material) {
            if let Value::String(m) = &mut p.value {
                f(m);
            }
        }
        for values in self.types.values_mut() {
            for (k, v) in values.iter_mut() {
                if let (true, Value::String(m)) = (params.contains(k), v) {
                    f(m);
                }
            }
        }
        if let Some(d) = &mut self.adaptive {
            for node in
                d.nodes.iter_mut().filter(|n| n.is_form() && !n.appearance.material.is_empty() && !params.contains(&n.appearance.material))
            {
                f(&mut node.appearance.material);
            }
        }
        for form in self.forms.iter_mut().filter(|x| !x.material.is_empty() && !params.contains(&x.material)) {
            f(&mut form.material);
        }
        // Its nested families' own, at every depth, and the materials its instances name that are no parameter of its.
        for i in 0..self.nested_instances.len() {
            let materials = self.instance_materials(i);
            for (k, e) in self.nested_instances[i].params.iter_mut() {
                if let Expr::Formula(m) = e
                    && materials.contains(k)
                    && !params.contains(m)
                {
                    f(m);
                }
            }
        }
        for n in &mut self.nested {
            n.family.material_refs_mut(f);
        }
    }

    /// The parameters nested instance `i` sets that are its family's materials.
    fn instance_materials(&self, i: usize) -> Vec<String> {
        let Some(inst) = self.nested_instances.get(i) else { return Vec::new() };
        let Some(n) = self.nested_family(&inst.family) else { return Vec::new() };
        inst.params.keys().filter(|k| n.family.material_parameter(k)).cloned().collect()
    }

    /// Parameter `old` named `new` everywhere the family names it: the parameter itself, its types' values, and every
    /// formula and reference to it (the parameters' formulas, the forms' sizes, materials and `visible`, the symbols' sizes,
    /// `visible` and `{label}` text, the connectors' values, and the adaptive definition's formulas and forms' materials:
    /// [`crate::adaptive::Definition::rename_parameter`]). Every part of a family that holds a formula or names a
    /// parameter must be walked here, so a rename never leaves one behind: a new such part adds its arm. The library
    /// entry (its GUID) and the components' values are the model's to follow (`Library::renamed`).
    pub fn rename_parameter(&mut self, old: &str, new: &str) {
        // Adaptive: every formula of its points and forms (what "=" ties to a parameter), and its forms made of a material
        // parameter.
        let material = self.material_parameter(old);
        if let Some(d) = &mut self.adaptive {
            d.rename_parameter(old, new, material, &|f| renamed_in_formula(f, old, new));
        }
        let mut expr = |e: &mut Expr| {
            if let Expr::Formula(f) = e {
                *f = renamed_in_formula(f, old, new);
            }
        };
        for p in &mut self.parameters {
            if p.name == old {
                p.name = new.to_string();
            }
            if let Some(f) = &mut p.formula {
                *f = renamed_in_formula(f, old, new);
            }
        }
        for values in self.types.values_mut() {
            if let Some(v) = values.remove(old) {
                values.insert(new.to_string(), v);
            }
        }
        for form in &mut self.forms {
            form.all_lines_mut().flat_map(sketch::FormLine::exprs_mut).for_each(&mut expr);
            [&mut form.from, &mut form.to].into_iter().for_each(&mut expr);
            form.visible.iter_mut().for_each(&mut expr);
            if form.material == old {
                form.material = new.to_string();
            }
        }
        for sym in &mut self.symbols {
            sym.line.iter_mut().flatten().for_each(&mut expr);
            if let Some(a) = &mut sym.arc {
                a.centre.iter_mut().chain([&mut a.radius, &mut a.from, &mut a.to]).for_each(&mut expr);
            }
            if let Some(l) = &mut sym.text {
                l.at.iter_mut().chain([&mut l.height]).for_each(&mut expr);
                l.value = renamed_in_label(&l.value, old, new);
            }
            if let Some(d) = &mut sym.dot {
                d.centre.iter_mut().chain([&mut d.radius]).for_each(&mut expr);
            }
            sym.visible.iter_mut().for_each(&mut expr);
        }
        self.room_calculation_point.iter_mut().flatten().for_each(&mut expr);
        for d in &mut self.dimensions {
            if d.label.as_deref() == Some(old) {
                d.label = Some(new.to_string());
            }
        }
        // A pivot's angle (F7d).
        for piv in self.lines.iter_mut().filter_map(|l| l.pivot.as_mut()).chain(self.planes.iter_mut().filter_map(|p| p.pivot.as_mut())) {
            if piv.parameter.as_deref() == Some(old) {
                piv.parameter = Some(new.to_string());
            }
        }
        // Its nested instances' places and the host parameters their "=" names: a word parameter's by its whole name.
        for i in 0..self.nested_instances.len() {
            let words: Vec<String> = self.instance_words(&self.nested_instances[i]).into_iter().map(String::from).collect();
            let inst = &mut self.nested_instances[i];
            inst.at.iter_mut().chain([&mut inst.offset, &mut inst.angle]).chain(inst.visible.iter_mut()).for_each(&mut expr);
            for (k, e) in inst.params.iter_mut() {
                match e {
                    Expr::Formula(f) if words.contains(k) => {
                        if f == old {
                            *f = new.to_string();
                        }
                    }
                    e => expr(e),
                }
            }
        }
        for c in &mut self.connectors {
            [&mut c.voltage, &mut c.power_factor].into_iter().for_each(&mut expr);
            match &mut c.load_va {
                Load::One(e) => expr(e),
                Load::PerPhase(es) => es.iter_mut().for_each(&mut expr),
            }
        }
    }

    /// Whether `name` is one of its material parameters.
    pub fn material_parameter(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Material)
    }

    /// The materials it names: its material parameters' values, its types', and its forms' own.
    pub fn materials(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for p in self.parameters.iter().filter(|p| p.kind == Kind::Material) {
            out.extend(p.value.as_str());
            out.extend(self.types.values().filter_map(|t| t.get(&p.name)).filter_map(Value::as_str));
        }
        out.extend(self.forms.iter().filter(|f| !self.material_parameter(&f.material)).map(|f| f.material.as_str()));
        if let Some(d) = &self.adaptive {
            out.extend(
                d.nodes
                    .iter()
                    .filter(|n| n.is_form() && !self.material_parameter(&n.appearance.material))
                    .map(|n| n.appearance.material.as_str()),
            );
        }
        for (i, inst) in self.nested_instances.iter().enumerate() {
            let materials = self.instance_materials(i);
            for (k, e) in &inst.params {
                if let Expr::Formula(m) = e
                    && materials.contains(k)
                    && !self.material_parameter(m)
                {
                    out.push(m.as_str());
                }
            }
        }
        for n in &self.nested {
            out.extend(n.family.materials());
        }
        out.retain(|m| !m.is_empty());
        out
    }

    /// Material `from`, renamed `to` wherever it names it.
    pub fn rename_material(&mut self, from: &str, to: &str) {
        let params: Vec<String> = self.parameters.iter().filter(|p| p.kind == Kind::Material).map(|p| p.name.clone()).collect();
        let swap = |v: &mut Value| {
            if v.as_str() == Some(from) {
                *v = Value::String(to.into());
            }
        };
        self.parameters.iter_mut().filter(|p| p.kind == Kind::Material).for_each(|p| swap(&mut p.value));
        self.types.values_mut().flat_map(|t| t.iter_mut()).filter(|(k, _)| params.contains(k)).for_each(|(_, v)| swap(v));
        if let Some(d) = &mut self.adaptive {
            for node in d.nodes.iter_mut().filter(|n| n.appearance.material == from && !params.contains(&n.appearance.material)) {
                node.appearance.material = to.into();
            }
        }
        self.forms.iter_mut().filter(|f| f.material == from && !params.contains(&f.material)).for_each(|f| f.material = to.into());
        for i in 0..self.nested_instances.len() {
            let materials = self.instance_materials(i);
            for (k, e) in self.nested_instances[i].params.iter_mut() {
                if let Expr::Formula(m) = e
                    && materials.contains(k)
                    && m == from
                    && !params.contains(m)
                {
                    *m = to.into();
                }
            }
        }
        for n in &mut self.nested {
            n.family.rename_material(from, to);
        }
    }

    /// Its parameters' values for type `type_name`, a component's `instance` values over them, and
    /// its formulas worked out; a hosted family's in a wall of the usual thickness.
    pub fn values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Val>, String> {
        self.values_in(type_name, instance, DEFAULT_WALL)
    }

    /// [`Family::values`], with a hosted family's wall `wall` mm thick: what its formulas read as `Wall Thickness`.
    pub fn values_in(&self, type_name: &str, instance: &BTreeMap<String, Value>, wall: f64) -> Result<BTreeMap<String, Val>, String> {
        self.values_seeded(type_name, instance, wall, &BTreeMap::new())
    }

    /// [`Family::values_in`], with `seeds` taken as some of its parameters' values over its type's and its own: what a host's
    /// nested instance sets them to (Revit's "=" on a nested family's parameters).
    pub fn values_seeded(
        &self,
        type_name: &str,
        instance: &BTreeMap<String, Value>,
        wall: f64,
        seeds: &BTreeMap<String, Val>,
    ) -> Result<BTreeMap<String, Val>, String> {
        let set = self.types.get(type_name).ok_or_else(|| {
            let names: Vec<&str> = self.types.keys().map(String::as_str).collect();
            format!("type `{type_name}` doesn't exist (there are: {})", names.join(", "))
        })?;
        let mut done = BTreeMap::new();
        if self.category.hosted() {
            done.insert(WALL.to_string(), Val::Number(wall));
        }
        done.extend(seeds.iter().map(|(k, v)| (k.clone(), v.clone())));
        let seeded = done.clone();
        for p in &self.parameters {
            self.resolve(&p.name, set, instance, &mut done, &mut Vec::new())?;
        }
        // A reporting parameter a dimension measures is what it measures, and the formulas that read it are worked out again
        // over it; no label reads one (`check_dimensions`), so the dimensions come out the same.
        let reported = self.reported(&done);
        if !reported.is_empty() {
            done = seeded;
            done.extend(reported.into_iter().map(|(name, v)| (name, Val::Number(v))));
            for p in &self.parameters {
                self.resolve(&p.name, set, instance, &mut done, &mut Vec::new())?;
            }
        }
        Ok(done)
    }

    /// Parameter `name`'s value, and those it takes from, into `done`; `path`, the formulas being
    /// worked out, catches one that comes back to itself.
    fn resolve(
        &self,
        name: &str,
        set: &BTreeMap<String, Value>,
        instance: &BTreeMap<String, Value>,
        done: &mut BTreeMap<String, Val>,
        path: &mut Vec<String>,
    ) -> Result<Val, String> {
        if let Some(v) = done.get(name) {
            return Ok(v.clone());
        }
        let p = self.parameters.iter().find(|p| p.name == name).ok_or_else(|| not_a_parameter(name, &self.parameters))?;
        if path.iter().any(|n| n == name) {
            return Err(format!("`{name}`'s formula comes back to itself"));
        }
        let v = match &p.formula {
            Some(_) if path.len() >= MAX_CHAIN => {
                return Err(format!("`{name}` takes from formulas taking from more than {MAX_CHAIN} others in a row: nested too deep"));
            }
            Some(f) => {
                path.push(name.to_string());
                let x = eval(f, &mut |n: &str| match self.resolve(n, set, instance, done, path)? {
                    Val::Number(x) => Ok(x),
                    Val::Text(_) => {
                        let kind = self.parameters.iter().find(|q| q.name == n).map_or(Kind::Material, |q| q.kind);
                        Err(format!("`{n}` is {}, not a number", kind.said()))
                    }
                });
                path.pop();
                let x = x.map_err(|e| format!("parameter `{name}`: {e}"))?;
                // A whole number's formula rounds half away from zero, as Revit's does.
                Val::Number(if p.kind == Kind::Integer { x.round() } else { x })
            }
            None => read(p, if p.instance { instance.get(name) } else { set.get(name) }.unwrap_or(&p.value))?,
        };
        done.insert(name.to_string(), v.clone());
        Ok(v)
    }

    /// How IFC writes a component of it: [`Category::ifc`], with its kind's predefined type where it says one. A generic family
    /// that remembers the class it came in as writes that class, and its predefined type where IFC4 has it for the class (else
    /// none defined; nothing for a class with none); a proxy that names it, for a class IFC4 doesn't have.
    pub fn ifc(&self) -> (Cow<'static, str>, Cow<'static, str>, Option<Cow<'static, str>>) {
        if let Some(m) = self.remembered() {
            let object = m.object_type.clone().map(Cow::Owned);
            return match crate::ifc::classes::class(&m.class) {
                Some(c) if c.3.is_empty() => (c.0.into(), "".into(), object),
                Some(c) => {
                    let p = m.predefined.as_deref().filter(|p| crate::ifc::classes::takes(c, p)).unwrap_or("NOTDEFINED");
                    (c.0.into(), format!(".{p}.").into(), object)
                }
                None => ("IFCBUILDINGELEMENTPROXY".into(), ".USERDEFINED.".into(), Some(object.unwrap_or_else(|| m.class.clone().into()))),
            };
        }
        let (class, predefined, object) = self.category.ifc();
        let kind = self.kind.as_deref().and_then(|k| self.category.kinds().iter().find(|(n, _)| *n == k)).map(|(_, t)| *t);
        (class.into(), kind.unwrap_or(predefined).into(), object.map(Cow::Borrowed))
    }

    /// The IFC class it remembers, where it is a generic family: what its components go out as.
    pub fn remembered(&self) -> Option<&IfcClass> {
        self.ifc.as_ref().filter(|_| self.category == Category::Generic)
    }

    /// Whether it's sound: its parameters named once each, so formulas can say them, and holding
    /// what their kinds hold; its types setting only the type parameters no formula works out; its
    /// materials in the library (`has_material`); and every type building.
    pub fn check(&self, has_material: &dyn Fn(&str) -> bool) -> Result<(), String> {
        self.check_in(has_material, &mut Vec::new(), &mut 0)
    }

    /// [`Family::check`], within a tree of nested families: `chain` the names of the families it is nested in, `total` the
    /// nested definitions counted so far ([`nested`]).
    pub(crate) fn check_in(&self, has_material: &dyn Fn(&str) -> bool, chain: &mut Vec<String>, total: &mut usize) -> Result<(), String> {
        if let Some(definition) = &self.adaptive {
            definition.check()?;
        }
        for (what, n, most) in [
            ("parameters", self.parameters.len(), MAX_PARAMETERS),
            ("forms", self.forms.len(), MAX_FORMS),
            ("symbols", self.symbols.len(), MAX_SYMBOLS),
            ("connectors", self.connectors.len(), MAX_CONNECTORS),
            ("types", self.types.len(), MAX_TYPES),
        ] {
            if n > most {
                return Err(format!("it has more than {most} {what}"));
            }
        }
        if self.types.is_empty() {
            return Err("it needs a type: add one to `types`".into());
        }
        if let Some(kind) = &self.kind {
            let kinds = self.category.kinds();
            if !kinds.iter().any(|(k, _)| k == kind) {
                let shown: String = kind.chars().take(40).collect();
                let names: Vec<&str> = kinds.iter().map(|(k, _)| *k).collect();
                return Err(if names.is_empty() {
                    format!("`kind` `{shown}`: a family of this category has no kinds; leave `kind` out")
                } else {
                    format!("`kind` is one of {}, not `{shown}`", names.join(", "))
                });
            }
        }
        if let Some(m) = &self.ifc {
            m.check()?;
        }
        self.check_planes()?;
        self.check_dimensions()?;
        self.check_pivots()?;
        if self.category.hosted() {
            for name in [WIDTH, HEIGHT] {
                if !self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Length && !p.reporting) {
                    return Err(format!("a door or window family needs a length parameter `{name}`: the size of the opening it makes"));
                }
            }
            if self.parameters.iter().any(|p| p.name == WALL) {
                return Err(format!("`{WALL}` is the wall's, which a door or a window reads; name the parameter otherwise"));
            }
        }
        let material_ok = |who: &str, m: &str| {
            if m.is_empty() || has_material(m) {
                Ok(())
            } else {
                Err(format!("{who}: material `{m}` isn't in the library; add it with `material`"))
            }
        };
        let long = |f: &str| f.chars().count() > MAX_FORMULA;
        let mut names = BTreeSet::new();
        for p in &self.parameters {
            let who = format!("parameter `{}`", p.name);
            if !nameable(&p.name) {
                return Err(format!("{who}: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}"));
            }
            if !names.insert(p.name.as_str()) {
                return Err(format!("{who} is named twice"));
            }
            match (&p.formula, p.kind) {
                (Some(_), Kind::Material) => return Err(format!("{who}: a material takes no formula")),
                (Some(_), k) if k.text() => return Err(format!("{who}: {} takes no formula", k.said())),
                (Some(f), _) if long(f) => return Err(format!("{who}: its formula is longer than {MAX_FORMULA} characters")),
                _ => {}
            }
            p.check_own(&who)?;
            if let (Kind::Material, Val::Text(m)) = (p.kind, read(p, &p.value)?) {
                material_ok(&who, &m)?;
            }
        }
        // A shared parameter's GUID is one parameter: two of one family never hold it.
        for (i, p) in self.parameters.iter().enumerate() {
            if let Some(q) = p.shared.and_then(|g| self.parameters[..i].iter().find(|q| q.shared == Some(g))) {
                return Err(format!(
                    "parameters `{}` and `{}` are shared with one GUID; a shared parameter is one of them",
                    q.name, p.name
                ));
            }
        }
        for (t, set) in &self.types {
            let who = format!("type `{t}`");
            if t.trim().is_empty() || t.chars().count() > MAX_TEXT {
                return Err(format!("{who}: a type needs a name, up to {MAX_TEXT} characters"));
            }
            for (k, v) in set {
                let p = self.parameters.iter().find(|p| p.name == *k).ok_or_else(|| format!("{who}: `{k}` isn't one of its parameters"))?;
                if p.instance {
                    return Err(format!("{who}: `{k}` is an instance parameter; each component sets its own"));
                }
                if let Some(f) = p.driven() {
                    return Err(format!(
                        "{who}: `{k}` is worked out by its formula `{f}`; change the formula, or take it away to set `{k}`"
                    ));
                }
                if p.reporting {
                    return Err(format!("{who}: `{k}` is a reporting parameter: its dimension says what it is, and nothing sets it"));
                }
                if let (Kind::Material, Val::Text(m)) = (p.kind, read(p, v)?) {
                    material_ok(&who, &m)?;
                }
            }
        }
        for (i, form) in self.forms.iter().enumerate() {
            let who = format!("form {}", i + 1);
            let ways = usize::from(form.boundary().next().is_some()) + usize::from(!form.mesh.is_empty());
            if ways != 1 {
                return Err(format!("{who}: it takes a `sketch` (or its `profile` or `circle`) or a `mesh`, one of them"));
            }
            if form.revolves() && !form.mesh.is_empty() {
                return Err(format!("{who}: a `mesh` is as it is; an `axis` turns a `sketch`, a `profile` or a `circle`"));
            }
            if form.sketch.len() > sketch::MAX_SKETCH {
                return Err(format!("{who}: its sketch has {} lines, more than {}", form.sketch.len(), sketch::MAX_SKETCH));
            }
            forms::check_kind(form).map_err(|e| format!("{who}: {e}"))?;
            if form.mesh.is_empty() {
                self.frame(&form.host).map_err(|e| format!("{who}: its host: {e}"))?;
            }
            if !self.parameters.iter().any(|p| p.name == form.material && p.kind == Kind::Material) {
                material_ok(&who, &form.material)?;
            }
            let mut exprs = form.all_lines().flat_map(sketch::FormLine::exprs).chain([&form.from, &form.to]).chain(&form.visible);
            if exprs.any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
        }
        if let Some(definition) = &self.adaptive {
            for node in &definition.nodes {
                if node.is_form()
                    && !node.appearance.void
                    && !self.parameters.iter().any(|p| p.name == node.appearance.material && p.kind == Kind::Material)
                {
                    material_ok(&node.id, &node.appearance.material)?;
                }
            }
        }
        for (i, y) in self.symbols.iter().enumerate() {
            let Some(label) = &y.text else { continue };
            for name in fields(&label.value).map_err(|e| format!("symbol {}: {e}", i + 1))? {
                if !self.parameters.iter().any(|p| p.name == name) {
                    return Err(format!("symbol {}: its text `{{{name}}}`: {}", i + 1, not_a_parameter(name, &self.parameters)));
                }
            }
        }
        for (i, y) in self.symbols.iter().enumerate() {
            let Some(name) = &y.subcategory else { continue };
            if self.subcategory_key(name).is_none() {
                let parent = self.category.graphics_category();
                let has: Vec<&str> = crate::graphics::subcategories(parent).map(|s| s.name).collect();
                let shown: String = name.chars().take(40).collect();
                return Err(format!(
                    "symbol {}: its subcategory `{shown}` isn't one of its category's ({}); leave it out to draw it in the category",
                    i + 1,
                    if has.is_empty() { "it has none".to_string() } else { has.join(", ") }
                ));
            }
        }
        let symbol_exprs = self.symbols.iter().flat_map(|y| {
            let label = y.text.iter().flat_map(|l| l.at.iter().chain([&l.height]));
            let dot = y.dot.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
            y.line
                .iter()
                .flatten()
                .chain(&y.visible)
                .chain(y.arc.iter().flat_map(|c| c.centre.iter().chain([&c.radius, &c.from, &c.to])))
                .chain(label)
                .chain(dot)
        });
        if symbol_exprs.into_iter().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
            return Err(format!("a formula of a symbol is longer than {MAX_FORMULA} characters"));
        }
        for (i, y) in self.symbols.iter().enumerate() {
            if let Some(v) = y.visibility {
                v.check_symbol().map_err(|e| format!("symbol {}: {e}", i + 1))?;
            }
        }
        self.check_options()?;
        self.check_connectors()?;
        self.check_nested(has_material, chain, total)?;
        // Each formula read once across the types; past what a family may build, refused.
        let (mut read, mut built, mut cuts) = (HashMap::new(), 0, csg::Meter::default());
        let mut solving = constraints::Meter::default();
        let mut meter = nested::Meter::default();
        for t in self.types.keys() {
            let pieces = self
                .values(t, &BTreeMap::new())
                .and_then(|v| {
                    self.marks_metered(&v, None, &mut meter)?;
                    self.check_dimensions_for(&v)?;
                    self.check_pivot_values(&v)?;
                    // Its dimensions solved for the type: what they hold built where they put it.
                    let mut pieces = match self.flexed_with(&v, &mut solving).map_err(|e| e.to_string())? {
                        Cow::Borrowed(_) => {
                            let mut p = self.build(&v, &mut read, &mut cuts)?;
                            p.extend(self.nested_pieces(&v, &mut meter)?);
                            p
                        }
                        Cow::Owned(f) => {
                            let mut p = f.build(&v, &mut HashMap::new(), &mut cuts)?;
                            p.extend(f.nested_pieces(&v, &mut meter)?);
                            p
                        }
                    };
                    pieces.shrink_to_fit();
                    Ok(pieces)
                })
                .map_err(|e| format!("type `{t}`: {e}"))?;
            built += pieces.iter().map(|p| p.tris.len()).sum::<usize>();
            if built > MAX_BUILT {
                return Err(format!("its types build more than {MAX_BUILT} triangles together"));
            }
        }
        Ok(())
    }

    /// Whether its connectors are sound: at most one `power`; each kind with its system, its poles and its numbers
    /// in range, for every type.
    fn check_connectors(&self) -> Result<(), String> {
        if self.connectors.iter().filter(|c| c.kind == ConnectorKind::Power).count() > 1 {
            return Err("it has two power connectors: keep one, a device draws its load through one".into());
        }
        let long = |e: &Expr| matches!(e, Expr::Formula(f) if f.chars().count() > MAX_FORMULA);
        for (i, c) in self.connectors.iter().enumerate() {
            let who = format!("connector {}", i + 1);
            if c.classification.chars().count() > MAX_TEXT {
                return Err(format!("{who}: its classification is longer than {MAX_TEXT} characters"));
            }
            let loads: &[Expr] = match &c.load_va {
                Load::One(e) => std::slice::from_ref(e),
                Load::PerPhase(v) => v,
            };
            if [&c.voltage, &c.power_factor].into_iter().chain(loads).any(long) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
            match (c.kind, c.system) {
                (ConnectorKind::Power, ConnectorSystem::PowerBalanced | ConnectorSystem::PowerUnbalanced)
                | (ConnectorKind::Switch, ConnectorSystem::Switch) => {}
                (ConnectorKind::Power, _) => {
                    return Err(format!("{who}: a power connector's system is `power-balanced` or `power-unbalanced`, not `switch`"));
                }
                (ConnectorKind::Switch, _) => {
                    return Err(format!("{who}: a switch connector's system is `switch`"));
                }
            }
            if !(1..=3).contains(&c.poles) {
                return Err(format!("{who}: poles are 1, 2 or 3, not {}", c.poles));
            }
            if let Load::PerPhase(v) = &c.load_va
                && (c.system != ConnectorSystem::PowerUnbalanced || v.len() != usize::from(c.poles))
            {
                return Err(format!(
                    "{who}: a load per phase is a list of one for each pole ({}) on a `power-unbalanced` connector; a balanced one takes a single load",
                    c.poles
                ));
            }
        }
        for t in self.types.keys() {
            self.connector_values(t, &BTreeMap::new()).map_err(|e| format!("type `{t}`: {e}"))?;
        }
        Ok(())
    }

    /// Its connectors, worked out for type `type_name` and a component's `instance` values.
    pub fn connector_values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<Vec<ConnectorValues>, String> {
        if self.connectors.is_empty() {
            return Ok(Vec::new());
        }
        let values = self.values(type_name, instance)?;
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for (i, c) in self.connectors.iter().enumerate() {
            let who = format!("connector {}", i + 1);
            let said = |what: &str, e: String| format!("{who}: its {what}: {e}");
            let voltage = number(&c.voltage, &values, &mut read).map_err(|e| said("voltage", e))?;
            let power_factor = number(&c.power_factor, &values, &mut read).map_err(|e| said("power factor", e))?;
            let by_phase = match &c.load_va {
                Load::One(e) => {
                    let all = number(e, &values, &mut read).map_err(|e| said("load", e))?;
                    if c.kind == ConnectorKind::Switch {
                        if all != 0.0 {
                            return Err(format!("{who}: a switch carries no load; leave `load_va` out"));
                        }
                        Vec::new()
                    } else {
                        vec![all / f64::from(c.poles.max(1)); usize::from(c.poles)]
                    }
                }
                Load::PerPhase(v) => {
                    v.iter().map(|e| number(e, &values, &mut read).map_err(|e| said("load", e))).collect::<Result<_, _>>()?
                }
            };
            if c.kind == ConnectorKind::Power {
                if voltage <= 0.0 || voltage > MAX_VOLTAGE {
                    return Err(format!("{who}: its voltage is {voltage} V; it takes more than 0 and up to {MAX_VOLTAGE}"));
                }
                if power_factor <= 0.0 || power_factor > 1.0 {
                    return Err(format!("{who}: its power factor is {power_factor}; it takes more than 0 and up to 1"));
                }
                if let Some(x) = by_phase.iter().find(|x| !(0.0..=MAX_LOAD_VA).contains(*x)) {
                    return Err(format!("{who}: its load is {x} VA; it takes 0 up to {MAX_LOAD_VA}"));
                }
            }
            out.push(ConnectorValues {
                id: c.id,
                guid: c.guid,
                kind: c.kind,
                system: c.system,
                poles: c.poles,
                voltage,
                load_va: by_phase.iter().sum(),
                load_va_by_phase: by_phase,
                power_factor,
                classification: c.classification.clone(),
            });
        }
        Ok(out)
    }

    /// Its visible forms' solids, sized by `values`, less what its voids cut.
    pub fn pieces(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Piece>, String> {
        self.pieces_at(values, None)
    }

    pub fn pieces_at(&self, values: &BTreeMap<String, Val>, placement: Option<&crate::adaptive::Placement>) -> Result<Vec<Piece>, String> {
        self.pieces_metered(values, placement, &mut nested::Meter::default())
    }

    /// [`Family::pieces_at`], its nested instances' builds counted by `meter`, shared by the whole tree.
    pub(crate) fn pieces_metered(
        &self,
        values: &BTreeMap<String, Val>,
        placement: Option<&crate::adaptive::Placement>,
        meter: &mut nested::Meter,
    ) -> Result<Vec<Piece>, String> {
        let flexed = self.flexed(values).map_err(|e| e.to_string())?;
        let mut pieces = flexed.build(values, &mut HashMap::new(), &mut csg::Meter::default())?;
        pieces.extend(flexed.nested_pieces(values, meter)?);
        if let Some(definition) = &self.adaptive {
            pieces.extend(definition.evaluate(values, placement.unwrap_or(&definition.preview)).pieces());
        }
        Ok(pieces)
    }

    /// The marks of its visible symbols, sized by `values`, in its own axes: an arc as a polyline of short chords, a label
    /// a label as its anchor (lettered by [`lettering`] where it is placed), a dot as it is.
    pub fn marks(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Mark>, String> {
        Ok(self.marks_tagged(values)?.into_iter().map(|(m, _)| m).collect())
    }

    /// The subcategory key (`dooropening`) a symbol naming `name` is drawn in: the one of its category's subcategories that
    /// is called so, in any case; none for a name its category has none of.
    pub fn subcategory_key(&self, name: &str) -> Option<&'static str> {
        let parent = self.category.graphics_category();
        crate::graphics::subcategories(parent).find(|s| s.name.eq_ignore_ascii_case(name.trim())).map(|s| s.key)
    }

    /// [`Family::marks`], each with the subcategory key it is drawn in (none, its category).
    pub fn marks_tagged(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Tagged>, String> {
        self.marks_metered(values, None, &mut nested::Meter::default())
    }

    /// [`Family::marks_tagged`] that a view at detail level `detail` draws: each symbol's Visibility Settings, a nested
    /// instance's over its family's symbols, and with none Medium and Fine ([`visibility::SYMBOL_DEFAULT`]).
    pub fn marks_in(&self, values: &BTreeMap<String, Val>, detail: crate::graphics::Detail) -> Result<Vec<Tagged>, String> {
        self.marks_metered(values, Some(detail), &mut nested::Meter::default())
    }

    /// [`Family::marks_tagged`], at `detail` if it says (else every symbol), its nested instances' counted by `meter`.
    pub(crate) fn marks_metered<'a>(
        &'a self,
        values: &BTreeMap<String, Val>,
        detail: Option<crate::graphics::Detail>,
        meter: &mut nested::Meter,
    ) -> Result<Vec<Tagged>, String> {
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for (i, sym) in self.symbols.iter().enumerate() {
            let said = |e: String| format!("symbol {}: {e}", i + 1);
            let ways = usize::from(!sym.line.is_empty())
                + usize::from(sym.arc.is_some())
                + usize::from(sym.text.is_some())
                + usize::from(sym.dot.is_some());
            if ways != 1 || sym.line.len() > MAX_PROFILE {
                return Err(said(format!("it takes a `line` of up to {MAX_PROFILE} points, an `arc`, a `text` or a `dot`, one of them")));
            }
            if let Some(v) = &sym.visible
                && number(v, values, &mut read).map_err(said)? == 0.0
            {
                continue;
            }
            if let Some(d) = detail
                && !sym.visibility.unwrap_or(visibility::SYMBOL_DEFAULT).at(d)
            {
                continue;
            }
            let sub = sym.subcategory.as_deref().and_then(|n| self.subcategory_key(n));
            let mut n = |e: &'a Expr| number(e, values, &mut read).map_err(said);
            if let Some(c) = &sym.dot {
                let (centre, r) = (P2(n(&c.centre[0])?, n(&c.centre[1])?), n(&c.radius)?);
                if r <= 0.0 {
                    return Err(said(format!("its dot has radius {r}; it needs a radius above zero")));
                }
                out.push((Mark::Dot { centre, radius: r }, sub));
                continue;
            }
            if let Some(l) = &sym.text {
                let (at, height) = (P2(n(&l.at[0])?, n(&l.at[1])?), n(&l.height)?);
                if height <= 0.0 {
                    return Err(said(format!("its text has height {height}; it needs a height above zero")));
                }
                let text = said_text(&l.value, values, &self.parameters).map_err(said)?;
                out.push((Mark::Label { at, height, text, align: l.align }, sub));
                continue;
            }
            let mut pts = Vec::new();
            for [a, b] in &sym.line {
                pts.push(P2(n(a)?, n(b)?));
            }
            if let Some(c) = &sym.arc {
                let (centre, r, from, to) = (P2(n(&c.centre[0])?, n(&c.centre[1])?), n(&c.radius)?, n(&c.from)?, n(&c.to)?);
                if r <= 0.0 || (to - from).abs() > 360.0 || to == from {
                    return Err(said(format!(
                        "its arc has radius {r} and runs from {from} to {to} degrees; it needs a radius above zero and up to a full turn"
                    )));
                }
                let steps = ((to - from).abs() / ARC_STEP).ceil().max(1.0) as usize;
                pts.extend((0..=steps).map(|k| {
                    let (s, c) = (from + (to - from) * k as f64 / steps as f64).to_radians().sin_cos();
                    P2(centre.0 + r * c, centre.1 + r * s)
                }));
            }
            out.push((Mark::Line(pts), sub));
        }
        out.extend(self.nested_marks(values, detail, meter)?);
        Ok(out)
    }

    /// Its visible forms' solids, sized by `values`, less what its voids cut; `read`, its formulas as
    /// read already, and `cuts`, what the cutting may still cost.
    fn build<'a>(
        &'a self,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        cuts: &mut csg::Meter,
    ) -> Result<Vec<Piece>, String> {
        let (mut out, mut voids) = (Vec::new(), Vec::new());
        for i in 0..self.forms.len() {
            let what = format!("form {}", i + 1);
            let Some(piece) = self.solid(i, values, read).map_err(|e| format!("{what}: {e}"))? else {
                continue;
            };
            if self.forms[i].void {
                voids.push((i, out.len(), piece.tris));
            } else {
                out.push(piece);
            }
        }
        // Each void cut from the solids listed before it that it reaches. A cut solid is no longer one round surface,
        // and its outline seen from above is as it was. ponytail: a notch or a hole doesn't show in plan; trace it if it must.
        for (i, before, void) in &voids {
            for piece in out.iter_mut().take(*before) {
                let tris = csg::subtract(&piece.tris, void, cuts).map_err(|e| format!("form {}: {e}", i + 1))?;
                if tris != piece.tris {
                    let (bottom, top) = heights(&tris);
                    *piece = Piece {
                        caps: tris.len(),
                        tris,
                        bottom,
                        top,
                        round: false,
                        outline: std::mem::take(&mut piece.outline),
                        holes: std::mem::take(&mut piece.holes),
                        material: std::mem::take(&mut piece.material),
                        vis: piece.vis,
                    };
                }
            }
        }
        out.retain(|p| !p.tris.is_empty());
        Ok(out)
    }

    /// Form `i`'s own solid, sized by `values`, before any void cuts it: `None` where its `visible`
    /// says it isn't there. `read`, its formulas as read already.
    pub(crate) fn solid<'a>(
        &'a self,
        i: usize,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Option<Piece>, String> {
        let form = &self.forms[i];
        let mut num = |e: &'a Expr| number(e, values, read);
        if let Some(v) = &form.visible
            && num(v)? == 0.0
        {
            return Ok(None);
        }
        let material = match values.get(&form.material) {
            Some(Val::Text(m)) => m.clone(),
            _ => form.material.clone(),
        };
        let piece = if !form.mesh.is_empty() {
            mesh(&form.mesh, material)?
        } else if form.kind() != forms::FormKind::Extrusion && form.kind() != forms::FormKind::Revolve {
            let frame = self.frame(&form.host).map_err(|e| format!("its host: {e}"))?;
            let (tris, plan) = forms::build(form, &frame, &mut num)?.unwrap_or_default();
            let (bottom, top) = heights(&tris);
            // Seen from above: a sweep on a level plane, the band it covers; else the hull of what it builds.
            let (outline, holes) =
                plan.unwrap_or_else(|| (crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect()), Vec::new()));
            Piece { caps: tris.len(), tris, outline, holes, bottom, top, material, round: false, vis: None }
        } else {
            let regions = form.loops(&mut num)?;
            let (from, to) = (num(&form.from)?, num(&form.to)?);
            if to <= from {
                let unit = if form.revolves() { " degrees" } else { "" };
                return Err(format!("sweeps nothing: from {from} to {to}{unit}"));
            }
            let frame = self.frame(&form.host).map_err(|e| format!("its host: {e}"))?;
            match form.axis_ends() {
                Some([p, q]) => {
                    if to - from > 360.0 {
                        return Err(format!("turns more than all the way round: from {from} to {to} degrees, 360 at most"));
                    }
                    let axis = [P2(num(&p[0])?, num(&p[1])?), P2(num(&q[0])?, num(&q[1])?)];
                    let (tris, outline) = revolve(&regions, axis, (from, to), &frame)?;
                    let (bottom, top) = heights(&tris);
                    Piece { caps: tris.len(), tris, outline, holes: Vec::new(), bottom, top, material, round: false, vis: None }
                }
                None => {
                    let (tris, caps) = sweep(&regions, from, to, &frame)?;
                    let (bottom, top) = heights(&tris);
                    // Seen from above: on a level plane, the profile where it lies (one loop); else the hull of what it sweeps.
                    let outline = match regions.as_slice() {
                        [one] if frame.z[0].abs() < 1e-12 && frame.z[1].abs() < 1e-12 => {
                            let mut ring: Vec<P2> = one.outer.iter().map(|p| frame.point(*p, 0.0)).map(|q| P2(q[0], q[1])).collect();
                            if crate::geom::area(&ring) < 0.0 {
                                ring.reverse();
                            }
                            ring
                        }
                        _ => crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect()),
                    };
                    Piece { tris, caps, outline, holes: Vec::new(), bottom, top, material, round: form.circle().is_some(), vis: None }
                }
            }
        };
        Ok(Some(Piece { vis: form.visibility, ..piece }))
    }

    /// Where each of its forms lies, sized by `values`, before any void cuts it: its place in
    /// `forms`, whether it is a void, and the box its solid fills in the family's own axes. A form
    /// that builds to nothing on its own — a formula it can't read, a profile with no area — is left
    /// out: what a warning tells is best worked out around it, not stopped by it.
    pub fn shapes(&self, values: &BTreeMap<String, Val>) -> Vec<Shape> {
        let Ok(f) = self.flexed(values) else { return Vec::new() };
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for i in 0..f.forms.len() {
            let Some(piece) = f.solid(i, values, &mut read).ok().flatten() else {
                continue;
            };
            out.extend(Shape::of(i, f.forms[i].void, &piece.tris));
        }
        out
    }

    /// The voids a door's or a window's family cuts its wall with, sized by `values`, in its own axes: each void that reaches
    /// across into the wall (its `Wall Thickness` about y = 0) and cuts none of its forms, by their boxes (none listed before it that it reaches, as [`Shape::over`] says, the test a warning
    /// of a void that cuts nothing makes), so one listed before them all. They are the opening's shape in the wall, in place
    /// of the rectangle of its `Width` and `Height`; none, of another category, or of a family with none, and the
    /// rectangle stands. A void that cuts a form (a sash's light) cuts only that form, as before.
    pub fn host_voids(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Vec<[P3; 3]>>, String> {
        if !self.category.hosted() || !self.forms.iter().any(|f| f.void) {
            return Ok(Vec::new());
        }
        // Across the wall it stands in: one that doesn't reach the wall cuts nothing, and leaves the rectangle.
        let wall = match values.get(WALL) {
            Some(Val::Number(t)) => *t,
            _ => DEFAULT_WALL,
        };
        self.free_voids(values, |shape| shape.max[1] > -wall / 2.0 && shape.min[1] < wall / 2.0)
    }

    /// The voids a component of it placed on a face cuts its host with (Place on Face), sized by `values`, in its own axes:
    /// each void that cuts none of its forms, by their boxes, as [`Family::host_voids`] finds a door's. Whether one reaches
    /// its host is for where it stands to say: one that doesn't cuts nothing. Revit's face-based families always cut their
    /// host with such voids, whatever `cut_with_voids_when_loaded` says (that is the project's voids cutting it).
    pub fn face_voids(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Vec<[P3; 3]>>, String> {
        if self.adaptive.is_some() || !self.forms.iter().any(|f| f.void) {
            return Ok(Vec::new());
        }
        self.free_voids(values, |_| true)
    }

    /// The voids that cut none of the solid forms listed before them, by their boxes, and that `reach` takes.
    fn free_voids(&self, values: &BTreeMap<String, Val>, reach: impl Fn(&Shape) -> bool) -> Result<Vec<Vec<[P3; 3]>>, String> {
        let f = self.flexed(values).map_err(|e| e.to_string())?;
        let mut read = HashMap::new();
        let (mut solids, mut out): (Vec<Shape>, Vec<Vec<[P3; 3]>>) = (Vec::new(), Vec::new());
        for i in 0..f.forms.len() {
            let Some(piece) = f.solid(i, values, &mut read).map_err(|e| format!("form {}: {e}", i + 1))? else {
                continue;
            };
            let Some(shape) = Shape::of(i, f.forms[i].void, &piece.tris) else { continue };
            if !shape.void {
                solids.push(shape);
            } else if !solids.iter().any(|solid| shape.over(solid)) && reach(&shape) {
                out.push(piece.tris);
            }
        }
        Ok(out)
    }

    /// What's wrong with it, which a change may not go ahead with, and what's worth knowing, which
    /// doesn't stop it: every finding, not only the first, so they're fixed in one go.
    pub fn problems(&self, has_material: &dyn Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
        validate::problems(self, has_material)
    }

    /// Each name its formulas say that isn't one of its parameters, a line each: what a command changing it refuses
    /// ([`Family::problems`]) and what opening a file that holds it warns of.
    pub fn unknown_names(&self) -> Vec<String> {
        validate::unknown_said(self)
    }
}

/// `e` as a number: itself, or its formula worked out over `values`; `read`, the formulas as read already.
/// The names between braces in a label's `value`: `{Circuit}` reads the parameter `Circuit`.
fn fields(value: &str) -> Result<Vec<&str>, String> {
    let (mut out, mut rest) = (Vec::new(), value);
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            return Err("its text has a `{` with no `}` to close it".into());
        };
        out.push(rest[open + 1..open + close].trim());
        rest = &rest[open + close + 1..];
    }
    Ok(out)
}

/// A label's `value` with each `{Parameter}` replaced by that parameter's value: a number as plainly as it reads (3, 2.5), an
/// area in m² and a volume in m³, words (a material's name, text) as they are.
fn said_text(value: &str, values: &BTreeMap<String, Val>, parameters: &[Parameter]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    for name in fields(value)? {
        let Some(open) = rest.find('{') else { break };
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').unwrap_or(0) + open;
        rest = &rest[close + 1..];
        match values.get(name) {
            Some(Val::Number(x)) => {
                // An area or a volume in m² or m³, as schedules show them.
                let x = match parameters.iter().find(|p| p.name == name).map(|p| p.kind) {
                    Some(Kind::Area) => x / 1e6,
                    Some(Kind::Volume) => x / 1e9,
                    _ => *x,
                };
                let plain = format!("{x:.3}");
                out.push_str(plain.trim_end_matches('0').trim_end_matches('.'));
            }
            Some(Val::Text(t)) => out.push_str(t),
            None => return Err(format!("its text reads `{{{name}}}`, which isn't one of the family's parameters")),
        }
        if out.chars().count() > MAX_TEXT {
            break;
        }
    }
    out.push_str(rest);
    if out.trim().is_empty() {
        return Err("its text is empty; give it a `value` with something to letter".into());
    }
    if out.chars().count() > MAX_TEXT {
        return Err(format!("its text is longer than {MAX_TEXT} characters"));
    }
    Ok(out)
}

/// `text` in our lettering, capitals `height` tall, `at` the middle of its line vertically and, as `align` says, its start,
/// middle or end (y up), as polylines.
pub fn lettering(text: &str, at: P2, height: f64, align: Align) -> Vec<Vec<P2>> {
    let h = height as f32;
    let width = f64::from(font::width(text, h));
    let left = at.0
        - match align {
            Align::Left => 0.0,
            Align::Center => width / 2.0,
            Align::Right => width,
        };
    font::strokes(text, 0.0, 0.0, h)
        .into_iter()
        .map(|line| line.into_iter().map(|(x, y)| P2(left + f64::from(x), at.1 - height / 2.0 - f64::from(y))).collect())
        .collect()
}

/// What `e` is worth for `values`: a number, or a formula worked out.
pub(crate) fn worth(e: &Expr, values: &BTreeMap<String, Val>) -> Result<f64, String> {
    number(e, values, &mut HashMap::new())
}

fn number<'a>(e: &'a Expr, values: &BTreeMap<String, Val>, read: &mut HashMap<&'a str, Result<Node, String>>) -> Result<f64, String> {
    let x = match e {
        Expr::Number(x) => *x,
        Expr::Formula(f) => match read.entry(f.as_str()).or_insert_with(|| parse(f)) {
            Ok(node) => worked(f, node, &mut |n: &str| match values.get(n) {
                Some(Val::Number(x)) => Ok(*x),
                Some(Val::Text(_)) => Err(format!("`{n}` is words (a material, text, a URL), not a number")),
                None => Err(format!("`{n}` isn't one of its parameters")),
            })?,
            Err(e) => return Err(e.clone()),
        },
    };
    if x.is_finite() && x.abs() <= MAX_COORD { Ok(x) } else { Err(format!("{x} is not a usable number")) }
}

/// How low and how high `tris` reach.
fn heights(tris: &[[P3; 3]]) -> (f64, f64) {
    tris.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])))
}

/// The triangles that cap a profile `ring`, counter-clockwise; or why it can't be capped: it crosses itself.
fn cap(ring: &[P2]) -> Result<Vec<[usize; 3]>, String> {
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let caps = crate::geom::triangulate(ring);
    let covered: f64 = caps.iter().map(|[a, b, c]| (pt(*b) - pt(*a)).cross(pt(*c) - pt(*a)) / 2.0).sum();
    let area = crate::geom::area(ring);
    if (covered - area).abs() > 1e-6 * area + 1e-3 {
        return Err("its profile crosses itself".into());
    }
    Ok(caps)
}

/// A loop's cap: its corners, and its triangles as three of them each.
type Capped = (Vec<P2>, Vec<[usize; 3]>);

/// The triangles that cap each loop of a profile, counter-clockwise, with the corners they index: a loop without holes as
/// [`cap`] caps it (as a profile from before sketches was), one with holes by the constrained triangulation, round them.
fn caps(regions: &[crate::geom::Region]) -> Result<Vec<Capped>, String> {
    regions
        .iter()
        .map(|r| {
            if r.holes.is_empty() {
                Ok((r.outer.clone(), cap(&r.outer)?))
            } else {
                let t = crate::triangulation::constrained(&r.outer, &r.holes, &[], &[])
                    .map_err(|e| format!("its profile can't be capped: {e}"))?;
                Ok((t.points, t.triangles))
            }
        })
        .collect()
}

/// Every ring of `regions`: each outline (counter-clockwise), then its holes (clockwise).
fn rings(regions: &[crate::geom::Region]) -> impl Iterator<Item = &Vec<P2>> {
    regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
}

/// `regions`, each outline counter-clockwise and its holes clockwise in `frame`'s plane, swept along its normal from `from`
/// to `to`: their caps, then their sides, every triangle turned out; and how many the caps take.
fn sweep(regions: &[crate::geom::Region], from: f64, to: f64, frame: &Frame) -> Result<(Vec<[P3; 3]>, usize), String> {
    // A normal that points down the family's axis it lies most along (Center (Front/Back)'s, -y) is swept the axis' own way,
    // in the frame mirrored: the same solid, its triangles as they were when such a form named `xz`.
    let k = (1..3).fold(0, |k, i| if frame.z[i].abs() > frame.z[k].abs() { i } else { k });
    let (frame, from, to) = if frame.z[k] < 0.0 { (Frame { z: frame.z.map(|x| -x), ..*frame }, -to, -from) } else { (*frame, from, to) };
    let at = |p: P2, t: f64| frame.point(p, t);
    let capped = caps(regions)?;
    let mut tris = Vec::new();
    for (points, caps) in &capped {
        let pt = |i: usize| points.get(i).copied().unwrap_or_default();
        for &[a, b, c] in caps {
            tris.push([at(pt(a), to), at(pt(b), to), at(pt(c), to)]);
            tris.push([at(pt(a), from), at(pt(c), from), at(pt(b), from)]);
        }
    }
    let count = tris.len();
    for ring in rings(regions) {
        for (a, b) in crate::geom::ring_edges(ring) {
            let (a0, b0, a1, b1) = (at(a, from), at(b, from), at(a, to), at(b, to));
            tris.push([a0, b0, b1]);
            tris.push([a0, b1, a1]);
        }
    }
    // A mirrored frame (its normal not x × y) turns its triangles the other way to face out.
    if !frame.right_handed() {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok((tris, count))
}

/// `regions`, each outline counter-clockwise and its holes clockwise in `frame`'s plane, turned about the line through
/// `axis` (in that plane) from `from` to `to` degrees, right-handed from the axis' first point toward its second: its
/// triangles, every one turned out, a full turn's sides only and a part-turn's with its two ends capped; and its outline
/// seen from above.
fn revolve(
    regions: &[crate::geom::Region],
    [p0, p1]: [P2; 2],
    (from, to): (f64, f64),
    frame: &Frame,
) -> Result<(Vec<[P3; 3]>, Vec<P2>), String> {
    if p0.dist(p1) < 1e-6 {
        return Err("its axis starts and ends at one point: it needs a direction".into());
    }
    let capped = caps(regions)?;
    // Each corner's distance along the axis, and out from it to its left (none, on it).
    let (along, left) = ((p1 - p0).unit(), (p1 - p0).unit().perp());
    let reach_of = |c: P2| {
        let (t, r) = ((c - p0).dot(along), (c - p0).dot(left));
        (t, if r.abs() < 1e-6 { 0.0 } else { r })
    };
    let reach: Vec<(f64, f64)> = rings(regions).flatten().map(|c| reach_of(*c)).collect();
    let (least, most) = reach.iter().fold((0.0_f64, 0.0_f64), |(lo, hi), (_, r)| (lo.min(*r), hi.max(*r)));
    if least < 0.0 && most > 0.0 {
        return Err("its profile crosses its axis: draw it to one side of it".into());
    }
    if least == 0.0 && most == 0.0 {
        return Err("its profile lies along its axis: it needs some width off it".into());
    }
    // In 3D: along the axis (`u`), out from it (`n`), and the way a corner out to its left turns (`w`). One out to
    // the right turns the other way, which turns its triangles inside out.
    let (o, u, n) = (frame.point(p0, 0.0), frame.dir(along), frame.dir(left));
    let w = cross(u, n);
    let turn = to - from;
    let full = turn > 360.0 - 1e-9;
    let steps = (turn / 360.0 * SEGMENTS as f64 - 1e-9).ceil().max(1.0) as usize;
    let angle = |k: usize| (from + turn * (if full { k % steps } else { k }) as f64 / steps as f64).to_radians().sin_cos();
    let at = |c: P2, k: usize| -> P3 {
        let (t, r) = reach_of(c);
        let (s, c) = angle(k);
        add(add(o, mul(u, t)), mul(add(mul(n, c), mul(w, s)), r)).map(|x| (x * 1e9).round() / 1e9 + 0.0)
    };
    let mut tris = Vec::new();
    let mut push = |t: [P3; 3]| {
        // A corner on the axis stays put as it turns, which leaves a side with no area.
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            tris.push(if least < 0.0 { [t[0], t[2], t[1]] } else { t });
        }
    };
    if !full {
        for (points, caps) in &capped {
            let pt = |i: usize| points.get(i).copied().unwrap_or_default();
            for &[a, b, c] in caps {
                push([at(pt(a), steps), at(pt(b), steps), at(pt(c), steps)]);
                push([at(pt(a), 0), at(pt(c), 0), at(pt(b), 0)]);
            }
        }
    }
    for ring in rings(regions) {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            for k in 0..steps {
                let (a0, b0, a1, b1) = (at(a, k), at(b, k), at(a, k + 1), at(b, k + 1));
                push([a0, b0, b1]);
                push([a0, b1, a1]);
            }
        }
    }
    // Seen from above, a part-turn about an upright axis is a slice of a ring from the profile's widest to its
    // narrowest; any other, the hull of what it comes to. ponytail: the hull stands for the outline of the rest.
    let outline = if u[0].abs() < 1e-9 && u[1].abs() < 1e-9 && !full {
        let inner = reach.iter().map(|(_, r)| r.abs()).fold(f64::INFINITY, f64::min);
        // A profile to the axis' right is out along minus `n`.
        let side = if least < 0.0 { -1.0 } else { 1.0 };
        let arc = |r: f64, k: usize| {
            let (s, c) = angle(k);
            P2(o[0] + side * r * (n[0] * c + w[0] * s), o[1] + side * r * (n[1] * c + w[1] * s))
        };
        let mut pts: Vec<P2> = (0..=steps).map(|k| arc(most.max(-least), k)).collect();
        if inner > 0.0 {
            pts.extend((0..=steps).rev().map(|k| arc(inner, k)));
        } else {
            pts.push(P2(o[0], o[1]));
        }
        if crate::geom::area(&pts) < 0.0 {
            pts.reverse();
        }
        pts
    } else {
        crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect())
    };
    Ok((tris, outline))
}

/// A form a file brought as triangles: its outline seen from above, the hull round them.
fn mesh(tris: &[[P3; 3]], material: String) -> Result<Piece, String> {
    if tris.len() > MAX_TRIANGLES {
        return Err(format!("it has more than {MAX_TRIANGLES} triangles"));
    }
    if tris.iter().flatten().flatten().any(|x| !x.is_finite() || x.abs() > MAX_COORD) {
        return Err("a corner of it isn't a usable number".into());
    }
    let (bottom, top) = tris.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
    // ponytail: the convex hull stands for its outline; trace its silhouette if hulls mislead.
    let outline = crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect());
    Ok(Piece { tris: tris.to_vec(), caps: tris.len(), outline, holes: Vec::new(), bottom, top, material, round: false, vis: None })
}

/// The families a new project starts with, BIMCraft's own: tables, chairs, beds, sofas, desks,
/// wardrobes, base cabinets, WCs, washbasins and bathtubs, named in `language`, their formulas in
/// its words. A table or a chair stands on its centre, its back (a chair's) to its +y; the rest
/// stand with their backs on their x axis, their fronts toward -y, so they go against a wall.
pub fn library(language: crate::language::Language) -> BTreeMap<String, Family> {
    use serde_json::json;
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height, length) = (w("Width"), w(msg!("Depth")), w("Height"), w(msg!("Length")));
    let (thick, leg, seat, mat) = (w(msg!("Thickness")), w(msg!("Leg Width")), w(msg!("Seat Height")), w("Material"));
    let (timber, fabric, porcelain, concrete) = (w(msg!("Timber")), w(msg!("Fabric")), w(msg!("Porcelain")), w(msg!("Concrete")));
    // A box: x from `x0` to `x1`, y from `y0` to `y1`, up from `z0` to `z1`, of `m`.
    let block = |[x0, y0, x1, y1]: [Value; 4], z0: Value, z1: Value, m: &str| json!({"profile": [[x0, y0], [x1.clone(), y0], [x1, y1.clone()], [x0, y1]], "from": z0, "to": z1, "material": m});
    let f = |e: String| Value::String(e);
    let n = |x: f64| json!(x);
    let size = |name: &str, value: f64| json!({"name": name, "value": value});
    let material = |name: &str, value: &str| json!({"name": name, "kind": "material", "value": value});
    let (half_w, half_l) = (format!("{width} / 2"), format!("{length} / 2"));
    let neg = |e: &str| f(format!("-({e})"));
    let mut families = Vec::new();
    // Four legs at the corners of a top `w` by `d`, 50 in from its edges, up to `z`.
    let legs = |w: &str, d: &str, z: &str| -> Vec<Value> {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .map(|(east, north)| {
                let x = if east {
                    [f(format!("({w}) / 2 - 50 - {leg}")), f(format!("({w}) / 2 - 50"))]
                } else {
                    [f(format!("-({w}) / 2 + 50")), f(format!("-({w}) / 2 + 50 + {leg}"))]
                };
                let y = if north {
                    [f(format!("({d}) / 2 - 50 - {leg}")), f(format!("({d}) / 2 - 50"))]
                } else {
                    [f(format!("-({d}) / 2 + 50")), f(format!("-({d}) / 2 + 50 + {leg}"))]
                };
                let [x0, x1] = x;
                let [y0, y1] = y;
                block([x0, y0, x1, y1], n(0.0), f(z.to_string()), &mat)
            })
            .collect()
    };
    let top = format!("{height} - {thick}");
    let mut forms = vec![block(
        [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
        f(top.clone()),
        f(height.clone()),
        &mat,
    )];
    forms.extend(legs(&width, &depth, &top));
    families.push((
        w(msg!("Table")),
        json!({
            "parameters": [size(&width, 1600.0), size(&depth, 900.0), size(&height, 750.0), size(&thick, 40.0), size(&leg, 60.0), material(&mat, &timber)],
            "types": {"1600 x 900": {}, "1200 x 800": {&width: 1200, &depth: 800}, "2000 x 1000": {&width: 2000, &depth: 1000}},
            "forms": forms
        }),
    ));
    // A chair: its seat, its back over the seat's rear edge, and its legs.
    let under = format!("{seat} - {thick}");
    let mut forms = vec![
        block(
            [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
            f(under.clone()),
            f(seat.clone()),
            &mat,
        ),
        json!({"plane": "xz", "profile": [[neg(&half_w), seat], [half_w, seat], [half_w, height], [neg(&half_w), height]],
               "from": format!("{depth} / 2 - {thick}"), "to": format!("{depth} / 2"), "material": mat}),
    ];
    forms.extend(legs(&format!("{width} + 100"), &format!("{depth} + 100"), &under));
    families.push((
        w(msg!("Chair")),
        json!({
            "parameters": [size(&width, 450.0), size(&depth, 450.0), size(&seat, 450.0), size(&height, 850.0), size(&thick, 40.0), size(&leg, 40.0), material(&mat, &timber)],
            "types": {"450 x 450": {}},
            "forms": forms
        }),
    ));
    // A bed against a wall: its base, its mattress, its headboard, and a pillow, two on a double.
    let head = w(msg!("Headboard Height"));
    let pillow = |x0: String, x1: String, shown: &str| {
        let mut b = block([f(x0), n(-500.0), f(x1), n(-100.0)], f(height.clone()), f(format!("{height} + 120")), &fabric);
        b["visible"] = json!(shown);
        b
    };
    families.push((
        w(msg!("Bed")),
        json!({
            "parameters": [size(&width, 1600.0), size(&length, 2000.0), size(&height, 450.0), size(&head, 900.0), material(&mat, &timber)],
            "types": {"1600 x 2000": {}, "900 x 2000": {&width: 900}, "1800 x 2000": {&width: 1800}},
            "forms": [
                block([neg(&half_w), neg(&length), f(half_w.clone()), n(0.0)], n(0.0), n(300.0), &mat),
                block([f(format!("-({width}) / 2 + 20")), f(format!("-({length}) + 20")), f(format!("{width} / 2 - 20")), n(-60.0)], n(300.0), f(height.clone()), &fabric),
                {"plane": "xz", "profile": [[neg(&half_w), 0], [half_w, 0], [half_w, head], [neg(&half_w), head]], "from": -60, "to": 0, "material": mat},
                pillow(format!("-({width}) / 2 + 80"), "-40".into(), &format!("{width} > 1200")),
                pillow("40".into(), format!("{width} / 2 - 80"), &format!("{width} > 1200")),
                pillow(format!("-({width}) / 2 + 80"), format!("{width} / 2 - 80"), &format!("{width} <= 1200"))
            ]
        }),
    ));
    // A sofa: its seat between its arms, its back.
    let arm = w(msg!("Arm Width"));
    families.push((
        w(msg!("Sofa")),
        json!({
            "parameters": [size(&width, 2000.0), size(&depth, 900.0), size(&seat, 420.0), size(&height, 800.0), size(&arm, 180.0), material(&mat, &fabric)],
            "types": {"2000 x 900": {}, "1600 x 900": {&width: 1600}},
            "forms": [
                block([f(format!("-({width}) / 2 + {arm}")), neg(&depth), f(format!("{width} / 2 - {arm}")), n(-200.0)], n(0.0), f(seat.clone()), &mat),
                block([neg(&half_w), n(-200.0), f(half_w.clone()), n(0.0)], n(0.0), f(height.clone()), &mat),
                block([neg(&half_w), neg(&depth), f(format!("-({width}) / 2 + {arm}")), n(-200.0)], n(0.0), n(600.0), &mat),
                block([f(format!("{width} / 2 - {arm}")), neg(&depth), f(half_w.clone()), n(-200.0)], n(0.0), n(600.0), &mat)
            ]
        }),
    ));
    // A desk: its top on two side panels, a modesty panel between them.
    families.push((
        w(msg!("Desk")),
        json!({
            "parameters": [size(&width, 1400.0), size(&depth, 700.0), size(&height, 740.0), size(&thick, 30.0), material(&mat, &timber)],
            "types": {"1400 x 700": {}, "1200 x 600": {&width: 1200, &depth: 600}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(top.clone()), f(height.clone()), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(format!("-({width}) / 2 + 25")), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("{width} / 2 - 25")), f(format!("-({depth}) + 20")), f(half_w.clone()), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("-({width}) / 2 + 25")), n(-60.0), f(format!("{width} / 2 - 25")), n(-40.0)], n(300.0), f(top.clone()), &mat)
            ]
        }),
    ));
    // A wardrobe on its plinth.
    families.push((
        w(msg!("Wardrobe")),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1200.0), size(&depth, 600.0), size(&height, 2100.0), material(&mat, &timber)],
            "types": {"1200 x 600": {}, "900 x 600": {&width: 900}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], n(80.0), f(height.clone()), &mat),
                block([f(format!("-({width}) / 2 + 50")), f(format!("-({depth}) + 50")), f(format!("{width} / 2 - 50")), n(0.0)], n(0.0), n(80.0), &mat)
            ]
        }),
    ));
    // A base cabinet: its plinth, set back for toes; its carcass; its worktop over the front.
    let worktop = w(msg!("Worktop"));
    families.push((
        w(msg!("Base Cabinet")),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1800.0), size(&depth, 600.0), size(&height, 900.0), material(&mat, &timber), material(&worktop, &concrete)],
            "types": {"1800 x 600": {}, "1200 x 600": {&width: 1200}, "600 x 600": {&width: 600}},
            "forms": [
                block([neg(&half_w), f(format!("-({depth}) + 60")), f(half_w.clone()), n(0.0)], n(0.0), n(100.0), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(half_w.clone()), n(0.0)], n(100.0), f(format!("{height} - 40")), &mat),
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 40")), f(height.clone()), &worktop)
            ]
        }),
    ));
    // A WC: its cistern against the wall, its bowl.
    families.push((
        w(msg!("WC")),
        json!({
            "category": "plumbing",
            "kind": "wc",
            "types": {"400 x 650": {}},
            "forms": [
                block([n(-200.0), n(-180.0), n(200.0), n(0.0)], n(380.0), n(800.0), &porcelain),
                block([n(-160.0), n(-380.0), n(160.0), n(-150.0)], n(0.0), n(400.0), &porcelain),
                {"circle": {"centre": [0, -470], "radius": 180}, "from": 0, "to": 400, "material": porcelain}
            ]
        }),
    ));
    // A washbasin on its pedestal.
    families.push((
        w(msg!("Washbasin")),
        json!({
            "category": "plumbing",
            "kind": "lav",
            "parameters": [size(&width, 600.0), size(&depth, 450.0), size(&height, 850.0)],
            "types": {"600 x 450": {}, "500 x 400": {&width: 500, &depth: 400}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 160")), f(height.clone()), &porcelain),
                {"circle": {"centre": [0, -150], "radius": 90}, "from": 0, "to": format!("{height} - 160"), "material": porcelain}
            ]
        }),
    ));
    // A bathtub along a wall: its bottom, and its four sides round the tub.
    let rim = height.clone();
    let inner = format!("-({width}) + 60");
    families.push((
        w(msg!("Bathtub")),
        json!({
            "category": "plumbing",
            "kind": "bath",
            "parameters": [size(&length, 1700.0), size(&width, 750.0), size(&height, 560.0)],
            "types": {"1700 x 750": {}, "1500 x 700": {&length: 1500, &width: 700}},
            "forms": [
                block([neg(&half_l), neg(&width), f(half_l.clone()), n(0.0)], n(0.0), n(120.0), &porcelain),
                block([neg(&half_l), n(-60.0), f(half_l.clone()), n(0.0)], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), neg(&width), f(half_l.clone()), f(inner.clone())], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), f(inner.clone()), f(format!("-({length}) / 2 + 60")), n(-60.0)], n(120.0), f(rim.clone()), &porcelain),
                block([f(format!("{length} / 2 - 60")), f(inner), f(half_l), n(-60.0)], n(120.0), f(rim), &porcelain)
            ]
        }),
    ));
    families.extend(hosted_library(language));
    families.extend(crate::fixtures::library(language));
    families.into_iter().filter_map(|(name, v)| serde_json::from_value(v).ok().map(|f| (name, f))).collect()
}

/// The doors and windows a new project starts with, in `language`: a single door, a double door, a sliding door, a
/// fixed window, a casement window and a sliding window, each with several sizes and saying how it opens. Their forms
/// stand in the opening's axes (x along the wall from the middle of the opening, y across it, 0 in the middle of the
/// wall and + toward its exterior, z up from the sill), and they take the thickness of the wall they're in,
/// `Wall Thickness`, as theirs.
fn hosted_library(language: crate::language::Language) -> Vec<(String, Value)> {
    use serde_json::json;
    // The subcategory a door's swing and a window's opening lines are drawn in, as Revit's: its English name, as `Width` is.
    const OPENING: &str = "Opening";
    let w = |s: &str| language.word(s).to_string();
    let (frame, depth, leaf) = (w(msg!("Frame Width")), w(msg!("Frame Depth")), w(msg!("Leaf Thickness")));
    let (frame_mat, leaf_mat, glass, metal, timber) =
        (w(msg!("Frame Material")), w(msg!("Leaf Material")), w(msg!("Glass")), w(msg!("Metal")), w("Timber"));
    let (width, height, wall, sill) = (WIDTH, HEIGHT, WALL, SILL);
    let size = |name: &str, value: f64| json!({"name": name, "value": value});
    let material = |name: &str, value: &str| json!({"name": name, "kind": "material", "value": value});
    // A box from `x.0` to `x.1` along, `y.0` to `y.1` across and `z.0` up to `z.1`, each a formula, made of `m`.
    let block = |x: [&str; 2], y: [&str; 2], z: [&str; 2], m: &str| json!({"profile": [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]], "from": z[0], "to": z[1], "material": m});
    // What draws a box, as `block` does.
    type Block<'a> = &'a dyn Fn([&str; 2], [&str; 2], [&str; 2], &str) -> Value;
    // The frame of a door: two jambs and a head across the wall's thickness, in the opening's width and height.
    let (left, right) = (format!("-{width} / 2 + {frame}"), format!("{width} / 2 - {frame}"));
    let (half_w, minus_half_w) = (format!("{width} / 2"), format!("-{width} / 2"));
    let (half_t, minus_half_t) = (format!("{wall} / 2"), format!("-{wall} / 2"));
    let under_head = format!("{height} - {frame}");
    let door_frame = |bx: Block<'_>| {
        vec![
            bx([&minus_half_w, &left], [&minus_half_t, &half_t], ["0", &under_head], &frame_mat),
            bx([&right, &half_w], [&minus_half_t, &half_t], ["0", &under_head], &frame_mat),
            bx([&minus_half_w, &half_w], [&minus_half_t, &half_t], [&under_head, height], &frame_mat),
        ]
    };
    let leaf_y = [format!("-{leaf} / 2"), format!("{leaf} / 2")];
    let (face_in, face_out) = (format!("-{leaf} / 2 - 55"), format!("{leaf} / 2 + 55"));
    let handle = |x: [&str; 2], bx: Block<'_>| {
        vec![bx(x, [&leaf_y[1], &face_out], ["1000", "1030"], &metal), bx(x, [&face_in, &leaf_y[0]], ["1000", "1030"], &metal)]
    };
    let sizes = |names: &[(&str, f64, f64)]| -> Value {
        names.iter().map(|(n, wd, h)| (n.to_string(), json!({width: wd, height: h}))).collect::<serde_json::Map<String, Value>>().into()
    };
    let mut out = Vec::new();

    // A single door, hinged on the jamb toward the wall's start and swinging to its exterior.
    let run = format!("{width} - 2 * {frame}");
    let mut forms = door_frame(&block);
    forms.push(block([&left, &right], [&leaf_y[0], &leaf_y[1]], ["0", &under_head], &leaf_mat));
    forms.extend(handle([&format!("{width} / 2 - {frame} - 120"), &format!("{width} / 2 - {frame} - 40")], &block));
    out.push((
        w(msg!("Single Door")),
        json!({
            "category": "door",
            "operation": "swing",
            "parameters": [size(width, 900.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &timber), material(&leaf_mat, &timber)],
            "types": sizes(&[("900 x 2100", 900.0, 2100.0), ("800 x 2100", 800.0, 2100.0), ("1000 x 2100", 1000.0, 2100.0)]),
            "forms": forms,
            "symbols": [
                {"line": [[&left, &half_t], [&left, &format!("{wall} / 2 + {run}")]], "subcategory": OPENING},
                {"arc": {"centre": [&left, &half_t], "radius": &run, "from": 0, "to": 90}, "subcategory": OPENING}
            ]
        }),
    ));

    // A double door: two leaves, each hinged on its jamb, meeting in the middle.
    let each = format!("({width} - 2 * {frame}) / 2");
    let mut forms = door_frame(&block);
    forms.push(block([&left, "-1"], [&leaf_y[0], &leaf_y[1]], ["0", &under_head], &leaf_mat));
    forms.push(block(["1", &right], [&leaf_y[0], &leaf_y[1]], ["0", &under_head], &leaf_mat));
    forms.extend(handle(["-120", "-40"], &block));
    forms.extend(handle(["40", "120"], &block));
    out.push((
        w(msg!("Double Door")),
        json!({
            "category": "door",
            "operation": "double-swing",
            "parameters": [size(width, 1500.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &timber), material(&leaf_mat, &timber)],
            "types": sizes(&[("1500 x 2100", 1500.0, 2100.0), ("1200 x 2100", 1200.0, 2100.0), ("1800 x 2100", 1800.0, 2100.0)]),
            "forms": forms,
            "symbols": [
                {"line": [[&left, &half_t], [&left, &format!("{wall} / 2 + {each}")]], "subcategory": OPENING},
                {"arc": {"centre": [&left, &half_t], "radius": &each, "from": 0, "to": 90}, "subcategory": OPENING},
                {"line": [[&right, &half_t], [&right, &format!("{wall} / 2 + {each}")]], "subcategory": OPENING},
                {"arc": {"centre": [&right, &half_t], "radius": &each, "from": 90, "to": 180}, "subcategory": OPENING}
            ]
        }),
    ));

    // A sliding door (porta de correr): two leaves on two tracks in the frame, overlapping 50 in the middle; the one
    // toward the exterior slides past the other. The plan draws each leaf on its track.
    let (past_l, past_r) = ("25", "-25");
    let inner_track = [format!("-{leaf} - 5"), "-5".to_string()];
    let outer_track = ["5".to_string(), format!("{leaf} + 5")];
    let (inner_y, outer_y) = (format!("-{leaf} / 2 - 5"), format!("{leaf} / 2 + 5"));
    let mut forms = door_frame(&block);
    forms.push(block([&left, past_l], [&inner_track[0], &inner_track[1]], ["0", &under_head], &leaf_mat));
    forms.push(block([past_r, &right], [&outer_track[0], &outer_track[1]], ["0", &under_head], &leaf_mat));
    forms.push(block(["15", "55"], [&outer_track[1], &format!("{leaf} + 30")], ["1000", "1150"], &metal));
    out.push((
        w(msg!("Sliding Door")),
        json!({
            "category": "door",
            "operation": "sliding",
            "parameters": [size(width, 2000.0), size(height, 2100.0), size(&frame, 50.0), size(&leaf, 40.0), material(&frame_mat, &metal), material(&leaf_mat, &glass)],
            "types": sizes(&[("2000 x 2100", 2000.0, 2100.0), ("1500 x 2100", 1500.0, 2100.0), ("2400 x 2100", 2400.0, 2100.0)]),
            "forms": forms,
            "symbols": [
                {"line": [[&left, &inner_y], [past_l, &inner_y]], "subcategory": OPENING},
                {"line": [[past_r, &outer_y], [&right, &outer_y]], "subcategory": OPENING}
            ]
        }),
    ));

    // The frame of a window: its jambs, its head and its sill, as deep as it says or as the wall is, if that's less.
    let deep = format!("min({depth}, {wall})");
    let (half_d, minus_half_d) = (format!("{deep} / 2"), format!("-{deep} / 2"));
    let (inner_l, inner_r, inner_b, inner_t) = (left.clone(), right.clone(), frame.clone(), under_head.clone());
    let window_frame = |bx: Block<'_>| {
        vec![
            bx([&minus_half_w, &inner_l], [&minus_half_d, &half_d], ["0", height], &frame_mat),
            bx([&inner_r, &half_w], [&minus_half_d, &half_d], ["0", height], &frame_mat),
            bx([&inner_l, &inner_r], [&minus_half_d, &half_d], [&inner_t, height], &frame_mat),
            bx([&inner_l, &inner_r], [&minus_half_d, &half_d], ["0", &inner_b], &frame_mat),
        ]
    };
    let window_sizes = sizes(&[("1200 x 1500", 1200.0, 1500.0), ("600 x 900", 600.0, 900.0), ("1800 x 1200", 1800.0, 1200.0)]);
    let mut types = window_sizes.clone();
    if let Some(t) = types.as_object_mut() {
        for (name, at) in [("600 x 900", 1100.0), ("1800 x 1200", 1000.0)] {
            if let Some(Value::Object(set)) = t.get_mut(name) {
                set.insert(sill.into(), json!(at));
            }
        }
    }
    let pane = |x: [&str; 2], z: [&str; 2]| block(x, ["-3", "3"], z, &glass);

    // A fixed window: its frame round one pane.
    let mut forms = window_frame(&block);
    forms.push(pane([&inner_l, &inner_r], [&inner_b, &inner_t]));
    out.push((
        w(msg!("Fixed Window")),
        json!({
            "category": "window",
            "operation": "fixed",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &timber)],
            "types": types.clone(),
            "forms": forms
        }),
    ));

    // A casement window: two sashes meeting in the middle, each a box with a void through it, its bars 40 wide
    // (20 where the sashes meet), and its pane after the void, so the void leaves it whole.
    let mut forms = window_frame(&block);
    let sash = |outer: [&str; 2], bars: [i32; 2]| -> Vec<Value> {
        let inside = [format!("({}) + {}", outer[0], bars[0]), format!("({}) - {}", outer[1], bars[1])];
        let (low, high) = (format!("{inner_b} + 40"), format!("{inner_t} - 40"));
        let mut hole = block([&inside[0], &inside[1]], ["-30", "30"], [&low, &high], "");
        hole["void"] = json!(true);
        vec![block(outer, ["-20", "20"], [&inner_b, &inner_t], &frame_mat), hole, pane([&inside[0], &inside[1]], [&low, &high])]
    };
    forms.extend(sash([&inner_l, "-1"], [40, 20]));
    forms.extend(sash(["1", &inner_r], [20, 40]));
    out.push((
        w(msg!("Casement Window")),
        json!({
            "category": "window",
            "operation": "double-swing",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &timber)],
            "types": types.clone(),
            "forms": forms
        }),
    ));

    // A sliding window (janela de correr): two sashes on two tracks, each a frame of bars 40 wide round its pane,
    // overlapping 40 in the middle; the one toward the exterior slides past the other.
    let mut forms = window_frame(&block);
    let (low, high) = (format!("{inner_b} + 40"), format!("{inner_t} - 40"));
    let sliding_sash = |x: [&str; 2], y: [&str; 2]| -> Vec<Value> {
        let (stile_l, stile_r) = (format!("({}) + 40", x[0]), format!("({}) - 40", x[1]));
        let middle = format!("(({}) + ({})) / 2", y[0], y[1]);
        let pane_y = [format!("{middle} - 3"), format!("{middle} + 3")];
        vec![
            block([x[0], &stile_l], y, [&inner_b, &inner_t], &frame_mat),
            block([&stile_r, x[1]], y, [&inner_b, &inner_t], &frame_mat),
            block([&stile_l, &stile_r], y, [&inner_b, &low], &frame_mat),
            block([&stile_l, &stile_r], y, [&high, &inner_t], &frame_mat),
            block([&stile_l, &stile_r], [&pane_y[0], &pane_y[1]], [&low, &high], &glass),
        ]
    };
    let (track_in, track_out) = ([format!("-{deep} / 2 + 5"), "-2".to_string()], ["2".to_string(), format!("{deep} / 2 - 5")]);
    forms.extend(sliding_sash([&inner_l, "20"], [&track_in[0], &track_in[1]]));
    forms.extend(sliding_sash(["-20", &inner_r], [&track_out[0], &track_out[1]]));
    let (in_y, out_y) = (format!("-{deep} / 4"), format!("{deep} / 4"));
    out.push((
        w(msg!("Sliding Window")),
        json!({
            "category": "window",
            "operation": "sliding",
            "parameters": [size(width, 1200.0), size(height, 1500.0), size(sill, 900.0), size(&frame, 50.0), size(&depth, 90.0), material(&frame_mat, &metal)],
            "types": types,
            "forms": forms,
            "symbols": [
                {"line": [[&inner_l, &in_y], ["20", &in_y]], "subcategory": OPENING},
                {"line": [["-20", &out_y], [&inner_r, &out_y]], "subcategory": OPENING}
            ]
        }),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with<'a>(values: &'a [(&'a str, f64)]) -> impl FnMut(&str) -> Result<f64, String> + 'a {
        move |n: &str| values.iter().find(|(k, _)| *k == n).map(|(_, v)| *v).ok_or_else(|| format!("no `{n}`"))
    }

    #[test]
    fn formulas_read_as_revits_do() {
        let values = [("Width", 1000.0), ("Leg Height", 700.0)];
        for (formula, want) in [
            ("Width / 2 + 10", 510.0),
            ("2 + 3 * 4", 14.0),
            ("(2 + 3) * 4", 20.0),
            ("-Width + 1", -999.0),
            ("2 ^ 3 ^ 2", 512.0),
            ("Leg Height - 2*3", 694.0),
            ("if(Width > 1000, 4, 2)", 2.0),
            ("if(Width >= 1000, 4, 2)", 4.0),
            ("if (Width = 1000, 1, 0)", 1.0),
            ("not(Width < 1000)", 1.0),
            ("and(1, 0) + or(1, 0)", 1.0),
            ("sqrt(16) + abs(-2) + min(3, 5) + max(3, 5)", 14.0),
            ("round(2.5) + roundup(2.1) + rounddown(2.9)", 8.0),
            ("sin(30) + cos(60)", 1.0),
            ("pi() * 0", 0.0),
            // The branch not taken isn't worked out.
            ("if(Width > 0, 1, 1 / 0)", 1.0),
        ] {
            let got = eval(formula, &mut with(&values)).unwrap();
            assert!((got - want).abs() < 1e-9, "{formula}: {got}, not {want}");
        }
        for (formula, says) in [
            ("Width +", "`Width +`"),
            ("Nope + 1", "Nope"),
            ("1 / 0", "isn't a number"),
            ("sqrt(-1)", "isn't a number"),
            ("if(1, 2)", "if"),
            ("frobnicate(1)", "frobnicate"),
            ("2 3", "`2 3`"),
            ("", "empty"),
        ] {
            let e = eval(formula, &mut with(&values)).unwrap_err();
            assert!(e.contains(says), "{formula}: {e}");
        }
        // Nested past reason: refused, not a stack overflow.
        let deep = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert!(eval(&deep, &mut with(&values)).unwrap_err().contains("deep"));
    }

    fn table() -> Family {
        serde_json::from_value(json!({
            "parameters": [
                {"name": "Width", "value": 1000},
                {"name": "Depth", "value": 600},
                {"name": "Height", "instance": true, "value": 750},
                {"name": "Top", "formula": "Height - 30"},
                {"name": "Area", "kind": "number", "formula": "Width * Depth / 1e6"},
                {"name": "Legs", "kind": "yesno", "value": true},
                {"name": "Finish", "kind": "material", "value": "Timber"}
            ],
            "types": {"1000 x 600": {}, "Small": {"Width": 800}},
            "forms": [
                {"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "from": "Top", "to": "Height", "material": "Finish"},
                {"profile": [[0, 0], [50, 0], [50, 50], [0, 50]], "from": 0, "to": "Top", "visible": "Legs"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn types_and_components_set_parameters_and_formulas_follow() {
        let f = table();
        let v = f.values("Small", &BTreeMap::from([("Height".to_string(), json!(700))])).unwrap();
        let n = |k: &str| match v.get(k) {
            Some(Val::Number(x)) => *x,
            other => panic!("{k}: {other:?}"),
        };
        assert_eq!([n("Width"), n("Depth"), n("Height"), n("Top"), n("Area"), n("Legs")], [800.0, 600.0, 700.0, 670.0, 0.48, 1.0]);
        assert_eq!(v.get("Finish"), Some(&Val::Text("Timber".into())));
        let mut cyclic = f.clone();
        cyclic.parameters[0].formula = Some("Depth + Top".into());
        cyclic.parameters[1].formula = Some("Width - 1".into());
        let e = cyclic.values("Small", &BTreeMap::new()).unwrap_err();
        assert!(e.contains("Width") && e.contains("itself"), "{e}");
        assert!(f.values("Huge", &BTreeMap::new()).unwrap_err().contains("Huge"));
    }

    #[test]
    fn long_formulas_and_long_chains_of_them_never_exhaust_the_stack() {
        // A flat sum of many terms reads; parameters each taking the next, deeper than any family
        // needs, are refused, and neither runs a small stack dry.
        let worked = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let flat = format!("1{}", " + 1".repeat(600));
                let sum = eval(&flat, &mut |_| Ok(0.0));
                let terms = format!("P{{}}{}", " + 1".repeat(500));
                let parameters: Vec<Value> = (0..64)
                    .map(|i| {
                        if i == 63 {
                            json!({"name": "P63", "value": 1})
                        } else {
                            json!({"name": format!("P{i}"), "formula": terms.replace("{}", &(i + 1).to_string())})
                        }
                    })
                    .collect();
                let f: Family = serde_json::from_value(json!({"parameters": parameters, "types": {"A": {}}})).unwrap();
                (sum, f.values("A", &BTreeMap::new()))
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(worked.0, Ok(601.0));
        assert!(worked.1.unwrap_err().contains("deep"));
    }

    #[test]
    fn a_family_too_big_to_build_is_refused() {
        let ring = |n: usize| -> Vec<Value> {
            (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| json!([100.0 * c, 100.0 * s])).collect()
        };
        let f: Family =
            serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"profile": ring(sketch::MAX_SKETCH + 1), "to": 10}]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("more than"));
        // Many types, each building a little: past what one family may build, refused.
        let types: serde_json::Map<String, Value> = (0..MAX_TYPES).map(|k| (format!("T{k}"), json!({}))).collect();
        let form = json!({"profile": ring(MAX_PROFILE), "to": 10});
        let f: Family = serde_json::from_value(json!({"types": types, "forms": [form, form]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("triangles"));
    }

    /// A closed surface's volume: the triangles' signed tetrahedra from the origin, positive while
    /// they face out.
    fn volume(tris: &[[P3; 3]]) -> f64 {
        tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0).sum::<f64>()
    }

    #[test]
    fn forms_sweep_their_profiles_across_their_planes() {
        let f = table();
        let v = f.values("1000 x 600", &BTreeMap::new()).unwrap();
        let pieces = f.pieces(&v).unwrap();
        assert_eq!(pieces.len(), 2);
        let top = &pieces[0];
        assert_eq!((top.bottom, top.top, top.material.as_str(), top.round), (720.0, 750.0, "Timber", false));
        assert!((volume(&top.tris) - 1000.0 * 600.0 * 30.0).abs() < 1e-3, "{}", volume(&top.tris));
        assert_eq!(top.outline, [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 600.0), P2(0.0, 600.0)]);
        // Hidden: no leg.
        let mut no_legs = v.clone();
        no_legs.insert("Legs".into(), Val::Number(0.0));
        assert_eq!(f.pieces(&no_legs).unwrap().len(), 1);
        // Upright profiles: a back across x and z, swept along y; a rail across y and z, along x;
        // a round post, its sides one surface.
        let f: Family = serde_json::from_value(json!({
            "types": {"A": {}},
            "forms": [
                {"plane": "xz", "profile": [[0, 400], [500, 400], [500, 900], [0, 900]], "from": 0, "to": 40},
                {"plane": "yz", "profile": [[0, 0], [300, 0], [300, 100]], "from": 10, "to": 210},
                {"circle": {"centre": [100, 100], "radius": 20}, "from": 0, "to": 1000}
            ]
        }))
        .unwrap();
        let pieces = f.pieces(&f.values("A", &BTreeMap::new()).unwrap()).unwrap();
        let back = &pieces[0];
        assert!((volume(&back.tris) - 500.0 * 500.0 * 40.0).abs() < 1e-3);
        assert_eq!((back.bottom, back.top), (400.0, 900.0));
        assert_eq!(back.outline, [P2(0.0, 0.0), P2(500.0, 0.0), P2(500.0, 40.0), P2(0.0, 40.0)]);
        let rail = &pieces[1];
        assert!((volume(&rail.tris) - 300.0 * 100.0 / 2.0 * 200.0).abs() < 1e-3);
        assert_eq!(rail.outline, [P2(10.0, 0.0), P2(210.0, 0.0), P2(210.0, 300.0), P2(10.0, 300.0)]);
        let post = &pieces[2];
        let ring = crate::geom::area(&post.outline);
        assert!(
            post.round && (volume(&post.tris) - ring * 1000.0).abs() < 1e-3 && (ring / (std::f64::consts::PI * 400.0) - 1.0).abs() < 0.01
        );
        // Nothing to sweep, or a profile with no area: said.
        let mut flat = table();
        flat.forms[0].to = Expr::Formula("Top".into());
        assert!(flat.pieces(&flat.values("Small", &BTreeMap::new()).unwrap()).unwrap_err().contains("form 1"));
    }

    /// The pieces of a family of `forms`, its one type built.
    fn built(forms: Value) -> Result<Vec<Piece>, String> {
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "parameters": [{"name": "R", "value": 300}], "forms": forms}))
            .map_err(|e| e.to_string())?;
        f.check(&|_| true)?;
        f.pieces(&f.values("A", &BTreeMap::new())?)
    }

    #[test]
    fn a_revolve_turns_its_profile_about_its_axis() {
        // A tube: a rectangle across x and z, 100 to `R` out from the z axis, turned all the way round it.
        let ring = json!([[100, 0], ["R", 0], ["R", 500], [100, 500]]);
        let tube = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 360}])).unwrap();
        let [p] = tube.as_slice() else { panic!("{tube:?}") };
        // Its sides are polygons of 32 sides: their area is a hair under the circle's.
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin() / std::f64::consts::PI;
        let want = std::f64::consts::PI * (300.0 * 300.0 - 100.0 * 100.0) * 500.0 * k;
        assert!((volume(&p.tris) / want - 1.0).abs() < 1e-6, "{} not {want}", volume(&p.tris));
        assert_eq!((p.bottom, p.top), (0.0, 500.0));
        assert!(
            (crate::geom::area(&p.outline) / (std::f64::consts::PI * 300.0 * 300.0 * k) - 1.0).abs() < 1e-6,
            "seen from above, its rim"
        );
        // A quarter of it, closed at both ends; its axis given the other way round, or across x and y.
        let quarter = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 1], [0, 0]], "from": 0, "to": 90}])).unwrap();
        assert!((volume(&quarter[0].tris) / (want / 4.0) - 1.0).abs() < 1e-6, "{}", volume(&quarter[0].tris));
        let half = built(json!([{"profile": [[100, 0], [300, 0], [300, 50], [100, 50]], "axis": [[0, 0], [0, 1]], "to": 180}])).unwrap();
        let ring_of = std::f64::consts::PI * (300.0 * 300.0 - 100.0 * 100.0) * 50.0 * k;
        assert!((volume(&half[0].tris) / (ring_of / 2.0) - 1.0).abs() < 1e-6, "{}", volume(&half[0].tris));
        assert!((half[0].top - half[0].bottom - 300.0).abs() < 1e-6, "turned out of its plane, 300 to one side");
        // A profile across its axis, an axis of one point, a turn of nothing or more than all the way: said.
        for (form, says) in [
            (json!({"plane": "xz", "profile": [[-100, 0], [300, 0], [300, 500]], "axis": [[0, 0], [0, 1]], "to": 360}), "crosses its axis"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 0]], "to": 360}), "axis"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "from": 90, "to": 90}), "nothing"),
            (json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 400}), "360"),
        ] {
            let e = built(json!([form])).unwrap_err();
            assert!(e.contains(says) && e.contains("form 1"), "{e}");
        }
    }

    #[test]
    fn a_revolve_faces_out_whatever_plane_side_and_direction_it_turns() {
        // The same tube, 100 to 300 out and 500 along: drawn across each plane, to either side of its axis, the axis given
        // either way round, from any angle: one volume, turned out.
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        let want = k * (300.0 * 300.0 - 100.0 * 100.0) * 500.0;
        let across = |a0: f64, a1: f64| json!([[a0, 0], [a1, 0], [a1, 500], [a0, 500]]);
        for (plane, profile, axis) in [
            ("xz", across(100.0, 300.0), json!([[0, 0], [0, 1]])),
            ("xz", across(100.0, 300.0), json!([[0, 1], [0, 0]])),
            ("xz", across(-300.0, -100.0), json!([[0, 0], [0, 1]])),
            ("xz", across(-300.0, -100.0), json!([[0, 1], [0, 0]])),
            ("yz", across(100.0, 300.0), json!([[0, 0], [0, 1]])),
            ("yz", across(-300.0, -100.0), json!([[0, 1], [0, 0]])),
            ("xy", json!([[0, 100], [500, 100], [500, 300], [0, 300]]), json!([[0, 0], [1, 0]])),
            ("xy", json!([[0, -300], [500, -300], [500, -100], [0, -100]]), json!([[1, 0], [0, 0]])),
            ("xy", json!([[0, 100], [500, 100], [500, 300], [0, 300]]), json!([[0, 0], [1, 0]])),
        ] {
            for (from, to) in [(0, 360), (-90, 270), (30, 390)] {
                let tube = built(json!([{"plane": plane, "profile": profile, "axis": axis, "from": from, "to": to}]));
                let tube = tube.unwrap_or_else(|e| panic!("{plane} {profile} {axis}: {e}"));
                let v = volume(&tube[0].tris);
                assert!((v / want - 1.0).abs() < 1e-6, "{plane} {profile} {axis} {from} to {to}: {v} not {want}");
            }
        }
        // A part turn is closed by two caps, turned out. About an axis beside the origin, with neither cap in a plane through it
        // (a cap in one adds nothing to the volume, however it faces), the volume is the sector's only if both are.
        let sector = k / 4.0 * (300.0 * 300.0 - 100.0 * 100.0) * 500.0;
        for (from, to) in [(30, 120), (-120, -30)] {
            for axis in [json!([[1000, 0], [1000, 1]]), json!([[1000, 1], [1000, 0]])] {
                let part =
                    built(json!([{"plane": "xz", "profile": across(1100.0, 1300.0), "axis": axis, "from": from, "to": to}])).unwrap();
                let v = volume(&part[0].tris);
                assert!((v / sector - 1.0).abs() < 1e-6, "{axis} {from} to {to}: {v} not {sector}");
            }
        }
    }

    #[test]
    fn a_part_turn_about_an_upright_axis_shows_a_slice_in_plan_and_a_circle_makes_a_torus() {
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        let ring = json!([[100, 0], [300, 0], [300, 500], [100, 500]]);
        let quarter = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 90}])).unwrap();
        let outline = &quarter[0].outline;
        // 4 sin δ (R² − r²) where δ is a step's angle: a quarter of the ring, as the polygon makes it, counter-clockwise, from +x.
        assert!((crate::geom::area(outline) / (k / 4.0 * (300.0 * 300.0 - 100.0 * 100.0)) - 1.0).abs() < 1e-6, "{outline:?}");
        assert!(outline.iter().all(|p| p.0 > -1e-6 && p.1 > -1e-6), "in the first quarter: {outline:?}");
        // The same turned the other way round the axis, the other way round the plan.
        let down = built(json!([{"plane": "xz", "profile": ring, "axis": [[0, 1], [0, 0]], "to": 90}])).unwrap();
        assert!(down[0].outline.iter().all(|p| p.0 > -1e-6 && p.1 < 1e-6), "{:?}", down[0].outline);
        // A profile on the axis: a slice of a disc, its centre one of its corners.
        let wedge =
            built(json!([{"plane": "xz", "profile": [[0, 0], [300, 0], [300, 500], [0, 500]], "axis": [[0, 0], [0, 1]], "to": 90}]));
        assert!((crate::geom::area(&wedge.unwrap()[0].outline) / (k / 4.0 * 300.0 * 300.0) - 1.0).abs() < 1e-6);
        // A circle turned about an axis beside it: a torus. Its volume is its circle's area times the way its centre goes.
        let donut = built(json!([{"plane": "xz", "circle": {"centre": [500, 100], "radius": 100}, "axis": [[0, 0], [0, 1]], "to": 360}]));
        let donut = donut.unwrap();
        let (v, exact) = (volume(&donut[0].tris), 2.0 * std::f64::consts::PI.powi(2) * 500.0 * 100.0 * 100.0);
        assert!((v / exact - 1.0).abs() < 0.03, "{v} not about {exact}");
        assert_eq!((donut[0].bottom, donut[0].top), (0.0, 200.0), "its circle's lowest and highest");
        // A mesh has no axis to turn about, and a form of more than one kind is still refused.
        let f: Family = serde_json::from_value(
            json!({"types": {"A": {}}, "forms": [{"mesh": [[[0, 0, 0], [1, 0, 0], [0, 1, 0]]], "axis": [[0, 0], [0, 1]]}]}),
        )
        .unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("axis"));
    }

    #[test]
    fn a_void_cuts_the_forms_it_passes_through_and_shows_nothing_itself() {
        let slab = json!({"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "from": 0, "to": 100});
        let hole = |shown: Value| json!({"profile": [[400, 400], [600, 400], [600, 600], [400, 600]], "from": -10, "to": 110, "void": true, "visible": shown});
        let cut = built(json!([slab, hole(json!(1))])).unwrap();
        let [p] = cut.as_slice() else { panic!("{cut:?}") };
        assert!((volume(&p.tris) - (1000.0 * 1000.0 - 200.0 * 200.0) * 100.0).abs() < 1e-3, "{}", volume(&p.tris));
        // Hidden, it cuts nothing; away from the slab, nothing is cut.
        assert!((volume(&built(json!([slab, hole(json!(0))])).unwrap()[0].tris) - 1e8).abs() < 1e-3);
        let away = json!({"profile": [[2000, 0], [2100, 0], [2100, 100], [2000, 100]], "to": 100, "void": true});
        assert!((volume(&built(json!([slab, away])).unwrap()[0].tris) - 1e8).abs() < 1e-3);
        // A void cuts what's listed before it: a form after it is whole, as a pane in the hole of a sash.
        let pane = json!({"profile": [[450, 450], [550, 450], [550, 550], [450, 550]], "from": 0, "to": 100});
        let both = built(json!([slab, hole(json!(1)), pane])).unwrap();
        assert_eq!(both.len(), 2);
        assert!((volume(&both[0].tris) - 9.6e7).abs() < 1e-3 && (volume(&both[1].tris) - 100.0 * 100.0 * 100.0).abs() < 1e-3);
        // A round bowl out of a block, as a basin's: what's left is the block less the bowl.
        let block = json!({"profile": [[-300, -300], [300, -300], [300, 300], [-300, 300]], "from": 0, "to": 200});
        let bowl = json!({"plane": "xz", "profile": [[0, 50], [200, 50], [200, 250], [0, 250]], "axis": [[0, 0], [0, 1]], "to": 360, "void": true});
        let basin = built(json!([block, bowl])).unwrap();
        let k = 16.0 * (std::f64::consts::TAU / 32.0).sin();
        assert!((volume(&basin[0].tris) - (600.0 * 600.0 * 200.0 - k * 200.0 * 200.0 * 150.0)).abs() < 1.0, "{}", volume(&basin[0].tris));
    }

    #[test]
    fn voids_of_many_triangles_never_exhaust_the_stack() {
        // Two revolves of 64 corners each, the one cutting the other, on a small stack: done, and quickly.
        let ring: Vec<Value> = (0..MAX_PROFILE)
            .map(|k| (k as f64 * std::f64::consts::TAU / MAX_PROFILE as f64).sin_cos())
            .map(|(s, c)| json!([400.0 + 100.0 * c, 300.0 + 100.0 * s]))
            .collect();
        let worked = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let started = std::time::Instant::now();
                let solid = json!({"plane": "xz", "profile": ring, "axis": [[0, 0], [0, 1]], "to": 360});
                let void =
                    json!({"profile": [[-1000, -1000], [1000, -1000], [1000, 0], [-1000, 0]], "from": -10, "to": 1000, "void": true});
                (built(json!([solid, void])), started.elapsed())
            })
            .unwrap()
            .join()
            .unwrap();
        let (pieces, took) = worked;
        let pieces = pieces.unwrap();
        assert!(volume(&pieces[0].tris) > 0.0 && took.as_secs_f64() < 10.0, "{took:?}: a hang, not a slow machine");
    }
    /// A one-type lighting family holding `connectors`.
    fn lamp(connectors: Value) -> Family {
        serde_json::from_value(json!({
            "category": "lighting",
            "parameters": [{"name": "Load", "kind": "number", "value": 0}],
            "types": {"9 W": {"Load": 9}, "18 W": {"Load": 18}},
            "connectors": connectors
        }))
        .unwrap()
    }

    fn power() -> Value {
        json!({"kind": "power", "system": "power-balanced", "poles": 1, "voltage": 127, "load_va": "Load", "power_factor": 0.9,
               "classification": "lighting"})
    }

    #[test]
    fn a_family_holds_connectors_and_a_connector_reads_its_load_from_the_type() {
        let f = lamp(json!([power()]));
        assert_eq!(f.check(&|_| true), Ok(()));
        for (t, load) in [("9 W", 9.0), ("18 W", 18.0)] {
            let c = f.connector_values(t, &BTreeMap::new()).unwrap();
            assert_eq!(c.len(), 1);
            assert_eq!((c[0].kind, c[0].system, c[0].poles), (ConnectorKind::Power, ConnectorSystem::PowerBalanced, 1));
            assert_eq!((c[0].voltage, c[0].load_va, c[0].power_factor), (127.0, load, 0.9));
            assert_eq!((c[0].load_va_by_phase.clone(), c[0].classification.as_str()), (vec![load], "lighting"));
        }
        // Written back as it was read, so a loaded family file keeps them.
        let back: Family = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn a_family_without_connectors_is_as_it_was() {
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}})).unwrap();
        assert!(f.connectors.is_empty());
        assert_eq!(f.check(&|_| true), Ok(()));
        assert!(serde_json::to_value(&f).unwrap().get("connectors").is_none(), "an empty list isn't written");
        assert!(f.connector_values("A", &BTreeMap::new()).unwrap().is_empty());
    }

    #[test]
    fn two_power_connectors_are_refused_and_the_message_says_so() {
        let e = lamp(json!([power(), power()])).check(&|_| true).unwrap_err();
        assert!(e.contains("two power connectors") && e.contains("keep one"), "{e}");
        // A switch beside a power connector is no second power connector.
        let sw = json!({"kind": "switch", "system": "switch"});
        assert_eq!(lamp(json!([power(), sw])).check(&|_| true), Ok(()));
    }

    #[test]
    fn a_connector_is_checked_for_what_it_says() {
        let bad = |change: Value, says: &str| {
            let mut c = power();
            c.as_object_mut().unwrap().extend(change.as_object().unwrap().clone());
            let e = lamp(json!([c])).check(&|_| true).unwrap_err();
            assert!(e.contains(says), "{change}: {e}");
        };
        bad(json!({"system": "switch"}), "system");
        bad(json!({"poles": 0}), "poles");
        bad(json!({"poles": 4}), "poles");
        bad(json!({"voltage": 0}), "voltage");
        bad(json!({"voltage": -127}), "voltage");
        bad(json!({"voltage": 1e12}), "voltage");
        bad(json!({"power_factor": 0}), "power factor");
        bad(json!({"power_factor": 1.5}), "power factor");
        bad(json!({"load_va": -1}), "load");
        bad(json!({"load_va": 1e300}), "load");
        bad(json!({"load_va": "Nope"}), "Nope");
        bad(json!({"load_va": [1, 2]}), "per phase");
        bad(json!({"classification": "x".repeat(MAX_TEXT + 1)}), "classification");
        let sw = |extra: Value| {
            let mut c = json!({"kind": "switch", "system": "switch"});
            c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            lamp(json!([c])).check(&|_| true)
        };
        assert_eq!(sw(json!({})), Ok(()));
        assert!(sw(json!({"load_va": 10})).unwrap_err().contains("switch"));
        assert!(sw(json!({"system": "power-balanced"})).unwrap_err().contains("system"));
        let many: Vec<Value> = (0..=MAX_CONNECTORS).map(|_| json!({"kind": "switch", "system": "switch"})).collect();
        assert!(lamp(json!(many)).check(&|_| true).unwrap_err().contains("connectors"));
    }

    #[test]
    fn an_unbalanced_connector_may_load_each_phase_differently() {
        let f = lamp(json!([{"kind": "power", "system": "power-unbalanced", "poles": 3, "voltage": 220,
                              "load_va": [1000, 2000, "Load"], "power_factor": 0.8}]));
        assert_eq!(f.check(&|_| true), Ok(()));
        let c = &f.connector_values("9 W", &BTreeMap::new()).unwrap()[0];
        assert_eq!(c.load_va_by_phase, vec![1000.0, 2000.0, 9.0]);
        assert_eq!(c.load_va, 3009.0);
        // A single number is shared out evenly; a balanced connector takes no list.
        let shared = lamp(json!([{"kind": "power", "system": "power-unbalanced", "poles": 3, "voltage": 220, "load_va": 3000}]));
        assert_eq!(shared.connector_values("9 W", &BTreeMap::new()).unwrap()[0].load_va_by_phase, vec![1000.0; 3]);
        let e = lamp(json!([{"kind": "power", "system": "power-balanced", "poles": 3, "voltage": 220, "load_va": [1, 2, 3]}]))
            .check(&|_| true)
            .unwrap_err();
        assert!(e.contains("balanced"), "{e}");
    }
}
