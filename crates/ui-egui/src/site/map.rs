//! The map a place or an area is picked on: a window with OpenStreetMap's map in it, to drag and zoom, where a click sets the
//! project's origin (Pick on Map in the Location box) or a drag draws the box to fetch (Pick on Map in the area box). What it
//! draws is `bimcraft_core::map`'s: this file is the window, the tile loader and the pictures.
//!
//! Tiles come from the fetcher the session holds (`GeoSource::tile`), two at a time on threads of their own, and only the ones the
//! window shows (what is no longer in view is dropped from the queue). With no fetcher, or no tile, the map is a plain ground with
//! its lines of latitude and longitude: the place can still be picked on it, or typed. The credit the tiles are owed is in their
//! corner.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bimcraft_core::geo::{Bbox, Fetcher, MAX_FETCH_AREA_KM2, Tile};
use bimcraft_core::georef::parse_coordinates;
use bimcraft_core::map::{MapView, TileKey, box_note};
use egui::{
    Align, Align2, Button, Color32, ColorImage, Context, FontId, Id as UiId, Layout, PointerButton, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// What the map is picked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pick {
    /// A place: the project's origin.
    Place,
    /// A box: the area to fetch.
    Area,
}

/// A tile of the map as the window holds it.
enum Slot {
    /// Asked for, not here yet.
    Pending,
    Ready(TextureHandle),
    /// It would not come; asked again after a while.
    Failed(Instant),
}

/// How long a tile that would not come is left before it is asked for again.
const RETRY: Duration = Duration::from_secs(30);
/// The most tiles held as pictures; the ones out of view go first.
const HOLD: usize = 300;
/// How many tiles are fetched at once: OpenStreetMap's tile policy allows two connections.
const WORKERS: usize = 2;
/// The map's size in the window.
const HEIGHT: f32 = 420.0;

/// The threads that fetch tiles: they take keys off a queue, skip what the window no longer wants, and answer on a channel.
struct Loader {
    ask: Sender<TileKey>,
    got: Receiver<(TileKey, Result<Option<Tile>, String>)>,
    /// What the window shows now: a key not in it is not fetched.
    wanted: Arc<Mutex<HashSet<TileKey>>>,
}

impl Loader {
    fn start(geo: Fetcher, ctx: &Context) -> Loader {
        let (ask, queue) = channel::<TileKey>();
        let (answer, got) = channel();
        let queue = Arc::new(Mutex::new(queue));
        let wanted: Arc<Mutex<HashSet<TileKey>>> = Arc::default();
        for i in 0..WORKERS {
            let (geo, queue, answer, wanted, wake) = (geo.clone(), queue.clone(), answer.clone(), wanted.clone(), ctx.clone());
            let spawned = std::thread::Builder::new().name(format!("map-tiles-{i}")).spawn(move || {
                // The lock is held only while waiting for a key: the other thread takes the next.
                while let Some(key) = queue.lock().ok().and_then(|q| q.recv().ok()) {
                    let wants = wanted.lock().map(|w| w.contains(&key)).unwrap_or(false);
                    let result = if wants {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| geo.tile(key.zoom, key.x, key.y)))
                            .unwrap_or_else(|_| Err("the fetcher stopped on this tile".to_string()))
                            .and_then(|tile| tile.check().map(|_| Some(tile)))
                    } else {
                        Ok(None)
                    };
                    if answer.send((key, result)).is_err() {
                        break;
                    }
                    wake.request_repaint();
                }
            });
            // A machine that cannot make a thread has the map without a picture.
            if spawned.is_err() {
                break;
            }
        }
        Loader { ask, got, wanted }
    }
}

/// The map window: where it is, what is picked on it and the tiles it holds.
pub(crate) struct MapBox {
    pick: Pick,
    view: MapView,
    /// The place picked, longitude and latitude.
    place: Option<(f64, f64)>,
    /// The box being drawn or drawn, and the corner it was begun at.
    area: Option<Bbox>,
    corner: Option<(f64, f64)>,
    /// In an area map: whether a drag draws the box (else it moves the map).
    drawing: bool,
    /// The project's origin, shown on the map.
    origin: Option<(f64, f64)>,
    tiles: HashMap<TileKey, Slot>,
    loader: Option<Loader>,
    /// What was typed to go to.
    goto: String,
    /// Something to say under the map: why a tile did not come, why a place is not one.
    note: Option<String>,
    /// Where the map is on the screen, last frame.
    canvas: Option<Rect>,
    /// How many tiles were asked for in all.
    asked: usize,
}

