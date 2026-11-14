//! A benchmark of the real window (`bimcraft --bench SCENARIO`): a scenario of steps a user takes (open a file, wait for it,
//! zoom and pan the plan, open the 3D view and orbit it, take a screenshot), driven through egui's raw input as the user's
//! wheel and mouse are, with every frame of the window timed. What a headless test measures is the core's work alone; this
//! is what the user waits for: the frames the window shows, on its own thread, with the card, the workers and the
//! compositor as they are.
//!
//! A scenario is a text file, a step a line (`#` begins a comment):
//!
//! ```text
//! open /path/to/tower.bimc     open a project, as File > Open does
//! wait 600                     until nothing is being made (the open, a plan, the 3D scene) for a few frames, at most 600 s
//! zoom in 10                   ten notches of the wheel over the middle of the view, a frame apart (`out` the other way)
//! pan 400 0 20                 a middle-button drag 400 px right over 20 frames
//! view 3d                      the default 3D view (`view plan` the first plan)
//! orbit 60                     Shift and the middle button dragged 6 px a frame for 60 frames
//! idle 2                       two seconds of frames with nothing asked
//! screenshot /tmp/plan.png     the window as it shows now
//! quit
//! ```
//!
//! The report (`--bench-out FILE`, JSON) has each step's time, its frames' times (the time between frames, which holds
//! everything the user waits for, and the window's own work in the frame), the memory the process had allocated and the
//! longest frame; a summary goes to standard error as each step ends.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Context, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Vec2, vec2};
use serde_json::{Value, json};

use crate::BimApp;

/// How many frames in a row nothing may be in the making for a wait to end.
const SETTLED_FRAMES: u32 = 5;

/// One step of a scenario.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Open(PathBuf),
    Wait(Duration),
    Zoom { notches: u32, inward: bool },
    Pan { dx: f32, dy: f32, frames: u32 },
    View3d,
    ViewPlan,
    Orbit(u32),
    Idle(Duration),
    Screenshot(PathBuf),
    Quit,
}

/// A scenario read from its text: an error names the line that isn't a step.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(w, r)| (w, r.trim()));
        let bad = |why: &str| format!("line {}: `{line}`: {why}", n + 1);
        let numbers = |want: usize| -> Result<Vec<f64>, String> {
            let v: Vec<f64> = rest.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|_| bad("numbers expected"))?;
            if v.len() != want || v.iter().any(|x| !x.is_finite() || x.abs() > 1e6) {
                return Err(bad(&format!("{want} numbers expected")));
            }
            Ok(v)
        };
        let count = |x: f64| -> Result<u32, String> {
            if (1.0..=100_000.0).contains(&x) && x.fract() == 0.0 { Ok(x as u32) } else { Err(bad("a count from 1 to 100000 expected")) }
        };
        let seconds = |x: f64| -> Result<Duration, String> {
            if (0.0..=86_400.0).contains(&x) { Ok(Duration::from_secs_f64(x)) } else { Err(bad("seconds from 0 to 86400 expected")) }
        };
        out.push(match word {
            "open" if !rest.is_empty() => Step::Open(PathBuf::from(rest)),
            "screenshot" if !rest.is_empty() => Step::Screenshot(PathBuf::from(rest)),
            "wait" if rest.is_empty() => Step::Wait(Duration::from_secs(3600)),
            "wait" => Step::Wait(seconds(numbers(1)?[0])?),
            "idle" => Step::Idle(seconds(numbers(1)?[0])?),
            "zoom" => {
                let (way, n) = rest.split_once(char::is_whitespace).map_or((rest, "1"), |(w, n)| (w, n.trim()));
                let inward = match way {
                    "in" => true,
                    "out" => false,
                    _ => return Err(bad("`zoom in N` or `zoom out N`")),
                };
                Step::Zoom { notches: count(n.parse().map_err(|_| bad("a count expected"))?)?, inward }
            }
            "pan" => {
                let v = numbers(3)?;
                Step::Pan { dx: v[0] as f32, dy: v[1] as f32, frames: count(v[2])? }
            }
            "orbit" => Step::Orbit(count(numbers(1)?[0])?),
            "view" if rest == "3d" => Step::View3d,
            "view" if rest == "plan" => Step::ViewPlan,
            "quit" if rest.is_empty() => Step::Quit,
            _ => return Err(bad("not a step: open, wait, zoom, pan, view, orbit, idle, screenshot or quit")),
        });
    }
    Ok(out)
}

/// A frame as the benchmark saw it.
#[derive(Clone, Copy, Debug)]
struct Frame {
    /// From the start of the frame before to the start of this one: what the user waited for it.
    interval: f64,
    /// What the window's own pass took, by eframe's count (the frame before's).
    cpu: f64,
    /// What the process held in memory, bytes ([`resident`]).
    memory: Option<usize>,
}

/// A step done, for the report.
struct Done {
    step: String,
    seconds: f64,
    frames: Vec<Frame>,
    note: Option<String>,
}

