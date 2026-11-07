//! Sheets: paper with an optional title block, and views placed on it at a scale. A sheet is drawn once,
//! at [`DENSITY`] px per paper mm, as a display list in paper pixels: what the PNG, the PDF and the
//! desktop's sheet view show. Each view on it is drawn as its own view draws it, moved onto the
//! paper and made the viewport's to pick, and titled under it; a section's picture of what lies
//! beyond, and the 3D view's, go in as images. A schedule is a ruled table under its title, the
//! size its lettering makes it. Text notes go on the paper itself.

use bimcraft_i18n::msg;

use crate::geom::P2;
use crate::graphics::{self, Look};
use crate::language::Language;
use crate::model::{Element, Guid, Id, Key, Model, Schedule, Sheet, Viewport, paper};
use crate::pen::Pen;
use crate::render::{self, Drawing, PAPER, Part, Prim, Rgb, View};
use crate::{font, schedule, view3d};

mod panel;

/// Paper px per mm in a sheet's drawing: plan lettering 10 px tall comes out 2 mm on paper.
pub const DENSITY: f64 = 5.0;
/// Room around what a view shows, on paper, for grid bubbles and the like: mm.
const ROOM: f64 = 12.0;
const INK: Rgb = Rgb(25, 25, 25);
/// A schedule's lettering and its title's, px: 2.5 mm and 3.5 mm on paper.
const CELL_TEXT: f32 = 12.5;
const TITLE_TEXT: f32 = 17.5;
/// A schedule's rows and its title's, and the room either side of a cell's text, px.
const ROW: f32 = 30.0;
const TITLE_ROW: f32 = 45.0;
const PAD: f32 = 10.0;

/// Paper mm (from the bottom left corner, y up) to the sheet drawing's pixels.
fn page(paper: P2) -> View {
    View {
        angle: 0.0,
        scale: DENSITY,
        cx: paper.0 / 2.0,
        cy: paper.1 / 2.0,
        width: (paper.0 * DENSITY).round() as u32,
        height: (paper.1 * DENSITY).round() as u32,
    }
}

/// View `view` at 1 : `scale` on paper: the frame its drawing gets, `ROOM` mm around what it shows
/// (a 3D view's picture, as large as the model comes out); a schedule's, its table's.
fn framing(m: &Model, view: impl Key, scale: f64) -> Option<View> {
    framing_in(m, &Work::new(), view, scale)
}

/// What a sheet's sections and elevations ask of the whole model, made once a sheet when the first asks.
type Work = std::cell::OnceCell<render::SectionWork>;

/// [`framing`], a section's of `work`.
fn framing_in(m: &Model, work: &Work, view: impl Key, scale: f64) -> Option<View> {
    let k = DENSITY / scale;
    let view = m.guid(view);
    let angle = render::view_angle(m, view);
    let pts: Vec<P2> = match m.elements.get(view)? {
        // A plan turned to true north takes the paper it turned takes: its points as they lie on the sheet.
        Element::Level(_) | Element::PlanView(_) => render::plan_extent(m, view).into_iter().map(|p| turn(p, angle)).collect(),
        Element::Section(_) | Element::Elevation(_) => {
            render::section_extent(m, work.get_or_init(|| render::SectionWork::new(m)), view, k, 1.0)?.to_vec()
        }
        Element::Schedule(sc) => {
            let (width, height) = table_size(&sc.name, &table_of(m, sc));
            return Some(View { angle: 0.0, scale: 1.0, cx: 0.0, cy: 0.0, width, height });
        }
        Element::PanelSchedule(ps) => {
            let (width, height) = panel::size(m, ps)?;
            return Some(View { scale: 1.0, cx: 0.0, cy: 0.0, width, height });
        }
        Element::View3D(v) => {
            let cam = view3d::camera_in(m, v.azimuth, v.elevation, k, ROOM * DENSITY, &Look::of(m, view));
            return Some(View { angle: 0.0, scale: k, cx: 0.0, cy: 0.0, width: cam.width, height: cam.height });
        }
        _ => return None,
    };
    let (lo, hi) = match pts.first() {
        Some(&first) => pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))),
        None => (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)),
    };
    let side = |d: f64| ((d * k + 2.0 * ROOM * DENSITY).ceil().clamp(1.0, f64::from(u32::MAX))) as u32;
    let c = turn(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), -angle);
    Some(View { angle, scale: k, cx: c.0, cy: c.1, width: side(hi.0 - lo.0), height: side(hi.1 - lo.1) })
}

/// `p` turned `angle` degrees counter-clockwise about the origin.
fn turn(p: P2, angle: f64) -> P2 {
    let (sin, cos) = angle.to_radians().sin_cos();
    P2(p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos)
}

/// How much paper view `view` takes at 1 : `scale`, mm.
pub fn size(m: &Model, view: impl Key, scale: f64) -> Option<P2> {
    framing(m, view, scale).map(|v| P2(f64::from(v.width) / DENSITY, f64::from(v.height) / DENSITY))
}

/// Whether any viewport of `view` takes its view's scale and hasn't `scale` yet: what `scale` asks before it
/// changes the model, so a scale a view already has leaves it as it was, with no undo step of its own.
pub fn follows_scale(m: &Model, view: impl Key, scale: f64) -> bool {
    let view = m.guid(view);
    m.elements.values().any(|e| matches!(e, Element::Viewport(vp) if vp.view == view && vp.follow_scale && vp.scale != scale))
}

