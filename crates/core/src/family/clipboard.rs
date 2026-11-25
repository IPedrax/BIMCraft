//! A family's clipboard. Copy to Clipboard (Ctrl+C) and Cut to Clipboard (Ctrl+X) take what is selected ([`Family::clip`]);
//! Paste (Ctrl+V) and Paste Aligned put copies of it back ([`Family::paste`], [`Family::paste_into_view`]), each paste with new
//! GUIDs of its own, so the clipboard pastes again and again. The copies are a Ctrl+drag's ([`Family::copies`]): what each is
//! drawn on is the copy of it where that is copied too, else what the original is drawn on; a dimension comes along where
//! every reference of it is copied, one selected or one that holds a constraint among the copies (a label, a lock, EQ,
//! Align's lock), and one that ties what is copied to what isn't is left behind ([`Clip::dropped`] counts those). What can't
//! be copied (a datum, the plane that defines the origin, what a pivot turns) refuses the copy as a Ctrl+drag's does. The
//! copies are held as a family of their own, apart from the document they came from, so the clipboard stays what was copied
//! whatever the document does after.
//!
//! The clipboard holds elements or, while a form is sketched, a sketch's lines ([`SketchClip`]): lines pasted into the same
//! sketch with new GUIDs, the locks and dimensions among them coming along and those to lines not copied dropped.
//!
//! Dimensions are drawn in a view (`DimView`): dimensions selected alone, measuring what isn't selected, are held as they are
//! ([`Clip::loose`]) and pasted into views (Aligned to Current View, Aligned to Selected Views), measuring the same references,
//! their labels, locks and EQ left behind, since those already hold in the view they were copied from.
//!
//! What is copied goes on the system clipboard as text, as a project's copy does ([`crate::clipboard`]), marked as a family's
//! ([`KIND`]) with where it was copied ([`Source`]): the window that copied it knows its own copy by that, and another window
//! knows it isn't its own ([`source`]). The text is the copy itself, held as the window holds it; a paste reads its source
//! alone, since a copy names the planes, parameters and nested families of the family it came from, and pasting into another
//! family isn't done yet. Nothing here is saved: the clipboard is the session's, no part of a family or a step of FORMAT.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use super::constraints::{Copies, CopyRefused, Copying, DimRef, DimView, FamilyDimension};
use super::{DATUMS, Family, PlaneRef};
use crate::geom::P2;
use crate::ids::Guid;
use crate::sketch::SketchLine;
use crate::view3d::P3;

/// What a family's clipboard text says it is: not a project's ([`crate::clipboard::KIND`]).
pub const KIND: &str = "bimcraft family clipboard";
/// The text's own format: one a newer BIMCraft wrote with a larger number is refused.
pub const VERSION: u32 = 1;

/// Where a family's clipboard text was copied: the family's name, and the copy's own GUID, by which the window that made it
/// knows it for its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub family: String,
    pub copy: Guid,
}

/// What a copy holds, as its text says it: elements, or lines of a sketch.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Copied<'a> {
    Elements(&'a Clip),
    Lines(&'a SketchClip),
}

/// The clipboard's text of `copied`, copied as `source` says.
pub fn text(source: &Source, copied: Copied) -> Result<String, String> {
    let failed = |e: serde_json::Error| format!("the clipboard couldn't be written: {e}");
    let head = serde_json::to_string(source).map_err(failed)?;
    let body = serde_json::to_string(&copied).map_err(failed)?;
    Ok(format!("{{\"bimcraft\":\"{KIND}\",\"version\":{VERSION},\"source\":{head},\"copied\":{body}}}"))
}

/// Whether `text` looks like a family's clipboard, without reading it.
pub fn is_family_clipboard(text: &str) -> bool {
    crate::clipboard::marked(text, KIND)
}

#[derive(Deserialize)]
struct Raw<'a> {
    bimcraft: String,
    version: u32,
    source: Source,
    #[serde(borrow, rename = "copied")]
    _copied: &'a RawValue,
}

