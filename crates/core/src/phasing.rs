//! Phases and design options. A project has phases in order (Existing, New Construction): each
//! element is made in one (its `Phase Created`, the first unless it says) and may be taken down in
//! a later one (`Phase Demolished`); a view has a phase, and a phase filter that says how each of
//! the four things an element can be in that phase (new, existing, demolished, temporary: made and
//! taken down within it) shows: as its category does, as the project's overrides for it say, or not
//! at all. A project may also have sets of design options: an element in one option of a set shows
//! only in a view that shows that option (a view's own choice, else the set's primary). Both are
//! values elements hold among their parameters, under names no project parameter may take, so
//! copies, `modify`, schedules and view filters reach them as they reach any other.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graphics::{Dash, Graphics, Override, Style};
use crate::language::Language;
use crate::model::{Element, Id, MAX_TEXT, Model};

/// The parameters an element holds its phase and its design option in.
pub const CREATED: &str = "Phase Created";
pub const DEMOLISHED: &str = "Phase Demolished";
pub const OPTION: &str = "Design Option";

pub const MAX_PHASES: usize = 64;
pub const MAX_PHASE_FILTERS: usize = 64;
pub const MAX_OPTION_SETS: usize = 32;
pub const MAX_OPTIONS: usize = 16;

/// Whether `name` is one of the parameters phases and design options live in.
pub fn reserved(name: &str) -> bool {
    matches!(name, CREATED | DEMOLISHED | OPTION)
}

/// What an element is in a view's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Made in it, and still standing at its end.
    New,
    /// Made before it, and still standing at its end.
    Existing,
    /// Made before it, and taken down in it.
    Demolished,
    /// Made and taken down in it.
    Temporary,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::New, Status::Existing, Status::Demolished, Status::Temporary];

    pub fn name(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Existing => "existing",
            Status::Demolished => "demolished",
            Status::Temporary => "temporary",
        }
    }
}

/// How a phase filter shows what is in a status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presentation {
    /// As its category's overrides, and the view's, say.
    #[default]
    ByCategory,
    /// As the project's overrides for the status say.
    Overridden,
    NotDisplayed,
}

/// A phase filter: how each status shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhaseFilter {
    pub new: Presentation,
    pub existing: Presentation,
    pub demolished: Presentation,
    pub temporary: Presentation,
}

impl PhaseFilter {
    pub fn of(&self, status: Status) -> Presentation {
        match status {
            Status::New => self.new,
            Status::Existing => self.existing,
            Status::Demolished => self.demolished,
            Status::Temporary => self.temporary,
        }
    }

    pub fn set(&mut self, status: Status, p: Presentation) {
        match status {
            Status::New => self.new = p,
            Status::Existing => self.existing = p,
            Status::Demolished => self.demolished = p,
            Status::Temporary => self.temporary = p,
        }
    }
}

/// A set of design options: the options, and which is the primary, the one a view shows unless it
/// chooses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSet {
    pub options: Vec<String>,
    pub primary: String,
}

/// A project's phases to start from, in `language`.
pub fn phases(language: Language) -> Vec<String> {
    ["Existing", "New Construction"].map(|w| language.word(w).to_string()).to_vec()
}

pub fn default_phases() -> Vec<String> {
    phases(Language::English)
}

/// The phase filters a project starts with, in `language`.
pub fn filters(language: Language) -> BTreeMap<String, PhaseFilter> {
    use Presentation::{ByCategory as B, NotDisplayed as N, Overridden as O};
    let f = |new, existing, demolished, temporary| PhaseFilter { new, existing, demolished, temporary };
    [
        ("Show All", f(B, B, B, B)),
        ("Show Complete", f(B, B, N, B)),
        ("Show New", f(B, N, N, N)),
        ("Show Previous + Demolished", f(N, B, O, N)),
        ("Show Previous + New", f(B, O, N, B)),
    ]
    .into_iter()
    .map(|(n, f)| (language.word(n).to_string(), f))
    .collect()
}

pub fn default_filters() -> BTreeMap<String, PhaseFilter> {
    filters(Language::English)
}

/// What each status is drawn as when a phase filter says so: what stood before in halftone, what
/// comes down dashed and red.
pub fn overrides() -> BTreeMap<Status, Override> {
    let red = Some([200, 40, 30]);
    let dashed = Style { color: red, dash: Some(Dash::Dashed), ..Style::default() };
    [
        (Status::Existing, Override { halftone: true, ..Override::default() }),
        (Status::Demolished, Override { projection: dashed.clone(), cut: dashed, ..Override::default() }),
    ]
    .into_iter()
    .collect()
}

pub fn default_overrides() -> BTreeMap<Status, Override> {
    overrides()
}

/// Whether a project's phases, phase filters and overrides are the ones a new English project has:
/// then its file leaves them out.
pub fn is_default_phases(p: &Vec<String>) -> bool {
    *p == default_phases()
}

pub fn is_default_filters(f: &BTreeMap<String, PhaseFilter>) -> bool {
    *f == default_filters()
}

pub fn is_default_overrides(o: &BTreeMap<Status, Override>) -> bool {
    *o == default_overrides()
}

/// Where phase `name` stands in the project's order.
pub fn index(m: &Model, name: &str) -> Option<usize> {
    m.phases.iter().position(|p| p == name)
}

/// The value of built-in parameter `key` an element has: its own, or a door's or window's none, its
/// wall's.
pub fn value<'a>(m: &'a Model, e: &'a Element, key: &str) -> Option<&'a str> {
    let own = e.params().and_then(|p| p.get(key)).and_then(Value::as_str);
    own.or_else(|| match e {
        Element::Door(o) | Element::Window(o) => m.wall(o.host)?.params.get(key)?.as_str(),
        _ => None,
    })
}

/// What element `e` is in the phase at `at` in the project's order; none when it isn't there: made
/// after it, or taken down before it. Only what holds parameters is phased: annotation is a view's.
pub fn status(m: &Model, e: &Element, at: usize) -> Option<Status> {
    e.params()?;
    let created = value(m, e, CREATED).and_then(|n| index(m, n)).unwrap_or(0);
    let demolished = value(m, e, DEMOLISHED).and_then(|n| index(m, n));
    if created > at {
        return None;
    }
    match demolished {
        Some(d) if d < at => None,
        Some(d) if d == at => Some(if created == at { Status::Temporary } else { Status::Demolished }),
        _ => Some(if created == at { Status::New } else { Status::Existing }),
    }
}

/// Whether anything in `m` is phased or in a design option.
pub fn any(m: &Model) -> bool {
    m.elements.values().any(|e| e.params().is_some_and(|p| p.keys().any(|k| reserved(k))))
}

/// Whether a view that shows the options `g` chooses shows element `e`: one in no option shows
/// everywhere; one in an option, where its set's chosen option, else its primary, is that one.
pub fn option_shown(m: &Model, g: &Graphics, e: &Element) -> bool {
    let Some((set, option)) = value(m, e, OPTION).and_then(|v| v.split_once(": ")) else { return true };
    let Some(s) = m.option_sets.get(set) else { return true };
    g.options.get(set).unwrap_or(&s.primary) == option
}

/// What a project's names of phases, phase filters and option sets and their options must be.
fn named(who: &str, n: &str) -> Result<(), String> {
    if n.trim().is_empty() || n.trim() != n || n.chars().count() > MAX_TEXT {
        return Err(format!(
            "{who} `{n}`: a name is 1 to {MAX_TEXT} characters, with nothing before or after it but letters, digits and the like"
        ));
    }
    Ok(())
}

