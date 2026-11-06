//! Manage > Coordinates: where the project stands in the world, as one window ([coordinates](../../../docs/areas/coordinates.md)).
//!
//! On top, always in sight, a summary of the active location: a dial of project north and true north, the Survey Point's
//! shared coordinates in the map's own unit, and the map system with the latitude and longitude it gives. Under it four tabs:
//! the locations as a table typed into (and shifted all at once when the survey changes), the three points side by side,
//! internal and shared, the map system picked by search, and a converter. Everything is a draft until OK or Apply, which run
//! the `crs`, `location` and `coordinates` commands as one undo step.

use std::collections::BTreeSet;

use bimcraft_core::coordinates::{Coordinates, Frame, Location, MAX_LOCATIONS, Move};
use bimcraft_core::crs::{self, Info};
use bimcraft_core::engine::Session;
use bimcraft_core::library::Kind;
use bimcraft_core::model::Model;
use bimcraft_core::places::{self, Kind as PlaceKind, Place, Suggestion};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{
    Align, Button, ComboBox, Context, CornerRadius, DragValue, Frame as UiFrame, Grid, Id as UiId, Layout, Margin, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::renames;
use crate::style::{ERROR_HEIGHT, boxed_text, pal};
use crate::{BimApp, Status};

/// The window's tabs.
const TABS: [&str; 4] = [msg!("Locations"), msg!("Points"), msg!("Map System"), msg!("Convert")];

/// How many systems a search lists.
const FOUND: usize = 40;

/// How many places a search lists, each with its systems.
const PLACES: usize = 6;

/// The frames the converter goes between, and what each is called.
const FRAMES: [(Frame, &str); 4] = [
    (Frame::Internal, msg!("Internal")),
    (Frame::Base, msg!("From the Project Base Point")),
    (Frame::Shared, msg!("Shared (the map's)")),
    (Frame::Wgs84, msg!("WGS84 (longitude, latitude)")),
];

/// A location as the window holds it: what it was called when the window opened (none, a new one), what it is called now,
/// and its numbers, mm and degrees, as the core keeps them.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    /// Its id in the project's library (`Kind::Location`), shown; none until a new one is applied.
    id: Option<String>,
    was: Option<String>,
    name: String,
    at: Location,
}

/// The Coordinates window: its tab, and the project's coordinates as edited and not yet applied.
pub(crate) struct CoordinatesDialog {
    tab: usize,
    rows: Vec<Row>,
    /// The locations taken away, by the names they had.
    gone: Vec<String>,
    /// The row of the active location.
    active: usize,
    base: [f64; 3],
    survey: [f64; 3],
    crs: Option<u32>,
    /// How a Survey Point typed elsewhere moves: the point alone (the world stays), or the shared system with it.
    survey_moves: Move,
    /// Shift All's amounts, mm.
    shift: [f64; 3],
    /// Specify Coordinates at a Point: the internal point, its shared coordinates, and the angle if it's given too.
    specify_at: [f64; 3],
    specify_shared: [f64; 3],
    specify_angle: Option<f64>,
    /// The map system's search: what is typed, what it found, for the text it found it for.
    search: String,
    found: Vec<Info>,
    /// The countries and cities it names, each with the systems its surveys use.
    places: Vec<(Place, Vec<Suggestion>)>,
    searched: String,
    /// Place by latitude and longitude.
    lat_lon: [f64; 2],
    /// The converter: from, to, the point typed.
    from: Frame,
    to: Frame,
    point: [f64; 3],
    /// The world map's view: the longitude and latitude at its middle, and pixels per degree (0 until it is first fitted).
    map_at: (f32, f32),
    map_zoom: f32,
    error: Option<String>,
}

impl CoordinatesDialog {
    /// Open on the project's coordinates.
    pub(crate) fn new(m: &Model) -> CoordinatesDialog {
        let co = &m.coordinates;
        let rows: Vec<Row> = co
            .locations
            .iter()
            .map(|(n, l)| Row {
                id: m.library.id(&Kind::Location, n).map(|i| i.to_string()),
                was: Some(n.clone()),
                name: n.clone(),
                at: l.clone(),
            })
            .collect();
        let active = rows.iter().position(|r| r.name == co.active).unwrap_or(0);
        let lat_lon =
            co.crs.and_then(|c| crs::to_wgs84(c, co.location().east, co.location().north).ok()).map_or([0.0; 2], |(lon, lat)| [lat, lon]);
        CoordinatesDialog {
            tab: 0,
            rows,
            gone: Vec::new(),
            active,
            base: co.base,
            survey: co.survey,
            crs: co.crs,
            survey_moves: Move::Point,
            shift: [0.0; 3],
            specify_at: co.survey,
            specify_shared: co.to_shared(co.survey),
            specify_angle: None,
            search: String::new(),
            found: Vec::new(),
            places: Vec::new(),
            searched: String::new(),
            lat_lon,
            from: Frame::Internal,
            to: Frame::Shared,
            point: [0.0; 3],
            map_at: (lat_lon[1] as f32, lat_lon[0] as f32),
            map_zoom: 0.0,
            error: None,
        }
    }

    /// The latitude and longitude Place would put the Survey Point at.
    #[cfg(test)]
    pub(crate) fn lat_lon(&self) -> [f64; 2] {
        self.lat_lon
    }

    /// What stopped OK or Apply, if anything did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The draft as the core's coordinates, to work things out with: each location under its row's number, so names being
    /// typed (blank, or the same twice) never lose one.
    fn draft(&self) -> Coordinates {
        let mut co = Coordinates::named("0");
        co.locations = self.rows.iter().enumerate().map(|(i, r)| (i.to_string(), r.at.clone())).collect();
        co.active = self.active.to_string();
        co.base = self.base;
        co.survey = self.survey;
        co.crs = self.crs;
        co
    }