impl MapBox {
    fn new(pick: Pick, view: MapView) -> MapBox {
        MapBox {
            pick,
            view,
            place: None,
            area: None,
            corner: None,
            drawing: true,
            origin: None,
            tiles: HashMap::new(),
            loader: None,
            goto: String::new(),
            note: None,
            canvas: None,
            asked: 0,
        }
    }

    /// Where a place is on the screen, when the map has been drawn.
    #[cfg(test)]
    pub(crate) fn screen_of(&self, lon: f64, lat: f64) -> Option<Pos2> {
        let r = self.canvas?;
        let (x, y) = self.view.to_screen(lon, lat, (f64::from(r.width()), f64::from(r.height())));
        Some(pos2(r.min.x + x as f32, r.min.y + y as f32))
    }

    #[cfg(test)]
    pub(crate) fn canvas_rect(&self) -> Rect {
        self.canvas.unwrap_or(Rect::NOTHING)
    }

    #[cfg(test)]
    pub(crate) fn canvas_centre(&self) -> Pos2 {
        self.canvas_rect().center()
    }

    #[cfg(test)]
    pub(crate) fn view(&self) -> MapView {
        self.view
    }

    #[cfg(test)]
    pub(crate) fn place(&self) -> Option<(f64, f64)> {
        self.place
    }

    #[cfg(test)]
    pub(crate) fn area(&self) -> Option<Bbox> {
        self.area
    }

    /// How many tiles the map asked for, in all.
    #[cfg(test)]
    pub(crate) fn asked(&self) -> usize {
        self.asked
    }

    /// How many of its tiles are pictures now.
    #[cfg(test)]
    pub(crate) fn ready(&self) -> usize {
        self.tiles.values().filter(|s| matches!(s, Slot::Ready(_))).count()
    }
}

