# -*- coding: utf-8 -*-
"""Build a BIMCraft cross-check description in Revit and write what Revit reports of it.

Run inside Revit, in an open, empty metric project (pyRevit, RevitPythonShell or Revit's Python node all do), with
DESCRIPTION set to the JSON file `export_crosscheck` wrote and RESULTS to the table to write. The project's own door and
window families are used, one type of each size the description asks for; nothing of the template is copied out.

The table is `kind,id,quantity,value`, the ids those of the description (BIMCraft's element ids), in square metres,
metres and cubic metres:

    room   enclosed, area_m2, perimeter_m, volume_m3
    wall   length_m, openings, area_m2, volume_m3
    floor  area_m2, volume_m3

Written for Revit 2022 and later (Floor.Create). An original script of BIMCraft's, under the project's licence; it calls
Revit's public API and holds nothing of Autodesk's.
"""

import io
import json
import math

from Autodesk.Revit.DB import (
    AreaVolumeSettings,
    Arc,
    BuiltInParameter,
    CompoundStructure,
    CompoundStructureLayer,
    CurveArray,
    CurveLoop,
    ElementId,
    FilteredElementCollector,
    Floor,
    FloorType,
    FamilySymbol,
    BuiltInCategory,
    Level,
    Line,
    MaterialFunctionAssignment,
    SpatialElementBoundaryLocation,
    SpatialElementType,
    Transaction,
    UV,
    Wall,
    WallType,
    XYZ,
)
from Autodesk.Revit.DB.Structure import StructuralType

DESCRIPTION = r"C:\crosscheck\small-tower-2.json"
RESULTS = r"C:\crosscheck\small-tower-2.csv"

MM = 1.0 / 304.8  # feet in a millimetre: Revit works in feet
M2 = 0.09290304  # square metres in a square foot
M3 = 0.028316846592  # cubic metres in a cubic foot
M = 0.3048  # metres in a foot


def xyz(p, z=0.0):
    return XYZ(p[0] * MM, p[1] * MM, z * MM)


def loop(points):
    """A closed CurveLoop through points in mm."""
    c = CurveLoop()
    for i in range(len(points)):
        a, b = points[i], points[(i + 1) % len(points)]
        if math.hypot(b[0] - a[0], b[1] - a[1]) > 0.5:
            c.Append(Line.CreateBound(xyz(a), xyz(b)))
    return c


def curves(points):
    """A closed CurveArray through points in mm."""
    c = CurveArray()
    for i in range(len(points)):
        a, b = points[i], points[(i + 1) % len(points)]
        if math.hypot(b[0] - a[0], b[1] - a[1]) > 0.5:
            c.Append(Line.CreateBound(xyz(a), xyz(b)))
    return c


def put(e, bip, value):
    """Set `e`'s built-in parameter `bip` where it has it and it can be set."""
    p = e.get_Parameter(bip)
    if p is not None and not p.IsReadOnly:
        p.Set(value)


def one_layer(doc, base, name, thickness):
    """A type duplicated from `base`, one layer `thickness` mm thick (its structure, so every face is a finish face)."""
    t = base.Duplicate(name)
    layer = CompoundStructureLayer(thickness * MM, MaterialFunctionAssignment.Structure, ElementId.InvalidElementId)
    t.SetCompoundStructure(CompoundStructure.CreateSimpleCompoundStructure([layer]))
    return t


def sized(doc, symbols, width, height, made):
    """A type of the project's door or window family `width` by `height` mm, made once."""
    key = (symbols[0].Family.Name, round(width), round(height))
    if key in made:
        return made[key]
    t = symbols[0].Duplicate("BIMCraft %d x %d" % (round(width), round(height)))
    put(t, BuiltInParameter.FAMILY_WIDTH_PARAM, width * MM)
    put(t, BuiltInParameter.FAMILY_HEIGHT_PARAM, height * MM)
    if not t.IsActive:
        t.Activate()
    made[key] = t
    return t


