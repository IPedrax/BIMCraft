//! Text notes edited where they are, as in Revit: the Text tool's click, or a double click on a note, opens its text on
//! the canvas (a plan, a section or a sheet) with a caret. Typing writes, Shift+Enter breaks the line, Enter, Esc or a
//! click outside it finishes, and the note is one `text` or `modify` command then. With Override Formatting, Ctrl+B,
//! Ctrl+I and Ctrl+U (or the Options Bar's B, I and U) make the selected part bold, italic or underlined, or not. What it
//! draws is the core's: the note as it will be (`render::sketch`), laid out by `render::note_layout`, so the caret and
//! the selection stand on its letters. And Text Type Properties, the dialog Edit Type opens for a note.

use crate::popup::Modal;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, LeaderKind, Model, Note};
use bimcraft_core::render::{self, Drawing};
use bimcraft_core::text::{self, Format, Layout, Style, TextType};
use bimcraft_i18n::msg;
use egui::{
    Button, ComboBox, Context, DragValue, Event, Id as UiId, Key, PointerButton, Pos2, Response, RichText, Shape, Stroke, TextEdit, Ui,
    vec2,
};
use serde_json::{Value, json};

use crate::canvas::{Place, paint};
use crate::lang::{t, tf};
use crate::style::canvas_pal;
use crate::typeprops::{self, Head, Naming};
use crate::{BimApp, Status};

/// Which of a note's formatting a key or a button flips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Bold,
    Italic,
    Underline,
}

/// A note being written or rewritten on the canvas: its characters and what each overrides, the caret and the other
/// end of the selection (character places), and the rest of the note it will be.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NoteEdit {
    /// The note edited; none for a new one.
    pub id: Option<Id>,
    /// The view it is in.
    pub view: Id,
    pub note: Note,
    pub chars: Vec<char>,
    pub formats: Vec<Format>,
    pub caret: usize,
    pub anchor: usize,
    /// A drag selecting is on.
    drag: bool,
}

impl NoteEdit {
    /// A new note of type `type_name` in view `view` at `at`, empty.
    pub(crate) fn new(m: &Model, view: Id, at: P2, type_name: String) -> NoteEdit {
        let note = Note {
            view: m.guid(view),
            at,
            text: String::new(),
            type_name,
            size: None,
            angle: 0.0,
            leader: None,
            leader_kind: LeaderKind::Straight,
            override_format: false,
            runs: Vec::new(),
        };
        NoteEdit { id: None, view, note, chars: Vec::new(), formats: Vec::new(), caret: 0, anchor: 0, drag: false }
    }

    /// Note `id` of `m`, its whole text selected, as Revit opens one.
    pub(crate) fn of(m: &Model, id: Id) -> Option<NoteEdit> {
        let Some(Element::Text(n)) = m.elements.get(id) else { return None };
        let view = m.elements.id(n.view)?;
        let chars: Vec<char> = n.text.chars().collect();
        let formats = text::formats(&n.runs, chars.len());
        let end = chars.len();
        Some(NoteEdit { id: Some(id), view, note: n.clone(), chars, formats, caret: end, anchor: 0, drag: false })
    }

    pub(crate) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The note as it stands now.
    pub(crate) fn draft(&self) -> Note {
        Note { text: self.text(), runs: text::runs(&self.formats), ..self.note.clone() }
    }

    /// The selection, first place first.
    pub(crate) fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub(crate) fn selected(&self) -> String {
        let (a, b) = self.range();
        self.chars[a..b].iter().collect()
    }

    /// `s` typed in place of the selection, its line breaks as they are, other control characters left out. What is typed
    /// takes the format of the character before it, as a word processor does.
    pub(crate) fn insert(&mut self, s: &str) {
        let (a, b) = self.range();
        let typed: Vec<char> = s.replace("\r\n", "\n").chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
        let room = text::MAX_NOTE.saturating_sub(self.chars.len() - (b - a));
        let typed = &typed[..typed.len().min(room)];
        let format = a.checked_sub(1).and_then(|i| self.formats.get(i)).copied().unwrap_or_default();
        self.chars.splice(a..b, typed.iter().copied());
        self.formats.splice(a..b, std::iter::repeat_n(format, typed.len()));
        self.caret = a + typed.len();
        self.anchor = self.caret;
    }

