# Proposed wiki patch: untouched type lengths

Unpublished. Checked against the live Types and Materials / Project Settings pages on November 18, 2026. This supplements upstream's metric-field fix and project-decimal setting.

## Types and Materials — after the Type Properties workflow list

Type dimension fields, layer thicknesses, room offsets and family type lengths keep their stored values when you leave them untouched. This applies when imperial display rounds a value or the stored value has more digits than typed input accepts. Editing one field does not round the others.

Typing into a field uses the usual unit parser. Deliberately retyping its displayed value counts as an edit: retyping `1/8"` sets 3.175 mm, while leaving a 3 mm value displayed as `1/8"` untouched keeps 3 mm. Newly typed lengths keep the existing 0.001 mm precision.

Repeated unchanged **Apply** or **OK** adds no extra Undo step. **Cancel** discards staged edits. Duplicate and reordered layer rows retain their untouched values.

This note covers the fields listed above, not Family Editor or the separate stair-rule table. Display units and project decimal settings continue to control presentation.
