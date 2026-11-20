//! Reservation as NBR 5626:2020 (Sistemas prediais de água fria e água quente) asks. Pure functions of their input: the
//! `reservoir_size` command is their caller, so none of this needs a tank or a pump in the model.
//!
//! - What the standard asks, and this module checks: the volume reserved is at least 24 h of the building's normal consumption (plus
//!   a fire reserve where one is stored with it); the potable volume is kept to what its quality allows, three days of consumption
//!   where the project cannot find a limit; an elevated reservoir has two compartments or more, a single-family house excepted, and
//!   the smallest holds the greatest peak of consumption over a normal maintenance; the supply refills the day's consumption in 6 h,
//!   or in 3 h for a single-family house; a pump station has two independent pumps or more, one of them a spare where it serves
//!   several towers.
//! - What it does not take from the standard, because the standard gives none: the consumption per person (a project's own
//!   assumption, from a source it names), how the volume divides between the lower and the upper reservoir (the usual share is only
//!   offered as a suggestion, a common practice and never a requirement), the fire reserve (the project's and the fire brigade's) and
//!   the Forchheimer pipe diameter. Every check says which of four kinds it is, and what a missing input leaves is `pending`, never
//!   a verdict.

use serde::{Deserialize, Serialize};

/// The rules a reservation is checked by: the Plumbing tab's Reservation section (`data/codes/plumbing/nbr.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservoirRules {
    /// The least reservation, in days of the building's normal consumption.
    pub min_days: f64,
    /// The potable volume not to pass where the project finds no limit of its own, in days of consumption.
    pub max_days: f64,
    /// Hours the supply has to refill the day's consumption, and for a single-family house.
    pub refill_hours: f64,
    pub refill_hours_house: f64,
    /// The fewest independent pumps of a pump station, and compartments of an elevated reservoir not a house's.
    pub min_pumps: u32,
    pub min_compartments: u32,
    /// The share of the potable volume the lower reservoir usually takes, per cent: a practice, never a requirement.
    pub usual_lower_percent: f64,
    /// What each check cites.
    pub standard: String,
}

impl crate::codes::Section for ReservoirRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Plumbing;
    const KEY: &'static str = "reservoir";
    fn check(&self) -> Result<(), String> {
        for (k, v, most) in [
            ("min_days", self.min_days, 365.0),
            ("max_days", self.max_days, 365.0),
            ("refill_hours", self.refill_hours, 240.0),
            ("refill_hours_house", self.refill_hours_house, 240.0),
            ("usual_lower_percent", self.usual_lower_percent, 100.0),
        ] {
            if !v.is_finite() || !(0.0..=most).contains(&v) {
                return Err(format!("reservation: `{k}` is from 0 to {most}, not {v}"));
            }
        }
        if self.refill_hours <= 0.0 || self.refill_hours_house <= 0.0 || self.min_days > self.max_days {
            return Err("reservation: the refill hours are more than 0, and `min_days` no more than `max_days`".into());
        }
        if self.min_pumps > 100 || self.min_compartments > 100 || self.standard.chars().count() > 500 {
            return Err("reservation: 100 pumps or compartments at most, and a source of 500 characters".into());
        }
        Ok(())
    }
}

