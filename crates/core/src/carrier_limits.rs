//! Project-defined cross-section capacity and measured product metadata.
//! National standards profiles are added separately from physical carrier geometry.

use serde::{Deserialize, Serialize};

use crate::model::MAX_COORD;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Project,
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
pub fn capacity(_profile: Profile, project_percent: f64, gross_area: f64, _section: Option<&Section>) -> Result<f64, String> {
    if !project_percent.is_finite() || project_percent <= 0.0 || project_percent > 100.0 {
        return Err("carrier project percentage must be greater than 0 and at most 100".into());
    }
    if !gross_area.is_finite() || gross_area <= 0.0 || gross_area > MAX_COORD * MAX_COORD {
        return Err("carrier gross section must be finite, positive and within the model limits".into());
    }
    Ok(gross_area * project_percent / 100.0)
}
