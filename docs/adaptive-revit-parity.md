# Adaptive families: how Revit does it, and where BIMCraft stands

A study made on 2026-11-02 before going further with adaptive families, so that what we build follows Revit's behaviour and
interface as closely as a clean-room tool can. The area's write-up is [adaptive-families.md](areas/adaptive-families.md); this
page is the reference it is measured against, and the plan of work that follows from it.

**Clean-room.** Everything here is behaviour, described in our own words: what a tool does, what a property means, which
options it offers and which one is the default. No help text, icons, templates or families were copied. Tool and property
names are kept where they are what a Revit user looks for, as BIMCraft already does (Make Adaptive, Create Form).

**Sources, and how much to trust each.** Every rule below is marked:

- **[observed]**: seen in Revit 2025 on 2026-11-02, through its API, in a Generic Model Adaptive family. The calls only read,
  or ran in a transaction that was rolled back.
- **[documented]**: Autodesk's help (the 2014 to 2025 pages) or the Revit API reference states it. The API reference was
  read through The Building Coder (Jeremy Tammik), revitapidocs.com and rvtdocs.com.
- **[practitioner]**: blogs, forums, Autodesk University handouts and tutorials, with no official page behind them.
- **[guess]**: inferred, and still to be checked in Revit.

The research notes with every URL are kept outside the repository; the main pages are listed at the end.

## 1. Revit's model

### 1.1 The family editor

- An adaptive family starts from the **Generic Model Adaptive** template. Make Adaptive exists only there and in the two
  pattern-based templates. The plain mass template does not have it [documented].
- The template, opened empty from Revit 2025's English templates, holds [observed]:
  - the level **Ref. Level** at 0;
  - two reference planes, **Center (Front/Back)** (normal Y) and **Center (Left/Right)** (normal X);
  - the views Floor Plan and Ceiling Plan "Ref. Level", the elevations Front, Back, Left and Right, and a 3D view "{3D}".
    The 3D view is where adaptive families are modelled.
  - one hidden reference point at the origin, and no adaptive points.
- It is the conceptual design environment [documented]:
  - levels and reference planes show as planes in 3D;
  - the work plane under the cursor highlights, and a click on it makes it the active one;
  - **Create > Work Plane** has **Set** and **Show**;
  - only named reference planes appear in the Options Bar's **Placement Plane** list.

### 1.2 Points

There are three kinds of reference point [documented]:

- **Free**: placed on a work plane, and it depends on nothing else. The 3D control moves it anywhere.
- **Hosted**: placed on a line, an edge, a face or another point's plane. It follows its host and slides only along it.
- **Driving**: a point that a line runs through. Moving it reshapes the line.

Each point carries **three planes of its own** (its XY, XZ and YZ). They move with the point, any of them can be picked as a work
plane, and points, lines and forms can be hosted on them. That is how a family is built: point on point, plane on plane
[documented].

**Point** (Properties > Adaptive Component) is the point's type [observed]:

- **Reference Point**;
- **Placement Point (Adaptive)**: clicked in order when an instance is placed;
- **Shape Handle Point (Adaptive)**: not clicked when placing; it is dragged afterwards to reshape the instance. It has no number.

**Make Adaptive** (Modify | Reference Points > Adaptive Component) turns the selected points into placement points. On a point
that is adaptive already it turns it back into a reference point: the same button is a toggle [documented].

**A point's Properties**, with Revit's defaults [observed]. Which rows appear depends on the point's kind:

| Property | Options | Default |
|---|---|---|
| Point | Reference Point / Placement Point (Adaptive) / Shape Handle Point (Adaptive) | Reference Point; Placement Point after Make Adaptive |
| Number | 1..N, placement points only | the next number |
| Show Placement Number | Never / When Selected / Always | Always |
| Orients to | six options, §1.4 | Instance (xyz) |
| Constrained | shape handles only: None, a plane or an axis of the point (§1.4) | None |
| Show Reference Planes | Never / When Selected / Always | Never on a reference point, Always on an adaptive one |
| Show Normal Reference Plane Only | hosted points: only the plane square to the host | Yes |
| Visible | whether the point shows when the family is in a project | No |
| Name | free text, shown as a tooltip | empty |
| Rotation Angle | turns the point's planes about its normal | 0° (read-only on a free adaptive point) |
| Mirrored, Flipped | mirror the point's planes, or flip its Z | No |
| Offset | points on a plane: distance along the plane's normal | 0 |
| Measurement Type, its value, Measure From | points on a line or edge (§1.5) | |
| Hosted U / V Parameter | points on a face | |
| Driving Curve(s), Driven by Host | whether a line runs through it, whether it is hosted | |
| Work Plane | the plane it is on, read-only | |

