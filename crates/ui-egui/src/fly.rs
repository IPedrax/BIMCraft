//! Fly mode (FY, or the wing on the 3D view's control bar): the 3D view flown through as a game's camera is. W and S fly
//! forward and back where the view looks, A and D to the side, Q rises and E falls; the mouse looks round, the eye standing
//! still; the wheel sets how fast it flies and Shift flies faster. Esc, FY or the wing again leave it, and so does leaving
//! the 3D view or the window. It is the 3D view's only: a parallel view is turned to perspective as it begins, since a
//! parallel view has no eye to fly.
//!
//! While it flies the pointer is held in the window and hidden (locked where the system can, else kept inside the window),
//! and the mouse's own motion turns the view. The camera is the core's (`Camera::flown`); a camera view writes where it was
//! left back to its camera once fly mode ends, not at every pause, so a flight is one step to undo.

use bimcraft_core::model::{Element, ViewCamera};
use bimcraft_core::view3d::{Camera, P3};
use bimcraft_i18n::msg;
use egui::{Align2, Context, Frame, Key, Margin, Rect, RichText, Vec2, ViewportCommand, pos2, vec2};
use serde_json::json;

use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// How far the view turns for a point the mouse moves, degrees.
const LOOK_DEGREES: f64 = 0.15;
/// The longest a frame flies for, seconds: the first frame after a pause flies no further than a 50 ms one.
const MAX_STEP: f64 = 0.05;
/// How much faster Shift flies.
const BOOST: f64 = 4.0;
/// The speeds it flies at, mm a second: the slowest and the fastest the wheel sets.
const MIN_SPEED: f64 = 100.0;
const MAX_SPEED: f64 = 200_000.0;
/// How much a wheel notch (about 50 points) speeds it up.
const NOTCH: f64 = 1.25;

/// What the window says while it flies.
pub(crate) const HINT: &str =
    msg!("W A S D fly, Q rises, E falls, the mouse looks round, the wheel sets the speed, Shift flies faster; Esc leaves fly mode");
const TIP: &str = msg!(
    "Fly Mode (FY): fly through the 3D view: W, A, S and D move, Q and E rise and fall, the mouse looks round, the wheel sets the \
     speed; Esc leaves it"
);

/// The view flying: how fast, mm a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fly {
    pub(crate) speed: f64,
}

impl Fly {
    /// As it begins over a scene within `bounds`: fast enough to cross it in about eight seconds.
    pub(crate) fn over(bounds: Option<(P3, P3)>) -> Fly {
        let across = bounds.map_or(20_000.0, |(lo, hi)| (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt());
        let speed = if across.is_finite() { (across / 8.0).clamp(500.0, 20_000.0) } else { 2_500.0 };
        Fly { speed }
    }

    /// The speed after the wheel turned `wheel` points (up is faster).
    pub(crate) fn wheeled(self, wheel: f32) -> Fly {
        let notches = f64::from(wheel) / 50.0;
        let speed = self.speed * NOTCH.powf(notches);
        Fly { speed: if speed.is_finite() { speed.clamp(MIN_SPEED, MAX_SPEED) } else { self.speed } }
    }
}

/// The keys held in a frame: ahead (W 1, S -1), to the right (D 1, A -1), up (Q 1, E -1), and Shift.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Keys {
    pub(crate) ahead: f64,
    pub(crate) aside: f64,
    pub(crate) rise: f64,
    pub(crate) boost: bool,
}

impl Keys {
    /// The keys held now, none while a text field has the keyboard or Ctrl, Cmd or Alt is down (Ctrl+S saves, it doesn't
    /// fly back; Alt+drag turns the view).
    pub(crate) fn held(ctx: &Context) -> Keys {
        if ctx.egui_wants_keyboard_input() {
            return Keys::default();
        }
        ctx.input(|i| {
            let m = i.modifiers;
            if m.ctrl || m.command || m.mac_cmd || m.alt {
                return Keys::default();
            }
            let axis = |plus: Key, minus: Key| f64::from(i8::from(i.key_down(plus)) - i8::from(i.key_down(minus)));
            Keys { ahead: axis(Key::W, Key::S), aside: axis(Key::D, Key::A), rise: axis(Key::Q, Key::E), boost: i.modifiers.shift }
        })
    }

    pub(crate) fn moving(&self) -> bool {
        self.ahead != 0.0 || self.aside != 0.0 || self.rise != 0.0
    }
}

/// How far the mouse moved this frame: its own motion where the system gives it (the pointer is held still), else the
/// pointer's.
pub(crate) fn look(ctx: &Context) -> Vec2 {
    ctx.input(|i| i.pointer.motion().unwrap_or_else(|| i.pointer.delta()))
}

/// `cam` flown for `seconds` at `fly`'s speed with `keys` held, and turned as the mouse moved `look` points.
pub(crate) fn step(cam: Camera, fly: Fly, keys: Keys, look: Vec2, seconds: f64) -> Camera {
    let s = if seconds.is_finite() { seconds.clamp(0.0, MAX_STEP) } else { 0.0 };
    let far = fly.speed * s * if keys.boost { BOOST } else { 1.0 };
    let (dx, dy) = (f64::from(look.x), f64::from(look.y));
    let (dx, dy) = if dx.is_finite() && dy.is_finite() { (dx, dy) } else { (0.0, 0.0) };
    cam.flown([keys.ahead * far, keys.aside * far, keys.rise * far], -dx * LOOK_DEGREES, dy * LOOK_DEGREES)
}

