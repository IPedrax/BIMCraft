//! Opening Revit files: the reader the desktop app attaches, what a window with and without it does with a `.rvt`, and what
//! comes of an import. No test reads a real Revit file: the reader here is a closure that answers as the test says.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use bimcraft_core::import::Imported;

use super::*;
use crate::revit::{Importer, open_extensions};
use crate::{OsEvent, Status};

/// What a Revit import brings: a project named Survey with one wall, and `notes` for what it left out.
fn survey(notes: &[&str]) -> Imported {
    let mut s = Session::new();
    s.script("wall {\"points\": [[0, 0], [3000, 0]]}").unwrap();
    let mut model = (*s.model).clone();
    model.name = "Survey".into();
    Imported { model, schema: "Revit 2024".into(), skipped: notes.iter().map(|n| n.to_string()).collect(), moved: None }
}

type Inbox = Rc<RefCell<Vec<OsEvent>>>;

struct Rig {
    h: Harness<'static, BimApp>,
    /// The system's requests, which the window reads once a frame.
    inbox: Inbox,
    /// The files the reader was asked for.
    asked: Arc<Mutex<Vec<PathBuf>>>,
}

/// The app on a project of four walls, with a reader that answers `answer()` whatever it is given.
fn rig(answer: impl Fn() -> Result<Imported, String> + Send + Sync + 'static) -> Rig {
    let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = Arc::clone(&asked);
    let reader: Importer = Arc::new(move |path: &Path| {
        log.lock().unwrap().push(path.to_path_buf());
        answer()
    });
    let mut h = app(BOX);
    h.state_mut().revit = Some(reader);
    let inbox: Inbox = Rc::default();
    let queue = Rc::clone(&inbox);
    h.state_mut().os_events = Some(Box::new(move || queue.borrow_mut().drain(..).collect()));
    Rig { h, inbox, asked }
}

impl Rig {
    /// The system asks to open `path`, as Finder or Open With does, and the frame that hears it.
    fn open(&mut self, path: &str) {
        self.inbox.borrow_mut().push(OsEvent::Open(path.into()));
        self.h.step();
    }

    /// Frames a few milliseconds apart until the import is over: the reader runs on a thread of its own.
    fn done(&mut self) {
        for _ in 0..2000 {
            if !self.h.state().importing() {
                return;
            }
            self.h.step();
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("gave up waiting for the import");
    }

    fn status(&self) -> Status {
        self.h.state().status.clone()
    }

    fn project(&self) -> String {
        self.h.state().session.model.name.clone()
    }
}

#[test]
fn a_build_without_the_reader_offers_no_revit_files_and_says_why_when_given_one() {
    assert!(!open_extensions(false).contains(&"rvt") && open_extensions(false).contains(&"ifc"));
    assert!(open_extensions(true).contains(&"rvt") && open_extensions(true).contains(&"bimcraft"));
    let mut h = app(BOX);
    assert!(h.state().revit.is_none(), "a window starts without one: the desktop's main attaches it");
    let model = Arc::clone(&h.state().session.model);
    assert!(!h.state_mut().open_path(Path::new("/nowhere/house.rvt")));
    let status = h.state().status.clone();
    assert!(status.error && status.text.starts_with("house.rvt: this build of BIMCraft was made without Revit support"), "{status:?}");
    assert!(Arc::ptr_eq(&model, &h.state().session.model), "the project is as it was");
}

#[test]
fn a_revit_file_the_system_opens_replaces_the_project_with_a_new_unsaved_one() {
    let mut r = rig(|| Ok(survey(&[])));
    let recent = std::env::temp_dir().join(format!("bimcraft-ui-revit-{}-opens.json", std::process::id()));
    r.h.state_mut().keep_recent_in(recent.clone());
    assert_eq!(walls(&r.h).len(), 4);
    r.open("/projects/house.rvt");
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(walls(&r.h).len(), 1, "the project that came in");
    assert!(r.h.state().session.path.is_none(), "saving it asks for a .bimcraft name: nothing is written back to the .rvt");
    assert!(!r.h.state().dirty() && r.h.state().session.history().is_empty());
    assert_eq!(*r.asked.lock().unwrap(), [PathBuf::from("/projects/house.rvt")]);
    assert!(r.h.state().recent.paths().is_empty() && !recent.exists(), "a Revit file does not go into File > Open Recent");
    let status = r.status();
    assert!(!status.error && status.text.starts_with("Opened from Revit 2024: "), "{status:?}");
}

/// A file dropped on the window, as the windowing layer hands it over.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("a test's file is not read".into())
    }
}