/// A scenario being run on a window.
pub struct Bench {
    steps: Vec<Step>,
    at: usize,
    /// When the step under way began, its frames, and how far through it is (wheel notches, drag frames).
    began: Instant,
    frames: Vec<Frame>,
    progress: u32,
    settled: u32,
    /// The frame before's start.
    last: Option<Instant>,
    done: Vec<Done>,
    out: Option<PathBuf>,
    /// What the step wants of this frame's input, put in by [`Bench::input`].
    events: Vec<Event>,
    /// Where the pointer is, in points: the middle of the view, once the window is laid out.
    pointer: Option<Pos2>,
    /// A screenshot asked for, to be written where.
    shot: Option<PathBuf>,
    started: Instant,
    /// What measures the process's memory: the core's meter, where the app installed one.
    memory: fn() -> Option<usize>,
    pub(crate) finished: bool,
}

impl Bench {
    pub fn new(steps: Vec<Step>, out: Option<PathBuf>) -> Bench {
        let now = Instant::now();
        Bench {
            steps,
            at: 0,
            began: now,
            frames: Vec::new(),
            progress: 0,
            settled: 0,
            last: None,
            done: Vec::new(),
            out,
            events: Vec::new(),
            pointer: None,
            shot: None,
            started: now,
            memory: resident,
            finished: false,
        }
    }

    /// The scenario at `path`, with its report to go to `out`.
    pub fn read(path: &Path, out: Option<PathBuf>) -> Result<Bench, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Bench::new(parse(&text).map_err(|e| format!("{}: {e}", path.display()))?, out))
    }

    /// This frame's input: what the step under way does with the wheel and the mouse, and the screenshot it waits for.
    pub(crate) fn input(&mut self, raw: &mut RawInput) {
        if let Some(path) = &self.shot {
            for e in &raw.events {
                if let Event::Screenshot { image, .. } = e {
                    let [w, h] = image.size;
                    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    let note = match bimcraft_core::render::rgba_png(w as u32, h as u32, &rgba)
                        .and_then(|png| std::fs::write(path, png).map_err(|e| e.to_string()))
                    {
                        Ok(()) => format!("{} ({w} x {h})", path.display()),
                        Err(e) => format!("not written: {e}"),
                    };
                    self.finish(Some(note));
                    break;
                }
            }
        }
        raw.events.append(&mut self.events);
    }

    /// Runs the step under way for this frame, on the window, before its pass.
    pub(crate) fn step(&mut self, app: &mut BimApp, ctx: &Context, cpu: f32) {
        let now = Instant::now();
        if let Some(last) = self.last {
            let memory = (self.memory)();
            self.frames.push(Frame { interval: (now - last).as_secs_f64(), cpu: f64::from(cpu), memory });
        }
        self.last = Some(now);
        // Every frame is drawn while a scenario runs, as if the user kept moving.
        ctx.request_repaint();
        let centre = ctx.content_rect().center() + vec2(0.0, 40.0);
        let pointer = *self.pointer.get_or_insert(centre);
        let Some(step) = self.steps.get(self.at).cloned() else {
            if !self.finished {
                self.finished = true;
                self.report();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        };
        let elapsed = now - self.began;
        match step {
            Step::Open(path) => {
                let opened = app.open_path(&path);
                self.finish(Some(if opened { format!("{} opened", path.display()) } else { format!("{} not opened", path.display()) }));
            }
            Step::Wait(most) => {
                self.settled = if app.settled() { self.settled + 1 } else { 0 };
                if self.settled >= SETTLED_FRAMES {
                    self.finish(None);
                } else if elapsed > most {
                    self.finish(Some(format!("still busy after {} s", most.as_secs())));
                }
            }
            Step::Zoom { notches, inward } => {
                self.events.push(Event::PointerMoved(pointer));
                if self.progress < notches {
                    let y = if inward { 1.0 } else { -1.0 };
                    self.events.push(Event::MouseWheel {
                        unit: MouseWheelUnit::Line,
                        delta: vec2(0.0, y),
                        modifiers: Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    });
                    self.progress += 1;
                } else {
                    self.finish(None);
                }
            }
            Step::Pan { dx, dy, frames } => self.drag(pointer, vec2(dx, dy) / frames as f32, frames, Modifiers::NONE),
            Step::Orbit(frames) => self.drag(pointer, vec2(6.0, 0.0), frames, Modifiers::SHIFT),
            Step::View3d => {
                app.open_3d(None);
                self.finish(None);
            }
            Step::ViewPlan => {
                app.open_first_plan();
                self.finish(None);
            }
            Step::Idle(d) => {
                if elapsed >= d {
                    self.finish(None);
                }
            }
            Step::Screenshot(path) => {
                if self.shot.is_none() {
                    self.shot = Some(path);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                } else if elapsed > Duration::from_secs(10) {
                    self.finish(Some("the window gave no screenshot".into()));
                }
            }
            Step::Quit => {
                self.steps.truncate(self.at);
            }
        }
    }

    /// A drag with the middle button of `frames` frames, `by` points a frame, from `from`, with `modifiers` held.
    fn drag(&mut self, from: Pos2, by: Vec2, frames: u32, modifiers: Modifiers) {
        let p = self.progress;
        let at = |i: u32| from + by * i as f32;
        if p == 0 {
            self.events.push(Event::ModifiersChanged(modifiers));
            self.events.push(Event::PointerMoved(from));
            self.events.push(Event::PointerButton { pos: from, button: PointerButton::Middle, pressed: true, modifiers });
        } else if p <= frames {
            self.events.push(Event::PointerMoved(at(p)));
        } else {
            self.events.push(Event::PointerButton { pos: at(frames), button: PointerButton::Middle, pressed: false, modifiers });
            self.events.push(Event::ModifiersChanged(Modifiers::NONE));
            self.finish(None);
            return;
        }
        self.progress += 1;
    }

    /// The step under way is done: said, kept for the report, and the next one begins.
    fn finish(&mut self, note: Option<String>) {
        let step = self.steps.get(self.at).map_or_else(String::new, |s| format!("{s:?}"));
        let seconds = self.began.elapsed().as_secs_f64();
        let frames = std::mem::take(&mut self.frames);
        eprintln!("bench: {}", line(&step, seconds, &frames, note.as_deref()));
        self.done.push(Done { step, seconds, frames, note });
        self.at += 1;
        self.began = Instant::now();
        (self.progress, self.settled, self.shot) = (0, 0, None);
    }

    /// The report, written where it was asked to go.
    fn report(&self) {
        let steps: Vec<Value> = self
            .done
            .iter()
            .map(|d| {
                let ms: Vec<f64> = d.frames.iter().map(|f| f.interval * 1000.0).collect();
                let cpu: Vec<f64> = d.frames.iter().map(|f| f.cpu * 1000.0).collect();
                json!({
                    "step": d.step,
                    "seconds": d.seconds,
                    "note": d.note,
                    "frames": ms.len(),
                    "frame_ms": stats(&ms),
                    "cpu_ms": stats(&cpu),
                    "memory_peak_mb": d.frames.iter().filter_map(|f| f.memory).max().map(|b| b as f64 / 1e6),
                    "frame_intervals_ms": ms,
                })
            })
            .collect();
        let report = json!({"seconds": self.started.elapsed().as_secs_f64(), "steps": steps});
        if let Some(out) = &self.out {
            match serde_json::to_string_pretty(&report) {
                Ok(text) => {
                    if let Err(e) = std::fs::write(out, text) {
                        eprintln!("bench: the report was not written to {}: {e}", out.display());
                    }
                }
                Err(e) => eprintln!("bench: {e}"),
            }
        }
        eprintln!("bench: done in {:.1} s", self.started.elapsed().as_secs_f64());
    }
}

