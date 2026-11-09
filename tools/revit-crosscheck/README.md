# The same numbers as Revit

A check that BIMCraft works out what Revit does: the small reference tower built in both, and each room's, wall's and
floor's quantities compared. Piece 10 of the [spatial core plan](../../docs/spatial-core-plan.md).

Revit can't run where BIMCraft is tested, so it's done in three steps: two here, one in Revit.

1. **Describe the tower** (any machine):

   ```sh
   bimcraft-cli run --cmd 'tower {"storeys": 2, "plate": "small"}' --cmd 'export_crosscheck {"path": "small-tower-2.json"}'
   ```

   The description holds plain numbers in millimetres: levels, wall and floor types by thickness, walls by their centre
   lines, floors, shafts, doors and windows (host, centre point, size), rooms by their points, and how rooms are measured.

2. **Build it in Revit** (Windows, Revit 2022 or later): open a new, empty metric project that has a door and a window
   family of your own loaded, set `DESCRIPTION` and `RESULTS` at the top of [`crosscheck.py`](crosscheck.py), and run it
   with pyRevit, RevitPythonShell or a Python node. It makes the levels, a one-layer type for each wall and floor
   thickness, the walls on their centre lines, the floors, the shafts, a type of your door and window families for each
   size, the doors and windows, and the rooms (each with BIMCraft's upper limit and offsets, each level with its
   computation height), with rooms measured as the description says;
   then writes Revit's numbers, `kind,id,quantity,value`, by BIMCraft's element ids.

3. **Compare** (any machine): commit the table as `crates/core/tests/revit/small-tower-2.csv` and take the `#[ignore]`
   off `crosscheck::tests::the_small_tower_has_the_numbers_revit_gives_it`; or, for a look first:

   ```sh
   bimcraft-cli run --cmd 'tower {"storeys": 2, "plate": "small"}' --cmd 'compare_crosscheck {"path": "small-tower-2.csv"}'
   ```

   It says how many quantities were compared, those that differ (ours and Revit's), and those only one side has. Each
   quantity has its tolerance (`crosscheck::tolerance`): a thousandth, or a square centimetre, a millimetre, a cubic
   decimetre.

What only Revit reports today (a wall's area and volume) is what BIMCraft has yet to work out; a
difference that comes from the two measuring a thing differently is written down in
[schedules](../../docs/areas/schedules.md), with the rule the test applies, never left to pass.

Clean-room: the script is BIMCraft's own, calls Revit's public API, and uses the families of the user's project; nothing
of Autodesk's is copied into this repository, and the table holds only numbers.