#[test]
fn a_file_dropped_on_the_window_opens_like_one_the_system_asked_for() {
    let mut r = rig(|| Ok(survey(&[])));
    r.h.input_mut().dropped_files.push(Arc::new(Dropped("/projects/dropped.rvt".into())));
    r.h.step();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(*r.asked.lock().unwrap(), [PathBuf::from("/projects/dropped.rvt")]);
}

#[test]
fn a_revit_file_named_on_the_command_line_is_opened_once_the_window_is_up() {
    let mut r = rig(|| Ok(survey(&[])));
    r.h.state_mut().open_later(PathBuf::from("/projects/house.rvt"));
    assert!(r.asked.lock().unwrap().is_empty(), "nothing is read before the first frame: the window comes first");
    r.h.step();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(r.asked.lock().unwrap().len(), 1);
    r.h.step();
    r.h.step();
    assert_eq!(r.asked.lock().unwrap().len(), 1, "and only once");
}

#[test]
fn unsaved_changes_are_asked_about_before_a_revit_file_replaces_the_project() {
    let mut r = rig(|| Ok(survey(&[])));
    r.h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [0, 9000]]})).unwrap();
    assert!(r.h.state().dirty());
    r.open("/projects/house.rvt");
    r.h.step();
    assert!(r.h.query_by_label_contains("Save changes to").is_some(), "asked, not dropped");
    assert!(r.asked.lock().unwrap().is_empty(), "and nothing is read before the answer");
    press(&mut r.h, "Cancel");
    assert!(r.asked.lock().unwrap().is_empty() && r.project() != "Survey", "Cancel leaves the project and reads nothing");
    r.open("/projects/house.rvt");
    r.h.step();
    r.h.get_by_label("Don't Save").click();
    r.h.step();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(r.asked.lock().unwrap().len(), 1);
}

#[test]
fn a_file_the_reader_refuses_is_said_in_the_window_and_the_project_stays() {
    let mut r = rig(|| Err("not a Revit file: its container is damaged".into()));
    let model = Arc::clone(&r.h.state().session.model);
    r.open("/projects/bad.rvt");
    r.done();
    let status = r.status();
    assert!(status.error && status.text == "Couldn't open bad.rvt: not a Revit file: its container is damaged", "{status:?}");
    assert!(Arc::ptr_eq(&model, &r.h.state().session.model) && !r.h.state().dirty());
}

#[test]
fn the_reader_stays_attached_after_a_project_comes_in() {
    let mut r = rig(|| Ok(survey(&[])));
    r.open("/projects/a.rvt");
    r.done();
    r.open("/projects/b.rvt");
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 2, "a new window state would have lost it");
    // And New, which starts over too, keeps it.
    press(&mut r.h, "File");
    r.h.get_by_label_contains("New").click();
    r.h.run();
    r.open("/projects/c.rvt");
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 3);
}

/// A reader that waits for the test to release it, as a big file does, and then brings `survey`: send once for each file.
fn gated() -> (Rig, std::sync::mpsc::Sender<()>) {
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let r = rig(move || {
        gate.lock().map_err(|e| e.to_string())?.recv_timeout(Duration::from_secs(20)).map_err(|e| e.to_string())?;
        Ok(survey(&[]))
    });
    (r, release)
}