/// A degree as a field holds it: seven places, no trailing zeros.
fn degrees(x: f64) -> String {
    let text = format!("{x:.7}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl BimApp {
    /// The map, while it is open.
    #[cfg(test)]
    pub(crate) fn map(&self) -> Option<&MapBox> {
        self.site.map.as_ref()
    }

    /// Opens the map to pick a place (for the Location box) or a box (for the area box), at what the box holds already, or at the
    /// project's origin, or at the world.
    pub(crate) fn open_map(&mut self, pick: Pick) {
        let origin = self.session.model.location.map(|l| (l.longitude, l.latitude));
        let world = MapView::new(10.0, 30.0, 2.0);
        let size = (720.0, f64::from(HEIGHT));
        let typed = |s: &str| crate::properties::number(s);
        let mut b = match pick {
            Pick::Place => {
                let here = self.site.location.as_ref().and_then(|l| Some((typed(&l.longitude)?, typed(&l.latitude)?))).or(origin);
                let mut b = MapBox::new(pick, here.map_or(world, |(lon, lat)| MapView::new(lon, lat, 15.0)));
                b.place = self.site.location.as_ref().and_then(|l| Some((typed(&l.longitude)?, typed(&l.latitude)?)));
                b
            }
            Pick::Area => {
                let drawn = self
                    .site
                    .area
                    .as_ref()
                    .filter(|a| !a.around)
                    .and_then(|a| Bbox::new(typed(&a.south)?, typed(&a.west)?, typed(&a.north)?, typed(&a.east)?).ok());
                let view = match (drawn, origin) {
                    (Some(x), _) => MapView::fit(&x, size),
                    (None, Some((lon, lat))) => MapView::new(lon, lat, 15.0),
                    (None, None) => world,
                };
                let mut b = MapBox::new(pick, view);
                b.area = drawn;
                b
            }
        };
        b.origin = origin;
        self.site.map = Some(b);
    }

    /// What the picked place or box goes into: the Location box's latitude and longitude, or the area box's four degrees.
    fn use_map(&mut self) {
        let Some(b) = self.site.map.take() else { return };
        match (b.pick, b.place, b.area) {
            (Pick::Place, Some((lon, lat)), _) => {
                if let Some(l) = self.site.location.as_mut() {
                    let (latitude, longitude) = (degrees(lat), degrees(lon));
                    (l.coordinates, l.coordinates_note, l.error) = (format!("{latitude}, {longitude}"), None, None);
                    (l.latitude, l.longitude) = (latitude, longitude);
                }
            }
            (Pick::Area, _, Some(x)) => {
                if let Some(a) = self.site.area.as_mut() {
                    a.around = false;
                    (a.south, a.west, a.north, a.east, a.error) =
                        (degrees(x.south), degrees(x.west), degrees(x.north), degrees(x.east), None);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn map_window(&mut self, ctx: &Context) {
        let geo = self.session.geo.clone();
        let Some(b) = self.site.map.as_mut() else { return };
        if b.loader.is_none()
            && let Some(geo) = geo.clone()
        {
            b.loader = Some(Loader::start(geo, ctx));
        }
        let online = geo.is_some();
        // What the threads brought.
        if let Some(l) = &b.loader {
            while let Ok((key, result)) = l.got.try_recv() {
                match result {
                    Ok(Some(tile)) if b.tiles.contains_key(&key) => {
                        let side = tile.side as usize;
                        let image = ColorImage::from_rgba_unmultiplied([side, side], &tile.rgba);
                        let name = format!("map-{}-{}-{}", key.zoom, key.x, key.y);
                        b.tiles.insert(key, Slot::Ready(ctx.load_texture(name, image, TextureOptions::LINEAR)));
                        b.note = None;
                    }
                    // Not wanted any more, or not held any more.
                    Ok(_) => {
                        if matches!(b.tiles.get(&key), Some(Slot::Pending)) {
                            b.tiles.remove(&key);
                        }
                    }
                    Err(e) => {
                        if b.tiles.contains_key(&key) {
                            b.tiles.insert(key, Slot::Failed(Instant::now()));
                        }
                        b.note = Some(e);
                    }
                }
            }
        }
        let title = match b.pick {
            Pick::Place => t("Pick the Project's Place on the Map"),
            Pick::Area => t("Pick an Area on the Map"),
        };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("site-map"), title).width(760.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                controls(ui, b);
                ui.add_space(4.0);
                canvas(ui, b, online);
                readout(ui, b, online);
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    let (label, ready) = match b.pick {
                        Pick::Place => (t("Use This Place"), b.place.is_some()),
                        Pick::Area => (t("Use This Box"), b.area.is_some_and(|x| box_note(&x).1)),
                    };
                    ok = ui.add_enabled(ready, Button::new(label)).clicked();
                });
            });
        });
        if ok {
            self.use_map();
        } else if close || r.should_close() {
            self.site.map = None;
        }
    }
}

/// The row above the map: zoom, how a drag acts, and a pair of coordinates to go to.
fn controls(ui: &mut Ui, b: &mut MapBox) {
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text(t("Zoom in")).clicked() {
            b.view.zoom_at((360.0, f64::from(HEIGHT) / 2.0), (720.0, f64::from(HEIGHT)), 1.0);
        }
        if ui.button("−").on_hover_text(t("Zoom out")).clicked() {
            b.view.zoom_at((360.0, f64::from(HEIGHT) / 2.0), (720.0, f64::from(HEIGHT)), -1.0);
        }
        if b.pick == Pick::Area {
            ui.separator();
            ui.radio_value(&mut b.drawing, true, t("Draw box")).on_hover_text(t("A drag draws the box to fetch"));
            ui.radio_value(&mut b.drawing, false, t("Move map"))
                .on_hover_text(t("A drag moves the map (so does the wheel pressed, or the right button)"));
        }
        ui.separator();
        let l = ui.label(t("Go to"));
        let field = ui.add_sized([180.0, 20.0], TextEdit::singleline(&mut b.goto).hint_text("51.2254809, 6.7700762")).labelled_by(l.id);
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(t("Go")).clicked() || enter) && !b.goto.trim().is_empty() {
            match parse_coordinates(&b.goto) {
                Ok((lat, lon)) => {
                    b.view = MapView::new(lon, lat, b.view.zoom.max(14.0));
                    if b.pick == Pick::Place {
                        b.place = Some((lon, lat));
                    }
                    b.note = None;
                }
                Err(e) => b.note = Some(e),
            }
        }
    });
}