/// What the process holds in memory now, bytes: its resident set where the system says (Linux), else what it has allocated by the
/// core's meter.
fn resident() -> Option<usize> {
    let pages = std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1)?.parse::<usize>().ok());
    pages.map(|p| p * 4096).or_else(bimcraft_core::memory::used)
}

/// The median, the 95th percentile and the longest of `ms`.
fn stats(ms: &[f64]) -> Value {
    if ms.is_empty() {
        return Value::Null;
    }
    let mut v = ms.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    json!({"median": at(0.5), "p95": at(0.95), "max": at(1.0)})
}

fn line(step: &str, seconds: f64, frames: &[Frame], note: Option<&str>) -> String {
    let mut ms: Vec<f64> = frames.iter().map(|f| f.interval * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    let pick = |q: f64| ms.get(((ms.len().max(1) - 1) as f64 * q).round() as usize).copied().unwrap_or(0.0);
    let memory = frames.iter().filter_map(|f| f.memory).max().map_or_else(String::new, |b| format!(", {:.0} MB", b as f64 / 1e6));
    let note = note.map_or_else(String::new, |n| format!(" — {n}"));
    format!(
        "{step}: {seconds:.2} s, {} frames, frame median {:.1} ms, p95 {:.1} ms, longest {:.1} ms{memory}{note}",
        frames.len(),
        pick(0.5),
        pick(0.95),
        pick(1.0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scenario_reads_its_steps_and_says_which_line_is_not_one() {
        let s = parse("open /a b.bimc\n# a comment\nwait 60\nzoom in 3\nzoom out 2\npan 10 -5 4\nview 3d\norbit 9\nidle 1.5\nscreenshot x.png\nquit\n")
            .unwrap();
        assert_eq!(
            s,
            vec![
                Step::Open("/a b.bimc".into()),
                Step::Wait(Duration::from_secs(60)),
                Step::Zoom { notches: 3, inward: true },
                Step::Zoom { notches: 2, inward: false },
                Step::Pan { dx: 10.0, dy: -5.0, frames: 4 },
                Step::View3d,
                Step::Orbit(9),
                Step::Idle(Duration::from_secs_f64(1.5)),
                Step::Screenshot("x.png".into()),
                Step::Quit,
            ]
        );
        for bad in ["jump 3", "zoom sideways 2", "orbit 0", "orbit -1", "wait forever", "pan 1 2", "idle 1e9", "view 4d"] {
            let e = parse(&format!("wait\n{bad}")).unwrap_err();
            assert!(e.starts_with("line 2:"), "{bad}: {e}");
        }
    }
}
