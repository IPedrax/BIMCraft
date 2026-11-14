//! Alignment guides (smart snapping) for tools such as wall drawing.
//!
//! Detects when the cursor aligns with reference points of existing walls (endpoints, midpoints,
//! faces, and axis intersections) along horizontal (Y), vertical (X), and diagonal axes, and when the
//! direction being drawn is parallel or square to a line already drawn, providing visual guide lines,
//! reference markers, and magnetic snapping.

use bimcraft_i18n::ui_msg;

use crate::geom::{self, P2};
use crate::model::{Element, Id, Model, TOL};
use crate::walls::{self, Axis};

/// Priority of reference points: Endpoint (0) > Midpoint (1) > Face (2) > Intersection (3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceType {
    Endpoint = 0,
    Midpoint = 1,
    Face = 2,
    Intersection = 3,
}

impl ReferenceType {
    pub fn name(self) -> &'static str {
        match self {
            ReferenceType::Endpoint => "Endpoint",
            ReferenceType::Midpoint => "Midpoint",
            ReferenceType::Face => "Face",
            ReferenceType::Intersection => "Intersection",
        }
    }
}

/// The axis of the alignment guide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignmentAxis {
    /// Vertical guide: cursor x-coordinate aligns with reference point's x.
    X,
    /// Horizontal guide: cursor y-coordinate aligns with reference point's y.
    Y,
    /// 45-degree diagonal guide (y - x = constant).
    Angle45,
    /// 135-degree diagonal guide (y + x = constant).
    Angle135,
    /// A direction borrowed from a line the cursor is near, through the point being drawn from:
    /// parallel to that line, or square to it.
    Inferred { along: P2, of: Inference },
}

/// Which way an [`AlignmentAxis::Inferred`] guide runs against the line it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inference {
    Parallel,
    Perpendicular,
}

impl AlignmentAxis {
    /// The direction the guide runs in, a unit vector.
    pub fn direction(self) -> P2 {
        match self {
            AlignmentAxis::X => P2(0.0, 1.0),
            AlignmentAxis::Y => P2(1.0, 0.0),
            AlignmentAxis::Angle45 => P2(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
            AlignmentAxis::Angle135 => P2(-std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
            AlignmentAxis::Inferred { along, .. } => along,
        }
    }

    /// What it's called, in English: what the cursor says it is lining up with.
    pub fn name(self) -> &'static str {
        match self {
            AlignmentAxis::X => ui_msg!("Vertical"),
            AlignmentAxis::Y => ui_msg!("Horizontal"),
            AlignmentAxis::Angle45 => "45°",
            AlignmentAxis::Angle135 => "135°",
            AlignmentAxis::Inferred { of: Inference::Parallel, .. } => ui_msg!("Parallel"),
            AlignmentAxis::Inferred { of: Inference::Perpendicular, .. } => ui_msg!("Perpendicular"),
        }
    }
}

/// A line segment for visual guide overlay rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideLine {
    pub from: P2,
    pub to: P2,
}

/// A reference point extracted from an existing wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReferencePoint {
    pub point: P2,
    pub kind: ReferenceType,
    pub wall_id: Option<Id>,
}

/// The result of an alignment detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentResult {
    /// The effective snapped position for the cursor (taking active alignments into account).
    pub snapped_point: P2,
    /// The line connecting the reference point to the snapped point.
    pub guide_line: GuideLine,
    /// The point the guide runs through: the reference point on the existing element that produced it,
    /// or the point being drawn from when the direction was borrowed.
    pub reference_point: P2,
    /// The axis of this alignment.
    pub axis: AlignmentAxis,
    /// The type of reference point.
    pub kind: ReferenceType,
    /// The wall the reference point is on, when it is a wall's: what the window names as why the point lined up.
    pub wall_id: Option<Id>,
}

/// Configuration for alignment guide detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentConfig {
    pub enabled: bool,
    /// Tolerance in model units (mm).
    pub tolerance: f64,
    pub endpoints: bool,
    pub midpoints: bool,
    pub faces: bool,
    pub intersections: bool,
    pub angles: bool,
}

impl Default for AlignmentConfig {
    fn default() -> AlignmentConfig {
        AlignmentConfig {
            enabled: true,
            tolerance: 80.0,
            endpoints: true,
            midpoints: true,
            faces: true,
            intersections: true,
            angles: false,
        }
    }
}

fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