/// The map itself: its tiles (or its lines), what is picked on it, and the dragging, wheel and clicks that move and pick.
fn canvas(ui: &mut Ui, b: &mut MapBox, online: bool) {
    let width = ui.available_width().clamp(320.0, 760.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, t("Map")));
    b.canvas = Some(rect);
    let size = (f64::from(rect.width()), f64::from(rect.height()));
    let local = |p: Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
    let hover = response.hover_pos().map(local);

    // The wheel zooms about the pointer; a double click zooms in on it.
    if let Some(p) = hover {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            b.view.zoom_at(p, size, f64::from(wheel) / 240.0);
        }
        if response.double_clicked() {
            b.view.zoom_at(p, size, 1.0);
        }
    }
    let drawing = b.pick == Pick::Area && b.drawing;
    let moved = response.dragged_by(PointerButton::Secondary)
        || response.dragged_by(PointerButton::Middle)
        || (response.dragged_by(PointerButton::Primary) && !drawing);
    if moved {
        let d = response.drag_delta();
        b.view.pan(f64::from(d.x), f64::from(d.y));
    }
    if drawing {
        if response.drag_started_by(PointerButton::Primary) {
            b.corner = ui.input(|i| i.pointer.press_origin()).map(|p| b.view.from_screen(local(p), size));
            b.area = None;
        }
        if response.dragged_by(PointerButton::Primary)
            && let (Some((lon, lat)), Some(now)) = (b.corner, response.interact_pointer_pos().map(local))
        {
            let from = b.view.to_screen(lon, lat, size);
            match b.view.box_between(from, now, size) {
                Ok(x) => {
                    b.area = Some(x);
                    b.note = None;
                }
                Err(_) => b.area = None,
            }
        }
    } else if b.pick == Pick::Place
        && response.clicked_by(PointerButton::Primary)
        && let Some(p) = response.interact_pointer_pos()
    {
        b.place = Some(b.view.from_screen(local(p), size));
        b.note = None;
    }

    // Ask for the tiles in view, and let go of the rest.
    let placed = b.view.visible(size);
    let keys: HashSet<TileKey> = placed.iter().map(|p| p.key).collect();
    b.tiles.retain(|_, s| !matches!(s, Slot::Failed(at) if at.elapsed() > RETRY));
    if let Some(l) = &b.loader {
        if let Ok(mut w) = l.wanted.lock() {
            *w = keys.clone();
        }
        for p in &placed {
            if let std::collections::hash_map::Entry::Vacant(slot) = b.tiles.entry(p.key) {
                slot.insert(Slot::Pending);
                b.asked += 1;
                let _ = l.ask.send(p.key);
            }
        }
    }
    if b.tiles.len() > HOLD {
        b.tiles.retain(|k, _| keys.contains(k));
    }

    let painter = ui.painter_at(rect);
    let at = |x: f64, y: f64| rect.min + vec2(x as f32, y as f32);
    painter.rect_filled(rect, 0.0, pal().panel);
    let mut any = false;
    for p in &placed {
        let r = Rect::from_min_size(at(p.at.0, p.at.1), Vec2::splat(p.side as f32));
        if let Some(Slot::Ready(tex)) = b.tiles.get(&p.key) {
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            any = true;
        }
    }
    // A map with no picture still has its lines of latitude and longitude.
    if !any {
        let (lons, lats) = b.view.graticule(size);
        let line = Stroke::new(1.0, pal().muted.gamma_multiply(0.5));
        let font = FontId::proportional(10.0);
        let middle = b.view.from_screen((size.0 / 2.0, size.1 / 2.0), size);
        for lon in lons {
            let x = b.view.to_screen(lon, middle.1, size).0;
            painter.vline(rect.min.x + x as f32, rect.y_range(), line);
            painter.text(at(x + 3.0, 2.0), Align2::LEFT_TOP, format!("{lon}°"), font.clone(), pal().muted);
        }
        for lat in lats {
            let y = b.view.to_screen(middle.0, lat, size).1;
            painter.hline(rect.x_range(), rect.min.y + y as f32, line);
            painter.text(at(2.0, y + 2.0), Align2::LEFT_TOP, format!("{lat}°"), font.clone(), pal().muted);
        }
        let why = if online { t("Map tiles are not here (yet)") } else { t("No map: this BIMCraft has no network access") };
        painter.text(rect.center_top() + vec2(0.0, 18.0), Align2::CENTER_TOP, why, FontId::proportional(13.0), pal().muted);
    }

    // The project's origin, the place picked and the box drawn.
    if let Some((lon, lat)) = b.origin {
        let (x, y) = b.view.to_screen(lon, lat, size);
        let c = at(x, y);
        let s = Stroke::new(1.5, Color32::from_rgb(40, 40, 40));
        painter.circle_stroke(c, 5.0, s);
        painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], s);
        painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], s);
    }
    if let Some((lon, lat)) = b.place {
        let (x, y) = b.view.to_screen(lon, lat, size);
        let c = at(x, y);
        painter.circle(c, 7.0, Color32::from_rgb(220, 40, 40), Stroke::new(2.0, Color32::WHITE));
        painter.circle_filled(c, 2.0, Color32::WHITE);
    }
    if let Some(x) = b.area {
        let ((x0, y0), (x1, y1)) = (b.view.to_screen(x.west, x.north, size), b.view.to_screen(x.east, x.south, size));
        let over = !box_note(&x).1;
        let colour = if over { Color32::from_rgb(220, 40, 40) } else { Color32::from_rgb(30, 110, 220) };
        let r = Rect::from_min_max(at(x0, y0), at(x1, y1));
        painter.rect_filled(r, 0.0, colour.gamma_multiply(0.18));
        painter.rect_stroke(r, 0.0, Stroke::new(2.0, colour), StrokeKind::Inside);
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, pal().muted), StrokeKind::Inside);
    // The credit the tiles are owed, where it is read.
    if any || b.loader.is_some() {
        let credit = "© OpenStreetMap contributors";
        let galley = painter.layout_no_wrap(credit.to_string(), FontId::proportional(10.0), Color32::from_gray(40));
        let r = Rect::from_min_size(rect.right_bottom() - galley.size() - vec2(8.0, 6.0), galley.size()).expand(3.0);
        painter.rect_filled(r, 2.0, Color32::from_white_alpha(200));
        painter.galley(r.min + vec2(3.0, 3.0), galley, Color32::from_gray(40));
    }
}

