# Coordinates: research

What Revit does with coordinates, what its users complain about, where BIMCraft stands, and where we can do better.
This write-up comes before the code. Its proposal is for the owner to decide on (see "Open questions" at the end).

## How Revit does it

### Three points

Revit has three points, and most of the confusion starts there:

- **Internal Origin.** The fixed origin of the file's own coordinates, where all geometry is stored. It has been visible
  since 2020.2, and it can't be moved. Accuracy and graphics get worse more than about 20 miles (32 km) from it. Revit
  warns about this when imports or links lie that far out, and it refuses some geometry beyond the limit.
- **Project Base Point (PBP).** A reference that people pick, usually a grid intersection or a corner of the building.
  Spot coordinates and level elevations can be reported from it. Moving it does nothing to the geometry. Before 2020.2,
  moving it while "clipped" moved the whole project's coordinates, and moving it "unclipped" moved only the point.
  Since 2020.2 it no longer clips: dragging it moves the reported numbers, and the "Move to Internal Origin" command was
  added.
- **Survey Point (SP).** A real-world reference: a benchmark, a geodetic marker, or the origin of the site's map grid. It
  is the visible marker of the hidden **Shared Site Origin**, the origin of the *shared coordinate system*.
  - Clipped (the default): moving it moves the Shared Site Origin, which changes every shared coordinate in the project.
  - Unclipped: it moves alone, and its own coordinates then read as an offset from the Shared Site Origin, which nobody
    can see.

  The "Survey Point" option of spot coordinates, spot elevations and a level's *Elevation Base* actually measures from
  the Shared Site Origin, not from the marker ([BIM Pure][bimpure]).

### Shared coordinates and Sites (Locations)

- A project's shared coordinates relate its internal coordinates to the shared system: an east/north offset, an
  elevation, and an *angle to True North*.
- A file can keep several named **Sites** (Manage > Location > Site). They are positions of the same building in the
  shared system, for example the same house placed three times on an estate. A link instance remembers which named
  position of the linked file it uses.
- The tools (Manage > Coordinates):
  - **Acquire Coordinates**: take the shared system from a link (a Revit model or a DWG). The usual way to georeference
    is to link the civil DWG and acquire from it; a DWG with a coordinate system also brings its GIS system (Revit 2018
    and later).
  - **Publish Coordinates**: push this host's shared system *into* a link, as a new or changed named position. The link
    has to be saved afterwards. Autodesk's help says this can't be done with cloud-collaborated links.
  - **Specify Coordinates at a Point**: type the N/E/elevation and angle of a point you pick.
  - **Report Shared Coordinates**: pick a point and read its shared coordinates.
  - **Reset Shared Coordinates** (2019 and later): back to the internal system.
  - **Reconcile**: when a link was moved in its own file, accept or reject the change.
- **Linking positions:** Auto, Origin to Origin; Auto, Center to Center; Auto, By Shared Coordinates; and Manual, by
  placing it. Revit links have one more: Project Base Point to Project Base Point.
- **Pinning** the Survey Point disables Rotate True North, Acquire Coordinates and Specify Coordinates
  ([Autodesk help][adsk-sp]).

### North

- **Project North** points up on the sheet. **True North** comes from the shared system's angle. Each plan view has an
  *Orientation* parameter set to either one.
- **Rotate True North** changes the angle. **Rotate Project North** turns the whole internal model, which really moves
  the geometry, so that a building drawn askew can sit square on the sheet.

### Elevations

- A level's type has an *Elevation Base*: Project Base Point or Survey Point.
- Spot elevations can measure from Relative (a level), Project Base Point or Survey Point. Spot coordinates choose
  their origin, and show N/E, E/N, N only or E only.
- The Survey Point's "Elev" is the elevation of the shared system: the height of the internal origin above datum.

### Geographic location and GIS

- **Location** (Manage > Location) holds a city or latitude/longitude and a time zone. It is used for sun studies and
  energy analysis, and it is independent of the shared coordinates.
- A real coordinate reference system (an EPSG code) comes in only by acquiring from a Civil 3D DWG that has one.
  Revit 2025 added horizontal coordinate systems for civil workflows ([Civil Survey Solutions][cs2025]).

### IFC

- The exporter's *Coordinate Base* option: Internal Coordinates, Project Base Point, Survey Point, Shared Coordinates,
  and so on (six choices in recent versions).
- In IFC4 it can write `IfcMapConversion` and `IfcProjectedCRS`. Many of the georeferencing complaints are here.

## What people complain about

From Autodesk's forums, the Revit IFC GitHub issues, and consultancy and blog posts:

1. **Too many points, and they don't say what they are.** Internal Origin, PBP, Survey Point and a hidden Shared Site
   Origin, plus clipped and unclipped, which changed meaning in 2020.2. A typical thread: both points read 0,0 and still
   don't line up with the origin ([forum][f-00]). Firms write internal "coordinates SOPs" because people get it wrong.
