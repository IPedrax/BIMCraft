//! The Site tab: where the project is on Earth (Location), the ground (Terrain from Elevation Data, Pad, Place on Terrain, Cut/Fill)
//! and what stands round it (Import from OpenStreetMap).
//!
//! Every button runs a command. The two that go to the network (`fetch_osm`, `fetch_terrain`) do it on a thread of their own, as
//! reading a Revit file does: the thread asks the fetcher the session holds, and what it brings is handed back to the unchanged
//! command through [`Prefetched`], so one validation, one undo step and one report, and the window never waits on the network.

mod map;

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::geo::{self, Bbox, Elevation, Layers, Prefetched, Reply};
use bimcraft_core::geom::P2;
use bimcraft_core::georef::parse_coordinates;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model, Pad};
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, Response, RichText, ScrollArea, TextEdit, Ui};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::properties::number;
use crate::style::pal;
use crate::{BimApp, Icon, Status, big_button, panel};

/// What a fetch is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Osm,
    Terrain,
}

/// What a fetch is made of: what for, over which box, the command's parameters, and (for heights) the spacing and source asked for.
type Built = (Kind, Bbox, Value, Option<(f64, Option<String>)>);

/// What a fetch brought, from its thread.
enum Fetched {
    Osm { query: String, reply: Reply },
    Heights { spacing: f64, source: Option<String>, got: Elevation },
}

/// A fetch running: for what, over which box, with which command's parameters, and its answer when it comes.
struct Job {
    kind: Kind,
    bbox: Bbox,
    params: Value,
    /// The Overpass query it asks with, for a fetch of buildings.
    query: Option<String>,
    answer: Receiver<Result<Fetched, String>>,
}

/// The Location box: where the project is on Earth, as typed.
pub(crate) struct LocationBox {
    /// A pair pasted from a map, `51.2254809, 6.7700762`: it fills the two fields under it.
    coordinates: String,
    /// What is wrong with it, while it is not a pair yet.
    coordinates_note: Option<String>,
    latitude: String,
    longitude: String,
    /// The ground height at the origin, in the project's units.
    elevation: String,
    true_north: String,
    /// `utm` (the place's zone) or an EPSG code.
    crs: String,
    /// What the project's grid is, once placed.
    grid: Option<String>,
    error: Option<String>,
}

/// The box for Import from OpenStreetMap and for Terrain from Elevation Data: the area, and what the command takes.
pub(crate) struct AreaBox {
    kind: Kind,
    /// A distance round the origin, or a box typed.
    around: bool,
    radius: String,
    south: String,
    west: String,
    north: String,
    east: String,
    // Buildings.
    parts: bool,
    default_height: String,
    level_height: String,
    // Terrain.
    spacing: String,
    source: usize,
    interval: String,
    set_elevation: bool,
    error: Option<String>,
}

/// What a fetch or a calculation says when it is done: kept in a box until it is closed.
pub(crate) struct Said {
    title: String,
    lines: Vec<String>,
    credit: Option<String>,
    /// The Overpass query a fetch asked with.
    query: Option<String>,
}

/// The Site tab's state.
#[derive(Default)]
pub(crate) struct Site {
    location: Option<LocationBox>,
    area: Option<AreaBox>,
    job: Option<Job>,
    said: Option<Said>,
    /// The map a place or an area is picked on.
    map: Option<map::MapBox>,
}

/// The elevation sources the Terrain box offers: how they are named to the command, and shown.
const SOURCES: [(&str, &str); 3] = [("", "Automatic"), ("terrarium", "Terrarium"), ("copernicus", "Copernicus GLO-30")];

/// A pad over `ring` on `level`, as the Pad tool's preview draws it: no name, 0 above the level, the default thickness.
pub(crate) fn pad_sketch(m: &Model, level: Id, ring: &[P2]) -> Pad {
    Pad { name: String::new(), level: m.guid(level), boundary: ring.to_vec(), offset: 0.0, thickness: 300.0, params: Default::default() }
}

