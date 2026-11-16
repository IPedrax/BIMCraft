# Writes places.tsv from Natural Earth's countries (ne_50m_admin_0_countries) and populated places
# (ne_10m_populated_places_simple), both .geojson in the folder given: see README.md.
import json, sys
d, out = sys.argv[1], sys.argv[2]
def clean(s):
    return " ".join((s or "").replace("\t", " ").replace("|", " ").split())
rows = ["# Natural Earth (public domain): countries and populated places, for finding a map system by place. See README.md."]
countries = [f["properties"] for f in json.load(open(f"{d}/ne_50m_admin_0_countries.geojson"))["features"]]
for c in sorted(countries, key=lambda c: c["NAME_EN"] or c["NAME"]):
    iso = c["ISO_A2_EH"]
    if not iso or iso == "-99":
        continue
    names = []
    for k in ["NAME_EN", "NAME", "NAME_LONG", "NAME_PT", "NAME_ES", "NAME_FR", "NAME_DE", "NAME_IT"]:
        n = clean(c.get(k))
        if n and n not in names:
            names.append(n)
    rows.append(f"C\t{iso}\t{c['LABEL_X']:.3f}\t{c['LABEL_Y']:.3f}\t{'|'.join(names)}")
places = [f["properties"] for f in json.load(open(f"{d}/ne_10m_populated_places_simple.geojson"))["features"]]
for p in sorted(places, key=lambda p: -(p["pop_max"] or 0)):
    iso = p["iso_a2"]
    if not iso or iso == "-99":
        continue
    name, ascii_ = clean(p["name"]), clean(p["nameascii"])
    rows.append(f"P\t{iso}\t{p['longitude']:.3f}\t{p['latitude']:.3f}\t{name}\t{ascii_ if ascii_ != name else ''}\t{clean(p['adm1name'])}")
open(out, "w", encoding="utf-8").write("\n".join(rows) + "\n")
print(len(rows), "rows")
