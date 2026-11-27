//! The typed length and angle in the window: what is typed while a tool in hand has a point picked (`reading.rs`), and Enter placing the
//! point it makes as a click there would. A child of `app` so it reaches the handlers that are private there.

use bimcraft_core::geom::P2;
use bimcraft_core::model::MAX_POINTS;

use super::App;
use crate::reading::{self, Field, Typed};
use crate::tooling::{self, Kind};

impl App {
    /// Whether the tool in hand takes a typed length or angle now: it has a point to measure from, and is one that picks points in a
    /// line (a wall with its Line shape, a run, a stair, a path, Move, Copy or Mirror), or Rotate with its centre picked.
    fn takes_typing(&self) -> bool {
        let Some(spec) = self.tool.and_then(tooling::spec_of) else { return false };
        match spec.kind {
            Kind::Run { shapes, .. } => !self.picked.is_empty() && (!shapes || self.tooling.shape == bimcraft_core::draw::Shape::Line),
            Kind::Span { .. } | Kind::Path { .. } | Kind::Shift | Kind::Reflect => !self.picked.is_empty(),
            Kind::Turn => self.picked.len() == 1,
            _ => false,
        }
    }

    /// Whether the tool in hand is Rotate, whose typed number is an angle.
    fn turning(&self) -> bool {
        self.tool.and_then(tooling::spec_of).is_some_and(|s| matches!(s.kind, Kind::Turn))
    }

    /// Text typed over the plan: what the tool takes of it is a length (an angle for Rotate). False when the tool takes none, so the key
    /// is the window's.
    pub fn type_text(&mut self, text: &str) -> bool {
        if !self.takes_typing() {
            return false;
        }
        let mut took = false;
        for ch in text.chars() {
            if self.turning() {
                self.typed.field = Field::Angle;
            }
            took |= self.typed.push(ch);
        }
        took
    }

    /// Backspace with something typed takes its last character. False when nothing is typed.
    pub fn typed_backspace(&mut self) -> bool {
        !self.typed.is_empty() && self.typed.pop()
    }

    /// Tab with something typed goes on to the other field. False when nothing is typed.
    pub fn typed_tab(&mut self) -> bool {
        !self.turning() && self.typed.switch()
    }

    /// Escape with something typed gives it up. False when nothing is typed.
    pub(super) fn typed_escape(&mut self) -> bool {
        let some = !self.typed.is_empty();
        self.typed.clear();
        some
    }

    /// What the status line says of what is typed, while something is.
    pub(super) fn reading_line(&self) -> Option<String> {
        (!self.typed.is_empty()).then(|| reading::reading(&self.typed))
    }

    /// Enter with something typed: the point it makes (a Rotate's angle, two rays) is picked as a click there would be. False when
    /// nothing is typed or the tool takes none; a number that is none is the message.
    pub(super) fn place_typed(&mut self) -> bool {
        if self.typed.is_empty() || !self.takes_typing() {
            return false;
        }
        let Some(&base) = self.picked.last() else { return false };
        let units = self.session.model.units;
        if self.turning() {
            let a = match reading::angle_of(&self.typed.angle) {
                Ok(a) => a,
                Err(why) => {
                    self.message = Some(why);
                    return true;
                }
            };
            // The rays the machine turns between: east, and the angle typed from it, about the centre.
            let (from, to) = (base + P2(1000.0, 0.0), base + P2(a.to_radians().cos(), a.to_radians().sin()) * 1000.0);
            self.typed.clear();
            for p in [from, to] {
                self.picked.push(p);
                self.tool_took_point();
            }
            return true;
        }
        let cursor = self.cursor_mm().map(|(x, y)| P2(x, y));
        match reading::point(base, cursor, &self.typed, units) {
            Ok(p) if self.picked.len() < MAX_POINTS => {
                self.typed.clear();
                self.picked.push(p);
                self.tool_took_point();
            }
            Ok(_) => self.message = Some(crate::lang::t("A tool takes at most so many points").to_string()),
            Err(why) => self.message = Some(why),
        }
        true
    }

    /// The typed text goes when a click spends it or the tool changes.
    pub(super) fn forget_typed(&mut self) {
        self.typed = Typed::default();
    }
}

#[cfg(test)]
#[path = "reading_tests.rs"]
mod tests;
