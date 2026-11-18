"""Launch an isolated desktop, run connected MCP, capture the window and close it.

Usage: python3 desktop_probe.py DESKTOP CLI OUTPUT_DIR
Run mcp_probe.py standalone first to produce fixture.bimc.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

desktop, cli, directory = sys.argv[1:4]
folder = Path(directory).resolve()
preferences = folder / "desktop-preferences"
preferences.mkdir(exist_ok=False)
environment = dict(os.environ, BIMCRAFT_FAVORITES=str(preferences / "favorites.json"), BIMCRAFT_NO_UPDATE_CHECK="1")
scenario = folder / "desktop-scenario.txt"
scenario.write_text(f"wait 30\nidle 30\nscreenshot {folder / 'desktop-plan.png'}\nview 3d\nwait 30\nscreenshot {folder / 'desktop-3d.png'}\nquit\n")
with (folder / "desktop.log").open("w") as log:
    process = subprocess.Popen([desktop, str(folder / "fixture.bimc"), "--control", "0", "--bench", str(scenario),
                                "--bench-out", str(folder / "desktop-frames.json")], env=environment, stdout=log, stderr=log)
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
        assert process.wait(timeout=90) == 0, "Desktop failed"
        assert all((folder / name).is_file() for name in ["desktop-plan.png", "desktop-3d.png"])
        print("Connected MCP and native window captures passed; isolated desktop closed.")
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
