//! Spot coordinates on the desktop: the Spot Coordinate tool's type, Spot Coordinate Type Properties (the dialog Edit Type
//! opens, every type parameter Revit's has: leader arrowhead and its size, the leader's and the arrowhead's pens and colour,
//! the symbol, the lettering and its colour, the coordinate origin, the top and bottom values, the indicators, the elevation,
//! units and alternate units), the grip on a selected spot coordinate's leader end, and Acquire Coordinates' pick in the view
//! (Coordinates > Manage > Pick in the View).

use bimcraft_core::coordinates::Origin;
use bimcraft_core::dims::Background;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::spots::{Alternate, Arrowhead, Indicator, SpotCoordinateType, Symbol, TextLocation, Value as Read};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{ComboBox, Context, DragValue, Id as UiId, Painter, PointerButton, Rect, Response, Stroke, StrokeKind, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{canvas_pal, pal};
use crate::typeprops::{self, Head, Naming, Table};
use crate::{BimApp, Tool};

/// Spot Coordinate Type Properties: one type's values, and the spot coordinates it opened for, which OK puts on the type shown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpotTypeDialog {
    name: String,
    t: SpotCoordinateType,
    elements: Vec<Id>,
    naming: Naming,
    error: Option<String>,
    find: String,
}

impl SpotTypeDialog {
    pub(crate) fn new(m: &Model, name: String, elements: Vec<Id>) -> SpotTypeDialog {
        let t = m.spot_coordinate_types.get(&name).cloned().unwrap_or_default();
        SpotTypeDialog { name, t, elements, naming: Naming::None, error: None, find: String::new() }
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn typed(&mut self) -> &mut SpotCoordinateType {
        &mut self.t
    }

    /// The `spot_coordinate_type` command for the type as typed, under `name` (from the one shown, for a duplicate).
    fn params(&self, name: &str) -> Value {
        let mut p = serde_json::to_value(&self.t).unwrap_or_else(|_| json!({}));
        if let Some(o) = p.as_object_mut() {
            o.insert("name".into(), json!(name));
            o.insert("like".into(), json!(self.name));
            // What isn't set is set back: null.
            for k in ["units", "decimals", "alternate_decimals"] {
                o.entry(k).or_insert(Value::Null);
            }
        }
        p
    }
}

/// Lengths on paper, from 0 to 50 mm.
fn mm(x: &mut f64) -> DragValue<'_> {
    DragValue::new(x).range(0.0..=50.0).speed(0.1).max_decimals(2).suffix(" mm")
}

/// A row choosing one of `options`, each a value and its label.
fn choice<T: Copy + PartialEq>(tb: &mut Table, label: &str, value: &mut T, options: &[(T, &str)]) {
    let labels: Vec<String> = options.iter().map(|(_, l)| t(l).to_string()).collect();
    let mut now = options.iter().position(|(v, _)| v == value).and_then(|i| labels.get(i)).cloned().unwrap_or_default();
    tb.pick(label, &mut now, &labels);
    if let Some((v, _)) = labels.iter().position(|l| *l == now).and_then(|i| options.get(i)) {
        *value = *v;
    }
}

/// How each arrowhead is named, as Revit's Leader Arrowhead lists them.
pub(crate) const ARROWHEADS: [(Arrowhead, &str); 5] = [
    (Arrowhead::Filled, msg!("Arrow Filled")),
    (Arrowhead::Empty, msg!("Arrow Empty")),
    (Arrowhead::Dot, msg!("Dot Filled")),
    (Arrowhead::EmptyDot, msg!("Dot Empty")),
    (Arrowhead::None, msg!("None")),
];

impl BimApp {
    /// The type a new spot coordinate is placed in: the one the Type Selector names, while the project has it, else the
    /// project's first in shared coordinates.
    pub(crate) fn spot_type(&self) -> Option<String> {
        let m = &self.session.model;
        match self.types.get("spotcoordinate") {
            Some(name) if m.spot_coordinate_types.contains_key(name) => Some(name.clone()),
            _ => bimcraft_core::spots::default_type(&m.spot_coordinate_types, Origin::Survey).cloned(),
        }
    }