/// Where a family's clipboard text was copied, its copy left unread; or why it can't be read.
pub fn source(text: &str) -> Result<Source, String> {
    let max = crate::clipboard::MAX_CLIPBOARD;
    if text.len() > max {
        return Err(format!("the clipboard is too large ({} bytes; a paste takes one under {max})", text.len()));
    }
    let raw: Raw = serde_json::from_str(text).map_err(|e| format!("the clipboard holds no family's elements ({e})"))?;
    if raw.bimcraft != KIND {
        return Err("the clipboard holds no family's elements".into());
    }
    if raw.version > VERSION {
        return Err(format!("the clipboard was written by a newer BIMCraft (clipboard version {}); update to paste it", raw.version));
    }
    Ok(raw.source)
}

/// What a copy put on the clipboard.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Clip {
    /// The copies as a family of their own: its planes, lines, forms, symbols, connectors, nested instances and the
    /// dimensions among them; what they are drawn on and isn't copied named by its GUID in the family copied from.
    held: Family,
    /// Of `held`'s dimensions, those copied because they were selected (the rest hold constraints among the copies).
    selected: Vec<Guid>,
    /// Dimensions selected with nothing they measure: each as it was, its label, locks and EQ taken off. Only when nothing
    /// else was copied: dimensions alone are what is pasted into views.
    pub loose: Vec<FamilyDimension>,
    /// How many constraints (labels, locks, EQ, Align's locks) between what was copied and what wasn't were left behind.
    pub dropped: usize,
}

/// Why a paste is refused.
#[derive(Clone, Debug, PartialEq)]
pub enum PasteRefused {
    /// What a copy is drawn on isn't in the family any more (it was deleted, or cut along with nothing drawn on it): its GUID.
    Host(Guid),
    /// A dimension pasted into a view measures what the family no longer has.
    Gone,
    /// Model elements are pasted where they go, not into views; dimensions alone are pasted into views alone.
    Kind,
}

impl Clip {
    /// How many elements it holds: forms, planes, lines, symbols, connectors, nested instances and the dimensions selected.
    pub fn count(&self) -> usize {
        let h = &self.held;
        h.planes.len()
            + h.lines.len()
            + h.forms.len()
            + h.symbols.len()
            + h.connectors.len()
            + h.nested_instances.len()
            + self.selected.len()
            + self.loose.len()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Whether it holds model elements (forms, planes, lines, symbols, connectors, nested instances): pasted where they go.
    pub fn model(&self) -> bool {
        let h = &self.held;
        !(h.planes.is_empty()
            && h.lines.is_empty()
            && h.forms.is_empty()
            && h.symbols.is_empty()
            && h.connectors.is_empty()
            && h.nested_instances.is_empty())
    }

    /// Whether it holds view-specific elements alone, dimensions copied without what they measure: pasted into views.
    pub fn view_specific(&self) -> bool {
        !self.model() && !self.loose.is_empty()
    }

    /// The views the loose dimensions were drawn in.
    pub fn views(&self) -> BTreeSet<DimView> {
        self.loose.iter().map(|d| d.view).collect()
    }
}

/// Whether `host` is one of the family's: one of its planes, a datum, or one of its reference lines.
fn has_host(f: &Family, host: &PlaneRef) -> bool {
    match *host {
        PlaneRef::Plane(g) => DATUMS.contains(&g) || f.planes.iter().any(|p| p.guid == g),
        PlaneRef::Line(g, _) => f.lines.iter().any(|l| l.guid == g),
    }
}

impl Family {
    /// What `copying` takes, copied for the clipboard (a Ctrl+drag's copies, where the originals are), or why not. A dimension
    /// selected whose references aren't all copied is held loose when nothing else is copied (it is pasted into views), else
    /// left behind.
    pub fn clip(&self, copying: &Copying) -> Result<Clip, CopyRefused> {
        let copies = self.copies(copying)?;
        let reporting = |l: &str| self.reporting(l);
        // The originals copied, by GUID: what a dimension measures to.
        let copied: BTreeSet<Guid> = copies.guids.keys().copied().collect();
        let names_copied = |r: &DimRef| copied_ref(r, &copied);
        let mut dropped = 0;
        for (k, d) in self.dimensions.iter().enumerate() {
            if copies.dimensions_of.contains(&k) || !d.constrains(&reporting) {
                continue;
            }
            if d.refs.iter().any(names_copied) {
                dropped += 1;
            }
        }
        let held = Family {
            planes: copies.planes,
            lines: copies.lines,
            forms: copies.forms,
            symbols: copies.symbols,
            connectors: copies.connectors,
            nested_instances: copies.nested,
            dimensions: copies.dimensions,
            ..Family::default()
        };
        let mut clip = Clip { held, selected: copies.selected, loose: Vec::new(), dropped };
        if !clip.model() {
            for d in self.dimensions.iter().filter(|d| copying.dimensions.contains(&d.guid)) {
                if d.constrains(&reporting) {
                    clip.dropped += 1;
                }
                clip.loose.push(FamilyDimension { id: 0, label: None, locked: Vec::new(), equal: false, alignment: false, ..d.clone() });
            }
            // A plain dimension measures; an Align lock is only its padlock, which stays where it is.
            clip.loose.retain(|d| !d.refs.is_empty());
        }
        Ok(clip)
    }

