//! The window's state and what its handlers do: the hub of this front end, and the one place it meets the core. It reads
//! `session.model`, draws it with `render::draw`, picks with `Drawing::pick`, and changes the model only through
//! `Session::execute`. It holds no widget: the elements are `ui`'s, so everything here is tested without a window.

use bimcraft_core::engine::Session;
use bimcraft_core::ids::Id;
use bimcraft_core::render::{self, Drawing};
use bimcraft_core::sample::SAMPLE;
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::props::{self, Row};
use crate::status;

/// How near a click or a hover must be to a line to take it, points.
const REACH: f32 = 4.0;
/// How far a press may travel and still be a click, points.
const CLICK: f32 = 4.0;
/// The canvas's size before the first layout says it, points.
const FIRST_SIZE: (f32, f32) = (900.0, 640.0);

pub struct App {
    pub session: Session,
    pub level: Option<Id>,
    pub level_name: String,
    pub camera: Camera,
    /// The plan canvas's size, points: what `render::draw` is asked for, taken from the layout.
    pub size: (f32, f32),
    fitted: bool,
    /// The camera has been moved by a person: a resize no longer frames the plan again.
    moved: bool,
    pub drawing: Option<Drawing>,
    /// Counts the drawings made: what the canvas's kept shapes are keyed by.
    pub revision: u64,
    pub hovered: Option<Id>,
    pub selection: Vec<Id>,
    /// The pointer over the plan, points from its top left.
    pub cursor: Option<(f32, f32)>,
    pub props: Vec<Row>,
    /// A command's refusal, as the core says it.
    pub message: Option<String>,
    /// How far the press now dragging has been followed, points.
    followed: (f32, f32),
}

impl App {
    pub fn new(session: Session) -> App {
        let mut app = App {
            session,
            level: None,
            level_name: String::new(),
            camera: Camera { cx: 0.0, cy: 0.0, scale: 0.1, angle: 0.0 },
            size: FIRST_SIZE,
            fitted: false,
            moved: false,
            drawing: None,
            revision: 0,
            hovered: None,
            selection: Vec::new(),
            cursor: None,
            props: Vec::new(),
            message: None,
            followed: (0.0, 0.0),
        };
        app.settle();
        app
    }

    /// The sample house (the engine's own script), the project the window opens on until it can open a file.
    pub fn sample() -> App {
        let mut session = Session::new();
        let made = session.script(SAMPLE);
        let mut app = App::new(session);
        app.message = made.err();
        app
    }

    /// After the model may have changed: the level it draws is one the model has, what is selected still exists, and the plan is
    /// drawn again.
    fn settle(&mut self) {
        let model = &self.session.model;
        let levels = model.levels();
        let kept = self.level.filter(|l| levels.iter().any(|(id, _)| id == l));
        let (level, name) = match kept.or_else(|| levels.first().map(|(id, _)| *id)) {
            Some(id) => (Some(id), levels.iter().find(|(l, _)| *l == id).map(|(_, l)| l.name.clone()).unwrap_or_default()),
            None => (None, String::new()),
        };
        if level != self.level {
            (self.fitted, self.moved) = (false, false);
        }
        (self.level, self.level_name) = (level, name);
        self.selection.retain(|id| self.session.model.elements.get(*id).is_some());
        self.hovered = self.hovered.filter(|id| self.session.model.elements.get(*id).is_some());
        self.refresh();
        self.fit();
        self.redraw();
    }

    /// Frames the level when it is first shown, or shown again at another size.
    fn fit(&mut self) {
        if let (false, Some(level)) = (self.fitted, self.level) {
            let view = render::plan_fit(&self.session.model, level, self.size.0 as u32, self.size.1 as u32, false);
            self.camera = Camera::from_view(view);
            self.fitted = true;
        }
    }

    /// Draws the level for the camera and canvas as they are.
    pub fn redraw(&mut self) {
        self.drawing = self.level.map(|level| render::draw(&self.session.model, level, self.camera.view(self.size.0, self.size.1)));
        self.revision += 1;
    }