    /// Backspace (`forward` false) or Delete: the selection, else the character before or after the caret.
    pub(crate) fn erase(&mut self, forward: bool) {
        let (a, b) = self.range();
        let (a, b) = match (a == b, forward) {
            (false, _) => (a, b),
            (true, false) if a > 0 => (a - 1, a),
            (true, true) if b < self.chars.len() => (b, b + 1),
            _ => return,
        };
        self.chars.drain(a..b);
        self.formats.drain(a..b);
        self.caret = a;
        self.anchor = a;
    }

    /// The caret put at `to`; with `extend`, the selection's other end stays.
    fn put(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.chars.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Left or Right: a selection given up on its side, else a character.
    pub(crate) fn step(&mut self, right: bool, extend: bool) {
        let (a, b) = self.range();
        let to = match (a != b && !extend, right) {
            (true, false) => a,
            (true, true) => b,
            (false, false) => self.caret.saturating_sub(1),
            (false, true) => self.caret + 1,
        };
        self.put(to, extend);
    }

    /// Up or Down: the line above or below, as near across as the caret is.
    pub(crate) fn line_step(&mut self, layout: &Layout, down: bool, extend: bool) {
        let (x, ..) = layout.caret(self.caret);
        let line = layout.line_of(self.caret);
        let to = match (down, layout.lines.get(line + 1)) {
            (true, Some(l)) => layout.index_at((x, l.top + 0.5 * layout.size)),
            (true, None) => self.chars.len(),
            (false, _) if line == 0 => 0,
            (false, _) => layout.lines.get(line - 1).map_or(0, |l| layout.index_at((x, l.top + 0.5 * layout.size))),
        };
        self.put(to, extend);
    }

    /// Home or End: the caret's line's start or end.
    pub(crate) fn line_end(&mut self, layout: &Layout, end: bool, extend: bool) {
        let to = layout.lines.get(layout.line_of(self.caret)).map_or(0, |l| if end { l.end } else { l.start });
        self.put(to, extend);
    }

    pub(crate) fn select_all(&mut self) {
        (self.anchor, self.caret) = (0, self.chars.len());
    }

    /// The word at place `i` selected.
    fn select_word(&mut self, i: usize) {
        let word = |c: &char| c.is_alphanumeric() || *c == '_';
        let start = self.chars[..i.min(self.chars.len())].iter().rposition(|c| !word(c)).map_or(0, |p| p + 1);
        let end = self.chars[i.min(self.chars.len())..].iter().position(|c| !word(c)).map_or(self.chars.len(), |p| i + p);
        (self.anchor, self.caret) = (start, end);
    }

    /// The selection made bold, italic or underlined, or not: not when all of it is already, as its type `base` and what
    /// it overrides letter it. Each character keeps it whatever type the note gets.
    pub(crate) fn toggle(&mut self, base: Style, which: Which) {
        let (a, b) = self.range();
        if a == b {
            return;
        }
        let get = |s: Style| match which {
            Which::Bold => s.bold,
            Which::Italic => s.italic,
            Which::Underline => s.underline,
        };
        let on = !self.formats[a..b].iter().all(|f| get(f.over(base)));
        for f in &mut self.formats[a..b] {
            match which {
                Which::Bold => f.bold = Some(on),
                Which::Italic => f.italic = Some(on),
                Which::Underline => f.underline = Some(on),
            }
        }
    }

    /// Whether every selected character is bold, italic or underlined, as its type `base` and its format letter it.
    pub(crate) fn all(&self, base: Style, which: Which) -> bool {
        let (a, b) = self.range();
        let get = |f: &Format| {
            let s = f.over(base);
            match which {
                Which::Bold => s.bold,
                Which::Italic => s.italic,
                Which::Underline => s.underline,
            }
        };
        a < b && self.formats[a..b].iter().all(get)
    }

    /// The command that makes the note as written: a new note's `text`, else a `modify` of what changed; none when there
    /// is nothing to make (a new note left empty, or one as it was).
    pub(crate) fn command(&self, m: &Model) -> Option<(&'static str, Value)> {
        let n = self.draft();
        if n.text.trim().is_empty() {
            return None;
        }
        let runs = serde_json::to_value(&n.runs).unwrap_or_default();
        let mut p = json!({"text": n.text, "at": [n.at.0, n.at.1], "type": n.type_name,
                           "override_format": n.override_format, "runs": runs, "leader_kind": n.leader_kind.name()});
        if let Some(l) = n.leader {
            p["leader"] = json!([l.0, l.1]);
        }
        match self.id {
            None => {
                p["view"] = json!(self.view);
                Some(("text", p))
            }
            Some(id) => {
                let was = match m.elements.get(id) {
                    Some(Element::Text(was)) => was,
                    _ => return None,
                };
                (*was != n).then(|| {
                    p["id"] = json!(id);
                    ("modify", p)
                })
            }
        }
    }
}

impl BimApp {
    /// The text type the Text tool writes in: the one picked in the Type Selector, while the project has it, else 2.5mm.
    pub(crate) fn note_type(&self) -> String {
        let m = &self.session.model;
        match self.types.get("text") {
            Some(t) if m.text_types.contains_key(t) => t.clone(),
            _ => m.default_type("text").unwrap_or_default().to_string(),
        }
    }