    /// The Spot Coordinate tool's click at `p`, snapped: the point it reads, then where its leader ends, which places it.
    pub(crate) fn spot_coordinate_click(&mut self, key: Id, first: Option<P2>, p: P2) {
        let Some(at) = first else {
            self.tool = Tool::SpotCoordinate(Some(p));
            return;
        };
        let mut params = json!({"at": [at.0, at.1], "end": [p.0, p.1], "view": key});
        if let Some(t) = self.spot_type() {
            params["type"] = json!(t);
        }
        if self.run("spot_coordinate", params).is_some() {
            self.tool = Tool::SpotCoordinate(None);
        }
    }

    /// Spot Coordinate Type Properties, while it's open.
    pub(crate) fn spot_type_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.spot_type_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let names: Vec<String> = model.spot_coordinate_types.keys().cloned().collect();
        let fonts = bimcraft_core::text::families();
        let (mut ok, mut cancel, mut apply, mut asked) = (false, false, false, None);
        let r = Modal::new(UiId::new("spot-type-properties"), t("Type Properties")).width(typeprops::WIDTH).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                asked = typeprops::head(
                    ui,
                    "spot-type",
                    &tf("System Family: {}", &[t("Spot Coordinates")]),
                    &names,
                    &d.name,
                    &mut d.naming,
                    true,
                );
                let below = typeprops::foot_height(ui, d.error.is_some());
                typeprops::table(ui, "spot-type", below, |tb| type_rows(tb, &mut d, &fonts));
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if cancel || r.should_close() {
            return;
        }
        match asked {
            Some(Head::Switch(name)) => d = SpotTypeDialog { elements: d.elements, ..SpotTypeDialog::new(&model, name, Vec::new()) },
            Some(Head::Duplicate(name)) => match self.session.execute("spot_coordinate_type", &d.params(&name)) {
                Ok(_) => d = SpotTypeDialog { elements: d.elements, ..SpotTypeDialog::new(&self.session.model, name, Vec::new()) },
                Err(e) => d.error = Some(e),
            },
            Some(Head::Rename(to)) => match self.session.execute("spot_coordinate_type", &json!({"name": d.name, "rename": to})) {
                Ok(_) => {
                    if self.types.get("spotcoordinate") == Some(&d.name) {
                        self.types.insert("spotcoordinate", to.clone());
                    }
                    let typed = d.t.clone();
                    d = SpotTypeDialog { elements: d.elements, ..SpotTypeDialog::new(&self.session.model, to, Vec::new()) };
                    d.t = typed;
                }
                Err(e) => d.error = Some(e),
            },
            None => {}
        }
        if ok || apply {
            let changed = model.spot_coordinate_types.get(&d.name) != Some(&d.t);
            let mut result = if changed { self.session.execute("spot_coordinate_type", &d.params(&d.name)).map(|_| ()) } else { Ok(()) };
            let moved: Vec<Id> = d
                .elements
                .iter()
                .copied()
                .filter(|id| {
                    matches!(self.session.model.elements.get(*id), Some(Element::Spot(x)) if x.type_name.as_deref() != Some(d.name.as_str()))
                })
                .collect();
            if ok && result.is_ok() && !moved.is_empty() {
                result = self.session.execute("modify", &json!({"ids": moved, "type": d.name})).map(|_| ());
            }
            match result {
                Ok(()) if ok => return,
                Ok(()) => d.error = None,
                Err(e) => d.error = Some(e),
            }
        }
        self.spot_type_dialog = Some(d);
    }

    /// The spot coordinate selected alone, with Modify: the one whose leader's end is a grip.
    pub(crate) fn spot_gripped(&self) -> Option<Id> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) if matches!(self.session.model.elements.get(id), Some(Element::Spot(sp)) if sp.coordinate && sp.leader) => {
                Some(id)
            }
            _ => None,
        }
    }

    /// Where the selected spot coordinate's leader ends in `view`.
    fn spot_end(&self, id: Id, view: &bimcraft_core::render::View) -> Option<P2> {
        let m = &self.session.model;
        match m.elements.get(id) {
            Some(Element::Spot(sp)) => Some(bimcraft_core::render::leader_end(m, *view, sp)),
            _ => None,
        }
    }

    /// A drag from the selected spot coordinate's leader end moves it there, snapped; on release, `modify` with `end`.
    pub(crate) fn spot_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &bimcraft_core::render::View, level: Id) {
        let Some(id) = self.spot_gripped() else {
            self.spot_grip = None;
            return;
        };
        let Some(end) = self.spot_end(id, view) else { return };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let (x, y) = view.px(end);
            if (rect.min + vec2(x, y)).distance(o) <= 8.0 {
                self.spot_grip = Some(id);
            }
        }
        if self.spot_grip != Some(id) {
            self.spot_grip = None;
            return;
        }
        if response.drag_stopped() {
            self.spot_grip = None;
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let m = std::sync::Arc::clone(&self.session.model);
                let to = self.snap(&m, level, view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at;
                self.snap_once = None;
                if to.dist(end) > 1e-6 {
                    self.run("modify", json!({"id": id, "end": [to.0, to.1]}));
                }
            }
        }
    }

    /// The selected spot coordinate's grip at its leader's end, and while it is dragged, the leader to the cursor.
    pub(crate) fn spot_grip_overlay(&self, p: &Painter, rect: Rect, view: &bimcraft_core::render::View, cursor: Option<P2>) {
        let Some(id) = self.spot_gripped() else { return };
        let Some(end) = self.spot_end(id, view) else { return };
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let live = cursor.filter(|_| self.spot_grip == Some(id));
        if let (Some(to), Some(Element::Spot(sp))) = (live, self.session.model.elements.get(id)) {
            p.line_segment([at(sp.at), at(to)], Stroke::new(1.5, pal().accent));
        }
        let r = Rect::from_center_size(at(live.unwrap_or(end)), vec2(9.0, 9.0));
        p.rect_filled(r, 1.0, if live.is_some() { pal().accent } else { canvas_pal().surface });
        p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
    }

    /// The project's shared coordinates taken from what `picked` is of: a linked model's element names its link.
    pub(crate) fn acquire_from(&mut self, picked: Id) {
        let from = bimcraft_core::link::link_of(&self.session.model, picked).unwrap_or(picked);
        if self.run("coordinates", json!({"acquire": from})).is_some() {
            let name = match self.session.model.elements.get(from) {
                Some(Element::Link(l)) => l.name.clone(),
                Some(Element::CadImport(c)) => c.cad.clone(),
                _ => String::new(),
            };
            self.tool = Tool::Modify;
            self.status = crate::Status { text: tf("Coordinates acquired from {}.", &[&name]), error: false };
        }
    }
}

