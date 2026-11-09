//! Wall location and attachment intent survive the same save/open commands used by CLI and MCP clients.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Guid, Location, Wall};
    use serde_json::json;

    fn walls(s: &Session) -> Vec<(Guid, Wall)> {
        s.model
            .elements
            .with_guids()
            .filter_map(|(_, guid, e)| match e {
                Element::Wall(w) => Some((guid, w.clone())),
                _ => None,
            })
            .collect()
    }

    fn reopen(s: &mut Session, extension: &str) -> Session {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("authoring.{extension}"));
        s.execute("save", &json!({"path":path})).unwrap();
        let mut read = Session::new();
        let opened = read.execute("open", &json!({"path":path})).unwrap();
        assert!(opened.get("left_out").is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)), "{opened}");
        assert!(opened.get("skipped").is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)), "{opened}");
        read
    }

    fn export(s: &mut Session) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wall.ifc");
        s.execute("save", &json!({"path":path})).unwrap();
        std::fs::read_to_string(path).unwrap()
    }

    fn open_ifc(text: &str) -> Session {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("external.ifc");
        std::fs::write(&path, text).unwrap();
        let mut read = Session::new();
        read.execute("open", &json!({"path":path})).unwrap();
        read
    }

    fn roof_wall() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points":[[0,0],[5000,0]],"height":2800}
            roof {"boundary":[[-500,-500],[5500,-500],[5500,3500],[-500,3500]],"gable":true}
            modify {"id":100002,"attach_top":100003}"#,
        )
        .unwrap();
        s
    }

    #[test]
    fn noncentral_location_lines_keep_the_drawn_endpoints_through_ifc_and_native_reopen() {
        let mut s = Session::new();
        s.script(r#"type {"category":"wall","name":"Layered","layers":[{"function":"finish","material":"Brick","thickness":40},{"function":"structure","material":"Concrete","thickness":180},{"function":"finish","material":"Plaster","thickness":20}]}"#).unwrap();
        for (i, location) in Location::ALL.into_iter().enumerate() {
            for (j, arc) in [0, 45, -60].into_iter().enumerate() {
                let y = (i * 3 + j) as f64 * 4000.0;
                let points = if j == 2 { json!([[4200, y], [200, y]]) } else { json!([[200, y], [4200, y]]) };
                s.execute("wall", &json!({"points":points,"type":"Layered","location":location.name(),"arc":arc})).unwrap();
            }
        }
        let expected = walls(&s);
        let mut read = reopen(&mut s, "ifc");
        for (guid, wall) in expected {
            let actual = read.model.elements.get(guid).unwrap();
            let Element::Wall(actual) = actual else { panic!("wall") };
            assert_eq!(actual.location, wall.location, "location line for {guid}");
            assert!(actual.start.dist(wall.start) < 0.002 && actual.end.dist(wall.end) < 0.002, "drawn endpoints for {guid}: {actual:?}");
            assert!((actual.arc - wall.arc).abs() < 0.00001);
        }
        assert_eq!(walls(&read), walls(&reopen(&mut read, "bimcraft")));
    }

    #[test]
    fn floor_attachments_keep_their_guids_after_ifc_and_native_reopen() {
        let mut s = Session::new();
        s.script(
            r#"level {"name":"Upper","elevation":4000}
            floor {"boundary":[[-1000,-1000],[6000,-1000],[6000,3000],[-1000,3000]],"offset":400}
            floor {"level":"Upper","boundary":[[-1000,-1000],[6000,-1000],[6000,3000],[-1000,3000]]}
            wall {"points":[[0,0],[5000,0]],"location":"exterior-finish","height":2800}
            modify {"id":100005,"attach_base":100003,"attach_top":100004}"#,
        )
        .unwrap();
        let expected = walls(&s);
        let mut read = reopen(&mut s, "ifc");
        for (guid, wall) in expected {
            let Element::Wall(actual) = read.model.elements.get(guid).unwrap() else { panic!("wall") };
            assert_eq!(actual.attach_top, wall.attach_top, "top attachment");
            assert_eq!(actual.attach_base, wall.attach_base, "base attachment");
            assert_eq!(actual.location, wall.location, "location line");
            assert!(actual.start.dist(wall.start) < 0.002 && actual.end.dist(wall.end) < 0.002);
        }
        assert_eq!(walls(&read), walls(&reopen(&mut read, "bimcraft")));
    }

    #[test]
    fn authored_endpoints_near_another_wall_are_not_extended_by_import_joining() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points":[[0,0],[4000,0]],"height":2800}
            wall {"points":[[3950,50],[3950,4000]],"height":2800,"location":"interior-finish"}"#,
        )
        .unwrap();
        let expected = walls(&s);
        let read = reopen(&mut s, "ifc");
        for (guid, wall) in expected {
            let Element::Wall(actual) = read.model.elements.get(guid).unwrap() else { panic!("wall") };
            assert_eq!((actual.start, actual.end, actual.location), (wall.start, wall.end, wall.location));
        }
    }

    #[test]
    fn explicit_roof_attachment_and_explicitly_unattached_metadata_override_the_geometry_guess() {
        let mut s = roof_wall();
        let (guid, original) = walls(&s).remove(0);
        let file = export(&mut s);
        let read = open_ifc(&file);
        assert_eq!(read.model.wall(guid).unwrap().attach_top, original.attach_top);
        // An external edit leaves the body alone but says the wall is no longer attached.
        let unattached = file.replace("'AttachTop'", "'OtherProperty'");
        let read = open_ifc(&unattached);
        assert_eq!(read.model.wall(guid).unwrap().attach_top, None);
        // Without BIMCraft's metadata, the original generic roof inference remains available.
        let generic = file.replace("'BIMCraft_Wall'", "'Other_Wall'");
        let read = open_ifc(&generic);
        assert_eq!(read.model.wall(guid).unwrap().attach_top, original.attach_top);
    }

    #[test]
    fn unknown_or_nontext_location_metadata_keeps_the_generic_body_reconstruction() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points":[[0,0],[5000,0]],"location":"exterior-finish","height":2800})).unwrap();
        let (guid, _) = walls(&s).remove(0);
        let file = export(&mut s);
        let generic = open_ifc(&file.replace("'BIMCraft_Wall'", "'Other_Wall'"));
        let expected = generic.model.wall(guid).unwrap();
        for value in ["IFCTEXT('future-location')", "IFCINTEGER(7)", "$"] {
            let read = open_ifc(&file.replace("IFCTEXT('exterior-finish')", value));
            let actual = read.model.wall(guid).unwrap();
            assert_eq!((actual.start, actual.end, actual.location), (expected.start, expected.end, Location::Centre));
        }
    }

    #[test]
    fn missing_or_wrong_kind_attachment_targets_leave_the_wall_present_and_unattached() {
        let mut s = roof_wall();
        let (guid, original) = walls(&s).remove(0);
        let file = export(&mut s);
        let target = bimcraft_core::ifc::compress(original.attach_top.unwrap().as_u128());
        let level = bimcraft_core::ifc::compress(original.level.as_u128());
        for value in ["invalid".to_string(), level, bimcraft_core::ifc::compress(123)] {
            let read = open_ifc(&file.replace(&format!("IFCIDENTIFIER('{target}')"), &format!("IFCIDENTIFIER('{value}')")));
            assert_eq!(read.model.wall(guid).unwrap().attach_top, None);
        }
    }

    #[test]
    fn reversed_layer_usage_keeps_the_generic_direction_and_centre_line() {
        let mut s = Session::new();
        s.script(r#"type {"category":"wall","name":"Layered","layers":[{"function":"finish","material":"Brick","thickness":40},{"function":"structure","material":"Concrete","thickness":180}]}
            wall {"points":[[0,0],[5000,0]],"type":"Layered","location":"exterior-finish","height":2800}"#).unwrap();
        let (guid, _) = walls(&s).remove(0);
        let file = export(&mut s).replace(".AXIS2.,.NEGATIVE.", ".AXIS2.,.POSITIVE.");
        let generic = open_ifc(&file.replace("'BIMCraft_Wall'", "'Other_Wall'"));
        let read = open_ifc(&file);
        let (actual, expected) = (read.model.wall(guid).unwrap(), generic.model.wall(guid).unwrap());
        assert_eq!((actual.start, actual.end, actual.location), (expected.start, expected.end, Location::Centre));
        assert!(actual.start.0 > actual.end.0);
    }
}