    /// The locations of `co`, as [`Self::draft`] numbered them, back into the rows.
    fn take(&mut self, co: &Coordinates) {
        for (i, r) in self.rows.iter_mut().enumerate() {
            if let Some(l) = co.locations.get(&i.to_string()) {
                r.at = l.clone();
            }
        }
    }

    fn active_location(&self) -> Location {
        self.rows.get(self.active).map(|r| r.at.clone()).unwrap_or_else(|| self.draft().location())
    }

    /// The map system, if the draft has one this version knows.
    fn info(&self) -> Option<Info> {
        self.crs.and_then(|c| crs::lookup(c).ok())
    }

    /// The map's grid unit: mm per unit, and its symbol. Metres without a map system.
    fn grid(&self) -> (f64, String) {
        match self.info() {
            Some(i) => (i.mm(), unit_symbol(&i.unit)),
            None => (1000.0, "m".into()),
        }
    }

    fn add(&mut self) {
        let names: Vec<&str> = self.rows.iter().map(|r| r.name.as_str()).collect();
        let name = spare(&names, t("New Location"));
        let at = self.active_location();
        self.rows.push(Row { id: None, was: None, name, at });
    }

    fn delete(&mut self, i: usize) {
        if i == self.active || i >= self.rows.len() {
            return;
        }
        if let Some(was) = self.rows.remove(i).was {
            self.gone.push(was);
        }
        if self.active > i {
            self.active -= 1;
        }
    }

    /// Shift All: the amounts added to every location.
    fn shift_all(&mut self) {
        for r in &mut self.rows {
            r.at.east += self.shift[0];
            r.at.north += self.shift[1];
            r.at.elevation += self.shift[2];
        }
        self.shift = [0.0; 3];
    }

    /// The Survey Point moved to `to`, as `survey_moves` says.
    fn move_survey(&mut self, to: [f64; 3]) {
        let mut co = self.draft();
        co.move_survey(to, self.survey_moves);
        self.survey = co.survey;
        self.take(&co);
    }

    fn specify(&mut self) {
        let mut co = self.draft();
        co.specify(self.specify_at, self.specify_shared, self.specify_angle);
        self.take(&co);
    }

    /// The active location's Survey Point placed at the latitude and longitude typed.
    fn place(&mut self) -> Result<(), String> {
        let code = self.crs.ok_or_else(|| t("Pick a map system first: latitude and longitude reach the grid through it.").to_string())?;
        let (e, n) = crs::from_wgs84(code, self.lat_lon[1], self.lat_lon[0])?;
        if let Some(r) = self.rows.get_mut(self.active) {
            (r.at.east, r.at.north) = (e, n);
        }
        Ok(())
    }

    /// A system picked from under a place: the map system, and the place as the latitude and longitude to Place by, the map
    /// gone to it. The Survey Point itself waits for Place: a city's middle is rarely the site.
    fn pick_for(&mut self, code: u32, place: &Place) {
        self.crs = Some(code);
        self.lat_lon = [place.lat, place.lon];
        self.map_at = (place.lon as f32, place.lat as f32);
        // Fitted again when the map is drawn: a country wide, a city close.
        self.map_zoom = if place.kind == PlaceKind::City { -8.0 } else { -40.0 };
    }

    /// The search's results, worked out again only when what is typed changes.
    fn find(&mut self) {
        let text = self.search.trim().to_string();
        if text != self.searched {
            let short = text.chars().count() < 2 && text.parse::<u32>().is_err();
            self.found = if short { Vec::new() } else { crs::search(&text, FOUND) };
            self.places = if short {
                Vec::new()
            } else {
                places::find(&text, PLACES).into_iter().map(|p| (places::systems(&p), p)).map(|(s, p)| (p, s)).collect()
            };
            self.searched = text;
        }
    }

    /// The draft is something the core may be asked for: every location has a name, and no two share one.
    fn check(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for r in &self.rows {
            let n = r.name.trim();
            if n.is_empty() {
                return Err(t("A location needs a name.").to_string());
            }
            if !seen.insert(n) {
                return Err(tf("Two locations are called {}.", &[n]));
            }
        }
        Ok(())
    }

    /// What the draft asks of the project, in an order the core can do it in: the map system; the renames (by
    /// `renames::plan`, which gets chains and swaps right); the points, their numbers kept (`move: system`), since the
    /// locations' own come next, as the draft worked them out; the active one; and last what was taken away.
    fn commands(&self, m: &Model) -> Result<Vec<(&'static str, Value)>, String> {
        self.check()?;
        let co = &m.coordinates;
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        if self.crs != co.crs {
            out.push(("crs", self.crs.map_or_else(|| json!({"clear": true}), |c| json!({"code": c}))));
        }
        let rows: Vec<renames::Row> = self
            .rows
            .iter()
            .map(|r| renames::Row { was: r.was.as_deref(), name: r.name.trim(), gone: false })
            .chain(self.gone.iter().map(|n| renames::Row { was: Some(n.as_str()), name: n.as_str(), gone: true }))
            .collect();
        let plan = renames::plan(&co.locations.keys().cloned().collect(), &rows);
        out.extend(plan.renames.iter().map(|(from, to)| ("location", json!({"name": from, "rename": to}))));
        if self.base != co.base || self.survey != co.survey {
            out.push(("coordinates", json!({"base": self.base, "survey": self.survey, "move": "system"})));
        }
        for r in &self.rows {
            let was = r.was.as_ref().and_then(|n| co.locations.get(n));
            if was != Some(&r.at) {
                let l = &r.at;
                out.push((
                    "location",
                    json!({"name": r.name.trim(), "east": l.east, "north": l.north, "elevation": l.elevation, "angle": l.angle, "scale": l.scale}),
                ));
            }
        }
        if let Some(r) = self.rows.get(self.active)
            && r.name.trim() != co.active
        {
            out.push(("location", json!({"name": r.name.trim(), "activate": true})));
        }
        out.extend(plan.deletes.iter().map(|n| ("location", json!({"name": n, "delete": true}))));
        Ok(out)
    }
}

