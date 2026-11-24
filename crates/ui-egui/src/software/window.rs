//! The window drawn on the processor: winit's window and event loop, egui-winit for input, the clipboard, IME, the cursor and
//! the window's commands (as eframe uses them), `raster.rs` for the pixels and `softbuffer` to show them.
//!
//! It draws only when asked: an event egui wants, a repaint egui asked for (its repaint callback wakes the loop at the time
//! it asked for), or the system's. Idle, it waits and takes no processor time. A frame that draws exactly what the last one
//! drew, with no texture changed and nothing exposed, isn't drawn or shown again: a pointer moving over nothing that
//! highlights costs the app's pass and nothing more. The app paces its frames itself (`pacing.rs`), as under eframe.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::epaint::textures::TexturesDelta;
use egui::{Color32, ColorImage, Context, Event, Pos2, Vec2, ViewportBuilder, ViewportCommand, ViewportEvent, ViewportId, ViewportInfo};
use egui_winit::winit;
use egui_winit::{ActionRequested, State};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use super::raster::{MAX_TEXTURE_SIDE, Raster, fingerprint};
use crate::{BimApp, Host};

/// What the window is cleared to under the panels (eframe's clear colour, opaque).
const CLEAR: Color32 = Color32::from_rgb(12, 12, 12);
/// The most pixels a window's buffer holds.
const MAX_PIXELS: usize = 16_384 * 16_384;
/// How long a hidden or minimised window waits between its frames, as eframe's does.
const HIDDEN: Duration = Duration::from_millis(10);

/// What wakes the loop: egui asked for a frame at this time.
#[derive(Debug)]
enum Wake {
    At(Instant),
}

/// What the app does once the window exists: what eframe's creation closure does.
pub type Start = Box<dyn FnOnce(&mut BimApp, &Context)>;

/// Opens the window `viewport` describes, centred, and runs `app` in it until it closes. `start` runs once the window is up.
pub fn run(app: BimApp, viewport: ViewportBuilder, start: Start) -> Result<(), String> {
    let event_loop = EventLoop::<Wake>::with_user_event().build().map_err(|e| format!("couldn't start the window's event loop: {e}"))?;
    let ctx = Context::default();
    let proxy = event_loop.create_proxy();
    ctx.set_request_repaint_callback(move |info| {
        let _ = proxy.send_event(Wake::At(Instant::now() + info.delay));
    });
    let mut runner = Runner {
        app,
        ctx,
        builder: viewport,
        start: Some(start),
        window: None,
        state: None,
        surface: None,
        info: ViewportInfo::default(),
        actions: Vec::new(),
        raster: Raster::default(),
        pending: TexturesDelta::default(),
        held: None,
        next: None,
        fresh: true,
        shown: false,
        last: None,
        cpu: None,
        times: std::env::var_os("BIMCRAFT_SOFT_TIMES").is_some_and(|v| !v.is_empty() && v != "0"),
        error: None,
    };
    event_loop.run_app(&mut runner).map_err(|e| format!("the window's event loop failed: {e}"))?;
    runner.error.take().map_or(Ok(()), Err)
}

/// Where a window of `inner` points goes to be centred on a monitor at `at` of `size` points: never above or left of it.
pub fn centred(inner: Vec2, at: Pos2, size: Vec2) -> Pos2 {
    at + ((size - inner) * 0.5).max(Vec2::ZERO)
}

/// Whether a frame that saw the window asked to close, with these commands from the app, ends the run: unless it said
/// `CancelClose` (the unsaved-changes prompt).
pub fn closes(close_requested: bool, commands: &[ViewportCommand]) -> bool {
    close_requested && !commands.contains(&ViewportCommand::CancelClose)
}

struct Runner {
    app: BimApp,
    ctx: Context,
    builder: ViewportBuilder,
    start: Option<Start>,
    window: Option<Rc<Window>>,
    state: Option<State>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    info: ViewportInfo,
    actions: Vec<ActionRequested>,
    raster: Raster,
    /// The textures egui changed since the last frame drawn.
    pending: TexturesDelta,
    /// Input a hidden window's frame didn't use, for the next.
    held: Option<egui::RawInput>,
    /// When egui asked for the next frame.
    next: Option<Instant>,
    /// The window's pixels must be drawn again whatever the frame (resized, exposed, first).
    fresh: bool,
    shown: bool,
    /// The last frame shown: its fingerprint.
    last: Option<u64>,
    /// What the last frame took, for the benchmark.
    cpu: Option<f32>,
    /// Each frame's times on standard error (`BIMCRAFT_SOFT_TIMES=1`), to see where a slow frame goes.
    times: bool,
    error: Option<String>,
}

/// The window, as the app's frames see it.
struct Soft<'a> {
    window: &'a Window,
    cpu: Option<f32>,
}

