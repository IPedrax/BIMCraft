# Railing tool upgrade: plan

Branch `feat/railing-tool-upgrade`. The railing today (`stairs::railing`, `stairs::railing_parts`, `RailingType` in
`model.rs`) is a line, sketched on a level or taken from a stair's or a ramp's walk line moved out to one side, carrying one
50 mm square rail at its type's `height` and 20 mm square balusters its type's `spacing` apart. A type has nothing else: no
profile, no material, no posts, no panels, no handrail apart from the top rail, no extensions, no supports.

## Research

How Revit, Archicad and Vectorworks make railings, and what their users complain of most. From the vendors' public help and
their users' forums (sources at the end); no content of theirs is used, only what the tools do and what people say of them.
Most forum threads are years old and some problems may be fixed in current releases; the patterns repeat across versions.

### Revit

- **A railing is a path and a type.** The path is sketched (lines and arcs) or made by the stair (`Railing > Place on
  Stair/Ramp`, tread or stringer). A railing can be hosted by a stair, a ramp, a floor, a roof or a toposurface (Pick New
  Host), and then follows its slope.
- **The type is a system of parts.** Top Rail (its own type: profile, height, extensions, terminations); Handrail 1 and 2
  (each its own type: height, profile, side, hand clearance, supports, extensions); Rail Structure (Non-Continuous), a list of
  intermediate rails, each a name, height, offset, profile and material; Baluster Placement.
- **Baluster Placement** is a dialog of its own. The main pattern is a row of balusters, each a family with a base (host,
  a rail) and a top (a rail), offsets and a distance from the previous. The pattern is justified (beginning, centre, end, or
  spread to fit); what's left over is truncated or filled with another baluster at a spacing; the pattern can break at each
  segment end, at angles over a value, or never. Posts are separate: start, corner and end posts, each a family. On stairs,
  "balusters per tread" places N per tread instead of the pattern. Panels (glass) are balusters too, of a panel family.
- **Continuous rails** (top rail, handrails) join by type: angled joins (extension or none), tangent joins (extend to meet,
  none), rail connections (trim or weld). Extensions at the beginning and end are to a wall, the floor or a post, with a
  length; terminations are families.
- **On a stair**, a railing has Landing Height Adjustment, Angled/Tangent Joins and Rail Connections as type properties, and
  Edit Rail (in recent versions) edits one continuous rail's path and per-segment heights (Edit Path, slope by host / flat /
  sloped per segment).

Usual complaints:

1. **Hardest tool to learn.** Types inside types (railing, top rail, handrail, baluster, post, panel, support, termination
   families), several dialogs, and results seen only after Finish. Users and trainers alike call it the hardest tool in Revit.
2. **Balusters on stairs.** Gaps under balusters at treads and stringers; balusters that don't meet the stringer; custom
   baluster families that look right flat and wrong on a slope; centre-justified patterns inconsistent between segments.
3. **Landings and transitions.** Rails that step, kink or don't meet at landings; glass panels that don't connect at a
   landing; railings on two separate stairs never join (draw them as one stair, and deleting the auto landing deletes its
   railing too). Spiral and curved stairs give wrong rail heights in several versions.
4. **No editing of one piece.** Can't move, delete or replace one baluster or post without breaking the pattern; supports are
   auto-placed where people don't want them (Tab, unpin, move); non-continuous rails can't be shorter than the path or start
   and end on their own.
5. **Everything is by type.** Extensions, supports and heights are type properties, so one railing with a different extension
   doubles the types. Users ask for instance values.
6. **Glass.** Panels are a fixed-width baluster, so the pattern fits round panels; on slopes and curves they don't reshape.
7. **Materials.** Each part's family has its own material; users ask for one material on the railing that every part takes.
8. **Wanted:** a rule-based "railing constructor" (straight, arc, sloped pieces and transitions, each with rules), gates, 3D
   railing paths for stairs with many flights, easier per-element editing.

### Archicad

