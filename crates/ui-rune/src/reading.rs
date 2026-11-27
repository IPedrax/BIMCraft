//! The typed length and angle: with a tool in hand and a point picked, digits typed over the plan are a length from that point (Tab
//! goes on to the angle, degrees counter-clockwise from east) and Enter places the point they make, toward the pointer when no angle is
//! typed. It is egui's `typed_length` and `drawing.other`, with the same rules: a field starts with a digit or a point (an angle also with
//! a minus) and, once it has something, takes everything after it, a unit, feet and inches or a fraction, which `Units::parse` reads.
//! The pure part: what is typed, and the point it makes.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{MAX_COORD, TOL};
use bimcraft_core::units::Units;

use crate::lang::{t, tf};

/// The most characters a field takes: what is typed is hostile like any text.
const MAX_CHARS: usize = 64;

/// Which field a typed character goes to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    Length,
    Angle,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Typed {
    pub length: String,
    pub angle: String,
    pub field: Field,
}

impl Typed {
    pub fn is_empty(&self) -> bool {
        self.length.is_empty() && self.angle.is_empty()
    }

    fn current(&mut self) -> &mut String {
        match self.field {
            Field::Length => &mut self.length,
            Field::Angle => &mut self.angle,
        }
    }

    /// A character typed into the field Tab is on. False when it is none of the field's: a field starts with a digit or a point (an
    /// angle may also with a minus), and takes any character once it has one.
    pub fn push(&mut self, ch: char) -> bool {
        let field = self.field;
        let text = self.current();
        let starts = ch.is_ascii_digit() || ch == '.' || (ch == '-' && field == Field::Angle);
        if ch.is_control() || (text.is_empty() && !starts) || text.chars().count() >= MAX_CHARS {
            return false;
        }
        text.push(ch);
        true
    }

    /// Backspace: the last character of the field Tab is on. False when it has none.
    pub fn pop(&mut self) -> bool {
        self.current().pop().is_some()
    }

    /// Tab: the other field. False when nothing is typed, so the key is another's.
    pub fn switch(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.field = if self.field == Field::Length { Field::Angle } else { Field::Length };
        true
    }

    pub fn clear(&mut self) {
        *self = Typed::default();
    }
}

/// A length typed in `units`, in mm: finite, above nothing and no farther than the model reaches.
fn length_of(text: &str, units: Units) -> Result<f64, String> {
    match units.parse(text) {
        Some(x) if x > 0.0 && x <= MAX_COORD => Ok(x),
        _ => Err(tf("`{}` is not a length", &[text.trim()])),
    }
}

/// An angle typed, degrees: a degree sign may end it, and it is a turn at most.
pub fn angle_of(text: &str) -> Result<f64, String> {
    match text.trim().trim_end_matches('°').trim().parse::<f64>() {
        Ok(a) if a.is_finite() && a.abs() <= 360.0 => Ok(a),
        _ => Err(tf("`{}` is not an angle", &[text.trim()])),
    }
}

/// The point the typed length and angle make from `base`. With no angle typed it lies toward `cursor` (east when that is `base`
/// itself); with no length, as far as the cursor is.
pub fn point(base: P2, cursor: Option<P2>, typed: &Typed, units: Units) -> Result<P2, String> {
    let angle = if typed.angle.trim().is_empty() { None } else { Some(angle_of(&typed.angle)?) };
    let length = if typed.length.trim().is_empty() { None } else { Some(length_of(&typed.length, units)?) };
    let toward = cursor.filter(|c| c.is_finite() && c.dist(base) > TOL);
    let direction = match (angle, toward) {
        (Some(a), _) => P2(a.to_radians().cos(), a.to_radians().sin()),
        (None, Some(c)) => (c - base).unit(),
        (None, None) => P2(1.0, 0.0),
    };
    let reach = match (length, toward) {
        (Some(l), _) => l,
        (None, Some(c)) => c.dist(base),
        (None, None) => return Err(t("Type a length: no pointer is over the plan to measure to").to_string()),
    };
    let p = base + direction * reach;
    if p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD {
        Ok(p)
    } else {
        Err(t("That point is outside what the model reaches").to_string())
    }
}

/// What the status line says while something is typed.
pub fn reading(typed: &Typed) -> String {
    let said = |s: &str, on: bool| if s.is_empty() { if on { "_".to_string() } else { "…".to_string() } } else { s.to_string() };
    let (length, angle) = (said(&typed.length, typed.field == Field::Length), said(&typed.angle, typed.field == Field::Angle));
    tf("Length {} | Angle {} | Tab switches the field, Enter places the point", &[&length, &angle])
}

#[cfg(test)]
mod tests;
