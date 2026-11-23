//! A family's options as the family document edits them (T5b): Revit's Family Category and Parameters dialog lists them in its
//! Family Parameters table, and Properties lists them in its Other group when nothing is selected. Both edit the draft's
//! [`OptionsText`], so the dialog's Apply, OK and Cancel and the document's history take them with the rest. What the
//! core's `family {options}` would refuse (an OmniClass number that isn't digits and dots, say) is
//! [`FamilyEditor::options_refusal`], which the dialog says inside itself and keeps it open for.
use super::*;
use bimcraft_core::family::OmniClass;
use bimcraft_core::units::Units;

/// Where Room Calculation Point starts when it is turned on: the insertion point, raised 300 mm so it is seen apart from it
/// in the family's views. Which room a component is in is decided in plan, so the room is the insertion point's until the
/// point is moved.
pub(crate) const ROOM_POINT_START: [f64; 3] = [0.0, 0.0, 300.0];

/// The labels of the options' rows, in Revit's order.
pub(crate) const WORK_PLANE_BASED: &str = msg!("Work Plane-Based");
pub(crate) const ALWAYS_VERTICAL: &str = msg!("Always Vertical");
pub(crate) const CUT_WITH_VOIDS: &str = msg!("Cut with Voids When Loaded");
pub(crate) const SHARED: &str = msg!("Shared");
pub(crate) const ROOM_POINT: &str = msg!("Room Calculation Point");
pub(crate) const CUT_IN_VIEWS: &str = msg!("Enable Cutting in Views");
pub(crate) const BY_CATEGORY: &str = msg!("By Category");
pub(crate) const OMNICLASS_NUMBER: &str = msg!("OmniClass Number");
pub(crate) const OMNICLASS_TITLE: &str = msg!("OmniClass Title");
pub(crate) const DEFAULT_ELEVATION: &str = msg!("Default Elevation");

/// The options as the document holds them: a flag each, the room calculation point (its coordinates numbers, mm, or formulas a
/// file brought), the override of the category's rule for sections, the OmniClass number and title as typed, and the
/// default elevation, mm.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OptionsText {
    pub(crate) work_plane_based: bool,
    pub(crate) always_vertical: bool,
    pub(crate) cut_with_voids_when_loaded: bool,
    pub(crate) shared: bool,
    pub(crate) room_calculation_point: Option<[Expr; 3]>,
    /// None: as the category says.
    pub(crate) cut_in_views: Option<bool>,
    pub(crate) omniclass_number: String,
    pub(crate) omniclass_title: String,
    pub(crate) default_elevation: f64,
}

impl OptionsText {
    /// The options family `f` has.
    pub(crate) fn of(f: &Family) -> OptionsText {
        let (omniclass_number, omniclass_title) = f.omniclass.as_ref().map(|o| (o.number.clone(), o.title.clone())).unwrap_or_default();
        OptionsText {
            work_plane_based: f.work_plane_based,
            always_vertical: f.always_vertical,
            cut_with_voids_when_loaded: f.cut_with_voids_when_loaded,
            shared: f.shared,
            room_calculation_point: f.room_calculation_point.clone(),
            cut_in_views: f.cut_in_views,
            omniclass_number,
            omniclass_title,
            default_elevation: f.default_elevation,
        }
    }

    /// Family `f` given these options. An OmniClass number and title both blank are none; either typed, they are one, which
    /// the family's check then reads.
    pub(crate) fn apply(&self, f: &mut Family) {
        f.work_plane_based = self.work_plane_based;
        f.always_vertical = self.always_vertical;
        f.cut_with_voids_when_loaded = self.cut_with_voids_when_loaded;
        f.shared = self.shared;
        f.room_calculation_point.clone_from(&self.room_calculation_point);
        f.cut_in_views = self.cut_in_views;
        let classified = !self.omniclass_number.is_empty() || !self.omniclass_title.is_empty();
        f.omniclass = classified.then(|| OmniClass { number: self.omniclass_number.clone(), title: self.omniclass_title.clone() });
        f.default_elevation = self.default_elevation;
    }
}

impl FamilyEditor {
    /// Why the `family` command would refuse the draft's options, if it would: the core's own words.
    pub(crate) fn options_refusal(&self) -> Option<String> {
        self.family().ok()?.check_options().err()
    }
}

