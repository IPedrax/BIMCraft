//! Open-stair user journeys through the CLI/MCP command interface.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use serde_json::{Value, json};

    fn stair(s: &mut Session, id: u64) -> Value {
        s.execute("elements", &json!({"category":"stair"})).unwrap()["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id)
            .unwrap()
            .clone()
    }

    #[test]
    fn open_construction_survives_native_and_ifc_save_reopen_and_can_be_closed_again() {
        let mut s = Session::new();
        let id = s
            .execute(
                "stair",
                &json!({"start":[0,0],"height":2880,"open":true,
            "landings":[{"at":8,"turn":90}],"railings":true}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        assert_eq!(stair(&mut s, id)["open"], true, "creating an open stair must not silently create solid construction");
        let guid = stair(&mut s, id)["guid"].clone();
        for extension in ["bimcraft", "ifc"] {
            let path = std::env::temp_dir().join(format!("bimcraft-open-stair-{}.{extension}", std::process::id()));
            s.execute("save", &json!({"path":path})).unwrap();
            let mut reopened = Session::new();
            reopened.execute("open", &json!({"path":path})).unwrap();
            let value = reopened.execute("elements", &json!({"category":"stair"})).unwrap()["elements"][0].clone();
            assert_eq!(value["open"], true, "{extension} must keep open construction");
            assert_eq!(value["guid"], guid, "stair identity survives {extension}");
            assert_eq!(value["risers"], 16);
            assert_eq!(value["landings"][0]["turn"], 90.0);
            let reopened_id = value["id"].as_u64().unwrap();
            reopened.execute("modify", &json!({"id":reopened_id,"open":false})).unwrap();
            assert_ne!(stair(&mut reopened, reopened_id)["open"], true);
            std::fs::remove_file(path).unwrap();
        }
    }

    // Sample the actual mesh sent to the desktop renderer, not private stair helper state.
    fn heights(s: &Session, id: u64, point: [f64; 2]) -> (f64, f64) {
        use bimcraft_core::{
            graphics::Look,
            view3d::{Scene, gpu::Mesh},
        };
        let mesh = Mesh::flat(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
        let mut z = Vec::new();
        for tri in mesh.vertices.as_chunks::<3>().0 {
            if tri[0].owner == 0 || mesh.owners[tri[0].owner as usize - 1].id != id {
                continue;
            }
            let p: Vec<[f64; 3]> = tri.iter().map(|v| std::array::from_fn(|k| f64::from(v.position[k]) + mesh.origin[k])).collect();
            let [x, y] = point;
            let d = (p[1][1] - p[2][1]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][1] - p[2][1]);
            if d.abs() < 1e-8 {
                continue;
            }
            let a = ((p[1][1] - p[2][1]) * (x - p[2][0]) + (p[2][0] - p[1][0]) * (y - p[2][1])) / d;
            let b = ((p[2][1] - p[0][1]) * (x - p[2][0]) + (p[0][0] - p[2][0]) * (y - p[2][1])) / d;
            let c = 1.0 - a - b;
            if a >= -1e-6 && b >= -1e-6 && c >= -1e-6 {
                z.push(a * p[0][2] + b * p[1][2] + c * p[2][2]);
            }
        }
        assert!(!z.is_empty(), "stair mesh must cover {point:?}");
        (z.iter().copied().fold(f64::INFINITY, f64::min), z.iter().copied().fold(f64::NEG_INFINITY, f64::max))
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 0.02, "expected {b}, got {a}");
    }

    #[test]
    fn treads_supports_and_landing_underside_survive_ifc_reopening() {
        for (case, angle, turn) in [(0, 0.0_f64, 0), (1, 37.0, 90), (2, 90.0, -90), (3, 180.0, 180), (4, -31.0, -180)] {
            let mut s = Session::new();
            let id = s
                .execute(
                    "stair",
                    &json!({"start":[200,300],"angle":angle,"height":2880,"open":true,
                "landings":[{"at":8,"turn":turn}]}),
                )
                .unwrap()["id"]
                .as_u64()
                .unwrap();
            let (sin, cos) = angle.to_radians().sin_cos();
            let point = |x: f64, y: f64| [200.0 + x * cos - y * sin, 300.0 + x * sin + y * cos];
            let check = |s: &Session, id| {
                let centre = heights(s, id, point(980.0, 0.0));
                close(centre.0, 680.0);
                close(centre.1, 720.0);
                for side in [-494.0, 494.0] {
                    let support = heights(s, id, point(980.0, side));
                    close(support.0, 520.0);
                    close(support.1, 770.0);
                }
                let pad = heights(s, id, point(2460.0, 0.0));
                close(pad.0, 1190.0);
                close(pad.1, 1440.0);
            };
            check(&s, id);
            let path = std::env::temp_dir().join(format!("bimcraft-open-mesh-{}-{case}.ifc", std::process::id()));
            s.execute("save", &json!({"path":path})).unwrap();
            let mut reopened = Session::new();
            reopened.execute("open", &json!({"path":path})).unwrap();
            let v = reopened.execute("elements", &json!({"category":"stair"})).unwrap();
            assert_eq!(v["count"], 1);
            check(&reopened, v["elements"][0]["id"].as_u64().unwrap());
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn two_riser_open_flights_keep_their_direction_and_height_on_ifc_import() {
        // A single tread cannot establish the run direction from tread centres. In the shallow
        // case the stringer's sloped top also points almost upward, but it is not a tread.
        for (height, riser, tread) in [(720.0, 180.0, 280.0), (200.0, 50.0, 1000.0)] {
            let mut s = Session::new();
            s.execute(
                "stair",
                &json!({"start":[100,200],"angle":33,"height":height,"riser":riser,"tread":tread,"open":true,
                "landings":[{"at":2,"turn":90}]}),
            )
            .unwrap();
            let path = std::env::temp_dir().join(format!("bimcraft-short-open-{}.ifc", std::process::id()));
            s.execute("save", &json!({"path":path})).unwrap();
            s.execute("open", &json!({"path":path})).unwrap();
            let v = s.execute("elements", &json!({"category":"stair"})).unwrap()["elements"][0].clone();
            assert_eq!(v["open"], true);
            assert_eq!(v["risers"], 4);
            close(v["angle"].as_f64().unwrap(), 33.0);
            close(v["width"].as_f64().unwrap(), 1000.0);
            close(v["tread"].as_f64().unwrap(), tread);
            close(v["height"].as_f64().unwrap(), height);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn format18_stairs_keep_solid_geometry_guids_and_unknown_field_reporting() {
        use bimcraft_core::model::{FORMAT, Model};
        let mut s = Session::new();
        let id = s.execute("stair", &json!({"start":[0,0],"height":2880,"landings":[{"at":8,"turn":90}]})).unwrap()["id"].as_u64().unwrap();
        let original = bimcraft_core::ifc::export(&s.model);
        let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        file["format"] = json!(18);
        file["elements"][id.to_string()]["future_note"] = json!("left out");
        let (m, left) = Model::read(&file.to_string()).unwrap();
        assert_eq!(m.format, FORMAT);
        assert_eq!(m.elements.guid(id), s.model.elements.guid(id));
        assert_eq!(m.library, s.model.library, "migration preserves library identities and references");
        assert_eq!(left, vec![format!("/elements/{id}/future_note")]);
        assert_eq!(
            bimcraft_core::ifc::export(&m).split_once("DATA;").unwrap().1,
            original.split_once("DATA;").unwrap().1,
            "migration must not change closed IFC geometry or GUIDs"
        );
        file["format"] = json!(FORMAT + 1);
        assert!(Model::read(&file.to_string()).unwrap_err().contains("newer BIMCraft"));
        assert_eq!(heights(&s, id, [980.0, 0.0]).0, 0.0);
    }

    #[test]
    fn invalid_open_parameters_roll_back_and_modify_undo_preserves_identity() {
        let mut s = Session::new();
        let id = s.execute("stair", &json!({"start":[0,0],"height":2880})).unwrap()["id"].as_u64().unwrap();
        let before = s.model.to_json().unwrap();
        for params in [
            json!({"start":[0,0],"open":"yes"}),
            json!({"start":[0,0],"open":true,"width":30}),
            json!({"start":[0,0],"open":true,"height":80,"riser":40}),
        ] {
            assert!(s.execute("stair", &params).is_err());
            assert_eq!(s.model.to_json().unwrap(), before);
        }
        s.execute("modify", &json!({"id":id,"open":true})).unwrap();
        close(heights(&s, id, [980.0, 0.0]).0, 680.0);
        assert!(s.execute("modify", &json!({"id":id,"width":20})).is_err());
        assert_eq!(stair(&mut s, id)["width"], 1000.0);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.to_json().unwrap(), before);
        s.execute("redo", &json!({})).unwrap();
        assert_eq!(stair(&mut s, id)["open"], true);
    }
}
