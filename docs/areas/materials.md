# Materials

Manage > Materials (`crates/ui-egui/src/materials.rs`) is the project's library in three columns. The left lists the materials,
with New (Custom or Standard Color), Duplicate, Delete, a search and a star on each. The middle is how the picked one looks. The
right is its cube and its parameters. Every change is a `material` command.

## Patterns

A material's surface and its cut each show a **foreground** over a **background** (`Material::surface`, `cut`, `surface_pattern`,
`cut_pattern`, `surface_color`, `cut_color`, `surface_background`, `cut_background`). A background is a `PatternLayer`: a pattern drawn
in code, or one of the project's fill patterns (`fill`), in its own colour. No colour means the drawing's ink, so a material
without the new fields draws as it always did. `render::material_surface` and `render::cut_layer` draw them, background first.
`Material::fills` and `fills_mut` are every fill pattern a material names: use them wherever a fill pattern's name is checked or renamed.
The drop-downs show each pattern through `render::pattern_swatch`.

## Parameters

`material` is a category of project parameters (`PARAMETER_CATEGORIES`), with no types (`TYPELESS`). Every material also has the
parameters of `materials::BUILT_IN`: Mark, Description, Class, Keywords, Comments, Manufacturer, Model, Cost and URL. A project parameter
for materials can't take one of those names. Values live in `Material::parameters`, set through the `material` command's
`parameters`. `materials::check` holds them to what each parameter holds. `materials::tidy` leaves out what a file, a favorite or
an inserted material holds of a parameter this project hasn't.

## The cube

`materials::cube` draws the preview: the colour, texture and transparency the 3D view shows, as RGBA, by following each pixel's line
of sight through the cube. It takes any turn and tilt, and is held to `MAX_CUBE` pixels.

## Standard colours

`standard_colors.rs` holds the collections and the `standard_color` command. A colour makes a basic material: its colour, solid on
surfaces and where cut, named `RAL 3020 Traffic red`.

Only openly licensed data ships:

- **RAL Classic** (`ral.rs`): numbers, English names and CIELAB values from the BSD-3-Clause
  [Color](https://github.com/lehins/Color) library, licence in `NOTICE`, converted to sRGB here.
- **CSS named colours**: the W3C's CSS Color specification.

Every other standard is the user's to bring. `import` reads a CSV (`code, name, #rrggbb` or `code, name, r, g, b`) into
`standard_colors.json` beside their favorites, never into a project and never into a package, because those tables have owners:
W3Schools' terms forbid copying its colour tables, and NCS needs a commercial licence from NCS Colour AB. Don't add a collection
without a licence that allows it, and show each collection's source in the picker.
