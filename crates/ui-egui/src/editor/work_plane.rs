//! T12: Revit's Work Plane dialog in the family document, for the view's work plane (Create > Work Plane > Set), for a
//! selected form's (its contextual tab's Edit Work Plane) and for the form being sketched (sketch mode's Work Plane > Set,
//! its lines going with the plane). Current Work Plane names it, with Show (the same toggle as the
//! ribbon's) and Dissociate (greyed: a family's work plane is always one of its planes or lines, which forms are drawn on).
//! Specify a new Work Plane: Name, from the family's planes as `Planes::listed` names them; Pick a plane, a reference plane's
//! or a datum's trace clicked in a plan or an elevation; Pick a line, a reference line clicked there, whose own plane is
//! used. OK closes the dialog and the click picks; Escape while picking brings the dialog back. A view's work plane that the
//! view doesn't show flat asks Go To View: the views that do (`views::flat_views`), and {3D}, which takes any; Open View
//! shows the one chosen with the plane set there. Sketch mode's Create > Extrusion or Revolve asks the same Go To View
//! (`on_work_plane`, `Then`), the sketch beginning in the view opened. A form's moves with the plane (`edit_work_plane`), so
//! it asks nothing.
use super::*;
use bimcraft_core::family::views::{self as fv, Which};
use bimcraft_core::ids::Guid;

/// What the dialog sets the work plane of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The view shown.
    View,
    /// Form `i` (Edit Work Plane).
    Form(usize),
    /// The form being sketched (F5's Work Plane > Set): its lines, in the plane's own axes, go with the plane.
    Sketch,
}

/// What follows once the view's work plane is set, at once or in the view Go To View opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// Nothing more: the view's work plane (Work Plane > Set).
    View,
    /// The form being sketched put on it too.
    Sketch,
    /// Create > Extrusion, Blend, Revolve, Sweep or Swept Blend (`kind`), solid or `void`: sketch mode on it (F5, F6).
    Create { kind: bimcraft_core::family::forms::FormKind, void: bool },
}

/// How the dialog's Specify a new Work Plane takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum By {
    #[default]
    Name,
    Plane,
    Line,
}

/// The Work Plane dialog, open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Dialog {
    pub(crate) target: Target,
    pub(crate) by: By,
    /// The plane Name has chosen.
    pub(crate) chosen: PlaneRef,
}

/// Pick a plane or Pick a line, waiting for its click in a plan or an elevation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Picking {
    pub(crate) target: Target,
    pub(crate) line: bool,
}

/// Go To View, asked for `plane`: the views that show it flat, then {3D} (none), the one chosen, and what follows there.
/// The one Go To View of the family document: the Work Plane dialog's and sketch mode's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GoTo {
    pub(crate) plane: PlaneRef,
    pub(crate) views: Vec<Option<Which>>,
    pub(crate) chosen: usize,
    pub(crate) then: Then,
}

impl FamilyEditor {
    /// The name the dialog gives plane `r`, as `Planes::listed` does, translated; a plane of the family's own with no name,
    /// `Reference Plane`.
    pub(crate) fn plane_label(&self, r: PlaneRef) -> String {
        self.plane_set()
            .listed()
            .into_iter()
            .find(|(p, _)| *p == r)
            .map(|(_, l)| t(&l).to_string())
            .unwrap_or_else(|| t("Reference Plane").to_string())
    }

    /// The work plane `target` has now: the view's, or the form's host.
    fn current_plane(&self, target: Target) -> Option<PlaneRef> {
        match target {
            Target::View => Some(self.views.work_plane()),
            Target::Form(i) => self.forms.get(i).map(|f| f.host),
            Target::Sketch => self.sketch.as_ref().map(|s| s.host),
        }
    }

    /// The Work Plane dialog opened for `target`, Name on the plane it has now.
    pub(crate) fn open_work_plane_dialog(&mut self, target: Target) {
        let Some(chosen) = self.current_plane(target) else { return };
        self.views.picking = None;
        self.views.drawing = None;
        self.views.dialog = Some(Dialog { target, by: By::Name, chosen });
    }

