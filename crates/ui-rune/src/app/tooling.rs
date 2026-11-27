//! The tools in the window: what `App` does with the points a tool in hand has had picked. The machine is `crate::tooling`, a pure
//! function of those points; this carries its answer out through `Session::execute` (a refusal is the status bar's message, and the
//! click that was refused is given back), and keeps what the Draw panel has set (`Tooling`). It is a child of `app` so it reaches
//! the handlers that are private there, and `app.rs` itself only calls in.

use bimcraft_core::curves::Curve;
use bimcraft_core::draw::{self, Shape};
use bimcraft_core::ids::Id;
use bimcraft_core::render::{self, Drawing};
use serde_json::Value;

use super::App;
use crate::stand;
use crate::tooling::{self, Chain, Command, Ctx, Held, Step, Tooling};

impl App {
    /// A click has just picked a point with a tool in hand: the tool takes it.
    pub(super) fn tool_took_point(&mut self) {
        // A click spends what was typed.
        self.forget_typed();
        self.drive(true);
    }

    /// Enter: the tool in hand is finished with the points it has. False when there is nothing to finish, so the key goes on.
    pub fn finish_tool(&mut self) -> bool {
        if self.tool.is_none() || self.picked.is_empty() {
            return false;
        }
        // Enter with a length or an angle typed places the point they make.
        if self.place_typed() {
            return true;
        }
        self.drive(false)
    }

    /// Backspace: the last point picked is given back. False when none has been, so the key goes on.
    pub fn back_point(&mut self) -> bool {
        if self.typed_backspace() {
            return true;
        }
        if self.tool.is_none() || self.picked.pop().is_none() {
            return false;
        }
        self.reland();
        true
    }

    /// The Draw panel's shape for the Wall tool; the points picked toward the one before are given up.
    pub fn set_shape(&mut self, shape: Shape) {
        if self.tooling.shape != shape {
            self.tooling.shape = shape;
            self.picked.clear();
            self.reland();
        }
    }

    /// What the next click would make with the Draw panel's shape in hand: the curves the points picked so far and the point the
    /// pointer is on draw, for a tool that takes a shape and a shape that is more than a line (which `paint::over` already shows).
    pub fn shape_preview(&self) -> Vec<Curve> {
        let takes_shape = self.tool.and_then(tooling::spec_of).is_some_and(tooling::Spec::takes_shape);
        let (Some(landing), true) = (&self.landing, takes_shape) else { return Vec::new() };
        let Tooling { shape, options, held, tangent, .. } = self.tooling;
        let m = &self.session.model;
        // The wall a click would make, as thick as its type: the faces along the segment to the pointer, as the wall will stand.
        if shape == Shape::Line {
            return self.picked.last().and_then(|a| stand::wall(m, &self.bar.settings, *a, landing.at)).into_iter().collect();
        }
        // Fillet Arc with its two walls picked: the arc and the walls cut back to it, at the radius typed or the pointer's.
        if shape == Shape::Fillet {
            let (&[_, _], Held::Pair(a, b)) = (self.picked.as_slice(), held) else { return Vec::new() };
            let radius = self.typed_radius().or_else(|| tooling::fillet_radius_at(m, a, b, landing.at));
            let walls = draw::element_curve(m, a).zip(draw::element_curve(m, b));
            let made = walls.zip(radius).and_then(|((ca, cb), r)| draw::fillet_lines(&ca, &cb, r));
            return made.map(|(a, arc, b)| vec![a, arc, b]).unwrap_or_default();
        }
        if self.picked.is_empty() {
            return Vec::new();
        }
        draw::preview(shape, &self.picked, landing.at, &options, tangent)
    }

    /// What a click of the tool in hand would put at the pointer, drawn as the plan draws it, and the id its strokes are owned by: a door or
    /// a window in the wall under the pointer, a column (`stand.rs`). None with another tool, or over no wall for a door.
    pub fn stands(&self) -> Option<(Drawing, Id)> {
        let (tool, level, landing) = (self.tool?, self.level?, self.landing.as_ref()?);
        let m = &self.session.model;
        let near = 2.0 * f64::from(super::REACH) / self.camera.scale;
        let e = stand::element(m, &self.bar.settings, tool, level, landing.at, near)?;
        let view = self.camera.view(self.size.0, self.size.1);
        Some((render::sketch(m, view, m.next_id, &e), m.next_id))
    }