- **A railing is segments and nodes.** It follows a reference line drawn as a polyline, or picked from a stair's or a slab's
  edge (associative: it updates when they move). A segment is the part between two nodes; nodes carry posts.
- **A segment carries a pattern**: inner posts, balusters, rails, a top rail, a handrail and panels. The rightmost inner post
  sets the pattern's length; the pattern is distributed by fixed length, by number, or best fit, and restarts at each node
  unless Pattern Continuity carries it on.
- **Every part is a library part** with its own settings, set for the whole railing in the tool's tree (Railing > Segment >
  Pattern > Post/Rail/Panel) or picked in Edit mode for one segment or node, which then stops following the general settings
  until reset. "Convert Pattern to Segment Length" turns each repetition into a segment so its inner posts can be moved.
- **Panels** fill between top rail, rails, inner posts or the segment's ends, at any height, offset from the line.
- **Ends and connections**: end posts, rail ends (to a wall, to the floor, bent, extended) and connections between segments
  (by node) are their own settings.

Usual complaints:

1. **Overcomplicated.** "Hours to make a simple handrail"; the settings tree is deep, the same thing is set in two places
   (the tool's dialog and Edit mode's palette) with different results, and some changes don't apply in the 3D window.
2. **Segments don't connect.** Two segments with different slopes or directions don't join cleanly; inner posts won't centre;
   behaviour inconsistent, blamed on associativity.
3. **Wrong heights on stairs.** The reference line snaps to wrong points and the rail slopes diagonally; insertion points land
   at the wrong Z; rail ends set in the stair settings apply to one end only.
4. **One element in plan.** A railing along a stair and up onto the slab is one element, so its plan display can't differ by
   part (below the cut dashed, above solid; lower rail under the slab); the break mark follows the stair. Workaround: two
   railings sharing a post.
5. **Edits lost.** A segment or node edited by hand no longer follows the overall settings, and resetting loses the edit.
6. **At most two handrails**, and ADA/accessible extensions hard to model.
7. **Wanted:** simpler presets, wall-mounted handrails without a guardrail, reliable stair association.

### Vectorworks

- **Two tools.** The Railing tool (Design Suite, recent versions) creates a railing along stairs or slabs automatically, or on
  a drawn path (polyline modes), as a handrail, a guardrail, or both; railings from adjoining edges are joined into one
  object, and Railing Join joins separate ones. The older Railing/Fence tool draws a path object with posts (round,
  rectangular, octagonal, a custom 2D profile or a 3D symbol), a top rail (shape or custom profile), rails and pickets; posts
  by approximate distance or by number, evenly along the path; Reshape adds or removes one post.
- **Styles** hold the settings (by style or by instance per parameter). The handrail has brackets, set by number or spacing
  and taken from the guardrail's posts when there is one, shown in plan or not; terminations extend along the whole path or
  end at the first and last post.
- **The stair object** has its own Railings tab, which puts guardrails along the stringers.

Usual complaints:

1. **The older Railing/Fence object doesn't know the stair.** Drawn along treads and landings it needs heavy hand-editing, and
   at a landing it makes a strange vertical jump; only usable in plan.
2. **Flat rails.** A rail drawn on a layer stays flat at that Z; 3D-line mode is the workaround, but it draws separate pieces
   that don't join at corners (U-shaped rails).
3. **Geometry often wrong**, and stair output made of polygons rather than solids.
4. **Stair railings only along the stringer**; to control them, turn them off and model the rail by hand, or ungroup the
   stair to a mesh and lose its parameters.
5. **Profiles**: the handrail tool only did square or rectangular profiles.
6. **Changes in direction**: users ask to connect handrail ends like walls.
7. **Tool resets** its settings after choosing a configuration from the resource popup.

### What the three have in common

- The railing is a **path plus a stack of parts** (rails at heights, posts at nodes, balusters or panels in a pattern), in all
  three. The difference is where the settings live: Revit's nested types, Archicad's segment tree, Vectorworks' styles.
- The **stair is where they fail**: heights at landings and turns, joins between runs and between a stair and the slab above,
  and the railing as one element across both.
