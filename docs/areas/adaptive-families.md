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

**What to do next** is under Status and next steps, below; each step is a tool of Revit's, with a core test for its edit or
command and a desktop test that drives it as a user would (`tests/adaptive_modelling.rs` shows how: `click_model` at a model
point, the view's buttons by label).

## Try it

1. Manage > New Family, any category; on the **Adaptive** tab, **Make this family adaptive** (or **Create four-point adaptive
   panel** for a ready-made one).
2. **Point**: click four points on the work plane. **Line**: click them in turn and the first again, closing a profile.
3. **Create Solid Form** with the profile selected. **Modify**: select the points (Ctrl adds) in the order they should be
   numbered and press **Make Adaptive**. Drag a gizmo arrow to see the form follow.
4. **OK**, then in a plan **Component** (CM) with the family: click its adaptive points in order; select the instance and drag
   its numbered grips.

`samples/adaptive-panel.json` (Family Editor > Load Family…) is a ready panel with four independently oriented points, a warped
extrusion, a six-DOF child point, a reference line and a point hosted on it; `bimcraft-cli run --script
samples/adaptive-panel.txt --save panel.bimcraft` places two instances of it, the second flexing one frame and its Thickness.

## Modelling as in Revit

The Adaptive tab of an adaptive family is a modelling view (`adaptive_view.rs`), as Revit's family editor for a Generic Model
Adaptive: a 3D view over a work plane at height 0 (a grid a metre apart), orbited by a drag on empty space, panned with the middle
button, zoomed with the wheel; the dialog widens to the screen for it. Its tools:

- **Point**: a click on the work plane places a reference point (a point on the world, `Definition::add_point`); a click on a line
  places a point hosted on it, at the parameter clicked (`add_hosted_point`).
- **Line**: clicks on points (or on the work plane, which places one) draw a chain through them; a click on the first point again
  closes it into a profile, Enter, Escape or a right click finish it open (`add_chain`).
- **Make Adaptive** numbers the points selected in the order they are made adaptive, as Revit does; **Make Reference Point** takes
  it back, and the higher numbers close up (`make_adaptive`, `make_reference`).
- **Create Solid Form** and **Create Void Form**: one closed profile selected is extruded `Depth` along its normal, two or more
  profiles are lofted in the order selected (`create_form`).
- **Modify**: a click selects (Ctrl adds), a drag of a point moves it over its height, and a point selected alone shows a gizmo:
  a drag of its red, green or blue arrow moves it along x, y or z (`move_point`; a point on a line slides along it, one hosted on
  a point changes its offset). **Delete** removes what is selected and what depends on it, as Revit does (`remove`).
- **Undo** steps back through the view's edits; Cancel forgets them all, Apply or OK makes the family the project's.

Beside the view, Properties shows what is selected: its relationship (host), offsets and rotations, a form's depth, void,
material and visibility, each a number or a formula of the family's parameters. Below it, **All elements and the test placement**
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
Create Form, the gizmo, Delete and Undo, Properties, and placing and reshaping instances in a plan. Each is covered by tests in
`adaptive/tests.rs`, `adaptive/edit_tests.rs` and the desktop's `tests/adaptive_modelling.rs`.

Next, in order: placing and dragging adaptive points in the project's 3D view (on levels and on other instances); reference planes
other than the work plane, and points placed on them; a point hosted on a form's face or edge by clicking it; parameters that drive
a form's depth or a point's offset from the view; dimensions in the family; divided surfaces with repeated panels.

## Frame convention

A Frame stores origin and unit right-handed X/Y/Z axes. Coordinates are millimetres, angles degrees, column-vector convention. OffsetX/Y/Z translate along the unrotated host axes. Rotation then occurs about the translated origin, with intrinsic local X, then moved local Y, then moved local Z: `Fchild = Fhost * T(offset) * Rx * Ry * Rz`. The origin is the pivot; rotation does not rotate the offset around the host origin. World, complete-frame, XY/XZ/YZ-plane, adaptive-point, curve and native mesh-subelement hosts are explicit alternatives. XZ uses (X,Z,-Y); YZ uses (Y,Z,X).

A definition supplies default frames for numbered adaptive points and a separate editable preview placement. A Component can override any numbered frame in its own coordinates; missing overrides use definition defaults. The component's normal level/offset/angle/mirror transform is applied once after evaluation (`Model::component_axes`; `Frame::from_local` and `Frame::into_local` go between the two). Context snapshots are in world coordinates: `adaptive_place` converts them into each instance's own axes, so the instance builds on the captured points whatever its `at`, angle, offset or level; `component` and `modify` take frames already in the instance's axes.

## Reference curves and orientation

Line, ordered open/closed Polyline and cubic Bezier refer to actual point IDs, including references across branches. Parameter measures the native curve domain (uniform by segment for polylines), NormalizedLength measures arc length fraction, Distance measures mm. From-end reverses measurement without reversing the curve tangent. Polyline arc lengths are exact. Bezier arc length uses a deterministic 256-interval chord table; it is an approximation, not a promised absolute tolerance. Bezier preview uses 64 intervals.

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

## Diagnostics, limits and persistence

`adaptive_evaluate {id}` returns the containing family, definition, exact placement, raw instance parameter overrides, selected type and evaluation with fully resolved parameters. `Evaluation::replay` uses the saved resolved parameters and frames for standalone reproduction. The ordered results and diagnostics identify node, error code, dependency path and cause; valid independent and upstream results survive missing dependencies, cycles and geometric failures. Structural invalidity is previewable while editing, but a committed definition must have unique IDs/numbers, existing dependencies and no cycles. Failed geometric instance placements remain saveable and reopenable.

Limits: 1024 nodes, dependency depth 128, polyline up to 256 points, 64 loft sections, 256 correspondence samples, per-mesh 4096 vertices/20000 triangles and per-evaluation 400000 triangles. Frames must be finite, orthonormal and within the coordinate budget. New projects use file format 12 so older builds cannot silently discard adaptive definitions; existing projects migrate with empty adaptive fields.

The initial scope deliberately has no graph editor, nested adaptive families, live cross-instance constraints or analytic host surfaces. Future editing must continue using this canonical definition rather than adding a disconnected geometry recipe.
