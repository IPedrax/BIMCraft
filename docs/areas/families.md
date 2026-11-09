# Families, forms, materials and textures

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A family (`Model::families`, `family.rs`) is parameters, types that set them and forms in its own axes: x along a
component's angle, y to its left, z up from its base; a form is a profile drawn on one of its reference planes or a reference
line's plane (its `host`, below) and swept along that plane's normal, its sizes numbers or formulas, or triangles as a file
brings them. A component places a type;
`Model::component_pieces` are its forms where it stands. Plans draw them from above, a form's edges hidden under a higher
form of the same component. A section cuts the forms of a family whose category is built into the building (`Category::cut`:
door, window, casework, generic, equipment, rack, pipe fittings and accessories), poché in their own shape where its plane passes
through them (`view3d::slice`: each triangle crossed, the crossings joined by place into loops, since `csg` leaves an edge split on
one side and not the other, and a loop inside another a hole); furniture, fixtures, devices and light fittings it shows beyond
the line and never cuts, as Revit does. The section's work holds no component's triangles: it makes those of a component it
cuts when the line's plane crosses its box, as it does a round run's. The
3D view takes a form's triangles that share an edge and turn under 20° there as one curved surface, so a curve draws no
edge between its facets (`view3d::facets`) and shades smoothly across them, each corner lit by the angle-weighted mean of
its own surface's normals there, so a crease keeps its step (`corner_normals`); IFC writes each form as a closed face set,
triangles turned out, and reads one family per family name the file gives.

A form is swept, or with an `axis` turned about it (`family::revolve`: `from` to `to` degrees, 32 steps to a turn, the profile to
one side of the axis); a `void` cuts the solid forms listed before it (`csg::subtract`: convex polygons in binary space
partitions, built and walked with a worklist rather than recursion and bounded by a `Meter`, so a hostile shape ends in an error;
a cut piece keeps its uncut outline in plan). A family of category `door` or `window` is hosted: its `Width` and `Height` (English
names, whatever the project's language) size the wall's opening, `Sill Height` is a window's default sill, its forms are drawn in
the opening's axes (x along the wall from the middle of the opening, y across it from the wall's middle plane and + toward its
exterior, z up from the sill) and its formulas read `Wall Thickness`; its `symbols` also draw in plan (a door's swing). Its `connectors` say what an electrical device draws ([structure and MEP](structure-and-mep.md)); a family file keeps them. An
`Opening` holds a `family`, or none (one of the project's own types): `Model::opening_dims` gives its size either way
(`opening_type` returns it by value), its flips are mirrors (`opening_local`), and `Element::family()` is the one way to ask what
family a component or an opening is of. A section cuts a family opening's forms where its line passes through them.
A hosted family's voids cut its wall as Revit's do (`Family::host_voids`): a void that reaches into the wall (its `Wall
Thickness` about the opening's middle plane), wherever it is listed, is the opening's shape in the wall in place of the
rectangle of its `Width` and `Height`, unless its `cuts` says otherwise (F10d, below); a family with no such void cuts the
rectangle as before. `Model::opening_voids_local` gives them in the family's axes with the opening's flips, `Model::opening_voids` where it
stands. The wall's prisms leave such an opening out (`walls::prisms_in`; `prisms_rect_in` keeps every rectangle) and
`view3d::wall_solids` takes each prism a void reaches as triangles less it (`csg::subtract`), a `Solid::Mesh` a section cuts
(`cut: true`), so the 3D view and sections show an arch as an arch and a void deeper than the wall goes right through it;
should cutting fail, every opening cuts its rectangle. `view3d::layered` splits such a piece into the wall type's layers (each
the piece within its two boundaries in plan, `walls::layer_lines`), and a section's lines between layers follow the cut round
the void. A plan clears the void's slice at its cut plane (`render::level_slice`) within the wall's faces, its edges across the
wall the jambs; whether the opening is drawn at all is still its rectangle's height. IFC writes the voids as the
`IfcOpeningElement`'s body, closed face sets placed at the opening's middle as its forms are, in place of the extruded
rectangle. A plan draws a family opening's forms the cut plane passes through as it cuts them (`view3d::level_slice`: a frame
extruded across the wall, its jambs), each edge of a void's cut from one face of the wall to the other a jamb. A void's size may read an instance parameter (a depth in its `from` and `to`), so each opening cuts its own.
Neither is new in a file, so nothing takes a FORMAT step: an older build reads the family and cuts the rectangle. Below the cut
plane (F10b), a plan sees the family's forms from it, in projection, as walls below the cut are drawn: each form's outline there
(its triangles clipped at the cut and laid flat, an edge drawn where one side of it is covered and the other not, so the
triangles a cut leaves along a face draw nothing between them), less what the cut covers (the forms' cut rings, and the wall
cut round the opening) and what a form reaching higher covers, in the category's projection pen, lighter than the cut's; an
edge the cut's line runs along already is left to it (`render::below_cut`). So a frame across the wall shows its lower rail
between its cut jambs, and a sill board outside the wall its edge; nothing above the cut is drawn. A family's plan is the
project's drawing of its scratch model, so it shows the same. Past `MAX_BELOW` triangles, each form's outline stands for it.

F10d, part of FORMAT 19: a void's `cuts` (`family::Cuts`): `both` (left out of the file) cuts the solid forms listed before it that it
reaches and, in a door's or a window's family, the wall wherever it reaches into it, whatever its place in the list; `host`
the wall alone, never the family's forms (the opening's void round its frame, which it would otherwise cut away: Revit's
Uncut Geometry, as one setting); `forms` the forms alone, leaving the wall to the rest (a sash's light, as the Casement
Window's starter has its two). Before `cuts`, a void cut the wall only if it cut none of the solid forms listed before it, by
their boxes, so the order mattered: a void had to be put first. `Family::check` refuses `cuts` on a solid; the "cuts
nothing" warning reads it (a void of `host` in a generic family cuts nothing); a work plane-based family's face voids follow
the same rule as a door's wall: a `both` or a `host` void whatever it overlaps and wherever it is listed, never a `forms`
one (`Family::face_voids`). In the family document, a void's Properties
(Identity Data) has Cuts (Forms and Host, Host, Forms) in a door's or a window's family.
`Model::migrate` takes a file of 17 or 18 to 19 (`Family::pin_cuts`; upstream's 18 changed only connectors, and its step
needs nothing): each void of a door's or a window's family that the old rule didn't cut the wall with, though it reaches into
it (it overlapped a solid listed before it in any of the family's types), says `forms`, so it cuts what it did; one that cut
the wall cut none of the forms, and reads as it did. The old rule was only ever in unreleased builds of the Family Editor:
upstream's 17 and 18 never cut a wall with a family's void, only with the opening's rectangle. Its limit: one `cuts` is a
void's for every type, and the old rule decided type by type. Where the types differ (a Window whose `Shift` puts its void
over its sill in one type and clear of it in another), or an instance parameter moves the void onto a solid or off it, the
union wins: overlapping in any type, as the type's values and the instance parameters' defaults have it, the void says
`forms` for all of them, so a type whose void was clear and cut the wall cuts it no more and keeps the opening's rectangle,
as upstream's 17 drew it (`host_void_tests`' Shift window). Pick `both` or `host` on that void to cut the wall again. A build
that reads up to 18 would leave `cuts` out and cut the wall with a `host` void listed after its frame no more. A family file
carries no FORMAT, so it is read as 19 reads it.