    /// A new note opened for writing in view `view` at `at`.
    pub(crate) fn start_note(&mut self, view: Id, at: P2) {
        self.finish_note();
        let mut e = NoteEdit::new(&self.session.model, view, at, self.note_type());
        // A note placed with a leader keeps the kind the Options Bar showed; one without says nothing about a leader.
        if self.note_leader {
            e.note.leader_kind = self.note_leader_kind;
        }
        self.note_edit = Some(e);
        self.status =
            Status { text: t("Type the note: Shift+Enter breaks the line, Enter or a click outside it finishes.").into(), error: false };
    }

    /// Note `id` opened for rewriting where it is.
    pub(crate) fn edit_note(&mut self, id: Id) {
        self.finish_note();
        self.note_edit = NoteEdit::of(&self.session.model, id);
        if self.note_edit.is_some() {
            self.selection = vec![id];
        }
    }

    /// The note being written made, and the editor closed. A new note stays selected; the Text tool stays on for the next.
    pub(crate) fn finish_note(&mut self) {
        let Some(e) = self.note_edit.take() else { return };
        let Some((id, params)) = e.command(&self.session.model) else { return };
        if let Some(made) = self.run(id, params)
            && let Some(new) = made["id"].as_u64().filter(|_| e.id.is_none())
        {
            self.selection = vec![new];
        }
    }

    /// Whether a note is being written in view `view`.
    pub(crate) fn writing_in(&self, view: Id) -> bool {
        self.note_edit.as_ref().is_some_and(|e| e.view == view)
    }

    /// The note being written's id while it is drawn: its own, or the one it will have.
    pub(crate) fn writing(&self) -> Option<Id> {
        self.note_edit.as_ref().map(|e| e.id.unwrap_or(self.session.model.next_id))
    }

    /// The type a note being written is lettered in.
    fn edited_type(&self, e: &NoteEdit) -> TextType {
        self.session.model.text_types.get(&e.note.type_name).cloned().unwrap_or_default()
    }

