//! Families: BIMCraft's own parametric components (never Autodesk's `.rfa`). A family has
//! parameters, types that set its type parameters, and forms, each a profile swept across a
//! sketch plane and sized by the parameters; a component places one of its types. Formulas read
//! as Revit's do: `Width / 2`, `if(Depth > 600, 4, 2)`.

use bimcraft_i18n::{msg, ui_msg};

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::csg;
use crate::geom::P2;
use crate::model::{MAX_COORD, MAX_TEXT};
use crate::view3d::{P3, add, cross, mul};

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
}

/// A parameter: a type's (each type its own value) or, `instance`, each component's.
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

/// Where a form's profile is drawn: across the family's x and y, swept up; or upright, across x
/// and z, swept along y; or across y and z, swept along x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[default]
    Xy,
    Xz,
    Yz,
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

/// A line a door's or a window's plan draws, in its own axes (a swing, a sill), and no view shows
/// otherwise: a `line` through points, or an `arc`; `visible` (a yes/no parameter, or a formula) whether it's there.
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
    pub visible: Option<Expr>,
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
/// its base): a profile drawn in `plane` and swept across it from `from` to `to`; or, with an
/// `axis`, turned about it from `from` to `to` degrees; or triangles, as a file brings them. A
/// material's name, or a material parameter's, says what it's made of; `visible` (a yes/no
/// parameter, or a formula) whether it's there. A `void` is no solid: it cuts the solid forms listed
/// before it that it passes through, and is never seen itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    #[serde(default)]
    pub plane: Plane,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle: Option<Circle>,
    #[serde(default)]
    pub from: Expr,
    #[serde(default)]
    pub to: Expr,
    /// A revolve: the line in the sketch plane its profile turns about, right-handed from the axis'
    /// first point toward its second; its `from` and `to` are angles, a full turn at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<[[Expr; 2]; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub void: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// Shared, so an undo step's copy of the model doesn't copy it.
    #[serde(default, skip_serializing_if = "no_mesh")]
    pub mesh: Arc<Vec<[P3; 3]>>,
}

fn no_mesh(mesh: &Arc<Vec<[P3; 3]>>) -> bool {
    mesh.is_empty()
}

/// A family: its parameters, its types (each its type parameters' values where they aren't the
/// parameters' own) and its forms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    #[serde(default)]
    pub category: Category,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub types: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    pub forms: Vec<Form>,
    /// What a door's or a window's plan draws besides its forms: lines of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
    /// How a door's or a window's leaves open, which IFC and schedules say; none, a door swings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<crate::model::Operation>,
    /// Where it joins a system: an electrical device's load, a switch. At most one is `power`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<Connector>,
}

/// A parameter's value, worked out: a number (yes/no, 1 or 0), or a material's name.
#[derive(Clone, Debug, PartialEq)]
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
    pub bottom: f64,
    pub top: f64,
    pub material: String,
    pub round: bool,
}

/// How deep a formula may nest.
const MAX_DEPTH: usize = 64;
/// Sides a circle's profile is drawn with.
const SEGMENTS: usize = 32;
/// Degrees a symbol's arc turns between the corners it is drawn with.
const ARC_STEP: f64 = 3.75;
/// Corners one profile may have.
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

/// A formula, read.
enum Node {
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
                text.parse().map(Node::Number).map_err(|_| format!("`{text}` isn't a number"))
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
    let node = r.compare().map_err(|e| format!("formula `{formula}`: {e}"))?;
    match r.peek() {
        Some(c) => Err(format!("formula `{formula}`: `{c}` follows where it should end")),
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
        Kind::Length | Kind::Number | Kind::Angle => match v {
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(Val::Number).ok_or_else(|| bad("a number")),
        },
    }
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
        for form in self.forms.iter_mut().filter(|x| !x.material.is_empty() && !params.contains(&x.material)) {
            f(&mut form.material);
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
        self.forms.iter_mut().filter(|f| f.material == from && !params.contains(&f.material)).for_each(|f| f.material = to.into());
    }