#[test]
fn while_a_revit_file_is_read_the_window_says_so_stays_alive_and_holds_still() {
    let (mut r, release) = gated();
    let model = Arc::clone(&r.h.state().session.model);
    let started = std::time::Instant::now();
    r.open("/projects/house.rvt");
    r.h.step();
    assert!(r.h.state().importing());
    assert!(r.h.query_by_label("Opening a Revit file: house.rvt").is_some(), "the box says what is going on");
    for _ in 0..5 {
        r.h.step();
    }
    assert!(started.elapsed() < Duration::from_secs(10), "frames ran while the reader waited: the window is not held by it");
    // Nothing starts or changes under the box: a new project, an undo, a second file.
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    r.h.step();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    r.h.step();
    assert!(Arc::ptr_eq(&model, &r.h.state().session.model) && r.project() != "Survey", "the project is as it was until the file is in");
    r.open("/projects/second.rvt");
    r.h.step();
    assert_eq!(r.asked.lock().unwrap().len(), 1, "a file is not read twice at once: the second waits");
    // It comes in, and then the file that waited, the last one asked for, is read in its turn.
    release.send(()).unwrap();
    release.send(()).unwrap();
    r.done();
    assert_eq!(r.project(), "Survey");
    assert_eq!(*r.asked.lock().unwrap(), [PathBuf::from("/projects/house.rvt"), PathBuf::from("/projects/second.rvt")]);
    assert!(r.h.query_by_label_contains("Opening a Revit file").is_none(), "and the box goes with the last");
}

#[test]
fn quitting_does_not_wait_for_a_revit_file_to_be_read() {
    let (mut r, release) = gated();
    r.open("/projects/house.rvt");
    r.inbox.borrow_mut().push(OsEvent::Quit);
    r.h.step();
    assert!(r.h.state().importing() && r.h.state().closing, "the system's Quit is heard, and the window closes");
    release.send(()).unwrap();
}

#[test]
fn a_reader_that_panics_is_a_message_and_the_window_goes_on() {
    let mut r = rig(|| panic!("boom"));
    let model = Arc::clone(&r.h.state().session.model);
    r.open("/projects/bad.rvt");
    r.done();
    let status = r.status();
    assert!(status.error && status.text == "Couldn't open bad.rvt: the Revit reader stopped on this file (boom)", "{status:?}");
    assert!(Arc::ptr_eq(&model, &r.h.state().session.model));
    assert!(r.h.query_by_label_contains("Opening a Revit file").is_none(), "the box is gone, and another file can be opened");
    r.open("/projects/again.rvt");
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 2);
}

/// What an import that left things out says: two groups left out (7 elements) and one note about an element that came in.
const NOTES: [&str; 3] = [
    "2 walls left out, their type has no layers (ids 7, 9)",
    "5 rooms left out, their walls do not enclose them (ids 1, 2, 3, 4, 5)",
    "1 window: narrowed to fit its neighbour (ids 12)",
];

#[test]
fn the_status_box_says_how_many_elements_came_in_and_how_many_were_left_out() {
    let mut r = rig(|| Ok(survey(&NOTES)));
    r.open("/projects/house.rvt");
    r.done();
    let n = r.h.state().session.model.elements.len();
    assert_eq!(r.status().text, format!("Opened from Revit 2024: {n} elements, 7 left out."));
    // One element is "1 element"; none left out is none.
    let mut r = rig(|| {
        let mut i = survey(&[]);
        i.model.elements.retain(|_, e| matches!(e, bimcraft_core::model::Element::Wall(_)));
        Ok(i)
    });
    r.open("/projects/house.rvt");
    r.done();
    assert_eq!(r.status().text, "Opened from Revit 2024: 1 element, 0 left out.");
}

#[test]
fn what_was_left_out_is_listed_in_a_report_that_closes_and_comes_back_from_the_file_menu() {
    let mut r = rig(|| Ok(survey(&NOTES)));
    r.open("/projects/house.rvt");
    r.done();
    r.h.run();
    for text in [
        "Revit import report",
        "house.rvt · Revit 2024",
        "Left out: 7",
        "2 walls left out, their type has no layers",
        "ids 7, 9",
        "5 rooms left out, their walls do not enclose them",
        "ids 1, 2, 3, 4, 5",
        "Noted",
        "1 window: narrowed to fit its neighbour",
        "ids 12",
        "1 Wall",
    ] {
        assert!(r.h.query_by_label_contains(text).is_some(), "the report shows `{text}`");
    }
    assert!(r.h.query_by_label_contains("Came in: ").is_some());
    // Closed, it is gone; File > Revit import report brings it back.
    press(&mut r.h, "Close");
    assert!(r.h.query_by_label_contains("Left out: 7").is_none());
    press(&mut r.h, "File");
    r.h.get_by_label_contains("Revit import report").click();
    r.h.run();
    assert!(r.h.query_by_label_contains("Left out: 7").is_some());
    press(&mut r.h, "Close");
    // Another project replaces it, and the report is not about that one.
    press(&mut r.h, "File");
    r.h.get_by_label_contains("New").click();
    r.h.run();
    press(&mut r.h, "File");
    assert!(r.h.query_by_label_contains("Revit import report").is_none(), "the menu item goes with the project it was about");
}

