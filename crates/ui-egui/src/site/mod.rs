//! The site tools of the Massing tab (Revit's Massing & Site): what stands round the site (Import from OpenStreetMap, Build
//! Shell). Where the project is on Earth is Manage > Location's (`coordinates.rs`): these fetch round the active site's place.
//!
//! Every button runs a command. The one that goes to the network (`fetch_osm`) does it on a thread of its own, as reading a Revit
//! file does: the thread asks the fetcher the session holds, and what it brings is handed back to the unchanged command through
//! [`Prefetched`], so one validation, one undo step and one report, and the window never waits on the network.

mod map;
mod shell;

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::geo::{self, Bbox, Layers, Prefetched, Reply};
use bimcraft_core::places;
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, Response, RichText, ScrollArea, TextEdit, Ui};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::properties::number;
use crate::style::pal;
use crate::{BimApp, Icon, big_button, panel};

/// What a fetch brought, from its thread.
struct Fetched {
    query: String,
    reply: Reply,
}

/// A fetch running: over which box, with which command's parameters, and its answer when it comes.
struct Job {
    bbox: Bbox,
    params: Value,
    /// The Overpass query it asks with, for a fetch of buildings.
    query: String,
    answer: Receiver<Result<Fetched, String>>,
}

/// The box for Import from OpenStreetMap: the area, and what the command takes.
pub(crate) struct AreaBox {
    /// A distance round the origin, or a box typed.
    around: bool,
    radius: String,
    south: String,
    west: String,
    north: String,
    east: String,
    default_height: String,
    level_height: String,
    /// When the site has no place on Earth yet, the projected CRS chosen for the bbox's centre.
    selected_crs: Option<u32>,
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
        panel(ui, msg!("Context"), |ui| {
            let tip =
                msg!("Import from OpenStreetMap: the buildings round the site's place, fetched from Overpass, as masses on the ground");
            if big_button(ui, Icon::FromMap, "Import from OpenStreetMap", self.site.area.is_some(), tip).clicked() {
                self.open_area();
            }
            let tip = msg!(
                "Build Shell: the outer walls, a floor at each storey, the roof and the doors of the selected buildings, as their OpenStreetMap tags describe them"
            );
            if big_button(ui, Icon::Shell, "Build Shell", self.site.shell.is_some(), tip).clicked() {
                self.open_shell();
            }
        });
    }

    // ---- The area to fetch ----

    fn open_area(&mut self) {
        let units = self.session.model.units;
        self.site.area = Some(AreaBox {
            around: true,
            radius: "500".into(),
            south: String::new(),
            west: String::new(),
            north: String::new(),
            east: String::new(),
            default_height: units.length(6000.0),
            level_height: units.length(3000.0),
            selected_crs: None,
            error: None,
        });
    }

    /// Projected CRSs proposed for `(lon, lat)`, ordered from the most recent national/regional grid to older/global fallbacks.
    fn proposed_crs(&self, lon: f64, lat: f64) -> Vec<(u32, String, String)> {
        places::systems_near(lon, lat).into_iter().map(|s| (s.info.code, s.info.name, t(s.why).to_string())).collect()
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
        let title = t("Import from OpenStreetMap");
        let offline = self.session.geo.is_none();
        let area = self.area_bbox(b);
        let live = area.as_ref().map(|x| x.area_km2()).map_err(Clone::clone);
        let area = area.ok();
        let needs_place = geo::Projector::of(&self.session.model).is_err();
        let proposed = if needs_place {
            area.map(|x| self.proposed_crs((x.west + x.east) * 0.5, (x.south + x.north) * 0.5)).unwrap_or_default()
        } else {
            Vec::new()
        };
        let unit = self.session.model.units.name();
        let Some(b) = self.site.area.as_mut() else { return };
        if needs_place && !proposed.is_empty() && !b.selected_crs.is_some_and(|c| proposed.iter().any(|(p, _, _)| *p == c)) {
            b.selected_crs = Some(proposed[0].0);
        }
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
                    text_field(ui, "Height of a building with none", &mut b.default_height, unit);
                    text_field(ui, "Height of a level", &mut b.level_height, unit);
                });
                ui.add_space(6.0);
                match &live {
                    Ok(km2) => ui.label(
                        RichText::new(tf("{} km² (at most {})", &[&format!("{km2:.2}"), &geo::MAX_FETCH_AREA_KM2.to_string()]))
                            .color(pal().muted),
                    ),
                    Err(e) => ui.label(RichText::new(e).color(pal().muted)),
                };
                if needs_place {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("Projected coordinate system")).strong());
                    if proposed.is_empty() {
                        ui.label(
                            RichText::new(t("No site location is set. Pick on Map or enter a box of degrees to see available systems."))
                                .color(pal().muted),
                        );
                    } else {
                        ui.label(
                            RichText::new(t("Sets Manage > Location to this area's centre using the chosen system:")).color(pal().muted),
                        );
                        for (code, name, why) in &proposed {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut b.selected_crs, Some(*code), format!("EPSG:{code} — {name}"));
                                ui.label(RichText::new(why).color(pal().muted).small());
                            });
                        }
                    }
                }
                // What will be asked of Overpass, as it will be asked: it follows the area and the check above.
                ui.add_space(6.0);
                egui::CollapsingHeader::new(t("Overpass query")).id_salt("site-query").default_open(true).show(ui, |ui| match area {
                    Some(x) => query_view(ui, &geo::overpass_query(&x, Layers::default())),
                    None => {
                        ui.label(RichText::new(t("Give the area first: the query is made from its box.")).color(pal().muted));
                    }
                });
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
        let built = (|| -> Result<(Bbox, Value), String> {
            let bbox = self.area_bbox(b)?;
            let mut params = json!({"bbox": bbox.as_array()});
            if !b.default_height.trim().is_empty() {
                params["default_height"] = json!(
                    units
                        .parse(&b.default_height)
                        .ok_or_else(|| tf("Height of a building with none: `{}` is not a length", &[b.default_height.trim()]))?
                );
            }
            if !b.level_height.trim().is_empty() {
                params["level_height"] = json!(
                    units.parse(&b.level_height).ok_or_else(|| tf("Height of a level: `{}` is not a length", &[b.level_height.trim()]))?
                );
            }
            Ok((bbox, params))
        })();
        let (bbox, params) = match built {
            Ok(x) => x,
            Err(e) => return self.area_error(e),
        };
        // The commands' own checks, before anyone waits for the network: no place on Earth, a bad height.
        if geo::Projector::of(&self.session.model).is_err() {
            let Some(code) = b.selected_crs else {
                return self.area_error(no_place());
            };
            let (center_lon, center_lat) = ((bbox.west + bbox.east) * 0.5, (bbox.south + bbox.north) * 0.5);
            let site_name = self.session.model.coordinates.active.clone();
            if let Err(e) = self.exec(
                "location",
                &json!({
                    "name": site_name,
                    "crs": format!("EPSG:{code}"),
                    "latitude": center_lat,
                    "longitude": center_lon,
                }),
            ) {
                return self.area_error(e);
            }
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
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let query = geo::overpass_query(&bbox, layers);
                geo.overpass(&bbox, &query).map(|reply| Fetched { query, reply })
            }))
            .unwrap_or_else(|_| Err("the fetcher stopped on this request".to_string()));
            let _ = send.send(got);
            wake.request_repaint();
        });
        if let Err(e) = spawned {
            return self.area_error(e.to_string());
        }
        let query = geo::overpass_query(&bbox, layers);
        self.site.job = Some(Job { bbox, params, query, answer });
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
        let Some(real) = self.session.geo.clone() else {
            return self.area_error(t("This BIMCraft has no network access.").to_string());
        };
        let held = Prefetched::new(real.clone(), job.bbox).with_osm(fetched.query, fetched.reply);
        // The unchanged command, over what was fetched; the real fetcher comes back whatever it says.
        self.session.geo = Some(std::sync::Arc::new(held));
        let out = self.run("fetch_osm", job.params);
        self.session.geo = Some(real);
        match out {
            Some(v) => {
                self.site.area = None;
                self.say_fetched(&v);
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                self.area_error(e);
            }
        }
    }

    /// The box that says what a fetch did, and the credit its data is owed.
    fn say_fetched(&mut self, v: &Value) {
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let mut lines = Vec::new();
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
        let title = t("Import from OpenStreetMap").to_string();
        let credit = v["attribution"].as_str().map(str::to_string);
        let query = v["fetched"]["query"].as_str().map(str::to_string);
        self.site.said = Some(Said { title, lines, credit, query });
    }

    /// The box over a fetch that is running.
    fn fetching_box(&mut self, ctx: &Context) {
        let Some(job) = &self.site.job else { return };
        let what = t("Fetching buildings from OpenStreetMap…");
        let km2 = tf("{} km²", &[&format!("{:.2}", job.bbox.area_km2())]);
        let mut cancel = false;
        Modal::new(UiId::new("site-fetching"), t("Fetching")).width(420.0).closable(false).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(what).strong());
                });
                ui.label(RichText::new(km2).color(pal().muted));
                ui.add_space(6.0);
                ui.label(t("Overpass query"));
                query_view(ui, &job.query);
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

    /// Every box of the site tools.
    pub(crate) fn site_windows(&mut self, ctx: &Context) {
        self.area_window(ctx);
        self.fetching_box(ctx);
        self.said_window(ctx);
        self.map_window(ctx);
        self.shell_window(ctx);
    }
}
