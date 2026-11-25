//! What a click or a box does to the selection, apart from where it came from. Revit's rules, which egui's canvas follows too: a
//! plain pick replaces the selection, Ctrl adds to it, Shift takes away; a box dragged left to right takes what it holds
//! (a window), right to left what it touches (crossing).

use bimcraft_core::ids::Id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Replace,
    Add,
    Remove,
}

impl How {
    /// From the modifier keys held (`cmd` is Ctrl off macOS): Shift wins when both are down, as in egui's canvas.
    pub fn of(shift: bool, cmd: bool) -> How {
        if shift {
            How::Remove
        } else if cmd {
            How::Add
        } else {
            How::Replace
        }
    }
}

/// Takes `found` into `selection` the way `how` says, keeping the order the selection was made in and each id once.
pub fn apply(selection: &mut Vec<Id>, found: Vec<Id>, how: How) {
    match how {
        How::Replace => *selection = found,
        How::Remove => selection.retain(|id| !found.contains(id)),
        How::Add => {
            for id in found {
                if !selection.contains(&id) {
                    selection.push(id);
                }
            }
        }
    }
}

/// A box being dragged out over the plan, in the canvas's points: from where the press began to where the pointer is now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rubber {
    pub from: (f32, f32),
    pub to: (f32, f32),
}

impl Rubber {
    /// Dragged right to left, it takes what it touches; left to right, only what it holds.
    pub fn crossing(&self) -> bool {
        self.to.0 < self.from.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modifiers_pick_how_a_pick_is_taken_and_shift_wins() {
        assert_eq!(How::of(false, false), How::Replace);
        assert_eq!(How::of(false, true), How::Add);
        assert_eq!(How::of(true, false), How::Remove);
        assert_eq!(How::of(true, true), How::Remove);
    }

    #[test]
    fn replace_add_and_remove_keep_the_order_and_each_id_once() {
        let mut s = vec![100001, 100002];
        apply(&mut s, vec![100002, 100003], How::Add);
        assert_eq!(s, vec![100001, 100002, 100003], "an id already there is not added twice");
        apply(&mut s, vec![100001, 100009], How::Remove);
        assert_eq!(s, vec![100002, 100003], "what is not selected takes nothing away");
        apply(&mut s, vec![100005], How::Replace);
        assert_eq!(s, vec![100005]);
        apply(&mut s, vec![], How::Add);
        apply(&mut s, vec![], How::Remove);
        assert_eq!(s, vec![100005], "finding nothing adds and takes away nothing");
        apply(&mut s, vec![], How::Replace);
        assert!(s.is_empty(), "a plain click on nothing clears");
    }

    #[test]
    fn a_box_is_crossing_only_when_dragged_right_to_left() {
        let r = |a: f32, b: f32| Rubber { from: (a, 0.0), to: (b, 50.0) };
        assert!(!r(10.0, 90.0).crossing());
        assert!(r(90.0, 10.0).crossing());
        assert!(!r(10.0, 10.0).crossing(), "straight down is a window");
    }
}