#[test]
fn an_import_that_left_nothing_out_opens_no_report_but_has_one_to_read() {
    let mut r = rig(|| Ok(survey(&[])));
    r.open("/projects/house.rvt");
    r.done();
    r.h.run();
    assert!(r.h.query_by_label_contains("Came in: ").is_none(), "nothing pops up over a clean import");
    press(&mut r.h, "File");
    r.h.get_by_label_contains("Revit import report").click();
    r.h.run();
    assert!(r.h.query_by_label_contains("Nothing was left out.").is_some());
    assert!(r.h.query_by_label_contains("Noted").is_none());
}

#[test]
fn the_report_speaks_the_interface_language_and_keeps_the_importers_notes_as_they_are() {
    let mut r = rig(|| Ok(survey(&NOTES)));
    r.h.state_mut().lang = crate::lang::Lang::Portuguese;
    r.open("/projects/house.rvt");
    r.done();
    r.h.run();
    for text in ["Relatório de importação do Revit", "Deixados de fora: 7", "Anotações", "1 Parede"] {
        assert!(r.h.query_by_label_contains(text).is_some(), "`{text}`");
    }
    assert!(r.h.query_by_label_contains("2 walls left out, their type has no layers").is_some(), "the core's notes stay English");
    assert!(r.h.query_by_label_contains("Left out: ").is_none() && r.h.query_by_label_contains("Came in").is_none());
}

#[test]
fn counting_what_a_note_says_was_left_out() {
    use crate::revit::count_left_out;
    assert_eq!(count_left_out("2 walls left out, their type has no layers (ids 7, 9)"), Some(2));
    assert_eq!(count_left_out("1 roof left out, it is by extrusion (ids 243274)"), Some(1));
    assert_eq!(count_left_out("1 window: narrowed to fit its neighbour (ids 12)"), None, "it came in");
    assert_eq!(count_left_out("3 levels with no name or elevation the reader can find (its block is missing or damaged)"), None);
    assert_eq!(count_left_out("many walls left out, whatever"), None, "no count is no count");
    assert_eq!(count_left_out(""), None);
    assert_eq!(count_left_out("2 windows: the one left of the door was narrowed, to fit (ids 3)"), None, "`left` is not `left out`");
}

#[test]
fn a_note_is_its_text_and_its_ids_and_a_note_without_ids_is_all_text() {
    use crate::revit::split_ids;
    assert_eq!(split_ids("2 walls left out, no layers (ids 7, 9)"), ("2 walls left out, no layers", Some("ids 7, 9")));
    assert_eq!(split_ids("1 window: narrowed to fit (the big one)"), ("1 window: narrowed to fit (the big one)", None));
    assert_eq!(split_ids("the file has no levels"), ("the file has no levels", None));
    assert_eq!(split_ids("5 doors (ids 1, …)"), ("5 doors", Some("ids 1, …")), "a list that goes on");
}

#[test]
fn what_came_in_is_listed_the_most_numerous_first() {
    let mut r = rig(|| Ok(survey(&[])));
    r.open("/projects/house.rvt");
    r.done();
    let counts = r.h.state().revit_report.as_ref().unwrap().counts.clone();
    let levels = r.h.state().session.model.levels().len();
    assert_eq!(counts.first(), Some(&("level".to_string(), levels)), "{counts:?}");
    assert!(counts.windows(2).all(|w| w[0].1 >= w[1].1), "{counts:?}");
}