    /// The clipboard's model elements, copied again with new GUIDs for this family, where they were copied: what is added
    /// (each list after the family's own) and, of its dimensions, those to select (`selected`, with the copies of the rest).
    /// Refused where what a copy is drawn on isn't the family's any more.
    pub fn paste(&self, clip: &Clip) -> Result<Copies, PasteRefused> {
        if !clip.model() {
            return Err(PasteRefused::Kind);
        }
        let h = &clip.held;
        let all = Copying {
            planes: h.planes.iter().map(|p| p.guid).collect(),
            lines: h.lines.iter().map(|l| l.guid).collect(),
            forms: (0..h.forms.len()).collect(),
            symbols: (0..h.symbols.len()).collect(),
            connectors: (0..h.connectors.len()).collect(),
            nested: (0..h.nested_instances.len()).collect(),
            dimensions: h.dimensions.iter().map(|d| d.guid).collect(),
        };
        // Copied once already, so nothing in it is a datum, the origin's plane or what a pivot turns.
        let mut out = h.copies(&all).map_err(|_| PasteRefused::Kind)?;
        out.selected = out
            .dimensions
            .iter()
            .zip(&out.dimensions_of)
            .filter(|(_, k)| h.dimensions.get(**k).is_some_and(|d| clip.selected.contains(&d.guid)))
            .map(|(d, _)| d.guid)
            .collect();
        // What each copy is drawn on: a copy, or the family's own.
        let own = |host: &PlaneRef| match host {
            PlaneRef::Plane(g) => out.planes.iter().any(|p| p.guid == *g),
            PlaneRef::Line(g, _) => out.lines.iter().any(|l| l.guid == *g),
        };
        let hosts = out.lines.iter().map(|l| l.host).chain(out.forms.iter().map(|f| f.host)).chain(out.nested.iter().map(|n| n.host));
        for host in hosts {
            if !own(&host) && !has_host(self, &host) {
                let (PlaneRef::Plane(g) | PlaneRef::Line(g, _)) = host;
                return Err(PasteRefused::Host(g));
            }
        }
        Ok(out)
    }

    /// The clipboard's loose dimensions, each with a new GUID, drawn in `view` and measuring what they measured; refused
    /// where the family no longer has it.
    pub fn paste_into_view(&self, clip: &Clip, view: DimView) -> Result<Vec<FamilyDimension>, PasteRefused> {
        if !clip.view_specific() {
            return Err(PasteRefused::Kind);
        }
        let pasted: Vec<FamilyDimension> =
            clip.loose.iter().map(|d| FamilyDimension { id: 0, guid: Guid::new(), view, ..d.clone() }).collect();
        let trial = Family { dimensions: pasted.clone(), ..self.clone() };
        if !trial.dangling_dimensions().is_empty() {
            return Err(PasteRefused::Gone);
        }
        Ok(pasted)
    }
}

/// Whether `r` names one of `copied`.
fn copied_ref(r: &DimRef, copied: &BTreeSet<Guid>) -> bool {
    match *r {
        DimRef::Plane(g) | DimRef::Form(g, _) | DimRef::Line(g, _) | DimRef::LinePoint(g, _) => copied.contains(&g),
        DimRef::Sketch(_, l) | DimRef::Point(_, l, _) => copied.contains(&l),
    }
}

/// What Copy put on the clipboard while a form was sketched: lines of one sketch (`form`, the form's GUID, and `part`, which
/// of its sketches), and the constraints among them.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SketchClip {
    pub form: Guid,
    pub part: usize,
    pub lines: Vec<SketchLine>,
    /// The dimensions and locks among the lines copied: each reference one of `lines`.
    pub constraints: Vec<FamilyDimension>,
    /// How many constraints between the lines copied and what wasn't were left behind.
    pub dropped: usize,
}