/// A name no row has: `base`, `base 2`…
fn spare(names: &[&str], base: &str) -> String {
    std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} {i}"))).find(|n| !names.contains(&n.as_str())).unwrap_or_default()
}

/// A grid unit as a field's suffix shows it.
fn unit_symbol(unit: &str) -> String {
    match unit {
        "metre" => "m".into(),
        "foot" => "ft".into(),
        "US survey foot" => "US ft".into(),
        other => other.into(),
    }
}

/// A shared value, mm, as the grid shows it: in its unit, to the millimetre.
fn shown(mm: f64, (per, unit): &(f64, String)) -> String {
    format!("{:.3} {unit}", mm / per)
}

/// A field for a shared value, kept in mm and typed in the grid's unit.
fn grid_field(ui: &mut Ui, mm: &mut f64, (per, unit): &(f64, String), name: String) -> bool {
    let mut v = *mm / per;
    let r = ui.add(DragValue::new(&mut v).speed(0.01).fixed_decimals(3).suffix(format!(" {unit}")).min_decimals(3));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, name.clone()));
    let changed = r.changed() && v.is_finite();
    if changed {
        *mm = v * per;
    }
    changed
}

/// A field for an internal length, mm, shown and typed in the project's units.
fn length_field(ui: &mut Ui, mm: &mut f64, units: Units, name: String) -> bool {
    let r = ui.add(
        DragValue::new(mm)
            .speed(10.0)
            .range(-1e9..=1e9)
            .custom_formatter(move |v, _| units.length(v))
            .custom_parser(move |s| units.parse(s)),
    );
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, name.clone()));
    r.changed()
}

/// A field for an angle, degrees.
fn angle_field(ui: &mut Ui, deg: &mut f64, name: String) -> bool {
    let r = ui.add(DragValue::new(deg).speed(0.1).range(-360.0..=360.0).max_decimals(6).suffix("°"));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, name.clone()));
    r.changed()
}

/// The heads of a table's columns.
fn heads(ui: &mut Ui, heads: &[String]) {
    for h in heads {
        ui.label(RichText::new(h).strong());
    }
    ui.end_row();
}

/// A note under a heading: what a part of the window is for.
fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(t(text)).color(pal().muted));
    ui.add_space(4.0);
}

/// A part of a tab: a heading in the theme's band colour, and what's under it.
fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(8.0);
    ui.label(RichText::new(t(title)).strong().size(14.0));
    ui.add_space(2.0);
    add(ui)
}

/// The dial: project north straight up, true north turned `angle` degrees clockwise from it (east positive).
fn dial(ui: &mut Ui, angle: f64) {
    let size = 76.0;
    let (rect, r) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect.expand(2.0));
    let c = rect.center();
    let radius = size / 2.0 - 12.0;
    p.circle(c, radius, pal().surface, Stroke::new(1.0, pal().border));
    for k in 0..12 {
        let a = (k as f32 * 30.0).to_radians();
        let (s, co) = a.sin_cos();
        let inner = if k % 3 == 0 { radius - 5.0 } else { radius - 3.0 };
        p.line_segment([c + vec2(s * inner, -co * inner), c + vec2(s * radius, -co * radius)], Stroke::new(1.0, pal().border));
    }
    // Project north: up the sheet, muted, a notch past the ring; true north is the one lettered.
    p.line_segment([c, c + vec2(0.0, -(radius + 6.0))], Stroke::new(1.0, pal().muted));
    p.add(egui::Shape::convex_polygon(
        vec![c + vec2(0.0, -(radius + 7.0)), c + vec2(3.0, -(radius + 2.0)), c + vec2(-3.0, -(radius + 2.0))],
        pal().muted,
        Stroke::NONE,
    ));
    // True north: an arrow in the accent, lettered at its tip.
    let (s, co) = (angle.to_radians() as f32).sin_cos();
    let dir = vec2(s, -co);
    let tip = c + dir * radius;
    let side = vec2(-dir.y, dir.x);
    p.line_segment([c - dir * (radius * 0.35), tip], Stroke::new(2.0, pal().accent));
    p.add(egui::Shape::convex_polygon(
        vec![tip + dir * 2.0, tip - dir * 7.0 + side * 4.0, tip - dir * 7.0 - side * 4.0],
        pal().accent,
        Stroke::NONE,
    ));
    p.circle_filled(c, 2.5, pal().accent);
    let label = c + dir * (radius + 7.0);
    p.text(label, egui::Align2::CENTER_CENTER, "N", egui::FontId::proportional(11.0), pal().accent);
    let tip = tf("True north is {}° from project north, clockwise", &[&format!("{angle:.3}")]);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, tip.clone()));
    r.on_hover_text(tip);
}

/// A small rounded tag: the map system's code, a frame's name.
fn chip(ui: &mut Ui, text: &str, strong: bool) {
    let (fill, ink) = if strong { (pal().select_fill, pal().accent_ink) } else { (pal().bar, pal().muted) };
    UiFrame::new().fill(fill).corner_radius(CornerRadius::same(9)).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).color(ink).size(12.0));
    });
}