    /// The keys while a note is being written: typing, the caret, the selection, the clipboard, its formatting, and
    /// finishing. They are the editor's alone.
    pub(crate) fn note_keys(&mut self, ctx: &Context) {
        let Some(mut e) = self.note_edit.take() else { return };
        let base = self.edited_type(&e).style();
        let layout = text::layout(
            &self.edited_type(&e),
            &e.text(),
            &text::styles(&self.edited_type(&e), &e.text(), e.note.override_format, &text::runs(&e.formats)),
        );
        let events = ctx.input_mut(|i| {
            let (mine, rest): (Vec<Event>, Vec<Event>) = std::mem::take(&mut i.events)
                .into_iter()
                .partition(|ev| matches!(ev, Event::Text(_) | Event::Key { .. } | Event::Copy | Event::Cut | Event::Paste(_)));
            i.events = rest;
            mine
        });
        let mut finish = false;
        let mut hint = None;
        for ev in events {
            match ev {
                Event::Text(s) => e.insert(&s),
                Event::Paste(s) => e.insert(&s),
                Event::Copy => ctx.copy_text(e.selected()),
                Event::Cut => {
                    ctx.copy_text(e.selected());
                    e.erase(false);
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    let (shift, command) = (modifiers.shift, modifiers.command);
                    match key {
                        Key::Enter if shift => e.insert("\n"),
                        Key::Enter | Key::Escape => finish = true,
                        Key::Backspace => e.erase(false),
                        Key::Delete => e.erase(true),
                        Key::ArrowLeft => e.step(false, shift),
                        Key::ArrowRight => e.step(true, shift),
                        Key::ArrowUp => e.line_step(&layout, false, shift),
                        Key::ArrowDown => e.line_step(&layout, true, shift),
                        Key::Home => e.line_end(&layout, false, shift),
                        Key::End => e.line_end(&layout, true, shift),
                        Key::Tab => e.insert("\t"),
                        Key::A if command => e.select_all(),
                        Key::B | Key::I | Key::U if command => {
                            let which = match key {
                                Key::B => Which::Bold,
                                Key::I => Which::Italic,
                                _ => Which::Underline,
                            };
                            if e.note.override_format {
                                e.toggle(base, which);
                            } else {
                                hint = Some(t("Check Override Formatting in the Options Bar to make a part bold, italic or underlined."));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        self.note_edit = Some(e);
        if let Some(h) = hint {
            self.status = Status { text: h.into(), error: true };
        }
        if finish {
            self.finish_note();
        }
    }

    /// The note being written in view `view`, over drawing `d` shown at `at` in `p`: the note as it will be, its selection
    /// and caret, and what the pointer does to them. A press outside it finishes it.
    pub(crate) fn note_canvas(&mut self, ui: &Ui, response: &Response, p: &egui::Painter, at: Place, d: &Drawing) {
        let model = std::sync::Arc::clone(&self.session.model);
        let Some(id) = self.writing() else { return };
        let Some(e) = self.note_edit.as_mut() else { return };
        let draft = e.draft();
        let shown = render::sketch(&model, d.view, id, &Element::Text(draft.clone()));
        paint(p, Place { hide: None, ..at }, &shown, &[], None, self.thin, &[]);
        let frame = shown.note_frame(&draft);
        let layout = render::note_layout(&model, &draft);
        let screen = |q: (f64, f64)| at.pos(frame.px(q));
        // Its box, a dashed frame round what it says.
        let [l, top, r, b] = layout.frame(0.5 * layout.size);
        let corners: Vec<Pos2> = [(l, top), (r, top), (r, b), (l, b), (l, top)].into_iter().map(screen).collect();
        p.extend(Shape::dashed_line(&corners, Stroke::new(1.0, canvas_pal().accent), 4.0, 3.0));
        let (a, z) = e.range();
        for [x0, y0, x1, y1] in layout.boxes(a, z) {
            let quad = vec![screen((x0, y0)), screen((x1, y0)), screen((x1, y1)), screen((x0, y1))];
            p.add(Shape::convex_polygon(quad, canvas_pal().accent.gamma_multiply(0.3), Stroke::NONE));
        }
        let (x, y0, y1) = layout.caret(e.caret);
        p.line_segment([screen((x, y0)), screen((x, y1))], Stroke::new(1.5, canvas_pal().accent));
        // The pointer: a press in it puts the caret (Shift: takes the selection to it), a drag selects, a double click
        // takes a word; a press outside it finishes it.
        let inside = |q: (f64, f64)| q.0 >= l && q.0 <= r && q.1 >= top && q.1 <= b;
        let (pressed, down, double, shift, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.modifiers.shift,
                i.pointer.interact_pos(),
            )
        });
        let local = pos.map(|q| frame.mm(((q.x - at.origin.x) / at.k, (q.y - at.origin.y) / at.k)));
        match local {
            Some(q) if pressed && response.hovered() && inside(q) => {
                e.put(layout.index_at(q), shift);
                e.drag = true;
            }
            Some(_) if pressed && response.hovered() => {
                self.finish_note();
                return;
            }
            Some(q) if e.drag && down => e.put(layout.index_at(q), true),
            _ => e.drag = false,
        }
        if double && local.is_some_and(inside) {
            e.select_word(e.caret);
        }
    }

    /// The Options Bar while a note is being written: its type, Override Formatting, B, I and U, and Finish.
    pub(crate) fn note_options(&mut self, ui: &mut Ui) {
        let names: Vec<String> = self.session.model.text_types.keys().cloned().collect();
        let Some(mut e) = self.note_edit.take() else { return };
        let base = self.edited_type(&e).style();
        let l = ui.label(t("Type:"));
        ComboBox::from_id_salt("note-type")
            .selected_text(&e.note.type_name)
            .show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(&mut e.note.type_name, n.clone(), n);
                }
            })
            .response
            .labelled_by(l.id);
        ui.checkbox(&mut e.note.override_format, t("Override Formatting"));
        // A note that has a leader may have it run a different way, as Revit's Properties does.
        if e.note.leader.is_some() {
            leader_type(ui, &mut e.note.leader_kind);
        }
        let fmt = e.note.override_format;
        for (which, label, tip) in [
            (Which::Bold, "B", msg!("Bold (Ctrl+B)")),
            (Which::Italic, "I", msg!("Italic (Ctrl+I)")),
            (Which::Underline, "U", msg!("Underline (Ctrl+U)")),
        ] {
            let mut text = RichText::new(label).strong();
            text = match which {
                Which::Bold => text,
                Which::Italic => text.italics(),
                Which::Underline => text.underline(),
            };
            let on = e.all(base, which);
            let r = ui.add_enabled(fmt, Button::new(text).selected(on).min_size(vec2(24.0, 22.0))).on_hover_text(t(tip));
            if r.clicked() {
                e.toggle(base, which);
            }
        }
        let finish = ui.button(t("Finish")).clicked();
        self.note_edit = Some(e);
        if finish {
            self.finish_note();
        }
    }
}

/// The Options Bar's Leader Type, for a note's leader: Straight, Two Segment or Curved.
pub(crate) fn leader_type(ui: &mut Ui, kind: &mut LeaderKind) {
    ui.label(t("Leader Type:"));
    for (k, label) in
        [(LeaderKind::Straight, msg!("Straight")), (LeaderKind::TwoSegment, msg!("Two Segment")), (LeaderKind::Curved, msg!("Curved"))]
    {
        if ui.selectable_label(*kind == k, t(label)).clicked() {
            *kind = k;
        }
    }
}

/// Text Type Properties: one text type's font, size, lettering and border, Duplicate, and OK, Cancel and Apply. OK puts
/// the notes it opened for on the type shown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextTypeDialog {
    name: String,
    t: TextType,
    elements: Vec<Id>,
    naming: Naming,
    error: Option<String>,
    /// What the font list is narrowed to, typed: a system has hundreds.
    find: String,
}

impl TextTypeDialog {
    pub(crate) fn new(m: &Model, name: String, elements: Vec<Id>) -> TextTypeDialog {
        let t = m.text_types.get(&name).cloned().unwrap_or_default();
        TextTypeDialog { name, t, elements, naming: Naming::None, error: None, find: String::new() }
    }