    /// `plane` made the view's work plane and `then` done, where the view shown sees it flat ({3D} sees any); else Go To
    /// View is asked, listing the views that do (`views::flat_views`) and {3D}, and it is done in the one opened. Work Plane
    /// > Set and sketch mode's Create > Extrusion or Revolve both come here.
    pub(crate) fn on_work_plane(&mut self, plane: PlaneRef, then: Then) {
        let normal = self.plane_set().frame(&plane).map(|f| f.z);
        match (self.views.shown, normal) {
            (Some(which), Ok(n)) if !which.faces(n) => {
                let mut views: Vec<Option<Which>> = fv::flat_views(n).into_iter().map(Some).collect();
                views.push(None);
                self.views.go_to = Some(GoTo { plane, views, chosen: 0, then });
            }
            _ => self.done_on(plane, then),
        }
    }

    /// The view shown given work plane `plane`, and `then` done on it.
    fn done_on(&mut self, plane: PlaneRef, then: Then) {
        self.views.set_work_plane(plane);
        match then {
            Then::View => {}
            Then::Sketch => {
                if let Some(s) = self.sketch.as_mut() {
                    s.host = plane;
                }
            }
            Then::Create { kind, void } => self.start_form_sketch(None, kind, void, plane, Vec::new()),
        }
    }

    /// `plane` taken for `target`: the view's work plane (Go To View, where the view doesn't show it flat), or the form put
    /// on it, moving with it.
    fn take_plane(&mut self, base: &Arc<Model>, target: Target, plane: PlaneRef) -> Result<(), String> {
        match target {
            Target::View => {
                self.on_work_plane(plane, Then::View);
                Ok(())
            }
            Target::Sketch => {
                self.on_work_plane(plane, Then::Sketch);
                Ok(())
            }
            Target::Form(i) => {
                if self.forms.get(i).is_some_and(|f| f.host == plane) {
                    return Ok(());
                }
                self.through(base, i, "edit_work_plane", |name, guid| json!({"family": name, "form": guid, "host": plane}))
            }
        }
    }

    /// A click while Pick a plane or Pick a line waits: the plane under it (`plane`), or the plane the reference line under
    /// it (`line`, its place in the family's lines) is drawn on, taken for what the dialog was opened for. What it says when
    /// nothing fit is for the status bar.
    pub(super) fn picked(&mut self, base: &Arc<Model>, plane: Option<Guid>, line: Option<usize>) -> Option<String> {
        let pk = self.views.picking?;
        let found = if pk.line {
            let host = self.family().ok().and_then(|f| line.and_then(|i| f.lines.get(i)).map(|l| l.host));
            host.ok_or_else(|| t("Pick a line: click a reference line in the view.").to_string())
        } else {
            plane.map(PlaneRef::Plane).ok_or_else(|| t("Pick a plane: click a reference plane or a datum in the view.").to_string())
        };
        match found {
            Ok(p) => {
                self.views.picking = None;
                self.take_plane(base, pk.target, p).err()
            }
            Err(e) => Some(e),
        }
    }

    /// Escape while Pick waits: the dialog back as it was opened. Whether it was picking.
    pub(crate) fn stop_picking(&mut self) -> bool {
        let Some(pk) = self.views.picking.take() else { return false };
        self.open_work_plane_dialog(pk.target);
        if let Some(d) = &mut self.views.dialog {
            d.by = if pk.line { By::Line } else { By::Plane };
        }
        true
    }

