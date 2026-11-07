# Adaptive families

An adaptive family is modelled and placed as in Revit's Generic Model Adaptive: reference points on a work plane, some made
adaptive and numbered, lines through them and forms made from the lines; an instance is placed by clicking its adaptive points
in order, and dragging one reshapes it. `Family.adaptive` (`adaptive::Definition`) is the one definition: every tool of the
modelling view, every edit in Properties and every command changes it, and the existing `family`, `component`, `modify`,
save/open and Undo paths carry it. There is no graph canvas. Its elements (nodes) have persistent family-local string IDs;
renaming one changes only its display name. A dependency never implies that one element owns another.

## For an agent picking this up

**The goal** is Revit's behaviour for adaptive components, in BIMCraft's own clean-room code: model a Generic Model Adaptive family
in a 3D view (points, adaptive numbering, lines, forms, gizmo), place instances by clicking their points, reshape them by their
grips. When in doubt about how a tool should behave, it should behave as Revit's does; the user tests it against Revit. Nothing
here is copied from Autodesk (the root rules).

**Read, in order:** this page; `crates/core/src/adaptive/mod.rs` (the definition, `Host`, `Operation`, `Placement`, the
evaluator), `adaptive/edit.rs` (every modelling edit and `Evaluation::pick`), `adaptive/frame.rs` (frames and their
local/project conversions), `adaptive/geometry.rs` (meshes); the commands `adaptive_capture`, `adaptive_place`, `adaptive_move`,
`adaptive_evaluate` in `commands.rs`; `Model::component_axes`, `adaptive_placement`, `adaptive_points` in `model.rs`. In the
desktop: `adaptive_view.rs` (the modelling view), `adaptive_editor.rs` (Properties and the element list), `adaptive_place.rs`
(placing and grips in a plan), and in `editor.rs` the wide Adaptive layout (`adaptive_side`).

**How the pieces talk.** The Family Editor edits a copy of the family, as all its tabs do, and makes it the project's with the
`family` command on Apply or OK; the modelling view's tools are pure edits of that copy's `Definition` (in the core, tested
there), so the window stays thin. In the project, every change is a command (`adaptive_place`, `adaptive_move`, `modify`), so
Undo, scripts and MCP get it too. An instance holds its frames in its own axes (`Component::adaptive`); captured and clicked
frames are the project's and `adaptive_place` converts them.

