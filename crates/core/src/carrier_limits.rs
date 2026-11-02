//! Maximum cross-section only, not a complete electrical/fire-safety compliance check.
//! SP 76.13330.2016, 6.3.2.4: box limits use the clear internal section, including cable sheaths.
//! Source and scope: docs/areas/carrier-norms.md (original 2016 text).

use serde::{Deserialize, Serialize};

use crate::model::MAX_COORD;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Project,
    #[serde(rename = "ru_sp76_2016_max_fill")]
    RuSp76_2016MaxFill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enclosure {
    BlindBox,
    LiddedBox,
    Tray,
    Ladder,
    Conduit,
}

impl Enclosure {
    pub fn key(self) -> &'static str {
        match self {
            Self::BlindBox => "blind_box",
            Self::LiddedBox => "lidded_box",
            Self::Tray => "tray",
            Self::Ladder => "ladder",
            Self::Conduit => "conduit",
        }
    }
}

/// Measured product data, in millimetres. Missing data remains unknown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosure: Option<Enclosure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_depth: Option<f64>,
}

impl Section {
    /// Validate known measurements; missing values remain unknown.
    pub fn check(&self) -> Result<(), String> {
        for (name, value) in [("clear_width", self.clear_width), ("clear_depth", self.clear_depth)] {
            if value.is_some_and(|v| !v.is_finite() || v <= 0.0 || v > MAX_COORD) {
                return Err(format!("carrier {name} must be finite, positive and at most {MAX_COORD} mm"));
            }
        }
        Ok(())
    }
}

/// Available cable section in mm2. Unknown or unsupported normative inputs are errors, never gross-area fallbacks.
pub fn capacity(profile: Profile, project_percent: f64, gross_area: f64, section: Option<&Section>) -> Result<f64, String> {
    if !project_percent.is_finite() || project_percent <= 0.0 || project_percent > 100.0 {
        return Err("carrier project percentage must be greater than 0 and at most 100".into());
    }
    if !gross_area.is_finite() || gross_area <= 0.0 || gross_area > MAX_COORD * MAX_COORD {
        return Err("carrier gross section must be finite, positive and within the model limits".into());
    }
    if profile == Profile::Project {
        return Ok(gross_area * project_percent / 100.0);
    }
    let section = section.ok_or("carrier internal section data is missing")?;
    section.check()?;
    let percent = match section.enclosure.ok_or("carrier enclosure kind is missing")? {
        Enclosure::BlindBox => 35.0,
        Enclosure::LiddedBox => 40.0,
        _ => return Err("SP 76.13330.2016 6.3.2.4 box maximum is not applicable to this enclosure".into()),
    };
    let width = section.clear_width.ok_or("carrier clear_width is missing")?;
    let depth = section.clear_depth.ok_or("carrier clear_depth is missing")?;
    let area = width * depth;
    if area > gross_area {
        return Err("carrier internal section exceeds its gross section; correct the measured dimensions".into());
    }
    Ok(area * percent / 100.0)
}
