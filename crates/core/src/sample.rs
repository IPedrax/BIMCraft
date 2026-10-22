//! A small single-storey house built from commands: the demo (`bimcraft-cli run --sample`) and
//! a test of the whole command set.

mod highrise;
pub use highrise::highrise;

pub const SAMPLE: &str = r#"# Sample house: 10 x 7 m, four rooms under a hipped roof and their ceilings, furnished, dimensioned and tagged, with elevations, a section, and three sheets.
new {"name": "Sample House"}
grid {"start": [0, -1500], "end": [0, 8500]}
grid {"start": [6000, -1500], "end": [6000, 8500]}
grid {"start": [10000, -1500], "end": [10000, 8500]}
grid {"start": [-1500, 0], "end": [11500, 0], "name": "A"}
grid {"start": [-1500, 7000], "end": [11500, 7000]}
# Outer walls drawn clockwise, their brick outside; their tops follow the roof once it's on.
wall {"points": [[0, 0], [0, 7000], [10000, 7000], [10000, 0]], "closed": true, "type": "Exterior - 300mm"}
wall {"points": [[6000, 0], [6000, 7000]], "type": "Interior - 100mm"}
wall {"points": [[0, 4000], [6000, 4000]], "type": "Interior - 100mm"}
wall {"points": [[6000, 3500], [10000, 3500]], "type": "Interior - 100mm"}
# Doors and windows of the project's families (forms, plan swing, glass), and three of its own plain types.
door {"at": [2000, 0], "family": "Single Door", "type": "900 x 2100", "flip_facing": true}
door {"at": [4500, 4000], "type": "Single 800 x 2100"}
door {"at": [6000, 1750], "family": "Single Door", "type": "800 x 2100", "flip_hand": true}
door {"at": [6000, 5250], "type": "Single 800 x 2100", "flip_facing": true}
window {"at": [4300, 0], "family": "Casement Window", "type": "1200 x 1500"}
window {"at": [3000, 7000], "family": "Casement Window", "type": "1200 x 1500"}
window {"at": [8000, 7000], "family": "Fixed Window", "type": "1200 x 1500"}
window {"at": [10000, 5250], "type": "Fixed 600 x 900"}
window {"at": [8000, 0]}
floor {}
roof {}
modify {"ids": [100007, 100008, 100009, 100010], "attach_top": 100024}
room {"point": [3000, 2000], "name": "Living"}
room {"point": [1500, 5000], "name": "Kitchen"}
room {"point": [8400, 4200], "name": "Bedroom"}
room {"point": [7600, 2600], "name": "Study"}
# Overall sizes outside the walls, the grids' spacing, and every door and window tagged.
dimension {"refs": [[-150, 3500], [10150, 3500]], "at": [5000, -1300]}
dimension {"refs": [[5000, -150], [5000, 7150]], "at": [10900, 3500]}
dimension {"refs": [[0, 8000], [6000, 8000], [10000, 8000]], "at": [5000, 8000]}
tag {}
# The four building elevations, and a section across the living room and the study, looking north.
elevation {}
section {"start": [-1500, 3000], "end": [11500, 3000]}
# Two sheets: the ground floor plan, and the section with two of the elevations.
sheet {"name": "Ground Floor"}
place {"sheet": "A101", "view": "Level 1"}
sheet {"name": "Section and Elevations"}
place {"sheet": "A102", "view": "Section 1", "at": [120, 215]}
place {"sheet": "A102", "view": "South", "at": [120, 110]}
place {"sheet": "A102", "view": "East", "at": [300, 160]}
# A note and a spot elevation on the ground floor, the room schedule beside its plan, and the 3D view on a sheet of its own.
text {"text": "Oak floor", "at": [6300, 900]}
spot {"at": [1000, 1000]}
place {"sheet": "A101", "view": "Room Schedule"}
sheet {"name": "3D View"}
place {"sheet": "A103", "view": "{3D}", "scale": 75}
# A ceiling in every room, 2600 mm up, tiled in the kitchen.
ceiling {"point": [3000, 2000]}
ceiling {"point": [3000, 5500], "type": "Tile 600 x 600"}
ceiling {"point": [8000, 5250]}
ceiling {"point": [8000, 1750]}
# Furniture from the project's families: a sofa; a dining table, its chairs and a base cabinet; a bed and a wardrobe; a desk and its chair.
component {"family": "Sofa", "type": "2000 x 900", "at": [3000, 3950]}
component {"family": "Table", "type": "1200 x 800", "at": [4300, 5900]}
component {"family": "Chair", "at": [4000, 5225], "angle": 180}
component {"family": "Chair", "at": [4600, 5225], "angle": 180}
component {"family": "Chair", "at": [4000, 6575]}
component {"family": "Chair", "at": [4600, 6575]}
component {"family": "Base Cabinet", "type": "1800 x 600", "at": [1200, 6850]}
component {"family": "Bed", "type": "1600 x 2000", "at": [8000, 6850]}
component {"family": "Wardrobe", "type": "1200 x 600", "at": [6050, 4200], "angle": 90}
component {"family": "Desk", "type": "1400 x 700", "at": [9850, 1500], "angle": -90}
component {"family": "Chair", "at": [8875, 1500], "angle": 90}
"#;

#[cfg(test)]
mod tests {
    use crate::engine::Session;
    use serde_json::json;

    #[test]
    fn highrise_is_an_original_valid_twenty_storey_project() {
        let mut s = Session::new();
        s.execute("highrise", &json!({})).unwrap();
        assert_eq!(s.model.levels().len(), 21);
        assert!((9_000..11_000).contains(&s.model.elements.len()));
        s.model.check().unwrap();
        for category in ["wall", "window", "floor", "ceiling", "structuralcolumn", "beam", "component"] {
            assert!(s.model.elements.values().any(|e| e.category() == category), "{category}");
        }
        let text = s.model.to_json().unwrap();
        let reopened = crate::model::Model::from_json(&text).unwrap();
        reopened.check().unwrap();
        assert_eq!(reopened.elements.len(), s.model.elements.len());
        assert!(s.path.is_none());
        assert!(s.history().is_empty());
    }

    #[test]
    fn highrise_rejects_unbounded_sizes_without_replacing_the_project() {
        let mut s = Session::new();
        let before = s.model.clone();
        for size in [json!(0), json!(81), json!(-1), json!(1.5), json!("20")] {
            assert!(s.execute("highrise", &json!({"storeys": size})).is_err());
            assert!(std::sync::Arc::ptr_eq(&before, &s.model));
        }
    }
}