Every number in the table (Offset, the measurement's value, Rotation Angle) can be tied to a family parameter, with the small
button at the end of its row [documented].

### 1.3 Numbering

All of these were observed in Revit:

- A point made adaptive takes the **next number**: with 4 placement points, the next one is 5.
- A point turned back into a reference point **closes the gap**: the numbers above it move down by one.
- Typing a number another point holds **swaps** the two. The number can be typed in Properties or on its label in the view
  [documented].
- Numbers always run 1..N without gaps. A shape handle has none.

### 1.4 Orients to

**Orients to** sets how an adaptive point's axes are taken once the family is placed. Three terms come first [documented]:

- **instance**: the instance's own axes;
- **host**: the element the point was clicked on (if it was clicked on nothing, the host is global);
- **global**: the project, or the family the instance is placed in.

| Option | Z from | X and Y from | Old name |
|---|---|---|---|
| **Instance (xyz)** (the default) | instance | instance | Orthogonal in Family |
| Instance (z) then Host (xy) | instance | host, its tangents projected | Vertical in Family |
| Host (xyz) | host | host | By Host Reference |
| Host and Loop System (xyz) | host | host, with the loop the points make. Needs 3 points or more; clicking them the other way round flips Z | Auto-Calculate |
| Global (xyz) | global | global | Orthogonal on Placement |
| Global (z) then Host (xy) | global | host, its tangents projected | Vertical on Placement |

**What each option gives, measured** [observed, 2026-11-02]. A 4-point panel of the studio's test family was placed:

- free, on a plane tilted 30°;
- hosted on a wall's face;
- hosted on the curved face of a half-cylinder in a mass;

each with its points in order and in reverse order, for every option. A point's Z was read from the family's points hosted at an
offset on each adaptive point's plane.

- **The instance's own axes** are the rotation that best takes the family's placement points onto the placed ones. They are the
  same whatever Orients to says. A panel placed on its family's own points with point 3 raised 260 mm leans its Z to (-0.0173,
  -0.0169, 0.9997) in Revit, (-0.0173, -0.0168, 0.9997) by a least-squares fit; raised 1000 mm, (-0.0668, -0.0651, 0.9956) in
  Revit, (-0.0663, -0.0645, 0.9957) by the fit [observed, 2026-11-02].
  - Its Z points to the side from which the placed points turn the same way as they do in the family. In the test family they
    turn clockwise seen from its Z, so the instance's Z is the side from which the placed loop looks clockwise.
  - Placing the points in reverse order flips it.
  - The host plays no part: on a wall placed in reverse order, Z points into the wall.
- **Instance (xyz)** and **Instance (z) then Host (xy)**: every point's Z is the instance's Z, one direction for the whole panel,
  even on the curved face.
- **Host (xyz)**: each point's Z is its host's normal at that point. On the curved face each point leans with the surface, and
  the order the points are clicked in makes no difference. A point on no host takes global Z.
- **Host and Loop System (xyz)**: the host's normal at each point, flipped for all of them when the points are placed in the
  order that winds against the family's.
- **Global (xyz)** and **Global (z) then Host (xy)**: global Z for every point, whatever the host.

X and Y were not measured (only Z shows in this family). The help says they come from the host's tangents in the "then Host
(xy)" options.

When a form could not be built, Revit resolved the error by deleting the instance, with a warning and nothing to undo it
[observed].

