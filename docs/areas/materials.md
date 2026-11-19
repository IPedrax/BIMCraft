# Materials

Manage > Materials (`crates/ui-egui/src/materials.rs`) is the project's library in three columns. The left lists the materials,
with New (Custom or Standard Color), Duplicate, Delete, a search and a star on each. The middle is how the picked one looks: its surface and its cut
each switch between Pattern and Image. The
right is its cube and its parameters. Every change is a `material` command.

## Patterns

A material's surface and its cut each show a **foreground** over a **background** (`Material::surface`, `cut`, `surface_pattern`,
`cut_pattern`, `surface_color`, `cut_color`, `surface_background`, `cut_background`). A background is a `PatternLayer`: a pattern drawn
in code, or one of the project's fill patterns (`fill`), in its own colour. No colour means the drawing's ink, so a material
without the new fields draws as it always did. `render::material_surface` and `render::cut_layer` draw them, background first.
`Material::fills` and `fills_mut` are every fill pattern a material names: use them wherever a fill pattern's name is checked or renamed.
The drop-downs show each pattern through `render::pattern_swatch`.

## Image fills

In place of both patterns, a surface or a cut may show a picture (`Material::surface_image`, `cut_image`; the `material` command's
`surface_image` and `cut_image`). An image fill is a `Texture` of kind `image`, a tile of it `size` mm across and up, turned and
moved as a texture is, and the project holds its picture in `Model::images` as a texture's. Its file is held to
`texture::FILL_MAX_BYTES` (8 MB) and kept at `FILL_MAX_SIDE` (512 px a side), smaller than a texture's, because a plan draws it
pixel by pixel: `render::image_fill` lays a `Prim::Image` over the ring's box on the view, clear outside the ring. A drawing's image
fills cover at most `render::MAX_IMAGE_FILL` pixels between them; past that, the rest fill in their picture's average colour, with a
note. Patterns show only at Fine detail and a cut image fill only where a layer is wide enough to hold a pattern; a section box's cap
fills in the picture's average colour. `Material::pictures` and `pictures_mut` are every picture a material names (its texture and
its image fills): use them wherever a picture is kept, copied or renamed.

The starter library's Grass, Gravel, Wood, Paving and Water have an image fill on their surfaces and the same picture as their
texture. Their pictures are in `crates/core/assets/materials`, 128 px JPEGs (18 KB in all) that `Picture::bundled` keeps as they are, so a new
project stays small. Water is 50% transparent (`WATER_TRANSPARENCY`). The sources and licences are in that folder's README and in NOTICE. Paving is CC BY 3.0 and must stay credited.

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
