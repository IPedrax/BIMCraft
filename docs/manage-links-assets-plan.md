# Manage Links and the Asset Manager: plan

Status: every step built (draft PR #540). Each step lands with
its test, and this file says where it stands. How it works now: [views and graphics](areas/views-and-graphics.md#manage-links)
and [assets](areas/assets.md).

Two dialogs, each with a command under it, so the CLI, scripts and MCP get the same thing the window shows:

- **Manage Links** is Revit's: every file the project reads from elsewhere, one tab for each kind, its path and status,
  and Reload, Reload From… and Unload. Each row says how many instances of it are placed.
- **The Asset Manager** collects what a project draws with: materials, fill types (filled region types), fill patterns,
  line patterns, line styles and pictures. Each says where it is used, and assets go out to and come in from a `.bima`
  file that carries its pictures.

## What exists already

- Links: `Element::Link` (`link.rs`), its file read into `Model::loaded` by `path`, `link` and `reload` (every link or one).
  A link's file isn't in the project, only its path. There is no unload and no reload from another path.
- CAD: `Model::cads` (`cad.rs`) holds each drawing's bytes, `path` set when it's linked; `CadImport` elements place it.
  `reload_cad {cad}` reads a linked one again. Imported drawings stay as they were imported.
- Point clouds: `Model::clouds`, `PointCloud` elements place a scan the project holds; `source` names the file it came from.
- 3D models: `import_mesh` makes Generic Model families from an OBJ or glTF and places them. Nothing remembers the file.
- Pictures: `Model::images`, PNGs that materials' textures use (`texture.rs`, at most `MAX_IMAGES`, `MAX_SIDE` px). A picture
  can't be placed in a view, and a PDF can't come in at all.
- The library (`library.rs`) gives materials, patterns, styles, region types and pictures ids and GUIDs. `insert.rs`
  already brings line styles, line patterns, fill patterns and region types across by name.

## Steps

### Manage Links

1. **Core: `links`, the list.** A read-only command, `{kind?}`, that answers one list for each tab: `projects` (BIMCraft
   links), `ifc` (links to an IFC), `cad`, `pdf`, `images`, `clouds` and `models`. One row for each file, as Revit lists a
   link's type: `name`, `path` as saved, `path_type` (relative or absolute), the path it resolves to, `status` (loaded,
   unloaded, not found, or imported for what the project holds and can't read again), `instances` (how many elements place it)
   and their `ids`. Tests: two links of one file are one row with two instances; a deleted file reads `not found`.
2. **Core: unload.** `unload {id | path}`: the link stays where it stands, draws nothing and isn't read when the project
   opens. `Link::unloaded` is kept in the file. `reload` loads it again. Tests: an unloaded link draws nothing, a saved and
   reopened project leaves it unloaded, reload brings it back.
3. **Core: Reload From.** `reload {id, path}` (and `reload_cad {cad, path}`) reads another file in its place. Every instance
   of that file follows, and a file that can't be read leaves everything as it was. Tests: one, failure rolls back.
4. **Core: CAD, clouds and 3D models in the list.** Linked and imported CAD with their instances; scans and the
   `PointCloud`s that place them (listed as imported: a scan's file isn't kept, so it isn't reloaded yet); families made by
   `import_mesh` remember their file (`Family::source`), so a 3D model is listed with its components and `reload_mesh`
   reads it again, or another file in its place.
5. **Desktop: Manage Links.** Insert > Link and Manage > Manage Project get **Manage Links**. A tab for each kind, a table
   of name, status, instances, saved path and path type, and Reload, Reload From…, Unload and Add…. Tested with egui_kittest.

### Pictures and PDFs in views

6. **Core: an image in a plan.** `image {path | picture, level?, view?, at?, width?, link?}` places a PNG or JPEG in the
   plans of a level, or one of them (`Element::Image`), drawn through the display list's `Prim::Image` and kept linked to its
   file when asked (`link: true`), so Manage Links' Images tab reloads it (`reload_image`). Insert > Image has Import Image…
   and Link Image…. Tests: the PNG's pixels where it's placed. Still to come: sheets, sections and drafting views, a turn,
   and pictures larger than a texture's 1024 px.
7. **Core: a PDF in a view.** A page of a PDF rendered to a picture and placed the same way, kept linked to its file. The
   renderer is `hayro` (pure Rust, Apache-2.0 OR MIT) with its default `embed-fonts`, PDFium's Foxit fonts (BSD-3), credited
   in `NOTICE`, so a PDF's text in an unembedded standard font is drawn. Built: `image` of a `.pdf` with `page`, Insert > PDF.

### The Asset Manager

8. **Core: `assets`, what the project draws with.** `{kind?}` lists materials, fill types, fill patterns, line patterns,
   line styles and pictures, each with its id, GUID and name and `used_by`: what uses it and where. For a material, the
   types whose layers are made of it, the families, and the faces painted with it. For a picture, the materials whose
   texture it is and the views it's placed in. For a fill pattern, the materials and fill types. For a line pattern, the
   styles. For a line style, the detail lines. Tests: each kind's users.
9. **Core: the `.bima` file.** `files::ASSETS` (`bima`). It's a zip, as a `.bimc` is (`bimc.rs`): `assets.json`, its
   `format` and each asset with its GUID, and `images/<id>.png`, each picture as the project holds it (a JPEG comes in and is
   kept as a PNG, so nothing is lost a second time). `export_assets {path, materials?, …}` writes the chosen assets, or all
   of them, with what they need: a material's fill patterns and picture, a style's line pattern, a fill type's patterns.
   `import_assets {path, names?, existing?: skip|replace|rename}` reads one kindly and strictly: sizes capped, pictures held
   to `MAX_BYTES` and checked as `texture` checks them, a newer `format` refused. Tests: a round trip keeps every asset and
   the picture's pixels; a hostile file is refused without a panic.
10. **Desktop: the Asset Manager.** Manage > Settings gets **Asset Manager**: a tab for each kind, a list with a preview
    (a swatch, the pattern, the picture), where each is used, and Import… and Export… (the selected ones or all).
11. **Docs and translations.** A section in `docs/areas/views-and-graphics.md` (links, images) and one in
    `docs/areas/families.md` or a new `docs/areas/assets.md`, the modules' lines in the maps, `cargo i18n update`.

## Open questions

- Revit's Manage Links also has a reference type (Overlay or Attachment), which decides whether a link's own links come
  with it. BIMCraft never brings a link's links, so every link is an Overlay. The column is left out until that changes.
- How large a picture placed in a view may be: 4096 px a side is the proposal, under the same file cap.