impl Host for Soft<'_> {
    fn refresh_millihertz(&self) -> Option<u32> {
        self.window.current_monitor()?.refresh_rate_millihertz()
    }

    fn cpu_usage(&self) -> Option<f32> {
        self.cpu
    }

    fn configure_surface(
        &mut self,
        _: &mut dyn FnMut(eframe::wgpu::Backend, eframe::egui_wgpu::SurfaceConfig) -> eframe::egui_wgpu::SurfaceConfig,
    ) {
    }
}

impl Runner {
    fn fail(&mut self, event_loop: &ActiveEventLoop, what: String) {
        self.error = Some(what);
        event_loop.exit();
    }

    /// The window as `builder` says, hidden until its first frame, centred on the main monitor and no bigger than it.
    fn open(&mut self, event_loop: &ActiveEventLoop) {
        let mut builder = self.builder.clone().with_visible(false);
        if let Some(monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
            let scale = monitor.scale_factor();
            let size = monitor.size().to_logical::<f32>(scale);
            let at = monitor.position().to_logical::<f32>(scale);
            let (size, at) = (Vec2::new(size.width, size.height), Pos2::new(at.x, at.y));
            if let Some(inner) = builder.inner_size {
                let inner = inner.min(size);
                builder = builder.with_inner_size(inner);
                if builder.position.is_none() {
                    builder = builder.with_position(centred(inner, at, size));
                }
            }
        }
        let window = match egui_winit::create_window(&self.ctx, event_loop, &builder) {
            Ok(w) => Rc::new(w),
            Err(e) => return self.fail(event_loop, format!("couldn't open the window: {e}")),
        };
        let surface = softbuffer::Context::new(Rc::clone(&window)).and_then(|c| softbuffer::Surface::new(&c, Rc::clone(&window)));
        match surface {
            Ok(s) => self.surface = Some(s),
            Err(e) => return self.fail(event_loop, format!("couldn't draw in the window: {e}")),
        }
        self.state = Some(State::new(
            self.ctx.clone(),
            ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(MAX_TEXTURE_SIDE),
        ));
        egui_winit::update_viewport_info(&mut self.info, &self.ctx, &window, true);
        if let Some(start) = self.start.take() {
            start(&mut self.app, &self.ctx);
        }
        window.request_redraw();
        self.window = Some(window);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let began = Instant::now();
        let (Some(window), Some(state)) = (self.window.clone(), self.state.as_mut()) else { return };
        egui_winit::update_viewport_info(&mut self.info, &self.ctx, &window, false);
        let mut raw = self.held.take().unwrap_or_default();
        raw.append(state.take_egui_input(&window));
        raw.viewports = std::iter::once((ViewportId::ROOT, self.info.clone())).collect();
        self.app.input_hook(&self.ctx, &mut raw);
        let close_requested = raw.viewport().close_requested();
        let visible = self.info.visible().unwrap_or(true) && self.info.minimized != Some(true);
        let mut host = Soft { window: &window, cpu: self.cpu };
        let (app, ctx) = (&mut self.app, &self.ctx);
        if !visible {
            // Hidden or minimised: the app's logic still runs (agents, Co-Work), its input kept for the frame that shows.
            let out = ctx.run_logic(&raw, |ctx| app.logic_on(ctx, &mut host));
            self.held = Some(raw);
            self.info.events.clear();
            state.handle_platform_output(&window, out.platform_output);
            let commands = out.viewport_commands.get(&ViewportId::ROOT).cloned().unwrap_or_default();
            let close = closes(close_requested, &commands);
            egui_winit::process_viewport_commands(ctx, &mut self.info, commands, &window, &mut self.actions);
            if close {
                event_loop.exit();
            }
            std::thread::sleep(HIDDEN);
            return;
        }
        let ran = Instant::now();
        let full = ctx.run_ui(raw, |ui| {
            app.logic_on(ui.ctx(), &mut host);
            app.show(ui);
        });
        self.info.events.clear();
        state.handle_platform_output(&window, full.platform_output);
        let commands = full.viewport_output.get(&ViewportId::ROOT).map(|o| o.commands.clone()).unwrap_or_default();
        let close = closes(close_requested, &commands);
        egui_winit::process_viewport_commands(ctx, &mut self.info, commands, &window, &mut self.actions);
        self.pending.append(full.textures_delta);
        let tessellating = Instant::now();
        let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
        let painting = Instant::now();
        self.paint(&window, &primitives, full.pixels_per_point);
        if self.times {
            let ms = |a: Instant, b: Instant| b.saturating_duration_since(a).as_secs_f64() * 1000.0;
            let n: usize =
                primitives.iter().map(|p| if let egui::epaint::Primitive::Mesh(m) = &p.primitive { m.indices.len() / 3 } else { 0 }).sum();
            let waited = self.app.frames.as_ref().map_or(0.0, |p| p.slept.1.as_secs_f64() * 1000.0);
            eprintln!(
                "software: pass {:.1} ms (the pacer's wait {waited:.1} ms of it), tessellate {:.1} ms ({} meshes, {n} triangles), paint {:.1} ms",
                ms(ran, tessellating),
                ms(tessellating, painting),
                primitives.len(),
                ms(painting, Instant::now())
            );
        }
        if close {
            event_loop.exit();
        }
        self.cpu = Some(began.elapsed().as_secs_f32());
    }

