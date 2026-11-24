//! The window's state and what its handlers do: the hub of this front end, and the one place it meets the core. It reads
//! `session.model`, draws it with `render::draw`, picks with `Drawing::pick`, and changes the model only through
//! `Session::execute`. It holds no widget: the elements are `ui`'s, so everything here is tested without a window.

use bimcraft_core::engine::Session;
use bimcraft_core::ids::Id;
use bimcraft_core::render::{self, Drawing};
use rune::{Pixels, TextEdit};
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::profile::Profile;
use crate::project::{self, Start};
use crate::props::{self, Kind, Row};
use crate::status;

/// How near a click or a hover must be to a line to take it, points.
const REACH: f32 = 4.0;
/// How far a press may travel and still be a click, points.
const CLICK: f32 = 4.0;
/// The canvas's size before the first layout says it, points.
const FIRST_SIZE: (f32, f32) = (900.0, 640.0);

/// How the plan reaches the screen: as Rune shapes (strokes and polygons the renderer draws), or as the core's own raster of it
/// (`render::raster`, tiny-skia on the CPU) shown as one image. `BIMCRAFT_RUNE_PLAN=pixels` picks the second: it is what a machine
/// with no graphics card may do better at, and the measurement in docs/areas/rune-ui.md compares the two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlanMode {
    #[default]
    Shapes,
    Pixels,
}

impl PlanMode {
    pub fn from_env() -> PlanMode {
        match std::env::var("BIMCRAFT_RUNE_PLAN").as_deref() {
            Ok("pixels") => PlanMode::Pixels,
            _ => PlanMode::Shapes,
        }
    }
}

/// A field of the selected element a person may try to change, with the text being typed into it.
pub struct Field {
    pub name: String,
    pub kind: Kind,
    pub edit: TextEdit,
}

pub struct App {
    pub session: Session,
    pub profile: Profile,
    /// The ribbon's tab, an index of `tools::TOOLS`.
    pub tab: usize,
    /// The model's levels, by id and name: what the browser lists.
    pub levels: Vec<(Id, String)>,
    pub mode: PlanMode,
    /// The plan as an image, in [`PlanMode::Pixels`]: replaced whenever the plan is drawn again, shown without the view re-running.
    pub pixels: Pixels,
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
    /// The tool picked in the Tools palette, by its id in `tools::TOOLS`.
    pub tool: Option<&'static str>,
    pub selection: Vec<Id>,
    /// The pointer over the plan, points from its top left.
    pub cursor: Option<(f32, f32)>,
    pub props: Vec<Row>,
    /// The fields of the selected element that can be edited.
    pub fields: Vec<Field>,
    /// Where an edit goes that has no field (a stale one): never read.
    pub scratch: TextEdit,
    /// A command's refusal, as the core says it.
    pub message: Option<String>,
    /// How far the press now dragging has been followed, points.
    followed: (f32, f32),
}

impl App {
    pub fn new(session: Session) -> App {
        let mut app = App {
            session,
            profile: Profile::default(),
            tab: 0,
            levels: Vec::new(),
            mode: PlanMode::from_env(),
            pixels: Pixels::new(),
            level: None,
            level_name: String::new(),
            camera: Camera { cx: 0.0, cy: 0.0, scale: 0.1, angle: 0.0 },
            size: FIRST_SIZE,
            fitted: false,
            moved: false,
            drawing: None,
            revision: 0,
            hovered: None,
            tool: None,
            selection: Vec::new(),
            cursor: None,
            props: Vec::new(),
            fields: Vec::new(),
            scratch: TextEdit::new(""),
            message: None,
            followed: (0.0, 0.0),
        };
        app.settle();
        app
    }

    /// The window as the arguments ask: the project, and the profile it is drawn in.
    pub fn start(start: &Start) -> App {
        let (session, why) = project::load(&start.source);
        let mut app = App::new(session);
        app.profile = start.profile;
        app.message = why;
        app
    }

    /// The sample house (the engine's own script).
    pub fn sample() -> App {
        App::start(&Start::default())
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
        self.levels = levels.iter().map(|(id, l)| (*id, l.name.clone())).collect();
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
        if let (PlanMode::Pixels, Some(d)) = (self.mode, &self.drawing) {
            let view = self.camera.view(self.size.0, self.size.1);
            if let Ok(rgba) = render::raster(d, view.width, view.height, 1.0, (0.0, 0.0), None) {
                self.pixels.set(view.width, view.height, rgba);
            }
        }
    }

