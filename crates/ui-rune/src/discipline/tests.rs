//! The discipline dialogs' shape: the kinds there are, and how each opens on the project.

use super::*;
use crate::app::App;

#[test]
fn every_kind_has_a_name_and_none_is_named_twice() {
    let mut names: Vec<&str> = Kind::ALL.iter().map(|k| k.label()).collect();
    assert!(names.iter().all(|n| !n.is_empty()));
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n);
}

#[test]
fn reinforce_needs_a_host_selected_and_the_quantities_open_on_the_project() {
    let app = App::sample();
    let m = &*app.session.model;
    let why = Kind::Reinforce.open(m, &[]).err().unwrap_or_default();
    assert!(why.contains("Select"), "{why}");
    for kind in [Kind::Materials, Kind::SteelList, Kind::CutPlan] {
        let d = kind.open(m, &[]).unwrap_or_else(|why| panic!("{kind:?}: {why}"));
        assert!(!d.title().is_empty() && !d.sections().is_empty() && !d.foot().is_empty(), "{kind:?}");
        assert_eq!(d.tabs().len(), 3, "the quantities have a page for each of the three");
    }
}