/// Every viewport of `view` that takes its view's scale takes `scale`: a viewport placed at its view's own scale
/// follows it as it changes, as it does in Revit, and one placed at a scale of its own keeps that one.
pub fn follow_scale(m: &mut Model, view: impl Key, scale: f64) {
    let view = m.guid(view);
    // Only those that differ, so a command that changes nothing leaves the model as it was.
    let placed: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Viewport(vp) if vp.view == view && vp.follow_scale && vp.scale != scale))
        .map(|(id, _)| *id)
        .collect();
    for id in placed {
        if let Some(Element::Viewport(vp)) = m.elements.get_mut(id) {
            vp.scale = scale;
        }
    }
}

/// A placed view's scale when none is given: its own, or, for an unscaled 3D view, the finest
/// offered project scale from the default up at which [`spot`] finds it and its title a place in the
/// sheet's room, clear of the views there and the title block (or in the title block's drawing
/// area). If none fits, use the coarsest offered scale. Assigned view and template scales are kept.
pub fn default_scale(m: &Model, sheet: impl Key, view: impl Key) -> f64 {
    let own = graphics::scale_of(m, view);
    let assigned = graphics::graphics_of(m, view).is_some_and(|g| g.resolved(m).scale.is_some());
    if assigned || !matches!(m.elements.get(view), Some(Element::View3D(_))) {
        return own;
    }
    let sheet = m.guid(sheet);
    let mut scales: Vec<f64> = m.units.scales().iter().copied().filter(|n| *n >= own).collect();
    scales.sort_by(f64::total_cmp);
    scales.iter().copied().find(|n| free_spot(m, sheet, view, *n).is_some()).or_else(|| scales.last().copied()).unwrap_or(own)
}

/// Viewport `id`'s view frame, and where that frame's top left corner sits in its sheet's drawing,
/// px: a point `p` of the view is at `frame.px(p)` plus that corner.
pub fn frame(m: &Model, id: impl Key) -> Option<(View, (f32, f32))> {
    frame_in(m, &Work::new(), id)
}

/// [`frame`], a section's of `work`.
fn frame_in(m: &Model, work: &Work, id: impl Key) -> Option<(View, (f32, f32))> {
    let Some(Element::Viewport(vp)) = m.elements.get(id) else { return None };
    let view = framing_in(m, work, vp.view, vp.scale)?;
    let (x, y) = page(paper(&m.sheet(vp.sheet)?.size)?).px(vp.at);
    Some((view, (x - view.width as f32 / 2.0, y - view.height as f32 / 2.0)))
}

/// Clearance between automatically placed views and the title block, mm.
const GAP: f64 = 10.0;
/// Room under a view for its name, rule and scale, mm. Schedules have their title inside the table.
const UNDER: f64 = 15.0;

/// The complete view and title on paper: left, bottom, right and top relative to the view's centre.
fn footprint(m: &Model, view: impl Key, scale: f64) -> Option<[f64; 4]> {
    let P2(w, h) = size(m, view, scale)?;
    if matches!(m.elements.get(view), Some(Element::Schedule(_))) {
        return Some([-w / 2.0, -h / 2.0, w / 2.0, h / 2.0]);
    }
    let name = m.view_name(view).unwrap_or_default();
    // Lettering uppercases the name while drawing it (a character may expand to more than one).
    let name_width = f64::from(font::width(&name.to_uppercase(), (3.0 * DENSITY) as f32)) / DENSITY;
    let rule = f64::from(font::width(name, (3.0 * DENSITY) as f32)) / DENSITY + 10.0;
    let scale_width = f64::from(font::width(&m.units.scale(scale), (2.0 * DENSITY) as f32)) / DENSITY;
    Some([-w / 2.0, -h / 2.0 - UNDER, (w / 2.0).max(-w / 2.0 + ROOM + name_width.max(rule).max(scale_width)), h / 2.0])
}

/// Where views go by default on a sheet: the box they keep inside (its title block's drawing area, else the border 5 mm
/// in), the title block they keep clear of (none in a drawing area, or on a sheet with no title block), the middle the
/// first goes in, and the paper, mm.
struct Room {
    inside: [f64; 4],
    title: Option<[f64; 4]>,
    middle: P2,
    paper: P2,
}

/// The [`Room`] of sheet `sheet`, an A3 of the built-in title block for anything else.
fn room(m: &Model, sheet: impl Key) -> Room {
    let sh = m.sheet(sheet);
    let size_name = sh.map(|s| s.size.as_str()).unwrap_or("A3");
    let paper = sh.and_then(|s| paper(&s.size)).unwrap_or(P2(420.0, 297.0));
    let margins = m.standard().sheet_margins(size_name);
    let own = sh.and_then(|s| m.title_blocks.get(&s.title_block));
    if let Some([lo, hi]) = own.and_then(|t| t.drawing_area) {
        return Room { inside: [lo.0, lo.1, hi.0, hi.1], title: None, middle: (lo + hi) * 0.5, paper };
    }
    // The title block in the border's bottom right corner: the project's the sheet names, else the built-in one.
    let right = paper.0 - margins.right;
    let title = match own {
        _ if sh.is_some_and(Sheet::has_no_title_block) => None,
        Some(t) => Some([right - t.width, margins.bottom, right, margins.bottom + t.height]),
        None => Some([
            right - margins.title_width.min(paper.0 - margins.left - margins.right),
            margins.bottom,
            right,
            margins.bottom + margins.title_height,
        ]),
    };
    // The first view stands in the middle of the border, above the title block.
    let above = title.map_or(margins.bottom, |t| t[3]);
    Room {
        inside: [margins.left + 5.0, margins.bottom + 5.0, right - 5.0, paper.1 - margins.top - 5.0],
        title,
        middle: P2((margins.left + right) / 2.0, (above + paper.1 - margins.top) / 2.0),
        paper,
    }
}