#[test]
fn the_report_as_text_has_everything_the_window_shows_however_long_it_is() {
    let notes: Vec<String> =
        (0..120).map(|k| format!("1 door left out, door {k} and door {} overlap in wall 34 (ids {})", k + 1, 1000 + k)).collect();
    let all = notes.clone();
    let mut r = rig(move || Ok(survey(&notes.iter().map(String::as_str).collect::<Vec<_>>())));
    r.open("/projects/house.rvt");
    r.done();
    let copied = r.h.state().revit_report.as_ref().unwrap().text();
    assert!(copied.starts_with("house.rvt · Revit 2024\n\nCame in: "), "{copied}");
    assert!(copied.contains("\nLeft out: 120\n"), "{copied}");
    for note in &all {
        assert!(copied.contains(&format!("• {note}\n")), "the note `{note}` with its ids, on one line");
    }
}

#[test]
fn copy_report_says_it_was_pressed_with_a_long_list_under_it_too() {
    let notes: Vec<String> =
        (0..120).map(|k| format!("1 door left out, door {k} and door {} overlap in wall 34 (ids {})", k + 1, 1000 + k)).collect();
    let mut r = rig(move || Ok(survey(&notes.iter().map(String::as_str).collect::<Vec<_>>())));
    r.open("/projects/house.rvt");
    r.done();
    // The first frames of a window are laid out smaller.
    r.h.run();
    r.h.run();
    assert!(r.h.query_by_label("Copy Report").is_some() && r.h.query_by_label("Copied").is_none());
    // The accessibility click: a pointer click at the button's middle misses it in this harness once the list is this long.
    r.h.get_by_label("Copy Report").click_accesskit();
    r.h.run();
    let (copied, report) = (r.h.query_by_label("Copied").is_some(), r.h.query_by_label("Copy Report").is_some());
    assert!(copied && !report, "the button says it was pressed: Copied {copied}, Copy Report {report}");
    assert!(r.h.state().report_open(), "and the report stays up");
}

#[test]
fn a_long_report_scrolls_inside_the_window_and_leaves_its_close_button_on_screen() {
    let notes: Vec<String> =
        (0..120).map(|k| format!("1 door left out, door {k} and door {} overlap in wall 34 (ids {})", k + 1, 1000 + k)).collect();
    let mut r = rig(move || {
        let mut i = survey(&[]);
        i.skipped = notes.clone();
        Ok(i)
    });
    r.open("/projects/house.rvt");
    r.done();
    r.h.run();
    r.h.run();
    let close = r.h.get_by_label("Close").rect();
    let window = r.h.ctx.content_rect();
    // Centred and about two thirds as tall as the window: not the whole of it, and nothing of the window's own is lost under it.
    assert!(close.bottom() < window.height() * 0.92 && close.bottom() > window.height() * 0.5, "Close at {close:?} in a window {window:?}");
    assert!(r.h.query_by_label_contains("Left out: 120").is_some());
}

#[test]
fn a_second_file_asked_for_while_one_is_read_is_not_started_and_of_two_at_once_the_last_wins() {
    let (mut r, release) = gated();
    assert!(r.h.state_mut().open_path(Path::new("/projects/first.rvt")));
    assert!(!r.h.state_mut().open_path(Path::new("/projects/second.rvt")), "one at a time, whoever asks");
    release.send(()).unwrap();
    r.done();
    assert_eq!(r.asked.lock().unwrap().len(), 1);
    // Two asked for in one frame: the first is not read at all.
    let (mut r, release) = gated();
    release.send(()).unwrap();
    r.inbox.borrow_mut().push(OsEvent::Open("/projects/a.rvt".into()));
    r.inbox.borrow_mut().push(OsEvent::Open("/projects/b.rvt".into()));
    r.h.step();
    r.done();
    assert_eq!(*r.asked.lock().unwrap(), [PathBuf::from("/projects/b.rvt")]);
}

#[test]
fn while_the_report_is_up_the_keys_belong_to_it() {
    let mut r = rig(|| Ok(survey(&NOTES)));
    r.open("/projects/house.rvt");
    r.done();
    r.h.run();
    assert!(r.h.state().report_open());
    let model = Arc::clone(&r.h.state().session.model);
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    r.h.step();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    r.h.step();
    assert!(Arc::ptr_eq(&model, &r.h.state().session.model) && r.project() == "Survey", "Ctrl+N started a project under the report");
}
