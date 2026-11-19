//! The site tools of the Massing tab (Revit's Massing & Site): the ground (Terrain from Elevation Data, a toposolid, and Place on
//! Terrain) and what stands round it (Import from OpenStreetMap, Build Shell). Where the project is on Earth is Manage > Location's
//! (`coordinates.rs`): these fetch round the active site's place.
//!
//! Every button runs a command. The two that go to the network (`fetch_osm`, `fetch_terrain`) do it on a thread of their own, as
//! reading a Revit file does: the thread asks the fetcher the session holds, and what it brings is handed back to the unchanged
//! command through [`Prefetched`], so one validation, one undo step and one report, and the window never waits on the network.

mod map;
mod shell;

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::geo::{self, Bbox, Elevation, Layers, Prefetched, Reply};
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
    Osm {
        query: String,
        reply: Reply,
    },
    Heights {
        spacing: f64,
        source: Option<String>,
        got: Elevation,
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
    area: Option<AreaBox>,
    job: Option<Job>,
    said: Option<Said>,
    /// The map an area is picked on.
    map: Option<map::MapBox>,
    /// The Build Shell box.
    shell: Option<shell::ShellBox>,
}

/// The elevation sources the Terrain box offers: how they are named to the command, and shown.
const SOURCES: [(&str, &str); 3] = [("", "Automatic"), ("terrarium", "Terrarium"), ("copernicus", "Copernicus GLO-30")];

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

/// What the site tools say when the project's site has no place on Earth yet.
fn no_place() -> String {
    t("The project's site has no place on Earth yet: give it a map system and its latitude and longitude in Manage > Location first.")
        .to_string()
}

impl BimApp {
    /// Whether a fetch is running.
    pub fn fetching(&self) -> bool {
        self.site.job.is_some()
    }

    /// The Massing tab's site panels: the ground and what stands round it.
    pub(crate) fn site_panels(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Site"), |ui| {
            let tip = msg!("Terrain from Elevation Data: the ground round the site's place, fetched from an elevation service, as a toposolid");
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
            let tip =
                msg!("Place on Terrain: stands the selected masses, columns, footings, components and walls on the toposolid under them");
            if big_button(ui, Icon::PlaceOnTerrain, "Place on Terrain", false, tip).clicked() {
                self.place_on_terrain();
            }
        });
        panel(ui, msg!("Context"), |ui| {
            let tip = msg!("Import from OpenStreetMap: the buildings round the site's place, fetched from Overpass, as masses on the ground");
            if big_button(ui, Icon::FromMap, "Import from OpenStreetMap", self.site.area.as_ref().is_some_and(|a| a.kind == Kind::Osm), tip)
                .clicked()
            {
                self.open_area(Kind::Osm);
            }
            let tip = msg!(
                "Build Shell: the outer walls, a floor at each storey, the roof and the doors of the selected buildings, as their OpenStreetMap tags describe them"
            );
            if big_button(ui, Icon::Shell, "Build Shell", self.site.shell.is_some(), tip).clicked() {
                self.open_shell();
            }
        });
    }

    // ---- Location ----

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
            let (lon, lat) = geo::Projector::of(&self.session.model).and_then(|p| p.origin()).map_err(|_| no_place())?;
            let metres = typed(&b.radius, "Distance round the origin (m)")?;
            Bbox::around(lon, lat, metres)?
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
            }
        })();
        let (kind, bbox, params, heights) = match built {
            Ok(x) => x,
            Err(e) => return self.area_error(e),
        };
        // The commands' own checks, before anyone waits for the network: no place on Earth, a spacing too fine, a bad height.
        if geo::Projector::of(&self.session.model).is_err() {
            return self.area_error(no_place());
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
        let Some(job) = self.site.job.take() else { return };
        let fetched = match answer {
            Ok(f) => f,
            Err(e) => return self.fetch_error(job.kind, e),
        };
        let Some(real) = self.session.geo.clone() else {
            return self.fetch_error(job.kind, t("This BIMCraft has no network access.").to_string());
        };
        let held = Prefetched::new(real.clone(), job.bbox);
        let held = match fetched {
            Fetched::Osm { query, reply } => held.with_osm(query, reply),
            Fetched::Heights { spacing, source, got } => held.with_elevation(spacing, source, got),
        };
        // The unchanged command, over what was fetched; the real fetcher comes back whatever it says.
        self.session.geo = Some(std::sync::Arc::new(held));
        let command = match job.kind {
            Kind::Osm => "fetch_osm",
            Kind::Terrain => "fetch_terrain",
        };
        let out = self.run(command, job.params);
        self.session.geo = Some(real);
        match out {
            Some(v) => {
                self.site.area = None;
                self.say_fetched(job.kind, &v);
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                self.fetch_error(job.kind, e);
            }
        }
    }

    /// What went wrong with a fetch, in the box that asked for it.
    fn fetch_error(&mut self, _kind: Kind, e: String) {
        self.area_error(e);
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
                    "A toposolid of {} heights, every {} m, from {}",
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
        // An imagery fetch counts its tiles, as far as the fetcher says; the others have nothing to count.
        let progress: Option<(usize, usize)> = None;
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
        self.area_window(ctx);
        self.fetching_box(ctx);
        self.said_window(ctx);
        self.map_window(ctx);
        self.shell_window(ctx);
    }

    // ---- Place on Terrain ----

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

}