/// Footprint `b` of a view, relative to its centre, moved to stand at `p`.
fn moved(b: [f64; 4], p: P2) -> [f64; 4] {
    [b[0] + p.0, b[1] + p.1, b[2] + p.0, b[3] + p.1]
}

/// Where view `view` at 1 : `scale` goes on sheet `sheet` by default: in its usual middle when
/// free, otherwise beside or below another view or in a corner, clear of their titles and the
/// title block, or inside the title block's drawing area when it gives one. When no candidate
/// fits, continue right of all the views, off the paper without stacking.
pub fn spot(m: &Model, sheet: impl Key, view: impl Key, scale: f64) -> P2 {
    let sheet = m.guid(sheet);
    free_spot(m, sheet, view, scale).unwrap_or_else(|| {
        // Keep overflow backgrounds wholly beyond the paper so they cannot erase its border or title block.
        let Room { middle, paper, inside, .. } = room(m, sheet);
        let right = placed(m, sheet).iter().map(|b| b[2]).reduce(f64::max).unwrap_or(inside[2]);
        P2(right.max(paper.0) + GAP - footprint(m, view, scale).unwrap_or([0.0; 4])[0], middle.1)
    })
}

/// The footprints of the views on sheet `sheet`, where they stand.
fn placed(m: &Model, sheet: Guid) -> Vec<[f64; 4]> {
    m.on_sheet(sheet)
        .into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Viewport(v)) if v.sheet == sheet => Some(moved(footprint(m, v.view, v.scale)?, v.at)),
            _ => None,
        })
        .collect()
}

/// [`spot`] when it finds view `view` at 1 : `scale` a place in the sheet's room; none when the view must go off the paper.
fn free_spot(m: &Model, sheet: Guid, view: impl Key, scale: f64) -> Option<P2> {
    let Room { inside: [west, south, east, north], title, middle, .. } = room(m, sheet);
    let taken = footprint(m, view, scale).unwrap_or([0.0; 4]);
    let placed = placed(m, sheet);
    let reserved: Vec<[f64; 4]> = placed.iter().copied().chain(title).collect();
    // Candidate coordinates subtract and add half sizes; tolerate only their floating-point roundoff.
    let eps = 1e-9;
    let apart = |a: [f64; 4], b: &[f64; 4]| {
        a[2] + GAP <= b[0] + eps || b[2] + GAP <= a[0] + eps || a[3] + GAP <= b[1] + eps || b[3] + GAP <= a[1] + eps
    };
    let free = |p: &P2| {
        let b = moved(taken, *p);
        b[0] + eps >= west && b[1] + eps >= south && b[2] <= east + eps && b[3] <= north + eps && reserved.iter().all(|o| apart(b, o))
    };
    if placed.is_empty() && free(&middle) {
        return Some(middle);
    }
    let right = placed.iter().map(|b| b[2]).reduce(f64::max).unwrap_or(east);
    let beside = P2(right + GAP - taken[0], middle.1);
    if free(&beside) {
        return Some(beside);
    }
    let mut spots: Vec<P2> = reserved
        .iter()
        .flat_map(|o| [P2(o[2] + GAP - taken[0], o[3] - taken[3]), P2(o[0] - taken[0], o[1] - GAP - taken[3])])
        .chain([
            P2(west - taken[0], north - taken[3]),
            P2(west - taken[0], south - taken[1]),
            P2(east - taken[2], north - taken[3]),
            P2(east - taken[2], south - taken[1]),
        ])
        .collect();
    // Prefer the topmost free candidate, then the leftmost.
    spots.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    spots.into_iter().find(free)
}

/// [`spot`], refused when the view and its title are larger than the drawing area its sheet's title block gives.
pub fn automatic_center(m: &Model, sheet: impl Key, view: impl Key, scale: f64) -> Result<P2, String> {
    let sheet = m.guid(sheet);
    let area = m.sheet(sheet).and_then(|s| m.title_blocks.get(&s.title_block)).and_then(|t| t.drawing_area);
    if let Some([lo, hi]) = area {
        let b = footprint(m, view, scale).ok_or_else(|| "the viewport frame cannot be measured at this scale".to_string())?;
        let (taken, available) = (P2(b[2] - b[0], b[3] - b[1]), hi - lo);
        if taken.0 > available.0 || taken.1 > available.1 {
            return Err(format!(
                "the viewport frame and its title are {:.1} × {:.1} mm at 1 : {scale}, and the title block's drawing area {:.1} × \
                 {:.1} mm: choose a smaller view scale or give `at`",
                taken.0, taken.1, available.0, available.1
            ));
        }
    }
    Ok(spot(m, sheet, view, scale))
}

/// What to say of view `view` at 1 : `scale` standing at `at` on sheet `sheet` when it, or its title, reaches past
/// the paper's edge: nothing when the paper holds it.
pub fn off_paper(m: &Model, sheet: impl Key, view: impl Key, scale: f64, at: P2) -> Option<String> {
    let Room { paper, .. } = room(m, sheet);
    let b = moved(footprint(m, view, scale)?, at);
    let eps = 1e-6;
    (b[0] < -eps || b[1] < -eps || b[2] > paper.0 + eps || b[3] > paper.1 + eps).then(|| {
        format!(
            "the view and its title, {:.1} × {:.1} mm at 1 : {scale}, reach past the {:.1} × {:.1} mm paper: give `at`, a \
             smaller `scale` or a larger sheet",
            b[2] - b[0],
            b[3] - b[1],
            paper.0,
            paper.1
        )
    })
}