/// The summary across the top: the dial, the active location, the Survey Point's numbers, the map system.
fn summary(ui: &mut Ui, d: &mut CoordinatesDialog) {
    let grid = d.grid();
    let l = d.active_location();
    let info = d.info();
    let at = d.draft().to_shared(d.survey);
    crate::style::card([false; 4]).inner_margin(Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            dial(ui, l.angle);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let label = ui.label(RichText::new(t("Active location")).color(pal().muted));
                    let names: Vec<String> = d.rows.iter().map(|r| r.name.clone()).collect();
                    let shown = names.get(d.active).cloned().unwrap_or_default();
                    let c = ComboBox::from_id_salt("coordinates-active").selected_text(RichText::new(shown).strong()).width(200.0).show_ui(
                        ui,
                        |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(&mut d.active, i, n);
                            }
                        },
                    );
                    c.response.labelled_by(label.id);
                    ui.add_space(8.0);
                    match &info {
                        Some(i) => {
                            chip(ui, &format!("EPSG:{}", i.code), true);
                            ui.label(RichText::new(&i.name).color(pal().muted));
                        }
                        None => chip(ui, t("No map system"), false),
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (label, v) in [(msg!("East"), at[0]), (msg!("North"), at[1]), (msg!("Elevation"), at[2])] {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(t(label)).color(pal().muted).size(11.0));
                            ui.label(RichText::new(shown(v, &grid)).monospace().size(17.0).color(pal().text));
                        });
                        ui.add_space(14.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t("Angle to true north")).color(pal().muted).size(11.0));
                        ui.label(RichText::new(format!("{:.3}°", l.angle)).monospace().size(17.0));
                    });
                    if let Some(i) = &info
                        && let Ok((lon, lat)) = crs::to_wgs84(i.code, at[0], at[1])
                    {
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(t("Latitude, longitude")).color(pal().muted).size(11.0));
                            ui.label(RichText::new(format!("{lat:.6}°, {lon:.6}°")).monospace().size(17.0));
                        });
                    }
                });
                ui.label(RichText::new(t("The Survey Point's shared coordinates, in the active location.")).color(pal().muted).size(11.0));
            });
        });
    });
}

/// The locations: a table typed into, the active one marked, and Shift All under it.
fn locations_tab(ui: &mut Ui, d: &mut CoordinatesDialog) {
    note(
        ui,
        msg!(
            "Each location gives the Survey Point's shared coordinates and the angle to true north, so one building can stand on several sites. Type any value; the model never moves."
        ),
    );
    let grid = d.grid();
    let mut delete = None;
    let one = d.rows.len() <= 1;
    Grid::new("coordinates-locations").striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        let unit = &grid.1;
        heads(
            ui,
            &[
                t("Active").into(),
                t("ID").into(),
                t("Name").into(),
                tf("East ({})", &[unit]),
                tf("North ({})", &[unit]),
                tf("Elevation ({})", &[unit]),
                t("Angle").into(),
                t("Scale").into(),
                String::new(),
            ],
        );
        for (i, r) in d.rows.iter_mut().enumerate() {
            let n = (i + 1).to_string();
            let radio = ui.radio(d.active == i, "");
            radio.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, d.active == i, tf("Location {} active", &[&n])));
            if radio.clicked() {
                d.active = i;
            }
            let id = r.id.clone().unwrap_or_else(|| "—".into());
            ui.label(RichText::new(id).monospace().color(pal().muted))
                .on_hover_text(t("Its id in the project, as every element and library entry has"));
            let name = ui.add_sized([170.0, 22.0], TextEdit::singleline(&mut r.name));
            name.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, tf("Location {} name", &[&n])));
            grid_field(ui, &mut r.at.east, &grid, tf("Location {} east", &[&n]));
            grid_field(ui, &mut r.at.north, &grid, tf("Location {} north", &[&n]));
            grid_field(ui, &mut r.at.elevation, &grid, tf("Location {} elevation", &[&n]));
            angle_field(ui, &mut r.at.angle, tf("Location {} angle", &[&n]));
            let s = ui.add(DragValue::new(&mut r.at.scale).speed(0.0001).range(0.9..=1.1).fixed_decimals(6));
            s.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("Location {} scale", &[&n])));
            let del = ui.add_enabled(!one && d.active != i, Button::new(t("Delete")).small());
            del.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !one && d.active != i, tf("Delete location {}", &[&n])));
            if del.clicked() {
                delete = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete {
        d.delete(i);
    }
    ui.add_space(6.0);
    let add = ui.add_enabled(d.rows.len() < MAX_LOCATIONS, Button::new(t("Add Location")));
    if add.on_hover_text(t("A new location, a copy of the active one")).clicked() {
        d.add();
    }
    section(ui, msg!("Shift every location"), |ui| {
        note(ui, msg!("The survey moved the site? Add the same amounts to every location at once, say +5 m of elevation."));
        ui.horizontal(|ui| {
            let [e, n, z] = &mut d.shift;
            for (label, v) in [(msg!("East"), e), (msg!("North"), n), (msg!("Elevation"), z)] {
                ui.label(t(label));
                grid_field(ui, v, &grid, tf("Shift {}", &[&t(label).to_lowercase()]));
                ui.add_space(6.0);
            }
            if ui.add_enabled(d.shift != [0.0; 3], Button::new(t("Shift All"))).clicked() {
                d.shift_all();
            }
        });
    });
}

