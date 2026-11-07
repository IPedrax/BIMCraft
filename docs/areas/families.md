# Families, forms, materials and textures

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A family (`Model::families`, `family.rs`) is parameters, types that set them and forms in its own axes: x along a
component's angle, y to its left, z up from its base; a form is a profile in a sketch plane (`xy` swept up, `xz` along y,
`yz` along x), its sizes numbers or formulas, or triangles as a file brings them. A component places a type;
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
Thickness` about the opening's middle plane) and cuts none of the family's own forms, by the same boxes the "cuts nothing"
warning reads (so one listed before them all), is the opening's shape in the wall in place of the rectangle of its `Width` and
`Height`; a void that cuts a form (a sash's light) cuts that form alone, and a family with no such void cuts the rectangle as
before. `Model::opening_voids_local` gives them in the family's axes with the opening's flips, `Model::opening_voids` where it
stands. The wall's prisms leave such an opening out (`walls::prisms_in`; `prisms_rect_in` keeps every rectangle) and
`view3d::wall_solids` takes each prism a void reaches as triangles less it (`csg::subtract`), a `Solid::Mesh` a section cuts
(`cut: true`), so the 3D view and sections show an arch as an arch and a void deeper than the wall goes right through it;
should cutting fail, every opening cuts its rectangle. `view3d::layered` splits such a piece into the wall type's layers (each
the piece within its two boundaries in plan, `walls::layer_lines`), and a section's lines between layers follow the cut round
the void. A plan clears the void's slice at its cut plane (`render::level_slice`) within the wall's faces, its edges across the
wall the jambs; whether the opening is drawn at all is still its rectangle's height. IFC writes the voids as the
`IfcOpeningElement`'s body, closed face sets placed at the opening's middle as its forms are, in place of the extruded
rectangle. A void's size may read an instance parameter (a depth in its `from` and `to`), so each opening cuts its own.
Neither is new in a file, so nothing takes a FORMAT step: an older build reads the family and cuts the rectangle.

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
(`editor.rs`) edits text (`FormText`, `ParamText`, ...) and changes the project's family only through a `family` command; its
preview is a scratch `Session` of its own, and it says what's wrong inside the window (the status box, over the view's corner, would lie on its
buttons).

A type of a category or of a family is renamed with `rename_type` (`Model::move_entry` moves it in its map, and `rename_entry` the
library entry and every reference `Model::lib_refs_mut` visits, so it keeps its GUID). A type or a family goes with `delete_type` (one entry, refused while an element uses it, and for the last type of a category or of a family: a
tool needs one to place) and `purge` (every entry of the kinds asked that nothing uses). Use is what `Model::lib_refs_mut` visits, the one place
that names every reference to a library entry: `Model::library_uses` counts them on a copy of the model, `Model::unused_entries` is one pass of
what purge takes out (each category keeps its default's type, else its first; a family that goes takes its types), the command repeats it
for what a pass frees (a family's material, a type's), at most 16 times, on a copy that becomes the model only when it isn't a `dry_run`.
`Model::remove_entry` is the single place that knows where each kind lives. The desktop's Purge Unused window (`purge.rs`) lists a dry run
for the kinds checked and runs the same; the Project Browser's Families lists the system types and has the right-click Delete.

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
parameter names the category of the families whose types it picks (`of`) and holds no value until nested families exist.
`group` is the heading it is listed under (`Group`, 14 of them); none, its kind's (`Parameter::group`: Dimensions for lengths,
areas, volumes and angles, Materials and Finishes for materials, else Other). `shared` is a shared parameter's GUID, the
same wherever it is used and distinct from the parameter's library-entry GUID (which is per project and per family): two
families' parameters of one shared GUID are one name and one kind, and one name shared is one GUID
(`Model::check_shared_parameters`); without it, it is the family's own. Schedules list every family parameter by name either
way, as they always have. A `reporting` parameter, a length or an angle, reads a dimension and never drives one: no formula,
type or component sets it (`Parameter::check_own`, `Family::check`, `Model::family_values_ok`) and, until labelled dimensions
exist, it holds its `value`, with a warning saying so. Discipline is implied by the kind (all Common), so there is no field.

`family {parameter: {name, rename?, …}}` is Edit Parameter: the fields merged over the one named (added if there's none), a
shared parameter's name and kind kept. `rename` goes through `Family::rename_parameter`, the one place that walks every part
of a family naming a parameter (formulas, forms' sizes, materials and `visible`, symbols' sizes and `{label}` text, connectors'
values: a new part that holds a formula adds its arm there), with `Library::renamed` keeping its GUID and the components'
values following. `instance` switched moves values, not loses them (`patch_parameter` in `commands.rs`): to instance, each
component takes its type's value (a type with nothing placed loses it, as Revit's does); to type, each type takes the value its
components agree on, refused naming the components where they don't. `delete_parameter` takes parameters away with their
values; whatever still names one is `Family::problems`'s to refuse. A file of 17 reads unchanged (every parameter the family's,
grouped by its kind); FORMAT 19 says an older build can't read the new kinds and fields (18 is another branch's number).

## Shared parameters file

A shared parameters file (`shared_parameters.rs`, the `shared_parameters` command, Manage > Shared Parameters) is a plain-text
file of the user's own, outside any project or family, as Revit keeps one: groups, and in them parameters, each a GUID, a name,
a data type, a description, whether it is visible and whether the user may change it. The command takes the file's `path` on
every call (Revit keeps it per user, never in the project; an agent says which file it means, and a front end remembers its
own), and only `load` changes the project. The layout is Revit's tab-separated one, from what is publicly documented and
commonly seen: `#` comments, a `*META`/`*GROUP`/`*PARAM` header naming the columns of the `META`/`GROUP`/`PARAM` lines after
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
user-modifiable values, the flag becomes a `Parameter` field and FORMAT 21 (20 is another branch's).

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
windows' opening lines are in Opening. The Family Editor's Symbols tab picks it from the category's subcategories.

`Model::component_marks` places a component's marks (mirrored, then turned and moved) and `Model::opening_marks` an opening's,
and `render::marks` draws them: lines in the category's pen (`BY_CATEGORY`, so Object Styles and Visibility/Graphics apply), dots
as filled discs. Coarse detail leaves them out. A label's anchor is placed with the component (turned, mirrored, moved) but its
letters never are: they are always drawn at 0 degrees in the view, left to right with a horizontal baseline, however the component
stands (the simplest rule; no flipping by quadrant). Symbols are annotation of the component's level: they draw in that level's
plan and its ceiling plan whatever height the component's forms reach (a ceiling light shows its symbols in the floor plan) and
on no other level's; the forms are outlined by the old rule (whole while any of them is between the view's depth and cut plane,
and not in ceiling plans). A component with no forms draws its symbols the same way. IFC export ignores symbols (they are plan notation, not geometry); the Family Editor's Symbols tab lists
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
