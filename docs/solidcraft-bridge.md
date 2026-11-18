# SolidCraft-backed families (opt-in review)

An embedded SolidCraft document can supply a family's geometry. BIMCraft Rust owns the family category,
parameter groups, type/instance scope and project values. Vanilla SolidCraft evaluates the original model
with those values. No cabinet, window, door or fixture names choose an implementation in the evaluator.

This is a proposed integration, not a bundled SolidCraft installation or an implementation of SolidCraft's
feature editor in BIMCraft Rust. The companion publisher below is a Python CLI. The separate local trial's
mapping dialog is **not** a native import dialog in this PR.

## Try it

Install a trusted vanilla SolidCraft **0.44.1** CLI separately and build this branch's BIMCraft Rust CLI.
The runtime is explicitly enabled by `BIMCRAFT_SOLIDCRAFT_CLI`; the application never downloads an evaluator
or searches another user's machine-specific path. Launch the desktop from an environment containing the
same setting. Only load trusted source documents: evaluation launches an external CAD process; it is not
an operating-system sandbox.

```sh
export BIMCRAFT_SOLIDCRAFT_CLI=/absolute/path/to/solidcraft-cli
cargo build -p bimcraft-cli
python3 tools/solidcraft/publisher.py examples/solidcraft/Window.sc --out /tmp/catalog.json
# Review the catalog and author an explicit mapping; these examples include theirs.
python3 tools/solidcraft/publisher.py examples/solidcraft/Window.sc \
  --mapping examples/solidcraft/Window.mapping.json --out /tmp/Window.bimf
cargo run -p bimcraft-cli -- run --cmd 'family {"path":"/tmp/Window.bimf"}'
python3 tools/solidcraft/showcase.py --cli target/debug/bimcraft-cli --out /tmp/bridge-showcase
```

The publisher lists source parameters, expressions, units and existing source comments. It does not invent
semantic descriptions. Its mapping explicitly names `category`, each selected parameter's `scope` and
`group`, optional `kind` (`integer` or `yesno` for unitless inputs), and dimension roles. Omitted parameters
stay in the source; formula-driven parameters are not independent editable inputs. Dimension roles map
arbitrary source names to Width/Height/Depth schedule semantics. The author remains responsible for useful
independent controls and valid combinations; a single source dimension does not become two independent axes.

The examples' parameter names and mapping choices are fixture data, not application rules. `showcase.py`
loads each family, places a component or a wall-hosted opening, changes instance inputs, saves, reopens,
exports GLB, and captures before/after. It also captures Undo and Redo states. Keep output outside the source tree.
The window and door use existing rectangular wall-opening behavior. The lantern is decorative geometry;
it supplies neither electrical connectors nor photometric simulation. There are no copied Revit families.

## Behavior and boundaries

| Action | Behavior in this proposal |
|---|---|
| Change an instance input | Properties/`modify` rebuild that placement through the authoritative command transaction. |
| Change a type input | Edit Type/`family` rebuild all placements of that type. |
| Failed or over-budget evaluation | Refuse the command; retain the previous model and history. |
| Undo/Redo, save/reopen | Preserve embedded source, bindings and effective values. Reopening needs the configured evaluator. |
| Edit Family → Save/Load into Project | Retain the source payload and mappings; source inputs remain parameters. |
| Rename/delete a bound input | Refused with instructions to edit the source and republish. Stable upstream parameter IDs are not available here. |
| Edit the source feature timeline | Use SolidCraft and republish. The bridge does not convert its features into native editable BC forms. |
| Materials, symbols, connectors, host voids | Not translated from SolidCraft in this slice. Native BC additions remain separate. |
| Unknown evaluator versions | Not qualified; pin 0.44.1. There is no automatic engine upgrade or compatibility claim for current SolidCraft main. |

The adapter requests `model.mesh` at 0.001 mm chord tolerance. SolidCraft floors tolerance at body size / 1e6.
It preserves component placements, validates closed finite meshes and retains the existing 400,000-triangle
family budget. It never relaxes tolerance to fit. The API's per-body limits (8,192 vertices / 16,384 triangles)
also apply. This is a meshing request, not a certified error bound for every possible surface.

A saved source is capped at 24 MiB base64, each reply at 32 MiB, the component/placement inventory at 4,096,
and each evaluator invocation at 120 seconds. Diagnostics, incomplete results, invalid topology or excess
size refuse the result. Successful results are cached with the evaluator path and effective input changes.
Evaluation currently runs synchronously on a cache miss; cancellable background evaluation, persisted mesh
fallback, broader platform/version qualification and a native mapping dialog are follow-up work.

Project format is 22: an older reader must refuse a source-backed project instead of dropping the source
field and silently showing an empty family. Older projects acquire empty optional fields during migration.
Family JSON readers in older versions may still ignore unknown fields; use the review build for `.bimf` too.

## References and provenance

Behavioral reference: Autodesk Revit **2026**, [Create Family Parameters](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-921F7A15-D191-4F75-8243-4989C482E253.htm)
and [Specify Family Category and Parameters](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-68EFCA67-4913-4E00-AB9E-F2E6A7BEF8C6.htm), consulted 2026-11-18.
These independent observations concern grouping, scope and category behavior; they are not a claim of full Revit parity.

| Reference expectation | Bridge comparison |
|---|---|
| Author chooses Instance or Type and Properties group | Supported by explicit mappings and native BC parameter semantics. |
| Category determines placement/host behavior | Existing BC categories used; no automatic host inference from source geometry. |
| Family Editor retains editable family parameters | Supported for published inputs; source timeline remains external (intentional departure). |
| Shape handles, source constraints and nested-family associations | Not translated; not qualified by this showcase. |

Window, door and lantern were authored independently for this study with vanilla SolidCraft 0.44.1
(e151d5ea6866fb055e011379153fb1cae0566f22). Cabinet sources are the independently developed SDH Woodwork
study models; the supplied private Autodesk `.rfa` and its geometry are not included or used as fixture content.
The public examples contain only our synthetic studies. See `examples/solidcraft/README.md` for controls.

Recheck every exported triangle against the stock evaluator with `python3 tools/solidcraft/verify_showcase.py /tmp/bridge-showcase`. This independently places the component tree and compares complete triangle multisets within 0.001 mm (GLB float32), plus exact saved Undo/Redo geometry. The command uses the same configured CLI.

Measured results, before/after images and test qualifications: [showcase evidence](evidence/solidcraft/README.md).
