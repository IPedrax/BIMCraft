# Assets: the Asset Manager and `.bima` files

The Asset Manager (Manage > Annotation Settings > Asset Manager, `crates/ui-egui/src/asset_manager.rs`) gathers what a project draws with:
its materials, fill types (filled region types), fill patterns, line patterns, line styles and pictures. Each kind has a tab.
Each asset shows where it is used, and assets go out to a `.bima` file and come back in from one. The window only shows what
the core's commands answer and runs them.

## What uses an asset (`assets`)

`assets.rs` answers `assets {kind?}`: a row for each asset with its library `id` and `guid` (`library.rs`), `uses` and
`used_by`. Users are found as Purge Unused counts them (`Model::lib_refs_mut`), but each by who holds the name:

- an element (`{kind: element, id, category}`): a detail line its style, a filled region its type, a face painted its material;
- a type (`{kind: type, category, name, what}`): a wall, floor, roof, ceiling or toposolid type's layer, a run type's material,
  a curtain panel's or mullion's material;
- a family, a material (its picture and its cut and surface fill patterns), a fill type (its patterns), a line style (its
  line pattern), a grid type (its line pattern), a view template's overrides, a group type's members, a phase override.

`delete_assets {<kind>: [names], …}` takes assets out (the Asset Manager's Delete, on the checked ones or the one picked): refused,
naming up to three users, for one something uses that isn't among those named, so a line style and its own pattern go
together. One undo step.

A picture also says its `size` in px and its `label`, the name it came in with. A picture lives in `Model::images`, keyed by
`img-<hash>` of its PNG (`texture::Picture::id`), so the same pixels are one picture.

## The `.bima` file (`bima.rs`)

`files::ASSETS` (`bima`). It's a zip, written and read by `bimc::zip_files` and `bimc::unzip_files`, the same in-house zip as
a project's: `assets.json` (deflated) and `images/<id>.png` or `images/<id>.jpg` (stored), each picture's file byte for byte, as
the project holds it. A picture that came in as a JPEG is held as that JPEG (`texture::Picture::import_at`), so it stays as
small as it was and going out and back loses nothing.

`assets.json` is `{format, materials, fill_types, fill_patterns, line_patterns, line_styles, pictures}`: each kind by name,
an asset as `{guid, value}` with its value as a project file writes it, a picture as `{guid, name, file}`.

- `export_assets {path, <kind>: [names] | true, …}` writes the chosen assets, or with none chosen all of them, and what they
  need: a material's fill patterns and picture, a fill type's patterns, a line style's line pattern (`bima::closure`).
- `import_assets {path, <kind>?, existing?}` brings pictures in first, then fill and line patterns, then what names them,
  each name the project has handled as `existing` says. `skip` keeps the project's, and what came with it uses that one.
  `replace` puts the file's in its place. `rename` brings it in as `name 2`, and what came with it is renamed to follow
  (`rename_refs`). A new asset keeps the file's GUID when the project has no entry of that GUID (`keep_guid`).
- It's read kindly. An asset this version can't deserialize, or a picture whose file is missing or broken, is `left_out`
  with why, and the rest comes in. A newer `format` is refused. So is a file past `MAX_FILE` (64 MiB) or with more than
  `MAX_ASSETS` of one kind, a zip past 64 entries, or an entry whose checksum fails. Pictures are held to
  `texture::MAX_BYTES` and the project's `MAX_IMAGES` (`Model::check`).

Tests: `crates/core/src/assets_tests.rs` (users, round trip with GUIDs and pixels, skip, replace, rename, hostile files) and
`crates/ui-egui/src/tests/asset_manager.rs`.
