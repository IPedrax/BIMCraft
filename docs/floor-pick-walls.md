# Which corner should a floor picked from walls use?

[Issue #301](https://github.com/IPedrax/BIMCraft/issues/301) asks to put a picked floor boundary on the core face. The premise for review is that two adjacent picks should meet where their selected longitudinal face references intersect. A wall can have a material seam at that junction whose position changes with butt order. A floor needs one shared vertex there.

## Reference and current behavior

| Workflow | Revit 2027 evidence | BIMCraft Rust at `b57b0a3f` |
| --- | --- | --- |
| Pick a boundary | Architectural Floor starts in Pick Walls. The core option makes the core the reference for edge offset. [Create a Floor](https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-A688CED0-E08C-4ED3-A8E8-14CBE78902BB.htm) | Floor opens a sketch. Selecting Pick Walls copies each wall's authored location line, including its arc. The plan and 3D handlers both call `sketch_click`. |
| Choose a side | Autodesk's floor exercise uses the flip control to pick the inside face with the core option cleared. [Terrain and Building Pad, floor steps 8–10](https://help.autodesk.com/cloudhelp/2027/ENU/Revit-GetStarted/files/GUID-E325CDE7-5468-43C8-BEAD-EC5AF13231F0.htm) | Picks on either side of the rectangle below produce the same four location lines. There is no per-pick core-face selector. |
| Complete the sketch | Finish requires a closed profile. [Create a Floor](https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-A688CED0-E08C-4ED3-A8E8-14CBE78902BB.htm) | Sketch lines stay in the draft until Finish. The existing `floor {sketch}` command validates the loop. Its `offset` parameter sets height above level. |

The native Revit 2027.3 (27.3.0.28) observation in [#273](https://github.com/IPedrax/BIMCraft/pull/273) measured two wall join orders in Fine plan. The floor vertices below are geometric expectations checked on synthetic BIMCraft Rust walls. A native floor pick on that corner is still needed to establish the Revit result.

## Reproduce the geometry

```sh
cargo test --locked -p bimcraft-ui-egui --lib pick_walls_closes_a_floor
cargo test --locked -p bimcraft-ui-egui --lib floor_core_picks_can_be_trimmed
cargo test --locked -p bimcraft-core --test compound_corner
```

[Rectangle fixture](../samples/floor-pick-walls.txt). Four clockwise walls enclose an 8,000 × 6,000 mm location-line rectangle. From exterior to interior the layers are 30 mm finish, 160 mm structure and 10 mm finish. The exterior lies to the left of each directed wall.

| Boundary | Lower left → upper right (mm) | Solved area (m²) |
| --- | --- | ---: |
| Desktop Location Line picks | (0, 0) → (8000, 6000) | 48.0000 |
| Joined exterior core faces, 70 mm out | (−70, −70) → (8070, 6070) | 49.9796 |
| Joined interior core faces, 90 mm in | (90, 90) → (7910, 5910) | 45.5124 |

Both core profiles close using `walls::layer_lines` and are accepted by `floor {sketch}`. Simply translating the four finite axes leaves their ends disconnected. Flipping only the west wall moves its exterior core reference from x = −70 to +70 mm and its interior core reference from x = +90 to −90 mm.

[L-corner fixture from #273](../samples/compound-corner.txt). At the origin H runs east and V turns south. H's core spans y = ±80 mm and V's spans x = ±60 mm.

| At the L (mm) | Outside | Inside |
| --- | --- | --- |
| Both joined face lines end at | (60, 80) | (−60, −80) |
| Untrimmed face endpoints, H / V | (0, 80) / (60, 0) | (0, −80) / (−60, 0) |
| Face references intersect, 25 mm farther to the selected side | (85, 105) | (−85, −105) |
| Ordinary cap after changing one finish material, H / V | (80, 80) / (60, 60) | (−80, −80) / (−60, −60) |

With H through, the material seam runs from (−60, −80) to (60, −80). The second endpoint is the elbow inside the stepped cap. Changing the through wall moves the seam, while the two face intersections stay put. Original #273 used element ID for butt order. Merged main uses the lower GUID. Reversing V and its layers preserves both turns but swaps V's named faces. With the ordinary cap the face endpoints are 28.28 mm apart. Intersecting the two reference lines still gives the proposed vertices.

The #273 butt requires two sufficiently long, straight, centre-location walls with matching vertical extents, fixed finish/structure/finish stacks and matching finishes on corresponding sides. Attachments, a nearby opening or another arm can change the cap. See the [wall-geometry limits](areas/levels-walls-roofs.md). Existing dimension references expose the wall centre, finish faces and ends. The automatic `floor {}` outline traces wall centre lines. Neither identifies an individually picked core face.

## Try it on the rectangle

1. Open the [rectangle fixture](../samples/floor-pick-walls.txt) as a project with `bimcraft-cli run --script samples/floor-pick-walls.txt --save rectangle.bimcraft`.
2. In plan, start Floor, then Pick Walls. The options bar defaults to Location Line. Choose **Core Face: Exterior** or **Core Face: Interior** and click each of the four walls. You can change the reference between picks. Edit Boundary on an existing floor offers the same choices.
3. Pick **Trim/Extend to Corner** and click two adjacent sketch lines near their middles, on the parts to keep. Repeat at the other three corners, then **Finish Edit Mode**. The clockwise fixture gives the exterior and interior footprints above. The desktop regression checks all four vertices, one model undo step at Finish, Undo/Redo and the saved and reopened boundary.

Scripts and MCP can request the same untrimmed sketch line with `sketch {"wall":100002,"face":"exterior-core"}`, substituting the picked wall's id or GUID. Use `interior-core` for the other face. Omitting `face` follows the authored location line, including its arc. The result's `line` is a standard sketch line. A core pick needs a straight wall type with one structural layer. Unsupported picks report an error. Moving a wall after Finish leaves the drawn floor boundary in place.

## Open native comparison

Measure the Revit 2027.3 compound L with core on and off, both sides, both butt orders and a nonzero edge offset. These cases inform a later automatic trim/extend decision. The draft uses explicit face selection, manual corner trims and zero edge offset. Curved core faces, absent or multiple structural layers and persistent wall associations remain open.
