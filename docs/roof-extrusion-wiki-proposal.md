# Wiki update proposed with extrusion roof Edit Profile

Apply these independently written additions when the feature is merged. This PR does not publish unreleased behavior
to the live wiki. Existing instructions for footprint roofs and other sketches remain unchanged.

## Roofs-and-Dormers.md — add under “Changing a roof”

### Editing an extrusion profile

Select a roof by extrusion and click **Edit Profile** in its contextual tools. The app opens the 3D view facing the
roof's upright reference plane. Pick a profile line, then drag an endpoint or edit **Start/End Along** and **Start/End
Height** in Properties. Along measures from the beginning of the roof's reference line; Height measures above the
roof's level plus offset. Lengths use the project's units. Joined endpoints move together.

Use Line or Polyline to draw straight segments; existing Split, Trim/Extend and Offset tools can revise them.
Keep one open chain, progressing along the reference line. Curved profiles, vertical segments, branches and closed
loops are unsupported. Snaps use the draft profile's own coordinates and geometry.

**Finish Edit Mode** applies the draft as one Undo step. If the profile is invalid, the draft stays open and the roof
is unchanged. **Cancel Edit Mode** discards the draft. After finishing, **Extrusion Start** and **Extrusion End** in
Properties change the sweep limits independently. Change pitch by editing the profile's rise and run; the footprint
Slope field does not apply to extrusion roofs.

Agents use the existing command, for example:

```text
modify {"id":100002,"extrusion":{"profile":[[0,0],[6000,3000]]}}
```

Use the actual roof ID. The partial update retains its reference line, sweep limits, level, offset and type.

## Sketch-Mode.md — add after “Starting and finishing”

**Extrusion roofs use an open profile.** Select one and choose **Edit Profile**. Its draft is edited in the 3D view on
the roof's upright reference plane, with straight Line/Polyline geometry, endpoint grips and numeric Along/Height
coordinates. Unlike footprint and floor boundaries, it must stay open and progress along the reference line. Finish
and Cancel have the same commit/discard behavior. Curved profile geometry and general sketch Move/Copy/Mirror are
not part of this editor.
