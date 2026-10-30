"""Change the walls of one type to another: all of them, or only those selected."""
__title__ = "Change Wall Type"
from typing import Annotated

import bimcraft as bc


def main(from_type: Annotated[str, bc.Library("type:wall")],
         to_type: Annotated[str, bc.Library("type:wall")],
         only_selected: bool = False):
    """Change every wall of one type to another type.

    from_type: the walls of this type change
    to_type: the type they become
    only_selected: change only the selected walls of that type
    """
    if from_type == to_type:
        return "Pick two different types."
    walls = [w for w in bc.query(category="wall") if w.get("type") == from_type]
    if only_selected:
        chosen = set(bc.selection())
        walls = [w for w in walls if w["id"] in chosen]
    if not walls:
        return "No walls of type %s%s." % (from_type, " are selected" if only_selected else "")
    ids = [w["id"] for w in walls]
    bc.modify(ids=ids, type=to_type)
    bc.select(ids)
    print("changed:", ", ".join(str(i) for i in ids))
    return "%d walls changed from %s to %s" % (len(ids), from_type, to_type)
