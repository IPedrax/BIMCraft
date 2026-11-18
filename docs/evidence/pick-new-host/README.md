# Pick New Host evidence

BIMCraft Rust, refreshed against `046b3575db132777da22655d6a44aa75b32b349c` (November 18 development main, package 0.12.0).
All public cases are synthetic. Agent: Codex desktop, `gpt-6-astra`, high reasoning effort, verified from this chat's local
turn configuration. No native Revit session or manual user acceptance is claimed.

Select one ordinary door/window in a floor or ceiling plan, choose **Pick New Host**, hover another ordinary straight wall,
and click. The original element and GUID survive; only host and offset change. Preview runs the same command in a scratch
session. A successful command is one Undo/Redo step. Escape, bad fit and stale previews leave the live model untouched.

Orientation follows the target wall's interior/exterior faces. Both hand/facing flags remain unchanged, so a reversed wall
can reverse the world direction. This was the requesting user's explicit policy, not an inference from Autodesk's docs.
Targets must share level and base elevation; sill remains unchanged. With the integrated family origins (#437), offset and
sill locate the insertion point; the physical opening centre and bottom follow its origin and flips. A negative stored
sill can be valid. Rehosting preserves physical height and does not settle the origin again. Group contexts, curved/curtain hosts, panel replacement
walls and pinned openings are refused. Activation outside plans and cross-host dragging are outside this slice.

## Test-first evidence

Before the command existed, `cargo test --locked -p bimcraft-core rehost_tests -- --nocapture` failed with
an unknown-command error naming `pick_new_host`. Before the UI action existed, the first egui test failed with
a missing-node query for `Pick New Host`. The same cases pass after implementation.

The focused core suite has eleven tests covering GUID/value preservation, reversed/rotated face orientation, raised hosts,
existing wall-top notch policy, thicker-wall family evaluation, tags/dimension references, save/reopen, old-host deletion,
read-only refusal, history, groups and curtain hosts. The geometry test compares incremental scene pixels with a fresh scene.
The UI suite exercises actual contextual activation/clicks, hover/Escape/refusal, view/selection/model changes and a TCP
control-port command interleaved with a pending user pick. The hostile-input sweep includes real opening/target payloads.
The refresh adds door/window cases for all four hand/facing combinations, moved XYZ origins, actually mirrored and rotated
target walls, independent vertex/hole calculations, Undo/Redo and native reopen. A UI case checks origin-aware preview and
click placement, including Space leaving the live opening unchanged during the pending pick.

This base includes #437 origins, #443 flips, #455 same-host dragging, #473 clipboard, #490 vertical tools and #495 roofs.
Their existing paths remain intact. Pending #504's slanted/profiled wall schema is not adopted; a control verifies that
its proposed fields still fail strict modification on this base. Its eventual integration requires a new eligibility audit.

## Automated UI captures

These are egui_kittest captures of the feature build before placement, during preview and after placement, not screenshots
of the upstream baseline or manual acceptance. Both profiles and all three appearances were rendered and visually inspected.
The public sample below uses Rev / Light; the reproducible capture test also produces BIMCraft / Light, Dark and Dark UI with
Light Canvas. Set `BIMCRAFT_REHOST_DUMP` to an output directory and run the `pick_new_host_screenshots` test with GPU access.

| Before placement | Preview | After placement |
|---|---|---|
| ![Before](before.png) | ![Preview](preview.png) | ![After](after.png) |

## Revit comparison

Versioned references: Revit 2026 [door rehosting](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-9958015A-2449-4EA0-804C-E561C3D24D6B.htm)
and [window rehosting](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D293BF67-A76A-4A61-8ECA-6CD92E54EF97.htm).
Only independently written behavior descriptions and links are used; no Autodesk assets or model contents are included.

| Workflow | This slice | Evidence boundary |
|---|---|---|
| Selection → Pick New Host → preview → click | Supported in floor/ceiling plans | Documentation reference and automated UI |
| Properties/type distinction | Keeps original type and instance values | Core and MCP; no default-type recreation |
| Interior/exterior-relative facing | Preserves both flags | User policy, rotated/reversed tests |
| Sill and levels | Same level/base only, unchanged sill | Explicit bounded policy; broader Revit rules not asserted |
| Placement precision | Insertion-point projection, origin-aware existing centre snap; command offset in mm | No separate snap engine added |
| Cancel/history | Escape cancels; one command/Undo/Redo; failed fit is atomic | Automated UI/core/MCP |
| Persistence/drawings | Native reopen and rebuilt old/new host cuts | Core and exported-output verification |
| 3D/section activation, curved/curtain/group hosts | Unavailable | Honest disabled UI/refusals; no parity claim |
| Ordinary same-host dragging | Integrated #455 behavior | This PR does not change it |

Area/user docs and a [proposed wiki patch](../../wiki-patches/pick-new-host.md) accompany the change. The wiki patch is not
published. This does not claim completion of the broader agentic document/revision contract or Co-Work acceptance.

## Independent output replay

`python3 replay.py /absolute/path/to/bimcraft-cli /absolute/output/directory` drives the real standalone MCP binary. Its
synthetic fixture has 6 m source/target walls, 200/400 mm thickness, matching 400 mm bases, one 1200 × 1500 mm window at
900 mm sill, and one 900 × 2100 mm door. The plan cut is 1500 mm above Level 1 to cross the raised window. The target wall
runs in the opposite direction. The replay checks identities/values, refusal, Undo/Redo including redo after refusal,
fresh-process native reopen, deletion of the old wall, read-only mode, and exports PNG, IFC and GLB.

With IfcOpenShell and pytest installed, `python3 verify_ifc.py /absolute/output/directory` checks the exported schema and
EXPRESS rules, product/void/host relationships, builds the wall/opening shapes and measures the actual wall net volumes.
Both exports have zero schema/EXPRESS findings. Measured volumes agree with independent rectangular-prism calculations:

| Wall | Before (m³) | After (m³) |
|---|---:|---:|
| Source, 200 mm | 2.862 | 3.600 |
| Target, 400 mm | 7.200 | 5.724 |

The source regains both cuts and the target loses the same opening area at its own thickness.
[Measurement output](ifc-measurements.json). The exported plan and 3D pictures were visually inspected.

![Rehosted door and window; source wall restored](after-3d.png)

## Completed checks

On macOS arm64 with Rust 1.99.1:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo test --locked --workspace`: **5,335 passed, 0 failed, 22 ignored**, including the hostile-input sweep. The wasm
  sample plugin was built before the suite. [Result totals](test-results.json).
- `cargo i18n update` and `cargo i18n check`: passed; new messages fall back to English until translated.
- Desktop and CLI development builds: passed. The desktop linker warned that its debug exception-unwind section exceeded
  the compact-unwind encoding limit; this is a build warning, not a test failure or a release-build result.

The same synthetic fixture was opened in the real candidate desktop with its own settings and control port. The replay
then used `bimcraft-cli mcp --connect`, exported the live result, and repeated the identity/history/refusal checks and IFC
measurements. Connected read-only MCP also refused mutation. This is automated desktop/MCP verification, distinct from
manual UI acceptance. Only that test desktop was closed afterward; no installed app was replaced.