    fn refresh(&mut self) {
        self.props = match self.selection.as_slice() {
            [id] => props::rows(&bimcraft_core::element_json(&self.session.model, *id)),
            _ => Vec::new(),
        };
    }

    /// The canvas has laid out at `w` x `h` points.
    pub fn resize(&mut self, w: f32, h: f32) {
        if !(w.is_finite() && h.is_finite()) || w < 2.0 || h < 2.0 || ((w - self.size.0).abs() < 0.5 && (h - self.size.1).abs() < 0.5) {
            return;
        }
        self.size = (w, h);
        // The first real size frames the plan; once a person has moved the camera a resize keeps what it looks at.
        if !self.moved {
            self.fitted = false;
        }
        self.fit();
        self.redraw();
    }

    /// The element nearest a point of the canvas, within [`REACH`].
    pub fn pick(&self, x: f32, y: f32) -> Option<Id> {
        self.drawing.as_ref()?.pick((x, y), REACH)
    }

    /// The pointer is at `x`, `y` over the plan.
    pub fn hover(&mut self, x: f32, y: f32) {
        self.cursor = (x.is_finite() && y.is_finite()).then_some((x, y));
        self.hovered = self.cursor.and_then(|(x, y)| self.pick(x, y));
    }

    pub fn leave(&mut self) {
        (self.cursor, self.hovered) = (None, None);
    }

    /// The pointer's place on the plan in model millimetres.
    pub fn cursor_mm(&self) -> Option<(f64, f64)> {
        let (x, y) = self.cursor?;
        let p = self.camera.view(self.size.0, self.size.1).mm((x, y));
        (p.0.is_finite() && p.1.is_finite()).then_some((p.0, p.1))
    }

    /// The wheel: closer (`factor` above 1) about the pointer.
    pub fn zoom(&mut self, x: f32, y: f32, factor: f32) {
        self.camera.zoom_about((x, y), self.size, f64::from(factor));
        self.moved = true;
        self.redraw();
    }

    /// A press on the plan starts a drag.
    pub fn grab(&mut self) {
        self.followed = (0.0, 0.0);
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the plan follows it.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        self.camera.pan(dx - self.followed.0, dy - self.followed.1, self.size);
        (self.followed, self.moved) = ((dx, dy), true);
        self.redraw();
    }

    /// The press ended at `x`, `y` after travelling `dx`, `dy`: a click if it hardly moved, which picks.
    pub fn release(&mut self, x: f32, y: f32, dx: f32, dy: f32) {
        if dx.hypot(dy) <= CLICK {
            self.select_at(x, y);
        }
    }

    /// Selects what is at the point, or nothing.
    pub fn select_at(&mut self, x: f32, y: f32) {
        self.selection = self.pick(x, y).into_iter().collect();
        self.refresh();
    }

    /// Runs a command; a refusal is kept for the status bar and changes nothing.
    fn run(&mut self, name: &str, params: &Value) -> bool {
        match self.session.execute(name, params) {
            Ok(_) => {
                self.message = None;
                self.settle();
                true
            }
            Err(refusal) => {
                self.message = Some(refusal);
                false
            }
        }
    }

    pub fn delete_selection(&mut self) -> bool {
        !self.selection.is_empty() && self.run("delete", &json!({"ids": self.selection}))
    }

    pub fn undo(&mut self) -> bool {
        self.run("undo", &json!({}))
    }

    pub fn redo(&mut self) -> bool {
        self.run("redo", &json!({}))
    }