/// The points: each internal and shared, side by side; how the Survey Point moves; Specify Coordinates at a Point.
fn points_tab(ui: &mut Ui, d: &mut CoordinatesDialog, units: Units) {
    note(
        ui,
        msg!(
            "Internal coordinates are the file's own, where the geometry is. The Project Base Point is a reference that elevations, spot coordinates and links measure from: moving it moves nothing else."
        ),
    );
    let grid = d.grid();
    let co = d.draft();
    let mut survey = d.survey;
    Grid::new("coordinates-points").striped(true).spacing([10.0, 5.0]).show(ui, |ui| {
        heads(
            ui,
            &[
                t("Point").into(),
                "X".into(),
                "Y".into(),
                "Z".into(),
                tf("East ({})", &[&grid.1]),
                tf("North ({})", &[&grid.1]),
                tf("Elevation ({})", &[&grid.1]),
            ],
        );
        let shared = |ui: &mut Ui, p: [f64; 3]| {
            let q = co.to_shared(p);
            for v in q {
                ui.label(RichText::new(format!("{:.3}", v / grid.0)).monospace().color(pal().muted));
            }
        };
        ui.label(t("Internal Origin"));
        for _ in 0..3 {
            ui.label(RichText::new(units.length(0.0)).color(pal().muted));
        }
        shared(ui, [0.0; 3]);
        ui.end_row();
        ui.label(t("Project Base Point"));
        for (k, axis) in ["x", "y", "z"].iter().enumerate() {
            length_field(ui, &mut d.base[k], units, tf("Project Base Point {}", &[axis]));
        }
        shared(ui, d.base);
        ui.end_row();
        ui.label(t("Survey Point"));
        for (k, axis) in ["x", "y", "z"].iter().enumerate() {
            length_field(ui, &mut survey[k], units, tf("Survey Point {}", &[axis]));
        }
        shared(ui, d.survey);
        ui.end_row();
    });
    if survey != d.survey {
        d.move_survey(survey);
    }
    section(ui, msg!("When the Survey Point moves"), |ui| {
        let choices = [
            (
                Move::Point,
                msg!("Move the point only"),
                msg!("The building stays where it is in the world: every location's numbers are worked out again."),
            ),
            (
                Move::System,
                msg!("Move the shared coordinates with it"),
                msg!("Its numbers stay, and the building moves in the world, as the point does."),
            ),
        ];
        for (how, label, says) in choices {
            ui.horizontal(|ui| {
                if ui.radio(d.survey_moves == how, t(label)).clicked() {
                    d.survey_moves = how;
                }
                ui.label(RichText::new(t(says)).color(pal().muted));
            });
        }
    });
    section(ui, msg!("Specify Coordinates at a Point"), |ui| {
        note(ui, msg!("Give an internal point the shared coordinates the survey gives it: the active location is worked out to match."));
        Grid::new("coordinates-specify").spacing([10.0, 5.0]).show(ui, |ui| {
            ui.label(t("Internal point"));
            for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                length_field(ui, &mut d.specify_at[k], units, tf("Specify at {}", &[axis]));
            }
            ui.end_row();
            ui.label(t("Shared coordinates"));
            for (k, axis) in ["east", "north", "elevation"].iter().enumerate() {
                grid_field(ui, &mut d.specify_shared[k], &grid, tf("Specify {}", &[axis]));
            }
            ui.end_row();
            ui.label(t("Angle to true north"));
            let mut turn = d.specify_angle.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut turn, t("Set it too"));
                let mut a = d.specify_angle.unwrap_or(d.active_location().angle);
                if turn {
                    angle_field(ui, &mut a, t("Specify angle").to_string());
                }
                d.specify_angle = turn.then_some(a);
            });
            ui.end_row();
        });
        ui.add_space(4.0);
        if ui.button(t("Specify")).clicked() {
            d.specify();
        }
    });
}

/// The map system: the one picked, a search of EPSG's, and the Survey Point placed by latitude and longitude.
fn map_tab(ui: &mut Ui, d: &mut CoordinatesDialog) {
    note(
        ui,
        msg!(
            "The projected coordinate system the shared coordinates are a grid of: a UTM zone, a State Plane zone, a national grid. It shows them in its unit, and takes them to latitude and longitude for the map and the sun."
        ),
    );
    section(ui, msg!("Map system"), |ui| system_picker(ui, d));
    section(ui, msg!("Place by latitude and longitude"), |ui| {
        note(
            ui,
            msg!("Where the Survey Point is on the globe, in WGS84 degrees: the active location's east and north are worked out from it."),
        );
        world_map(ui, d);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (k, label, range) in [(0, msg!("Latitude"), 90.0), (1, msg!("Longitude"), 180.0)] {
                let l = ui.label(t(label));
                let r = ui.add(DragValue::new(&mut d.lat_lon[k]).speed(0.0001).range(-range..=range).fixed_decimals(7).suffix("°"));
                r.labelled_by(l.id);
                ui.add_space(8.0);
            }
            if ui.add_enabled(d.crs.is_some(), Button::new(t("Place"))).clicked() {
                d.error = d.place().err();
            }
        });
    });
}

