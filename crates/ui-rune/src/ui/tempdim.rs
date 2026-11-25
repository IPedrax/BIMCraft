//! The temporary dimensions over the plan: the lines of what the element selected alone measures (one inert layer, `plan-dims`) and a
//! number to type into at the middle of each (`plan-dim-<n>`, a `rune_kit::TextInput` on the number being typed, `App::temp`; Enter commits
//! it, `App::commit_dim`). Nothing is built while there are none to show, so the plan is as it was.

use rune::{El, Tx, canvas};
use rune_kit::{Kit, TextInput};

use crate::app::App;
use crate::lang::t;
use crate::paint::{self, Palette};

/// A number's field: its width and height, points.
const FIELD: (f32, f32) = (76.0, 24.0);

/// The dimensions' elements: their lines and a field for each number, none when there is nothing to show.
pub fn dimensions(s: &App, pal: &Palette) -> Vec<El> {
    let shown = s.dims_shown();
    if shown.is_empty() {
        return Vec::new();
    }
    let view = s.camera.view(s.size.0, s.size.1);
    let kit = Kit::neutral(s.dark());
    let (mut placed, mut fields) = (Vec::new(), Vec::new());
    for (i, f) in shown.iter().enumerate() {
        let Some(p) = paint::place_dimension(&f.dim, &view) else { continue };
        placed.push(p);
        let input = TextInput::new(&f.edit)
            .label(t("Temporary dimension"))
            .width(Some(FIELD.0))
            .on_edit(move |s: &mut App| s.temp.get_mut(i).map_or(&mut s.palette.scratch, |f| &mut f.edit))
            .on_submit(move |s: &mut App, _: &mut Tx| {
                s.commit_dim(i);
            })
            .build(&kit);
        fields.push(input.key(format!("plan-dim-{i}")).pin(Some(p.middle.0 - FIELD.0 / 2.0), Some(p.middle.1 - FIELD.1 / 2.0), None, None));
    }
    if placed.is_empty() {
        return Vec::new();
    }
    let lines = paint::dimension_lines(&placed, &view, pal).shape;
    let mut out = vec![canvas(lines).key("plan-dims").inert().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))];
    out.extend(fields);
    out
}