    /// The Work Plane dialog, while open; what went wrong, for the status bar.
    pub(crate) fn work_plane_dialog(&mut self, ctx: &Context, base: &Arc<Model>) -> Option<String> {
        let mut d = self.views.dialog.clone()?;
        let listed = self.plane_set().listed();
        let current = self.current_plane(d.target).map(|p| self.plane_label(p)).unwrap_or_default();
        // Pick takes a click in a plan or an elevation, where planes and lines are drawn.
        let pickable = self.views.shown.is_some();
        let (mut ok, mut cancel, mut show) = (false, false, false);
        let title = if matches!(d.target, Target::Form(_)) { t("Edit Work Plane") } else { t("Work Plane") };
        let r = Modal::new(UiId::new("family-work-plane"), title).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("Current Work Plane")).strong());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let l = ui.label(t("Name:"));
                    ui.label(RichText::new(&current).strong()).labelled_by(l.id);
                });
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let on = self.views.show_work_plane;
                    let tip = msg!("Show: the work plane tinted in the view, on or off");
                    show = ui.add(egui::Button::new(t("Show")).selected(on)).on_hover_text(t(tip)).clicked();
                    let tip = msg!("Dissociate: a family's work plane is always one of its planes or lines, which forms are drawn on");
                    ui.add_enabled(false, egui::Button::new(t("Dissociate"))).on_disabled_hover_text(t(tip));
                });
                ui.add_space(8.0);
                ui.label(RichText::new(t("Specify a new Work Plane")).strong());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.radio_value(&mut d.by, By::Name, t("Name"));
                    let shown = listed.iter().find(|(p, _)| *p == d.chosen).map(|(_, l)| t(l).to_string()).unwrap_or_default();
                    let mut picked = None;
                    egui::ComboBox::from_id_salt("family-work-plane-name").selected_text(shown).width(300.0).show_ui(ui, |ui| {
                        for (p, label) in &listed {
                            if ui.selectable_label(d.chosen == *p, t(label)).clicked() {
                                picked = Some(*p);
                            }
                        }
                    });
                    if let Some(p) = picked {
                        (d.chosen, d.by) = (p, By::Name);
                    }
                });
                let off = off_view();
                for (by, label) in
                    [(By::Plane, msg!("Pick a plane")), (By::Line, msg!("Pick a line and use the work plane it was sketched in"))]
                {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.add_enabled_ui(pickable, |ui| ui.radio_value(&mut d.by, by, t(label))).response.on_disabled_hover_text(t(off));
                    });
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if show {
            self.views.show_work_plane = !self.views.show_work_plane;
        }
        if ok {
            self.views.dialog = None;
            return match d.by {
                By::Name => self.take_plane(base, d.target, d.chosen).err(),
                By::Plane | By::Line if pickable => {
                    self.views.picking = Some(Picking { target: d.target, line: d.by == By::Line });
                    None
                }
                By::Plane | By::Line => Some(t(off_view()).to_string()),
            };
        }
        if cancel || r.should_close() {
            self.views.dialog = None;
        } else {
            self.views.dialog = Some(d);
        }
        None
    }

    /// Go To View, while asked (by the Work Plane dialog or by Create > Extrusion or Revolve): the views that show the plane
    /// flat, and {3D}; Open View (or a double click) shows the one chosen with the plane set there and what was asked for
    /// done, Cancel does nothing.
    pub(crate) fn go_to_view_dialog(&mut self, ctx: &Context) {
        let Some(mut g) = self.views.go_to.clone() else { return };
        let name = self.plane_label(g.plane);
        let (mut open, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("family-go-to-view"), t("Go To View")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(tf("{} isn't parallel to this view. Open a view that shows it flat:", &[&name]));
                ui.add_space(6.0);
                for (k, which) in g.views.iter().enumerate() {
                    let r = ui.selectable_label(g.chosen == k, super::views::view_title(*which));
                    if r.clicked() {
                        g.chosen = k;
                    }
                    if r.double_clicked() {
                        open = true;
                    }
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    open = ui.button(t("Open View")).clicked() || open;
                });
            });
        });
        if open {
            self.views.go_to = None;
            if let Some(which) = g.views.get(g.chosen).copied() {
                self.views.show(which);
                self.done_on(g.plane, g.then);
            }
        } else if cancel || r.should_close() {
            self.views.go_to = None;
        } else {
            self.views.go_to = Some(g);
        }
    }
}

/// Why Pick can't be done in {3D}.
fn off_view() -> &'static str {
    msg!("Pick works in a plan or an elevation, where the family's planes and lines are drawn")
}