/// A system the search found, as a row of the list: its code, its name, and its unit (or why it's offered), in columns;
/// picked by a click. Under a place, `place` names it for screen readers and it sits indented.
fn found_row(ui: &mut Ui, i: &Info, on: bool, place: Option<&str>, why: Option<&str>, first: bool) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let label = format!("EPSG:{} {}, {}", i.code, i.name, i.unit);
    let label = place.map_or_else(|| label.clone(), |p| format!("{p}: {label}"));
    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, label.clone()));
    let p = ui.painter();
    if on || r.hovered() {
        p.rect_filled(rect, 4.0, if on { pal().select_fill } else { pal().hover_fill });
    }
    let y = rect.center().y;
    let left = rect.left() + if place.is_some() { 26.0 } else { 8.0 };
    let font = egui::FontId::proportional(13.0);
    let code = if on { pal().accent_ink } else { pal().accent };
    p.text(pos2(left, y), egui::Align2::LEFT_CENTER, format!("EPSG:{}", i.code), egui::FontId::monospace(12.5), code);
    let name = p.text(pos2(left + 96.0, y), egui::Align2::LEFT_CENTER, &i.name, font.clone(), pal().text);
    if first {
        // The one to pick: a small tag after its name.
        let tag = p.layout_no_wrap(t("Recommended").to_string(), egui::FontId::proportional(11.0), pal().accent_ink);
        let at = egui::Rect::from_min_size(pos2(name.right() + 8.0, y - tag.size().y / 2.0 - 1.0), tag.size() + vec2(12.0, 2.0));
        p.rect_filled(at, 8.0, pal().select_fill);
        p.galley(at.min + vec2(6.0, 1.0), tag, pal().accent_ink);
    }
    let right = match why {
        Some(w) => format!("{} · {}", t(w), unit_symbol(&i.unit)),
        None => unit_symbol(&i.unit),
    };
    p.text(pos2(rect.right() - 8.0, y), egui::Align2::RIGHT_CENTER, right, font, pal().muted);
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A place the search found, as a heading over its systems: a pin, its name, where it is.
fn place_row(ui: &mut Ui, p: &Place) {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let label = p.label();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label.clone()));
    let painter = ui.painter();
    let y = rect.center().y + 1.0;
    // A pin: a dot with a ring, as on the map.
    let c = pos2(rect.left() + 13.0, y);
    painter.circle(c, 4.5, pal().accent, Stroke::new(1.5, pal().surface));
    let name = painter.text(pos2(rect.left() + 26.0, y), egui::Align2::LEFT_CENTER, &p.name, egui::FontId::proportional(13.5), pal().text);
    let mut rest: Vec<&str> = Vec::new();
    if p.kind == PlaceKind::City {
        if !p.region.is_empty() && p.region != p.name {
            rest.push(&p.region);
        }
        rest.push(&p.country);
    }
    let rest = if p.kind == PlaceKind::Country { t("Country").to_string() } else { rest.join(", ") };
    painter.text(pos2(name.right() + 8.0, y), egui::Align2::LEFT_CENTER, rest, egui::FontId::proportional(12.0), pal().muted);
    let deg = format!("{:.3}°, {:.3}°", p.lat, p.lon);
    painter.text(pos2(rect.right() - 8.0, y), egui::Align2::RIGHT_CENTER, deg, egui::FontId::monospace(11.0), pal().muted);
}

/// The map system as a dropdown: the one picked (or none) on a button, which opens a search of places and of EPSG's
/// systems. A place lists the systems its surveys use under it, the one to pick first tagged; a system picked under a place
/// takes the place too, for Place. Clear beside it takes the map system away.
fn system_picker(ui: &mut Ui, d: &mut CoordinatesDialog) {
    let info = d.info();
    let mut button = egui::Rect::NOTHING;
    let r = ui.horizontal(|ui| {
        let clear = 70.0;
        let (rect, r) = ui.allocate_exact_size(vec2((ui.available_width() - clear).max(200.0), 40.0), Sense::click());
        button = rect;
        r.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Choose a map system")));
        let open = egui::Popup::is_id_open(ui.ctx(), UiId::new("coordinates-system-list"));
        let p = ui.painter();
        let fill = if r.hovered() || open { pal().hover_fill } else { pal().surface };
        p.rect_filled(rect, 6.0, fill);
        p.rect_stroke(rect, 6.0, Stroke::new(1.0, if open { pal().accent } else { pal().border }), egui::StrokeKind::Inside);
        let y = rect.center().y;
        match &info {
            Some(i) => {
                let code = p.layout_no_wrap(format!("EPSG:{}", i.code), egui::FontId::proportional(12.0), pal().accent_ink);
                let chip = egui::Rect::from_min_size(pos2(rect.left() + 10.0, y - 10.0), vec2(code.size().x + 16.0, 20.0));
                p.rect_filled(chip, 9.0, pal().select_fill);
                p.galley(pos2(chip.left() + 8.0, y - code.size().y / 2.0), code, pal().accent_ink);
                let name =
                    p.text(pos2(chip.right() + 10.0, y), egui::Align2::LEFT_CENTER, &i.name, egui::FontId::proportional(15.0), pal().text);
                let unit = tf("Unit: {}", &[&i.unit]);
                p.text(pos2(name.right() + 12.0, y), egui::Align2::LEFT_CENTER, unit, egui::FontId::proportional(12.5), pal().muted);
            }
            None => {
                let none = t("None: shared coordinates in metres, and no latitude or longitude. Choose one…");
                p.text(pos2(rect.left() + 12.0, y), egui::Align2::LEFT_CENTER, none, egui::FontId::proportional(13.0), pal().muted);
            }
        }
        let c = pos2(rect.right() - 16.0, y);
        let caret = if open {
            [c + vec2(-4.0, 2.0), c + vec2(4.0, 2.0), c + vec2(0.0, -3.0)]
        } else {
            [c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
        };
        p.add(egui::Shape::convex_polygon(caret.to_vec(), pal().icon, Stroke::NONE));
        if r.has_focus() {
            p.rect_stroke(rect, 6.0, crate::style::focus_ring(), egui::StrokeKind::Inside);
        }
        if ui
            .add_enabled(info.is_some(), Button::new(t("Clear")))
            .on_hover_text(t("No map system: shared coordinates in metres, and no latitude or longitude"))
            .clicked()
        {
            d.crs = None;
        }
        r.on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    let r = r.inner;
    let id = UiId::new("coordinates-system-list");
    let opened = r.clicked() && !egui::Popup::is_id_open(ui.ctx(), id);
    let mut picked: Option<(u32, Option<Place>)> = None;
    egui::Popup::from_toggle_button_response(&r)
        .id(id)
        .width(button.width())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(button.width() - 16.0);
            let s = ui.add(
                TextEdit::singleline(&mut d.search)
                    .hint_text(t("A city, a country, an EPSG code or words of a name: São Paulo, Germany, 31983, UTM 23S"))
                    .desired_width(f32::INFINITY),
            );
            s.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, t("Find a map system")));
            if opened {
                s.request_focus();
            }
            d.find();
            ui.add_space(4.0);
            ScrollArea::vertical()
                .id_salt("coordinates-found")
                .max_height(360.0)
                .min_scrolled_height(360.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    if d.searched.is_empty() {
                        ui.label(RichText::new(t("Type where the project is, or the system's name or code.")).color(pal().muted));
                    } else if d.found.is_empty() && d.places.is_empty() {
                        ui.label(RichText::new(t("No place, and no projected system of EPSG's, matches.")).color(pal().muted));
                    }
                    if !d.places.is_empty() {
                        group_head(ui, msg!("Places"));
                        for (place, systems) in &d.places {
                            ui.add_space(2.0);
                            place_row(ui, place);
                            for (k, s) in systems.iter().enumerate() {
                                if found_row(ui, &s.info, d.crs == Some(s.info.code), Some(&place.name), Some(s.why), k == 0) {
                                    picked = Some((s.info.code, Some(place.clone())));
                                }
                            }
                        }
                    }
                    if !d.found.is_empty() {
                        group_head(ui, msg!("Systems"));
                        for i in &d.found {
                            if found_row(ui, i, d.crs == Some(i.code), None, None, false) {
                                picked = Some((i.code, None));
                            }
                        }
                    }
                });
        });
    if let Some((code, place)) = picked {
        match place {
            Some(p) => d.pick_for(code, &p),
            None => d.crs = Some(code),
        }
        egui::Popup::close_id(ui.ctx(), id);
    }
}

