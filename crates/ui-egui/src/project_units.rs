//! Manage > Project Units, as Revit's: a table of the kinds of value a project shows, each with the format it is shown in,
//! and a dialog of its own for the one picked. Length takes its units, its rounding and its places; the others take theirs.
//! Every change is the `units` command, so it is checked, rolled back when it breaks a rule, and undone like any other.

use bimcraft_core::dims::{PositiveSign, ZeroSign};
use bimcraft_core::units::{AreaUnit, DecimalStyle, FormatOptions, MassDensityUnit, SlopeUnit, SpeedUnit, TimeUnit, Units, VolumeUnit};
use egui::{Align, Button, ComboBox, Context, Grid, Id as UiId, Layout, RichText, TextEdit, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::t;
use crate::popup::Modal;
use crate::settings::{Precision, precision_choices, precision_label, precision_of, precision_params};
use crate::style::pal;

/// A kind of value the project shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quantity {
    Length,
    Area,
    Volume,
    Angle,
    Slope,
    MassDensity,
    Time,
    Speed,
    Currency,
}

impl Quantity {
    const ALL: [Quantity; 9] = [
        Quantity::Length,
        Quantity::Area,
        Quantity::Volume,
        Quantity::Angle,
        Quantity::Slope,
        Quantity::MassDensity,
        Quantity::Time,
        Quantity::Speed,
        Quantity::Currency,
    ];

    fn core(self) -> bimcraft_core::units::Quantity {
        match self {
            Quantity::Length => bimcraft_core::units::Quantity::Length,
            Quantity::Area => bimcraft_core::units::Quantity::Area,
            Quantity::Volume => bimcraft_core::units::Quantity::Volume,
            Quantity::Angle => bimcraft_core::units::Quantity::Angle,
            Quantity::Slope => bimcraft_core::units::Quantity::Slope,
            Quantity::MassDensity => bimcraft_core::units::Quantity::MassDensity,
            Quantity::Time => bimcraft_core::units::Quantity::Time,
            Quantity::Speed => bimcraft_core::units::Quantity::Speed,
            Quantity::Currency => bimcraft_core::units::Quantity::Currency,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Quantity::Length => "Length",
            Quantity::Area => "Area",
            Quantity::Volume => "Volume",
            Quantity::Angle => "Angle",
            Quantity::Slope => "Slope",
            Quantity::MassDensity => "Mass Density",
            Quantity::Time => "Time",
            Quantity::Speed => "Speed",
            Quantity::Currency => "Currency",
        }
    }
}

/// What the format dialog holds until OK or Cancel: the kind, and what it is set to.
enum Draft {
    Length { units: Units, precision: Precision },
    Area(AreaUnit),
    Volume(VolumeUnit),
    Angle,
    Slope(SlopeUnit),
    MassDensity(MassDensityUnit),
    Time(TimeUnit),
    Speed(SpeedUnit),
    Currency(String),
}

#[derive(Default)]
pub(crate) struct ProjectUnitsDialog {
    /// The kind whose format dialog is open over the table, if any.
    open: Option<Quantity>,
    draft: Option<Draft>,
    /// The kind's format options, edited beside its unit.
    options: FormatOptions,
    said: Option<String>,
}

impl ProjectUnitsDialog {
    fn open(&mut self, quantity: Quantity, m: &bimcraft_core::model::Model) {
        self.open = Some(quantity);
        self.said = None;
        self.options = m.display.options(quantity.core()).clone();
        self.draft = Some(match quantity {
            Quantity::Length => Draft::Length { units: m.units, precision: precision_of(m) },
            Quantity::Area => {
                Draft::Area(m.display.area.unwrap_or(if m.units.imperial() { AreaUnit::SquareFeet } else { AreaUnit::SquareMetres }))
            }
            Quantity::Volume => {
                Draft::Volume(m.display.volume.unwrap_or(if m.units.imperial() { VolumeUnit::CubicFeet } else { VolumeUnit::CubicMetres }))
            }
            Quantity::Angle => Draft::Angle,
            Quantity::Slope => Draft::Slope(m.display.slope.unwrap_or_default()),
            Quantity::MassDensity => Draft::MassDensity(m.display.mass_density.unwrap_or_default()),
            Quantity::Time => Draft::Time(m.display.time.unwrap_or_default()),
            Quantity::Speed => Draft::Speed(m.display.speed.unwrap_or_default()),
            Quantity::Currency => Draft::Currency(m.display.currency.clone().unwrap_or_else(|| "$".to_string())),
        });
    }

