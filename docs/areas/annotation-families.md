# Annotation families

A family of category `annotation` is two-dimensional: lines, arcs, dots, fills, text and labels in its
own millimetres. In a project it is placed as an `Annotation` element on a sheet or on a flat view: a
level plan, a ceiling plan, a section or an elevation. It is absent from {3D}, IFC and GLB, and
schedules do not see it.

## `scale_with_view`

The `scale_with_view` flag is a family field, on by default. It sets what a family unit means:

| `scale_with_view` | Family unit | A 1×1 square in a 1:100 plan | On 1:100 paper | On a sheet |
|---|---|---|---|---|
| `true` | 1 mm on paper | 100×100 mm in the model | 1×1 mm | 1×1 mm |
| `false` | 1 mm in the model | 1×1 mm in the model | 0.01×0.01 mm | 1×1 mm |

A sheet has no model scale, so both settings give one family unit = one paper millimetre. Changing a
view's scale redraws its instances: with `true` their paper size stays the same, with `false` their
model size does.

## Commands

```text
family {"name":"Table","category":"annotation",
        "parameters":[{"name":"Width","kind":"length","value":120},
                      {"name":"Height","kind":"length","value":48},
                      {"name":"C1","kind":"text","instance":true,"value":""}],
        "types":{"Standard":{}},
        "symbols":[{"fill":{"rings":[[[0,0],["Width",0],["Width","Height"],[0,"Height"]]],"type":"Masking"}},
                   {"line":[[0,0],[0,"Height"]]},
                   {"text":{"at":[2,"Height / 2"],"height":2.5,"value":"{C1}"}}]}

annotation {"family":"Table","type":"Standard","sheet":"A101","at":[20,220],
            "params":{"C1":"1  Screw 8×220"}}

annotation {"family":"Table","type":"Standard","view":"Level 1","at":[0,0]}
```

`annotation` takes exactly one of `sheet` and `view`. `at` is in the view's own terms: paper
millimetres on a sheet, plan millimetres in a plan, along the line and up in a section or an
elevation. `modify` changes `at`, `angle`, `mirrored`, `type` and `params`; `move`, `rotate`,
`mirror`, `delete` and copy/paste work as they do for `text` in the same view.

## Symbols

- `line`, `arc` — the project's line style in `style` (`Thin Lines` by default); a circle is an `arc`
  from 0 to 360°.
- `fill` — closed `rings` (a ring inside a ring is a hole), the project's fill type in `type`
  (`Masking`, `Solid Black`, a hatch). A mask hides the model under it, not other annotations.
- `text` — a `{Parameter}` label; `text_type` is the project's text type, `valign` (`top`, `middle`,
  `bottom`), `wrap` is the wrap width in family units. Without `text_type` the family's stroke
  lettering is used, as before.
- `dot` — a filled dot.

## Family editor

`File > New > Family > Generic Annotation` opens a paper plan: an annotation family has no {3D},
ceiling or elevation tabs. The plan's frame is `Width × Height` (100 × 50 mm by default), so a line
across the canvas stays a line in paper millimetres, not in model metres. **Scale with View** is in
Family Category and Parameters. Forms, connectors and adaptive tools are unavailable for an
annotation family: it is two-dimensional.

The category is called **Generic Annotation** everywhere: New Family, the Project Browser and the
family document's title.

In Annotate:

- **Line** — click its points in the plan; Enter or a double-click ends it.
- **Arc** — the first click sets the start, the second the end, the third a point the arc passes
  through; the centre and radius are computed. The start, middle and end are grips: drag one and the
  arc is rebuilt through the other two.
- **Text** — a click places plain text; its content, height and type are edited in Properties.
- **Label** — a click places a label reading the family's first parameter (`{Width}`) until
  Properties name another.
- **Dot** — a click places a dot; it is drawn as a disc (1 mm radius by default).
- **Filled Region** — click the contour's corners; the first corner again or Enter closes the fill.
- **Masking Region** — the same with the `Masking` type: it hides the model under it.
- A filled region's boundary is invisible in a project (as in Revit); the family editor shows it as
  a dashed hint. That line never reaches the family, a sheet or the project.

Drawing uses the same snaps as the project: endpoints, midpoints and intersections of symbol and form
lines, reference planes and lines, guides from the previous point, Shift and the snap marker. The
family editor has its own **Manage > Snaps**: object snaps, angles, alignment guides, tolerance and
snapping are configured for this family and are not overridden by the project's settings after the
first change.

**Guides from grids** is about structural grids only, not a coordinate grid. The snap engine
otherwise rounds a free point to 10 mm, which is far too coarse in a family, so its Snaps has a
separate **Snap to coordinates** with a **Step** in millimetres. The step is **1 mm** by default and
the switch is off: with nothing to snap to, a point follows the cursor exactly. Turn it on to round
free points to the chosen step.

Symbols in the plan are selected by clicking and moved by dragging. The selected symbol shows its
grips: every point of a line, every corner of each ring of a region (both Filled and Masking), and
the start, middle and end of an arc. Drag a grip to change only it, or the symbol as a whole to move
all its grips. The selected symbol's Properties are shown in Properties. A label naming a parameter
the family does not have no longer blanks the view: it draws `{Parameter}` as a placeholder, and
Properties shows a red message listing the parameters that exist, under the Text field.

**Move, Copy, Rotate, Mirror, Measure and Create Similar** in this branch are PR #598's tools; there
are no copies of them here. Annotation symbols take part in its selection (click, box, highlight),
and Move/Copy carry the rings of a Filled or Masking Region too. Rotate, Mirror and Scale of symbols
behave as #598 defines them; there are no annotation-specific variants of those tools.

## Limitations

- The number of rows and cells of a table is fixed by the template; there are no arrays or
  repeaters.
- `title_block` remains its own mechanism; an annotation family does not replace it.
- Sheet fields in labels (`SHEET_NUMBER` and the like) are a later stage.
