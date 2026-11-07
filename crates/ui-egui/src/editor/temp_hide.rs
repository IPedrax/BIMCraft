//! Temporary Hide/Isolate in the family document's view, as the project's view has it (`footer.rs`) and Revit's family editor
//! does: Hide Element (HH), Hide Category (HC), Isolate Element (HI), Isolate Category (IC) and Reset (HR), from the view control
//! bar's menu or the keys. What is hidden is the view's for now: no part of the draft, so neither saved, nor a step of its
//! history, nor an unsaved change; it goes when the family closes. While it hides anything, the view wears the project's cyan
//! frame and the bar says so.
//!
//! A family has no categories of elements to hide as a project has, so the view's own stand for them, as Revit's family editor
//! names them: the forms (solid and void alike: they have no subcategory), the reference points, the lines (an adaptive
//! family's reference and model lines), the reference planes (the datums and the family's own planes, which the modelling view
//! draws) and, in the preview of a door or a window, the wall it stands in. Only forms, points and lines can be selected, so
//! only they are hidden by Hide Category; Isolate Category hides every other one the view draws, and Isolate Element the
//! reference planes or the wall too, as Revit's hides every element but those selected.
use super::*;
use crate::adaptive_view::Hidden;
use bimcraft_core::adaptive::{Definition, Operation as Node};
use std::collections::BTreeSet;

/// What the family document's view counts as a category, for Hide and Isolate Category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    /// Every form, solid or void.
    Forms,
    /// An adaptive family's reference and adaptive points.
    Points,
    /// An adaptive family's lines.
    Lines,
    /// The datums and the family's own reference planes, as the modelling view draws them.
    Planes,
    /// The wall a door's or a window's preview stands in.
    Host,
}

/// What the family document's view hides for now.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Temp {
    /// A family's forms, by their `tag` (one that isn't adaptive).
    forms: BTreeSet<u64>,
    /// An adaptive family's elements, by id.
    nodes: BTreeSet<String>,
    /// A family's reference planes, by GUID, selected in a plan or an elevation (F4).
    planes: BTreeSet<bimcraft_core::ids::Guid>,
    kinds: BTreeSet<Kind>,
}

impl Temp {
    /// Whether it hides anything: the view wears the frame.
    pub(crate) fn hides(&self) -> bool {
        !self.forms.is_empty() || !self.nodes.is_empty() || !self.planes.is_empty() || !self.kinds.is_empty()
    }
}

/// The category an adaptive family's element `n` is drawn as.
fn kind_of(n: &bimcraft_core::adaptive::Node) -> Kind {
    match n.operation {
        _ if n.is_form() => Kind::Forms,
        Node::Point { .. } => Kind::Points,
        _ => Kind::Lines,
    }
}

/// What the family document's temporary hide was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// Hide Element (HH), or with `category` Hide Category (HC).
    Hide { category: bool },
    /// Isolate Element (HI), or with `category` Isolate Category (IC).
    Isolate { category: bool },
    /// Reset Temporary Hide/Isolate (HR).
    Reset,
}

impl FamilyEditor {
    /// What the view hides for now.
    pub(crate) fn temporary(&self) -> &Temp {
        &self.document.temp
    }

    /// Whether something the view shows is selected: what Hide and Isolate act on.
    pub(crate) fn temp_picked(&self) -> bool {
        match &self.adaptive {
            Some(d) => self.modeller.selection.iter().any(|id| d.nodes.iter().any(|n| &n.id == id)),
            None => self.selected_form().is_some() || self.selected_plane().is_some(),
        }
    }

