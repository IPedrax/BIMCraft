# Proposed wiki update for vertical drawing tools

Apply [vertical-tools.patch](vertical-tools.patch) to the `IPedrax/BIMCraft.wiki` repository after the feature is accepted.
It was prepared against wiki commit `53fc764cfccd8a2a1791af09558253394eb73db2` on November 16, 2026. The live wiki has not been
edited: publishing these capabilities there before the code merges would describe unavailable behavior on main.

The patch updates four existing pages, following Wiki Plan's user instructions first / agent parameters last convention:

- **Sections and Elevations:** exact supported references, Move/Trim categories, cancellation and agent view coordinates.
- **Dimensions:** separate plan constraints from vertical measurement; document `view`, explicit refs and format 21.
- **Modify Tools:** supported vertical Move and detail Trim, atomic refusal, host restrictions and displacement preview.
- **Detail Lines and Line Styles:** view-local drawing, coordinate meaning, Pick Lines/Fillet and snapping limits.

For a maintainer with the wiki clone and this repository available:

```sh
git apply --check /path/to/BIMCraft/docs/wiki-proposals/vertical-tools.patch
git apply /path/to/BIMCraft/docs/wiki-proposals/vertical-tools.patch
```

Review the patch against the wiki's then-current content before publishing. The README and `docs/areas/desktop.md` /
`docs/areas/views-and-graphics.md` updates are already part of the feature PR. The before/after pictures and exact
reproducer are in [the evidence page](../vertical-tools-evidence.md).
