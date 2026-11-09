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

/// The least reservation, in days of the building's normal consumption (24 h).
pub const MIN_DAYS: f64 = 1.0;
/// The potable volume the standard recommends not to pass where the project finds no limit of its own, in days of consumption.
pub const MAX_DAYS: f64 = 3.0;
/// Hours the supply has to refill the day's consumption.
pub const REFILL_HOURS: f64 = 6.0;
/// Hours it has for a single-family house.
pub const REFILL_HOURS_HOUSE: f64 = 3.0;
/// The fewest independent pumps of a pump station.
pub const MIN_PUMPS: u32 = 2;
/// The fewest compartments of an elevated reservoir that is not a single-family house's.
pub const MIN_COMPARTMENTS: u32 = 2;
/// The share of the potable volume the lower reservoir usually takes, per cent: an engineering practice, never the standard's.
pub const USUAL_LOWER_PERCENT: f64 = 60.0;

const STANDARD: &str = "NBR 5626:2020, reservation, as read from its text";

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
    pub source: &'static str,
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

fn check(id: &'static str, status: Status, text: impl Into<String>) -> Check {
    Check { id, kind: Kind::Requirement, status, text: text.into(), source: STANDARD }
}

fn verdict(ok: bool) -> Status {
    if ok { Status::Ok } else { Status::Fail }
}

/// Sizes the reservation of `i` and checks it: the volumes, the refill flow and the checks of the standard it can make.
pub fn size(i: &Input) -> Outcome {
    let potable_l = i.daily_l * i.days;
    let total_l = potable_l + i.fire_l;
    let refill_hours = if i.house { REFILL_HOURS_HOUSE } else { REFILL_HOURS };
    let min_fill_flow_lps = i.daily_l / (refill_hours * 3600.0);
    let split =
        i.lower_percent.map(|p| Split { lower_percent: p, lower_l: potable_l * p / 100.0, upper_l: potable_l * (100.0 - p) / 100.0 });
    let mut checks = vec![
        check(
            "autonomy",
            verdict(i.days >= MIN_DAYS - 1e-9),
            format!("{} day(s) of consumption reserved; at least {MIN_DAYS} (24 h)", i.days),
        ),
        check(
            "potability",
            verdict(i.days <= MAX_DAYS + 1e-9),
            format!(
                "{} day(s) of potable water; {MAX_DAYS} at most where the project finds no limit of its own, or it shows how the quality is kept; the fire reserve is not counted",
                i.days
            ),
        ),
    ];
    checks.push(match i.fill_flow_lps {
        Some(q) => check(
            "refill",
            verdict(q >= min_fill_flow_lps - 1e-9),
            format!("the supply of {q} L/s against {min_fill_flow_lps:.3} L/s that refills {:.0} L in {refill_hours} h", i.daily_l),
        ),
        None => check(
            "refill",
            Status::Pending,
            format!("`fill_flow` (L/s) not given: the supply has to refill the day's consumption in {refill_hours} h, {min_fill_flow_lps:.3} L/s"),
        ),
    });
    if !i.house {
        checks.push(match i.compartments {
            Some(n) => check(
                "compartments",
                verdict(n >= MIN_COMPARTMENTS),
                format!("{n} compartment(s) in the elevated reservoir; {MIN_COMPARTMENTS} at least, but for a single-family house"),
            ),
            None => check(
                "compartments",
                Status::Pending,
                format!("`compartments` not given: an elevated reservoir has {MIN_COMPARTMENTS} at least"),
            ),
        });
        checks.push(match (i.smallest_l, i.peak_l) {
            (Some(s), Some(p)) => check(
                "smallest_compartment",
                verdict(s >= p - 1e-9),
                format!("the smallest compartment holds {s} L; the greatest peak of consumption over a normal maintenance is {p} L"),
            ),
            _ => check(
                "smallest_compartment",
                Status::Pending,
                "`smallest_l` and `peak_l` not both given: the smallest compartment holds the greatest peak of consumption over a normal maintenance",
            ),
        });
    }
    checks.push(match i.pumps {
        Some(n) => check("pumps", verdict(n >= MIN_PUMPS), format!("{n} pump(s) of independent operation; {MIN_PUMPS} at least")),
        None => check("pumps", Status::Pending, format!("`pumps` not given: {MIN_PUMPS} independent pumps at least")),
    });
    if i.towers >= 2 {
        checks.push(check(
            "spare_pump",
            Status::Pending,
            format!(
                "{} towers share the station: one pump is the spare of the others, and the one that starts alternates automatically; how many serve at once is the project's to say",
                i.towers
            ),
        ));
    }
    Outcome { potable_l, total_l, refill_hours, min_fill_flow_lps, split, suggested_lower_percent: USUAL_LOWER_PERCENT, checks }
}

#[cfg(test)]
mod tests;