    /// Carries out what the machine says to do after a click (`clicked`) or Enter.
    fn drive(&mut self, clicked: bool) -> bool {
        let (Some(id), Some(level)) = (self.tool, self.level) else { return false };
        let Some(spec) = tooling::spec_of(id) else { return false };
        self.refresh_tangent();
        let step = {
            let ctx = Ctx {
                model: &self.session.model,
                level,
                hovered: self.hovered,
                shape: self.tooling.shape,
                options: self.tooling.options,
                selection: &self.selection,
                held: self.tooling.held,
                reach: 2.0 * f64::from(super::REACH) / self.camera.scale,
                tangent: self.tooling.tangent,
            };
            if clicked { tooling::advance(spec, &ctx, &self.picked) } else { tooling::finish(spec, &ctx, &self.picked) }
        };
        // What the Options Bar has set joins the params of what the click makes.
        let step = self.feed_options(spec, step);
        match step {
            Step::More => {}
            Step::Ignore => {
                if clicked {
                    self.picked.pop();
                }
            }
            Step::Hold(held) => self.tooling.held = held,
            Step::Done => {
                self.picked.clear();
                self.tooling.held = Held::Nothing;
            }
            Step::Refuse(why) => {
                self.message = Some(why);
                if clicked {
                    self.picked.pop();
                }
            }
            Step::Run { commands, keep } => {
                let last = self.picked.last().copied();
                if let Some(made) = self.run_commands(&commands) {
                    match (keep, last) {
                        (Chain::Last, Some(p)) => self.picked = vec![p],
                        (Chain::Whole, _) => {}
                        _ => self.picked.clear(),
                    }
                    self.tooling.held = Held::Nothing;
                    // A copy is what is selected after it, as in the egui window; Move, Copy, Rotate and Mirror are then done with.
                    if commands.first().is_some_and(|(name, _)| *name == "copy") {
                        let copies = made.first().and_then(|v| v["ids"].as_array());
                        let copies: Vec<Id> = copies.map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                        if !copies.is_empty() {
                            self.select(copies);
                        }
                    }
                    if spec.ends_tool() {
                        self.tool = None;
                    }
                    // A door or a window placed takes its flip with it: the next starts as the bar has it.
                    self.bar.settings.flip = false;
                    // A roof or a floor made is what is selected, and the tool goes back to Modify, as in the egui window.
                    if matches!(spec.id, "roof" | "floor") {
                        let id = made.first().and_then(|v| v["id"].as_u64());
                        self.tool = None;
                        if let Some(id) = id {
                            self.select(vec![id]);
                        }
                    }
                } else if clicked {
                    // The core refused: the point clicked is not kept, and the message says why.
                    self.picked.pop();
                }
            }
        }
        // A chain of tangent arcs goes on from the one just made.
        self.refresh_tangent();
        self.reland();
        true
    }

    /// The way the line before ran where the Tangent End Arc in hand starts, at the first point picked: what the machine and the ghost
    /// leave along. None with another tool or shape, or when no wall or line ends there.
    fn refresh_tangent(&mut self) {
        let arc = self.tool.and_then(tooling::spec_of).is_some_and(tooling::Spec::takes_shape) && self.tooling.shape == Shape::TangentArc;
        let first = self.picked.first().copied().zip(self.level).filter(|_| arc);
        self.tooling.tangent = first.and_then(|(p, level)| tooling::tangent_at(&self.session.model, level, p));
    }

    /// One command, or several as one undo step, and what they made. A refusal leaves the model as it was and is the message.
    pub(super) fn run_commands(&mut self, commands: &[Command]) -> Option<Vec<Value>> {
        if let [(name, params)] = commands {
            return match self.session.execute(name, params) {
                Ok(made) => {
                    self.message = None;
                    self.settle();
                    Some(vec![made])
                }
                Err(why) => {
                    self.message = Some(why);
                    None
                }
            };
        }
        let made = self
            .session
            .transaction("Draw", |s| commands.iter().map(|(name, params)| s.execute(name, params)).collect::<Result<Vec<Value>, String>>());
        match made {
            Ok(made) => {
                self.message = None;
                self.settle();
                Some(made)
            }
            Err(why) => {
                self.message = Some(why);
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "tooling_tests.rs"]
mod tests;