    /// What stopped OK, Apply, Duplicate or Rename, if anything did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The `text_type` command for the type as typed, under `name` (from the one shown, for a duplicate).
    fn params(&self, name: &str) -> Value {
        let t = &self.t;
        json!({"name": name, "like": self.name, "font": t.font, "size": t.size, "bold": t.bold, "italic": t.italic,
               "underline": t.underline, "width_factor": t.width_factor, "color": t.color, "border": t.border,
               "border_offset": t.border_offset, "border_pen": t.border_pen.0})
    }
}

impl BimApp {
    /// Text Type Properties, while it's open.
    pub(crate) fn text_type_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.text_type_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let names: Vec<String> = model.text_types.keys().cloned().collect();
        let fonts = text::families();
        let (mut ok, mut cancel, mut apply, mut asked) = (false, false, false, None);
        let r = Modal::new(UiId::new("text-type-properties"), t("Type Properties")).width(crate::typeprops::WIDTH).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                asked = typeprops::head(ui, "text-type", &tf("System Family: {}", &[t("Text")]), &names, &d.name, &mut d.naming, true);
                let below = typeprops::foot_height(ui, d.error.is_some());
                typeprops::table(ui, "text-type", below, |tb| {
                    // As Revit's: how it's drawn, then its lettering.
                    tb.group("Graphics");
                    tb.color(t("Color"), &mut d.t.color);
                    tb.drag(t("Border Line Weight"), DragValue::new(&mut d.t.border_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
                    tb.check(t("Show Border"), &mut d.t.border);
                    tb.drag(
                        t("Leader/Border Offset"),
                        DragValue::new(&mut d.t.border_offset).range(0.0..=50.0).speed(0.1).max_decimals(2).suffix(" mm"),
                    );
                    tb.group("Text");
                    let font = &mut d.t.font;
                    let find = &mut d.find;
                    tb.row(t("Text Font"), |ui, l| {
                        ComboBox::from_id_salt("text-type-font")
                            .selected_text(font.as_str())
                            .width(ui.available_width())
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                ui.add(TextEdit::singleline(find).hint_text(t("Search fonts")).desired_width(180.0));
                                let find = find.trim().to_lowercase();
                                for f in fonts.iter().filter(|f| f.to_lowercase().contains(&find)) {
                                    ui.selectable_value(font, f.clone(), f);
                                }
                            })
                            .response
                            .labelled_by(l)
                    });
                    // What letters it on this system, where that is another font.
                    let drawn = text::resolve(&d.t.font, false, false);
                    if !drawn.family().eq_ignore_ascii_case(d.t.font.trim()) {
                        tb.shown("", &tf("Not on this system: drawn with {}", &[drawn.family()]));
                    }
                    tb.drag(t("Text Size"), DragValue::new(&mut d.t.size).range(0.1..=100.0).speed(0.1).max_decimals(2).suffix(" mm"));
                    tb.check(t("Bold"), &mut d.t.bold);
                    tb.check(t("Italic"), &mut d.t.italic);
                    tb.check(t("Underline"), &mut d.t.underline);
                    tb.drag(t("Width Factor"), DragValue::new(&mut d.t.width_factor).range(0.1..=10.0).speed(0.01).max_decimals(2));
                });
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if cancel || r.should_close() {
            return;
        }
        match asked {
            Some(Head::Switch(name)) => d = TextTypeDialog { elements: d.elements, ..TextTypeDialog::new(&model, name, Vec::new()) },
            Some(Head::Duplicate(name)) => match self.exec("text_type", &d.params(&name)) {
                Ok(_) => d = TextTypeDialog { elements: d.elements, ..TextTypeDialog::new(&self.session.model, name, Vec::new()) },
                Err(e) => d.error = Some(e),
            },
            Some(Head::Rename(to)) => match self.exec("text_type", &json!({"name": d.name, "rename": to})) {
                Ok(_) => {
                    if self.types.get("text") == Some(&d.name) {
                        self.types.insert("text", to.clone());
                    }
                    // What was typed and not yet applied stays.
                    let typed = d.t.clone();
                    d = TextTypeDialog { elements: d.elements, ..TextTypeDialog::new(&self.session.model, to, Vec::new()) };
                    d.t = typed;
                }
                Err(e) => d.error = Some(e),
            },
            None => {}
        }
        if ok || apply {
            let changed = model.text_types.get(&d.name) != Some(&d.t);
            let mut result = if changed { self.exec("text_type", &d.params(&d.name)).map(|_| ()) } else { Ok(()) };
            let moved: Vec<Id> = d
                .elements
                .iter()
                .copied()
                .filter(|id| matches!(self.session.model.elements.get(*id), Some(Element::Text(n)) if n.type_name != d.name))
                .collect();
            if ok && result.is_ok() && !moved.is_empty() {
                result = self.exec("modify", &json!({"ids": moved, "type": d.name})).map(|_| ());
            }
            match result {
                Ok(()) if ok => return,
                Ok(()) => d.error = None,
                Err(e) => d.error = Some(e),
            }
        }
        self.text_type_dialog = Some(d);
    }
}