    /// Draws the frame's `primitives` into the window, unless they are what it already shows; answers screenshots, and the
    /// cut, copy and paste the app asked for.
    fn paint(&mut self, window: &Window, primitives: &[egui::ClippedPrimitive], ppp: f32) {
        let size = window.inner_size();
        let (w, h) = (size.width as usize, size.height as usize);
        let mut shots = Vec::new();
        for action in std::mem::take(&mut self.actions) {
            let Some(state) = self.state.as_mut() else { break };
            match action {
                ActionRequested::Screenshot(data) => shots.push(data),
                ActionRequested::Cut => state.egui_input_mut().events.push(Event::Cut),
                ActionRequested::Copy => state.egui_input_mut().events.push(Event::Copy),
                ActionRequested::Paste => {
                    if let Some(text) = state.clipboard_text().map(|t| t.replace("\r\n", "\n")).filter(|t| !t.is_empty()) {
                        state.egui_input_mut().events.push(Event::Paste(text));
                    }
                }
            }
        }
        self.raster.set_textures(&self.pending);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            self.raster.free_textures(&mut self.pending);
            return;
        };
        let print = fingerprint(primitives, ppp, [w, h]);
        let same = !self.fresh && shots.is_empty() && self.pending.set.is_empty() && self.last == Some(print);
        if !same
            && w.saturating_mul(h) <= MAX_PIXELS
            && let Some(surface) = self.surface.as_mut()
            && surface.resize(nw, nh).is_ok()
            && let Ok(mut buffer) = surface.buffer_mut()
        {
            self.raster.draw(&mut buffer, w, h, ppp, CLEAR, primitives);
            for data in shots {
                let pixels = buffer.iter().map(|p| Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, *p as u8)).collect();
                let image = Arc::new(ColorImage::new([w, h], pixels));
                if let Some(state) = self.state.as_mut() {
                    state.egui_input_mut().events.push(Event::Screenshot { viewport_id: ViewportId::ROOT, user_data: data, image });
                }
                self.ctx.request_repaint();
            }
            if buffer.present().is_ok() {
                (self.last, self.fresh) = (Some(print), false);
            }
        }
        self.raster.free_textures(&mut self.pending);
        if !self.shown {
            window.set_visible(true);
            self.shown = true;
        }
    }
}

impl Drop for Runner {
    /// Textures egui changed after the last frame drawn are dropped with the window (egui checks, in a debug build, that none is
    /// dropped unapplied).
    fn drop(&mut self) {
        self.pending.clear();
    }
}

impl ApplicationHandler<Wake> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.open(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else { return };
        match &event {
            WindowEvent::RedrawRequested => return self.frame(event_loop),
            WindowEvent::CloseRequested => {
                // The app is asked first: an unsaved project's prompt answers CancelClose.
                self.info.events.push(ViewportEvent::Close);
                self.ctx.request_repaint();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Occluded(_) | WindowEvent::ThemeChanged(_) => {
                self.fresh = true;
                window.request_redraw();
            }
            WindowEvent::Destroyed => event_loop.exit(),
            _ => {}
        }
        if let Some(state) = self.state.as_mut()
            && state.on_window_event(&window, &event).repaint
        {
            window.request_redraw();
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        // The pointer's own motion, which the 3D view's fly mode reads while the cursor is held.
        if let DeviceEvent::MouseMotion { delta } = event
            && let (Some(window), Some(state)) = (self.window.as_ref(), self.state.as_mut())
            && state.on_mouse_motion(delta)
        {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: Wake) {
        let Wake::At(when) = event;
        self.next = Some(self.next.map_or(when, |n| n.min(when)));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.next {
            Some(when) if when <= Instant::now() => {
                self.next = None;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(when) => event_loop.set_control_flow(ControlFlow::WaitUntil(when)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_centred_on_its_monitor_and_never_off_its_top_left() {
        assert_eq!(centred(Vec2::new(1440.0, 900.0), Pos2::new(0.0, 0.0), Vec2::new(1920.0, 1080.0)), Pos2::new(240.0, 90.0));
        assert_eq!(centred(Vec2::new(1440.0, 900.0), Pos2::new(1920.0, 0.0), Vec2::new(1920.0, 1080.0)), Pos2::new(2160.0, 90.0));
        assert_eq!(centred(Vec2::new(3000.0, 900.0), Pos2::new(0.0, 0.0), Vec2::new(1920.0, 1080.0)), Pos2::new(0.0, 90.0));
    }

    #[test]
    fn closing_ends_the_run_unless_the_app_cancels_it() {
        assert!(closes(true, &[]));
        assert!(closes(true, &[ViewportCommand::Title("x".into())]));
        assert!(!closes(true, &[ViewportCommand::CancelClose]), "the unsaved-changes prompt");
        assert!(!closes(false, &[]));
    }
}