/// What a check rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Requirement,
    Historical,
    Practice,
    Assumption,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Requirement => "current requirement",
            Kind::Historical => "historical reference",
            Kind::Practice => "engineering practice",
            Kind::Assumption => "configurable assumption",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Fail,
    Pending,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Fail => "fail",
            Status::Pending => "pending",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: &'static str,
    pub kind: Kind,
    pub status: Status,
    pub text: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct Input {
    /// The building's normal consumption in a day, litres.
    pub daily_l: f64,
    /// Days of it the reservoirs hold.
    pub days: f64,
    /// A fire reserve stored with the water, litres.
    pub fire_l: f64,
    pub house: bool,
    pub lower_percent: Option<f64>,
    pub compartments: Option<u32>,
    pub fill_flow_lps: Option<f64>,
    pub pumps: Option<u32>,
    pub towers: u32,
    pub smallest_l: Option<f64>,
    pub peak_l: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Split {
    pub lower_percent: f64,
    pub lower_l: f64,
    pub upper_l: f64,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    /// Days of consumption times the day's consumption, litres.
    pub potable_l: f64,
    /// The potable volume and the fire reserve.
    pub total_l: f64,
    pub refill_hours: f64,
    /// The supply that refills the day's consumption in those hours, L/s.
    pub min_fill_flow_lps: f64,
    pub split: Option<Split>,
    pub suggested_lower_percent: f64,
    pub checks: Vec<Check>,
}

fn check(r: &ReservoirRules, id: &'static str, status: Status, text: impl Into<String>) -> Check {
    Check { id, kind: Kind::Requirement, status, text: text.into(), source: r.standard.clone() }
}

fn verdict(ok: bool) -> Status {
    if ok { Status::Ok } else { Status::Fail }
}

/// Sizes the reservation of `i` by the library's rules.
pub fn size(i: &Input) -> Outcome {
    size_by(i, &crate::codes::library_rules::<ReservoirRules>())
}

/// Sizes the reservation of `i` and checks it by `r`: the volumes, the refill flow and the checks of the standard it can make.
pub fn size_by(i: &Input, r: &ReservoirRules) -> Outcome {
    let potable_l = i.daily_l * i.days;
    let total_l = potable_l + i.fire_l;
    let refill_hours = if i.house { r.refill_hours_house } else { r.refill_hours };
    let min_fill_flow_lps = i.daily_l / (refill_hours * 3600.0);
    let split =
        i.lower_percent.map(|p| Split { lower_percent: p, lower_l: potable_l * p / 100.0, upper_l: potable_l * (100.0 - p) / 100.0 });
    let mut checks = vec![
        check(
            r,
            "autonomy",
            verdict(i.days >= r.min_days - 1e-9),
            format!("{} day(s) of consumption reserved; at least {} (24 h)", i.days, r.min_days),
        ),
        check(
            r,
            "potability",
            verdict(i.days <= r.max_days + 1e-9),
            format!(
                "{} day(s) of potable water; {} at most where the project finds no limit of its own, or it shows how the quality is kept; the fire reserve is not counted",
                i.days, r.max_days
            ),
        ),
    ];
    checks.push(match i.fill_flow_lps {
        Some(q) => check(
            r,
            "refill",
            verdict(q >= min_fill_flow_lps - 1e-9),
            format!("the supply of {q} L/s against {min_fill_flow_lps:.3} L/s that refills {:.0} L in {refill_hours} h", i.daily_l),
        ),
        None => check(
            r,
            "refill",
            Status::Pending,
            format!("`fill_flow` (L/s) not given: the supply has to refill the day's consumption in {refill_hours} h, {min_fill_flow_lps:.3} L/s"),
        ),
    });
    if !i.house {
        checks.push(match i.compartments {
            Some(n) => check(
                r,
                "compartments",
                verdict(n >= r.min_compartments),
                format!("{n} compartment(s) in the elevated reservoir; {} at least, but for a single-family house", r.min_compartments),
            ),
            None => check(
                r,
                "compartments",
                Status::Pending,
                format!("`compartments` not given: an elevated reservoir has {} at least", r.min_compartments),
            ),
        });
        checks.push(match (i.smallest_l, i.peak_l) {
            (Some(s), Some(p)) => check(
                r,
                "smallest_compartment",
                verdict(s >= p - 1e-9),
                format!("the smallest compartment holds {s} L; the greatest peak of consumption over a normal maintenance is {p} L"),
            ),
            _ => check(
                r,
                "smallest_compartment",
                Status::Pending,
                "`smallest_l` and `peak_l` not both given: the smallest compartment holds the greatest peak of consumption over a normal maintenance",
            ),
        });
    }
    checks.push(match i.pumps {
        Some(n) => check(r, "pumps", verdict(n >= r.min_pumps), format!("{n} pump(s) of independent operation; {} at least", r.min_pumps)),
        None => check(r, "pumps", Status::Pending, format!("`pumps` not given: {} independent pumps at least", r.min_pumps)),
    });
    if i.towers >= 2 {
        checks.push(check(
            r,
            "spare_pump",
            Status::Pending,
            format!(
                "{} towers share the station: one pump is the spare of the others, and the one that starts alternates automatically; how many serve at once is the project's to say",
                i.towers
            ),
        ));
    }
    Outcome { potable_l, total_l, refill_hours, min_fill_flow_lps, split, suggested_lower_percent: r.usual_lower_percent, checks }
}

#[cfg(test)]
mod tests;
