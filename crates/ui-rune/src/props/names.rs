//! The names of an element's category, of one (`Wall`) and of several (`Walls`), in the interface's language: the Properties title
//! ("Wall (1)", "Walls (3)") and the filter of a mixed selection. A copy of egui's `lang::category` (a duplicate to fold into the core,
//! with the table of rows, the first time egui adopts it); the catalogs it reads are egui's, so its words are translated already.

use bimcraft_i18n::msg;

use crate::lang::{t, tc};

/// (the category's key, its name of one, its name of several).
const NAMES: &[(&str, &str, &str)] = &[
    ("wall", msg!("category", "Wall"), msg!("category-plural", "Walls")),
    ("door", msg!("category", "Door"), msg!("category-plural", "Doors")),
    ("window", msg!("category", "Window"), msg!("category-plural", "Windows")),
    ("floor", msg!("category", "Floor"), msg!("category-plural", "Floors")),
    ("room", msg!("category", "Room"), msg!("category-plural", "Rooms")),
    ("level", msg!("category", "Level"), msg!("category-plural", "Levels")),
    ("grid", msg!("category", "Grid"), msg!("category-plural", "Grids")),
    ("dimension", msg!("category", "Dimension"), msg!("category-plural", "Dimensions")),
    ("tag", msg!("category", "Tag"), msg!("category-plural", "Tags")),
    ("section", msg!("category", "Section"), msg!("category-plural", "Sections")),
    ("elevation", msg!("category", "Elevation"), msg!("category-plural", "Elevations")),
    ("sheet", msg!("category", "Sheet"), msg!("category-plural", "Sheets")),
    ("viewport", msg!("category", "Viewport"), msg!("category-plural", "Viewports")),
    ("column", msg!("category", "Column"), msg!("category-plural", "Columns")),
    ("component", msg!("category", "Component"), msg!("category-plural", "Components")),
    ("furniture", msg!("category", "Furniture"), msg!("category-plural", "Furniture")),
    ("casework", msg!("category", "Casework"), msg!("category-plural", "Casework")),
    ("plumbing", msg!("category", "Plumbing Fixture"), msg!("category-plural", "Plumbing Fixtures")),
    ("drain", msg!("category", "Drain"), msg!("category-plural", "Drains")),
    ("pipefitting", msg!("category", "Pipe Fitting"), msg!("category-plural", "Pipe Fittings")),
    ("pipeaccessory", msg!("category", "Pipe Accessory"), msg!("category-plural", "Pipe Accessories")),
    ("equipment", msg!("category", "Electrical Equipment"), msg!("category-plural", "Electrical Equipment")),
    ("outlet", msg!("category", "Outlet"), msg!("category-plural", "Outlets")),
    ("switch", msg!("category", "Switch"), msg!("category-plural", "Switches")),
    ("airterminal", msg!("category", "Air Terminal"), msg!("category-plural", "Air Terminals")),
    ("earthing", msg!("category", "Earthing Electrode"), msg!("category-plural", "Earthing Electrodes")),
    ("inspectionbox", msg!("category", "Inspection Box"), msg!("category-plural", "Inspection Boxes")),
    ("lighting", msg!("category", "Lighting Fixture"), msg!("category-plural", "Lighting Fixtures")),
    ("generic", msg!("category", "Generic Component"), msg!("category-plural", "Generic Components")),
    ("dataoutlet", msg!("category", "Data Outlet"), msg!("category-plural", "Data Outlets")),
    ("voiceoutlet", msg!("category", "Voice Outlet"), msg!("category-plural", "Voice Outlets")),
    ("patchpanel", msg!("category", "Patch Panel"), msg!("category-plural", "Patch Panels")),
    ("rack", msg!("category", "Rack"), msg!("category-plural", "Racks")),
    ("site", msg!("category", "Site"), msg!("category-plural", "Site")),
    ("stair", msg!("category", "Stair"), msg!("category-plural", "Stairs")),
    ("roof", msg!("category", "Roof"), msg!("category-plural", "Roofs")),
    ("ceiling", msg!("category", "Ceiling"), msg!("category-plural", "Ceilings")),
    ("shaft", msg!("category", "Shaft Opening"), msg!("category-plural", "Shaft Openings")),
    ("mass", msg!("category", "Mass"), msg!("category-plural", "Masses")),
    ("material", msg!("category", "Material"), msg!("category-plural", "Materials")),
    ("toposolid", msg!("category", "Toposolid"), msg!("category-plural", "Toposolids")),
    ("region", msg!("category", "Filled Region"), msg!("category-plural", "Filled Regions")),
    ("massfloor", msg!("category", "Mass Floor"), msg!("category-plural", "Mass Floors")),
    ("framing", msg!("category", "Framing"), msg!("category-plural", "Framing Types")),
    ("framingmember", msg!("category", "Framing Member"), msg!("category-plural", "Framing Members")),
    ("ramp", msg!("category", "Ramp"), msg!("category-plural", "Ramps")),
    ("railing", msg!("category", "Railing"), msg!("category-plural", "Railings")),
    ("propertyline", msg!("category", "Property Line"), msg!("category-plural", "Property Lines")),
    ("curtainpanel", msg!("category", "Curtain Panel"), msg!("category-plural", "Curtain Panels")),
    ("mullion", msg!("category", "Mullion"), msg!("category-plural", "Mullions")),
    ("cornermullion", msg!("category", "Corner Mullion"), msg!("category-plural", "Corner Mullions")),
    ("roomseparator", msg!("category", "Room Separator"), msg!("category-plural", "Room Separators")),
    ("beam", msg!("category", "Beam"), msg!("category-plural", "Beams")),
    ("structuralcolumn", msg!("category", "Structural Column"), msg!("category-plural", "Structural Columns")),
    ("footing", msg!("category", "Footing"), msg!("category-plural", "Footings")),
    ("stripfooting", msg!("category", "Strip Footing"), msg!("category-plural", "Strip Footings")),
    ("pipe", msg!("category", "Pipe"), msg!("category-plural", "Pipes")),
    ("duct", msg!("category", "Duct"), msg!("category-plural", "Ducts")),
    ("conduit", msg!("category", "Conduit"), msg!("category-plural", "Conduits")),
    ("cabletray", msg!("category", "Cable Tray"), msg!("category-plural", "Cable Trays")),
    ("wire", msg!("category", "Wire"), msg!("category-plural", "Wires")),
    ("cable", msg!("category", "Cable"), msg!("category-plural", "Cables")),
    ("downconductor", msg!("category", "Down Conductor"), msg!("category-plural", "Down Conductors")),
    ("groundconductor", msg!("category", "Earthing Conductor"), msg!("category-plural", "Earthing Conductors")),
    ("text", msg!("category", "Text Note"), msg!("category-plural", "Text Notes")),
    ("line", msg!("category", "Detail Line"), msg!("category-plural", "Lines")),
    ("spot", msg!("category", "Spot Elevation"), msg!("category-plural", "Spot Elevations")),
    ("spotcoordinate", msg!("category", "Spot Coordinate"), msg!("category-plural", "Spot Coordinates")),
    ("schedule", msg!("category", "Schedule"), msg!("category-plural", "Schedules")),
    ("view3d", msg!("category", "3D View"), msg!("category-plural", "3D Views")),
    ("link", msg!("category", "Linked Model"), msg!("category-plural", "Linked Models")),
    ("pointcloud", msg!("category", "Point Cloud"), msg!("category-plural", "Point Clouds")),
    ("cadimport", msg!("category", "CAD Import"), msg!("category-plural", "CAD Imports")),
    ("multicategory", msg!("category", "Multi-Category"), msg!("category-plural", "Multi-Category")),
    ("circuit", msg!("category", "Circuit"), msg!("category-plural", "Circuits")),
    ("project", msg!("category", "Project Information"), msg!("category-plural", "Project Information")),
    ("modelgroup", msg!("category", "Model Group"), msg!("category-plural", "Model Groups")),
    ("detailgroup", msg!("category", "Detail Group"), msg!("category-plural", "Detail Groups")),
    ("repeatingdetail", msg!("category", "Repeating Detail"), msg!("category-plural", "Repeating Details")),
];

/// `category`'s name: of one, or of `several`; the key itself for a category with no name.
pub fn category(category: &str, several: bool) -> String {
    let Some(&(_, one, many)) = NAMES.iter().find(|(key, ..)| *key == category) else { return category.to_string() };
    let (context, english) = if several { ("category-plural", many) } else { ("category", one) };
    // A name the catalogs have only as a plain message (Casework, Outlets...) is the plain one.
    match tc(context, english) {
        named if named != english => named.to_string(),
        _ => t(english).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_category_is_named_of_one_and_of_several_and_an_unnamed_one_is_its_key() {
        assert_eq!((category("wall", false), category("wall", true)), ("Wall".to_string(), "Walls".to_string()));
        assert_eq!(category("shaft", false), "Shaft Opening");
        assert_eq!(category("stripfooting", true), "Strip Footings");
        assert_eq!(category("something-new", true), "something-new");
    }

    #[test]
    fn every_category_the_table_has_rows_for_is_named() {
        for key in
            ["wall", "door", "window", "room", "floor", "ceiling", "roof", "stair", "ramp", "railing", "column", "beam", "level", "grid"]
        {
            assert_ne!(category(key, false), key, "{key} has a name");
        }
    }
}