    /// The `units` command's parameters for the draft, or None when there is nothing to change.
    fn params(&self) -> Option<Value> {
        let quantity = self.open?;
        let mut p = match self.draft.as_ref()? {
            Draft::Length { units, precision } => {
                let mut p = precision_params(*precision);
                if let Some(o) = p.as_object_mut() {
                    o.insert("length".into(), json!(units.name()));
                }
                p
            }
            Draft::Area(unit) => json!({"area": unit.name()}),
            Draft::Volume(unit) => json!({"volume": unit.name()}),
            Draft::Angle => json!({}),
            Draft::Slope(unit) => json!({"slope": unit.name()}),
            Draft::MassDensity(unit) => json!({"mass_density": unit.name()}),
            Draft::Time(unit) => json!({"time": unit.name()}),
            Draft::Speed(unit) => json!({"speed": unit.name()}),
            Draft::Currency(symbol) => json!({"currency": symbol.trim()}),
        };
        if let Some(o) = p.as_object_mut() {
            o.insert("format".into(), format_params(quantity.core(), &self.options));
        }
        Some(p)
    }
}

/// `value` to `places` after the point, then its unit.
fn figure(value: f64, places: usize, unit: &str) -> String {
    format!("{value:.places$} {unit}")
}

/// A sample value of each kind as the project shows it, for the table's Format column.
fn sample(m: &bimcraft_core::model::Model, quantity: Quantity) -> String {
    let text = match quantity {
        Quantity::Length => m.with_unit(1235.0),
        Quantity::Area => {
            let (value, unit) = m.area(1.2345e6);
            figure(value, 2, unit)
        }
        Quantity::Volume => {
            let (value, unit) = m.volume_shown(1.23456e9);
            figure(value, 3, unit)
        }
        Quantity::Angle => "12.35°".to_string(),
        Quantity::Slope => {
            let (value, unit) = m.slope(12.345);
            format!("{value:.2}{unit}")
        }
        Quantity::MassDensity => {
            let (value, unit) = m.mass_density(1234.567);
            figure(value, 1, unit)
        }
        Quantity::Time => {
            let (value, unit) = m.time(1234.6);
            figure(value, 1, unit)
        }
        Quantity::Speed => {
            let (value, unit) = m.speed(12.345);
            figure(value, 2, unit)
        }
        Quantity::Currency => figure(1234.57, 2, m.display.currency()),
    };
    m.display.decimal_style.apply(text, false)
}

/// A rounding choice as the format dialog writes it, with no unit: `10`, `1`, `0.00`.
fn precision_number(precision: Precision) -> String {
    match precision {
        Precision::Before(digits) => format!("{}", 10u64.pow(u32::from(digits))),
        Precision::Whole => "1".to_string(),
        Precision::After(0) => "1".to_string(),
        Precision::After(digits) => format!("0.{}", "0".repeat(usize::from(digits))),
    }
}

/// The format's rounding and places for a quantity other than a length: to 10, 100 or 1000, whole, or places after the point.
fn rounding_row(ui: &mut egui::Ui, quantity: Quantity, o: &mut FormatOptions) {
    let default = quantity.core().default_decimals();
    let selected = if let Some(digits) = o.rounding {
        Precision::Before(digits)
    } else if o.decimals == Some(0) {
        Precision::Whole
    } else {
        Precision::After(o.decimals.unwrap_or(default))
    };
    let choices = [
        Precision::Before(1),
        Precision::Before(2),
        Precision::Before(3),
        Precision::Whole,
        Precision::After(1),
        Precision::After(2),
        Precision::After(3),
        Precision::After(4),
    ];
    ui.horizontal(|ui| {
        ui.label(t("Rounding"));
        let mut picked = selected;
        ComboBox::from_id_salt(("format-rounding", quantity.label())).selected_text(precision_number(selected)).width(230.0).show_ui(
            ui,
            |ui| {
                for p in choices {
                    if ui.selectable_label(p == selected, precision_number(p)).clicked() {
                        picked = p;
                    }
                }
            },
        );
        if picked != selected {
            match picked {
                Precision::Before(digits) => {
                    o.rounding = Some(digits);
                    o.decimals = Some(0);
                }
                Precision::Whole => {
                    o.rounding = None;
                    o.decimals = Some(0);
                }
                Precision::After(digits) => {
                    o.rounding = None;
                    o.decimals = Some(digits);
                }
            }
        }
    });
}

