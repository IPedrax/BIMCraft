"""Synthetic precision checks through the real CLI's stdio MCP transport.

Usage: python3 mcp_probe.py CLI OUTPUT_DIR [--connect PORT]
For --connect, launch the isolated desktop on fixture.bimc first.
"""
import argparse
import json
from pathlib import Path
import selectors
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("cli")
    parser.add_argument("output", type=Path)
    parser.add_argument("--connect")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    mode = "connected" if args.connect else "standalone"
    transcript = []
    command = [args.cli, "mcp"] + (["--connect", args.connect] if args.connect else [])
    with (args.output / f"{mode}-stderr.log").open("w") as errors:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, text=True)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)

        def rpc(method, params):
            request = {"jsonrpc": "2.0", "id": len(transcript) + 1, "method": method, "params": params}
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            assert selector.select(45), f"timeout: {method}"
            line = process.stdout.readline()
            assert line, "MCP process exited"
            reply = json.loads(line)
            transcript.append({"request": request, "reply": reply})
            assert "error" not in reply, reply
            return reply["result"]

        def tool(name, arguments, error=False):
            result = rpc("tools/call", {"name": name, "arguments": arguments})
            assert bool(result.get("isError")) == error, result
            text = result["content"][0]["text"]
            return text if error else json.loads(text)

        def execute(name, params=None, error=False):
            return tool("execute", {"command": name, "params": params or {}}, error)

        def values():
            return tool("query_elements", {"category": "component"})["elements"][0]["values"]

        def wall():
            return tool("inspect_model", {})["types"]["wall"]["Precision Wall"]

        try:
            rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                               "clientInfo": {"name": "precision-regression", "version": "1"}})
            if not args.connect:
                tool("script", {"text": Path(__file__).with_name("fixture.txt").read_text(), "atomic": True})
                execute("save", {"path": str((args.output / "fixture.bimc").resolve())})
            else:
                tool("desktop_context", {})
            assert values()["Gap"] == 3 and values()["Width"] == 55
            assert wall()["thickness"] == 58
            execute("family", {"name": "Precision Block", "types": {"A": {"Gap": 3, "Width": 125}}})
            assert values()["Gap"] == 3 and values()["Width"] == 125
            execute("undo")
            assert values()["Width"] == 55
            execute("redo")
            assert values()["Width"] == 125
            execute("type", {"category": "wall", "name": "Precision Wall", "layers": [
                {"function": "finish", "thickness": 3}, {"function": "structure", "thickness": 125}]})
            assert wall()["thickness"] == 128
            execute("type", {"category": "wall", "name": "Precision Wall", "thickness": -1}, error=True)
            assert wall()["thickness"] == 128
            execute("undo")
            assert wall()["thickness"] == 58
            execute("redo")
            assert wall()["thickness"] == 128
            saved = str((args.output / f"{mode}.bimc").resolve())
            execute("save", {"path": saved})
            execute("save", {"path": str((args.output / f"{mode}.ifc").resolve())})
            if not args.connect:
                execute("open", {"path": saved})
                assert values()["Gap"] == 3 and values()["Width"] == 125
                assert [layer["thickness"] for layer in wall()["layers"]] == [3, 125]
            summary = {"mode": mode, "passed": True, "gap_mm": values()["Gap"],
                       "width_mm": values()["Width"], "wall_mm": wall()["thickness"], "saved": saved}
            (args.output / f"{mode}-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
            print(json.dumps(summary))
        finally:
            (args.output / f"{mode}-transcript.json").write_text(json.dumps(transcript, indent=2) + "\n")
            process.stdin.close()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)


if __name__ == "__main__":
    main()