- **Per-piece edits** fight the pattern in all three.
- **Too many places to set one thing**, and results seen late.
- **Ends** (extensions to a wall or the floor, returns, terminations) are hard, and accessibility codes ask for them.

## Decided (2026-11-24)

- **Several hosts.** A railing follows stairs, ramps and floors together: a stair and the floor it comes up to. Picked as
  Edit Group picks members: Edit Hosts, then Add and Remove, clicking the hosts.
- **No top rail and handrail types.** Revit's split is too complicated. A railing type is made visually, as Archicad's and
  BIMCraft's curtain wall Edit Pattern are: add a part, click it, set its shape (rectangle, circle; a custom profile later),
  material, height and offset.
- **Rails** are named, so any upright can go up to one ("up to Rail1") or stand on one.
- **Uprights** are of two kinds: a post stands on the host; a baluster stands where its bottom says (the host or a rail),
  with offsets at both ends.
- **A panel** can fill each bay between posts instead of balusters: material, thickness, gaps at the bottom, the top and the
  posts, and an offset across.
- **Height** is worked out: the top surface of the top rail.

## First version (built)

- Core: `RailingType` gains `rails`, `posts` (posts and balusters) and `panel` (`model.rs`); `railings.rs` lays them along a
  railing's line (`railings::build`) and checks them (`check_type`); a type without them is the older one rail and
  balusters. `Railing` gains `hosts`; `railings::follow` gives a line the heights of the hosts under it. Commands: `type`
  takes `rails`, `posts`, `panel`; `railing` takes `hosts`; `railing_hosts {id, add, remove}`. Deleting a host drops it;
  copying a railing with its hosts points at the copies.
- 3D: each part in its material, round ones as tubes. IFC: each part as a prism (round ones as their box). Plan: the line, as
  before.
- Desktop: Type Properties of a railing type shows its height and Structure: Edit…, which opens Edit Railing (`railingtype.rs`):
  Add Rail, Add Post, Add Baluster, Add Panel, Remove; a list of parts; the picked part's fields; a preview of two bays in
  elevation and a section across, where a click picks a part. A railing selected: Modify tab, Railing > Edit Hosts; Add,
  Remove, Finish; the hosts lit while editing.

Second round (comments of 2026-11-24): Edit Railing in three columns of a fixed size (parts by kind; parameters by
section; the preview), in plain English; posts lined up with every so many steps, on the first and last too by default;
posts and balusters standing on the treads; rails over the nosings as one mitred sweep each, climbing smoothly from a
landing into the next flight; sloped and shaped floors, roofs and toposolids as hosts; Edit Path. Checked on straight, L,
U, winder and spiral stairs.