    /// Its parameters' values for type `type_name`, a component's `instance` values over them, and
    /// its formulas worked out; a hosted family's in a wall of the usual thickness.
    pub fn values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Val>, String> {
        self.values_in(type_name, instance, DEFAULT_WALL)
    }

    /// [`Family::values`], with a hosted family's wall `wall` mm thick: what its formulas read as `Wall Thickness`.
    pub fn values_in(&self, type_name: &str, instance: &BTreeMap<String, Value>, wall: f64) -> Result<BTreeMap<String, Val>, String> {
        let set = self.types.get(type_name).ok_or_else(|| {
            let names: Vec<&str> = self.types.keys().map(String::as_str).collect();
            format!("type `{type_name}` doesn't exist (there are: {})", names.join(", "))
        })?;
        let mut done = BTreeMap::new();
        if self.category.hosted() {
            done.insert(WALL.to_string(), Val::Number(wall));
        }
        for p in &self.parameters {
            self.resolve(&p.name, set, instance, &mut done, &mut Vec::new())?;
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
        let p = self.parameters.iter().find(|p| p.name == name).ok_or_else(|| format!("`{name}` isn't one of its parameters"))?;
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
                    Val::Text(_) => Err(format!("`{n}` is a material, not a number")),
                });
                path.pop();
                Val::Number(x.map_err(|e| format!("parameter `{name}`: {e}"))?)
            }
            None => read(p, if p.instance { instance.get(name) } else { set.get(name) }.unwrap_or(&p.value))?,
        };
        done.insert(name.to_string(), v.clone());
        Ok(v)
    }

    /// Whether it's sound: its parameters named once each, so formulas can say them, and holding
    /// what their kinds hold; its types setting only the type parameters no formula works out; its
    /// materials in the library (`has_material`); and every type building.
    pub fn check(&self, has_material: &dyn Fn(&str) -> bool) -> Result<(), String> {
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
        if self.category.hosted() {
            for name in [WIDTH, HEIGHT] {
                if !self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Length) {
                    return Err(format!("a door or window family needs a length parameter `{name}`: the size of the opening it makes"));
                }
            }
            if self.parameters.iter().any(|p| p.name == WALL) {
                return Err(format!("`{WALL}` is the wall's, which a door or a window reads; name the parameter otherwise"));
            }
        } else if !self.symbols.is_empty() {
            return Err("symbols are the lines a door's or a window's plan draws; this family isn't one".into());
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
                (Some(f), _) if long(f) => return Err(format!("{who}: its formula is longer than {MAX_FORMULA} characters")),
                _ => {}
            }
            if let Val::Text(m) = read(p, &p.value)? {
                material_ok(&who, &m)?;
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
                if p.formula.is_some() {
                    return Err(format!("{who}: `{k}` is worked out by its formula"));
                }
                if let Val::Text(m) = read(p, v)? {
                    material_ok(&who, &m)?;
                }
            }
        }
        for (i, form) in self.forms.iter().enumerate() {
            let who = format!("form {}", i + 1);
            let ways = usize::from(!form.profile.is_empty()) + usize::from(form.circle.is_some()) + usize::from(!form.mesh.is_empty());
            if ways != 1 {
                return Err(format!("{who}: it takes a `profile`, a `circle` or a `mesh`, one of them"));
            }
            if form.axis.is_some() && !form.mesh.is_empty() {
                return Err(format!("{who}: a `mesh` is as it is; an `axis` turns a `profile` or a `circle`"));
            }
            if !self.parameters.iter().any(|p| p.name == form.material && p.kind == Kind::Material) {
                material_ok(&who, &form.material)?;
            }
            let circle = form.circle.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
            let mut exprs = form
                .profile
                .iter()
                .flatten()
                .chain([&form.from, &form.to])
                .chain(&form.visible)
                .chain(circle)
                .chain(form.axis.iter().flatten().flatten());
            if exprs.any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
        }
        let symbol_exprs = self.symbols.iter().flat_map(|y| {
            y.line.iter().flatten().chain(&y.visible).chain(y.arc.iter().flat_map(|c| c.centre.iter().chain([&c.radius, &c.from, &c.to])))
        });
        if symbol_exprs.into_iter().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
            return Err(format!("a formula of a symbol is longer than {MAX_FORMULA} characters"));
        }
        self.check_connectors()?;
        // Each formula read once across the types; past what a family may build, refused.
        let (mut read, mut built, mut cuts) = (HashMap::new(), 0, csg::Meter::default());
        for t in self.types.keys() {
            let pieces = self
                .values(t, &BTreeMap::new())
                .and_then(|v| {
                    self.strokes(&v)?;
                    self.build(&v, &mut read, &mut cuts)
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
        self.build(values, &mut HashMap::new(), &mut csg::Meter::default())
    }

    /// The lines of its visible symbols, sized by `values`, in its own axes: an arc as a polyline of short chords.
    pub fn strokes<'a>(&'a self, values: &BTreeMap<String, Val>) -> Result<Vec<Vec<P2>>, String> {
        let mut read = HashMap::new();
        let mut out = Vec::new();
        for (i, sym) in self.symbols.iter().enumerate() {
            let said = |e: String| format!("symbol {}: {e}", i + 1);
            if let Some(v) = &sym.visible
                && number(v, values, &mut read).map_err(said)? == 0.0
            {
                continue;
            }
            let ways = usize::from(!sym.line.is_empty()) + usize::from(sym.arc.is_some());
            if ways != 1 || sym.line.len() > MAX_PROFILE {
                return Err(said(format!("it takes a `line` of up to {MAX_PROFILE} points, or an `arc`, one of them")));
            }
            let mut pts = Vec::new();
            for [a, b] in &sym.line {
                pts.push(P2(number(a, values, &mut read).map_err(said)?, number(b, values, &mut read).map_err(said)?));
            }
            if let Some(c) = &sym.arc {
                let mut n = |e: &'a Expr| number(e, values, &mut read).map_err(said);
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
            out.push(pts);
        }
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
        let mut num = |e: &'a Expr| number(e, values, read);
        let (mut out, mut voids) = (Vec::new(), Vec::new());
        for (i, form) in self.forms.iter().enumerate() {
            let what = format!("form {}", i + 1);
            let said = |e: String| format!("{what}: {e}");
            if let Some(v) = &form.visible
                && num(v).map_err(said)? == 0.0
            {
                continue;
            }
            let material = match values.get(&form.material) {
                Some(Val::Text(m)) => m.clone(),
                _ => form.material.clone(),
            };
            let piece = if !form.mesh.is_empty() {
                mesh(&form.mesh, material).map_err(said)?
            } else {
                let ring: Vec<P2> = match &form.circle {
                    Some(c) => {
                        let (x, y, r) = (num(&c.centre[0]).map_err(said)?, num(&c.centre[1]).map_err(said)?, num(&c.radius).map_err(said)?);
                        if r <= 0.0 {
                            return Err(said(format!("its circle's radius is {r}; it needs one above zero")));
                        }
                        (0..SEGMENTS)
                            .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
                            .map(|(s, c)| P2(x + r * c, y + r * s))
                            .collect()
                    }
                    None if form.profile.len() > MAX_PROFILE => {
                        return Err(said(format!("its profile has more than {MAX_PROFILE} corners")));
                    }
                    None => form.profile.iter().map(|[a, b]| Ok(P2(num(a)?, num(b)?))).collect::<Result<_, String>>().map_err(said)?,
                };
                let (from, to) = (num(&form.from).map_err(said)?, num(&form.to).map_err(said)?);
                if ring.len() < 3 || crate::geom::area(&ring).abs() < 1.0 {
                    return Err(said("its profile has no area: it needs three corners or more, or a circle".into()));
                }
                if to <= from {
                    let unit = if form.axis.is_some() { " degrees" } else { "" };
                    return Err(format!("{what} sweeps nothing: from {from} to {to}{unit}"));
                }
                let ring = if crate::geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring };
                match &form.axis {
                    Some([p, q]) => {
                        if to - from > 360.0 {
                            return Err(format!("{what} turns more than all the way round: from {from} to {to} degrees, 360 at most"));
                        }
                        let mut corner = |c: &'a [Expr; 2]| Ok(P2(num(&c[0])?, num(&c[1])?));
                        let axis = [corner(p).map_err(said)?, corner(q).map_err(said)?];
                        let (tris, outline) = revolve(&ring, axis, (from, to), form.plane).map_err(said)?;
                        let (bottom, top) = heights(&tris);
                        Piece { caps: tris.len(), tris, outline, bottom, top, material, round: false }
                    }
                    None => {
                        let (tris, caps) = sweep(&ring, from, to, form.plane).map_err(said)?;
                        let span = |k: usize| {
                            ring.iter()
                                .map(|p| if k == 0 { p.0 } else { p.1 })
                                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
                        };
                        let ((a0, a1), (b0, b1)) = (span(0), span(1));
                        let (outline, bottom, top) = match form.plane {
                            Plane::Xy => (ring.clone(), from, to),
                            Plane::Xz => (vec![P2(a0, from), P2(a1, from), P2(a1, to), P2(a0, to)], b0, b1),
                            Plane::Yz => (vec![P2(from, a0), P2(to, a0), P2(to, a1), P2(from, a1)], b0, b1),
                        };
                        Piece { tris, caps, outline, bottom, top, material, round: form.circle.is_some() }
                    }
                }
            };
            if form.void {
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
                        material: std::mem::take(&mut piece.material),
                    };
                }
            }
        }
        out.retain(|p| !p.tris.is_empty());
        Ok(out)
    }
}