**Testing while working:** `cargo test -p bimcraft-core --lib adaptive` (the core's adaptive tests, about a second once built)
and `cargo test -p bimcraft-ui-egui adaptive` (the window's). The core is optimised even in a dev build, which makes each build
take minutes; `cargo --config 'profile.dev.package.bimcraft-core.opt-level=0' test …` builds it faster. The whole workspace's tests
take about 25 minutes. To look at the Adaptive tab in the three looks, set `BIMCRAFT_ADAPTIVE_UI_DUMP` to a folder (under
`target/`) and run `cargo test -p bimcraft-ui-egui adaptive_editor_visual_preview_in_three_looks`; to try it by hand,
`cargo run -p bimcraft`.

**What Revit does**, tool by tool and with its defaults, and where we differ, is in
[adaptive-revit-parity.md](../adaptive-revit-parity.md): read it before changing a tool. **What to do next** is under Status and
next steps, below; each step is a tool of Revit's, with a core test for its edit or
command and a desktop test that drives it as a user would (`tests/adaptive_modelling.rs` shows how: `click_model` at a model
point, the view's buttons by label).

## Try it

1. Manage > New Family, any category; on the **Adaptive** tab, **Make this family adaptive** (or **Create four-point adaptive
   panel** for a ready-made one). The family opens as a document of its own, taking the window.
2. Create > **Point Element**: click four points on the work plane. **Line**: click them in turn and the first again, closing
   a profile.
3. **Create Form** > **Solid Form** with the profile selected. **Modify**: select the points (Ctrl adds) in the order they should
   be numbered; on **Modify | Reference Points**, **Make Adaptive**. Drag a gizmo arrow to see the form follow.
4. **Load into Project and Close**, then in a plan **Component** (CM) with the family: click its adaptive points in order;
   select the instance and drag its numbered grips.

`samples/adaptive-panel.json` (Family Editor > Load Family…) is a ready panel with four adaptive points on the family's axes (two raised), a warped
extrusion, a reference line and a point hosted on it; `bimcraft-cli run --script
samples/adaptive-panel.txt --save panel.bimcraft` places two instances of it, the second flexing one frame and its Thickness.

## Modelling as in Revit

An adaptive family is edited as Revit edits a family: a **family document** that takes the whole window
(`editor/document.rs`, `BimApp::family_doc`), in place of the Family Editor dialog the other families keep. The window's title
and title bar name the family and its view. Its own ribbon has Revit's tabs, Create, Insert, View, Manage and Modify, and a
contextual tab for the tool in hand or what is selected, as Revit names them ("Modify | Place Reference Point", "Modify | Place
Lines", "Modify | Reference Points", "Modify | Lines", "Modify | Form", "Modify | Multi-Select"): Modify's panels with the
selection's own; it comes up when it appears and goes back to the tab it came from. Its panels lay out as Revit's, and a tool
BIMCraft does not have yet shows greyed where Revit has it ("Not in BIMCraft yet"):

- **Select** (Create, Modify, contextual): Modify.
- **Properties** (the same): the palette, **Family Category and Parameters** and **Family Types** (dialogs, as Revit's: the
  name and category; the parameters and the types).
- **Draw** (Create, and the contextual tab while drawing): Model, Reference, Plane, then the gallery: Line and Point Element
  work; Rectangle, Inscribed Polygon, Circle, Center-ends Arc, Spline, Ellipse, Spline Through Points, Pick Lines, Pick Faces
  are greyed, as are Reference and Plane.
- **Work Plane** (the same): **Set** picks the work plane in the view (as Placement Plane's Pick), **Show** draws it (tinted,
  with a grid); Viewer is greyed.
- **Model**, **Datum** (Create): greyed (Component, Model Text, Level, Reference Plane).
- **Form** (Create, and the contextual tab with lines selected): Create Form > Solid Form or Void Form.
- **Test Placement** (Create, BIMCraft's own): Replay Instance, Capture Placement, Apply to Instance, Place Instance.
- **Family** (Insert): Load Family, Save Family, Add to Favorites; **Import** greyed.
- **Graphics**, **Windows** (View) and **Settings** (Manage): greyed.
- **Clipboard**, **Modify**, **Measure** (Modify, contextual): Delete; **Move**, **Copy** and **Rotate**, as Revit's: two clicks in
  the view, on points exactly or on the work plane (Move and Copy from the first to the second; Rotate about the selection's
  centre, square to the work plane, from where the angle starts to where it ends, to the whole degree), then back to Modify,
  the copies selected (`Definition::move_elements`, `copy_elements`, `rotate_elements`: what is selected with what it is built
  on, a line's points, never a point's host; a point hosted on another moves within it, a copied placement point is numbered
  next). Align, Offset, the Mirrors, Trim/Extend, Split, Paste and Measure are greyed.
- **Adaptive Component** (contextual, with points selected): Make Adaptive.
- **Family Editor** (every tab): Load into Project, Load into Project and Close.

What they ask for goes through the code the dialog's buttons use (`FamilyEditor::ask`, `family_asked`). The docked Properties
palette shows what is selected with Revit's groups (Adaptive Component, Graphics, Identity Data; a type-selector box naming it),
or the family itself; the Project Browser lists the family's views as the template has them (Ref. Level's plans, `{3D}`, the four
elevations: only `{3D}` is drawn yet). The view fills the pane under its tab (whose × is **Close Family**: what was not loaded is
forgotten), with a view control bar (Zoom to Fit), the Options Bar over it (the contextual tab's name; with lines selected, the
depth Create Form extrudes), Undo in the title bar stepping through the family's edits, and the status bar saying what the tool
asks for.

The modelling view (`adaptive_view.rs`) is a 3D view drawn as Revit's of a family: a grey ground lighter towards the bottom (on
the drawing's own ground: lighter in a dark interface when the canvas is kept light), Ref. Level a dash-dot outline, the
reference planes dashed green, forms light grey with their edges where their faces fold (`Mesh::folds`), points as black dots,
placement numbers in blue; and Revit's **ViewCube** at the top right (a face clicked looks from that side, `cube_faces`; the house
over it is the default view; a drag on it orbits). It opens framing the family and its datums, and is orbited by Shift and the middle button (as Revit's), panned with the middle button, zoomed with the wheel; a drag on
empty space draws a selection box, as Revit's: left to right it takes what lies wholly inside it (a solid outline), right to
left what it touches (dashed), Ctrl adding to the selection and Shift taking away (`Evaluation::in_box`). It draws the family's
datums: Ref. Level and the reference planes Center (Front/Back) and Center (Left/Right), each the outline of a rectangle
round what the family takes up, named, and the family's own named reference planes the same way, dashed green; and, with Work
Plane > Show, a grid a metre apart on the work plane Point Element and Line draw on. That is Revit's
**Placement Plane**, in the Options Bar while drawing (`WorkPlane`, `Modeller::work_plane`): one of the family's planes (Ref.
Level to begin with) or one of a point's three planes, or **Pick…**, first in the list as in Revit: the next click picks the plane under
the pointer, lit as the pointer passes over it (`Evaluation::pick_plane`: a point's plane under the pointer first, the nearest the
eye, and only where there is none one of the family's, since those fill the room and one in front of a point would hide its planes;
Escape gives up). The family's planes reach 300 mm beyond what the family takes up, and at least 1.5 m from its origin. A point
drawn on one has its axes and keeps it as its Work Plane (`Host::World`'s `plane`); one drawn on a point's plane is hosted on it,
at an offset in it, and follows that point (`Definition::add_point_on`). Properties shows it under Constraints as **Work Plane**.

**Work planes are the family's planes.** An adaptive family has no datums of its own: its work planes are the family's
reference planes (`Family::planes`, [families](families.md#reference-planes)) and its reference lines' four faces, named by a
`PlaneRef` as a form's `host` is (`WorkPlane::Plane`; `WorkPlane::Point` is still a point's own three planes). Placement Plane
and Pick list them as `Planes::listed` does: the datums first, named as Revit names them (`Level : Ref. Level`, `Reference
Plane : Center (Front/Back)`, `Reference Plane : Center (Left/Right)`), then each named plane of the family's own (`Reference
Plane : name`; an unnamed one is not listed, as in Revit), then each reference line's faces (`Reference Line 1 : Horizontal`,
Vertical, Start, End). Working out where one is takes the family's planes and lines (`Planes`, `Family::plane_set`; the
Family Editor keeps them beside the definition it edits); `Planes::default()` knows the three datums. A world point's `plane`
is `{"plane": guid}` or `{"line": guid, "face": …}`; it keeps its own `frame`, so the plane only says where it was drawn. A
file from before (FORMAT 18 and 19) held `"datum": "level" | "front_back" | "left_right"`, which reads as Ref. Level, Center
(Front/Back) and Center (Left/Right) (`plane_or_datum`), whose frames are the ones those datums had, so nothing moves. A plane
a point is drawn on is one of the family's (`Family::check_planes` says which point names one it hasn't), can't be deleted
while the point is on it (`Family::hosted_on`), and takes the point along when the sync gives it a new GUID (`Family::rehost`).
Renaming a family parameter renames it in every formula of the definition, which is where "=" ties a value to one, and in the
forms made of a material parameter (`Definition::rename_parameter`, called by `Family::rename_parameter`).

Its tools:

- **Point Element**: a click on the work plane places a reference point on it (`Definition::add_point_on`); a click on a line
  places a point hosted on it, at the parameter clicked (`add_hosted_point`); a click within 8 px of a point snaps onto it,
  exactly where it falls on the work plane (on that point's own plane, on it at no offset), a square round it showing the snap
  while the pointer is there.
- **Line**: clicks on points (or on the work plane, which places one) draw a chain through them; a click on the first point again
  closes it into a profile, Enter, Escape or a right click finish it open (`add_chain`).
- **Make Adaptive** is a toggle, as Revit's: the points selected are made placement points, numbered next in the order they are
  made adaptive and oriented to the instance (Revit's default); if all of them are adaptive already, they go back to reference
  points and the higher numbers close up (`make_adaptive`, `make_reference`).
- **Create Solid Form** and **Create Void Form**: one closed profile selected is extruded `Depth` along its normal, two or more
  profiles are lofted in the order selected (`create_form`).
- **Modify**: a click selects (Ctrl adds), a drag of a point moves it over its height, and a point selected alone shows a gizmo:
  a drag of its red, green or blue arrow moves it along x, y or z (`move_point`; a point on a line slides along it, one hosted on
  a point changes its offset). **Delete** removes what is selected and what depends on it, as Revit does (`remove`).
- **Undo** steps back through the view's edits; Cancel forgets them all, Apply or OK makes the family the project's.

Beside the view, Properties shows what is selected. A point first has Revit's fields (`point_properties`): under Constraints its
**Work Plane**, where it is along its line or edge (named after its measurement, as Revit names it) and how it is turned now
(Rotation X, Y, Z); then **Point** (Reference
Point, Placement Point (Adaptive) or Shape Handle Point (Adaptive): `set_point_type`), **Number** (typing one another point holds
swaps the two: `set_number`), **Show Placement Number**, **Orients to**, **Constrained** on a shape handle, then **Show
Reference Planes** and **Visible**, with Revit's defaults (`Node::look`). The view draws a placement point filled and numbered, a
shape handle a filled square, a reference point a ring, and each point's three planes (light blue outlines of squares centred on it) and three axes (short light blue arrows, the gizmo's long ones when it is selected) when Show Reference Planes
says so. A form has **Material** (Materials and Finishes), **Visible** and Solid/Void. Each number, Yes/No and material ends in
Revit's **=**, and so does each of a point's offsets and rotations in its relationship (OffsetX, OffsetY, OffsetZ, RotX, RotY,
RotZ: Revit's single Offset and Rotation Angle are OffsetZ and RotZ, so they are not shown twice): **Associate Family
Parameter** lists the family's parameters holding the same (a length for an offset, an angle for a rotation) and <None>; OK ties the row to the one chosen (its expression becomes the parameter's name, and the row
shows its value, greyed), <None> unties it keeping the value it had. **New Parameter** there asks Revit's Parameter
Properties (a name, Type or Instance; the kind is the row's) and adds it to the family at once, with the row's value
(`adaptive_editor::associate`, held open in `FamilyEditor::associating`). Then come its relationship (host) and a form's depth and void. Below it, **All elements and the test placement**
lists every element with the same fields, the test placement's frames, and what the evaluation says; that is where a relationship
the tools don't make yet (a point on a form's face by index, an intersection, a Bezier) is set. What is under the pointer is the
core's to say (`Evaluation::pick`: a point first, then a line, then the form nearest the camera).

Above the view, with adaptive instances selected in the project, **Capture selected placement frames in project coordinates**
takes their points as the test placement (see Context capture). Selecting an adaptive instance and choosing Edit Family reopens it
on that instance's own placement, type and instance parameters: **Apply test frames to this instance** gives it the frames edited
(`modify`, in its own axes), **Place test frames as a new instance** places another on them (`adaptive_place`, the frames taken as
the project's, each adaptive point without a test frame of its own getting the definition's: `Definition::completed`). The test
placement and each instance's placement are independent snapshots; saving a family keeps its test placement and never changes an
instance's.

## Placing and reshaping instances

In a plan, Component with an adaptive family places it as Revit does (`adaptive_place.rs`): each click is the next adaptive point,
on the level or, within 10 px, exactly on another adaptive instance's point; the last click places the instance
(`adaptive_place`), Escape forgets the points clicked and keeps the tool. An adaptive instance selected alone shows a grip on each
adaptive point, numbered; a drag moves that point over the plan at its height (`adaptive_move`, one undo step), and the form
follows.

## Status and next steps

Done: the definition and its evaluator (frames, hosts, curves, forms, diagnostics), persistence (file format 12), the commands
(`adaptive_capture`, `adaptive_place`, `adaptive_move`, `adaptive_evaluate`), the modelling view with Point, Line, Make Adaptive,
Create Form, the gizmo, Delete and Undo, Properties, and placing and reshaping instances in a plan. Of the parity study's step 1:
point types with shape handles, Revit's numbering, Orients to as measured in Revit, the six measurement types, and the show
options. Each is covered by tests in `adaptive/tests.rs`, `adaptive/edit_tests.rs`, `adaptive/revit_tests.rs` (Revit's rules)
and the desktop's `tests/adaptive_modelling.rs`.

Next: the order of work in [adaptive-revit-parity.md](../adaptive-revit-parity.md), the study of how Revit does it (with its
defaults, observed in Revit 2025) and where each tool here differs. What is left of step 1: Mirrored and Flipped, and dragging shape handles in a project;
then the family editor's space, Create Form, the 3D control, placing
in the project, and divided surfaces.

## Frame convention

A Frame stores origin and unit right-handed X/Y/Z axes. Coordinates are millimetres, angles degrees, column-vector convention. OffsetX/Y/Z translate along the unrotated host axes. Rotation then occurs about the translated origin, with intrinsic local X, then moved local Y, then moved local Z: `Fchild = Fhost * T(offset) * Rx * Ry * Rz`. The origin is the pivot; rotation does not rotate the offset around the host origin. World, complete-frame, XY/XZ/YZ-plane, adaptive-point, curve and native mesh-subelement hosts are explicit alternatives. XZ uses (X,Z,-Y); YZ uses (Y,Z,X).

**Orients to** (`OrientsTo`, `oriented`) sets an adaptive point's axes once placed, as measured in Revit 2025
([the study](../adaptive-revit-parity.md) §1.4). A placement frame is where the point was placed and its host's axes there (a click
on the level gives the project's). The instance's own axes (`Evaluation::instance`) are the rigid motion that best takes the
family's placement points onto the placed ones (`frame::best_fit`, Horn's quaternion method; one point only moves, points in a line
turn the shortest way), so they flip when the points are placed in the other order. Instance (xyz), Revit's default for a new
point, takes the point's family axes through the instance's; Instance (z) then Host (xy) takes Z so and X from the host; Host
(xyz) takes the host's; Host and Loop System (xyz) the host's, flipped (Z reversed, X and Y swapped) when the placed points wind
against the family's (`frame::newell`); Global (xyz) the project's; Global (z) then Host (xy) Z up and X from the host. A file
from before Orients to reads its points as Host (xyz), which is what they did. A **shape handle** (`Host::Handle`) has no number
and is not clicked: it is where the family has it carried by the instance's axes, or where the instance's `Placement::handles`
puts it (by the point's id), and its `Constrained` is kept for dragging it.

A definition supplies default frames for numbered adaptive points and a separate editable preview placement. A Component can override any numbered frame in its own coordinates; missing overrides use definition defaults. The component's normal level/offset/angle/mirror transform is applied once after evaluation (`Model::component_axes`; `Frame::from_local` and `Frame::into_local` go between the two). Context snapshots are in world coordinates: `adaptive_place` converts them into each instance's own axes, so the instance builds on the captured points whatever its `at`, angle, offset or level; `component` and `modify` take frames already in the instance's axes.

## Reference curves and orientation

Line, ordered open/closed Polyline and cubic Bezier refer to actual point IDs, including references across branches. The measurement types are Revit's: NonNormalizedCurveParameter (mm along a straight line, 0 to the number of segments along a polyline, 0..1 on a Bezier), NormalizedCurveParameter (that as a fraction, uniform by segment for polylines), SegmentLength (mm along the line), NormalizedSegmentLength (the arc length fraction), ChordLength (mm in a straight line from the end measured from: the first place that far), and Angle, refused since no line here is an arc; files read the old names `parameter`, `normalized_length` and `distance` as these. From-end reverses measurement without reversing the curve tangent. Polyline arc lengths are exact. Bezier arc length uses a deterministic 256-interval chord table; it is an approximation, not a promised absolute tolerance. Bezier preview uses 64 intervals.

Curve-hosted points choose their orientation explicitly:

- InterpolateNormals: spherical interpolation of endpoint Z normals; X is the interpolated endpoint X hint projected perpendicular to Z, Y = Z cross X. This interpolates normals, not complete frames.
- ProjectInterpolatedNormals (default): X = forward unit curve tangent, Z = interpolated endpoint normal projected perpendicular to X and normalized, Y = Z cross X.
- WorldVertical: Z = world Z; X = tangent projected into the horizontal plane, Y = Z cross X.
- ProjectWorldZ: X = forward tangent; Z = world Z projected perpendicular to X, Y = Z cross X.

Antipodal normals follow a deterministic great circle. A degenerate projected direction uses the world axis least parallel to the surviving axis (ties X then Y then Z). Additional RotX/Y/Z apply after the base orientation. Polyline joints use the following segment's tangent (the final endpoint uses the last segment); this is a deliberate corner discontinuity, not smooth interpolation.

## Geometry and topology

Patch triangulates a closed simple chain. Extrude translates the original 3D boundary and its cap triangulation: it does **not** flatten a warped boundary. Default extrusion direction is the smallest-eigenvalue covariance normal of the boundary, with deterministic positive Z/Y/X sign; callers can supply an explicit direction. Clockwise and counterclockwise rings produce consistent directed edges. Loft connects an ordered series of open or closed profiles; all must agree on closure. The union of profile corner parameters preserves unequal profile corners; correspondence is parameter-based, not geometric matching. Closed lofts are capped.

The actual kernel is BIMCraft's polygonal triangulation and BSP subtraction, not analytic B-rep. A closed profile must be simple and have nonzero area in its best-fit projection; folds or crossings in that projection are rejected with a diagnostic. Warped boundaries passing that check are supported. Closed meshes validate nonzero volume and two opposite directed uses per edge. Arbitrary global 3D self-intersection of a loft or swept form is not certified; split complicated profiles into simpler explicit forms and inspect them. There are no analytic NURBS faces here.

A host reference is owner node ID + mesh connectivity token + native triangle/edge/vertex index. The token survives coordinate flex and rejects connectivity changes. Face coordinates are barycentric on a triangle; edges include triangulation diagonals, in sorted vertex-pair order. Vertex and edge normals average incident triangle area normals. Their auxiliary triangles never become independent model elements. Topology tokens and counts are shown under Evaluation and host references; `adaptive_evaluate` returns full indexed meshes. Hidden forms retain geometry for hosts. Appearance affects rendering; voids require closed solids and use the native CSG kernel. A failed cut is diagnosed and preceding geometry is retained atomically. As in traditional families, plan outlines retain their uncut footprint after CSG.

Direction Intersection takes two explicit frame axes, tests a unique intersection and optionally forward rays, and reports parallel or skew directions. It never infers relationships from proximity.

## Context capture and propagation

`adaptive_capture {sources:[{id,host:{type:"frame",point:"p1"}}, ...]}` captures ordered full project frames from existing adaptive components. IDs or GUIDs are accepted. Face/edge/vertex host structures can be supplied instead, with owner form and current topology token. Output is a Placement with world frames, context text, and source component GUID + exact host per placement number. These source references are provenance of a snapshot, not live constraints: deleting or moving a source does not silently change an existing instance. Capture again to refresh intentionally. Capture supports adaptive component geometry; picking arbitrary wall/roof surfaces directly in the viewport is not yet exposed.

Use captured output as `family {name,adaptive:{nodes,preview:placement}}` for authoring in that context, and `adaptive_place {family,type?,placements:[{adaptive:placement,at?,level?,angle?,offset?,params?}, ...]}` to place on the same hosts: up to 1000 panels in one atomic Undo step, each held in its own axes (`at` defaults to the first frame's point), providing a route for a caller to generate a grid of host snapshots. Face and edge hosts read their barycentric coordinates and parameter with the source instance's resolved parameters. Automatic grid discovery and live project-host constraints are not implemented.

An adaptive component (frames of its own, or its family's test placement) is placed by its points and has no work plane but its level: `Model::check` refuses a `host` on one (a project's reference plane, [families](families.md)), so `component` and `edit_work_plane` can't put one on a plane, and `Model::component_placement` and `settle_hosts` leave it upright where its points put it (`adaptive/host_tests.rs`).

## Diagnostics, limits and persistence

`adaptive_evaluate {id}` returns the containing family, definition, exact placement, raw instance parameter overrides, selected type and evaluation with fully resolved parameters. `Evaluation::replay` uses the saved resolved parameters and frames for standalone reproduction. The ordered results and diagnostics identify node, error code, dependency path and cause; valid independent and upstream results survive missing dependencies, cycles and geometric failures. Structural invalidity is previewable while editing, but a committed definition must have unique IDs/numbers, existing dependencies and no cycles. Failed geometric instance placements remain saveable and reopenable.

Limits: 1024 nodes, dependency depth 128, polyline up to 256 points, 64 loft sections, 256 correspondence samples, per-mesh 4096 vertices/20000 triangles and per-evaluation 400000 triangles. Frames must be finite, orthonormal and within the coordinate budget. New projects use file format 12 so older builds cannot silently discard adaptive definitions; existing projects migrate with empty adaptive fields.

The initial scope deliberately has no graph editor, nested adaptive families, live cross-instance constraints or analytic host surfaces. Future editing must continue using this canonical definition rather than adding a disconnected geometry recipe.
