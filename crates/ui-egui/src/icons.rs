//! The icons: monochrome, original BIMCraft artwork on a 24 × 24 grid. A single 1.5 pt pen is
//! used at every display size; compact glyphs leave out secondary details at 19 pt and below. Shapes
//! are centred in the smaller dimension of their allocation, including tall or wide rectangles. No
//! fill, embedded colour or third-party assets are used. The `icon_catalog` example exports these
//! outlines for review.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::TAU;

pub const STROKE_WIDTH: f32 = 1.5;

macro_rules! icon_names {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($name),* }
        impl Icon {
            pub const ALL: &'static [Self] = &[$(Self::$name),*];
            pub const fn name(self) -> &'static str {
                match self { $(Self::$name => stringify!($name)),* }
            }
        }
    };
}

icon_names! {
    Modify,
    Wall,
    Door,
    Window,
    Room,
    Level,
    Grid,
    Undo,
    Redo,
    Open,
    Save,
    Fit,
    ZoomIn,
    ZoomOut,
    ThinLines,
    Schedule,
    CloseInactive,
    Interface,
    Properties,
    TypeProperties,
    Delete,
    Filter,
    Close,
    Plan,
    Move,
    Copy,
    Rotate,
    MirrorPick,
    MirrorDraw,
    Align,
    Offset,
    Trim,
    Split,
    Dimension,
    Tag,
    TagAll,
    View3D,
    Section,
    Elevation,
    Sheet,
    PlaceView,
    Column,
    Beam,
    Footing,
    WallFooting,
    Pipe,
    Fixture,
    Fitting,
    Accessory,
    Conduit,
    CableTray,
    Wire,
    Equipment,
    Device,
    Luminaire,
    AirTerminal,
    DownConductor,
    Electrode,
    InspectionBox,
    GroundConductor,
    DataOutlet,
    VoiceOutlet,
    Rack,
    PatchPanel,
    Cabling,
    Stair,
    Roof,
    Theme,
    Text,
    Spot,
    Angular,
    Radial,
    Crop,
    Units,
    Line,
    Arc,
    Flip,
    Attach,
    Detach,
    Floor,
    Ceiling,
    Shaft,
    Mass,
    SlopeArrow,
    Ramp,
    Railing,
    ExtrusionRoof,
    Dormer,
    GableEdge,
    Materials,
    Component,
    LoadFamily,
    Link,
    Parameters,
    Measure,
    Snaps,
    Visibility,
    Template,
    Phasing,
    Options,
    SelectById,
    IdsOfSelection,
    Demolish,
    Paint,
    Unpaint,
    Favorite,
    FamilyEditor,
    TempHide,
    Reveal,
    Settings,
    Dock,
    Minimize,
    Expand,
    Search,
    Legend,
    Home,
    NewFile,
    Code,
    Check,
    Back,
    LineWeights,
    ObjectStyles,
    Rectangle,
    FilledRegion,
    MaskingRegion,
    FillPatterns,
    Circle,
    Inscribed,
    Circumscribed,
    CentreArc,
    TangentArc,
    Fillet,
    Spline,
    FitSpline,
    Ellipse,
    PartialEllipse,
    Polyline,
    PickLines,
    JoinPolyline,
    ExplodePolyline,
}

/// A single unfilled contour in grid coordinates, shared by egui and the catalog.
#[derive(Clone, Debug)]
pub struct Outline {
    pub points: Vec<Pos2>,
    pub closed: bool,
}