/// `e` as a number: itself, or its formula worked out over `values`; `read`, the formulas as read already.
fn number<'a>(e: &'a Expr, values: &BTreeMap<String, Val>, read: &mut HashMap<&'a str, Result<Node, String>>) -> Result<f64, String> {
    let x = match e {
        Expr::Number(x) => *x,
        Expr::Formula(f) => match read.entry(f.as_str()).or_insert_with(|| parse(f)) {
            Ok(node) => worked(f, node, &mut |n: &str| match values.get(n) {
                Some(Val::Number(x)) => Ok(*x),
                Some(Val::Text(_)) => Err(format!("`{n}` is a material, not a number")),
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

/// The point of a plane's sketch at `p`, `t` along the way it's swept; with `t` zero, the direction
/// `p` is in the family's axes.
fn embed(plane: Plane, p: P2, t: f64) -> P3 {
    match plane {
        Plane::Xy => [p.0, p.1, t],
        Plane::Xz => [p.0, t, p.1],
        Plane::Yz => [t, p.0, p.1],
    }
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

/// `ring`, counter-clockwise in `plane`, swept across it from `from` to `to`: its caps, then its
/// sides, every triangle turned out; and how many the caps take.
fn sweep(ring: &[P2], from: f64, to: f64, plane: Plane) -> Result<(Vec<[P3; 3]>, usize), String> {
    let at = |p: P2, t: f64| embed(plane, p, t);
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let caps = cap(ring)?;
    let mut tris = Vec::with_capacity(caps.len() * 2 + ring.len() * 2);
    for &[a, b, c] in &caps {
        tris.push([at(pt(a), to), at(pt(b), to), at(pt(c), to)]);
        tris.push([at(pt(a), from), at(pt(c), from), at(pt(b), from)]);
    }
    for (a, b) in crate::geom::ring_edges(ring) {
        let (a0, b0, a1, b1) = (at(a, from), at(b, from), at(a, to), at(b, to));
        tris.push([a0, b0, b1]);
        tris.push([a0, b1, a1]);
    }
    // Across x and z the sweep runs along y: a mirror image of the others' axes, so its triangles
    // turn the other way to face out.
    if plane == Plane::Xz {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok((tris, caps.len() * 2))
}

/// `ring`, counter-clockwise in `plane`, turned about the line through `axis` (in `plane`) from `from` to
/// `to` degrees, right-handed from the axis' first point toward its second: its triangles, every one turned
/// out, a full turn's sides only and a part-turn's with its two ends capped; and its outline seen from above.
fn revolve(ring: &[P2], [p0, p1]: [P2; 2], (from, to): (f64, f64), plane: Plane) -> Result<(Vec<[P3; 3]>, Vec<P2>), String> {
    if p0.dist(p1) < 1e-6 {
        return Err("its axis starts and ends at one point: it needs a direction".into());
    }
    let caps = cap(ring)?;
    // Each corner's distance along the axis, and out from it to its left (none, on it).
    let (along, left) = ((p1 - p0).unit(), (p1 - p0).unit().perp());
    let reach: Vec<(f64, f64)> =
        ring.iter().map(|c| ((*c - p0).dot(along), (*c - p0).dot(left))).map(|(t, r)| (t, if r.abs() < 1e-6 { 0.0 } else { r })).collect();
    let (least, most) = reach.iter().fold((0.0_f64, 0.0_f64), |(lo, hi), (_, r)| (lo.min(*r), hi.max(*r)));
    if least < 0.0 && most > 0.0 {
        return Err("its profile crosses its axis: draw it to one side of it".into());
    }
    if least == 0.0 && most == 0.0 {
        return Err("its profile lies along its axis: it needs some width off it".into());
    }
    // In 3D: along the axis (`u`), out from it (`n`), and the way a corner out to its left turns (`w`). One out to
    // the right turns the other way, which turns its triangles inside out.
    let (o, u, n) = (embed(plane, p0, 0.0), embed(plane, along, 0.0), embed(plane, left, 0.0));
    let w = cross(u, n);
    let turn = to - from;
    let full = turn > 360.0 - 1e-9;
    let steps = (turn / 360.0 * SEGMENTS as f64 - 1e-9).ceil().max(1.0) as usize;
    let angle = |k: usize| (from + turn * (if full { k % steps } else { k }) as f64 / steps as f64).to_radians().sin_cos();
    let at = |i: usize, k: usize| -> P3 {
        let (t, r) = reach.get(i).copied().unwrap_or_default();
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
        for &[a, b, c] in &caps {
            push([at(a, steps), at(b, steps), at(c, steps)]);
            push([at(a, 0), at(c, 0), at(b, 0)]);
        }
    }
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        for k in 0..steps {
            let (a0, b0, a1, b1) = (at(i, k), at(j, k), at(i, k + 1), at(j, k + 1));
            push([a0, b0, b1]);
            push([a0, b1, a1]);
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
    Ok(Piece { tris: tris.to_vec(), caps: tris.len(), outline, bottom, top, material, round: false })
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
                {"line": [[&left, &half_t], [&left, &format!("{wall} / 2 + {run}")]]},
                {"arc": {"centre": [&left, &half_t], "radius": &run, "from": 0, "to": 90}}
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
                {"line": [[&left, &half_t], [&left, &format!("{wall} / 2 + {each}")]]},
                {"arc": {"centre": [&left, &half_t], "radius": &each, "from": 0, "to": 90}},
                {"line": [[&right, &half_t], [&right, &format!("{wall} / 2 + {each}")]]},
                {"arc": {"centre": [&right, &half_t], "radius": &each, "from": 90, "to": 180}}
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
                {"line": [[&left, &inner_y], [past_l, &inner_y]]},
                {"line": [[past_r, &outer_y], [&right, &outer_y]]}
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
                {"line": [[&inner_l, &in_y], ["20", &in_y]]},
                {"line": [["-20", &out_y], [&inner_r, &out_y]]}
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
            serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"profile": ring(MAX_PROFILE + 1), "to": 10}]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("corners"));
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
