# Proposed wiki additions — #513

Prepared against the live **Project Settings** and **Types and Materials** pages read November 18, 2026. These additions are proposed, not published.

## Project Settings — after the Units paragraph

Family type lengths and native type dimensions in Type Properties show additional digits when the usual display rounding would hide part of the value. In a metre project, a 3 mm type length reads `0.003`, and 55 mm reads `0.055`. You can also enter an explicit unit, such as `3 mm`. Applying the type again or editing another field keeps untouched lengths at their original stored precision. Newly typed lengths retain the existing input precision of 0.001 mm.

Drawing and schedule rounding is unchanged. Project Units does not yet offer a project-wide decimal-places setting.

## Types and Materials — after Edit Type's action list

Type Properties keeps untouched family type lengths, native dimensions, layer thicknesses and room finish offsets at their stored precision. **Apply** commits the current edit and leaves the dialog open; **Cancel** discards edits made since the last Apply. Enter a length in the project's unit, or include its unit (`0.003` in a metre project, or `3 mm`). Repeated Apply does not round those lengths to the drawing display precision. Duplicate carries the same precise dimensions into the new type.