/// The `units` command's `format` object for `quantity`, as the dialog holds it.
fn format_params(quantity: bimcraft_core::units::Quantity, o: &FormatOptions) -> Value {
    json!({
        "quantity": quantity.name(),
        "show_units": o.show_units,
        "positive": o.positive,
        "zero": o.zero,
        "suppress_trailing_zeros": o.suppress_trailing_zeros,
        "suppress_zero_feet": o.suppress_zero_feet,
        "digit_grouping": o.digit_grouping,
        "suppress_spaces": o.suppress_spaces,
        "rounding": o.rounding,
        "decimals": o.decimals,
        "symbol": o.symbol,
    })
}

impl BimApp {
    pub(crate) fn open_project_units(&mut self) {
        self.project_units = Some(ProjectUnitsDialog::default());
    }

    pub(crate) fn project_units_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.project_units.take() else { return };
        // The format dialog stands in place of the table, as Revit's does.
        if d.open.is_some() {
            self.project_units = Some(d);
            self.project_units_format_window(ctx);
            return;
        }
        let model = std::sync::Arc::clone(&self.session.model);
        let mut close = false;
        let mut pick = None;
        let r = Modal::new(UiId::new("project-units"), t("Project Units")).width(560.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(t("Discipline"));
                    ComboBox::from_id_salt("project-units-discipline").selected_text(t("Common")).width(220.0).show_ui(ui, |ui| {
                        let _ = ui.selectable_label(true, t("Common"));
                    });
                });
                ui.add_space(6.0);
                Grid::new("project-units-table").num_columns(2).spacing(vec2(6.0, 2.0)).min_col_width(170.0).striped(true).show(ui, |ui| {
                    ui.label(RichText::new(t("Units")).strong());
                    ui.label(RichText::new(t("Format")).strong());
                    ui.end_row();
                    for q in Quantity::ALL {
                        ui.label(t(q.label()));
                        if ui.add_sized([260.0, 20.0], Button::new(sample(&model, q))).clicked() {
                            pick = Some(q);
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new(t("Decimal symbol/digit grouping:")).color(pal().muted));
                let style = self.session.model.display.decimal_style;
                let mut picked = style;
                ComboBox::from_id_salt("project-units-decimal-symbol").selected_text(style.label()).width(220.0).show_ui(ui, |ui| {
                    for s in DecimalStyle::ALL {
                        if ui.selectable_label(picked == s, s.label()).clicked() {
                            picked = s;
                        }
                    }
                });
                if picked != style {
                    self.run("units", json!({"decimal_style": picked.name()}));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.add(Button::new(t("OK")).min_size(vec2(76.0, 0.0))).clicked() {
                        close = true;
                    }
                });
            });
        });
        if let Some(q) = pick {
            d.open(q, &model);
        }
        if let Some(said) = d.said.take() {
            self.status = crate::Status { text: said, error: true };
        }
        if close || r.should_close() {
            self.project_units = None;
        } else {
            self.project_units = Some(d);
        }
        self.project_units_format_window(ctx);
    }

    /// The format dialog over the table, for the kind picked.
    fn project_units_format_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.project_units.take() else { return };
        let Some(quantity) = d.open else {
            self.project_units = Some(d);
            return;
        };
        let global_show = self.show_units;
        let mut ok = false;
        let mut cancel = false;
        let r = Modal::new(UiId::new("project-units-format"), t("Format")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t(quantity.label())).strong());
                ui.add_space(4.0);
                let show_units_label = d.options.show_units.unwrap_or(global_show);
                match d.draft.as_mut() {
                    Some(Draft::Length { units, precision }) => {
                        ui.horizontal(|ui| {
                            ui.label(t("Units"));
                            let before = *units;
                            ComboBox::from_id_salt("format-length-units").selected_text(t(units.label())).width(230.0).show_ui(ui, |ui| {
                                for u in Units::ALL {
                                    if ui.selectable_label(*units == u, t(u.label())).clicked() {
                                        *units = u;
                                    }
                                }
                            });
                            // The places of one unit are not another's: a new unit starts whole.
                            if *units != before {
                                *precision = Precision::Whole;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(t("Rounding"));
                            let choices = precision_choices(*units);
                            ComboBox::from_id_salt("format-length-rounding")
                                .selected_text(precision_label(*units, show_units_label, *precision))
                                .width(230.0)
                                .show_ui(ui, |ui| {
                                    for p in choices {
                                        if ui.selectable_label(*precision == p, precision_label(*units, show_units_label, p)).clicked() {
                                            *precision = p;
                                        }
                                    }
                                });
                        });
                    }
                    Some(Draft::Area(unit)) => {
                        unit_combo(ui, "format-area-units", t("Units"), AreaUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::Volume(unit)) => {
                        unit_combo(ui, "format-volume-units", t("Units"), VolumeUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::Angle) => {
                        ui.label(t("Degrees (°)"));
                    }
                    Some(Draft::Slope(unit)) => {
                        unit_combo(ui, "format-slope-units", t("Units"), SlopeUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::MassDensity(unit)) => {
                        unit_combo(ui, "format-density-units", t("Units"), MassDensityUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::Time(unit)) => {
                        unit_combo(ui, "format-time-units", t("Units"), TimeUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::Speed(unit)) => {
                        unit_combo(ui, "format-speed-units", t("Units"), SpeedUnit::ALL, unit, |u| t(u.label()));
                    }
                    Some(Draft::Currency(symbol)) => {
                        ui.horizontal(|ui| {
                            ui.label(t("Symbol"));
                            ui.add(TextEdit::singleline(symbol).desired_width(120.0));
                        });
                    }
                    None => {}
                }
                if let Some(q) = d.open
                    && q != Quantity::Length
                {
                    rounding_row(ui, q, &mut d.options);
                }
                ui.add_space(6.0);
                let mut show = d.options.show_units.unwrap_or(global_show);
                if ui.checkbox(&mut show, t("Show units in values")).changed() {
                    d.options.show_units = Some(show);
                }
                let mut plus = d.options.positive == Some(PositiveSign::Always);
                if ui.checkbox(&mut plus, t("Show + for positive values")).changed() {
                    d.options.positive = Some(if plus { PositiveSign::Always } else { PositiveSign::Never });
                }
                let mut signed = d.options.zero == Some(ZeroSign::Signed)
                    || (d.options.zero.is_none() && d.options.positive == Some(PositiveSign::Always));
                if ui.checkbox(&mut signed, t("Show ± for zero")).changed() {
                    d.options.zero = Some(if signed { ZeroSign::Signed } else { ZeroSign::Plain });
                }
                ui.checkbox(&mut d.options.suppress_trailing_zeros, t("Suppress trailing 0's"));
                ui.checkbox(&mut d.options.suppress_zero_feet, t("Suppress 0 feet"));
                ui.checkbox(&mut d.options.digit_grouping, t("Use digit grouping"));
                ui.checkbox(&mut d.options.suppress_spaces, t("Suppress spaces"));
                ui.horizontal(|ui| {
                    ui.label(t("Unit symbol"));
                    let mut symbol = d.options.symbol.clone().unwrap_or_default();
                    if ui.add(TextEdit::singleline(&mut symbol).desired_width(180.0)).changed() {
                        d.options.symbol = if symbol.trim().is_empty() { None } else { Some(symbol) };
                    }
                });
                if let Some(e) = &d.said {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let size = vec2(76.0, 0.0);
                    cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                    ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                });
            });
        });
        if ok {
            match d.params() {
                Some(p) => {
                    if self.run("units", p).is_none() {
                        d.said = Some(std::mem::take(&mut self.status).text);
                    } else {
                        d.open = None;
                        d.draft = None;
                    }
                }
                None => {
                    d.open = None;
                    d.draft = None;
                }
            }
        } else if cancel || r.should_close() {
            d.open = None;
            d.draft = None;
        }
        self.project_units = Some(d);
    }
}

/// A row of `label` and a drop-down of `options`, each named by `label_of`, for the draft's unit.
fn unit_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    options: impl IntoIterator<Item = T>,
    current: &mut T,
    label_of: impl Fn(T) -> &'static str,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ComboBox::from_id_salt(id).selected_text(t(label_of(*current))).width(230.0).show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(*current == o, t(label_of(o))).clicked() {
                    *current = o;
                }
            }
        });
    });
}
