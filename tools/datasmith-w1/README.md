# Datasmith W1 reimport experiment

This editor-hosted prototype checks whether an Unreal material applied to a BIMCraft wall survives a change to its hosted window. It exports a synthetic 12 ft × 8 ft wall and one 36 in square window as a Datasmith scene with two mesh actors. It then moves the window 16 in along the wall, writes over the same scene, reimports it, saves the level and reopens it.

## Run it

Use an Apple Silicon Mac with Xcode and its C++ toolchain, Python 3.8 or later, the repository's Rust toolchain (1.90 or later), and a local Unreal Engine 5.7.4 editor installation with engine headers. The included empty project enables Datasmith Importer, Python Editor Script Plugin, Editor Scripting Utilities and Procedural Mesh Component. The bridge builds against the matching editor libraries on your machine.

Run from the repository root. Adjust the Xcode and Engine paths for your installation.

```sh
cargo build --locked -p bimcraft-cli
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
python3 tools/datasmith-w1/run.py \
  --cli target/debug/bimcraft-cli \
  --engine "/Users/Shared/Epic Games/UE_5.7/Engine"
```

The runner prints a fresh temporary working directory. It creates both model revisions from `CON03-W1.commands`, builds the bridge, and launches the headless Unreal Editor commandlet. A completed run prints `PASS` and the location of `evidence/`. Inspect `before.json`, `after.json` and `reopened.json` there, or open `Receiver/W1.uproject` and load `/Game/W1/ReimportLab` to inspect the saved result. On failure, check `unreal.log` and `editor-console.log` in the printed directory.

## Expected measurements

The test intersects the imported front wall triangles at 1300 mm above the wall base. It separately measures the window mesh in world coordinates. Both measurements must match the expected jamb positions within 0.01 mm.

| Measurement | Before edit | After reimport and after reopening |
| --- | --- | --- |
| Left / right jamb from wall start | 1371.6 / 2286.0 mm | 1778.0 / 2692.4 mm |
| Window sill / head | 914.4 / 1828.8 mm | 914.4 / 1828.8 mm |
| Wall / window triangles | 48 / 12 | 48 / 12 |
| Mesh actors under one Datasmith root | 2 | 2 |
| Wall material, slot 0 | M_DeepTeal_LapSiding | M_DeepTeal_LapSiding |

The receiver checks actor objects and the wall material object across reimport. It checks the native document UID, element IDs and GUIDs against the Datasmith keys, then checks those keys, actor paths, mesh references and the material assignment across level reload. Unreal regenerated its editor actor GUIDs on reimport in the tested build. Use the document and element keys for source identity.

Verified November 14, 2026 with BIMCraft Rust 0.12.0 built from upstream main `a0c57b738e308d433ab4b8c0c93d7fff9e53bc1b` and Unreal Engine 5.7.4 (CL 51494982). This base includes the official v0.12.0 release at `355a90d8de43f2619a691ae8552d561e4b962b9d` and subsequent large-project changes. Reading the mesh sections in the editor produces a warning about CPU access in cooked builds.

## Exporter scope

The Python adapter reads the saved document for identity and its matching GLB for tessellation. The bridge writes a Datasmith scene and two adjacent mesh assets through the installed editor libraries. Each W1 element has one triangle primitive and one material slot. The window pivot assumes an unrotated wall on the positive X axis. This experiment covers file-based reimport for that geometry on macOS arm64. General model export, standalone SDK builds, Direct Link, packaged games and other platforms remain untested.

The integrated export engine and Publisher already provide a GLB writer in `crates/core/src/export/formats/model.rs`. It calls the same `gltf::export` tessellator as the CLI `save` path used here. This prototype consumes that GLB output in an external editor test. Registering Datasmith as a Publisher format is future integration work.

Twinmotion receiver behavior remains unverified. At the November 14 local inventory check, the installed Epic manifests and application folders contained Unreal Engine 5.7.4 and its plugins, with no Twinmotion installation found. The measured acceptance above applies to the Unreal Editor receiver.

The workflow references are Autodesk's documentation for [Revit 2026 visualization from a 3D view](https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-98192ABD-4E6F-4AF5-B909-FF0795B0AA39.htm) and Epic's [Unreal Engine 5.7 Datasmith reimport workflow](https://dev.epicgames.com/documentation/en-us/unreal-engine/reimporting-datasmith-content-into-unreal-engine?application_version=5.7).
