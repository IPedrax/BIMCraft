//! The Site tab: where the project is on Earth (Location), the ground (Terrain from Elevation Data, Pad, Place on Terrain, Cut/Fill)
//! and what stands round it (Import from OpenStreetMap).
//!
//! Every button runs a command. The two that go to the network (`fetch_osm`, `fetch_terrain`) do it on a thread of their own, as
//! reading a Revit file does: the thread asks the fetcher the session holds, and what it brings is handed back to the unchanged
//! command through [`Prefetched`], so one validation, one undo step and one report, and the window never waits on the network.

mod map;
mod shell;

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::geo::{self, Bbox, Elevation, Layers, Prefetched, Reply};
use bimcraft_core::geom::P2;
use bimcraft_core::georef::parse_coordinates;
use bimcraft_core::ids::Id;
use bimcraft_core::imagery::{self, Imagery, imagery_ask};
use bimcraft_core::model::{Element, Model, Pad};
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, Response, RichText, ScrollArea, TextEdit, Ui};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::properties::number;
use crate::settings::Place;
use crate::style::pal;
use crate::{BimApp, Icon, Status, big_button, panel};

/// What a fetch is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Osm,
    Terrain,
    Imagery,
}

/// What a fetch is made of: what for, over which box, the command's parameters, and (for heights) the spacing and source asked for.
type Built = (Kind, Bbox, Value, Option<(f64, Option<String>)>);

/// What a fetch brought, from its thread.
enum Fetched {
    Osm {
        query: String,
        reply: Reply,
    },
    Heights {
        spacing: f64,
        source: Option<String>,
        got: Elevation,
    },
    /// What an imagery source brought, and the sources passed over before it (their names, and why).
    Imagery {
        got: Imagery,
        skipped: Vec<(String, String)>,
    },
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
    /// What the place is called in the list of recent places; blank is its coordinates.
    name: String,
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
    default_height: String,
    level_height: String,
    // Terrain.
    spacing: String,
    source: usize,
    interval: String,
    set_elevation: bool,
    /// Whether the buildings the import made are stood on the ground this fetch makes.
    stand_buildings: bool,
    error: Option<String>,
}

/// The box for Aerial Imagery: from which source, and how many pixels the picture has along its long side.
pub(crate) struct ImageryBox {
    /// 0 for automatic, else one more than the source's place in the core's table.
    source: usize,
    side: String,
    error: Option<String>,
}

/// What the Aerial Imagery box tells before anything is asked: a line for each source it would try, in order, a note when the best of
/// them is too coarse for buildings, and why it cannot fetch when it cannot (which turns Fetch off).
#[derive(Default)]
struct ImageryPreview {
    lines: Vec<String>,
    coarse: Option<String>,
    error: Option<String>,
}

/// The Align Imagery box: the toposurface whose photograph it moves, and how far it is shifted, as typed (project units).
pub(crate) struct AlignBox {
    id: Id,
    east: String,
    north: String,
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
    imagery: Option<ImageryBox>,
    align: Option<AlignBox>,
    job: Option<Job>,
    said: Option<Said>,
    /// The map a place or an area is picked on.
    map: Option<map::MapBox>,
    /// The Build Shell box.
    shell: Option<shell::ShellBox>,
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

/// How a recent place is listed: its name and where, or just where.
fn place_label(p: &Place) -> String {
    let at = format!("{:.6}, {:.6}", p.latitude, p.longitude);
    if p.name.is_empty() { at } else { format!("{} ({at})", p.name) }
}

/// The place a located project is at.
fn place_of(l: &bimcraft_core::georef::Location) -> Place {
    Place {
        latitude: l.latitude,
        longitude: l.longitude,
        true_north: l.true_north,
        crs: l.crs.map_or_else(|| "utm".to_string(), |c| c.to_string()),
        name: String::new(),
    }
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
            let tip = msg!(
                "Aerial Imagery: photographs of the ground, fetched from an imagery service and laid over the selected terrain (the only one when none is selected)"
            );
            if big_button(ui, Icon::Aerial, "Aerial Imagery", self.site.imagery.is_some(), tip).clicked() {
                self.open_imagery();
            }
            let tip = msg!(
                "Align Imagery: move the photographs on the selected terrain (the only one when none is selected) east and north to line them up with the map"
            );
            let wears = self.aligned_terrain().is_some();
            if ui.add_enabled_ui(wears, |ui| big_button(ui, Icon::Move, "Align Imagery", self.site.align.is_some(), tip)).inner.clicked() {
                self.open_align();
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
            let tip = msg!(
                "Build Shell: the outer walls, a floor at each storey and the roof of the selected buildings, as their OpenStreetMap tags describe them"
            );
            if big_button(ui, Icon::Shell, "Build Shell", self.site.shell.is_some(), tip).clicked() {
                self.open_shell();
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
                name: self.places.iter().find(|p| p.same_as(&place_of(&l))).map(|p| p.name.clone()).unwrap_or_default(),
                error: None,
            },
            // A project with no place starts at the last one set.
            None => match self.places.first() {
                Some(p) => LocationBox {
                    coordinates: String::new(),
                    coordinates_note: None,
                    latitude: p.latitude.to_string(),
                    longitude: p.longitude.to_string(),
                    elevation: units.length(0.0),
                    true_north: p.true_north.to_string(),
                    crs: p.crs.clone(),
                    grid: None,
                    name: p.name.clone(),
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
                    name: String::new(),
                    error: None,
                },
            },
        };
        self.site.location = Some(b);
    }