    /// Hide, Isolate or Reset, on what is selected; the selection goes, as the project's does. Why not, when nothing is.
    pub(crate) fn temp(&mut self, asked: Asked) -> Result<(), &'static str> {
        if asked == Asked::Reset {
            self.document.temp = Temp::default();
            return Ok(());
        }
        if !self.temp_picked() {
            return Err(msg!("select what to hide or isolate first"));
        }
        // What is selected, and its categories; then every element of the view and every category it draws.
        let (forms, nodes, kinds, every_form, every_node, every_kind): (BTreeSet<u64>, BTreeSet<String>, BTreeSet<Kind>, _, _, _) =
            match &self.adaptive {
                Some(d) => {
                    let picked: Vec<_> = d.nodes.iter().filter(|n| self.modeller.selection.contains(&n.id)).collect();
                    (
                        BTreeSet::new(),
                        picked.iter().map(|n| n.id.clone()).collect(),
                        picked.iter().map(|n| kind_of(n)).collect(),
                        BTreeSet::new(),
                        d.nodes.iter().map(|n| n.id.clone()).collect::<BTreeSet<_>>(),
                        BTreeSet::from([Kind::Forms, Kind::Points, Kind::Lines, Kind::Planes]),
                    )
                }
                None => {
                    let picked = self.selected_form().map(|i| self.forms[i].tag);
                    let host = if self.category.hosted() { vec![Kind::Host] } else { Vec::new() };
                    let kind = if self.selected_plane().is_some() { Kind::Planes } else { Kind::Forms };
                    (
                        picked.into_iter().collect(),
                        BTreeSet::new(),
                        BTreeSet::from([kind]),
                        self.forms.iter().map(|f| f.tag).collect::<BTreeSet<_>>(),
                        BTreeSet::new(),
                        host.into_iter().chain([Kind::Forms, Kind::Planes]).collect(),
                    )
                }
            };
        // A plane selected in a plan or an elevation, and every plane of the family's own (the datums are Planes alone).
        let plane: BTreeSet<bimcraft_core::ids::Guid> = self.selected_plane().into_iter().collect();
        let every_plane: BTreeSet<bimcraft_core::ids::Guid> = self.planes.iter().filter(|p| !p.level).map(|p| p.guid).collect();
        let t = &mut self.document.temp;
        match asked {
            Asked::Hide { category: false } => {
                t.forms.extend(forms);
                t.nodes.extend(nodes);
                t.planes.extend(plane);
            }
            Asked::Hide { category: true } => t.kinds.extend(kinds),
            // All but what is selected goes: the other elements, and the categories that aren't elements to select.
            Asked::Isolate { category: false } => {
                t.forms = every_form.difference(&forms).copied().collect();
                t.nodes = every_node.difference(&nodes).cloned().collect();
                if plane.is_empty() {
                    t.kinds = every_kind.into_iter().filter(|k| matches!(k, Kind::Planes | Kind::Host)).collect();
                } else {
                    t.planes = every_plane.difference(&plane).copied().collect();
                    t.kinds = every_kind.into_iter().filter(|k| matches!(k, Kind::Host)).collect();
                }
            }
            Asked::Isolate { category: true } => {
                (t.forms, t.nodes, t.planes) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
                t.kinds = every_kind.difference(&kinds).copied().collect();
            }
            Asked::Reset => {}
        }
        self.form = None;
        self.views.plane = None;
        self.modeller.selection.clear();
        Ok(())
    }

    /// Whether plane `g` (a datum or the family's own) is hidden for now, in a plan or an elevation.
    pub(super) fn temp_hidden_plane(&self, g: bimcraft_core::ids::Guid) -> bool {
        let t = &self.document.temp;
        t.kinds.contains(&Kind::Planes) || t.planes.contains(&g)
    }

    /// Whether form `i` of a family that isn't adaptive is hidden for now.
    pub(super) fn temp_hidden_form(&self, i: usize) -> bool {
        let t = &self.document.temp;
        t.kinds.contains(&Kind::Forms) || self.forms.get(i).is_some_and(|f| t.forms.contains(&f.tag))
    }

    /// Whether the wall a door's or a window's preview stands in is hidden for now.
    pub(super) fn temp_hidden_host(&self) -> bool {
        self.document.temp.kinds.contains(&Kind::Host)
    }

    /// What the modelling view of adaptive family `d` leaves out for now.
    pub(super) fn temp_hidden(&self, d: &Definition) -> Hidden {
        let t = &self.document.temp;
        let nodes = d.nodes.iter().filter(|n| t.nodes.contains(&n.id) || t.kinds.contains(&kind_of(n))).map(|n| n.id.clone()).collect();
        Hidden { nodes, datums: t.kinds.contains(&Kind::Planes) }
    }
}

