//! Where a project comes from, and what the program's arguments ask: `--sample` (the default), `--highrise`, a file (opened by the
//! core's `open`, as egui does), and `--rev` or `--bimcraft` for the profile. A flag this crate does not know (Rune's own, `--software`)
//! is left to whoever reads it, and the last project named wins, as in the egui app.

use std::ffi::OsString;
use std::path::PathBuf;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use serde_json::json;

use crate::profile::Profile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Sample,
    Highrise,
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub source: Source,
    pub profile: Profile,
}

impl Default for Start {
    fn default() -> Start {
        Start { source: Source::Sample, profile: Profile::Bimcraft }
    }
}

/// What the arguments (the program's own, without its name) ask.
pub fn parse(args: &[OsString]) -> Start {
    let mut start = Start::default();
    for arg in args {
        match arg.to_str() {
            Some("--sample") => start.source = Source::Sample,
            Some("--highrise") => start.source = Source::Highrise,
            Some("--rev") => start.profile = Profile::Rev,
            Some("--bimcraft") => start.profile = Profile::Bimcraft,
            Some(flag) if flag.starts_with('-') => {}
            _ => start.source = Source::File(PathBuf::from(arg)),
        }
    }
    start
}

/// The session a start opens on, and what stopped it, if anything: a project that would not open leaves an empty one and says why.
/// Nothing is left to undo, as in egui's start.
pub fn load(source: &Source) -> (Session, Option<String>) {
    let mut session = Session::new();
    let made = match source {
        Source::Sample => session.script(SAMPLE).map(|_| ()),
        Source::Highrise => session.execute("highrise", &json!({})).map(|_| ()),
        Source::File(path) => session.execute("open", &json!({ "path": path })).map(|_| ()),
    };
    match made {
        Ok(()) => {
            let (model, path) = ((*session.model).clone(), session.path.clone());
            session.reset(model, path);
            (session, None)
        }
        Err(why) => (Session::new(), Some(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_arguments_name_a_project_and_a_profile() {
        assert_eq!(parse(&args(&[])), Start::default());
        assert_eq!(parse(&args(&["--highrise", "--rev"])), Start { source: Source::Highrise, profile: Profile::Rev });
        assert_eq!(parse(&args(&["house.bimc"])).source, Source::File("house.bimc".into()));
        assert_eq!(parse(&args(&["a.bimc", "--sample"])).source, Source::Sample, "the last project wins");
        assert_eq!(parse(&args(&["--rev", "--bimcraft"])).profile, Profile::Bimcraft);
        assert_eq!(parse(&args(&["--software"])), Start::default(), "a flag this crate does not know asks nothing of it");
    }

    #[test]
    fn a_project_that_will_not_open_leaves_an_empty_one_and_says_why() {
        let (session, why) = load(&Source::File("/no/such/project.bimc".into()));
        assert!(why.is_some());
        assert!(session.path.is_none(), "an empty project has no file");
        let (session, why) = load(&Source::Sample);
        assert!(why.is_none() && session.model.levels().len() >= 2, "the sample house has its levels");
    }
}