/// All contours of the selected drawing, in the 24 pt design space.
pub fn outlines(icon: Icon, compact: bool) -> Vec<Outline> {
    let mut g = Glyph::default();
    match icon {
        Icon::Modify => {
            // A pointer with a clear tip, one diagonal axis and an attached tail.
            g.closed(&[(5.0, 3.0), (5.0, 20.0), (10.0, 15.0), (13.0, 21.0), (16.0, 19.5), (13.0, 13.5), (20.0, 13.5)]);
        }
        Icon::Wall => {
            if compact {
                g.closed(&[(3.0, 7.0), (21.0, 7.0), (21.0, 17.0), (3.0, 17.0)]);
                g.line(&[(7.0, 17.0), (11.0, 7.0)]);
                g.line(&[(13.0, 17.0), (17.0, 7.0)]);
            } else {
                g.closed(&[(3.0, 7.0), (21.0, 7.0), (21.0, 17.0), (3.0, 17.0)]);
                g.line(&[(5.0, 17.0), (9.0, 7.0)]);
                g.line(&[(10.0, 17.0), (14.0, 7.0)]);
                g.line(&[(15.0, 17.0), (19.0, 7.0)]);
            }
        }
        Icon::Door => {
            // A plan-view opening between two short wall sections. The leaf
            // hinges at the left jamb; its swing stays clear of the wall depth.
            g.closed(&[(2.0, 16.0), (6.0, 16.0), (6.0, 20.0), (2.0, 20.0)]);
            g.closed(&[(18.0, 16.0), (22.0, 16.0), (22.0, 20.0), (18.0, 20.0)]);
            g.line(&[(6.0, 16.0), (6.0, 4.0)]);
            g.arc((6.0, 16.0), 12.0, -90.0, 0.0);
        }
        Icon::Window => {
            // Larger pane openings leave a slim frame and slim central mullions.
            // Compact sizes retain slightly more space between parallel strokes.
            g.closed(&[(2.0, 2.0), (22.0, 2.0), (22.0, 22.0), (2.0, 22.0)]);
            let panes = if compact { [(5.0, 10.5), (13.5, 19.0)] } else { [(4.5, 10.75), (13.25, 19.5)] };
            for (left, right) in panes {
                for (top, bottom) in panes {
                    g.closed(&[(left, top), (right, top), (right, bottom), (left, bottom)]);
                }
            }
        }
        Icon::Room => {
            // A bounded room with a centered two-line room label.
            g.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            g.line(&[(8.0, 10.0), (16.0, 10.0)]);
            g.line(&[(10.0, 14.0), (14.0, 14.0)]);
        }
        Icon::Level => {
            // The level head is clear of the datum until its tip, with a balanced line.
            g.line(&[(3.0, 16.0), (21.0, 16.0)]);
            g.closed(&[(12.0, 7.0), (20.0, 7.0), (16.0, 16.0)]);
        }
        Icon::Grid => {
            // A centered grid bubble on its datum axis.
            g.line(&[(12.0, 12.0), (12.0, 21.0)]);
            g.circle((12.0, 7.0), 5.0);
        }
        Icon::Undo => {
            // A tangential return arrow; Redo is its exact reflection.
            g.line(&[(4.0, 9.0), (14.0, 9.0)]);
            g.line(&[(4.0, 9.0), (8.0, 5.0)]);
            g.line(&[(4.0, 9.0), (8.0, 13.0)]);
            g.line(&[(14.0, 19.0), (8.0, 19.0)]);
            g.arc((14.0, 14.0), 5.0, -90.0, 90.0);
        }
        Icon::Redo => {
            // Exact mirror of Undo.
            return outlines(Icon::Undo, compact)
                .into_iter()
                .map(|mut p| {
                    for point in &mut p.points {
                        point.x = 24.0 - point.x;
                    }
                    p
                })
                .collect();
        }
        Icon::Open => {
            // An open folder with a separate angled front flap.
            g.line(&[(3.0, 19.0), (3.0, 5.0), (9.0, 5.0), (12.0, 8.0), (21.0, 8.0)]);
            g.closed(&[(3.0, 19.0), (6.0, 10.0), (22.0, 10.0), (19.0, 19.0)]);
        }
        Icon::Save => {
            // Preserves the current save/download-to-tray metaphor.
            g.line(&[(12.0, 3.0), (12.0, 14.0)]);
            g.line(&[(8.0, 10.0), (12.0, 14.0), (16.0, 10.0)]);
            g.line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Icon::Fit => {
            // Four equal right-angle fit corners.
            g.line(&[(3.0, 9.0), (3.0, 3.0), (9.0, 3.0)]);
            g.line(&[(15.0, 3.0), (21.0, 3.0), (21.0, 9.0)]);
            g.line(&[(21.0, 15.0), (21.0, 21.0), (15.0, 21.0)]);
            g.line(&[(9.0, 21.0), (3.0, 21.0), (3.0, 15.0)]);
        }
        Icon::ZoomIn => {
            g.line(&[(15.0, 15.0), (21.0, 21.0)]);
            g.line(&[(7.0, 10.0), (13.0, 10.0)]);
            g.line(&[(10.0, 7.0), (10.0, 13.0)]);
            g.circle((10.0, 10.0), 7.0);
        }
        Icon::ZoomOut => {
            g.line(&[(15.0, 15.0), (21.0, 21.0)]);
            g.line(&[(7.0, 10.0), (13.0, 10.0)]);
            g.circle((10.0, 10.0), 7.0);
        }
        Icon::ThinLines => {
            g.line(&[(3.0, 6.0), (21.0, 6.0)]);
            g.line(&[(3.0, 12.0), (21.0, 12.0)]);
            g.line(&[(3.0, 18.0), (21.0, 18.0)]);
        }
        Icon::Schedule => {
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.line(&[(3.0, 9.0), (21.0, 9.0)]);
            g.line(&[(9.0, 4.0), (9.0, 20.0)]);
            if !compact {
                g.line(&[(3.0, 14.5), (21.0, 14.5)]);
            }
        }
        Icon::CloseInactive => {
            // Close the extra views: a front view with an X and a rear view corner.
            g.line(&[(3.0, 7.0), (3.0, 3.0), (15.0, 3.0)]);
            g.closed(&[(7.0, 7.0), (21.0, 7.0), (21.0, 21.0), (7.0, 21.0)]);
            g.line(&[(11.0, 11.0), (17.0, 17.0)]);
            g.line(&[(17.0, 11.0), (11.0, 17.0)]);
        }
        Icon::Interface => {
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.line(&[(3.0, 9.0), (21.0, 9.0)]);
            g.line(&[(9.0, 9.0), (9.0, 20.0)]);
        }
        Icon::Properties => {
            g.closed(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(8.0, 8.0), (16.0, 8.0)]);
            g.line(&[(8.0, 12.0), (16.0, 12.0)]);
            g.line(&[(8.0, 16.0), (13.0, 16.0)]);
        }
        Icon::TypeProperties => {
            // A centered T on the same properties sheet.
            g.closed(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(8.0, 8.0), (16.0, 8.0)]);
            g.line(&[(12.0, 8.0), (12.0, 17.0)]);
        }
        Icon::Delete => {
            // A waste bin for deletion, now distinct from dismissing a panel.
            if compact {
                g.line(&[(4.0, 6.0), (20.0, 6.0)]);
                g.line(&[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)]);
                g.line(&[(6.0, 6.0), (7.0, 21.0), (17.0, 21.0), (18.0, 6.0)]);
                g.line(&[(12.0, 10.0), (12.0, 17.0)]);
            } else {
                g.line(&[(4.0, 6.0), (20.0, 6.0)]);
                g.line(&[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)]);
                g.line(&[(6.0, 6.0), (7.0, 21.0), (17.0, 21.0), (18.0, 6.0)]);
                g.line(&[(10.0, 10.0), (10.0, 17.0)]);
                g.line(&[(14.0, 10.0), (14.0, 17.0)]);
            }
        }
        Icon::Filter => {
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (14.0, 12.0), (14.0, 19.0), (10.0, 21.0), (10.0, 12.0)]);
        }
        Icon::Close => {
            g.line(&[(5.0, 5.0), (19.0, 19.0)]);
            g.line(&[(19.0, 5.0), (5.0, 19.0)]);
        }
        Icon::Plan => {
            // A plan with connected internal partitions and a deliberate opening.
            g.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            g.line(&[(4.0, 12.0), (12.0, 12.0), (12.0, 20.0)]);
            g.line(&[(15.0, 4.0), (15.0, 9.0)]);
        }
        Icon::Move => {
            g.line(&[(12.0, 3.0), (12.0, 21.0)]);
            g.line(&[(3.0, 12.0), (21.0, 12.0)]);
            g.line(&[(8.5, 6.5), (12.0, 3.0), (15.5, 6.5)]);
            g.line(&[(8.5, 17.5), (12.0, 21.0), (15.5, 17.5)]);
            g.line(&[(6.5, 8.5), (3.0, 12.0), (6.5, 15.5)]);
            g.line(&[(17.5, 8.5), (21.0, 12.0), (17.5, 15.5)]);
        }
        Icon::Copy => {
            // Equal overlapping rectangles with hidden rear edges removed.
            g.closed(&[(8.0, 3.0), (20.0, 3.0), (20.0, 15.0), (8.0, 15.0)]);
            g.line(&[(8.0, 9.0), (4.0, 9.0), (4.0, 21.0), (16.0, 21.0), (16.0, 15.0)]);
        }
        Icon::Rotate => {
            // A circular arc with a tangential return arrow; no floating arrowhead.
            g.line(&[(1.5, 15.0), (4.0, 12.0), (6.5, 15.0)]);
            g.arc((12.0, 12.0), 8.0, -90.0, 180.0);
        }
        Icon::MirrorPick => {
            // Mirrored triangles around a dashed picked axis.
            g.closed(&[(3.0, 18.0), (8.0, 7.0), (8.0, 18.0)]);
            g.closed(&[(21.0, 18.0), (16.0, 7.0), (16.0, 18.0)]);
            g.line(&[(12.0, 3.0), (12.0, 6.0)]);
            g.line(&[(12.0, 10.0), (12.0, 14.0)]);
            g.line(&[(12.0, 18.0), (12.0, 21.0)]);
        }
        Icon::MirrorDraw => {
            // Same mirrored shapes, with a drawn axis and endpoint ticks.
            g.closed(&[(3.0, 18.0), (8.0, 7.0), (8.0, 18.0)]);
            g.closed(&[(21.0, 18.0), (16.0, 7.0), (16.0, 18.0)]);
            g.line(&[(12.0, 3.0), (12.0, 21.0)]);
            g.line(&[(10.0, 3.0), (14.0, 3.0)]);
            g.line(&[(10.0, 21.0), (14.0, 21.0)]);
        }
        Icon::Align => {
            g.line(&[(4.0, 3.0), (4.0, 21.0)]);
            g.line(&[(20.0, 3.0), (20.0, 21.0)]);
            g.line(&[(16.0, 12.0), (8.0, 12.0)]);
            g.line(&[(11.0, 9.0), (8.0, 12.0), (11.0, 15.0)]);
        }
        Icon::Offset => {
            // Parallel lines with a centered perpendicular offset arrow.
            g.line(&[(3.0, 14.0), (14.0, 3.0)]);
            g.line(&[(10.0, 21.0), (21.0, 10.0)]);
            g.line(&[(8.5, 8.5), (15.0, 15.0)]);
            g.line(&[(15.0, 11.0), (15.0, 15.0), (11.0, 15.0)]);
        }
        Icon::Trim => {
            // A clean right-angle join and a short leftover segment.
            g.line(&[(5.0, 21.0), (5.0, 5.0), (21.0, 5.0)]);
            g.line(&[(5.0, 5.0), (2.0, 2.0)]);
        }
        Icon::Split => {
            // Two separated segments with a narrow cut mark.
            g.closed(&[(3.0, 8.0), (9.0, 8.0), (9.0, 16.0), (3.0, 16.0)]);
            g.closed(&[(15.0, 8.0), (21.0, 8.0), (21.0, 16.0), (15.0, 16.0)]);
            g.line(&[(13.0, 3.0), (11.0, 21.0)]);
        }
        Icon::Dimension => {
            g.line(&[(4.0, 5.0), (4.0, 21.0)]);
            g.line(&[(20.0, 5.0), (20.0, 21.0)]);
            g.line(&[(2.0, 16.0), (22.0, 16.0)]);
            g.line(&[(2.0, 18.0), (6.0, 14.0)]);
            g.line(&[(18.0, 18.0), (22.0, 14.0)]);
            g.line(&[(9.0, 9.0), (15.0, 9.0)]);
        }
        Icon::Tag => {
            // A readable tag box with a connected leader.
            g.closed(&[(8.0, 4.0), (21.0, 4.0), (21.0, 13.0), (8.0, 13.0)]);
            g.line(&[(8.0, 13.0), (3.0, 20.0)]);
            g.line(&[(12.0, 8.5), (17.0, 8.5)]);
        }
        Icon::TagAll => {
            // Two separated annotation tags, avoiding three unreadable overlapping boxes.
            g.closed(&[(3.0, 3.0), (15.0, 3.0), (15.0, 10.0), (3.0, 10.0)]);
            g.closed(&[(7.0, 14.0), (21.0, 14.0), (21.0, 21.0), (7.0, 21.0)]);
            g.line(&[(9.0, 10.0), (5.0, 14.0)]);
            g.line(&[(7.0, 21.0), (3.0, 21.0)]);
            if !compact {
                g.line(&[(6.0, 6.5), (11.0, 6.5)]);
                g.line(&[(11.0, 17.5), (17.0, 17.5)]);
            }
        }
        Icon::View3D => {
            // Match the supplied house reference: a broad roof plane, slight
            // eave overhangs, an uninterrupted gable face and a tall doorway.
            g.closed(&[(6.75, 7.5), (15.75, 3.0), (21.0, 9.75), (12.0, 14.25)]);
            g.line(&[(2.85, 10.5), (6.75, 7.5)]);
            g.line(&[(3.5, 10.0), (3.5, 16.75), (12.0, 21.0), (20.5, 16.75), (20.5, 10.0)]);
            g.line(&[(12.0, 14.25), (12.0, 21.0)]);
            if compact {
                // Preserve the doorway, widening it to keep a clear interior.
                g.line(&[(5.75, 17.875), (5.75, 12.625), (9.25, 14.375), (9.25, 19.625)]);
            } else {
                g.line(&[(6.25, 18.125), (6.25, 12.875), (8.75, 14.125), (8.75, 19.375)]);
            }
        }
        Icon::Section => {
            // A section head, broken cut line and perpendicular end tick.
            g.closed(&[(3.0, 8.0), (3.0, 16.0), (9.0, 16.0)]);
            g.line(&[(9.0, 16.0), (12.0, 16.0)]);
            g.line(&[(15.0, 16.0), (18.0, 16.0)]);
            g.line(&[(21.0, 12.0), (21.0, 20.0)]);
        }
        Icon::Elevation => {
            // An elevation direction marker, with the triangle above the circular head.
            g.closed(&[(7.0, 10.0), (12.0, 3.0), (17.0, 10.0)]);
            g.circle((12.0, 16.0), 5.0);
        }
        Icon::Sheet => {
            // A drawing sheet and title block; shared edges are drawn once.
            g.closed(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)]);
            g.line(&[(13.0, 14.0), (21.0, 14.0)]);
            g.line(&[(13.0, 14.0), (13.0, 19.0)]);
            if !compact {
                g.line(&[(17.0, 14.0), (17.0, 19.0)]);
            }
        }
        Icon::PlaceView => {
            // A sheet with a placed viewport and an unobstructed add mark.
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.closed(&[(6.0, 7.0), (13.0, 7.0), (13.0, 14.0), (6.0, 14.0)]);
            g.line(&[(17.0, 11.0), (17.0, 17.0)]);
            g.line(&[(14.0, 14.0), (20.0, 14.0)]);
        }
        Icon::Column => {
            g.closed(&[
                (6.0, 4.0),
                (18.0, 4.0),
                (18.0, 7.0),
                (15.0, 7.0),
                (15.0, 17.0),
                (18.0, 17.0),
                (18.0, 20.0),
                (6.0, 20.0),
                (6.0, 17.0),
                (9.0, 17.0),
                (9.0, 7.0),
                (6.0, 7.0),
            ]);
            g.line(&[(9.0, 7.0), (15.0, 7.0)]);
            g.line(&[(9.0, 17.0), (15.0, 17.0)]);
        }
        Icon::Beam => {
            // A connected I-section with equal flange thickness and a centered web.
            g.closed(&[
                (4.0, 4.0),
                (20.0, 4.0),
                (20.0, 7.0),
                (14.0, 7.0),
                (14.0, 17.0),
                (20.0, 17.0),
                (20.0, 20.0),
                (4.0, 20.0),
                (4.0, 17.0),
                (10.0, 17.0),
                (10.0, 7.0),
                (4.0, 7.0),
            ]);
        }
        Icon::Footing => {
            // A centered column on an isolated pad, with a single joined silhouette.
            g.closed(&[(9.0, 4.0), (15.0, 4.0), (15.0, 12.0), (20.0, 12.0), (20.0, 19.0), (4.0, 19.0), (4.0, 12.0), (9.0, 12.0)]);
            g.line(&[(9.0, 12.0), (15.0, 12.0)]);
            if !compact {
                g.line(&[(4.0, 16.0), (20.0, 16.0)]);
            }
        }
        Icon::WallFooting => {
            // A broad wall over a continuous strip footing, distinct from the isolated pad.
            g.closed(&[(6.0, 4.0), (18.0, 4.0), (18.0, 12.0), (21.0, 12.0), (21.0, 19.0), (3.0, 19.0), (3.0, 12.0), (6.0, 12.0)]);
            g.line(&[(6.0, 12.0), (18.0, 12.0)]);
            if !compact {
                g.line(&[(9.0, 4.0), (9.0, 12.0)]);
                g.line(&[(15.0, 4.0), (15.0, 12.0)]);
            }
        }
        Icon::Pipe => {
            // Parallel barrel edges, a visible elliptical bore at the near end
            // and only the outside half of the far end. All joins are tangent.
            g.line(&[(4.0, 15.0), (15.0, 4.0)]);
            g.line(&[(9.0, 20.0), (20.0, 9.0)]);
            g.ellipse((6.5, 17.5), (2.5, 2.5), (-1.0, 1.0), true);
            if compact {
                g.line(&[(15.0, 4.0), (20.0, 9.0)]);
            } else {
                g.ellipse((17.5, 6.5), (2.5, 2.5), (-1.0, 1.0), false);
            }
        }
        Icon::Fixture => {
            // A washbasin with a curved bowl, tap and connected pedestal.
            g.line(&[(4.0, 10.0), (20.0, 10.0)]);
            g.line(&[(12.0, 10.0), (12.0, 5.0), (16.0, 5.0), (16.0, 7.0)]);
            g.line(&[(10.0, 16.0), (10.0, 20.0), (14.0, 20.0), (14.0, 16.0)]);
            g.cubic([(4.0, 10.0), (4.0, 18.0), (20.0, 18.0), (20.0, 10.0)]);
        }
        Icon::Fitting => {
            // A flanged pipe elbow with a smooth bend and consistent bore.
            g.line(&[(3.0, 5.0), (12.0, 5.0)]);
            g.line(&[(3.0, 11.0), (12.0, 11.0)]);
            g.line(&[(15.0, 14.0), (15.0, 21.0)]);
            g.line(&[(21.0, 14.0), (21.0, 21.0)]);
            g.line(&[(3.0, 3.0), (3.0, 13.0)]);
            g.line(&[(13.0, 21.0), (22.0, 21.0)]);
            g.cubic([(12.0, 5.0), (16.9706, 5.0), (21.0, 9.0294), (21.0, 14.0)]);
            g.cubic([(12.0, 11.0), (13.6569, 11.0), (15.0, 12.3431), (15.0, 14.0)]);
        }
        Icon::Accessory => {
            // A gate valve: two opposed cones, stem and handwheel.
            g.closed(&[(3.0, 10.0), (3.0, 20.0), (12.0, 15.0)]);
            g.closed(&[(21.0, 10.0), (21.0, 20.0), (12.0, 15.0)]);
            g.line(&[(12.0, 15.0), (12.0, 5.0)]);
            g.line(&[(8.0, 5.0), (16.0, 5.0)]);
        }
        Icon::Conduit => {
            // A bent electrical tube with concentric inner and outer bends.
            g.line(&[(4.0, 16.0), (12.0, 16.0)]);
            g.line(&[(4.0, 21.0), (12.0, 21.0)]);
            g.line(&[(15.0, 4.0), (15.0, 13.0)]);
            g.line(&[(20.0, 4.0), (20.0, 13.0)]);
            g.line(&[(15.0, 4.0), (20.0, 4.0)]);
            g.line(&[(4.0, 16.0), (4.0, 21.0)]);
            g.cubic([(12.0, 16.0), (13.6569, 16.0), (15.0, 14.6569), (15.0, 13.0)]);
            g.cubic([(12.0, 21.0), (16.4183, 21.0), (20.0, 17.4183), (20.0, 13.0)]);
        }
        Icon::CableTray => {
            // A ladder tray with three equally spaced rungs, taller than the window symbol.
            g.line(&[(3.0, 6.0), (21.0, 6.0)]);
            g.line(&[(3.0, 18.0), (21.0, 18.0)]);
            g.line(&[(6.0, 6.0), (6.0, 18.0)]);
            g.line(&[(12.0, 6.0), (12.0, 18.0)]);
            g.line(&[(18.0, 6.0), (18.0, 18.0)]);
        }
        Icon::Wire => {
            // A smooth continuous wire, replacing the zigzag that read as a resistor.
            g.cubic([(3.0, 15.0), (6.0, 3.0), (9.0, 3.0), (12.0, 12.0)]);
            g.cubic([(12.0, 12.0), (15.0, 21.0), (18.0, 21.0), (21.0, 9.0)]);
        }
        Icon::Equipment => {
            // A distribution board with a display and rows of breakers.
            g.closed(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]);
            g.closed(&[(8.0, 7.0), (16.0, 7.0), (16.0, 11.0), (8.0, 11.0)]);
            g.line(&[(8.0, 15.0), (16.0, 15.0)]);
            if !compact {
                g.line(&[(8.0, 18.0), (16.0, 18.0)]);
            }
        }
        Icon::Device => {
            // An electrical outlet plate, with two slots and a centered earth contact.
            g.closed(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(9.0, 8.0), (9.0, 12.0)]);
            g.line(&[(15.0, 8.0), (15.0, 12.0)]);
            g.line(&[(12.0, 16.0), (12.0, 18.0)]);
        }
        Icon::Luminaire => {
            // A pendant with a centered suspension, equal shade slopes and three light rays.
            g.line(&[(12.0, 3.0), (12.0, 8.0)]);
            g.closed(&[(8.0, 8.0), (16.0, 8.0), (20.0, 15.0), (4.0, 15.0)]);
            g.line(&[(12.0, 18.0), (12.0, 21.0)]);
            g.line(&[(7.0, 18.0), (5.0, 21.0)]);
            g.line(&[(17.0, 18.0), (19.0, 21.0)]);
        }
        Icon::AirTerminal => {
            // The Lightning Protection tool: a lightning rod and base, not an HVAC diffuser.
            g.line(&[(12.0, 3.0), (12.0, 17.0)]);
            g.line(&[(10.0, 6.0), (12.0, 3.0), (14.0, 6.0)]);
            g.closed(&[(8.0, 17.0), (16.0, 17.0), (19.0, 21.0), (5.0, 21.0)]);
        }
        Icon::DownConductor => {
            // A vertical down conductor held by three evenly spaced clips.
            g.line(&[(12.0, 3.0), (12.0, 21.0)]);
            g.line(&[(8.0, 6.0), (16.0, 6.0)]);
            g.line(&[(8.0, 12.0), (16.0, 12.0)]);
            g.line(&[(8.0, 18.0), (16.0, 18.0)]);
        }
        Icon::Electrode => {
            // The standard earth symbol: lead plus three progressively shorter bars.
            g.line(&[(12.0, 3.0), (12.0, 11.0)]);
            g.line(&[(5.0, 11.0), (19.0, 11.0)]);
            g.line(&[(8.0, 15.0), (16.0, 15.0)]);
            g.line(&[(10.0, 19.0), (14.0, 19.0)]);
        }
        Icon::InspectionBox => {
            // A buried inspection chamber with rim and accessible inner cover.
            g.line(&[(5.0, 7.0), (5.0, 21.0), (19.0, 21.0), (19.0, 7.0)]);
            g.line(&[(3.0, 7.0), (21.0, 7.0)]);
            g.closed(&[(9.0, 11.0), (15.0, 11.0), (15.0, 17.0), (9.0, 17.0)]);
        }
        Icon::GroundConductor => {
            // An earthing grid of intersecting conductors.
            g.line(&[(3.0, 8.0), (21.0, 8.0)]);
            g.line(&[(3.0, 16.0), (21.0, 16.0)]);
            g.line(&[(8.0, 3.0), (8.0, 21.0)]);
            g.line(&[(16.0, 3.0), (16.0, 21.0)]);
        }
        Icon::DataOutlet => {
            // An RJ45 jack with its keyed opening; small contact marks disappear in compact mode.
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.closed(&[(7.0, 8.0), (17.0, 8.0), (17.0, 13.0), (15.0, 13.0), (15.0, 16.0), (9.0, 16.0), (9.0, 13.0), (7.0, 13.0)]);
            if !compact {
                g.line(&[(10.0, 8.0), (10.0, 11.0)]);
                g.line(&[(14.0, 8.0), (14.0, 11.0)]);
            }
        }
        Icon::VoiceOutlet => {
            // A telephone handset on an outlet plate; distinct from data ports.
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.line(&[(7.0, 12.0), (7.0, 10.0), (10.0, 8.0), (14.0, 8.0), (17.0, 10.0), (17.0, 12.0)]);
            g.line(&[(7.0, 12.0), (10.0, 12.0), (10.0, 10.0)]);
            g.line(&[(14.0, 10.0), (14.0, 12.0), (17.0, 12.0)]);
        }
        Icon::Rack => {
            // Three rack units with aligned handles; handles omitted at small sizes.
            g.closed(&[(5.0, 3.0), (19.0, 3.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(5.0, 9.0), (19.0, 9.0)]);
            g.line(&[(5.0, 15.0), (19.0, 15.0)]);
            if !compact {
                g.line(&[(8.0, 6.0), (10.0, 6.0)]);
                g.line(&[(8.0, 12.0), (10.0, 12.0)]);
                g.line(&[(8.0, 18.0), (10.0, 18.0)]);
            }
        }
        Icon::PatchPanel => {
            // A port strip with evenly spaced sockets; compact ports become clean marks.
            if compact {
                g.closed(&[(3.0, 6.0), (21.0, 6.0), (21.0, 18.0), (3.0, 18.0)]);
                g.line(&[(7.0, 10.0), (7.0, 14.0)]);
                g.line(&[(12.0, 10.0), (12.0, 14.0)]);
                g.line(&[(17.0, 10.0), (17.0, 14.0)]);
            } else {
                g.closed(&[(3.0, 6.0), (21.0, 6.0), (21.0, 18.0), (3.0, 18.0)]);
                g.closed(&[(6.0, 10.0), (9.0, 10.0), (9.0, 14.0), (6.0, 14.0)]);
                g.closed(&[(15.0, 10.0), (18.0, 10.0), (18.0, 14.0), (15.0, 14.0)]);
            }
            if !compact {
                g.line(&[(12.0, 10.0), (12.0, 14.0)]);
            }
        }
        Icon::Cabling => {
            // Three parallel cables, with identical bends and even spacing.
            g.line(&[(3.0, 3.0), (8.0, 3.0), (14.0, 9.0), (21.0, 9.0)]);
            g.line(&[(3.0, 9.0), (8.0, 9.0), (14.0, 15.0), (21.0, 15.0)]);
            g.line(&[(3.0, 15.0), (8.0, 15.0), (14.0, 21.0), (21.0, 21.0)]);
        }
        Icon::Stair => {
            g.closed(&[
                (4.0, 20.0),
                (4.0, 16.0),
                (8.0, 16.0),
                (8.0, 12.0),
                (12.0, 12.0),
                (12.0, 8.0),
                (16.0, 8.0),
                (16.0, 4.0),
                (20.0, 4.0),
                (20.0, 20.0),
            ]);
        }
        Icon::Roof => {
            g.closed(&[(3.0, 12.0), (12.0, 4.0), (21.0, 12.0), (19.0, 14.0), (12.0, 7.777778), (5.0, 14.0)]);
            g.line(&[(6.0, 13.111111), (6.0, 20.0), (18.0, 20.0), (18.0, 13.111111)]);
        }
        Icon::Theme => {
            // An outlined half-disc with an interior semicircle; no solid fill.
            g.line(&[(12.0, 3.0), (12.0, 21.0)]);
            g.circle((12.0, 12.0), 9.0);
            g.arc((12.0, 12.0), 5.0, -90.0, 90.0);
        }
        Icon::Text => {
            g.line(&[(4.0, 20.0), (12.0, 4.0), (20.0, 20.0)]);
            g.line(&[(7.0, 14.0), (17.0, 14.0)]);
        }
        Icon::Spot => {
            g.line(&[(3.0, 16.0), (21.0, 16.0)]);
            g.closed(&[(5.0, 8.0), (13.0, 8.0), (9.0, 16.0)]);
            g.line(&[(15.0, 7.0), (21.0, 7.0)]);
            g.line(&[(15.0, 11.0), (19.0, 11.0)]);
        }
        Icon::Angular => {
            // Two rays and their shared-center angular dimension arc.
            g.line(&[(4.0, 20.0), (21.0, 20.0)]);
            g.line(&[(4.0, 20.0), (14.0, 3.0)]);
            g.arc((4.0, 20.0), 10.0, -59.534454, 0.0);
        }
        Icon::Radial => {
            // A radius arrow from the center toward the circle; no filled center dot.
            let diagonal = 9.0 * std::f32::consts::FRAC_1_SQRT_2;
            let tip = (12.0 + diagonal, 12.0 - diagonal);
            g.line(&[(12.0, 12.0), tip]);
            g.line(&[(tip.0 - 4.0, tip.1), tip, (tip.0, tip.1 + 4.0)]);
            g.circle((12.0, 12.0), 9.0);
        }
        Icon::Crop => {
            g.line(&[(7.0, 3.0), (7.0, 17.0), (21.0, 17.0)]);
            g.line(&[(3.0, 7.0), (17.0, 7.0), (17.0, 21.0)]);
        }
        Icon::Units => {
            // A rectangular unit rule with alternating graduations.
            g.closed(&[(3.0, 6.0), (21.0, 6.0), (21.0, 18.0), (3.0, 18.0)]);
            g.line(&[(7.0, 6.0), (7.0, 12.0)]);
            g.line(&[(12.0, 6.0), (12.0, 10.0)]);
            g.line(&[(17.0, 6.0), (17.0, 12.0)]);
        }
        Icon::Line => {
            // A segment between outlined endpoint handles; handles do not overlap its body.
            g.line(&[(7.0, 17.0), (17.0, 7.0)]);
            g.closed(&[(3.0, 17.0), (7.0, 17.0), (7.0, 21.0), (3.0, 21.0)]);
            g.closed(&[(17.0, 3.0), (21.0, 3.0), (21.0, 7.0), (17.0, 7.0)]);
        }
        Icon::Arc => {
            // A smooth curve joining two endpoint handles, with the midpoint indication omitted when small.
            g.closed(&[(3.0, 15.0), (7.0, 15.0), (7.0, 19.0), (3.0, 19.0)]);
            g.closed(&[(17.0, 15.0), (21.0, 15.0), (21.0, 19.0), (17.0, 19.0)]);
            g.cubic([(5.0, 15.0), (5.0, 3.0), (19.0, 3.0), (19.0, 15.0)]);
            if !compact {
                g.line(&[(10.0, 6.0), (14.0, 6.0)]);
            }
        }
        Icon::Flip => {
            // Opposed arrows on either side of the wall centerline.
            g.closed(&[(3.0, 10.0), (21.0, 10.0), (21.0, 14.0), (3.0, 14.0)]);
            g.line(&[(12.0, 3.0), (12.0, 7.0)]);
            g.line(&[(9.0, 6.0), (12.0, 3.0), (15.0, 6.0)]);
            g.line(&[(12.0, 17.0), (12.0, 21.0)]);
            g.line(&[(9.0, 18.0), (12.0, 21.0), (15.0, 18.0)]);
        }
        Icon::Attach => {
            // A wall and an upward arrow toward its host roof; the wall outline leaves the arrow clear.
            g.line(&[(3.0, 8.0), (12.0, 3.0), (21.0, 8.0)]);
            g.line(&[(7.0, 13.0), (7.0, 21.0), (17.0, 21.0), (17.0, 13.0)]);
            g.line(&[(12.0, 14.0), (12.0, 7.0)]);
            g.line(&[(9.5, 9.5), (12.0, 7.0), (14.5, 9.5)]);
        }
        Icon::Detach => {
            g.line(&[(3.0, 8.0), (12.0, 3.0), (21.0, 8.0)]);
            g.line(&[(7.0, 15.0), (7.0, 21.0), (17.0, 21.0), (17.0, 15.0)]);
            g.line(&[(12.0, 8.0), (12.0, 15.0)]);
            g.line(&[(9.5, 12.5), (12.0, 15.0), (14.5, 12.5)]);
        }
        Icon::Floor => {
            g.closed(&[(3.0, 11.0), (13.0, 6.0), (21.0, 10.0), (11.0, 15.0)]);
            g.line(&[(3.0, 11.0), (3.0, 14.0), (11.0, 18.0), (21.0, 13.0), (21.0, 10.0)]);
            g.line(&[(11.0, 15.0), (11.0, 18.0)]);
        }
        Icon::Ceiling => {
            // A suspended tiled ceiling above a separate floor datum; not a floating pair of bars.
            if compact {
                g.closed(&[(3.0, 8.0), (21.0, 8.0), (21.0, 12.0), (3.0, 12.0)]);
                g.line(&[(7.0, 4.0), (7.0, 8.0)]);
                g.line(&[(17.0, 4.0), (17.0, 8.0)]);
                g.line(&[(12.0, 8.0), (12.0, 12.0)]);
                g.line(&[(3.0, 20.0), (21.0, 20.0)]);
            } else {
                g.closed(&[(3.0, 8.0), (21.0, 8.0), (21.0, 12.0), (3.0, 12.0)]);
                g.line(&[(7.0, 4.0), (7.0, 8.0)]);
                g.line(&[(17.0, 4.0), (17.0, 8.0)]);
                g.line(&[(9.0, 8.0), (9.0, 12.0)]);
                g.line(&[(15.0, 8.0), (15.0, 12.0)]);
                g.line(&[(3.0, 20.0), (21.0, 20.0)]);
            }
        }
        Icon::Shaft => {
            // A crossed opening in plan, centered on a square keyline.
            g.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            g.line(&[(4.0, 4.0), (20.0, 20.0)]);
            g.line(&[(20.0, 4.0), (4.0, 20.0)]);
        }
        Icon::Mass => {
            // A rectangular volume with parallel receding edges.
            g.closed(&[(4.0, 9.0), (15.0, 9.0), (15.0, 20.0), (4.0, 20.0)]);
            g.line(&[(4.0, 9.0), (9.0, 4.0), (20.0, 4.0), (20.0, 15.0), (15.0, 20.0)]);
            g.line(&[(15.0, 9.0), (20.0, 4.0)]);
        }
        Icon::SlopeArrow => {
            // A constant-depth inclined slab; the slope arrow is parallel to its face.
            g.closed(&[(3.0, 16.0), (21.0, 7.0), (21.0, 11.0), (3.0, 20.0)]);
            g.line(&[(5.0, 11.0), (15.0, 6.0)]);
            g.line(&[(11.5, 5.0), (15.0, 6.0), (13.7, 9.4)]);
        }
        Icon::Ramp => {
            g.closed(&[(3.0, 19.0), (21.0, 19.0), (21.0, 10.0)]);
            g.line(&[(6.0, 12.0), (16.0, 7.0)]);
            g.line(&[(12.5, 6.0), (16.0, 7.0), (14.7, 10.4)]);
        }
        Icon::Railing => {
            // Three equally spaced balusters between a handrail and base rail.
            g.line(&[(3.0, 5.0), (21.0, 5.0)]);
            g.line(&[(5.0, 5.0), (5.0, 19.0)]);
            g.line(&[(12.0, 5.0), (12.0, 19.0)]);
            g.line(&[(19.0, 5.0), (19.0, 19.0)]);
            g.line(&[(3.0, 19.0), (21.0, 19.0)]);
        }
        Icon::ExtrusionRoof => {
            // An open gable profile swept along a shared receding axis; all matching roof slopes are parallel.
            g.line(&[(3.0, 15.0), (9.0, 8.0), (15.0, 15.0)]);
            g.line(&[(9.0, 8.0), (15.0, 4.0), (21.0, 11.0), (15.0, 15.0)]);
            g.line(&[(3.0, 15.0), (9.0, 11.0)]);
        }
        Icon::Dormer => {
            // A glazed gabled projection. The front IS the window, with a roof
            // ridge and triangular cheek returning into the host roof plane.
            // The glazed front stays legible without a nested window rectangle.
            g.closed(&[(8.0, 3.0), (22.0, 3.0), (16.0, 21.0), (2.0, 21.0)]);
            g.line(&[(6.0, 11.0), (10.0, 7.0), (14.0, 11.0)]);
            g.closed(&[(6.0, 11.0), (14.0, 11.0), (14.0, 18.0), (6.0, 18.0)]);
            g.line(&[(10.0, 7.0), (14.0, 5.0), (18.0, 9.0), (14.0, 11.0)]);
            g.line(&[(18.0, 9.0), (14.0, 18.0)]);
            if !compact {
                g.line(&[(10.0, 11.0), (10.0, 18.0)]);
            }
        }
        Icon::GableEdge => {
            // Roof plan with a hipped end, ridge and a marked gable edge.
            g.closed(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)]);
            g.line(&[(3.0, 5.0), (10.0, 12.0), (3.0, 19.0)]);
            g.line(&[(10.0, 12.0), (18.0, 12.0)]);
            if !compact {
                g.line(&[(18.0, 8.0), (18.0, 16.0)]);
            }
        }
        Icon::Materials => {
            // An outlined shaded sphere: curvature, no color or filled half-disc.
            g.circle((12.0, 12.0), 9.0);
            g.arc((13.0, 13.0), 5.0, 190.0, 265.0);
            g.cubic([(3.0, 12.0), (8.0, 18.0), (16.0, 18.0), (21.0, 12.0)]);
        }
        Icon::Component => {
            // A generic component is a simple closed box in axonometric view.
            g.closed(&[(12.0, 3.0), (21.0, 8.0), (21.0, 16.0), (12.0, 21.0), (3.0, 16.0), (3.0, 8.0)]);
            g.line(&[(3.0, 8.0), (12.0, 13.0), (21.0, 8.0)]);
            g.line(&[(12.0, 13.0), (12.0, 21.0)]);
        }
        Icon::LoadFamily => {
            // Load into an open container: entry path and arrow stay legible.
            g.line(&[(4.0, 12.0), (8.0, 12.0)]);
            g.line(&[(16.0, 12.0), (20.0, 12.0), (20.0, 21.0), (4.0, 21.0), (4.0, 12.0)]);
            g.line(&[(12.0, 3.0), (12.0, 16.0)]);
            g.line(&[(8.0, 12.0), (12.0, 16.0), (16.0, 12.0)]);
        }
        Icon::Link => {
            // Two links of a chain, each through the other.
            g.closed(&[(5.0, 8.0), (13.0, 8.0), (15.0, 10.0), (15.0, 14.0), (13.0, 16.0), (5.0, 16.0), (3.0, 14.0), (3.0, 10.0)]);
            g.closed(&[(11.0, 8.0), (19.0, 8.0), (21.0, 10.0), (21.0, 14.0), (19.0, 16.0), (11.0, 16.0), (9.0, 14.0), (9.0, 10.0)]);
        }
        Icon::Parameters => {
            // Three independent parameter sliders with a consistent two-point knob radius.
            g.line(&[(3.0, 6.0), (9.0, 6.0)]);
            g.line(&[(13.0, 6.0), (21.0, 6.0)]);
            g.line(&[(3.0, 12.0), (14.0, 12.0)]);
            g.line(&[(18.0, 12.0), (21.0, 12.0)]);
            g.line(&[(3.0, 18.0), (6.0, 18.0)]);
            g.line(&[(10.0, 18.0), (21.0, 18.0)]);
            g.circle((11.0, 6.0), 2.0);
            g.circle((16.0, 12.0), 2.0);
            g.circle((8.0, 18.0), 2.0);
        }
        Icon::Measure => {
            // A diagonal ruler with ticks perpendicular to its top edge, distinct from Units.
            g.closed(&[(3.0, 8.0), (17.0, 3.0), (21.5, 15.6), (7.5, 20.6)]);
            g.line(&[(6.5, 6.75), (8.0, 10.95)]);
            g.line(&[(11.0, 5.142857), (13.0, 10.742857)]);
            g.line(&[(15.5, 3.535714), (17.0, 7.735714)]);
        }
        Icon::Snaps => {
            // A point handle and separated crosshairs; no dense four-way overlap.
            g.closed(&[(9.0, 9.0), (15.0, 9.0), (15.0, 15.0), (9.0, 15.0)]);
            g.line(&[(12.0, 3.0), (12.0, 7.0)]);
            g.line(&[(12.0, 17.0), (12.0, 21.0)]);
            g.line(&[(3.0, 12.0), (7.0, 12.0)]);
            g.line(&[(17.0, 12.0), (21.0, 12.0)]);
        }
        Icon::Visibility => {
            // A smooth eye contour with a centered unfilled iris.
            g.circle((12.0, 12.0), 3.0);
            g.cubic([(3.0, 12.0), (7.0, 3.0), (17.0, 3.0), (21.0, 12.0)]);
            g.cubic([(21.0, 12.0), (17.0, 21.0), (7.0, 21.0), (3.0, 12.0)]);
        }
        Icon::Template => {
            g.closed(&[(5.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            g.line(&[(8.0, 12.0), (16.0, 12.0)]);
            g.line(&[(8.0, 16.0), (14.0, 16.0)]);
        }
        Icon::Phasing => {
            // A stepped timeline with phase ticks, rather than an architectural stair silhouette.
            g.line(&[(3.0, 19.0), (8.0, 19.0), (8.0, 13.0), (14.0, 13.0), (14.0, 7.0), (21.0, 7.0)]);
            g.line(&[(3.0, 22.0), (21.0, 22.0)]);
            if !compact {
                g.line(&[(3.0, 16.0), (3.0, 19.0)]);
                g.line(&[(8.0, 10.0), (8.0, 13.0)]);
                g.line(&[(14.0, 4.0), (14.0, 7.0)]);
            }
        }
        Icon::Options => {
            // Two equal branches from one design into its alternative options.
            g.line(&[(3.0, 12.0), (8.0, 12.0), (15.0, 6.0), (17.0, 6.0)]);
            g.line(&[(8.0, 12.0), (15.0, 18.0), (17.0, 18.0)]);
            g.circle((19.0, 6.0), 2.0);
            g.circle((19.0, 18.0), 2.0);
        }
        Icon::SelectById => {
            // The enlarged lens leaves room for a complete number sign even at
            // toolbar size. Both crossbars are essential to its identity.
            g.line(&[(15.65, 15.65), (21.0, 21.0)]);
            g.line(&[(8.5, 6.5), (7.5, 13.5)]);
            g.line(&[(12.5, 6.5), (11.5, 13.5)]);
            g.line(&[(6.0, 8.5), (14.0, 8.5)]);
            g.line(&[(6.0, 11.5), (14.0, 11.5)]);
            g.circle((10.0, 10.0), 8.0);
        }
        Icon::IdsOfSelection => {
            // A selection label with a string hole and aligned identifier rows.
            g.closed(&[(10.0, 3.0), (21.0, 3.0), (21.0, 21.0), (10.0, 21.0), (3.0, 12.0)]);
            g.line(&[(13.0, 8.0), (18.0, 8.0)]);
            g.line(&[(13.0, 12.0), (18.0, 12.0)]);
            g.line(&[(13.0, 16.0), (16.0, 16.0)]);
            g.circle((8.0, 12.0), 1.5);
        }
        Icon::Demolish => {
            // A hammer with a handle perpendicular to its head and separate rubble marks.
            g.closed(&[(7.0, 7.0), (11.0, 3.0), (21.0, 13.0), (17.0, 17.0)]);
            g.line(&[(12.0, 12.0), (4.0, 20.0)]);
            g.line(&[(15.0, 21.0), (21.0, 21.0)]);
            if !compact {
                g.line(&[(19.0, 18.0), (22.0, 18.0)]);
            }
        }
        Icon::Paint | Icon::Unpaint => {
            // An upright metal paint can with an elliptical rim and bottom,
            // vertical sides and a semicircular wire handle attached to the rim.
            // A separate drop or minus mark carries the operation at the right.
            g.line(&[(3.0, 9.0), (3.0, 18.0)]);
            g.line(&[(15.0, 9.0), (15.0, 18.0)]);
            g.cubic([(3.0, 18.0), (3.0, 22.0), (15.0, 22.0), (15.0, 18.0)]);
            g.arc((9.0, 9.0), 6.0, -180.0, 0.0);
            if compact {
                // Keep the front lip; omit the close parallel back edge.
                g.cubic([(3.0, 9.0), (3.0, 11.666667), (15.0, 11.666667), (15.0, 9.0)]);
            } else {
                g.ellipse((9.0, 9.0), (6.0, 0.0), (0.0, 2.0), true);
            }
            if matches!(icon, Icon::Unpaint) {
                g.line(&[(18.0, 16.0), (22.0, 16.0)]);
            } else {
                // A true pointed drop, with mirrored sides and a rounded base.
                g.cubic([(20.0, 12.0), (19.5, 13.5), (18.0, 15.0), (18.0, 16.5)]);
                g.cubic([(18.0, 16.5), (18.0, 19.166667), (22.0, 19.166667), (22.0, 16.5)]);
                g.cubic([(22.0, 16.5), (22.0, 15.0), (20.5, 13.5), (20.0, 12.0)]);
            }
        }
        Icon::Favorite => {
            // A symmetric five-point star on the same optical keyline.
            g.closed(&[
                (12.0, 3.0),
                (14.8, 8.7),
                (21.0, 9.6),
                (16.5, 14.0),
                (17.6, 20.2),
                (12.0, 17.3),
                (6.4, 20.2),
                (7.5, 14.0),
                (3.0, 9.6),
                (9.2, 8.7),
            ]);
        }
        Icon::FamilyEditor => {
            // An editable component: a small volume plus a separate pencil, distinct from View3D.
            g.closed(&[(4.0, 8.0), (11.0, 4.0), (18.0, 8.0), (11.0, 12.0)]);
            g.line(&[(4.0, 8.0), (4.0, 16.0), (11.0, 20.0)]);
            g.line(&[(11.0, 12.0), (11.0, 20.0)]);
            g.line(&[(18.0, 8.0), (18.0, 11.0)]);
            g.closed(&[(15.0, 20.0), (20.0, 15.0), (22.0, 17.0), (17.0, 22.0), (15.0, 22.0)]);
        }
        Icon::TempHide => {
            // A consistent pair of temporary-visibility glasses; the bridge is clear.
            g.closed(&[(3.0, 11.0), (10.0, 11.0), (10.0, 16.0), (3.0, 16.0)]);
            g.closed(&[(14.0, 11.0), (21.0, 11.0), (21.0, 16.0), (14.0, 16.0)]);
            g.line(&[(10.0, 12.0), (14.0, 12.0)]);
            g.line(&[(3.0, 11.0), (5.0, 5.0)]);
            g.line(&[(21.0, 11.0), (19.0, 5.0)]);
        }
        Icon::Reveal => {
            // An outlined reveal bulb and screw base.
            g.line(&[(8.5, 15.5), (9.0, 17.0), (9.0, 20.0), (15.0, 20.0), (15.0, 17.0), (15.5, 15.5)]);
            g.line(&[(10.0, 22.0), (14.0, 22.0)]);
            g.cubic([(8.5, 15.5), (0.0, 9.0), (7.0, 3.0), (12.0, 3.0)]);
            g.cubic([(12.0, 3.0), (17.0, 3.0), (24.0, 9.0), (15.5, 15.5)]);
        }
        Icon::Settings => {
            // A single outlined gear with equal tooth rhythm and one unfilled hub.
            g.closed(&[
                (18.728832, 10.070539),
                (20.834645, 10.282719),
                (20.834645, 13.717281),
                (18.728832, 13.929461),
                (18.122338, 15.393667),
                (19.461338, 17.032736),
                (17.032736, 19.461338),
                (15.393667, 18.122338),
                (13.929461, 18.728832),
                (13.717281, 20.834645),
                (10.282719, 20.834645),
                (10.070539, 18.728832),
                (8.606333, 18.122338),
                (6.967264, 19.461338),
                (4.538662, 17.032736),
                (5.877662, 15.393667),
                (5.271168, 13.929461),
                (3.165355, 13.717281),
                (3.165355, 10.282719),
                (5.271168, 10.070539),
                (5.877662, 8.606333),
                (4.538662, 6.967264),
                (6.967264, 4.538662),
                (8.606333, 5.877662),
                (10.070539, 5.271168),
                (10.282719, 3.165355),
                (13.717281, 3.165355),
                (13.929461, 5.271168),
                (15.393667, 5.877662),
                (17.032736, 4.538662),
                (19.461338, 6.967264),
                (18.122338, 8.606333),
            ]);
            g.circle((12.0, 12.0), 3.5);
        }
        Icon::Dock => {
            // A docked sidebar in outline, without the previous filled panel.
            g.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
            g.line(&[(9.0, 4.0), (9.0, 20.0)]);
        }
        Icon::Minimize => {
            g.line(&[(4.0, 15.0), (12.0, 7.0), (20.0, 15.0)]);
        }
        Icon::Expand => {
            g.line(&[(4.0, 9.0), (12.0, 17.0), (20.0, 9.0)]);
        }
        Icon::Search => {
            // A magnifier with a tangent handle and no internal marks.
            g.line(&[(15.0, 15.0), (21.0, 21.0)]);
            g.circle((10.0, 10.0), 7.0);
        }
        Icon::Legend => {
            // Two swatches and their aligned descriptions.
            g.closed(&[(3.0, 4.0), (8.0, 4.0), (8.0, 9.0), (3.0, 9.0)]);
            g.line(&[(12.0, 6.5), (21.0, 6.5)]);
            g.closed(&[(3.0, 15.0), (8.0, 15.0), (8.0, 20.0), (3.0, 20.0)]);
            g.line(&[(12.0, 17.5), (21.0, 17.5)]);
        }
        Icon::Home => {
            // Home retains its door, keeping it distinct from the modeling Roof icon.
            g.line(&[(3.0, 11.0), (12.0, 3.0), (21.0, 11.0)]);
            g.line(&[(5.0, 9.222222), (5.0, 21.0), (19.0, 21.0), (19.0, 9.222222)]);
            g.line(&[(10.0, 21.0), (10.0, 15.0), (14.0, 15.0), (14.0, 21.0)]);
        }
        Icon::NewFile => {
            g.closed(&[(5.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (5.0, 21.0)]);
            g.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            g.line(&[(9.0, 15.0), (15.0, 15.0)]);
            g.line(&[(12.0, 12.0), (12.0, 18.0)]);
        }
        Icon::Code => {
            g.line(&[(8.0, 6.0), (3.0, 12.0), (8.0, 18.0)]);
            g.line(&[(16.0, 6.0), (21.0, 12.0), (16.0, 18.0)]);
            g.line(&[(14.0, 4.0), (10.0, 20.0)]);
        }
        Icon::Check => {
            g.line(&[(7.0, 12.0), (10.5, 15.5), (17.0, 9.0)]);
            g.circle((12.0, 12.0), 9.0);
        }
        Icon::Back => {
            // A plain arrow pointing left, its head the same angle as Undo's.
            g.line(&[(20.0, 12.0), (4.0, 12.0)]);
            g.line(&[(10.0, 6.0), (4.0, 12.0), (10.0, 18.0)]);
        }
        Icon::LineWeights => {
            // Three pens, each wider: a stroke, then bars as wide as a heavier pen draws.
            g.line(&[(3.0, 5.0), (21.0, 5.0)]);
            g.closed(&[(3.0, 10.0), (21.0, 10.0), (21.0, 12.5), (3.0, 12.5)]);
            g.closed(&[(3.0, 16.0), (21.0, 16.0), (21.0, 20.5), (3.0, 20.5)]);
        }
        Icon::ObjectStyles => {
            // A wall cut, hatched as a cut is, before a plain one seen beyond.
            g.closed(&[(3.0, 4.0), (10.0, 4.0), (10.0, 20.0), (3.0, 20.0)]);
            g.line(&[(3.0, 11.0), (10.0, 4.0)]);
            g.line(&[(3.0, 18.0), (10.0, 11.0)]);
            if !compact {
                g.line(&[(5.0, 20.0), (10.0, 15.0)]);
            }
            g.closed(&[(14.0, 8.0), (21.0, 8.0), (21.0, 20.0), (14.0, 20.0)]);
        }
        Icon::FilledRegion => {
            // An irregular sketched region, hatched inside its outline.
            g.closed(&[(3.0, 6.0), (15.0, 3.0), (21.0, 10.0), (17.0, 21.0), (5.0, 18.0)]);
            g.line(&[(4.2, 12.0), (10.5, 4.6)]);
            g.line(&[(4.8, 17.0), (16.2, 4.3)]);
            if !compact {
                g.line(&[(9.0, 19.0), (19.4, 7.9)]);
            }
            g.line(&[(14.0, 20.3), (20.0, 13.5)]);
        }
        Icon::MaskingRegion => {
            // A plain region over a hatch: what's under it hidden where it lies.
            g.line(&[(3.0, 9.0), (9.0, 3.0)]);
            g.line(&[(3.0, 15.0), (8.0, 10.0)]);
            g.line(&[(3.0, 21.0), (8.0, 16.0)]);
            g.line(&[(15.0, 3.0), (21.0, 3.0)]);
            g.closed(&[(8.0, 8.0), (21.0, 8.0), (21.0, 21.0), (8.0, 21.0)]);
        }
        Icon::FillPatterns => {
            // Three swatches: hatched, crosshatched, solid's outline.
            g.closed(&[(3.0, 3.0), (11.0, 3.0), (11.0, 11.0), (3.0, 11.0)]);
            g.line(&[(3.0, 7.0), (7.0, 3.0)]);
            g.line(&[(7.0, 11.0), (11.0, 7.0)]);
            g.closed(&[(13.0, 3.0), (21.0, 3.0), (21.0, 11.0), (13.0, 11.0)]);
            g.line(&[(17.0, 3.0), (17.0, 11.0)]);
            g.line(&[(13.0, 7.0), (21.0, 7.0)]);
            g.closed(&[(3.0, 13.0), (21.0, 13.0), (21.0, 21.0), (3.0, 21.0)]);
            if !compact {
                g.line(&[(3.0, 17.0), (21.0, 17.0)]);
            }
        }
        Icon::Circle => {
            // A circle by its centre, a grip there, and a point on it.
            g.circle((12.0, 12.0), 8.5);
            if !compact {
                g.closed(&[(10.5, 10.5), (13.5, 10.5), (13.5, 13.5), (10.5, 13.5)]);
            }
        }
        Icon::Inscribed => {
            // A hexagon about a grip at its centre, the radius out to a corner.
            g.closed(&hexagon(9.0));
            g.closed(&[(10.5, 10.5), (13.5, 10.5), (13.5, 13.5), (10.5, 13.5)]);
            g.line(&[(13.5, 12.0), (21.0, 12.0)]);
        }
        Icon::Circumscribed => {
            // A hexagon about a grip at its centre, the radius square onto the middle of a side.
            g.closed(&hexagon(8.0 / (TAU / 12.0).cos()));
            g.closed(&[(10.5, 10.5), (13.5, 10.5), (13.5, 13.5), (10.5, 13.5)]);
            g.line(&[(12.0, 13.5), (12.0, 20.0)]);
            if !compact {
                g.line(&[(12.0, 17.5), (14.5, 17.5), (14.5, 20.0)]);
            }
        }
        Icon::CentreArc => {
            // A quarter arc round a grip at its centre, with the radius to its middle.
            g.closed(&[(4.0, 16.0), (8.0, 16.0), (8.0, 20.0), (4.0, 20.0)]);
            g.arc((6.0, 18.0), 14.0, -90.0, 0.0);
            if !compact {
                g.line(&[(8.6, 15.4), (14.5, 9.5)]);
            }
        }
        Icon::TangentArc => {
            // A line, then an arc going on from its end the way it ran, to a grip.
            g.line(&[(3.0, 19.5), (10.0, 19.5)]);
            g.arc((10.0, 9.5), 10.0, 90.0, 0.0);
            g.closed(&[(18.0, 5.5), (22.0, 5.5), (22.0, 9.5), (18.0, 9.5)]);
            if !compact {
                g.line(&[(10.0, 17.0), (10.0, 22.0)]);
            }
        }
        Icon::Fillet => {
            // Two lines rounded where they'd meet.
            g.line(&[(4.5, 21.0), (4.5, 12.5)]);
            g.arc((12.5, 12.5), 8.0, 180.0, 270.0);
            g.line(&[(12.5, 4.5), (21.0, 4.5)]);
            if !compact {
                g.line(&[(12.5, 12.5), (8.0, 8.0)]);
            }
        }
        Icon::Spline => {
            // A curve led by its control points: grips off it, where it bends toward them.
            g.cubic([(4.0, 18.0), (7.0, 3.0), (17.0, 21.0), (20.0, 6.0)]);
            g.closed(&[(2.0, 18.0), (6.0, 18.0), (6.0, 22.0), (2.0, 22.0)]);
            g.closed(&[(18.0, 2.0), (22.0, 2.0), (22.0, 6.0), (18.0, 6.0)]);
            if !compact {
                g.closed(&[(8.0, 2.0), (11.0, 2.0), (11.0, 5.0), (8.0, 5.0)]);
                g.closed(&[(13.0, 19.0), (16.0, 19.0), (16.0, 22.0), (13.0, 22.0)]);
            }
        }
        Icon::FitSpline => {
            // A curve passing through its points, each a ring on it.
            g.cubic([(4.6, 15.6), (6.0, 7.0), (9.5, 7.5), (12.0, 12.0)]);
            g.cubic([(12.0, 12.0), (14.5, 16.5), (18.0, 17.0), (19.4, 8.4)]);
            g.circle((3.5, 18.0), 2.0);
            g.circle((20.5, 6.0), 2.0);
            if !compact {
                g.circle((12.0, 12.0), 2.0);
            }
        }
        Icon::Ellipse => {
            // An ellipse about its centre.
            g.ellipse((12.0, 12.0), (9.5, 0.0), (0.0, 6.0), true);
            if !compact {
                g.line(&[(12.0, 10.0), (12.0, 14.0)]);
                g.line(&[(10.0, 12.0), (14.0, 12.0)]);
            }
        }
        Icon::PartialEllipse => {
            // Half an ellipse across its axis, between grips at its ends.
            g.ellipse((12.0, 16.0), (8.5, 0.0), (0.0, 8.5 * 0.8), false);
            g.closed(&[(1.5, 16.0), (5.5, 16.0), (5.5, 20.0), (1.5, 20.0)]);
            g.closed(&[(18.5, 16.0), (22.5, 16.0), (22.5, 20.0), (18.5, 20.0)]);
            // Its axis, between the grips: dashed, or one line where dashes would blur.
            if compact {
                g.line(&[(5.5, 18.0), (18.5, 18.0)]);
            } else {
                g.line(&[(8.0, 18.0), (10.5, 18.0)]);
                g.line(&[(13.5, 18.0), (16.0, 18.0)]);
            }
        }
        Icon::Polyline => {
            // One line through several points, a grip at either end.
            g.line(&[(4.0, 18.0), (9.0, 8.0), (15.0, 15.0), (20.0, 6.0)]);
            g.closed(&[(2.0, 18.0), (6.0, 18.0), (6.0, 22.0), (2.0, 22.0)]);
            g.closed(&[(18.0, 2.0), (22.0, 2.0), (22.0, 6.0), (18.0, 6.0)]);
        }
        Icon::PickLines => {
            // A line, and the pointer that picks it.
            g.line(&[(3.0, 7.0), (21.0, 7.0)]);
            g.closed(&[(9.0, 10.0), (9.0, 20.5), (11.8, 17.7), (13.6, 21.0), (15.4, 20.1), (13.7, 16.8), (17.5, 16.8)]);
        }
        Icon::JoinPolyline => {
            // Two lines meeting at one grip: one line now.
            g.line(&[(3.0, 20.0), (10.0, 11.0)]);
            g.line(&[(14.0, 11.0), (21.0, 20.0)]);
            g.closed(&[(10.0, 7.0), (14.0, 7.0), (14.0, 11.0), (10.0, 11.0)]);
            if !compact {
                g.line(&[(5.0, 4.0), (8.0, 6.5)]);
                g.line(&[(19.0, 4.0), (16.0, 6.5)]);
            }
        }
        Icon::ExplodePolyline => {
            // A polyline come apart into its pieces.
            g.line(&[(3.0, 18.0), (7.5, 10.0)]);
            g.line(&[(10.0, 9.0), (14.0, 15.0)]);
            g.line(&[(16.5, 14.0), (21.0, 6.0)]);
            if !compact {
                g.line(&[(8.5, 13.5), (8.5, 16.0)]);
                g.line(&[(15.5, 9.5), (15.5, 7.0)]);
            }
        }
        Icon::Rectangle => {
            // A sketched rectangle by its two opposite corners, each a grip; its sides stop at the grips.
            g.closed(&[(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)]);
            g.closed(&[(17.0, 17.0), (21.0, 17.0), (21.0, 21.0), (17.0, 21.0)]);
            g.line(&[(7.0, 5.0), (19.0, 5.0), (19.0, 17.0)]);
            g.line(&[(17.0, 19.0), (5.0, 19.0), (5.0, 7.0)]);
        }
    }
    g.0
}

/// A hexagon about the middle, its corners `r` from it, two of them level with it.
fn hexagon(r: f32) -> [(f32, f32); 6] {
    std::array::from_fn(|k| {
        let a = TAU * k as f32 / 6.0;
        (12.0 + r * a.cos(), 12.0 + r * a.sin())
    })
}

#[derive(Default)]
struct Glyph(Vec<Outline>);

impl Glyph {
    fn line(&mut self, points: &[(f32, f32)]) {
        self.0.push(Outline { points: points.iter().map(|&(x, y)| pos2(x, y)).collect(), closed: false });
    }

    fn closed(&mut self, points: &[(f32, f32)]) {
        self.0.push(Outline { points: points.iter().map(|&(x, y)| pos2(x, y)).collect(), closed: true });
    }

    // Curves share a 2.8 degree maximum angular step, so small arcs do not turn
    // into visible polygons when inspected enlarged or on a high-DPI display.
    fn arc(&mut self, center: (f32, f32), radius: f32, from: f32, to: f32) {
        let start = from.to_radians();
        let sweep = (to - from).to_radians();
        let steps = (sweep.abs() / TAU * 128.0).ceil().max(2.0) as usize;
        let points = (0..=steps)
            .map(|i| {
                let angle = start + sweep * i as f32 / steps as f32;
                pos2(center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
            })
            .collect();
        self.0.push(Outline { points, closed: false });
    }

    fn circle(&mut self, center: (f32, f32), radius: f32) {
        let points = (0..128)
            .map(|i| {
                let angle = TAU * i as f32 / 128.0;
                pos2(center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
            })
            .collect();
        self.0.push(Outline { points, closed: true });
    }

    fn ellipse(&mut self, center: (f32, f32), a: (f32, f32), b: (f32, f32), closed: bool) {
        let (start, sweep, count) = if closed { (0.0, TAU, 128) } else { (TAU * 0.5, TAU * 0.5, 65) };
        let steps = if closed { count } else { count - 1 };
        let points = (0..count)
            .map(|i| {
                let angle = start + sweep * i as f32 / steps as f32;
                pos2(center.0 + a.0 * angle.cos() + b.0 * angle.sin(), center.1 + a.1 * angle.cos() + b.1 * angle.sin())
            })
            .collect();
        self.0.push(Outline { points, closed });
    }

    fn cubic(&mut self, control: [(f32, f32); 4]) {
        let [a, b, c, d] = control.map(|(x, y)| pos2(x, y).to_vec2());
        let points = (0..=48)
            .map(|i| {
                let t = i as f32 / 48.0;
                let u = 1.0 - t;
                (a * u.powi(3) + b * 3.0 * u.powi(2) * t + c * 3.0 * u * t.powi(2) + d * t.powi(3)).to_pos2()
            })
            .collect();
        self.0.push(Outline { points, closed: false });
    }
}

/// Fit an icon without distortion; keep one pen weight across all strokes.
pub fn shapes(icon: Icon, rect: Rect, ink: Color32) -> Vec<Shape> {
    let side = rect.width().min(rect.height());
    if !side.is_finite() || side <= 0.0 || !rect.min.is_finite() || !rect.max.is_finite() {
        return Vec::new();
    }
    let scale = side / 24.0;
    let origin = rect.center() - vec2(side, side) * 0.5;
    let stroke = Stroke::new(STROKE_WIDTH, ink);
    outlines(icon, side <= 19.0)
        .into_iter()
        .map(|outline| {
            let points = outline.points.into_iter().map(|p| origin + p.to_vec2() * scale).collect();
            if outline.closed { Shape::closed_line(points, stroke) } else { Shape::line(points, stroke) }
        })
        .collect()
}

pub fn paint(painter: &Painter, icon: Icon, rect: Rect, ink: Color32) {
    painter.extend(shapes(icon, rect, ink));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_size_has_finite_geometry_and_room_for_its_stroke() {
        for &icon in Icon::ALL {
            for compact in [false, true] {
                let paths = outlines(icon, compact);
                assert!(!paths.is_empty(), "{icon:?}, compact={compact}");
                for path in paths {
                    assert!(path.points.len() >= if path.closed { 3 } else { 2 }, "{icon:?}");
                    for p in path.points {
                        assert!(p.is_finite(), "{icon:?}");
                        assert!((1.5..=22.5).contains(&p.x) && (1.5..=22.5).contains(&p.y), "{icon:?}: {p:?}");
                    }
                }
            }
            for size in [15.0, 18.0, 19.0, 22.0, 24.0, 48.0] {
                for allocation in [vec2(size, size), vec2(size * 2.0, size), vec2(size, size * 2.0)] {
                    let rect = Rect::from_min_size(pos2(37.0, 53.0), allocation);
                    let paths = shapes(icon, rect, Color32::WHITE);
                    assert!(!paths.is_empty());
                    for shape in paths {
                        let Shape::Path(path) = shape else {
                            panic!("Only unfilled paths are allowed");
                        };
                        assert_eq!(path.stroke.width, STROKE_WIDTH, "{icon:?}");
                        assert_eq!(path.fill, Color32::TRANSPARENT, "{icon:?}");
                        let bounds = Rect::from_points(&path.points).expand(STROKE_WIDTH * 0.5);
                        assert!(rect.contains_rect(bounds), "{icon:?} at {size}: {bounds:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn small_sizes_reduce_detail_without_changing_the_symbols() {
        for icon in [Icon::Dormer, Icon::DataOutlet, Icon::Equipment, Icon::Rack, Icon::TagAll] {
            assert!(outlines(icon, true).len() < outlines(icon, false).len(), "{icon:?}");
        }
    }

    #[test]
    fn invalid_allocations_are_ignored() {
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(shapes(Icon::Wall, Rect::from_min_size(Pos2::ZERO, vec2(size, size)), Color32::WHITE).is_empty());
        }
    }
}