fn dedup_references(refs: &mut Vec<ReferencePoint>) {
    refs.sort_by_key(|r| r.kind as u8);
    // A grid of TOL-sized cells, so a point is only compared with those in its own and the next cells: n log n, not n squared.
    let cell = |p: P2| ((p.0 / TOL).floor() as i64, (p.1 / TOL).floor() as i64);
    let mut seen: std::collections::HashMap<(i64, i64), Vec<P2>> = std::collections::HashMap::new();
    refs.retain(|r| {
        let (cx, cy) = cell(r.point);
        let near = (cx.saturating_sub(1)..=cx.saturating_add(1)).any(|x| {
            (cy.saturating_sub(1)..=cy.saturating_add(1))
                .any(|y| seen.get(&(x, y)).is_some_and(|ps| ps.iter().any(|p| p.dist(r.point) <= TOL)))
        });
        if !near {
            seen.entry((cx, cy)).or_default().push(r.point);
        }
        !near
    });
}

/// A guide the cursor is riding: which way it runs, and the point it runs through.
#[derive(Clone, Copy, Debug)]
struct Guide {
    axis: AlignmentAxis,
    anchor: P2,
    kind: ReferenceType,
    wall_id: Option<Id>,
}

impl Guide {
    /// How far the cursor is off it, mm.
    fn deviation(self, p: P2) -> f64 {
        (p - self.anchor).cross(self.axis.direction()).abs()
    }

    /// The cursor's own point on it.
    fn project(self, p: P2) -> P2 {
        let d = self.axis.direction();
        self.anchor + d * ((p - self.anchor).dot(d))
    }

    fn result(self, snapped_point: P2) -> AlignmentResult {
        AlignmentResult {
            snapped_point,
            guide_line: GuideLine { from: self.anchor, to: snapped_point },
            reference_point: self.anchor,
            axis: self.axis,
            kind: self.kind,
            wall_id: self.wall_id,
        }
    }
}

/// Service that indexes wall reference points and detects alignment guides.
#[derive(Clone, Debug, Default)]
pub struct AlignmentGuideService {
    config: AlignmentConfig,
    references: Vec<ReferencePoint>,
    /// Indices of `references` sorted by X coordinate for fast O(log N) range queries.
    by_x: Vec<usize>,
    /// Indices of `references` sorted by Y coordinate for fast O(log N) range queries.
    by_y: Vec<usize>,
    /// The straight runs of what's drawn: a direction the cursor can borrow for a guide.
    lines: Vec<(P2, P2)>,
}

impl AlignmentGuideService {
    /// Creates a service from explicit reference points and configuration.
    pub fn new(mut references: Vec<ReferencePoint>, config: AlignmentConfig) -> Self {
        references.retain(|r| match r.kind {
            ReferenceType::Endpoint => config.endpoints,
            ReferenceType::Midpoint => config.midpoints,
            ReferenceType::Face => config.faces,
            ReferenceType::Intersection => config.intersections,
        });

        let mut by_x: Vec<usize> = (0..references.len()).collect();
        by_x.sort_by(|&a, &b| references[a].point.0.total_cmp(&references[b].point.0));

        let mut by_y: Vec<usize> = (0..references.len()).collect();
        by_y.sort_by(|&a, &b| references[a].point.1.total_cmp(&references[b].point.1));

        Self { config, references, by_x, by_y, lines: Vec::new() }
    }

