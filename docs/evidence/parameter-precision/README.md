# Preserve untouched type dimensions — #513 follow-up

**November 21 refresh:** see [the conflict refresh report](refresh-2026-11-21.md) for the current base and headless validation boundary. The original November 18 test-first, visual and runtime evidence below remains historical and tied to its original build.

BIMCraft Rust v0.12.0, refreshed against upstream `e61e6d6d6999416ea2282de7d3d60b84ccce2401`. Upstream `c4a3fa25` repaired metric field formatting, and `faaee4fa` added project decimal settings. This PR retains those changes and addresses the remaining raw-value/history regressions. There is no new core formatter or precision UI.

## Test-first evidence

The synthetic regressions replayed on unchanged new main before this repair: **seven failed, six passed** (one passing test is optional screenshot capture). [before-tests.log](before-tests.log) records the results.

| Action on new main | Raw result before | Required result |
|---|---:|---:|
| Untouched family Apply in feet | Gap 3 → 3.048 mm | 3 mm |
| Type `0.125` ft, repeat Apply | 38.1 → 39.624 mm | 38.1 mm |
| Edit another field, reorder/duplicate a layer, or edit a room offset | 3.000123456789 → 3 mm | Original stored value |
| Edit Width, Apply, unchanged OK, one Undo | Width stays 125 mm | Width returns to 55 mm |

The first candidate and its metric reproduction are preserved in [commit f4f0bf50](https://github.com/drsurfbum/BIMCraft-Rust/tree/f4f0bf50cc94bdf3b7fd8d5367d5d9f0f3f834f9/docs/evidence/parameter-precision). Its formatting implementation was superseded by upstream and is not part of this final diff.

## Repair and scope

`OriginalLength` stages the model value, initial display text and actual edit state for native dimension fields, layer rows, room offsets and family type lengths. Untouched fields retain the original number; edited fields use the existing parser. Actual edit state matters: a stored 3 mm may display as `1/8"`. Leaving it alone keeps 3 mm; deliberately retyping `1/8"` stores 3.175 mm. Layer reordering carries the snapshot. Unchanged Apply/OK skips a redundant type command/history step.

Mutations still use authoritative commands and validation. No file format, command schema, core units formatter, project setting or drawing/schedule behavior changes. Newly typed lengths still round to 0.001 mm. Instance and project type parameters retain their existing changed-text-only paths. Family Editor, the separate stair-rule table and other dialogs are outside this bounded repair.

The focused suite has 13 behavioral tests plus optional visual capture. It covers all six unit systems, explicit units, another field, repeated Apply, Cancel/Escape, invalid input, family/native Undo/Redo, native reopen, Duplicate, reordered layers, submicron values, deliberate retyping and upstream metre decimal settings from two to five places.

## Runtime and visuals

[fixture.txt](fixture.txt) creates a generic block and two-layer wall. The UI test fixture is independent and only 500 mm high; the runtime block is 2000 mm high so it crosses the plan cut. All models/images are synthetic.

`mcp_probe.py CLI OUTPUT_DIR` drives real JSON-RPC stdio. `desktop_probe.py DESKTOP CLI OUTPUT_DIR` runs the same semantic commands in an isolated native window with separate preferences and update checking disabled, then cleans up only its own saved test process. It is not a manual close/unsaved-prompt acceptance test. Optional `--capture` requests native plan/3D images: those requests timed out on this refreshed build, so native screenshot acceptance is not claimed. The six focused egui renders below are the visual evidence. `ifc_probe.py OUTPUT_DIR` independently checks schema/EXPRESS rules and mesh bounds with IfcOpenShell and pytest.

```sh
cargo test --locked --workspace precision_
BIMCRAFT_PRECISION_DUMP=/absolute/output/folder cargo test --locked --workspace precision_screenshots
python3 docs/evidence/parameter-precision/mcp_probe.py /absolute/path/bimcraft-cli /absolute/output/folder
```

The refresh preserves upstream field appearance. Captures: [Rev light](after/rev-light.png), [Rev dark](after/rev-dark.png), [Rev light canvas](after/rev-light-canvas.png), [BIMCraft light](after/bimcraft-light.png), [BIMCraft dark](after/bimcraft-dark.png), [BIMCraft light canvas](after/bimcraft-light-canvas.png). Six profile/theme captures verify presentation; the raw-value table and assertions establish behavior. Automated captures are not manual user acceptance.

## Revit comparison

Reference versions: Revit LT 2026 [Project Units](https://help.autodesk.com/cloudhelp/2026/ENU/RevitLT-Customize/files/GUID-3AC18F2E-AEB3-43F5-A77A-4E9B72CE9357.htm) and Revit API 26.4.0.0 [FormatOptions.Accuracy](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-API-MainReference/files/html/bbba6604-8961-3a26-4e1f-73c2a2169464.htm). Documentation references, not native Revit observations. No Autodesk prose/assets copied into implementation or fixtures.

| Workflow | Bounded BIMCraft Rust evidence |
|---|---|
| Display versus stored precision | Retains upstream display settings and untouched model values. Documentation supports treating display rounding separately. |
| Activation and type/instance distinction | Select the synthetic element, Edit Type, stage edits. Type commands remain shared by UI/MCP; adjacent instance/project-type paths tested. |
| Numeric input | Metric/imperial, explicit units, deliberate retyping, parser precision and atomic refusal tested. |
| Apply/Cancel/history/reopen | Tests read stored values; one Undo reverses the real family/native edit. |
| Geometry/layers/export | Layer edits, reordering and Duplicate covered; independent IFC checks measure synthetic geometry. |
| Grips, snapping and hosting | No changes or additional parity claims. |

## Checks and attribution

On macOS arm64 with Rust/Cargo 1.99.1, development/test debug information and incremental compilation disabled:

- `cargo fmt --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- Translation extraction and check: passed; this repair adds no translatable strings.
- `cargo test --locked --workspace`: **5,347 passed, 0 failed, 22 ignored** across 59 test binaries; sample wasm plugin built first.
- Final focused workspace run with graphics capture: **14 passed, 0 failed** (13 behavioral tests plus capture); all six images inspected.
- Desktop and CLI development builds: passed. Tested source/catalog hashes are in [checks/source-hashes.json](checks/source-hashes.json).
- Standalone and real connected MCP passed numeric edits, atomic refusal, Undo/Redo and persistence/export. Independent reopen retained Gap 3 mm, Width 125 mm and wall layers 3 + 125 mm.
- IfcOpenShell 0.8.4.post1: zero schema/EXPRESS findings in three synthetic exports; wall bounds 4 × 0.128 × 3 m and block bounds 0.125 × 0.5 × 2 m.

[Check logs](checks/) and [runtime results](runtime/) are committed. Native screenshot requests timed out; manual window closing, native visual acceptance and native Revit parity remain unverified.

README/area docs and the [proposed wiki patch](../../wiki-patches/parameter-precision.md) are updated; wiki patch unpublished. Agent: **Codex desktop / gpt-6-astra / high reasoning effort**, verified from recorded turn configuration. No subagents.