/// A query shown as it goes out: monospace, selectable, with a button that copies it.
fn query_view(ui: &mut Ui, query: &str) {
    ScrollArea::vertical().id_salt("site-query-text").max_height(120.0).show(ui, |ui| {
        ui.label(RichText::new(query).monospace().size(11.0));
    });
    if ui.small_button(t("Copy")).clicked() {
        ui.ctx().copy_text(query.to_string());
    }
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) -> Response {
    let l = ui.label(t(label));
    let r = ui.add_sized([220.0, 20.0], TextEdit::singleline(value).hint_text(hint)).labelled_by(l.id);
    ui.end_row();
    r
}

/// A number typed, or what to say about it.
fn typed(text: &str, what: &str) -> Result<f64, String> {
    number(text).ok_or_else(|| tf("{}: `{}` is not a number", &[t(what), text.trim()]))
}

impl BimApp {
    /// Whether a fetch is running.
    pub fn fetching(&self) -> bool {
        self.site.job.is_some()
    }

    /// The Site tab.
    pub(crate) fn site_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", crate::Tool::Modify)]);
        panel(ui, msg!("Project Location"), |ui| {
            let tip = msg!(
                "Location: where the project's origin is on Earth: latitude, longitude, ground height, true north and the grid it is in"
            );
            if big_button(ui, Icon::Location, "Location", self.site.location.is_some(), tip).clicked() {
                self.open_location();
            }
        });
        panel(ui, msg!("Terrain"), |ui| {
            let tip = msg!("Terrain from Elevation Data: the ground round the origin, fetched from an elevation service, as a toposurface");
            if big_button(
                ui,
                Icon::Terrain,
                "Terrain from Elevation Data",
                self.site.area.as_ref().is_some_and(|a| a.kind == Kind::Terrain),
                tip,
            )
            .clicked()
            {
                self.open_area(Kind::Terrain);
            }
            let tip = msg!("Pad: click the corners of a building pad's outline; Enter, or the first corner again, finishes; Esc cancels");
            if big_button(ui, Icon::Pad, "Pad", crate::structure::same_tool(&self.tool, &crate::Tool::Pad(Vec::new())), tip).clicked() {
                self.set_tool(crate::Tool::Pad(Vec::new()));
            }
            let tip =
                msg!("Place on Terrain: stands the selected masses, columns, footings, components and walls on the ground under them");
            if big_button(ui, Icon::PlaceOnTerrain, "Place on Terrain", false, tip).clicked() {
                self.place_on_terrain();
            }
            let tip = msg!("Cut/Fill: the earth the selected pads take out of the terrain and put in (all the pads when none is selected)");
            if big_button(ui, Icon::CutFill, "Cut/Fill", false, tip).clicked() {
                self.cut_fill();
            }
        });
        panel(ui, msg!("Context"), |ui| {
            let tip = msg!("Import from OpenStreetMap: the buildings round the origin, fetched from Overpass, as masses on the ground");
            if big_button(ui, Icon::FromMap, "Import from OpenStreetMap", self.site.area.as_ref().is_some_and(|a| a.kind == Kind::Osm), tip)
                .clicked()
            {
                self.open_area(Kind::Osm);
            }
        });
    }

    // ---- Location ----

    fn open_location(&mut self) {
        let units = self.session.model.units;
        let b = match self.session.model.location {
            Some(l) => LocationBox {
                coordinates: String::new(),
                coordinates_note: None,
                latitude: l.latitude.to_string(),
                longitude: l.longitude.to_string(),
                elevation: units.length(l.elevation),
                true_north: l.true_north.to_string(),
                crs: l.crs.map_or_else(|| "utm".to_string(), |c| c.to_string()),
                grid: l.grid().ok().map(|g| format!("{} (EPSG:{})", g.name, g.epsg)),
                error: None,
            },
            None => LocationBox {
                coordinates: String::new(),
                coordinates_note: None,
                latitude: String::new(),
                longitude: String::new(),
                elevation: units.length(0.0),
                true_north: "0".into(),
                crs: "utm".into(),
                grid: None,
                error: None,
            },
        };
        self.site.location = Some(b);
    }

    fn location_window(&mut self, ctx: &Context) {
        let Some(b) = self.site.location.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let mut pick = None;
        let unit = self.session.model.units.name();
        let r = Modal::new(UiId::new("site-location"), t("Location")).width(440.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Where the project's origin is on Earth. Everything fetched is placed from it."));
                ui.add_space(6.0);
                Grid::new("site-location-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    // A pair copied from a map goes in the first row, or straight into either of the next two.
                    if text_field(ui, "Coordinates", &mut b.coordinates, "51.2254809, 6.7700762").changed() {
                        b.coordinates_note = None;
                        if !b.coordinates.trim().is_empty() {
                            match parse_coordinates(&b.coordinates) {
                                Ok((latitude, longitude)) => (b.latitude, b.longitude) = (latitude.to_string(), longitude.to_string()),
                                Err(e) => b.coordinates_note = Some(e),
                            }
                        }
                    }
                    let pasted = text_field(ui, "Latitude (°)", &mut b.latitude, "-23.5505").changed();
                    let pasted = text_field(ui, "Longitude (°)", &mut b.longitude, "-46.6333").changed() || pasted;
                    if pasted {
                        let both = parse_coordinates(&b.latitude).or_else(|_| parse_coordinates(&b.longitude));
                        if let Ok((latitude, longitude)) = both {
                            (b.latitude, b.longitude) = (latitude.to_string(), longitude.to_string());
                        }
                    }
                    text_field(ui, "Ground height", &mut b.elevation, unit);
                    text_field(ui, "True north (°)", &mut b.true_north, "0");
                    text_field(ui, "Grid", &mut b.crs, "utm");
                    ui.end_row();
                });
                if let Some(note) = &b.coordinates_note {
                    ui.label(RichText::new(note).color(pal().muted));
                }
                if ui.button(t("Pick on Map…")).on_hover_text(t("Click the project's origin on a map")).clicked() {
                    pick = Some(map::Pick::Place);
                }
                ui.label(RichText::new(t("Grid: utm, for the place's own zone, or an EPSG code such as 31983.")).color(pal().muted));
                if let Some(g) = &b.grid {
                    ui.label(RichText::new(tf("In {}", &[g])).color(pal().muted));
                }
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(!b.latitude.trim().is_empty() && !b.longitude.trim().is_empty(), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            self.apply_location();
        } else if close || r.should_close() {
            self.site.location = None;
        } else if let Some(pick) = pick {
            self.open_map(pick);
        }
    }

    /// Runs `geolocate` for what the box says. A refusal stays in the box.
    fn apply_location(&mut self) {
        let Some(b) = self.site.location.as_ref() else { return };
        let units = self.session.model.units;
        let built = (|| -> Result<Value, String> {
            let elevation = units.parse(&b.elevation).ok_or_else(|| tf("Ground height: `{}` is not a length", &[b.elevation.trim()]))?;
            let crs = b.crs.trim();
            Ok(json!({
                "latitude": typed(&b.latitude, "Latitude (°)")?,
                "longitude": typed(&b.longitude, "Longitude (°)")?,
                "elevation": elevation,
                "true_north": typed(&b.true_north, "True north (°)")?,
                "crs": if crs.is_empty() { "utm" } else { crs },
            }))
        })();
        let params = match built {
            Ok(p) => p,
            Err(e) => {
                if let Some(b) = self.site.location.as_mut() {
                    b.error = Some(e);
                }
                return;
            }
        };
        match self.run("geolocate", params) {
            Some(_) => self.site.location = None,
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(b) = self.site.location.as_mut() {
                    b.error = Some(e);
                }
            }
        }
    }

    // ---- The area to fetch ----

    fn open_area(&mut self, kind: Kind) {
        let units = self.session.model.units;
        self.site.area = Some(AreaBox {
            kind,
            around: true,
            radius: "500".into(),
            south: String::new(),
            west: String::new(),
            north: String::new(),
            east: String::new(),
            parts: false,
            default_height: units.length(6000.0),
            level_height: units.length(3000.0),
            spacing: String::new(),
            source: 0,
            interval: units.length(1000.0),
            set_elevation: true,
            error: None,
        });
    }

    /// The box the area says: a distance round the origin, or four typed degrees; and small enough.
    fn area_bbox(&self, b: &AreaBox) -> Result<Bbox, String> {
        let bbox = if b.around {
            let Some(l) = self.session.model.location else {
                return Err(t("The project has no location yet: set it with Location first.").to_string());
            };
            let metres = typed(&b.radius, "Distance round the origin (m)")?;
            Bbox::around(l.longitude, l.latitude, metres)?
        } else {
            Bbox::new(
                typed(&b.south, "South (°)")?,
                typed(&b.west, "West (°)")?,
                typed(&b.north, "North (°)")?,
                typed(&b.east, "East (°)")?,
            )?
        };
        bbox.check_area()?;
        Ok(bbox)
    }

    fn area_window(&mut self, ctx: &Context) {
        // While a fetch runs, its own box is the only one: the area box is back, with what went wrong, if it did.
        if self.site.job.is_some() {
            return;
        }
        let Some(b) = self.site.area.as_ref() else { return };
        let (title, kind) = match b.kind {
            Kind::Osm => (t("Import from OpenStreetMap"), Kind::Osm),
            Kind::Terrain => (t("Terrain from Elevation Data"), Kind::Terrain),
        };
        let offline = self.session.geo.is_none();
        let area = self.area_bbox(b);
        let live = area.as_ref().map(|x| x.area_km2()).map_err(Clone::clone);
        let area = area.ok();
        let unit = self.session.model.units.name();
        let Some(b) = self.site.area.as_mut() else { return };
        let mut pick = None;
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("site-area"), title).width(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                if offline {
                    ui.label(
                        RichText::new(t(
                            "This BIMCraft has no network access (BIMCRAFT_NO_NETWORK is set, or it runs in a sandbox that has none).",
                        ))
                        .color(pal().error),
                    );
                    ui.add_space(6.0);
                }
                ui.radio_value(&mut b.around, true, t("A distance round the project's origin"));
                ui.radio_value(&mut b.around, false, t("A box of degrees"));
                if ui.button(t("Pick on Map…")).on_hover_text(t("Draw the box on a map")).clicked() {
                    pick = Some(map::Pick::Area);
                }
                ui.add_space(4.0);
                Grid::new("site-area-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    if b.around {
                        text_field(ui, "Distance round the origin (m)", &mut b.radius, "500");
                    } else {
                        text_field(ui, "South (°)", &mut b.south, "-23.56");
                        text_field(ui, "West (°)", &mut b.west, "-46.64");
                        text_field(ui, "North (°)", &mut b.north, "-23.54");
                        text_field(ui, "East (°)", &mut b.east, "-46.62");
                    }
                    match kind {
                        Kind::Osm => {
                            text_field(ui, "Height of a building with none", &mut b.default_height, unit);
                            text_field(ui, "Height of a level", &mut b.level_height, unit);
                        }
                        Kind::Terrain => {
                            let default = b.spacing.is_empty();
                            text_field(ui, "Spacing (m)", &mut b.spacing, if default { "automatic" } else { "" });
                            let l = ui.label(t("Elevation data"));
                            let c = egui::ComboBox::from_id_salt("site-source").selected_text(t(SOURCES[b.source].1)).show_ui(ui, |ui| {
                                for (i, (_, name)) in SOURCES.iter().enumerate() {
                                    ui.selectable_value(&mut b.source, i, t(name));
                                }
                            });
                            c.response.labelled_by(l.id);
                            ui.end_row();
                            text_field(ui, "Contour interval", &mut b.interval, unit);
                        }
                    }
                });
                match kind {
                    Kind::Osm => {
                        ui.checkbox(&mut b.parts, t("Parts of buildings too"));
                    }
                    Kind::Terrain => {
                        ui.checkbox(&mut b.set_elevation, t("Take the project's ground height from the data, when it has none"));
                    }
                }
                ui.add_space(6.0);
                match &live {
                    Ok(km2) => ui.label(
                        RichText::new(tf("{} km² (at most {})", &[&format!("{km2:.2}"), &geo::MAX_FETCH_AREA_KM2.to_string()]))
                            .color(pal().muted),
                    ),
                    Err(e) => ui.label(RichText::new(e).color(pal().muted)),
                };
                // What will be asked of Overpass, as it will be asked: it follows the area and the check above.
                if kind == Kind::Osm {
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new(t("Overpass query")).id_salt("site-query").default_open(true).show(ui, |ui| match area {
                        Some(x) => query_view(ui, &geo::overpass_query(&x, Layers { parts: b.parts })),
                        None => {
                            ui.label(RichText::new(t("Give the area first: the query is made from its box.")).color(pal().muted));
                        }
                    });
                }
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(!offline && live.is_ok(), Button::new(t("Fetch"))).clicked();
                });
            });
        });
        if ok {
            self.start_fetch(ctx);
        } else if close || r.should_close() {
            self.site.area = None;
        } else if let Some(pick) = pick {
            self.open_map(pick);
        }
    }

    // ---- The fetch ----

    /// Starts the fetch the area box describes, on a thread; the box stays until it is over (its refusals appear in it).
    fn start_fetch(&mut self, ctx: &Context) {
        let Some(b) = self.site.area.as_ref() else { return };
        let units = self.session.model.units;
        let built = (|| -> Result<Built, String> {
            let bbox = self.area_bbox(b)?;
            let mut params = json!({"bbox": bbox.as_array()});
            match b.kind {
                Kind::Osm => {
                    params["parts"] = json!(b.parts);
                    if !b.default_height.trim().is_empty() {
                        params["default_height"] = json!(
                            units
                                .parse(&b.default_height)
                                .ok_or_else(|| tf("Height of a building with none: `{}` is not a length", &[b.default_height.trim()]))?
                        );
                    }
                    if !b.level_height.trim().is_empty() {
                        params["level_height"] = json!(
                            units
                                .parse(&b.level_height)
                                .ok_or_else(|| tf("Height of a level: `{}` is not a length", &[b.level_height.trim()]))?
                        );
                    }
                    Ok((Kind::Osm, bbox, params, None))
                }
                Kind::Terrain => {
                    let spacing = if b.spacing.trim().is_empty() { geo::default_spacing(&bbox) } else { typed(&b.spacing, "Spacing (m)")? };
                    let source = Some(SOURCES[b.source].0).filter(|s| !s.is_empty()).map(str::to_string);
                    params["spacing"] = json!(spacing);
                    if let Some(s) = &source {
                        params["source"] = json!(s);
                    }
                    params["set_elevation"] = json!(b.set_elevation);
                    if !b.interval.trim().is_empty() {
                        params["interval"] = json!(
                            units.parse(&b.interval).ok_or_else(|| tf("Contour interval: `{}` is not a length", &[b.interval.trim()]))?
                        );
                    }
                    Ok((Kind::Terrain, bbox, params, Some((spacing, source))))
                }
            }
        })();
        let (kind, bbox, params, heights) = match built {
            Ok(x) => x,
            Err(e) => return self.area_error(e),
        };
        // The commands' own checks, before anyone waits for the network: no location, a spacing too fine, a bad height.
        if self.session.model.location.is_none() {
            return self.area_error(t("The project has no location yet: set it with Location first.").to_string());
        }
        let Some(geo) = self.session.geo.clone() else {
            return self.area_error(t("This BIMCraft has no network access.").to_string());
        };
        if self.site.job.is_some() {
            return;
        }
        let (send, answer) = channel();
        let layers = Layers { parts: params["parts"].as_bool().unwrap_or(false) };
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("site-fetch".into()).spawn(move || {
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match heights {
                None => {
                    let query = geo::overpass_query(&bbox, layers);
                    geo.overpass(&bbox, &query).map(|reply| Fetched::Osm { query, reply })
                }
                Some((spacing, source)) => {
                    geo.elevation(&bbox, spacing, source.as_deref()).map(|got| Fetched::Heights { spacing, source, got })
                }
            }))
            .unwrap_or_else(|_| Err("the fetcher stopped on this request".to_string()));
            let _ = send.send(got);
            wake.request_repaint();
        });
        if let Err(e) = spawned {
            return self.area_error(e.to_string());
        }
        let query = (kind == Kind::Osm).then(|| geo::overpass_query(&bbox, layers));
        self.site.job = Some(Job { kind, bbox, params, query, answer });
    }

    fn area_error(&mut self, e: String) {
        if let Some(b) = self.site.area.as_mut() {
            b.error = Some(e);
        }
    }

    /// Takes what the fetch brought, when it has come, and runs the command over it.
    pub(crate) fn poll_site(&mut self, ctx: &Context) {
        let Some(job) = &self.site.job else { return };
        let answer = match job.answer.try_recv() {
            Ok(a) => a,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the fetch stopped without an answer".to_string()),
        };
        let Some(job) = self.site.job.take() else { return };
        let fetched = match answer {
            Ok(f) => f,
            Err(e) => return self.area_error(e),
        };
        let Some(real) = self.session.geo.clone() else { return self.area_error(t("This BIMCraft has no network access.").to_string()) };
        let held = Prefetched::new(real.clone(), job.bbox);
        let held = match fetched {
            Fetched::Osm { query, reply } => held.with_osm(query, reply),
            Fetched::Heights { spacing, source, got } => held.with_elevation(spacing, source, got),
        };
        // The unchanged command, over what was fetched; the real fetcher comes back whatever it says.
        self.session.geo = Some(std::sync::Arc::new(held));
        let command = if job.kind == Kind::Osm { "fetch_osm" } else { "fetch_terrain" };
        let out = self.run(command, job.params);
        self.session.geo = Some(real);
        match out {
            Some(v) => {
                self.site.area = None;
                self.say_fetched(job.kind, &v);
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                self.area_error(e);
            }
        }
    }

    /// The box that says what a fetch did, and the credit its data is owed.
    fn say_fetched(&mut self, kind: Kind, v: &Value) {
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let mut lines = Vec::new();
        let title = match kind {
            Kind::Osm => {
                lines.push(tf(
                    "{} buildings made, {} updated, {} left out",
                    &[&n("made").to_string(), &n("updated").to_string(), &n("skipped").to_string()],
                ));
                if n("off_terrain") > 0 {
                    lines.push(tf("{} are not over the terrain: they stand at the level", &[&n("off_terrain").to_string()]));
                }
                if n("courtyards_filled") > 0 {
                    lines.push(tf("{} courtyards filled in", &[&n("courtyards_filled").to_string()]));
                }
                if let Some(r) = v["skipped_by_reason"].as_object() {
                    for (why, count) in r {
                        lines.push(format!("{count} × {why}"));
                    }
                }
                t("Import from OpenStreetMap").to_string()
            }
            Kind::Terrain => {
                lines.push(tf(
                    "A toposurface of {} points, every {} m, from {}",
                    &[
                        &n("points").to_string(),
                        &v["spacing"].as_f64().unwrap_or(0.0).round().to_string(),
                        v["source"].as_str().unwrap_or("?"),
                    ],
                ));
                if let Some(z) = v["elevation_set"].as_f64() {
                    lines.push(tf("The project's ground height is now {} m above sea level", &[&format!("{:.1}", z / 1000.0)]));
                }
                t("Terrain from Elevation Data").to_string()
            }
        };
        let credit = v["attribution"].as_str().map(str::to_string);
        let query = v["fetched"]["query"].as_str().map(str::to_string);
        self.site.said = Some(Said { title, lines, credit, query });
    }

    /// The box over a fetch that is running.
    fn fetching_box(&mut self, ctx: &Context) {
        let Some(job) = &self.site.job else { return };
        let what = match job.kind {
            Kind::Osm => t("Fetching buildings from OpenStreetMap…"),
            Kind::Terrain => t("Fetching elevation data…"),
        };
        let km2 = tf("{} km²", &[&format!("{:.2}", job.bbox.area_km2())]);
        let mut cancel = false;
        Modal::new(UiId::new("site-fetching"), t("Fetching")).width(420.0).closable(false).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(what).strong());
                });
                ui.label(RichText::new(km2).color(pal().muted));
                if let Some(q) = &job.query {
                    ui.add_space(6.0);
                    ui.label(t("Overpass query"));
                    query_view(ui, q);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        // What the thread brings after this is not looked at.
        if cancel {
            self.site.job = None;
        }
    }

    fn said_window(&mut self, ctx: &Context) {
        let Some(s) = &self.site.said else { return };
        let mut close = false;
        let r = Modal::new(UiId::new("site-said"), s.title.clone()).width(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                for line in &s.lines {
                    ui.label(line);
                }
                if let Some(q) = &s.query {
                    ui.add_space(8.0);
                    ui.label(RichText::new(t("Query used")).strong());
                    query_view(ui, q);
                }
                if let Some(c) = &s.credit {
                    ui.add_space(8.0);
                    ui.label(RichText::new(c).color(pal().muted));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                });
            });
        });
        if close || r.should_close() {
            self.site.said = None;
        }
    }

    /// Every box of the Site tab.
    pub(crate) fn site_windows(&mut self, ctx: &Context) {
        self.location_window(ctx);
        self.area_window(ctx);
        self.fetching_box(ctx);
        self.said_window(ctx);
        self.map_window(ctx);
    }

    // ---- The Pad tool, Place on Terrain, Cut/Fill ----

    /// The Pad tool's Options Bar: how far above its level its top is, and how thick it is.
    pub(crate) fn pad_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Offset:"));
        ui.add(TextEdit::singleline(&mut self.pad_offset).desired_width(64.0)).labelled_by(l.id);
        let l = ui.label(t("Thickness:"));
        ui.add(TextEdit::singleline(&mut self.pad_thickness).desired_width(64.0)).labelled_by(l.id);
    }

    /// The Pad tool's Enter, or its first corner clicked again: a pad over the outline clicked, on the plan's level. Fewer than
    /// three corners make none.
    pub(crate) fn finish_pad(&mut self, level: Id) {
        let crate::Tool::Pad(path) = &mut self.tool else { return };
        let path = std::mem::take(path);
        if path.len() < 3 {
            return;
        }
        let units = self.session.model.units;
        let (Some(offset), Some(thickness)) = (units.parse(&self.pad_offset), units.parse(&self.pad_thickness)) else {
            self.status = Status { text: t("pad: the offset and the thickness are lengths").to_string(), error: true };
            return;
        };
        let boundary: Vec<Value> = path.iter().map(|p| json!([p.0, p.1])).collect();
        self.run("pad", json!({"boundary": boundary, "level": level, "offset": offset, "thickness": thickness}));
    }

    /// Place on Terrain: the selected things stand on the ground.
    fn place_on_terrain(&mut self) {
        if self.selection.is_empty() {
            self.status = Status { text: t("Select what is to stand on the terrain first.").to_string(), error: true };
            return;
        }
        let Some(v) = self.run("place_on_terrain", json!({"ids": self.selection})) else { return };
        let moved = v["moved"].as_array().map_or(0, Vec::len);
        let skipped = v["skipped"].as_array().map_or(0, Vec::len);
        let text = if skipped == 0 {
            tf("{} stand on the terrain", &[&moved.to_string()])
        } else {
            tf("{} stand on the terrain; {} are not over it", &[&moved.to_string(), &skipped.to_string()])
        };
        self.status = Status { text, error: false };
    }

    /// Cut/Fill: what the selected pads (all of them, when none is) take out of the terrain and put in.
    fn cut_fill(&mut self) {
        let m = &self.session.model;
        let chosen: Vec<Id> = self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::Pad(_)))).collect();
        let params = if chosen.is_empty() { json!({}) } else { json!({"pads": chosen}) };
        let Some(v) = self.run("cut_fill", params) else { return };
        let m3 = |x: &Value| format!("{:.1}", x.as_f64().unwrap_or(0.0));
        let mut lines = Vec::new();
        for pad in v["pads"].as_array().into_iter().flatten() {
            let name = pad["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| pad["id"].to_string(), str::to_string);
            lines.push(tf(
                "{}: cut {} m³, fill {} m³ (net {} m³)",
                &[&name, &m3(&pad["cut_m3"]), &m3(&pad["fill_m3"]), &m3(&pad["net_m3"])],
            ));
        }
        let totals = &v["totals"];
        lines.push(tf(
            "All: cut {} m³, fill {} m³; net {} m³ (positive is earth to bring in)",
            &[&m3(&totals["cut_m3"]), &m3(&totals["fill_m3"]), &m3(&totals["net_m3"])],
        ));
        self.site.said = Some(Said { title: t("Cut/Fill").to_string(), lines, credit: None, query: None });
    }
}
