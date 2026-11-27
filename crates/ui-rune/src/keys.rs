//! Typed shortcuts, Revit's two letters (`WA` for Wall, `ZF` for Zoom to Fit, `3D`): what a letter typed over the window does to the
//! letters waiting before it. The table is the core's (`shortcuts`), which egui matches the same way, so the two windows have the same
//! keys; this is the matcher, with no toolkit, tested without a window.
//!
//! A plain letter or digit that begins a shortcut waits for the next for [`WAIT`] seconds. The second completes one, and it runs; or
//! it does not, and then the first is dropped and the second waits in its place (so `QWA` is `WA`). A letter that begins none does
//! nothing. Escape, or time, drops what waits.

use bimcraft_core::shortcuts::{self, Target};

/// How long a letter waits for the next, seconds.
pub const WAIT: f64 = 2.0;

/// What a typed letter did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// It begins a shortcut: the next letter may complete it.
    Waiting,
    /// It completed one: run it.
    Run(Target),
    /// It begins no shortcut and completes none: the window's.
    Nothing,
}

/// The letters typed and waiting, and when the last came (the window's clock).
#[derive(Clone, Debug, Default)]
pub struct Typed {
    text: String,
    at: f64,
}

impl Typed {
    /// The letters waiting, upper case: what the status bar shows.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// A letter or digit typed at `now`. What is not one is nothing a shortcut has, and drops what waits.
    pub fn push(&mut self, c: char, now: f64) -> Step {
        if !c.is_ascii_alphanumeric() {
            self.clear();
            return Step::Nothing;
        }
        // Too long a wait, or a clock that is no number, and what came before is dropped.
        if !self.text.is_empty() && !(now - self.at <= WAIT) {
            self.clear();
        }
        self.at = if now.is_finite() { now } else { 0.0 };
        let c = c.to_ascii_uppercase();
        self.text.push(c);
        if let Some(target) = shortcuts::target(&self.text) {
            self.clear();
            return Step::Run(target);
        }
        if shortcuts::begins(&self.text) {
            return Step::Waiting;
        }
        // The letters before it begin nothing it ends: it stands alone.
        self.text.clear();
        if shortcuts::begins(&c.to_string()) {
            self.text.push(c);
            Step::Waiting
        } else {
            Step::Nothing
        }
    }

    /// Drops what has waited past [`WAIT`] by `now`: true when something was waiting.
    pub fn expire(&mut self, now: f64) -> bool {
        let late = !self.text.is_empty() && !(now - self.at <= WAIT);
        if late {
            self.clear();
        }
        late
    }

    /// Seconds from `now` until what waits is dropped.
    pub fn due(&self, now: f64) -> Option<f64> {
        (!self.text.is_empty()).then(|| (self.at + WAIT - now).max(0.0)).filter(|d| d.is_finite())
    }
}

/// A chord as the core's table writes it (`Ctrl+Shift+Z`, `F3`, `Delete`), for the key that was pressed: None for a key that is not
/// one a chord names.
pub fn written(key: &rune::Key, mods: rune::Mods) -> Option<String> {
    use rune::Key;
    let name = match key {
        Key::Command(c) if !c.is_control() && !c.is_whitespace() => c.to_ascii_uppercase().to_string(),
        Key::Function(n) => format!("F{n}"),
        Key::Delete => "Delete".to_string(),
        _ => return None,
    };
    let mut parts: Vec<&str> = Vec::new();
    if mods.cmd {
        parts.push("Ctrl");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if mods.alt {
        parts.push("Alt");
    }
    parts.push(&name);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests;
