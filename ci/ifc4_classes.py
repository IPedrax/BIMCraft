"""Print IFC4's element classes as `crates/core/src/ifc/classes.rs` holds them: read from the IFC4 ADD2 TC1 schema
(ISO 16739-1:2018) that IfcOpenShell carries.

Usage: python ci/ifc4_classes.py > table.txt, then put the rows in `CLASSES`.

A class is every IfcElement that isn't abstract, a feature (an opening, a projection, a surface feature) or a virtual
element. Its row: its name as STEP writes it; what follows its Tag, `P` its predefined type, `$` an attribute left
unset, `*` a derived one and `.NOTDEFINED.` a required enumeration; the same for its type object after ElementType,
where the schema has a `<CLASS>TYPE` that isn't abstract; and its predefined types. A class with a required attribute
that can't be written so is left out, as is a type object with one.
"""

import ifcopenshell.ifcopenshell_wrapper as w

S = w.schema_by_name("IFC4")


def subtypes(e):
    out = [e]
    for s in e.subtypes():
        out += subtypes(s)
    return out


def under(e, name):
    while e is not None:
        if e.name() == name:
            return True
        e = e.supertype()
    return False


def items(attribute):
    """The enumeration an attribute is of, if it is one."""
    t = attribute.type_of_attribute()
    for _ in range(4):
        if isinstance(t, w.enumeration_type):
            return list(t.enumeration_items())
        if isinstance(t, w.named_type):
            t = t.declared_type()
        elif isinstance(t, w.type_declaration):
            t = t.declared_type()
        else:
            return None
    return None


def after(e, start):
    """What follows attribute `start - 1`, and its predefined types; None if one can't be written."""
    parts, kinds = [], ""
    for i, (a, derived) in enumerate(zip(e.all_attributes(), e.derived())):
        if i < start:
            continue
        if derived:
            parts.append("*")
        elif a.name() == "PredefinedType":
            parts.append("P")
            kinds = " ".join(items(a) or [])
        elif a.optional():
            parts.append("$")
        elif "NOTDEFINED" in (items(a) or []):
            parts.append(".NOTDEFINED.")
        else:
            return None, None
    return ",".join(parts), kinds


def main():
    rows = []
    for e in sorted(subtypes(S.declaration_by_name("IfcElement")), key=lambda x: x.name().upper()):
        if e.is_abstract() or under(e, "IfcFeatureElement") or e.name() == "IfcVirtualElement":
            continue
        tail, kinds = after(e, 8)
        if tail is None:
            continue
        typed = None
        try:
            t = S.declaration_by_name(e.name() + "Type")
        except Exception:  # the schema has none
            t = None
        if t is not None and not t.is_abstract():
            typed, type_kinds = after(t, 9)
            if typed is not None and kinds and type_kinds != kinds:
                raise SystemExit(f"{e.name()}'s type takes other predefined types than it")
        typed = "None" if typed is None else f'Some("{typed}")'
        rows.append(f'    ("{e.name().upper()}", "{tail}", {typed}, "{kinds}"),')
    print(f"pub const CLASSES: [Class; {len(rows)}] = [")
    print("\n".join(rows))
    print("];")


if __name__ == "__main__":
    main()