/// `text` with its left end at paper point `at` (mm), capitals `height` mm tall.
fn left(d: &mut Drawing, text: &str, at: P2, height: f64) {
    let h = (height * DENSITY) as f32;
    let (x, y) = d.view.px(at);
    d.text(text, (x + font::width(text, h) / 2.0, y), h, INK);
}

/// Keep lettering at its usual height, or shrink it to its title-block cell's width, mm.
fn fitted_height(text: &str, height: f64, width: f64) -> f64 {
    let taken = f64::from(font::width(text, (height * DENSITY) as f32)) / DENSITY;
    if taken > width { height * width / taken } else { height }
}

/// `text` lettered `height` mm tall, its right end at `at`.
fn right(d: &mut Drawing, text: &str, at: P2, height: f64) {
    let h = (height * DENSITY) as f32;
    let (x, y) = d.view.px(at);
    d.text(text, (x - font::width(text, h) / 2.0, y), h, INK);
}

/// Draw standard folding marks (marcas de dobramento) as specified in NBR 17067.
fn draw_folding_marks(d: &mut Drawing, paper: P2, size: &str, margins: crate::standards::SheetMargins) {
    let s = crate::model::paper_name(size);
    let x_folds: &[f64] = match s.as_str() {
        "A3" => &[210.0],
        "A2" => &[210.0, 402.0],
        "A1" => &[210.0, 395.0, 580.0, 765.0],
        "A0" => &[210.0, 395.0, 580.0, 765.0, 950.0, 1135.0],
        _ => &[],
    };
    for &dist_from_right in x_folds {
        let x = paper.0 - dist_from_right;
        if x > margins.left && x < paper.0 - margins.right {
            d.line(&[P2(x, 0.0), P2(x, margins.bottom)], Pen::FINE, INK, false);
            d.line(&[P2(x, paper.1 - margins.top), P2(x, paper.1)], Pen::FINE, INK, false);
        }
    }
    let y_folds: &[f64] = match s.as_str() {
        "A2" | "A1" => &[297.0],
        "A0" => &[297.0, 594.0],
        _ => &[],
    };
    for &y in y_folds {
        if y < paper.1 - margins.top {
            d.line(&[P2(0.0, y), P2(margins.left, y)], Pen::FINE, INK, false);
            d.line(&[P2(paper.0 - margins.right, y), P2(paper.0, y)], Pen::FINE, INK, false);
        }
    }
}

/// The border, filing margin on the left and borders on the other sides, and the title
/// box in the bottom right corner, adapted to `std`: the project, the sheet's name, its scale, and its number. A sheet that
/// names a title block of the project's has that one there instead, its fields filled in; one with none
/// ([`crate::titleblock::NONE`]) has nothing of it, border and folding marks included, for one drawn on the sheet itself.
fn title_block(d: &mut Drawing, m: &Model, id: impl Key, paper: P2, views: &[&Viewport]) {
    let Some(sheet) = m.sheet(id).filter(|sh| !sh.has_no_title_block()) else { return };
    let (project, std) = (&m.name, m.standard());
    let margins = std.sheet_margins(&sheet.size);
    let rect = |a: P2, b: P2| vec![a, P2(b.0, a.1), b, P2(a.0, b.1)];
    let template = m.title_blocks.get(&sheet.title_block);
    if !template.is_some_and(|t| t.template_border) {
        d.line(&rect(P2(margins.left, margins.bottom), P2(paper.0 - margins.right, paper.1 - margins.top)), Pen::BORDER, INK, true);
    }
    if let Some(tb) = template {
        let at = P2(paper.0 - margins.right - tb.width, margins.bottom);
        let scale = scale_value(views, m.language, m.units);
        crate::titleblock::draw(d, tb, at, &crate::titleblock::values(m, id, &scale));
        if margins.folding_marks {
            draw_folding_marks(d, paper, &sheet.size, margins);
        }
        return;
    }
    let width = margins.title_width.min(paper.0 - margins.left - margins.right);
    let height = margins.title_height;
    let (x0, x1, y0, y1) = (paper.0 - margins.right - width, paper.0 - margins.right, margins.bottom, margins.bottom + height);
    d.line(&rect(P2(x0, y0), P2(x1, y1)), Pen::BORDER, INK, true);
    d.line(&[P2(x0, y0 + 26.0), P2(x1, y0 + 26.0)], Pen::PROJECTION, INK, false);
    d.line(&[P2(x1 - 50.0, y0), P2(x1 - 50.0, y0 + 26.0)], Pen::PROJECTION, INK, false);
    // Measure the capitals that are drawn, including expansions such as ß to SS.
    let (project, name, scale, number) = (
        project.to_uppercase(),
        sheet.name.to_uppercase(),
        scale_note(views, m.language, m.units).to_uppercase(),
        sheet.number.to_uppercase(),
    );
    left(d, &project, P2(x0 + 4.0, y0 + 33.0), fitted_height(&project, 5.0, width - 8.0));
    // Right of the project, what its information says of the client, and its number, status and issue date.
    if let Some((_, info)) = m.project_info() {
        let client = info.client.to_uppercase();
        right(d, &client, P2(x1 - 4.0, y0 + 36.0), fitted_height(&client, 2.5, width / 2.0));
        let said: Vec<&str> =
            [&info.number, &info.status, &info.issue_date].into_iter().map(String::as_str).filter(|t| !t.is_empty()).collect();
        let said = said.join("   ").to_uppercase();
        right(d, &said, P2(x1 - 4.0, y0 + 30.0), fitted_height(&said, 2.0, width / 2.0));
    }
    left(d, &name, P2(x0 + 4.0, y0 + 17.0), fitted_height(&name, 3.5, width - 58.0));
    left(d, &scale, P2(x0 + 4.0, y0 + 7.0), fitted_height(&scale, 2.0, width - 58.0));
    let number_height = (fitted_height(&number, 7.0, 42.0) * DENSITY) as f32;
    let at = d.view.px(P2(x1 - 25.0, y0 + 13.0));
    d.text(&number, at, number_height, INK);

    if margins.folding_marks {
        draw_folding_marks(d, paper, &sheet.size, margins);
    }
}

