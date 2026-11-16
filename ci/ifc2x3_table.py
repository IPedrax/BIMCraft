"""Print the table `crates/core/src/ifc/v2x3/table.rs` holds: how each entity the IFC4 writer makes is written in IFC2x3,
read from the IFC2x3 TC1 and IFC4 ADD2 TC1 schemas that IfcOpenShell carries.

Usage: python ci/ifc2x3_table.py > crates/core/src/ifc/v2x3/table.rs, from the repository's root, then `cargo fmt`.

The entities are those `crates/core/src/ifc.rs` names, the classes of `crates/core/src/ifc/classes.rs` with their type
objects, and the few only IFC2x3 has that the writer makes for it. A row: the entity as IFC4 names it; the IFC2x3 entity
it is written as (itself, or the nearest supertype IFC2x3 can make; a proxy for an element with none, nothing for
a type object IFC2x3 lacks or anything else); how many attributes that one has; `E` for an element, `R` for anything else with an owner history, `-`
without one; and its attributes from the third on that are an enumeration (with its values) or required (with none).
"""

import re

import ifcopenshell.ifcopenshell_wrapper as w

OLD, NEW = w.schema_by_name("IFC2X3"), w.schema_by_name("IFC4")
ONLY_2X3 = ["IFCDOORSTYLE", "IFCWINDOWSTYLE"]


def entities(schema):
    return {d.name().upper(): d for d in schema.declarations() if isinstance(d, w.entity)}


def under(e, name):
    while e is not None:
        if e.name() == name:
            return True
        e = e.supertype()
    return False


def home(name, old, new):
    """The IFC2x3 entity `name` is written as."""
    if name in old and not old[name].is_abstract():
        return name
    if name in new and under(new[name], "IfcTypeObject"):
        return ""  # A type object IFC2x3 has no class for: a door's and a window's are styles there, the rest go without.
    e = new[name].supertype() if name in new else None
    while e is not None:
        n = e.name().upper()
        if n in old and not old[n].is_abstract():
            return n
        e = e.supertype()
    if name in new and under(new[name], "IfcElementComponent"):
        return "IFCDISCRETEACCESSORY"
    if name in new and under(new[name], "IfcElement"):
        return "IFCBUILDINGELEMENTPROXY"
    return ""


def values(attribute):
    """An attribute's enumeration's values; none for any other type."""
    t = attribute.type_of_attribute()
    if isinstance(t, w.named_type) and isinstance(t.declared_type(), w.enumeration_type):
        return list(t.declared_type().enumeration_items())
    return None


def main():
    old, new = entities(OLD), entities(NEW)
    text = open("crates/core/src/ifc.rs").read() + open("crates/core/src/ifc/classes.rs").read()
    names = set(re.findall(r"IFC[A-Z0-9]+", text))
    names |= {n + "TYPE" for n in names}
    names = sorted(n for n in names if n in new or n in old) + ONLY_2X3
    rows = []
    for name in sorted(set(names)):
        to = home(name, old, new)
        if not to:
            rows.append((name, "", 0, "-", []))
            continue
        attributes = list(old[to].all_attributes())
        rooted = len(attributes) > 1 and attributes[1].name() == "OwnerHistory"
        kind = "E" if under(old[to], "IfcElement") else "R" if rooted else "-"
        specs = []
        for i, a in enumerate(attributes):
            if i < 2 and rooted:
                continue
            v = values(a)
            if v is not None:
                specs.append((i, not a.optional(), " ".join(v)))
            elif not a.optional():
                specs.append((i, True, ""))
        same = name in new and to == name and [x.name() for x in new[name].all_attributes()] == [x.name() for x in attributes]
        if same and not rooted and all(values(a) == values(b) and a.optional() >= b.optional()
                                       for a, b in zip(attributes, new[name].all_attributes())):
            continue  # Written in IFC2x3 as it is in IFC4.
        rows.append((name, to, len(attributes), kind, specs))
    print("//! Made by `ci/ifc2x3_table.py` from the IFC2x3 TC1 and IFC4 ADD2 TC1 schemas: see `super`.")
    print()
    print("use super::Row;")
    print()
    print(f"pub(super) static ROWS: [Row; {len(rows)}] = [")
    for name, to, count, kind, specs in rows:
        inner = ", ".join(f'({i}, {str(r).lower()}, "{v}")' for i, r, v in specs)
        print(f'    Row {{ from: "{name}", to: "{to}", count: {count}, kind: \'{kind}\', attributes: &[{inner}] }},')
    print("];")


if __name__ == "__main__":
    main()
