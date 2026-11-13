//! Manage > Run Script: a script of commands, one a line as the command line and agents write them (`id {json params}`, `#`
//! a comment), tried first and then run on the open project. Try is the core's dry run (`Session::script_dry_run`): what it
//! would make, change and take away, and the warnings it would leave, with the project untouched. Run is all or nothing and
//! one undo step, `Script` (`Session::script_atomic`), and only after the same dry run passes, so a script never opens,
//! saves or replaces the project behind the window's back: a line that would touch a file or the history is refused before
//! anything runs.

use egui::{Align, Button, Context, Id as UiId, Layout, RichText, ScrollArea, TextEdit};
use serde_json::Value;

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Status};

/// The Run Script dialog: the script as typed or pasted, and what the last Try or Run said of it.
#[derive(Default)]
pub(crate) struct RunScript {
    pub(crate) text: String,
    /// What the last Try found, or why it or Run was refused; `error` when refused.
    said: Option<(String, bool)>,
    /// The text box takes the keys when the dialog opens.
    focus: bool,
}

/// What a dry run's reply says in a line: how many elements it would make, change and take away, whether it would change
/// anything else, and the warnings the model would have.
pub(crate) fn summary(dry: &Value) -> String {
    let n = |k: &str| dry[k].as_array().map_or(0, Vec::len).to_string();
    let mut s = tf("Would make {}, change {} and remove {} elements.", &[&n("made"), &n("changed"), &n("removed")]);
    if dry["other_changes"].as_bool() == Some(true) {
        s.push(' ');
        s.push_str(t("It changes the project's settings or library too."));
    }
    let warnings = dry["warnings"].as_array().map_or(0, Vec::len);
    if warnings > 0 {
        s.push(' ');
        s.push_str(&tf("The project would then have {} warnings.", &[&warnings.to_string()]));
    }
    s
}

impl RunScript {
    /// What the last Try or Run said, and whether it refused.
    #[cfg(test)]
    pub(crate) fn said(&self) -> Option<(&str, bool)> {
        self.said.as_ref().map(|(s, e)| (s.as_str(), *e))
    }
}

impl BimApp {
    /// Manage > Run Script.
    pub(crate) fn open_run_script(&mut self) {
        self.run_script = Some(RunScript { focus: true, ..RunScript::default() });
    }

    /// Run `text` on the project as one undo step, once a dry run of it passes. What it did, or why not, in the status box;
    /// whether it ran.
    pub(crate) fn run_script_text(&mut self, text: &str) -> Result<String, String> {
        let dry = self.session.script_dry_run(text)?;
        let lines = self.exec_script(text)?.len();
        let done = tf("Ran {} lines as one undo step. {}", &[&lines.to_string(), &summary(&dry)]);
        self.status = Status { text: done.clone(), error: false };
        Ok(done)
    }

    pub(crate) fn run_script_window(&mut self, ctx: &Context) {
        let Some(d) = self.run_script.as_mut() else { return };
        let (mut try_it, mut run, mut close) = (false, false, false);
        let any = d.text.lines().any(|l| !l.trim().is_empty() && !l.trim().starts_with('#'));
        let r = Modal::new(UiId::new("run-script"), t("Run Script")).width(640.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                let label = ui.label(t("Commands, one a line (id {json params}; # starts a comment):"));
                let edit = ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut d.text)
                                .code_editor()
                                .desired_rows(10)
                                .desired_width(f32::INFINITY)
                                .hint_text("wall {\"points\": [[0, 0], [5000, 0]]}"),
                        )
                    })
                    .inner;
                if std::mem::take(&mut d.focus) {
                    edit.request_focus();
                }
                let edit = edit.labelled_by(label.id);
                if edit.changed() {
                    d.said = None;
                }
                ui.add_space(4.0);
                ui.label(RichText::new(t("Try shows what it would do and changes nothing. Run does it as one undo step.")).weak());
                if let Some((said, error)) = &d.said {
                    ui.add_space(4.0);
                    let text = RichText::new(said.as_str());
                    ui.label(if *error { text.color(ui.visuals().error_fg_color) } else { text });
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    run = ui.add_enabled(any, Button::new(t("Run"))).clicked();
                    try_it = ui.add_enabled(any, Button::new(t("Try"))).clicked();
                });
            });
        });
        if try_it {
            let text = d.text.clone();
            let said = match self.session.script_dry_run(&text) {
                Ok(dry) => (summary(&dry), false),
                Err(e) => (e, true),
            };
            if let Some(d) = self.run_script.as_mut() {
                d.said = Some(said);
            }
        } else if run {
            let text = d.text.clone();
            match self.run_script_text(&text) {
                // Done: the dialog closes, and the status box says what it did.
                Ok(_) => self.run_script = None,
                Err(e) => {
                    if let Some(d) = self.run_script.as_mut() {
                        d.said = Some((e, true));
                    }
                }
            }
        } else if close || r.should_close() {
            self.run_script = None;
        }
    }
}
