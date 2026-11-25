//! Maximum fill of a measured cable carrier.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enclosure {
    BlindBox,
    LiddedBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ClearSection {
    Round {
        diameter: f64,
    },
    Rectangular {
        width: f64,
        depth: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enclosure: Option<Enclosure>,
    },
}

impl ClearSection {
    pub(crate) fn check(self, who: &str, category: &str, ty: &crate::model::RunType) -> Result<(), String> {
        match (self, category, ty.round) {
            (Self::Round { diameter }, "conduit", true) if diameter > 0.0 && diameter <= ty.width => Ok(()),
            (Self::Rectangular { width, depth, .. }, "cabletray", false)
                if width > 0.0 && width <= ty.width && depth > 0.0 && depth <= ty.depth =>
            {
                Ok(())
            }
            (Self::Round { .. }, "conduit", true) | (Self::Rectangular { .. }, "cabletray", false) => {
                Err(format!("{who}: clear dimensions must be positive and fit inside the outside section"))
            }
            _ => Err(format!("{who}: a clear diameter needs a round conduit and a clear width and depth need a rectangular cable tray")),
        }
    }
}

pub(crate) enum Profile {
    Nbr5410,
    Sp76(Enclosure),
}

impl Profile {
    pub(crate) fn capacity(self, clear_area: f64, count: usize) -> Result<(f64, f64), String> {
        if !clear_area.is_finite() || clear_area <= 0.0 {
            return Err("carrier clear area must be finite and positive".into());
        }
        let limit = match self {
            Self::Nbr5410 => {
                use crate::codes::{Discipline, find_edition};
                use crate::conduit::Rules;
                use std::sync::OnceLock;
                static RULES: OnceLock<Option<Rules>> = OnceLock::new();
                RULES
                    .get_or_init(|| {
                        let section = find_edition(Discipline::Electrical, "nbr-5410", "2004")?.section("conduit_fill")?;
                        serde_json::from_value(serde_json::Value::Object(section.clone())).ok()
                    })
                    .as_ref()
                    .ok_or("NBR 5410:2004 conduit fill rules are unavailable")?
                    .limit(count)
            }
            Self::Sp76(Enclosure::BlindBox) => 0.35,
            Self::Sp76(Enclosure::LiddedBox) => 0.40,
        };
        Ok((limit, clear_area * limit))
    }
}