/// The words over the view while it flies, at the top of `rect`, the speed it flies at in `speed`.
pub(crate) fn banner(ctx: &Context, rect: Rect, speed: &str) {
    // Below the view's tabs, which float over its top in BIMCraft's profile (as the streamed note keeps clear of them).
    let at = pos2(rect.center().x, rect.top() + 48.0);
    egui::Area::new(egui::Id::new("fly-mode-banner"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(at)
        .constrain_to(rect)
        .show(ctx, |ui| {
            let p = pal();
            Frame::NONE
                .fill(p.select_fill)
                .stroke(egui::Stroke::new(1.0, p.accent))
                .corner_radius(8.0)
                .inner_margin(Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), crate::icons::Icon::Fly, icon, p.accent_ink);
                        ui.label(RichText::new(t("Fly mode")).strong().color(p.accent_ink));
                        ui.label(RichText::new(speed).color(p.accent_ink));
                    });
                    ui.label(RichText::new(t(HINT)).small().color(p.muted));
                });
        });
}

impl BimApp {
    /// Fly Mode (FY, the wing): flies the 3D view, or stops. Elsewhere than the 3D view, says it is the 3D view's.
    pub(crate) fn toggle_fly(&mut self) {
        if self.orbit.fly.is_some() {
            self.land();
            self.status = Status { text: t("Fly mode off").into(), error: false };
            return;
        }
        if self.sheet != Sheet::View3D {
            self.status = Status { text: t("fly mode is for the 3D view: open it (3D) first").into(), error: true };
            return;
        }
        // A parallel view has no eye: it is seen in perspective first, as Revit's walk is.
        let parallel = match self.active_3d().and_then(|id| self.session.model.elements.get(id)) {
            Some(bimcraft_core::model::Element::View3D(v)) => v.lens().is_none(),
            _ => true,
        };
        if parallel {
            let view = self.active_3d().map_or_else(|| json!("{3D}"), |id| json!(id));
            if self.run("perspective", json!({"view": view, "on": true})).is_none() {
                return;
            }
        }
        let bounds = self.orbit.scene.as_ref().and_then(|(_, s)| s.bounds());
        self.orbit.fly = Some(Fly::over(bounds));
        // The banner over the view says how; the status bar only that it flies.
        self.status = Status { text: t("Fly mode on").into(), error: false };
    }

    /// Each frame, before the keys: Esc ends fly mode, as leaving the 3D view or the window does, or a dialog opening (it
    /// takes the keys, and the mouse would go on turning the view behind it); and the window holds the pointer, hidden, while
    /// it flies, and lets it go after.
    pub(crate) fn fly_frame(&mut self, ctx: &Context) {
        if self.orbit.fly.is_some() {
            let focused = ctx.input(|i| i.focused);
            let escape = !ctx.egui_wants_keyboard_input() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            if escape || self.sheet != Sheet::View3D || !focused || self.active_family().is_some() || self.keys_taken(ctx) {
                self.land();
                self.status = Status { text: t("Fly mode off").into(), error: false };
            }
        }
        let hold = self.orbit.fly.is_some();
        if hold != self.orbit.fly_held {
            self.orbit.fly_held = hold;
            if hold {
                // Kept in the window where the system can't lock it in place (X11, where the hidden pointer still roams the
                // window, so `raw_input_hook` drops clicks while it flies): a lock replaces the confinement.
                ctx.send_viewport_cmd(ViewportCommand::CursorGrab(egui::CursorGrab::Confined));
                ctx.send_viewport_cmd(ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
                ctx.send_viewport_cmd(ViewportCommand::CursorVisible(false));
            } else {
                ctx.send_viewport_cmd(ViewportCommand::CursorGrab(egui::CursorGrab::None));
                ctx.send_viewport_cmd(ViewportCommand::CursorVisible(true));
            }
        }
    }

    /// Fly mode ends. A camera view keeps where it flew to, written now as one step to undo: the 3D view's own write-back
    /// runs only while it is drawn, and fly mode also ends by leaving it.
    fn land(&mut self) {
        self.orbit.fly = None;
        let Some(id) = self.active_3d().filter(|_| !self.passive) else { return };
        let (stored, lens) = match self.session.model.elements.get(id) {
            Some(Element::View3D(v)) => (v.camera, v.lens()),
            _ => return,
        };
        let (Some(stored), Some(lens), Some(cam)) = (stored, lens, self.orbit.camera) else { return };
        let Some(eye) = cam.eye() else { return };
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
        let same = near(stored.eye, eye) && near(stored.target, cam.target);
        if !same && self.run("camera", json!({"view": id, "eye": eye, "target": cam.target, "lens": lens})).is_some() {
            self.orbit.built = Some((id, ViewCamera { eye, target: cam.target }, Some(lens)));
        }
    }

    /// The wing on the 3D view's control bar.
    pub(crate) fn fly_button(&mut self, ui: &mut egui::Ui) {
        if crate::small_button(ui, crate::icons::Icon::Fly, "Fly Mode", self.orbit.fly.is_some(), TIP).clicked() {
            self.toggle_fly();
        }
    }
}

/// A speed as the banner shows it: metres a second, or feet in a project in feet and inches.
pub(crate) fn speed_text(units: bimcraft_core::units::Units, speed: f64) -> String {
    match units.imperial() {
        true => crate::lang::tf("{} ft/s", &[&format!("{:.1}", speed / 304.8)]),
        false => crate::lang::tf("{} m/s", &[&format!("{:.1}", speed / 1000.0)]),
    }
}