    /// Extracts reference points from all elements visible/present on `level` in `model`,
    /// including walls, grids, runs (beams, pipes...), columns, floors, roofs, ceilings, shafts, and sections.
    pub fn from_model(m: &Model, level: Id, config: AlignmentConfig) -> Self {
        let level_guid = m.guid(level);
        let mut refs = Vec::new();
        let mut straight_lines: Vec<(P2, P2)> = Vec::new();

        // 1. Walls
        let mut walls_info = Vec::new();
        for (id, e) in m.elements.iter() {
            if let Element::Wall(w) = e
                && m.cut_by(w, level_guid)
            {
                let axis = Axis::of(w);
                let (f0, f1) = walls::faces(m, w);
                walls_info.push((Some(*id), axis, (f0, f1)));
                if axis.centre().is_none() && axis.length() > TOL {
                    straight_lines.push((axis.start, axis.end));
                }
            }
        }
        refs.extend(Self::extract_references(&walls_info, &config));

        // 2. Grids (visible across all levels)
        for (id, e) in m.elements.iter() {
            if let Element::Grid(g) = e {
                let len = g.start.dist(g.end);
                if len > TOL {
                    if config.endpoints {
                        refs.push(ReferencePoint { point: g.start, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                        refs.push(ReferencePoint { point: g.end, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                    }
                    if config.midpoints {
                        let mid = (g.start + g.end) * 0.5;
                        refs.push(ReferencePoint { point: mid, kind: ReferenceType::Midpoint, wall_id: Some(*id) });
                    }
                    straight_lines.push((g.start, g.end));
                }
            }
        }

        // 3. Columns, Structural Columns, Footings
        for (id, e) in m.elements.iter() {
            match e {
                Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) if c.level == level_guid && config.endpoints => {
                    refs.push(ReferencePoint { point: c.at, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                }
                _ => {}
            }
        }

        // 4. Runs (Beams, Pipes, Conduits, etc.)
        for (id, e) in m.elements.iter() {
            match e {
                crate::model::run_arm!(r) if r.level == level_guid => {
                    let len = r.start.dist(r.end);
                    if len > TOL {
                        if config.endpoints {
                            refs.push(ReferencePoint { point: r.start, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                            refs.push(ReferencePoint { point: r.end, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                        }
                        if config.midpoints {
                            let mid = (r.start + r.end) * 0.5;
                            refs.push(ReferencePoint { point: mid, kind: ReferenceType::Midpoint, wall_id: Some(*id) });
                        }
                        straight_lines.push((r.start, r.end));
                    }
                }
                _ => {}
            }
        }

        // 5. Sections and Elevations
        for (id, e) in m.elements.iter() {
            if let Element::Section(s) | Element::Elevation(s) = e {
                let len = s.start.dist(s.end);
                if len > TOL {
                    if config.endpoints {
                        refs.push(ReferencePoint { point: s.start, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                        refs.push(ReferencePoint { point: s.end, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                    }
                    if config.midpoints {
                        let mid = (s.start + s.end) * 0.5;
                        refs.push(ReferencePoint { point: mid, kind: ReferenceType::Midpoint, wall_id: Some(*id) });
                    }
                    straight_lines.push((s.start, s.end));
                }
            }
        }

        // 6. Floors, Roofs, Ceilings, Shafts
        for (id, e) in m.elements.iter() {
            let loops: Vec<&[P2]> = match e {
                Element::Floor(f) if f.level == level_guid => {
                    std::iter::once(f.boundary.as_slice()).chain(f.openings.iter().map(|c| c.as_slice())).collect()
                }
                Element::Roof(r) if r.level == level_guid => {
                    std::iter::once(r.boundary.as_slice()).chain(r.courtyards.iter().map(|c| c.as_slice())).collect()
                }
                Element::Ceiling(c) if c.level == level_guid => std::iter::once(c.boundary.as_slice()).collect(),
                Element::Shaft(s) => std::iter::once(s.boundary.as_slice()).collect(),
                _ => Vec::new(),
            };
            for loop_pts in loops {
                let n = loop_pts.len();
                for i in 0..n {
                    let p1 = loop_pts[i];
                    let p2 = loop_pts[(i + 1) % n];
                    if config.endpoints {
                        refs.push(ReferencePoint { point: p1, kind: ReferenceType::Endpoint, wall_id: Some(*id) });
                    }
                    if config.midpoints && p1.dist(p2) > TOL {
                        refs.push(ReferencePoint { point: (p1 + p2) * 0.5, kind: ReferenceType::Midpoint, wall_id: Some(*id) });
                    }
                }
            }
        }

        // 7. Intersections between straight lines across all element types
        if config.intersections && straight_lines.len() >= 2 {
            for (i, &(a1, b1)) in straight_lines.iter().enumerate() {
                for &(a2, b2) in straight_lines.iter().skip(i + 1) {
                    if let Some(q) = geom::line_intersection(a1, b1, a2, b2) {
                        let on_1 = geom::dist_to_segment(q, a1, b1) <= TOL;
                        let on_2 = geom::dist_to_segment(q, a2, b2) <= TOL;
                        if on_1 && on_2 {
                            let near_end = q.dist(a1) <= TOL || q.dist(b1) <= TOL || q.dist(a2) <= TOL || q.dist(b2) <= TOL;
                            if !near_end {
                                refs.push(ReferencePoint { point: q, kind: ReferenceType::Intersection, wall_id: None });
                            }
                        }
                    }
                }
            }
        }

        dedup_references(&mut refs);
        let mut service = Self::new(refs, config);
        service.lines = straight_lines;
        service
    }

    /// Extracts reference points from a list of wall tuples (id, axis, faces).
    pub fn from_walls(walls: &[(Option<Id>, Axis, (f64, f64))], config: AlignmentConfig) -> Self {
        let refs = Self::extract_references(walls, &config);
        Self::new(refs, config)
    }

    /// Extracts reference points (endpoints, midpoints, faces, intersections) from walls.
    pub fn extract_references(walls: &[(Option<Id>, Axis, (f64, f64))], config: &AlignmentConfig) -> Vec<ReferencePoint> {
        let mut refs = Vec::new();

        for &(id, axis, faces) in walls {
            let len = axis.length();
            if len <= TOL {
                continue;
            }

            // Endpoints
            if config.endpoints {
                refs.push(ReferencePoint { point: axis.start, kind: ReferenceType::Endpoint, wall_id: id });
                refs.push(ReferencePoint { point: axis.end, kind: ReferenceType::Endpoint, wall_id: id });
            }

            // Midpoint
            if config.midpoints {
                let mid = axis.at(len / 2.0, 0.0);
                refs.push(ReferencePoint { point: mid, kind: ReferenceType::Midpoint, wall_id: id });
            }

            // Faces
            if config.faces {
                let (f0, f1) = faces;
                for f in [f0, f1] {
                    if f.abs() > TOL {
                        refs.push(ReferencePoint { point: axis.at(0.0, f), kind: ReferenceType::Face, wall_id: id });
                        refs.push(ReferencePoint { point: axis.at(len, f), kind: ReferenceType::Face, wall_id: id });
                        refs.push(ReferencePoint { point: axis.at(len / 2.0, f), kind: ReferenceType::Face, wall_id: id });
                    }
                }
            }
        }

        // Intersections between straight wall axes
        if config.intersections && walls.len() >= 2 {
            for (i, &(_, a_i, _)) in walls.iter().enumerate() {
                if a_i.centre().is_some() {
                    continue; // Skip curved walls for axis intersection
                }
                for &(_, a_j, _) in walls.iter().skip(i + 1) {
                    if a_j.centre().is_some() {
                        continue;
                    }
                    if let Some(q) = geom::line_intersection(a_i.start, a_i.end, a_j.start, a_j.end) {
                        let on_i = geom::dist_to_segment(q, a_i.start, a_i.end) <= TOL;
                        let on_j = geom::dist_to_segment(q, a_j.start, a_j.end) <= TOL;
                        if on_i && on_j {
                            // Only add if not already coinciding with an endpoint
                            let near_end =
                                q.dist(a_i.start) <= TOL || q.dist(a_i.end) <= TOL || q.dist(a_j.start) <= TOL || q.dist(a_j.end) <= TOL;
                            if !near_end {
                                refs.push(ReferencePoint { point: q, kind: ReferenceType::Intersection, wall_id: None });
                            }
                        }
                    }
                }
            }
        }

        refs
    }

    /// Finds alignments for `cursor`: the axis guides of the reference points near it, and a direction
    /// borrowed from the lines drawn, all through `start_point` where they are borrowed ones.
    pub fn find_alignments(&self, cursor: P2, start_point: Option<P2>, tolerance: f64) -> Vec<AlignmentResult> {
        if !self.config.enabled || (self.references.is_empty() && self.lines.is_empty()) || !tolerance.is_finite() || tolerance <= 0.0 {
            return Vec::new();
        }

        let is_self = |p: P2| {
            if let Some(start) = start_point { p.dist(start) <= TOL } else { false }
        };

        // 1. Find best X alignment (vertical guide: R.x == cursor.x within tolerance)
        let mut best_x: Option<(ReferencePoint, f64, f64)> = None; // (ref, dist_to_cursor, dev)
        let lo_x = self.by_x.partition_point(|&idx| self.references[idx].point.0 < cursor.0 - tolerance);
        let hi_x = self.by_x.partition_point(|&idx| self.references[idx].point.0 <= cursor.0 + tolerance);

        for &idx in &self.by_x[lo_x..hi_x] {
            let r = self.references[idx];
            if is_self(r.point) {
                continue;
            }
            let dev = (r.point.0 - cursor.0).abs();
            let dist = r.point.dist(cursor);
            let is_better = match &best_x {
                None => true,
                Some((prev, prev_dist, prev_dev)) => (r.kind as u8, dist, dev) < (prev.kind as u8, *prev_dist, *prev_dev),
            };
            if is_better {
                best_x = Some((r, dist, dev));
            }
        }

        // 2. Find best Y alignment (horizontal guide: R.y == cursor.y within tolerance)
        let mut best_y: Option<(ReferencePoint, f64, f64)> = None;
        let lo_y = self.by_y.partition_point(|&idx| self.references[idx].point.1 < cursor.1 - tolerance);
        let hi_y = self.by_y.partition_point(|&idx| self.references[idx].point.1 <= cursor.1 + tolerance);

        for &idx in &self.by_y[lo_y..hi_y] {
            let r = self.references[idx];
            if is_self(r.point) {
                continue;
            }
            let dev = (r.point.1 - cursor.1).abs();
            let dist = r.point.dist(cursor);
            let is_better = match &best_y {
                None => true,
                Some((prev, prev_dist, prev_dev)) => (r.kind as u8, dist, dev) < (prev.kind as u8, *prev_dist, *prev_dev),
            };
            if is_better {
                best_y = Some((r, dist, dev));
            }
        }

        // 3. A direction borrowed from a straight run, through the point being drawn from: a line
        //    drawn beside a slanted wall comes out parallel to it.
        let mut guides: Vec<Guide> = Vec::with_capacity(3);
        if let Some((r, _, _)) = best_x {
            guides.push(Guide { axis: AlignmentAxis::X, anchor: r.point, kind: r.kind, wall_id: r.wall_id });
        }
        if let Some((r, _, _)) = best_y {
            guides.push(Guide { axis: AlignmentAxis::Y, anchor: r.point, kind: r.kind, wall_id: r.wall_id });
        }
        if let Some(start) = start_point
            && let Some(g) = self.inferred(cursor, start, tolerance)
        {
            guides.push(g);
        }
        if guides.is_empty() {
            return if self.config.angles { self.find_diagonal_alignment(cursor, start_point, tolerance) } else { Vec::new() };
        }

        // Two guides crossing say where the point goes, and both are named; one says only one of its
        // coordinates, so the other stays under the cursor.
        let mut best: Option<(usize, usize, P2, f64)> = None;
        for (i, a) in guides.iter().enumerate() {
            for (j, b) in guides.iter().enumerate().skip(i + 1) {
                let Some(q) = geom::line_intersection(a.anchor, a.anchor + a.axis.direction(), b.anchor, b.anchor + b.axis.direction())
                else {
                    continue;
                };
                // Nearly parallel guides meet far away: that is not the point the cursor is choosing.
                if q.dist(cursor) > tolerance * 2.0 {
                    continue;
                }
                let cost = a.deviation(cursor) + b.deviation(cursor);
                if best.is_none_or(|(.., cheapest)| cost < cheapest) {
                    best = Some((i, j, q, cost));
                }
            }
        }
        if let Some((i, j, q, _)) = best {
            let snapped_point = micron(q);
            return vec![guides[i].result(snapped_point), guides[j].result(snapped_point)];
        }
        let Some(g) = guides.iter().copied().min_by(|a, b| a.deviation(cursor).total_cmp(&b.deviation(cursor))) else {
            return Vec::new();
        };
        vec![g.result(micron(g.project(cursor)))]
    }

    /// The guide borrowed from the lines drawn, through `start`: the one whose direction the line
    /// being drawn is nearest, parallel to it or square to it. Only a direction is borrowed, never a
    /// place, so a wall across the plan makes as good a parallel as the one under the cursor.
    fn inferred(&self, cursor: P2, start: P2, tolerance: f64) -> Option<Guide> {
        let mut best: Option<(f64, AlignmentAxis)> = None;
        let v = cursor - start;
        for &(a, b) in &self.lines {
            let len = a.dist(b);
            if len <= TOL {
                continue;
            }
            let along = (b - a) * (1.0 / len);
            let square = P2(-along.1, along.0);
            for (off, axis) in [
                (v.cross(along).abs(), AlignmentAxis::Inferred { along, of: Inference::Parallel }),
                (v.cross(square).abs(), AlignmentAxis::Inferred { along: square, of: Inference::Perpendicular }),
            ] {
                // East and north are X's and Y's own guides, always on: a wall along one adds nothing.
                let d = axis.direction();
                if d.0.abs() < 0.02 || d.1.abs() < 0.02 {
                    continue;
                }
                if off <= tolerance && best.is_none_or(|(nearest, _)| off < nearest) {
                    best = Some((off, axis));
                }
            }
        }
        best.map(|(_, axis)| Guide { axis, anchor: start, kind: ReferenceType::Endpoint, wall_id: None })
    }

    /// Checks 45° and 135° diagonal alignments if enabled.
    fn find_diagonal_alignment(&self, cursor: P2, start_point: Option<P2>, tolerance: f64) -> Vec<AlignmentResult> {
        let is_self = |p: P2| {
            if let Some(start) = start_point { p.dist(start) <= TOL } else { false }
        };

        let inv_sqrt2 = 1.0 / std::f64::consts::SQRT_2;
        let mut best: Option<(AlignmentAxis, ReferencePoint, P2, f64)> = None;

        for &r in &self.references {
            if is_self(r.point) {
                continue;
            }

            // 45° line: (y - R.y) = (x - R.x) => (x - y) - (R.x - R.y) = 0
            let d45 = ((cursor.0 - cursor.1) - (r.point.0 - r.point.1)).abs() * inv_sqrt2;
            if d45 <= tolerance {
                let u = P2(1.0, 1.0) * inv_sqrt2;
                let proj = r.point + u * ((cursor - r.point).dot(u));
                let dist = r.point.dist(cursor);
                let is_better = match &best {
                    None => true,
                    Some((_, prev_r, _, prev_dist)) => (r.kind as u8, dist) < (prev_r.kind as u8, *prev_dist),
                };
                if is_better {
                    best = Some((AlignmentAxis::Angle45, r, proj, dist));
                }
            }

            // 135° line: (y - R.y) = -(x - R.x) => (x + y) - (R.x + R.y) = 0
            let d135 = ((cursor.0 + cursor.1) - (r.point.0 + r.point.1)).abs() * inv_sqrt2;
            if d135 <= tolerance {
                let u = P2(-1.0, 1.0) * inv_sqrt2;
                let proj = r.point + u * ((cursor - r.point).dot(u));
                let dist = r.point.dist(cursor);
                let is_better = match &best {
                    None => true,
                    Some((_, prev_r, _, prev_dist)) => (r.kind as u8, dist) < (prev_r.kind as u8, *prev_dist),
                };
                if is_better {
                    best = Some((AlignmentAxis::Angle135, r, proj, dist));
                }
            }
        }

        if let Some((axis, r, proj, _)) = best {
            let p = micron(proj);
            vec![AlignmentResult {
                snapped_point: p,
                guide_line: GuideLine { from: r.point, to: p },
                reference_point: r.point,
                axis,
                kind: r.kind,
                wall_id: r.wall_id,
            }]
        } else {
            Vec::new()
        }
    }

    pub fn references(&self) -> &[ReferencePoint] {
        &self.references
    }

    pub fn config(&self) -> &AlignmentConfig {
        &self.config
    }
}

/// Standalone convenience function to find alignments against references.
pub fn find_alignments(cursor: P2, start_point: Option<P2>, references: &[ReferencePoint], tolerance: f64) -> Vec<AlignmentResult> {
    let service = AlignmentGuideService::new(references.to_vec(), AlignmentConfig { tolerance, ..Default::default() });
    service.find_alignments(cursor, start_point, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alignment_x() {
        let refs = vec![ReferencePoint { point: P2(2000.0, 5000.0), kind: ReferenceType::Endpoint, wall_id: None }];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor near X = 2000.0 (deviation 5mm, within tolerance 80mm)
        let cursor = P2(2005.0, 1000.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 80.0);

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].axis, AlignmentAxis::X);
        assert_eq!(res[0].reference_point, P2(2000.0, 5000.0));
        assert_eq!(res[0].snapped_point, P2(2000.0, 1000.0));
        assert_eq!(res[0].guide_line.from, P2(2000.0, 5000.0));
        assert_eq!(res[0].guide_line.to, P2(2000.0, 1000.0));
    }

    #[test]
    fn test_alignment_y() {
        let refs = vec![ReferencePoint { point: P2(5000.0, 3000.0), kind: ReferenceType::Endpoint, wall_id: None }];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor near Y = 3000.0 (deviation 4mm, within tolerance 80mm)
        let cursor = P2(1000.0, 2996.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 80.0);

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].axis, AlignmentAxis::Y);
        assert_eq!(res[0].reference_point, P2(5000.0, 3000.0));
        assert_eq!(res[0].snapped_point, P2(1000.0, 3000.0));
        assert_eq!(res[0].guide_line.from, P2(5000.0, 3000.0));
        assert_eq!(res[0].guide_line.to, P2(1000.0, 3000.0));
    }

    #[test]
    fn test_alignment_dual_xy() {
        let refs = vec![
            ReferencePoint { point: P2(2000.0, 8000.0), kind: ReferenceType::Endpoint, wall_id: None },
            ReferencePoint { point: P2(7000.0, 4000.0), kind: ReferenceType::Endpoint, wall_id: None },
        ];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor near X = 2000 and Y = 4000
        let cursor = P2(2003.0, 3998.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 80.0);

        assert_eq!(res.len(), 2);
        assert_eq!(res[0].axis, AlignmentAxis::X);
        assert_eq!(res[1].axis, AlignmentAxis::Y);
        // Snapped point locks to the intersection of both alignment lines: (2000, 4000)
        assert_eq!(res[0].snapped_point, P2(2000.0, 4000.0));
        assert_eq!(res[1].snapped_point, P2(2000.0, 4000.0));
        assert_eq!(res[0].guide_line.from, P2(2000.0, 8000.0));
        assert_eq!(res[1].guide_line.from, P2(7000.0, 4000.0));
    }

    #[test]
    fn test_no_alignment() {
        let refs = vec![ReferencePoint { point: P2(2000.0, 2000.0), kind: ReferenceType::Endpoint, wall_id: None }];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor far from both X = 2000 and Y = 2000 (tolerance 10mm)
        let cursor = P2(2050.0, 2050.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 10.0);

        assert!(res.is_empty());
    }

    #[test]
    fn test_tie_breaking_priority_endpoint_over_midpoint() {
        let refs = vec![
            ReferencePoint { point: P2(2000.0, 1000.0), kind: ReferenceType::Midpoint, wall_id: None },
            ReferencePoint { point: P2(2000.0, 3000.0), kind: ReferenceType::Endpoint, wall_id: None },
        ];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor closer to Midpoint in Y (dist to 1000 is 400mm vs dist to 3000 is 1600mm),
        // Y is far from both (>80mm tolerance) so only X aligns.
        // Endpoint has higher priority than Midpoint, so Endpoint wins X alignment.
        let cursor = P2(2002.0, 1400.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 80.0);

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].kind, ReferenceType::Endpoint);
        assert_eq!(res[0].reference_point, P2(2000.0, 3000.0));
    }

    #[test]
    fn test_tie_breaking_closer_distance_same_priority() {
        let refs = vec![
            ReferencePoint { point: P2(2000.0, 5000.0), kind: ReferenceType::Endpoint, wall_id: None },
            ReferencePoint { point: P2(2000.0, 1200.0), kind: ReferenceType::Endpoint, wall_id: None },
        ];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        // Cursor at (2002, 1000): distance to (2000, 1200) is ~200mm, to (2000, 5000) is ~4000mm
        let cursor = P2(2002.0, 1000.0);
        let res = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 80.0);

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].reference_point, P2(2000.0, 1200.0));
    }

    #[test]
    fn test_zoom_dependent_tolerance() {
        let refs = vec![ReferencePoint { point: P2(2000.0, 5000.0), kind: ReferenceType::Endpoint, wall_id: None }];
        let service = AlignmentGuideService::new(refs, AlignmentConfig::default());

        let cursor = P2(2050.0, 1000.0); // 50mm off X = 2000

        // Screen tolerance 8px at zoom 0.1 px/mm => 80mm model tolerance => ALIGNS
        let tol_zoomed_out = 8.0 / 0.1;
        let res_zoomed_out = service.find_alignments(cursor, Some(P2(0.0, 0.0)), tol_zoomed_out);
        assert_eq!(res_zoomed_out.len(), 1);

        // Screen tolerance 8px at zoom 1.0 px/mm => 8mm model tolerance => DOES NOT ALIGN
        let tol_zoomed_in = 8.0 / 1.0;
        let res_zoomed_in = service.find_alignments(cursor, Some(P2(0.0, 0.0)), tol_zoomed_in);
        assert!(res_zoomed_in.is_empty());
    }

    #[test]
    fn a_thousand_walls_are_indexed_and_answer_a_thousand_queries() {
        let mut walls = Vec::with_capacity(1000);
        for i in 0..1000 {
            let x = (i % 50) as f64 * 3000.0;
            let y = (i / 50) as f64 * 3000.0;
            let axis = Axis::new(P2(x, y), P2(x + 2500.0, y), 0.0);
            walls.push((Some(100000 + i as u64), axis, (100.0, -100.0)));
        }

        let config = AlignmentConfig::default();
        let service = AlignmentGuideService::from_walls(&walls, config);
        assert!(service.references().len() >= 2000);

        // No timing is asserted (the project keeps none in tests): the queries must simply all answer.
        for i in 0..1000 {
            let cursor = P2((i as f64 * 150.0) % 150000.0, (i as f64 * 150.0) % 60000.0);
            let _ = service.find_alignments(cursor, Some(P2(0.0, 0.0)), 50.0);
        }
    }
}

#[cfg(test)]
mod why_tests {
    use super::*;
    use crate::engine::Session;

    /// A slanted wall, a level, and the service over them: what a line drawn beside it borrows.
    fn beside_a_slanted_wall() -> (Session, Id) {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0, 0], [4000, 2309]]}"#).unwrap();
        let level = s.model.levels()[0].0;
        (s, level)
    }

    /// A guide the cursor is near runs through the point being drawn from, parallel to the wall beside
    /// it: Plasticity's `Parallel`, the direction taken and made exact.
    #[test]
    fn a_line_borrows_the_direction_of_the_wall_beside_it() {
        let (s, level) = beside_a_slanted_wall();
        let config = AlignmentConfig { angles: false, ..AlignmentConfig::default() };
        let service = AlignmentGuideService::from_model(&s.model, level, config);
        let start = P2(0.0, 2000.0);
        let along = P2(4000.0, 2309.0) * (1.0 / P2(4000.0, 2309.0).length());
        // A thousand millimetres out along the wall's direction, 7 mm off it.
        let cursor = start + along * 1000.0 + P2(-along.1, along.0) * 7.0;

        let found = service.find_alignments(cursor, Some(start), 80.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(matches!(found[0].axis, AlignmentAxis::Inferred { of: Inference::Parallel, .. }), "{found:?}");
        assert!(found[0].axis.direction().cross(along).abs() < 1e-9, "the wall's own direction");
        assert_eq!(found[0].reference_point, start, "through the point being drawn from");
        assert!(found[0].snapped_point.dist(cursor) <= 80.0, "on the guide");
        assert!((found[0].snapped_point - start).cross(along).abs() < 1e-3, "the snapped point is on it");
    }

    /// Square to the line beside it, the same guide turned by ninety degrees.
    #[test]
    fn a_line_borrows_the_direction_square_to_the_wall_beside_it() {
        let (s, level) = beside_a_slanted_wall();
        let config = AlignmentConfig { angles: false, ..AlignmentConfig::default() };
        let service = AlignmentGuideService::from_model(&s.model, level, config);
        let start = P2(0.0, 2000.0);
        let along = P2(4000.0, 2309.0) * (1.0 / P2(4000.0, 2309.0).length());
        let square = P2(-along.1, along.0);
        let cursor = start + square * 1000.0 + along * 7.0;

        let found = service.find_alignments(cursor, Some(start), 80.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(matches!(found[0].axis, AlignmentAxis::Inferred { of: Inference::Perpendicular, .. }), "{found:?}");
        assert!(found[0].snapped_point.dist(cursor) <= 80.0);
        assert!((found[0].snapped_point - start).dot(along).abs() < 1e-3, "square to the wall, through the start");
    }

    /// A borrowed direction and an axis guide crossing: the point is where they meet, as Plasticity's
    /// `Intersection, Parallel` locks both degrees of freedom at once.
    #[test]
    fn a_borrowed_direction_crosses_an_axis_guide_where_the_two_meet() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0, 0], [4000, 2309]]}"#).unwrap();
        s.script(r#"wall {"points": [[1000, 6000], [1000, 7000]]}"#).unwrap();
        let level = s.model.levels()[0].0;
        let config = AlignmentConfig { angles: false, ..AlignmentConfig::default() };
        let service = AlignmentGuideService::from_model(&s.model, level, config);
        let start = P2(0.0, 2000.0);
        // Near the vertical wall's end (x = 1000) and near the slanted wall's parallel through `start`.
        let cursor = P2(1005.0, 2580.0);

        let found = service.find_alignments(cursor, Some(start), 80.0);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].axis, AlignmentAxis::X);
        assert!(matches!(found[1].axis, AlignmentAxis::Inferred { of: Inference::Parallel, .. }), "{found:?}");
        assert_eq!(found[0].snapped_point, found[1].snapped_point, "one point, both guides");
        assert!((found[0].snapped_point.0 - 1000.0).abs() < 1e-3, "on the vertical guide");
        assert!((found[0].snapped_point.1 - 2577.35).abs() < 0.1, "and on the borrowed one");
    }

    /// A direction a wall does not run in is not borrowed: no jump onto a line the cursor is nowhere near.
    #[test]
    fn a_direction_no_line_runs_in_is_not_borrowed() {
        let (s, level) = beside_a_slanted_wall();
        let config = AlignmentConfig { angles: false, ..AlignmentConfig::default() };
        let service = AlignmentGuideService::from_model(&s.model, level, config);
        let start = P2(0.0, 2000.0);
        let sixty = 60.0_f64.to_radians();
        let cursor = start + P2(sixty.cos(), sixty.sin()) * 1000.0;

        assert!(service.find_alignments(cursor, Some(start), 80.0).is_empty());
    }

    /// An alignment says which wall it lines up with, so the window can say why a point snapped.
    #[test]
    fn an_alignment_names_the_wall_it_lines_up_with() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0, 0], [5000, 0]]}"#).unwrap();
        let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
        let level = s.model.levels()[0].0;
        let config = AlignmentConfig {
            enabled: true,
            tolerance: 50.0,
            endpoints: true,
            midpoints: true,
            faces: true,
            intersections: true,
            angles: false,
        };
        let service = AlignmentGuideService::from_model(&s.model, level, config);
        let found = service.find_alignments(P2(5005.0, 3000.0), Some(P2(0.0, 3000.0)), 50.0);
        assert!(found.first().is_some_and(|a| a.wall_id == Some(wall)), "{found:?}");
    }
}