    fn refresh(&mut self) {
        let json = match self.selection.as_slice() {
            [id] => bimcraft_core::element_json(&self.session.model, *id),
            _ => serde_json::Value::Null,
        };
        self.props = props::rows(&json);
        self.fields =
            props::editable(&json).into_iter().map(|(name, kind, text)| Field { name, kind, edit: TextEdit::new(text) }).collect();
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

    /// Selects these elements (ids that no longer exist drop out when the model next changes).
    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids;
        self.refresh();
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

    /// Shows another level's plan, framed.
    pub fn set_level(&mut self, id: Id) {
        if self.level == Some(id) || !self.levels.iter().any(|(l, _)| *l == id) {
            return;
        }
        (self.fitted, self.moved) = (false, false);
        self.selection.clear();
        self.hovered = None;
        self.level = Some(id);
        self.settle();
    }

    /// Opens the ribbon's tab `i` of `tools::TOOLS`, if there is one.
    pub fn set_tab(&mut self, i: usize) {
        if i < crate::tools::TOOLS.len() {
            self.tab = i;
        }
    }

    /// Picks the tool with this id, or puts it down when it is the one in hand; an id the palette does not have picks nothing.
    pub fn set_tool(&mut self, id: &str) {
        let picked = crate::tools::id_of(id);
        self.tool = if picked == self.tool { None } else { picked };
    }

    /// The text typed into field `i` of the selected element goes to the core as a `modify`; a number that is no number, or a refusal,
    /// is said in the status bar and changes nothing.
    pub fn commit_field(&mut self, i: usize) -> bool {
        let (Some(field), [id]) = (self.fields.get(i), self.selection.as_slice()) else { return false };
        let (name, id) = (field.name.clone(), *id);
        match props::parse(field.kind, &field.edit.text) {
            Ok(value) => self.run("modify", &json!({"id": id, name: value})),
            Err(why) => {
                self.message = Some(format!("{name}: {why}"));
                false
            }
        }
    }

    /// Opens the project at `path`, framed on its first level.
    pub fn open_path(&mut self, path: std::path::PathBuf) -> bool {
        let opened = self.run("open", &json!({ "path": path }));
        if opened {
            (self.fitted, self.moved) = (false, false);
            self.selection.clear();
            self.settle();
        }
        opened
    }

    /// Whether the project has a file to save to.
    pub fn has_file(&self) -> bool {
        self.session.path.is_some()
    }

    pub fn save(&mut self) -> bool {
        self.run("save", &json!({}))
    }

    pub fn save_as(&mut self, path: std::path::PathBuf) -> bool {
        self.run("save", &json!({ "path": path }))
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
            tool: self.tool.and_then(crate::tools::label),
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
    fn the_plan_can_be_the_cores_own_raster_shown_as_one_image() {
        let mut app = App::sample();
        app.mode = PlanMode::Pixels;
        app.redraw();
        let size = app.pixels.with_rgba(|w, h, bytes| (w, h, bytes.len()));
        let (w, h, len) = size.expect("an image");
        assert_eq!((w as f32, h as f32), app.size);
        assert_eq!(len, (w * h * 4) as usize, "RGBA, row by row");
        let ink = app.pixels.with_rgba(|_, _, bytes| bytes.chunks(4).any(|p| p[0] < 128)).unwrap_or(false);
        assert!(ink, "the plan has dark lines on its paper");
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
    fn another_level_is_shown_framed_and_a_stale_one_is_refused() {
        let mut app = App::sample();
        assert!(app.levels.len() >= 2, "the sample house has two levels: {:?}", app.levels);
        let first = app.level;
        let other = app.levels.iter().map(|(id, _)| *id).find(|id| Some(*id) != first).expect("a second level");
        app.selection = vec![1];
        let revision = app.revision;
        app.set_level(other);
        assert_eq!(app.level, Some(other));
        assert!(app.selection.is_empty() && app.revision > revision && app.drawing.is_some());
        assert_eq!(app.level_name, app.levels.iter().find(|(id, _)| *id == other).map(|(_, n)| n.clone()).unwrap_or_default());
        app.set_level(7);
        assert_eq!(app.level, Some(other), "an id that is no level changes nothing");
        app.set_tab(3);
        app.set_tab(99);
        assert_eq!(app.tab, 3);
    }

    #[test]
    fn a_field_typed_into_goes_to_the_engine_as_a_modify_and_a_bad_number_never_does() {
        let mut app = App::sample();
        let level = app.level.expect("a level");
        app.selection = vec![level];
        app.refresh();
        let name = app.fields.iter().position(|f| f.name == "name").expect("a level has a name to edit");
        assert_eq!(app.fields[name].kind, Kind::Text);
        app.fields[name].edit = TextEdit::new("Ground");
        assert!(app.commit_field(name), "refused: {:?}", app.message);
        assert_eq!(app.level_name, "Ground", "the engine renamed the level and the window follows");
        assert!(app.undo(), "one undo step: {:?}", app.message);
        assert_ne!(app.level_name, "Ground");

        app.selection = vec![level];
        app.refresh();
        let number = app.fields.iter().position(|f| f.kind == Kind::Number).expect("a number to edit");
        app.fields[number].edit = TextEdit::new("lots");
        let before = app.revision;
        assert!(!app.commit_field(number));
        assert!(app.message.as_deref().is_some_and(|m| m.contains("not a number")), "{:?}", app.message);
        assert_eq!(app.revision, before, "nothing was asked of the engine");
        assert!(!app.commit_field(999), "a field that is not there asks nothing");
    }

    #[test]
    fn a_saved_project_opens_again_framed_on_its_first_level() {
        let dir = std::env::temp_dir().join(format!("bimcraft-rune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let file = dir.join("house.bimc");
        let mut app = App::sample();
        assert!(!app.has_file());
        assert!(app.save_as(file.clone()), "refused: {:?}", app.message);
        assert!(app.has_file() && file.exists());
        assert!(app.save(), "a saved project saves to its own file: {:?}", app.message);
        let mut other = App::new(Session::new());
        assert_ne!(other.levels, app.levels, "a new project is not the sample house");
        assert!(other.open_path(file.clone()), "refused: {:?}", other.message);
        assert_eq!(other.levels, app.levels);
        assert!(other.drawing.is_some());
        assert!(!other.open_path(dir.join("missing.bimc")) && other.message.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tool_is_picked_put_down_and_a_stale_id_picks_nothing() {
        let mut app = App::sample();
        app.set_tool("wall");
        assert_eq!(app.tool, Some("wall"));
        assert!(app.status().contains("Wall"), "the bar names the tool: {}", app.status());
        app.set_tool("door");
        assert_eq!(app.tool, Some("door"));
        app.set_tool("door");
        assert_eq!(app.tool, None, "a second press puts it down");
        app.set_tool("no such tool");
        assert_eq!(app.tool, None);
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