def build(doc, d):
    """The description `d` built in `doc`: each element by its BIMCraft id."""
    settings = AreaVolumeSettings.GetAreaVolumeSettings(doc)
    settings.ComputeVolumes = True
    where = SpatialElementBoundaryLocation.Finish if d["room_area"] == "finish" else SpatialElementBoundaryLocation.Center
    settings.SetSpatialElementBoundaryLocation(where, SpatialElementType.Room)

    levels = {}
    for l in sorted(d["levels"], key=lambda l: l["elevation"]):
        lv = Level.Create(doc, l["elevation"] * MM)
        lv.Name = "BIMCraft " + l["name"]
        put(lv, BuiltInParameter.LEVEL_ROOM_COMPUTATION_HEIGHT, d["computation_height"] * MM)
        levels[l["guid"]] = lv

    base_wall = FilteredElementCollector(doc).OfClass(WallType).FirstElement()
    wall_types = {t["name"]: one_layer(doc, base_wall, "BIMCraft " + t["name"], t["thickness"]) for t in d["wall_types"]}
    base_floor = FilteredElementCollector(doc).OfClass(FloorType).FirstElement()
    floor_types = {t["name"]: one_layer(doc, base_floor, "BIMCraft " + t["name"], t["thickness"]) for t in d["floor_types"]}

    walls, floors, rooms = {}, {}, {}
    for w in d["walls"]:
        lv = levels[w["level"]]
        if w.get("mid"):
            curve = Arc.Create(xyz(w["start"], lv.Elevation / MM), xyz(w["end"], lv.Elevation / MM), xyz(w["mid"], lv.Elevation / MM))
        else:
            curve = Line.CreateBound(xyz(w["start"], lv.Elevation / MM), xyz(w["end"], lv.Elevation / MM))
        wall = Wall.Create(doc, curve, wall_types[w["type"]].Id, lv.Id, w["height"] * MM, w["base_offset"] * MM, False, False)
        put(wall, BuiltInParameter.WALL_KEY_REF_PARAM, 0)  # the location line: the wall's centre line
        if w.get("top_level"):
            put(wall, BuiltInParameter.WALL_HEIGHT_TYPE, levels[w["top_level"]].Id)
            put(wall, BuiltInParameter.WALL_TOP_OFFSET, 0.0)
        walls[w["guid"]] = (w["id"], wall)

    for f in d["floors"]:
        loops = [loop(f["boundary"])] + [loop(o) for o in f["openings"]]
        floor = Floor.Create(doc, loops, floor_types[f["type"]].Id, levels[f["level"]].Id)
        put(floor, BuiltInParameter.FLOOR_HEIGHTABOVELEVEL_PARAM, f["offset"] * MM)
        floors[f["guid"]] = (f["id"], floor)

    for s in d["shafts"]:
        bottom = levels[s["level"]]
        top = levels.get(s.get("top_level") or "")
        opening = doc.Create.NewOpening(bottom, top or bottom, curves(s["boundary"]))
        put(opening, BuiltInParameter.WALL_BASE_OFFSET, s["base_offset"] * MM)
        if top is None:
            put(opening, BuiltInParameter.WALL_HEIGHT_TYPE, ElementId.InvalidElementId)
            put(opening, BuiltInParameter.WALL_USER_HEIGHT_PARAM, s["height"] * MM)

    families = {
        "door": list(FilteredElementCollector(doc).OfCategory(BuiltInCategory.OST_Doors).OfClass(FamilySymbol)),
        "window": list(FilteredElementCollector(doc).OfCategory(BuiltInCategory.OST_Windows).OfClass(FamilySymbol)),
    }
    made = {}
    for o in d["openings"]:
        if not families[o["kind"]]:
            raise Exception("the project has no %s family loaded: load one of your own" % o["kind"])
        _, host = walls[o["host"]]
        lv = doc.GetElement(host.LevelId)
        t = sized(doc, families[o["kind"]], o["width"], o["height"], made)
        inst = doc.Create.NewFamilyInstance(xyz(o["at"], lv.Elevation / MM), t, host, lv, StructuralType.NonStructural)
        put(inst, BuiltInParameter.INSTANCE_SILL_HEIGHT_PARAM, o["sill"] * MM)

    # A room reaches up to the next level, so its volume is its storey's.
    order = [l["guid"] for l in sorted(d["levels"], key=lambda l: l["elevation"])]
    above = dict(zip(order, order[1:]))
    for r in d["rooms"]:
        room = doc.Create.NewRoom(levels[r["level"]], UV(r["point"][0] * MM, r["point"][1] * MM))
        room.Name = r["name"]
        room.Number = r["number"]
        if r["level"] in above:
            put(room, BuiltInParameter.ROOM_UPPER_LEVEL, levels[above[r["level"]]].Id)
            put(room, BuiltInParameter.ROOM_UPPER_OFFSET, 0.0)
        rooms[r["guid"]] = (r["id"], room)
    return walls, floors, rooms


def number(e, bip, unit):
    p = e.get_Parameter(bip)
    return p.AsDouble() * unit if p is not None and p.HasValue else None


def report(doc, d, walls, floors, rooms):
    hosted = {}
    for o in d["openings"]:
        hosted[o["host"]] = hosted.get(o["host"], 0) + 1
    rows = []
    for guid, (i, room) in rooms.items():
        enclosed = room.Area > 0
        rows.append(("room", i, "enclosed", 1 if enclosed else 0))
        rows.append(("room", i, "area_m2", room.Area * M2))
        rows.append(("room", i, "perimeter_m", room.Perimeter * M if enclosed else 0.0))
        rows.append(("room", i, "volume_m3", room.Volume * M3))
    for guid, (i, wall) in walls.items():
        rows.append(("wall", i, "length_m", wall.Location.Curve.Length * M))
        rows.append(("wall", i, "openings", hosted.get(guid, 0)))
        for q, bip, unit in (("area_m2", BuiltInParameter.HOST_AREA_COMPUTED, M2), ("volume_m3", BuiltInParameter.HOST_VOLUME_COMPUTED, M3)):
            v = number(wall, bip, unit)
            if v is not None:
                rows.append(("wall", i, q, v))
    for guid, (i, floor) in floors.items():
        for q, bip, unit in (("area_m2", BuiltInParameter.HOST_AREA_COMPUTED, M2), ("volume_m3", BuiltInParameter.HOST_VOLUME_COMPUTED, M3)):
            v = number(floor, bip, unit)
            if v is not None:
                rows.append(("floor", i, q, v))
    return rows


def main(doc):
    with io.open(DESCRIPTION, encoding="utf-8") as f:
        d = json.load(f)
    if d.get("format") != "bimcraft-crosscheck" or d.get("version") != 1 or d.get("units") != "mm":
        raise Exception("not a BIMCraft cross-check description of version 1")
    t = Transaction(doc, "BIMCraft cross-check")
    t.Start()
    built = build(doc, d)
    t.Commit()  # Revit works out rooms, joins and quantities as the transaction ends
    doc.Regenerate()
    rows = report(doc, d, *built)
    with io.open(RESULTS, "w", encoding="utf-8", newline="\n") as f:
        f.write(u"kind,id,quantity,value\n")
        for kind, i, q, v in rows:
            f.write(u"%s,%d,%s,%r\n" % (kind, i, q, float(v)))
    print("wrote %d rows to %s" % (len(rows), RESULTS))


main(__revit__.ActiveUIDocument.Document)  # noqa: F821 (the Revit Python shells define it)