/// What is under the map: where the pointer is, and what is picked, with the box's area against what one fetch may cover.
fn readout(ui: &mut Ui, b: &MapBox, online: bool) {
    let hovered = b.canvas.and_then(|r| {
        let p = ui.input(|i| i.pointer.hover_pos())?;
        r.contains(p).then(|| {
            b.view.from_screen((f64::from(p.x - r.min.x), f64::from(p.y - r.min.y)), (f64::from(r.width()), f64::from(r.height())))
        })
    });
    ui.horizontal(|ui| {
        match hovered {
            Some((lon, lat)) => ui.label(RichText::new(format!("{}, {}", degrees(lat), degrees(lon))).monospace().color(pal().muted)),
            None => ui.label(RichText::new(t("Drag to move, wheel to zoom, double click to zoom in")).color(pal().muted)),
        };
    });
    match (b.pick, b.place, b.area) {
        (Pick::Place, Some((lon, lat)), _) => {
            ui.label(RichText::new(tf("Place: {}, {}", &[&degrees(lat), &degrees(lon)])).strong());
        }
        (Pick::Place, None, _) => {
            ui.label(t("Click the place: it becomes the project's origin."));
        }
        (Pick::Area, _, Some(x)) => {
            let (km2, fits) = box_note(&x);
            let text = tf("{} km² (at most {})", &[&format!("{km2:.2}"), &MAX_FETCH_AREA_KM2.to_string()]);
            ui.label(if fits { RichText::new(text).strong() } else { RichText::new(text).color(pal().error) });
        }
        (Pick::Area, _, None) => {
            ui.label(t("Drag a box over the area to fetch."));
        }
    }
    if let Some(n) = &b.note {
        ui.label(RichText::new(n).color(pal().muted));
    }
    if !online {
        ui.label(RichText::new(t("The place can still be picked on the lines, or typed.")).color(pal().muted));
    }
}
