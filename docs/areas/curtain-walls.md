# Curtain walls

A curtain wall is a `Wall` whose type holds a pattern (`WallType::curtain`, `curtain.rs`), so rooms, joins, constraints, tags and
filters treat it as any wall. Its panels and mullions are worked out from that pattern and the wall's size by
`curtain::layout`, never stored one by one.

## Types

Panels and mullions are system families: `Types::curtain_panel` (the Generic Panel family: `Glazed` and `Spandrel` to start with;
thickness, material, offset) and `Types::mullion` (Rectangular Mullion: width, depth, `offset_x`, `offset_y`, material, both offsets
from its centre). `MullionProfile` holds the section, so a drawn profile can be another kind. They are library entries
(`Kind::Type("curtainpanel")`, `Kind::Type("mullion")`), and a pattern's names of them are `CurtainLayout::names_mut`. `type` makes and
edits them. A project of file format 16 gets the starter set when read.

## The pattern

`CurtainLayout` has `columns` and `rows`, each a `Direction`: segments in mm, a `Fit` and a `Justify`. `panels[row][column]` fills a
cell; `vertical[row][column]` is the edge along the line at that column's start, in that row; `horizontal[row][column]` the edge
along the line at that row's bottom, in that column. Each may be empty. `borders` are the wall's own edges. `continuous` says which
mullions run through where they cross. The line that starts a module also closes the one before it.

`type {category: "wall", name, curtain: {columns: {segments}, rows: {segments}, panel?, mullion?, …}}` makes one; `panel` and `mullion`
fill what is left out, and null means none. A curtain wall type's `thickness` is never given: `curtain::sync` makes it what the
panels and mullions the pattern uses reach (`envelope`), after every command.

## The layout

`divide` lays a pattern over a length from where it is justified. A `Span` keeps `index`, its count from the pattern's origin,
which is its name as the wall changes; a part's `Address` is made of those. `layout` then places:

- **Border mullions** stand inside the wall, so it never reaches past its ends, its base or its top.
- **Mullions** sit on their line plus their offsets. A mullion that would run into its neighbour goes, the border winning. The run
  that isn't `continuous` is cut back to the faces of those it meets.
- **Panels** sit in their cell plus their offset, cut back to any mullion that reaches them in depth. A panel in front of the
  mullions isn't cut (structural glazing), and with no mullion panels run gapless.
- **Malformed:** a cell left no more than `min_panel` gets no panel. At an end's cut segment that goes in `Layout::malformed`; in a
  whole segment `Model::check` refuses the pattern. Nothing of zero size is ever drawn, and the panel comes back by itself once the
  gap holds one. `CurtainEdits::absorb_columns` and `absorb_rows` (Remove Last Interior Grid) take the short segment into its
  neighbour while it is too small; the wall keeps its length.

## Drawn once

`Layout::shapes` holds each distinct part once (kind, type, size) and `Layout::parts` places them. These shapes are silent: no list or
file shows them. They are what lets identical parts share geometry when the 3D instancing of PR #188 arrives; today each part is
still one `Solid::Box` (`view3d::curtain_solids`), and sections and elevations read those solids.

An unedited part has no id of its own, only its `Address`: an agreed exception to the id rule, since a tower's façades would use up
the six-digit ids and every change of length would rewrite them.

## What it can't be yet

`curtain::check` refuses a curved curtain wall, an attached top or base, a location line other than the exterior or interior face, a
wall of over `MAX_CELLS` cells, and a door or window hosted directly in one. Corner mullions are held for their own discussion.
