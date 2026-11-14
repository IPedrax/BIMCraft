//! IFC2x3 (ISO/PAS 16739:2005, `IFC2X3` TC1), written from what the IFC4 writer makes: `downgrade` turns each entity into the
//! one IFC2x3 has for it. Most differ only by rule, which `table.rs` holds (made by `ci/ifc2x3_table.py` from the two schemas):
//! the class it is written as (an IFC4-only class is its nearest IFC2x3 supertype, so a pipe segment is a flow segment and what
//! it is stays on its type object; an element with none is a proxy), how many attributes that has, the owner history every
//! rooted entity must have there, and the values its enumerations take. The few that differ in kind are written here (a door's
//! or a window's type is a style, a see-through shading is a rendering) or by the writer itself, which asks `Schema::Ifc2x3`:
//! triangles as a faceted B-rep, a material list for a constituent set, a material for a profile set, the map conversion as the
//! project's `ePSet_MapConversion`, a swept disk's ends.

mod table;

/// How an entity of IFC4 is written in IFC2x3 (see `ci/ifc2x3_table.py`).
pub(super) struct Row {
    /// The entity as the writer names it.
    from: &'static str,
    /// The IFC2x3 entity it is written as; empty for one IFC2x3 has nothing for.
    to: &'static str,
    /// How many attributes that has.
    count: usize,
    /// `E` an element, `R` anything else with an owner history, `-` without one.
    kind: char,
    /// Its attributes that are an enumeration or required: where (from 0), whether required, and an enumeration's values.
    attributes: &'static [(usize, bool, &'static str)],
}

fn row(name: &str) -> Option<&'static Row> {
    table::ROWS.binary_search_by(|r| r.from.cmp(name)).ok().and_then(|i| table::ROWS.get(i))
}

/// Whether IFC2x3 has a type object for `class` (an IFC4 `<CLASS>TYPE`): its own, or a door's or a window's style.
pub(super) fn has_type(class: &str) -> bool {
    matches!(class, "IFCDOORTYPE" | "IFCWINDOWTYPE") || row(class).is_none_or(|r| !r.to.is_empty())
}