**Constrained** on a shape handle keeps it on one of the point's planes (YZ, ZX, XY) or axes (X, Y, Z). The interface offers
them as Center (Left/Right), Center (Front/Back) and Ref. Level [documented; the mapping to the API's seven values is a guess].

### 1.5 Hosting and measuring

A point is hosted in one of six ways [documented]:

- on an edge or line, by a measurement;
- on a face, by U and V;
- on a plane, by a position, an X direction and an Offset;
- at the crossing of two edges;
- where an edge meets a face;
- on another point.

On a line, X follows the tangent and the point may turn about it. On a face or plane, Z is the normal.

On a line the place is given by **Measurement Type** [observed]:

- Non-Normalized Curve Parameter;
- Normalized Curve Parameter;
- Segment Length (along the curve);
- Normalized Segment Length;
- Chord Length (in a straight line);
- Angle (arcs and circles only).

**Measure From** (Beginning or End) says which end the measurement starts from. Arrows by the point flip it [documented].

In the studio's test family, points sit on the planes of the adaptive points at an Offset of 400 mm, on reference lines at a
Segment Length of 400 mm, and at the crossings of lines. That is the everyday way a family is built [observed].

### 1.6 Lines

- The Draw panel has a **Model / Reference** switch [documented]:
  - a model line is taken into the form it makes;
  - a reference line stays, and the form follows it. Its profile is locked to it.
  - **Is Reference Line** switches a line between the two.
- **Spline Through Points**: two points selected give a line, three or more a spline through them. The points become driving
  points [documented].
- The Options Bar while drawing [documented]:
  - **Placement Plane**;
  - **Make surface from closed loops**;
  - **3D Snapping**, which snaps to points and vertices in 3D, so a line drawn point to point is tied to them;
  - **Chain**, **Offset** and **Radius**.
- **Draw on Work Plane** makes free points; **Draw on Face** hosts them on what is clicked [documented].

### 1.7 Create Form

There is one button (Create Form, with Solid Form and Void Form below it), and Revit works out the form from what is selected
[documented, with the practitioner rows marked]:

| Selected | Result |
|---|---|
| one open line or edge | a surface, the line extruded |
| one closed loop | a solid extrusion, its height then dragged with the 3D control |
| several separate closed loops | an extrusion each |
| two or more profiles on different planes | a loft |
| a profile and a line square to it | a sweep |
| two or more profiles and one path | a swept blend |
| a profile and a straight line on its plane | a revolve |
| a closed loop of reference lines | a choice between surface and solid, picked in the view [practitioner] |

A form selected shows **Modify | Form** [documented]:

- **X-Ray**, **Add Edge**, **Add Profile**, **Dissolve**, **Lock Profiles**, **Pick New Host**;
- **Edit Profile**;
- **Divide Surface**.

A void cuts the solids it meets.

### 1.8 The 3D control

A selected point, edge, face or form shows the 3D control [documented]:

- arrows move along an axis, and angle-shaped handles move in a plane;
- blue is global Z, red and green are the global horizontals, and orange is any local axis;
- **Spacebar** switches between global and the selection's own axes;
- a hosted point slides only along its host.

**Tab** cycles between overlapping candidates and **Ctrl** adds to the selection [practitioner].

### 1.9 In a project

**Placing** [documented]:

- Component, or a drag from the Project Browser, then a click per placement point in number order;
- a preview of the shape follows the cursor;
- the order of the clicks matters: an extrusion clicked counterclockwise points the other way;
- **Esc after k clicks places the instance with those k points**;
- shape handles are not clicked.

**Where a click lands:** on the work plane, or on what it snaps to: a vertex, an edge, a face, a point, a node of a divided
surface. The point is then hosted on it. Adaptive points do not use the usual smart snapping [practitioner].

**Editing a placed instance:**

- its points can be selected (Tab) and dragged; a hosted one slides on its host [documented];
- **Pick New Host** rehosts a point [documented];
- the instance has **Flipped** [documented];
- Ctrl-drag copies the instance [documented];
- a tag follows the points [practitioner];
- deleting a host deletes the instance without warning, by one 2014 report [practitioner].

**Repeat:** an instance whose points sit on nodes of a divided path or surface is repeated over all of them as one element
[documented].

**API shape**, for agents and import [documented]:

- AdaptiveComponentFamilyUtils and AdaptiveComponentInstanceUtils;
- placement points come back in number order;
- batch placement takes one list of points per instance;
- moving many points is fast when each point's coordinate system is set, and slow with one move per element.

### 1.10 What practitioners struggle with

[practitioner, ranked by how often it comes up]

1. **Orientation.** Panels flip or turn inside out. The six renamed options are confusing, and the click order decides "up".
2. **Point order.** It is invisible while placing; a wrong click means starting again; scripts slip on numbering.
3. **Strict hosting and nesting.** Categories are limited, and deleting a host deletes the instance.
4. **Weak drawings and schedules.** Tags jump, and there is no area, no angle and no point coordinates without Dynamo.
5. **Fragile families.** Forms fail to build, and instances delete themselves when flexed.
6. **Slow at scale.** Thousands of panels are slow.
7. **No numeric editing.** A point's X, Y, Z can't be typed.
8. **Poor IFC.** Instances come out as proxies.

## 2. Where BIMCraft stands

Status: **same** as Revit; **differs** (we have it, but not as Revit has it); **missing**.

### 2.1 The family editor

| Revit | BIMCraft today | Status |
|---|---|---|
| Ref. Level and the two Center reference planes | the same, drawn in the 3D view | same |
| Plan, elevation and 3D views of the family | one 3D view | missing |
| Work Plane Set / Show; the plane under the cursor | Placement Plane in the Options Bar: Pick… (the plane under the pointer), the datums, or a point's planes | differs: no Set / Show buttons |
| Each point's three planes, pickable as work planes | drawn, picked with Pick… or chosen from the list; points drawn on them are hosted | same |
| Draw on Work Plane / Draw on Face | Point on the plane, or on a line | differs |
| 3D Snapping, Chain | Line clicks points | differs |
| Model / Reference lines, Is Reference Line | one kind of line | missing |
| Spline Through Points | Line, Polyline, Bezier (Bezier is ours) | differs |

### 2.2 Points

| Revit | BIMCraft today | Status |
|---|---|---|
| Point type: Reference / Placement / Shape Handle | the same, in Properties | same (a shape handle is not yet dragged in a project) |
| Make Adaptive as a toggle | the same | same |
| Next number on Make Adaptive; gaps closed; typing a number swaps | the same, the number typed in Properties | differs: not yet typed on its label in the view |
| Show Placement Number, Show Reference Planes, Visible | the same, with Revit's defaults | same |
| Orients to, six options, default Instance (xyz) | the same, by the rules measured (§1.4) | same; X and Y of the "then Host" options follow the help, unmeasured |
| Offset along the normal, Rotation Angle | OffsetZ and RotZ, beside OffsetX/Y and RotX/Y (a superset), each with its "=" | differs: three of each |
| Mirrored, Flipped | none | missing |
| Six measurement types and Measure From | the same | same; Angle is refused, since there are no arcs yet |
| Hosting on a line: X along the tangent, turned by Rotation Angle | four orientation rules of our own | differs |
| Hosting on a face (U, V), at a crossing of edges, edge and face | face, edge, vertex of a form by index; Intersection of two directions | differs |
| The grey button that ties a number to a parameter | "=" on each offset and rotation, the measurement's value, Visible and a form's Material: Associate Family Parameter with New Parameter; every field of the relationship also takes a formula | same |

### 2.3 Forms

| Revit | BIMCraft today | Status |
|---|---|---|
| Create Form works out the form from the selection | Create Solid/Void Form: one loop extruded by Depth, two or more lofted | differs |
| Surface from an open line, sweep, swept blend, revolve | no | missing |
| Extrusion height dragged with the 3D control | Depth in Properties | differs |
| X-Ray, Add Edge, Add Profile, Dissolve, Lock Profiles, Edit Profile | no | missing |
| A void cuts solids | yes | same |

### 2.4 The 3D control

| Revit | BIMCraft today | Status |
|---|---|---|
| Axis arrows and plane handles | axis arrows | differs |
| Colours: blue Z, orange local; Spacebar for global/local | red/green/blue for X/Y/Z, global only | differs |
| A hosted point slides on its host | yes | same |
| Tab cycles candidates | no | missing |

### 2.5 In the project

| Revit | BIMCraft today | Status |
|---|---|---|
| Click points in order, preview under the cursor | click points, in plan only | differs: no 3D, no preview |
| Esc after k points places with k | Esc forgets the points | **differs** |
| Clicks host on faces, edges, points, nodes | snaps to other instances' points only | differs |
| Points dragged in plan and 3D | numbered grips in plan | differs: no 3D |
| Pick New Host, Flipped, Ctrl-drag copy | no | missing |
| Shape handles dragged after placing | no | missing |
| Divided path and surface, Repeat | no | missing |
| Batch placement (API) | `adaptive_place`, up to 1,000 in one undo step | same, and as a command |

## 3. Where we mean to do better

These are kept to where Revit is painful (§1.10). None of them changes what a Revit user's hands already know.

- **Show each point's axes** while placing and on its grips, and say in Properties what the chosen Orients to does.
- **Reverse, rotate the start or flip** a placed instance without placing it again.
- **Backspace** takes back the last point while placing, and the next number shows at the cursor.
- **A point's X, Y, Z typed** in Properties.
- **Read-only fields for schedules**: point coordinates, edge lengths, angles, area, how far a panel is from flat.
- **Deleting a host** leaves the points where they are, with a warning, instead of deleting the instance.
- **A failure is a diagnostic**, never a deleted instance (we have this already).
- **Agents and scripts** place hundreds of instances in one undo step (we have this already).

## 4. The order of work

Each step is Revit's behaviour first, with a core test for the edit or command and a desktop test that drives it as a user would.
The data comes first: every interface step stands on it.

1. **The point as Revit has it.** Done on 2026-11-02 (the "=" on 2026-11-03), except Mirrored and
   Flipped, and dragging a shape handle in a project.
   - The point type with shape handles.
   - Numbering: the next number, gaps closed, a typed number swaps.
   - Orients to, with its six options and its default.
   - Show Placement Number, Show Reference Planes and Visible.
   - The measurement types and Measure From; a single Offset and a Rotation Angle.
   - Our offsets and rotations on three axes stay in the engine as a superset, behind the same fields. A file needs a migrate
     step.
2. **The family editor's space.**
   - Ref. Level and the two Center planes.
   - Work Plane Set / Show, with the plane under the cursor.
   - Each point's three planes, pickable.
   - Draw on Work Plane / Draw on Face; 3D Snapping and Chain.
   - Model and reference lines; Spline Through Points.
   - Properties grouped and named as Revit's.
3. **Create Form as Revit does it.**
   - The form worked out from the selection: surface, extrusion, loft, sweep, revolve.
   - The extrusion's height dragged.
   - Then Modify | Form, starting with Edit Profile and Dissolve.
4. **The 3D control.** Plane handles, Revit's colours, Spacebar for global and local, Tab to cycle.
5. **Placing in the project as Revit does.**
   - Esc places with the points clicked.
   - A preview under the cursor.
   - Placing in the 3D view.
   - Clicks host on faces, edges and points.
   - Flipped, Pick New Host, shape handles dragged.
6. **Divided path and surface, and Repeat.**

How the instance's axes follow from its points, and what each Orients to option gives on a sloped, a vertical and a curved host,
were measured (§1.4). Still to try by hand in Revit, since the API cannot press Esc:

- what Esc after some clicks leaves the unclicked points at;
- whether the Component tool then starts the next instance.

The test project is kept at `Documents\revit-adaptive-tests\adaptive-mass-test.rvt` on the studio machine, outside the
repository.

## Main sources

- Autodesk Revit help, the pages on adaptive components, reference points, the conceptual design environment, forms and the
  3D control (2014 to 2025): help.autodesk.com.
- The Revit API reference for AdaptiveComponentFamilyUtils, AdaptiveComponentInstanceUtils, ReferencePoint and
  PointElementReference: revitapidocs.com, rvtdocs.com.
- The Building Coder, Jeremy Tammik:
  - "The Revit Form Creation API" (0179);
  - What's New in the 2012, 2013, 2014 and 2016 APIs (0903, 0904, 0938, 1311);
  - 1573 (hosting on a family instance's edge);
  - 1581 (planes along a reference line);
  - 1616 (placing many panels fast).

  All at jeremytammik.github.io/tbc.
- Practitioners: RevitCat (adaptive component origins, forms in the conceptual environment), Graitec (curved glass with
  adaptive components), bdmackey consulting, nikog.net, Modelical, Autodesk forum threads.