2. **Moving something you didn't mean to.** Dragging a clipped Survey Point quietly moves the shared system for the
   whole project and, once published, for its links. There is no history and no warning, and the fix is "Reset and
   re-acquire" ([IMAGINiT][imaginit], [forum][f-shared]).
3. **Publish is fragile.** It writes a position into another file, which has to be saved, mustn't be open, and can't be
   in the cloud. Forgetting to save loses the transform. Publishing from a CAD-linked site makes trouble
   ([forum][f-shared]).
4. **Unclipped Survey Point traps.** Its coordinates become an offset from a point nobody can see, and spot coordinates
   don't measure from it ([BIM Pure][bimpure]).
5. **The 20-mile limit.** Survey coordinates (UTM, State Plane) are hundreds of km out, so people have to keep geometry
   at the internal origin and carry the offset, and DWGs and point clouds at true coordinates trigger the warning
   ([forum][f-20mi]).
6. **No grid-to-ground scale factor.** State Plane and UTM coordinates need a combined scale factor. Revit has none, so
   points drift by centimetres over a large site. US Survey Feet weren't supported as a unit, and the point cloud scale
   factor is truncated to six decimals ([forum][f-20mi]).
7. **IFC georeferencing is unreliable.**
   - Rooms exported "shared" sit high above the rest by the project elevation ([revit-ifc #907][gh907]).
   - The IfcSite elevation option was broken from 2019 to 2023 ([forum][f-site]).
   - True North gets applied twice (once in placement, once through `TrueNorth`/`IfcMapConversion`).
   - Openings come in at 0,0,0 when a georeferenced IFC is imported.
   - Latitude and longitude are missing even when georeferenced ([BIM me UP!][bimmeup]).
   - Viewers place the same file differently, local against global.
   - A Revit export relinked into Revit is displaced by the Survey Point ([forum][f-ifcimp]).
8. **IFC import offers no control** over which point an IFC's local origin goes to (Internal Origin, PBP or SP)
   ([Revit Idea][idea-ifc]).
9. **Rotate Project North really moves the model.** Done late, it breaks links, dimensions and views.
10. **Location (lat/long) and shared coordinates are two unrelated settings** that can disagree: sun studies at one
    place, the survey at another.
11. **Workshared friction.** Changing coordinates borrows the Shared Site element (Project Information), which blocks
    or is blocked by other users.

## Where BIMCraft stands (main at 5a283703)

- Millimetres; x east, y north, z up. There is one origin, the file's, and nothing else: no base point, survey point,
  north, shared system or location.
- **IFC export** (`ifc.rs` ~778): `IfcSite` and `IfcBuilding` sit at 0,0,0 with no `RefLatitude`, no `TrueNorth`,
  and no `IfcMapConversion` or `IfcProjectedCRS`.
- **IFC import** (`import.rs` ~1464): a model far from the origin (beyond 1e8 mm = 100 km) is shifted to near it, and
  `Imported::moved` reports by how much. That shift is then thrown away: it isn't stored in the model, so exporting
  again loses it. `IfcMapConversion` isn't read.
- **Links** (`Link { at, angle }`): placed by hand, origin to origin plus a move and a turn. There is nothing like "by
  shared coordinates".
- **CAD import** and **point clouds** (`cad.rs`, `cloud.rs`): read far coordinates, and the cloud keeps its own
  `origin` in f64 and points in f32 from there. Neither relates to a project system.
- **Spot elevations** (`Spot`, `spot_value`) measure from the model's z = 0. There are no spot coordinates.
- **Levels** have one elevation, from 0.
- **The 3D view and the canvas** use f64 in the model and f32 on the GPU. Big coordinates would cost precision on the
  card, which is the same reason behind Revit's 20-mile limit.

This is a chance as much as a gap: nothing is built on the confusing parts yet.

## Where we can do better

Ideas, roughly from most to least worth it:

1. **Fewer, clearer points.** Keep Revit's words, so its users find them, but give them one meaning each:
   - *Internal Origin*: fixed, where geometry is stored. It is never far from the building, and we never ask the user
     to manage it.
   - *Project Base Point*: a named reference that reports, and nothing more. Moving it never moves geometry, and there is
     no clipped mode.
   - *Survey Point*: the marker **is** the shared origin. There is no hidden second point and no unclipped mode. A
     benchmark somewhere else is just a spot coordinate or an annotation.

   The transform from internal to shared coordinates is shown as plain numbers in one place (E, N, elevation, angle to
   True North, scale), and it is editable by command.
2. **Coordinates as data, not as a drag.** Changing the shared system is a command (`coordinates`, say) with explicit
   values or "acquire from link X". Moving the Survey Point by grip goes through that same command, asks first (or is
   locked by default), and is one undo step with a readable description. Each change records who, when and from what
   to what (a short history on the model), so "who moved our coordinates?" has an answer.
3. **No Publish into other files.** Instead of writing into a link, a link is placed **by shared coordinates** and stays
   there: its transform is computed from both projects' shared systems each time it loads. Named positions (Revit's
   Sites) live in the *host* as the link instance's own choice. A link whose shared system changed is reported on
   reload, with "keep where it was" or "follow its coordinates" (Reconcile, without writing to the other file).
4. **Big coordinates handled for the user.** Anything coming in far away (IFC with `IfcMapConversion` or large
   placements, DXF, point clouds) is shifted near the origin **and the shift is stored as the shared system**, so it
   round-trips. That is what `import.rs` already half does. The user never sees a "20 miles" warning, because geometry
   is always stored locally.
5. **A real CRS and a scale factor.** Store an optional EPSG code or name plus a map conversion: eastings, northings,
   orthogonal height, rotation and **scale** (grid to ground), exactly IFC4's `IfcMapConversion` and `IfcProjectedCRS`.
   Use the scale in shared coordinates and in reporting. Revit can't do this, and surveyors ask for it.
6. **One location.** Latitude and longitude are derived from the CRS when there is one, and typed when there isn't.
   Sun and true north then can't disagree with the survey.
7. **Correct IFC georeferencing, tested through the file.**
   - Export: write `IfcMapConversion` and `IfcProjectedCRS` when there is a CRS. Otherwise write `IfcSite.RefLatitude`
     and `RefLongitude` and `RefElevation`, plus a `TrueNorth` that agrees with the conversion, and never double-rotate.
     One option chooses where the file's local origin sits: Internal, Project Base Point or Survey Point.
   - Import: read `IfcMapConversion` (with an IFC2x3 `ePSet_MapConversion` fallback) into the shared system.
   - The checks in the tests: IfcOpenShell's georeference reading agrees, and an export imported again round-trips the
     same numbers.
8. **Project North without moving the model.** The angle between Project and True North is a property of the shared
   system, and a plan's *Orientation* (Project or True North) only turns the view. "Rotate Project North" turns the
   view's up, not the geometry, so nothing that depends on the geometry breaks.
9. **Spot coordinates and elevations that say what they measure from.** The origin is a parameter of the spot's *type*
   (internal, PBP, survey), and its text can carry a prefix ("N", "E", "EL"). Levels get an Elevation Base on their
   type the same way.
10. **Agents first.** Every coordinate fact is in `info` and the MCP output: the transform, the CRS, each point's
    internal and shared position, and "where is element X in shared coordinates". A command converts points both ways.
    Revit only offers this through its API.
11. **Visible everywhere.** The three markers can be shown in plans, sections and the 3D view (Revit's are plan-only and
    don't print), with a True North arrow that is an annotation reading the shared angle.

## A first cut of the model (for discussion)

```rust
/// Model::coordinates
pub struct Coordinates {
    /// Project Base Point, internal mm: where it stands.
    pub base: P3,
    /// The shared system: the Survey Point is its origin, in internal mm, and the shared
    /// x axis is turned `angle` degrees from internal x (counter-clockwise), True North.
    pub survey: P3,
    pub angle: f64,
    /// Shared coordinates of the Survey Point: E, N, elevation, mm (often 0, 0, 0; large for a map grid).
    pub shared: [f64; 3],
    /// Grid-to-ground: shared = internal * scale. 1 unless a CRS says otherwise.
    pub scale: f64,
    /// A map's coordinate reference system, e.g. "EPSG:31983", when georeferenced.
    pub crs: Option<String>,
    /// Latitude and longitude, degrees, and how high it is; derived from `crs` when there is one.
    pub location: Option<Location>,
    /// Who changed it, when, from what: a short history.
    pub history: Vec<CoordinateChange>,
}
```

The library entries and elements stay as they are. Placement in a link gains `by: Origin | Shared | Manual`. Spot
types gain `from: Internal | Base | Survey`, as do level types. All of this sits under the GUID standard, and the file
format bumps with a `migrate` step (everything at 0, angle 0, scale 1).

Shared coordinates are f64 throughout, so a UTM northing of ~7,500,000 m (7.5e9 mm) keeps sub-micrometre precision.
Only geometry has to stay near the internal origin (f32 on the card), which point 4 guarantees.

## Stages (a sketch)

1. **Core model and commands.** `Coordinates` on the model; the commands `coordinates` (set and report),
   `specify_coordinates`, `report_coordinates` (points both ways) and `reset_coordinates`; `Model::check` rules (finite,
   in range, scale in (0.9, 1.1)); migrate. Tests through commands.
2. **IFC.** Export the local origin choice, `IfcSite` references, `TrueNorth`, `IfcMapConversion` and
   `IfcProjectedCRS`. Import reads them and keeps the far shift as the shared system. Round-trip and IfcOpenShell checks.
3. **Spots and levels.** Spot coordinate tool and types, spot elevation and level Elevation Base.
4. **Links by shared coordinates.** Placement by shared system, named positions in the host, reconcile on reload.
   Acquire from a link, a DXF or an IFC.
5. **Desktop.** Markers in views (with VG category, hidden by default as Revit's), grips that ask, Manage > Coordinates
   menu, Location dialog, view Orientation (Project or True North) with a north arrow annotation.
6. **CAD and point clouds** placed by shared coordinates; DXF's `$INSBASE` and geo data where given.

## Open questions for the owner

- Do we keep Revit's **clipped/unclipped** at all, for people who expect it, or drop it as above (Survey Point is the
  shared origin, full stop)?
- **Named positions (Sites):** needed now, or later with links?
- **CRS:** store only the EPSG code and conversion numbers (IFC's model), or also compute lat/long from a projection?
  The latter needs a projection library (proj4rs, pure Rust) or our own UTM and transverse Mercator, which is enough for
  SIRGAS 2000 / UTM and most of the world.
- Which **IFC local origin** is the default on export: Survey Point (agrees with other trades) or Project Base Point
  (Revit's habit)?
- **Brazil first?** SIRGAS 2000 UTM zones (EPSG 31978–31985) as the starter list, with NBR 14166 (local topographic
  system) as a later option.

## Sources

- [Understanding Revit's Mysterious Survey Point (BIM Pure)][bimpure]
- [Autodesk help: Survey Point (Revit 2025)][adsk-sp]
- [Autodesk Community: Survey point and base point both at 0,0 but not at the origin][f-00]
- [Autodesk Community: PBP and SP vs Internal Origin best practice](https://forums.autodesk.com/t5/revit-architecture-forum/project-base-point-and-survey-point-vs-internal-origin-best/td-p/12489735)
- [Autodesk Community: Shared coordinates not transferring][f-shared]
- [Autodesk Community: State Plane coordinate point clouds (scale factor, 20 miles)][f-20mi]
- [Autodesk Community: Wrong site elevation in IFC export][f-site]
- [Autodesk Community: IFC import not aligning to Survey Point][f-ifcimp]
- [Revit Idea: IFC import using Project Base Point, not Revit origin][idea-ifc]
- [revit-ifc issue #907: rooms high above the model with shared coordinates][gh907]
- [BIM me UP!: Revit, IFC and coordinate systems][bimmeup]
- [IMAGINiT: Shared Coordinate Best Practices, part 3][imaginit]
- [What's New in Revit 2025 (Civil Survey Solutions)][cs2025]
- [Modelical: Coordinates in Revit](https://www.modelical.com/en/coordinates-in-revit/)
- [Graphisoft: Archicad's Survey Point](https://helpcenter.graphisoft.com/?p=63066)
- [Esri: geolocating CAD and BIM from the start](https://www.esri.com/arcgis-blog/products/arcgis-pro/data-management/geolocation-autodesk-autocadcivil-3drevit-and-arcgis-pro)
- [Pix4D community: IFC from Revit, IfcMapConversion](https://community.pix4d.com/t/export-ifc-from-revit-geolocation/29413)

[bimpure]: https://www.bimpure.com/blog/understanding-revits-mysterious-survey-point
[adsk-sp]: https://help.autodesk.com/view/RVT/2025/ENU/?guid=GUID-260A130C-4F8C-4C05-80DF-018BF2A2DA29
[f-00]: https://forums.autodesk.com/t5/revit-architecture-forum/survey-point-and-base-point-both-at-0-0-but-they-do-not-line-up/td-p/10935193
[f-shared]: https://forums.autodesk.com/t5/revit-architecture-forum/shared-coordinates-not-transferring-in-shared-survey-point/td-p/9009089
[f-20mi]: https://forums.autodesk.com/t5/revit-architecture-forum/state-plane-coord-point-clouds/m-p/6000589
[f-site]: https://forums.autodesk.com/t5/revit-structure-forum/wrong-site-elevation-in-ifc-export/m-p/11202114
[f-ifcimp]: https://forums.autodesk.com/t5/revit-architecture-forum/ifc-import-not-aligning-to-survey-point/m-p/12888234
[idea-ifc]: https://forums.autodesk.com/t5/revit-ideas/ifc-import-using-project-base-point-not-revit-origin/idi-p/6975115
[gh907]: https://github.com/Autodesk/revit-ifc/issues/907
[bimmeup]: https://bim-me-up.com/en/revit-ifc-and-coordinate-systems/
[imaginit]: https://resources.imaginit.com/how-tos/shared-coordinate-best-practices-part-3
[cs2025]: https://civilsurveysolutions.com.au/blog/whats-new-in-revit-2025/