/// An entity's name and its attributes as written, split at the commas between them.
pub(super) fn split(entity: &str) -> Option<(&str, Vec<&str>)> {
    let open = entity.find('(')?;
    let inner = entity.get(open + 1..)?.strip_suffix(')')?;
    let (mut args, mut depth, mut quoted, mut start) = (Vec::new(), 0usize, false, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                args.push(inner.get(start..i)?);
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(inner.get(start..)?);
    Some((entity.get(..open)?, args))
}

/// Whether an attribute is written as an enumeration's value (`.FLOOR.`), not a boolean's.
fn enumerated(arg: &str) -> bool {
    arg.len() > 2 && arg.starts_with('.') && arg.ends_with('.') && !matches!(arg, ".T." | ".F." | ".U.")
}

/// `entity`, as the IFC4 writer made it, as IFC2x3 has it; `owner` is the file's owner history.
pub(super) fn downgrade(entity: &str, owner: u64) -> String {
    let Some((name, args)) = split(entity) else { return entity.to_string() };
    let mut args: Vec<String> = args.into_iter().map(str::to_string).collect();
    let at = |args: &[String], i: usize| args.get(i).cloned().unwrap_or_else(|| "$".to_string());
    let set = |i: usize| args.get(i).is_some_and(|a| a != "$");
    let name = match name {
        // A door's type is its style: its operation, of no construction said, its sizes the occurrence's own.
        "IFCDOORTYPE" => {
            let operation = at(&args, 10);
            args.truncate(8);
            args.extend([operation, ".NOTDEFINED.".into(), ".F.".into(), ".F.".into()]);
            "IFCDOORSTYLE"
        }
        // A window's too: its panels are its operation there.
        "IFCWINDOWTYPE" => {
            let panels = at(&args, 10);
            args.truncate(8);
            args.extend([".NOTDEFINED.".into(), panels, ".F.".into(), ".F.".into()]);
            "IFCWINDOWSTYLE"
        }
        // A bounded value has no set point in IFC2x3: one that is only that is a single value.
        "IFCPROPERTYBOUNDEDVALUE" if set(5) && !set(2) && !set(3) => {
            args = vec![at(&args, 0), at(&args, 1), at(&args, 5), at(&args, 4)];
            "IFCPROPERTYSINGLEVALUE"
        }
        // How see-through a surface is belongs to a rendering there.
        "IFCSURFACESTYLESHADING" if set(1) => {
            args.resize(8, "$".to_string());
            args.push(".NOTDEFINED.".into());
            "IFCSURFACESTYLERENDERING"
        }
        other => other,
    };
    by_rule(name, args, owner)
}

/// `name` with `args` by its row of the table: its class, its attributes cut or filled to those IFC2x3 has, its owner history,
/// and each enumeration a value it takes there (none defined, or unset, for one it doesn't).
fn by_rule(name: &str, mut args: Vec<String>, owner: u64) -> String {
    let written = |name: &str, args: &[String]| format!("{name}({})", args.join(","));
    let Some(row) = row(name).filter(|r| !r.to.is_empty()) else { return written(name, &args) };
    args.resize(row.count, "$".to_string());
    let rooted = row.kind != '-';
    let mut lacking = false;
    for (i, arg) in args.iter_mut().enumerate() {
        if rooted && i == 1 {
            *arg = format!("#{owner}");
        }
        if rooted && i < 2 {
            continue;
        }
        match row.attributes.iter().find(|a| a.0 == i) {
            Some((_, required, values)) if !values.is_empty() => {
                let taken = enumerated(arg) && values.split(' ').any(|v| arg.get(1..arg.len() - 1) == Some(v));
                if !taken {
                    *arg = match (required, values.split(' ').find(|v| *v == "NOTDEFINED").or_else(|| values.split(' ').next())) {
                        (true, Some(v)) => format!(".{v}."),
                        _ => "$".to_string(),
                    };
                }
            }
            // Required, and not an enumeration: an enumeration's value there is IFC4's attribute, another one.
            Some(_) => {
                if enumerated(arg) {
                    *arg = "$".to_string();
                }
                lacking |= arg == "$";
            }
            None if enumerated(arg) => *arg = "$".to_string(),
            None => {}
        }
    }
    // An element short of something IFC2x3 requires of its class (a tendon's diameter) is a proxy, which requires nothing.
    if lacking && row.kind == 'E' && row.to != "IFCBUILDINGELEMENTPROXY" {
        args.truncate(8);
        return by_rule("IFCBUILDINGELEMENTPROXY", args, owner);
    }
    written(row.to, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_in_order_for_its_search() {
        assert!(table::ROWS.windows(2).all(|w| w[0].from < w[1].from));
        assert!(table::ROWS.iter().all(|r| r.attributes.iter().all(|a| a.0 < r.count)));
    }

    #[test]
    fn attributes_split_at_their_own_commas_only() {
        let (name, args) = split("IFCX('a, (b''',(#1,#2),IFCTEXT('c,d'),$)").unwrap();
        assert_eq!(name, "IFCX");
        assert_eq!(args, ["'a, (b'''", "(#1,#2)", "IFCTEXT('c,d')", "$"]);
        assert!(split("IFCX").is_none());
    }

    #[test]
    fn an_entity_takes_its_ifc2x3_class_attributes_and_owner_history() {
        // A wall loses its predefined type; a pipe segment is a flow segment; a chimney, a proxy.
        assert_eq!(downgrade("IFCWALL('g',$,'W',$,'T',#1,#2,'7',.STANDARD.)", 9), "IFCWALL('g',#9,'W',$,'T',#1,#2,'7')");
        assert_eq!(downgrade("IFCPIPESEGMENT('g',$,'P',$,$,#1,#2,'7',.RIGIDSEGMENT.)", 9), "IFCFLOWSEGMENT('g',#9,'P',$,$,#1,#2,'7')");
        assert_eq!(downgrade("IFCCHIMNEY('g',$,'C',$,$,#1,#2,'7',.NOTDEFINED.)", 9), "IFCBUILDINGELEMENTPROXY('g',#9,'C',$,$,#1,#2,'7',$)");
        // What has no owner history, and what is the same in both, stays as it is.
        assert_eq!(downgrade("IFCMATERIAL('Oak',$,$)", 9), "IFCMATERIAL('Oak')");
        assert_eq!(downgrade("IFCCARTESIANPOINT((0.,0.))", 9), "IFCCARTESIANPOINT((0.,0.))");
        assert_eq!(downgrade("IFCQUANTITYAREA('NetArea',$,$,2.,$)", 9), "IFCQUANTITYAREA('NetArea',$,$,2.)");
    }

    #[test]
    fn enumerations_take_values_ifc2x3_has() {
        // A room is neither said inside nor outside; a wall type IFC4 added is none defined; a slab's kind stays.
        assert_eq!(
            downgrade("IFCSPACE('g',$,'1',$,$,#1,#2,'Hall',.ELEMENT.,.SPACE.,$)", 9),
            "IFCSPACE('g',#9,'1',$,$,#1,#2,'Hall',.ELEMENT.,.NOTDEFINED.,$)"
        );
        assert_eq!(downgrade("IFCWALLTYPE('g',$,'W',$,$,$,$,$,$,.PARTITIONING.)", 9), "IFCWALLTYPE('g',#9,'W',$,$,$,$,$,$,.NOTDEFINED.)");
        assert_eq!(downgrade("IFCSLAB('g',$,'S',$,$,#1,#2,'7',.FLOOR.)", 9), "IFCSLAB('g',#9,'S',$,$,#1,#2,'7',.FLOOR.)");
    }

    #[test]
    fn door_and_window_types_are_styles_and_a_roofs_has_none() {
        assert_eq!(
            downgrade("IFCDOORTYPE('g',$,'D',$,$,$,$,$,$,.DOOR.,.SINGLE_SWING_LEFT.,.T.,$)", 9),
            "IFCDOORSTYLE('g',#9,'D',$,$,$,$,$,.SINGLE_SWING_LEFT.,.NOTDEFINED.,.F.,.F.)"
        );
        assert_eq!(
            downgrade("IFCWINDOWTYPE('g',$,'W',$,$,$,$,$,$,.WINDOW.,.SINGLE_PANEL.,.T.,$)", 9),
            "IFCWINDOWSTYLE('g',#9,'W',$,$,$,$,$,.NOTDEFINED.,.SINGLE_PANEL.,.F.,.F.)"
        );
        assert!(has_type("IFCDOORTYPE") && has_type("IFCWALLTYPE") && has_type("IFCPIPESEGMENTTYPE"));
        assert!(!has_type("IFCROOFTYPE") && !has_type("IFCFOOTINGTYPE"));
    }

    #[test]
    fn a_set_point_is_a_single_value_and_a_clear_shading_a_rendering() {
        assert_eq!(
            downgrade("IFCPROPERTYBOUNDEDVALUE('RatedVoltage',$,$,$,$,IFCELECTRICVOLTAGEMEASURE(230.))", 9),
            "IFCPROPERTYSINGLEVALUE('RatedVoltage',$,IFCELECTRICVOLTAGEMEASURE(230.),$)"
        );
        assert_eq!(downgrade("IFCSURFACESTYLESHADING(#4,0.5)", 9), "IFCSURFACESTYLERENDERING(#4,0.5,$,$,$,$,$,$,.NOTDEFINED.)");
        assert_eq!(downgrade("IFCSURFACESTYLESHADING(#4,$)", 9), "IFCSURFACESTYLESHADING(#4)");
    }

    #[test]
    fn an_element_short_of_what_its_class_requires_is_a_proxy() {
        let tendon = "IFCTENDON('g',$,'T',$,$,#1,#2,'7',$,.NOTDEFINED.,$,$,$,$,$,$,$)";
        assert_eq!(downgrade(tendon, 9), "IFCBUILDINGELEMENTPROXY('g',#9,'T',$,$,#1,#2,'7',$)");
    }
}