/// The options' rows, in a two-column grid already begun: a label, then its value `w` wide, each row ended. `salt` keeps the
/// dialog's fields apart from the palette's; `category` decides what Work Plane-Based allows and what By Category means.
pub(crate) fn rows(ui: &mut Ui, o: &mut OptionsText, category: Category, units: Units, salt: &str, w: f32) {
    let hosted = category.hosted();
    // A door's or a window's family can't be one, though one a file made so can still be turned off.
    let allowed = !hosted || o.work_plane_based;
    tick(ui, WORK_PLANE_BASED, &mut o.work_plane_based, allowed)
        .on_disabled_hover_text(t("A door or a window stands in its wall: it can't be work plane-based."));
    // Allowed off without Work Plane-Based too, as agreed: a wall-hosted family has it.
    tick(ui, ALWAYS_VERTICAL, &mut o.always_vertical, true);
    tick(ui, CUT_WITH_VOIDS, &mut o.cut_with_voids_when_loaded, true);
    tick(ui, SHARED, &mut o.shared, true);
    let mut on = o.room_calculation_point.is_some();
    if tick(ui, ROOM_POINT, &mut on, true).changed() {
        o.room_calculation_point = on.then(|| ROOM_POINT_START.map(Expr::Number));
    }
    if let Some(point) = o.room_calculation_point.as_mut() {
        for (axis, e) in [msg!("X"), msg!("Y"), msg!("Z")].into_iter().zip(point.iter_mut()) {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(t(axis));
            });
            let name = format!("{} {axis}", t(ROOM_POINT));
            match e {
                Expr::Number(x) => {
                    if let Some(mm) = length(ui, UiId::new((salt, "room-point", axis)), None, &name, *x, units, w) {
                        *x = mm;
                    }
                }
                // A formula a file or the `family` command gave it: shown, not typed over.
                Expr::Formula(f) => {
                    let mut shown = f.clone();
                    let r = ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(w));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, false, name.as_str()));
                }
            }
            ui.end_row();
        }
    }
    cut_in_views(ui, o, category);
    text(ui, salt, OMNICLASS_NUMBER, &mut o.omniclass_number, w, Some("23.30.20.17"));
    text(ui, salt, OMNICLASS_TITLE, &mut o.omniclass_title, w, None);
    let l = ui.label(t(DEFAULT_ELEVATION));
    if let Some(mm) = length(ui, UiId::new((salt, "default-elevation")), Some(l.id), t(DEFAULT_ELEVATION), o.default_elevation, units, w) {
        o.default_elevation = mm;
    }
    ui.end_row();
}

/// A row of a tick, named `label`.
fn tick(ui: &mut Ui, label: &str, on: &mut bool, enabled: bool) -> egui::Response {
    let l = ui.label(t(label));
    let r = ui.add_enabled(enabled, egui::Checkbox::without_text(on)).labelled_by(l.id);
    let name = t(label).to_string();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, *on, name.as_str()));
    ui.end_row();
    r
}

/// Enable Cutting in Views: Revit's tick, and beside it By Category, ticked while the category's rule decides (the tick then
/// shows that rule, greyed). Untick By Category and the tick is the family's own.
fn cut_in_views(ui: &mut Ui, o: &mut OptionsText, category: Category) {
    let l = ui.label(t(CUT_IN_VIEWS));
    ui.horizontal(|ui| {
        let mut on = o.cut_in_views.unwrap_or_else(|| category.cut());
        let own = o.cut_in_views.is_some();
        let r = ui.add_enabled(own, egui::Checkbox::without_text(&mut on)).labelled_by(l.id);
        let name = t(CUT_IN_VIEWS).to_string();
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, own, on, name.as_str()));
        if r.changed() {
            o.cut_in_views = Some(on);
        }
        let mut by = !own;
        let tip = if category.cut() { t("By its category, sections cut it.") } else { t("By its category, sections don't cut it.") };
        if ui.checkbox(&mut by, t(BY_CATEGORY)).on_hover_text(tip).changed() {
            o.cut_in_views = (!by).then(|| category.cut());
        }
    });
    ui.end_row();
}

/// A row of text, named `label`, `w` wide, with a hint when it is blank.
fn text(ui: &mut Ui, salt: &str, label: &str, value: &mut String, w: f32, hint: Option<&str>) {
    let l = ui.label(t(label));
    let mut edit = TextEdit::singleline(value).id_salt((salt, label));
    if let Some(h) = hint {
        edit = edit.hint_text(h);
    }
    let r = ui.add_sized([w, ui.spacing().interact_size.y], edit).labelled_by(l.id);
    let name = t(label).to_string();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.as_str()));
    ui.end_row();
}

/// A length typed in the project's units, `w` wide, named `name` and labelled by `label`: kept as typed while it has the focus (so "2." is not read
/// back as "2"), shown as `mm` otherwise. What was typed, mm, when it changed and reads as a length within reach.
fn length(ui: &mut Ui, key: UiId, label: Option<UiId>, name: &str, mm: f64, units: Units, w: f32) -> Option<f64> {
    let mut typed = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| units.length(mm));
    let mut r = ui.add_sized([w, ui.spacing().interact_size.y], TextEdit::singleline(&mut typed).id_salt(key));
    if let Some(l) = label {
        r = r.labelled_by(l);
    }
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name));
    let read = r.changed().then(|| units.parse(&typed)).flatten().filter(|x| x.is_finite() && x.abs() <= bimcraft_core::model::MAX_COORD);
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, typed));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    read
}