/// A schedule's table: its headings; its rows' cells as shown, its totals' last; each column's
/// width, px; and whether it has totals.
struct Table {
    headings: Vec<String>,
    rows: Vec<Vec<String>>,
    widths: Vec<f32>,
    totals: bool,
}

fn table_of(m: &Model, sc: &Schedule) -> Table {
    let t = schedule::table(m, sc).unwrap_or(schedule::Table { columns: Vec::new(), rows: Vec::new(), ids: Vec::new(), totals: None });
    let said = |c: &schedule::Column, v: &serde_json::Value| {
        let s = schedule::shown(m.units, c.kind, v);
        if v.is_boolean() { m.language.word(&s).to_string() } else { s }
    };
    let mut rows: Vec<Vec<String>> = t.rows.iter().map(|r| r.iter().zip(&t.columns).map(|(v, c)| said(c, v)).collect()).collect();
    if let Some(totals) = &t.totals {
        let mut row: Vec<String> = totals.iter().zip(&t.columns).map(|(v, c)| said(c, v)).collect();
        if let Some(first) = row.first_mut().filter(|f| f.is_empty()) {
            *first = m.language.word("Total").to_string();
        }
        rows.push(row);
    }
    let headings: Vec<String> = t
        .columns
        .iter()
        .map(|c| schedule::heading(m.units, &schedule::Column { heading: m.language.word(&c.heading).to_string(), kind: c.kind }))
        .collect();
    let widths = (0..headings.len())
        .map(|i| {
            let widest = headings.get(i).map_or(0.0, |c| font::width(c, CELL_TEXT));
            rows.iter().filter_map(|r| r.get(i)).map(|c| font::width(c, CELL_TEXT)).fold(widest, f32::max) + 2.0 * PAD
        })
        .collect();
    Table { headings, rows, widths, totals: t.totals.is_some() }
}

fn table_size(name: &str, t: &Table) -> (u32, u32) {
    let width = t.widths.iter().sum::<f32>().max(2.0 * PAD + font::width(name, TITLE_TEXT));
    (width.ceil() as u32, (TITLE_ROW + ROW * (t.rows.len() + 1) as f32).ceil() as u32)
}

/// Schedule `sc` as a ruled table from its top left corner at `(x, y)` px: its title, then its
/// headings and its rows, a column each, its totals under a heavier rule.
fn table(d: &mut Drawing, m: &Model, sc: &Schedule, (x, y): (f32, f32)) {
    let t = table_of(m, sc);
    let (w, h) = table_size(&sc.name, &t);
    // On whole pixels, so its rules come out crisp.
    let (x, y, w, h) = (x.round(), y.round(), w as f32, h as f32);
    let rule = |d: &mut Drawing, a: (f32, f32), b: (f32, f32), pen: Pen| {
        d.stroke(vec![a, b], pen, INK, None, false);
    };
    d.stroke(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], Pen::HEAVY, INK, None, true);
    d.text(&sc.name, (x + w / 2.0, y + TITLE_ROW / 2.0), TITLE_TEXT, INK);
    let top = y + TITLE_ROW;
    rule(d, (x, top), (x + w, top), Pen::PROJECTION);
    let last = t.rows.len();
    for (i, cells) in std::iter::once(&t.headings).chain(&t.rows).enumerate() {
        let row = top + ROW * i as f32;
        if i > 0 {
            rule(d, (x, row), (x + w, row), if i == 1 || (t.totals && i == last) { Pen::MEDIUM } else { Pen::FINE });
        }
        let mut left = x;
        for (text, width) in cells.iter().zip(&t.widths) {
            d.text(text, (left + PAD + font::width(text, CELL_TEXT) / 2.0, row + ROW / 2.0), CELL_TEXT, INK);
            left += width;
        }
    }
    let mut left = x;
    for width in t.widths.iter().take(t.widths.len().saturating_sub(1)) {
        left += width;
        rule(d, (left, top), (left, y + h), Pen::FINE);
    }
}

/// The scale a sheet's title box gives, in `language` and named as `units` name it: its views', when they share one.
fn scale_note(views: &[&Viewport], language: Language, units: crate::units::Units) -> String {
    match views.split_first() {
        None => String::new(),
        Some((first, rest)) if rest.iter().all(|v| v.scale == first.scale) => {
            format!("{} {}", language.word(msg!("SCALE")), units.scale(first.scale))
        }
        Some(_) => language.word(msg!("SCALE AS INDICATED")).into(),
    }
}