/// A heading in the dropdown's list: Places, Systems.
fn group_head(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(t(title).to_uppercase()).size(11.0).strong().color(pal().muted));
    ui.add_space(2.0);
}

/// The world map, offline: Natural Earth's coastlines and borders on a plain latitude-longitude grid, every location a pin
/// (the active one in the accent), and the latitude and longitude typed as a cross. Dragged to pan, scrolled to zoom, and a
/// click puts the latitude and longitude there, for Place. A placeholder until a tile map.
fn world_map(ui: &mut Ui, d: &mut CoordinatesDialog) {
    let size = vec2(ui.available_width(), 300.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click_and_drag());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, t("World map")));
    if d.map_zoom < 0.0 {
        // A place picked: so many degrees across.
        d.map_zoom = rect.width() / -d.map_zoom;
    } else if d.map_zoom == 0.0 {
        // Fitted to show the whole world, or a continent round a place already set.
        d.map_zoom = if d.crs.is_some() { rect.width() / 60.0 } else { rect.width() / 360.0 };
    }
    let fit = rect.width() / 360.0;
    if r.dragged() {
        let dv = r.drag_delta() / d.map_zoom;
        d.map_at = ((d.map_at.0 - dv.x).clamp(-180.0, 180.0), (d.map_at.1 + dv.y).clamp(-90.0, 90.0));
    }
    let to_px = |at: (f32, f32), zoom: f32, (lon, lat): (f32, f32)| {
        pos2(rect.center().x + (lon - at.0) * zoom, rect.center().y - (lat - at.1) * zoom)
    };
    let from_px = |at: (f32, f32), zoom: f32, p: egui::Pos2| (at.0 + (p.x - rect.center().x) / zoom, at.1 - (p.y - rect.center().y) / zoom);
    if let Some(hover) = r.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            // Zoomed about the cursor: the place under it stays under it.
            let under = from_px(d.map_at, d.map_zoom, hover);
            let zoom = (d.map_zoom * (scroll / 200.0).exp()).clamp(fit, fit * 400.0);
            d.map_at = (under.0 - (hover.x - rect.center().x) / zoom, under.1 + (hover.y - rect.center().y) / zoom);
            d.map_zoom = zoom;
        }
    }
    let (at, zoom) = (d.map_at, d.map_zoom);
    let p = ui.painter_at(rect);
    let sea = if crate::style::dark() { pal().surface.gamma_multiply(0.9) } else { egui::Color32::from_rgb(0xEE, 0xF4, 0xFA) };
    p.rect_filled(rect, CornerRadius::same(6), sea);
    // The graticule: every 30°, the equator a little darker.
    for k in -6..=6 {
        let deg = k as f32 * 30.0;
        let ink = Stroke::new(if k == 0 { 1.0 } else { 0.5 }, pal().border);
        let x = to_px(at, zoom, (deg, 0.0)).x;
        p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], ink);
        if deg.abs() <= 90.0 {
            let y = to_px(at, zoom, (0.0, deg)).y;
            p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], ink);
        }
    }
    let (coast, border) = (Stroke::new(1.1, pal().muted), Stroke::new(0.6, pal().border));
    for l in bimcraft_core::world::lines() {
        let pts: Vec<egui::Pos2> = l.points.iter().map(|q| to_px(at, zoom, *q)).collect();
        let bounds = egui::Rect::from_points(&pts);
        if !bounds.intersects(rect) {
            continue;
        }
        let stroke = if l.land { coast } else { border };
        if l.land {
            p.add(egui::Shape::closed_line(pts, stroke));
        } else {
            p.add(egui::Shape::line(pts, stroke));
        }
    }
    // The locations, as pins.
    if let Some(code) = d.crs {
        for (i, row) in d.rows.iter().enumerate() {
            let Ok((lon, lat)) = crs::to_wgs84(code, row.at.east, row.at.north) else { continue };
            let c = to_px(at, zoom, (lon as f32, lat as f32));
            if !rect.contains(c) {
                continue;
            }
            let ink = if i == d.active { pal().accent } else { pal().muted };
            p.circle(c, 5.5, ink, Stroke::new(1.5, pal().surface));
            p.text(c + vec2(8.0, -1.0), egui::Align2::LEFT_CENTER, &row.name, egui::FontId::proportional(11.5), pal().text);
        }
    }
    // The latitude and longitude typed: a cross.
    let typed = to_px(at, zoom, (d.lat_lon[1] as f32, d.lat_lon[0] as f32));
    if rect.contains(typed) {
        let s = Stroke::new(1.5, pal().error);
        p.line_segment([typed - vec2(6.0, 0.0), typed + vec2(6.0, 0.0)], s);
        p.line_segment([typed - vec2(0.0, 6.0), typed + vec2(0.0, 6.0)], s);
    }
    p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
    // What a click would set, and whose map it is.
    let credit = "Natural Earth";
    p.text(rect.right_bottom() - vec2(6.0, 4.0), egui::Align2::RIGHT_BOTTOM, credit, egui::FontId::proportional(10.0), pal().muted);
    if let Some(hover) = r.hover_pos() {
        let (lon, lat) = from_px(at, zoom, hover);
        let text = format!("{lat:.4}°, {lon:.4}°");
        p.text(rect.left_bottom() + vec2(6.0, -4.0), egui::Align2::LEFT_BOTTOM, text, egui::FontId::monospace(11.0), pal().text);
    }
    if r.clicked()
        && let Some(click) = r.interact_pointer_pos()
    {
        let (lon, lat) = from_px(at, zoom, click);
        d.lat_lon = [f64::from(lat.clamp(-90.0, 90.0)), f64::from(lon.clamp(-180.0, 180.0))];
    }
}

