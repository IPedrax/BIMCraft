"""Independently check the synthetic MCP exports with IfcOpenShell.

Usage: python3 ifc_probe.py OUTPUT_DIR
Requires ifcopenshell and pytest (for EXPRESS rules).
"""
import json
from pathlib import Path
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.validate


folder = Path(sys.argv[1])
results = []
for path in sorted(folder.glob("*.ifc")):
    model = ifcopenshell.open(str(path))
    logger = ifcopenshell.validate.json_logger()
    ifcopenshell.validate.validate(model, logger, express_rules=True)
    assert not logger.statements, logger.statements
    settings = ifcopenshell.geom.settings()
    settings.set(settings.USE_WORLD_COORDS, True)
    elements = []
    for element in model.by_type("IfcElement"):
        shape = ifcopenshell.geom.create_shape(settings, element)
        vertices = shape.geometry.verts
        size = [max(vertices[i::3]) - min(vertices[i::3]) for i in range(3)]
        expected = [4.0, 0.128, 3.0] if element.is_a("IfcWall") else [0.125, 0.5, 2.0]
        assert all(abs(a - b) < 1e-8 for a, b in zip(size, expected)), (element.is_a(), size, expected)
        elements.append({"class": element.is_a(), "size_m": size})
    assert len(elements) == 2, elements
    results.append({"file": path.name, "schema": model.schema, "findings": 0, "elements": elements})
assert results, "No IFC exports found"
summary = {"ifcopenshell": ifcopenshell.version, "exports": results}
(folder / "ifc-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