/// Whether the project's phases, phase filters, overrides and option sets are sound.
pub(crate) fn check_project(m: &Model) -> Result<(), String> {
    if m.phases.is_empty() || m.phases.len() > MAX_PHASES {
        return Err(format!("a project has 1 to {MAX_PHASES} phases, not {}", m.phases.len()));
    }
    let mut seen = BTreeSet::new();
    for p in &m.phases {
        named("phase", p)?;
        if !seen.insert(p.as_str()) {
            return Err(format!("phase `{p}` is in the project twice"));
        }
    }
    if m.phase_filters.len() > MAX_PHASE_FILTERS {
        return Err(format!("the project has {} phase filters; it holds up to {MAX_PHASE_FILTERS}", m.phase_filters.len()));
    }
    for n in m.phase_filters.keys() {
        named("phase filter", n)?;
    }
    for (s, o) in &m.phase_overrides {
        o.check(&format!("the {} phase's overrides", s.name()))?;
        if o.hidden || o.transparency > 0 {
            return Err(format!("the {} phase's overrides: a phase filter hides what it doesn't display; overrides draw", s.name()));
        }
    }
    if m.option_sets.len() > MAX_OPTION_SETS {
        return Err(format!("the project has {} option sets; it holds up to {MAX_OPTION_SETS}", m.option_sets.len()));
    }
    for (n, s) in &m.option_sets {
        named("design option set", n)?;
        if n.contains(':') {
            return Err(format!("design option set `{n}`: a name has no colon in it"));
        }
        if s.options.is_empty() || s.options.len() > MAX_OPTIONS {
            return Err(format!("design option set `{n}` has 1 to {MAX_OPTIONS} options, not {}", s.options.len()));
        }
        let mut seen = BTreeSet::new();
        for o in &s.options {
            named(&format!("design option set `{n}`: option"), o)?;
            if o.contains(':') {
                return Err(format!("design option set `{n}`: option `{o}`: a name has no colon in it"));
            }
            if !seen.insert(o.as_str()) {
                return Err(format!("design option set `{n}` has option `{o}` twice"));
            }
        }
        if !s.options.contains(&s.primary) {
            return Err(format!("design option set `{n}`: its primary `{}` isn't one of its options", s.primary));
        }
    }
    Ok(())
}

/// Whether the project has design option `value` (`set: option`).
pub fn option_exists(m: &Model, value: &str) -> bool {
    value.split_once(": ").is_some_and(|(s, o)| m.option_sets.get(s).is_some_and(|set| set.options.iter().any(|x| x == o)))
}

/// Whether `v`, the value of built-in parameter `key`, is one the project has; none for another key.
pub(crate) fn check_value(m: &Model, who: &str, key: &str, v: &Value) -> Option<Result<(), String>> {
    let text = v.as_str();
    Some(match key {
        CREATED | DEMOLISHED => match text {
            Some(n) if index(m, n).is_some() => Ok(()),
            _ => Err(format!("{who}: `{key}` is one of the project's phases ({}), not {v}", m.phases.join(", "))),
        },
        OPTION => match text.and_then(|t| t.split_once(": ")) {
            Some((s, o)) if m.option_sets.get(s).is_some_and(|set| set.options.iter().any(|x| x == o)) => Ok(()),
            _ => Err(format!("{who}: `{key}` is `set: option`, one the project has, not {v}")),
        },
        _ => return None,
    })
}

/// Whether the elements' phases are in order: nothing taken down before it was made.
pub(crate) fn check_order(m: &Model, who: &str, e: &Element) -> Result<(), String> {
    let created = value(m, e, CREATED).and_then(|n| index(m, n)).unwrap_or(0);
    if let Some(d) = value(m, e, DEMOLISHED).and_then(|n| index(m, n))
        && d < created
    {
        return Err(format!("{who}: it can't be demolished in a phase before the one it was made in"));
    }
    Ok(())
}

/// Every element holding built-in parameter `key` with value `value`.
pub(crate) fn holders(m: &Model, key: &str, value: &str) -> Vec<Id> {
    m.elements
        .iter()
        .filter(|(_, e)| e.params().and_then(|p| p.get(key)).and_then(Value::as_str) == Some(value))
        .map(|(id, _)| *id)
        .collect()
}

/// Name the first phase in what was made in it, which holds no value: before another comes first, or it goes.
pub(crate) fn pin_first(m: &mut Model) {
    let Some(first) = m.phases.first().cloned() else { return };
    // A door or window follows its wall, which is named here too.
    let loose: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| !matches!(e, Element::Door(_) | Element::Window(_)) && e.params().is_some() && value(m, e, CREATED).is_none())
        .map(|(id, _)| *id)
        .collect();
    for id in loose {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(CREATED.into(), Value::String(first.clone()));
        }
    }
}

/// Whether anything holds phase `name`: an element made or taken down in it (the first phase holds what has no value),
/// a view or a template showing it, or the session.
pub(crate) fn holds_phase(m: &Model, session: Option<&str>, name: &str) -> bool {
    session == Some(name)
        || !holders(m, CREATED, name).is_empty()
        || !holders(m, DEMOLISHED, name).is_empty()
        || (m.phases.first().map(String::as_str) == Some(name)
            && m.elements.values().any(|e| e.params().is_some() && value(m, e, CREATED).is_none()))
        || m.elements.values().any(|e| crate::graphics::graphics_of_element(e).is_some_and(|g| g.phase.as_deref() == Some(name)))
        || m.view_templates.values().any(|t| t.graphics.phase.as_deref() == Some(name))
}

/// What was made, taken down and shown in phase `from` is now `to`'s (phase `from` is already out of the list or renamed):
/// the elements, the views and the templates. Nothing is taken down before it was made.
pub(crate) fn move_phase(m: &mut Model, from: &str, to: Option<&str>) {
    let Some(to) = to else { return };
    for key in [CREATED, DEMOLISHED] {
        for id in holders(m, key, from) {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.insert(key.into(), Value::String(to.into()));
            }
        }
    }
    for g in m.elements.values_mut().filter_map(crate::graphics::graphics_mut) {
        if g.phase.as_deref() == Some(from) {
            g.phase = Some(to.into());
        }
    }
    for t in m.view_templates.values_mut() {
        if t.graphics.phase.as_deref() == Some(from) {
            t.graphics.phase = Some(to.into());
        }
    }
    rename_in_rules(m, CREATED, from, to);
    rename_in_rules(m, DEMOLISHED, from, to);
    // Taken down where they were made, at the earliest: where a door or window is made is where its wall is.
    let at = |n: &str| m.phases.iter().position(|p| p == n);
    let early: Vec<(Id, String)> = m
        .elements
        .iter()
        .filter_map(|(id, e)| {
            let down = e.params()?.get(DEMOLISHED)?.as_str().and_then(at)?;
            let made = value(m, e, CREATED).and_then(at).unwrap_or(0);
            (down < made).then(|| (*id, m.phases.get(made).cloned().unwrap_or_default()))
        })
        .collect();
    for (id, name) in early {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(DEMOLISHED.into(), Value::String(name));
        }
    }
}

/// The rules of view filters and schedules that say field `field` is `old` say it is `new`.
fn rename_in_rules(m: &mut Model, field: &str, old: &str, new: &str) {
    let swap = |f: &mut crate::schedule::Filter| {
        if f.field == field && f.value.as_str() == Some(old) {
            f.value = Value::String(new.into());
        }
    };
    m.view_filters.values_mut().flat_map(|v| v.rules.iter_mut()).for_each(swap);
    for e in m.elements.values_mut() {
        if let Element::Schedule(sc) = e {
            sc.filters.iter_mut().for_each(swap);
        }
    }
}