How a door or a window opens is its family's `operation` (`model::Operation`: swing, double-swing, sliding, double-sliding,
folding, fixed), or for one of the project's own types the type's (`OpeningType::operation`); none, a door swings. IFC writes
it as the door's `OperationType` (`Operation::ifc_door`, its side from `flip_hand`: `SLIDING_TO_RIGHT`) and a window's
`PartitioningType` (two leaves, `DOUBLE_PANEL_VERTICAL`); the import reads a door's back, or its `IfcDoorType`'s (IFC4) or
`IfcDoorStyle`'s (IFC2x3), onto the type it makes. Schedules have it as the `Operation` field, in the project's language. A
window's panel operation (`IfcWindowPanelProperties`, a sliding sash) needs an `IfcWindowType`, which the export doesn't write yet.
What BIMCraft has no element for comes in from IFC as a component of a generic family that remembers the class it came in as
(`Family::ifc`, an `IfcClass`: `{"class": "IFCPLATE", "predefined": "CURTAIN_PANEL"}`): a covering that is no ceiling, a curtain
wall's panels and mullions, members that don't frame as a beam ([IFC import](ifc-import.md)), plates, duct fittings, terrain (`IfcGeographicElement`), a lift, anything the file builds
that has a body BIMCraft can read. The import reads it, after every element of BIMCraft's own, through the same path as
furniture (`import::ifc`'s `component`, families settled by `settle`): its predefined type is its own, else its type object's,
`NOTDEFINED` left out; its object type is remembered where it says what it is (the predefined type is `USERDEFINED`, or it isn't
the name the family and type take), so a family is one per name and class, and one of another class takes the name numbered on
(`Fin (2)`). Never generic: openings and the other features, virtual elements, annotations, spatial elements, grids, ports,
the structural analysis model (`import::NOT_GENERIC`), a part of an element that came in (a wall's or a roof's), and an
assembly (`IfcCurtainWall`, `IfcElementAssembly`) whose parts came in, which is left out and listed as such; one whose parts
have no body comes in itself. What stays out is listed by class, with what has no body BIMCraft can read said so, and the
`open` reply's `generic` counts what came in. The export (`Family::ifc` and `ifc/classes.rs`, IFC4's element classes as
`ci/ifc4_classes.py` reads them from the schema) writes a generic family's components as the class remembered, its attributes
after `Tag` as IFC4 lays them out (an element assembly's place ahead of its predefined type, a door's sizes), the predefined
type where IFC4's class takes it (else `.NOTDEFINED.`; nothing for a class with none, `IfcFlowSegment`), the object type
remembered else the `Family:Type` name, and a type object only where IFC4 has a `<CLASS>TYPE` that isn't abstract (none for a
`*StandardCase` or a flow terminal), with the predefined type but `USERDEFINED`, which a type object would have to name. A
class IFC4 doesn't have (IFC4X3's `IfcKerb`) goes out as a proxy, `USERDEFINED`, its object type the class. The `family`
command sets it (`ifc`, written in capitals, `null` to forget it) and `Family::check` refuses what IFC can't write: a class that
isn't `IFC` and capitals, a predefined type of anything but capitals, digits and `_`, an object type of more than a line. A
family of another category keeps what it remembers but goes out as its category says. An older BIMCraft leaves `ifc` out of
a project and opens it, its components going out as proxies, so the field took no `FORMAT` step.
A material's `texture` (`texture.rs`) is a pattern drawn in code or a picture of `Model::images` (a PNG or JPEG comes in, is kept
as a PNG of at most 1024 px, its header checked before the decoder makes room for it), laid on a face at its `size` in mm by the
face's own axes (`view3d::basis`: across is level, up goes up it, east and north where it lies flat), sampled per pixel from the
depth buffer, in the 3D view alone, from a mip chain. A triangle carries a skin (`view3d::skin`): in its low 24 bits a texture id (one more than its material's place in the
library), in its high 8 its material's `transparency`; `Scene::new` resolves the textures. A material's `transparency` (percent,
0 to 100, 0 opaque and not written, the starter Glass 70) is how see-through the 3D view draws what is made of it, with no view
override: a layer's skin wins over its element's, paint gives a face its own, and a family-less window's glass takes the Glass
material's. IFC carries it in `IFCSURFACESTYLESHADING`'s second argument (0 opaque to 1 clear, `$` for opaque), read back from a
shading or a rendering. Only the nearest see-through surface is blended, so a glass wall's far face and stacked glass don't
accumulate; plans and sections ignore it. Paint (`paint.rs`) is `{face: material}` in an element's `params` under `Paint`, a reserved
name: a triangle's face is read from its winding, so every solid emits triangles turned out (`Solid::tris`, `prism`), and
`Image::face` says which face a pixel is on. Favorites (`favorites.rs`) are a file of the user's own (`default_path`:
`BIMCRAFT_FAVORITES`, else the configuration folder) with materials and their pictures, families and their materials; they change
a project only through the `favorite` command (`load` never overwrites without `replace`). The desktop's Family Editor
(`editor.rs`, `editor/document.rs`) opens every family in a document of its own, with a family ribbon,
docked Properties and Project Browser, and a 3D view: an adaptive family's modelling view and tools
([adaptive families](adaptive-families.md)), another's {3D}. Each document's views are tabs of the window's, after the
project's (`Sheet::Family`, `editor/tabs.rs`), titled "{family} - {view}" with the family's icon; any number of documents may be
open, each with its own draft, history (Undo on its tabs undoes its steps, never the project's), selection and tool, and the
project's tab shows the project's ribbon, Properties, Browser and tool again. A view the document goes to (its Project Browser,
Go To View) opens its tab; closing a tab that isn't the document's last only closes it. The last one, or Close Family (that
tab's ×), closes the document: with changes not loaded, "Save changes to {family}?" asks first (Load into Project and Close,
Close Without Loading, Cancel), and a nested family's document open over it closes first. File > New, Open and Close, and
quitting, ask about each document before the project's own question. Load into Project leaves the document open and clean;
Load into Project and Close goes back to the tab it was opened from. Edit Family on a family whose document is open, or opening
a file that is, brings that document forward: a family is never open twice. A new blank family is "Family1", "Family2" and so
on. A nested family's document (Edit Family in a family) reads Load into Family; it loads into the document it came from. A
family opened from the start page with no project behind it shows no project tabs until something is loaded into the blank
project. Family tabs neither float nor tile. Properties show what is selected, as Revit's do: nothing
selected, the family's own (Identity Data, then Family Category and Parameters' Other group); a form, a symbol, a reference
plane or line, a dimension or a nested instance, its own. The forms and the plan symbols of a family that isn't adaptive are
listed in the Project Browser (Forms and Symbols, `family_browser`): a click selects one, and its right-click menu has Select,
Delete and, for a form, Move Up and Move Down (the list's order is what a void's "the forms listed above it" means) and Edit
Extrusion, Edit Blend and the like (a mesh from a file has no sketch, and so none). Forms are made on the Create tab (Extrusion,
Blend, Revolve, Sweep, Swept Blend, Void Forms), symbols on Annotate's Detail panel (Symbolic Line, a line or an arc; Text; Dot),
each made selected so Properties show its rows, and Manage's Adaptive Component makes the family adaptive (Make Family Adaptive,
or Four-Point Panel for a ready-made one); Family Types and Family Category and Parameters open dialogs
(Apply, OK, and Cancel putting back what the dialog began from). Extrusion, Blend, Revolve, Sweep, Swept Blend and Void Forms
sketch forms (F5 and F6, below). Undo and Redo are the document's own (`Draft`), and never undo the project.
Family Types is Revit's (`editor/types_dialog.rs`): Type name, a drop-down of the types with New Type (a name asked, the type
shown copied), Rename Type and Delete Type (refused for the last, saying why); Search parameters (by name, case aside); one table,
Parameter, Value, Formula and Lock, under the groups' headings in `Group::ALL`'s order (the Properties palette's), each folding.
A type parameter's Value is the type shown's own (else the family's), an instance parameter's, marked "(default)", the family's;
a row a formula drives is greyed with what it comes to; words and reporting parameters take no formula. Apply and OK are refused,
with T9's words, while a parameter's formula doesn't read (`formula_refusal`). Lock (T21) locks every dimension the parameter
labels (their first segment's padlock), so a drag in a view can't change its value; it is greyed for a parameter that labels
no dimension or that a formula works out. Apply and OK are refused, as "Constraints are not satisfied" (*Dimensions and
constraints*), for a value a type's dimensions can't hold. Under the table: New and Edit Parameter (Parameter Properties: family or shared, picked from a shared parameters
file by path; name, Type or Instance, reporting, kind, a family type's category, the group), Edit's rename walking the draft as
`Family::rename_parameter` does; Delete Parameter, the `family` command's `delete_parameter` on a private session holding the
draft, so it is refused with the command's reason while a formula or a form uses it; Move Up and Move Down within a group and
Ascending and Descending sorting each group, since `Family::parameters`' order is saved and is the order every list shows.
Lookup tables and type catalogs are not there.
The editor keeps text drafts (`FormText`, `ParamText`, ...) and changes the project's family only when Load into Project
or Load into Project and Close runs a `family` command. Close Family discards the draft. Save Family and Add to Favorites
prepare a private `Session`, so saving never implicitly loads the draft into the project. The preview is a scratch
`Session` of its own; errors appear inside the document or its dialog. Connectors, reference planes and lines, form hosts,
parameter metadata and symbols remain in the draft even when their dedicated tools are not yet available.

A family document opens as Revit's does (`editor.rs`): File > New > Family… (`new_family_window`: the category picked, as Revit's
New Family picks a template, then `new_family`) and the start page's Families group, under Models, with New Family (the
categories dropped down) and Open Family…; File > Open > Family… and Open Family… (`open_family`, `open_family_file`) open a
family file, BIMCraft's own JSON as Save Family writes it and Load Family reads it (`read_family_file`), as a document that isn't
the project's: Load into Project loads it, asking first (Family Already Exists) when the project has a family of its name, and the
project is left as it is. The family document stands over a project, which the window always has: the blank one it starts with
when given no file, so a family opened from the start page needs nothing more; closed, it goes back to the start page
(`family_from_home`) unless the page was over a project or the family was loaded into the blank one. A file that isn't a family
opens nothing and says why. Edit Family (`edit_family`) is on the contextual tab's Mode panel, at its right end as Revit's (`mode_panel`): a family's instance
selected (the tab named for its family's category, Modify | Furniture), or a component being placed (Modify | Place Component,
beside Load Family, the family being placed); and on the right-click menu. Manage has neither New Family nor Edit Family.

A form selected in the document (from the Project Browser's Forms, or by a click in a view; Escape, a click in the view on
nothing or Modify lets it go) shows its own Properties in place of the family's, as Revit's do for an Extrusion (`editor/extrusion.rs`):
Constraints (Extrusion End and Start in the project's units, each with `=`; a Blend's Second End and First End; a revolve's
End Angle and Start Angle; a Sweep's Path and Profile, a Swept Blend's Path, Profile 1 and Profile 2, each the sketch Edit Sweep
or Edit Swept Blend draws, by its count of lines; the Work Plane, read-only, named as `Planes::listed` names it), Graphics (Visible with `=`; Visibility/Graphics Overrides' Edit… opens F8's Visibility Settings, *Visibility Settings* below), Materials
and Finishes (Material with `=`) and Identity Data (Solid/Void). A value tied to a parameter or worked out by a formula is greyed. The
contextual tab, and the Options Bar, are named for its kind as Revit's (Modify | Extrusion, Modify | Blend, Modify | Revolve,
Modify | Sweep, Modify | Swept Blend, and Modify | Void Extrusion and so on; a mesh from a file, Modify | Form), with Delete and Edit Work Plane, which opens the Work Plane dialog for the form (T12, below); the Options Bar shows
Depth (End less Start: typed, Start stays and End moves; greyed while End is tied). Edits go to the draft and its history. Solid/Void
and Edit Work Plane run `family` (its `form` patch) and `edit_work_plane` on a private session holding the draft, as Save Family
does, and take the form back with the id and GUID it had: the form moves with the plane. Its shape is its sketch's: Edit
Extrusion and the like draw it again, and its dimensions flex it; Properties have no field of its corners. A symbol selected (from
the browser's Symbols) shows its rows (`symbol_properties`): Constraints (a line's points, as typed; an arc's, a text's or a dot's
centre, radius or height and angles, each with `=`; a text's words and alignment), Graphics (Visible with `=`, its Subcategory
and Detail Levels' Edit…), on the contextual tab "Modify | Symbols" (Delete). Symbols draw in plan only, so their numbers are
typed rather than dragged in a view. Sketch mode (Edit Extrusion, Finish and
Cancel Edit Mode, Create Extrusion's Depth while sketching) is F5's (below); its Visibility panel's Visibility Settings is F8's (below).

{3D} is drawn by the project's 3D view (`orbit.rs`, `editor/lit3d.rs`, `BimApp::family_3d`): the document owns an `Orbit`, whose
worker makes the scene and the card's mesh of a scratch model in which every solid form and every nested instance is placed as
a family of its own where the family is (`FamilyEditor::stage_3d`, staged again only when the draft, the type, Preview
Visibility, the Detail Level, Temporary Hide/Isolate or a void selected changes; a door's or a window's opening stays in its
wall with its solids hidden, its parts generic copies at the opening's frame, as the views' ghosts are). Its picture is the
card's or the processor's, as the project's is, and the card keeps the project's mesh parked beside it (`Gpu::hold_for`), so
neither is uploaded again when the other view shows. It is moved as the project's 3D view is: a primary drag orbits about the
point under the pointer, the middle button pans, Shift and the middle button orbit, the wheel zooms toward the point under the
cursor, the 3D mouse moves it, the triad (left of the ViewCube) and Revit's ViewCube turn it, and Zoom to Fit fits it; while
nothing has moved it, it fits itself again as its pane is resized. It shows what is selected as the project's 3D view does (T11):
a form, a void or a nested instance selected is tinted halfway in the selection's colour with its edges whole, through the
project's own tints, so what stands in front of it still hides it; a void, never seen, is drawn see-through while selected
(staged again when the selection changes). The form or nested instance under the pointer, picked in the picture on screen
(`Picker`, its element mapped back to its part), is lit in the hover colour, and a click selects it, a click on nothing letting
the selection go. A reference plane selected in a plan or an elevation, and Work Plane > Show, are laid over the picture as
marks depth tested as the project's work plane is (`view3d::PlaneMark`); a reference line selected, as a strip a few pixels wide.
The connectors' symbols, the sketch and its padlocks are drawn over it through the `Orbit`'s camera.

A family's connectors (#269) are elements of its document, as Revit's: Create > Connectors places a pipe, power, switch, data
or earthing one at the origin, selected; the browser lists them under Connectors, and a selected one's Properties type its name,
domain, flow, size, place and direction (numbers or formulas), and a device's poles, voltage, load, power factor and
classification. Each with a place is drawn over {3D}, the plans and the elevations as a symbol the same size on screen at any
zoom (a pipe's a circle, a power, switch or data connector's a square, an earthing one's the earthing sign) and an arrow the
way it faces (none where it faces the eye), in amber; the one under the pointer lit in the hover colour and a click selecting
it, before what lies behind it; the selected one in the selection's (`editor/connector_marks.rs`). It is an overlay drawn
through the view's projection alone, so it is not hidden by the forms in front of it.

Expression-valued form and symbol properties (their numbers, Visible and a form's Material) offer `=` (`editor/association.rs`),
which opens the Associate Family Parameter an adaptive element's rows open (`adaptive_editor::associate`, through the `Tied`
trait; `Field::Form` and `Field::Symbol` name the row). It lists family parameters of the property's kind and can create a
new compatible type or instance parameter, starting from what the row comes to. A bound field shows its evaluated value and
is read-only. Choosing `<None>` removes the association while retaining that evaluated value. Cancel leaves the property
unchanged; parameter creation stays in the family draft until the family is loaded.

A type of a category or of a family is renamed with `rename_type` (`Model::move_entry` moves it in its map, and `rename_entry` the
library entry and every reference `Model::lib_refs_mut` visits, so it keeps its GUID). A type or a family goes with `delete_type` (one entry, refused while an element uses it, and for the last type of a category or of a family: a
tool needs one to place) and `purge` (every entry of the kinds asked that nothing uses). Use is what `Model::lib_refs_mut` visits, the one place
that names every reference to a library entry: `Model::library_uses` counts them on a copy of the model, `Model::unused_entries` is one pass of
what purge takes out (each category keeps its default's type, else its first; a family that goes takes its types), the command repeats it
for what a pass frees (a family's material, a type's), at most 16 times, on a copy that becomes the model only when it isn't a `dry_run`.
`Model::remove_entry` is the single place that knows where each kind lives. The desktop's Purge Unused window (`purge.rs`) lists a dry run
for the kinds checked and runs the same; the Project Browser's Families lists the system types and has the right-click Delete.

Adaptive definitions, frame conventions, context snapshots and replay: [Adaptive families](adaptive-families.md).
The `family` command works out the whole change before the model is touched (`commands.rs::family`), so reading one costs no undo step
and a family `Model::check` would refuse never enters it. `family {name}` alone reads: the definition with every form's and symbol's `id`
and `GUID`, each type's `values` worked out, and `warnings` — no `model_mut()`, no file. Without `name` it lists the project's families
with their category and how much of each there is; `save` still writes one to a file. `dry_run: true` builds the change it's given
locally and replies `ok`, `warnings`, `changes` (parameters and types added, taken away and changed; forms and symbols replaced, patched
and deleted by id; the rename) instead of doing it; a refusal comes back as an error exactly as it would without it, carrying every error
and warning found, not only the first. One form or symbol is patched by its `id` or `guid` (`form`/`symbol`: its fields merged over what
that one holds, a `null` taking one away, its place, id and GUID kept; `delete_form`/`delete_symbol` to take it away), where `forms` and
`symbols` still replace whole lists — `keep_parts` binds by place, so patching is what leaves the other parts' ids alone. Validation is
`family/validate.rs` (`Family::problems`): every formula read at once (`names`, unknown parameters besides a hosted family's
`Wall Thickness`, syntax errors with the character they stop at), then `Family::check`, plus geometry warnings best worked out around
what doesn't build — a void reaching no solid before it, a form wholly outside its opening, one thicker than its wall, a door's or
window's plan with no symbols — each aggregated as `(in type …)`. `Family::shapes` gives each form's box before any void cuts it, from
the same `solid` `build` uses. `render {family, type, mode}` draws a family on its own: a `Session::new()` of its own holding the
project's materials and images, a wall of `wall_thickness` mm for a hosted family (a floor of its own footprint otherwise) with the type
in it, then `render_view` on that model — `3d`, `plan` (what its symbols draw) or `elevation` (front-on) — and the project keeps no
element and no undo step from it, the way the Family Editor's own preview already worked (`editor.rs::scratch`).


## Parameters

A family's `Parameter` (`family.rs`) is, as Revit's Parameter Properties describe one, a kind, a group, Type or Instance,
Family or Shared, and maybe reporting. Its `kind` is a length (mm), a number, an `integer` (a whole number, refused
otherwise; its formula rounds half away from zero), an angle, an `area` (mm²) or a `volume` (mm³), so a formula over
lengths in mm needs no conversion (`Width * Depth` is an area), schedules and `{label}` text showing them in m² and m³;
yes or no; or words: a material, `text`, a `url` (`MAX_URL`, one line) or a `familytype`. Words take no formula and no formula
reads them (`Kind::text`; `family/validate.rs` refuses the formula, a material still where it is worked out). A family type
parameter names the category of the families whose types it picks (`of`) and holds no value until T24 (a nested family's type chosen by it).
`group` is the heading it is listed under (`Group`, 14 of them); none, its kind's (`Parameter::group`: Dimensions for lengths,
areas, volumes and angles, Materials and Finishes for materials, else Other). `shared` is a shared parameter's GUID, the
same wherever it is used and distinct from the parameter's library-entry GUID (which is per project and per family): two
families' parameters of one shared GUID are one name and one kind, and one name shared is one GUID
(`Model::check_shared_parameters`); without it, it is the family's own. Schedules list every family parameter by name either
way, as they always have. A `reporting` parameter, a length or an angle, reads a dimension and never drives one: no formula,
type or component sets it (`Parameter::check_own`, `Family::check`, `Model::family_values_ok`): a dimension labelled with it
measures it (`Family::values_in` works its formulas out again over what it measures), and one no dimension is labelled with
holds its `value`, with a warning saying so. No label may be worked out from a reporting parameter a dimension measures. Discipline is implied by the kind (all Common), so there is no field.

`family {parameter: {name, rename?, …}}` is Edit Parameter: the fields merged over the one named (added if there's none), a
shared parameter's name and kind kept. `rename` goes through `Family::rename_parameter`, the one place that walks every part
of a family naming a parameter (formulas, forms' sizes, materials and `visible`, symbols' sizes and `{label}` text, connectors'
values: a new part that holds a formula adds its arm there), with `Library::renamed` keeping its GUID and the components'
values following. `instance` switched moves values, not loses them (`patch_parameter` in `commands.rs`): to instance, each
component takes its type's value (a type with nothing placed loses it, as Revit's does); to type, each type takes the value its
components agree on, refused naming the components where they don't. `delete_parameter` takes parameters away with their
values; whatever still names one is `Family::problems`'s to refuse. A file of 17 reads unchanged (every parameter the family's,
grouped by its kind); FORMAT 19 says an older build can't read the new kinds and fields.

### Formulas

A formula (`family::eval`, `names`) reads numbers, comparisons, `if`, `and`, `or`, `not` and the functions `FUNCTIONS` lists, over
the family's parameters by name (names hold spaces and are case sensitive). A number in it takes a unit right after it
(`UNITS`): `mm`, `cm`, `dm`, `m`, `mm²`, `cm²`, `m²`, `mm³`, `cm³`, `m³`, and `°` (or `º`) for degrees, each worked out into the
family's own (mm, mm², mm³, degrees), so `if(Swing Angle = 90°, …)` and `if(Width > 52.6 mm, 1.2 m, 300 mm)` read as Revit's do,
in a comparison or a result. A word after a number is a unit or refused, naming it and the units there are; no units are
checked against each other (Revit's "inconsistent units" is not asked), and imperial feet and inches are not read.
`renamed_in_formula` leaves a unit alone, so a parameter named `m` renamed keeps `2 m` as it was.

Every formula a family holds is listed in one place (`validate::every_formula`: its parameters', its forms' sizes, profile,
circle, axis and `visible`, its symbols' sizes, labels' place and height and `visible`, its room calculation point's, its
connectors' and its adaptive definition's, `adaptive::Definition::exprs`), and each name one says that isn't a parameter (nor a
door's or window's `Wall Thickness`) is refused, in every branch of its `if`s, not only those worked out: "the following is not
a valid parameter: X. Note that parameter names are case sensitive.", naming the parameter it differs from in case only
(`family::not_a_parameter`). A label's `{X}` and a room calculation point say it in the same words. Where it is refused: the
`family` command refuses any (`Family::problems`, so Edit Family and Load into Project too); every other command that is an undo
step, but `insert`, is refused when it leaves a family it made or changed with a name it didn't have before
(`family::check_family_edits`, run by the engine beside `check_edits`; a favorite loaded is such). `Family::check`, and so
`Model::check`, doesn't ask it: a file from before this check may hold one in a branch never taken, and it still opens, `open`
listing each as `warnings` and `family {name}` as `errors`, until a change to that family mends it. Refusing it on check would
have left such a file unopenable, and every command on it refused.

A parameter a formula works out (`Parameter::driven`) is set by nothing: a type that sets it is refused (`Family::check`), and
a component's instance value of it (`Model::family_values_ok`), the refusal naming the formula. Edit Parameter giving a
parameter a formula takes away what its types and its components held of it, as Revit's does. The desktop shows such a value,
greyed and taking no typing, as what it comes to: the Family Editor's Parameters and Types tabs (for the type previewed, or the
type picked), the project's Properties and Type Properties.

### Visibility

A form's `visible` is a yes/no parameter (`group: "visibility"` lists it under Visibility, as Revit's) or a formula; worked out
as zero, the form is left out of what the family builds for that type or component (`Family::solid`), so a plan, the 3D view,
a section and IFC leave it out alike, and a change of type, of the type's tick or of the component's own value follows. The
family document's Properties (a selected form's, or a symbol's) show it as Revit's do: a Visible tick with "=" at its end (Associate Family Parameter,
the same dialog an adaptive element's rows open, through `adaptive_editor::Tied`), which ties it to a yes/no parameter or, with
<None>, unties it keeping what it came to; tied, or given a formula, the tick is greyed, showing what the type previewed comes
to, the formula beside it. As Revit's family editor, the document's preview draws a solid form the type previewed hides ghosted
(grey and see-through: a material of the preview's scratch project, `editor.rs::ghosted`), so it can still be picked and edited;
the view control bar's Preview Visibility shows the family as placed, the form left out, with an orange frame round the view and
its name over it. The camera is fitted to every form either way, so the toggle doesn't move it; a change of type redraws both. The
toggle is the document's (`document::State`), off when a family opens, and no part of the draft: not saved, no unsaved change, no
step of its history. A void hidden in a type cuts nothing in either.

Temporary Hide/Isolate works in the family document's view as in the project's (`editor/temp_hide.rs`): Hide Element (HH), Hide
Category (HC), Isolate Element (HI), Isolate Category (IC) and Reset (HR), from the keys (or what Keyboard Shortcuts made them) or
the view control bar's Temporary Hide/Isolate menu (Apply Hide/Isolate to View greyed: a family's views keep nothing hidden for
good yet). It acts on what is selected: the form selected in a family that isn't adaptive, the points, lines and forms selected in
the modelling view; the selection goes. A family has no categories of elements, so the view's own stand for them, as Revit's
family editor names them: Forms (solid and void alike: forms have no subcategory), Reference Points, Lines, Reference Planes (the
datums and the family's planes the modelling view draws) and, in a door's or a window's preview, the wall it stands in. Only forms,
points and lines are selected, so they are what HC hides; IC hides every other category the view draws, and HI the reference
planes or the wall with the other elements. The preview leaves a hidden solid form out of what it draws (`temp_hide::without`; a
hidden void still cuts, being never drawn) and hides the wall by its category; the modelling view neither draws nor picks a
hidden element (`adaptive_view::Hidden`), the datums staying placed round everything. A form is known by a `tag` the document
gives it, so a hidden one stays hidden as forms move in the list. A hidden form stays in the browser's Forms. What is hidden is the
document's (`document::State::temp`), as Preview Visibility: not saved, no step of the history, no unsaved change, gone when the
family closes. While it hides anything the view wears the project's cyan frame (`footer::FRAME`) and the bar says
"Temporary Hide/Isolate (HR resets)"; Preview Visibility's orange frame is then drawn just inside it, so both show.

### Visibility Settings (F8)

F8, part of FORMAT 19 (`family/visibility.rs`). Revit's Family Element Visibility Settings: a form's, a void's and a nested instance's
`visibility` is `{plan, front_back, left_right, coarse, medium, fine, only_when_cut}`, each true by default but `only_when_cut`,
and a file or a command writes only what differs (`{"plan": false}`); a symbol's takes only `coarse`, `medium` and `fine`
(`Visibility::check_symbol`, refused in `Family::check` naming the symbol), as Revit's symbolic lines do. They are drawing
only: `Family::pieces` still builds every form `visible` leaves in, so IFC, schedules, bounds and snapping are untouched, and a
form is drawn where both `visible` and its settings allow it. A void cuts whatever its settings say (they are kept, and change
nothing: cutting per view would change the solid per view and its IFC). A nested instance's settings AND with each of its
family's forms' and symbols' (`Visibility::both`, `nested.rs`); a nested piece's view direction is read against the host's axes.

Each built form carries its settings as `Piece::vis`. What a view reads, by `Look::forms()` (`FormView`: how it looks, set by
`Look::of`, and its detail level, by `Look::with`):

- **Plans and ceiling plans** read `plan`, cut or not (Revit applies Plan/RCP only to a family the plan cuts; BIMCraft draws a
  family's symbols in its level's plans whatever height its forms reach, so applying it always keeps a hidden leaf from
  doubling its symbolic swing), and the view's detail level; `only_when_cut` draws a form only where the cut plane passes
  through it (`render::plan_draws`; the family's own plan, `family_plan`, the same). A door's or a window's forms with no
  settings keep the old rule: left out of a plan at Coarse with its symbols.
- **Sections and elevations** read `front_back` where they look nearer the component's y axis (its family's Front and Back
  elevations' way; a tie counts as Front/Back) and `left_right` nearer its x (`visibility::class`), and their detail level;
  the projection never draws `only_when_cut`, the poché where the line cuts does (`render::section_base` asks
  `look.forms_show(vis, true)`).
- **3D views** read only their detail level, whichever way they look, as Revit's 3D always shows the form; never
  `only_when_cut`. A 3D view has a Detail Level now: the `detail` command takes a 3D view or `{3D}` (made when its level first
  changes, as `visual_style` makes it), and the 3D view's control bar and its Properties (Graphics) have it. It changes nothing else in 3D yet.

The 3D scene keeps a form's settings packed on each of its triangles (`Tri::vis`, `Solid::Mesh`'s `vis`:
`visibility::packed`, the hide flags and the component's x axis in plan to a half turn; 0 for anything without settings), and
every picture, pick, wireframe and the card's mesh (`view3d/gpu.rs`) leaves out those `Look::forms_show` refuses. A copy whose
triangles carry settings is never instanced on the card (`placements`), so the card's per-triangle filter sees it. Symbols:
`Family::marks_in(values, detail)` (`Model::component_marks_in`, `Model::opening_marks_in`) gives those drawn at a detail level,
a symbol with no settings at Medium and Fine (`SYMBOL_DEFAULT`, the old "Coarse leaves them out"); so a door authored as
Revit's, its leaf `{plan: false}` and its swing a symbol, shows the swing in plan where the leaf's outline was.

In the family document (`editor/visibility.rs`), Family Element Visibility Settings opens from a form's or a nested instance's
contextual tab (Visibility > Visibility Settings), from a form's Properties (Graphics > Visibility/Graphics Overrides, Edit…)
and from a selected symbol's Properties (Detail Levels, Edit…; only its detail levels can be changed):
View Specific Display (Plan/RCP, Front/Back, Left/Right, Only shown when cut), Detail Levels (Coarse, Medium, Fine), Default,
OK (into the draft, a step of its history) and Cancel. The document's view control bar has a Detail Level menu (`Views::detail`,
the session's, Fine when a family opens). Its views draw a form its settings leave out of that view (`views::hidden_in`)
**halftone**, as Revit's family views grey it, so it can still be seen and picked; Preview Visibility on leaves it out, as the
project would. The plans and elevations take it into the ghost component of forms the type hides (`views.rs::ghosts`), which a
door's or a window's now has too: a generic copy of the family, its wall's thickness a parameter, placed at the opening's
frame. {3D} ghosts the forms its Detail Level leaves out in the ghost material. Model lines and model text in families, and
symbolic lines drawn in elevations, are not here.

## Shared parameters file

A shared parameters file (`shared_parameters.rs`, the `shared_parameters` command, Manage > Shared Parameters) is a plain-text
file of the user's own, outside any project or family, as Revit keeps one: groups, and in them parameters, each a GUID, a name,
a data type, a description, whether it is visible and whether the user may change it. The command takes the file's `path` on
every call (Revit keeps it per user, never in the project; an agent says which file it means, and a front end remembers its
own), and only `load` changes the project. The layout is Revit's tab-separated one, from what Autodesk documents publicly
in the Revit API guide's [Definition File](https://help.autodesk.com/cloudhelp/2015/ENU/Revit-API/files/GUID-9C7F6E02-05C2-4F72-8108-60F2175FB9E3.htm) page and what such files commonly hold: `#` comments, a `*META`/`*GROUP`/`*PARAM` header naming the columns of the `META`/`GROUP`/`PARAM` lines after
it, read by column name so version 1 (no `DESCRIPTION`, `USERMODIFIABLE`) and version 2 (with them and `HIDEWHENNOVALUE`) both
read, and a column this version doesn't know passed over; a reader must be at least `MINVERSION`, so a file asking for more
than 2 is refused. It is written as version 2, UTF-8 with a byte order mark and Windows line ends, groups and their
parameters in order and our own comment line; it reads UTF-8, UTF-16 either way (by its mark, or by a first character with
a zero byte) and, failing UTF-8, Latin-1. The file is hostile: at most 4 MiB (`MAX_FILE`), 100,000 lines, 64 fields a line,
names of `MAX_TEXT`, descriptions of 1024 characters, no control characters, whole numbers in range, flags 1 or 0, GUIDs that
parse (never an id, never nil), one GUID a parameter, one id and one name a group, every parameter in a group it has; each
refusal names its line, and a file that doesn't read is never written over.

`DATATYPE` maps to a family parameter's kind (`kind_of`): `TEXT`, `INTEGER`, `NUMBER`, `LENGTH`, `AREA`, `VOLUME`, `ANGLE`,
`YESNO`, `URL`, `MATERIAL` and `FAMILYTYPE`, and the newer spec ids for the same (`autodesk.spec.aec:length-2.0.0`, the
version left out). A family type's `DATACATEGORY` is Revit's built-in category number (`-2000080` furniture), known for the
categories BIMCraft has an equivalent of (`CATEGORY_IDS`); one it doesn't know reads as a family type with no category, which
`load` takes `of` for, and a family type of a category with no number (a rack) can't be written. A type BIMCraft has no kind
for (`CURRENCY`, the HVAC ones) is listed with `kind: null` and its `line`, kept as written when the file is written again,
and refused, naming its line, only when it is loaded into a family: a real file holds many such, and refusing the whole
file would make it useless.

`load` adds one of the file's parameters (by `name`, or `guid` where the name is there twice) to a family through the
`family` command's Edit Parameter: `shared` its GUID, name and kind the file's (which the family then keeps from changing),
`instance` and `under` (the heading) the caller's. It refuses, before `Model::check` would, saying which family holds what:
a family with a parameter of that name or that GUID already; the project sharing the GUID under another name or kind; the
project sharing the name as another GUID. `export` writes a family's (or every family's) shared parameters to the file,
creating it, new ones in `group` (by default the family's name): an entry of that GUID is kept as the file has it, its
description too, and one the file holds as another name or kind, or the name as another GUID, is refused naming its line.

The description, visibility and user-modifiable flag stay in the file: a family's `Parameter` has no field for them, since
nothing in BIMCraft reads them yet (no tooltip, no hidden parameters, no read-only values), so no `FORMAT` step was needed.
An export of a parameter the file doesn't have writes it visible, modifiable and with no description. When BIMCraft enforces
user-modifiable values, the flag becomes a `Parameter` field and a FORMAT of its own.

## Reference planes

A family's `planes` (`family/planes.rs`) are Revit's reference planes, each a `RefPlane`: the line it was drawn as (`start`,
`end`, in the family's axes) and `up`, its other direction, so its normal is `(end - start) × up`; its own axes are x along the
line, y `up` made square to it and z the normal, its origin the point of it nearest the family's origin (a profile on a plane
parallel to a datum sits where it would on the datum, moved along the normal). Every family holds the three datums first,
with the same GUIDs in every family (`REF_LEVEL`, `CENTER_FRONT_BACK`, `CENTER_LEFT_RIGHT`): Ref. Level (the family's xy,
normal +z, a `level`, its reference `bottom`), Center (Front/Back) (xz: x along +x, y up, normal -y; `center_front_back`) and
Center (Left/Right) (yz: x along +y, y up, normal +x; `center_left_right`). They are pinned: a file that leaves them out or
moves them gets them as they are (`planes::with_datums`, `Family::settle_planes`), and the command refuses moving, changing
or deleting them. Being in every family, they are no parts of the library and have no id; a plane of the family's own is a
part (`HasPart`, `parts_now`), with an id and a GUID. Each has a `reference` (T1; Revit's Is Reference: `not_a_reference`,
`strong`, `weak` by default, and `left`, `center_left_right`, `right`, `front`, `center_front_back`, `back`, `bottom`,
`center_elevation`, `top`, each of these one plane's or line's in the family), a `subcategory` and `defines_origin`.
`Family::check_planes` says what's wrong: a name used twice, a named reference taken twice, numbers past `MAX_COORD`, a line
with one point, an `up` along the line, the origin defined twice in a direction, or by a plane square to none of x, y and z.
The datums define the origin until a plane of the family's own in their direction takes it; moving the origin there (and
correcting placed components' `at` so they stay put) is not done yet: the flag is kept and checked only.

A family's `lines` are its reference lines (T18), each `start` to `end` drawn on a `host` it lies on, with four planes: its
host's through it (`horizontal`), square to that through it (`vertical`), and square to it at each end (`start`, `end`), their
origin the line's start (`end`'s its end), so what is drawn on one turns with it when its constraints turn it (F7c, below).
A line may be drawn on an earlier line's plane, never a later one's.

A form's `host` is a `PlaneRef`, `{"plane": guid}` or `{"line": guid, "face": …}`, Ref. Level when none: its profile is
drawn in the host's axes and `from` and `to` run along its normal (Revit's Extrusion Start and End); a revolve turns in it
(`Family::frame`, `sweep`, `revolve`, whose triangles turn round where a frame is mirrored). `plane` (`xy`, `xz`, `yz`) is the
shorthand from before (`family::Plane`), read as it always was by the command and by a file of 17 (`FormIn`): `xy` and `yz`
are Ref. Level and Center (Left/Right) as they were; `xz` swept along +y and Center (Front/Back)'s normal is -y, so its
`from` and `to` are negated and swap places (a formula wrapped as `-(…)`), a revolve's angles kept. The starter families
still say `plane`, and build the same triangles (`family_plane_tests`); FORMAT 19 holds this change. The `family` command
replaces the lists with `ref_planes` and `ref_lines` (datums given back as they are are kept), patches one with `ref_plane`
and `ref_line` and deletes with `delete_ref_plane` and `delete_ref_line` by id or GUID, as forms; deleting one a form or a
line is drawn on is refused, naming them. A new plane or line may carry a GUID of its own, so forms are drawn on it in the
same command; a host given as an id is resolved to that plane's GUID (`Family::resolve_hosts`). A plane or line whose GUID
the sync gives anew (a family loaded twice) takes what is drawn on it along (`Family::rehost`). The Family Editor keeps the
planes, lines and hosts as they are; its Plane list picks a datum, and its plans and elevations draw, select and move
planes (*The family's views*, below). A connector has no place yet: when it gets `at` and
`direction` (upstream #269) it takes an optional host too. IFC is unchanged: it writes the forms' triangles.

An adaptive family's work planes are these planes too: a reference point is drawn on a datum, a named plane of the family's own
or a reference line's face, and keeps it as its Work Plane by `PlaneRef`; the three datums an adaptive definition had of its own
before are the datum planes, read from an older file by name ([adaptive families](adaptive-families.md)). `Family::hosted_on`,
`rehost` and `resolve_hosts` follow its points as they follow forms and lines.

## Dimensions and constraints

F7a (`family/constraints.rs`). A family's `dimensions` are Revit's family dimensions, each a part (id and GUID, `HasPart`): an
aligned dimension between two or more parallel references, `{"plane": guid}` (a datum or one of its own) or `{"form": guid,
"end": "start" | "end"}` (an extrusion's Extrusion Start or End face, its host a plane), in order along the way it measures,
drawn in one of its views (`view`: plan, ceiling, front, back, left, right; the references must be seen edge on there) with its
line through `at`. `label` (a dimension of one segment, a length parameter) makes that segment the parameter's value in every
type; `locked` (Revit's padlock) keeps a segment at a length (a labelled one's lock is Family Types' Lock: it stops a drag
from changing the parameter); `equal` keeps every segment one length (EQ); `alignment` is the lock Align leaves, two references,
one segment locked at 0, drawn as a padlock and never printed. `Family::check_dimensions` refuses what can't be (a reference
it hasn't, references not parallel or not seen edge on, a label that isn't a length or is on a string, a lock past the last
segment), and caps them (`MAX_DIMENSIONS`, `MAX_DIM_REFS`).

Flexing a type solves them (`Family::solve`, `Family::flexed`): the unknowns are the offsets along their own normals of the
planes a constraint measures to (a datum or a plane that defines the origin stays) and the extrusion ends one measures to;
each label, lock, EQ and Align lock is a linear row with coefficients that never depend on the values; and the solution is
the least change from where they are drawn that holds every row (an extrusion's end gives way first, `END_GIVES`), each
connected group of unknowns on its own, by modified Gram-Schmidt in a fixed order, so it is deterministic. A row that adds
nothing to those before it is refused for every type as over-constrained, naming the dimensions it repeats; a family accepted
solves for any values but a labelled length of 0 or less (refused for its type, "Constraints are not satisfied"). The work is
capped (`Meter`, shared by the types of one check). Nothing solved is stored: `pieces_at`, `shapes`, the voids and the check
build `flexed`, a copy with the planes moved (the reference lines and forms drawn on them with them) and the ends the
constraints set given as numbers, so a type's geometry depends on its values only, never on the type flexed before it, and a
family with no constraint is built as it was, borrowed. `Family::drag_plane` is a plane dragged in a view, from where the type
has it: a labelled dimension measuring to it (unlocked, its parameter no formula's) takes the new length; else the constraints
move what they hold with it, or refuse ("Constraints are not satisfied", the dimensions that hold it); `remove_constraints` is
Remove Constraints. `constraints::align` is Align. Deleting a plane or a form takes it out of the dimensions that measure to
it (`forget_reference`; one left with a single reference goes). The `family` command takes `dimensions`, `dimension` (added,
or patched by id or GUID) and `delete_dimension`, and reads a type's solve with `solve`. A dimension measuring to what the
family hasn't is refused, unless `invalid_dimensions: "delete"` (T14's Delete Dimension(s)); a file opened leaves it out and
lists it in `left_out` (`Model::read`). Part of FORMAT 19.

In the family document (`editor/dimensions.rs`), Create > Dimension > Aligned Dimension (DI) picks reference planes (by the
view's drawing) and extrusion ends seen edge on (`constraints::end_faces`), parallel to the first, and a click on nothing
places the string (its line through the click); a dimension being placed follows the pointer. The marks are the core's
(`constraints::marks`), painted over the view's picture: its line, ticks, each segment's length (or EQ, or `Label = length`),
read up the page. A click on one selects it ("Modify | Dimensions"): a padlock under each segment locks it at its length or
unlocks it, the EQ toggle over a string turns equality on or off, and the Options Bar's Label gives it `<None>`, a length
parameter or Create Parameter… (Parameter Properties: name, Type or Instance, reporting). A label, lock or EQ that adds
nothing to the constraints is refused, "The dimension is over-constrained" (T19). Delete takes it away. Align (AL, Modify)
picks a reference then what moves onto it (a plane, or an extrusion's end); a padlock waits beside it, and a click on it locks
them; an Align lock's padlock unlocks it when clicked. A plane its dimensions hold is dragged through them (`drag_plane`); a
refusal, like Family Types' values a type can't hold, asks "Constraints are not satisfied" with Remove Constraints (their
labels, locks and EQ taken off, and what was being done done) or Cancel. An extrusion end a dimension sets is greyed in its
Properties and the Options Bar's Depth. A reload whose family has dimensions measuring to nothing (`exists.rs`) asks Invalid
Dimensions first: Delete Dimension(s) or Cancel, and nothing passes it over. Angular dimensions and reference lines that turn
are F7c's (below).

F7b, part of FORMAT 19: sketch lines locked. A dimension may also measure to a straight line of a form's sketch, `{"form": guid,
"line": guid}` (the plane through it square to the form's plane: Revit's sketch line seen edge on), to a line's end or a
circle's centre, `{"form", "line", "point": "start" | "end" | "centre"}` (measured along the dimension's other references, so
one of them must face a way), and to a reference line's face, `{"line": guid, "face": …}` (fixed but for moving with the plane
it is drawn on). A command may name the form and the line by their ids. `kind: "radial" | "diameter"` measures one circle of
a sketch seen flat in the view, with a label or a lock. Flexing adds the sketches' vertices as unknowns (the ends of lines that
meet within `TOL` one vertex, an axis line's and a circle's centre their own; `Layout`), each circle's radius, and the host
plane's offset where a point is measured off it; a line's place is its start's, and a soft row keeps it parallel, left out
where the other rows hold it already (two ends held never read as over-constrained). Vertices and radii give way like
extrusion ends, so lines follow planes. `moved` writes the solved points and radii as numbers in the flexed copy; a coordinate no
row touches keeps its formula. A line's direction is the type's, so `check` checks each type's (`check_dimensions_for`). A form
given again (`forms`, `form`) keeps the dimensions of the lines that keep their GUIDs, and those of the lines gone go
(`forget_lines_gone`, `deleted_dimensions`); copies follow the sketch lines' new GUIDs. An arc follows its ends unless its
radius or centre is measured (F7c). `solve` lists each form's sketch for the type. The conservative bounds of a
constrained family are its flexed one's.

In the family document, Aligned Dimension and Align also pick a form's straight sketch line where the view sees it edge on (any
line of a sketch on a plane seen flat; an extrusion's side where its line runs the way the view looks, as Revit's extrusion
face), a circle's centre and a reference line (the face the view sees edge on): `constraints::sketch_faces`, `sketch_circles`,
`line_faces`. Create > Dimension's Radial and Diameter pick a circle by its edge, then where the dimension goes; its mark is a
leader with an arrow and `R`/`⌀` (or `Label = length`) level beyond it. Align on a finished form's line moves it, the lines
joined to it stretching, through the constraints (refused where they hold it), and leaves Align's padlock. While a form is
sketched (Modify > Align or AL in a plan or an elevation), Align takes a plane, a reference line or another line, then a line of
the sketch, moved with `align::sketch_lines`; its open padlock locks them, and the padlocks of the form's locks show beside their
lines, a click unlocking one. They are written at Finish (a refusal by the constraints keeps sketch mode open) and dropped by
Cancel. An Align padlock, on a finished form and while sketching, sits beside the line it locks, a few pixels off it to the side away from the
form's inside, slid along it as little as keeps it off every line drawn and every other padlock (`dimensions::padlock_spot`);
Align's offered padlock too. Edit Extrusion opens the sketch as the type previewed flexes it, and a line that comes back there keeps its formulas
(`sketch::kept` against the lines shown). Every line gets a GUID of its own at Finish, a new form its GUID at once, so a lock or
a dimension can name them before the family is loaded.

F7c, part of FORMAT 19: angles, turning reference lines and arcs. `kind: "angular"` measures the angles
between 2 to `MAX_DIM_REFS` lines seen as lines in its view: reference planes seen edge on (their traces), reference lines
(`{"line": guid, "face": …}`, any face: the line's way counts) and straight sketch lines on a plane seen flat. Segment *i* is the
angle from reference *i* to *i + 1*, in degrees; the sector is the one `at` lies in as drawn (each line's half on `at`'s side of
where the first two cross, `ray_signs`), kept for every type, so the angle reads continuously as the lines turn. A label is an
Angle parameter (0 ≤ value < `MAX_ANGLE`, 180), a lock keeps the angle (`Lock.length` in degrees), EQ keeps a fan's angles
equal (three lines or more); no Align lock. Neighbours parallel as drawn are refused. A dimension may measure to a reference
line's end, `{"line": guid, "point": "start" | "end"}`, along its other references (the rig's start is held where two planes
cross by two Align locks to it); radial and diameter dimensions take an arc, and `{"form", "line", "point": "centre"}` an arc's
centre; and forms drawn on a reference line's face are measured like any other (F7b's refusal is gone). A reference line a
constraint measures becomes four unknowns in the axes of what it is drawn on: its start, its angle and its length (a line not
measured follows what it is drawn on, rigidly, as forms and lines on its faces follow it); an arc whose radius or centre is
measured, its turn beside its end vertices. Lines give way like planes; vertices, ends and turns before them. An aligned
dimension to a turning line's face measures the face through its start and adds a soft row keeping the line's angle, left out
where an angle sets it.

The solve (`solve_with`) writes every row as its linearisation at the point reached (`Ad`, forward-mode derivatives, as a `Lin`),
so a linear row is the row itself. With no line or arc unknown and no angle between sketch lines it is F7a's single least-norm
solve, untouched. Otherwise it is Gauss-Newton on the least change: each step holds the rows linearised where it has got to
with the least weighted change from the drawn point (`solve_rows(x₀, …, rows(x_k))`), halved back (at most `MAX_HALVINGS`)
until the residual falls, and goes on until the rows hold and the step comes to nothing, at most `MAX_STEPS`; what is drawn
(each segment's way, each angle's sector) is read once at the drawn point (`Memo`). Over-constraint is the rank test on the first
step, at the drawn point; a step away from it leaves a row that lost rank to the residual. What doesn't settle, or would give
a line no length, an arc a straight or whole turn, says "Constraints are not satisfied: they can't all hold for these values",
naming the dimensions. Deterministic: fixed orders, step and halving counts, no hashing, every type from the drawn point.
`moved` puts each solved line in its host's flexed axes (to 1e-9 mm) and carries lines drawn on a moved or turned line with
it; `edits` gives a drag or Align `LineEnds` and `Turn`; `measured` reads angles in degrees (reporting Angle parameters
report); `solve` lists the lines. `marks` takes the drawn family too, for the sector.

In the family document, Create > Dimension > Angular picks lines that cross the one before (`dimensions::line_at`,
`crosses_last`, through `constraints::view_way`), then a click on nothing places the arc, following the pointer until then. Its
mark is the core's: an arc between each two lines at `at`'s distance from where they cross, arrows at its ends, a short witness
across it on each line, each angle level just outside the middle of its arc; selected, a padlock inside each arc and the EQ
toggle past a string's first end. The Options Bar's Label lists Angle parameters for it, and Create Parameter… makes an Angle
of what it measures. Radial and Diameter also pick an arc by its edge within its sweep (`constraints::Round`), and an arc's
centre is picked as a circle's. Turning a reference line with the mouse is F7d's pivot and its rotate grip (below), with
T18's line grips.

F7d, part of FORMAT 19: pivots (`family/constraints/pivot.rs`). A reference line or a reference plane
may carry a `pivot` (`Pivot`), one each: it keeps a point of its element (`at`: `start`, `end` or `{"along": mm}`; a plane's
along the line it is drawn as) on a point (`to`, `PivotPoint`: where two planes holding the pivot's axis cross, a reference
line's end, a point of a sketch, or a fixed point), and turns about it; the axis is the normal of what a line is drawn on
(a line on Center (Front/Back) is a horizontal hinge, a sash) or a plane's `up`. Its swing, counter-clockwise about the axis
from `from` (the element as drawn, a reference plane's trace, a reference line), is the Angle parameter `parameter` (type or
instance, a formula's or not, never a reporting one), else its own `angle`; `min` and `max` bound it (±360 without them).
Revit's door swing is a line, two Align locks, an angular dimension and a label; here it is a line and a pivot with a Swing
Angle, and limits Revit hasn't. In the solve (`Problem::pivot_rows`) a pivoted line is F7c's four unknowns and its pivot two
rows (the point at `at`, the drawn distance along it so a turn keeps its length, on the pivot point, across the axis) and
a swing row; a pivoted plane is one unknown, its turn (`Unknown::PlaneTurn`), its axes the drawn ones turned about the axis
through `at` and carried across the axis onto the pivot point (`Problem::plane_frame_ad`), so forms, lines and nested
instances drawn on it turn with it rigidly, as the flexed copy keeps (`RefPlane::turned`, never in a file). The pivot point
is read where the solve has got to (a label that moves a crossing's plane moves the hinge) and never moved by the pivot. The
swing row is hard for a parameter and soft for the pivot's own angle, so an angular dimension may set a pivoted element's
angle instead (F7c's angular dimensions work on it unchanged). An aligned dimension to a turning plane measures each
reference's distance from it along its normal as it turns (`turned_stations`), a sketch line in it kept parallel to it, so a
form Align-locked to a pivoted plane follows it. Rows name their pivot, so over-constraint says "reference line 1's pivot is
over-constrained: dimension 1, 2 hold what it holds already" (its start Align-locked to the crossing besides), and a pivot's
parameter beside an angle label on the same line is refused the same way. A flexed copy solved again (drags, Align, marks)
reads "as drawn" through `Pivot::turned`. `Family::check_pivots` refuses a pivot on a datum or a plane that defines the
origin, on itself, on planes that don't hold its axis or never cross, limits past ±360 or past one another, its own angle
past them, a parameter that isn't an Angle, and a cycle (a pivot reading, through its point, its reference or its line's
host, what turns with it); `check_pivot_values` a type's value past the limits and `Model::check` a component's ("`Swing
Angle` is 120°; reference line 1's pivot turns from 0° to 90°"). Deleting a plane, a line, a form or a sketch line a pivot
names takes the pivot off (`forget_pivots`); a file's pivot naming what the family hasn't is left out (`dangling_pivots`).
`drag_pivot` is the rotate grip: toward a point it gives the swing (whole degrees, within 5° of 0, a limit or a multiple of
45° snapped to it, stopped at the limits) as `Edit::Parameter`, or as `Edit::PivotAngle` with no parameter; a formula's
parameter refuses. A pivoted plane can't be dragged square to itself. `drag_line_end` is T18's end grip. `pivot_marks` and
`pivot_targets` are what a view draws and snaps to. The `family` command patches `pivot` through `ref_line` and `ref_plane`
(`null` takes it off) and `solve` lists each plane's ends. FORMAT 19: a build that reads up to 18 would leave `pivot` out of the file.

In the family document (`editor/pivot.rs`), a click on a reference line selects it ("Modify | Reference Lines", Delete,
over a plane's trace beneath it). Rotation > Pivot, on a selected line or plane of the family's own, waits for a click: the
point within reach where two planes' traces cross, a line's end or a sketch's point, else the click's own point; the element's
start or end within reach, else the point along it, is what it holds. Every pivot draws a small circle with a cross; a limited
one its swing range as a thin dashed arc, always (it is the door's swing symbol, worth seeing unselected; an unlimited one
draws none, a whole circle being noise); the selected element its rotate grip at its far end with an arrow, its swing beside
it while it is dragged. Properties' Constraints show the Pivot Point, the Swing Angle with "=" (Associate Family Parameter,
`Field::Pivot`; untied, the pivot keeps the angle it came to), Minimum Angle and Maximum Angle (blank: none), each refused
with the core's words; Rotation > Remove Pivot takes it off. An unpivoted line selected shows a square grip at each end.
Out of scope: a pivoted plane dragged square to itself, the grip in {3D}, a reporting parameter reading a swing (F7e).

## The family's views

F4: the family document's Project Browser opens the Ref. Level floor and ceiling plans and the four elevations, Back, Front, Left
and Right, beside {3D}, and the view's tab and the window's title name the one shown (`Floor Plan: Ref. Level`, `Elevation:
Front`). Each is the project's own drawing of a scratch model (`family/views.rs`, `stage`), as `render {family}` and the 3D
preview make theirs: the family placed at the origin (a door or a window in its wall, a window's sill on Ref. Level, since its
axes rise from the sill, F10), the first level renamed Ref. Level and
the others taken away, each of the family's planes but Ref. Level a project reference plane there, across what the views frame
(named as it is, mapped back to the family's plane by GUID in `Staged::planes`), and four elevations outside it, Front looking
north, so its right is east, Back south, Left east and Right west, as Revit's. The mapping lives in the scratch model alone:
the project gets no element and no step from a view, and the family only the planes drawn and moved in one. A view's scale is
the one at which, fitted, its annotation and pens are their size on paper, so names read as on a sheet and grow when it is
zoomed (the wheel; the middle button moves it). The plan is `render::draw` with, over it, each component's forms seen from above
at any height and, where a form crosses the level's cut plane (1200), its outline there in the cut pen (`render::family_plan`,
`plan_cut`, the section's slicing turned on its side); a project's plans never cut a component. The ceiling plan is the project's.
An elevation is a section's picture and drawing (`section_picture`, `section_over`): forms seen, Ref. Level a level with its name.
The planes are drawn with the rules in [views and graphics](views-and-graphics.md#reference-planes-in-views). The family's reference
lines (`Family.lines`) are drawn over each plan and elevation as the view sees them, solid in the planes' green and the thinnest
pen, as Revit's: true length where their plane is square to the view, foreshortened where it isn't, nothing for one seen end on
(`render::reference_lines`). They are owned by nothing in the drawing; a click selects one where a view draws it (`Staged::line_at`, F7d). Views are drawn on
the interface's thread, being one family, and kept until what they show changes (`editor/views.rs`).

In a family that isn't adaptive, a click selects a form (`views::form_at`: the one nearest the eye there, highest in plan; its
Properties and contextual tab as T11's) or a plane (Properties: its name, read-only for a datum; "Modify | Reference Planes",
whose Delete takes away a plane of the family's own unless something is drawn on it). Planes may lie on one another (F10c: a
window's Sill on Ref. Level, as Revit's window template has it): each is drawn, and where the pointer is over several
(`Views::under`, each once, nearest first) the one a click takes is lit and Tab steps to the next, as Revit's Tab does, for
selecting, Aligned Dimension, Align, Pick Lines and the Work Plane dialog's Pick alike; Ref. Level is picked by its level. The
app's Tab hook leaves Tab to the family document while it is open. The blank window (File > New > Family… > Window) has Sill: a
plane on Ref. Level seen edge on in the elevations, Align-locked to it, so a Height dimensioned from it moves its head alone (a
window's axes rise from its sill, so the sill's plane and Ref. Level are one; without the lock the least-change solve would
share a Height's change between them); a door's blank has none, as Revit's door template. The selected form is outlined, the
selected plane's trace drawn heavier. A plane of the family's own, selected, moves square to itself by a drag; a datum doesn't.
Reference Plane (RP, the Create tab's Datum) draws one by two clicks: in plan upright (`up` the vertical), in an elevation
`up` the way it looks, so its normal lies in the elevation's plane (`views::new_plane`); it has no name, is Weak, and is a step
of the document's history (`Draft` keeps the planes). Work Plane on the Create tab: each view has its own work plane for the
session (Ref. Level in plan, the Center plane an elevation faces), which Set sets through Revit's Work Plane dialog (T12, below); Show
tints it, all over where the view looks straight at it, along its trace where across it. {3D} has a work plane of its own too (Ref.
Level until Set, by name alone): Show draws it
into its picture over the patch the project's 3D view lays its work plane on (`workplane::patch`), what the family takes up and
300 mm round: tinted as the plans tint it, with its outline and a grid through the family's origin whose step leaves about 5 to 20
lines across it (10, 50, 100, 500 or 1000 mm, `workplane::grid_step`), depth tested against the picture
(`view3d::Image::draw_marks`, through `workplane::plane_mark`, as the project's 3D view draws its own), so a form in front of the
plane hides it and one behind it is seen through its tint.
An adaptive family's modelling view keeps its own Work Plane panel and Show. An elevation is framed on what it shows as a project's
is (`render::section_extent`), so its levels' names past their lines' ends aren't cut at the picture's edge.

The Work Plane dialog (T12, `editor/work_plane.rs`), as Revit's: Current Work Plane names the plane as `Planes::listed` does, with Show
(the ribbon's toggle) and Dissociate, greyed (a family's work plane is always one of its planes or lines, which forms are drawn on).
Specify a new Work Plane: Name, the family's planes as `Planes::listed` lists them; Pick a plane, a reference plane's or a datum's
trace; Pick a line and use the work plane it was sketched in, a reference line as the view draws it (`Staged::line_at`), whose host is
taken. The two Picks need a plan or an elevation, where planes and lines are drawn; in {3D} they are greyed. OK with a Pick closes the
dialog and the next click picks (the status bar says what to click; a click on nothing waits on); Escape brings the dialog back. The
view's work plane is set where the view shows the plane flat (`Which::faces`; {3D} takes any); else Go To View asks, listing the views
that do (`views::flat_views`: the two plans for a level plane, Front and Back or Left and Right for an upright one square to them) and
{3D}; Open View (or a double click) shows the one chosen with the plane set there, Cancel sets nothing. So a trace picked in a plan or
an elevation, always edge-on there, goes through Go To View, as in Revit. From a form's Edit Work Plane the dialog runs
`edit_work_plane` through the draft, as before, and the form moves with its plane; nothing is asked. While Preview Visibility is off, the solid forms the type hides are a second component of a
family of their own in the scratch model, drawn halftone, as {3D} ghosts them (not for a door or a window, whose wall holds one
opening); on, they are left out. Temporary Hide/Isolate leaves a form hidden for now out of every view; a selected plane can be
hidden (HH), isolated or hidden with every plane (HC). It keeps **one state for the whole family document**, every view showing
what it hides, a deliberate limit: Revit keeps it per view, and Apply Hide/Isolate to View stays greyed. An adaptive family's
plans and elevations show its test placement and nothing more (no selection, no Reference Plane, no drag); its tools stay in
{3D}, its modelling view. None of this is a field of the family or a step of FORMAT: what a view keeps is the session's.

## Sketches of forms

F5, part of FORMAT 19. A form's profile is its `sketch` (`family/sketch.rs`): lines (`start`, `end`), arcs (`turn` degrees
counter-clockwise from start to end) and whole circles (`centre`, `radius`) in its host plane's own axes, each number a number or a
formula, each line a part of the family (an id and a GUID, `parts_now`'s "sketch line", kept by place when a form is given again,
`Form::keep_lines`) so F7 can dimension and lock it. A revolve's axis is a line of its sketch with `axis: true`, one at most. The
shorthands from before, `profile` (corners in turn), `circle` and `axis`, are read as such lines (`FormIn`), by a file of 17 or a
command, and never written; a `form` patch giving one replaces what it stands for. Built (`Form::loops`), a sketch that is one
loop of straight lines in order, or one circle, is built as a profile always was (the same triangles and IFC:
`family_sketch_tests` against `testdata/family_profiles.golden.json`, which the last build before sketches wrote); any other is solved as the
project's sketches are (`sketch::solve`: every problem named, each line by its id), arcs cut into chords 32 to a turn: an outline
counter-clockwise with its holes clockwise, an island an outline again, several loops one form of several solids. A loop with holes
is capped by the constrained triangulation (`triangulation::constrained`), every ring gets its sides, and a revolve turns them all.
Only a lone circle is `round` in plan. `edit_work_plane`'s `keep` maps the sketch's points, a mirroring plane turning its arcs the
other way; `rename_parameter` and `every_formula` reach its formulas; the conservative bounds box an arc by its circle.

In the family document (`editor/sketch.rs`), Create > Extrusion or Revolve, or Void Forms > Void Extrusion or Void Revolve, sketch
on the view's work plane: "Modify | Create Extrusion" and the like, its Mode (Finish and Cancel Edit Mode), Draw (Boundary Line
and, for a revolve, Axis Line; Line, Rectangle, the polygons, Circle, the three arcs, Fillet Arc and, in a plan or an elevation,
Pick Lines: in a plan or an elevation a plane's or reference line's trace (where the type previewed puts it, as the view draws
it), or another form's edge; in {3D} another form's edge (`snap::creases`), a reference line or the line a reference plane was
drawn as, which {3D} draws while sketching, seen square on to the work plane, an edge square to it refused with a message; no
spline or ellipse), Modify (pick, Delete, a picked line's end dragged; Trim/Extend to Corner, TR, two lines trimmed or extended
to where they meet, each keeping the part clicked, `draw::corner`, F10a) and Work Plane (Set, the Work Plane dialog for the sketch: the plane taken, its lines go with it in the plane's own
axes; Show). The pointer snaps as a project's view snaps (`snap::among`, the same switches, guides, order and
markers, `canvas::snap_mark`) to the reference planes' traces across the work plane, where they cross, the other forms' edges
and their ends seen on it, the reference lines and the sketch's own lines (`Family::sketch_targets`). Chain, on the Options Bar
for Line and the chained arcs, is kept for the session: off, each starts afresh. A plan or an elevation that doesn't see the work plane flat asks Go To View, offering
those that do and {3D}: the Work Plane dialog's one Go To View (`FamilyEditor::on_work_plane`, `work_plane::Then`), the sketch
beginning in the view opened; {3D} draws on the plane where the pointer shows it. The lines are the project's sketch's
(`sketch::Sketch`, with its local Ctrl+Z), in the plane's axes. The Options Bar has Depth (End less Start), Sides, Radius and
Offset, and for Pick Lines Revit's Lock (F10a, kept for the session, off to begin with): a line picked with it on, with no
Offset and in a plan or an elevation, is locked to what it was picked from (a plane, a reference line, another form's line: what
Align would take there, `dimensions::reference_at`), the same lock Align's padlock leaves (`alignment`, written at Finish,
dropped by Cancel, its padlock shown closed beside the line, a click unlocking it); greyed with an Offset and in {3D}, where
Align isn't either. Properties the Start and End (a revolve's Start Angle and End Angle, as a selected one's Properties name
them), Material and Solid/Void. Finish checks the lines as the core does
(`family::sketch::finish_check`): open loops, crossing or overlapping lines, a revolve with no axis or a profile across it are
refused, said in the project's words with the lines shown red, the draft left as it was; then each type is built with the form,
and a fault of this form refuses it too. On success the form is in the draft, one step of its history, and selected. Cancel leaves
the draft (and so its Unsaved mark) as it was. A selected form's contextual tab has Edit Extrusion or Edit Revolve, as a double click on a form in a plan, an elevation or {3D}
(`views::form_at_3d`) does: its lines worked
out for the type shown, in the view shown if it sees the plane flat, else {3D}; a line that keeps its GUID and comes to where it
was keeps its formulas (`family::sketch::kept`), one moved holds numbers. A selected extrusion's Start and End show shape handles
in a plan or an elevation that sees its plane edge-on; dragged, they move along the normal, a step of the history; an end tied to a
parameter or a formula is greyed and a drag on it is refused, saying why. Where a form's dimensions and locks put its outline,
Start or End otherwise for the type previewed than it is drawn, its Properties say so under a note naming the type (F10,
`held_forms`), its ends a dimension sets greyed as they come to: the drawn numbers stay, and its Edit tool or the dimensions
change them.

## Blends, sweeps and swept blends

F6, part of FORMAT 19 (`family/forms.rs`). A form's `sketch` is always drawn in its host plane's axes: an extrusion's or a revolve's
profile, a Blend's base, a Sweep's or a Swept Blend's path. What a form is follows from what it holds (`Form::kind`,
`FormKind`): a `top` (lines in the host's axes) makes it a Blend, its base at `from` (First End) and its top at `to` (Second
End) along the normal; a `sweep` (`{profile, end?}`) a Sweep, or with `end` a Swept Blend. A profile is drawn in the profile
plane's axes (`forms::profile_frame`): x to the right of someone looking along the path, y the host's normal, its origin on
the path, so it is the same at every point of the path and the plane it is shown on is only where it is drawn (a Sweep's at
the middle of the path's first line, Revit's default; a Swept Blend's at the path's start and end). `twist` (Edit Vertices'
Twist) moves the top's, or the second profile's, first vertex on. `Family::check` (`forms::check_kind`) refuses a `top` with
a `sweep`, either with an axis or a mesh, a twist on anything but a blend or a swept blend or past `MAX_TWIST`, and a swept
blend's path of more than one line. Every line of `top`, `sweep.profile` and `sweep.end` is a part ("sketch line"), kept by
place within its group (`Form::keep_lines`); `rename_parameter`, `every_formula` and the formula checks reach them
(`Form::all_lines`). A build that reads up to 18 would drop the three fields and build a Blend as an extrusion of its base, so it
must refuse 19 as newer; a file of 17 or 18 holds none, and the step to 19 has nothing to do for them.

Built (`forms::build`, from `Family::solid`): a Blend's ends and a Swept Blend's profiles are one loop each (several, or a
hole, refused, as Revit's); a Sweep's profile may have several loops and holes. `forms::mapped` pairs two rings: each starts
at its vertex nearest the angle (about its middle, from +x) of the first ring's least-angled vertex, the second moved on by
`twist`; equal counts pair vertex to vertex, others are walked together taking the shorter diagonal each step, measured
as the turn between the two vertices about their own rings' middles (not as a length, which pairs a ring off the other's
middle across it and folds a swept blend's sides), a vertex of one held while the other's run past it (a fan, as Revit's
square-to-circle blend), so no vertex is added. A Swept Blend's end stations are its profiles exactly, so a held vertex
stays one point there. The path
(`forms::path`) is the sketch's lines chained end to end, open or closed, or one circle; a gap, a branch, two pieces, a line
of no length or a crossing are refused, naming where. Arcs are chorded 32 to a turn. At each vertex of the path the profile
is mitred along the bisector (`stations`); where a bend's set-backs either side of a segment (the profile's reach on the
inside times tan of half the turn) are longer than it, or the path turns back, it is refused ("would fold over itself").
A Swept Blend's rings are interpolated along the path's length. Every solid is closed and turned out by its signed volume
(no volume refused), capped where the path is open. On a level host a sweep's plan outline is the band it covers
(`Piece::holes` carries the hole of one round a closed path, which `render::component` draws), else the hull of its
triangles, as for any form not flat on a level plane. IFC writes the triangles as every form's (a closed face set).

Constraints (F7) reach a Blend's base and top and a sweep's path: they are `Form::plane_lines`, the sketch then the top,
whose vertices join within one group only (a base corner and a top corner over each other stay two); a Blend's First and
Second End are measured as an extrusion's Start and End. A sweep's profile lines lie in a plane of their own and aren't
measured yet: a dimension naming one is refused, saying so (F6d). `edit_work_plane`'s `keep` maps a Blend's top too (and
swaps base and top on a plane facing the other way) and refuses a sweep on a plane facing the other way or mirrored.

In the family document (`editor/sketch.rs`), Create > Blend, Sweep and Swept Blend, and Void Forms' Void Blend, Void Sweep
and Void Swept Blend, open sketch mode with every part of the form: `FormSketch::part` (`Part::Main`, `Top`, `Profile`,
`End`) is on screen, the others wait in `parts` and are drawn grey on their own planes (`other_parts`). A Blend's tab is
"Modify | Create Blend Base Boundary" with Edit Top ("… Top Boundary", drawn on the plane at its Second End) and Edit Base;
a Sweep's "Modify | Sweep > Sketch Path" with Sketch Path, Pick Path (Pick Lines on the path), Edit Profile and a greyed
Select Profile showing "<By Sketch>"; a Swept Blend's the same with Profile 1 and Profile 2. A profile can't be edited
before the path chains (said in the status bar); its plane faces along the path, so a view that doesn't see it flat gives
way to one that does (an elevation), else {3D} (`show_part`). Edit Vertices' Twist Left and Twist Right step `twist`. The
Options Bar's Depth is a Blend's Second End less its First; Properties show First End and Second End, a sweep no ends.
Finish checks each part (`finish_f6`), putting the part at fault on screen with its lines red, then builds the form with
every type as F5 does (`commit_form`); Edit Blend, Edit Sweep and Edit Swept Blend (and a double click) sketch it again,
each part keeping its lines' formulas where they don't move.

Left out, each a task: Edit Vertices' connecting chosen vertex pairs by hand (F6b); profile families (BIMCraft has no
Profile category: loading one, `sweep.family`, its offsets and angle; F6c); dimensions and locks on a sweep's profile (F6d);
paths across planes and Pick Path of 3D edges (F6e).

## Family options

A family's options are those of Revit's Family Category and Parameters dialog (`family/options.rs`), each a flat field of
`Family` left out of a file while it holds its default, so no `FORMAT` step: an older build leaves a field it doesn't know
out of the file (`lenient.rs`) and opens it, the family then as it was before the option was set (cut by its category, its
room its insertion point's, no classification). `family {name, options: {…}}` merges what it is given over what the family
has, `null` taking one back to its default, and every reply that shows a family carries `options` with all eight, defaults
too (`Family::options_json`); a dry run lists the ones it would change in `changes.options`. A family file holds them flat,
as a project does, and `path` reads them back into `options`. The family document edits them (T5b, `editor/options.rs`) in Family Category and
Parameters' Family Parameters table (Parameter | Value, Revit's order, under the category list and its filter) and in
Properties' Other group when nothing is selected, both in the draft, so the dialog's visit is one step of its history and
Cancel puts back what it began from; it sends them back whole when it loads or saves. Room Calculation Point turned on
starts at the insertion point, 300 mm up (so the room is the insertion point's until it is moved); a coordinate a formula
gives is shown, not typed over. Enable Cutting in Views is a tick beside By Category: ticked, the category's rule decides
and the tick shows it, greyed. OK or Apply runs `Family::check_options`, and what it refuses (an OmniClass number that
isn't digits and dots) is said in the dialog, which stays open. Turning Work Plane-Based off while a component of the
family is on a plane is refused by the project on Load into Project, said in the document.

- `work_plane_based` (off) lets a component of the family be hosted on a reference plane of the project, and
  `always_vertical` (on, Revit's default, and allowed off without `work_plane_based`: a wall-hosted family has it too) keeps
  one on a sloped plane upright; off, it tilts with the plane (*Work planes*, below). A door's or a window's family is never
  work plane-based, and the option can't be turned off while a component of the family is on a plane.
- `default_elevation` (0, mm): the height over its level a component of a family that isn't work plane-based is placed at
  when `component` is given no `offset` (Revit's Default Elevation); finite and within `MAX_COORD`. A component placed keeps
  its offset, so changing it moves nothing. An adaptive one (`Model::placed_by_points`, by `component` or `adaptive_place`)
  never takes it: Revit places it by its points alone.
- `cut_with_voids_when_loaded` (off) is stored only: a project's voids cutting a component will read it (T23).
- `shared` (off) makes the family a shared nested family when another nests it (*Nested families*, below).
- `cut_in_views`: none, a section cuts the family's forms as its category says (`Category::cut`); `true` or `false`
  overrides that. `Family::cuts_in_views` is the one place that decides, for a component's solids in the 3D scene
  (`view3d.rs`), the components a section cuts (`render.rs`) and a family door's or window's forms, which their category
  always cut before.
- `room_calculation_point`: `[x, y, z]` in the family's axes, mm, numbers or formulas that read only its length
  parameters (a door's or a window's `Wall Thickness` too), worked out for each type by `Family::check` (finite, within
  `MAX_COORD`) and renamed with them. `Model::component_room_point` places it with the component (mirrored, turned, moved,
  its height included), else takes the insertion point, and `Rooms::component` finds the room on the component's level
  whose volume holds it, as Revit's point has to fall inside the room's volume: its boundary at its walls' centre lines
  holds it in plan (whatever the project measures areas at, as #280 counts components too), and its height lies from the room's bottom (its level plus `base_offset`) up to its top (its
  `top_level`, else its own, plus `limit_offset`), both ends included (`Rooms::room_holding`; the first by id; none outside
  every room, or over or under it, as a ceiling luminaire hung over a room's top). The starter wall devices
  (the outlets, the switch, the distribution board, the data and voice outlets, the patch panel: `fixtures.rs`) have a
  point as Revit's do, `ROOM_POINT_OUT` (150 mm) in front of the wall's face at the middle of their height, so one on a
  wall's face is in the room on its side however rooms are measured; a component without a point is where its insertion
  point is, as before. Nothing in BIMCraft put a component in a room before, so this is that rule: `elements`
  gives each component's `room` (its GUID, or null). IFC keeps every component contained in its storey, as before, and
  adds one `IfcRelReferencedInSpatialStructure` for each room's `IfcSpace`, listing the components in it, after every other
  entity, so the rest of the file is as it was (`ifc::export_with` without them is the test's oracle); the import finds the
  storey by containment and passes the references over. Schedules have no Room field yet, and doors and
  windows have no From Room and To Room: a door's or a window's point is stored and checked but placed nowhere.
- `omniclass`: `{number, title}`, plain text, no table bundled: the number digits in groups between single dots
  (`23.30.20.17`), at most 64; the title one line of at most `MAX_TEXT`. `elements` shows it on each element of the family,
  and IFC writes one `IfcClassification` named OmniClass, an `IfcClassificationReference` for each number and title, and an
  `IfcRelAssociatesClassification` to the type objects of the families that have it (to the occurrence where IFC4 has no
  type object for its class). The import doesn't read it back yet.

## Work planes

Revit's Edit Work Plane and work plane-based families (T4, T13). In a family, `edit_work_plane {family, form | ref_line,
host, keep?, dry_run?}` puts a form or a reference line on another of its planes (a `host` by name, id or GUID, as forms take
it), building a `family` patch, so its checks, dry run and one undo step are the `family` command's. By default it moves with
the plane, as Revit's does: a form keeps its profile and its `from` and `to` in the new plane's axes, a line its ends (taken
from the old plane's axes to the new one's), and what is drawn on the line's planes follows. With `keep` it stays where it is:
a form only on a plane parallel to its old one (facing either way), its profile mapped and its depth moved by the distance
between the planes (negated and swapped facing the other way; a formula is kept and wrapped, `(…) + d`); a line only on a
plane it already lies on. A mesh form has no work plane to change. `host` may also be a name, as the Work Plane dialog lists it
(`"Reference Line 1 : Vertical"`, `"Reference Plane : Left"`) or a plane's own (`Planes::by_name`).

The project has reference planes (`hosting.rs`, `ReferencePlane`, category `refplane`): drawn by `reference_plane {start,
end, up?, name?, level?}` as a family's are, along a line with an `up` (the vertical by default, so a line drawn in plan makes
an upright plane), their own axes x along the line, y `up` made square to it, z the normal, the origin `start`, so what they
host moves and turns with them. A plan draws an upright one as a dashed line; `move`, `copy`, `rotate` and `mirror` take its
ends and its `up` (each end keeps its height). A name is one plane's.

A component of a work plane-based family may be hosted on one (`Component::host`, a `Hosted`: the plane, `uv` its insertion
point in the plane's axes, `offset` its Offset from Host along the normal). The host is a `HostRef` tagged by kind
(`{"plane": guid}`), and a face is another kind beside it (*Place on Face*, below); one on a level has no host, its level
its work plane. `uv` is the truth: after every command `Model::settle_hosts` works the component's `at` and `offset`
(Elevation from Level, over its `level`, its Schedule Level) out from it, so whatever reads a component's `at` reads where it
is, moving or turning the plane moves it, and a level moved under it changes its Elevation from Level and leaves it on its
plane. A command that moves the component itself (`move`, `copy`, `rotate`, `mirror`, `modify` of `at` or `offset`) moves its
`at`, which is taken onto the plane (straight down or up, along the normal for an upright plane) as its new `uv`. `component
{…, host: {plane: name | id | GUID}}` (Place on Work Plane) takes `at` [x, y] straight onto the plane (an upright plane needs
[x, y, z], taken along its normal), `offset` as its Offset from Host and `level` as its Schedule Level (by default the
highest level at or below it; Revit lets it change, and the component stays where it is); `edit_work_plane {ids, host}` puts
components on a level (at their plan point, on it) or a plane (taken onto it; from another plane, its Offset from Host kept).
Deleting a plane deletes what it hosts. Turning a plane about the vertical turns what it hosts with it (`settle_hosts`
adds the turn of the plane's line in plan to the component's `angle`, unless the same command turned the component too).
`Model::check` refuses a host that isn't a reference plane and one on a family that isn't work plane-based.

`Model::component_placement` is the one place a component's family axes are put in the model, for the plan, the 3D view and
its graphics card's copies, sections, bounds, rooms and IFC: upright, turned `angle` about z, unless it is on a plane that
isn't level and its family isn't Always Vertical; then it tilts by the least turn that takes the vertical to the plane's
normal, so on a level plane both are the same and `angle` still turns it about its own z. Tilted, its plan outline is the
hull of its triangles seen from above and it draws no plan symbols; a hosted component isn't banded by height
(`derived.rs`), since its family says whether it tilts. Adaptive components (`Model::placed_by_points`: frames of their own, or an adaptive family's
test placement) take their frame from their own points: `component_placement` places them upright, as before, whatever they
hold, `settle_hosts` never moves them, and `Model::check` refuses a `host` on one ("adaptive components take their place from
their points"), so `component` and `edit_work_plane` can't put one on a plane. `elements` gives each component's `work_plane` (`Level : Level 1`, `Reference Plane : Ramp`) and `offset_from_host`
(its host's offset, or its height over its level), which `modify` also takes; component schedules have Elevation from
Level, Work Plane and Offset from Host besides Level, its Schedule Level. IFC keeps it in its Schedule Level's storey, its
`IfcAxis2Placement3D` taking the tilt as its Axis and RefDirection; the project's reference planes aren't exported. FORMAT
19 holds this change: a build that reads up to 18 would stand a tilted component upright and lose its plane; a file of 17
or 18 holds neither.

The desktop's Component tool, placing a work plane-based family, shows Place on Work Plane and a Placement Plane (the plan's
level, or a reference plane of the project) on its Options Bar, and for any family Rotate after placement, which selects what
it placed and starts Rotate about its point. Properties shows a work plane-based family's component's Schedule Level,
Elevation from Level (read only on a plane), Work Plane and Offset from Host; another's, as Revit's level-based ones, its
Level and Elevation from Level (the same `offset`). Selected work plane-based components (not adaptive ones) have Revit's
Work Plane panel on their contextual tab: Edit Work Plane opens the Work Plane dialog (T12) on them, naming the first one's plane,
and runs `edit_work_plane {ids, host}`: Name puts them on a level or a named reference plane, Pick a plane on the face of a wall,
a floor, a ceiling or a roof clicked in the 3D view (`Model::face_role`), Pick a line on the level of a line clicked; their
Offset from Host is kept, and there is no Pick three points, Offset or Dissociate for them. The Reference Plane tool (`Tool::ReferencePlane`: RP, the `reference_plane`
command's alias, as Revit's; Architecture > Datum's Ref Plane, beside Set, where Revit's Work Plane panel has it; Find or
Create Similar) draws an upright plane through two clicks in plan. A 3D view's work plane may be a reference plane of the
project, which it then follows (T12, [views and graphics](views-and-graphics.md)). Picking a sloped plane in plan and drawing
planes in sections are not here.

### Place on Face

Revit's Place on Face (T16): a work plane-based family's component hosted on a face of a wall, a floor, a ceiling or a roof,
`host` `{"face": {"element": guid, "face": role}}` (`HostRef::Face`). No option of its own: Revit lets a work plane-based
family take a face as its work plane, and its face-based families' voids that cut the host are read from the voids
themselves (below). A face is named by what it is to its element (`FaceRole`, paint's names): a wall's `exterior` (to the left of
start to end) or `interior` side, a floor's, a ceiling's or a roof's `top` or `bottom`; never by its triangles, which every
join, opening and shape point renumbers. `uv` is in the face's own terms (`Model::face_at`, `face_uv`): on a wall's side
`[along, up]`, mm along its location line from its start (as a door's offset) and up from its own base (as a sill), the
face found at that station (out from the centre on an arc), so a wall moved, turned, made thicker or of another type takes
it along; on a floor's, a ceiling's or a roof's face `[x, y]`, the plan point, the face's height found again under it (the
piece of a shaped floor, the roof's slope, the underside by its type's thickness or variable layer), so a change of offset,
slope, thickness or type lifts and tilts it. `move`, `rotate`, `mirror` and `copy` of a floor, a ceiling or a roof take what
is on its faces with it (`relocate` and `duplicate`, `face_hosted`); a wall's need nothing, but a mirrored or copied wall's
are put as far from its other end, turned and mirrored (`side_mirrored`), and a flipped wall's go to its other side, the
same face. Deleting the host deletes them. One its face no longer reaches (the floor's outline made smaller, an opening
cut there, a wall shortened) stays on the face's plane carried on (`HostAt::on`) and `Model::warnings` names it, as Revit
warns of a lost association; `check` refuses only a face its element doesn't have. `settle_hosts` looks at what a command
changed, what points at it and what the walls, floors, ceilings and roofs among those host, so a level moved moves what is
on its walls.

Always Vertical is one rule, `hosting::vertical_z`: on, z stays up on every host, a ceiling's underside too; off, z is the
face's normal, so it tilts with a slope, stands out from a wall and hangs from an underside. On a floor's, a ceiling's or a
roof's face it then tilts as on a plane; on a wall's side `angle` turns it from the face's level direction (`HostAt::across`,
to the right of someone facing the wall), about z or the normal, so it turns with its wall, and following the face its y
goes up it. `component {…, host: {face: {element, face}}}` takes `at` [x, y] onto a floor's, a ceiling's or a roof's face
and [x, y, z] onto a wall's side (its nearest point; [x, y] alone is refused there), `offset` along the face's normal;
`edit_work_plane {ids, host: {face: …}}` puts components on one. Work Plane reads `Floor : Generic - 200mm : Top`, `elements`
gives `host_element` and component schedules a Host field. IFC keeps the placement and writes no host relation (IFC4 has
none for an element that fills nothing). FORMAT 19 holds this change: a build that reads up to 18 knows no `host`, and a file
of 17 or 18 holds none.
The desktop's Component tool offers Place on Face beside Place on Work Plane: in plan it places on the top of the highest
floor or roof of the level under the click, in a ceiling plan on the underside of the lowest ceiling (`Model::face_in_plan`),
in the 3D view on the face clicked (`Model::face_role`). A wall's top and ends, the faces of other elements and of linked
models are not here.

A face-hosted component's voids cut its host (T16b), as a Revit face-based family's do: each void of its family that cuts
the host (`cuts` `both`, the default, or `host`), whatever its place in the family's list and whatever it overlaps (F10d's
rule, as a door's in its wall; `Family::face_voids`), and reaches into its host makes an opening, a recess or a penetration
there; a `forms` void cuts the family's own forms alone. Before, a `both` void listed after a solid it overlapped cut the
face no more (T22's order rule, which F10d dropped for walls only); Place on Face came with 19, so no file needs a step.
There is no option: any face-hosted component's such voids cut, whatever
`cut_with_voids_when_loaded` says, which is about the project's voids cutting a component; and a work plane-based family's
void that cuts none of its forms is not warned of as cutting nothing. `Model::face_voids` gives a component's voids placed
where it stands (mirrored, on its face's frame), `Model::hosted_voids` those of everything on a host's faces (found through
the lookup's referrers, so an edit of the component or the host makes the other again in the 3D scene). A wall's go with its
doors' and windows' (`walls::host_voids_in`) through `view3d::wall_solids`; a floor's, a ceiling's and a roof's solids
(`view3d::slab_solids`) go through the same `cut_by_voids`: each solid a void's box reaches becomes its triangles less the
voids (`csg::subtract`), a `Solid::Mesh` a section cuts, and every other host and solid is left as it was, so only hosts
with such voids pay for csg; should cutting fail, the host is drawn uncut. `view3d::layered` splits a cut slab's piece into its
type's layers (each the piece within that layer of the uncut slab), and a section draws the layers of a cut piece that still
has two upright sides (`render::quad`). A plan, which sees a floor's or a roof's top, draws the hole a millimetre under its
top face (`render::slab_holes`), its edge and the floor's surface pattern left out of it; a ceiling plan likewise just over a
ceiling's underside; a plan's cut through a wall clears it where it passes through the voids, as a door's do. IFC writes an
`IfcOpeningElement` for each component whose voids reach its host (`view3d::reaching_voids`), its body the voids as closed
face sets in the host's frame, and an `IfcRelVoidsElement` to the `IfcSlab`, `IfcRoof`, `IfcCovering` or `IfcWall`. Nothing
new is in a file. A void family placed alone (T23) and Cut Geometry are not here.

## Plan symbols

A family's `symbols`, of any category, are what its plan draws besides its forms, in its own axes: each is exactly one of a
`line` (up to `MAX_PROFILE` points), an `arc`, a `text` or a `dot`, with `visible` (a yes/no parameter or a formula) for any.
`Family::marks` works them out, for a type's values, as `Mark`s: a `Mark::Line` (an arc as chords of `ARC_STEP` degrees; a
label as a `Mark::Label`, lettered with `font.rs` by `lettering` where it is drawn) or a `Mark::Dot`. A `text` is `{at, height, value, align?}`: capitals `height` mm
tall, `at` the middle of the line, started, centred or ended there (`Align`); `{Parameter Name}` in `value` reads that parameter
(`said_text`: a number as plainly as it reads, a material by its name), an unknown name or an unclosed `{` is refused by
`Family::check`, and the lettered text is at most `MAX_TEXT` characters. `at` is the middle of the line of text vertically, and `align`
sets the horizontal anchor. A `dot` is a `Circle`, drawn solid in ink. Everything
numeric is checked finite and within `MAX_COORD`; a label's height and a dot's radius must be above zero.

A symbol may name the subcategory of its family's category it is drawn in (`subcategory`, by its English name, any case:
`Opening` for a door or a window; `Family::check` refuses a name its category has none of, saying which it has); a view hides and
restyles it apart from the rest (Doors > Opening, [views and graphics](views-and-graphics.md)). The starter doors' swings and the
windows' opening lines are in Opening. A selected symbol's Properties pick it from the category's subcategories.

`Model::component_marks` places a component's marks (mirrored, then turned and moved) and `Model::opening_marks` an opening's,
and `render::marks` draws them: lines in the category's pen (`BY_CATEGORY`, so Object Styles and Visibility/Graphics apply), dots
as filled discs. Coarse detail leaves them out, unless their Visibility Settings tick Coarse (F8, *Visibility Settings*). A label's anchor is placed with the component (turned, mirrored, moved) but its
letters never are: they are always drawn at 0 degrees in the view, left to right with a horizontal baseline, however the component
stands (the simplest rule; no flipping by quadrant). Symbols are annotation of the component's level: they draw in that level's
plan and its ceiling plan whatever height the component's forms reach (a ceiling light shows its symbols in the floor plan) and
on no other level's; the forms are outlined by the old rule (whole while any of them is between the view's depth and cut plane,
and not in ceiling plans). A component with no forms draws its symbols the same way. IFC export ignores symbols (they are plan notation, not geometry); the Family Editor's Annotate > Detail makes
all four parts. Library parts: a symbol of any kind has its own id and GUID (`HasPart`, `keep_parts`). A file written with
`text` or `dot` symbols, or symbols on a category other than a door or a window, is read by an older version only with them left
out or refused, so the file format is unchanged.


## Symbol sets

A symbol set (`symbol_set.rs`) is families that are a plan symbol and nothing else, loaded on demand and never part of a new
project: the starter fixtures (`fixtures.rs`) are in every project, and 76 more would crowd every family list. The
`symbol_set {set, names?}` command adds a set's families to the project (the Electrical tab's Symbols > Load AREI Symbols
button runs it); a family the project has by that name stays as it is, so loading twice changes nothing and an edited family is
never overwritten (`loaded` and `kept` in the reply). Each family has one type, `Standard`, no forms, and the symbols of
`crates/core/data/arei-symbols.json`, in the family `symbols` schema (mm, y up, the origin at the symbol's own origin): a
switch is in category `switch`, a socket in `outlet`, a data socket in `dataoutlet`, a light point or luminaire in `lighting`
and everything else (appliances, consumers, sources, domotics, protection) in `generic`, so the Device and Lighting Fixture
tools list their own kinds and the others are placed with the Component tool. They have no connectors, loads or 3D forms: they
are plan symbols only for now, and a family without a connector raises no warning in `inspect`.

The `arei` set is the graphical symbols of Table 2.23 of the AREI/RGIE (FOD Economie, Book 1), redrawn originally. The names
are those printed in its Dutch and French versions (`name_nl`, `name_fr` in the data), the English ones are working names. The
symbols are drawn at about 30 mm per drawing point, so a socket is about 25 cm wide on a plan. `names` (`en`, `nl`, `fr`) picks
the language of the family names; as given none, a French project gets the French wording, a Dutch project the Dutch
and every other language the English.


## Nested families

T6a (`family/nested.rs`). A family may nest families, as Revit's Insert > Load Family in the Family Editor does: `nested` holds
each one's name and whole definition (`NestedFamily`), carried inside the host, so a family file and a project's family hold what
they nest. `nested_instances` (`NestedInstance`) place their types: each on a plane of the host (`host`, a `PlaneRef`, Ref. Level
when none) at `at` in that plane's axes, `offset` along its normal and turned `angle` degrees about it, `visible` a yes/no or a
formula, and `params` setting the nested family's parameters, type or instance: a number, or a formula over the host's
(Associate Family Parameter: `{"W": "Width"}`); a material, a text or a URL parameter names one of the host's of its kind, or
holds the material's name or the words, as a form's `material` does. A parameter the nested family works out by a formula, a
reporting one and a family type (T24's) can't be set. Both are parts (`HasPart`: ids and GUIDs, kept by place); the definition
inside is the nested family's own, its forms' and planes' ids and GUIDs not the project library's (a nested copy of a project
family holds the same GUIDs). Every walk that names a parameter or a material reaches them: `rename_parameter`, `every_formula`,
`material_refs_mut`, `materials`, `rename_material` (into the definitions, at every depth), `hosted_on`, `rehost` and
`resolve_hosts`; deleting a plane an instance stands on is refused, naming it.

`Family::pieces_at` adds each visible instance's pieces (`nested_pieces`): the nested family's values for its type, the
instance's `params` worked out over the host's values and seeded over the type's (`Family::values_seeded`), built (its own nested
instances too), then placed on its host plane's frame, flexed with the host's type, so an instance follows a plane a label moves.
Plans, sections, the 3D view, the family's own views, `render` and IFC all read `pieces_at`, so the nested geometry is in the
host's triangles and in its IFC body with nothing else to do; an instance standing upright on a level plane adds its plan
symbols to the host's marks. A host's voids don't cut nested instances and a nested void cuts its own family only (Revit's
default; a void cutting the host is T23's). `Family::check` builds every type with them and checks each definition in turn: no
family nested in itself or in one it is nested in (`Model::check` knows the project family's name), names unique, at most
`MAX_NESTING` (5) levels, `MAX_NESTED` (32) families and `MAX_NESTED_INSTANCES` (256) instances to a family, `MAX_NESTED_TOTAL`
(128) definitions in a tree, and a build meter of `MAX_NESTED_BUILDS` (2048) instance builds shared by the tree, so a hostile
file ends in an error.

The `family` command: `nest {path | definition, name?, overwrite?, shared_nested?}` loads a family into the family
(`nested::nest_into`), refused over one it nests defined otherwise unless `overwrite` says `version` (its values kept where they
fit, through the reload rules above) or `version_and_values`; `nested` replaces the list, `delete_nested` takes families away
(refused while an instance places one); `nested_instance` patches one by id or GUID (none adds one; a `host` may be the Work
Plane dialog's name), `nested_instances` replaces the list, `delete_nested_instance` takes them away; `dry_run`'s `changes` say
`nested` and `nested_instances`.

A nested family whose `shared` option is on is shared. Loaded into a target (the project's families, or a host's nested list),
a family brings the shared families nested in it, reached through shared ones only (a family that isn't shared keeps what it
nests to itself, `Family::shared_within`): one the target hasn't is added to it; one it has otherwise is asked about
("Shared Family Already Loaded", `nested::reconcile_shared`; `shared_nested: {name: project | family}` in the command): `project` gives the family loaded the target's (the default when
unanswered), `family` overwrites the target's with the family's, its values too. One definition of a shared family is kept
across the target: after one changes in the project, every family of the project that nests it, at any depth, takes the same
(`Family::replace_shared`). Within the family loaded, too: before it is reconciled, every copy of a shared family nested in
it is made the first one met going down its nested families in order (`Family::unify_shared`: two branches P and Q each
holding Leaf, defined otherwise, load with P's Leaf in both and in the project's), and what the target takes is read once
the answers are applied, so no definition taken brings back a copy another replaced. A shared nested instance is built into its host as any other: its own element in the project
(Tab-select, schedules, tags, IFC aggregated into its host) is T6b's. A nested family's type chosen by a family type parameter
is T24's. A door's leaf on a hinge (T26) is a nested instance on a pivoted line's face or on a pivoted plane, placed on its
host's flexed frame, so it turns with the pivot (F7d, *Dimensions and constraints*): `family_pivot_tests` swings a nested leaf
by an instance `Swing Angle` and finds it, and the door's IFC body, as drawn by hand at 45°. An instance's `angle` may also
read an instance parameter.

In the family document (`editor/nested.rs`): Insert > Load Family loads a family file into the family, asking Family Already
Exists and Shared Family Already Loaded against its nested families (`exists.rs`'s `After::Nest`); Create > Component (CM) places
an instance of one, picked in the Options Bar, by a click in a plan or an elevation, on that view's work plane (from {3D} the floor
plan opens); a click on an instance selects it ("Modify | Nested Instance"), its outline drawn, and Delete takes it away. Its
Properties: the type (any nested family's), the Work Plane, X, Y, Offset and Angle, Visible, and its family's parameters under
their groups, each with Revit's "=" (`Field::Nested`, `NestedRows`). The Project Browser's Families lists the nested families
and their types, with Edit Family and Delete on the right-click. Edit Family (also on the instance's contextual tab) opens the
nested family's own document over the host's (`BimApp::family_hosts`): its Load into Project and Load into Project and Close load
it back into the host, asking Family Already Exists in the family's words; Close Family goes back without loading. The project is
untouched until the host is loaded. A nested instance is moved by its Properties; dragging it in a view is T6b's. FORMAT 19
says an older build would leave both fields out of a file.

## Reloading a family

`family {path, name?, overwrite?}` can choose how to load over a family already in the project:

- `no` keeps the project definition and adds no undo step (`overwritten: false` in the reply).
- `version` replaces the definition but keeps compatible project parameter defaults and existing type values.
- `version_and_values` replaces the definition and takes its defaults and matching type values from the file.

Compatibility means the same case-sensitive parameter name, kind, Type/Instance setting and shared GUID (including both absent).
Reporting parameters never carry values over. Family Type parameters also require the same `of` category; their values remain
null until nested-family support. Neither compatible version is driven by
a formula. Incoming formulas always belong to the new definition. Project-only types remain, keeping compatible values;
new parameters and types use the incoming values. Explicit instance overrides are never rewritten by a family reload.
Optional definition fields omitted from an explicit replacement are reset, rather than retained from the old definition.
Parts keep identities through the existing `keep_parts` rule. Model validation refuses an incompatible reload atomically;
the normal undo restores a successful reload. `dry_run` previews the change without writing it.

An explicit reload of an identical definition is skipped with `overwritten: false`, without adding an undo step.

A reload keeps what stands on the family where it is: components hosted on a reference plane or on a face of a wall, a floor,
a ceiling or a roof (`host`) keep their host, place and Offset from Host, and adaptive components keep their points. A file's
`adaptive` definition is loaded with the rest of it; a version that is no longer work plane-based, or no longer adaptive,
while components rely on it is refused as a whole. The incoming family's formulas are checked as any edit's are: one that
names a parameter the incoming family doesn't have refuses the reload (`no` never reads the file's formulas).

The option is only accepted with `path` or `definition` (the family as such a file holds it, given in the command, read
strictly), and is the caller's decision: a field named `overwrite` inside a family file cannot choose the policy. With no option the original merge behaviour is preserved, including partial definitions. A family not
yet in the project is loaded normally under all three policies. This changes commands, not the saved file format.

Dimension references to family forms and reference planes are not supported yet; reload does not silently delete project
dimensions.

In the desktop (`editor/exists.rs`), Load into Project, Load into Project and Close and the Component tool's Load Family ask
as Revit's "Family Already Exists" does when the project has a family of that name defined otherwise: Overwrite the existing
version (`version`), Overwrite the existing version and its parameter values (`version_and_values`), or Cancel, which keeps the
project's (as `no`) and changes nothing. The dialog only picks `overwrite`: the family document loads its draft as a
`definition`, a file by its `path`. A family the project hasn't, or one that would load unchanged (the draft as the project's
family opens, or the core's dry run saying `overwritten: false`), asks nothing and the unchanged one adds no undo step. A
document renamed renames the project's family as before, asking nothing. Load into Project and Close closes only once the
family went in; on Cancel or a refusal the document stays open, a refusal said in it.

A load asks its questions in turn (`Question`): the family's own, then "Shared Family Already Loaded" for each shared family
nested in it that the project has otherwise (keep the project's version, or overwrite it with the family's), as the core's dry
run says them (`shared_conflicts`); the answers go into the command as `shared_nested` and are kept as the load's `Decided`. Loading
into a family (Insert > Load Family in a family document, or Load into Project from a nested family's document) asks the same
two questions against that family's nested families, in the family's words (*Nested families*).

Load matches family and type names exactly, including case. The caller's `name` overrides the file's name. A new family name
creates a separate library definition; imported part identities are reconciled by the normal library sync and do not rename
or replace an existing family by GUID. A renamed type is added while the old type and its placed instances remain. Switching
instances to the new family or type is a separate explicit edit.

## Exit test (F10)

0.8's exit criterion: a parametric window drawn from blank and taken all the way through, as a user does it. Two twins build the
same window: reference planes Left and Right (the opening's sides) and Head, and the blank's Sill, on Ref. Level and locked to it,
since a window's axes rise from its sill; an Aligned Dimension Left to Right labelled Width with EQ about Center (Left/Right), and
Sill to Head labelled Height (a door or window family always has Width and Height, so the labels take them); a frame extruded on
Center (Front/Back), 80 deep, its outline locked to the four planes (picked from them with Pick Lines' Lock and trimmed to
corners, F10a) and its light dimensioned and locked 50 mm inside them; and a void extrusion there through the wall, its outline
Align-locked to the planes, listed after the frame: it cuts the wall wherever it is listed, and Cuts: Host keeps it off the frame
(F10d). Three types (900 x 1200, 1200 x 1500, 600 x 600), loaded, placed in a wall, flexed through its types, a window
schedule, IFC.

- **The desktop** (`crates/ui-egui/src/tests/family_exit.rs`): every step through the window, from File > New > Family… > Window
  to File > Export's IFC: the Reference Plane tool and Properties' Name, Aligned Dimension (Tab onto Sill) and the Options Bar's
  Label, EQ, Extrusion with Pick Lines, its Lock and Trim/Extend to Corner, Rectangle, Void Extrusion with Rectangle and Align's
  padlocks, the dimensions' padlocks, the void's Cuts in Properties, the void picked in the browser's Forms, Family Types, Load into Project and Close,
  the Window tool and the Type Selector, View > Schedules. At each type it measures what is
  built: the frame's pieces (`Model::opening_pieces`) and a picture of the 3D view probed across the opening's edges (the wall
  just outside, the frame just inside, through the light). The IFC is read back for each IfcWindow's size, its
  IfcOpeningElement, the IfcRelVoidsElement to its IfcWall and Width and Height in `Qto_WindowBaseQuantities`.
  `cargo test -p bimcraft-ui-egui family_exit`; with `BIMCRAFT_F10_DUMP` set to a folder it also saves the screenshots
  (`f10-*.png`: Pick Lines with Lock, Tab onto Sill, the family's plan and Front elevation, Family Types, the project's plan,
  elevation and 3D view at two types, the schedule) and its IFC (`f10-window.ifc`) there.
- **Commands** (`ci/family-window.txt`): the same family through the `family` command, placed, flexed with `modify` and
  scheduled; `the_exit_tests_window_flexes_as_built_schedules_and_goes_to_ifc` (`crates/core/src/tests.rs`) checks it, and CI's
  IFC job writes it (`bimcraft-cli run --script ci/family-window.txt --save family-window.ifc`) and validates it with
  IfcOpenShell (`python ci/validate_ifc.py family-window.ifc`: the EXPRESS rules and a shape for every product).

What it found and fixed: Ref. Level, drawn as a level in an elevation, couldn't be picked as a reference (`Staged::plane_of`); a
window stood `Sill Height` above its planes in its document's views (its scratch model now places it on Ref. Level); a frame
extruded across the wall was outlined whole in plan instead of cut as its jambs (`view3d::level_slice` of each piece the cut
plane crosses, in the project's plans and the family's); a void straight through the wall lost its jambs in plan (an edge from
one face to the other is a jamb, not the face's); the Delete key did nothing to a selected form or plane in the family
document; the Forms tab's Delete left dimensions measuring to the form gone (Load into Project refused them), and its row ran
past the palette's edge; it showed a locked form's corners as drawn, not as the type previewed puts them.

The follow-ups it left open, done after it: Pick Lines' Lock and Trim/Extend to Corner in sketch mode (F10a,
`family_sketch_locks`' `pick_lines_with_lock_…`); a window's forms below the cut seen in plan (F10b, `family::views`'
`a_window_familys_plan_sees_its_forms_…`); planes on one another, Tab and the blank window's Sill (F10c, `family_views`'
`a_sill_plane_on_ref_level_…`); a void cutting the host whatever its place in the list, and Cuts (F10d,
`host_void_tests`). What they found: in a plan or an elevation, Pick Lines put a line where a plane is drawn rather than where
the type previewed puts it, as the view shows it; it takes the one shown now.

## Where the code lives

Moved from the root `CLAUDE.md`'s module map, which names the files and points here for what each owns.

In `crates/core`: `family.rs`, `family/validate.rs`, `csg.rs`, `fixtures.rs`, `favorites.rs`, `shared_parameters.rs`:
families, what's wrong with one and what's worth knowing about it (its formulas, its checks, its forms' boxes), solids cut
from solids, the starter fixtures, the user's favorites, the shared parameters file. `hosting.rs`: the project's reference
planes, components hosted on them or on faces (Place on Face), and where a component's family axes are. The rest of
`family/`:

- `family/planes.rs`: reference planes and lines, forms' hosts.
- `family/options.rs`: Family Category and Parameters' options.
- `family/views.rs`: a family's plans and elevations.
- `family/sketch.rs`: forms' sketches.
- `family/forms.rs`: Blend, Sweep and Swept Blend: their kinds, paths, vertex mapping and triangles.
- `family/constraints.rs`: its dimensions, labels, locks and EQ, solved as a type flexes; `family/constraints/pivot.rs`,
  lines and planes that turn about a pivot.
- `family/nested.rs`: nested families and their instances, built into the host.
- `family/visibility.rs`: Visibility Settings: which views and detail levels draw a form, a symbol or a nested instance.

In `crates/ui-egui`, the Family Editor:

- `editor.rs`: family drafts. `editor/document.rs`: the family document every family opens as. `editor/tabs.rs`: the documents open, as tabs beside the project's, and the rules for closing them.
- `editor/association.rs`: parameter associations. `editor/extrusion.rs`: a selected form's Properties.
- `editor/exists.rs`: Family Already Exists. `editor/types_dialog.rs`: Family Types.
- `editor/temp_hide.rs`: the document's Temporary Hide/Isolate. `editor/views.rs`: its plans and elevations.
- `editor/sketch.rs`: sketch mode. `editor/dimensions.rs`: their dimensions and Align.
- `editor/work_plane.rs`: its Work Plane dialog and Go To View.
- `editor/options.rs`: the family's options (Family Category and Parameters, Properties' Other).
- `editor/nested.rs`: its nested families (Load Family, Component, Edit Family). `editor/pivot.rs`: its pivots and grips.
- `editor/connectors.rs`: a family's connectors in the document, placed from Create > Connectors, listed in the browser, typed
  in Properties. `editor/connector_marks.rs`: their symbols and arrows over {3D}, plans and elevations, lit and picked (an
  overlay drawn through a view's projection alone).
- `editor/lit3d.rs`: {3D} staged for the project's 3D view (`FamilyScene`), its selection and hover highlight. `editor/visibility.rs`: Family Element Visibility Settings.