/// Whether element `id` of `m` is a note, which a double click edits.
pub(crate) fn is_note(m: &Model, id: Id) -> bool {
    matches!(m.elements.get(id), Some(Element::Text(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing(text: &str) -> (Model, NoteEdit) {
        let m = Model::new("x");
        let mut e = NoteEdit::new(&m, 100_000, P2(0.0, 0.0), "2.5mm".into());
        e.insert(text);
        (m, e)
    }

    #[test]
    fn typing_erasing_and_moving_the_caret_work_on_characters_not_bytes() {
        let (_, mut e) = editing("őz");
        e.insert("\n");
        e.insert("Жx\u{7}");
        assert_eq!(e.text(), "őz\nЖx");
        e.erase(false);
        e.step(false, false);
        e.step(false, true);
        assert_eq!(e.selected(), "\n");
        e.erase(true);
        assert_eq!(e.text(), "őzЖ");
        e.select_all();
        e.insert("new");
        assert_eq!((e.text(), e.caret), ("new".to_string(), 3));
    }

    #[test]
    fn formatting_flips_the_selection_and_typing_takes_the_format_before_it() {
        let (m, mut e) = editing("plain words");
        e.note.override_format = true;
        (e.anchor, e.caret) = (0, 5);
        let base = Style::default();
        e.toggle(base, Which::Bold);
        assert!(e.all(base, Which::Bold));
        e.toggle(base, Which::Bold);
        assert!(!e.all(base, Which::Bold) && e.formats[0].bold == Some(false), "kept, as not bold, whatever the type");
        e.toggle(base, Which::Italic);
        e.put(5, false);
        e.insert("!");
        assert_eq!(e.formats[5].italic, Some(true));
        let (c, p) = e.command(&m).unwrap();
        assert_eq!(c, "text");
        assert_eq!(p["runs"][0]["to"], 6);
        // A note left empty makes nothing.
        let (m, e) = editing("  ");
        assert!(e.command(&m).is_none());
    }

    #[test]
    fn the_caret_goes_up_and_down_lines_and_to_their_ends() {
        let (m, mut e) = editing("abc\nde\nfghij");
        let t = m.text_types["2.5mm"].clone();
        let layout = text::layout(&t, &e.text(), &text::styles(&t, &e.text(), false, &[]));
        e.put(12, false);
        e.line_step(&layout, false, false);
        assert_eq!(e.caret, 6, "the end of a shorter line");
        e.line_end(&layout, false, true);
        assert_eq!(e.range(), (4, 6));
        e.line_step(&layout, true, false);
        e.line_step(&layout, true, false);
        assert_eq!(e.caret, 12, "down from the last line, its end");
        e.select_word(9);
        assert_eq!(e.selected(), "fghij");
    }
}
