"""Export the two one-slot W1 elements inside the matching Unreal Editor process."""
import ctypes as C
import json
from pathlib import Path
import struct
import xml.etree.ElementTree as ET


class Part(C.Structure):
    """C ABI shared with bridge.cpp. TCHAR is UTF-16 on the tested editor build."""
    _fields_ = [
        (name, C.c_char_p) for name in ("actor", "mesh", "material", "label")
    ] + [
        (name, C.POINTER(C.c_float)) for name in ("positions", "normals", "uvs")
    ] + [
        ("indices", C.POINTER(C.c_int32)), ("vertex_count", C.c_int32),
        ("index_count", C.c_int32), ("pivot", C.c_double * 3), ("rgba", C.c_float * 4),
    ]


def wide(value):
    return str(value).encode("utf-16-le") + b"\0\0"


def read_glb(path):
    data = path.read_bytes()
    magic, version, size, json_size, kind = struct.unpack_from("<4sIII4s", data)
    if (magic, version, size, kind) != (b"glTF", 2, len(data), b"JSON"):
        raise ValueError(f"Expected a GLB 2.0 file at {path}")
    end = 20 + json_size
    header = json.loads(data[20:end])
    bin_size, kind = struct.unpack_from("<I4s", data, end)
    binary = data[end + 8:]
    if kind != b"BIN\0" or len(binary) != bin_size:
        raise ValueError("Expected one embedded binary chunk")

    def accessor(index):
        entry = header["accessors"][index]
        formats = {("VEC3", 5126): "<fff", ("SCALAR", 5125): "<I"}
        fmt = formats[entry["type"], entry["componentType"]]
        view = header["bufferViews"][entry["bufferView"]]
        start = view.get("byteOffset", 0) + entry.get("byteOffset", 0)
        stride = view.get("byteStride", struct.calcsize(fmt))
        return [struct.unpack_from(fmt, binary, start + i * stride) for i in range(entry["count"])]

    return header, accessor


def floats(rows):
    values = [value for row in rows for value in row]
    return (C.c_float * len(values))(*values)


def write_scene(model_path, glb_path, output, library, version):
    model = json.loads(model_path.read_bytes())
    header, accessor = read_glb(glb_path)
    nodes = header["nodes"]
    if sorted(n["extras"]["category"] for n in nodes) != ["wall", "window"]:
        raise ValueError("The W1 fixture needs one wall and one window")

    parts = []
    for node in nodes:
        if node.keys() & {"scale", "rotation", "translation", "matrix", "children"}:
            raise ValueError("The W1 adapter expects flat nodes with model-space vertices")
        element = model["elements"][str(node["extras"]["id"])]
        if element["guid"] != node["extras"]["guid"]:
            raise ValueError("The native document and GLB must describe the same revision")
        primitive, = header["meshes"][node["mesh"]]["primitives"]
        points = accessor(primitive["attributes"]["POSITION"])
        normals = accessor(primitive["attributes"]["NORMAL"])
        indices = [value for (value,) in accessor(primitive["indices"])]
        if (primitive.get("mode", 4) != 4 or len(indices) % 3 or not indices or
                len(points) != len(normals) or max(indices) >= len(points)):
            raise ValueError("Expected indexed triangles with a normal for each vertex")
        key = f"{model['uid']:016x}_" + element["guid"].replace("-", "")
        role = element["category"]
        if role == "wall":
            layer, = model["types"]["wall"]["Timber 2x4 - 16 in OC"]["layers"]
            slot = layer["material"] or layer["guid"]
            origin = (0, 0, 0)
            label = "Exterior Wall"
        elif role == "window":
            slot = element["type"]
            origin = (element["offset"] / 10, 0, element["sill"] / 10)
            label = element["mark"]
        else:
            raise ValueError(f"Unsupported fixture element: {role}")

        # GLB metres (X,Y,Z) to UE centimetres (X,Z,Y). Reflection reverses winding.
        positions = [(100*x-origin[0], 100*z-origin[1], 100*y-origin[2]) for x,y,z in points]
        directions = [(x, z, y) for x, y, z in normals]
        uvs = []
        for (x, y, z), normal in zip(positions, directions):
            axis = max(range(3), key=lambda i: abs(normal[i]))
            uvs.append(((y if axis == 0 else x) / 100, -(y if axis == 2 else z) / 100))
        rgba = header["materials"][primitive["material"]]["pbrMetallicRoughness"]["baseColorFactor"]
        material = f"M_{model['uid']:016x}_{role}_{slot.replace('-', '')}"
        parts.append(Part(wide("A_" + key), wide("G_" + key), wide(material), wide(label),
                          floats(positions), floats(directions), floats(uvs),
                          (C.c_int32 * len(indices))(*indices), len(points), len(indices),
                          (C.c_double * 3)(*origin), (C.c_float * 4)(*rgba)))
    (output / "CON03_W1_Assets").mkdir(parents=True, exist_ok=True)
    bridge = C.CDLL(str(library))
    bridge.WriteW1Scene.argtypes = [C.c_char_p, C.c_char_p, C.POINTER(Part), C.c_int32]
    bridge.WriteW1Scene.restype = C.c_int
    if bridge.WriteW1Scene(wide(output), wide(version), (Part * len(parts))(*parts), len(parts)):
        raise RuntimeError("Datasmith mesh export failed")
    scene = output / "CON03_W1.udatasmith"
    assets = ET.parse(scene).findall(".//StaticMesh/file")
    if len(assets) != 2 or not all((output / Path(f.attrib["path"].replace("\\", "/"))).is_file()
                                  for f in assets):
        raise RuntimeError("Expected two mesh payloads referenced by the scene")
    return scene