/// Option `option` of set `set` is gone: no view or template chooses it.
pub(crate) fn forget_option(m: &mut Model, set: &str, option: &str) {
    let forget = |g: &mut Graphics| {
        if g.options.get(set).is_some_and(|o| o == option) {
            g.options.remove(set);
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(forget);
    m.view_templates.values_mut().for_each(|t| forget(&mut t.graphics));
}

/// Option `old` of set `set` is `new` now: in the set, in the elements in it and in the views that choose it.
pub(crate) fn rename_option(m: &mut Model, set: &str, old: &str, new: &str) {
    m.library.renamed(&crate::library::Kind::DesignOption, &format!("{set}: {old}"), &format!("{set}: {new}"));
    if let Some(s) = m.option_sets.get_mut(set) {
        for o in s.options.iter_mut().filter(|o| *o == old) {
            *o = new.into();
        }
        if s.primary == old {
            s.primary = new.into();
        }
    }
    for id in holders(m, OPTION, &format!("{set}: {old}")) {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(OPTION.into(), Value::String(format!("{set}: {new}")));
        }
    }
    rename_in_rules(m, OPTION, &format!("{set}: {old}"), &format!("{set}: {new}"));
    let choose = |g: &mut Graphics| {
        if let Some(o) = g.options.get_mut(set).filter(|o| *o == old) {
            *o = new.into();
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(choose);
    m.view_templates.values_mut().for_each(|t| choose(&mut t.graphics));
}

/// Set `old` is `new` now: its options, the elements in them and the views that choose among them.
pub(crate) fn rename_set(m: &mut Model, old: &str, new: &str) {
    m.library.set_renamed(old, new);
    let Some(s) = m.option_sets.remove(old) else { return };
    for o in &s.options {
        for id in holders(m, OPTION, &format!("{old}: {o}")) {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.insert(OPTION.into(), Value::String(format!("{new}: {o}")));
            }
        }
    }
    for o in &s.options {
        rename_in_rules(m, OPTION, &format!("{old}: {o}"), &format!("{new}: {o}"));
    }
    m.option_sets.insert(new.into(), s);
    let choose = |g: &mut Graphics| {
        if let Some(o) = g.options.remove(old) {
            g.options.insert(new.into(), o);
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(choose);
    m.view_templates.values_mut().for_each(|t| choose(&mut t.graphics));
}

/// Set `set` is gone: no view chooses among its options.
pub(crate) fn forget_set(m: &mut Model, set: &str) {
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(|g| {
        g.options.remove(set);
    });
    m.view_templates.values_mut().for_each(|t| {
        t.graphics.options.remove(set);
    });
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::geom::P2;
    use crate::graphics::testing::{PAPER, drawn_near, house, lines, owned, plan, run};
    use crate::model::Element;
    use crate::render::Rgb;

    /// The parameters an element holds, null when none.
    fn params(s: &Session, id: u64) -> Value {
        s.model.elements.get(id).and_then(Element::params).filter(|p| !p.is_empty()).map_or(Value::Null, |p| json!(p))
    }

    /// The walls of `house`: the first existing, the second new, the third demolished (made existing,
    /// taken down in the new phase), the fourth temporary (made and taken down in the new phase).
    fn phased_house(s: &mut Session) -> Vec<u64> {
        let walls = house(s);
        run(s, "modify", json!({"id": walls[1], "params": {"Phase Created": "New Construction"}}));
        run(s, "modify", json!({"id": walls[2], "params": {"Phase Demolished": "New Construction"}}));
        run(s, "modify", json!({"id": walls[3], "params": {"Phase Created": "New Construction", "Phase Demolished": "New Construction"}}));
        walls
    }

    /// Which of `walls` the plan of Level 1 draws.
    fn shown(s: &Session, walls: &[u64]) -> Vec<bool> {
        let d = plan(s, 100000);
        walls.iter().map(|w| !owned(&d, *w).is_empty()).collect()
    }

    #[test]
    fn a_project_starts_with_two_phases_and_the_phase_filters_that_show_them_in_its_language() {
        let s = Session::new();
        assert_eq!(s.model.phases, ["Existing", "New Construction"]);
        let names: Vec<&str> = s.model.phase_filters.keys().map(String::as_str).collect();
        assert_eq!(names, ["Show All", "Show Complete", "Show New", "Show Previous + Demolished", "Show Previous + New"]);
        let mut p = Session::new();
        run(&mut p, "new", json!({"language": "pt"}));
        assert_eq!(p.model.phases, ["Existente", "Nova construção"]);
        assert!(p.model.phase_filters.contains_key("Mostrar tudo"));
        assert_eq!(run(&mut p, "phase", json!({}))["phases"], json!(["Existente", "Nova construção"]));
    }

    #[test]
    fn what_a_session_makes_is_in_the_phase_it_is_in_and_the_first_phase_is_no_value_at_all() {
        let mut s = Session::new();
        let walls = house(&mut s);
        assert_eq!(params(&s, walls[0]), Value::Null);
        run(&mut s, "phase", json!({"now": "New Construction"}));
        let made = run(&mut s, "wall", json!({"points": [[0, 7000], [3000, 7000]]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, made), json!({"Phase Created": "New Construction"}));
        // A copy made in the first phase is existing, whatever it was copied from.
        run(&mut s, "phase", json!({"now": "Existing"}));
        let copy = run(&mut s, "copy", json!({"ids": [made], "by": [0, 1000]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, copy), Value::Null);
        run(&mut s, "phase", json!({"now": null}));
        let again = run(&mut s, "copy", json!({"ids": [made], "by": [0, 2000]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, again), json!({"Phase Created": "New Construction"}), "no phase set: a copy keeps its original's");
        let e = s.execute("phase", &json!({"now": "Nowhere"})).unwrap_err();
        assert!(e.contains("Nowhere"), "{e}");
        assert_eq!(run(&mut s, "phase", json!({}))["now"], Value::Null);
    }

    #[test]
    fn a_view_shows_each_status_of_its_phase_as_its_phase_filter_says() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        let both = |s: &mut Session, filter: Value| {
            run(s, "graphics", json!({"view": 100000, "phase_filter": filter}));
            shown(s, &walls)
        };
        // The last phase, and every status as its category does, by default.
        assert_eq!(shown(&s, &walls), [true, true, true, true]);
        assert_eq!(both(&mut s, json!("Show Complete")), [true, true, false, true], "what's demolished is gone");
        assert_eq!(both(&mut s, json!("Show New")), [false, true, false, false]);
        assert_eq!(both(&mut s, json!("Show Previous + Demolished")), [true, false, true, false]);
        // Existing in halftone and demolished dashed in red, as the project's overrides say.
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[2]).contains(&(10, Rgb(200, 40, 30), Some(vec![10.0, 5.0]))), "{:?}", lines(&d, walls[2]));
        assert_eq!(both(&mut s, json!("Show Previous + New")), [true, true, false, true]);
        let d = plan(&s, 100000);
        assert!(lines(&d, walls[0]).contains(&(10, Rgb(140, 140, 140), None)), "{:?}", lines(&d, walls[0]));
        assert!(lines(&d, walls[1]).contains(&(10, Rgb(25, 25, 25), None)));
        // No filter again: as it was.
        assert_eq!(both(&mut s, Value::Null), [true, true, true, true]);
        // An earlier phase: what is made later isn't there, and what is taken down later still stands.
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing"}));
        assert_eq!(shown(&s, &walls), [true, false, true, false]);
        // Another view's phase is its own; a section and the 3D view follow the same rules.
        run(&mut s, "graphics", json!({"view": 100001, "phase": "Existing", "phase_filter": "Show New"}));
        assert_eq!(shown(&s, &walls), [true, false, true, false], "Level 1 as it was");
    }

    #[test]
    fn a_wall_taken_down_before_a_phase_is_not_in_it_whatever_the_filter_says() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Demolished": "Demolition"}}));
        // The default view's phase is the last: New Construction, after the demolition.
        assert_eq!(shown(&s, &walls), [false, true, true, true]);
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Demolition"}));
        assert_eq!(shown(&s, &walls), [true, true, true, true], "in the phase it comes down, it still stands");
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": "Show New"}));
        assert_eq!(shown(&s, &walls), [true, true, true, true], "made in the first phase, they are new in it");
        run(&mut s, "graphics", json!({"view": 100000, "phase": "New Construction", "phase_filter": "Show New"}));
        assert_eq!(shown(&s, &walls), [false, false, false, false], "and existing in the last, where the first came down");
    }

    #[test]
    fn a_door_with_no_phase_of_its_own_follows_its_wall() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Demolished": "New Construction"}}));
        run(&mut s, "graphics", json!({"view": 100000, "phase_filter": "Show Complete"}));
        let d = plan(&s, 100000);
        assert!(owned(&d, walls[0]).is_empty() && owned(&d, door).is_empty(), "taken down with its wall");
        assert_eq!(drawn_near(&d, P2(2000.0, 0.0), 100.0), 0, "and nothing of its opening is left where it was");
        // Its own say beats its wall's: made in the first phase, while its wall is made in the second.
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Created": "New Construction", "Phase Demolished": null}}));
        run(&mut s, "modify", json!({"id": door, "params": {"Phase Created": "Existing"}}));
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": null}));
        let d = plan(&s, 100000);
        assert!(owned(&d, walls[0]).is_empty() && !owned(&d, door).is_empty(), "there before the wall it's in");
    }

    #[test]
    fn phase_filters_and_overrides_are_the_projects_to_change() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        let r = run(
            &mut s,
            "phase_filter",
            json!({"name": "Renovation", "new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"}),
        );
        assert_eq!(
            r["filter"],
            json!({"new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"})
        );
        run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
        run(&mut s, "graphics", json!({"view": 100000, "phase_filter": "Renovation"}));
        assert_eq!(shown(&s, &walls), [true, true, false, false]);
        assert!(lines(&plan(&s, 100000), walls[1]).contains(&(10, Rgb(0, 0, 200), None)));
        assert!(lines(&plan(&s, 100000), walls[0]).contains(&(10, Rgb(25, 25, 25), None)));
        // The built-in ones can change too, and a filter in use can't go.
        run(&mut s, "phase_filter", json!({"name": "Show New", "existing": "overridden"}));
        let e = s.execute("phase_filter", &json!({"name": "Renovation", "delete": true})).unwrap_err();
        assert!(e.contains("Level 1"), "{e}");
        run(&mut s, "graphics", json!({"view": 100000, "phase_filter": null}));
        run(&mut s, "phase_filter", json!({"name": "Renovation", "delete": true}));
        assert!(!s.model.phase_filters.contains_key("Renovation"));
        // Overrides don't hide: a phase filter does, by not displaying.
        let e = s.execute("phase", &json!({"overrides": {"existing": {"hidden": true}}})).unwrap_err();
        assert!(e.contains("hid"), "{e}");
    }

    #[test]
    fn phases_are_added_in_order_renamed_with_what_holds_them_and_deleted_into_another() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "phase", json!({"name": "Furniture"}));
        run(&mut s, "phase", json!({"name": "Landscape", "after": "Furniture"}));
        assert_eq!(s.model.phases, ["Existing", "Demolition", "New Construction", "Furniture", "Landscape"]);
        run(&mut s, "graphics", json!({"view": 100000, "phase": "New Construction"}));
        run(&mut s, "phase", json!({"now": "New Construction"}));
        let r = run(&mut s, "phase", json!({"name": "New Construction", "rename": "Works"}));
        assert_eq!((r["now"].clone(), r["phases"][2].clone()), (json!("Works"), json!("Works")));
        assert_eq!(params(&s, walls[1]), json!({"Phase Created": "Works"}));
        assert_eq!(params(&s, walls[3]), json!({"Phase Created": "Works", "Phase Demolished": "Works"}));
        let Some(Element::Level(l)) = s.model.elements.get(100000) else { panic!("level") };
        assert_eq!(l.graphics.phase.as_deref(), Some("Works"), "views follow it");
        // Taken out, into another: its elements are made, and taken down, in that one.
        run(&mut s, "phase", json!({"name": "Works", "delete": true, "into": "Demolition"}));
        assert_eq!(s.model.phases, ["Existing", "Demolition", "Furniture", "Landscape"]);
        assert_eq!(params(&s, walls[1]), json!({"Phase Created": "Demolition"}));
        assert_eq!(params(&s, walls[3]), json!({"Phase Created": "Demolition", "Phase Demolished": "Demolition"}));
        let Some(Element::Level(l)) = s.model.elements.get(100000) else { panic!("level") };
        assert_eq!(l.graphics.phase.as_deref(), Some("Demolition"));
        assert_eq!(run(&mut s, "phase", json!({}))["now"], json!("Demolition"));
        // Mistakes say what's wrong, and change nothing.
        let before = s.model.clone();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("phase", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"name": "Furniture"}), "already");
        bad(json!({"name": "New", "before": "Nowhere"}), "Nowhere");
        bad(json!({"name": "New", "before": "Existing", "after": "Furniture"}), "before");
        bad(json!({"name": "Nowhere", "rename": "X"}), "Nowhere");
        bad(json!({"name": "Furniture", "rename": "Existing"}), "Existing");
        bad(json!({"name": "Furniture", "rename": ""}), "name");
        bad(json!({"name": "Demolition", "delete": true}), "into");
        bad(json!({"name": "Demolition", "delete": true, "into": "Demolition"}), "into");
        bad(json!({"name": "Furniture", "delete": true, "into": "Nowhere"}), "Nowhere");
        bad(json!({"name": "  "}), "name");
        bad(json!({"now": 5}), "now");
        bad(json!({"overrides": 5}), "overrides");
        bad(json!({"overrides": {"future": {}}}), "future");
        assert_eq!(*s.model, *before);
        // The last phase can't go, nor the only one.
        run(&mut s, "phase", json!({"name": "Landscape", "delete": true, "into": "Furniture"}));
        run(&mut s, "phase", json!({"name": "Furniture", "delete": true, "into": "Demolition"}));
        run(&mut s, "phase", json!({"name": "Demolition", "delete": true, "into": "Existing"}));
        let e = s.execute("phase", &json!({"name": "Existing", "delete": true, "into": "Existing"})).unwrap_err();
        assert!(e.contains("only"), "{e}");
    }

    #[test]
    fn phases_are_fields_of_their_elements_for_schedules_and_view_filters() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        let r =
            run(&mut s, "schedule", json!({"name": "Phasing", "category": "walls", "fields": ["Id", "Phase Created", "Phase Demolished"]}));
        let rows = r["rows"].as_array().unwrap();
        let id = |w: u64| json!(w as f64);
        assert_eq!(rows[0], json!([id(walls[0]), "Existing", null]), "no value is the first phase");
        assert_eq!(rows[1], json!([id(walls[1]), "New Construction", null]));
        assert_eq!(rows[2], json!([id(walls[2]), "Existing", "New Construction"]));
        let r = run(
            &mut s,
            "filter",
            json!({"name": "New work", "categories": ["walls"], "rules": [{"field": "Phase Created", "op": "equals", "value": "New Construction"}]}),
        );
        assert_eq!(r["matches"], json!([walls[1], walls[3]]));
        let r = run(
            &mut s,
            "filter",
            json!({"name": "Coming down", "categories": ["walls"], "rules": [{"field": "Phase Demolished", "op": "has-value"}]}),
        );
        assert_eq!(r["matches"], json!([walls[2], walls[3]]));
        // A project parameter can't take the name.
        let e = s.execute("parameter", &json!({"name": "Phase Created", "categories": ["wall"]})).unwrap_err();
        assert!(e.contains("Phase Created"), "{e}");
    }

    #[test]
    fn an_element_is_made_in_a_phase_the_project_has_and_taken_down_after_it() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let before = s.model.clone();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("modify", &json!({"id": walls[0], "params": p})).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"Phase Created": "Nowhere"}), "Nowhere");
        bad(json!({"Phase Created": 5}), "Phase Created");
        bad(json!({"Phase Demolished": "Nowhere"}), "Phase Demolished");
        bad(json!({"Phase Created": "New Construction", "Phase Demolished": "Existing"}), "before");
        bad(json!({"Design Option": "A"}), "Design Option");
        bad(json!({"Design Option": "Nowhere: A"}), "Nowhere");
        assert_eq!(*s.model, *before);
        // A door's phase isn't a project parameter either way; rooms and the others hold them.
        let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "modify", json!({"id": room, "params": {"Phase Created": "New Construction"}}));
        // Saved with the project and read back.
        let path = std::env::temp_dir().join(format!("bimcraft-phase-{}.bimcraft", std::process::id()));
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": "Show New"}));
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(t.model, s.model);
        assert_eq!(t.model.phases, ["Existing", "Demolition", "New Construction"]);
    }

    #[test]
    fn design_options_show_in_the_views_that_show_them_the_primary_unless_a_view_chooses() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let r = run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        assert_eq!((r["options"].clone(), r["primary"].clone()), (json!(["Brick", "Glass"]), json!("Brick")));
        run(&mut s, "modify", json!({"ids": [walls[0]], "params": {"Design Option": "Facade: Brick"}}));
        run(&mut s, "modify", json!({"ids": [walls[2]], "params": {"Design Option": "Facade: Glass"}}));
        // The primary shows by default; what is in no option shows in all.
        assert_eq!(shown(&s, &walls), [true, true, false, true]);
        run(&mut s, "graphics", json!({"view": 100000, "options": {"Facade": "Glass"}}));
        assert_eq!(shown(&s, &walls), [false, true, true, true]);
        // Another view is its own, and null takes the choice back.
        run(&mut s, "graphics", json!({"view": 100001, "options": {"Facade": "Brick"}}));
        assert_eq!(shown(&s, &walls), [false, true, true, true]);
        run(&mut s, "graphics", json!({"view": 100000, "options": {"Facade": null}}));
        assert_eq!(shown(&s, &walls), [true, true, false, true]);
        // Changing the primary changes what views that don't choose show.
        run(&mut s, "option", json!({"set": "Facade", "primary": "Glass"}));
        assert_eq!(shown(&s, &walls), [false, true, true, true]);
        // What a session makes while an option is active is in it, doors with their walls.
        run(&mut s, "option", json!({"now": "Facade: Brick"}));
        let made = run(&mut s, "wall", json!({"points": [[0, 7000], [3000, 7000]]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, made), json!({"Design Option": "Facade: Brick"}));
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "option", json!({"now": null}));
        run(&mut s, "modify", json!({"id": door, "params": {"Design Option": null}}));
        let d = plan(&s, 100000);
        assert!(owned(&d, walls[0]).is_empty() && owned(&d, door).is_empty(), "a door follows its wall into its option");
        assert_eq!(drawn_near(&d, P2(2000.0, 0.0), 100.0), 0, "and leaves no opening behind");
    }

    #[test]
    fn an_option_set_changes_only_by_rules_that_keep_what_is_in_it() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Design Option": "Facade: Glass"}}));
        let before = s.model.clone();
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("option", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"set": "Facade", "options": ["Brick"]}), "Glass");
        bad(json!({"set": "Facade", "primary": "Nowhere"}), "Nowhere");
        bad(json!({"set": "Facade", "options": []}), "options");
        bad(json!({"set": "Facade", "options": ["A", "A"]}), "twice");
        bad(json!({"set": "A:B", "options": ["x"]}), "colon");
        bad(json!({"set": "Roof", "options": ["x: y"]}), "colon");
        bad(json!({"set": "Roof"}), "options");
        bad(json!({"set": "Nowhere", "delete": true}), "Nowhere");
        bad(json!({"now": "Nowhere: A"}), "Nowhere");
        bad(json!({"now": 5}), "now");
        bad(json!({}), "set");
        assert_eq!(*s.model, *before);
        // Renamed, the elements follow.
        run(&mut s, "option", json!({"set": "Facade", "option": "Glass", "rename": "Curtain wall"}));
        assert_eq!(params(&s, walls[0]), json!({"Design Option": "Facade: Curtain wall"}));
        run(&mut s, "option", json!({"set": "Facade", "rename": "Envelope"}));
        assert_eq!(params(&s, walls[0]), json!({"Design Option": "Envelope: Curtain wall"}));
        assert!(s.model.option_sets.contains_key("Envelope"));
    }

    #[test]
    fn accepting_the_primary_keeps_its_elements_in_the_model_and_takes_the_others_away() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"], "primary": "Glass"}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Design Option": "Facade: Brick"}}));
        run(&mut s, "modify", json!({"id": walls[2], "params": {"Design Option": "Facade: Glass"}}));
        let r = run(&mut s, "option", json!({"set": "Facade", "accept": true}));
        assert!(
            r["deleted"].as_array().unwrap().contains(&json!(walls[0])) && r["deleted"].as_array().unwrap().contains(&json!(door)),
            "{r}"
        );
        assert!(!s.model.elements.contains_key(walls[0]), "the brick wall and its door went");
        assert_eq!(params(&s, walls[2]), Value::Null, "the glass wall is in the model now");
        assert!(s.model.option_sets.is_empty());
        // Deleting a set takes its elements, undoably.
        run(&mut s, "option", json!({"set": "Roof", "options": ["Hip", "Gable"]}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Design Option": "Roof: Gable"}}));
        run(&mut s, "option", json!({"set": "Roof", "delete": true}));
        assert!(!s.model.elements.contains_key(walls[1]));
        run(&mut s, "undo", json!({}));
        assert!(s.model.elements.contains_key(walls[1]) && s.model.option_sets.contains_key("Roof"));
    }

    #[test]
    fn templates_control_a_views_phase_and_its_options() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Design Option": "Facade: Glass"}}));
        run(
            &mut s,
            "graphics",
            json!({"view": 100000, "phase": "Existing", "phase_filter": "Show Complete", "options": {"Facade": "Glass"}}),
        );
        let r = run(&mut s, "template", json!({"name": "Survey", "from": 100000}));
        assert_eq!(r["controls"], json!(["categories", "filters", "range", "phase", "options", "scale", "detail", "style"]));
        assert_eq!(
            (r["phase"].clone(), r["phase_filter"].clone(), r["options"].clone()),
            (json!("Existing"), json!("Show Complete"), json!({"Facade": "Glass"}))
        );
        run(&mut s, "graphics", json!({"view": 100001, "template": "Survey"}));
        let g = run(&mut s, "graphics", json!({"view": 100001}));
        assert_eq!((g["phase"].clone(), g["options"].clone()), (json!("Existing"), json!({"Facade": "Glass"})));
        // What it controls isn't the view's to change.
        let e = s.execute("graphics", &json!({"view": 100001, "phase": "New Construction"})).unwrap_err();
        assert!(e.contains("Survey"), "{e}");
        run(&mut s, "template", json!({"name": "Survey", "controls": ["categories"]}));
        run(&mut s, "graphics", json!({"view": 100001, "phase": "New Construction"}));
    }

    #[test]
    fn sections_and_3d_views_follow_their_phase_too_and_graphics_names_only_what_the_project_has() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "graphics", json!({"view": section, "phase_filter": "Show Complete"}));
        let view = crate::render::section_fit(&s.model, section, 800, 600);
        let d = crate::render::section(&s.model, section, view);
        // The east wall (the second) is new and cut; the west (the fourth) is temporary and cut; both show.
        assert!(!owned(&d, walls[1]).is_empty() && !owned(&d, walls[3]).is_empty());
        run(&mut s, "graphics", json!({"view": section, "phase_filter": "Show New"}));
        let d = crate::render::section(&s.model, section, view);
        assert!(!owned(&d, walls[1]).is_empty() && owned(&d, walls[3]).is_empty(), "only what's new");
        // The 3D view: nothing of any wall in a phase before any was made but the first's.
        run(&mut s, "graphics", json!({"view": "{3D}", "phase": "Existing", "phase_filter": "Show New"}));
        let shown = |pm: &tiny_skia::Pixmap| pm.pixels().iter().filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255)).count();
        let path = std::env::temp_dir().join(format!("bimcraft-phase3d-{}.png", std::process::id()));
        run(&mut s, "render", json!({"view": "{3D}", "width": 200, "height": 150, "path": path.to_str().unwrap()}));
        let pm = tiny_skia::Pixmap::load_png(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let in_the_first = shown(&pm);
        assert!(in_the_first > 100, "the walls made in the first phase stand in it, new there");
        // In the last phase only the second wall is new: less is drawn.
        run(&mut s, "graphics", json!({"view": "{3D}", "phase": "New Construction"}));
        run(&mut s, "render", json!({"view": "{3D}", "width": 200, "height": 150, "path": path.to_str().unwrap()}));
        let pm = tiny_skia::Pixmap::load_png(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let in_the_last = shown(&pm);
        assert!(in_the_last > 0 && in_the_last < in_the_first, "{in_the_last} of the second wall, {in_the_first} of two");
        // Named in a view, a phase, a filter, a set and its option must be the project's.
        let mut bad = |p: Value, says: &str| {
            let e = s.execute("graphics", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        };
        bad(json!({"view": 100000, "phase": "Nowhere"}), "Nowhere");
        bad(json!({"view": 100000, "phase_filter": "Nope"}), "Nope");
        bad(json!({"view": 100000, "options": {"Nope": "x"}}), "Nope");
        bad(json!({"view": 100000, "phase": 5}), "phase");
        bad(json!({"view": 100000, "options": 5}), "options");
        bad(json!({"view": 100000, "options": {"Facade": 5}}), "Facade");
        let e = s.execute("option", &json!({"set": "Facade", "options": ["Brick"]})).map(|_| ());
        assert!(e.is_ok());
        let e = s.execute("graphics", &json!({"view": 100000, "options": {"Facade": "Nope"}})).unwrap_err();
        assert!(e.contains("Nope"), "{e}");
    }

    #[test]
    fn inspect_tells_a_projects_phases_phase_filters_overrides_and_option_sets() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        let r = run(&mut s, "inspect", json!({}));
        assert_eq!(r["phases"], json!(["Existing", "New Construction"]));
        assert!(r["phase_filters"]["Show All"].is_object() && r["option_sets"]["Facade"]["primary"] == "Brick");
        assert!(r["phase_overrides"]["demolished"]["cut"]["dash"] == "dashed");
    }

    #[test]
    fn a_sheet_prints_each_view_in_its_phase() {
        let mut s = Session::new();
        let walls = phased_house(&mut s);
        run(&mut s, "sheet", json!({"name": "Plans"}));
        run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}));
        let (sheet, vp) = (100006, 100007);
        let colour = |s: &Session| {
            let d = crate::sheet::draw(&s.model, sheet).unwrap();
            let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
            let (v, (ox, oy)) = crate::sheet::frame(&s.model, vp).unwrap();
            let (x, y) = v.px(crate::geom::P2(4000.0, 6000.0));
            let c = pm.pixel((x + ox) as u32, (y + oy) as u32).unwrap();
            Rgb(c.red(), c.green(), c.blue())
        };
        assert_ne!(colour(&s), PAPER, "the third wall stands");
        run(&mut s, "graphics", json!({"view": 100000, "phase_filter": "Show Complete"}));
        assert_eq!(colour(&s), PAPER, "taken down in the new phase");
        let _ = walls;
    }

    #[test]
    fn redoing_leaves_what_was_made_in_the_phase_it_was_made_in() {
        let mut s = Session::new();
        house(&mut s);
        // Made in the first phase, which holds no value; the session is in the last by the time it is redone.
        run(&mut s, "phase", json!({"now": "Existing"}));
        let made = run(&mut s, "wall", json!({"points": [[0, 7000], [3000, 7000]]}))["ids"][0].as_u64().unwrap();
        run(&mut s, "undo", json!({}));
        run(&mut s, "phase", json!({"now": "New Construction"}));
        run(&mut s, "redo", json!({}));
        assert_eq!(params(&s, made), Value::Null, "redone, not made again in the phase the session is in now");
    }

    #[test]
    fn the_default_3d_view_shows_the_last_phase_and_the_primary_options_before_anything_changes_it() {
        use crate::graphics::Look;
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Design Option": "Facade: Glass"}}));
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Demolished": "Demolition"}}));
        assert!(s.model.default_3d().is_none());
        let look = Look::of_3d(&s.model);
        let seen: Vec<bool> = walls.iter().map(|w| look.shows(*w)).collect();
        assert_eq!(seen, [false, false, true, true], "one is in an option the view doesn't show, one came down before the last phase");
        // The command that renders it reads the same look: a lone wall in the other option leaves nothing to draw.
        let mut t = Session::new();
        let wall = run(&mut t, "wall", json!({"points": [[0, 0], [4000, 0]]}))["ids"][0].as_u64().unwrap();
        run(&mut t, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        let path = std::env::temp_dir().join(format!("bimcraft-phase3d-default-{}.png", std::process::id()));
        let drawn = |t: &mut Session| {
            run(t, "render", json!({"view": "{3D}", "width": 200, "height": 150, "path": path.to_str().unwrap()}));
            let pm = tiny_skia::Pixmap::load_png(&path).unwrap();
            // What isn't paper, nor the levels' planes.
            pm.pixels().iter().filter(|c| ![(255, 255, 255), (60, 100, 150)].contains(&(c.red(), c.green(), c.blue()))).count()
        };
        assert!(drawn(&mut t) > 100);
        run(&mut t, "modify", json!({"id": wall, "params": {"Design Option": "Facade: Glass"}}));
        assert_eq!(drawn(&mut t), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn combining_phases_never_leaves_a_wall_taken_down_before_it_was_made() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "phase", json!({"name": "Furniture"}));
        // Made in the second phase, taken down in the third...
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Created": "Demolition", "Phase Demolished": "New Construction"}}));
        // ...and the second goes into the last: it is made there now, and comes down no earlier than that.
        run(&mut s, "phase", json!({"name": "Demolition", "delete": true, "into": "Furniture"}));
        assert_eq!(params(&s, walls[0]), json!({"Phase Created": "Furniture", "Phase Demolished": "Furniture"}));
    }

    #[test]
    fn a_phase_a_view_shows_is_in_use_and_goes_into_another() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "graphics", json!({"view": 100000, "phase": "New Construction"}));
        let e = s.execute("phase", &json!({"name": "New Construction", "delete": true})).unwrap_err();
        assert!(e.contains("into"), "{e}");
        run(&mut s, "phase", json!({"name": "New Construction", "delete": true, "into": "Existing"}));
        let Some(Element::Level(l)) = s.model.elements.get(100000) else { panic!("level") };
        assert_eq!(l.graphics.phase.as_deref(), Some("Existing"));
    }

    #[test]
    fn a_phase_put_in_before_the_first_leaves_what_was_made_in_the_first_where_it_was() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "phase", json!({"name": "Survey", "before": "Existing"}));
        assert_eq!(s.model.phases, ["Survey", "Existing", "New Construction"]);
        assert_eq!(params(&s, walls[0]), json!({"Phase Created": "Existing"}), "what holds no value was in the first");
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": "Show New"}));
        assert_eq!(shown(&s, &walls), [true, true, true, true], "made in Existing, they are new in it");
        assert!(!owned(&plan(&s, 100000), door).is_empty());
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Survey", "phase_filter": null}));
        assert_eq!(shown(&s, &walls), [false, false, false, false], "and not there before it was made");
    }

    #[test]
    fn deleting_the_first_phase_takes_what_was_made_in_it_into_the_one_named() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "Demolition"}}));
        // What was made in the first phase is in it, though it holds no value: it has to go somewhere.
        let e = s.execute("phase", &json!({"name": "Existing", "delete": true})).unwrap_err();
        assert!(e.contains("into"), "{e}");
        // Into the last, past the second, which becomes the first: made there, not in it.
        run(&mut s, "phase", json!({"name": "Existing", "delete": true, "into": "New Construction"}));
        assert_eq!(s.model.phases, ["Demolition", "New Construction"]);
        assert_eq!(params(&s, walls[0]), json!({"Phase Created": "New Construction"}));
        assert_eq!(params(&s, walls[1]), json!({"Phase Created": "Demolition"}), "what was made in the second still was");
        // A project with nothing in its first phase lets it go.
        let mut t = Session::new();
        run(&mut t, "phase", json!({"name": "Existing", "delete": true}));
        assert_eq!(t.model.phases, ["New Construction"]);
    }

    #[test]
    fn a_schedule_says_the_phase_a_door_follows_its_wall_to() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "door", json!({"at": [2000, 0]}));
        run(
            &mut s,
            "modify",
            json!({"id": walls[0], "params": {"Phase Created": "New Construction", "Phase Demolished": "New Construction"}}),
        );
        let r =
            run(&mut s, "schedule", json!({"name": "Door phases", "category": "doors", "fields": ["Phase Created", "Phase Demolished"]}));
        assert_eq!(r["rows"][0], json!(["New Construction", "New Construction"]));
    }

    #[test]
    fn schedules_sort_and_group_a_phase_field_in_the_projects_order_not_by_its_name() {
        let mut s = Session::new();
        let walls = house(&mut s);
        // Existing, Demolition, New Construction: not the order the names sort in.
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        let held = [
            (walls[0], json!({"Phase Demolished": "New Construction"})),
            (walls[1], json!({"Phase Created": "Demolition"})),
            (walls[2], json!({"Phase Created": "New Construction"})),
            (walls[3], json!({"Phase Demolished": "Demolition"})),
        ];
        for (id, params) in held {
            run(&mut s, "modify", json!({"id": id, "params": params}));
        }
        // The walls in the order a schedule sorted by `field` lists them; the ones that tie keep their order.
        let order = |s: &mut Session, field: &str, descending: bool| -> Vec<u64> {
            let sort = json!([{"field": field, "descending": descending}]);
            let r = run(s, "schedule", json!({"name": "Phases", "category": "walls", "fields": ["Id"], "sort": sort}));
            r["rows"].as_array().unwrap().iter().map(|row| row[0].as_f64().unwrap() as u64).collect()
        };
        let w = &walls;
        assert_eq!(order(&mut s, "Phase Created", false), [w[0], w[3], w[1], w[2]], "no phase is the first");
        assert_eq!(order(&mut s, "Phase Created", true), [w[2], w[1], w[0], w[3]]);
        assert_eq!(order(&mut s, "Phase Demolished", false), [w[3], w[0], w[1], w[2]], "none after every phase");
        assert_eq!(order(&mut s, "Phase Demolished", true), [w[1], w[2], w[0], w[3]]);
        // Grouped, a row for each phase, in its order.
        let by = |s: &mut Session, field: &str, descending: bool| {
            let sort = json!([{"field": field, "descending": descending}]);
            let fields = json!([field, "Count"]);
            run(s, "schedule", json!({"name": field, "category": "walls", "fields": fields, "sort": sort, "grouped": true}))["rows"].clone()
        };
        assert_eq!(by(&mut s, "Phase Created", false), json!([["Existing", 2.0], ["Demolition", 1.0], ["New Construction", 1.0]]));
        assert_eq!(by(&mut s, "Phase Demolished", true), json!([[null, 2.0], ["New Construction", 1.0], ["Demolition", 1.0]]));
        // A door with no phase of its own sorts where its wall's phase puts it.
        run(&mut s, "door", json!({"at": [2000, 0]}));
        run(&mut s, "door", json!({"at": [8000, 3000]}));
        let fields = json!(["Wall", "Phase Created"]);
        let r = run(&mut s, "schedule", json!({"name": "Doors", "category": "doors", "fields": fields, "sort": ["Phase Created"]}));
        assert_eq!(r["rows"], json!([[w[0] as f64, "Existing"], [w[1] as f64, "Demolition"]]));
        // Levels hold no phases: a calculated field of that name there sorts as the numbers it comes to.
        let fields = json!(["Name", {"name": "Phase Created", "formula": "Elevation", "kind": "length"}]);
        let sort = json!([{"field": "Phase Created", "descending": true}]);
        let r = run(&mut s, "schedule", json!({"name": "Heights", "category": "levels", "fields": fields, "sort": sort}));
        assert_eq!(r["rows"], json!([["Level 2", 3000.0], ["Level 1", 0.0]]));
    }

    #[test]
    fn a_view_or_a_template_that_chose_among_a_set_stops_when_the_set_goes() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "graphics", json!({"view": 100000, "options": {"Facade": "Glass"}}));
        run(&mut s, "template", json!({"name": "T", "from": 100000}));
        run(&mut s, "option", json!({"set": "Facade", "accept": true}));
        let Some(Element::Level(l)) = s.model.elements.get(100000) else { panic!("level") };
        assert!(l.graphics.options.is_empty());
        assert!(s.model.view_templates["T"].graphics.options.is_empty());
    }

    #[test]
    fn what_a_session_makes_leaves_out_a_phase_or_an_option_the_project_no_longer_has() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "phase", json!({"now": "Demolition"}));
        run(&mut s, "option", json!({"now": "Facade: Glass"}));
        // Undone, the set and the phase are gone, and the session still names them.
        run(&mut s, "undo", json!({}));
        run(&mut s, "undo", json!({}));
        assert!(s.model.option_sets.is_empty() && s.model.phases.len() == 2);
        let made = run(&mut s, "wall", json!({"points": [[0, 7000], [3000, 7000]]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, made), Value::Null, "made with no phase and no option, not with names the project hasn't");
    }

    #[test]
    fn a_split_wall_keeps_the_phase_and_the_option_of_the_wall_it_came_from() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        // The first wall holds an option and no phase, the second a phase and no option: its halves hold the same, not the session's.
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Design Option": "Facade: Glass"}}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "Existing"}}));
        run(&mut s, "phase", json!({"now": "New Construction"}));
        run(&mut s, "option", json!({"now": "Facade: Brick"}));
        let both = [
            (walls[0], [3000, 0], json!({"Design Option": "Facade: Glass"})),
            (walls[1], [8000, 2000], json!({"Phase Created": "Existing"})),
        ];
        for (wall, at, held) in both {
            let r = run(&mut s, "split", json!({"id": wall, "at": at}));
            let halves: Vec<u64> = r["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
            assert_eq!(halves.len(), 2, "{r}");
            for h in halves {
                assert_eq!(params(&s, h), held, "a half of wall {wall}");
            }
        }
    }

    #[test]
    fn a_refused_phase_or_option_command_leaves_the_session_as_it_was() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "phase", json!({"now": "New Construction"}));
        let e = s.execute("phase", &json!({"name": "New Construction", "rename": "Works", "now": "Nowhere"})).unwrap_err();
        assert!(e.contains("Nowhere"), "{e}");
        assert_eq!(s.phase.as_deref(), Some("New Construction"), "the rename was undone, and so was what it did to the session");
        assert_eq!(s.model.phases, ["Existing", "New Construction"]);
        run(&mut s, "option", json!({"set": "S", "options": ["a", "b"]}));
        run(&mut s, "option", json!({"now": "S: a"}));
        let e = s.execute("option", &json!({"set": "S", "rename": "T", "now": "Nowhere: x"})).unwrap_err();
        assert!(e.contains("Nowhere"), "{e}");
        assert_eq!(s.option.as_deref(), Some("S: a"));
        assert!(s.model.option_sets.contains_key("S"));
    }

    #[test]
    fn a_copy_made_in_a_phase_is_new_in_it_and_what_is_made_with_its_values_given_keeps_them() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Demolished": "New Construction"}}));
        run(&mut s, "phase", json!({"now": "New Construction"}));
        // A copy of what comes down in this phase doesn't come down with it: it is made here.
        let copy = run(&mut s, "copy", json!({"ids": [walls[0]], "by": [0, 2000]}))["ids"][0].as_u64().unwrap();
        assert_eq!(params(&s, copy), json!({"Phase Created": "New Construction"}));
        // So is the copy a mirror or a rotation makes.
        let both = [
            ("mirror", json!({"ids": [walls[0]], "axis": [[0, 3000], [8000, 3000]], "copy": true})),
            ("rotate", json!({"ids": [walls[0]], "about": [0, 0], "angle": 90, "copy": true})),
        ];
        for (id, p) in both {
            let made = run(&mut s, id, p)["ids"][0].as_u64().unwrap();
            assert_eq!(params(&s, made), json!({"Phase Created": "New Construction"}), "{id}");
        }
        // What is made with its phase and its option given keeps them, the first phase named as it was.
        run(&mut s, "option", json!({"set": "S", "options": ["a", "b"]}));
        run(&mut s, "option", json!({"now": "S: a"}));
        let c = run(
            &mut s,
            "component",
            json!({"family": "WC", "at": [1000, 1000], "params": {"Phase Created": "Existing", "Design Option": "S: b"}}),
        )["id"]
            .as_u64()
            .unwrap();
        assert_eq!(params(&s, c), json!({"Phase Created": "Existing", "Design Option": "S: b"}));
        // And what is made with none is made in the session's.
        let d = run(&mut s, "component", json!({"family": "WC", "at": [3000, 1000]}))["id"].as_u64().unwrap();
        assert_eq!(params(&s, d), json!({"Phase Created": "New Construction", "Design Option": "S: a"}));
    }

    #[test]
    fn a_door_made_in_the_first_phase_does_not_follow_a_wall_made_later() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Created": "New Construction"}}));
        run(&mut s, "phase", json!({"now": "Existing"}));
        let door = run(&mut s, "door", json!({"wall": walls[0], "offset": 2000}))["id"].as_u64().unwrap();
        assert_eq!(params(&s, door), json!({"Phase Created": "Existing"}), "named, or it would follow its wall");
    }

    #[test]
    fn combining_phases_clamps_a_doors_demolition_to_where_its_wall_is_made_now() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        for n in ["X", "Y"] {
            run(&mut s, "phase", json!({"name": n, "before": "New Construction"}));
        }
        run(&mut s, "phase", json!({"name": "Z"}));
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Created": "X"}}));
        run(&mut s, "modify", json!({"id": door, "params": {"Phase Demolished": "Y"}}));
        run(&mut s, "phase", json!({"name": "X", "delete": true, "into": "Z"}));
        assert_eq!(params(&s, door), json!({"Phase Demolished": "Z"}), "it follows its wall, which is made in Z now");
    }

    #[test]
    fn a_door_stays_with_its_wall_when_a_phase_goes_in_before_the_first() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "phase", json!({"name": "Survey", "before": "Existing"}));
        assert_eq!(params(&s, walls[0]), json!({"Phase Created": "Existing"}));
        assert_eq!(params(&s, door), Value::Null, "it still follows its wall");
        run(&mut s, "modify", json!({"id": walls[0], "params": {"Phase Created": "New Construction"}}));
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing"}));
        let d = plan(&s, 100000);
        assert!(owned(&d, walls[0]).is_empty(), "the wall isn't made yet");
        assert_eq!(drawn_near(&d, P2(2000.0, 0.0), 100.0), 0, "and nor is its door: nothing of either is drawn");
    }

    #[test]
    fn a_door_and_its_tag_leave_the_plan_with_the_wall_a_phase_takes_away() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "door", json!({"at": [2000, 0]}));
        run(&mut s, "window", json!({"at": [8000, 3000]}));
        run(&mut s, "tag", json!({}));
        let (door_at, window_at) = (P2(2000.0, 0.0), P2(8000.0, 3000.0));
        let d = plan(&s, 100000);
        assert!(drawn_near(&d, door_at, 100.0) > 0 && drawn_near(&d, window_at, 100.0) > 0);
        run(&mut s, "modify", json!({"ids": [walls[0], walls[1]], "params": {"Phase Demolished": "New Construction"}}));
        run(&mut s, "graphics", json!({"view": 100000, "phase_filter": "Show Complete"}));
        let d = plan(&s, 100000);
        assert_eq!(drawn_near(&d, door_at, 100.0), 0, "no gap, jambs or tag where the wall was");
        assert_eq!(drawn_near(&d, window_at, 100.0), 0);
    }

    #[test]
    fn filters_and_schedules_that_name_a_phase_or_an_option_follow_its_new_name() {
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "New Construction"}}));
        run(&mut s, "option", json!({"set": "S", "options": ["a", "b"]}));
        run(&mut s, "modify", json!({"id": walls[2], "params": {"Design Option": "S: b"}}));
        let rule = |field: &str, value: &str| json!({"categories": ["walls"], "rules": [{"field": field, "op": "equals", "value": value}]});
        let mut made = |name: &str, field: &str, value: &str| {
            let mut p = rule(field, value);
            p["name"] = json!(name);
            run(&mut s, "filter", p);
        };
        made("New work", "Phase Created", "New Construction");
        made("In b", "Design Option", "S: b");
        run(
            &mut s,
            "schedule",
            json!({"name": "New walls", "category": "walls", "fields": ["Id"], "filters": [{"field": "Phase Created", "op": "equals", "value": "New Construction"}]}),
        );
        run(&mut s, "phase", json!({"name": "New Construction", "rename": "Works"}));
        run(&mut s, "option", json!({"set": "S", "option": "b", "rename": "beta"}));
        run(&mut s, "option", json!({"set": "S", "rename": "T"}));
        assert_eq!(run(&mut s, "filter", json!({"name": "New work"}))["matches"], json!([walls[1]]));
        assert_eq!(run(&mut s, "filter", json!({"name": "In b"}))["matches"], json!([walls[2]]));
        let rows = run(&mut s, "schedule", json!({"name": "New walls"}))["rows"].as_array().map(Vec::len);
        assert_eq!(rows, Some(1), "the schedule still lists the new wall");
        // Combined into another phase, a rule on it is on that one.
        run(&mut s, "phase", json!({"name": "Works", "delete": true, "into": "Existing"}));
        assert_eq!(run(&mut s, "filter", json!({"name": "New work"}))["matches"].as_array().map(Vec::len), Some(4));
    }

    #[test]
    fn an_option_a_view_chose_can_go_and_the_view_goes_back_to_the_primary() {
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "option", json!({"set": "S", "options": ["a", "b"]}));
        run(&mut s, "graphics", json!({"view": 100000, "options": {"S": "b"}}));
        run(&mut s, "template", json!({"name": "T", "from": 100000}));
        run(&mut s, "option", json!({"set": "S", "options": ["a"]}));
        let Some(Element::Level(l)) = s.model.elements.get(100000) else { panic!("level") };
        assert!(l.graphics.options.is_empty());
        assert!(s.model.view_templates["T"].graphics.options.is_empty());
    }

    #[test]
    fn a_family_cant_have_an_instance_parameter_a_phase_or_an_option_lives_in() {
        let mut s = Session::new();
        for name in ["Phase Created", "Phase Demolished", "Design Option"] {
            let p = json!({"name": "Box", "parameters": [{"name": name, "instance": true, "value": 100}]});
            let e = s.execute("family", &p).unwrap_err();
            assert!(e.contains(name), "{e}");
        }
        assert!(!s.model.families.contains_key("Box"));
    }

    #[test]
    fn a_file_whose_project_parameter_is_named_like_a_phase_or_an_option_opens_with_it_renamed() {
        let mut s = Session::new();
        let walls = house(&mut s);
        let path = std::env::temp_dir().join(format!("bimcraft-reserved-{}.bimcraft", std::process::id()));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        // What the version before phases let a project do.
        let mut file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        file["parameters"] = json!({"Design Option": {"kind": "text", "categories": ["wall"]}});
        file["elements"][walls[0].to_string()]["params"] = json!({"Design Option": "Brick facade"});
        std::fs::write(&path, file.to_string()).unwrap();
        let mut t = Session::new();
        let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(r["renamed"], json!({"Design Option": "Design Option Parameter"}));
        assert!(t.model.parameters.contains_key("Design Option Parameter") && !t.model.parameters.contains_key("Design Option"));
        assert_eq!(params(&t, walls[0]), json!({"Design Option Parameter": "Brick facade"}), "its value went with it");
    }
}
