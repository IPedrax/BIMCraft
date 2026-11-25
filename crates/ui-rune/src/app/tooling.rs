//! The tools in the window: what `App` does with the points a tool in hand has had picked. The machine is `crate::tooling`, a pure
//! function of those points; this carries its answer out through `Session::execute` (a refusal is the status bar's message, and the
//! click that was refused is given back), and keeps what the Draw panel has set (`Tooling`). It is a child of `app` so it reaches
//! the handlers that are private there, and `app.rs` itself only calls in.

use bimcraft_core::curves::Curve;
use bimcraft_core::draw::{self, Shape};
use serde_json::Value;

use super::App;
use crate::tooling::{self, Chain, Command, Ctx, Step, Tooling};

impl App {
    /// A click has just picked a point with a tool in hand: the tool takes it.
    pub(super) fn tool_took_point(&mut self) {
        self.drive(true);
    }

    /// Enter: the tool in hand is finished with the points it has. False when there is nothing to finish, so the key goes on.
    pub fn finish_tool(&mut self) -> bool {
        if self.tool.is_none() || self.picked.is_empty() {
            return false;
        }
        self.drive(false)
    }

    /// Backspace: the last point picked is given back. False when none has been, so the key goes on.
    pub fn back_point(&mut self) -> bool {
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
        let Tooling { shape, options } = self.tooling;
        if shape == Shape::Line || self.picked.is_empty() {
            return Vec::new();
        }
        draw::preview(shape, &self.picked, landing.at, &options, None)
    }

    /// Carries out what the machine says to do after a click (`clicked`) or Enter.
    fn drive(&mut self, clicked: bool) -> bool {
        let (Some(id), Some(level)) = (self.tool, self.level) else { return false };
        let Some(spec) = tooling::spec_of(id) else { return false };
        let step = {
            let ctx =
                Ctx { model: &self.session.model, level, hovered: self.hovered, shape: self.tooling.shape, options: self.tooling.options };
            if clicked { tooling::advance(spec, &ctx, &self.picked) } else { tooling::finish(spec, &ctx, &self.picked) }
        };
        match step {
            Step::More => {}
            Step::Ignore => {
                if clicked {
                    self.picked.pop();
                }
            }
            Step::Done => self.picked.clear(),
            Step::Refuse(why) => {
                self.message = Some(why);
                if clicked {
                    self.picked.pop();
                }
            }
            Step::Run { commands, keep } => {
                let last = self.picked.last().copied();
                if self.run_commands(&commands) {
                    match (keep, last) {
                        (Chain::Last, Some(p)) => self.picked = vec![p],
                        (Chain::Whole, _) => {}
                        _ => self.picked.clear(),
                    }
                } else if clicked {
                    // The core refused: the point clicked is not kept, and the message says why.
                    self.picked.pop();
                }
            }
        }
        self.reland();
        true
    }

    /// One command, or several as one undo step. A refusal leaves the model as it was and is the message.
    fn run_commands(&mut self, commands: &[Command]) -> bool {
        if let [(name, params)] = commands {
            return self.run(name, params);
        }
        let made = self
            .session
            .transaction("Draw", |s| commands.iter().map(|(name, params)| s.execute(name, params)).collect::<Result<Vec<Value>, String>>());
        match made {
            Ok(_) => {
                self.message = None;
                self.settle();
                true
            }
            Err(why) => {
                self.message = Some(why);
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "tooling_tests.rs"]
mod tests;