/// The scale a title block's `{SCALE}` gives: its views' (`1 : 100`), when they share one, else "AS INDICATED", in `language`.
fn scale_value(views: &[&Viewport], language: Language, units: crate::units::Units) -> String {
    match views.split_first() {
        None => String::new(),
        Some((first, rest)) if rest.iter().all(|v| v.scale == first.scale) => units.scale(first.scale),
        Some(_) => language.word(msg!("AS INDICATED")).into(),
    }
}

/// What each field of sheet `id`'s title block says, by key: [`crate::titleblock::values`], its scale as its title box gives it.
pub fn title_values(m: &Model, id: impl Key) -> std::collections::BTreeMap<String, String> {
    let guid = m.guid(id);
    let scaled: Vec<&Viewport> = m
        .on_sheet(id)
        .into_iter()
        .filter_map(|v| match m.elements.get(v) {
            Some(Element::Viewport(vp)) if vp.sheet == guid && !matches!(m.elements.get(vp.view), Some(Element::Schedule(_))) => Some(vp),
            _ => None,
        })
        .collect();
    crate::titleblock::values(m, id, &scale_value(&scaled, m.language, m.units))
}

/// Sheet `id` on its paper: its title block, then each view on it, at its scale and titled.
pub fn draw(m: &Model, id: impl Key) -> Option<Drawing> {
    let sheet = m.sheet(id)?;
    let guid = m.guid(id);
    let paper = paper(&sheet.size)?;
    // What is on it, in the order of its ids: its viewports, and the 2D drawn on it.
    let on: Vec<(Id, &Element)> = m.on_sheet(id).into_iter().filter_map(|id| Some((id, m.elements.get(id)?))).collect();
    let views: Vec<(Id, &Viewport)> = on
        .iter()
        .filter_map(|&(vid, e)| match e {
            Element::Viewport(v) if v.sheet == guid => Some((vid, v)),
            _ => None,
        })
        .collect();
    let mut d = Drawing::new(page(paper));
    // The paper's own lines (the title block, the views' titles, schedules' rules) are annotation; its views come sized.
    d.weights = m.line_weights().at(m.units.default_scale());
    d.own(m.elements.id(id));
    // Schedules keep their own size: no scale of theirs.
    let scaled: Vec<&Viewport> = views
        .iter()
        .filter(|(_, v)| !matches!(m.elements.get(v.view), Some(Element::Schedule(_) | Element::PanelSchedule(_))))
        .map(|(_, v)| *v)
        .collect();
    title_block(&mut d, m, id, paper, &scaled);
    let mut scene = None;
    let work = Work::new();
    for (vid, vp) in views {
        let Some((view, (ox, oy))) = frame_in(m, &work, vid) else { continue };
        d.own(Some(vid));
        // Paper under it, its own: a click anywhere over a view picks it.
        let (w, h) = (view.width as f32, view.height as f32);
        d.push(Prim::Fill { rings: vec![vec![(ox, oy), (ox + w, oy), (ox + w, oy + h), (ox, oy + h)]], color: PAPER });
        let inner = match m.elements.get(vp.view) {
            Some(Element::Level(_) | Element::PlanView(_)) => render::draw_at(m, vp.view, view, 1.0),
            Some(Element::Section(_) | Element::Elevation(_)) => {
                // Its regions keep the ink of this very picture.
                let w = work.get_or_init(|| render::SectionWork::new(m));
                let img = render::section_picture_in(m, w, vp.view, view, &[]);
                let drawing = match &img {
                    Some(img) => render::section_at_over_in(m, w, vp.view, view, 1.0, img),
                    None => render::section_at(m, vp.view, view, 1.0),
                };
                if let Some(img) = img {
                    d.push(Prim::Image { at: (ox, oy), width: img.width, height: img.height, rgba: img.rgba, ink: false });
                }
                drawing
            }
            // Its title is its first row; it has no view title under it.
            Some(Element::Schedule(sc)) => {
                table(&mut d, m, sc, (ox, oy));
                continue;
            }
            Some(Element::PanelSchedule(ps)) => {
                panel::draw(&mut d, m, ps, (ox, oy));
                continue;
            }
            Some(Element::View3D(v)) => {
                let look = Look::of(m, vp.view);
                let cam = view3d::camera_of(m, v, view.scale, ROOM * DENSITY, &look);
                let mut img = scene.get_or_insert_with(|| view3d::Scene::new(m)).render_in(&cam, &[], &look);
                // What its section box cuts, filled.
                if look.section_box().is_some() {
                    img.capped(&cam, &view3d::CapSource::new(m).caps_in(m, m.elements.id(vp.view), &look), &[], &look);
                }
                d.push(Prim::Image { at: (ox, oy), width: img.width, height: img.height, rgba: img.rgba, ink: false });
                // Its dimensions, over the picture: lines of the sheet's, at paper size.
                let mut inner = Drawing::new(view);
                for mk in crate::dims3d::marks(m, vp.view, &cam, &look, DENSITY) {
                    inner.own(Some(mk.owner));
                    inner.push(Prim::Line { pts: mk.pts, width: mk.width, color: mk.color, dash: None, closed: false });
                }
                inner
            }
            _ => continue,
        };
        for note in inner.notes {
            if d.notes.len() < 32 && !d.notes.contains(&note) {
                d.notes.push(note);
            }
        }
        // Its own title, owned by no element, gives way to the viewport's.
        for ((prim, owner), part) in inner.prims.into_iter().zip(inner.owners).zip(inner.parts) {
            if owner.is_some() {
                d.part(part);
                d.push(prim.moved(ox, oy));
            }
        }
        d.part(Part::Projection);
        // The title, under it: the view's name over a line, its scale under that.
        let name = m.view_name(vp.view).unwrap_or_default();
        let corner = d.view.mm((ox, oy + view.height as f32));
        let x = corner.0 + ROOM;
        left(&mut d, name, P2(x, corner.1 - 5.0), 3.0);
        let line = f64::from(font::width(name, (3.0 * DENSITY) as f32)) / DENSITY + 10.0;
        d.line(&[P2(x, corner.1 - 8.0), P2(x + line, corner.1 - 8.0)], Pen::MEDIUM, INK, false);
        left(&mut d, &m.units.scale(vp.scale), P2(x, corner.1 - 11.0), 2.0);
    }
    for &(nid, e) in &on {
        if let Element::FilledRegion(r) = e
            && r.view == guid
        {
            d.own(Some(nid));
            render::filled_region(&mut d, m, r);
        }
    }
    for &(nid, e) in &on {
        if let Element::DetailGroup(g) = e
            && g.view == guid
        {
            d.own(Some(nid));
            render::detail_group(&mut d, m, g);
        }
        if let Element::RepeatingDetail(r) = e
            && r.view == guid
        {
            d.own(Some(nid));
            r.groups().iter().for_each(|g| render::detail_group(&mut d, m, g));
        }
        if let Element::Text(n) = e
            && n.view == guid
        {
            d.own(Some(nid));
            render::note(&mut d, m, n);
        }
        if let Element::DetailLine(l) = e
            && l.view == guid
        {
            d.own(Some(nid));
            render::detail_line(&mut d, m, l);
        }
    }
    d.own(None);
    d.weigh(|_| true);
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::geom::P2;
    use crate::render::{POCHE, png};

    fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
        let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
        let c = pm.pixel(x as u32, y as u32).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn sheets_draw_a_title_block_and_their_views_at_scale() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               sheet {"name": "Plans"}
               place {"sheet": "A101", "view": 100000}"#,
        )
        .unwrap();
        // Walls 3 to 6, the sheet 7, its viewport 8.
        let d = draw(&s.model, 100006).unwrap();
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 120 && g < 120 && b < 120
        };
        assert!(dark(d.view.px(P2(20.0, 150.0))), "the border, 20 mm in from the left");
        assert!(dark(d.view.px(P2(415.0 - 5.0, 10.0))) || dark(d.view.px(P2(410.0, 10.0))), "the border's bottom right");
        // The plan at 1 : 100, where the viewport puts it: a wall in poché.
        let (v, (ox, oy)) = frame(&s.model, 100007).unwrap();
        let at = |p: P2| {
            let (x, y) = v.px(p);
            (x + ox, y + oy)
        };
        assert_eq!(pixel(&png, at(P2(4000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert!((v.scale - DENSITY / 100.0).abs() < 1e-12, "1 : 100");
        // What a viewport draws is the viewport's to pick; the title block is the sheet's.
        assert_eq!(d.pick(at(P2(4000.0, 0.0)), 2.0), Some(100007));
        assert_eq!(d.pick(d.view.px(P2(20.0, 150.0)), 2.0), Some(100006));
    }

    #[test]
    fn a_viewport_follows_its_views_scale_unless_it_was_given_one() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               section {"start": [-1000, 3000], "end": [9000, 3000]}
               sheet {"name": "Plans"}
               sheet {"name": "Sections"}
               place {"sheet": "A101", "view": "Level 1"}
               place {"sheet": "A102", "view": "Section 1", "scale": 50}"#,
        )
        .unwrap();
        let vp = |s: &Session, name: &str| -> (Id, Viewport) {
            s.model
                .elements
                .iter()
                .find_map(|(id, e)| match e {
                    Element::Viewport(v) if s.model.view_name(v.view) == Some(name) => Some((*id, v.clone())),
                    _ => None,
                })
                .expect("a viewport of that view")
        };
        let paper = |s: &Session, id: Id| {
            let (v, _) = frame(&s.model, id).unwrap();
            P2(f64::from(v.width) / DENSITY, f64::from(v.height) / DENSITY)
        };
        let (plan, placed) = vp(&s, "Level 1");
        assert_eq!((placed.scale, placed.follow_scale), (100.0, true), "placed at the view's own scale");
        let before = paper(&s, plan);
        // The view's scale changes: its viewport takes it, and the sheet's title gives it.
        s.execute("scale", &serde_json::json!({"view": "Level 1", "scale": 50})).unwrap();
        let (plan, now) = vp(&s, "Level 1");
        assert_eq!(now.scale, 50.0, "the viewport takes the view's new scale");
        assert_eq!(paper(&s, plan), size(&s.model, s.model.elements.id(now.view).unwrap(), 50.0).unwrap(), "and is drawn at it");
        assert!(paper(&s, plan).0 > before.0 * 1.5, "{:?} against {before:?}", paper(&s, plan));
        let sheet = s.model.sheet_numbered("A101").unwrap();
        assert_eq!(title_values(&s.model, sheet).get("SCALE").map(String::as_str), Some("1 : 50"));
        // One placed at a scale of its own keeps it: its view's scale no longer moves it.
        let (section, pinned) = vp(&s, "Section 1");
        assert_eq!((pinned.scale, pinned.follow_scale), (50.0, false), "told a scale of its own");
        s.execute("scale", &serde_json::json!({"view": "Section 1", "scale": 20})).unwrap();
        let (_, kept) = vp(&s, "Section 1");
        assert_eq!(kept.scale, 50.0, "it keeps the scale it was placed at");
        assert_eq!(paper(&s, section), size(&s.model, s.model.elements.id(kept.view).unwrap(), 50.0).unwrap());
        let sheet = s.model.sheet_numbered("A102").unwrap();
        assert_eq!(title_values(&s.model, sheet).get("SCALE").map(String::as_str), Some("1 : 50"));
        // Given a scale and told to follow in the same step, it follows: it takes its view's scale, not the one given.
        s.execute("modify", &serde_json::json!({"id": section, "scale": 10, "follow_scale": true})).unwrap();
        let (_, now) = vp(&s, "Section 1");
        assert_eq!((now.scale, now.follow_scale), (20.0, true));
        s.execute("scale", &serde_json::json!({"view": "Section 1", "scale": 25})).unwrap();
        assert_eq!(vp(&s, "Section 1").1.scale, 25.0, "and follows it from then on");
        // Given a scale alone, it keeps it.
        s.execute("modify", &serde_json::json!({"id": section, "scale": 10})).unwrap();
        assert_eq!((vp(&s, "Section 1").1.scale, vp(&s, "Section 1").1.follow_scale), (10.0, false));
    }

    #[test]
    fn a_schedule_on_a_sheet_shows_its_own_fields_and_its_totals_in_the_projects_language() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               room {"point": [4000, 3000], "name": "Living"}
               sheet {"name": "Areas"}
               schedule {"name": "Room Areas", "category": "rooms", "fields": ["Name", {"name": "Area", "total": true}, {"name": "Big", "formula": "Area > 40", "kind": "yesno"}]}
               place {"sheet": "A101", "view": "Room Areas", "at": [100, 220]}"#,
        )
        .unwrap();
        let sc = s.model.elements.values().find_map(|e| if let Element::Schedule(sc) = e { Some(sc.clone()) } else { None }).unwrap();
        let t = table_of(&s.model, &sc);
        assert_eq!(t.headings, ["Name", "Area (m²)", "Big"]);
        assert_eq!(t.rows, [vec!["Living", "48.00", "Yes"], vec!["Total", "48.00", ""]]);
        assert!(t.totals);
        // A row more than its rows and headings: its totals.
        let vp = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Viewport(_)).then_some(*id)).unwrap();
        let (frame, _) = frame(&s.model, vp).unwrap();
        assert_eq!(frame.height as f32, TITLE_ROW + 3.0 * ROW);
        s.execute("language", &serde_json::json!({"code": "pt"})).unwrap();
        let t = table_of(&s.model, &sc);
        assert_eq!((t.headings[1].as_str(), t.rows[0][2].as_str(), t.rows[1][0].as_str()), ("Área (m²)", "Sim", "Total"));
    }

    #[test]
    fn schedules_3d_views_and_notes_go_on_sheets() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               room {"point": [4000, 3000], "name": "Living"}
               sheet {"name": "Schedules"}
               place {"sheet": "A101", "view": "Room Schedule", "at": [100, 220]}
               place {"sheet": "A101", "view": "{3D}", "at": [300, 150], "scale": 200}
               text {"view": "A101", "text": "Not for construction", "at": [30, 40]}"#,
        )
        .unwrap();
        // Walls 3 to 6, the room 7, the sheet 8; the schedule 9 and its viewport 10; the 3D view 11
        // and its viewport 12; the note 13.
        let d = draw(&s.model, 100007).unwrap();
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 120 && g < 120 && b < 120
        };
        // The schedule's rules: its outline's top left corner, and the line under its headings.
        let (table, (ox, oy)) = frame(&s.model, 100009).unwrap();
        assert!(table.width > 200 && table.height > 100, "{table:?}");
        assert!(dark((ox + 0.5, oy + 0.5)), "its outline");
        assert_eq!(d.pick((ox + 4.0, oy + 4.0), 2.0), Some(100009));
        // The 3D view's picture: shaded faces, not paper, somewhere in its frame.
        let (picture, (x3, y3)) = frame(&s.model, 100011).unwrap();
        let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let shaded = (0..picture.width)
            .flat_map(|x| (0..picture.height).map(move |y| (x, y)))
            .filter_map(|(x, y)| pm.pixel(x3 as u32 + x, y3 as u32 + y))
            .filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255))
            .count();
        assert!(shaded > 100, "{shaded}");
        // The note, on the paper.
        let at = d.view.px(P2(30.0, 40.0));
        assert_eq!(d.pick((at.0 + 1.0, at.1 + 6.0), 2.0), Some(100012));
    }

    #[test]
    fn a_3d_view_on_a_sheet_is_drawn_in_its_visual_style() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               sheet {"name": "Views"}
               place {"sheet": "A101", "view": "{3D}", "at": [300, 150], "scale": 200}"#,
        )
        .unwrap();
        // Walls 3 to 6, the sheet 7, the 3D view 8 and its viewport 9.
        let inked = |s: &Session| {
            let d = draw(&s.model, 100006).unwrap();
            let (picture, (x, y)) = frame(&s.model, 100008).unwrap();
            let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
            (0..picture.width)
                .flat_map(|px| (0..picture.height).map(move |py| (px, py)))
                .filter_map(|(px, py)| pm.pixel(x as u32 + px, y as u32 + py))
                .filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255))
                .count()
        };
        let shaded = inked(&s);
        s.execute("visual_style", &serde_json::json!({"view": 100007, "style": "wireframe"})).unwrap();
        let wire = inked(&s);
        assert!(wire > 50 && wire < shaded / 4, "{wire} of {shaded}");
    }
}