/// The converter: a point from one frame to another, through the draft.
fn convert_tab(ui: &mut Ui, d: &mut CoordinatesDialog) {
    note(ui, msg!("A point in one frame, in another: the file's own, from the Project Base Point, shared, or WGS84. Lengths in mm."));
    let combo = |ui: &mut Ui, salt: &str, label: &str, value: &mut Frame| {
        let l = ui.label(t(label));
        let shown = FRAMES.iter().find(|(f, _)| f == value).map_or("", |(_, n)| t(n));
        let c = ComboBox::from_id_salt(salt).selected_text(shown).width(220.0).show_ui(ui, |ui| {
            for (f, n) in FRAMES {
                ui.selectable_value(value, f, t(n));
            }
        });
        c.response.labelled_by(l.id);
    };
    ui.horizontal(|ui| {
        combo(ui, "coordinates-from", msg!("From"), &mut d.from);
        ui.add_space(12.0);
        if ui.button("⇄").on_hover_text(t("Swap")).clicked() {
            std::mem::swap(&mut d.from, &mut d.to);
        }
        ui.add_space(12.0);
        combo(ui, "coordinates-to", msg!("To"), &mut d.to);
    });
    ui.add_space(6.0);
    let axes = if d.from == Frame::Wgs84 {
        [msg!("Longitude"), msg!("Latitude"), msg!("Elevation")]
    } else {
        [msg!("X / East"), msg!("Y / North"), msg!("Z / Elevation")]
    };
    ui.horizontal(|ui| {
        for (k, label) in axes.iter().enumerate() {
            let l = ui.label(t(label));
            let r = ui.add(DragValue::new(&mut d.point[k]).speed(1.0).max_decimals(7));
            r.labelled_by(l.id);
            ui.add_space(8.0);
        }
    });
    ui.add_space(8.0);
    let result = d.draft().convert(d.point, d.from, d.to);
    UiFrame::new().fill(pal().bar).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match result {
            Ok(q) => {
                let text = if d.to == Frame::Wgs84 {
                    format!("{:.8}°, {:.8}°, {:.3}", q[0], q[1], q[2])
                } else {
                    format!("{:.3}, {:.3}, {:.3}", q[0], q[1], q[2])
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&text).monospace().size(16.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t("Copy")).clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                    });
                });
            }
            Err(e) => {
                ui.label(RichText::new(e).color(pal().muted));
            }
        }
    });
}

impl BimApp {
    /// The Coordinates window, while it's open.
    pub(crate) fn coordinates_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.coordinates_dialog.take() else { return };
        let units = self.session.model.units;
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("coordinates"), t("Coordinates")).width(980.0).min_height(560.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                summary(ui, &mut d);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (i, label) in TABS.iter().enumerate() {
                        if ui.selectable_label(d.tab == i, t(label)).clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.separator();
                match d.tab {
                    0 => locations_tab(ui, &mut d),
                    1 => points_tab(ui, &mut d, units),
                    2 => map_tab(ui, &mut d),
                    _ => convert_tab(ui, &mut d),
                }
                if let Some(error) = &d.error {
                    ui.add_space(6.0);
                    boxed_text(ui, "coordinates-error", pal().error, error, ERROR_HEIGHT);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        let failed = (ok || apply) && !self.apply_coordinates(&mut d, apply);
        if failed || !(ok || cancel || r.should_close()) {
            self.coordinates_dialog = Some(d);
        }
    }

    /// OK and Apply: the draft, as one undo step, all or nothing; a refusal keeps the window open, saying why in itself.
    fn apply_coordinates(&mut self, d: &mut CoordinatesDialog, apply: bool) -> bool {
        d.error = None;
        let done = d.commands(&self.session.model).and_then(|commands| {
            if commands.is_empty() {
                return Ok(());
            }
            self.session.transaction("Coordinates", |s: &mut Session| commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ())))
        });
        if let Err(e) = done {
            d.error = Some(e.clone());
            self.status = Status { text: e, error: true };
            return false;
        }
        self.status = Status::default();
        if apply {
            let tab = d.tab;
            *d = CoordinatesDialog::new(&self.session.model);
            d.tab = tab;
        }
        true
    }

    /// Rotate Project North, from the ribbon: by the active location's angle (Align with True North), or by `angle`.
    pub(crate) fn rotate_project_north(&mut self, angle: Option<f64>) {
        let p = match angle {
            Some(a) => json!({"angle": a}),
            None => json!({"true_north": true}),
        };
        self.run("rotate_project_north", p);
    }
}
