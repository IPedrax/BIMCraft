"""Launch an isolated desktop, run MCP, capture it and clean up only that process.

Usage: python3 desktop_probe.py DESKTOP CLI OUTPUT_DIR [--capture]
Run mcp_probe.py standalone first to produce fixture.bimc.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

desktop, cli, directory = sys.argv[1:4]
capture = "--capture" in sys.argv[4:]
folder = Path(directory).resolve()
preferences = folder / "desktop-preferences"
preferences.mkdir(exist_ok=False)
environment = dict(os.environ, BIMCRAFT_FAVORITES=str(preferences / "favorites.json"), BIMCRAFT_NO_UPDATE_CHECK="1")
scenario = folder / "desktop-scenario.txt"
scenario.write_text(f"wait 30\nidle 30\nscreenshot {folder / 'desktop-plan.png'}\nview 3d\nwait 30\nscreenshot {folder / 'desktop-3d.png'}\nquit\n")
with (folder / "desktop.log").open("w") as log:
    command = [desktop, str(folder / "fixture.bimc"), "--control", "0"]
    if capture:
        command += ["--bench", str(scenario), "--bench-out", str(folder / "desktop-frames.json")]
    process = subprocess.Popen(command, env=environment, stdout=log, stderr=log)
    try:
        bridge = preferences / "bridge.json"
        deadline = time.monotonic() + 40
        while not bridge.exists():
            assert process.poll() is None, "Desktop exited before its control port was ready"
            assert time.monotonic() < deadline, "Desktop control port did not become ready"
            time.sleep(0.1)
        port = str(json.loads(bridge.read_text())["port"])
        subprocess.run([sys.executable, str(Path(__file__).with_name("mcp_probe.py")), cli, str(folder), "--connect", port],
                       env=environment, check=True, timeout=60)
        if capture:
            deadline = time.monotonic() + 75
            captures = [folder / name for name in ["desktop-plan.png", "desktop-3d.png", "desktop-frames.json"]]
            while not all(path.is_file() for path in captures):
                assert process.poll() is None, "Desktop exited before its captures completed"
                assert time.monotonic() < deadline, "Desktop captures did not complete"
                time.sleep(0.1)
            report = json.loads((folder / "desktop-frames.json").read_text())
            assert report, "Empty native frame report"
        # A semantic save writes the file but does not necessarily clear the window's UI dirty marker.
        # A close prompt is outside this precision probe; the saved synthetic process is ours to clean up.
        natural_exit = process.poll()
        if natural_exit is None:
            process.terminate()
            process.wait(timeout=10)
        else:
            assert natural_exit == 0, "Desktop failed"
        (folder / "desktop-cleanup.json").write_text(json.dumps({"captures_requested": capture, "captures_completed": capture,
            "natural_exit": natural_exit, "cleanup": "owned process terminated" if natural_exit is None else "natural exit"}, indent=2) + "\n")
        print("Connected MCP passed; owned test process cleaned up (not a manual close test).")
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