/// What a paste into a sketch adds: the lines and the constraints among them.
#[derive(Clone, Debug, Default)]
pub struct SketchPasted {
    pub lines: Vec<SketchLine>,
    pub constraints: Vec<FamilyDimension>,
}

impl SketchClip {
    /// Lines `lines` of part `part` of form `form`'s sketch copied, with `constraints` (the dimensions and locks in effect,
    /// `reporting` saying which labels hold nothing): those among the lines come along, those to what isn't copied are
    /// counted and left behind.
    pub fn of(
        form: Guid,
        part: usize,
        lines: &[SketchLine],
        constraints: &[FamilyDimension],
        reporting: &dyn Fn(&str) -> bool,
    ) -> SketchClip {
        let copied: BTreeSet<Guid> = lines.iter().map(|l| l.guid).filter(|g| !g.is_nil()).collect();
        let mine = |r: &DimRef| match *r {
            DimRef::Sketch(f, l) | DimRef::Point(f, l, _) => f == form && copied.contains(&l),
            _ => false,
        };
        let mut out = SketchClip { form, part, lines: lines.to_vec(), ..SketchClip::default() };
        for d in constraints.iter().filter(|d| d.constrains(reporting)) {
            if d.refs.iter().all(mine) {
                out.constraints.push(d.clone());
            } else if d.refs.iter().any(mine) {
                out.dropped += 1;
            }
        }
        out
    }

    /// Where the lines' middle is: the middle of the box round their points, in the sketch's plane.
    pub fn centre(&self) -> Option<P2> {
        let pts: Vec<P2> = self.lines.iter().flat_map(|l| l.curve.points()).collect();
        let lo = pts.iter().copied().reduce(|a, p| P2(a.0.min(p.0), a.1.min(p.1)))?;
        let hi = pts.iter().copied().reduce(|a, p| P2(a.0.max(p.0), a.1.max(p.1)))?;
        Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
    }

    /// The lines again with new GUIDs, moved `by` in the sketch's plane (`along`, the same in the family's axes, for where
    /// the dimensions' lines are drawn), and the constraints among them naming the new lines.
    pub fn paste(&self, by: P2, along: P3) -> SketchPasted {
        let mut renamed: Vec<(Guid, Guid)> = Vec::new();
        let lines = self
            .lines
            .iter()
            .map(|l| {
                let guid = Guid::new();
                if !l.guid.is_nil() {
                    renamed.push((l.guid, guid));
                }
                SketchLine { id: 0, guid, curve: l.curve.map(|q| q + by), ..l.clone() }
            })
            .collect();
        let new = |l: Guid| renamed.iter().find(|(a, _)| *a == l).map_or(l, |(_, b)| *b);
        let constraints = self
            .constraints
            .iter()
            .map(|d| {
                let refs = d
                    .refs
                    .iter()
                    .map(|r| match *r {
                        DimRef::Sketch(f, l) => DimRef::Sketch(f, new(l)),
                        DimRef::Point(f, l, s) => DimRef::Point(f, new(l), s),
                        other => other,
                    })
                    .collect();
                let at = std::array::from_fn(|k| d.at[k] + along[k]);
                FamilyDimension { id: 0, guid: Guid::new(), refs, at, ..d.clone() }
            })
            .collect();
        SketchPasted { lines, constraints }
    }
}