    /// The status bar's line.
    pub fn status(&self) -> String {
        let hovered = self.hovered.and_then(|id| self.session.model.elements.get(id).map(|e| (e.category(), id)));
        status::line(&status::Facts {
            level: &self.level_name,
            cursor: self.cursor_mm(),
            hovered,
            selected: self.selection.len(),
            message: self.message.as_deref(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point of the canvas that picks an element, found by looking over the plan.
    fn pickable(app: &App) -> Option<(f32, f32, Id)> {
        let (w, h) = app.size;
        (0..(h as u32))
            .step_by(6)
            .flat_map(|y| (0..(w as u32)).step_by(6).map(move |x| (x as f32, y as f32)))
            .find_map(|(x, y)| app.pick(x, y).map(|id| (x, y, id)))
    }

    #[test]
    fn the_sample_house_opens_on_its_first_level_drawn_and_framed() {
        let app = App::sample();
        assert!(app.message.is_none(), "the sample script runs: {:?}", app.message);
        assert!(app.level.is_some() && !app.level_name.is_empty());
        let d = app.drawing.as_ref().expect("a drawing");
        assert!(!d.prims.is_empty());
        let c = app.camera;
        assert!(c.scale > 0.0 && c.cx.is_finite() && c.cy.is_finite());
        assert!(pickable(&app).is_some(), "something on the plan can be picked");
    }

    #[test]
    fn a_click_selects_what_it_hits_and_shows_its_properties() {
        let mut app = App::sample();
        let (x, y, id) = pickable(&app).expect("a pickable point");
        app.release(x, y, 1.0, 1.0);
        assert_eq!(app.selection, vec![id]);
        assert!(app.props.iter().any(|r| r.name == "id" && r.value == id.to_string()), "{:?}", app.props);
        app.release(x, y, 40.0, 0.0);
        assert_eq!(app.selection, vec![id], "a drag is not a click");
        app.select_at(-1000.0, -1000.0);
        assert!(app.selection.is_empty() && app.props.is_empty());
    }

    #[test]
    fn a_wheel_zooms_about_the_pointer_and_a_drag_pans_the_plan() {
        let mut app = App::sample();
        let before = app.camera;
        let at = (300.0, 200.0);
        let held = app.camera.view(app.size.0, app.size.1).mm(at);
        app.zoom(at.0, at.1, 2.0);
        assert!((app.camera.scale - before.scale * 2.0).abs() < 1e-9);
        let back = app.camera.view(app.size.0, app.size.1).px(held);
        assert!((back.0 - at.0).abs() < 0.01 && (back.1 - at.1).abs() < 0.01, "the point under the wheel stays under it");
        let revision = app.revision;
        app.grab();
        app.drag(10.0, 0.0);
        app.drag(30.0, 5.0);
        let moved = app.camera.view(app.size.0, app.size.1).px(held);
        assert!((moved.0 - (at.0 + 30.0)).abs() < 0.01 && (moved.1 - (at.1 + 5.0)).abs() < 0.01, "followed to the last point: {moved:?}");
        assert!(app.revision > revision, "each move draws again");
    }

    #[test]
    fn deleting_goes_through_the_engine_and_undo_brings_the_element_back() {
        let mut app = App::sample();
        let (x, y, id) = pickable(&app).expect("a pickable point");
        app.select_at(x, y);
        assert!(app.session.model.elements.get(id).is_some());
        assert!(app.delete_selection(), "refused: {:?}", app.message);
        assert!(app.session.model.elements.get(id).is_none(), "the engine deleted it");
        assert!(app.selection.is_empty());
        assert!(app.undo(), "refused: {:?}", app.message);
        assert!(app.session.model.elements.get(id).is_some(), "undo gave it back");
        assert!(!app.delete_selection(), "with nothing selected there is nothing to delete");
    }

    #[test]
    fn a_refusal_is_kept_and_the_model_is_left_as_it_was() {
        let mut app = App::sample();
        let count = app.session.model.elements.len();
        app.selection = vec![1];
        assert!(!app.delete_selection());
        assert!(app.message.is_some(), "the status bar says why");
        assert_eq!(app.session.model.elements.len(), count);
    }

    #[test]
    fn hostile_pointer_numbers_change_nothing() {
        let mut app = App::sample();
        let (camera, revision) = (app.camera, app.revision);
        app.hover(f32::NAN, 1.0);
        assert_eq!(app.cursor, None);
        app.drag(f32::INFINITY, 0.0);
        app.zoom(f32::NAN, 0.0, 2.0);
        app.zoom(0.0, 0.0, f32::NAN);
        app.resize(f32::NAN, 100.0);
        app.resize(0.0, 0.0);
        assert_eq!(app.camera, camera);
        assert_eq!(app.size, FIRST_SIZE);
        assert!(app.revision >= revision);
    }
}
