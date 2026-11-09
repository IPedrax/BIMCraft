//! A separator drawn along an offset wall's face must meet its end without moving the drawn line.
#[cfg(test)]
mod tests {
    use bimcraft_core::engine::Session;
    use bimcraft_core::step::{self, V};
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
    }

    fn fixture(start: [f64; 2], reversed: bool, rotated: bool, bounding: bool) -> Session {
        let mut s = Session::new();
        let point = |[x, y]: [f64; 2]| if rotated { [-y, x] } else { [x, y] };
        let outer = [[0.0, 0.0], [9600.0, 0.0], [9600.0, 6400.0], [0.0, 6400.0]].map(point);
        run(&mut s, "wall", json!({"points": outer, "closed": true}));
        let mut partition = [[0.0, 3200.0], [4200.0, 3200.0]].map(point);
        let mut separator = [start, [9500.0, 3200.0]].map(point);
        if reversed {
            partition.reverse();
            separator.reverse();
        }
        run(
            &mut s,
            "wall",
            json!({"points": partition, "location": if reversed {"interior-finish"} else {"exterior-finish"},
                "room_bounding": bounding}),
        );
        run(&mut s, "room_separator", json!({"points": separator}));
        for (at, name) in [([2000.0, 1600.0], "South"), ([2000.0, 4800.0], "North")] {
            run(&mut s, "room", json!({"point": point(at), "name": name}));
        }
        s
    }

    fn rooms(s: &mut Session) -> Vec<Value> {
        run(s, "elements", json!({"category": "room"}))["elements"].as_array().unwrap().clone()
    }

    fn assert_areas(s: &mut Session, at: &str, want: [f64; 2]) {
        run(s, "room_area", json!({"at": at}));
        let got = rooms(s);
        assert_eq!(got.len(), 2, "{at}: {got:?}");
        for (room, area) in got.iter().zip(want) {
            assert_eq!(room["enclosed"], true, "{at}: {got:?}");
            assert!(room.get("shares_space_with").is_none(), "{at}: {got:?}");
            assert!((room["area_m2"].as_f64().unwrap() - area).abs() < 1e-6, "{at}: {got:?}");
        }
    }

    #[test]
    fn continuing_a_wall_end_separates_rooms_in_both_directions_and_on_rotated_plans() {
        for reversed in [false, true] {
            for rotated in [false, true] {
                let mut s = fixture([4200.0, 3200.0], reversed, rotated, true);
                // Centreline boundary: 4.2 m at y=3.1 m, a short cap, then 5.4 m at y=3.2 m.
                assert_areas(&mut s, "centre", [30.30, 31.14]);
                // Finish boundary deducts the 0.2 m partition's body, rather than sharing the gross areas.
                assert_areas(&mut s, "finish", [28.32, 29.14]);
                assert_eq!(run(&mut s, "elements", json!({"category": "roomseparator"}))["elements"][0]["length"], 5300.0);
                assert_eq!(run(&mut s, "floor", json!({}))["area_m2"], 61.44, "separators do not split slabs");
            }
        }
    }

    #[test]
    fn a_parallel_separator_does_not_join_a_gap_a_wall_side_or_a_nonbounding_wall() {
        for (start, bounding) in [
            ([4250.0, 3200.0], true), // A real gap beyond the end cap.
            ([4100.0, 3200.0], true), // Along the wall's side, not its end.
            ([4200.0, 3250.0], true), // Beside but outside its thickness.
            ([4200.0, 3200.0], false),
        ] {
            let mut s = fixture(start, false, false, bounding);
            let got = rooms(&mut s);
            assert_eq!(got[0]["area_m2"], 61.44, "{start:?} {bounding}: {got:?}");
            assert_eq!(got[1]["enclosed"], false, "{start:?} {bounding}: {got:?}");
            assert_eq!(got[1]["shares_space_with"], got[0]["id"]);
        }
    }

    #[test]
    fn wall_end_rooms_survive_native_reopen_and_export_distinct_gross_and_net_ifc_quantities() {
        let mut s = fixture([4200.0, 3200.0], false, false, true);
        assert_areas(&mut s, "centre", [30.30, 31.14]);
        let before = run(&mut s, "elements", json!({"category": "roomseparator"}));
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("separator-end.bimcraft");
        let ifc = dir.path().join("separator-end.ifc");
        run(&mut s, "save", json!({"path": native}));
        let mut reopened = Session::new();
        let opened = run(&mut reopened, "open", json!({"path": native}));
        assert!(opened.get("left_out").is_none(), "{opened}");
        assert_eq!(run(&mut reopened, "elements", json!({"category": "roomseparator"})), before);
        assert_areas(&mut reopened, "centre", [30.30, 31.14]);
        assert_areas(&mut reopened, "finish", [28.32, 29.14]);
        run(&mut reopened, "save", json!({"path": ifc}));
        let file = step::parse(&std::fs::read(&ifc).unwrap()).unwrap();
        let spaces: Vec<_> = file.entities.values().filter(|e| e.name == "IFCSPACE").collect();
        assert_eq!(spaces.len(), 2);
        assert!(spaces.iter().all(|e| matches!(e.args.get(6), Some(V::Ref(_)))), "both spaces have real bodies");
        for (name, mut want) in [("GrossFloorArea", vec![30.30, 31.14]), ("NetFloorArea", vec![28.32, 29.14])] {
            let mut got: Vec<f64> = file
                .entities
                .values()
                .filter(|e| e.name == "IFCQUANTITYAREA" && e.args.first() == Some(&V::Str(name.into())))
                .map(|e| match e.args.get(3) {
                    Some(V::Real(n)) => *n,
                    Some(V::Int(n)) => *n as f64,
                    value => panic!("unexpected area {value:?}"),
                })
                .collect();
            got.sort_by(f64::total_cmp);
            want.sort_by(f64::total_cmp);
            assert_eq!(got, want, "{name}");
        }
        let mut imported = Session::new();
        run(&mut imported, "open", json!({"path": ifc}));
        assert_areas(&mut imported, "centre", [30.30, 31.14]);
        assert_areas(&mut imported, "finish", [28.32, 29.14]);
    }

    #[test]
    fn invalid_separator_commands_leave_the_existing_room_journey_intact() {
        let mut s = fixture([4200.0, 3200.0], false, false, true);
        assert_areas(&mut s, "centre", [30.30, 31.14]);
        let before = s.model.clone();
        for params in [
            json!({"points": [[4200,3200]]}),
            json!({"points": [[4200,3200],[4200,3200]]}),
            json!({"points": [[4200,3200],[9500,3200]], "level": "Missing"}),
            json!({"points": [[4200,3200],[1e30,3200]]}),
        ] {
            assert!(s.execute("room_separator", &params).is_err(), "{params}");
            assert_eq!(s.model, before);
        }
        assert_areas(&mut s, "centre", [30.30, 31.14]);
        assert_areas(&mut s, "finish", [28.32, 29.14]);
    }

    #[test]
    fn rendered_plan_fills_both_independent_rooms_on_opposite_sides_of_the_partition() {
        let mut s = fixture([4200.0, 3200.0], false, false, true);
        let got = rooms(&mut s);
        let colours = [[235, 130, 120], [120, 170, 235]];
        for (room, colour) in got.iter().zip(colours) {
            run(&mut s, "modify", json!({"id": room["id"], "colour": colour}));
        }
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("rooms.png");
        run(&mut s, "render", json!({"path": png, "width": 1000, "height": 800}));
        let image = tiny_skia::Pixmap::decode_png(&std::fs::read(png).unwrap()).unwrap();
        for (index, colour) in colours.into_iter().enumerate() {
            let mut halves = [0, 0];
            for y in 0..image.height() {
                for x in 0..image.width() {
                    let pixel = image.pixel(x, y).unwrap();
                    if [pixel.red(), pixel.green(), pixel.blue()] == colour {
                        halves[usize::from(y >= image.height() / 2)] += 1;
                    }
                }
            }
            assert!(halves[index ^ 1] > 20_000, "room {index} must have a visible fill: {halves:?}");
            assert!(halves[index ^ 1] > halves[index] * 5, "room {index} must stay on its own side: {halves:?}");
        }
    }
}