/// Family `f` as the preview draws it while forms are hidden for now: each solid form `hidden` says is left out. A void stays,
/// cutting as it did: the preview never draws one, so hiding it hides nothing. None when none is.
pub(super) fn without(f: &Family, hidden: impl Fn(usize) -> bool) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for (i, form) in g.forms.iter_mut().enumerate() {
        if !form.void && hidden(i) {
            form.visible = Some(Expr::Number(0.0));
            any = true;
        }
    }
    any.then_some(g)
}

impl BimApp {
    /// Temporary Hide/Isolate in the family document's view; what it can't do, in the status bar.
    pub(crate) fn family_temp(&mut self, asked: Asked) {
        let Some(ed) = self.family_editor.as_mut() else { return };
        if let Err(why) = ed.temp(asked) {
            self.status = Status { text: t(why).into(), error: true };
        }
    }

    /// The view control bar's Temporary Hide/Isolate, as the project's view has it, and while it hides anything the bar's note.
    pub(super) fn family_temp_menu(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        let hides = ed.temporary().hides();
        let tip = msg!("Temporary Hide/Isolate: hide or isolate what is selected, or its categories, in this view until you reset it");
        let r = crate::small_button(ui, crate::icons::Icon::TempHide, "Temporary Hide/Isolate", hides, tip);
        // Revit greys out what needs a selection until there is one.
        let picked = ed.temp_picked();
        let mut asked = None;
        egui::Popup::menu(&r).show(|ui| {
            for (label, hide, category) in [
                (msg!("Hide Element (HH)"), true, false),
                (msg!("Hide Category (HC)"), true, true),
                (msg!("Isolate Element (HI)"), false, false),
                (msg!("Isolate Category (IC)"), false, true),
            ] {
                if ui.add_enabled(picked, egui::Button::new(t(label))).clicked() {
                    asked = Some(if hide { Asked::Hide { category } } else { Asked::Isolate { category } });
                }
            }
            ui.separator();
            // A family's views keep nothing hidden for good yet.
            ui.add_enabled(false, egui::Button::new(t("Apply Hide/Isolate to View"))).on_disabled_hover_text(t("Not in BIMCraft yet"));
            if ui.add_enabled(hides, egui::Button::new(t("Reset Temporary Hide/Isolate (HR)"))).clicked() {
                asked = Some(Asked::Reset);
            }
        });
        if let Some(asked) = asked
            && let Err(why) = ed.temp(asked)
        {
            self.status = Status { text: t(why).into(), error: true };
        }
        if ed.temporary().hides() {
            ui.add_space(8.0);
            ui.label(RichText::new(tf("{} (HR resets)", &[t("Temporary Hide/Isolate")])).color(crate::footer::FRAME));
        }
    }

    /// The family document's two-letter keys: HH, HC, HI, IC and HR, or what Keyboard Shortcuts made them.
    pub(super) fn family_typed(&mut self, ctx: &Context) {
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        });
        for ch in typed.chars().filter(char::is_ascii_alphanumeric) {
            self.typed.push(ch.to_ascii_uppercase());
            if self.typed.len() > 2 {
                self.typed.remove(0);
            }
            if self.typed.len() < 2 {
                continue;
            }
            let action = match self.custom_shortcuts.get(&self.typed) {
                Some(action) => Some(action.as_str()),
                None => crate::SHORTCUTS.iter().find(|(k, _)| *k == self.typed).map(|(_, a)| *a),
            };
            // Reference Plane, in a family's plan or elevation (F4): the family document's own.
            if self.typed == "RP"
                && let Some(ed) = self.family_editor.as_mut()
                && ed.editing_2d()
            {
                self.typed.clear();
                ed.reference_plane_tool();
                continue;
            }
            let asked = match action {
                Some("Temporary Hide Element") => Asked::Hide { category: false },
                Some("Temporary Hide Category") => Asked::Hide { category: true },
                Some("Temporary Isolate Element") => Asked::Isolate { category: false },
                Some("Temporary Isolate Category") => Asked::Isolate { category: true },
                Some("Reset Temporary Hide/Isolate") => Asked::Reset,
                _ => continue,
            };
            self.typed.clear();
            self.family_temp(asked);
        }
    }
}
