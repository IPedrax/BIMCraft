"""Build and run the synthetic W1 reimport test on Apple Silicon."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path, help="BIMCraft CLI executable")
    parser.add_argument("--engine", required=True, type=Path, help="Unreal 5.7 Engine directory")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("the bridge build requires an Apple Silicon Mac")

    source = Path(__file__).resolve().parent
    cli = args.cli.resolve(strict=True)
    engine = args.engine.resolve(strict=True)
    editor = engine / "Binaries/Mac/UnrealEditor-Cmd"
    # Use a short space-free commandlet entry path even when the checkout has spaces.
    work = Path(tempfile.mkdtemp(prefix="bimcraft-w1-", dir="/tmp")).resolve()
    print(f"Working directory: {work}", flush=True)
    fixture = work / "fixture"
    fixture.mkdir()
    receiver = work / "Receiver"
    receiver.mkdir()
    project = receiver / "W1.uproject"
    shutil.copyfile(source / project.name, project)
    entry = work / "verify_unreal.py"
    shutil.copyfile(source / entry.name, entry)
    shutil.copyfile(source / "write_scene.py", work / "write_scene.py")

    env = {**os.environ, "BIMCRAFT_PLUGINS": "", "PYTHONDONTWRITEBYTECODE": "1",
           "W1_WORK": str(work)}
    env["W1_VERSION"] = subprocess.check_output(
        [cli, "--version"], env=env, text=True
    ).strip().split()[-1]
    subprocess.run([sys.executable, source / "build_bridge.py", engine, work / "w1.dylib"],
                   env=env, check=True)
    original = fixture / "original.bimcraft"
    subprocess.run([cli, "run", "--script", source / "CON03-W1.commands",
                    "--save", original, "--save", fixture / "original.glb"], env=env, check=True)
    # Reopen the same document so both writes carry the original persistent element keys.
    subprocess.run([cli, "run", original, "--cmd", 'modify {"id":100003,"offset":2235.2}',
                    "--save", fixture / "moved.bimcraft", "--save", fixture / "moved.glb"],
                   env=env, check=True)
    log = work / "unreal.log"
    with (work / "editor-console.log").open("w") as console:
        subprocess.run([editor, project, "-run=pythonscript", f"-script={entry}", "-nullrhi",
                        "-unattended", "-nosound", "-NoSplash", "-NoP4", f"-abslog={log}"],
                       env=env, stdout=console, stderr=subprocess.STDOUT, check=True)
    # UE can return zero even when a Python commandlet didn't finish.
    result = json.loads((work / "evidence/PASS.json").read_text())
    print(f"PASS on Unreal {result['engine']}. Measurements: {work / 'evidence'}")


if __name__ == "__main__":
    main()