Third round: a rail is solved, then swept: the solver finds its real bends and rounds them (corner and bend radii per
rail), the sweeper keeps its faces flat; a gooseneck where a flight starts above a landing (the type's transition); the stair
no longer pulls the line down past its foot; Edit Path in 3D on the host's plane; a sketch grip dragged snaps from its line's
other end, with Shift and the guides.

Next, after comments: custom profiles, the plan showing rails and posts, ends and extensions, gates.

## What the research suggested for BIMCraft

Proposals, to settle before building:

1. **A railing type as a stack, in one place.** Top rail, handrails (any number, each a side, a height, a profile, an
   offset, wall-mounted or on posts), intermediate rails (heights, profiles, each its own start and end), posts (start,
   corner, end, and every N mm or by count), infill: balusters at a spacing (justified, spread to fit, or N per tread), panels
   between posts, or none. Profiles from `profiles.rs` or round/rectangle. A material per part, one material on the type that
   every part takes unless it names its own.
2. **One continuous rail, its heights by rules.** The rail's line in 3D is worked out from the host: run slopes, landings
   flat, a smooth or a stepped transition at the turn (Revit's angled/tangent joins), a height kept at the nosing line on
   runs and above the floor on landings. The stair and the slab above make one railing that joins.
3. **Instance overrides** of extensions, height and side, so a type isn't copied for one end.
4. **Ends:** extensions (horizontal by a length, down to the floor, return to a wall, to a post) at each end, as the code
   asks; NBR 9050's 300 mm and the codes in `codes.rs` can check them.
5. **Per-piece edits** that survive: hide or move one post, change one baluster; kept as exceptions by position, so the
   pattern stays the rule.
6. **Plan display by part:** below and above the cut plane, a railing shown as the view range shows a stair, with the break
   line.
7. **Hosts:** stair, ramp, floor edge, roof edge, toposolid, and sketched on a level with a slope.
8. **Gates**, later.

Each is a command or a parameter, tested through its output (the 3D scene, the plan), as the rest of BIMCraft.

## Sources

Revit (Autodesk help and forums):
- [Railing System Type Properties](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-6385104B-4295-4659-855F-BA7DD2793CD5.htm)
- [Edit Baluster Placement Dialog: Main Pattern](https://help.autodesk.com/cloudhelp/2023/ENU/Revit-ArchDesign/files/GUID-426BE12E-A906-457B-A205-A5DFF08B07D7.htm)
- [Create a Custom Baluster Pattern](https://help.autodesk.com/cloudhelp/2018/ENU/Revit-Model/files/GUID-84F925AC-E09A-4C79-A7F7-EA2F20FF6364.htm)
- [Define a Continuous Rail Extension](https://help.autodesk.com/cloudhelp/2020/ENU/Revit-Model/files/GUID-F9E9C135-949F-41B2-B179-3CAE7024D616.htm)
- [BIM Pure: 14 tips to understand Revit railings](https://www.bimpure.com/blog/14-tips-to-understand-revit-railings)
- [Revit Pure pamphlet: Railings part 1](https://static1.squarespace.com/static/5605a932e4b0055d57211846/t/5f99c642729ea21601fb79af/1603913283971/RP-Pamphlet18-Railings-Part-1.pdf)
- Forum: [non-continuous railing issue](https://forums.autodesk.com/t5/revit-architecture-forum/railing-non-continuous-railing-issue/m-p/7466289),
  [stair balusters have gaps](https://forums.autodesk.com/t5/revit-architecture-forum/stairs-balusters-have-gaps/td-p/9130345/page/2),
  [balusters don't meet stringer](https://forums.autodesk.com/t5/revit-architecture-forum/can-t-get-balusters-to-meet-stringer/m-p/11362347),
  [inconsistent railing at landing stringers](https://forums.autodesk.com/t5/revit-architecture-forum/inconsistent-railing-treatment-at-landing-stringers/m-p/11654132),
  [glass panel railing at stair landing](https://forums.autodesk.com/t5/revit-architecture-forum/glass-panel-railing-at-stair-landing-2017/m-p/7614322),
  [joining rails from separate stairs](https://forums.autodesk.com/t5/revit-architecture-forum/joining-rails-from-separate-stairs/m-p/7009563),
  [centre justification of balusters](https://forums.autodesk.com/t5/revit-architecture-forum/center-justification-of-balusters-in-a-railing-segment/m-p/9394556),
  [railing pattern design](https://forums.autodesk.com/t5/revit-architecture-forum/railing-pattern-design/m-p/5593362),
  [curved railing](https://forums.autodesk.com/t5/revit-architecture-forum/how-to-create-curved-railing/m-p/8002046),
  [spiral staircase railing height](https://forums.autodesk.com/t5/revit-architecture-forum/spiral-staircase-railing-height/m-p/13189396),
  [mandatory railing improvements](https://forums.autodesk.com/t5/revit-architecture-forum/mandatory-railing-improvements/m-p/6820682)
- Ideas: [railing constructor](https://forums.autodesk.com/t5/revit-ideas/railing-constructor/idi-p/8255018),
  [extension parameters by instance](https://forums.autodesk.com/t5/revit-ideas/railing-extension-parameters-should-be-instance-based/idi-p/12489277),
  [rail structure to begin and end](https://forums.autodesk.com/t5/revit-ideas/railing-allow-rail-structure-to-begin-and-end/idi-p/6703283),
  [make railing more user friendly](https://forums.autodesk.com/t5/revit-ideas/please-make-railing-more-user-friendly/idi-p/6700456),
  [railing material](https://forums.autodesk.com/t5/revit-ideas/railing-material/idc-p/11150810)

Archicad (Graphisoft help and community):
- [Railing Settings: Overview (AC29)](https://help.graphisoft.com/AC/29/INT/_AC29_Help/040_ElementsVB/040_ElementsVB-211.htm)
- [Railing Tool Basics: Convert Pattern to Segment Length](https://community.graphisoft.com/t5/Modeling/Railing-Tool-Basics-Convert-Pattern-To-Segment-Length/ta-p/303679),
  [Pattern Distribution Settings](https://community.graphisoft.com/t5/Modeling/Railing-Tool-Basics-Pattern-Distribution-Settings/ta-p/303682)
- Forum: [inner posts](https://community.graphisoft.com/t5/Modeling/Railing-Tool-and-Inner-Posts-what-am-I-missing/td-p/295381),
  [AC21 railing: GS you can do better](https://community.graphisoft.com/t5/Modeling/AC21-Railing-GS-you-can-do-better/td-p/298639/page/2),
  [AC22 railing tool](https://community.graphisoft.com/t5/Modeling/AC-22-Railing-Tool/m-p/271417),
  [buggy railing tool still](https://community.graphisoft.com/t5/Libraries-objects/buggy-railing-tool-still/m-p/681616),
  [connecting segments](https://community.graphisoft.com/t5/Modeling/Railing-tool-connecting-segments/m-p/282775),
  [hours for a simple handrail](https://community.graphisoft.com/t5/Modeling/Why-does-it-take-hours-to-try-and-do-a-SIMPLE-handrail-etc/m-p/603293),
  [handrail tool broken](https://community.graphisoft.com/t5/Libraries-objects/handrail-tool-is-beyond-stupid-and-broken/m-p/396114),
  [AC21 handrail tool](https://community.graphisoft.com/t5/Modeling/AC21-handrail-tool/m-p/291845),
  [multiple stairs](https://community.graphisoft.com/t5/Modeling/Troubles-with-railing-when-connecting-multiple-stairs/m-p/240826),
  [railings not represented properly](https://community.graphisoft.com/t5/Collaborate-forum/Railings-not-being-represented-properly/m-p/357974),
  [stair rail plan view](https://community.graphisoft.com/t5/Modeling/Stair-Rail-Plan-View-Problem/m-p/256242),
  [wishlist: improvement on the railing tool](https://community.graphisoft.com/t5/Wishlist/Improvement-on-the-Railing-tool/idc-p/646362)

Vectorworks (help and community board):
- [Creating railings (2025)](https://app-help.vectorworks.net/2025/eng/VW2025_Guide/Stairs/Creating_railings.htm),
  [Editing railings (2025)](https://app-help.vectorworks.net/2025/eng/VW2025_Guide/Stairs/Editing_railings.htm),
  [Stair settings: Railings tab (2026)](https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Stairs/Stair_settings_Railings_tab.htm),
  [Railing and fence settings (2023)](https://app-help.vectorworks.net/2023/eng/VW2023_Guide/SiteModel3/Railing_and_fence_settings.htm)
- Forum: [stair handrail, custom or settings](https://forum.vectorworks.net/topic/100300-stair-handrail-custom-or-settings/),
  [railing tool / stair handrail 3D](https://forum.vectorworks.net/index.php?/topic/48208-railing-tool-stair-handrail-3d/),
  [stair tool pain points](https://forum.vectorworks.net/index.php?/topic/58406-stair-tool-pain-points/),
  [stair tool rehab](https://forum.vectorworks.net/topic/51961-stair-tool-rehab-for-2050/),
  [handrail profiles](https://forum.vectorworks.net/topic/34137-handrail-profiles/)