    fn location_window(&mut self, ctx: &Context) {
        let Some(b) = self.site.location.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let mut pick = None;
        let (mut chosen, mut forget) = (None, None);
        let places = &self.places;
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
                    text_field(ui, "Name", &mut b.name, t("Kept in the recent places"));
                });
                if !places.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("Recent places")).strong());
                    for (i, p) in places.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.button(place_label(p)).clicked() {
                                chosen = Some(i);
                            }
                            if ui.small_button("✕").on_hover_text(t("Forget this place")).clicked() {
                                forget = Some(i);
                            }
                        });
                    }
                    ui.add_space(4.0);
                }
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
        if let Some(i) = forget {
            self.places.remove(i);
        }
        if let (Some(i), Some(b)) = (chosen, self.site.location.as_mut())
            && let Some(p) = self.places.get(i)
        {
            (b.latitude, b.longitude, b.true_north) = (p.latitude.to_string(), p.longitude.to_string(), p.true_north.to_string());
            (b.crs, b.name, b.coordinates, b.coordinates_note, b.error) = (p.crs.clone(), p.name.clone(), String::new(), None, None);
        }
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
        // What the project is put at is kept for next time, when the command takes it.
        let place = self.site.location.as_ref().and_then(|b| {
            Some(Place {
                latitude: params["latitude"].as_f64()?,
                longitude: params["longitude"].as_f64()?,
                true_north: params["true_north"].as_f64()?,
                crs: params["crs"].as_str().unwrap_or("utm").chars().take(16).collect(),
                name: b.name.trim().chars().take(80).collect(),
            })
        });
        match self.run("geolocate", params) {
            Some(_) => {
                self.site.location = None;
                if let Some(p) = place {
                    crate::settings::remember(&mut self.places, p);
                }
            }
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
            default_height: units.length(6000.0),
            level_height: units.length(3000.0),
            spacing: String::new(),
            source: 0,
            interval: units.length(1000.0),
            set_elevation: true,
            stand_buildings: true,
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
            Kind::Imagery => return,
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
                        // (Aerial imagery has a box of its own.)
                        Kind::Imagery => {}
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
                if kind == Kind::Terrain {
                    ui.checkbox(&mut b.set_elevation, t("Take the project's ground height from the data, when it has none"));
                    ui.checkbox(&mut b.stand_buildings, t("Stand the imported buildings on the new ground"));
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
                        Some(x) => query_view(ui, &geo::overpass_query(&x, Layers::default())),
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

    // ---- Aerial imagery ----

    fn open_imagery(&mut self) {
        self.site.imagery = Some(ImageryBox { source: 0, side: imagery::DEFAULT_SIDE.to_string(), error: None });
    }

    /// The parameters of `fetch_imagery` the box describes, with the selected toposurface if one is.
    fn imagery_params(&self, b: &ImageryBox) -> Result<Value, String> {
        let side = b.side.trim().parse::<u32>().map_err(|_| tf("Picture size: `{}` is not a whole number of pixels", &[b.side.trim()]))?;
        let mut params = json!({"max_side": side});
        if let Some(source) = b.source.checked_sub(1).and_then(|i| imagery::SOURCES.get(i)) {
            params["source"] = json!(source.id);
        }
        let model = &self.session.model;
        if let Some(id) = self.selection.iter().copied().find(|id| matches!(model.elements.get(id), Some(Element::Toposurface(_)))) {
            params["id"] = json!(id);
        }
        Ok(params)
    }

    /// What the box would ask for, worked out as the command works it out (`imagery_ask`), and nothing is asked of a service.
    fn imagery_preview(&self, b: &ImageryBox) -> ImageryPreview {
        let ask = self.imagery_params(b).and_then(|params| imagery_ask("fetch_imagery", &self.session.model, &params));
        let ask = match ask {
            Ok(ask) if ask.tries.is_empty() => return ImageryPreview { error: Some(ask.reason), ..Default::default() },
            Ok(ask) => ask,
            Err(e) => return ImageryPreview { error: Some(e), ..Default::default() },
        };
        let megabytes = |bytes: u64| format!("{:.1}", bytes as f64 / 1_048_576.0);
        let lines = ask
            .tries
            .iter()
            .map(|(source, plan)| {
                tf(
                    "{}: {} m a pixel, {} × {} tiles (about {} MB), a picture of {} × {} pixels",
                    &[
                        source.name,
                        &format!("{:.2}", plan.metres_per_pixel),
                        &plan.cols.to_string(),
                        &plan.rows.to_string(),
                        &megabytes(plan.estimated_bytes()),
                        &plan.width.to_string(),
                        &plan.height.to_string(),
                    ],
                )
            })
            .collect();
        // The first is the one used, unless it brings nothing: if it is too coarse, say so before the wait.
        let coarse = ask.tries.first().filter(|(_, plan)| plan.is_coarse()).map(|(source, plan)| {
            tf(
                "The finest photographs of this place are from {}, at {} m a pixel: too coarse to recognise buildings",
                &[source.name, &format!("{:.1}", plan.metres_per_pixel)],
            )
        });
        ImageryPreview { lines, coarse, error: None }
    }

    // ---- Aligning the photograph ----

    /// The toposurface whose photograph Align Imagery moves: the selected one that wears a photograph, else the only one that does.
    fn aligned_terrain(&self) -> Option<Id> {
        let m = &self.session.model;
        let wears = |id: &Id| matches!(m.elements.get(id), Some(Element::Toposurface(t)) if imagery::shift_of(m, t).is_some());
        self.selection.iter().copied().find(wears).or_else(|| {
            let all: Vec<Id> = m.elements.keys().copied().filter(wears).collect();
            if let [one] = all.as_slice() { Some(*one) } else { None }
        })
    }

    fn open_align(&mut self) {
        let Some(id) = self.aligned_terrain() else { return };
        let m = &self.session.model;
        let (east, north) = match m.elements.get(id) {
            Some(Element::Toposurface(t)) => imagery::shift_of(m, t).unwrap_or((0.0, 0.0)),
            _ => (0.0, 0.0),
        };
        let units = m.units;
        self.site.align = Some(AlignBox { id, east: units.length(east), north: units.length(north), error: None });
    }

    /// Moves the photograph by what the box holds, or puts it back; the box stays so that it can be nudged again and again.
    fn apply_align(&mut self, reset: bool) {
        let Some(b) = self.site.align.as_ref() else { return };
        let units = self.session.model.units;
        let id = b.id;
        let params = if reset {
            json!({"id": id, "reset": true})
        } else {
            let length = |text: &str, what: &str| units.parse(text).ok_or_else(|| tf("{}: `{}` is not a length", &[t(what), text.trim()]));
            match (length(&b.east, "Shift east"), length(&b.north, "Shift north")) {
                (Ok(east), Ok(north)) => json!({"id": id, "east": east, "north": north}),
                (Err(e), _) | (_, Err(e)) => {
                    if let Some(b) = self.site.align.as_mut() {
                        b.error = Some(e);
                    }
                    return;
                }
            }
        };
        let out = self.run("shift_imagery", params);
        let error = std::mem::take(&mut self.status).text;
        if let Some(b) = self.site.align.as_mut() {
            match out {
                Some(v) => {
                    let shown = |key: &str| units.length(v[key].as_f64().unwrap_or(0.0));
                    (b.east, b.north, b.error) = (shown("east"), shown("north"), None);
                }
                None => b.error = Some(error),
            }
        }
    }

    fn align_window(&mut self, ctx: &Context) {
        if self.site.job.is_some() {
            return;
        }
        let unit = self.session.model.units.name();
        let Some(b) = self.site.align.as_mut() else { return };
        let (mut apply, mut reset, mut close) = (false, false, false);
        let r = Modal::new(UiId::new("site-align"), t("Align Imagery")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Move the photographs east and north, from where the fetch laid them, until they line up with the map."));
                ui.add_space(6.0);
                Grid::new("site-align-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    text_field(ui, "Shift east", &mut b.east, unit);
                    text_field(ui, "Shift north", &mut b.north, unit);
                });
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    reset = ui.button(t("Reset")).clicked();
                    apply = ui.button(t("Apply")).clicked();
                });
            });
        });
        if apply {
            self.apply_align(false);
        } else if reset {
            self.apply_align(true);
        } else if close || r.should_close() {
            self.site.align = None;
        }
    }

    fn imagery_error(&mut self, e: String) {
        if let Some(b) = self.site.imagery.as_mut() {
            b.error = Some(e);
        }
    }

    fn imagery_window(&mut self, ctx: &Context) {
        // While a fetch runs, its own box is the only one: this one is back, with what went wrong, if it did.
        if self.site.job.is_some() {
            return;
        }
        let offline = self.session.geo.is_none();
        let Some(preview) = self.site.imagery.as_ref().map(|b| self.imagery_preview(b)) else { return };
        let Some(b) = self.site.imagery.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("site-imagery"), t("Aerial Imagery")).width(460.0).show(ctx, |ui| {
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
                ui.label(t("Photographs of the ground the terrain covers, fetched from an imagery service and laid over it."));
                ui.add_space(6.0);
                Grid::new("site-imagery-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Imagery from"));
                    let selected = match b.source.checked_sub(1).and_then(|i| imagery::SOURCES.get(i)) {
                        Some(source) => source.name.to_string(),
                        None => t("Automatic").to_string(),
                    };
                    let c = egui::ComboBox::from_id_salt("site-imagery-source").selected_text(selected).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.source, 0, t("Automatic"));
                        for (i, source) in imagery::SOURCES.iter().enumerate() {
                            ui.selectable_value(&mut b.source, i + 1, source.name);
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    text_field(ui, "Picture size (pixels)", &mut b.side, "1024");
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(t(
                        "Automatic takes the finest source that has the place. The photographs' own terms of use go with the picture.",
                    ))
                    .color(pal().muted),
                );
                // What will be asked, before anything is: a line for each source in the order it is tried, or why nothing can be.
                ui.add_space(6.0);
                match &preview.error {
                    Some(e) => {
                        ui.label(RichText::new(e).color(pal().error));
                    }
                    None => {
                        for line in &preview.lines {
                            ui.label(line);
                        }
                        if let Some(note) = &preview.coarse {
                            ui.label(RichText::new(note).color(pal().error));
                        }
                    }
                }
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(!offline && preview.error.is_none(), Button::new(t("Fetch"))).clicked();
                });
            });
        });
        if ok {
            self.start_imagery(ctx);
        } else if close || r.should_close() {
            self.site.imagery = None;
        }
    }

    /// Starts the imagery fetch the box describes, on a thread; the box stays until it is over (its refusals appear in it). What to ask
    /// for is the command's own working out (`imagery_ask`), before anyone waits for the network.
    fn start_imagery(&mut self, ctx: &Context) {
        let Some(b) = self.site.imagery.as_ref() else { return };
        let params = match self.imagery_params(b) {
            Ok(p) => p,
            Err(e) => return self.imagery_error(e),
        };
        let Some(geo) = self.session.geo.clone() else {
            return self.imagery_error(t("This BIMCraft has no network access.").to_string());
        };
        let ask = match imagery_ask("fetch_imagery", &self.session.model, &params) {
            Ok(a) if a.tries.is_empty() => return self.imagery_error(a.reason),
            Ok(a) => a,
            Err(e) => return self.imagery_error(e),
        };
        if self.site.job.is_some() {
            return;
        }
        let mut params = params;
        params["id"] = json!(ask.id);
        let (send, answer) = channel();
        let tries = ask.tries.clone();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("site-fetch".into()).spawn(move || {
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // The first source that brings imagery wins, and each one passed over is kept with why (as the command does); every
                // reason is the answer if none does.
                let mut skipped = Vec::new();
                for (source, plan) in &tries {
                    match geo.imagery(source.id, plan) {
                        Ok(got) => return Ok(Fetched::Imagery { got, skipped }),
                        Err(why) => skipped.push((source.name.to_string(), imagery::short_reason(&why))),
                    }
                }
                Err(skipped.into_iter().map(|(_, why)| why).collect::<Vec<_>>().join("; "))
            }))
            .unwrap_or_else(|_| Err("the fetcher stopped on this request".to_string()));
            let _ = send.send(got);
            wake.request_repaint();
        });
        if let Err(e) = spawned {
            return self.imagery_error(e.to_string());
        }
        self.site.job = Some(Job { kind: Kind::Imagery, bbox: ask.envelope, params, query: None, answer });
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
                    params["stand_buildings"] = json!(b.stand_buildings);
                    if !b.interval.trim().is_empty() {
                        params["interval"] = json!(
                            units.parse(&b.interval).ok_or_else(|| tf("Contour interval: `{}` is not a length", &[b.interval.trim()]))?
                        );
                    }
                    Ok((Kind::Terrain, bbox, params, Some((spacing, source))))
                }
                Kind::Imagery => Err("an imagery fetch has its own box".to_string()),
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
        let layers = Layers::default();
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
        let Some(mut job) = self.site.job.take() else { return };
        let fetched = match answer {
            Ok(f) => f,
            Err(e) => return self.fetch_error(job.kind, e),
        };
        let Some(real) = self.session.geo.clone() else {
            return self.fetch_error(job.kind, t("This BIMCraft has no network access.").to_string());
        };
        let held = Prefetched::new(real.clone(), job.bbox);
        let mut passed_over = Vec::new();
        let held = match fetched {
            Fetched::Osm { query, reply } => held.with_osm(query, reply),
            Fetched::Heights { spacing, source, got } => held.with_elevation(spacing, source, got),
            // The command is told which source won, so it asks for just that one, and is handed what was fetched.
            Fetched::Imagery { got, skipped } => {
                job.params["source"] = json!(got.source);
                passed_over = skipped;
                held.with_imagery(got)
            }
        };
        // The unchanged command, over what was fetched; the real fetcher comes back whatever it says.
        self.session.geo = Some(std::sync::Arc::new(held));
        let command = match job.kind {
            Kind::Osm => "fetch_osm",
            Kind::Terrain => "fetch_terrain",
            Kind::Imagery => "fetch_imagery",
        };
        let out = self.run(command, job.params);
        self.session.geo = Some(real);
        match out {
            Some(mut v) => {
                self.site.area = None;
                self.site.imagery = None;
                // The command was asked for the one source that won, so the ones passed over before it are the thread's to say.
                if !passed_over.is_empty() {
                    v["skipped"] = passed_over.iter().map(|(name, why)| json!({"name": name, "why": why})).collect();
                }
                self.say_fetched(job.kind, &v);
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                self.fetch_error(job.kind, e);
            }
        }
    }

    /// What went wrong with a fetch, in the box that asked for it.
    fn fetch_error(&mut self, kind: Kind, e: String) {
        match kind {
            Kind::Imagery => self.imagery_error(e),
            Kind::Osm | Kind::Terrain => self.area_error(e),
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
                if n("pieces") > 0 {
                    lines.push(tf(
                        "{} buildings with courtyards cut into {} pieces that leave them open",
                        &[&n("pieces_of").to_string(), &n("pieces").to_string()],
                    ));
                }
                if n("outlines_replaced") > 0 {
                    lines.push(tf("{} outlines replaced by their building parts", &[&n("outlines_replaced").to_string()]));
                }
                if n("entrances") > 0 {
                    lines.push(tf("{} entrances and exits placed on their buildings", &[&n("entrances").to_string()]));
                }
                if n("entrances_unplaced") > 0 {
                    lines.push(tf("{} entrances are not on any building", &[&n("entrances_unplaced").to_string()]));
                }
                if n("outlines_kept") > 0 {
                    lines.push(tf("{} outlines kept: their parts cover too little of them", &[&n("outlines_kept").to_string()]));
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
                // What happened to the buildings the import made: they follow the ground, or are said not to.
                let stood = &v["stood"];
                let count = |k: &str| stood[k].as_u64().unwrap_or(0);
                if count("moved") > 0 {
                    lines.push(tf("{} buildings stand on the new ground", &[&count("moved").to_string()]));
                }
                if count("shells_updated") > 0 {
                    lines.push(tf("{} of their walls, floors and roofs followed", &[&count("shells_updated").to_string()]));
                }
                if count("off_terrain") > 0 {
                    lines.push(tf(
                        "{} buildings are not over the new ground and stay where they were",
                        &[&count("off_terrain").to_string()],
                    ));
                }
                let left = stood["shells_left"].as_array().map(Vec::as_slice).unwrap_or_default();
                if let Some(first) = left.first() {
                    lines.push(tf(
                        "The walls of {} buildings could not follow them and were left: {}",
                        &[&left.len().to_string(), first["why"].as_str().unwrap_or("?")],
                    ));
                }
                t("Terrain from Elevation Data").to_string()
            }
            Kind::Imagery => {
                let pixels = |i: usize| v["picture"][i].as_u64().unwrap_or(0).to_string();
                lines.push(tf(
                    "Photographs from {}, {} m a pixel (zoom {})",
                    &[
                        v["name"].as_str().unwrap_or("?"),
                        &format!("{:.2}", v["metres_per_pixel"].as_f64().unwrap_or(0.0)),
                        &n("zoom").to_string(),
                    ],
                ));
                lines.push(tf("A picture of {} × {} pixels, laid over the terrain", &[&pixels(0), &pixels(1)]));
                let blank = v["without_imagery_percent"].as_f64().unwrap_or(0.0);
                if blank >= 0.5 {
                    lines.push(tf("{} % of it has no imagery and shows white", &[&format!("{blank:.0}")]));
                }
                if v["cached"].as_bool() == Some(true) {
                    lines.push(t("The tiles were kept from an earlier fetch").to_string());
                }
                // A source passed over is said, so that a coarser picture than expected is never a mystery.
                for passed in v["skipped"].as_array().into_iter().flatten() {
                    lines.push(tf(
                        "{} was passed over: {}",
                        &[passed["name"].as_str().unwrap_or("?"), passed["why"].as_str().unwrap_or("?")],
                    ));
                }
                if let Some(terms) = v["terms"].as_str() {
                    lines.push(tf("Terms of use: {}", &[terms]));
                }
                t("Aerial Imagery").to_string()
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
            Kind::Imagery => t("Fetching aerial imagery…"),
        };
        let km2 = tf("{} km²", &[&format!("{:.2}", job.bbox.area_km2())]);
        // An imagery fetch counts its tiles, as far as the fetcher says; the others have nothing to count.
        let progress = if job.kind == Kind::Imagery { self.session.geo.as_ref().and_then(|g| g.imagery_progress()) } else { None };
        let mut cancel = false;
        Modal::new(UiId::new("site-fetching"), t("Fetching")).width(420.0).closable(false).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(what).strong());
                });
                ui.label(RichText::new(km2).color(pal().muted));
                if let Some((done, total)) = progress.filter(|(_, total)| *total > 0) {
                    ui.add_space(6.0);
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32));
                    ui.label(tf("{} of {} tiles", &[&done.to_string(), &total.to_string()]));
                }
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
        self.imagery_window(ctx);
        self.align_window(ctx);
        self.fetching_box(ctx);
        self.said_window(ctx);
        self.map_window(ctx);
        self.shell_window(ctx);
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
