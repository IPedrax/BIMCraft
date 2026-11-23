use super::{MAX_COORD, MAX_POINTS, R, Session, Value, bad, choice, element_id, json, num, only};

pub(super) fn carrier_fill(s: &mut Session, p: &Value) -> R {
    use crate::carrier_fill::{ClearSection, Enclosure, Profile};
    const C: &str = "carrier_fill";
    only(C, p, &["id", "profile", "areas", "enclosure", "inside_diameter", "inside_width", "inside_depth"])?;
    let id = element_id(C, &s.model, &p["id"])?;
    let element = s.model.elements.get(id).ok_or_else(|| bad(C, "carrier does not exist"))?;
    let ty = s.model.run_type(element).ok_or_else(|| bad(C, "select a conduit or cable tray"))?;
    let profile = choice(C, p, "profile", &["nbr5410_2004", "ru_sp76_2016_max_fill"], "")?;
    let explicit = ["enclosure", "inside_width", "inside_depth", "inside_diameter"].iter().any(|key| p.get(key).is_some());
    let dimension =
        |key| -> Result<f64, String> { num(C, p, key)?.filter(|n| *n > 0.0).ok_or_else(|| bad(C, format!("give a positive {key}"))) };
    let (profile_rule, inside) = match (profile, element.category(), ty.round) {
        ("nbr5410_2004", "conduit", true) => {
            only(C, p, &["id", "profile", "areas", "inside_diameter"])?;
            let diameter = match (explicit, ty.clear_section) {
                (false, Some(ClearSection::Round { diameter })) => diameter,
                _ => dimension("inside_diameter")?,
            };
            if diameter > ty.width {
                return Err(bad(C, "inside diameter exceeds the carrier's outside diameter"));
            }
            (Profile::Nbr5410, std::f64::consts::PI * diameter * diameter / 4.0)
        }
        ("ru_sp76_2016_max_fill", "cabletray", false) => {
            only(C, p, &["id", "profile", "areas", "enclosure", "inside_width", "inside_depth"])?;
            let (width, depth, enclosure) = match (explicit, ty.clear_section) {
                (false, Some(ClearSection::Rectangular { width, depth, enclosure: Some(enclosure) })) => (width, depth, enclosure),
                _ => {
                    let enclosure = match choice(C, p, "enclosure", &["blind_box", "lidded_box"], "")? {
                        "blind_box" => Enclosure::BlindBox,
                        "lidded_box" => Enclosure::LiddedBox,
                        _ => return Err(bad(C, "enclosure is required")),
                    };
                    (dimension("inside_width")?, dimension("inside_depth")?, enclosure)
                }
            };
            if width > ty.width || depth > ty.depth {
                return Err(bad(C, "inside dimensions exceed the carrier's outside dimensions"));
            }
            (Profile::Sp76(enclosure), width * depth)
        }
        ("nbr5410_2004", _, _) => return Err(bad(C, "NBR 5410 requires a round conduit")),
        ("ru_sp76_2016_max_fill", _, _) => return Err(bad(C, "SP 76 box fill requires a rectangular cable tray type")),
        _ => return Err(bad(C, "profile is required")),
    };
    let areas = p["areas"]
        .as_array()
        .filter(|a| a.len() <= MAX_POINTS)
        .ok_or_else(|| bad(C, format!("give an areas array with at most {MAX_POINTS} entries")))?;
    let occupied = areas.iter().try_fold(0.0, |sum, area| {
        let area = area
            .as_f64()
            .filter(|a| a.is_finite() && *a > 0.0 && *a <= MAX_COORD * MAX_COORD)
            .ok_or_else(|| bad(C, "each cable area must be positive, finite and within the model limits"))?;
        Ok::<_, String>(sum + area)
    })?;
    let count = areas.len();
    let (limit, capacity) = profile_rule.capacity(inside, count).map_err(|e| bad(C, e))?;
    Ok(json!({
        "profile":profile, "id":id, "count":count, "inside_mm2":inside, "occupied_mm2":occupied,
        "limit_percent":limit * 100.0, "capacity_mm2":capacity, "ok":occupied <= capacity * (1.0 + 1e-12)
    }))
}