/// Every row of Spot Coordinate Type Properties, grouped as Revit's.
fn type_rows(tb: &mut Table, d: &mut SpotTypeDialog, fonts: &[String]) {
    let x = &mut d.t;
    tb.group("Constraints");
    tb.check(t("Rotate with Component"), &mut x.rotate_with_component);
    tb.group("Graphics");
    choice(tb, t("Leader Arrowhead"), &mut x.leader_arrowhead, &ARROWHEADS);
    tb.drag(t("Arrowhead Size"), mm(&mut x.arrowhead_size));
    tb.drag(t("Leader Line Weight"), DragValue::new(&mut x.leader_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
    tb.drag(t("Leader Arrowhead Line Weight"), DragValue::new(&mut x.arrowhead_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
    tb.color(t("Leader Color"), &mut x.leader_color);
    choice(
        tb,
        t("Symbol"),
        &mut x.symbol,
        &[(Symbol::None, msg!("None")), (Symbol::Cross, msg!("Cross")), (Symbol::Target, msg!("Target"))],
    );
    tb.drag(t("Symbol Size"), mm(&mut x.symbol_size));
    tb.group("Text");
    tb.drag(t("Width Factor"), DragValue::new(&mut x.width_factor).range(0.1..=10.0).speed(0.01).max_decimals(2));
    tb.check(t("Underline"), &mut x.underline);
    tb.check(t("Italic"), &mut x.italic);
    tb.check(t("Bold"), &mut x.bold);
    tb.drag(t("Text Size"), DragValue::new(&mut x.text_size).range(0.1..=100.0).speed(0.1).max_decimals(2).suffix(" mm"));
    tb.drag(t("Text Offset from Leader"), mm(&mut x.text_offset_leader));
    tb.drag(t("Text Offset from Symbol"), mm(&mut x.text_offset_symbol));
    choice(
        tb,
        t("Text Location"),
        &mut x.text_location,
        &[(TextLocation::AboveLeader, msg!("Above Leader")), (TextLocation::BelowLeader, msg!("Below Leader"))],
    );
    let font = &mut x.font;
    let find = &mut d.find;
    tb.row(t("Text Font"), |ui, l| {
        ComboBox::from_id_salt("spot-type-font")
            .selected_text(font.as_str())
            .width(ui.available_width())
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.add(TextEdit::singleline(find).hint_text(t("Search fonts")).desired_width(180.0));
                let find = find.trim().to_lowercase();
                for f in fonts.iter().filter(|f| f.to_lowercase().contains(&find)) {
                    ui.selectable_value(font, f.clone(), f);
                }
            })
            .response
            .labelled_by(l)
    });
    tb.color(t("Text Color"), &mut x.text_color);
    choice(
        tb,
        t("Text Background"),
        &mut x.background,
        &[(Background::Opaque, msg!("Opaque")), (Background::Transparent, msg!("Transparent"))],
    );
    choice(
        tb,
        t("Coordinate Origin"),
        &mut x.origin,
        &[(Origin::Base, msg!("Project Base Point")), (Origin::Survey, msg!("Survey Point")), (Origin::Internal, msg!("Internal Origin"))],
    );
    let values = [(Read::NorthSouth, msg!("North / South")), (Read::EastWest, msg!("East / West")), (Read::None, msg!("None"))];
    choice(tb, t("Top Value"), &mut x.top, &values);
    choice(tb, t("Bottom Value"), &mut x.bottom, &values);
    tb.text(t("North / South Indicator"), &mut x.north_south_indicator, "N ");
    tb.text(t("East / West Indicator"), &mut x.east_west_indicator, "E ");
    tb.check(t("Include Elevation"), &mut x.include_elevation);
    tb.text(t("Elevation Indicator"), &mut x.elevation_indicator, "EL ");
    choice(
        tb,
        t("Indicator as Prefix / Suffix"),
        &mut x.indicator,
        &[(Indicator::Prefix, msg!("Prefix")), (Indicator::Suffix, msg!("Suffix"))],
    );
    let mut units: Vec<(Option<Units>, &str)> = vec![(None, msg!("Use Project Settings"))];
    units.extend(Units::ALL.iter().map(|u| (Some(*u), u.label())));
    choice(tb, t("Units Format"), &mut x.units, &units);
    let mut places: Vec<(Option<u8>, String)> = vec![(None, t("As the units show").to_string())];
    places.extend((0..=6u8).map(|n| (Some(n), n.to_string())));
    let options: Vec<(Option<u8>, &str)> = places.iter().map(|(v, l)| (*v, l.as_str())).collect();
    choice(tb, t("Decimal Places"), &mut x.decimals, &options);
    choice(
        tb,
        t("Alternate Units"),
        &mut x.alternate,
        &[(Alternate::None, msg!("None")), (Alternate::Right, msg!("Right")), (Alternate::Below, msg!("Below"))],
    );
    let alt: Vec<(Units, &str)> = Units::ALL.iter().map(|u| (*u, u.label())).collect();
    choice(tb, t("Alternate Units Format"), &mut x.alternate_units, &alt);
    choice(tb, t("Alternate Decimal Places"), &mut x.alternate_decimals, &options);
    tb.text(t("Alternate Units Prefix"), &mut x.alternate_prefix, "");
    tb.text(t("Alternate Units Suffix"), &mut x.alternate_suffix, "");
}
