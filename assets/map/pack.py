import json, struct, sys
d = sys.argv[1]; out = sys.argv[2]
def lines(path, closed):
    res = []
    for f in json.load(open(path))["features"]:
        g = f["geometry"]; t = g["type"]; c = g["coordinates"]
        if t == "Polygon": rings = c
        elif t == "MultiPolygon": rings = [r for p in c for r in p]
        elif t == "LineString": rings = [c]
        elif t == "MultiLineString": rings = c
        else: continue
        for r in rings:
            pts = []
            for lon, lat, *_ in r:
                q = (round(lon * 100), round(lat * 100))
                if not pts or pts[-1] != q: pts.append(q)
            if len(pts) >= 2: res.append((closed, pts))
    return res
all_ = lines(f"{d}/ne_110m_land.geojson", 1) + lines(f"{d}/ne_110m_admin_0_boundary_lines_land.geojson", 0)
b = bytearray(b"BCWM1")
b += struct.pack("<I", len(all_))
n = 0
for closed, pts in all_:
    b += struct.pack("<BH", closed, len(pts)); n += len(pts)
    for x, y in pts: b += struct.pack("<hh", x, y)
open(out, "wb").write(b)
print(len(all_), "lines", n, "points", len(b), "bytes")
